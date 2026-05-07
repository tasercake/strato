# Appendix E: Repository Structure & Implementation Plan

### Repository Layout

```
strato/                              # Monorepo root
├── Cargo.toml                       # Rust workspace definition
├── Cargo.lock
├── pyproject.toml                   # Python annotations package ("strato")
├── LICENSE
├── README.md
│
├── crates/
│   ├── strato_ty_adapter/           # Facade over vendored Ruff/ty
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── project.rs           # Owns ty_project::ProjectDatabase setup
│   │       ├── facade.rs            # Stable Strato semantic query API
│   │       ├── targets.rs           # ResolvedTarget, DefinitionKey, CallableInfo
│   │       └── patches.rs           # Compile-time assertions for vendored patch APIs
│   │
│   ├── strato_core/                 # Core analysis library
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── discovery.rs         # Phase 1: file discovery, config loading
│   │       ├── parser.rs            # Phase 2: syntax extraction from Ruff parsed modules
│   │       ├── semantics.rs         # Phase 3: normalized facts from strato_ty_adapter
│   │       ├── graph.rs             # Phase 4: call graph data structures
│   │       ├── graph_builder.rs     # Phase 4: call graph construction
│   │       ├── annotator.rs         # Phase 5: blocking annotation
│   │       ├── propagator.rs        # Phase 6: blocking propagation (SCC)
│   │       ├── reporter.rs          # Phase 7: diagnostic generation
│   │       ├── types.rs             # Shared types
│   │       └── database/
│   │           ├── mod.rs           # BlockingDatabase
│   │           ├── stdlib.rs        # Built-in stdlib entries
│   │           ├── network.rs       # Built-in network lib entries
│   │           ├── database.rs      # Built-in database lib entries
│   │           └── subprocess.rs    # Built-in subprocess entries
│   │
│   ├── strato_cache/                # Caching subsystem
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── manifest.rs          # Cache manifest (content hashes)
│   │       ├── storage.rs           # Cache read/write operations
│   │       └── invalidation.rs      # Cache invalidation logic
│   │
│   └── strato_cli/                  # CLI binary
│       ├── Cargo.toml
│       ├── pyproject.toml           # PyPI "strato-cli" package (maturin)
│       └── src/
│           ├── main.rs              # Entry point
│           ├── args.rs              # CLI argument parsing (clap)
│           ├── output/
│           │   ├── mod.rs
│           │   ├── text.rs          # Text output formatter
│           │   ├── json.rs          # JSON output formatter
│           │   └── sarif.rs         # SARIF output formatter
│           └── config.rs            # pyproject.toml config parsing
│
├── vendor/
│   ├── ruff-strato-patches.md        # Patch ledger: rationale, upstreamability, tests
│   └── ruff/                         # Pinned Ruff monorepo submodule with Strato patches
│       └── crates/
│           ├── ruff_db/
│           ├── ruff_python_ast/
│           ├── ruff_python_parser/
│           ├── ty_project/
│           ├── ty_module_resolver/
│           ├── ty_python_core/
│           └── ty_python_semantic/
│
├── python/                          # Python annotations package
│   └── strato/
│       ├── __init__.py
│       ├── _annotations.py          # @blocking, @non_blocking, @unblocker
│       └── py.typed                 # PEP 561 marker
│
├── tests/
│   ├── fixtures/                    # Test Python projects
│   │   ├── a01_direct_blocking/     # A1: direct call in async
│   │   │   ├── fixture.toml         # Explicit runs, config source, assertion scope
│   │   │   ├── main.py              # Fixture source
│   │   │   └── expected.json        # Expected JSON used by manifest runs
│   │   ├── a02_transitive_blocking/ # A2: transitive blocking
│   │   ├── a03_executor_safe/       # A3: run_in_executor
│   │   ├── a04_to_thread_safe/      # A4: asyncio.to_thread
│   │   ├── a05_sync_only_safe/      # A5: sync standalone
│   │   ├── a06_blocking_annotation/ # A6: @blocking decorator
│   │   ├── a07_non_blocking_override/ # A7: @non_blocking
│   │   ├── a08_property_blocking/   # A8: @property
│   │   ├── a09_dunder_blocking/     # A9: dunder methods
│   │   ├── a10_cross_file/          # A10: blocking across files
│   │   ├── a11_deep_transitive/     # A11: deep call chain
│   │   ├── a12_multiple_callers/    # A12: multiple async callers
│   │   ├── a13_mixed_safe_unsafe/   # A13: mixed safe/unsafe
│   │   ├── a14_unblocker_basic/     # A14: @unblocker
│   │   ├── a15_executor_wrapper_config/ # A15: configured wrappers
│   │   ├── a16_intermediate_property/ # A16: intermediate property classification
│   │   ├── a17_intermediate_dunder/  # A17: intermediate dunder classification
│   │   ├── a18_non_blocking_scc/     # A18: SCC + @non_blocking
│   │   ├── a19_alias_wrapper/       # A19: alias-based wrapper
│   │   ├── a20_deterministic_ordering/ # A20: deterministic ordering
│   │   ├── a21_cache_parity/        # A21: fresh/cached parity
│   │   ├── a22_star_import/         # A22: star import
│   │   ├── a23_namespace_package/   # A23: namespace package
│   │   ├── a24_related_locations/   # A24: related locations
│   │   └── a25_syntax_warnings/     # A25: syntax warnings
│   ├── integration/                 # Rust integration tests
│   │   ├── harness.rs               # Shared test harness
│   │   ├── test_direct_blocking.rs
│   │   ├── test_indirect_blocking.rs
│   │   ├── test_executor.rs
│   │   ├── test_annotations.rs
│   │   ├── test_property.rs
│   │   ├── test_dunder.rs
│   │   ├── test_cross_file.rs
│   │   ├── test_output_formats.rs
│   │   └── test_performance.rs
│   └── unit/
│
├── stubs/                           # Example .pyi stubs
│   └── examples/
│       └── redis.pyi
│
└── docs/
    └── rules/
        ├── STRATO001.md
        ├── STRATO002.md
        ├── STRATO003.md
        └── STRATO004.md
```

