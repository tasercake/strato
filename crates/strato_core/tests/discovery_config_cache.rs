#![allow(missing_docs)]

use camino::Utf8PathBuf;
use strato_core::{
    AnalysisOptions, ConfigSource,
    discovery::{DiscoverError, discover_project},
    types::{CallableParam, FileKind, InterventionStrategy},
};

fn fixture_root(name: &str) -> Utf8PathBuf {
    let path =
        Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("../../tests/fixtures/{name}"));
    Utf8PathBuf::from_path_buf(std::fs::canonicalize(path).expect("fixture path exists"))
        .expect("fixture path is utf-8")
}

#[test]
fn config_loads_fixture_relative_executor_wrapper() {
    let root = fixture_root("a15_executor_wrapper_config");

    let manifest = discover_project(
        root.as_std_path(),
        &AnalysisOptions {
            config: ConfigSource::Path(root.join("pyproject.toml").into_std_path_buf()),
            ..AnalysisOptions::defaults()
        },
    )
    .expect("discover fixture");

    assert_eq!(
        manifest
            .config
            .executor_wrappers
            .get("mylib.offload")
            .map(|wrapper| &wrapper.callable_param),
        Some(&CallableParam::Position(0))
    );
    assert!(manifest.files.iter().any(|file| {
        file.path.ends_with("main.py") && file.kind == FileKind::Source && file.is_first_party
    }));
    assert!(manifest.files.iter().any(|file| {
        file.path.ends_with("mylib.py") && file.kind == FileKind::Source && file.is_first_party
    }));
    assert!(
        manifest
            .files
            .iter()
            .all(|file| file.content_hash.len() == 64)
    );
}

#[test]
fn discovery_applies_src_roots_excludes_and_deterministic_ordering() {
    let root = fixture_root("a43_src_roots_exclude_boundaries");

    let manifest = discover_project(
        root.as_std_path(),
        &AnalysisOptions {
            config: ConfigSource::Path(root.join("pyproject.toml").into_std_path_buf()),
            ..AnalysisOptions::defaults()
        },
    )
    .expect("discover fixture");
    let paths = manifest
        .files
        .iter()
        .map(|file| file.path.strip_prefix(&root).unwrap().to_string())
        .collect::<Vec<_>>();

    assert_eq!(paths, vec!["src/app.py"]);
    assert_eq!(manifest.source_roots, vec![root.join("src")]);
}

#[test]
fn discovery_classifies_configured_stub_paths_as_third_party_stubs() {
    let root = fixture_root("a26_stub_annotation");

    let manifest = discover_project(
        root.as_std_path(),
        &AnalysisOptions {
            config: ConfigSource::Path(root.join("pyproject.toml").into_std_path_buf()),
            ..AnalysisOptions::defaults()
        },
    )
    .expect("discover fixture");

    assert!(manifest.files.iter().any(|file| {
        file.path.ends_with("stubs/thirdparty.pyi")
            && file.kind == FileKind::Stub
            && !file.is_first_party
    }));
}

#[test]
fn config_validation_reports_fatal_config_errors() {
    let root = fixture_root("a41_invalid_config_fails");

    let error = discover_project(
        root.as_std_path(),
        &AnalysisOptions {
            config: ConfigSource::Path(root.join("pyproject.toml").into_std_path_buf()),
            ..AnalysisOptions::defaults()
        },
    )
    .expect_err("invalid config must fail");

    assert!(matches!(error, DiscoverError::Config { .. }));
    assert!(error.to_string().contains("Invalid strategy"));
}

#[test]
fn excluded_sources_are_fatal_no_analyzable_source_files() {
    let root = fixture_root("a44_no_analyzable_source_files");

    let error = discover_project(
        root.as_std_path(),
        &AnalysisOptions {
            config: ConfigSource::Path(root.join("pyproject.toml").into_std_path_buf()),
            ..AnalysisOptions::defaults()
        },
    )
    .expect_err("excluded files must leave no analyzable sources");

    assert!(matches!(error, DiscoverError::NoAnalyzableSourceFiles));
}

#[test]
fn config_merges_blocking_sections_deterministically() {
    let root = fixture_root("a38_blocking_config_add_configured");

    let manifest = discover_project(
        root.as_std_path(),
        &AnalysisOptions {
            config: ConfigSource::Path(root.join("pyproject.toml").into_std_path_buf()),
            ..AnalysisOptions::defaults()
        },
    )
    .expect("discover fixture");

    let entry = manifest
        .blocking_database
        .entries
        .get("legacy.slow")
        .expect("configured blocking function");
    assert_eq!(entry.category.as_str(), "other");
    assert_eq!(
        manifest.config.intervention_strategy,
        InterventionStrategy::FirstPartyDeepest
    );
}
