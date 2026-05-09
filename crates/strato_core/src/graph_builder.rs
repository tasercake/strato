//! Phase 4 callable graph construction over Strato-owned facts.

use std::collections::{BTreeMap, BTreeSet};

use camino::Utf8PathBuf;

use crate::{
    annotator::classify_annotations,
    graph::{BlockingStatus, CallEdge, CallGraph, CallableKind, EdgeKind},
    types::{
        BlockingDatabase, CallableParam, ExecutorWrapperConfig, FileKind, FileSyntax,
        FunctionSyntax, SemanticCall, SemanticFacts, SemanticTarget, SourceLocation,
    },
};

/// Build a deterministic callable-level call graph from parser and semantic facts.
#[must_use]
pub fn build_call_graph(
    syntax_by_path: &BTreeMap<Utf8PathBuf, FileSyntax>,
    semantic_facts: &SemanticFacts,
    blocking_database: &BlockingDatabase,
    executor_wrappers: &BTreeMap<String, ExecutorWrapperConfig>,
) -> CallGraph {
    let mut builder = GraphBuilder::new(blocking_database, executor_wrappers);
    builder.register_callable_nodes(syntax_by_path);
    builder.stub_blocking_targets = stub_blocking_targets(syntax_by_path);
    builder.apply_annotations(syntax_by_path, semantic_facts);
    builder.add_semantic_edges(semantic_facts);
    builder.graph
}

struct GraphBuilder<'a> {
    graph: CallGraph,
    definition_index: BTreeMap<String, String>,
    decorators_by_function: BTreeMap<String, BTreeSet<String>>,
    blocking_database: &'a BlockingDatabase,
    executor_wrappers: BTreeMap<String, ExecutorWrapperConfig>,
    protected_ranges_by_function: BTreeMap<String, Vec<SourceRange>>,
    stub_blocking_targets: BTreeSet<String>,
}

#[derive(Debug, Clone, Copy)]
struct SourceRange {
    start: u32,
    end: u32,
}

impl<'a> GraphBuilder<'a> {
    fn new(
        blocking_database: &'a BlockingDatabase,
        executor_wrappers: &'a BTreeMap<String, ExecutorWrapperConfig>,
    ) -> Self {
        Self {
            graph: CallGraph::default(),
            definition_index: BTreeMap::new(),
            decorators_by_function: BTreeMap::new(),
            blocking_database,
            executor_wrappers: executor_wrappers.clone(),
            protected_ranges_by_function: BTreeMap::new(),
            stub_blocking_targets: BTreeSet::new(),
        }
    }

    fn register_callable_nodes(&mut self, syntax_by_path: &BTreeMap<Utf8PathBuf, FileSyntax>) {
        let mut functions = syntax_by_path
            .iter()
            .flat_map(|(path, syntax)| {
                syntax
                    .functions
                    .iter()
                    .map(move |function| (path.clone(), function))
            })
            .collect::<Vec<_>>();
        functions.sort_by(|left, right| {
            left.1
                .qualified_name
                .cmp(&right.1.qualified_name)
                .then_with(|| left.0.cmp(&right.0))
                .then_with(|| left.1.location.cmp(&right.1.location))
        });

        for (path, function) in functions {
            let kind = callable_kind(function);
            let blocking_status = first_party_blocking_status(
                &path,
                &function.qualified_name,
                self.blocking_database,
            );
            self.graph.add_node(
                function.qualified_name.clone(),
                kind,
                function.is_async,
                Some(function.location),
                blocking_status,
            );
            self.definition_index.insert(
                format!("{path}:{}", function.qualified_name),
                function.qualified_name.clone(),
            );
            self.definition_index
                .entry(format!("{path}:{}", function.name))
                .or_insert_with(|| function.qualified_name.clone());
            self.decorators_by_function.insert(
                function.qualified_name.clone(),
                function
                    .decorators
                    .iter()
                    .map(|decorator| decorator.trim().trim_start_matches('@').to_string())
                    .collect(),
            );
        }
    }

    fn add_semantic_edges(&mut self, semantic_facts: &SemanticFacts) {
        for calls in semantic_facts.calls_by_path.values() {
            let mut ordered = calls.iter().collect::<Vec<_>>();
            ordered.sort_by(|left, right| {
                left.location
                    .cmp(&right.location)
                    .then_with(|| left.expression.cmp(&right.expression))
            });
            for call in ordered {
                self.add_semantic_edge(call);
            }
        }
    }

