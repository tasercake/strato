//! Phase 3 semantic fact normalization through the adapter boundary.

use std::collections::{BTreeMap, BTreeSet};

use strato_ty_adapter::{facade::FacadeError, targets as adapter};
use thiserror::Error;

use crate::{
    parser::{ParsedProject, convert_location, utf8_path},
    types::{AnalysisWarning, FileKind, SemanticCall, SemanticFacts, SemanticTarget},
};

/// Fatal semantic setup errors.
#[derive(Debug, Error)]
pub enum SemanticError {
    /// Reserved for future fatal semantic setup failures.
    #[error(transparent)]
    Adapter(#[from] FacadeError),
}

/// Query normalized semantic facts through the adapter facade.
pub fn analyze_semantics(parsed: &ParsedProject) -> Result<SemanticFacts, SemanticError> {
    let mut calls_by_path = BTreeMap::new();
    let mut warnings = Vec::new();

    for file in parsed.facade.files() {
        let path = utf8_path(file.path());
        match parsed.facade.semantic_facts_in_file(file.id()) {
            Ok(semantics) => {
                let path = utf8_path(&semantics.path);
                let calls = semantics
                    .calls
                    .into_iter()
                    .map(convert_call)
                    .collect::<Vec<_>>();
                let is_stub = parsed
                    .syntax_by_path
                    .get(&path)
                    .is_some_and(|syntax| syntax.kind == FileKind::Stub);
                if !calls.is_empty() && !is_stub {
                    calls_by_path.insert(path, calls);
                }
            }
            Err(error) => warnings.push(AnalysisWarning::Adapter {
                path: Some(path),
                error: error.to_string(),
            }),
        }
    }

    Ok(SemanticFacts {
        calls_by_path,
        warnings,
    })
}

fn convert_call(call: adapter::AdapterCallSemantic) -> SemanticCall {
    SemanticCall {
        enclosing_qualified_name: call.enclosing_qualified_name,
        expression: call.expression,
        target: convert_target(call.target),
        is_event_loop_run_in_executor: call.is_event_loop_run_in_executor,
        location: convert_location(call.location),
    }
}

fn convert_target(target: adapter::ResolvedTarget) -> SemanticTarget {
    match target {
        adapter::ResolvedTarget::FirstPartyDefinition(definition) => {
            if definition.as_str().ends_with(":<anonymous>") {
                SemanticTarget::Unknown
            } else {
                SemanticTarget::FirstPartyDefinition(definition.as_str().to_owned())
            }
        }
        adapter::ResolvedTarget::ExternalQualifiedNames(names) => {
            SemanticTarget::ExternalQualifiedNames(names.into_iter().collect::<BTreeSet<_>>())
        }
        adapter::ResolvedTarget::Unknown => SemanticTarget::Unknown,
    }
}
