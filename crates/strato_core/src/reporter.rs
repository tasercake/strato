//! Deterministic diagnostic reporting from propagated blocking facts.

use std::{cmp::Ordering, collections::BTreeMap};

use camino::{Utf8Path, Utf8PathBuf};
use serde::Serialize;
use serde_json::Value;

use crate::{
    graph::{CallGraph, EdgeKind, NodeId},
    propagator::{BlockingReason, ChainLink, PropagationResult},
    types::{
        AnalysisWarning, BlockingDatabase, DiagnosticSeverity, FileKind, FileSyntax,
        InterventionStrategy, SourceLocation,
    },
};

/// JSON schema version emitted by the core reporter.
pub const REPORT_VERSION: &str = "1.0";

/// Complete deterministic JSON-facing report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Report {
    /// Output schema version.
    pub version: &'static str,
    /// Blocking diagnostics in deterministic order.
    pub diagnostics: Vec<Diagnostic>,
    /// Recoverable analysis warnings in deterministic order.
    pub warnings: Vec<ReportWarning>,
}

impl Report {
    /// Serialize this report as a JSON value.
    #[must_use]
    pub fn to_json_value(&self) -> Value {
        serde_json::to_value(self).expect("report serialization is infallible")
    }
}

/// A single blocking diagnostic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Diagnostic {
    /// Stable Strato diagnostic code.
    pub code: ErrorCode,
    /// Diagnostic severity.
    pub severity: ReportSeverity,
    /// Human-readable diagnostic message.
    pub message: String,
    /// Primary intervention location.
    pub primary_location: ReportLocation,
    /// Additional deterministic context locations.
    pub related_locations: Vec<RelatedLocation>,
    /// Async-entry to blocking-root chain.
    pub chain: Vec<ReportChainLink>,
    /// Remediation help text.
    pub help: String,
    /// Intervention strategy used for primary-location selection.
    pub intervention_strategy: ReportInterventionStrategy,
}

/// Stable diagnostic code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum ErrorCode {
    /// Direct blocking call in async function.
    #[serde(rename = "STRATO001")]
    Strato001,
    /// Transitive blocking call reachable from async context.
    #[serde(rename = "STRATO002")]
    Strato002,
    /// Blocking property access from async context.
    #[serde(rename = "STRATO003")]
    Strato003,
    /// Blocking dunder invocation from async context.
    #[serde(rename = "STRATO004")]
    Strato004,
}

/// JSON diagnostic severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ReportSeverity {
    /// Error severity.
    Error,
    /// Warning severity.
    Warning,
}

impl From<DiagnosticSeverity> for ReportSeverity {
    fn from(value: DiagnosticSeverity) -> Self {
        match value {
            DiagnosticSeverity::Error => Self::Error,
            DiagnosticSeverity::Warning => Self::Warning,
        }
    }
}

/// JSON intervention-strategy label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ReportInterventionStrategy {
    /// Deepest first-party call-site strategy.
    #[serde(rename = "first-party-deepest")]
    FirstPartyDeepest,
    /// Async boundary strategy.
    #[serde(rename = "async-boundary")]
    AsyncBoundary,
}

impl From<InterventionStrategy> for ReportInterventionStrategy {
    fn from(value: InterventionStrategy) -> Self {
        match value {
            InterventionStrategy::FirstPartyDeepest => Self::FirstPartyDeepest,
            InterventionStrategy::AsyncBoundary => Self::AsyncBoundary,
        }
    }
}

/// 1-indexed JSON source location.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReportLocation {
    /// Project-relative `/`-normalized file path.
    pub file: String,
    /// 1-indexed line.
    pub line: usize,
    /// 1-indexed character column.
    pub column: usize,
    /// Optional exclusive end line.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_line: Option<usize>,
    /// Optional exclusive end column.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_column: Option<usize>,
}

/// Related diagnostic context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RelatedLocation {
    /// Project-relative `/`-normalized file path.
    pub file: String,
    /// 1-indexed line.
    pub line: usize,
    /// 1-indexed character column.
    pub column: usize,
    /// Context message.
    pub message: String,
}

/// One function entry in the diagnostic chain.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct ReportChainLink {
    /// Stable display name.
    pub function: String,
    /// Project-relative file, or null for external roots.
    pub file: Option<String>,
    /// Definition line, or null for external roots.
    pub line: Option<usize>,
    /// Whether this chain entry is async.
    pub is_async: bool,
    /// Whether this chain entry is first-party.
    pub is_first_party: bool,
}

