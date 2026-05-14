//! CLI argument parsing for the `strato` binary.

use std::path::PathBuf;

use clap::{ArgAction, Parser, Subcommand, ValueEnum};

/// Parsed command-line arguments.
#[derive(Debug, Parser)]
#[command(version, about = "Detect blocking calls in Python async contexts")]
pub(crate) struct Cli {
    /// Command to execute.
    #[command(subcommand)]
    pub(crate) command: Command,
}

/// Supported top-level commands.
#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Analyze Python files under a path.
    Check {
        /// Files or directories to analyze.
        #[arg(value_name = "PATHS", default_value = ".")]
        paths: Vec<PathBuf>,
        /// Path to pyproject.toml.
        #[arg(long, value_name = "PATH")]
        config: Option<PathBuf>,
        /// Output format.
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
        /// Override intervention point strategy.
        #[arg(long, value_enum)]
        intervention_strategy: Option<InterventionStrategy>,
        /// Override diagnostic severity.
        #[arg(long, value_enum)]
        severity: Option<Severity>,
        /// Disable caching for this run.
        #[arg(long)]
        no_cache: bool,
        /// Clear the cache before analysis.
        #[arg(long)]
        clear_cache: bool,
        /// Override first-party module detection.
        #[arg(long, value_name = "MODULES", value_delimiter = ',')]
        first_party: Vec<String>,
        /// Override Python version.
        #[arg(long, value_name = "VER")]
        python_version: Option<String>,
        /// Suppress non-diagnostic output.
        #[arg(short, long)]
        quiet: bool,
        /// Show detailed analysis progress.
        #[arg(short, long, action = ArgAction::Count)]
        verbose: u8,
    },
}

/// Supported diagnostic output formats.
#[derive(Copy, Clone, Debug, ValueEnum)]
pub(crate) enum OutputFormat {
    /// Human-readable text output.
    Text,
    /// Machine-readable JSON output.
    Json,
    /// SARIF output for code scanning integrations.
    Sarif,
}

/// Supported intervention point strategies.
#[derive(Copy, Clone, Debug, ValueEnum)]
pub(crate) enum InterventionStrategy {
    /// Report deepest first-party call site.
    FirstPartyDeepest,
    /// Report the async-to-sync boundary.
    AsyncBoundary,
}

impl From<InterventionStrategy> for strato_core::types::InterventionStrategy {
    fn from(value: InterventionStrategy) -> Self {
        match value {
            InterventionStrategy::FirstPartyDeepest => Self::FirstPartyDeepest,
            InterventionStrategy::AsyncBoundary => Self::AsyncBoundary,
        }
    }
}

/// Supported diagnostic severities.
#[derive(Copy, Clone, Debug, ValueEnum)]
pub(crate) enum Severity {
    /// Emit diagnostics as errors.
    Error,
    /// Emit diagnostics as warnings.
    Warning,
}

impl From<Severity> for strato_core::types::DiagnosticSeverity {
    fn from(value: Severity) -> Self {
        match value {
            Severity::Error => Self::Error,
            Severity::Warning => Self::Warning,
        }
    }
}

impl From<OutputFormat> for strato_core::types::OutputFormat {
    fn from(value: OutputFormat) -> Self {
        match value {
            OutputFormat::Text => Self::Text,
            OutputFormat::Json => Self::Json,
            OutputFormat::Sarif => Self::Sarif,
        }
    }
}
