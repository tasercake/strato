# Appendix C: Output Format Specifications

### C.1 Text Format

The text format is the default human-readable output, providing compiler-style diagnostics with source context.

**Format Specification:**

```
<CODE>: <MESSAGE>

  --> <FILE>:<LINE>:<COLUMN>
   |
<LINE_NUM> | <SOURCE_LINE>
   | <UNDERLINE_WITH_MESSAGE>
   |
   = chain: <FUNCTION_1> -> <FUNCTION_2> -> ... -> <BLOCKING_CALL>
   = help: <REMEDIATION_ADVICE>
```

**Example (A2 Test Case):**

```
STRATO002: Async function 'handler' calls blocking function 'helper'

  --> example.py:7:5
   |
 7 |     helper()
   |     ^^^^^^^^ calls blocking function
   |
   = chain: handler -> helper -> time.sleep
   = help: Wrap in `await asyncio.to_thread(...)` or use async alternative

Found 1 blocking issue in 1 file (2 functions analyzed)
```

### C.2 JSON Format

Machine-readable structured output for programmatic consumption and CI integration.

**Schema Definition:**

```json
{
  "version": "1.0",
  "diagnostics": [
    {
      "code": "string (STRATO001-STRATO004)",
      "severity": "string (error | warning)",
      "message": "string",
      "primary_location": {
        "file": "string (relative path)",
        "line": "integer (1-indexed)",
        "column": "integer (1-indexed)",
        "end_line": "integer (1-indexed, optional)",
        "end_column": "integer (1-indexed, optional)"
      },
      "related_locations": [
        {
          "file": "string",
          "line": "integer",
          "column": "integer",
          "message": "string"
        }
      ],
      "chain": [
        {
          "function": "string (fully qualified name)",
          "file": "string | null",
          "line": "integer | null",
          "is_async": "boolean",
          "is_first_party": "boolean"
        }
      ],
      "help": "string",
      "intervention_strategy": "string"
    }
  ],
  "warnings": [
    {
      "message": "string",
      "file": "string (optional)"
    }
  ],
  "stats": {
    "files_analyzed": "integer",
    "functions_analyzed": "integer",
    "call_graph_nodes": "integer",
    "call_graph_edges": "integer",
    "blocking_functions_found": "integer",
    "analysis_time_ms": "integer"
  }
}
```

**Required Fields:** `version`, `diagnostics`, `stats` always present. Within each diagnostic: `code`, `severity`, `message`, `primary_location` required. `related_locations`, `chain`, `help` optional.

**Example (A2 Test Case):**

```json
{
  "version": "1.0",
  "diagnostics": [
    {
      "code": "STRATO002",
      "severity": "error",
      "message": "Async function 'handler' calls blocking function 'helper'",
      "primary_location": {
        "file": "example.py",
        "line": 7,
        "column": 5,
        "end_line": 7,
        "end_column": 13
      },
      "related_locations": [
        {
          "file": "example.py",
          "line": 3,
          "column": 1,
          "message": "helper defined here"
        },
        {
          "file": "example.py",
          "line": 4,
          "column": 5,
          "message": "blocking call: time.sleep"
        }
      ],
      "chain": [
        {
          "function": "handler",
          "file": "example.py",
          "line": 6,
          "is_async": true,
          "is_first_party": true
        },
        {
          "function": "helper",
          "file": "example.py",
          "line": 3,
          "is_async": false,
          "is_first_party": true
        },
        {
          "function": "time.sleep",
          "file": null,
          "line": null,
          "is_async": false,
          "is_first_party": false
        }
      ],
      "help": "Wrap in `await asyncio.to_thread(...)` or use async alternative",
      "intervention_strategy": "first-party-deepest"
    }
  ],
  "warnings": [],
  "stats": {
    "files_analyzed": 1,
    "functions_analyzed": 2,
    "call_graph_nodes": 2,
    "call_graph_edges": 1,
    "blocking_functions_found": 1,
    "analysis_time_ms": 15
  }
}
```

