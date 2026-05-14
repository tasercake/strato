//! Core types and scaffolding for the Strato analyzer.

use std::{collections::BTreeMap, fs, path::PathBuf};

use camino::Utf8PathBuf;
use reporter::{ReportInput, build_report};
use serde_json::Value;
use thiserror::Error;
use types::{AnalysisWarning, FileSyntax};

pub mod annotator;
pub mod database;
pub mod discovery;
pub mod graph;
pub mod graph_builder;
pub mod parser;
pub mod propagator;
pub mod reporter;
pub mod semantics;
pub mod types;

/// Strato version used for cache compatibility.
pub const STRATO_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Errors returned by the analysis scaffolding.
#[derive(Debug, Error)]
pub enum AnalysisError {
    /// The requested analysis phase has not been implemented yet.
    #[error("analysis engine is not implemented yet")]
    NotImplemented,
    /// Discovery failed before full analysis could start.
    #[error(transparent)]
    Discovery(#[from] discovery::DiscoverError),
    /// Parsing setup failed before recoverable file warnings could be emitted.
    #[error(transparent)]
    Parse(#[from] parser::ParseError),
    /// Semantic analysis setup failed before recoverable file warnings could be emitted.
    #[error(transparent)]
    Semantic(#[from] semantics::SemanticError),
    /// Source text could not be read for report coordinate conversion.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// Cache storage failed.
    #[error(transparent)]
    Cache(#[from] strato_cache::StorageError),
}

/// Configuration source for an analysis run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigSource {
    /// Auto-discover configuration from pyproject.toml, falling back to built-in defaults.
    Defaults,
    /// Use built-in defaults only.
    BuiltInDefaults,
    /// Load configuration from this path.
    Path(PathBuf),
}

/// Options for one analysis run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalysisOptions {
    /// Configuration source.
    pub config: ConfigSource,
    /// Override diagnostic intervention strategy.
    pub intervention_strategy: Option<types::InterventionStrategy>,
    /// Override diagnostic severity.
    pub severity: Option<types::DiagnosticSeverity>,
    /// Override output format in effective config.
    pub output_format: Option<types::OutputFormat>,
    /// Override cache use for this run.
    pub cache_enabled: Option<bool>,
    /// Clear the cache before analysis.
    pub clear_cache: bool,
    /// Override first-party module detection.
    pub first_party_modules: Option<Vec<String>>,
    /// Override Python version.
    pub python_version: Option<String>,
}

impl AnalysisOptions {
    /// Return options using auto-discovered configuration and no CLI overrides.
    #[must_use]
    pub const fn defaults() -> Self {
        Self {
            config: ConfigSource::Defaults,
            intervention_strategy: None,
            severity: None,
            output_format: None,
            cache_enabled: None,
            clear_cache: false,
            first_party_modules: None,
            python_version: None,
        }
    }
}

/// Result of one analysis run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalysisOutput {
    /// Process-style exit code.
    pub exit_code: i32,
    /// JSON payload in Strato's v1 schema.
    pub json: Value,
}

/// Analyze a file or directory with explicit options.
pub fn analyze_path_with_options(
    path: impl AsRef<std::path::Path>,
    options: &AnalysisOptions,
) -> Result<AnalysisOutput, AnalysisError> {
    let mut manifest = discovery::discover_project(path, options)?;
    apply_overrides(&mut manifest.config, options)?;
    if options.clear_cache {
        strato_cache::CacheStorage::new(manifest.config.cache_dir.clone()).clear()?;
    }

    let parsed = if manifest.config.cache_enabled {
        parser::parse_project_with_cache(&manifest)?
    } else {
        parser::parse_project(&manifest)?
    };
    let semantics = semantics::analyze_semantics(&parsed)?;
    let mut warnings = parsed.warnings;
    warnings.extend(semantics.warnings.clone());
    warnings.extend(derived_warnings(
        &parsed.syntax_by_path,
        &semantics,
        &manifest.config,
    ));
    let mut graph = graph_builder::build_call_graph(
        &parsed.syntax_by_path,
        &semantics,
        &manifest.blocking_database,
        &manifest.escape_hatch_config.executor_wrappers,
    );
    let propagation = propagator::propagate_blocking(&mut graph);
    let source_text_by_path = source_text_by_path(&parsed.syntax_by_path)?;
    let report = build_report(ReportInput {
        project_root: &manifest.config.root,
        graph: &graph,
        propagation: &propagation,
        syntax_by_path: &parsed.syntax_by_path,
        source_text_by_path: &source_text_by_path,
        blocking_database: &manifest.blocking_database,
        warnings: &warnings,
        severity: manifest.config.severity,
        intervention_strategy: manifest.config.intervention_strategy,
    });
    let exit_code = i32::from(!report.diagnostics.is_empty());

    Ok(AnalysisOutput {
        exit_code,
        json: report.to_json_value(),
    })
}

fn derived_warnings(
    syntax_by_path: &BTreeMap<Utf8PathBuf, FileSyntax>,
    semantics: &types::SemanticFacts,
    config: &types::StratoConfig,
) -> Vec<AnalysisWarning> {
    let mut warnings = Vec::new();
    for (path, syntax) in syntax_by_path {
        warnings.extend(unresolved_import_warnings(path, syntax, semantics));
        if !python_supports_asyncio_to_thread(&config.python_version)
            && semantics
                .calls_by_path
                .get(path)
                .is_some_and(|calls| calls.iter().any(is_asyncio_to_thread_call))
        {
            warnings.push(AnalysisWarning::General {
                path: Some(path.clone()),
                message: format!(
                    "asyncio.to_thread is unavailable for configured python_version {}; executor protection was not applied",
                    config.python_version
                ),
            });
        }
    }
    warnings
}

fn unresolved_import_warnings(
    path: &Utf8PathBuf,
    syntax: &FileSyntax,
    semantics: &types::SemanticFacts,
) -> Vec<AnalysisWarning> {
    let Some(calls) = semantics.calls_by_path.get(path) else {
        return Vec::new();
    };
    syntax
        .imports
        .iter()
        .filter(|import| import.level == 0)
        .filter_map(|import| {
            let module = import.module.as_ref()?;
            let imported_name = import.name.as_ref()?;
            let local_name = import.alias.as_ref().unwrap_or(imported_name);
            calls
                .iter()
                .any(|call| {
                    call.target == types::SemanticTarget::Unknown
                        && call_callee(call.expression.as_str()) == Some(local_name.as_str())
                })
                .then(|| AnalysisWarning::General {
                    path: Some(path.clone()),
                    message: format!("Unresolvable import: {module}"),
                })
        })
        .collect()
}

fn call_callee(expression: &str) -> Option<&str> {
    expression.split_once('(').map(|(callee, _)| callee.trim())
}

fn python_supports_asyncio_to_thread(version: &str) -> bool {
    !matches!(version, "3.7" | "3.8")
}

fn is_asyncio_to_thread_call(call: &types::SemanticCall) -> bool {
    match &call.target {
        types::SemanticTarget::ExternalQualifiedNames(names) => {
            names.iter().any(|name| name == "asyncio.to_thread")
        }
        types::SemanticTarget::FirstPartyDefinition(_) | types::SemanticTarget::Unknown => false,
    }
}

fn apply_overrides(
    config: &mut types::StratoConfig,
    options: &AnalysisOptions,
) -> Result<(), AnalysisError> {
    if let Some(strategy) = options.intervention_strategy {
        config.intervention_strategy = strategy;
    }
    if let Some(severity) = options.severity {
        config.severity = severity;
    }
    if let Some(output_format) = options.output_format {
        config.output_format = output_format;
    }
    if let Some(cache_enabled) = options.cache_enabled {
        config.cache_enabled = cache_enabled;
    }
    if let Some(python_version) = &options.python_version {
        validate_python_version(python_version)?;
        config.python_version.clone_from(python_version);
    }
    let _ = &options.first_party_modules;
    Ok(())
}

fn validate_python_version(version: &str) -> Result<(), AnalysisError> {
    if matches!(
        version,
        "3.7" | "3.8" | "3.9" | "3.10" | "3.11" | "3.12" | "3.13" | "3.14" | "3.15"
    ) {
        return Ok(());
    }
    Err(discovery::DiscoverError::Config {
        message: "Invalid python_version: must be '3.7'...'3.15'".to_string(),
    }
    .into())
}

fn source_text_by_path(
    syntax_by_path: &BTreeMap<Utf8PathBuf, FileSyntax>,
) -> Result<BTreeMap<Utf8PathBuf, String>, std::io::Error> {
    syntax_by_path
        .keys()
        .map(|path| fs::read_to_string(path).map(|text| (path.clone(), text)))
        .collect()
}
