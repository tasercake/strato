//! Fixture loading for acceptance tests.

use std::{collections::BTreeMap, fs, io};

use camino::{Utf8Path, Utf8PathBuf};
use serde::Deserialize;
use serde_json::Value;
use thiserror::Error;

/// A single acceptance fixture and its expected analyzer outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptanceFixture {
    /// Appendix B identifier, for example `A1`.
    pub id: String,
    /// Human-readable fixture name.
    pub name: String,
    /// Fixture directory.
    pub root: Utf8PathBuf,
    /// Python source files that make up the fixture.
    pub sources: Vec<Utf8PathBuf>,
    /// Optional configuration files used by this fixture.
    pub config_files: Vec<Utf8PathBuf>,
    /// Expected analyzer outcome for the fixture's single run.
    pub expected: ExpectedOutput,
    /// Explicit fixture invocation and assertion contract.
    pub manifest: FixtureManifest,
}

/// Explicit fixture invocation and assertion contract.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FixtureManifest {
    /// Appendix B identifier, for example `A1`.
    pub id: String,
    /// Human-readable fixture name.
    pub name: String,
    /// Python source files that make up the fixture.
    pub source_files: Vec<Utf8PathBuf>,
    /// Optional configuration files used by this fixture.
    pub config_files: Vec<Utf8PathBuf>,
    /// Other fixture inputs, such as stubs or helper package metadata.
    #[serde(default)]
    pub extra_files: Vec<Utf8PathBuf>,
    /// Analyzer run over this fixture.
    pub run: FixtureRun,
}

/// One explicit analyzer run over a fixture.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FixtureRun {
    /// Stable run name within the fixture.
    pub name: String,
    /// The behavior this run exists to protect.
    pub purpose: String,
    /// CLI arguments, excluding the executable name.
    pub args: Vec<String>,
    /// Cache mode for this run.
    pub cache: String,
}

/// Expected analyzer JSON output for an acceptance fixture.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ExpectedOutput {
    /// Expected process exit code.
    pub exit_code: i32,
    /// Expected fatal analysis error substring, when analysis cannot produce JSON.
    #[serde(default)]
    pub error: Option<String>,
    /// Assertion mode: `full_json` or `partial_json`.
    pub mode: String,
    /// Top-level JSON sections this run asserts.
    #[serde(rename = "assert")]
    pub assert_sections: Vec<String>,
    /// Expected analyzer JSON output.
    pub output: Value,
}

/// Fixture loading errors.
#[derive(Debug, Error)]
pub enum FixtureError {
    /// The fixture path is not valid UTF-8.
    #[error("fixture path is not valid UTF-8: {0}")]
    NonUtf8Path(String),
    /// Filesystem access failed.
    #[error("failed to read fixture path {path}: {source}")]
    Io {
        /// Path being read.
        path: Utf8PathBuf,
        /// Underlying I/O error.
        source: io::Error,
    },
    /// JSON parsing failed.
    #[error("failed to parse fixture expectation {path}: {source}")]
    Json {
        /// Expectation path being parsed.
        path: Utf8PathBuf,
        /// Underlying JSON error.
        source: serde_json::Error,
    },
    /// TOML parsing failed.
    #[error("failed to parse fixture manifest {path}: {source}")]
    Toml {
        /// Manifest path being parsed.
        path: Utf8PathBuf,
        /// Underlying TOML error.
        source: toml::de::Error,
    },
    /// Fixture metadata or expectations are invalid.
    #[error("invalid fixture {fixture}: {message}")]
    Invalid {
        /// Fixture identifier or path.
        fixture: String,
        /// Validation failure.
        message: String,
    },
}

