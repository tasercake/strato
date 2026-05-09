//! SARIF v2.1.0 output formatter.

use serde_json::{Value, json};

/// Render the core report as a basic SARIF v2.1.0 log.
pub(crate) fn render(report: &Value) -> Result<String, serde_json::Error> {
    let sarif = json!({
        "$schema": "https://raw.githubusercontent.com/oasis-tcs/sarif-spec/main/sarif-2.1/schema/sarif-schema-2.1.0.json",
        "version": "2.1.0",
        "runs": [{
            "tool": {
                "driver": {
                    "name": "strato",
                    "version": env!("CARGO_PKG_VERSION"),
                    "informationUri": env!("CARGO_PKG_REPOSITORY"),
                    "rules": rules(),
                }
            },
            "results": results(report),
        }]
    });
    serde_json::to_string_pretty(&sarif).map(|mut output| {
        output.push('\n');
        output
    })
}

fn rules() -> Vec<Value> {
    vec![
        rule(
            "STRATO001",
            "DirectBlockingInAsync",
            "Direct blocking call in async function",
        ),
        rule(
            "STRATO002",
            "TransitiveBlockingInAsync",
            "Transitive blocking call reachable from async context",
        ),
        rule(
            "STRATO003",
            "BlockingPropertyInAsync",
            "Blocking @property getter accessed in async context",
        ),
        rule(
            "STRATO004",
            "BlockingDunderInAsync",
            "Blocking dunder invocation in async context",
        ),
    ]
}

fn rule(id: &str, name: &str, description: &str) -> Value {
    json!({
        "id": id,
        "name": name,
        "shortDescription": { "text": description }
    })
}

fn results(report: &Value) -> Vec<Value> {
    report["diagnostics"]
        .as_array()
        .into_iter()
        .flatten()
        .map(result)
        .collect()
}

fn result(diagnostic: &Value) -> Value {
    let code = diagnostic["code"].as_str().unwrap_or("STRATO");
    let message = diagnostic["message"].as_str().unwrap_or("blocking issue");
    let severity = diagnostic["severity"].as_str().unwrap_or("error");
    let mut value = json!({
        "ruleId": code,
        "level": severity,
        "message": { "text": message },
        "locations": [physical_location(&diagnostic["primary_location"])],
    });
    if let Some(related) = related_locations(diagnostic) {
        value["relatedLocations"] = related;
    }
    if let Some(code_flow) = code_flow(diagnostic) {
        value["codeFlows"] = json!([code_flow]);
    }
    value
}

fn physical_location(location: &Value) -> Value {
    json!({
        "physicalLocation": {
            "artifactLocation": {
                "uri": location["file"].as_str().unwrap_or("<unknown>")
            },
            "region": region(location)
        }
    })
}

fn region(location: &Value) -> Value {
    let mut region = json!({
        "startLine": location["line"].as_u64().unwrap_or(1),
        "startColumn": location["column"].as_u64().unwrap_or(1),
    });
    if let Some(line) = location["end_line"].as_u64() {
        region["endLine"] = json!(line);
    }
    if let Some(column) = location["end_column"].as_u64() {
        region["endColumn"] = json!(column);
    }
    region
}

fn related_locations(diagnostic: &Value) -> Option<Value> {
    let locations = diagnostic["related_locations"].as_array()?;
    (!locations.is_empty()).then(|| {
        Value::Array(
            locations
                .iter()
                .enumerate()
                .map(|(index, location)| {
                    json!({
                        "id": index + 1,
                        "message": { "text": location["message"].as_str().unwrap_or("related location") },
                        "physicalLocation": {
                            "artifactLocation": { "uri": location["file"].as_str().unwrap_or("<unknown>") },
                            "region": {
                                "startLine": location["line"].as_u64().unwrap_or(1),
                                "startColumn": location["column"].as_u64().unwrap_or(1),
                            }
                        }
                    })
                })
                .collect(),
        )
    })
}

fn code_flow(diagnostic: &Value) -> Option<Value> {
    let chain = diagnostic["chain"].as_array()?;
    (!chain.is_empty()).then(|| {
        json!({
            "threadFlows": [{
                "locations": chain.iter().map(thread_flow_location).collect::<Vec<_>>()
            }]
        })
    })
}

fn thread_flow_location(link: &Value) -> Value {
    json!({
        "location": {
            "message": { "text": link["function"].as_str().unwrap_or("call") },
            "physicalLocation": {
                "artifactLocation": { "uri": link["file"].as_str().unwrap_or("<external>") },
                "region": { "startLine": link["line"].as_u64().unwrap_or(1) }
            }
        }
    })
}
