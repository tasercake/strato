#![allow(missing_docs)]

use std::fs;

use camino::{Utf8Path, Utf8PathBuf};
use strato_cache::{CacheArtifact, CacheArtifactKind, CacheStorage, StorageKey, sha256_hex};
use strato_core::{AnalysisOptions, ConfigSource};
use tempfile::TempDir;

fn write(path: &Utf8Path, text: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent");
    }
    fs::write(path, text).expect("write file");
}

fn project() -> (TempDir, Utf8PathBuf) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).expect("utf8 tempdir");
    write(
        &root.join("main.py"),
        "import time\n\nasync def handler():\n    time.sleep(1)\n",
    );
    (temp, root)
}

fn analyze(root: &Utf8Path, cache_enabled: Option<bool>, clear_cache: bool) -> serde_json::Value {
    strato_core::analyze_path_with_options(
        root.as_std_path(),
        &AnalysisOptions {
            cache_enabled,
            clear_cache,
            ..AnalysisOptions::defaults()
        },
    )
    .expect("analysis succeeds")
    .json
}

fn storage(root: &Utf8Path) -> CacheStorage {
    CacheStorage::new(root.join(".strato_cache"))
}

fn syntax_key(root: &Utf8Path, file_name: &str) -> StorageKey {
    storage(root)
        .read_manifest()
        .expect("read manifest")
        .expect("manifest exists")
        .entries
        .keys()
        .find(|key| key.kind == CacheArtifactKind::Syntax && key.key.ends_with(file_name))
        .cloned()
        .expect("syntax key exists")
}

#[test]
fn fresh_and_cached_runs_produce_identical_diagnostics_and_warnings() {
    let (_temp, root) = project();

    let fresh = analyze(&root, Some(false), true);
    let cached_seed = analyze(&root, Some(true), true);
    let cached_hit = analyze(&root, Some(true), false);

    assert_eq!(fresh["diagnostics"], cached_seed["diagnostics"]);
    assert_eq!(fresh["warnings"], cached_seed["warnings"]);
    assert_eq!(cached_seed["diagnostics"], cached_hit["diagnostics"]);
    assert_eq!(cached_seed["warnings"], cached_hit["warnings"]);
}

#[test]
fn cache_hit_reuses_permitted_syntax_artifact() {
    let (_temp, root) = project();

    let seeded = analyze(&root, Some(true), true);
    let artifact = storage(&root)
        .read(&syntax_key(&root, "main.py"))
        .expect("read cache")
        .expect("syntax artifact exists");
    let CacheArtifact::Syntax(cached) = artifact else {
        panic!("expected syntax cache artifact");
    };
    let cached_hit = analyze(&root, Some(true), false);

    assert_eq!(seeded["diagnostics"], cached_hit["diagnostics"]);
    assert_eq!(cached.syntax.call_sites[0].expression, "time.sleep(1)");
}

#[test]
fn cache_invalidates_when_file_content_hash_changes() {
    let (_temp, root) = project();
    analyze(&root, Some(true), true);
    let old_hash = sha256_hex(
        fs::read(root.join("main.py"))
            .expect("read original")
            .as_slice(),
    );

    write(
        &root.join("main.py"),
        "import time\n\nasync def handler():\n    time.sleep(2)\n",
    );
    analyze(&root, Some(true), false);
    let artifact = storage(&root)
        .read(&syntax_key(&root, "main.py"))
        .expect("read cache")
        .expect("syntax artifact exists");
    let CacheArtifact::Syntax(cached) = artifact else {
        panic!("expected syntax cache artifact");
    };

    assert_ne!(cached.content_hash, old_hash);
    assert_eq!(
        cached.content_hash,
        sha256_hex(
            fs::read(root.join("main.py"))
                .expect("read changed")
                .as_slice()
        )
    );
}

#[test]
fn cache_manifest_tracks_added_and_deleted_files() {
    let (_temp, root) = project();
    analyze(&root, Some(true), true);
    let original = storage(&root)
        .read_manifest()
        .expect("read manifest")
        .expect("manifest exists");

    write(&root.join("helper.py"), "def helper():\n    return 1\n");
    analyze(&root, Some(true), false);
    let with_added = storage(&root)
        .read_manifest()
        .expect("read manifest")
        .expect("manifest exists");

    fs::remove_file(root.join("helper.py")).expect("delete helper");
    analyze(&root, Some(true), false);
    let after_delete = storage(&root)
        .read_manifest()
        .expect("read manifest")
        .expect("manifest exists");

    assert_eq!(original.entries.len() + 1, with_added.entries.len());
    assert_eq!(original.entries.len(), after_delete.entries.len());
}

#[test]
fn cache_invalidates_when_config_changes() {
    let (_temp, root) = project();
    let config = root.join("pyproject.toml");
    write(
        &config,
        "[tool.strato]\nintervention_strategy = \"first-party-deepest\"\n",
    );
    let options = |clear_cache| AnalysisOptions {
        config: ConfigSource::Path(config.clone().into_std_path_buf()),
        cache_enabled: Some(true),
        clear_cache,
        ..AnalysisOptions::defaults()
    };
    strato_core::analyze_path_with_options(root.as_std_path(), &options(true)).expect("analysis");
    let before = storage(&root)
        .read_manifest()
        .expect("read manifest")
        .expect("manifest exists");

    write(
        &config,
        "[tool.strato]\nintervention_strategy = \"async-boundary\"\n",
    );
    strato_core::analyze_path_with_options(root.as_std_path(), &options(false)).expect("analysis");
    let after = storage(&root)
        .read_manifest()
        .expect("read manifest")
        .expect("manifest exists");

    assert_ne!(before.config_hash, after.config_hash);
}

#[test]
fn cache_invalidates_when_strato_version_changes() {
    let (_temp, root) = project();
    analyze(&root, Some(true), true);
    let mut manifest = storage(&root)
        .read_manifest()
        .expect("read manifest")
        .expect("manifest exists");
    manifest.strato_version = "old-version".to_string();
    storage(&root)
        .write_manifest(&manifest)
        .expect("write old manifest");

    analyze(&root, Some(true), false);
    let refreshed = storage(&root)
        .read_manifest()
        .expect("read manifest")
        .expect("manifest exists");

    assert_eq!(refreshed.strato_version, strato_core::STRATO_VERSION);
}

#[test]
fn clear_cache_removes_stale_cache_contents_before_analysis() {
    let (_temp, root) = project();
    analyze(&root, Some(true), true);
    let stale = root.join(".strato_cache").join("stale.txt");
    write(&stale, "stale");

    analyze(&root, Some(true), true);

    assert!(!stale.exists());
    assert!(
        storage(&root)
            .read_manifest()
            .expect("read manifest")
            .is_some()
    );
}
