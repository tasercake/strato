# 1. Executive Summary

**Strato** is a Rust-based static analysis tool that detects blocking function calls inside Python async contexts. Unlike existing linters (flake8-async, ruff ASYNC2XX) which only catch **direct** blocking calls, Strato performs **full transitive call-graph analysis** — tracing through intermediary sync functions to find hidden blocking calls that would stall the event loop.

### The Novel Contribution

No existing tool catches this:

```python
def sync_helper():
    time.sleep(1)          # Blocking call hidden here

async def handler():
    sync_helper()          # Strato catches this. No other tool does.
```

Strato builds a project-wide call graph, propagates "blocking" status through function call chains using SCC-based linear-time analysis, and reports when blocking code is reachable from async contexts — with configurable error reporting that shows diagnostics in the user's own code, not deep in third-party libraries.

### Key Design Choices

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Analysis approach | Full transitive call graph | Only way to catch indirect blocking (the core value proposition) |
| Precision policy | High precision (Unknown = skip) | Zero false positives — users trust every diagnostic |
| Propagation algorithm | SCC-based (Tarjan's) | O(V+E) linear time, handles cycles elegantly |
| Type inference | Astral's `ty` crate | Full alias tracking, return types, MRO — critical for method/property resolution |
| Language | Rust | Performance parity with ruff; access to ruff parser crates |
| Distribution | Dual PyPI packages | Zero binary footprint in production (`strato` annotations + `strato-cli` binary) |
| Error reporting | Configurable intervention point | Default: deepest first-party function (most actionable) |
| Blocking database | Curated ~80 entries + user-extensible | High signal, low noise; extensible via config and `@blocking` decorator |
| Executor wrappers | Generalized registry | Built-in + config + `@unblocker` decorator for custom wrappers |
| v1 scope | asyncio only | Bounded complexity; architecture supports future framework expansion |

### v1 Scope Boundaries

**In scope:** asyncio blocking detection, transitive call graph, SCC propagation, property/dunder detection, executor wrapper recognition, 80+ built-in blocking functions, text/JSON/SARIF output, incremental caching.

**Out of scope:** trio/curio/anyio, dynamic imports, runtime analysis, cross-package analysis, auto-fix, IDE integration. See [Section 12](./12-known-limitations-scope-boundaries.md#12-known-limitations--scope-boundaries) for the full limitations matrix.

### Error Codes

| Code | What it catches |
|------|----------------|
| STRATO001 | Direct blocking call in async function |
| STRATO002 | Indirect blocking via sync intermediary (the novel case) |
| STRATO003 | Blocking `@property` accessed in async context |
| STRATO004 | Blocking dunder method invoked in async context |

### Performance Targets

| Scenario | Target |
|----------|--------|
| Fresh analysis (500 files) | < 5 seconds |
| Cached analysis (no changes) | < 500 milliseconds |

**Tags**: everyone
