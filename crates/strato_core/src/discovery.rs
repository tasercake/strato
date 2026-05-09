//! Phase 1 discovery and configuration loading.

use std::{collections::BTreeMap, fs, io, path::Path};

use camino::{Utf8Path, Utf8PathBuf};
use thiserror::Error;
use toml::Value;

use crate::{
    AnalysisOptions, ConfigSource,
    database::effective_database,
    types::{
        BlockingCategory, BlockingConfig, BlockingEntry, CallableParam, DiagnosticSeverity,
        EscapeHatchConfig, ExecutorWrapperConfig, FileEntry, FileKind, FileManifest, OutputFormat,
        StratoConfig,
    },
};

/// Discovery-phase fatal errors.
#[derive(Debug, Error)]
pub enum DiscoverError {
    /// Invalid configuration.
    #[error("{message}")]
    Config {
        /// Human-readable configuration error.
        message: String,
    },
    /// I/O failure.
    #[error(transparent)]
    Io(#[from] io::Error),
    /// No analyzable source files remain.
    #[error("No analyzable source files")]
    NoAnalyzableSourceFiles,
}

/// Discover and classify Python files for one analysis root.
pub fn discover_project(
    path: impl AsRef<Path>,
    options: &AnalysisOptions,
) -> Result<FileManifest, DiscoverError> {
    let analysis_root = normalize_path(path.as_ref())?;
    let config = load_config(&analysis_root, &options.config)?;
    let source_roots = source_roots(&analysis_root, &config)?;
    let mut files = BTreeMap::new();

    for source_root in &source_roots {
        collect_python_files(source_root, &config, true, &mut files)?;
    }
    for stub_path in &config.stub_paths {
        collect_python_files(stub_path, &config, false, &mut files)?;
    }

    let files = files.into_values().collect::<Vec<_>>();
    if !files
        .iter()
        .any(|file| file.kind == FileKind::Source && file.is_first_party)
    {
        return Err(DiscoverError::NoAnalyzableSourceFiles);
    }

    let blocking_database = effective_database(&config.blocking);
    let escape_hatch_config = EscapeHatchConfig {
        executor_wrappers: config.executor_wrappers.clone(),
    };

    Ok(FileManifest {
        files,
        source_roots,
        config,
        blocking_database,
        escape_hatch_config,
    })
}

/// Load effective configuration for `analysis_root`.
pub fn load_config(
    analysis_root: &Utf8Path,
    source: &ConfigSource,
) -> Result<StratoConfig, DiscoverError> {
    match source {
        ConfigSource::Defaults => Ok(StratoConfig::defaults(analysis_root.to_path_buf())),
        ConfigSource::Path(path) => load_config_path(&normalize_path(path)?),
    }
}

fn load_config_path(path: &Utf8Path) -> Result<StratoConfig, DiscoverError> {
    let root = path
        .parent()
        .unwrap_or_else(|| Utf8Path::new("."))
        .to_path_buf();
    let text = fs::read_to_string(path)?;
    let value = Value::Table(
        toml::from_str(&text).map_err(|error| DiscoverError::Config {
            message: error.to_string(),
        })?,
    );
    let mut config = StratoConfig::defaults(root.clone());
    let Some(strato) = value
        .get("tool")
        .and_then(|tool| tool.get("strato"))
        .and_then(Value::as_table)
    else {
        return Ok(config);
    };

    if let Some(src_roots) = strato.get("src_roots") {
        config.src_roots = Some(parse_path_list(src_roots, &root, "src_roots")?);
    }
    if let Some(exclude) = strato.get("exclude") {
        config.exclude = parse_string_list(exclude, "exclude")?;
    }
    if let Some(stub_paths) = strato.get("stub_paths") {
        config.stub_paths = parse_path_list(stub_paths, &root, "stub_paths")?;
    }
    if let Some(python_version) = strato.get("python_version") {
        let value = required_string(python_version, "python_version")?;
        if !matches!(
            value,
            "3.7" | "3.8" | "3.9" | "3.10" | "3.11" | "3.12" | "3.13" | "3.14" | "3.15"
        ) {
            return config_error("Invalid python_version: must be '3.7'...'3.15'");
        }
        config.python_version = value.to_string();
    }
    if let Some(strategy) = strato.get("intervention_strategy") {
        config.intervention_strategy = match required_string(strategy, "intervention_strategy")? {
            "first-party-deepest" => crate::types::InterventionStrategy::FirstPartyDeepest,
            "async-boundary" => crate::types::InterventionStrategy::AsyncBoundary,
            _ => {
                return config_error(
                    "Invalid strategy: must be 'first-party-deepest' or 'async-boundary'",
                );
            }
        };
    }
    if let Some(severity) = strato.get("severity") {
        config.severity = match required_string(severity, "severity")? {
            "error" => DiagnosticSeverity::Error,
            "warning" => DiagnosticSeverity::Warning,
            _ => return config_error("Invalid severity: must be 'error' or 'warning'"),
        };
    }
    if let Some(format) = strato.get("output_format") {
        config.output_format = match required_string(format, "output_format")? {
            "text" => OutputFormat::Text,
            "json" => OutputFormat::Json,
            "sarif" => OutputFormat::Sarif,
            _ => return config_error("Invalid output_format: must be 'text', 'json', or 'sarif'"),
        };
    }
    if let Some(cache_dir) = strato.get("cache_dir") {
        config.cache_dir = root.join(required_string(cache_dir, "cache_dir")?);
    }
    if let Some(cache_enabled) = strato.get("cache_enabled") {
        config.cache_enabled = cache_enabled
            .as_bool()
            .ok_or_else(|| DiscoverError::Config {
                message: "cache_enabled must be a boolean".to_string(),
            })?;
    }
    if let Some(blocking) = strato.get("blocking").and_then(Value::as_table) {
        config.blocking = parse_blocking(blocking)?;
    }
    if let Some(wrappers) = strato.get("executor-wrappers").and_then(Value::as_table) {
        config.executor_wrappers = parse_executor_wrappers(wrappers)?;
    }

    Ok(config)
}

fn source_roots(
    analysis_root: &Utf8Path,
    config: &StratoConfig,
) -> Result<Vec<Utf8PathBuf>, DiscoverError> {
    let roots = if let Some(configured) = &config.src_roots {
        configured.clone()
    } else {
        auto_source_roots(analysis_root)?
    };
    for root in &roots {
        if !root.exists() {
            return config_error(&format!("Source root '{root}' does not exist"));
        }
    }
    Ok(roots)
}

fn auto_source_roots(analysis_root: &Utf8Path) -> Result<Vec<Utf8PathBuf>, DiscoverError> {
    let pyproject = analysis_root.join("pyproject.toml");
    if pyproject.exists() {
        let text = fs::read_to_string(&pyproject)?;
        if let Ok(table) = toml::from_str::<toml::Table>(&text) {
            let value = Value::Table(table);
            if let Some(where_value) = value
                .get("tool")
                .and_then(|tool| tool.get("setuptools"))
                .and_then(|setuptools| setuptools.get("packages"))
                .and_then(|packages| packages.get("find"))
                .and_then(|find| find.get("where"))
            {
                let roots = parse_path_list(where_value, analysis_root, "where")?;
                if !roots.is_empty() {
                    return Ok(roots);
                }
            }
        }
    }
    let src = analysis_root.join("src");
    if src.is_dir() {
        return Ok(vec![src]);
    }
    Ok(vec![analysis_root.to_path_buf()])
}

fn collect_python_files(
    root: &Utf8Path,
    config: &StratoConfig,
    first_party: bool,
    files: &mut BTreeMap<Utf8PathBuf, FileEntry>,
) -> Result<(), DiscoverError> {
    if !root.exists() {
        return config_error(&format!("Source root '{root}' does not exist"));
    }
    let mut stack = vec![root.to_path_buf()];
    while let Some(path) = stack.pop() {
        let metadata = fs::metadata(&path)?;
        if metadata.is_dir() {
            let mut children = fs::read_dir(&path)?.collect::<Result<Vec<_>, _>>()?;
            children.sort_by_key(std::fs::DirEntry::path);
            for child in children.into_iter().rev() {
                stack.push(normalize_path(&child.path())?);
            }
            continue;
        }
        if is_excluded(&path, config) {
            continue;
        }
        let kind = match path.extension() {
            Some("py") if first_party => FileKind::Source,
            Some("pyi") => FileKind::Stub,
            _ => continue,
        };
        let bytes = fs::read(&path)?;
        files.insert(
            path.clone(),
            FileEntry {
                path: path.clone(),
                content_hash: strato_cache::sha256_hex(&bytes),
                kind,
                is_first_party: first_party,
            },
        );
    }
    Ok(())
}

fn is_excluded(path: &Utf8Path, config: &StratoConfig) -> bool {
    let relative = path
        .strip_prefix(&config.root)
        .unwrap_or(path)
        .as_str()
        .replace('\\', "/");
    config
        .exclude
        .iter()
        .any(|pattern| glob_matches(pattern, &relative))
}

fn glob_matches(pattern: &str, relative: &str) -> bool {
    if let Some(prefix) = pattern.strip_suffix("/**") {
        relative == prefix || relative.starts_with(&format!("{prefix}/"))
    } else if let Some(suffix) = pattern.strip_prefix("**/") {
        relative.ends_with(suffix)
    } else {
        relative == pattern
    }
}

fn parse_path_list(
    value: &Value,
    root: &Utf8Path,
    field: &str,
) -> Result<Vec<Utf8PathBuf>, DiscoverError> {
    Ok(parse_string_list(value, field)?
        .into_iter()
        .map(|path| root.join(path))
        .collect())
}

fn parse_string_list(value: &Value, field: &str) -> Result<Vec<String>, DiscoverError> {
    value
        .as_array()
        .ok_or_else(|| DiscoverError::Config {
            message: format!("{field} must be a list of strings"),
        })?
        .iter()
        .map(|item| {
            item.as_str()
                .map(ToString::to_string)
                .ok_or_else(|| DiscoverError::Config {
                    message: format!("{field} must be a list of strings"),
                })
        })
        .collect()
}

fn parse_blocking(table: &toml::Table) -> Result<BlockingConfig, DiscoverError> {
    let mut config = BlockingConfig::default();
    if let Some(add) = table.get("add") {
        for entry in add.as_array().ok_or_else(|| DiscoverError::Config {
            message: "blocking.add must be a list".to_string(),
        })? {
            let entry = entry.as_table().ok_or_else(|| DiscoverError::Config {
                message: "blocking.add entries must be tables".to_string(),
            })?;
            let name = required_table_string(entry, "name")?.to_string();
            let help = required_table_string(entry, "help")?.to_string();
            let category = required_table_string(entry, "category")?;
            let category = BlockingCategory::parse(category).ok_or_else(|| DiscoverError::Config {
                message: format!("Unknown category '{category}'. Valid: sleep, network-io, file-io, subprocess, database-io, user-input, other"),
            })?;
            config.add.push(BlockingEntry {
                name,
                help,
                category,
            });
        }
    }
    if let Some(remove) = table.get("remove") {
        config.remove = parse_string_list(remove, "blocking.remove")?
            .into_iter()
            .collect();
    }
    if let Some(modules) = table.get("blocking_modules") {
        config.blocking_modules = parse_string_list(modules, "blocking.blocking_modules")?
            .into_iter()
            .collect();
    }
    Ok(config)
}

fn parse_executor_wrappers(
    table: &toml::Table,
) -> Result<BTreeMap<String, ExecutorWrapperConfig>, DiscoverError> {
    let mut wrappers = BTreeMap::new();
    for (name, value) in table {
        let wrapper = value.as_table().ok_or_else(|| DiscoverError::Config {
            message: format!("Executor wrapper '{name}' must be a table"),
        })?;
        let callable_param =
            wrapper
                .get("callable_param")
                .ok_or_else(|| DiscoverError::Config {
                    message: format!(
                        "Executor wrapper '{name}' missing required field 'callable_param'"
                    ),
                })?;
        let callable_param = if let Some(index) = callable_param.as_integer() {
            let index = u64::try_from(index).map_err(|_| DiscoverError::Config {
                message: format!("Executor wrapper '{name}' callable_param must be an integer index or keyword name"),
            })?;
            CallableParam::Position(index)
        } else if let Some(keyword) = callable_param.as_str() {
            CallableParam::Keyword(keyword.to_string())
        } else {
            return config_error(&format!(
                "Executor wrapper '{name}' callable_param must be an integer index or keyword name"
            ));
        };
        wrappers.insert(name.clone(), ExecutorWrapperConfig { callable_param });
    }
    Ok(wrappers)
}

fn required_string<'a>(value: &'a Value, field: &str) -> Result<&'a str, DiscoverError> {
    value.as_str().ok_or_else(|| DiscoverError::Config {
        message: format!("{field} must be a string"),
    })
}

fn required_table_string<'a>(
    table: &'a toml::Table,
    field: &str,
) -> Result<&'a str, DiscoverError> {
    table
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| DiscoverError::Config {
            message: if field == "name" {
                "Blocking entry missing required field 'name'".to_string()
            } else {
                format!("Blocking entry missing required field '{field}'")
            },
        })
}

fn config_error<T>(message: &str) -> Result<T, DiscoverError> {
    Err(DiscoverError::Config {
        message: message.to_string(),
    })
}

fn normalize_path(path: &Path) -> Result<Utf8PathBuf, DiscoverError> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    Utf8PathBuf::from_path_buf(if absolute.exists() {
        absolute.canonicalize()?
    } else {
        absolute
    })
    .map_err(|path| DiscoverError::Config {
        message: format!("Path is not valid UTF-8: {}", path.display()),
    })
}
