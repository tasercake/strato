#![allow(missing_docs)]

use camino::Utf8PathBuf;
use strato_core::{
    AnalysisOptions, ConfigSource, analyze_path_with_options,
    discovery::discover_project,
    parser::parse_project,
    semantics::analyze_semantics,
    types::{AnalysisWarning, FileKind, SemanticTarget},
};

fn fixture_root(name: &str) -> Utf8PathBuf {
    Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("../../tests/fixtures/{name}"))
}

#[test]
fn parser_reports_syntax_errors_as_warnings_and_continues() {
    let root = fixture_root("a25_syntax_warnings");
    let manifest = discover_project(root.as_std_path(), &AnalysisOptions::defaults())
        .expect("discover syntax fixture");

    let parsed = parse_project(&manifest).expect("parse through adapter");

    assert!(parsed.warnings.iter().any(|warning| matches!(
        warning,
        AnalysisWarning::SyntaxError { path, error }
            if path.ends_with("invalid.py") && !error.is_empty()
    )));
    assert!(parsed.syntax_by_path.iter().any(|(path, syntax)| {
        path.ends_with("valid.py")
            && syntax
                .functions
                .iter()
                .any(|function| function.qualified_name == "handler")
    }));
}

#[test]
fn parser_extracts_owned_function_class_import_and_decorator_facts() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let root = Utf8PathBuf::from_path_buf(tempdir.path().to_path_buf()).expect("utf8 tempdir");
    std::fs::write(
        root.join("sample.py"),
        r"import os as operating
from .helpers import value as helper_value

@registry.item
class Worker(Base):
    @classmethod
    async def run(cls):
        return helper_value()
",
    )
    .expect("write sample");
    let manifest = discover_project(root.as_std_path(), &AnalysisOptions::defaults())
        .expect("discover temp project");

    let parsed = parse_project(&manifest).expect("parse through adapter");
    let syntax = parsed
        .syntax_by_path
        .values()
        .find(|syntax| syntax.path.ends_with("sample.py"))
        .expect("sample syntax");

    assert!(syntax.imports.iter().any(|import| {
        import.module.as_deref() == Some("os") && import.alias.as_deref() == Some("operating")
    }));
    assert!(syntax.imports.iter().any(|import| {
        import.module.as_deref() == Some("helpers")
            && import.name.as_deref() == Some("value")
            && import.alias.as_deref() == Some("helper_value")
            && import.level == 1
    }));
    assert!(syntax.classes.iter().any(|class| {
        class.qualified_name == "Worker"
            && class.bases == vec!["Base".to_string()]
            && class.decorators == vec!["registry.item".to_string()]
    }));
    assert!(syntax.functions.iter().any(|function| {
        function.qualified_name == "Worker.run"
            && function.is_async
            && function.decorators == vec!["classmethod".to_string()]
    }));
}

#[test]
fn stubs_contribute_declarations_and_decorators_but_not_body_calls() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let root = Utf8PathBuf::from_path_buf(tempdir.path().to_path_buf()).expect("utf8 tempdir");
    std::fs::create_dir(root.join("stubs")).expect("create stubs");
    std::fs::write(
        root.join("pyproject.toml"),
        "[tool.strato]\nstub_paths = [\"stubs\"]\n",
    )
    .expect("write config");
    std::fs::write(
        root.join("main.py"),
        "from thirdparty import slow\n\ndef handler():\n    slow()\n",
    )
    .expect("write source");
    std::fs::write(
        root.join("stubs/thirdparty.pyi"),
        "from strato import blocking\n\n@blocking\ndef slow() -> None:\n    forbidden_body_call()\n",
    )
    .expect("write stub");
    let manifest = discover_project(
        root.as_std_path(),
        &AnalysisOptions {
            config: ConfigSource::Path(root.join("pyproject.toml").into_std_path_buf()),
            ..AnalysisOptions::defaults()
        },
    )
    .expect("discover temp project");

    let parsed = parse_project(&manifest).expect("parse through adapter");
    let stub = parsed
        .syntax_by_path
        .values()
        .find(|syntax| syntax.path.ends_with("thirdparty.pyi"))
        .expect("stub syntax");
    let semantics = analyze_semantics(&parsed).expect("semantic facts through adapter");

    assert_eq!(stub.kind, FileKind::Stub);
    assert!(stub.functions.iter().any(|function| {
        function.qualified_name == "slow" && function.decorators == vec!["blocking".to_string()]
    }));
    assert!(
        stub.call_sites.is_empty(),
        "stub bodies must not produce call-site syntax"
    );
    assert!(
        semantics
            .calls_by_path
            .get(&stub.path)
            .is_none_or(Vec::is_empty)
    );
}

#[test]
fn semantics_keep_normalized_call_targets_in_memory() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let root = Utf8PathBuf::from_path_buf(tempdir.path().to_path_buf()).expect("utf8 tempdir");
    std::fs::write(
        root.join("sample.py"),
        "def blocking():\n    pass\n\ndef handler():\n    blocking()\n",
    )
    .expect("write sample");
    let manifest = discover_project(root.as_std_path(), &AnalysisOptions::defaults())
        .expect("discover temp project");
    let parsed = parse_project(&manifest).expect("parse through adapter");

    let semantics = analyze_semantics(&parsed).expect("semantic facts through adapter");

    let calls = semantics
        .calls_by_path
        .values()
        .next()
        .expect("semantic calls");
    assert!(calls.iter().any(|call| {
        call.expression == "blocking()"
            && matches!(call.target, SemanticTarget::FirstPartyDefinition(_))
    }));
}

#[test]
fn analysis_warns_for_general_unresolved_import_without_inferring_blocking() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let root = Utf8PathBuf::from_path_buf(tempdir.path().to_path_buf()).expect("utf8 tempdir");
    std::fs::write(
        root.join("main.py"),
        r"from missing_vendor import slow

async def handler():
    slow()
",
    )
    .expect("write sample");

    let output = analyze_path_with_options(root.as_std_path(), &AnalysisOptions::defaults())
        .expect("analyze temp project");

    assert_eq!(output.exit_code, 0);
    assert_eq!(output.json["diagnostics"].as_array().unwrap().len(), 0);
    assert_eq!(
        output.json["warnings"][0]["message"],
        "Unresolvable import: missing_vendor"
    );
}

#[test]
fn analysis_warns_for_to_thread_when_python_version_lacks_api_without_raw_prefix_match() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let root = Utf8PathBuf::from_path_buf(tempdir.path().to_path_buf()).expect("utf8 tempdir");
    std::fs::write(
        root.join("main.py"),
        r"import asyncio
import time

async def handler():
    await (asyncio.to_thread)(time.sleep, 1)
",
    )
    .expect("write sample");
    let options = AnalysisOptions {
        python_version: Some("3.8".to_string()),
        ..AnalysisOptions::defaults()
    };

    let output =
        analyze_path_with_options(root.as_std_path(), &options).expect("analyze temp project");

    assert_eq!(output.exit_code, 0);
    assert!(output.json["warnings"].as_array().unwrap().iter().any(|warning| {
        warning["message"]
            == "asyncio.to_thread is unavailable for configured python_version 3.8; executor protection was not applied"
    }));
}
