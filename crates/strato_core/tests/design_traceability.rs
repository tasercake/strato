#![allow(missing_docs)]

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TraceabilityEntry {
    requirement: &'static str,
    docs: &'static [&'static str],
    owning_crate_module: &'static str,
    implementation_task: &'static str,
    fixture_or_test_coverage: &'static str,
    verification_command: &'static str,
}

const DESIGN_TRACEABILITY_MATRIX: &[TraceabilityEntry] = &[
    TraceabilityEntry {
        requirement: "Discovery phase loads config, source manifests, hashes, and blocking database inputs",
        docs: &[
            "docs/book/src/analysis-pipeline.md:3",
            "docs/book/src/analysis-pipeline.md:6",
        ],
        owning_crate_module: "strato_core::discovery",
        implementation_task: "Task 6",
        fixture_or_test_coverage: "crates/strato_core/tests/acceptance.rs:56 validates A1-A51 fixture truth",
        verification_command: "cargo test -p strato_core acceptance_fixtures_are_well_formed",
    },
    TraceabilityEntry {
        requirement: "Parse phase extracts Strato-owned syntax from vendored Ruff parsed modules",
        docs: &[
            "docs/book/src/analysis-pipeline.md:3",
            "docs/book/src/analysis-pipeline.md:6",
        ],
        owning_crate_module: "strato_core::parser",
        implementation_task: "Task 7",
        fixture_or_test_coverage: "A1-A51 acceptance fixtures plus parse/extraction unit coverage",
        verification_command: "cargo test -p strato_core parser semantics -- --nocapture",
    },
    TraceabilityEntry {
        requirement: "Semantics phase uses strato_ty_adapter facade and Strato-owned target types",
        docs: &[
            "docs/book/src/analysis-pipeline.md:3",
            "docs/book/src/analysis-pipeline.md:6",
            "docs/book/src/analysis-pipeline.md:71",
            "docs/book/src/analysis-pipeline.md:72",
            "docs/book/src/analysis-pipeline.md:96",
        ],
        owning_crate_module: "strato_core::semantics",
        implementation_task: "Task 7",
        fixture_or_test_coverage: "Facade compile/API tests and A1-A51 acceptance fixtures",
        verification_command: "cargo test -p strato_core parser semantics -- --nocapture",
    },
    TraceabilityEntry {
        requirement: "Build phase consumes facade facts and skips Unknown targets without creating guessed edges",
        docs: &[
            "docs/book/src/analysis-pipeline.md:3",
            "docs/book/src/analysis-pipeline.md:6",
            "docs/book/src/analysis-pipeline.md:118",
            "docs/book/src/call-graph-type-resolution.md:188",
            "docs/book/src/call-graph-type-resolution.md:240",
        ],
        owning_crate_module: "strato_core::graph / strato_core::graph_builder",
        implementation_task: "Task 8",
        fixture_or_test_coverage: "Unknown-target precision fixtures and graph-builder unit tests",
        verification_command: "cargo test -p strato_core graph_builder -- --nocapture",
    },
    TraceabilityEntry {
        requirement: "Annotate phase applies blocking DB, user config, and annotations with documented precedence",
        docs: &[
            "docs/book/src/analysis-pipeline.md:3",
            "docs/book/src/analysis-pipeline.md:6",
            "docs/book/src/blocking-function-database-annotations.md:36",
            "docs/book/src/blocking-function-database-annotations.md:70",
            "docs/book/src/blocking-function-database-annotations.md:273",
        ],
        owning_crate_module: "strato_core::annotator",
        implementation_task: "Task 9",
        fixture_or_test_coverage: "Blocking database/config/annotation fixtures in A1-A51",
        verification_command: "cargo test -p strato_core blocking annotator -- --nocapture",
    },
    TraceabilityEntry {
        requirement: "Propagate phase uses Tarjan SCC decomposition and topological blocking propagation",
        docs: &[
            "docs/book/src/analysis-pipeline.md:3",
            "docs/book/src/analysis-pipeline.md:6",
            "docs/book/src/blocking-propagation.md:3",
            "docs/book/src/blocking-propagation.md:23",
        ],
        owning_crate_module: "strato_core::propagator",
        implementation_task: "Task 10",
        fixture_or_test_coverage: "Recursive and transitive blocking fixtures in A1-A51",
        verification_command: "cargo test -p strato_core propagator tarjan -- --nocapture",
    },
    TraceabilityEntry {
        requirement: "Report phase emits deterministic output schema and sorted diagnostics",
        docs: &[
            "docs/book/src/analysis-pipeline.md:3",
            "docs/book/src/analysis-pipeline.md:6",
            "docs/book/src/appendix-c-output-format-specifications.md:92",
            "docs/book/src/error-reporting-diagnostics.md:383",
        ],
        owning_crate_module: "strato_core::reporter",
        implementation_task: "Task 11",
        fixture_or_test_coverage: "Full-JSON A1-A51 fixture comparisons",
        verification_command: "cargo test -p strato_core reporter diagnostics -- --nocapture",
    },
    TraceabilityEntry {
        requirement: "Adapter boundary keeps Ruff/ty access behind strato_ty_adapter and required patch APIs",
        docs: &[
            "docs/book/src/analysis-pipeline.md:71",
            "docs/book/src/analysis-pipeline.md:72",
            "docs/book/src/analysis-pipeline.md:96",
            "docs/book/src/analysis-pipeline.md:118",
        ],
        owning_crate_module: "strato_ty_adapter",
        implementation_task: "Task 5",
        fixture_or_test_coverage: "Boundary compile tests plus facade API assertions",
        verification_command: "cargo test -p strato_ty_adapter",
    },
    TraceabilityEntry {
        requirement: "cache boundary stores only Strato-owned discovery/syntax artifacts",
        docs: &[
            "docs/book/src/supporting-systems.md:119",
            "docs/book/src/supporting-systems.md:135",
            "docs/book/src/supporting-systems.md:179",
        ],
        owning_crate_module: "strato_cache",
        implementation_task: "Task 6 / Task 13",
        fixture_or_test_coverage: "Cache integration tests and A1-A51 cached/uncached parity",
        verification_command: "cargo test -p strato_cache && cargo test -p strato_core cache -- --nocapture",
    },
    TraceabilityEntry {
        requirement: "Error policy treats config/no-source failures as fatal and recoverable file failures as warnings",
        docs: &[
            "docs/book/src/supporting-systems.md:59",
            "docs/book/src/supporting-systems.md:68",
        ],
        owning_crate_module: "strato_core::discovery / strato_core::reporter / strato_cli",
        implementation_task: "Task 6 / Task 11 / Task 12",
        fixture_or_test_coverage: "Fatal error and warning fixtures in A1-A51",
        verification_command: "cargo test -p strato_core discovery config cache -- --nocapture; cargo test -p strato_core reporter diagnostics -- --nocapture; cargo test -p strato_cli",
    },
    TraceabilityEntry {
        requirement: "CLI exposes strato check options and exit-code behavior",
        docs: &[
            "docs/book/src/supporting-systems.md:7",
            "docs/book/src/supporting-systems.md:20",
        ],
        owning_crate_module: "strato_cli",
        implementation_task: "Task 12",
        fixture_or_test_coverage: "CLI integration tests over A1-A51 fixtures",
        verification_command: "cargo test -p strato_cli",
    },
    TraceabilityEntry {
        requirement: "Python package provides Strato annotations and PEP 561 marker",
        docs: &["docs/book/src/appendix-e-repository-structure-implementation-plan.md:76"],
        owning_crate_module: "python/strato",
        implementation_task: "Task 14",
        fixture_or_test_coverage: "Python package smoke tests and annotation fixtures in A1-A51",
        verification_command: "python -c \"from strato import blocking, non_blocking, unblocker; f=lambda: 1; assert blocking(f) is f; assert non_blocking(f) is f; assert unblocker(f) is f\"",
    },
];