impl AcceptanceFixture {
    /// Loads all fixture directories beneath `root`, sorted by path for deterministic tests.
    pub fn load_all(root: &Utf8Path) -> Result<Vec<Self>, FixtureError> {
        let entries = fs::read_dir(root).map_err(|source| FixtureError::Io {
            path: root.to_path_buf(),
            source,
        })?;

        let mut dirs = entries
            .map(|entry| {
                let entry = entry.map_err(|source| FixtureError::Io {
                    path: root.to_path_buf(),
                    source,
                })?;
                Utf8PathBuf::from_path_buf(entry.path())
                    .map_err(|path| FixtureError::NonUtf8Path(path.display().to_string()))
            })
            .collect::<Result<Vec<_>, _>>()?;

        dirs.sort();
        dirs.into_iter()
            .filter(|path| path.is_dir())
            .map(|path| Self::load(&path))
            .collect()
    }

    /// Loads one fixture directory.
    pub fn load(root: &Utf8Path) -> Result<Self, FixtureError> {
        let manifest_path = root.join("fixture.toml");
        let manifest_text =
            fs::read_to_string(&manifest_path).map_err(|source| FixtureError::Io {
                path: manifest_path.clone(),
                source,
            })?;
        let manifest = toml::from_str::<FixtureManifest>(&manifest_text).map_err(|source| {
            FixtureError::Toml {
                path: manifest_path,
                source,
            }
        })?;
        let expected = load_expected_output(root)?;
        let sources = manifest.source_files.clone();
        let config_files = manifest.config_files.clone();

        validate_manifest(root, &expected, &manifest)?;

        Ok(Self {
            id: fixture_id(root),
            name: manifest.name.clone(),
            root: root.to_path_buf(),
            sources,
            config_files,
            expected,
            manifest,
        })
    }
}

fn load_expected_output(root: &Utf8Path) -> Result<ExpectedOutput, FixtureError> {
    let expected_path = root.join("expected.json");
    let expected_text = fs::read_to_string(&expected_path).map_err(|source| FixtureError::Io {
        path: expected_path.clone(),
        source,
    })?;
    let expected = serde_json::from_str::<ExpectedOutput>(&expected_text).map_err(|source| {
        FixtureError::Json {
            path: expected_path,
            source,
        }
    })?;
    Ok(expected)
}

fn validate_manifest(
    root: &Utf8Path,
    expected: &ExpectedOutput,
    manifest: &FixtureManifest,
) -> Result<(), FixtureError> {
    let fixture = root
        .file_name()
        .map_or_else(|| root.to_string(), ToString::to_string);
    invalid_if(
        manifest.id != fixture_id(root),
        &fixture,
        format!("manifest id '{}' does not match directory id", manifest.id),
    )?;
    invalid_if(
        manifest.source_files.is_empty(),
        &fixture,
        "manifest must list at least one source file".to_string(),
    )?;
    validate_sorted_unique(&fixture, "source_files", &manifest.source_files)?;
    validate_sorted_unique(&fixture, "config_files", &manifest.config_files)?;
    validate_sorted_unique(&fixture, "extra_files", &manifest.extra_files)?;
    invalid_if(
        manifest.name.is_empty(),
        &fixture,
        "manifest name must not be empty".to_string(),
    )?;
    validate_source_paths(root, &fixture, &manifest.source_files)?;
    validate_manifest_paths(root, &fixture, "config_files", &manifest.config_files)?;
    validate_manifest_paths(root, &fixture, "extra_files", &manifest.extra_files)?;
    validate_all_inputs_accounted(root, &fixture, manifest)?;
    validate_run(&fixture, &manifest.run)?;
    let location_files = expected_location_files(&manifest.source_files, &manifest.extra_files);
    validate_expected_metadata(&fixture, expected)?;
    validate_expected_json(
        root,
        &location_files,
        &fixture,
        &expected.output,
        expected.mode == "full_json",
    )?;
    Ok(())
}

fn expected_location_files(
    source_files: &[Utf8PathBuf],
    extra_files: &[Utf8PathBuf],
) -> Vec<Utf8PathBuf> {
    let mut files = source_files.to_vec();
    files.extend(
        extra_files
            .iter()
            .filter(|path| path.extension() == Some("pyi"))
            .cloned(),
    );
    files.sort();
    files.dedup();
    files
}

