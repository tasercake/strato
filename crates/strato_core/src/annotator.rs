//! Annotation classification over Strato-owned syntax and semantic facts.

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    graph::BlockingStatus,
    types::{CallableParam, ExecutorWrapperConfig, FileSyntax, SemanticFacts, SemanticTarget},
};

/// Resolved annotation effects for graph construction.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AnnotationEffects {
    /// Blocking status overrides by first-party qualified function name.
    pub statuses: BTreeMap<String, BlockingStatus>,
    /// First-party executor wrappers declared with `@unblocker`.
    pub executor_wrappers: BTreeMap<String, ExecutorWrapperConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum AnnotationKind {
    Blocking,
    NonBlocking,
    Unblocker { callable_param: CallableParam },
}

/// Classify resolved Strato annotations from syntax plus facade-normalized call facts.
#[must_use]
pub fn classify_annotations(
    syntax_by_path: &BTreeMap<camino::Utf8PathBuf, FileSyntax>,
    semantic_facts: &SemanticFacts,
) -> AnnotationEffects {
    let decorators_by_function = syntax_by_path
        .values()
        .flat_map(|syntax| syntax.functions.iter())
        .map(|function| {
            (
                function.qualified_name.clone(),
                function
                    .decorators
                    .iter()
                    .map(|decorator| normalize_decorator_expression(decorator))
                    .collect::<BTreeSet<_>>(),
            )
        })
        .collect::<BTreeMap<_, _>>();

    let mut effects = AnnotationEffects::default();

    for calls in semantic_facts.calls_by_path.values() {
        for call in calls {
            let Some(enclosing) = call.enclosing_qualified_name.as_deref() else {
                continue;
            };
            let Some(decorators) = decorators_by_function.get(enclosing) else {
                continue;
            };
            let expression = normalize_decorator_expression(call.expression.as_str());
            if !decorators.contains(&expression) {
                continue;
            }
            let Some(annotation) = annotation_kind(&call.target, expression.as_str()) else {
                continue;
            };
            match annotation {
                AnnotationKind::Blocking => {
                    effects
                        .statuses
                        .entry(enclosing.to_string())
                        .or_insert(BlockingStatus::KnownBlocking);
                }
                AnnotationKind::NonBlocking => {
                    effects
                        .statuses
                        .insert(enclosing.to_string(), BlockingStatus::KnownNonBlocking);
                }
                AnnotationKind::Unblocker { callable_param } => {
                    effects.executor_wrappers.insert(
                        enclosing.to_string(),
                        ExecutorWrapperConfig { callable_param },
                    );
                }
            }
        }
    }

    effects
}

fn annotation_kind(target: &SemanticTarget, expression: &str) -> Option<AnnotationKind> {
    let annotation_name = match target {
        SemanticTarget::ExternalQualifiedNames(names) => names.iter().find_map(|name| {
            ["blocking", "non_blocking", "unblocker"]
                .into_iter()
                .find(|annotation| is_strato_annotation(name, annotation))
        })?,
        SemanticTarget::FirstPartyDefinition(_) | SemanticTarget::Unknown => return None,
    };

    match annotation_name {
        "blocking" => Some(AnnotationKind::Blocking),
        "non_blocking" => Some(AnnotationKind::NonBlocking),
        "unblocker" => Some(AnnotationKind::Unblocker {
            callable_param: unblocker_callable_param(expression),
        }),
        _ => None,
    }
}

fn is_strato_annotation(name: &str, annotation: &str) -> bool {
    name == format!("strato.{annotation}") || name == format!("strato._annotations.{annotation}")
}

fn normalize_decorator_expression(expression: &str) -> String {
    expression.trim().trim_start_matches('@').trim().to_string()
}

fn unblocker_callable_param(expression: &str) -> CallableParam {
    let Some((_, args)) = expression.split_once('(') else {
        return CallableParam::Position(0);
    };
    let Some(args) = args.strip_suffix(')') else {
        return CallableParam::Position(0);
    };
    for argument in split_arguments(args) {
        let Some((name, value)) = argument.split_once('=') else {
            continue;
        };
        if name.trim() != "callable_param" {
            continue;
        }
        let value = value.trim();
        if let Ok(index) = value.parse::<u64>() {
            return CallableParam::Position(index);
        }
        if let Some(keyword) = unquote(value) {
            return CallableParam::Keyword(keyword.to_string());
        }
    }
    CallableParam::Position(0)
}

fn unquote(value: &str) -> Option<&str> {
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            value
                .strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        })
}

fn split_arguments(args: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0_u32;
    let mut start = 0_usize;
    for (index, ch) in args.char_indices() {
        match ch {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(args[start..index].trim().to_string());
                start = index + 1;
            }
            _ => {}
        }
    }
    if start < args.len() {
        parts.push(args[start..].trim().to_string());
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unblocker_param_defaults_and_parses_keyword() {
        assert_eq!(
            unblocker_callable_param("unblocker"),
            CallableParam::Position(0)
        );
        assert_eq!(
            unblocker_callable_param("unblocker(callable_param=\"target\")"),
            CallableParam::Keyword("target".to_string())
        );
    }
}