fn design_traceability_matrix() -> Vec<TraceabilityEntry> {
    DESIGN_TRACEABILITY_MATRIX.to_vec()
}

fn compliance_errors(entries: &[TraceabilityEntry]) -> Vec<String> {
    let mut errors = Vec::new();

    for required in [
        "Discovery",
        "Parse",
        "Semantics",
        "Build",
        "Annotate",
        "Propagate",
        "Report",
        "strato_ty_adapter",
        "Unknown targets",
        "Tarjan",
        "cache boundary",
        "Error policy",
        "CLI",
        "Python package",
    ] {
        if !entries
            .iter()
            .any(|entry| entry.requirement.contains(required))
        {
            errors.push(format!("missing {required}"));
        }
    }

    for required_doc in [
        "docs/book/src/analysis-pipeline.md:3",
        "docs/book/src/analysis-pipeline.md:6",
        "docs/book/src/analysis-pipeline.md:71",
        "docs/book/src/analysis-pipeline.md:72",
        "docs/book/src/analysis-pipeline.md:96",
        "docs/book/src/analysis-pipeline.md:118",
        "docs/book/src/call-graph-type-resolution.md:188",
        "docs/book/src/call-graph-type-resolution.md:240",
        "docs/book/src/blocking-propagation.md:3",
        "docs/book/src/blocking-propagation.md:23",
        "docs/book/src/supporting-systems.md:119",
        "docs/book/src/supporting-systems.md:135",
        "docs/book/src/supporting-systems.md:179",
        "docs/book/src/supporting-systems.md:59",
        "docs/book/src/supporting-systems.md:68",
        "docs/book/src/appendix-c-output-format-specifications.md:92",
        "docs/book/src/error-reporting-diagnostics.md:383",
        "docs/book/src/blocking-function-database-annotations.md:36",
        "docs/book/src/blocking-function-database-annotations.md:70",
        "docs/book/src/blocking-function-database-annotations.md:273",
        "docs/book/src/supporting-systems.md:7",
        "docs/book/src/supporting-systems.md:20",
        "docs/book/src/appendix-e-repository-structure-implementation-plan.md:76",
        "crates/strato_core/tests/acceptance.rs:56",
    ] {
        if !entries.iter().any(|entry| {
            entry.docs.contains(&required_doc)
                || entry.fixture_or_test_coverage.contains(required_doc)
        }) {
            errors.push(format!("missing doc reference {required_doc}"));
        }
    }

    for (index, entry) in entries.iter().enumerate() {
        if entry.docs.is_empty() {
            errors.push(format!("entry {index} has no docs"));
        }
        if entry.owning_crate_module.is_empty() {
            errors.push(format!("entry {index} has no owner"));
        }
        if entry.implementation_task.is_empty() {
            errors.push(format!("entry {index} has no implementation task"));
        }
        if entry.fixture_or_test_coverage.is_empty() {
            errors.push(format!("entry {index} has no coverage"));
        }
        if entry.verification_command.is_empty() {
            errors.push(format!("entry {index} has no verification command"));
        }
    }

    errors
}