fn validate_run(fixture: &str, run: &FixtureRun) -> Result<(), FixtureError> {
    invalid_if(
        run.name.is_empty(),
        fixture,
        "run name must not be empty".to_string(),
    )?;
    invalid_if(
        run.purpose.is_empty(),
        fixture,
        format!("run '{}' purpose must not be empty", run.name),
    )?;
    invalid_if(
        run.args
            .iter()
            .filter(|arg| arg.as_str() == "--output")
            .count()
            != 1,
        fixture,
        format!("run '{}' must specify exactly one --output flag", run.name),
    )?;
    invalid_if(
        run.args.iter().any(|arg| arg == "--format"),
        fixture,
        format!("run '{}' must use --output, not --format", run.name),
    )?;
    invalid_if(
        !run.args
            .windows(2)
            .any(|args| args[0] == "--output" && args[1] == "json"),
        fixture,
        format!(
            "run '{}' must request JSON output with '--output json'",
            run.name
        ),
    )?;
    invalid_if(
        !matches!(run.cache.as_str(), "disabled" | "fresh" | "cached"),
        fixture,
        format!("run '{}' has invalid cache mode '{}'", run.name, run.cache),
    )?;
    Ok(())
}

fn validate_expected_metadata(
    fixture: &str,
    expected: &ExpectedOutput,
) -> Result<(), FixtureError> {
    if expected.error.is_some() {
        invalid_if(
            !matches!(expected.exit_code, 2 | 3),
            fixture,
            "fatal expected errors must use exit code 2 or 3".to_string(),
        )?;
    }
    invalid_if(
        !matches!(expected.mode.as_str(), "full_json" | "partial_json"),
        fixture,
        format!(
            "expected.json has invalid expectation mode '{}'",
            expected.mode
        ),
    )?;
    invalid_if(
        expected.assert_sections.is_empty(),
        fixture,
        "expected.json must assert at least one JSON section".to_string(),
    )?;
    for section in &expected.assert_sections {
        invalid_if(
            !matches!(section.as_str(), "version" | "diagnostics" | "warnings"),
            fixture,
            format!("expected.json asserts unknown JSON section '{section}'"),
        )?;
    }
    Ok(())
}

fn validate_manifest_paths(
    root: &Utf8Path,
    fixture: &str,
    field: &str,
    paths: &[Utf8PathBuf],
) -> Result<(), FixtureError> {
    for path in paths {
        validate_fixture_relative_path(fixture, field, path)?;
        invalid_if(
            !root.join(path).is_file(),
            fixture,
            format!("{field} entry '{path}' does not exist"),
        )?;
    }
    Ok(())
}

fn validate_sorted_unique(
    fixture: &str,
    field: &str,
    paths: &[Utf8PathBuf],
) -> Result<(), FixtureError> {
    let mut sorted = paths.to_vec();
    sorted.sort();
    sorted.dedup();
    invalid_if(
        sorted != paths,
        fixture,
        format!("{field} entries must be sorted and unique"),
    )
}

fn validate_source_paths(
    root: &Utf8Path,
    fixture: &str,
    paths: &[Utf8PathBuf],
) -> Result<(), FixtureError> {
    validate_manifest_paths(root, fixture, "source_files", paths)?;
    for path in paths {
        invalid_if(
            path.extension() != Some("py"),
            fixture,
            format!("source_files entry '{path}' must be a .py file"),
        )?;
    }
    Ok(())
}

fn validate_fixture_relative_path(
    fixture: &str,
    field: &str,
    path: &Utf8Path,
) -> Result<(), FixtureError> {
    invalid_if(
        path.is_absolute()
            || path
                .components()
                .any(|component| component.as_str() == ".."),
        fixture,
        format!("{field} entry '{path}' must be fixture-relative"),
    )
}