    fn apply_annotations(
        &mut self,
        syntax_by_path: &BTreeMap<Utf8PathBuf, FileSyntax>,
        semantic_facts: &SemanticFacts,
    ) {
        let effects = classify_annotations(syntax_by_path, semantic_facts);
        for (qualified_name, status) in effects.statuses {
            if let Some(id) = self.graph.node_id(qualified_name.as_str()) {
                self.graph.set_blocking_status(id, status);
            }
        }
        self.executor_wrappers.extend(effects.executor_wrappers);
    }

    fn add_semantic_edge(&mut self, call: &SemanticCall) {
        let Some(enclosing) = call.enclosing_qualified_name.as_deref() else {
            return;
        };
        let Some(from) = self.graph.node_id(enclosing) else {
            return;
        };
        let protected = self.is_in_protected_range(enclosing, call.location);

        if let Some(wrapper_name) = self.executor_wrapper_name(call) {
            self.add_executor_argument_edge(from, wrapper_name.as_str(), call);
            self.add_protected_callable_argument_range(enclosing, wrapper_name.as_str(), call);
            return;
        }

        let Some(to) = self.node_for_target(&call.target) else {
            return;
        };
        let is_decorator = self.is_parsed_decorator(enclosing, call.expression.as_str());
        let Some(kind) = edge_kind(call, self.graph.nodes()[to.0].kind, is_decorator) else {
            return;
        };

        self.graph.add_edge(CallEdge {
            from,
            to,
            kind,
            location: call.location,
            in_executor: call.is_event_loop_run_in_executor,
            via: None,
            protected,
        });
    }

    fn add_executor_argument_edge(
        &mut self,
        from: crate::graph::NodeId,
        wrapper_name: &str,
        call: &SemanticCall,
    ) {
        let Some(callable_argument) = callable_argument(
            call.expression.as_str(),
            wrapper_name,
            &self.executor_wrappers,
        ) else {
            return;
        };
        let target = SemanticTarget::ExternalQualifiedNames(BTreeSet::from([callable_argument]));
        let Some(to) = self.node_for_target(&target) else {
            return;
        };
        let via = self.ensure_external_node(wrapper_name, BlockingStatus::Unknown);
        self.graph.add_edge(CallEdge {
            from,
            to,
            kind: EdgeKind::DirectCall,
            location: call.location,
            in_executor: true,
            via: Some(via),
            protected: true,
        });
    }

    fn executor_wrapper_name(&self, call: &SemanticCall) -> Option<String> {
        if let SemanticTarget::ExternalQualifiedNames(names) = &call.target
            && names.iter().any(|name| name == "asyncio.to_thread")
        {
            return Some("asyncio.to_thread".to_string());
        }
        if call.is_event_loop_run_in_executor {
            let callee = call.expression.split_once('(')?.0.trim();
            if callee == "asyncio.to_thread" || callee.ends_with(".run_in_executor") {
                return Some(callee.to_string());
            }
            return Some("asyncio.to_thread".to_string());
        }
        match &call.target {
            SemanticTarget::ExternalQualifiedNames(names) => names
                .iter()
                .find(|name| self.executor_wrappers.contains_key(name.as_str()))
                .cloned(),
            SemanticTarget::FirstPartyDefinition(definition) => self
                .definition_index
                .get(definition)
                .filter(|qualified_name| {
                    self.executor_wrappers.contains_key(qualified_name.as_str())
                })
                .cloned(),
            SemanticTarget::Unknown => None,
        }
    }

    fn add_protected_callable_argument_range(
        &mut self,
        enclosing: &str,
        wrapper_name: &str,
        call: &SemanticCall,
    ) {
        let Some(range) = callable_argument_range(
            call.expression.as_str(),
            call.location.start,
            wrapper_name,
            &self.executor_wrappers,
        ) else {
            return;
        };
        self.protected_ranges_by_function
            .entry(enclosing.to_owned())
            .or_default()
            .push(range);
    }

    fn is_in_protected_range(&self, enclosing: &str, location: SourceLocation) -> bool {
        self.protected_ranges_by_function
            .get(enclosing)
            .is_some_and(|ranges| {
                ranges
                    .iter()
                    .any(|range| location.start >= range.start && location.end <= range.end)
            })
    }

    fn node_for_target(&mut self, target: &SemanticTarget) -> Option<crate::graph::NodeId> {
        match target {
            SemanticTarget::FirstPartyDefinition(definition) => self
                .definition_index
                .get(definition)
                .and_then(|qualified_name| self.graph.node_id(qualified_name)),
            SemanticTarget::ExternalQualifiedNames(names) => self.external_node_for_aliases(names),
            SemanticTarget::Unknown => None,
        }
    }