### Cargo Workspace

**Workspace Members:**

| Crate | Purpose | Dependencies |
|-------|---------|--------------|
| `strato_ty_adapter` | Stable facade over vendored Ruff/ty project, parser, resolver, and semantic APIs | `ruff_db`, `ruff_python_ast`, `ty_project`, `ty_module_resolver`, `ty_python_core`, `ty_python_semantic` via `vendor/ruff` paths |
| `strato_core` | Core analysis library (7-phase pipeline) | `strato_ty_adapter`, `petgraph`, `serde`, `rayon`, `thiserror` |
| `strato_cache` | Incremental caching subsystem | `serde`, `bincode`, `sha2` |
| `strato_cli` | CLI binary and output formatters | `strato_core`, `strato_cache`, `clap`, `miette`, `serde_json`, `toml`, `globset` |

**Vendored Ruff/ty Dependencies:**

| Dependency | Version/Source | Purpose |
|------------|----------------|---------|
| `vendor/ruff` | Git submodule pinned to an audited Ruff commit | Source for Ruff parser, AST, database, and ty crates |
| `ruff_db` | Path dependency from `vendor/ruff/crates/ruff_db` | Source text, file IDs, parsed modules, Salsa database traits |
| `ruff_python_parser` | Path dependency from `vendor/ruff/crates/ruff_python_parser` | Python parser used by `ruff_db::parsed_module` |
| `ruff_python_ast` | Path dependency from `vendor/ruff/crates/ruff_python_ast` | Python AST types and visitors |
| `ty_project` | Path dependency from `vendor/ruff/crates/ty_project` | Project discovery/indexing and `ProjectDatabase` |
| `ty_module_resolver` | Path dependency from `vendor/ruff/crates/ty_module_resolver` | Module/search-path resolution |
| `ty_python_core` | Path dependency from `vendor/ruff/crates/ty_python_core` | Core semantic IDs, definitions, scopes, and program state |
| `ty_python_semantic` | Path dependency from `vendor/ruff/crates/ty_python_semantic`, patched if needed | Type/name/attribute/call semantic facts |

The crate list above identifies key consumed crates, not a partial checkout. Strato vendors the entire Ruff monorepo because these crates depend on additional internal Ruff/ty crates such as `ty_vendored`, `ty_static`, `ty_combine`, `ty_site_packages`, and other workspace members.

**Other External Dependencies:**

| Dependency | Version/Source | Purpose |
|------------|----------------|---------|
| `petgraph` | `0.6` | Call graph data structure |
| `serde` | `1` (derive) | Serialization |
| `bincode` | `1` | Binary cache format |
| `clap` | `4` (derive) | CLI argument parsing |
| `rayon` | `1` | Parallel file processing |
| `sha2` | `0.10` | File content hashing |
| `miette` | `7` (fancy) | Beautiful error output |

