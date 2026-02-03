# 4. Architecture Overview

### System Diagram

```
                              pyproject.toml
                                    |
                                    v
                          +-------------------+
                          |   1. DISCOVERY     |
                          |  Find Python files |
                          |  Load config       |
                          +--------+----------+
                                   |
                    File paths + config
                                   |
                                   v
                          +-------------------+
                          |   2. PARSE         |  <-- ruff_python_parser
                          | Parse all files    |      (parallelized)
                          | Build per-file AST |
                          +--------+----------+
                                   |
                        Per-file ASTs
                                   |
                                   v
                          +-------------------+
                          |   3. RESOLVE       |
                          | Map imports to     |
                          | source files       |
                          | Build symbol table |
                          +--------+----------+
                                   |
                     Cross-file symbol map
                                   |
                                   v
                          +-------------------+
                          |   4. BUILD         |
                          | Construct project- |
                          | wide call graph    |
                          +--------+----------+
                                   |
                           Call graph
                                   |
                                   v
                          +-------------------+
                          |   5. ANNOTATE      |
                          | Mark known         |
                          | blocking functions |
                          | from DB + @blocking|
                          +--------+----------+
                                   |
                     Annotated call graph
                                   |
                                   v
                          +-------------------+
                          |   6. PROPAGATE     |
                          | SCC decomposition  |
                          | + topological      |
                          | blocking spread    |
                          +--------+----------+
                                   |
                  Fully propagated graph
                                   |
                                   v
                          +-------------------+
                          |   7. REPORT        |
                          | Find async->block  |
                          | paths. Format      |
                          | diagnostics.       |
                          +-------------------+
                                   |
                    Text / JSON / SARIF output
```

### Component Map

```
strato-cli (Rust binary)
├── strato_core          # Core analysis library
│   ├── discovery        # File finder, config loader
│   ├── parser           # Thin wrapper over ruff_python_parser
│   ├── resolver         # Module resolver (import → file mapping)
│   ├── graph            # Call graph data structures
│   ├── annotator        # Blocking function database + decorator detection
│   ├── propagator       # SCC-based blocking propagation
│   └── reporter         # Diagnostic generation + formatting
├── strato_cache         # Incremental caching system
└── strato_cli           # CLI entry point, arg parsing, output formatting

strato (Python package)
└── strato/
    ├── __init__.py      # Re-exports decorators
    ├── _annotations.py  # @blocking, @non_blocking, @unblocker definitions
    └── py.typed         # PEP 561 marker
```

### Key Data Structures

| Structure | Purpose | Defined In |
|-----------|---------|------------|
| `ModuleMap` | Maps module paths to file paths | [Section 5.3](./05-analysis-pipeline.md#53-phase-3-resolve-module-resolution) |
| `SymbolTable` | Maps qualified names to definitions | [Section 5.3](./05-analysis-pipeline.md#53-phase-3-resolve-module-resolution) |
| `CallGraph` | Directed graph of function call relationships | [Section 6.1](./06-call-graph-type-resolution.md#61-graph-data-model) |
| `BlockingDatabase` | Registry of known blocking functions | [Section 8.1](./08-blocking-function-database-annotations.md#81-database-structure) |
| `EscapeHatchRegistry` | Patterns recognized as safe executor wrapping | [Section 9.3](./09-escape-hatches-executor-wrappers.md#93-generalized-wrapper-registry) |
| `Diagnostic` | Reported issue with location, chain, and help text | [Section 10.1](./10-error-reporting-diagnostics.md#101-error-codes) |
| `AnalysisCache` | Serialized per-file results for incremental analysis | [Section 11.3](./11-supporting-systems.md#113-caching-strategy) |

### Public API Contract (`strato_core`)

```rust
/// Top-level entry point: run the full analysis pipeline (Phases 1–7).
pub fn analyze(project_path: &Path, config: &Config) -> Result<AnalysisResult, AnalysisError>;

/// Configuration loaded from pyproject.toml [tool.strato] or defaults.
pub struct Config {
    pub src_roots: Vec<PathBuf>,            // Default: auto-detected
    pub python_version: PythonVersion,       // Default: "3.9"
    pub intervention_strategy: InterventionStrategy, // Default: FirstPartyDeepest
    pub severity: Severity,                  // Default: Error
    pub exclude: Vec<String>,                // Default: []
    pub stub_paths: Vec<PathBuf>,            // Default: []
    pub cache_dir: PathBuf,                  // Default: ".strato_cache"
    pub cache_enabled: bool,                 // Default: true
    pub blocking_config: BlockingConfig,     // Default: built-in database only
    pub escape_hatch_config: EscapeHatchConfig, // Default: built-in patterns only
}

/// Result of a complete analysis run.
pub struct AnalysisResult {
    pub diagnostics: Vec<Diagnostic>,    // Sorted per deterministic output rules
    pub stats: AnalysisStats,
}

/// Analysis statistics for --stats output.
pub struct AnalysisStats {
    pub files_analyzed: usize,
    pub functions_analyzed: usize,
    pub call_graph_nodes: usize,
    pub call_graph_edges: usize,
    pub blocking_functions_found: usize,
    pub analysis_time_ms: u64,
    pub cache_hits: usize,
    pub cache_misses: usize,
}

/// Errors that can occur during analysis.
pub enum AnalysisError {
    ConfigError(ConfigError),   // Exit code 2
    IoError(std::io::Error),
    AllParsesFailed,            // Exit code 3
}
```
