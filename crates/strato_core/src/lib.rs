//! Core types and scaffolding for the Strato analyzer.

use thiserror::Error;

/// Errors returned by the analysis scaffolding.
#[derive(Debug, Error)]
pub enum AnalysisError {
    /// The requested analysis phase has not been implemented yet.
    #[error("analysis engine is not implemented yet")]
    NotImplemented,
}