    fn external_node_for_aliases(
        &mut self,
        aliases: &BTreeSet<String>,
    ) -> Option<crate::graph::NodeId> {
        aliases.iter().find_map(|alias| {
            self.external_blocking_status(alias.as_str())
                .map(|status| self.ensure_external_node(alias.as_str(), status))
        })
    }

    fn ensure_external_node(
        &mut self,
        alias: &str,
        status: BlockingStatus,
    ) -> crate::graph::NodeId {
        self.graph.add_node(
            alias.to_string(),
            CallableKind::Function,
            false,
            None,
            status,
        )
    }

    fn external_blocking_status(&self, alias: &str) -> Option<BlockingStatus> {
        if self.blocking_database.matches_blocking_target(alias)
            || self.stub_blocking_targets.contains(alias)
        {
            Some(BlockingStatus::KnownBlocking)
        } else {
            None
        }
    }

    fn is_parsed_decorator(&self, enclosing: &str, expression: &str) -> bool {
        self.decorators_by_function
            .get(enclosing)
            .is_some_and(|decorators| {
                decorators.contains(expression.trim().trim_start_matches('@'))
            })
    }
}

fn stub_blocking_targets(syntax_by_path: &BTreeMap<Utf8PathBuf, FileSyntax>) -> BTreeSet<String> {
    let mut targets = BTreeSet::new();
    for (path, syntax) in syntax_by_path {
        if syntax.kind != FileKind::Stub {
            continue;
        }
        let Some(module) = path.file_stem() else {
            continue;
        };
        for function in &syntax.functions {
            if function.decorators.iter().any(|decorator| {
                let decorator = decorator.trim().trim_start_matches('@').trim();
                decorator == "blocking" || decorator.ends_with(".blocking")
            }) {
                targets.insert(format!("{module}.{}", function.qualified_name));
            }
        }
    }
    targets
}

fn first_party_blocking_status(
    path: &Utf8PathBuf,
    qualified_name: &str,
    blocking_database: &BlockingDatabase,
) -> BlockingStatus {
    let stem = path.with_extension("");
    let module = stem
        .file_name()
        .map_or_else(String::new, std::string::ToString::to_string);
    let module_name = format!("{module}.{qualified_name}");
    if blocking_database.matches_blocking_target(qualified_name)
        || blocking_database.matches_blocking_target(&module_name)
    {
        BlockingStatus::KnownBlocking
    } else {
        BlockingStatus::Unknown
    }
}

fn callable_kind(function: &FunctionSyntax) -> CallableKind {
    if function.name.starts_with("<lambda>") || function.qualified_name.contains(".<lambda>@") {
        return CallableKind::Lambda;
    }
    if function.name.starts_with("__") && function.name.ends_with("__") {
        return CallableKind::DunderMethod;
    }
    if has_decorator(function, "property") {
        return CallableKind::Property;
    }
    if has_decorator(function, "classmethod") {
        return CallableKind::ClassMethod;
    }
    if has_decorator(function, "staticmethod") {
        return CallableKind::StaticMethod;
    }
    if function.qualified_name.contains('.') {
        if function.is_async {
            CallableKind::AsyncMethod
        } else {
            CallableKind::Method
        }
    } else if function.is_async {
        CallableKind::AsyncFunction
    } else {
        CallableKind::Function
    }
}

fn has_decorator(function: &FunctionSyntax, decorator: &str) -> bool {
    function
        .decorators
        .iter()
        .any(|value| value == decorator || value.ends_with(&format!(".{decorator}")))
}

fn edge_kind(
    call: &SemanticCall,
    target_kind: CallableKind,
    is_decorator: bool,
) -> Option<EdgeKind> {
    let expression = call.expression.trim();
    if matches!(target_kind, CallableKind::Property) {
        return Some(EdgeKind::PropertyAccess);
    }
    if is_decorator {
        return Some(EdgeKind::DecoratorCall);
    }
    if matches!(target_kind, CallableKind::DunderMethod) {
        return Some(EdgeKind::ImplicitDunder);
    }
    if !expression.contains('(') {
        return None;
    }
    if expression.starts_with("super().") {
        return Some(EdgeKind::SuperCall);
    }
    if expression
        .split_once('(')
        .is_some_and(|(callee, _)| callee.contains('.'))
    {
        Some(EdgeKind::MethodCall)
    } else {
        Some(EdgeKind::DirectCall)
    }
}

