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
│   ├── strato_core/                 # Core analysis library
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── discovery.rs         # Phase 1: file discovery, config loading
│   │       ├── parser.rs            # Phase 2: parser abstraction layer
│   │       ├── semantics.rs         # Phase 3: ty-backed semantic layer
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
├── python/                          # Python annotations package
│   └── strato/
│       ├── __init__.py
│       ├── _annotations.py          # @blocking, @non_blocking, @unblocker
│       └── py.typed                 # PEP 561 marker
│
├── tests/
│   ├── fixtures/                    # Test Python projects
│   │   ├── smoke/                   # Minimal fixture
│   │   ├── a01_direct_blocking/     # A1: direct call in async
│   │   ├── a02_indirect_blocking/   # A2: sync intermediary
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
│   │   └── large_project/           # Performance test fixture
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
| `strato_core` | Core analysis library (7-phase pipeline) | `ruff_python_parser`, `ruff_python_ast`, `petgraph`, `serde`, `rayon`, `thiserror` |
| `strato_cache` | Incremental caching subsystem | `serde`, `bincode`, `sha2` |
| `strato_cli` | CLI binary and output formatters | `strato_core`, `strato_cache`, `clap`, `miette`, `serde_json`, `toml`, `globset` |

**Key External Dependencies:**

| Dependency | Version/Source | Purpose |
|------------|----------------|---------|
| `ruff_python_parser` | Pinned ruff git rev | Python AST parsing |
| `ruff_python_ast` | Pinned ruff git rev | Python AST types |
| `ty_python_semantic` | Pinned ruff/ty git rev | Module, name, and type semantics |
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
| M-1 | ty Integration Spike | Validate ty crate API and semantic facts at pinned rev | Small |
| M0 | Project Scaffolding | Compiling workspace with stub modules | Small |
| M1 | Parser + Discovery | Parse Python files using ruff, discover project files | Medium |
| M2 | Semantic Layer | ty-backed module/name/type facts normalized for Strato | Large |
| M3 | Call Graph | Project-wide call graph construction | Large |
| M4 | Blocking Database | 60 known blocking functions with help text | Medium |
| M5 | Propagation | SCC-based blocking propagation (Tarjan's algorithm) | Medium |
| M6 | Escape Hatches | `run_in_executor`, `to_thread`, `@unblocker` detection | Small |
| M7 | Properties + Dunders | Implicit call detection (`@property`, `__str__`, etc.) | Medium |
| M8 | Diagnostics | Error reporting with intervention strategies | Medium |
| M9 | CLI + Output | Working binary with text/JSON/SARIF output | Medium |
| M10 | Caching | Incremental analysis with content-based invalidation | Medium |
| M11 | Integration Tests | All 19 acceptance test fixtures pass | Medium |
| M12 | Performance + Polish | Performance validated, README, maturin build | Medium |

**Critical Path:** M-1 -> M0 -> M1 -> M2 -> M3 -> M4 -> M5 -> M6 -> M7 -> M8 -> M9 -> M10 -> M11 -> M12 (strictly sequential)

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
cargo run -p strato_cli -- check <path> --format json
cargo run -p strato_cli -- check <path> --format sarif
```
