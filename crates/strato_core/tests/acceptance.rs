#![allow(missing_docs)]

use std::sync::OnceLock;

use camino::Utf8PathBuf;
use serde_json::Value;

mod test_fixtures;

use test_fixtures::{AcceptanceFixture, FixtureRun};

#[derive(Debug, Clone, PartialEq, Eq)]
struct FixtureRunOutput {
    exit_code: i32,
    json: Value,
}

fn analyze_fixture_run(
    fixture: &AcceptanceFixture,
    run: &FixtureRun,
) -> Result<FixtureRunOutput, strato_core::AnalysisError> {
    let config = if run.config == "defaults" {
        strato_core::ConfigSource::Defaults
    } else {
        strato_core::ConfigSource::Path(fixture.root.join(&run.config).into_std_path_buf())
    };
    let output = strato_core::analyze_path_with_options(
        fixture.root.as_std_path(),
        &strato_core::AnalysisOptions { config },
    )?;

    Ok(FixtureRunOutput {
        exit_code: output.exit_code,
        json: output.json,
    })
}

fn fixture_root() -> Utf8PathBuf {
    Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn fixtures() -> &'static [AcceptanceFixture] {
    static FIXTURES: OnceLock<Vec<AcceptanceFixture>> = OnceLock::new();
    FIXTURES.get_or_init(|| {
        AcceptanceFixture::load_all(&fixture_root()).expect("load acceptance fixtures")
    })
}

fn fixture_by_id(id: &str) -> &'static AcceptanceFixture {
    fixtures()
        .iter()
        .find(|fixture| fixture.id == id)
        .unwrap_or_else(|| panic!("fixture {id} not found"))
}

#[test]
fn acceptance_fixtures_are_well_formed() {
    let fixtures = fixtures();

    let ids = fixtures
        .iter()
        .map(|fixture| fixture.id.clone())
        .collect::<Vec<_>>();
    let expected_ids = (1..=51).map(|id| format!("A{id}")).collect::<Vec<_>>();
    assert_eq!(ids, expected_ids);
    assert!(fixtures.iter().all(|fixture| !fixture.sources.is_empty()));
    assert!(
        fixtures
            .iter()
            .all(|fixture| !fixture.manifest.runs.is_empty())
    );
    assert!(
        fixtures
            .iter()
            .all(|fixture| fixture.manifest.runs.len() == 1)
    );
    assert!(fixtures.iter().any(|fixture| {
        fixture.expected.output["diagnostics"]
            .as_array()
            .is_some_and(Vec::is_empty)
    }));
    assert!(fixtures.iter().any(|fixture| {
        fixture.expected.output["warnings"]
            .as_array()
            .is_some_and(|warnings| !warnings.is_empty())
    }));
    assert_full_json_contract_covers_error_codes(fixtures);
}

fn assert_full_json_contract_covers_error_codes(fixtures: &[AcceptanceFixture]) {
    let mut covered_codes = fixtures
        .iter()
        .flat_map(|fixture| {
            std::iter::once(&fixture.expected).filter(|expected| expected.mode == "full_json")
        })
        .flat_map(|expected| {
            expected.output["diagnostics"]
                .as_array()
                .into_iter()
                .flatten()
        })
        .filter_map(|diagnostic| diagnostic["code"].as_str())
        .collect::<Vec<_>>();

    covered_codes.sort_unstable();
    covered_codes.dedup();
    assert_eq!(
        covered_codes,
        vec!["STRATO001", "STRATO002", "STRATO003", "STRATO004"]
    );
}

fn assert_fixture_matches_expected(fixture: &AcceptanceFixture) {
    for run in &fixture.manifest.runs {
        let expected = &fixture.expected;
        if let Some(expected_error) = &expected.error {
            let config = if run.config == "defaults" {
                strato_core::ConfigSource::Defaults
            } else {
                strato_core::ConfigSource::Path(fixture.root.join(&run.config).into_std_path_buf())
            };
            let error = strato_core::analyze_path_with_options(
                fixture.root.as_std_path(),
                &strato_core::AnalysisOptions { config },
            )
            .expect_err("expected fatal analysis error");
            assert!(
                error.to_string().contains(expected_error),
                "{}: {} ({}) expected error containing '{}', got '{}'",
                fixture.id,
                fixture.name,
                run.name,
                expected_error,
                error
            );
            continue;
        }
        let actual_run = analyze_fixture_run(fixture, run).expect("analyze fixture run");
        assert_eq!(
            actual_run.exit_code, expected.exit_code,
            "{}: {} ({}) exit code",
            fixture.id, fixture.name, run.name
        );
        let actual = actual_run.json;
        if expected.mode == "full_json" {
            assert_eq!(
                actual, expected.output,
                "{}: {} ({})",
                fixture.id, fixture.name, run.name
            );
        } else {
            for section in &expected.assert_sections {
                let actual_section = normalize_partial_section(section, &actual[section]);
                let expected_section =
                    normalize_partial_section(section, &expected.output[section]);
                assert_json_subset(
                    &expected_section,
                    &actual_section,
                    &format!(
                        "{}: {} ({}) section {section}",
                        fixture.id, fixture.name, run.name
                    ),
                );
            }
        }
    }
}

macro_rules! acceptance_fixture_test {
    ($fn_name:ident, $id:literal) => {
        #[test]
        fn $fn_name() {
            let fixture = fixture_by_id($id);
            assert_fixture_matches_expected(fixture);
        }
    };
}