/// A recoverable analysis warning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReportWarning {
    /// Human-readable warning message.
    pub message: String,
    /// Project-relative file path when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
}

/// Inputs required to build a deterministic report.
#[derive(Clone, Copy)]
pub struct ReportInput<'a> {
    /// Project root used for relative output paths.
    pub project_root: &'a Utf8Path,
    /// Final call graph after propagation.
    pub graph: &'a CallGraph,
    /// SCC propagation facts.
    pub propagation: &'a PropagationResult,
    /// Syntax facts for first-party location ownership.
    pub syntax_by_path: &'a BTreeMap<Utf8PathBuf, FileSyntax>,
    /// Source text by path for byte-offset to 1-indexed coordinate conversion.
    pub source_text_by_path: &'a BTreeMap<Utf8PathBuf, String>,
    /// Effective blocking database for help text.
    pub blocking_database: &'a BlockingDatabase,
    /// Recoverable warnings collected by earlier phases.
    pub warnings: &'a [AnalysisWarning],
    /// Configured diagnostic severity.
    pub severity: DiagnosticSeverity,
    /// Configured intervention strategy.
    pub intervention_strategy: InterventionStrategy,
}

/// Convert propagated blocking facts into deterministic output-facing diagnostics.
#[must_use]
pub fn build_report(input: ReportInput<'_>) -> Report {
    let context = ReportContext::new(&input);
    let mut diagnostics = input
        .propagation
        .blocking_reasons
        .iter()
        .filter_map(|(node_id, reason)| diagnostic_for_reason(*node_id, reason, &input, &context))
        .collect::<Vec<_>>();
    diagnostics.sort_by(compare_diagnostics);

    let mut warnings = input
        .warnings
        .iter()
        .map(|warning| report_warning(warning, input.project_root))
        .collect::<Vec<_>>();
    warnings.sort_by(compare_warnings);

    Report {
        version: REPORT_VERSION,
        diagnostics,
        warnings,
    }
}

struct ReportContext {
    definitions: BTreeMap<String, DefinitionLocation>,
}

impl ReportContext {
    fn new(input: &ReportInput<'_>) -> Self {
        let mut definitions = BTreeMap::new();
        for (path, syntax) in input.syntax_by_path {
            for function in &syntax.functions {
                if let Some(location) = location_for(
                    path,
                    function.location,
                    input.project_root,
                    input.source_text_by_path,
                ) {
                    definitions.insert(
                        function.qualified_name.clone(),
                        DefinitionLocation {
                            display_name: first_party_display_name(
                                path,
                                input.project_root,
                                &function.qualified_name,
                                function.is_async,
                            ),
                            location: location.clone(),
                            is_async: function.is_async,
                            is_first_party: syntax.kind == FileKind::Source,
                            is_stub_blocking: syntax.kind == FileKind::Stub
                                && function
                                    .decorators
                                    .iter()
                                    .any(|decorator| is_blocking_decorator(decorator)),
                        },
                    );
                    if syntax.kind == FileKind::Stub
                        && function
                            .decorators
                            .iter()
                            .any(|decorator| is_blocking_decorator(decorator))
                    {
                        let module_name = module_name_for_path(path, input.project_root);
                        definitions.insert(
                            format!("{module_name}.{}", function.qualified_name),
                            DefinitionLocation {
                                display_name: format!("{module_name}.{}", function.qualified_name),
                                location,
                                is_async: function.is_async,
                                is_first_party: false,
                                is_stub_blocking: true,
                            },
                        );
                    }
                }
            }
        }
        Self { definitions }
    }
}

fn is_blocking_decorator(decorator: &str) -> bool {
    let decorator = decorator.trim().trim_start_matches('@').trim();
    decorator == "blocking" || decorator.ends_with(".blocking")
}

fn module_name_for_path(path: &Utf8Path, project_root: &Utf8Path) -> String {
    let relative = path.strip_prefix(project_root).unwrap_or(path);
    let without_extension = relative.with_extension("");
    let components = without_extension
        .components()
        .filter_map(|component| match component {
            camino::Utf8Component::Normal(part) if part != "stubs" => Some(part.to_owned()),
            _ => None,
        })
        .collect::<Vec<_>>();
    components.join(".")
}

#[derive(Clone)]
struct DefinitionLocation {
    display_name: String,
    location: ReportLocation,
    is_async: bool,
    is_first_party: bool,
    is_stub_blocking: bool,
}

