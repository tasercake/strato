#![allow(missing_docs)]

use camino::Utf8PathBuf;
use strato_core::fixtures::AcceptanceFixture;

fn fixture_root() -> Utf8PathBuf {
    Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

#[test]
fn acceptance_fixtures_are_well_formed() {
    let fixtures = AcceptanceFixture::load_all(&fixture_root()).expect("load acceptance fixtures");

    assert_eq!(fixtures.len(), 25);
    assert!(fixtures.iter().all(|fixture| fixture.id.starts_with('A')));
    assert!(fixtures.iter().all(|fixture| !fixture.sources.is_empty()));
    assert!(
        fixtures
            .iter()
            .all(|fixture| !fixture.manifest.runs.is_empty())
    );
    assert!(
        fixtures
            .iter()
            .any(|fixture| fixture.manifest.runs.len() > 1)
    );
    assert!(fixtures.iter().any(|fixture| {
        fixture.expected["diagnostics"]
            .as_array()
            .is_some_and(Vec::is_empty)
    }));
    assert!(fixtures.iter().any(|fixture| {
        fixture.expected["warnings"]
            .as_array()
            .is_some_and(|warnings| !warnings.is_empty())
    }));
}

#[test]
#[ignore = "analysis engine is not implemented yet"]
fn acceptance_fixtures_match_expected_diagnostics() {
    let fixtures = AcceptanceFixture::load_all(&fixture_root()).expect("load acceptance fixtures");

    for fixture in fixtures {
        let actual = strato_core::analyze_fixture(&fixture).expect("analyze fixture");
        for run in &fixture.manifest.runs {
            if run.expectation.mode == "full_json" {
                assert_eq!(
                    actual, fixture.expected,
                    "{}: {} ({})",
                    fixture.id, fixture.name, run.name
                );
            } else {
                for section in &run.expectation.assert_sections {
                    assert_eq!(
                        actual[section], fixture.expected[section],
                        "{}: {} ({}) section {section}",
                        fixture.id, fixture.name, run.name
                    );
                }
            }
        }
    }
}