acceptance_fixture_test!(acceptance_a1_direct_blocking, "A1");
acceptance_fixture_test!(acceptance_a2_transitive_blocking, "A2");
acceptance_fixture_test!(acceptance_a3_executor_safe, "A3");
acceptance_fixture_test!(acceptance_a4_to_thread_safe, "A4");
acceptance_fixture_test!(acceptance_a5_sync_only_safe, "A5");
acceptance_fixture_test!(acceptance_a6_blocking_annotation, "A6");
acceptance_fixture_test!(acceptance_a7_non_blocking_override, "A7");
acceptance_fixture_test!(acceptance_a8_property_blocking, "A8");
acceptance_fixture_test!(acceptance_a9_dunder_blocking, "A9");
acceptance_fixture_test!(acceptance_a10_cross_file, "A10");
acceptance_fixture_test!(acceptance_a11_deep_transitive, "A11");
acceptance_fixture_test!(acceptance_a12_multiple_callers, "A12");
acceptance_fixture_test!(acceptance_a13_mixed_safe_unsafe, "A13");
acceptance_fixture_test!(acceptance_a14_unblocker_basic, "A14");
acceptance_fixture_test!(acceptance_a15_executor_wrapper_config, "A15");
acceptance_fixture_test!(acceptance_a16_intermediate_property, "A16");
acceptance_fixture_test!(acceptance_a17_intermediate_dunder, "A17");
acceptance_fixture_test!(acceptance_a18_non_blocking_scc, "A18");
acceptance_fixture_test!(acceptance_a19_alias_wrapper, "A19");
acceptance_fixture_test!(acceptance_a20_deterministic_ordering, "A20");
acceptance_fixture_test!(acceptance_a21_cache_parity, "A21");
acceptance_fixture_test!(acceptance_a22_star_import, "A22");
acceptance_fixture_test!(acceptance_a23_namespace_package, "A23");
acceptance_fixture_test!(acceptance_a24_related_locations, "A24");
acceptance_fixture_test!(acceptance_a25_syntax_warnings, "A25");
acceptance_fixture_test!(acceptance_a26_stub_annotation, "A26");
acceptance_fixture_test!(acceptance_a27_blocking_config_add, "A27");
acceptance_fixture_test!(acceptance_a28_blocking_config_remove, "A28");
acceptance_fixture_test!(acceptance_a29_blocking_module_prefix, "A29");
acceptance_fixture_test!(acceptance_a30_python_version_to_thread, "A30");
acceptance_fixture_test!(acceptance_a31_unresolved_call_precision, "A31");
acceptance_fixture_test!(acceptance_a32_partial_executor_wrapper, "A32");
acceptance_fixture_test!(acceptance_a33_method_call_resolution, "A33");
acceptance_fixture_test!(acceptance_a34_callable_object_dunder, "A34");
acceptance_fixture_test!(acceptance_a35_dunder_operations, "A35");
acceptance_fixture_test!(acceptance_a36_deterministic_ordering_repeat, "A36");
acceptance_fixture_test!(acceptance_a37_cache_parity_cached, "A37");
acceptance_fixture_test!(acceptance_a38_blocking_config_add_configured, "A38");
acceptance_fixture_test!(acceptance_a39_blocking_config_remove_configured, "A39");
acceptance_fixture_test!(acceptance_a40_python_310_to_thread, "A40");
acceptance_fixture_test!(acceptance_a41_invalid_config_fails, "A41");
acceptance_fixture_test!(acceptance_a42_keyword_executor_wrapper, "A42");
acceptance_fixture_test!(acceptance_a43_src_roots_exclude_boundaries, "A43");
acceptance_fixture_test!(acceptance_a44_no_analyzable_source_files, "A44");
acceptance_fixture_test!(acceptance_a45_assignment_shadowing_import, "A45");
acceptance_fixture_test!(acceptance_a46_reexported_import_resolution, "A46");
acceptance_fixture_test!(acceptance_a47_relative_import_resolution, "A47");
acceptance_fixture_test!(acceptance_a48_parameter_type_method_resolution, "A48");
acceptance_fixture_test!(acceptance_a49_stub_method_annotation, "A49");
acceptance_fixture_test!(acceptance_a50_inherited_property_resolution, "A50");
acceptance_fixture_test!(acceptance_a51_unresolved_import_precision, "A51");

fn assert_json_subset(expected: &Value, actual: &Value, context: &str) {
    match (expected, actual) {
        (Value::Object(expected), Value::Object(actual)) => {
            for (key, expected_value) in expected {
                let actual_value = actual
                    .get(key)
                    .unwrap_or_else(|| panic!("{context}: missing expected key '{key}'"));
                assert_json_subset(expected_value, actual_value, &format!("{context}.{key}"));
            }
        }
        (Value::Array(expected), Value::Array(actual)) => {
            assert_eq!(
                expected.len(),
                actual.len(),
                "{context}: array length differs"
            );
            for (index, (expected_value, actual_value)) in expected.iter().zip(actual).enumerate() {
                assert_json_subset(expected_value, actual_value, &format!("{context}[{index}]"));
            }
        }
        _ => assert_eq!(actual, expected, "{context}: value differs"),
    }
}

fn normalize_partial_section(section: &str, value: &Value) -> Value {
    let mut normalized = value.clone();
    if section == "diagnostics"
        && let Some(diagnostics) = normalized.as_array_mut()
    {
        for diagnostic in diagnostics {
            if let Some(object) = diagnostic.as_object_mut() {
                object.remove("message");
                object.remove("help");
                if let Some(related_locations) = object
                    .get_mut("related_locations")
                    .and_then(Value::as_array_mut)
                {
                    for related_location in related_locations {
                        if let Some(related_object) = related_location.as_object_mut() {
                            related_object.remove("message");
                        }
                    }
                }
            }
        }
    }
    normalized
}