fn validate_all_inputs_accounted(
    root: &Utf8Path,
    fixture: &str,
    manifest: &FixtureManifest,
) -> Result<(), FixtureError> {
    let mut accounted = BTreeMap::new();
    accounted.insert(Utf8PathBuf::from("fixture.toml"), "manifest");
    for path in &manifest.source_files {
        insert_accounted_input(&mut accounted, fixture, path, "source_files")?;
    }
    for path in &manifest.config_files {
        insert_accounted_input(&mut accounted, fixture, path, "config_files")?;
    }
    for path in &manifest.extra_files {
        insert_accounted_input(&mut accounted, fixture, path, "extra_files")?;
    }
    let expectation_path = Utf8PathBuf::from("expected.json");
    match accounted.get(&expectation_path).copied() {
        Some(existing) => {
            return Err(FixtureError::Invalid {
                fixture: fixture.to_string(),
                message: format!(
                    "fixture input '{expectation_path}' is listed in both {existing} and expected.json"
                ),
            });
        }
        None => {
            accounted.insert(expectation_path, "expectation");
        }
    }
    for path in collect_all_files(root)? {
        invalid_if(
            !accounted.contains_key(&path),
            fixture,
            format!(
                "fixture input '{path}' is not listed in source_files, config_files, extra_files, or expected.json"
            ),
        )?;
    }
    Ok(())
}

fn insert_accounted_input(
    accounted: &mut BTreeMap<Utf8PathBuf, &'static str>,
    fixture: &str,
    path: &Utf8PathBuf,
    category: &'static str,
) -> Result<(), FixtureError> {
    if let Some(existing) = accounted.insert(path.clone(), category) {
        return Err(FixtureError::Invalid {
            fixture: fixture.to_string(),
            message: format!("fixture input '{path}' is listed in both {existing} and {category}"),
        });
    }
    Ok(())
}

fn validate_expected_json(
    root: &Utf8Path,
    sources: &[Utf8PathBuf],
    fixture: &str,
    expected: &Value,
    requires_diagnostic_text: bool,
) -> Result<(), FixtureError> {
    invalid_if(
        !expected["version"].is_string(),
        fixture,
        "expected JSON must contain string field 'version'".to_string(),
    )?;
    let diagnostics = expected["diagnostics"]
        .as_array()
        .ok_or_else(|| FixtureError::Invalid {
            fixture: fixture.to_string(),
            message: "expected JSON must contain array field 'diagnostics'".to_string(),
        })?;
    invalid_if(
        !expected["warnings"].is_array(),
        fixture,
        "expected JSON must contain array field 'warnings'".to_string(),
    )?;

    let source_lines = load_source_lines(root, sources)?;
    for diagnostic in diagnostics {
        validate_diagnostic(fixture, &source_lines, diagnostic, requires_diagnostic_text)?;
    }
    for warning in expected["warnings"].as_array().expect("validated above") {
        validate_warning(fixture, &source_lines, warning)?;
    }
    Ok(())
}

fn validate_warning(
    fixture: &str,
    source_lines: &BTreeMap<Utf8PathBuf, Vec<String>>,
    warning: &Value,
) -> Result<(), FixtureError> {
    invalid_if(
        !warning["message"].is_string(),
        fixture,
        "warning must contain string field 'message'".to_string(),
    )?;
    if let Some(file) = warning["file"].as_str() {
        invalid_if(
            !source_lines.contains_key(&Utf8PathBuf::from(file)),
            fixture,
            format!("warning references unknown source file '{file}'"),
        )?;
    } else {
        invalid_if(
            !warning["file"].is_null(),
            fixture,
            "warning file must be a string when present".to_string(),
        )?;
    }
    Ok(())
}

