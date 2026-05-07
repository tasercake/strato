//! Core types and scaffolding for the Strato analyzer.

pub mod test_fixtures;

use serde_json::Value;
use thiserror::Error;

pub use test_fixtures::{AcceptanceFixture, ExpectedOutput, FixtureRun};

/// Result of running one manifest-declared fixture invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixtureRunOutput {
    /// Process-style exit code for this run.
    pub exit_code: i32,
    /// JSON output emitted by the run.
    pub json: Value,
}

/// Errors returned by the analysis scaffolding.
#[derive(Debug, Error)]
pub enum AnalysisError {
    /// The requested analysis phase has not been implemented yet.
    #[error("analysis engine is not implemented yet")]
    NotImplemented,
}

/// Runs analysis for one acceptance fixture.
pub fn analyze_fixture(_fixture: &AcceptanceFixture) -> Result<Value, AnalysisError> {
    Err(AnalysisError::NotImplemented)
}

/// Runs one manifest-declared analysis over an acceptance fixture.
pub fn analyze_fixture_run(
    _fixture: &AcceptanceFixture,
    _run: &FixtureRun,
) -> Result<FixtureRunOutput, AnalysisError> {
    Err(AnalysisError::NotImplemented)
}