fn diagnostic_for_reason(
    node_id: NodeId,
    reason: &BlockingReason,
    input: &ReportInput<'_>,
    context: &ReportContext,
) -> Option<Diagnostic> {
    let chain = async_first_party_chain(reason)?;
    if input.graph.nodes().get(node_id.0)?.qualified_name != chain[0].function_name {
        return None;
    }

    let code = classify_error_code(chain);
    let primary_link = select_primary_link(chain, code, input.intervention_strategy)?;
    let mut primary_location = location_for_link(
        primary_link,
        input.project_root,
        input.source_text_by_path,
        context,
    )?;
    if code == ErrorCode::Strato003 {
        adjust_bare_property_location(&mut primary_location, primary_link, input);
    }
    let root = input.graph.nodes().get(reason.root_cause.0)?;
    let root_name = root.qualified_name.as_str();
    let message_subject = special_subject(chain, code).unwrap_or(root_name);
    let diagnostic_chain = report_chain(chain, root_name, context);
    let related_locations = related_locations(chain, code, root_name, context, &primary_location);
    let help = help_text(code, root_name, input.blocking_database);

    Some(Diagnostic {
        code,
        severity: input.severity.into(),
        message: message_text(code, &chain[0].function_name, message_subject),
        primary_location,
        related_locations,
        chain: diagnostic_chain,
        help,
        intervention_strategy: input.intervention_strategy.into(),
    })
}

fn adjust_bare_property_location(
    location: &mut ReportLocation,
    link: &ChainLink,
    input: &ReportInput<'_>,
) {
    let Some(definition) = link.function_name.as_str().pipe(|name| {
        input.syntax_by_path.values().find_map(|syntax| {
            syntax
                .functions
                .iter()
                .find(|function| function.qualified_name == name)
                .map(|_| syntax.path.clone())
        })
    }) else {
        return;
    };
    let Some(source) = input.source_text_by_path.get(&definition) else {
        return;
    };
    let Some(source_line) = source.lines().nth(location.line.saturating_sub(1)) else {
        return;
    };
    let attr_index = location.column.saturating_sub(1);
    if attr_index > source_line.len() {
        return;
    }
    let before = &source_line[..attr_index];
    let after = &source_line[attr_index..];
    let leading = before.len() - before.trim_start().len();
    let expression_prefix = before[leading..].trim_end();
    if !expression_prefix.is_empty()
        && expression_prefix
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '.')
        && after
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch.is_ascii_whitespace())
    {
        location.column = leading + 1;
    }
}

trait Pipe: Sized {
    fn pipe<T>(self, f: impl FnOnce(Self) -> T) -> T {
        f(self)
    }
}
impl<T> Pipe for T {}

fn special_subject(chain: &[ChainLink], code: ErrorCode) -> Option<&str> {
    let edge_kind = match code {
        ErrorCode::Strato003 => EdgeKind::PropertyAccess,
        ErrorCode::Strato004 => EdgeKind::ImplicitDunder,
        ErrorCode::Strato001 | ErrorCode::Strato002 => return None,
    };
    chain
        .iter()
        .find(|link| link.edge_kind == edge_kind)
        .map(|link| link.callee_name.as_str())
}

fn async_first_party_chain(reason: &BlockingReason) -> Option<&[ChainLink]> {
    let chain = reason.chain_links.as_slice();
    let first = chain.first()?;
    (first.is_async && first.is_first_party).then_some(chain)
}

fn classify_error_code(chain: &[ChainLink]) -> ErrorCode {
    for link in chain {
        match link.edge_kind {
            EdgeKind::PropertyAccess => return ErrorCode::Strato003,
            EdgeKind::ImplicitDunder => return ErrorCode::Strato004,
            _ => {}
        }
    }
    if chain.len() == 1 && chain[0].is_async {
        ErrorCode::Strato001
    } else {
        ErrorCode::Strato002
    }
}

fn select_primary_link(
    chain: &[ChainLink],
    code: ErrorCode,
    strategy: InterventionStrategy,
) -> Option<&ChainLink> {
    match code {
        ErrorCode::Strato003 => chain
            .iter()
            .find(|link| link.edge_kind == EdgeKind::PropertyAccess),
        ErrorCode::Strato004 => chain
            .iter()
            .find(|link| link.edge_kind == EdgeKind::ImplicitDunder),
        ErrorCode::Strato001 | ErrorCode::Strato002 => match strategy {
            InterventionStrategy::FirstPartyDeepest => chain
                .iter()
                .rev()
                .find(|link| link.is_first_party && link.call_site_location.is_some())
                .or_else(|| chain.first()),
            InterventionStrategy::AsyncBoundary => select_async_boundary(chain),
        },
    }
}

