# Supporting Systems

> **Decision recap:** [Caching Strategy](./design-overview.md#caching-strategy) – file-level caching with SHA-256 content hashing for Strato-owned Phase 1 and Phase 2 extraction artifacts, excluding vendored Ruff parsed modules, Phase 3 vendored ty semantic results, call graph edges, propagation, and diagnostics from the cache. [Distribution](./design-overview.md#distribution) – dual PyPI packages with zero binary footprint and an optional tiny pure-Python annotation package in production.

[tooling]

### CLI Interface

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

  --output <FORMAT>          Output format.
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
                              Values: 3.7, 3.8, ..., 3.15

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
| 0 | No blocking issues found (some files may have syntax warnings) |
| 1 | Blocking issues detected |
| 2 | Configuration error (invalid config, missing source roots) |
| 3 | No analyzable source files remain (no analysis possible) |

**Syntax error policy**: Ruff's parser is error-resilient and usually returns an AST plus syntax diagnostics. Individual file syntax errors are **non-fatal** – strato emits a warning for each affected file and continues on remaining analyzable files. Exit code 3 is returned **only** when no source file can be analyzed. Warnings do NOT affect exit code.

#### Example Usage

```bash
# Basic analysis
strato check src/

# CI pipeline (JSON output, fail on issues)
strato check src/ --output json > report.json

# GitHub Code Scanning
strato check src/ --output sarif > results.sarif

# Override strategy
strato check src/ --intervention-strategy async-boundary

# Fresh analysis (ignore cache)
strato check src/ --no-cache

# Show stats
strato check src/ --stats
```

### Configuration Loading

Strato loads configuration from `pyproject.toml` under the `[tool.strato]` section. Configuration precedence:

**CLI flags > config file > defaults**

#### Configuration Discovery

The `--config` flag accepts an explicit path to `pyproject.toml`. If omitted, strato walks up the directory tree from the current working directory until it finds a `pyproject.toml` containing a `[tool.strato]` section. If no config is found, all settings use defaults.

#### Configuration Validation

Strato validates the config at startup and exits with code 2 on error:

| Check | Error Message |
|-------|--------------|
| `src_roots` path doesn't exist | `Source root '{path}' does not exist` |
| `src_roots` path has no `.py` or `.pyi` files | `Source root '{path}' contains no Python files` |
| Invalid `python_version` | `Invalid python_version: must be '3.7'...'3.15'` |
| Invalid `intervention_strategy` | `Invalid strategy: must be 'first-party-deepest' or 'async-boundary'` |
| Invalid `severity` | `Invalid severity: must be 'error' or 'warning'` |
| Invalid `output_format` | `Invalid output_format: must be 'text', 'json', or 'sarif'` |
| `blocking.add` entry missing `name` | `Blocking entry missing required field 'name'` |
| Invalid `category` in blocking entry | `Unknown category '{cat}'. Valid: sleep, network-io, file-io, subprocess, database-io, user-input, other` |
| Executor wrapper missing `callable_param` | `Executor wrapper '{name}' missing required field 'callable_param'` |
| Invalid executor wrapper `callable_param` | `Executor wrapper '{name}' callable_param must be an integer index or keyword name` |

For the complete configuration schema with all available options, see [Appendix D: Configuration Schema](./appendix-d-configuration-schema.md#appendix-d-configuration-schema).

### Caching Strategy

Strato implements file-level caching to accelerate incremental analysis. The cache stores Strato-owned per-file extraction artifacts keyed by SHA-256 content hash. Parsed modules and semantic facts are owned by the vendored Ruff/ty `ProjectDatabase` for the current run and are not serialized by Strato.

#### What Is Cached

Each file produces a **per-file analysis result** that can be cached:

```rust
struct CachedFileResult {
    content_hash: [u8; 32],          // SHA-256 of file contents
    syntax: FileSyntax,              // Declarations, imports-as-syntax, decorators
    raw_decorators: Vec<DecoratorSyntax>, // Semantic annotation classification happens through the facade
}
```

#### What Is NOT Cached

- **Ruff parsed modules**: `ruff_db::parsed::parsed_module` is cached by Salsa within the current run. Strato does not serialize Ruff ASTs separately.
- **ty semantic results**: Vendored ty uses Salsa for incremental computation, which maintains its own in-memory cache. Salsa's cache is not serialized by Strato and is designed for single-session use.
- **Resolved semantic facts**: Module/name/type/call/property/dunder results from the Strato ty facade are rebuilt or re-queried each run.
- **Call edges and call graph structure**: Rebuilt each run because edge targets depend on current semantic facts.
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
| File content changed (hash mismatch) | Re-extract Strato syntax for that file; Ruff/ty reparses within its database as needed |
| File added | Load Ruff parsed module, extract Strato syntax, merge into call graph |
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
     └─ Miss: Load Ruff parsed module → extract → serialize to cache
  3. Initialize/query vendored Ruff/ty ProjectDatabase through StratoTyFacade
  4. Build call edges and project call graph from parsed modules + facade facts

Always recompute (not cached):
  - Ruff/ty ProjectDatabase and semantic facade facts
  - Call edges and call graph structure
  - Blocking propagation (SCC + topological)
  - Diagnostics (generated from propagated graph)
```

### Performance Targets

| Scenario | Target | Rationale |
|----------|--------|-----------|
| Cached run (no changes) | < 500ms for 500 files | Hash comparison + cached Strato syntax + Ruff/ty setup and facade queries + graph rebuild + propagation |
| Fresh run (first analysis) | < 5s for 500 files | Ruff parse + Strato extraction + resolve + build + propagate |
| Incremental (1 file changed) | < 1s for 500 files | Re-extract 1 file + full facade query/graph rebuild |

#### Time Distribution (Fresh Run)

| Phase | Percentage | Optimization Strategy |
|-------|-----------|----------------------|
| Parse + syntax extraction | ~60% | Reuse Ruff parsed modules; cache Strato-owned extraction |
| Semantic setup/queries (Ruff/ty facade) | ~25% | Salsa in-run incremental computation |
| Propagation | ~10% | SCC-based linear-time algorithm |
| Reporting | ~5% | Minimal graph traversal |

#### Performance Complicating Factors

Ruff-level performance (200ms for 630 files) is difficult for **fresh** runs because:
1. **Cross-file coordination**: ruff analyzes files independently; Strato merges results for the call graph
2. **Module resolution**: Every import requires filesystem lookups
3. **Graph construction**: Visiting every function body and resolving callees
4. **Propagation**: Even at O(V+E), thousands of functions with tens of thousands of edges

However, **cached runs** can approach Ruff-level speed only if vendored Ruff/ty setup, facade queries, and graph construction stay cheap enough; there is no Strato-owned cross-run Ruff/ty cache and no cached call-edge set.

### Distribution & Packaging

Strato is distributed as **two separate PyPI packages** to maintain zero binary footprint in production.

#### Package 1: `strato` (Pure Python Annotations)

- **Size**: ~5 KB (pure Python, no dependencies)
- **Runtime cost**: Zero (decorators are identity functions)
- **Python support**: Python 3.7+, using Python 3.7-compatible type syntax
- **Install**: `pip install strato`

Contains: `__init__.py`, `_annotations.py` (`@blocking`, `@non_blocking`, `@unblocker`), `py.typed` (PEP 561 marker)

#### Package 2: `strato-cli` (Rust Binary)

- Built with `maturin` using `bindings = "bin"` (binary distribution)
- Platform-specific wheels: Linux x86_64/aarch64, macOS x86_64/aarch64, Windows x86_64
- **Install**: `pip install strato-cli`

#### Zero Binary Footprint Principle

**Production**: `pip install strato` (~5 KB, zero dependencies) only when annotation decorators are imported by application code
**Development**: `pip install strato[cli]` (includes strato-cli binary)
