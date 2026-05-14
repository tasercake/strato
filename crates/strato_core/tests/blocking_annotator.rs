#![allow(missing_docs)]

use std::collections::{BTreeMap, BTreeSet};

use camino::Utf8PathBuf;
use strato_core::{
    database::effective_database,
    graph::BlockingStatus,
    graph_builder::build_call_graph,
    types::{
        BlockingCategory, BlockingConfig, BlockingEntry, FileKind, FileSyntax, FunctionSyntax,
        SemanticCall, SemanticFacts, SemanticTarget, SourceLocation,
    },
};

fn loc(start: u32) -> SourceLocation {
    SourceLocation {
        start,
        end: start + 1,
    }
}

fn function(
    name: &str,
    qualified_name: &str,
    is_async: bool,
    decorators: &[&str],
    start: u32,
) -> FunctionSyntax {
    FunctionSyntax {
        name: name.to_string(),
        qualified_name: qualified_name.to_string(),
        is_async,
        decorators: decorators
            .iter()
            .map(std::string::ToString::to_string)
            .collect(),
        location: loc(start),
    }
}

fn syntax(path: &Utf8PathBuf, functions: Vec<FunctionSyntax>) -> BTreeMap<Utf8PathBuf, FileSyntax> {
    BTreeMap::from([(
        path.clone(),
        FileSyntax {
            path: path.clone(),
            kind: FileKind::Source,
            functions,
            classes: Vec::new(),
            imports: Vec::new(),
            call_sites: Vec::new(),
        },
    )])
}

fn first_party(path: &Utf8PathBuf, name: &str) -> SemanticTarget {
    SemanticTarget::FirstPartyDefinition(format!("{path}:{name}"))
}

fn externals(names: &[&str]) -> SemanticTarget {
    SemanticTarget::ExternalQualifiedNames(
        names.iter().map(std::string::ToString::to_string).collect(),
    )
}

fn call(
    enclosing: &str,
    expression: &str,
    target: SemanticTarget,
    in_executor: bool,
    start: u32,
) -> SemanticCall {
    SemanticCall {
        enclosing_qualified_name: Some(enclosing.to_string()),
        expression: expression.to_string(),
        target,
        is_event_loop_run_in_executor: in_executor,
        location: loc(start),
    }
}

fn semantic_facts(path: &Utf8PathBuf, calls: Vec<SemanticCall>) -> SemanticFacts {
    SemanticFacts {
        calls_by_path: BTreeMap::from([(path.clone(), calls)]),
        warnings: Vec::new(),
    }
}

#[test]
fn blocking_database_contains_documented_builtins_and_aliases() {
    let database = effective_database(&BlockingConfig::default());

    assert_eq!(database.entries.len(), 61);
    assert_eq!(
        database.entries["requests.get"].category,
        BlockingCategory::NetworkIo
    );
    assert_eq!(
        database.entries["builtins.input"].category,
        BlockingCategory::UserInput
    );
    assert_eq!(
        database.canonical_name("_socket.socket.connect"),
        Some("socket.socket.connect")
    );
}

#[test]
fn blocking_config_add_remove_and_modules_merge_deterministically() {
    let config = BlockingConfig {
        add: vec![BlockingEntry {
            name: "project.slow".to_string(),
            help: "Use project.async_slow".to_string(),
            category: BlockingCategory::Other,
        }],
        remove: BTreeSet::from(["time.sleep".to_string()]),
        blocking_modules: BTreeSet::from(["legacy.mod".to_string()]),
    };

    let database = effective_database(&config);

    assert!(!database.entries.contains_key("time.sleep"));
    assert_eq!(
        database.entries["project.slow"].help,
        "Use project.async_slow"
    );
    assert!(database.matches_blocking_target("legacy.mod.worker"));
    assert!(!database.matches_blocking_target("legacy.module_extra.worker"));
}

#[test]
fn annotator_applies_blocking_and_non_blocking_precedence() {
    let path = Utf8PathBuf::from("/workspace/app.py");
    let syntax_by_path = syntax(
        &path,
        vec![
            function("custom_slow", "custom_slow", false, &["blocking"], 1),
            function(
                "conflicting",
                "conflicting",
                false,
                &["blocking", "non_blocking"],
                10,
            ),
        ],
    );
    let semantic_facts = semantic_facts(
        &path,
        vec![
            call(
                "custom_slow",
                "blocking",
                externals(&["strato.blocking"]),
                false,
                2,
            ),
            call(
                "conflicting",
                "blocking",
                externals(&["strato.blocking"]),
                false,
                11,
            ),
            call(
                "conflicting",
                "non_blocking",
                externals(&["strato.non_blocking"]),
                false,
                12,
            ),
        ],
    );

    let graph = build_call_graph(
        &syntax_by_path,
        &semantic_facts,
        &effective_database(&BlockingConfig::default()),
        &BTreeMap::new(),
    );

    assert_eq!(
        graph.node("custom_slow").map(|node| node.blocking_status),
        Some(BlockingStatus::KnownBlocking)
    );
    assert_eq!(
        graph.node("conflicting").map(|node| node.blocking_status),
        Some(BlockingStatus::KnownNonBlocking)
    );
}

