# 11. Supporting Systems

> **Decision recap:** [Decision 3.13](./03-design-decisions.md#313-caching-strategy-and-ty-boundary) – file-level caching with SHA-256 content hashing, excluding ty results and propagation from the cache. [Decision 3.11](./03-design-decisions.md#311-distribution-dual-pypi-packages) – dual PyPI packages with zero production footprint.

[tooling]

### 11.1 CLI Interface

The `strato` command provides a single primary subcommand for analysis:

```
strato check [PATHS...] [OPTIONS]

Analyze Python files for blocking calls in async contexts.

ARGUMENTS:
  [PATHS...]                 Files or directories to analyze.
                             Default: current directory.

OPTIONS:
  --config <PATH>            Path to pyproject.toml.
                             Default: auto-detect (walk up).

  --format <FORMAT>          Output format.
                             Values: text (default), json, sarif

  --intervention-strategy    Override intervention point strategy.
  <STRATEGY>                 Values: first-party-deepest (default),
                                     async-boundary

  --severity <LEVEL>         Override diagnostic severity.
                             Values: error (default), warning

  --no-cache                 Disable caching for this run.

  --clear-cache              Clear the cache before analysis.

  --first-party <MODULES>    Override first-party module detection.
                             Comma-separated top-level package names.

  --python-version <VER>     Override Python version.
                             Values: 3.7, 3.8, ..., 3.13

  --stats                    Show analysis statistics after run.

  -q, --quiet                Suppress non-diagnostic output.
  -v, --verbose              Show detailed analysis progress.
  --help                     Show help message.
  --version                  Show strato version.
```

#### Binary Naming Convention

| Context | Name |
|---------|------|
| Cargo package name | `strato_cli` |
| Compiled binary | `strato` (via `[[bin]] name = "strato"`) |
| PyPI package name | `strato-cli` |
| User-facing command | `strato check src/` |

#### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | No blocking issues found (some files may have parse warnings) |
| 1 | Blocking issues detected |
| 2 | Configuration error (invalid config, missing source roots) |
| 3 | All files failed to parse (no analysis possible) |

**Parse error policy**: Individual file parse errors are **non-fatal** – strato emits a warning for each unparseable file and continues on remaining files. Exit code 3 is returned **only** when every file fails to parse. Warnings do NOT affect exit code.

#### Example Usage

```bash
# Basic analysis
strato check src/

# CI pipeline (JSON output, fail on issues)
strato check src/ --format json > report.json

# GitHub Code Scanning
strato check src/ --format sarif > results.sarif

# Override strategy
strato check src/ --intervention-strategy async-boundary

# Fresh analysis (ignore cache)
strato check src/ --no-cache

# Show stats
strato check src/ --stats
```

### 11.2 Configuration Loading

Strato loads configuration from `pyproject.toml` under the `[tool.strato]` section. Configuration precedence:

**CLI flags > config file > defaults**

#### Configuration Discovery

The `--config` flag accepts an explicit path to `pyproject.toml`. If omitted, strato walks up the directory tree from the current working directory until it finds a `pyproject.toml` containing a `[tool.strato]` section. If no config is found, all settings use defaults.

#### Configuration Validation

Strato validates the config at startup and exits with code 2 on error:

| Check | Error Message |
|-------|--------------|
| `src_roots` path doesn't exist | `Source root '{path}' does not exist` |
| `src_roots` path has no `.py` files | `Source root '{path}' contains no Python files` |
| Invalid `python_version` | `Invalid python_version: must be '3.7'...'3.13'` |
| Invalid `intervention_strategy` | `Invalid strategy: must be 'first-party-deepest' or 'async-boundary'` |
| `blocking.add` entry missing `name` | `Blocking entry missing required field 'name'` |
| Invalid `category` in blocking entry | `Unknown category '{cat}'. Valid: sleep, network-io, file-io, subprocess, database-io, user-input, other` |

For the complete configuration schema with all available options, see [Appendix D: Configuration Schema](./appendix-d-configuration-schema.md#appendix-d-configuration-schema).

### 11.3 Caching Strategy

Strato implements file-level caching to accelerate incremental analysis. The cache stores per-file parse results and symbol extraction, keyed by SHA-256 content hash.

#### What Is Cached

Each file produces a **per-file analysis result** that can be cached:

```rust
struct CachedFileResult {
    content_hash: [u8; 32],          // SHA-256 of file contents
    symbols: Vec<SymbolDef>,         // Symbols defined in this file
    imports: Vec<ImportStatement>,   // Import statements
    call_edges: Vec<CallEdge>,       // Call edges from functions in this file
    annotations: Vec<AnnotationEntry>, // @blocking, @non_blocking found
}
```

#### What Is NOT Cached

- **Type inference results**: The `ty` crate uses Salsa for incremental computation, which maintains its own in-memory cache. Salsa's cache is not serializable and is designed for single-session use.
- **Call graph structure**: Rebuilt from cached (or fresh) per-file call edges. This is fast (inserting edges into the graph structure).
- **Blocking propagation**: Always rerun. Linear-time O(V+E), completes in milliseconds.

#### Cache Location and Format

Default: `.strato_cache/` in the project root. Binary format using `bincode` for fast serialization.

```
.strato_cache/
├── manifest.bin         # Maps file paths to content hashes
├── files/
│   ├── {hash1}.bin      # CachedFileResult for file 1
│   ├── {hash2}.bin      # CachedFileResult for file 2
│   └── ...
└── version              # Cache format version (invalidate on upgrade)
```

#### Cache Invalidation

| Trigger | Action |
|---------|--------|
| File content changed (hash mismatch) | Re-parse that file |
| File added | Parse new file, merge into call graph |
| File deleted | Remove from call graph, re-propagate |
| Config changed | Full re-analysis |
| strato version changed | Full invalidation |
| `--clear-cache` flag | Delete cache directory |

#### Caching Flow

```
For each file in project:
  1. Compute SHA-256 hash
  2. Check manifest for matching hash
     ├─ Hit:  Load cached CachedFileResult
     └─ Miss: Parse → extract → serialize to cache
  3. Merge file's call edges into project call graph

Always recompute (not cached):
  - Call graph structure (rebuilt from edges)
  - Blocking propagation (SCC + topological)
  - Diagnostics (generated from propagated graph)
```

### 11.4 Performance Targets

| Scenario | Target | Rationale |
|----------|--------|-----------|
| Cached run (no changes) | < 500ms for 500 files | Hash comparison + graph rebuild + propagation |
| Fresh run (first analysis) | < 5s for 500 files | Full parse + resolve + build + propagate |
| Incremental (1 file changed) | < 1s for 500 files | Re-parse 1 file + full graph rebuild |

#### Time Distribution (Fresh Run)

| Phase | Percentage | Optimization Strategy |
|-------|-----------|----------------------|
| Parse | ~60% | Parallel parsing with `rayon` |
| Type queries (ty) | ~25% | Salsa incremental computation |
| Propagation | ~10% | SCC-based linear-time algorithm |
| Reporting | ~5% | Minimal graph traversal |

#### Performance Complicating Factors

Ruff-level performance (200ms for 630 files) is difficult for **fresh** runs because:
1. **Cross-file coordination**: ruff analyzes files independently; Strato merges results for the call graph
2. **Module resolution**: Every import requires filesystem lookups
3. **Graph construction**: Visiting every function body and resolving callees
4. **Propagation**: Even at O(V+E), thousands of functions with tens of thousands of edges

However, **cached runs** approach ruff-level speed because: no parsing, no AST walking, graph rebuild from cached edges is fast, propagation is a single linear pass.

### 11.5 Distribution & Packaging

Strato is distributed as **two separate PyPI packages** to maintain zero binary footprint in production.

#### Package 1: `strato` (Pure Python Annotations)

- **Size**: ~5 KB (pure Python, no dependencies)
- **Runtime cost**: Zero (decorators are identity functions)
- **Install**: `pip install strato`

Contains: `__init__.py`, `_annotations.py` (`@blocking`, `@non_blocking`, `@unblocker`), `py.typed` (PEP 561 marker)

#### Package 2: `strato-cli` (Rust Binary)

- Built with `maturin` using `bindings = "bin"` (binary distribution)
- Platform-specific wheels: Linux x86_64/aarch64, macOS x86_64/aarch64, Windows x86_64
- **Install**: `pip install strato-cli`

#### Zero Binary Footprint Principle

**Production**: `pip install strato` (~5 KB, zero dependencies)
**Development**: `pip install strato[cli]` (includes strato-cli binary)