fn validate_diagnostic(
    fixture: &str,
    source_lines: &BTreeMap<Utf8PathBuf, Vec<String>>,
    diagnostic: &Value,
    requires_text: bool,
) -> Result<(), FixtureError> {
    invalid_if(
        !diagnostic.is_object(),
        fixture,
        "diagnostic must be an object".to_string(),
    )?;
    for field in ["code", "severity"] {
        invalid_if(
            !diagnostic[field].is_string(),
            fixture,
            format!("diagnostic must contain string field '{field}'"),
        )?;
    }
    invalid_if(
        !matches!(
            diagnostic["code"].as_str(),
            Some("STRATO001" | "STRATO002" | "STRATO003" | "STRATO004")
        ),
        fixture,
        format!(
            "diagnostic has invalid code '{}'",
            diagnostic["code"].as_str().unwrap_or_default()
        ),
    )?;
    invalid_if(
        !matches!(diagnostic["severity"].as_str(), Some("error" | "warning")),
        fixture,
        format!(
            "diagnostic has invalid severity '{}'",
            diagnostic["severity"].as_str().unwrap_or_default()
        ),
    )?;
    invalid_if(
        !matches!(
            diagnostic["intervention_strategy"].as_str(),
            Some("first-party-deepest" | "async-boundary")
        ),
        fixture,
        "diagnostic must contain valid field 'intervention_strategy'".to_string(),
    )?;
    validate_location(fixture, source_lines, &diagnostic["primary_location"], true)?;
    if let Some(related_locations_value) = diagnostic.get("related_locations") {
        let related_locations =
            related_locations_value
                .as_array()
                .ok_or_else(|| FixtureError::Invalid {
                    fixture: fixture.to_string(),
                    message: "diagnostic field 'related_locations' must be an array".to_string(),
                })?;
        for location in related_locations {
            validate_location(fixture, source_lines, location, false)?;
            invalid_if(
                !location["message"].is_string(),
                fixture,
                "related location must contain string field 'message'".to_string(),
            )?;
        }
    }
    if let Some(chain_value) = diagnostic.get("chain") {
        let chain = chain_value
            .as_array()
            .ok_or_else(|| FixtureError::Invalid {
                fixture: fixture.to_string(),
                message: "diagnostic field 'chain' must be an array".to_string(),
            })?;
        for entry in chain {
            validate_chain_entry(fixture, source_lines, entry)?;
        }
    }
    if let Some(help) = diagnostic.get("help") {
        invalid_if(
            !help.is_string(),
            fixture,
            "diagnostic field 'help' must be a string".to_string(),
        )?;
    }
    if let Some(message) = diagnostic.get("message") {
        invalid_if(
            !message.is_string(),
            fixture,
            "diagnostic field 'message' must be a string".to_string(),
        )?;
    } else if requires_text {
        return Err(FixtureError::Invalid {
            fixture: fixture.to_string(),
            message: "diagnostic must contain string field 'message'".to_string(),
        });
    }
    Ok(())
}

fn validate_chain_entry(
    fixture: &str,
    source_lines: &BTreeMap<Utf8PathBuf, Vec<String>>,
    entry: &Value,
) -> Result<(), FixtureError> {
    invalid_if(
        !entry["function"].is_string(),
        fixture,
        "chain entry must contain string field 'function'".to_string(),
    )?;
    invalid_if(
        !entry["is_async"].is_boolean(),
        fixture,
        "chain entry must contain boolean field 'is_async'".to_string(),
    )?;
    invalid_if(
        !entry["is_first_party"].is_boolean(),
        fixture,
        "chain entry must contain boolean field 'is_first_party'".to_string(),
    )?;
    if entry["file"].is_null() && entry["line"].is_null() {
        invalid_if(
            entry["is_first_party"].as_bool() != Some(false),
            fixture,
            "phantom chain entry must have is_first_party=false".to_string(),
        )?;
        return Ok(());
    }
    invalid_if(
        entry["is_first_party"].as_bool() != Some(true),
        fixture,
        "first-party chain entry with file/line must have is_first_party=true".to_string(),
    )?;
    let file = entry["file"]
        .as_str()
        .ok_or_else(|| FixtureError::Invalid {
            fixture: fixture.to_string(),
            message: "first-party chain entry must contain string field 'file'".to_string(),
        })?;
    let line = entry["line"]
        .as_u64()
        .ok_or_else(|| FixtureError::Invalid {
            fixture: fixture.to_string(),
            message: "first-party chain entry must contain integer field 'line'".to_string(),
        })?;
    let source_line = source_line(source_lines, fixture, file, line)?;
    let function = entry["function"].as_str().expect("validated above");
    let short_name = function.rsplit('.').next().unwrap_or(function);
    if short_name.starts_with("<lambda>") {
        return invalid_if(
            !source_line.contains("lambda"),
            fixture,
            format!(
                "lambda chain entry '{function}' points to {file}:{line}, which is not a lambda"
            ),
        );
    }
    invalid_if(
        !source_line.contains(&format!("def {short_name}")),
        fixture,
        format!("chain entry '{function}' points to {file}:{line}, which is not its definition"),
    )
}

