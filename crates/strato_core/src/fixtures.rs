//! Acceptance fixture loading.

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
    /// Expected production JSON output.
    pub expected: ExpectedOutput,
    /// Explicit fixture invocation and assertion contract.
    pub manifest: FixtureManifest,
}

/// Explicit fixture invocation and assertion contract.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct FixtureManifest {
    /// Appendix B identifier, for example `A1`.
    pub id: String,
    /// Human-readable fixture name.
    pub name: String,
    /// Python source files that make up the fixture.
    pub source_files: Vec<Utf8PathBuf>,
    /// Optional configuration files used by this fixture.
    pub config_files: Vec<Utf8PathBuf>,
    /// Named analyzer runs over this fixture.
    pub runs: Vec<FixtureRun>,
}

/// One explicit analyzer run over a fixture.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct FixtureRun {
    /// Stable run name within the fixture.
    pub name: String,
    /// The behavior this run exists to protect.
    pub purpose: String,
    /// CLI arguments, excluding the executable name.
    pub args: Vec<String>,
    /// Configuration source: `defaults` or a fixture-relative config path.
    pub config: String,
    /// Cache mode for this run.
    pub cache: String,
    /// Expected process exit code.
    pub expected_exit_code: i32,
    /// Expected-output assertion mode and scope.
    pub expectation: FixtureExpectation,
}

/// Expected-output assertion mode and scope.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct FixtureExpectation {
    /// Assertion mode: `full_json` or `partial_json`.
    pub mode: String,
    /// Fixture-relative JSON expectation path.
    pub path: Utf8PathBuf,
    /// Top-level JSON sections this run asserts.
    #[serde(rename = "assert")]
    pub assert_sections: Vec<String>,
}

/// Expected analyzer JSON output for an acceptance fixture.
pub type ExpectedOutput = Value;

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
        let expected_path = root.join("expected.json");
        let expected_text =
            fs::read_to_string(&expected_path).map_err(|source| FixtureError::Io {
                path: expected_path.clone(),
                source,
            })?;
        let expected =
            serde_json::from_str::<ExpectedOutput>(&expected_text).map_err(|source| {
                FixtureError::Json {
                    path: expected_path,
                    source,
                }
            })?;
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
        let sources = collect_files(root, "py")?;
        let config_files = collect_config_files(root);

        validate_manifest(root, &sources, &config_files, &expected, &manifest)?;

        Ok(Self {
            id: fixture_id(root),
            name: fixture_name(root),
            root: root.to_path_buf(),
            sources,
            config_files,
            expected,
            manifest,
        })
    }
}

fn validate_manifest(
    root: &Utf8Path,
    sources: &[Utf8PathBuf],
    config_files: &[Utf8PathBuf],
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
        manifest.source_files != sources,
        &fixture,
        format!(
            "manifest source_files {:?} do not match discovered sources {:?}",
            manifest.source_files, sources
        ),
    )?;
    invalid_if(
        manifest.config_files != config_files,
        &fixture,
        format!(
            "manifest config_files {:?} do not match discovered config files {:?}",
            manifest.config_files, config_files
        ),
    )?;
    invalid_if(
        manifest.runs.is_empty(),
        &fixture,
        "manifest must define at least one run".to_string(),
    )?;

    for run in &manifest.runs {
        validate_run(root, config_files, &fixture, run)?;
    }
    validate_expected_json(root, sources, &fixture, expected)
}

fn validate_run(
    root: &Utf8Path,
    config_files: &[Utf8PathBuf],
    fixture: &str,
    run: &FixtureRun,
) -> Result<(), FixtureError> {
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
        !run.args.iter().any(|arg| arg == "--output"),
        fixture,
        format!("run '{}' must explicitly select --output", run.name),
    )?;
    invalid_if(
        run.config != "defaults" && !config_files.iter().any(|path| path == run.config.as_str()),
        fixture,
        format!(
            "run '{}' references undeclared config '{}'",
            run.name, run.config
        ),
    )?;
    invalid_if(
        !matches!(run.cache.as_str(), "disabled" | "fresh" | "cached"),
        fixture,
        format!("run '{}' has invalid cache mode '{}'", run.name, run.cache),
    )?;
    invalid_if(
        !matches!(run.expectation.mode.as_str(), "full_json" | "partial_json"),
        fixture,
        format!(
            "run '{}' has invalid expectation mode '{}'",
            run.name, run.expectation.mode
        ),
    )?;
    invalid_if(
        !root.join(&run.expectation.path).exists(),
        fixture,
        format!(
            "run '{}' expectation path '{}' does not exist",
            run.name, run.expectation.path
        ),
    )?;
    invalid_if(
        run.expectation.assert_sections.is_empty(),
        fixture,
        format!("run '{}' must assert at least one JSON section", run.name),
    )
}

fn validate_expected_json(
    root: &Utf8Path,
    sources: &[Utf8PathBuf],
    fixture: &str,
    expected: &ExpectedOutput,
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
    invalid_if(
        !expected["stats"].is_object(),
        fixture,
        "expected JSON must contain object field 'stats'".to_string(),
    )?;

    let source_lines = load_source_lines(root, sources)?;
    for diagnostic in diagnostics {
        validate_diagnostic(fixture, &source_lines, diagnostic)?;
    }
    Ok(())
}

fn validate_diagnostic(
    fixture: &str,
    source_lines: &BTreeMap<Utf8PathBuf, Vec<String>>,
    diagnostic: &Value,
) -> Result<(), FixtureError> {
    for field in ["code", "severity", "message"] {
        invalid_if(
            !diagnostic[field].is_string(),
            fixture,
            format!("diagnostic must contain string field '{field}'"),
        )?;
    }
    validate_location(fixture, source_lines, &diagnostic["primary_location"], true)?;
    if let Some(related_locations) = diagnostic["related_locations"].as_array() {
        for location in related_locations {
            validate_location(fixture, source_lines, location, false)?;
        }
    }
    if let Some(chain) = diagnostic["chain"].as_array() {
        for entry in chain {
            validate_chain_entry(fixture, source_lines, entry)?;
        }
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
    if entry["file"].is_null() && entry["line"].is_null() {
        return Ok(());
    }
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

fn fixture_name(root: &Utf8Path) -> String {
    root.file_name().map_or_else(String::new, |name| {
        name.split('_')
            .skip(1)
            .map(|word| {
                let mut chars = word.chars();
                match chars.next() {
                    Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                    None => String::new(),
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    })
}

fn collect_config_files(root: &Utf8Path) -> Vec<Utf8PathBuf> {
    let pyproject = root.join("pyproject.toml");
    if pyproject.exists() {
        vec![Utf8PathBuf::from("pyproject.toml")]
    } else {
        Vec::new()
    }
}

fn collect_files(root: &Utf8Path, extension: &str) -> Result<Vec<Utf8PathBuf>, FixtureError> {
    let mut files = Vec::new();
    collect_files_inner(root, root, extension, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_files_inner(
    fixture_root: &Utf8Path,
    dir: &Utf8Path,
    extension: &str,
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
            collect_files_inner(fixture_root, &path, extension, files)?;
        } else if path.extension() == Some(extension) {
            let relative = path
                .strip_prefix(fixture_root)
                .expect("path is below fixture root");
            files.push(relative.to_path_buf());
        }
    }
    Ok(())
}