### Implementation Milestones

| Milestone | Name | Key Deliverable | Effort |
|-----------|------|-----------------|--------|
| M-2 | Vendor Ruff Baseline | Add pinned `vendor/ruff` submodule, path dependencies, patch ledger, and documented upgrade procedure | Medium |
| M-1 | Facade + Patch Spike | Implement `strato_ty_adapter`, add surgical vendored Ruff/ty APIs for all required semantic facts, and prove all facade queries on fixtures | Large |
| M0 | Project Scaffolding | Compiling workspace with stub modules and vendored Ruff/ty path dependencies | Small |
| M1 | Parser + Discovery | Index project via `ty_project`, load Ruff parsed modules for `.py` and `.pyi`, extract `FileSyntax`, and load the effective blocking database before graph construction | Medium |
| M2 | Semantic Layer | Facade-backed module/name/type/call/property/dunder facts normalized for Strato | Large |
| M3 | Call Graph | Project-wide call graph construction | Large |
| M4 | Blocking Database | 61 known blocking functions with help text | Medium |
| M5 | Propagation | SCC-based blocking propagation (Tarjan's algorithm) | Medium |
| M6 | Escape Hatches | `run_in_executor`, `to_thread`, `@unblocker` detection | Small |
| M7 | Properties + Dunders | Implicit call detection (`@property`, `__str__`, etc.) | Medium |
| M8 | Diagnostics | Error reporting with intervention strategies | Medium |
| M9 | CLI + Output | Working binary with text/JSON/SARIF output | Medium |
| M10 | Caching | Incremental analysis with content-based invalidation | Medium |
| M11 | Integration Tests | All 25 acceptance test fixtures pass | Medium |
| M12 | Performance + Polish | Performance validated, README, maturin build | Medium |

**Critical Path:** M-2 -> M-1 -> M0 -> M1 -> M2 -> M3 -> M4 -> M5 -> M6 -> M7 -> M8 -> M9 -> M10 -> M11 -> M12 (strictly sequential)

### Vendored Ruff Patch Policy

Ruff/ty patches are allowed, but must stay narrow and auditable.

| Rule | Requirement |
|------|-------------|
| Patch location | All modifications live under `vendor/ruff` on a Strato-maintained branch or patch queue |
| Patch purpose | Expose semantic facts needed by `strato_ty_adapter`; never implement Strato blocking policy in Ruff/ty |
| Patch ledger | Every change is recorded in `vendor/ruff-strato-patches.md` with file, rationale, upstreamability, and test coverage |
| Facade boundary | Only `strato_ty_adapter` may depend directly on Ruff/ty internals |
| Upgrade process | Updating Ruff requires replaying patches, running facade conformance tests, all acceptance fixtures, and determinism tests |

Required patched/facade facts for v1:

| Fact | Needed For |
|------|------------|
| `definitions_for_call` for `ExprCall` callee | Direct calls, aliases, methods, constructors, callable objects |
| `definitions_for_callable_reference` for expressions passed as values | Synthetic `in_executor=true` edges and configured wrapper callable arguments |
| Descriptor-aware property getter target for `ExprAttribute` | STRATO003, returning the `property.fget` definition rather than only the descriptor object |
| `definitions_for_dunder_operation` for Strato's operation enum | STRATO004 for unary, binary, comparison, conversion, formatting, subscript, iterator, context-manager, and `__call__` operations |
| Event-loop `run_in_executor` target identity | Built-in executor-wrapper detection without Strato-owned assignment heuristics |
| Deterministic qualified display name for `Definition` | Node display, config matching, diagnostics |
| External qualified aliases for resolved non-first-party calls | Blocking DB phantom matching across public names, re-exports, inherited definitions, and implementation modules |
| Parsed module access from the same ty database | Avoid independent double parsing |

Only the Ruff monorepo is vendored under `vendor/ruff`. Strato does not vendor the standalone `ty` package wrapper; all Rust path dependencies point directly at `vendor/ruff/crates/...`.

### Build & Test

```bash
# Build all crates
cargo build

# Build release binary
cargo build --release -p strato_cli

# Run all tests
cargo test

# Run performance tests
cargo test test_performance --release

# Build Python wheel (requires maturin)
maturin build -m crates/strato_cli/Cargo.toml

# Install annotations package
pip install -e .

# Run analysis
cargo run -p strato_cli -- check <path>
cargo run -p strato_cli -- check <path> --output json
cargo run -p strato_cli -- check <path> --output sarif
```