fn validate_location(
    fixture: &str,
    source_lines: &BTreeMap<Utf8PathBuf, Vec<String>>,
    location: &Value,
    primary: bool,
) -> Result<(), FixtureError> {
    let file = location["file"]
        .as_str()
        .ok_or_else(|| FixtureError::Invalid {
            fixture: fixture.to_string(),
            message: "location must contain string field 'file'".to_string(),
        })?;
    let line = location["line"]
        .as_u64()
        .ok_or_else(|| FixtureError::Invalid {
            fixture: fixture.to_string(),
            message: format!("location for {file} must contain integer field 'line'"),
        })?;
    let column = location["column"]
        .as_u64()
        .ok_or_else(|| FixtureError::Invalid {
            fixture: fixture.to_string(),
            message: format!("location for {file}:{line} must contain integer field 'column'"),
        })?;
    let source_line = source_line(source_lines, fixture, file, line)?;
    let max_column = source_line.chars().count() + 1;
    let column = usize::try_from(column).map_err(|_| FixtureError::Invalid {
        fixture: fixture.to_string(),
        message: format!("location {file}:{line}:{column} has an unsupported column value"),
    })?;
    invalid_if(
        column == 0 || column > max_column,
        fixture,
        format!("location {file}:{line}:{column} is outside the source line"),
    )?;
    invalid_if(
        source_line
            .chars()
            .nth(column.saturating_sub(1))
            .is_some_and(char::is_whitespace),
        fixture,
        format!("location {file}:{line}:{column} points at whitespace"),
    )?;
    let trimmed = source_line.trim_start();
    invalid_if(
        primary && (trimmed.starts_with("def ") || trimmed.starts_with("async def ")),
        fixture,
        format!("primary_location {file}:{line}:{column} points at a function definition"),
    )
}

fn source_line<'a>(
    source_lines: &'a BTreeMap<Utf8PathBuf, Vec<String>>,
    fixture: &str,
    file: &str,
    line: u64,
) -> Result<&'a str, FixtureError> {
    let path = Utf8PathBuf::from(file);
    let lines = source_lines
        .get(&path)
        .ok_or_else(|| FixtureError::Invalid {
            fixture: fixture.to_string(),
            message: format!("location references unknown source file '{file}'"),
        })?;
    let index = usize::try_from(line)
        .ok()
        .and_then(|line| line.checked_sub(1));
    index
        .and_then(|index| lines.get(index).map(String::as_str))
        .ok_or_else(|| FixtureError::Invalid {
            fixture: fixture.to_string(),
            message: format!("location references missing source line {file}:{line}"),
        })
}

fn load_source_lines(
    root: &Utf8Path,
    sources: &[Utf8PathBuf],
) -> Result<BTreeMap<Utf8PathBuf, Vec<String>>, FixtureError> {
    let mut lines = BTreeMap::new();
    for source in sources {
        let path = root.join(source);
        let text = fs::read_to_string(&path).map_err(|source| FixtureError::Io { path, source })?;
        lines.insert(
            source.clone(),
            text.lines().map(ToString::to_string).collect(),
        );
    }
    Ok(lines)
}

fn invalid_if(condition: bool, fixture: &str, message: String) -> Result<(), FixtureError> {
    if condition {
        Err(FixtureError::Invalid {
            fixture: fixture.to_string(),
            message,
        })
    } else {
        Ok(())
    }
}