#[test]
fn design_traceability_covers_mandatory_design_items() {
    let entries = design_traceability_matrix();
    let errors = compliance_errors(&entries);

    for entry in &entries {
        println!(
            "{} => {} | {} | {} | {} | {}",
            entry.requirement,
            entry.docs.join(", "),
            entry.owning_crate_module,
            entry.implementation_task,
            entry.fixture_or_test_coverage,
            entry.verification_command
        );
    }

    assert!(errors.is_empty(), "{}", errors.join("\n"));
}

#[test]
fn design_traceability_rejects_incomplete_checklist() {
    let incomplete = [TraceabilityEntry {
        requirement: "Discovery phase only",
        docs: &["docs/book/src/analysis-pipeline.md:3"],
        owning_crate_module: "strato_core::discovery",
        implementation_task: "Task 7",
        fixture_or_test_coverage: "A1-A51 acceptance fixtures",
        verification_command: "cargo test -p strato_core acceptance_fixtures_are_well_formed",
    }];

    let errors = compliance_errors(&incomplete);

    println!("incomplete checklist errors: {}", errors.join(" | "));

    assert!(
        errors
            .iter()
            .any(|error| error == "missing strato_ty_adapter")
    );
    assert!(errors.iter().any(|error| error == "missing Tarjan"));
    assert!(errors.iter().any(|error| error == "missing cache boundary"));
}
