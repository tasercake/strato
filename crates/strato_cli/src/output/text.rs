//! Human-readable text output formatter.

use std::fmt::Write as _;

/// Render a compact text summary from the core reporter JSON value.
#[must_use]
pub(crate) fn render(report: &serde_json::Value) -> String {
    let diagnostics = report["diagnostics"]
        .as_array()
        .map_or(&[][..], Vec::as_slice);
    let warnings = report["warnings"].as_array().map_or(&[][..], Vec::as_slice);
    let mut output = String::new();
    if diagnostics.is_empty() {
        output.push_str("No blocking issues found\n");
    } else {
        for diagnostic in diagnostics {
            let code = diagnostic["code"].as_str().unwrap_or("STRATO");
            let severity = diagnostic["severity"].as_str().unwrap_or("error");
            let message = diagnostic["message"].as_str().unwrap_or("blocking issue");
            let location = &diagnostic["primary_location"];
            let file = location["file"].as_str().unwrap_or("<unknown>");
            let line = location["line"].as_u64().unwrap_or(1);
            let column = location["column"].as_u64().unwrap_or(1);
            let _ = writeln!(
                output,
                "{file}:{line}:{column}: {severity} {code}: {message}"
            );
            if let Some(help) = diagnostic["help"].as_str() {
                let _ = writeln!(output, "  help: {help}");
            }
        }
    }
    for warning in warnings {
        let message = warning["message"].as_str().unwrap_or("analysis warning");
        if let Some(file) = warning["file"].as_str() {
            let _ = writeln!(output, "warning: {file}: {message}");
        } else {
            let _ = writeln!(output, "warning: {message}");
        }
    }
    output
}
