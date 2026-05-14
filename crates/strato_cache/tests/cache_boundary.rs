#![allow(missing_docs)]

use std::collections::BTreeMap;
use std::fs;

use camino::Utf8PathBuf;
use strato_cache::{
    CacheArtifact, CacheArtifactKind, CacheInvalidation, CacheManifest, CacheStorage,
    CachedFileKind, CachedFileResult, DecoratorSyntax, FileSyntax, StorageKey, SyntaxLocation,
};

#[test]
fn cache_manifest_accepts_only_strato_owned_artifact_kinds() {
    let kinds = CacheArtifactKind::all();

    assert_eq!(
        kinds,
        [
            CacheArtifactKind::Discovery,
            CacheArtifactKind::Syntax,
            CacheArtifactKind::Decorators,
        ]
    );
    assert!(!kinds.iter().any(|kind| kind.as_str().contains("semantic")));
    assert!(!kinds.iter().any(|kind| kind.as_str().contains("graph")));
    assert!(
        !kinds
            .iter()
            .any(|kind| kind.as_str().contains("diagnostic"))
    );
}

#[test]
fn cache_manifest_records_entries_in_deterministic_order() {
    let mut manifest = CacheManifest::new(1);
    manifest.record(StorageKey::new(CacheArtifactKind::Syntax, "bbb"), "hash-b");
    manifest.record(
        StorageKey::new(CacheArtifactKind::Discovery, "aaa"),
        "hash-a",
    );

    let keys = manifest.entries.keys().cloned().collect::<Vec<_>>();

    assert_eq!(
        keys,
        vec![
            StorageKey::new(CacheArtifactKind::Discovery, "aaa"),
            StorageKey::new(CacheArtifactKind::Syntax, "bbb"),
        ]
    );
}

#[test]
fn cached_file_result_contains_only_syntax_and_decorator_artifacts() {
    let result = CachedFileResult {
        content_hash: "0".repeat(64),
        syntax: FileSyntax {
            path: Utf8PathBuf::from("pkg/mod.py"),
            kind: CachedFileKind::Source,
            functions: vec![strato_cache::FunctionSyntax {
                name: "fn".to_string(),
                qualified_name: "pkg.mod.fn".to_string(),
                is_async: true,
                decorators: Vec::new(),
                location: SyntaxLocation { start: 0, end: 2 },
            }],
            classes: Vec::new(),
            imports: vec![strato_cache::ImportSyntax {
                module: Some("time".to_string()),
                name: None,
                alias: None,
                level: 0,
                location: SyntaxLocation { start: 0, end: 4 },
            }],
            call_sites: vec![strato_cache::CallSiteSyntax {
                enclosing_qualified_name: Some("pkg.mod.fn".to_string()),
                expression: "time.sleep".to_string(),
                location: SyntaxLocation { start: 10, end: 20 },
            }],
        },
        raw_decorators: vec![DecoratorSyntax {
            target: "pkg.mod.fn".to_string(),
            expression: "blocking".to_string(),
        }],
    };
    let artifact = CacheArtifact::Syntax(result.clone());

    assert_eq!(artifact.kind(), CacheArtifactKind::Syntax);
    assert_eq!(result.syntax.functions[0].qualified_name, "pkg.mod.fn");
    assert_eq!(result.syntax.call_sites[0].expression, "time.sleep");
}

#[test]
fn cache_manifest_tracks_version_and_config_compatibility() {
    let manifest = CacheManifest::with_metadata(1, "0.1.0", "config-a");

    assert!(manifest.is_compatible(1, "0.1.0", "config-a"));
    assert!(!manifest.is_compatible(1, "0.2.0", "config-a"));
    assert!(!manifest.is_compatible(1, "0.1.0", "config-b"));
}

#[test]
fn serialized_artifacts_do_not_contain_forbidden_boundary_names() {
    let artifact = CacheArtifact::Syntax(CachedFileResult {
        content_hash: "0".repeat(64),
        syntax: FileSyntax {
            path: Utf8PathBuf::from("pkg/mod.py"),
            kind: CachedFileKind::Source,
            functions: Vec::new(),
            classes: Vec::new(),
            imports: Vec::new(),
            call_sites: Vec::new(),
        },
        raw_decorators: Vec::new(),
    });
    let bytes = bincode::serialize(&artifact).expect("serialize allowed artifact");
    let text = String::from_utf8_lossy(&bytes);

    for forbidden in [
        "SemanticFacts",
        "SemanticTarget",
        "CallGraph",
        "Propagation",
        "Diagnostic",
        "Report",
        "Salsa",
        "ProjectDatabase",
    ] {
        assert!(
            !text.contains(forbidden),
            "serialized cache leaked {forbidden}"
        );
    }
}

#[test]
fn invalidation_detects_changed_content_hashes() {
    let mut old = CacheManifest::new(1);
    let mut current = BTreeMap::new();
    let key = StorageKey::new(CacheArtifactKind::Discovery, "file.py");
    old.record(key.clone(), "old");
    current.insert(key.clone(), "new".to_string());

    let invalidation = CacheInvalidation::between(&old, &current);

    assert_eq!(invalidation.changed, vec![key]);
}

#[test]
fn cache_storage_round_trips_allowed_artifacts() {
    let root = std::env::temp_dir().join(format!("strato-cache-test-{}", std::process::id()));
    let storage = CacheStorage::try_from(root.as_path()).expect("utf8 cache root");
    let key = StorageKey::new(CacheArtifactKind::Decorators, "pkg/mod.py");
    let artifact = CacheArtifact::Decorators(vec![DecoratorSyntax {
        target: "pkg.mod.fn".to_string(),
        expression: "blocking".to_string(),
    }]);

    storage.write(&key, &artifact).expect("write artifact");
    let restored = storage.read(&key).expect("read artifact");
    fs::remove_dir_all(root).expect("clean cache test dir");

    assert_eq!(restored, Some(artifact));
}