fn select_async_boundary(chain: &[ChainLink]) -> Option<&ChainLink> {
    chain
        .windows(2)
        .find_map(|window| (window[0].is_async && !window[1].is_async).then_some(&window[0]))
        .or_else(|| chain.first())
}

fn location_for_link(
    link: &ChainLink,
    project_root: &Utf8Path,
    source_text_by_path: &BTreeMap<Utf8PathBuf, String>,
    context: &ReportContext,
) -> Option<ReportLocation> {
    let definition = context.definitions.get(&link.function_name)?;
    let path = Utf8PathBuf::from(project_root).join(&definition.location.file);
    location_for(
        &path,
        link.call_site_location?,
        project_root,
        source_text_by_path,
    )
}

fn report_chain(
    chain: &[ChainLink],
    root_name: &str,
    context: &ReportContext,
) -> Vec<ReportChainLink> {
    let mut entries = chain
        .iter()
        .map(|link| {
            chain_entry(
                &link.function_name,
                link.is_async,
                link.is_first_party,
                context,
            )
        })
        .collect::<Vec<_>>();
    entries.push(chain_entry(root_name, false, false, context));
    entries
}

fn chain_entry(
    function_name: &str,
    fallback_async: bool,
    fallback_first_party: bool,
    context: &ReportContext,
) -> ReportChainLink {
    if let Some(definition) = context.definitions.get(function_name) {
        return ReportChainLink {
            function: definition.display_name.clone(),
            file: definition
                .is_first_party
                .then(|| definition.location.file.clone()),
            line: definition
                .is_first_party
                .then_some(definition.location.line),
            is_async: definition.is_async,
            is_first_party: definition.is_first_party,
        };
    }
    ReportChainLink {
        function: display_name(function_name),
        file: None,
        line: None,
        is_async: fallback_async,
        is_first_party: fallback_first_party,
    }
}

fn related_locations(
    chain: &[ChainLink],
    code: ErrorCode,
    root_name: &str,
    context: &ReportContext,
    primary_location: &ReportLocation,
) -> Vec<RelatedLocation> {
    let mut locations = Vec::new();
    if let Some(async_definition) = context.definitions.get(&chain[0].function_name) {
        locations.push(related(
            &async_definition.location,
            format!(
                "async function {} defined here",
                context
                    .definitions
                    .get(&chain[0].function_name)
                    .map_or_else(
                        || display_name(&chain[0].function_name),
                        |definition| { definition.display_name.clone() }
                    )
            ),
        ));
    }

    match code {
        ErrorCode::Strato001 => {}
        ErrorCode::Strato002 => {
            for link in chain.iter().skip(1).filter(|link| link.is_first_party) {
                if let Some(definition) = context.definitions.get(&link.function_name) {
                    let mut location = definition.location.clone();
                    location.column = 1;
                    locations.push(related(
                        &location,
                        format!("{} defined here", definition.display_name),
                    ));
                }
            }
            locations.push(related(
                primary_location,
                format!("blocking call: {root_name}"),
            ));
        }
        ErrorCode::Strato003 => add_special_related(
            &mut locations,
            chain,
            EdgeKind::PropertyAccess,
            "property getter defined here",
            context,
        ),
        ErrorCode::Strato004 => add_special_related(
            &mut locations,
            chain,
            EdgeKind::ImplicitDunder,
            "dunder method defined here",
            context,
        ),
    }

    if let Some(root_definition) = context.definitions.get(root_name) {
        let message = if root_definition.is_stub_blocking {
            format!("{root_name} marked blocking with @blocking in stub")
        } else {
            format!("blocking function {} defined here", display_name(root_name))
        };
        locations.push(related(&root_definition.location, message));
    }

    dedupe_related(locations)
}

fn add_special_related(
    locations: &mut Vec<RelatedLocation>,
    chain: &[ChainLink],
    edge_kind: EdgeKind,
    message: &str,
    context: &ReportContext,
) {
    if let Some(link) = chain.iter().find(|link| link.edge_kind == edge_kind)
        && let Some(definition) = context.definitions.get(&link.callee_name)
    {
        locations.push(related(&definition.location, message.to_string()));
    }
}

