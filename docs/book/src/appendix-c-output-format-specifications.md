# Appendix C: Output Format Specifications

### Text Format

The text format is the default human-readable output, providing compiler-style diagnostics with source context.

All emitted source coordinates use 1-indexed lines and 1-indexed columns. Columns are character positions in the decoded source line, not byte offsets.

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
STRATO002: Transitive blocking call reachable from async context

  --> example.py:7:5
   |
 7 |     time.sleep(1)
   |     ^^^^^^^^^^^^^ calls blocking function
   |
   = chain: handler -> helper -> time.sleep
   = help: Wrap in `await asyncio.to_thread(...)` or use async alternative

Found 1 blocking issue in 1 file (2 functions analyzed)
```

### JSON Format

Machine-readable structured output for programmatic consumption and CI integration.

The diagnostic location field is `primary_location`. JSON output uses the same source coordinate convention as text output: 1-indexed lines and 1-indexed columns, with end positions optional and exclusive of the highlighted span's final character.

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
          "line": "integer (1-indexed)",
          "column": "integer (1-indexed)",
          "message": "string"
        }
      ],
      "chain": [
        {
          "function": "string (stable diagnostic display name)",
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
  ]
}
```

**Required Fields:** `version`, `diagnostics`, and `warnings` are always present. Within each diagnostic: `code`, `severity`, `message`, `primary_location`, and `intervention_strategy` are required. `related_locations`, `chain`, and `help` are optional. Full-JSON fixture comparisons cover every JSON field.

`chain.function` is a stable diagnostic display label, not the internal node key. It may omit the top-level `main.` module prefix for readability, while retaining module or class qualifiers when needed to disambiguate cross-file functions, methods, properties, and dunders.

**Example (A2 Test Case):**

```json
{
  "version": "1.0",
  "diagnostics": [
    {
      "code": "STRATO002",
      "severity": "error",
      "message": "Transitive blocking call reachable from async context",
      "primary_location": {
        "file": "example.py",
        "line": 7,
        "column": 5,
        "end_line": 7,
        "end_column": 18
      },
      "related_locations": [
        {
          "file": "example.py",
          "line": 3,
          "column": 11,
          "message": "async function handler defined here"
        },
        {
          "file": "example.py",
          "line": 6,
          "column": 1,
          "message": "helper defined here"
        },
        {
          "file": "example.py",
          "line": 7,
          "column": 5,
          "message": "blocking call: time.sleep"
        }
      ],
      "chain": [
        {
          "function": "handler",
          "file": "example.py",
          "line": 3,
          "is_async": true,
          "is_first_party": true
        },
        {
          "function": "helper",
          "file": "example.py",
          "line": 6,
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
  "warnings": []
}
```

**Ordering:** `diagnostics` sorted by `primary_location.file`, `primary_location.line`, `primary_location.column`, then `code`. `related_locations` are ordered by their role in the diagnostic rule, then by file, line, and column within the same role. `chain` ordered from async entry to blocking call. Phantom node locations serialize as `null`.

### SARIF v2.1.0 Format

Compatible with GitHub Code Scanning, Azure DevOps, and CI/CD platforms supporting SARIF v2.1.0.

SARIF regions use SARIF's native 1-indexed `startLine`, `startColumn`, `endLine`, and `endColumn` fields, preserving the same coordinates as Strato JSON output.

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
              "name": "TransitiveBlockingInAsync",
              "shortDescription": { "text": "Transitive blocking call reachable from async context" }
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
          "message": { "text": "Transitive blocking call reachable from async context" },
          "locations": [
            {
              "physicalLocation": {
                "artifactLocation": { "uri": "example.py" },
                "region": { "startLine": 7, "startColumn": 5, "endLine": 7, "endColumn": 18 }
              }
            }
          ],
          "relatedLocations": [
            {
              "id": 0,
              "message": { "text": "async context entry point" },
              "physicalLocation": {
                "artifactLocation": { "uri": "example.py" },
                "region": { "startLine": 3 }
              }
            },
            {
              "id": 1,
              "message": { "text": "helper defined here" },
              "physicalLocation": {
                "artifactLocation": { "uri": "example.py" },
                "region": { "startLine": 6 }
              }
            },
            {
              "id": 2,
              "message": { "text": "blocking call: time.sleep" },
              "physicalLocation": {
                "artifactLocation": { "uri": "example.py" },
                "region": { "startLine": 7 }
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
                          "region": { "startLine": 3 }
                        }
                      }
                    },
                    {
                      "location": {
                        "message": { "text": "calls helper()" },
                        "physicalLocation": {
                          "artifactLocation": { "uri": "example.py" },
                          "region": { "startLine": 4 }
                        }
                      }
                    },
                    {
                      "location": {
                        "message": { "text": "calls blocking time.sleep()" },
                        "physicalLocation": {
                          "artifactLocation": { "uri": "example.py" },
                          "region": { "startLine": 7 }
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
