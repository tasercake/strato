#![allow(missing_docs)]

use std::{collections::BTreeSet, fs};

use camino::{Utf8Path, Utf8PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
struct SourceFile {
    path: Utf8PathBuf,
    text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Violation {
    path: Utf8PathBuf,
    line: usize,
    message: String,
}

#[test]
fn anti_shortcut_checks_reject_fixture_or_expected_output_coupling() {
    let fixture_fingerprints = vec![
        "A1".to_string(),
        "a01_direct_blocking".to_string(),
        "Direct Blocking".to_string(),
    ];
    let files = vec![SourceFile {
        path: Utf8PathBuf::from("crates/strato_core/src/lib.rs"),
        text: r#"
fn shortcut(fixture: &str) -> &'static str {
    if fixture.ends_with("a01_direct_blocking") || fixture == "A1" {
        return std::fs::read_to_string("expected.json").unwrap();
    }
    "ok"
}
"#
        .to_string(),
    }];

    let violations = production_shortcut_violations(&files, &fixture_fingerprints);

    assert!(
        violations
            .iter()
            .any(|violation| violation.message.contains("expected.json")),
        "expected a production expected.json read violation, got {violations:#?}"
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.message.contains("fixture fingerprint")),
        "expected a fixture fingerprint coupling violation, got {violations:#?}"
    );
}

#[test]
fn boundary_checks_reject_direct_ruff_or_ty_core_access() {
    let files = vec![SourceFile {
        path: Utf8PathBuf::from("crates/strato_core/src/lib.rs"),
        text: "use ruff_python_parser::parse_module;\nfn f() { ty_python_semantic::run(); }\n"
            .to_string(),
    }];
    let manifest = r#"
[package]
name = "strato_core"

[dependencies]
ruff_python_parser.workspace = true
ty_python_semantic = "0.0.1"
"#;

    let source_violations = direct_core_import_violations(&files);
    let dependency_violations = direct_core_dependency_violations(manifest);

    assert!(
        source_violations
            .iter()
            .any(|violation| violation.message.contains("Ruff/ty import")),
        "expected direct Ruff/ty source import violation, got {source_violations:#?}"
    );
    assert!(
        dependency_violations
            .iter()
            .any(|violation| violation.message.contains("ruff_python_parser")),
        "expected direct Ruff dependency violation, got {dependency_violations:#?}"
    );
    assert!(
        dependency_violations
            .iter()
            .any(|violation| violation.message.contains("ty_python_semantic")),
        "expected direct ty dependency violation, got {dependency_violations:#?}"
    );
}

#[test]
fn fixture_schema_guard_rejects_weakened_loader_checks() {
    let weakened_loader = r#"
fn validate_expected_json(expected: &Value) -> Result<(), Error> {
    let diagnostics = expected["diagnostics"].as_array().unwrap_or(&[]);
    Ok(())
}
"#;

    let violations = fixture_schema_guard_violations(weakened_loader);

    assert!(
        violations
            .iter()
            .any(|violation| violation.message.contains("version")),
        "expected missing version schema guard violation, got {violations:#?}"
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.message.contains("warnings")),
        "expected missing warnings schema guard violation, got {violations:#?}"
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.message.contains("exactly one run")),
        "expected missing single-run schema guard violation, got {violations:#?}"
    );
}

#[test]
fn current_fixture_loader_schema_guards_remain_intact() {
    let loader_text =
        std::fs::read_to_string(repo_root().join("crates/strato_core/tests/test_fixtures.rs"))
            .expect("read fixture loader");

    let violations = fixture_schema_guard_violations(&loader_text);

    assert!(
        violations.is_empty(),
        "fixture loader schema guards must not be weakened:\n{violations:#?}"
    );
}

#[test]
fn clean_strato_core_sources_do_not_use_acceptance_shortcuts() {
    let production_files = load_strato_core_production_files();
    let fixture_fingerprints = load_fixture_fingerprints();

    let violations = production_shortcut_violations(&production_files, &fixture_fingerprints);

    assert!(
        violations.is_empty(),
        "production source must not couple to fixture outputs or identities:\n{violations:#?}"
    );
}