fn fixture_id(root: &Utf8Path) -> String {
    root.file_name()
        .and_then(|name| name.split('_').next())
        .map_or_else(String::new, |id| {
            let upper = id.to_ascii_uppercase();
            upper
                .strip_prefix('A')
                .and_then(|number| number.parse::<u8>().ok())
                .map_or(upper, |number| format!("A{number}"))
        })
}

fn collect_all_files(root: &Utf8Path) -> Result<Vec<Utf8PathBuf>, FixtureError> {
    let mut files = Vec::new();
    collect_all_files_inner(root, root, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_all_files_inner(
    fixture_root: &Utf8Path,
    dir: &Utf8Path,
    files: &mut Vec<Utf8PathBuf>,
) -> Result<(), FixtureError> {
    for entry in fs::read_dir(dir).map_err(|source| FixtureError::Io {
        path: dir.to_path_buf(),
        source,
    })? {
        let entry = entry.map_err(|source| FixtureError::Io {
            path: dir.to_path_buf(),
            source,
        })?;
        let path = Utf8PathBuf::from_path_buf(entry.path())
            .map_err(|path| FixtureError::NonUtf8Path(path.display().to_string()))?;
        if path.is_dir() {
            if path.file_name() == Some(".strato_cache") {
                continue;
            }
            collect_all_files_inner(fixture_root, &path, files)?;
        } else {
            let relative = path
                .strip_prefix(fixture_root)
                .expect("path is below fixture root");
            files.push(relative.to_path_buf());
        }
    }
    Ok(())
}

// These are tests for our test harness (lol)
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp_fixture() -> (tempfile::TempDir, Utf8PathBuf) {
        let temp = tempfile::tempdir().expect("create temp fixture");
        let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).expect("utf8 temp path");
        fs::write(
            root.join("main.py"),
            "import time\n\nasync def handler():\n    time.sleep(1)\n",
        )
        .expect("write source");
        (temp, root)
    }

    fn valid_expected() -> Value {
        json!({
            "version": "1.0",
            "diagnostics": [{
                "code": "STRATO001",
                "severity": "error",
                "message": "Direct blocking call in async function",
                "primary_location": { "file": "main.py", "line": 4, "column": 5 },
                "chain": [
                    { "function": "handler", "file": "main.py", "line": 3, "is_async": true, "is_first_party": true },
                    { "function": "time.sleep", "file": null, "line": null, "is_async": false, "is_first_party": false }
                ],
                "intervention_strategy": "first-party-deepest"
            }],
            "warnings": []
        })
    }

    fn valid_expected_file() -> Value {
        json!({
            "exit_code": 0,
            "mode": "partial_json",
            "assert": ["diagnostics"],
            "output": valid_expected()
        })
    }

    #[test]
    fn expected_json_requires_intervention_strategy() {
        let (_temp, root) = temp_fixture();
        let mut expected = valid_expected();
        expected["diagnostics"][0]
            .as_object_mut()
            .expect("diagnostic object")
            .remove("intervention_strategy");

        let err = validate_expected_json(
            &root,
            &[Utf8PathBuf::from("main.py")],
            "fixture",
            &expected,
            true,
        )
        .expect_err("missing intervention strategy should fail");
        assert!(err.to_string().contains("intervention_strategy"));
    }

    #[test]
    fn expected_json_accepts_current_top_level_shape() {
        let (_temp, root) = temp_fixture();

        validate_expected_json(
            &root,
            &[Utf8PathBuf::from("main.py")],
            "fixture",
            &valid_expected(),
            true,
        )
        .expect("current top-level shape should validate");
    }

    #[test]
    fn expected_json_allows_partial_diagnostics_to_omit_text_fields() {
        let (_temp, root) = temp_fixture();
        let mut expected = valid_expected();
        let diagnostic = expected["diagnostics"][0]
            .as_object_mut()
            .expect("diagnostic object");
        diagnostic.remove("message");
        diagnostic.remove("help");

        validate_expected_json(
            &root,
            &[Utf8PathBuf::from("main.py")],
            "fixture",
            &expected,
            false,
        )
        .expect("partial diagnostic should not need text fields");
    }

    #[test]
    fn expected_json_requires_text_fields_for_full_diagnostics() {
        let (_temp, root) = temp_fixture();
        let mut expected = valid_expected();
        expected["diagnostics"][0]
            .as_object_mut()
            .expect("diagnostic object")
            .remove("message");

        let err = validate_expected_json(
            &root,
            &[Utf8PathBuf::from("main.py")],
            "fixture",
            &expected,
            true,
        )
        .expect_err("full diagnostic should require message");
        assert!(err.to_string().contains("message"));
    }

    #[test]
    fn expected_json_validates_warning_shape() {
        let (_temp, root) = temp_fixture();
        let mut expected = valid_expected();
        expected["warnings"] = json!([{ "message": 123, "file": "main.py" }]);

        let err = validate_expected_json(
            &root,
            &[Utf8PathBuf::from("main.py")],
            "fixture",
            &expected,
            true,
        )
        .expect_err("non-string warning message should fail");
        assert!(err.to_string().contains("warning"));
    }

    #[test]
    fn manifest_cannot_list_fixture_toml_as_input() {
        let (_temp, root) = temp_fixture();
        let id = fixture_id(&root);
        fs::write(
            root.join("expected.json"),
            valid_expected_file().to_string(),
        )
        .expect("write expectation");
        fs::write(
            root.join("fixture.toml"),
            format!(
                r#"id = "{id}"
name = "Fixture"
source_files = ["main.py"]
config_files = ["fixture.toml"]

[run]
name = "default"
purpose = "rejects fixture.toml as a manifest-declared input"
args = ["check", ".", "--output", "json"]
cache = "disabled"
"#
            ),
        )
        .expect("write manifest");

        let err = AcceptanceFixture::load(&root).expect_err("fixture.toml input should fail");
        assert!(err.to_string().contains("fixture.toml"));
    }

    #[test]
    fn fixture_validation_rejects_legacy_runs_missing_run_and_bad_asserts() {
        for (manifest_body, expected_file, expected_message) in [
            (
                r#"[[runs]]
name = "default"
purpose = "legacy plural runs table"
args = ["check", ".", "--output", "json"]
cache = "disabled"
"#,
                valid_expected_file(),
                "unknown field `runs`",
            ),
            (r#""#, valid_expected_file(), "missing field `run`"),
            (
                r#"[run]
name = "default"
purpose = "bad assert"
args = ["check", ".", "--output", "json"]
cache = "disabled"
"#,
                json!({
                    "exit_code": 0,
                    "mode": "partial_json",
                    "assert": ["bogus"],
                    "output": valid_expected()
                }),
                "unknown JSON section",
            ),
        ] {
            let (_temp, root) = temp_fixture();
            let id = fixture_id(&root);
            fs::write(root.join("expected.json"), expected_file.to_string())
                .expect("write expectation");
            fs::write(
                root.join("fixture.toml"),
                format!(
                    "id = \"{id}\"\nname = \"Fixture\"\nsource_files = [\"main.py\"]\nconfig_files = []\n\n{manifest_body}"
                ),
            )
            .expect("write manifest");

            let err = AcceptanceFixture::load(&root).expect_err("manifest should fail");
            assert!(
                err.to_string().contains(expected_message),
                "expected error containing {expected_message}, got {err}"
            );
        }
    }

    #[test]
    fn expected_json_rejects_malformed_optional_diagnostic_fields() {
        for (field, value) in [
            ("chain", json!({})),
            ("related_locations", json!("oops")),
            ("help", json!(false)),
            ("message", json!(false)),
        ] {
            let (_temp, root) = temp_fixture();
            let mut expected = valid_expected();
            expected["diagnostics"][0][field] = value;

            let err = validate_expected_json(
                &root,
                &[Utf8PathBuf::from("main.py")],
                "fixture",
                &expected,
                true,
            )
            .expect_err("malformed optional diagnostic field should fail");
            assert!(err.to_string().contains(field));
        }
    }
}