fn related(location: &ReportLocation, message: String) -> RelatedLocation {
    RelatedLocation {
        file: location.file.clone(),
        line: location.line,
        column: location.column,
        message,
    }
}

fn dedupe_related(locations: Vec<RelatedLocation>) -> Vec<RelatedLocation> {
    let mut deduped = Vec::new();
    for location in locations {
        if !deduped.contains(&location) {
            deduped.push(location);
        }
    }
    deduped
}

fn message_text(code: ErrorCode, async_name: &str, root_name: &str) -> String {
    match code {
        ErrorCode::Strato001 => format!(
            "Direct blocking call to '{}' in async function '{}'",
            display_name(root_name),
            display_name(async_name)
        ),
        ErrorCode::Strato002 => "Transitive blocking call reachable from async context".to_string(),
        ErrorCode::Strato003 => format!(
            "Async function '{}' accesses blocking property '{}'",
            display_name(async_name),
            display_name(root_name)
        ),
        ErrorCode::Strato004 => format!(
            "Async function '{}' calls blocking dunder method '{}'",
            display_name(async_name),
            display_name(root_name)
        ),
    }
}

fn help_text(code: ErrorCode, _root_name: &str, _blocking_database: &BlockingDatabase) -> String {
    match code {
        ErrorCode::Strato003 => {
            "Avoid blocking I/O in property getters used from async code".to_string()
        }
        ErrorCode::Strato004 => {
            "Avoid blocking I/O in dunder methods invoked from async code".to_string()
        }
        ErrorCode::Strato001 => {
            "Wrap in `await asyncio.to_thread(...)` or use async alternative".to_string()
        }
        ErrorCode::Strato002 => {
            "Wrap the blocking call in `await asyncio.to_thread(...)` or use async alternative"
                .to_string()
        }
    }
}

fn report_warning(warning: &AnalysisWarning, project_root: &Utf8Path) -> ReportWarning {
    match warning {
        AnalysisWarning::SyntaxError { path, error: _ } => {
            let file = relative_path(path, project_root);
            ReportWarning {
                message: format!("Syntax error in {file}"),
                file: Some(file),
            }
        }
        AnalysisWarning::Adapter { path, error } => ReportWarning {
            message: format!("adapter warning: {error}"),
            file: path.as_ref().map(|path| relative_path(path, project_root)),
        },
        AnalysisWarning::General { path, message } => ReportWarning {
            message: message.clone(),
            file: path.as_ref().map(|path| relative_path(path, project_root)),
        },
    }
}

fn location_for(
    path: &Utf8Path,
    source_location: SourceLocation,
    project_root: &Utf8Path,
    source_text_by_path: &BTreeMap<Utf8PathBuf, String>,
) -> Option<ReportLocation> {
    let source = source_text_by_path.get(path)?;
    let (line, column) = coordinate(source, source_location.start)?;
    let _ = source_location.end;
    Some(ReportLocation {
        file: relative_path(path, project_root),
        line,
        column,
        end_line: None,
        end_column: None,
    })
}

fn coordinate(source: &str, offset: u32) -> Option<(usize, usize)> {
    let offset = usize::try_from(offset).ok()?;
    if offset > source.len() || !source.is_char_boundary(offset) {
        return None;
    }
    let prefix = &source[..offset];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let column = prefix
        .rsplit_once('\n')
        .map_or(prefix, |(_, suffix)| suffix)
        .chars()
        .count()
        + 1;
    Some((line, column))
}

fn relative_path(path: &Utf8Path, project_root: &Utf8Path) -> String {
    path.strip_prefix(project_root)
        .unwrap_or(path)
        .as_str()
        .replace('\\', "/")
}

fn first_party_display_name(
    path: &Utf8Path,
    project_root: &Utf8Path,
    qualified_name: &str,
    is_async: bool,
) -> String {
    if is_async {
        return display_name(qualified_name);
    }
    let relative = path.strip_prefix(project_root).unwrap_or(path);
    let stem_path = relative.with_extension("");
    let module = stem_path.as_str().replace('\\', "/").replace('/', ".");
    let module = module.strip_suffix(".__init__").unwrap_or(&module);
    if module == "main"
        || module.rsplit('.').next() == Some("main")
        || qualified_name.starts_with(&format!("{module}."))
    {
        display_name(qualified_name)
    } else {
        format!("{module}.{}", display_name(qualified_name))
    }
}

fn display_name(name: &str) -> String {
    name.strip_prefix("main.").unwrap_or(name).to_string()
}