fn callable_argument(
    expression: &str,
    wrapper_name: &str,
    executor_wrappers: &BTreeMap<String, ExecutorWrapperConfig>,
) -> Option<String> {
    let (callee, args) = expression.trim().split_once('(')?;
    if callee.trim() != wrapper_name {
        return None;
    }
    let args = args.strip_suffix(')')?;
    let arguments = split_arguments(args);
    let argument = if wrapper_name == "asyncio.to_thread" {
        arguments.first()?.as_str()
    } else if wrapper_name.ends_with(".run_in_executor") {
        arguments.get(1)?.as_str()
    } else {
        let wrapper = executor_wrappers.get(wrapper_name)?;
        match &wrapper.callable_param {
            CallableParam::Position(index) => {
                arguments.get(usize::try_from(*index).ok()?)?.as_str()
            }
            CallableParam::Keyword(keyword) => {
                arguments
                    .iter()
                    .find_map(|argument| {
                        argument
                            .split_once('=')
                            .filter(|(name, _)| name.trim() == keyword)
                    })?
                    .1
            }
        }
    };
    let argument = argument.trim();
    if argument.is_empty() || argument.contains('(') {
        None
    } else {
        Some(argument.to_string())
    }
}

fn callable_argument_range(
    expression: &str,
    expression_start: u32,
    wrapper_name: &str,
    executor_wrappers: &BTreeMap<String, ExecutorWrapperConfig>,
) -> Option<SourceRange> {
    let (callee, args) = expression.trim().split_once('(')?;
    if callee.trim() != wrapper_name {
        return None;
    }
    let args = args.strip_suffix(')')?;
    let argument_index = if wrapper_name == "asyncio.to_thread" {
        0
    } else if wrapper_name.ends_with(".run_in_executor") {
        1
    } else {
        match &executor_wrappers.get(wrapper_name)?.callable_param {
            CallableParam::Position(index) => usize::try_from(*index).ok()?,
            CallableParam::Keyword(keyword) => {
                return keyword_argument_range(expression, expression_start, args, keyword);
            }
        }
    };
    positional_argument_range(expression, expression_start, args, argument_index)
}

fn positional_argument_range(
    expression: &str,
    expression_start: u32,
    args: &str,
    argument_index: usize,
) -> Option<SourceRange> {
    argument_ranges(expression, expression_start, args)
        .into_iter()
        .nth(argument_index)
        .map(|(_, range)| range)
}

fn keyword_argument_range(
    expression: &str,
    expression_start: u32,
    args: &str,
    keyword: &str,
) -> Option<SourceRange> {
    argument_ranges(expression, expression_start, args)
        .into_iter()
        .find_map(|(argument, range)| {
            let (name, value) = argument.split_once('=')?;
            if name.trim() != keyword {
                return None;
            }
            let value_offset = argument.len() - value.trim_start().len();
            let value_offset = u32::try_from(value_offset).ok()?;
            Some(SourceRange {
                start: range.start.checked_add(value_offset)?,
                end: range.end,
            })
        })
}

fn argument_ranges(
    expression: &str,
    expression_start: u32,
    args: &str,
) -> Vec<(String, SourceRange)> {
    let Some(args_start) = expression.find('(').map(|index| index + 1) else {
        return Vec::new();
    };
    let mut ranges = Vec::new();
    let mut depth = 0_u32;
    let mut start = 0_usize;
    for (index, ch) in args.char_indices() {
        match ch {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                push_argument_range(
                    &mut ranges,
                    expression_start,
                    args_start,
                    args,
                    start,
                    index,
                );
                start = index + 1;
            }
            _ => {}
        }
    }
    if start < args.len() {
        push_argument_range(
            &mut ranges,
            expression_start,
            args_start,
            args,
            start,
            args.len(),
        );
    }
    ranges
}

fn push_argument_range(
    ranges: &mut Vec<(String, SourceRange)>,
    expression_start: u32,
    args_start: usize,
    args: &str,
    start: usize,
    end: usize,
) {
    let raw = &args[start..end];
    let leading = raw.len() - raw.trim_start().len();
    let trailing = raw.trim_end().len();
    let trimmed_start = start + leading;
    let trimmed_end = start + trailing;
    if trimmed_start >= trimmed_end {
        return;
    }
    let Some(relative_start) = u32::try_from(args_start + trimmed_start).ok() else {
        return;
    };
    let Some(relative_end) = u32::try_from(args_start + trimmed_end).ok() else {
        return;
    };
    let Some(start) = expression_start.checked_add(relative_start) else {
        return;
    };
    let Some(end) = expression_start.checked_add(relative_end) else {
        return;
    };
    ranges.push((
        args[trimmed_start..trimmed_end].to_string(),
        SourceRange { start, end },
    ));
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
