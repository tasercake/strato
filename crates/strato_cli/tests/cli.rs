#![allow(missing_docs)]

use std::process::Command;

use assert_cmd::cargo::cargo_bin;
use serde_json::Value;

fn fixture_path(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name);
    path.to_string_lossy().into_owned()
}

fn run_strato(args: &[&str]) -> std::process::Output {
    Command::new(cargo_bin("strato"))
        .args(args)
        .output()
        .expect("run strato")
}

fn stdout_json(output: &std::process::Output) -> Value {
    serde_json::from_slice(&output.stdout).expect("stdout is valid JSON")
}

#[test]
fn help_documents_check_options() {
    let output = run_strato(&["check", "--help"]);

    assert!(output.status.success(), "help failed: {output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    for option in [
        "--config",
        "--output",
        "--intervention-strategy",
        "--severity",
        "--no-cache",
        "--clear-cache",
        "--first-party",
        "--python-version",
        "--quiet",
        "--verbose",
    ] {
        assert!(
            stdout.contains(option),
            "missing {option} in help: {stdout}"
        );
    }
}

#[test]
fn json_output_reports_blocking_diagnostics_and_exit_code_one() {
    let output = run_strato(&[
        "check",
        &fixture_path("a01_direct_blocking"),
        "--output",
        "json",
    ]);

    assert_eq!(output.status.code(), Some(1), "output: {output:?}");
    assert!(output.stderr.is_empty(), "stderr: {output:?}");
    let json = stdout_json(&output);
    assert_eq!(json["version"], "1.0");
    assert!(
        json["diagnostics"]
            .as_array()
            .is_some_and(|items| !items.is_empty())
    );
    assert!(json["warnings"].is_array());
}

#[test]
fn json_output_returns_zero_when_no_blocking_issues_exist() {
    let output = run_strato(&[
        "check",
        &fixture_path("a05_sync_only_safe"),
        "--output",
        "json",
    ]);

    assert_eq!(output.status.code(), Some(0), "output: {output:?}");
    let json = stdout_json(&output);
    assert!(json["diagnostics"].as_array().is_some_and(Vec::is_empty));
    assert!(json["warnings"].is_array());
}

#[test]
fn sarif_output_uses_basic_v2_1_0_shape() {
    let output = run_strato(&[
        "check",
        &fixture_path("a01_direct_blocking"),
        "--output",
        "sarif",
    ]);

    assert_eq!(output.status.code(), Some(1), "output: {output:?}");
    let json = stdout_json(&output);
    assert_eq!(json["version"], "2.1.0");
    assert_eq!(json["runs"][0]["tool"]["driver"]["name"], "strato");
    assert!(
        json["runs"][0]["results"]
            .as_array()
            .is_some_and(|items| !items.is_empty())
    );
}

#[test]
fn text_output_does_not_use_scaffold_message() {
    let output = run_strato(&["check", &fixture_path("a05_sync_only_safe")]);

    assert_eq!(output.status.code(), Some(0), "output: {output:?}");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !combined.contains("not implemented"),
        "scaffold leaked: {combined}"
    );
}

#[test]
fn fatal_config_errors_exit_two() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let config_path = tempdir.path().join("pyproject.toml");
    std::fs::write(&config_path, "[tool.strato]\npython_version = '3.16'\n")
        .expect("write invalid config");
    std::fs::write(
        tempdir.path().join("main.py"),
        "async def main():\n    pass\n",
    )
    .expect("write source");

    let output = run_strato(&[
        "check",
        tempdir.path().to_str().expect("utf8 path"),
        "--config",
        config_path.to_str().expect("utf8 config path"),
        "--output",
        "json",
    ]);

    assert_eq!(output.status.code(), Some(2), "output: {output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("Invalid python_version"));
}

#[test]
fn no_analyzable_sources_exit_three() {
    let tempdir = tempfile::tempdir().expect("tempdir");

    let output = run_strato(&[
        "check",
        tempdir.path().to_str().expect("utf8 path"),
        "--output",
        "json",
    ]);

    assert_eq!(output.status.code(), Some(3), "output: {output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("No analyzable source files"));
}