fn compare_diagnostics(left: &Diagnostic, right: &Diagnostic) -> Ordering {
    left.primary_location
        .file
        .cmp(&right.primary_location.file)
        .then_with(|| left.primary_location.line.cmp(&right.primary_location.line))
        .then_with(|| {
            left.primary_location
                .column
                .cmp(&right.primary_location.column)
        })
        .then_with(|| left.code.cmp(&right.code))
        .then_with(|| left.chain.cmp(&right.chain))
        .then_with(|| left.message.cmp(&right.message))
}

fn compare_warnings(left: &ReportWarning, right: &ReportWarning) -> Ordering {
    left.file
        .cmp(&right.file)
        .then_with(|| left.message.cmp(&right.message))
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use camino::Utf8PathBuf;
    use serde_json::json;

    use crate::{
        graph::{BlockingStatus, CallEdge, CallGraph, CallableKind, EdgeKind, NodeId},
        propagator::propagate_blocking,
        reporter::{ReportInput, build_report},
        types::{
            AnalysisWarning, BlockingCategory, BlockingDatabase, BlockingEntry, DiagnosticSeverity,
            FileKind, FileSyntax, FunctionSyntax, InterventionStrategy, SourceLocation,
        },
    };

    fn loc(source: &str, needle: &str) -> SourceLocation {
        let start = source.find(needle).expect("needle in source");
        SourceLocation {
            start: start.try_into().expect("test offset fits u32"),
            end: (start + needle.len())
                .try_into()
                .expect("test offset fits u32"),
        }
    }

    fn loc_last(source: &str, needle: &str) -> SourceLocation {
        let start = source.rfind(needle).expect("needle in source");
        SourceLocation {
            start: start.try_into().expect("test offset fits u32"),
            end: (start + needle.len())
                .try_into()
                .expect("test offset fits u32"),
        }
    }

    fn function(
        graph: &mut CallGraph,
        name: &str,
        is_async: bool,
        status: BlockingStatus,
        location: SourceLocation,
    ) -> NodeId {
        graph.add_node(
            name.to_string(),
            if is_async {
                CallableKind::AsyncFunction
            } else {
                CallableKind::Function
            },
            is_async,
            Some(location),
            status,
        )
    }

    fn external(graph: &mut CallGraph, name: &str) -> NodeId {
        graph.add_node(
            name.to_string(),
            CallableKind::Function,
            false,
            None,
            BlockingStatus::KnownBlocking,
        )
    }

    fn call(
        graph: &mut CallGraph,
        from: NodeId,
        to: NodeId,
        kind: EdgeKind,
        location: SourceLocation,
    ) {
        graph.add_edge(CallEdge {
            from,
            to,
            kind,
            location,
            in_executor: false,
            via: None,
            protected: false,
        });
    }

    fn syntax(
        path: &Utf8PathBuf,
        functions: Vec<(&str, bool, SourceLocation)>,
    ) -> BTreeMap<Utf8PathBuf, FileSyntax> {
        BTreeMap::from([(
            path.clone(),
            FileSyntax {
                path: path.clone(),
                kind: FileKind::Source,
                functions: functions
                    .into_iter()
                    .map(|(name, is_async, location)| FunctionSyntax {
                        name: name.rsplit('.').next().unwrap_or(name).to_string(),
                        qualified_name: name.to_string(),
                        is_async,
                        decorators: Vec::new(),
                        location,
                    })
                    .collect(),
                classes: Vec::new(),
                imports: Vec::new(),
                call_sites: Vec::new(),
            },
        )])
    }

    fn database() -> BlockingDatabase {
        BlockingDatabase {
            entries: BTreeMap::from([(
                "time.sleep".to_string(),
                BlockingEntry {
                    name: "time.sleep".to_string(),
                    help: "Use asyncio.sleep()".to_string(),
                    category: BlockingCategory::Sleep,
                },
            )]),
            blocking_modules: BTreeSet::new(),
        }
    }

    fn report_json(
        graph: &CallGraph,
        propagation: &crate::propagator::PropagationResult,
        path: &Utf8PathBuf,
        source: &str,
        warnings: &[AnalysisWarning],
    ) -> serde_json::Value {
        let root = Utf8PathBuf::from("/workspace");
        let source_text = BTreeMap::from([(path.clone(), source.to_string())]);
        let syntax_by_path = syntax(
            path,
            graph
                .nodes()
                .iter()
                .filter_map(|node| {
                    node.location
                        .map(|location| (node.qualified_name.as_str(), node.is_async, location))
                })
                .collect(),
        );
        build_report(ReportInput {
            project_root: &root,
            graph,
            propagation,
            syntax_by_path: &syntax_by_path,
            source_text_by_path: &source_text,
            blocking_database: &database(),
            warnings,
            severity: DiagnosticSeverity::Error,
            intervention_strategy: InterventionStrategy::FirstPartyDeepest,
        })
        .to_json_value()
    }

    #[test]
    fn reporter_diagnostics_json_shape_and_one_indexed_coordinates_are_deterministic() {
        let source = "async def handler():\n    helper()\n\ndef helper():\n    time.sleep(1)\n";
        let path = Utf8PathBuf::from("/workspace/main.py");
        let mut graph = CallGraph::default();
        let handler = function(
            &mut graph,
            "handler",
            true,
            BlockingStatus::Unknown,
            loc(source, "handler"),
        );
        let helper = function(
            &mut graph,
            "helper",
            false,
            BlockingStatus::Unknown,
            loc_last(source, "helper"),
        );
        let sleep = external(&mut graph, "time.sleep");
        call(
            &mut graph,
            handler,
            helper,
            EdgeKind::DirectCall,
            loc(source, "helper()"),
        );
        call(
            &mut graph,
            helper,
            sleep,
            EdgeKind::DirectCall,
            loc(source, "time.sleep(1)"),
        );
        let propagation = propagate_blocking(&mut graph);

        let first = report_json(&graph, &propagation, &path, source, &[]);
        let second = report_json(&graph, &propagation, &path, source, &[]);

        assert_eq!(first, second);
        assert_eq!(
            serde_json::to_string(&first).unwrap(),
            serde_json::to_string(&second).unwrap()
        );
        assert_eq!(
            first,
            json!({
                "version": "1.0",
                "diagnostics": [{
                    "code": "STRATO002",
                    "severity": "error",
                    "message": "Transitive blocking call reachable from async context",
                    "primary_location": {"file": "main.py", "line": 5, "column": 5},
                    "related_locations": [
                        {"file": "main.py", "line": 1, "column": 11, "message": "async function handler defined here"},
                        {"file": "main.py", "line": 4, "column": 1, "message": "helper defined here"},
                        {"file": "main.py", "line": 5, "column": 5, "message": "blocking call: time.sleep"}
                    ],
                    "chain": [
                        {"function": "handler", "file": "main.py", "line": 1, "is_async": true, "is_first_party": true},
                        {"function": "helper", "file": "main.py", "line": 4, "is_async": false, "is_first_party": true},
                        {"function": "time.sleep", "file": null, "line": null, "is_async": false, "is_first_party": false}
                    ],
                    "help": "Wrap the blocking call in `await asyncio.to_thread(...)` or use async alternative",
                    "intervention_strategy": "first-party-deepest"
                }],
                "warnings": []
            })
        );
    }

    #[test]
    fn reporter_diagnostics_sort_by_location_code_chain_and_warn_deterministically() {
        let source = "async def zed():\n    time.sleep(1)\nasync def alpha():\n    time.sleep(1)\n";
        let path = Utf8PathBuf::from("/workspace/main.py");
        let mut graph = CallGraph::default();
        let zed = function(
            &mut graph,
            "zed",
            true,
            BlockingStatus::Unknown,
            loc(source, "zed"),
        );
        let alpha = function(
            &mut graph,
            "alpha",
            true,
            BlockingStatus::Unknown,
            loc(source, "alpha"),
        );
        let sleep = external(&mut graph, "time.sleep");
        call(
            &mut graph,
            zed,
            sleep,
            EdgeKind::DirectCall,
            loc(source, "time.sleep(1)"),
        );
        let second_sleep_offset = source.rfind("time.sleep(1)").expect("second sleep");
        call(
            &mut graph,
            alpha,
            sleep,
            EdgeKind::DirectCall,
            SourceLocation {
                start: second_sleep_offset.try_into().unwrap(),
                end: (second_sleep_offset + "time.sleep(1)".len())
                    .try_into()
                    .unwrap(),
            },
        );
        let propagation = propagate_blocking(&mut graph);
        let warnings = vec![
            AnalysisWarning::Adapter {
                path: None,
                error: "zeta".to_string(),
            },
            AnalysisWarning::SyntaxError {
                path: path.clone(),
                error: "alpha".to_string(),
            },
            AnalysisWarning::Adapter {
                path: Some(path.clone()),
                error: "beta".to_string(),
            },
        ];

        let json = report_json(&graph, &propagation, &path, source, &warnings);
        let diagnostics = json["diagnostics"].as_array().expect("diagnostics array");
        assert_eq!(diagnostics[0]["chain"][0]["function"], "zed");
        assert_eq!(diagnostics[1]["chain"][0]["function"], "alpha");
        assert_eq!(diagnostics[0]["primary_location"]["line"], 2);
        assert_eq!(diagnostics[1]["primary_location"]["line"], 4);
        assert_eq!(
            json["warnings"],
            json!([
                {"message": "adapter warning: zeta"},
                {"message": "Syntax error in main.py", "file": "main.py"},
                {"message": "adapter warning: beta", "file": "main.py"}
            ])
        );
    }

    #[test]
    fn reporter_diagnostics_classify_property_and_dunder_edges() {
        let source = "async def dunder_handler():\n    str(obj)\nasync def property_handler():\n    obj.data\ndef __str__():\n    time.sleep(1)\ndef data():\n    time.sleep(1)\n";
        let path = Utf8PathBuf::from("/workspace/main.py");
        let mut graph = CallGraph::default();
        let dunder_handler = function(
            &mut graph,
            "dunder_handler",
            true,
            BlockingStatus::Unknown,
            loc(source, "dunder_handler"),
        );
        let property_handler = function(
            &mut graph,
            "property_handler",
            true,
            BlockingStatus::Unknown,
            loc(source, "property_handler"),
        );
        let dunder = function(
            &mut graph,
            "Thing.__str__",
            false,
            BlockingStatus::Unknown,
            loc(source, "__str__"),
        );
        let property = function(
            &mut graph,
            "Thing.data",
            false,
            BlockingStatus::Unknown,
            loc(source, "data"),
        );
        let sleep = external(&mut graph, "time.sleep");
        call(
            &mut graph,
            dunder_handler,
            dunder,
            EdgeKind::ImplicitDunder,
            loc(source, "str(obj)"),
        );
        call(
            &mut graph,
            property_handler,
            property,
            EdgeKind::PropertyAccess,
            loc(source, "obj.data"),
        );
        call(
            &mut graph,
            dunder,
            sleep,
            EdgeKind::DirectCall,
            loc(source, "time.sleep(1)"),
        );
        let second_sleep_offset = source.rfind("time.sleep(1)").expect("second sleep");
        call(
            &mut graph,
            property,
            sleep,
            EdgeKind::DirectCall,
            SourceLocation {
                start: second_sleep_offset.try_into().unwrap(),
                end: (second_sleep_offset + "time.sleep(1)".len())
                    .try_into()
                    .unwrap(),
            },
        );
        let propagation = propagate_blocking(&mut graph);

        let json = report_json(&graph, &propagation, &path, source, &[]);
        let diagnostics = json["diagnostics"].as_array().unwrap();
        let codes = diagnostics
            .iter()
            .map(|diagnostic| diagnostic["code"].as_str().unwrap())
            .collect::<Vec<_>>();
        let messages = diagnostics
            .iter()
            .map(|diagnostic| diagnostic["message"].as_str().unwrap())
            .collect::<Vec<_>>();

        assert_eq!(codes, vec!["STRATO004", "STRATO003"]);
        assert_eq!(
            messages,
            vec![
                "Async function 'dunder_handler' calls blocking dunder method 'Thing.__str__'",
                "Async function 'property_handler' accesses blocking property 'Thing.data'",
            ]
        );
    }

    #[test]
    fn reporter_diagnostics_unknown_or_sync_only_reasons_are_silent() {
        let source =
            "def sync_only():\n    time.sleep(1)\nasync def unknown_handler():\n    unresolved()\n";
        let path = Utf8PathBuf::from("/workspace/main.py");
        let mut graph = CallGraph::default();
        let sync_only = function(
            &mut graph,
            "sync_only",
            false,
            BlockingStatus::Unknown,
            loc(source, "sync_only"),
        );
        let sleep = external(&mut graph, "time.sleep");
        call(
            &mut graph,
            sync_only,
            sleep,
            EdgeKind::DirectCall,
            loc(source, "time.sleep(1)"),
        );
        let propagation = propagate_blocking(&mut graph);

        let json = report_json(&graph, &propagation, &path, source, &[]);

        assert_eq!(json["diagnostics"], json!([]));
        assert_eq!(json["warnings"], json!([]));
    }
}