**Ordering:** `diagnostics` sorted by file path, line, column. `chain` ordered from async entry to blocking call. Phantom node locations serialize as `null`.

### C.3 SARIF v2.1.0 Format

Compatible with GitHub Code Scanning, Azure DevOps, and CI/CD platforms supporting SARIF v2.1.0.

**Mapping to SARIF:**

| Strato Concept | SARIF Element |
|----------------|---------------|
| `primary_location` | `locations[0].physicalLocation` |
| `related_locations` | `relatedLocations` array |
| `chain` | `codeFlows[0].threadFlows[0].locations` |
| `severity` | `level` (error / warning / note) |

**Example (A2 Test Case):**

```json
{
  "$schema": "https://raw.githubusercontent.com/oasis-tcs/sarif-spec/main/sarif-2.1/schema/sarif-schema-2.1.0.json",
  "version": "2.1.0",
  "runs": [
    {
      "tool": {
        "driver": {
          "name": "strato",
          "version": "0.1.0",
          "informationUri": "https://github.com/owner/strato",
          "rules": [
            {
              "id": "STRATO001",
              "name": "DirectBlockingInAsync",
              "shortDescription": { "text": "Direct blocking call in async function" }
            },
            {
              "id": "STRATO002",
              "name": "IndirectBlockingInAsync",
              "shortDescription": { "text": "Blocking call reachable from async context via sync intermediary" }
            },
            {
              "id": "STRATO003",
              "name": "BlockingPropertyInAsync",
              "shortDescription": { "text": "Blocking @property getter accessed in async context" }
            },
            {
              "id": "STRATO004",
              "name": "BlockingDunderInAsync",
              "shortDescription": { "text": "Blocking dunder method invoked in async context" }
            }
          ]
        }
      },
      "results": [
        {
          "ruleId": "STRATO002",
          "level": "error",
          "message": { "text": "Async function 'handler' calls blocking function 'helper'" },
          "locations": [
            {
              "physicalLocation": {
                "artifactLocation": { "uri": "example.py" },
                "region": { "startLine": 7, "startColumn": 5, "endLine": 7, "endColumn": 13 }
              }
            }
          ],
          "relatedLocations": [
            {
              "id": 0,
              "message": { "text": "async context entry point" },
              "physicalLocation": {
                "artifactLocation": { "uri": "example.py" },
                "region": { "startLine": 6 }
              }
            },
            {
              "id": 1,
              "message": { "text": "helper defined here" },
              "physicalLocation": {
                "artifactLocation": { "uri": "example.py" },
                "region": { "startLine": 3 }
              }
            },
            {
              "id": 2,
              "message": { "text": "blocking call: time.sleep" },
              "physicalLocation": {
                "artifactLocation": { "uri": "example.py" },
                "region": { "startLine": 4 }
              }
            }
          ],
          "codeFlows": [
            {
              "threadFlows": [
                {
                  "locations": [
                    {
                      "location": {
                        "message": { "text": "async function handler()" },
                        "physicalLocation": {
                          "artifactLocation": { "uri": "example.py" },
                          "region": { "startLine": 6 }
                        }
                      }
                    },
                    {
                      "location": {
                        "message": { "text": "calls helper()" },
                        "physicalLocation": {
                          "artifactLocation": { "uri": "example.py" },
                          "region": { "startLine": 7 }
                        }
                      }
                    },
                    {
                      "location": {
                        "message": { "text": "calls blocking time.sleep()" },
                        "physicalLocation": {
                          "artifactLocation": { "uri": "example.py" },
                          "region": { "startLine": 4 }
                        }
                      }
                    }
                  ]
                }
              ]
            }
          ]
        }
      ]
    }
  ]
}
```

**SARIF-Specific Notes:**
- All four STRATO rules declared in tool driver, even if not all triggered
- `artifactLocation.uri` uses relative paths from project root
- Line and column numbers are 1-indexed per SARIF specification
- `codeFlows` ordered by execution sequence (async entry -> blocking call)
- Phantom nodes omit `physicalLocation`