#[test]
fn clean_strato_core_keeps_ruff_and_ty_behind_adapter_boundary() {
    let production_files = load_strato_core_production_files();
    let manifest_text = std::fs::read_to_string(repo_root().join("crates/strato_core/Cargo.toml"))
        .expect("read strato_core manifest");

    let mut violations = direct_core_import_violations(&production_files);
    violations.extend(direct_core_dependency_violations(&manifest_text));

    assert!(
        violations.is_empty(),
        "strato_core must access Ruff/ty only through strato_ty_adapter:\n{violations:#?}"
    );
}

fn repo_root() -> Utf8PathBuf {
    Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn load_strato_core_production_files() -> Vec<SourceFile> {
    let root = repo_root();
    load_rust_files(&root.join("crates/strato_core/src"), &root)
}

fn load_fixture_fingerprints() -> Vec<String> {
    let root = repo_root();
    let fixture_root = root.join("tests/fixtures");
    let mut fingerprints = BTreeSet::new();

    for entry in fs::read_dir(fixture_root.as_std_path()).expect("read fixture root") {
        let entry = entry.expect("read fixture directory entry");
        let path = Utf8PathBuf::from_path_buf(entry.path()).expect("fixture path is UTF-8");
        if !path.is_dir() {
            continue;
        }

        let directory_name = path.file_name().expect("fixture directory name");
        fingerprints.insert(directory_name.to_string());
        fingerprints.insert(fixture_id_from_directory(directory_name));

        let manifest_text = fs::read_to_string(path.join("fixture.toml").as_std_path())
            .expect("read fixture manifest");
        if let Some(name) = fixture_manifest_name(&manifest_text) {
            fingerprints.insert(name);
        }
    }

    fingerprints.into_iter().collect()
}

fn production_shortcut_violations(
    files: &[SourceFile],
    fixture_fingerprints: &[String],
) -> Vec<Violation> {
    let mut violations = Vec::new();
    for file in files {
        for (line_index, line) in file.text.lines().enumerate() {
            if line.contains("expected.json") {
                violations.push(Violation {
                    path: file.path.clone(),
                    line: line_index + 1,
                    message: "production source references expected.json".to_string(),
                });
            }

            for fingerprint in fixture_fingerprints {
                if line_contains_fixture_fingerprint(line, fingerprint) {
                    violations.push(Violation {
                        path: file.path.clone(),
                        line: line_index + 1,
                        message: format!(
                            "production source contains fixture fingerprint '{fingerprint}'"
                        ),
                    });
                }
            }
        }
    }
    violations
}

fn direct_core_import_violations(files: &[SourceFile]) -> Vec<Violation> {
    let mut violations = Vec::new();
    for file in files {
        for (line_index, line) in file.text.lines().enumerate() {
            let code = line.split("//").next().unwrap_or(line);
            for prefix in ["ruff_", "ty_"] {
                if let Some(crate_name) = raw_crate_reference(code, prefix) {
                    violations.push(Violation {
                        path: file.path.clone(),
                        line: line_index + 1,
                        message: format!(
                            "direct Ruff/ty import '{crate_name}' bypasses strato_ty_adapter"
                        ),
                    });
                }
            }
        }
    }
    violations
}

fn direct_core_dependency_violations(manifest: &str) -> Vec<Violation> {
    let value = toml::from_str::<toml::Value>(manifest).expect("parse strato_core manifest");
    let mut dependency_names = BTreeSet::new();
    collect_dependency_names(&value, &mut dependency_names);

    dependency_names
        .into_iter()
        .filter(|name| name.starts_with("ruff_") || name.starts_with("ty_"))
        .map(|name| Violation {
            path: Utf8PathBuf::from("crates/strato_core/Cargo.toml"),
            line: manifest_line_for_key(manifest, &name),
            message: format!("direct Ruff/ty dependency '{name}' bypasses strato_ty_adapter"),
        })
        .collect()
}

fn fixture_schema_guard_violations(loader_text: &str) -> Vec<Violation> {
    [
        (
            "version",
            "expected JSON must contain string field 'version'",
        ),
        (
            "diagnostics",
            "expected JSON must contain array field 'diagnostics'",
        ),
        (
            "warnings",
            "expected JSON must contain array field 'warnings'",
        ),
        ("exactly one run", "manifest must define exactly one run"),
        (
            "unknown JSON section",
            "expected.json asserts unknown JSON section",
        ),
        (
            "all inputs accounted",
            "is not listed in source_files, config_files, extra_files, or expected.json",
        ),
    ]
    .into_iter()
    .filter(|(_, needle)| !loader_text.contains(needle))
    .map(|(guard, _)| Violation {
        path: Utf8PathBuf::from("crates/strato_core/tests/test_fixtures.rs"),
        line: 1,
        message: format!("fixture schema guard for {guard} is missing"),
    })
    .collect()
}

fn load_rust_files(src_root: &Utf8Path, repo_root: &Utf8Path) -> Vec<SourceFile> {
    let mut paths = Vec::new();
    collect_rust_paths(src_root, &mut paths);
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let relative = path
                .strip_prefix(repo_root)
                .expect("production file below repo root")
                .to_path_buf();
            SourceFile {
                path: relative,
                text: fs::read_to_string(path.as_std_path()).expect("read production source"),
            }
        })
        .collect()
}