#[test]
fn annotator_keeps_known_non_blocking_local_only() {
    let path = Utf8PathBuf::from("/workspace/app.py");
    let syntax_by_path = syntax(
        &path,
        vec![function(
            "actually_safe",
            "actually_safe",
            false,
            &["non_blocking"],
            1,
        )],
    );
    let semantic_facts = semantic_facts(
        &path,
        vec![
            call(
                "actually_safe",
                "non_blocking",
                externals(&["strato.non_blocking"]),
                false,
                2,
            ),
            call(
                "actually_safe",
                "time.sleep(1)",
                externals(&["time.sleep"]),
                false,
                3,
            ),
        ],
    );

    let graph = build_call_graph(
        &syntax_by_path,
        &semantic_facts,
        &effective_database(&BlockingConfig::default()),
        &BTreeMap::new(),
    );

    assert_eq!(
        graph.node("actually_safe").map(|node| node.blocking_status),
        Some(BlockingStatus::KnownNonBlocking)
    );
    assert_eq!(
        graph.node("time.sleep").map(|node| node.blocking_status),
        Some(BlockingStatus::KnownBlocking)
    );
}

#[test]
fn annotator_turns_resolved_unblocker_into_protected_executor_edge() {
    let path = Utf8PathBuf::from("/workspace/app.py");
    let syntax_by_path = syntax(
        &path,
        vec![
            function(
                "custom_offload",
                "custom_offload",
                false,
                &["unblocker(callable_param=\"target\")"],
                1,
            ),
            function("handler", "handler", true, &[], 10),
        ],
    );
    let semantic_facts = semantic_facts(
        &path,
        vec![
            call(
                "custom_offload",
                "unblocker(callable_param=\"target\")",
                externals(&["strato.unblocker"]),
                false,
                2,
            ),
            call(
                "handler",
                "custom_offload(target=time.sleep)",
                first_party(&path, "custom_offload"),
                false,
                11,
            ),
        ],
    );

    let graph = build_call_graph(
        &syntax_by_path,
        &semantic_facts,
        &effective_database(&BlockingConfig::default()),
        &BTreeMap::new(),
    );

    assert!(graph.edge_snapshots().iter().any(|edge| edge
        == "handler -> time.sleep [DirectCall executor=true via=custom_offload protected=true]"));
}

#[test]
fn event_loop_run_in_executor_marks_callable_argument_edge_protected() {
    let path = Utf8PathBuf::from("/workspace/app.py");
    let syntax_by_path = syntax(&path, vec![function("handler", "handler", true, &[], 1)]);
    let semantic_facts = semantic_facts(
        &path,
        vec![call(
            "handler",
            "loop.run_in_executor(None, time.sleep, 1)",
            externals(&["asyncio.BaseEventLoop.run_in_executor"]),
            true,
            2,
        )],
    );

    let graph = build_call_graph(
        &syntax_by_path,
        &semantic_facts,
        &effective_database(&BlockingConfig::default()),
        &BTreeMap::new(),
    );

    assert!(graph.edge_snapshots().iter().any(|edge| edge
        == "handler -> time.sleep [DirectCall executor=true via=loop.run_in_executor protected=true]"));
}

#[test]
fn blocking_external_nodes_require_resolved_aliases_not_lookalike_text() {
    let path = Utf8PathBuf::from("/workspace/app.py");
    let syntax_by_path = syntax(&path, vec![function("handler", "handler", true, &[], 1)]);
    let semantic_facts = semantic_facts(
        &path,
        vec![
            call(
                "handler",
                "time.sleep(1)",
                SemanticTarget::Unknown,
                false,
                10,
            ),
            call(
                "handler",
                "sock.connect(addr)",
                externals(&["_socket.socket.connect"]),
                false,
                20,
            ),
        ],
    );

    let graph = build_call_graph(
        &syntax_by_path,
        &semantic_facts,
        &effective_database(&BlockingConfig::default()),
        &BTreeMap::new(),
    );

    assert!(graph.node("time.sleep").is_none());
    assert_eq!(
        graph
            .node("_socket.socket.connect")
            .map(|node| node.blocking_status),
        Some(BlockingStatus::KnownBlocking)
    );
}
