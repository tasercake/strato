//! JSON output formatter.

/// Render the core reporter JSON value with deterministic pretty formatting.
pub(crate) fn render(report: &serde_json::Value) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(report).map(|mut output| {
        output.push('\n');
        output
    })
}
