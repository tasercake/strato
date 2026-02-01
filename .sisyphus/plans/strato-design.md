# Strato: Async Blocking Call Detector — Architecture Design Document

> **Version**: 1.0-draft
> **Status**: Design
> **Author**: Prometheus (planning agent)
> **Date**: 2026-01-29

> **IMPORTANT — Reference Integrity**: This is a design document for a project that does not yet exist. All file paths, directory structures, and code listings describe **planned artifacts to be created**, not existing files. The only files that exist in the repository at the time of writing are this document, the project README, and the planning drafts.
>
> **File path references** throughout this document (e.g., `crates/strato_core/src/resolver.rs`) are **specifications of files to be created**, not references to existing files. No milestone or section requires reading a file that does not yet exist — instead, the design sections (1–20) serve as the **authoritative specification** that implementers follow. Milestone 0 (Project Scaffolding) creates the skeleton directory structure; all subsequent milestones build on that skeleton.
>
> **Reviewers**: Do not flag planned file paths as "missing" — they are intentionally prospective. The document is self-contained: all algorithms, data structures, and contracts are defined inline.

---

## Table of Contents

1. [Executive Summary](#1-executive-summary)
2. [Problem Statement](#2-problem-statement)
3. [Architecture Overview](#3-architecture-overview)
4. [Analysis Pipeline](#4-analysis-pipeline)
5. [Module Resolver](#5-module-resolver)
6. [Call Graph Construction](#6-call-graph-construction)
7. [Blocking Propagation Algorithm](#7-blocking-propagation-algorithm)
8. [Error Reporting Model](#8-error-reporting-model)
9. [Blocking Function Database](#9-blocking-function-database)
10. [Edge Cases: Properties and Dunder Methods](#10-edge-cases-properties-and-dunder-methods)
11. [Escape Hatch Recognition](#11-escape-hatch-recognition)
12. [Annotations API](#12-annotations-api)
13. [Configuration](#13-configuration)
14. [CLI Interface](#14-cli-interface)
15. [Output Formats](#15-output-formats)
16. [Caching Strategy](#16-caching-strategy)
17. [Distribution and Packaging](#17-distribution-and-packaging)
18. [Repository Structure](#18-repository-structure)
19. [Performance](#19-performance)
20. [Limitations and Future Work](#20-limitations-and-future-work)
21. [Implementation Plan](#21-implementation-plan)

**Appendices**
- [Appendix A: Acceptance Test Cases](#appendix-a-acceptance-test-cases)
- [Appendix B: Test Harness Specification](#appendix-b-test-harness-specification)

---

## 1. Executive Summary

**Strato** is a Rust-based static analysis tool that detects blocking function calls inside Python async contexts. Unlike existing tools (flake8-async, ruff ASYNC2XX) which only catch direct blocking calls, strato performs **full transitive call-graph analysis** — tracing through intermediary sync functions to find hidden blocking calls that would stall the event loop.

### What Makes Strato Novel

No existing tool solves this:

```python
def sync_helper():
    time.sleep(1)          # Blocking call hidden here

async def handler():
    sync_helper()          # Strato catches this. No other tool does.
```

Strato builds a project-wide call graph, propagates "blocking" status through function call chains, and reports when blocking code is reachable from async contexts — with configurable error reporting that shows diagnostics in the user's own code, not deep in third-party libraries.

### Key Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Language | Rust | Performance parity with ruff/ty |
| Parser | Ruff crates (`ruff_python_parser`, `ruff_python_ast`) | Fastest Python parser, MIT licensed |
| Analysis | Project-wide call graph + fixpoint propagation | Only way to catch transitive blocking |
| Precision | High (skip unresolvable calls) | Fewer false positives preferred |
| Distribution | `strato` (Python annotations) + `strato-cli` (Rust binary) | Zero binary footprint in production |
| Config | `pyproject.toml` `[tool.strato]` | Standard Python convention |
| Output | Text + JSON + SARIF | Human + scripting + CI integration |
| v1 Scope | asyncio only | Bounded complexity |

---

## 2. Problem Statement

### The Core Problem

In Python's asyncio, calling a blocking function (e.g., `time.sleep()`, `requests.get()`) inside an async context halts the event loop. This is catastrophic in applications that depend on a single event loop — web servers, chat applications, real-time systems.

The problem is that **it's far too easy to accidentally invoke a blocking function inside an async context**, especially through:
- Sync helper functions that internally block
- `@property` getters that perform I/O
- Dunder methods (`__str__`, `__eq__`) called implicitly
- Third-party library functions whose blocking nature isn't obvious

### Detection Cases

| Case | Python Code | Expected Result | Difficulty |
|------|-------------|-----------------|------------|
| Direct blocking in async | `async def f(): time.sleep(1)` | ERROR | Easy — existing tools handle this |
| Wrapped in executor | `await loop.run_in_executor(None, time.sleep, 1)` | OK | Easy — recognize safe pattern |
| Sync standalone | `def f(): time.sleep(1)` | OK (if never called from async) | Easy — no async context |
| **Sync called from async** | `async def f(): helper()` where `helper` blocks | **ERROR** | **Hard — requires call graph** |
| **Property blocks** | `async def f(): _ = obj.data` where `data` is blocking `@property` | **ERROR** | **Hard — property looks like attribute** |
| **Dunder blocks** | `async def f(): str(obj)` where `__str__` blocks | **ERROR** | **Hard — implicit call** |

### Why Existing Tools Fail

| Tool | Approach | Catches Direct | Catches Transitive |
|------|----------|---------------|-------------------|
| flake8-async | Pattern matching in async functions | Yes | No |
| ruff ASYNC2XX | Same as flake8-async | Yes | No |
| PyCG | Call graph (no async awareness) | N/A | N/A |
| **strato** | Call graph + async context + blocking propagation | **Yes** | **Yes** |

---

## 3. Architecture Overview

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
                          | Fixpoint: spread   |
                          | "blocking" through |
                          | call chains        |
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
│   ├── propagator       # Fixpoint blocking propagation
│   └── reporter         # Diagnostic generation + formatting
├── strato_cache         # Incremental caching system
└── strato_cli           # CLI entry point, arg parsing, output formatting

strato (Python package)
└── strato/
    ├── __init__.py      # Re-exports decorators
    ├── _annotations.py  # @blocking, @non_blocking definitions
    └── py.typed         # PEP 561 marker
```

### Key Data Structures (Overview)

These are described in detail in their respective sections. Summary:

| Structure | Purpose | Section |
|-----------|---------|---------|
| `ModuleMap` | Maps module paths to file paths | [5. Module Resolver](#5-module-resolver) |
| `SymbolTable` | Maps qualified names to definitions | [5. Module Resolver](#5-module-resolver) |
| `CallGraph` | Directed graph of function call relationships | [6. Call Graph Construction](#6-call-graph-construction) |
| `BlockingDatabase` | Registry of known blocking functions | [9. Blocking Function Database](#9-blocking-function-database) |
| `EscapeHatchRegistry` | Patterns recognized as safe executor wrapping | [11. Escape Hatch Recognition](#11-escape-hatch-recognition) |
| `DiagnosticSet` | Collection of reported issues | [8. Error Reporting Model](#8-error-reporting-model) |
| `AnalysisCache` | Serialized per-file results for incremental analysis | [16. Caching Strategy](#16-caching-strategy) |

### Public API Contract (`strato_core`)

The `strato_core` crate exposes a public API used by `strato_cli` and by integration tests. This is the contract that the test harness (Appendix B) and the CLI (Section 14) depend on:

```rust
// strato_core/src/lib.rs — public API

/// Top-level entry point: run the full analysis pipeline (Phases 1–7).
pub fn analyze(project_path: &Path, config: &Config) -> Result<AnalysisResult, AnalysisError> {
    // 1. Discovery
    // 2. Parse (parallel via rayon)
    // 3. Resolve
    // 4. Build call graph
    // 5. Annotate
    // 6. Propagate
    // 7. Report
}

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

impl Config {
    /// Load from a pyproject.toml path. Falls back to defaults for missing fields.
    pub fn from_pyproject(path: &Path) -> Result<Self, ConfigError>;

    /// All-defaults config (no pyproject.toml).
    pub fn default() -> Self;
}

/// Result of a complete analysis run.
pub struct AnalysisResult {
    /// All diagnostics found, sorted per Deterministic Output Rules (Section 8).
    pub diagnostics: Vec<Diagnostic>,
    /// Analysis statistics.
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
    pub cache_hits: usize,     // Files reused from cache
    pub cache_misses: usize,   // Files re-parsed
}

/// Errors that can occur during analysis.
pub enum AnalysisError {
    /// Configuration is invalid (exit code 2)
    ConfigError(ConfigError),
    /// IO error reading files
    IoError(std::io::Error),
    /// All files failed to parse (exit code 3)
    AllParsesFailed,
}
```

**Note**: `Diagnostic`, `Location`, `ChainLink`, `InterventionStrategy`, and `BlockingStatus` are defined in their respective sections (Section 8, Section 6) and re-exported from `strato_core`.

### Test Helper Functions (for integration tests)

The integration test harness (Appendix B) and the output/performance tests use these helper functions. They are defined in `tests/integration/harness.rs`:

```rust
// tests/integration/harness.rs — test helper functions

use strato_core::{Config, AnalysisResult, analyze};
use std::path::Path;
use std::process::Command;

/// Run strato analysis using the library API (for unit-style integration tests).
pub fn run_fixture(fixture_name: &str) {
    // ... (see Appendix B for full implementation)
}

/// Run the strato CLI binary and capture stdout (for output format tests).
/// Note: env!("CARGO_BIN_EXE_strato") resolves to the binary named "strato"
/// defined in crates/strato_cli/Cargo.toml's [[bin]] section.
pub fn run_strato_check_with_format(fixture_path: &str, format: &str) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_strato"))
        .args(["check", fixture_path, "--format", format])
        .output()
        .expect("Failed to run strato binary");
    String::from_utf8(output.stdout).expect("Invalid UTF-8")
}

/// Run the strato CLI binary and return its exit code.
pub fn run_strato_exit_code(fixture_path: &str) -> i32 {
    let status = Command::new(env!("CARGO_BIN_EXE_strato"))
        .args(["check", fixture_path])
        .status()
        .expect("Failed to run strato binary");
    status.code().unwrap_or(-1)
}

/// Delete cache directory for a fixture (for performance tests).
pub fn clear_cache(fixture_path: &str) {
    let cache_dir = Path::new(fixture_path).join(".strato_cache");
    if cache_dir.exists() {
        std::fs::remove_dir_all(&cache_dir).ok();
    }
}

/// Run analysis via library API and return result (for performance tests).
pub fn run_strato_check(fixture_path: &str) -> Result<AnalysisResult, strato_core::AnalysisError> {
    let config = Config::default();
    analyze(Path::new(fixture_path), &config)
}
```

---

## 4. Analysis Pipeline

### Phase 1: Discovery

**Purpose**: Find all Python files to analyze and load configuration.

**Inputs**: CLI arguments (paths, config file overrides)

**Process**:
1. Locate `pyproject.toml` — walk up from target directory until found.
2. Parse `[tool.strato]` section for configuration.
3. Determine first-party source roots (see [Section 13: Configuration](#13-configuration) for auto-detection logic).
4. Discover all `.py` and `.pyi` files within source roots via parallel filesystem walk.
5. Build file manifest: `Vec<SourceFile>` with path, content hash (SHA-256), and classification (first-party vs. third-party).

**Outputs**: `ProjectManifest` containing file list, config, and source root boundaries.

**Caching interaction**: Compare content hashes against cache. Files with unchanged hashes can skip phases 2-4 and reuse cached per-file results.

---

### Phase 2: Parse

**Purpose**: Parse all Python files into ASTs.

**Inputs**: `ProjectManifest` from Phase 1

**Process**:
1. For each file in the manifest, call `ruff_python_parser::parse_module(source)`.
2. Parsing is **embarrassingly parallel** — use `rayon` thread pool.
3. Collect parse errors (invalid syntax) as non-fatal diagnostics. Continue analysis on parseable files.
4. For each parsed file, extract a `FileSymbols` structure:
   - All function/method definitions (with `is_async` flag)
   - All class definitions (for method resolution)
   - All import statements
   - All decorator applications (looking for `@blocking`, `@non_blocking`, `@property`)

**Outputs**: `Vec<ParsedFile>` containing ASTs and per-file symbol summaries.

**Performance**: This is the easiest phase to parallelize and should approach ruff-level speed since it uses the same parser.

**Abstraction layer**: The parser is accessed through a `trait PythonParser`:
```rust
trait PythonParser {
    fn parse_module(&self, source: &str) -> Result<ParsedModule, ParseError>;
}

struct RuffParser;
impl PythonParser for RuffParser {
    fn parse_module(&self, source: &str) -> Result<ParsedModule, ParseError> {
        // Thin wrapper around ruff_python_parser::parse_module
    }
}
```
This trait exists solely to isolate strato from ruff API changes. If ruff breaks, only the `RuffParser` implementation needs updating.

---

### Phase 3: Resolve

**Purpose**: Map import statements to source files. Build a cross-file symbol table.

**Inputs**: `Vec<ParsedFile>` from Phase 2, `ProjectManifest` from Phase 1

**Process**: See [Section 5: Module Resolver](#5-module-resolver) for full algorithm.

**Outputs**: `ModuleMap` (import paths → file paths) and `SymbolTable` (qualified names → definitions).

---

### Phase 4: Build

**Purpose**: Construct the project-wide call graph.

**Inputs**: `Vec<ParsedFile>` from Phase 2, `SymbolTable` from Phase 3

**Process**: See [Section 6: Call Graph Construction](#6-call-graph-construction) for full algorithm.

**Outputs**: `CallGraph` — a directed graph where nodes are functions/methods/properties and edges are call relationships.

---

### Phase 5: Annotate

**Purpose**: Mark nodes in the call graph as "blocking" based on the database and decorators.

**Inputs**: `CallGraph` from Phase 4, `BlockingDatabase`, `ParsedFile` ASTs

**Process**:
1. Walk the blocking function database. For each entry, find the corresponding node in the call graph and mark it as `BlockingStatus::KnownBlocking`.
2. Walk all function definitions looking for `@blocking` and `@non_blocking` decorators. Mark corresponding nodes.
3. For `.pyi` stub files: only check for `@blocking`/`@non_blocking` decorators (no body analysis).
4. All other nodes default to `BlockingStatus::Unknown`.

**Outputs**: Annotated `CallGraph` with initial blocking/non-blocking/unknown status on each node.

---

### Phase 6: Propagate

**Purpose**: Spread "blocking" status through the call graph to fixed point.

**Inputs**: Annotated `CallGraph` from Phase 5

**Process**: See [Section 7: Blocking Propagation Algorithm](#7-blocking-propagation-algorithm) for full algorithm.

**Outputs**: Fully propagated `CallGraph`. After propagation, each node has one of the following states:
- `KnownBlocking`: From database or `@blocking` annotation. **Triggers diagnostics.**
- `PropagatedBlocking`: Transitively calls a blocking function (not through executor). **Triggers diagnostics.**
- `KnownNonBlocking`: From `@non_blocking` annotation. **Never triggers diagnostics**, even if callees are blocking.
- `Unknown`: Could not be resolved or has no blocking callees. **Does NOT trigger diagnostics.** Unknown is NOT equivalent to NotBlocking — it means "we don't know." This is the high-precision design: we only report what we can prove.

**Critical semantic**: `Unknown` nodes are **not** reclassified to `NotBlocking` after propagation. They remain `Unknown` indefinitely. The reporting phase (Phase 7) only generates diagnostics for nodes with `KnownBlocking` or `PropagatedBlocking` status that are reachable from async contexts.

---

### Phase 7: Report

**Purpose**: Find async functions that can reach blocking code. Generate diagnostics.

**Inputs**: Propagated `CallGraph` from Phase 6, `ProjectManifest` configuration

**Process**: See [Section 8: Error Reporting Model](#8-error-reporting-model) for full algorithm.

**Outputs**: `DiagnosticSet` formatted as Text, JSON, or SARIF.

---

## 5. Module Resolver

### Scope

The module resolver maps Python import statements to source files. It is the **highest-risk component** in strato and must be built and tested as an isolated, separately-testable subsystem.

### Supported Import Forms (v1)

| Import Form | Example | Supported |
|-------------|---------|-----------|
| Absolute import | `import myapp.utils` | Yes |
| From-import | `from myapp.utils import helper` | Yes |
| Relative import | `from . import sibling` | Yes |
| Relative from-import | `from ..utils import helper` | Yes |
| `__init__.py` packages | `from myapp import subpackage` | Yes |
| Multi-level | `from myapp.sub.deep import func` | Yes |

### Unsupported Import Forms (v1)

| Import Form | Example | Why |
|-------------|---------|-----|
| Star imports | `from myapp.utils import *` | Cannot determine which names are exported without deep analysis of `__all__`. Treat as unresolvable. |
| Conditional imports | `try: import ujson as json except: import json` | **Partially supported** (best-effort): strato takes the **first branch** of `try/except` import blocks and ignores subsequent branches. No diagnostic is emitted for the heuristic. This covers the common pattern of "prefer fast library, fall back to stdlib." If the first branch's import is unresolvable (e.g., `ujson` not installed), the import is treated as unresolvable. |
| Dynamic imports | `importlib.import_module(name)` | Runtime-determined. Unresolvable. |
| Namespace packages | PEP 420 (no `__init__.py`) | Adds complexity. Require explicit `__init__.py`. |
| `.pth` files | `site-packages/*.pth` | Exotic path manipulation. Out of scope. |
| Import hooks | `sys.meta_path` / `sys.path_hooks` | Runtime modification. Unresolvable. |

### Resolution Algorithm

```
FUNCTION resolve_import(import_stmt, current_file, source_roots) -> Option<ResolvedModule>:

  1. Determine import kind:
     - If relative (has leading dots): resolve relative to current file's package
     - If absolute: resolve against source roots

  2. For RELATIVE imports:
     a. Count leading dots to determine parent level
     b. Compute base package path from current file's location
     c. Navigate up `dot_count - 1` directories from base
     d. Append the imported module path
     e. Look up the resulting path (see step 4)

  3. For ABSOLUTE imports:
     a. For each source root (in order):
        - Try to find the module at source_root / module_path
        - If found, return it
     b. If not found in any source root:
        - Check the blocking database for known third-party stubs
        - If found in stubs, return stub entry
        - Otherwise, mark as unresolvable

  4. Module path lookup (used by both relative and absolute):
     Given a dotted path like "myapp.utils.helper":
     a. Try as file: {base}/myapp/utils/helper.py
     b. Try as package: {base}/myapp/utils/helper/__init__.py
     c. Try as attribute of file: {base}/myapp/utils.py (then look for "helper" symbol in that file)
     d. Try as .pyi stub: {base}/myapp/utils/helper.pyi
     e. If none found: unresolvable
```

### Source Root Auto-Detection

Strato determines first-party source roots from `pyproject.toml`:

```
FUNCTION detect_source_roots(pyproject_path) -> Vec<Path>:

  1. Read pyproject.toml

  2. Check [tool.strato] for explicit src_roots:
     - If present: use those paths (relative to pyproject.toml)
     - If present and empty: error (misconfiguration)

  3. If not configured, auto-detect:
     a. Check [tool.setuptools.packages.find] for "where" key
     b. Check for common layouts:
        - src/ layout: if {project}/src/ exists and has .py files → ["src"]
        - flat layout: if {project}/*.py or {project}/{package_name}/ → ["."]
     c. Fallback: use project root ["."]

  4. Validate: each source root must exist and contain at least one .py file
```

### First-Party vs Third-Party Classification

A file is **first-party** if its path is under any configured source root. Everything else is **third-party**.

```
FUNCTION is_first_party(file_path, source_roots) -> bool:
  return source_roots.any(|root| file_path.starts_with(root))
```

This classification is used by:
- The error reporting model (intervention point selection)
- The blocking database (first-principles analysis applies to first-party; stubs for third-party)
- Cache invalidation (only first-party files trigger re-analysis)

### Data Structures

```rust
/// Maps Python module paths to filesystem locations
struct ModuleMap {
    /// "myapp.utils" → "/project/src/myapp/utils.py"
    modules: HashMap<ModulePath, ResolvedModule>,
}

struct ResolvedModule {
    file_path: PathBuf,
    kind: ModuleKind, // File, Package (__init__.py), Stub (.pyi)
}

/// Maps fully qualified names to their definitions (project-wide)
struct SymbolTable {
    /// "myapp.utils.helper" → FunctionDef { is_async: false, line: 42, ... }
    symbols: HashMap<QualifiedName, SymbolDef>,
}

enum SymbolDef {
    Function { is_async: bool, location: Location },
    Class { methods: Vec<QualifiedName>, location: Location },
    Variable { location: Location },
    Unresolved,
}
```

### Scope-Level Binding Model (for Type Inference)

The `SymbolTable` above maps **qualified names to definitions** (project-wide). But within a function body, the call graph builder needs to resolve **local variable bindings** to infer types for method/property/dunder resolution. This is handled by a separate, per-function `ScopeBindings` structure:

```rust
/// Per-function local bindings — used by infer_simple_type() during call graph construction.
/// Built by scanning statements in the function body top-to-bottom before visiting call expressions.
struct ScopeBindings {
    /// Local variable name → what we know about its type
    locals: HashMap<String, LocalBinding>,
    /// The enclosing class (if this function is a method)
    enclosing_class: Option<QualifiedName>,
}

enum LocalBinding {
    /// Variable assigned from a constructor call: `x = MyClass()`
    /// We know x's type is MyClass.
    ClassInstance { class_name: QualifiedName },

    /// Variable assigned from an import: `from myapp.utils import helper`
    /// or: `import myapp.utils as utils`
    /// We know the qualified name of the imported symbol.
    Import { qualified_name: QualifiedName },

    /// Variable assigned from a known function return — NOT tracked in v1.
    /// This is the "no full type inference" boundary.
    /// Example: `x = get_loader()` — we don't know x's type.
    Unknown,
}
```

**How `ScopeBindings` is built** (during Phase 4, before visiting call expressions):

```
FUNCTION build_scope_bindings(func_def, symbol_table, enclosing_class) -> ScopeBindings:

  bindings = ScopeBindings::new(enclosing_class)

  FOR each statement in func_def.body:
    MATCH statement:
      // x = MyClass()
      Assign(target=Name(var), value=Call(func=Name(class_name))):
        IF symbol_table.is_class(class_name):
          bindings.locals[var] = ClassInstance { class_name: resolve(class_name) }

      // x = MyClass(args...)
      Assign(target=Name(var), value=Call(func=Attribute(...))):
        // Try to resolve — if it's a known class, bind it
        resolved = symbol_table.resolve(call.func)
        IF resolved is Some(class_def):
          bindings.locals[var] = ClassInstance { class_name: resolved }

      // from X import Y — already handled by import resolution; look up in SymbolTable
      // import X as alias — already handled by import resolution

      // All other assignments → Unknown
      Assign(target=Name(var), _):
        bindings.locals[var] = Unknown

  RETURN bindings
```

**Connection to `infer_simple_type()`**: The pseudocode in Section 6 uses `symbol_table.lookup(name)` which returns `ClassInstance`/`Import` — this actually refers to `scope_bindings.locals[name]`, NOT the project-wide `SymbolTable`. To clarify, here is the corrected resolution chain:

```
FUNCTION infer_simple_type(expr, scope_bindings, symbol_table) -> Option<QualifiedName>:

  MATCH expr:
    Name("self"):
      RETURN scope_bindings.enclosing_class

    Name("cls"):
      RETURN scope_bindings.enclosing_class

    Name(name):
      // First check local bindings (ScopeBindings)
      MATCH scope_bindings.locals.get(name):
        Some(ClassInstance { class_name }):
          RETURN Some(class_name)
        Some(Import { qualified_name }):
          RETURN Some(qualified_name)
        Some(Unknown) | None:
          // Fall back to project-wide symbol table (for top-level names/imports)
          MATCH symbol_table.resolve_in_scope(name, current_file):
            Some(SymbolDef::Class { .. }) => RETURN Some(qualified_name_of_class)
            Some(SymbolDef::Function { .. }) => RETURN None  // function, not a type
            _ => RETURN None

    // Constructor call: MyClass()
    Call(func=Name(class_name)) where symbol_table.is_class(class_name):
      RETURN Some(resolve(class_name))

    _:
      RETURN None
```

**Key invariant**: `ScopeBindings` is **always** built per-function before any call edge visitors run. It is a single top-to-bottom pass over statements — no fixpoint, no iteration, no cross-function analysis. This keeps it simple and fast.

---

## 6. Call Graph Construction

### Overview

The call graph is a directed graph where:
- **Nodes** represent callable entities (functions, methods, properties, dunder methods)
- **Edges** represent call relationships (direct call, property access, implicit dunder invocation)

### Node Types

```rust
enum CallableKind {
    Function,           // def foo(): ...
    AsyncFunction,      // async def foo(): ...
    Method,             // def method(self): ... (inside class)
    AsyncMethod,        // async def method(self): ...
    Property,           // @property def prop(self): ...
    ClassMethod,        // @classmethod def cm(cls): ...
    StaticMethod,       // @staticmethod def sm(): ...
    Lambda,             // lambda x: ...
    DunderMethod,       // __init__, __str__, etc.
}

struct CallGraphNode {
    id: NodeId,
    qualified_name: QualifiedName,     // "myapp.utils.helper"
    kind: CallableKind,
    is_async: bool,
    location: Option<Location>,        // None for phantom nodes (external symbols)
    blocking_status: BlockingStatus,   // Set during Annotate phase
}

enum BlockingStatus {
    KnownBlocking,       // From database or @blocking → TRIGGERS diagnostics
    KnownNonBlocking,    // From @non_blocking → NEVER triggers diagnostics
    PropagatedBlocking,  // Determined via propagation → TRIGGERS diagnostics
    Unknown,             // Not resolved or no blocking callees → DOES NOT trigger diagnostics
                         // Unknown stays Unknown forever — it is NOT reclassified to NotBlocking.
                         // This preserves high-precision: only proven blocking triggers errors.
}
```

### Edge Types

```rust
enum CallEdgeKind {
    DirectCall,         // foo()
    MethodCall,         // obj.method()
    PropertyAccess,     // obj.prop (triggers @property getter)
    ImplicitDunder,     // str(obj) → obj.__str__()
    SuperCall,          // super().method()
    DecoratorCall,      // @decorator applied to function
}

struct CallEdge {
    from: NodeId,       // Caller
    to: NodeId,         // Callee
    kind: CallEdgeKind,
    location: Location, // Where the call happens
    in_executor: bool,  // True if wrapped in run_in_executor/to_thread
}
```

### Construction Algorithm

```
FUNCTION build_call_graph(parsed_files, symbol_table) -> CallGraph:

  graph = new CallGraph()

  // Phase A: Register all callable nodes
  FOR each file in parsed_files:
    FOR each function/method/class in file.definitions:
      node = create_node(definition, file)
      graph.add_node(node)

  // Phase B: Walk function bodies to find call edges
  FOR each file in parsed_files:
    FOR each function_def in file:
      visitor = CallEdgeVisitor::new(function_def, symbol_table, graph)
      visitor.walk(function_def.body)

  RETURN graph
```

### Call Edge Visitor (The Core AST Walker)

The `CallEdgeVisitor` traverses each function body and identifies call edges:

```
CLASS CallEdgeVisitor:
  current_function: NodeId
  symbol_table: &SymbolTable
  graph: &mut CallGraph
  in_executor_context: bool  // Track if we're inside run_in_executor args

  // Direct function call: foo()
  VISIT ExprCall(call):
    callee = resolve_callee(call.func)
    IF callee is Some:
      edge_kind = determine_edge_kind(call)
      graph.add_edge(current_function, callee, edge_kind, in_executor=in_executor_context)

    // Check if this is an executor call — mark the callable argument as protected
    IF is_executor_call(call):
      // EXECUTOR SCOPE RULE:
      // In `loop.run_in_executor(executor, func, arg1, arg2)`:
      //   - arg[0] (executor): NOT protected — it's the executor instance, not offloaded work
      //   - arg[1] (func): PROTECTED — this is the callable that runs in the thread pool
      //   - arg[2..] (arg1, arg2, ...): NOT protected — these are plain data arguments passed
      //     to func; they are NOT callables being offloaded
      // In `asyncio.to_thread(func, arg1, arg2)`:
      //   - arg[0] (func): PROTECTED — the callable offloaded to a thread
      //   - arg[1..] (arg1, arg2, ...): NOT protected — plain data arguments
      //
      // "Protected" means: any call edges originating from resolving that argument
      // are marked with `in_executor: true`, which suppresses blocking propagation.
      // Only the CALLABLE ARGUMENT (the function being offloaded) gets this treatment.
      // The remaining positional arguments are data, not callables, and are visited normally.
      protected_arg_index = get_executor_callable_arg_position(call)
      FOR i, arg in enumerate(call.args):
        IF i == protected_arg_index:
          in_executor_context = true
          visit(arg)
          in_executor_context = false
        ELSE:
          visit(arg)
    ELSE:
      visit(call.args)

  // Attribute access: obj.attr (might be @property)
  VISIT ExprAttribute(attr):
    IF symbol_table.is_property(attr.value_type, attr.attr_name):
      callee = symbol_table.resolve_property(attr.value_type, attr.attr_name)
      IF callee is Some:
        graph.add_edge(current_function, callee, PropertyAccess)

  // Implicit dunder calls (see Section 10 for full mapping)
  VISIT ExprBinOp(binop):    // a + b → a.__add__(b)
    resolve_dunder_call(binop.left_type, dunder_for_op(binop.op))

  VISIT ExprCompare(cmp):    // a == b → a.__eq__(b)
    resolve_dunder_call(cmp.left_type, dunder_for_cmp(cmp.op))

  VISIT ExprCall where func is builtin:  // str(x) → x.__str__()
    IF is_builtin_that_invokes_dunder(call):
      dunder = builtin_to_dunder(call.func_name)
      resolve_dunder_call(call.args[0].type, dunder)
```

### Callee Resolution

Resolving what function a call expression refers to:

```
FUNCTION resolve_callee(expr) -> Option<NodeId>:

  MATCH expr:
    // Simple name: foo()
    Name(name):
      qualified = symbol_table.resolve_name(name, current_scope)
      RETURN graph.find_node(qualified)

    // Attribute: obj.method()
    Attribute(value, attr):
      // Try to resolve the type of `value`
      value_type = infer_simple_type(value)
      IF value_type is Some:
        qualified = value_type + "." + attr
        RETURN graph.find_node(qualified)
      ELSE:
        RETURN None  // Unresolvable — skip silently (high precision)

    // Subscript, starred, etc.:
    _:
      RETURN None  // Unresolvable
```

### Simple Type Inference (NOT Full Type Inference)

Strato does **not** implement full type inference. It performs **syntactic type resolution** — a minimal, best-effort approach:

```
FUNCTION infer_simple_type(expr) -> Option<QualifiedName>:

  MATCH expr:
    // Direct name with known type
    Name(name):
      binding = symbol_table.lookup(name)
      IF binding is ClassInstance(class_name):
        RETURN Some(class_name)
      IF binding is Import(qualified_name):
        RETURN Some(qualified_name)
      RETURN None

    // Constructor call: MyClass()
    Call(func=Name(class_name)) where is_class(class_name):
      RETURN Some(class_name)

    // self/cls in methods
    Name("self"):
      RETURN Some(current_class)

    Name("cls"):
      RETURN Some(current_class)

    // Everything else
    _:
      RETURN None
```

This catches the most common cases:
- `self.method()` — resolves via class hierarchy
- `MyClass().method()` — resolves via constructor
- `imported_module.function()` — resolves via import table

It intentionally skips complex cases (return values of arbitrary functions, conditional types, etc.) in favor of precision.

### External Symbol Modeling (stdlib / third-party)

A critical design question: how do calls to `time.sleep()`, `requests.get()`, etc. become resolvable call-graph nodes when their source files are **not** in the project's source roots?

**Answer**: Strato uses **phantom nodes** — pre-seeded call-graph nodes for every entry in the blocking database. These nodes have no source location but carry blocking status.

```
PHASE 4 INITIALIZATION (before visiting function bodies):

  // Step 0: Pre-seed phantom nodes for ALL blocking database entries
  FOR each entry in blocking_database.entries:
    phantom = CallGraphNode {
      id: generate_id(),
      qualified_name: entry.qualified_name,  // e.g., "time.sleep"
      kind: Function,
      is_async: false,
      location: None,                         // No source location — phantom node
      blocking_status: KnownBlocking,
    }
    graph.add_node(phantom)

  // Now proceed with Phase A (register first-party callable nodes)
  // and Phase B (walk bodies to find edges)
```

**How imports of external symbols create edges**:

When the call graph builder encounters `time.sleep(1)` in a function body:

1. `resolve_callee(Name("time").Attribute("sleep"))` is called
2. The symbol table looks up `time` → finds it's an import (`import time`)
3. The qualified name `time.sleep` is constructed
4. `graph.find_node("time.sleep")` finds the **phantom node** pre-seeded from the blocking DB
5. An edge is created from the calling function to the phantom node

**What if an import resolves to something NOT in the blocking database?**
- `import json; json.dumps(data)` → `json.dumps` is not in the DB → no phantom node → `graph.find_node("json.dumps")` returns `None` → call is treated as unresolvable → skipped silently (high precision rule)

**Import binding rules for external modules**:

| Import Form | `SymbolTable` / `ScopeBindings` Effect | Resolution |
|-------------|---------------------------------------|------------|
| `import time` | `ScopeBindings: time → Import { qualified_name: "time" }` | `time.sleep` → resolves to `"time.sleep"` |
| `from time import sleep` | `ScopeBindings: sleep → Import { qualified_name: "time.sleep" }` | `sleep()` → resolves to `"time.sleep"` |
| `import requests` | `ScopeBindings: requests → Import { qualified_name: "requests" }` | `requests.get()` → resolves to `"requests.get"` |
| `from requests import get` | `ScopeBindings: get → Import { qualified_name: "requests.get" }` | `get()` → resolves to `"requests.get"` |
| `import requests as r` | `ScopeBindings: r → Import { qualified_name: "requests" }` | `r.get()` → resolves to `"requests.get"` |

**Key invariant**: External (third-party/stdlib) symbols only become graph nodes if they appear in the blocking database. All other external calls are invisible to the analysis. This is by design — strato only tracks blocking, not all calls.

### Qualified Name Conventions

A `QualifiedName` is a dot-separated string uniquely identifying a callable. The derivation rules are:

| Entity | Convention | Example |
|--------|-----------|---------|
| Top-level function | `{module_path}.{function_name}` | `myapp.utils.helper` |
| Class | `{module_path}.{class_name}` | `myapp.models.User` |
| Instance method | `{module_path}.{class_name}.{method_name}` | `myapp.models.User.save` |
| Class method | `{module_path}.{class_name}.{method_name}` | `myapp.models.User.from_dict` |
| Static method | `{module_path}.{class_name}.{method_name}` | `myapp.models.User.validate` |
| Property getter | `{module_path}.{class_name}.{property_name}` | `myapp.models.User.full_name` |
| Dunder method | `{module_path}.{class_name}.{dunder_name}` | `myapp.models.User.__str__` |
| Lambda | `{module_path}.{enclosing_func}.<lambda:{line}>` | `myapp.utils.process.<lambda:42>` |
| Nested function | `{module_path}.{enclosing_func}.{nested_name}` | `myapp.utils.outer.inner` |
| External (phantom) | `{module_path}.{function_name}` | `time.sleep`, `requests.get` |

**Module path derivation from file path**:

```
FUNCTION derive_module_path(file_path: &Path, source_roots: &[Path]) -> ModulePath:

  // Find the source root that contains this file
  root = source_roots.find(|r| file_path.starts_with(r))
  IF root is None:
    RETURN file_path_to_dotted(file_path)  // Fallback for unrooted files

  // Strip source root prefix
  relative = file_path.strip_prefix(root)

  // Convert path separators to dots, strip .py/.pyi extension
  // "myapp/utils/helper.py" → "myapp.utils.helper"
  // "myapp/utils/__init__.py" → "myapp.utils"
  parts = relative.components()
  IF parts.last() == "__init__.py":
    parts = parts[..parts.len()-1]  // Drop __init__.py
  ELSE:
    parts.last_mut().strip_suffix(".py").strip_suffix(".pyi")

  RETURN parts.join(".")
```

**Lambda naming**: Lambdas get a synthetic name based on their source line number (e.g., `<lambda:42>`). This is deterministic given the file content but NOT guaranteed unique if multiple lambdas appear on the same line. In that case, append a 0-based index: `<lambda:42:0>`, `<lambda:42:1>`.

---

## 7. Blocking Propagation Algorithm

### Overview

After the call graph is constructed and initial blocking annotations are applied (Phase 5), the propagation phase spreads "blocking" status through the graph. If function `A` calls function `B`, and `B` is blocking, then `A` is also blocking (unless the call is wrapped in an executor).

### The Fixpoint Problem

Naive iterative propagation has a problem: **cycles in the call graph** (mutual recursion):

```python
def foo():
    bar()

def bar():
    foo()    # Cycle! Does foo block? Only if bar blocks, but bar blocks only if foo blocks...
```

Solution: **Strongly Connected Component (SCC) decomposition** followed by **topological propagation**.

### Algorithm: SCC-Based Blocking Propagation

```
FUNCTION propagate_blocking(graph: &mut CallGraph):

  // Step 1: Decompose into Strongly Connected Components
  // Using Tarjan's algorithm: O(V + E)
  sccs = tarjan_scc(graph)

  // Step 2: Build condensation graph (DAG of SCCs)
  // Each SCC becomes a single node. Edges between SCCs are AGGREGATED:
  //
  // SCC-to-SCC EDGE AGGREGATION RULE:
  // When multiple individual call edges exist between functions in SCC_A and SCC_B,
  // the condensed edge is marked `all_calls_in_executor = true` ONLY IF every
  // individual edge from any function in SCC_A to any function in SCC_B has
  // `in_executor = true`. If even ONE edge is NOT in an executor, the condensed
  // edge has `all_calls_in_executor = false`, meaning blocking WILL propagate.
  //
  // Formally: condensed_edge.all_calls_in_executor =
  //   individual_edges.iter().all(|e| e.in_executor)
  //
  condensation = build_condensation(graph, sccs)

  // Step 3: Topological sort of condensation (reverse post-order)
  topo_order = topological_sort(condensation)

  // Step 4: Propagate in topological order (leaves first)
  FOR each scc_node in topo_order (bottom-up):

    // Step 4a: Check if entire SCC is shielded by @non_blocking
    // NON_BLOCKING RULE (SCC level):
    // If ANY function in the SCC is KnownNonBlocking, the entire SCC is treated
    // as non-blocking. Rationale: @non_blocking is a user assertion that this
    // code is safe. Since SCC members are mutually recursive, one @non_blocking
    // member shields the cycle.
    scc_has_non_blocking = false
    FOR each func in scc_node.functions:
      IF func.blocking_status == KnownNonBlocking:
        scc_has_non_blocking = true
        BREAK

    IF scc_has_non_blocking:
      scc_node.is_blocking = false
      CONTINUE  // Skip to next SCC — do not propagate blocking through this SCC

    // Step 4b: Check if any function in this SCC is directly blocking
    scc_is_blocking = false
    FOR each func in scc_node.functions:
      IF func.blocking_status == KnownBlocking:
        scc_is_blocking = true
        BREAK

    // Step 4c: Check if any callee SCC (already processed) is blocking
    IF NOT scc_is_blocking:
      FOR each outgoing_edge in condensation.edges_from(scc_node):
        callee_scc = outgoing_edge.target

        // Skip edges that go through executors (all calls via executor)
        IF outgoing_edge.all_calls_in_executor:
          CONTINUE

        IF callee_scc.is_blocking:
          scc_is_blocking = true
          BREAK

    // Step 4d: Mark all functions in this SCC
    IF scc_is_blocking:
      scc_node.is_blocking = true
      FOR each func in scc_node.functions:
        IF func.blocking_status == Unknown:
          func.blocking_status = PropagatedBlocking

          // Record the propagation path for error reporting
          func.blocking_reason = trace_blocking_path(func, graph)
```

### Complexity Analysis

| Step | Algorithm | Complexity |
|------|-----------|------------|
| SCC decomposition | Tarjan's | O(V + E) |
| Condensation | Graph contraction | O(V + E) |
| Topological sort | Kahn's/DFS | O(V + E) |
| Propagation | Single pass over DAG | O(V + E) |
| **Total** | **Single pass** | **O(V + E)** |

Where V = number of functions, E = number of call edges.

**This is linear time.** There is no iterative fixpoint — the SCC decomposition eliminates cycles, and the topological ordering ensures each node is processed exactly once. This is critical for performance.

### The `@non_blocking` Override

If a function is annotated with `@non_blocking`, it is treated as definitively non-blocking regardless of what it calls:

```python
@non_blocking
def looks_scary():
    # User asserts this is safe, even though it calls blocking code.
    # Maybe it's behind a conditional, or the user knows it's wrapped.
    some_complex_pattern()
```

During propagation, `@non_blocking` nodes are never marked as blocking, and blocking status does not propagate through them.

### Edge Handling: Executor Calls

Edges marked with `in_executor: true` do not propagate blocking status. The whole purpose of `run_in_executor` is to offload blocking work to a thread pool.

```
IF edge.in_executor:
  // This call is safe — the blocking function runs in a thread, not the event loop
  SKIP propagation through this edge
```

### Blocking Path Tracing

For error reporting, we need to know *how* a function became blocking — the chain from the async context to the ultimate blocking call. This is stored during propagation:

```rust
struct BlockingReason {
    /// The ultimate blocking call (e.g., time.sleep)
    root_cause: NodeId,
    /// The call chain as (caller, call_site, callee) tuples.
    /// Each entry records: which function calls which, and WHERE in the source
    /// code the call happens.
    ///
    /// Example for: async handler() → helper() → time.sleep()
    ///   chain_links = [
    ///     ChainLink { function: handler, call_site: handler.py:5:4, callee: helper },
    ///     ChainLink { function: helper,  call_site: helper.py:3:4,  callee: time.sleep },
    ///   ]
    ///
    /// The chain always starts at the async function and ends at the blocking root.
    chain_links: Vec<ChainLink>,
}
```

```rust
struct ChainLink {
    /// The calling function's qualified name.
    function_name: QualifiedName,
    /// The calling function's DEFINITION location (where `def function_name` appears).
    /// Used for chain display (function reference). None for phantom (external) nodes.
    function_location: Option<Location>,
    /// The CALL SITE location within the calling function's body — the exact
    /// expression where the next function in the chain is invoked.
    /// This is the span that gets underlined in text output.
    /// None for phantom nodes (they have no source to point to).
    call_site_location: Option<Location>,
    /// The callee's qualified name (what is being called at the call site).
    callee_name: QualifiedName,
    /// Whether the calling function is async.
    is_async: bool,
    /// Whether the calling function is first-party.
    is_first_party: bool,
}
```

**Key distinction**: `function_location` points to where the calling function is *defined* (useful for "function X calls function Y" messages). `call_site_location` points to the exact *call expression* within that function (useful for diagnostic underlines and primary location selection).

**`primary_location` derivation**:

```
FUNCTION derive_primary_location(chain: &BlockingReason, strategy: InterventionStrategy) -> Location:

  // Apply intervention strategy to select the intervention ChainLink
  selected_link = select_intervention_link(chain.chain_links, strategy)

  // The primary location is the CALL SITE where the selected function
  // calls the next function in the chain (i.e., the expression to underline).
  RETURN selected_link.call_site_location
    .unwrap_or(selected_link.function_location.unwrap())
```

**For `first-party-deepest`**: Walk the chain from the blocking end backward; the deepest first-party link's `call_site_location` is the primary location. In A2 (`handler → helper → time.sleep`), the deepest first-party is `helper` calling `time.sleep` at `helper.py:5`, so primary location = line 5 (the `time.sleep(1)` call site inside `helper`).

**Multiple call sites between same nodes**: When function A calls function B at multiple locations, `BlockingReason` stores the **first** (smallest line, then column) call site. This is deterministic.

This chain is computed via BFS from the newly-blocked function toward any `KnownBlocking` callee, selecting the shortest path per the Blocking Reason Path Selection rules in Section 8.

---

## 8. Error Reporting Model

### Diagnostic Structure

```rust
struct Diagnostic {
    /// Unique error code (e.g., "STRATO001")
    code: ErrorCode,
    /// Severity level
    severity: Severity, // Error, Warning
    /// The "intervention point" — where the user should look
    primary_location: Location,
    /// Human-readable message
    message: String,
    /// The call chain from async context to blocking call
    blocking_chain: Vec<ChainLink>,
    /// Which intervention strategy was used
    strategy: InterventionStrategy,
    /// Static suggestion for fixing the issue (from BlockingDatabase).
    /// This is a human-readable string, NOT an autofix.
    /// Example: "Use `asyncio.sleep()` instead"
    help: Option<String>,
}

/// Source location with range information.
/// All fields are required. Ranges are inclusive on start, exclusive on end.
struct Location {
    /// File path (relative to project root, `/`-normalized)
    file: String,
    /// Start line (1-based)
    line: usize,
    /// Start column (0-based, UTF-8 byte offset within line)
    column: usize,
    /// End line (1-based). For single-expression diagnostics, equals `line`.
    end_line: usize,
    /// End column (0-based, UTF-8 byte offset). Points one past the last character.
    end_column: usize,
}

// LOCATION DERIVATION FROM RUFF AST:
//
// Ruff AST nodes provide `TextRange` (from `ruff_text_size::TextRange`, re-exported
// by `ruff_python_ast`). This is a byte-offset range from the start of the source file.
//
// Conversion to (line, column) uses these types from the ruff crates:
//   - `ruff_source_file::SourceCode` — wraps source text, provides line index
//   - `ruff_source_file::SourceLocation` — contains `row: OneIndexed, column: OneIndexed`
//   - `ruff_source_file::LineIndex` — precomputed newline positions for O(log n) lookup
//
// At the pinned rev (091d0af), the conversion is:
//
//   use ruff_source_file::{SourceCode, LineIndex};
//   use ruff_text_size::TextRange;
//
//   fn location_from_range(range: TextRange, source: &SourceCode, file: &str) -> Location {
//       let start = source.source_location(range.start());
//       let end = source.source_location(range.end());
//       Location {
//           file: file.to_string(),
//           line: start.row.get(),          // 1-based (OneIndexed)
//           column: start.column.get() - 1, // Convert 1-based to 0-based for internal use
//           end_line: end.row.get(),
//           end_column: end.column.get() - 1,
//       }
//   }
//
// NOTE: `ruff_source_file` may need to be added as a workspace dependency:
//   ruff_source_file = { git = "https://github.com/astral-sh/ruff", rev = "091d0af..." }
// Check the pinned rev's workspace members to confirm the exact crate path.
//
// WHICH AST SPAN TO USE:
// - For FUNCTION DEFINITIONS: use the `name` identifier range (not the entire def)
// - For CALL SITES: use the full `ExprCall` range (includes parens)
// - For PROPERTY ACCESS: use the `Attribute.attr` identifier range
// - For DUNDER OPERATIONS: use the operator/builtin call range
//
// COLUMN CONVENTION (end-to-end):
// - Internal (Location struct): 0-based byte offset (matches ruff)
// - Text output display: 1-based column (add 1 when formatting for humans)
// - JSON output: 0-based (matches internal, same as LSP convention)
// - SARIF output: 1-based column (SARIF spec requires 1-based)
//
// This means the text formatter and SARIF formatter add 1 to column values;
// JSON output uses internal values directly.

// ChainLink is defined in Section 7 (Blocking Propagation Algorithm).
// See that section for the full struct with function_location, call_site_location,
// callee_name, is_async, and is_first_party fields.
//
// NOTE: In JSON output, phantom node locations serialize as `"file": null, "line": null`
// (as shown in Section 15). SARIF output omits `physicalLocation` for chain links
// without source locations.
```

### Error Codes

| Code | Meaning | Severity |
|------|---------|----------|
| `STRATO001` | Direct blocking call in async function | Error |
| `STRATO002` | Indirect blocking call via sync intermediary | Error |
| `STRATO003` | Blocking `@property` accessed in async context | Error |
| `STRATO004` | Blocking dunder method invoked in async context | Error |

### Error Code Classification Algorithm

The error code is determined by inspecting the `BlockingReason.chain_links`:

```
FUNCTION classify_error_code(chain: &BlockingReason) -> ErrorCode:

  // The first link is always from the async function.
  // The last link's callee is the blocking root cause.
  first_link = chain.chain_links[0]
  last_link = chain.chain_links.last()

  // Check the edge kind of the last link to the blocking root
  last_edge_kind = graph.edge_kind(last_link.function_name, last_link.callee_name)

  // STRATO003: Property access to a blocking getter
  IF last_edge_kind == PropertyAccess:
    RETURN STRATO003

  // STRATO004: Implicit dunder call that blocks
  IF last_edge_kind == ImplicitDunder:
    RETURN STRATO004

  // STRATO001 vs STRATO002: Is the blocking call directly in an async function?
  // "Direct" means: chain has exactly 1 link AND the caller is async.
  // That means: async_func directly calls blocking_func with no intermediaries.
  IF chain.chain_links.len() == 1 AND first_link.is_async:
    RETURN STRATO001  // Direct blocking call in async function

  // Otherwise: there are intermediary sync functions between async and blocker
  RETURN STRATO002
```

**Examples**:
- A1: `async handler() → time.sleep()` → 1 link, caller is async → **STRATO001**
- A2: `async handler() → helper() → time.sleep()` → 2 links → **STRATO002**
- A8: `async handler() → loader.data [PropertyAccess] → requests.get()` → PropertyAccess edge → **STRATO003**
- A9: `async handler() → str(obj) [ImplicitDunder] → __str__() → requests.get()` → ImplicitDunder edge → **STRATO004**

### Intervention Point Strategy

The "intervention point" is the primary location shown in the diagnostic — the place in the user's code where they should make a change.

#### Strategy: `first-party-deepest` (Default)

Select the **deepest function in first-party code** on the call chain between the async context and the blocking call.

```
FUNCTION select_intervention_point(chain: &[ChainLink], strategy: Strategy) -> &ChainLink:

  MATCH strategy:
    FirstPartyDeepest:
      // Walk the chain from the blocking end toward the async end
      // Find the deepest first-party function
      FOR link in chain.iter().rev():  // reverse = from blocker toward async
        IF link.is_first_party:
          RETURN link
      // Fallback: if no first-party code on path (all third-party), use async boundary
      RETURN select_async_boundary(chain)

    AsyncBoundary:
      RETURN select_async_boundary(chain)

FUNCTION select_async_boundary(chain: &[ChainLink]) -> &ChainLink:
  // Find the transition: last async function before sync code that leads to blocking
  FOR i in 0..chain.len()-1:
    IF chain[i].is_async AND NOT chain[i+1].is_async:
      RETURN &chain[i]
  RETURN &chain[0]  // Fallback: first element
```

#### Example

```python
# src/myapp/handler.py
async def handle_request():          # [0] async, first-party
    await process()                   # [1] async, first-party

# src/myapp/processor.py
async def process():                  # [1] async, first-party
    validate(data)                    # [2] sync, first-party   <-- async-boundary

# src/myapp/validator.py
def validate(data):                   # [2] sync, first-party
    check_db(data)                    # [3] sync, first-party   <-- first-party-deepest

# src/myapp/db.py
def check_db(data):                   # [3] sync, first-party
    psycopg2.connect(...)             # [4] sync, third-party, BLOCKING
```

- **`first-party-deepest`** reports at `check_db()` in `db.py` line N — "check_db() calls psycopg2.connect() which blocks the event loop"
- **`async-boundary`** reports at `process()` calling `validate()` — "async function process() calls sync chain that leads to blocking psycopg2.connect()"

### Diagnostic Message Format

```
STRATO002: Blocking call reachable from async context

  --> src/myapp/db.py:15:5
   |
15 |     psycopg2.connect(dsn)
   |     ^^^^^^^^^^^^^^^^^^^^ blocks the event loop
   |
   = call chain: process() -> validate() -> check_db() -> psycopg2.connect()
   = help: Use `asyncpg` or wrap in `await loop.run_in_executor(None, psycopg2.connect, dsn)`
```

The `help` message is **not** an autofix — it's a static suggestion string associated with each blocking function in the database. No code generation or transformation.

### Deterministic Output Rules

For test stability and reproducible CI runs, all outputs must be deterministic. The following tie-breaking rules apply:

#### Diagnostic Ordering

When multiple diagnostics are emitted, they are sorted by this key (lexicographic, ascending):

1. **File path** (string comparison, using `/`-normalized relative paths)
2. **Line number** (numeric, ascending)
3. **Column number** (numeric, ascending)
4. **Error code** (string comparison: STRATO001 < STRATO002 < STRATO003 < STRATO004)

This produces a stable, reproducible diagnostic order regardless of internal processing order (parallel parsing, hash map iteration, etc.).

#### Blocking Reason Path Selection

When a function has **multiple paths** to different blocking roots (e.g., it calls both `time.sleep` and `requests.get`), the `BlockingReason` stores the **shortest path** (fewest call chain links). If multiple paths have the same length, select the path whose root cause (`root_cause` node) has the lexicographically smallest `qualified_name`.

```
FUNCTION select_blocking_reason(func, graph) -> BlockingReason:
  all_paths = find_all_paths_to_blocking_roots(func, graph)  // BFS from func to any KnownBlocking node

  // Sort by: (path_length ASC, root_cause.qualified_name ASC)
  all_paths.sort_by(|a, b|
    a.len().cmp(&b.len())
      .then(a.root_cause.qualified_name.cmp(&b.root_cause.qualified_name))
  )

  RETURN all_paths[0]  // Shortest path, lexicographically first root on ties
```

#### Intervention Point Tie-Breaking

When the `first-party-deepest` strategy finds **multiple first-party functions at the same depth** (same distance from the blocking root), select the one with the lexicographically smallest `qualified_name`. If still tied (same function), select the call site with the smallest `(line, column)` pair.

```
FUNCTION select_intervention_point(candidates: &[ChainLink]) -> &ChainLink:
  // candidates = all first-party links at the deepest level
  candidates.sort_by(|a, b|
    a.function_name.cmp(&b.function_name)
      .then(a.location.line.cmp(&b.location.line))
      .then(a.location.column.cmp(&b.location.column))
  )
  RETURN &candidates[0]
```

---

## 9. Blocking Function Database

### Structure

The blocking database is a registry of functions known to block the event loop. It ships with strato and is extended via configuration.

```rust
struct BlockingDatabase {
    entries: HashMap<QualifiedName, BlockingEntry>,
}

struct BlockingEntry {
    qualified_name: QualifiedName,  // e.g., "time.sleep"
    category: BlockingCategory,
    help_message: String,           // Suggestion for async alternative
    source: EntrySource,           // BuiltIn, UserConfig, Annotation
}

enum BlockingCategory {
    Sleep,          // time.sleep, etc.
    NetworkIO,      // requests.get, urllib, socket
    FileIO,         // open, os.read, os.write
    SubProcess,     // subprocess.run, subprocess.call
    DatabaseIO,     // psycopg2.connect, sqlite3.connect
    UserInput,      // input()
    Other,
}

enum EntrySource {
    BuiltIn,        // Ships with strato
    UserConfig,     // From pyproject.toml [tool.strato.blocking]
    Annotation,     // From @blocking decorator in source code
}
```

### Built-In Entries (v1)

#### Sleep

| Function | Help |
|----------|------|
| `time.sleep` | Use `asyncio.sleep()` |

#### Network I/O

| Function | Help |
|----------|------|
| `requests.get` | Use `aiohttp` or `httpx` |
| `requests.post` | Use `aiohttp` or `httpx` |
| `requests.put` | Use `aiohttp` or `httpx` |
| `requests.delete` | Use `aiohttp` or `httpx` |
| `requests.patch` | Use `aiohttp` or `httpx` |
| `requests.head` | Use `aiohttp` or `httpx` |
| `requests.options` | Use `aiohttp` or `httpx` |
| `requests.request` | Use `aiohttp` or `httpx` |
| `requests.Session.get` | Use `aiohttp.ClientSession` |
| `requests.Session.post` | Use `aiohttp.ClientSession` |
| `requests.Session.put` | Use `aiohttp.ClientSession` |
| `requests.Session.delete` | Use `aiohttp.ClientSession` |
| `requests.Session.patch` | Use `aiohttp.ClientSession` |
| `requests.Session.head` | Use `aiohttp.ClientSession` |
| `requests.Session.options` | Use `aiohttp.ClientSession` |
| `requests.Session.request` | Use `aiohttp.ClientSession` |
| `requests.Session.send` | Use `aiohttp.ClientSession` |
| `urllib.request.urlopen` | Use `aiohttp` |
| `http.client.HTTPConnection.request` | Use `aiohttp` |
| `http.client.HTTPSConnection.request` | Use `aiohttp` |
| `socket.socket.connect` | Use `asyncio` streams |
| `socket.socket.recv` | Use `asyncio` streams |
| `socket.socket.send` | Use `asyncio` streams |
| `socket.socket.accept` | Use `asyncio.start_server()` |
| `socket.socket.sendall` | Use `asyncio` streams |
| `socket.socket.recvfrom` | Use `asyncio` datagram |
| `socket.create_connection` | Use `asyncio.open_connection()` |

#### File I/O

| Function | Help |
|----------|------|
| `builtins.open` | Use `aiofiles.open()` |
| `io.open` | Use `aiofiles.open()` |
| `os.read` | Use `aiofiles` or `run_in_executor` |
| `os.write` | Use `aiofiles` or `run_in_executor` |
| `os.fdopen` | Use `aiofiles` |
| `pathlib.Path.read_text` | Use `aiofiles` |
| `pathlib.Path.read_bytes` | Use `aiofiles` |
| `pathlib.Path.write_text` | Use `aiofiles` |
| `pathlib.Path.write_bytes` | Use `aiofiles` |
| `os.listdir` | Use `run_in_executor` |
| `os.scandir` | Use `run_in_executor` |
| `os.stat` | Use `run_in_executor` |
| `os.path.exists` | Use `run_in_executor` |
| `os.path.isfile` | Use `run_in_executor` |
| `os.path.isdir` | Use `run_in_executor` |
| `glob.glob` | Use `run_in_executor` |
| `glob.iglob` | Use `run_in_executor` |
| `shutil.copy` | Use `run_in_executor` |
| `shutil.move` | Use `run_in_executor` |
| `shutil.rmtree` | Use `run_in_executor` |

#### Subprocess

| Function | Help |
|----------|------|
| `subprocess.run` | Use `asyncio.create_subprocess_exec()` |
| `subprocess.call` | Use `asyncio.create_subprocess_exec()` |
| `subprocess.check_call` | Use `asyncio.create_subprocess_exec()` |
| `subprocess.check_output` | Use `asyncio.create_subprocess_exec()` |
| `subprocess.Popen.wait` | Use `asyncio.create_subprocess_exec()` |
| `subprocess.Popen.communicate` | Use `asyncio.create_subprocess_exec()` |
| `os.system` | Use `asyncio.create_subprocess_shell()` |
| `os.popen` | Use `asyncio.create_subprocess_shell()` |

#### Database

| Function | Help |
|----------|------|
| `psycopg2.connect` | Use `asyncpg` |
| `sqlite3.connect` | Use `aiosqlite` |
| `pymysql.connect` | Use `aiomysql` |

#### User Input

| Function | Help |
|----------|------|
| `builtins.input` | Use async input library or `run_in_executor` |

### User Configuration Extension

Users can add, remove, or override entries in `pyproject.toml`:

```toml
[tool.strato.blocking]
# Add custom blocking functions
add = [
    { name = "mylib.slow_func", help = "Use mylib.async_slow_func instead", category = "other" },
    { name = "redis.Redis.get", help = "Use aioredis", category = "network-io" },
]

# Remove built-in entries (false positives for your project)
remove = [
    "builtins.open",  # Our open() is monkeypatched to be async-safe
]

# Mark entire modules as blocking
blocking_modules = [
    "legacy_module",  # Everything in this module blocks
]
```

---

## 10. Edge Cases: Properties and Dunder Methods

### Properties

A `@property` getter is called implicitly when accessing an attribute. Strato must recognize this pattern:

```python
class DataLoader:
    @property
    def data(self):
        return requests.get(self.url).json()  # Blocking!

async def handler():
    loader = DataLoader()
    result = loader.data  # This LOOKS like attribute access, but calls a blocking function
```

**Detection**: During call graph construction, when an `ExprAttribute` node is visited:
1. Look up the attribute name in the class definition of the inferred type.
2. If the attribute is defined as a `@property`, treat the access as a function call.
3. Add a `PropertyAccess` edge from the current function to the property getter.

**Challenge**: The inferred type of `loader` must be resolved. Using the simple type inference from Section 6, `DataLoader()` resolves to `DataLoader`, so `loader.data` is resolved to `DataLoader.data` property getter.

### Dunder Methods

Dunder methods are called implicitly by Python operations. Strato must map these operations to method calls:

```python
class SlowSerializer:
    def __str__(self):
        return requests.get(f"{self.url}/serialize").text  # Blocking!

async def handler():
    obj = SlowSerializer()
    print(str(obj))     # Implicitly calls obj.__str__() — blocking!
    f"{obj}"            # Also calls __str__!
    if obj == other:    # Calls __eq__
        pass
```

### Dunder Mapping Table

Operations that strato maps to dunder calls:

| Python Operation | Dunder Method | AST Node |
|------------------|---------------|----------|
| `str(x)` | `x.__str__()` | `ExprCall(func=Name("str"))` |
| `repr(x)` | `x.__repr__()` | `ExprCall(func=Name("repr"))` |
| `bool(x)` | `x.__bool__()` | `ExprCall(func=Name("bool"))` |
| `int(x)` | `x.__int__()` | `ExprCall(func=Name("int"))` |
| `float(x)` | `x.__float__()` | `ExprCall(func=Name("float"))` |
| `len(x)` | `x.__len__()` | `ExprCall(func=Name("len"))` |
| `iter(x)` | `x.__iter__()` | `ExprCall(func=Name("iter"))` |
| `next(x)` | `x.__next__()` | `ExprCall(func=Name("next"))` |
| `hash(x)` | `x.__hash__()` | `ExprCall(func=Name("hash"))` |
| `x + y` | `x.__add__(y)` | `ExprBinOp(op=Add)` |
| `x - y` | `x.__sub__(y)` | `ExprBinOp(op=Sub)` |
| `x * y` | `x.__mul__(y)` | `ExprBinOp(op=Mult)` |
| `x / y` | `x.__truediv__(y)` | `ExprBinOp(op=Div)` |
| `x == y` | `x.__eq__(y)` | `ExprCompare(op=Eq)` |
| `x != y` | `x.__ne__(y)` | `ExprCompare(op=NotEq)` |
| `x < y` | `x.__lt__(y)` | `ExprCompare(op=Lt)` |
| `x > y` | `x.__gt__(y)` | `ExprCompare(op=Gt)` |
| `x <= y` | `x.__le__(y)` | `ExprCompare(op=LtE)` |
| `x >= y` | `x.__ge__(y)` | `ExprCompare(op=GtE)` |
| `x[k]` | `x.__getitem__(k)` | `ExprSubscript` |
| `x[k] = v` | `x.__setitem__(k, v)` | `StmtAssign(targets=[Subscript])` |
| `del x[k]` | `x.__delitem__(k)` | `StmtDelete(targets=[Subscript])` |
| `k in x` | `x.__contains__(k)` | `ExprCompare(op=In)` |
| `x(...)` | `x.__call__(...)` | `ExprCall` (when x is not a known function) |
| `f"{x}"` / `format(x)` | `x.__format__()` | `JoinedStr` / `ExprCall(func=Name("format"))` |
| `with x:` | `x.__enter__()`, `x.__exit__()` | `StmtWith` |
| `for i in x:` | `x.__iter__()`, iterator `__next__()` | `StmtFor` |

**Implementation note**: Strato only maps these dunders when the type of the operand can be resolved via simple type inference. If the type is unknown, the dunder call is unresolvable and skipped silently (high precision rule).

### Context Manager Detection

```python
class BlockingConnection:
    def __enter__(self):
        self.conn = psycopg2.connect(...)  # Blocking!
        return self

    def __exit__(self, *args):
        self.conn.close()

async def handler():
    with BlockingConnection() as conn:  # __enter__ blocks!
        pass
```

`StmtWith` translates to `__enter__()` and `__exit__()` calls on the context manager expression.

---

## 11. Escape Hatch Recognition

### Overview

An "escape hatch" is a pattern that correctly offloads a blocking call to a thread pool, making it safe to use in async contexts. Strato must recognize these patterns and suppress diagnostics.

### Recognized Patterns (v1 — asyncio only)

```python
# Pattern 1: loop.run_in_executor()
loop = asyncio.get_running_loop()
await loop.run_in_executor(None, blocking_func, arg1, arg2)
await loop.run_in_executor(executor, blocking_func, arg1, arg2)

# Pattern 2: asyncio.to_thread() (Python 3.9+)
await asyncio.to_thread(blocking_func, arg1, arg2)

# Pattern 3: Combined with functools.partial
from functools import partial
await loop.run_in_executor(None, partial(blocking_func, arg1))

# Pattern 4: Lambda wrapping
await loop.run_in_executor(None, lambda: blocking_func(arg1))
```

### Detection Mechanism

During call edge construction (Phase 4), the visitor checks if the current call expression matches an escape hatch pattern:

```
FUNCTION is_executor_call(call: &ExprCall) -> bool:
  MATCH call.func:
    // asyncio.to_thread(func, ...)
    Attribute(value=Name("asyncio"), attr="to_thread"):
      RETURN true

    // loop.run_in_executor(executor, func, ...)
    Attribute(value, attr="run_in_executor"):
      RETURN is_likely_event_loop(value)

    _:
      RETURN false

// Concrete rules for determining if a value expression is likely an event loop.
// This is a syntactic heuristic — no type inference required.
FUNCTION is_likely_event_loop(value: &Expr) -> bool:

  MATCH value:
    // Case 1: Direct call result — asyncio.get_running_loop() or asyncio.get_event_loop()
    // e.g., `asyncio.get_running_loop().run_in_executor(...)`
    Call(func=Attribute(value=Name("asyncio"), attr)):
      RETURN attr IN ["get_running_loop", "get_event_loop"]

    // Case 2: Variable previously assigned from asyncio loop getter
    // e.g., `loop = asyncio.get_running_loop()` ... `loop.run_in_executor(...)`
    Name(name):
      // Look up the assignment of `name` in the current scope.
      // Walk backwards through statements in the current function body.
      binding = lookup_assignment_in_scope(name, current_function)
      MATCH binding:
        // Assigned from asyncio.get_running_loop()
        Assign(value=Call(func=Attribute(value=Name("asyncio"), attr))):
          RETURN attr IN ["get_running_loop", "get_event_loop"]
        // Any other assignment — not provably an event loop
        _:
          RETURN false

    // Case 3: Anything else (attribute chains, subscripts, function returns)
    // Not provably an event loop — return false (high precision: skip it)
    _:
      RETURN false
```

When an escape hatch is detected, the **callable argument** (the function being offloaded) is protected. However, passing a callable as an argument (e.g., `run_in_executor(None, time.sleep, 1)`) is NOT a call expression in the AST — it's a `Name` reference. Therefore, strato must create a **synthetic call edge** to model the offloading:

**Synthetic Edge Rule for Executor Calls**:

```
WHEN is_executor_call(call) is true:

  callable_arg = call.args[get_executor_callable_arg_position(call)]

  // Resolve the callable argument to a graph node
  MATCH callable_arg:
    // Case 1: Direct name reference — time.sleep, my_func
    Name(name) | Attribute(value, attr):
      callee = resolve_callee(callable_arg)
      IF callee is Some:
        // Create SYNTHETIC edge with in_executor=true
        graph.add_edge(current_function, callee, DirectCall, in_executor=true)

    // Case 2: functools.partial(func, arg1, ...) — unwrap to the underlying callable
    Call(func=Attribute(value=Name("partial"|"functools"), attr="partial"), args=[real_func, ...]):
      callee = resolve_callee(real_func)
      IF callee is Some:
        graph.add_edge(current_function, callee, DirectCall, in_executor=true)

    // Case 3: lambda: func(arg1) — walk the lambda body normally, but with in_executor_context=true
    Lambda(body):
      in_executor_context = true
      visit(body)  // Any edges found inside the lambda body are marked in_executor=true
      in_executor_context = false

    // Case 4: Anything else (variable, complex expression) — unresolvable, skip
    _:
      // Cannot determine what callable is being offloaded. Skip silently.
      PASS
```

**Key invariant**: The synthetic edge ensures that `time.sleep` (a phantom node with `KnownBlocking`) is connected to the calling function but with `in_executor=true`, so blocking status does NOT propagate backward through this edge. Without the synthetic edge, `time.sleep` would be a disconnected node and the executor pattern would be invisible to the analysis.

### Escape Hatch Registry (Extensibility)

```rust
struct EscapeHatchRegistry {
    patterns: Vec<EscapeHatchPattern>,
}

struct EscapeHatchPattern {
    /// Qualified name of the escape function
    /// e.g., "asyncio.to_thread"
    function_name: QualifiedName,
    /// Which argument position contains the callable being offloaded
    /// For run_in_executor: position 1 (0=executor, 1=func)
    /// For to_thread: position 0 (0=func)
    callable_arg_position: usize,
}
```

v1 ships with:
```rust
vec![
    EscapeHatchPattern { function_name: "asyncio.to_thread", callable_arg_position: 0 },
    // run_in_executor is detected structurally (method on event loop)
    // rather than by qualified name, since the loop variable name varies
]
```

Users can add custom escape hatches in configuration:
```toml
[tool.strato.escape_hatches]
add = [
    { name = "myproject.utils.offload", callable_arg = 0 },
]
```

---

## 12. Annotations API

### Python Package: `strato`

The `strato` package is a pure Python package providing decorator annotations. It has **zero dependencies** and **zero runtime impact** (decorators are transparent wrappers).

### Decorator Definitions

```python
# strato/_annotations.py

from typing import TypeVar, Callable

F = TypeVar("F", bound=Callable)


def blocking(func: F) -> F:
    """Mark a function as blocking.

    When strato analyzes your code, functions decorated with @blocking
    are treated as blocking the event loop, similar to time.sleep()
    or requests.get().

    Usage:
        from strato import blocking

        @blocking
        def my_slow_function():
            # This does something that blocks...
            ...

        async def handler():
            my_slow_function()  # strato will flag this!
    """
    func.__strato_blocking__ = True  # type: ignore[attr-defined]
    return func


def non_blocking(func: F) -> F:
    """Mark a function as non-blocking.

    When strato analyzes your code, functions decorated with @non_blocking
    are treated as safe to call from async contexts, even if their bodies
    contain calls that strato would otherwise consider blocking.

    Use this when strato produces a false positive, or when you know
    a function is safe despite appearances.

    Usage:
        from strato import non_blocking

        @non_blocking
        def actually_safe():
            # strato would flag this, but we know it's safe because
            # the blocking call is behind a condition that's never
            # true in async contexts.
            ...
    """
    func.__strato_non_blocking__ = True  # type: ignore[attr-defined]
    return func
```

### `__init__.py`

```python
# strato/__init__.py
from strato._annotations import blocking, non_blocking

__all__ = ["blocking", "non_blocking"]
__version__ = "0.1.0"
```

### How Strato Detects Annotations

During Phase 2 (Parse), the AST walker looks for decorator applications:

```
FUNCTION detect_annotations(func_def: &StmtFunctionDef) -> Option<AnnotationType>:

  FOR decorator in func_def.decorator_list:
    MATCH decorator:
      // @blocking
      Name("blocking"):
        IF is_imported_from_strato("blocking"):
          RETURN Some(AnnotationType::Blocking)

      // @strato.blocking
      Attribute(value=Name("strato"), attr="blocking"):
        RETURN Some(AnnotationType::Blocking)

      // @non_blocking
      Name("non_blocking"):
        IF is_imported_from_strato("non_blocking"):
          RETURN Some(AnnotationType::NonBlocking)

      // @strato.non_blocking
      Attribute(value=Name("strato"), attr="non_blocking"):
        RETURN Some(AnnotationType::NonBlocking)

  RETURN None
```

### `.pyi` Stub Support

#### Stub Resolution Data Flow

Stubs are integrated into the analysis pipeline as follows:

1. **Phase 1 (Discovery)**: The file manifest includes `.pyi` files found in:
   - **Source roots** (alongside `.py` files) — standard practice for type stubs
   - **`stub_paths`** (from config) — dedicated directories for custom stubs
   - Search order: source roots first, then `stub_paths` in order listed

2. **Phase 3 (Resolve)**: The module resolver tries `.pyi` as a resolution target (see step 4d of the resolution algorithm in Section 5). When both `foo.py` and `foo.pyi` exist:
   - The `.py` file is used for call graph construction (body analysis)
   - The `.pyi` file is used for annotation extraction (`@blocking`/`@non_blocking` only)
   - If only `.pyi` exists (no `.py`), it is used solely for annotations (no body analysis possible)

3. **Phase 5 (Annotate)**: `.pyi` files are scanned for `@blocking`/`@non_blocking` decorators. Their annotations override or supplement database entries for the same qualified name.

4. **First-party classification**: `.pyi` files in `stub_paths` are classified as **third-party** (they annotate external libraries). `.pyi` files in source roots follow normal classification.

Users can create `.pyi` stub files to annotate third-party libraries without modifying their source:

```python
# stubs/redis.pyi
from strato import blocking

class Redis:
    @blocking
    def get(self, key: str) -> bytes: ...

    @blocking
    def set(self, key: str, value: bytes) -> None: ...
```

Strato parses `.pyi` files and extracts `@blocking`/`@non_blocking` annotations. Since stubs have no bodies, only annotations matter.

---

## 13. Configuration

### Full Schema

```toml
[tool.strato]

# Source roots for first-party code detection.
# If omitted, auto-detected from project layout.
# Paths are relative to pyproject.toml location.
src_roots = ["src"]

# Minimum Python version of the analyzed codebase.
# Affects which escape hatches are recognized.
# Default: "3.9" (supports asyncio.to_thread)
python_version = "3.9"

# Intervention point strategy for error reporting.
# Options: "first-party-deepest" (default), "async-boundary"
intervention_strategy = "first-party-deepest"

# Severity of diagnostics.
# Options: "error" (default), "warning"
severity = "error"

# Paths to exclude from analysis (glob patterns).
exclude = [
    "tests/**",
    "migrations/**",
    "**/conftest.py",
]

# Additional paths to search for .pyi stubs with @blocking annotations
stub_paths = ["stubs/"]

# Cache directory. Default: ".strato_cache"
cache_dir = ".strato_cache"

# Enable/disable caching. Default: true
cache_enabled = true


[tool.strato.blocking]

# Add custom blocking function entries
add = [
    { name = "redis.Redis.get", help = "Use aioredis", category = "network-io" },
    { name = "redis.Redis.set", help = "Use aioredis", category = "network-io" },
]

# Remove built-in entries (suppress specific false positives)
remove = [
    "builtins.open",
]

# Mark entire modules as blocking (all functions in the module)
blocking_modules = [
    "legacy_sync_module",
]


[tool.strato.escape_hatches]

# Add custom escape hatch patterns
add = [
    { name = "myproject.utils.run_async", callable_arg = 0 },
]
```

### Configuration Validation

Strato validates the config at startup and exits with code 2 on error:

| Check | Error Message |
|-------|--------------|
| `src_roots` path doesn't exist | `Source root '{path}' does not exist` |
| `src_roots` path has no `.py` files | `Source root '{path}' contains no Python files` |
| Invalid `python_version` | `Invalid python_version: must be '3.7', '3.8', ..., '3.13'` |
| Invalid `intervention_strategy` | `Invalid strategy: must be 'first-party-deepest' or 'async-boundary'` |
| `blocking.add` entry missing `name` | `Blocking entry missing required field 'name'` |
| Invalid `category` in blocking entry | `Unknown category '{cat}'. Valid: sleep, network-io, file-io, subprocess, database-io, user-input, other` |

---

## 14. CLI Interface

### Commands

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
                             Comma-separated top-level package names: "myapp,mylib"
                             
                             Semantics: When provided, a file is classified as
                             first-party if its module path starts with any of
                             the listed names. This REPLACES (not augments) the
                             auto-detected or configured src_roots classification.
                             
                             Example: --first-party myapp,mylib
                             → "myapp.utils.helper" is first-party
                             → "mylib.core" is first-party  
                             → "requests.get" is third-party
                             
                             Precedence: --first-party > [tool.strato].src_roots > auto-detection
                             
                             Algorithm:
                               is_first_party(module_path) =
                                 first_party_modules.any(|m| module_path.starts_with(m + ".") || module_path == m)

  --python-version <VER>     Override Python version.
                             Values: 3.7, 3.8, ..., 3.13

  --stats                    Show analysis statistics after run.
                             (files parsed, functions analyzed,
                              call graph size, etc.)

  -q, --quiet                Suppress non-diagnostic output.

  -v, --verbose              Show detailed analysis progress.

  --help                     Show this help message.

  --version                  Show strato version.
```

### Binary Naming Convention

| Context | Name | Explanation |
|---------|------|-------------|
| Cargo package name | `strato_cli` | Rust crate under `crates/strato_cli/` |
| Compiled binary | `strato` | Set via `[[bin]] name = "strato"` in `crates/strato_cli/Cargo.toml` |
| PyPI package name | `strato-cli` | Installed via `pip install strato-cli` |
| User-facing command | `strato` | The command users type: `strato check src/` |
| Cargo run | `cargo run -p strato_cli --` | Runs the `strato` binary from the workspace |
| Test harness reference | `env!("CARGO_BIN_EXE_strato")` | Resolved by Cargo to the built binary path |
| Release binary path | `target/release/strato` | After `cargo build --release -p strato_cli` |

The `crates/strato_cli/Cargo.toml` must include:
```toml
[[bin]]
name = "strato"
path = "src/main.rs"
```

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | No blocking issues found (some files may have parse warnings) |
| 1 | Blocking issues detected (takes priority over parse warnings) |
| 2 | Configuration error (invalid config, missing source roots) |
| 3 | All files failed to parse (no analysis possible) |

**Parse error policy**: Individual file parse errors are **non-fatal** — strato emits a warning diagnostic for each unparseable file and continues analysis on the remaining files. Exit code 3 is returned **only** when every file in the project fails to parse, making analysis impossible. If some files parse and blocking issues are found, exit code 1 is returned (blocking issues take priority). If some files parse but no blocking issues are found, exit code 0 is returned (parse warnings are informational only).

### Example Usage

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

---

## 15. Output Formats

### Text Format (Default)

```
STRATO002: Blocking call reachable from async context

  --> src/myapp/db.py:15:5
   |
15 |     psycopg2.connect(dsn)
   |     ^^^^^^^^^^^^^^^^^^^^ blocks the event loop
   |
   = chain: process() -> validate() -> check_db() -> psycopg2.connect()
   = help: Use `asyncpg` or wrap in `await loop.run_in_executor(None, ...)`

STRATO003: Blocking property accessed in async context

  --> src/myapp/loader.py:8:5
   |
 8 |     @property
 9 |     def data(self):
   |         ^^^^ property getter blocks
   |
   = chain: handler() -> loader.data -> requests.get()
   = help: Make async, or use `run_in_executor`

Found 2 blocking issues in 15 files (43 functions analyzed)
```

### JSON Format

```json
{
  "version": "1.0",
  "diagnostics": [
    {
      "code": "STRATO002",
      "severity": "error",
      "message": "Blocking call reachable from async context",
      "primary_location": {
        "file": "src/myapp/db.py",
        "line": 15,
        "column": 5,
        "end_line": 15,
        "end_column": 25
      },
      "chain": [
        {
          "function": "myapp.processor.process",
          "file": "src/myapp/processor.py",
          "line": 10,
          "is_async": true,
          "is_first_party": true
        },
        {
          "function": "myapp.validator.validate",
          "file": "src/myapp/validator.py",
          "line": 5,
          "is_async": false,
          "is_first_party": true
        },
        {
          "function": "myapp.db.check_db",
          "file": "src/myapp/db.py",
          "line": 12,
          "is_async": false,
          "is_first_party": true
        },
        {
          "function": "psycopg2.connect",
          "file": null,
          "line": null,
          "is_async": false,
          "is_first_party": false
        }
      ],
      "help": "Use `asyncpg` or wrap in `await loop.run_in_executor(None, ...)`",
      "intervention_strategy": "first-party-deepest"
    }
  ],
  "stats": {
    "files_analyzed": 15,
    "functions_analyzed": 43,
    "call_graph_nodes": 43,
    "call_graph_edges": 67,
    "blocking_functions_found": 2,
    "analysis_time_ms": 142
  }
}
```

### SARIF Format

SARIF (Static Analysis Results Interchange Format) v2.1.0, compatible with GitHub Code Scanning:

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
              "shortDescription": {
                "text": "Direct blocking call in async function"
              },
              "helpUri": "https://strato.dev/rules/STRATO001"
            },
            {
              "id": "STRATO002",
              "name": "IndirectBlockingInAsync",
              "shortDescription": {
                "text": "Blocking call reachable from async context via sync intermediary"
              },
              "helpUri": "https://strato.dev/rules/STRATO002"
            },
            {
              "id": "STRATO003",
              "name": "BlockingPropertyInAsync",
              "shortDescription": {
                "text": "Blocking @property getter accessed in async context"
              },
              "helpUri": "https://strato.dev/rules/STRATO003"
            },
            {
              "id": "STRATO004",
              "name": "BlockingDunderInAsync",
              "shortDescription": {
                "text": "Blocking dunder method invoked in async context"
              },
              "helpUri": "https://strato.dev/rules/STRATO004"
            }
          ]
        }
      },
      "results": [
        {
          "ruleId": "STRATO002",
          "level": "error",
          "message": {
            "text": "Blocking call reachable from async context: psycopg2.connect()"
          },
          "locations": [
            {
              "physicalLocation": {
                "artifactLocation": {
                  "uri": "src/myapp/db.py"
                },
                "region": {
                  "startLine": 15,
                  "startColumn": 5,
                  "endLine": 15,
                  "endColumn": 25
                }
              }
            }
          ],
          "relatedLocations": [
            {
              "id": 0,
              "message": { "text": "async context entry point" },
              "physicalLocation": {
                "artifactLocation": { "uri": "src/myapp/processor.py" },
                "region": { "startLine": 10 }
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
                        "message": { "text": "async function process()" },
                        "physicalLocation": {
                          "artifactLocation": { "uri": "src/myapp/processor.py" },
                          "region": { "startLine": 10 }
                        }
                      }
                    },
                    {
                      "location": {
                        "message": { "text": "calls validate()" },
                        "physicalLocation": {
                          "artifactLocation": { "uri": "src/myapp/validator.py" },
                          "region": { "startLine": 5 }
                        }
                      }
                    },
                    {
                      "location": {
                        "message": { "text": "calls check_db()" },
                        "physicalLocation": {
                          "artifactLocation": { "uri": "src/myapp/db.py" },
                          "region": { "startLine": 12 }
                        }
                      }
                    },
                    {
                      "location": {
                        "message": { "text": "calls blocking psycopg2.connect()" },
                        "physicalLocation": {
                          "artifactLocation": { "uri": "src/myapp/db.py" },
                          "region": { "startLine": 15 }
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

---

## 16. Caching Strategy

### What Is Cached

Each file produces a **per-file analysis result** that can be cached:

```rust
struct CachedFileResult {
    /// SHA-256 of file contents
    content_hash: [u8; 32],
    /// Symbols defined in this file
    symbols: Vec<SymbolDef>,
    /// Import statements in this file
    imports: Vec<ImportStatement>,
    /// Call edges originating from functions in this file
    call_edges: Vec<CallEdge>,
    /// Annotations (@blocking, @non_blocking) found in this file
    annotations: Vec<AnnotationEntry>,
}
```

### Cache Location

Default: `.strato_cache/` in the project root. Configurable via `cache_dir` in config.

### Cache Format

Binary format using `bincode` for fast serialization/deserialization. The cache directory structure:

```
.strato_cache/
├── manifest.bin         # Maps file paths to content hashes
├── files/
│   ├── {hash1}.bin      # CachedFileResult for file 1
│   ├── {hash2}.bin      # CachedFileResult for file 2
│   └── ...
└── version              # Cache format version (invalidate on upgrade)
```

### Cache Invalidation

| Trigger | Action |
|---------|--------|
| File content changed (hash mismatch) | Re-parse that file, rebuild its call edges |
| File added | Parse new file, merge into call graph |
| File deleted | Remove from call graph, re-propagate |
| Config changed | Full re-analysis (config affects interpretation) |
| strato version changed | Full invalidation (cache format may differ) |
| `--clear-cache` flag | Delete cache directory, full re-analysis |

### Cache Granularity

v1 uses **file-level caching**: each file's parse result and symbol extraction is cached independently. The call graph and propagation are rebuilt from cached per-file results.

This means:
- **Cache hit**: File unchanged → skip parsing and symbol extraction → reuse cached call edges
- **Cache miss**: File changed → re-parse → re-extract → rebuild affected call edges
- **Graph rebuild**: Always rebuilt from (cached or fresh) per-file edges. This is fast (just inserting edges into the graph structure).
- **Propagation**: Always rerun (linear time, fast).

### Performance Impact

| Phase | Without Cache | With Cache (no changes) |
|-------|--------------|------------------------|
| Parse | Full parse (slow) | Skip entirely |
| Resolve | Full resolution | Reuse module map |
| Build | Full graph construction | Rebuild from cached edges (fast) |
| Propagate | Full propagation | Full propagation (always runs, O(V+E)) |
| **Total** | 100% | ~20% (dominated by graph rebuild + propagation) |

---

## 17. Distribution and Packaging

### Two PyPI Packages

#### Package 1: `strato` (Pure Python Annotations)

**File location**: `pyproject.toml` (monorepo root)

```toml
# pyproject.toml (root) — Python annotations package
[build-system]
requires = ["hatchling"]
build-backend = "hatchling.build"

[project]
name = "strato"
version = "0.1.0"
description = "Annotations for the strato async blocking detector"
requires-python = ">=3.8"
license = "MIT"
dependencies = []  # Zero dependencies

[project.optional-dependencies]
cli = ["strato-cli>=0.1.0"]
```

- **Size**: ~5 KB
- **Dependencies**: None
- **Runtime cost**: Zero (decorators are identity functions)
- **Install**: `uv pip install strato` (production) or `uv pip install strato[cli]` (dev)

#### Package 2: `strato-cli` (Rust Binary)

**File location**: `crates/strato_cli/pyproject.toml`

```toml
# crates/strato_cli/pyproject.toml — Rust binary package
[build-system]
requires = ["maturin>=1.0,<2.0"]
build-backend = "maturin"

[project]
name = "strato-cli"
version = "0.1.0"
description = "CLI for the strato async blocking detector"
requires-python = ">=3.8"
license = "MIT"

# No [project.scripts] — with bindings = "bin", maturin packages the Rust binary
# directly. The installed command name is determined by [[bin]] in Cargo.toml.
# See Section 14 "Binary Naming Convention" for details.

[tool.maturin]
bindings = "bin"
```

Built with `maturin` using `bindings = "bin"` (binary distribution, no Python bindings needed). Produces platform-specific wheels for:
- Linux x86_64 (manylinux)
- Linux aarch64
- macOS x86_64
- macOS aarch64 (Apple Silicon)
- Windows x86_64

### Additional Distribution Channels

| Channel | Command |
|---------|---------|
| PyPI (primary) | `uv pip install strato[cli]` |
| Cargo (for Rust users) | `cargo install strato-cli` |
| Homebrew (macOS) | `brew install strato` |
| GitHub Releases | Download binary from releases page |

---

## 18. Repository Structure

```
strato/                              # Monorepo root
├── Cargo.toml                       # Rust workspace definition
├── Cargo.lock
├── pyproject.toml                   # Python annotations package ("strato")
├── LICENSE                          # MIT
├── README.md
│
├── crates/                          # Rust crates
│   ├── strato_core/                 # Core analysis library
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── discovery.rs         # Phase 1: file discovery, config loading
│   │       ├── parser.rs            # Phase 2: parser abstraction layer
│   │       ├── resolver.rs          # Phase 3: module resolver
│   │       ├── graph.rs             # Phase 4: call graph data structures
│   │       ├── graph_builder.rs     # Phase 4: call graph construction
│   │       ├── annotator.rs         # Phase 5: blocking annotation
│   │       ├── propagator.rs        # Phase 6: blocking propagation (SCC)
│   │       ├── reporter.rs          # Phase 7: diagnostic generation
│   │       ├── types.rs             # Shared types (QualifiedName, Location, etc.)
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
│       ├── Cargo.toml               # Rust crate config
│       ├── pyproject.toml           # PyPI "strato-cli" package (maturin build)
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
│       ├── _annotations.py          # @blocking, @non_blocking
│       └── py.typed                 # PEP 561 marker
│
├── tests/                           # Integration tests
│   ├── fixtures/                    # Test Python projects (see Appendix B for full list)
│   │   ├── smoke/                   # Minimal fixture for M0-M10 verification
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
│   ├── schemas/                     # Vendored validation schemas
│   │   └── sarif-schema-2.1.0.json  # SARIF v2.1.0 JSON Schema (vendored)
│   ├── integration/                 # Rust integration tests
│   │   ├── harness.rs               # Shared test harness (Appendix B)
│   │   ├── test_direct_blocking.rs  # A1
│   │   ├── test_indirect_blocking.rs # A2
│   │   ├── test_executor.rs         # A3, A4
│   │   ├── test_sync_only.rs        # A5
│   │   ├── test_annotations.rs      # A6, A7
│   │   ├── test_property.rs         # A8
│   │   ├── test_dunder.rs           # A9
│   │   ├── test_cross_file.rs       # A10
│   │   ├── test_deep_transitive.rs  # A11
│   │   ├── test_multiple_callers.rs # A12
│   │   ├── test_mixed.rs            # A13
│   │   ├── test_output_formats.rs   # JSON/SARIF schema validation
│   │   └── test_performance.rs      # Performance benchmarks
│   └── unit/                        # Rust unit tests (in each crate)
│
├── stubs/                           # Example .pyi stubs for third-party libs
│   └── examples/
│       └── redis.pyi
│
└── docs/                            # Documentation (future)
    └── rules/
        ├── STRATO001.md
        ├── STRATO002.md
        ├── STRATO003.md
        └── STRATO004.md
```

### Cargo Workspace

```toml
# Cargo.toml (workspace root)
[workspace]
members = [
    "crates/strato_core",
    "crates/strato_cache",
    "crates/strato_cli",
]
resolver = "2"

[workspace.dependencies]
# Ruff crates (pinned to specific commit)
ruff_python_parser = { git = "https://github.com/astral-sh/ruff", rev = "091d0af2ab026a08b82d4aa7d3ab6b1ca4db778c" }
ruff_python_ast = { git = "https://github.com/astral-sh/ruff", rev = "091d0af2ab026a08b82d4aa7d3ab6b1ca4db778c" }

# Graph library
petgraph = "0.6"

# Serialization
serde = { version = "1", features = ["derive"] }
bincode = "1"
serde_json = "1"

# CLI
clap = { version = "4", features = ["derive"] }

# Parallelism
rayon = "1"

# File hashing
sha2 = "0.10"

# TOML parsing
toml = "0.8"

# Glob patterns
globset = "0.4"

# Error handling
thiserror = "2"
miette = { version = "7", features = ["fancy"] }  # Beautiful error output
```

---

## 19. Performance

### Performance Targets

| Scenario | Target | Rationale |
|----------|--------|-----------|
| Cached run (no changes) | < 500ms for 500 files | Hash comparison + graph rebuild + propagation |
| Fresh run (first analysis) | < 5s for 500 files | Full parse + resolve + build + propagate |
| Incremental (1 file changed) | < 1s for 500 files | Re-parse 1 file + full graph rebuild + propagation |

### Optimization Strategies

**Phase 2 (Parse): Parallel parsing with rayon**
- Python file parsing is embarrassingly parallel
- Ruff's parser is already the fastest available (~2x faster than alternatives)
- Use `rayon::par_iter()` to parse all files concurrently

**Phase 3 (Resolve): Cached module map**
- Module resolution results are cached per-file
- Import statements rarely change even when function bodies do
- The module map is rebuilt only when imports change

**Phase 4 (Build): Incremental edge construction**
- Per-file call edges are cached
- On cache hit: insert cached edges directly into graph (no AST walk)
- Graph construction is O(E) where E = total edges

**Phase 6 (Propagate): SCC-based linear time**
- Tarjan's SCC: O(V + E)
- Topological propagation: O(V + E)
- No iterative fixpoint — guaranteed single pass

**Phase 7 (Report): Minimal work**
- Simple graph traversal from async nodes
- O(V) in the worst case

### Performance Complicating Factors

The following make ruff-level performance (200ms for 630 files) difficult for **fresh** runs:

1. **Cross-file coordination**: ruff analyzes files independently. Strato must merge results across files for the call graph. This coordination overhead is inherent to cross-file analysis.

2. **Module resolution**: Every import statement requires filesystem lookups (checking if paths exist). This involves I/O that single-file tools avoid.

3. **Graph construction**: Building the call graph requires visiting every function body and resolving callees against the cross-file symbol table.

4. **Propagation**: Even at O(V+E), a 500-file project may have thousands of functions and tens of thousands of edges.

However, **cached runs** approach ruff-level speed because:
- No parsing (hash comparison only)
- No AST walking (cached edges)
- Graph rebuild from cached edges is fast
- Propagation is a single linear pass

---

## 20. Limitations and Future Work

### Known Limitations (v1)

| Limitation | Impact | Mitigation |
|-----------|--------|------------|
| No type inference | Can't resolve calls through variables | `@blocking` decorator for manual annotation |
| No dynamic dispatch | `getattr()`, monkey patching invisible | Skip silently (high precision) |
| No star import resolution | `from x import *` is opaque | Treat as unresolvable |
| asyncio-only | trio/anyio escape hatches not recognized | Configurable in future versions |
| First-party focus | Third-party source traversal limited | Built-in database + stubs |
| No namespace packages | PEP 420 not supported | Require `__init__.py` |
| No conditional imports | `try/except` imports not fully handled | Best-effort: take first branch |

### Future Work (v2+)

| Feature | Description | Priority |
|---------|-------------|----------|
| trio/anyio support | Recognize trio and anyio escape hatches | High |
| Framework integration | Django `sync_to_async`, FastAPI auto-threading | High |
| IDE/LSP server | Real-time feedback in editors | Medium |
| Autofix suggestions | Generate code that wraps blocking calls | Medium |
| Full trace visualization | Interactive HTML report showing call chains | Medium |
| Type-aware resolution | Use type hints to resolve more calls | Medium |
| Third-party source traversal | Deep-scan installed packages | Medium |
| Incremental graph updates | Only rebuild affected subgraph on change | Low |
| Watch mode | Continuous analysis on file save | Low |
| GitHub Action | Pre-built CI integration | Low |

---

## Appendix A: Acceptance Test Cases

These test cases define the expected behavior of strato v1. Each is a self-contained Python snippet with the expected diagnostic output.

### Test Conventions

**All fixtures are run with `Config::default()`**, which uses:
- Intervention strategy: `first-party-deepest`
- Severity: `error`
- All code in fixtures is treated as first-party

**`blocking_chain` definition**: The chain is a list of **function nodes** on the path from the async entry point to the blocking call (inclusive of both endpoints). Each node is a function/method/property that appears in the call sequence.

**Chain counting rule**: `chain_length` = number of nodes in the chain, including:
- The async function that is the entry point (first node)
- Every intermediary sync function
- The blocking function itself (last node)

**Examples**:
- `async handler() → time.sleep()` → chain = [handler, time.sleep] → `chain_length = 2`
- `async handler() → helper() → time.sleep()` → chain = [handler, helper, time.sleep] → `chain_length = 3`
- `async handler() → level_1() → level_2() → level_3() → time.sleep()` → chain = [handler, level_1, level_2, level_3, time.sleep] → `chain_length = 5`

**Primary location**: All expected locations use the `first-party-deepest` strategy (the default). Each fixture's `expected.json` specifies the exact line number for this strategy only.

### A1: Direct Blocking in Async (STRATO001)

```python
# test_direct.py
import time

async def handler():
    time.sleep(1)  # STRATO001: Direct blocking call in async function
```

**Expected**: 1 diagnostic at line 5. Chain: [handler, time.sleep]. `chain_length = 2`.

### A2: Indirect Blocking via Sync Intermediary (STRATO002)

```python
# test_indirect.py
import time

def helper():
    time.sleep(1)

async def handler():
    helper()  # STRATO002: Indirect blocking via helper() -> time.sleep()
```

**Expected**: 1 diagnostic at line 5 (`time.sleep(1)` inside `helper()`). Chain: [handler, helper, time.sleep]. `chain_length = 3`.

### A3: Executor Wrapping is Safe

```python
# test_executor.py
import asyncio
import time

async def handler():
    loop = asyncio.get_running_loop()
    await loop.run_in_executor(None, time.sleep, 1)  # Safe
```

**Expected**: 0 diagnostics.

### A4: `asyncio.to_thread` is Safe

```python
# test_to_thread.py
import asyncio
import time

async def handler():
    await asyncio.to_thread(time.sleep, 1)  # Safe
```

**Expected**: 0 diagnostics.

### A5: Sync-Only Code is Safe

```python
# test_sync_only.py
import time

def handler():
    time.sleep(1)  # Fine — not in async context
```

**Expected**: 0 diagnostics.

### A6: `@blocking` Decorator

```python
# test_blocking_annotation.py
from strato import blocking

@blocking
def custom_slow():
    pass  # Body doesn't matter — marked as blocking

async def handler():
    custom_slow()  # STRATO002: custom_slow is @blocking
```

**Expected**: 1 diagnostic.

### A7: `@non_blocking` Override

```python
# test_non_blocking.py
import time
from strato import non_blocking

@non_blocking
def actually_safe():
    time.sleep(1)  # Would normally be blocking, but user says it's fine

async def handler():
    actually_safe()  # No diagnostic — @non_blocking overrides
```

**Expected**: 0 diagnostics.

### A8: Blocking Property (STRATO003)

```python
# test_property.py
import requests

class DataLoader:
    @property
    def data(self):
        return requests.get("https://example.com").json()

async def handler():
    loader = DataLoader()
    result = loader.data  # STRATO003: @property getter blocks
```

**Expected**: 1 diagnostic.

### A9: Blocking Dunder (STRATO004)

```python
# test_dunder.py
import requests

class RemoteObject:
    def __str__(self):
        return requests.get(self.url).text

async def handler():
    obj = RemoteObject()
    name = str(obj)  # STRATO004: __str__ blocks
```

**Expected**: 1 diagnostic.

### A10: Cross-File Detection

```python
# utils.py
import time

def slow_util():
    time.sleep(1)
```

```python
# handler.py
from utils import slow_util

async def handler():
    slow_util()  # STRATO002: cross-file blocking via slow_util -> time.sleep
```

**Expected**: 1 diagnostic.

### A11: Deep Transitive Chain

```python
# test_deep.py
import time

def level_3():
    time.sleep(1)

def level_2():
    level_3()

def level_1():
    level_2()

async def handler():
    level_1()  # STRATO002: handler -> level_1 -> level_2 -> level_3 -> time.sleep
```

**Expected**: 1 diagnostic. Chain: [handler, level_1, level_2, level_3, time.sleep]. `chain_length = 5`.

### A12: Multiple Async Callers

```python
# test_multiple_callers.py
import time

def blocker():
    time.sleep(1)

async def handler_a():
    blocker()  # STRATO002

async def handler_b():
    blocker()  # STRATO002
```

**Expected**: 2 diagnostics (one for each async caller).

### A13: Mixed Safe and Unsafe Calls

```python
# test_mixed.py
import asyncio
import time

def helper():
    time.sleep(1)

async def handler():
    await asyncio.to_thread(helper)  # Safe — in executor
    helper()  # STRATO002 — not in executor
```

**Expected**: 1 diagnostic (only the direct `helper()` call).

---

## Appendix B: Test Harness Specification

### Fixture Structure

Each acceptance test case from Appendix A maps to a **fixture directory** under `tests/fixtures/`. A fixture is a self-contained mini Python project that strato can analyze:

```
tests/fixtures/
├── a01_direct_blocking/
│   ├── test_direct.py              # Python source from A1
│   └── expected.json               # Golden output (JSON format)
├── a02_indirect_blocking/
│   ├── test_indirect.py
│   └── expected.json
├── a03_executor_safe/
│   ├── test_executor.py
│   └── expected.json
├── a04_to_thread_safe/
│   ├── test_to_thread.py
│   └── expected.json
├── a05_sync_only_safe/
│   ├── test_sync_only.py
│   └── expected.json
├── a06_blocking_annotation/
│   ├── test_blocking_annotation.py
│   └── expected.json
├── a07_non_blocking_override/
│   ├── test_non_blocking.py
│   └── expected.json
├── a08_property_blocking/
│   ├── test_property.py
│   └── expected.json
├── a09_dunder_blocking/
│   ├── test_dunder.py
│   └── expected.json
├── a10_cross_file/
│   ├── utils.py                    # Multi-file fixture
│   ├── handler.py
│   └── expected.json
├── a11_deep_transitive/
│   ├── test_deep.py
│   └── expected.json
├── a12_multiple_callers/
│   ├── test_multiple_callers.py
│   └── expected.json
└── a13_mixed_safe_unsafe/
    ├── test_mixed.py
    └── expected.json
```

### Golden Output Format (`expected.json`)

Each fixture's `expected.json` defines the **exact expected diagnostics** in a normalized, diff-friendly format:

```json
{
  "fixture": "a01_direct_blocking",
  "expected_diagnostics": [
    {
      "code": "STRATO001",
      "file": "test_direct.py",
      "line": 5,
      "message_contains": "Direct blocking call in async function",
      "chain_length": 2,
      "chain_root": "time.sleep"
    }
  ],
  "expected_diagnostic_count": 1,
  "expected_exit_code": 1
}
```

**Fields:**
- `code`: Exact error code (STRATO001-STRATO004)
- `file`: Relative path within fixture directory
- `line`: Line number of the primary diagnostic location
- `message_contains`: Substring that must appear in the diagnostic message
- `chain_length`: Number of links in the blocking call chain
- `chain_root`: The ultimate blocking function at the end of the chain
- `expected_diagnostic_count`: Total number of diagnostics for this fixture
- `expected_exit_code`: Expected CLI exit code (0=clean, 1=issues found)

### Rust Integration Test Structure

Integration tests live in `tests/integration/` and use a shared test harness module:

```rust
// tests/integration/harness.rs — shared test infrastructure

use std::path::Path;
use serde::Deserialize;

#[derive(Deserialize)]
struct ExpectedOutput {
    fixture: String,
    expected_diagnostics: Vec<ExpectedDiagnostic>,
    expected_diagnostic_count: usize,
    expected_exit_code: i32,
}

#[derive(Deserialize)]
struct ExpectedDiagnostic {
    code: String,
    file: String,
    line: usize,
    message_contains: String,
    chain_length: usize,
    chain_root: String,
}

/// Run strato analysis on a fixture directory and compare against expected.json
fn run_fixture(fixture_name: &str) {
    let fixture_dir = Path::new("tests/fixtures").join(fixture_name);
    let expected_path = fixture_dir.join("expected.json");

    // 1. Load expected output
    let expected: ExpectedOutput = serde_json::from_str(
        &std::fs::read_to_string(&expected_path).unwrap()
    ).unwrap();

    // 2. Run strato analysis on fixture directory (using strato_core library API)
    let config = strato_core::Config::default();
    let result = strato_core::analyze(&fixture_dir, &config).unwrap();

    // 3. Assert diagnostic count
    assert_eq!(
        result.diagnostics.len(),
        expected.expected_diagnostic_count,
        "Fixture {}: expected {} diagnostics, got {}",
        fixture_name, expected.expected_diagnostic_count, result.diagnostics.len()
    );

    // 4. Assert each expected diagnostic is present
    for expected_diag in &expected.expected_diagnostics {
        let matching = result.diagnostics.iter().find(|d| {
            d.code == expected_diag.code
            && d.primary_location.file.ends_with(&expected_diag.file)
            && d.primary_location.line == expected_diag.line
        });

        assert!(
            matching.is_some(),
            "Fixture {}: expected diagnostic {} at {}:{} not found",
            fixture_name, expected_diag.code, expected_diag.file, expected_diag.line
        );

        let diag = matching.unwrap();
        assert!(
            diag.message.contains(&expected_diag.message_contains),
            "Fixture {}: diagnostic message '{}' does not contain '{}'",
            fixture_name, diag.message, expected_diag.message_contains
        );
        assert_eq!(
            diag.blocking_chain.len(),
            expected_diag.chain_length,
            "Fixture {}: expected chain length {}, got {}",
            fixture_name, expected_diag.chain_length, diag.blocking_chain.len()
        );
    }

    // 5. Assert exit code
    let exit_code = if result.diagnostics.is_empty() { 0 } else { 1 };
    assert_eq!(
        exit_code, expected.expected_exit_code,
        "Fixture {}: expected exit code {}, got {}",
        fixture_name, expected.expected_exit_code, exit_code
    );
}
```

Each integration test file invokes the harness:

```rust
// tests/integration/test_direct_blocking.rs
mod harness;

#[test]
fn test_a01_direct_blocking() {
    harness::run_fixture("a01_direct_blocking");
}
```

```rust
// tests/integration/test_cross_file.rs
mod harness;

#[test]
fn test_a10_cross_file() {
    harness::run_fixture("a10_cross_file");
}
```

### What Constitutes "Pass"

A test **passes** when ALL of the following hold:
1. **Diagnostic count matches**: `result.diagnostics.len() == expected_diagnostic_count`
2. **Each expected diagnostic found**: Matched by (code, file, line) triple
3. **Message content correct**: Diagnostic message contains `message_contains` substring
4. **Chain length correct**: Blocking chain has expected number of links
5. **Exit code correct**: CLI would return expected exit code

A test **fails** on ANY mismatch, with a descriptive assertion message identifying the fixture and the specific mismatch.

### Running Tests

```bash
# Run all integration tests
cargo test --tests

# Run a specific fixture test
cargo test test_a01_direct_blocking

# Run with output on failure
cargo test --tests -- --nocapture
```

---

## 21. Implementation Plan

### Overview

This section defines the sequenced implementation milestones for building strato v1. Each milestone produces a working, testable artifact. Milestones are ordered by dependency — later milestones build on earlier ones.

### Tool Prerequisites

The following tools must be installed to run all verification commands across milestones:

| Tool | Purpose | Install | Required By |
|------|---------|---------|-------------|
| `rustup` + `cargo` | Rust build system | `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \| sh` | All milestones |
| `python` (3.8+) | Python annotations package testing | System Python or `pyenv` | M0 |
| `maturin` (1.x) | Rust → Python wheel building | `pip install maturin` | M12 |
| `hyperfine` | CLI benchmarking | `cargo install hyperfine` | M12 (perf protocol) |
| `cargo-flamegraph` | CPU profiling | `cargo install flamegraph` | M12 (profiling) |
| `node` + `npx` | SARIF schema validation | `brew install node` or system package | M12 (SARIF validation) |
| `ajv-cli` | JSON Schema validator | `npx ajv-cli` (auto-installed) | M12 (SARIF validation) |
| `gh` (GitHub CLI) | SARIF upload test | `brew install gh` | M12 (manual step) |

**Note**: M0–M11 only require `rustup`/`cargo` and `python`. The additional tools are only needed for M12 (polish + release).

**Reference Integrity Note**: All file paths in this plan (e.g., `crates/strato_core/src/resolver.rs`) describe artifacts **to be created** during implementation. They do not exist in the repository today. Each milestone below specifies exactly which files to create and which to modify. **Milestone 0 (Project Scaffolding) creates the skeleton** — all subsequent milestones reference files that exist after M0 is complete. The design sections (1–20) are the **authoritative specification**: when a milestone says "implement Section 5", the implementer reads Section 5's algorithms and data structures from this document.

### Milestone 0: Project Scaffolding

**Goal**: Set up the Rust workspace, Python package, and project skeleton.

**Files to create**:
- `Cargo.toml` (workspace root — defines workspace members and shared dependencies)
- `crates/strato_core/Cargo.toml`
- `crates/strato_core/src/lib.rs` (empty pub mod declarations)
- `crates/strato_core/src/types.rs` (shared types: `QualifiedName`, `Location`, `ModulePath`)
- `crates/strato_cache/Cargo.toml`
- `crates/strato_cache/src/lib.rs` (stub)
- `crates/strato_cli/Cargo.toml`
- `crates/strato_cli/pyproject.toml` (maturin-based PyPI package for `strato-cli`)
- `crates/strato_cli/src/main.rs` (stub: prints version)
- `python/strato/__init__.py`
- `python/strato/_annotations.py` (`@blocking`, `@non_blocking`)
- `python/strato/py.typed`
- `pyproject.toml` (root — Python annotations package "strato")
- `tests/integration/harness.rs` (test harness from Appendix B)
- `tests/fixtures/smoke/test_smoke.py` (minimal fixture for M9/M10 verification — contains one `async def` calling `time.sleep`)
- `tests/fixtures/smoke/expected.json` (expected: 1 diagnostic, STRATO001)

**Verification**:
```bash
cargo build                    # All crates compile
cargo test                     # No test failures (no tests yet, but harness compiles)
python -c "from strato import blocking, non_blocking; print('OK')"  # Python package works
ls tests/fixtures/smoke/       # Smoke fixture exists
```

**Depends on**: Nothing (start here)

---

### Milestone 1: Parser Abstraction + File Discovery

**Goal**: Parse Python files using ruff and discover project files.

**Files to create**:
- `crates/strato_core/src/discovery.rs` — File discovery, source root detection, `pyproject.toml` loading
- `crates/strato_core/src/parser.rs` — `trait PythonParser`, `RuffParser` implementation, `FileSymbols` extraction

**Files to modify**:
- `crates/strato_core/src/lib.rs` — Add `pub mod discovery; pub mod parser;`
- `crates/strato_core/Cargo.toml` — Add `ruff_python_parser`, `ruff_python_ast`, `rayon`, `toml`, `globset` dependencies

**Verification**:
```bash
cargo test -p strato_core      # Unit tests for parser and discovery
```

**Unit tests to write**:
- `parser::test_parse_simple_function` — Parse a function def, verify AST
- `parser::test_parse_async_function` — Parse async def, verify `is_async` flag
- `parser::test_parse_error_non_fatal` — Invalid syntax produces error, doesn't panic
- `discovery::test_detect_src_layout` — Detect `src/` layout from pyproject.toml
- `discovery::test_detect_flat_layout` — Detect flat layout
- `discovery::test_exclude_patterns` — Glob exclusion works

**Depends on**: Milestone 0

---

### Milestone 2: Module Resolver

**Goal**: Map import statements to source files. Build cross-file symbol table.

**Files to create**:
- `crates/strato_core/src/resolver.rs` — `ModuleMap`, `SymbolTable`, resolution algorithm

**Files to modify**:
- `crates/strato_core/src/lib.rs` — Add `pub mod resolver;`

**Fixtures to create** (for testing):
- `tests/fixtures/resolver_basic/` — Simple project with absolute imports
- `tests/fixtures/resolver_relative/` — Relative imports across packages
- `tests/fixtures/resolver_init_package/` — `__init__.py` package imports

**Verification**:
```bash
cargo test -p strato_core resolver  # Unit tests for resolver
```

**Unit tests to write**:
- `resolver::test_absolute_import` — `import myapp.utils` resolves to `myapp/utils.py`
- `resolver::test_from_import` — `from myapp.utils import helper` resolves
- `resolver::test_relative_import` — `from . import sibling` resolves
- `resolver::test_relative_parent_import` — `from ..utils import helper` resolves
- `resolver::test_init_package` — `from myapp import subpackage` resolves to `__init__.py`
- `resolver::test_unresolvable_returns_none` — Missing module returns `None`
- `resolver::test_source_root_ordering` — Multiple source roots tried in order
- `resolver::test_pyi_stub_resolution` — `.pyi` file found alongside `.py`

**Depends on**: Milestone 1

---

### Milestone 3: Call Graph Data Structures + Construction

**Goal**: Build a project-wide call graph from parsed files and symbol table.

**Files to create**:
- `crates/strato_core/src/graph.rs` — `CallGraph`, `CallGraphNode`, `CallEdge`, `BlockingStatus` types
- `crates/strato_core/src/graph_builder.rs` — `CallEdgeVisitor`, callee resolution, simple type inference

**Files to modify**:
- `crates/strato_core/src/lib.rs` — Add `pub mod graph; pub mod graph_builder;`
- `crates/strato_core/Cargo.toml` — Add `petgraph` dependency

**Verification**:
```bash
cargo test -p strato_core graph  # Unit tests for graph construction
```

**Unit tests to write**:
- `graph_builder::test_direct_call_edge` — `foo()` creates edge from caller to `foo`
- `graph_builder::test_method_call_edge` — `obj.method()` creates edge with inferred type
- `graph_builder::test_self_method_call` — `self.method()` resolves within class
- `graph_builder::test_unresolvable_call_skipped` — Dynamic call creates no edge
- `graph_builder::test_lambda_node` — Lambda expressions registered as nodes
- `graph_builder::test_simple_type_inference_constructor` — `MyClass()` infers type `MyClass`
- `graph_builder::test_simple_type_inference_self` — `self` infers current class

**Depends on**: Milestone 2

---

### Milestone 4: Blocking Database + Annotation Detection

**Goal**: Mark known blocking functions and detect `@blocking`/`@non_blocking` decorators.

**Files to create**:
- `crates/strato_core/src/annotator.rs` — Annotation detection logic
- `crates/strato_core/src/database/mod.rs` — `BlockingDatabase` struct
- `crates/strato_core/src/database/stdlib.rs` — `time.sleep`, `builtins.open`, etc.
- `crates/strato_core/src/database/network.rs` — `requests.*`, `socket.*`, etc.
- `crates/strato_core/src/database/database.rs` — `psycopg2.*`, `sqlite3.*`, etc.
- `crates/strato_core/src/database/subprocess.rs` — `subprocess.*`, `os.system`, etc.

**Files to modify**:
- `crates/strato_core/src/lib.rs` — Add `pub mod annotator; pub mod database;`

**Verification**:
```bash
cargo test -p strato_core annotator  # Unit tests
cargo test -p strato_core database   # Database entry verification
```

**Unit tests to write**:
- `database::test_builtin_entries_complete` — Every entry enumerated in Section 9 of this document is present in the database. The test iterates over a hardcoded list matching the tables in Section 9 and asserts each is found. (The Section 9 tables are the authoritative list; the implementer transcribes them into the test.)
- `database::test_fixture_required_entries` — All blocking functions referenced by Appendix A fixtures are present: `time.sleep`, `requests.get` (at minimum)
- `database::test_time_sleep_help_message` — `time.sleep` entry has help text "Use `asyncio.sleep()`"
- `database::test_user_config_add` — Custom entry added from config
- `database::test_user_config_remove` — Built-in entry removed via config
- `annotator::test_detect_blocking_decorator` — `@blocking` from strato recognized
- `annotator::test_detect_non_blocking_decorator` — `@non_blocking` recognized
- `annotator::test_detect_strato_dot_blocking` — `@strato.blocking` recognized
- `annotator::test_ignore_unrelated_decorator` — Random decorators ignored

**Depends on**: Milestone 3

---

### Milestone 5: Blocking Propagation (SCC Algorithm)

**Goal**: Propagate blocking status through the call graph using SCC decomposition.

**Files to create**:
- `crates/strato_core/src/propagator.rs` — Tarjan's SCC, condensation, topological propagation

**Files to modify**:
- `crates/strato_core/src/lib.rs` — Add `pub mod propagator;`

**Verification**:
```bash
cargo test -p strato_core propagator  # Unit tests
```

**Unit tests to write**:
- `propagator::test_direct_blocking_propagation` — A calls B (blocking) → A becomes PropagatedBlocking
- `propagator::test_transitive_propagation` — A→B→C(blocking) → A and B become PropagatedBlocking
- `propagator::test_executor_edge_blocks_propagation` — Executor edge prevents propagation
- `propagator::test_non_blocking_stops_propagation` — `@non_blocking` node blocks propagation
- `propagator::test_cycle_handling` — Mutual recursion with one blocking member → both blocking
- `propagator::test_cycle_no_blocking` — Mutual recursion, no blocking → both stay Unknown
- `propagator::test_unknown_stays_unknown` — Nodes with no blocking callees remain Unknown
- `propagator::test_blocking_reason_path` — `BlockingReason.call_chain` is correct

**Depends on**: Milestone 4

---

### Milestone 6: Escape Hatch Recognition

**Goal**: Detect `run_in_executor` and `asyncio.to_thread` patterns.

**Files to modify**:
- `crates/strato_core/src/graph_builder.rs` — Add `is_executor_call`, `is_likely_event_loop`, executor scope logic

**Verification**:
```bash
cargo test -p strato_core graph_builder::test_executor  # Executor-specific tests
```

**Unit tests to write**:
- `graph_builder::test_executor_run_in_executor` — `loop.run_in_executor(None, func)` marks edge
- `graph_builder::test_executor_to_thread` — `asyncio.to_thread(func)` marks edge
- `graph_builder::test_executor_only_callable_arg_protected` — Only position 1 (run_in_executor) or 0 (to_thread) is protected
- `graph_builder::test_executor_partial_wrapping` — `partial(func, arg)` detected
- `graph_builder::test_is_likely_event_loop_direct` — `asyncio.get_running_loop().run_in_executor(...)` recognized
- `graph_builder::test_is_likely_event_loop_variable` — `loop = asyncio.get_running_loop(); loop.run_in_executor(...)` recognized
- `graph_builder::test_is_likely_event_loop_unknown_var` — `x.run_in_executor(...)` where x is unknown → not recognized (high precision)

**Depends on**: Milestone 5

---

### Milestone 7: Properties and Dunder Methods

**Goal**: Detect blocking `@property` getters and implicit dunder method calls.

**Files to modify**:
- `crates/strato_core/src/graph_builder.rs` — Add property detection, dunder mapping table, context manager detection

**Verification**:
```bash
cargo test -p strato_core graph_builder::test_property  # Property tests
cargo test -p strato_core graph_builder::test_dunder    # Dunder tests
```

**Unit tests to write**:
- `graph_builder::test_property_access_creates_edge` — `obj.prop` creates PropertyAccess edge when prop is `@property`
- `graph_builder::test_property_non_property_attribute_no_edge` — Regular attribute access creates no edge
- `graph_builder::test_dunder_str_builtin` — `str(obj)` creates edge to `obj.__str__()`
- `graph_builder::test_dunder_eq_operator` — `a == b` creates edge to `a.__eq__()`
- `graph_builder::test_dunder_getitem` — `x[k]` creates edge to `x.__getitem__()`
- `graph_builder::test_dunder_with_statement` — `with x:` creates edges to `__enter__` and `__exit__`
- `graph_builder::test_dunder_for_loop` — `for i in x:` creates edge to `x.__iter__()`
- `graph_builder::test_dunder_fstring` — `f"{x}"` creates edge to `x.__format__()`
- `graph_builder::test_dunder_unknown_type_skipped` — Unknown type → no dunder edge (high precision)

**Depends on**: Milestone 6

---

### Milestone 8: Diagnostic Reporting

**Goal**: Generate diagnostic messages with intervention point selection.

**Files to create**:
- `crates/strato_core/src/reporter.rs` — `Diagnostic`, `DiagnosticSet`, intervention point strategy, error codes

**Files to modify**:
- `crates/strato_core/src/lib.rs` — Add `pub mod reporter;`

**Verification**:
```bash
cargo test -p strato_core reporter  # Unit tests
```

**Unit tests to write**:
- `reporter::test_first_party_deepest_strategy` — Selects deepest first-party function
- `reporter::test_async_boundary_strategy` — Selects async→sync transition point
- `reporter::test_all_third_party_fallback` — Falls back to async boundary when no first-party code in chain
- `reporter::test_error_code_strato001` — Direct blocking gets STRATO001
- `reporter::test_error_code_strato002` — Indirect blocking gets STRATO002
- `reporter::test_error_code_strato003` — Property blocking gets STRATO003
- `reporter::test_error_code_strato004` — Dunder blocking gets STRATO004
- `reporter::test_diagnostic_message_format` — Message contains chain summary

**Implementation notes for M8**:
- The reporter receives the propagated `CallGraph` and iterates over all async nodes that have outgoing edges (direct or transitive) to `KnownBlocking` or `PropagatedBlocking` nodes.
- For each such async node, it extracts the `BlockingReason.call_chain` (computed during propagation in M5).
- The **primary diagnostic location** is selected by applying the intervention strategy to the chain (see "Intervention Point Strategy" earlier in this section).
- The **AST span** for the primary underline/caret range comes from the `Location` stored on the selected `ChainLink`. For `first-party-deepest`, this is the call-site `ExprCall` range of the deepest first-party function's call to its blocking callee.
- `help` text is pulled from the `BlockingDatabase` entry for the `BlockingReason.root_cause` node. If the root cause is not in the database (propagated through user code only), `help` is `None`.

**Depends on**: Milestone 7

---

### Milestone 9: CLI + Output Formats

**Goal**: Build the CLI entry point with text, JSON, and SARIF output.

**Files to create**:
- `crates/strato_cli/src/args.rs` — CLI argument parsing with clap
- `crates/strato_cli/src/config.rs` — pyproject.toml `[tool.strato]` parsing
- `crates/strato_cli/src/output/mod.rs`
- `crates/strato_cli/src/output/text.rs` — Text formatter (miette-based)
- `crates/strato_cli/src/output/json.rs` — JSON formatter
- `crates/strato_cli/src/output/sarif.rs` — SARIF v2.1.0 formatter

**Files to modify**:
- `crates/strato_cli/src/main.rs` — Wire up full pipeline: discovery → parse → resolve → build → annotate → propagate → report → format

**Verification**:
```bash
cargo build -p strato_cli                           # Binary compiles
cargo run -p strato_cli -- check --help              # Help output shows all options
cargo run -p strato_cli -- check tests/fixtures/smoke/ --format json  # Produces JSON (smoke fixture from M0)
```

**Depends on**: Milestone 8

---

### Milestone 10: Caching System

**Goal**: Add incremental caching to skip re-parsing unchanged files.

**Files to create**:
- `crates/strato_cache/src/manifest.rs` — Cache manifest (file path → content hash mapping)
- `crates/strato_cache/src/storage.rs` — Binary cache read/write using bincode
- `crates/strato_cache/src/invalidation.rs` — Cache invalidation logic

**Files to modify**:
- `crates/strato_cache/src/lib.rs` — Public API: `Cache::load`, `Cache::save`, `Cache::is_fresh`
- `crates/strato_cache/Cargo.toml` — Add `bincode`, `serde`, `sha2` dependencies
- `crates/strato_cli/src/main.rs` — Integrate cache into pipeline (check cache before parse, save after)

**Verification**:
```bash
# First run: creates cache (using smoke fixture from M0)
cargo run -p strato_cli -- check tests/fixtures/smoke/ --stats
# Second run: should show cache hit in stats
cargo run -p strato_cli -- check tests/fixtures/smoke/ --stats
# With --no-cache: ignores cache
cargo run -p strato_cli -- check tests/fixtures/smoke/ --no-cache --stats
# With --clear-cache: deletes and rebuilds
cargo run -p strato_cli -- check tests/fixtures/smoke/ --clear-cache --stats
```

**Depends on**: Milestone 9

---

### Milestone 11: Integration Tests (Appendix A)

**Goal**: Create all 13 fixture directories and integration tests from Appendix A.

**Files to create**:
- All 13 fixture directories under `tests/fixtures/` (see Appendix B for structure)
- Each fixture's Python source files and `expected.json`
- `tests/integration/test_direct_blocking.rs` (A1)
- `tests/integration/test_indirect_blocking.rs` (A2)
- `tests/integration/test_executor.rs` (A3, A4)
- `tests/integration/test_sync_only.rs` (A5)
- `tests/integration/test_annotations.rs` (A6, A7)
- `tests/integration/test_property.rs` (A8)
- `tests/integration/test_dunder.rs` (A9)
- `tests/integration/test_cross_file.rs` (A10)
- `tests/integration/test_deep_transitive.rs` (A11)
- `tests/integration/test_multiple_callers.rs` (A12)
- `tests/integration/test_mixed.rs` (A13)
- `tests/integration/test_output_formats.rs` (JSON and SARIF output validation)

**Verification**:
```bash
cargo test --tests              # All 13+ integration tests pass
cargo test --tests -- --nocapture  # With full output for debugging
```

**Depends on**: Milestone 10 (full pipeline must be operational)

---

### Milestone 12: Performance Testing + Polish

**Goal**: Validate performance targets and polish for release.

**Files to create**:
- `tests/fixtures/large_project/` — Auto-generated 500-file Python project for benchmarking (see generation script below)
- `tests/fixtures/generate_large_project.py` — Script to deterministically generate the 500-file fixture
- `tests/integration/test_performance.rs` — Performance assertions
- `tests/schemas/sarif-schema-2.1.0.json` — Vendored SARIF schema (download: `curl -o tests/schemas/sarif-schema-2.1.0.json https://raw.githubusercontent.com/oasis-tcs/sarif-spec/main/sarif-2.1/schema/sarif-schema-2.1.0.json`)
- `stubs/examples/redis.pyi` — Example `.pyi` stub file

**Tasks**:
- Run benchmarks against performance targets (Section 19) using the measurement protocol below
- Profile with `cargo flamegraph` on the large_project fixture. Optimization is **done** when: (a) performance targets pass, OR (b) the top 3 hotspots are documented as inherent to the algorithm (e.g., "parsing is CPU-bound on AST construction") with no further actionable optimization.
- Validate SARIF output:
  **(a) Automated (in `test_sarif_output_schema`)**: The integration test validates structural correctness (required fields present, correct types) as defined in the test code. This is sufficient for CI.
  **(b) Schema validation**: Run `cargo run -p strato_cli -- check tests/fixtures/smoke/ --format sarif > /tmp/test.sarif`, then validate with: `npx ajv-cli validate -s sarif-schema-2.1.0.json -d /tmp/test.sarif`. The schema file is vendored at `tests/schemas/sarif-schema-2.1.0.json` (downloaded once from `https://raw.githubusercontent.com/oasis-tcs/sarif-spec/main/sarif-2.1/schema/sarif-schema-2.1.0.json` and committed). Pass = exit code 0 from `ajv-cli`. Fail = any validation error.
  **(c) GitHub upload (manual, one-time)**: Upload the SARIF file to a test repository via `gh api -X POST /repos/{owner}/{repo}/code-scanning/sarifs -F sarif=@/tmp/test.sarif -F commit_sha=$(git rev-parse HEAD) -F ref=refs/heads/main`. Acceptance: HTTP 202 response. This is a one-time manual verification step during initial release; document it as a comment in `tests/integration/test_output_formats.rs`.
- Write README.md with installation, usage, and configuration docs.
  **Acceptance**: README.md exists and contains: (1) project description, (2) installation commands for `strato` and `strato[cli]`, (3) basic usage example (`strato check src/`), (4) configuration section showing `pyproject.toml` example, (5) link to rule documentation.
  **Verification**: `test -f README.md && grep -q 'strato check' README.md && grep -q 'pyproject.toml' README.md`
- Set up maturin build for `strato-cli` PyPI package.
  **Acceptance**: `maturin build` produces a `.whl` file in `target/wheels/`.
  **Verification** (run from `crates/strato_cli/` directory): `maturin build && ls ../../target/wheels/strato_cli-*.whl`
  Note: maturin reads `crates/strato_cli/pyproject.toml` and `crates/strato_cli/Cargo.toml`. The `-m` flag can also be used from the workspace root: `maturin build -m crates/strato_cli/Cargo.toml`

**Performance Measurement Protocol**:

| Parameter | Value |
|-----------|-------|
| Build mode | `--release` (always; debug builds are not benchmarked) |
| Warmup runs | 3 runs before measurement (prime filesystem cache) |
| Measurement runs | 5 timed runs; report **median** wall-clock time |
| Timing command | `hyperfine --warmup 3 --runs 5 'target/release/strato check tests/fixtures/large_project/'` (binary name = `strato`, see Binary Naming Convention in Section 14) |
| Machine spec | Record: OS, CPU model, core count, RAM, disk type (SSD/NVMe) |
| CI variability | CI tests use a ±30% tolerance band (e.g., target <5s → CI asserts <6.5s) |
| Cache isolation | For "cached" benchmarks: run once to build cache, then time subsequent runs |
| Environment | `RAYON_NUM_THREADS` unset (uses all available cores) |

**Performance test assertions** (in `test_performance.rs`):

```rust
#[test]
fn test_fresh_run_500_files() {
    // Delete cache first
    clear_cache("tests/fixtures/large_project/");
    let start = Instant::now();
    let result = run_strato_check("tests/fixtures/large_project/");
    let elapsed = start.elapsed();
    // Release mode: < 5s (CI tolerance: < 6.5s)
    assert!(elapsed < Duration::from_millis(6500),
        "Fresh run took {:?}, expected < 6.5s", elapsed);
    assert!(result.is_ok());
}

#[test]
fn test_cached_run_500_files() {
    // First run builds cache
    run_strato_check("tests/fixtures/large_project/").unwrap();
    // Second run should use cache
    let start = Instant::now();
    let result = run_strato_check("tests/fixtures/large_project/");
    let elapsed = start.elapsed();
    // Release mode: < 500ms (CI tolerance: < 650ms)
    assert!(elapsed < Duration::from_millis(650),
        "Cached run took {:?}, expected < 650ms", elapsed);
    assert!(result.is_ok());
}
```

**CLI Output Schema Validation** (in `test_output_formats.rs`):

```rust
#[test]
fn test_json_output_schema() {
    let output = run_strato_check_with_format("tests/fixtures/a01_direct_blocking/", "json");
    let json: serde_json::Value = serde_json::from_str(&output).expect("Valid JSON");
    // Required top-level fields
    assert!(json["version"].is_string());
    assert!(json["diagnostics"].is_array());
    assert!(json["stats"].is_object());
    // Required diagnostic fields
    for diag in json["diagnostics"].as_array().unwrap() {
        assert!(diag["code"].is_string());
        assert!(diag["severity"].is_string());
        assert!(diag["message"].is_string());
        assert!(diag["primary_location"].is_object());
        assert!(diag["primary_location"]["file"].is_string());
        assert!(diag["primary_location"]["line"].is_number());
        assert!(diag["chain"].is_array());
    }
}

#[test]
fn test_sarif_output_schema() {
    let output = run_strato_check_with_format("tests/fixtures/a01_direct_blocking/", "sarif");
    let sarif: serde_json::Value = serde_json::from_str(&output).expect("Valid JSON");
    // SARIF v2.1.0 required fields
    assert_eq!(sarif["version"], "2.1.0");
    assert!(sarif["runs"].is_array());
    let run = &sarif["runs"][0];
    assert!(run["tool"]["driver"]["name"].is_string());
    assert!(run["tool"]["driver"]["rules"].is_array());
    assert!(run["results"].is_array());
    // Each result must have ruleId, level, message, locations
    for result in run["results"].as_array().unwrap() {
        assert!(result["ruleId"].is_string());
        assert!(result["level"].is_string());
        assert!(result["message"]["text"].is_string());
        assert!(result["locations"].is_array());
    }
}

#[test]
fn test_text_output_exit_codes() {
    // Fixture with issues → exit code 1
    let exit = run_strato_exit_code("tests/fixtures/a01_direct_blocking/");
    assert_eq!(exit, 1);
    // Fixture with no issues → exit code 0
    let exit = run_strato_exit_code("tests/fixtures/a05_sync_only_safe/");
    assert_eq!(exit, 0);
}
```

**Verification**:
```bash
cargo test test_performance --release               # Performance within targets (must use --release)
cargo test test_json_output_schema                   # JSON schema passes
cargo test test_sarif_output_schema                  # SARIF schema passes
cargo test test_text_output_exit_codes               # Exit codes correct
cargo build --release -p strato_cli                  # Release build succeeds
maturin build -m crates/strato_cli/Cargo.toml        # PyPI wheel builds (from workspace root)
```

**Depends on**: Milestone 11

---

### Milestone Summary

| Milestone | Name | Key Output | Depends On | Est. Effort |
|-----------|------|-----------|------------|-------------|
| 0 | Project Scaffolding | Compiling workspace | — | Small |
| 1 | Parser + Discovery | Parse Python files | M0 | Medium |
| 2 | Module Resolver | Cross-file import resolution | M1 | Large |
| 3 | Call Graph | Project-wide call graph | M2 | Large |
| 4 | Blocking Database | 80+ known blocking functions | M3 | Medium |
| 5 | Propagation | SCC-based blocking propagation | M4 | Medium |
| 6 | Escape Hatches | Executor pattern recognition | M5 | Small |
| 7 | Properties + Dunders | Implicit call detection | M6 | Medium |
| 8 | Diagnostics | Error reporting with strategies | M7 | Medium |
| 9 | CLI + Output | Working binary with 3 formats | M8 | Medium |
| 10 | Caching | Incremental analysis | M9 | Medium |
| 11 | Integration Tests | All 13 acceptance tests pass | M10 | Medium |
| 12 | Performance + Polish | Release-ready | M11 | Medium |

**Critical path**: M0 → M1 → M2 → M3 → M4 → M5 → M6 → M7 → M8 → M9 → M10 → M11 → M12

**Total milestones**: 13 (sequential — each builds on the previous)

---

*End of design document.*