fn collect_rust_paths(dir: &Utf8Path, paths: &mut Vec<Utf8PathBuf>) {
    for entry in fs::read_dir(dir.as_std_path()).expect("read Rust source directory") {
        let entry = entry.expect("read Rust source entry");
        let path = Utf8PathBuf::from_path_buf(entry.path()).expect("Rust source path is UTF-8");
        if path.is_dir() {
            collect_rust_paths(&path, paths);
        } else if path.extension() == Some("rs") {
            paths.push(path);
        }
    }
}

fn fixture_id_from_directory(directory_name: &str) -> String {
    directory_name
        .split('_')
        .next()
        .map_or_else(String::new, |id| {
            let upper = id.to_ascii_uppercase();
            upper
                .strip_prefix('A')
                .and_then(|number| number.parse::<u8>().ok())
                .map_or(upper, |number| format!("A{number}"))
        })
}

fn fixture_manifest_name(manifest_text: &str) -> Option<String> {
    toml::from_str::<toml::Value>(manifest_text)
        .ok()
        .and_then(|manifest| {
            manifest
                .get("name")
                .and_then(toml::Value::as_str)
                .map(str::to_string)
        })
}

fn line_contains_fixture_fingerprint(line: &str, fingerprint: &str) -> bool {
    if fingerprint.starts_with('A')
        && fingerprint[1..]
            .chars()
            .all(|character| character.is_ascii_digit())
    {
        line_has_quoted_value(line, fingerprint)
    } else {
        line.contains(fingerprint)
    }
}

fn line_has_quoted_value(line: &str, value: &str) -> bool {
    line.contains(&format!("\"{value}\""))
        || line.contains(&format!("'{value}'"))
        || line.contains(&format!("#\"{value}\"#"))
}

fn raw_crate_reference(line: &str, prefix: &str) -> Option<String> {
    for (start, _) in line.match_indices(prefix) {
        if start > 0
            && line[..start]
                .chars()
                .next_back()
                .is_some_and(is_rust_identifier_character)
        {
            continue;
        }

        let mut end = start + prefix.len();
        for character in line[end..].chars() {
            if is_rust_identifier_character(character) {
                end += character.len_utf8();
            } else {
                break;
            }
        }

        let crate_name = &line[start..end];
        let after = line[end..].trim_start();
        if after.starts_with("::") || after.starts_with("as ") || after.starts_with(';') {
            return Some(crate_name.to_string());
        }
    }
    None
}

fn is_rust_identifier_character(character: char) -> bool {
    character == '_' || character.is_ascii_alphanumeric()
}

fn collect_dependency_names(value: &toml::Value, names: &mut BTreeSet<String>) {
    for table_name in ["dependencies", "dev-dependencies", "build-dependencies"] {
        if let Some(table) = value.get(table_name).and_then(toml::Value::as_table) {
            names.extend(table.keys().cloned());
        }
    }

    if let Some(targets) = value.get("target").and_then(toml::Value::as_table) {
        for target in targets.values() {
            collect_dependency_names(target, names);
        }
    }
}

fn manifest_line_for_key(manifest: &str, key: &str) -> usize {
    manifest
        .lines()
        .position(|line| line.trim_start().starts_with(key))
        .map_or(1, |index| index + 1)
}
