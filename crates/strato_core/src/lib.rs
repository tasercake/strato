//! Core types and scaffolding for the Strato analyzer.

use std::path::PathBuf;

use serde_json::Value;
use thiserror::Error;

/// Errors returned by the analysis scaffolding.
#[derive(Debug, Error)]
pub enum AnalysisError {
    /// The requested analysis phase has not been implemented yet.
    #[error("analysis engine is not implemented yet")]
    NotImplemented,
}

/// Configuration source for an analysis run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigSource {
    /// Use built-in defaults only.
    Defaults,
    /// Load configuration from this path.
    Path(PathBuf),
}

/// Options for one analysis run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalysisOptions {
    /// Configuration source.
    pub config: ConfigSource,
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
    _path: impl AsRef<std::path::Path>,
    _options: &AnalysisOptions,
) -> Result<AnalysisOutput, AnalysisError> {
    Err(AnalysisError::NotImplemented)
}
