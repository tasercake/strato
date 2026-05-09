#![allow(missing_docs)]

use std::collections::{BTreeMap, BTreeSet};

use camino::Utf8PathBuf;
use strato_core::{
    graph::{BlockingStatus, CallableKind},
    graph_builder::build_call_graph,
    types::{
        BlockingCategory, BlockingDatabase, BlockingEntry, CallableParam, ExecutorWrapperConfig,
        FileKind, FileSyntax, FunctionSyntax, SemanticCall, SemanticFacts, SemanticTarget,
        SourceLocation,
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

fn first_party(path: &Utf8PathBuf, name: &str) -> SemanticTarget {
    SemanticTarget::FirstPartyDefinition(format!("{path}:{name}"))
}

fn externals(names: &[&str]) -> SemanticTarget {
    SemanticTarget::ExternalQualifiedNames(
        names.iter().map(std::string::ToString::to_string).collect(),
    )
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
        blocking_modules: BTreeSet::from(["legacy.mod".to_string()]),
    }
}

#[test]
fn graph_builder_registers_callable_kinds_and_first_party_edges() {
    let path = Utf8PathBuf::from("/workspace/app.py");
    let syntax_by_path = syntax(
        &path,
        vec![
            function("handler", "handler", true, &[], 1),
            function("helper", "helper", false, &[], 10),
            function("method", "Worker.method", false, &[], 20),
            function("factory", "Worker.factory", false, &["classmethod"], 30),
            function("parse", "Worker.parse", false, &["staticmethod"], 40),
            function("data", "Worker.data", false, &["property"], 50),
            function("__str__", "Worker.__str__", false, &[], 60),
            function("<lambda>", "handler.<lambda>@70:1", false, &[], 70),
        ],
    );
    let semantic_facts = SemanticFacts {
        calls_by_path: BTreeMap::from([(
            path.clone(),
            vec![
                call(
                    "handler",
                    "helper()",
                    first_party(&path, "helper"),
                    false,
                    100,
                ),
                call(
                    "handler",
                    "worker.method()",
                    first_party(&path, "method"),
                    false,
                    110,
                ),
                call(
                    "handler",
                    "worker.data",
                    first_party(&path, "data"),
                    false,
                    120,
                ),
                call(
                    "handler",
                    "str(worker)",
                    first_party(&path, "__str__"),
                    false,
                    130,
                ),
                call(
                    "handler",
                    "super().method()",
                    first_party(&path, "method"),
                    false,
                    140,
                ),
            ],
        )]),
        warnings: Vec::new(),
    };

    let graph = build_call_graph(
        &syntax_by_path,
        &semantic_facts,
        &database(),
        &BTreeMap::new(),
    );

    let kinds = graph
        .nodes()
        .iter()
        .map(|node| (node.qualified_name.as_str(), node.kind))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(kinds["handler"], CallableKind::AsyncFunction);
    assert_eq!(kinds["helper"], CallableKind::Function);
    assert_eq!(kinds["Worker.method"], CallableKind::Method);
    assert_eq!(kinds["Worker.factory"], CallableKind::ClassMethod);
    assert_eq!(kinds["Worker.parse"], CallableKind::StaticMethod);
    assert_eq!(kinds["Worker.data"], CallableKind::Property);
    assert_eq!(kinds["Worker.__str__"], CallableKind::DunderMethod);
    assert_eq!(kinds["handler.<lambda>@70:1"], CallableKind::Lambda);

    let snapshots = graph.edge_snapshots();
    assert!(
        snapshots
            .iter()
            .any(|edge| edge
                == "handler -> helper [DirectCall executor=false via=- protected=false]")
    );
    assert!(
        snapshots.iter().any(|edge| edge
            == "handler -> Worker.method [MethodCall executor=false via=- protected=false]")
    );
    assert!(snapshots.iter().any(|edge| edge
        == "handler -> Worker.data [PropertyAccess executor=false via=- protected=false]"));
    assert!(snapshots.iter().any(|edge| edge
        == "handler -> Worker.__str__ [ImplicitDunder executor=false via=- protected=false]"));
    assert!(
        snapshots.iter().any(|edge| edge
            == "handler -> Worker.method [SuperCall executor=false via=- protected=false]")
    );
}

#[test]
fn graph_builder_materializes_only_known_external_phantoms_and_skips_unknowns() {
    let path = Utf8PathBuf::from("/workspace/app.py");
    let syntax_by_path = syntax(&path, vec![function("handler", "handler", true, &[], 1)]);
    let semantic_facts = SemanticFacts {
        calls_by_path: BTreeMap::from([(
            path,
            vec![
                call(
                    "handler",
                    "time.sleep(1)",
                    externals(&["time.sleep"]),
                    false,
                    10,
                ),
                call(
                    "handler",
                    "legacy.mod.slow()",
                    externals(&["legacy.mod.slow"]),
                    false,
                    20,
                ),
                call(
                    "handler",
                    "legacy.module_extra.slow()",
                    externals(&["legacy.module_extra.slow"]),
                    false,
                    30,
                ),
                call("handler", "unknown()", SemanticTarget::Unknown, false, 40),
            ],
        )]),
        warnings: Vec::new(),
    };

    let graph = build_call_graph(
        &syntax_by_path,
        &semantic_facts,
        &database(),
        &BTreeMap::new(),
    );

    let node_names = graph
        .nodes()
        .iter()
        .map(|node| node.qualified_name.as_str())
        .collect::<BTreeSet<_>>();
    assert!(node_names.contains("time.sleep"));
    assert!(node_names.contains("legacy.mod.slow"));
    assert!(!node_names.contains("legacy.module_extra.slow"));
    assert!(!node_names.contains("unknown"));
    assert_eq!(
        graph.node("time.sleep").map(|node| node.blocking_status),
        Some(BlockingStatus::KnownBlocking)
    );
    assert_eq!(
        graph
            .node("legacy.mod.slow")
            .map(|node| node.blocking_status),
        Some(BlockingStatus::KnownBlocking)
    );
    assert_eq!(graph.edges().len(), 2);
}

#[test]
fn graph_builder_skips_unknown_dynamic_call_property_and_dunder_without_artifacts() {
    let path = Utf8PathBuf::from("/workspace/app.py");
    let syntax_by_path = syntax(&path, vec![function("handler", "handler", true, &[], 1)]);
    let semantic_facts = SemanticFacts {
        calls_by_path: BTreeMap::from([(
            path,
            vec![
                call("handler", "funcs[0]()", SemanticTarget::Unknown, false, 10),
                call(
                    "handler",
                    "item.unknown_property",
                    SemanticTarget::Unknown,
                    false,
                    20,
                ),
                call(
                    "handler",
                    "left + right",
                    SemanticTarget::Unknown,
                    false,
                    30,
                ),
            ],
        )]),
        warnings: Vec::new(),
    };

    let graph = build_call_graph(
        &syntax_by_path,
        &semantic_facts,
        &database(),
        &BTreeMap::new(),
    );

    assert_eq!(graph.edge_snapshots(), Vec::<String>::new());
    assert_eq!(graph.nodes().len(), 1);
    assert!(graph.node("handler").is_some());
    assert!(graph.node("funcs[0]").is_none());
    assert!(graph.node("item.unknown_property").is_none());
    assert!(graph.node("left.__add__").is_none());
}

#[test]
fn graph_builder_marks_decorator_and_executor_wrapper_edges() {
    let path = Utf8PathBuf::from("/workspace/app.py");
    let syntax_by_path = syntax(
        &path,
        vec![
            function("decorator", "decorator", false, &[], 1),
            function("handler", "handler", true, &["decorator"], 10),
        ],
    );
    let semantic_facts = SemanticFacts {
        calls_by_path: BTreeMap::from([(
            path.clone(),
            vec![
                call(
                    "handler",
                    "decorator",
                    first_party(&path, "decorator"),
                    false,
                    11,
                ),
                call(
                    "handler",
                    "asyncio.to_thread(time.sleep, 1)",
                    externals(&["asyncio.to_thread"]),
                    true,
                    20,
                ),
                call(
                    "handler",
                    "mylib.offload(time.sleep, 1)",
                    externals(&["mylib.offload"]),
                    false,
                    30,
                ),
            ],
        )]),
        warnings: Vec::new(),
    };
    let wrappers = BTreeMap::from([(
        "mylib.offload".to_string(),
        ExecutorWrapperConfig {
            callable_param: CallableParam::Position(0),
        },
    )]);

    let graph = build_call_graph(&syntax_by_path, &semantic_facts, &database(), &wrappers);

    assert!(
        graph.edge_snapshots().iter().any(|edge| edge
            == "handler -> decorator [DecoratorCall executor=false via=- protected=false]")
    );
    assert!(graph.edge_snapshots().iter().any(|edge| edge == "handler -> time.sleep [DirectCall executor=true via=asyncio.to_thread protected=true]"));
    assert!(graph.edge_snapshots().iter().any(|edge| edge
        == "handler -> time.sleep [DirectCall executor=true via=mylib.offload protected=true]"));
}

#[test]
fn graph_builder_uses_parsed_decorators_to_classify_decorator_edges() {
    let path = Utf8PathBuf::from("/workspace/app.py");
    let syntax_by_path = syntax(
        &path,
        vec![
            function("decorator", "decorator", false, &[], 1),
            function("helper", "helper", false, &[], 5),
            function("handler", "handler", true, &["decorator"], 10),
        ],
    );
    let semantic_facts = SemanticFacts {
        calls_by_path: BTreeMap::from([(
            path.clone(),
            vec![
                call(
                    "handler",
                    "decorator",
                    first_party(&path, "decorator"),
                    false,
                    11,
                ),
                call("handler", "helper", first_party(&path, "helper"), false, 12),
            ],
        )]),
        warnings: Vec::new(),
    };

    let graph = build_call_graph(
        &syntax_by_path,
        &semantic_facts,
        &database(),
        &BTreeMap::new(),
    );

    assert!(graph.edge_snapshots().iter().any(|edge| {
        edge == "handler -> decorator [DecoratorCall executor=false via=- protected=false]"
    }));
    assert!(
        !graph
            .edge_snapshots()
            .iter()
            .any(|edge| edge.contains("handler -> helper"))
    );
}

#[test]
fn graph_builder_is_deterministic_across_repeated_builds() {
    let path = Utf8PathBuf::from("/workspace/app.py");
    let syntax_by_path = syntax(
        &path,
        vec![
            function("b", "b", false, &[], 20),
            function("a", "a", false, &[], 10),
        ],
    );
    let semantic_facts = SemanticFacts {
        calls_by_path: BTreeMap::from([(
            path.clone(),
            vec![
                call("a", "b()", first_party(&path, "b"), false, 30),
                call("a", "time.sleep(1)", externals(&["time.sleep"]), false, 40),
            ],
        )]),
        warnings: Vec::new(),
    };

    let first = build_call_graph(
        &syntax_by_path,
        &semantic_facts,
        &database(),
        &BTreeMap::new(),
    );
    let second = build_call_graph(
        &syntax_by_path,
        &semantic_facts,
        &database(),
        &BTreeMap::new(),
    );

    assert_eq!(first.node_snapshots(), second.node_snapshots());
    assert_eq!(first.edge_snapshots(), second.edge_snapshots());
}
