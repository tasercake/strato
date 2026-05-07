//! Core types and scaffolding for the Strato analyzer.

pub mod fixtures;

use thiserror::Error;

pub use fixtures::{AcceptanceFixture, ExpectedOutput};

/// Errors returned by the analysis scaffolding.
#[derive(Debug, Error)]
pub enum AnalysisError {
    /// The requested analysis phase has not been implemented yet.
    #[error("analysis engine is not implemented yet")]
    NotImplemented,
}

/// Runs analysis for one acceptance fixture.
pub fn analyze_fixture(_fixture: &AcceptanceFixture) -> Result<ExpectedOutput, AnalysisError> {
    Err(AnalysisError::NotImplemented)
}
