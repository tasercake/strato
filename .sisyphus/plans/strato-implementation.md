# Strato v1: Full Implementation Plan (M0–M12)

## TL;DR

> **Quick Summary**: Implement the complete Strato async blocking call detector from the approved design document (`.sisyphus/plans/strato-design.md`). 13 milestones, strictly sequential, converting a 3,850-line architecture spec into a working Rust CLI tool with Python annotations package.
>
> **Deliverables**:
> - Rust workspace with 3 crates (`strato_core`, `strato_cache`, `strato_cli`)
> - Python annotations package (`strato`) with `@blocking`/`@non_blocking`
> - CLI binary (`strato check`) with text, JSON, and SARIF output
> - 13 acceptance test fixtures with golden output
> - Performance-validated on 500-file benchmark
>
> **Estimated Effort**: XL (13 milestones, ~60 TODOs)
> **Parallel Execution**: NO — strictly sequential (each milestone depends on previous)
> **Critical Path**: M0 → M1 → M2 → M3 → M4 → M5 → M6 → M7 → M8 → M9 → M10 → M11 → M12

---

## Context

### Original Request

Implement the full Strato project from the approved architecture design document. The design describes a Rust-based static analysis tool that detects blocking function calls inside Python async contexts via transitive call-graph analysis.

### Design Document

**The authoritative specification is `.sisyphus/plans/strato-design.md`** (3,850 lines, 21 sections, 2 appendices). Every TODO below references specific sections by number. When implementing, READ THE REFERENCED SECTION — it contains exact algorithms, data structures, type definitions, and pseudocode.

### Metis Review — Addressed Gaps

The following issues were identified by Metis and are incorporated into this plan:

| Gap | Resolution | Affected TODO |
|-----|-----------|---------------|
| Ruff crate compilation at pinned rev is unvalidated | M0 includes explicit ruff dependency validation step | TODO 1 |
| Integration test directory structure unclear | Tests go under `crates/strato_core/tests/integration/` with `strato_cli` as dev-dependency | TODO 1 |
| Python import needs `PYTHONPATH` | Fixed in M0 acceptance criteria | TODO 1 |
| `analyze()` orchestrator function not explicitly created | Added as explicit task in M9 | TODO 10 |
| Performance tests need `--release` flag | Explicit in M12 criteria | TODO 13 |
| Missing serde derives for cache types | Progressive derive addition noted in M0–M8 | TODO 1 guardrail |
| `from strato import blocking` in fixtures needs resolution | Annotator matches decorator name pattern, not import resolution — documented | TODO 5 |
| Deterministic output requires BTreeMap | Guardrail applied globally | All TODOs |
| `ruff_source_file`/`ruff_text_size` might not exist at pinned rev | M0 validates; fallback to equivalent types if needed | TODO 1 |

---

## Work Objectives

### Core Objective

Build strato v1: a Rust CLI tool that performs transitive call-graph analysis on Python projects to detect blocking calls reachable from async contexts.

### Concrete Deliverables

- `strato` CLI binary (Rust, via `strato_cli` crate)
- `strato` Python package (annotations: `@blocking`, `@non_blocking`)
- 80+ built-in blocking function entries
- Text, JSON, and SARIF output formats
- Incremental caching system
- 13 acceptance test fixtures with golden output
- Performance: <5s fresh, <500ms cached on 500 files

### Definition of Done

- [ ] `cargo build` succeeds (all 3 crates compile)
- [ ] `cargo test` passes (all unit + integration tests)
- [ ] `cargo test --tests --release` passes (performance tests within targets)
- [ ] `strato check tests/fixtures/smoke/` produces correct diagnostic output
- [ ] All 13 acceptance test fixtures pass golden output comparison
- [ ] Python annotations package importable: `from strato import blocking, non_blocking`

### Must Have

- Full 7-phase analysis pipeline (Discovery → Parse → Resolve → Build → Annotate → Propagate → Report)
- SCC-based blocking propagation (Tarjan's algorithm)
- Executor escape hatch recognition (`run_in_executor`, `asyncio.to_thread`)
- Property and dunder method detection
- Cross-file analysis
- `@blocking` / `@non_blocking` annotation support
- Configurable intervention strategy (`first-party-deepest`, `async-boundary`)
- 4 error codes: STRATO001–STRATO004

### Must NOT Have (Guardrails)

- **G1: Unknown stays Unknown** — MUST NOT reclassify `Unknown` `BlockingStatus` nodes to `NotBlocking` or any other state after propagation. Unknown is a permanent terminal state. (Design doc lines 460–462)
- **G2: No full type inference** — MUST NOT implement type inference beyond `self`, `cls`, constructor calls (`MyClass()`), and direct imports. Unknown type = skip silently. (Design doc Section 6 lines 888–914)
- **G3: No v2 features** — MUST NOT implement any feature from Section 20 (trio/anyio, framework integration, autofix, watch mode, LSP, namespace packages)
- **G4: Each milestone must compile** — MUST run `cargo build` and `cargo test` at end of each milestone with zero failures
- **G5: Deterministic output** — MUST use `BTreeMap` or explicit sorting for all collections that affect diagnostic output order. `HashMap` is acceptable for internal lookups only.
- **G6: Exact error code semantics** — STRATO001 (chain_len=1 + async caller), STRATO002 (chain_len>1), STRATO003 (PropertyAccess edge), STRATO004 (ImplicitDunder edge). No merging or reinterpreting.
- **G7: No premature optimization** — MUST NOT profile or benchmark before M12. Correctness first.
- **G8: Only design-specified tests** — Write ONLY the tests listed in each milestone. No extra edge case tests.
- **G9: Only Section 9 DB entries** — MUST NOT add blocking database entries beyond what's in the design doc's Section 9 tables.
- **G10: Only `strato check`** — No other CLI commands. No `strato init`, `strato fix`, etc.
- **G11: Serde derives progressive** — Add `#[derive(Serialize, Deserialize)]` to types as they're created (M0 onwards), so M10 caching doesn't require mass refactoring.
- **G12: No over-documentation** — Minimal `///` doc comments. Code is self-documenting per design doc. No README changes until M12.

---

## Verification Strategy

### Test Decision

- **Infrastructure exists**: NO (greenfield project)
- **User wants tests**: YES (unit tests per milestone + integration tests in M11)
- **Framework**: Rust built-in `#[test]` + `cargo test`
- **QA approach**: Unit tests per milestone (design-specified), integration tests via golden output comparison (M11), performance tests (M12)

### Test Structure

- **Unit tests**: Inline `#[cfg(test)] mod tests` within each `.rs` file
- **Integration tests**: `crates/strato_core/tests/integration/` directory with shared harness
- **Performance tests**: `crates/strato_core/tests/integration/test_performance.rs` (release-only)

### Verification Commands Per Milestone

| Milestone | Command | What It Proves |
|-----------|---------|---------------|
| M0 | `cargo build && PYTHONPATH=python python3 -c "from strato import blocking, non_blocking; print('OK')"` | Workspace compiles, Python pkg works |
| M1 | `cargo test -p strato_core` | Parser + discovery unit tests pass |
| M2 | `cargo test -p strato_core resolver` | Module resolver tests pass |
| M3 | `cargo test -p strato_core graph` | Call graph construction tests pass |
| M4 | `cargo test -p strato_core annotator && cargo test -p strato_core database` | Blocking DB + annotations pass |
| M5 | `cargo test -p strato_core propagator` | SCC propagation tests pass |
| M6 | `cargo test -p strato_core graph_builder::test_executor` | Executor recognition tests pass |
| M7 | `cargo test -p strato_core graph_builder::test_property && cargo test -p strato_core graph_builder::test_dunder` | Property + dunder tests pass |
| M8 | `cargo test -p strato_core reporter` | Diagnostic reporter tests pass |
| M9 | `cargo build -p strato_cli && cargo run -p strato_cli -- check --help` | CLI binary works |
| M10 | `cargo test -p strato_cache` | Caching tests pass |
| M11 | `cargo test --tests` | All 13 integration tests pass |
| M12 | `cargo test --tests --release` | Performance + schema tests pass |

---

## Execution Strategy

### Sequential Execution (No Parallelization)

All 13 milestones are strictly sequential. Each builds on the previous.

```
M0: Project Scaffolding
 └─> M1: Parser + Discovery
      └─> M2: Module Resolver
           └─> M3: Call Graph Construction
                └─> M4: Blocking Database + Annotations
                     └─> M5: Blocking Propagation (SCC)
                          └─> M6: Escape Hatch Recognition
                               └─> M7: Properties + Dunders
                                    └─> M8: Diagnostic Reporting
                                         └─> M9: CLI + Output Formats
                                              └─> M10: Caching System
                                                   └─> M11: Integration Tests
                                                        └─> M12: Performance + Polish
```

### Agent Dispatch Summary

| TODO | Milestone | Category | Skills | Background |
|------|-----------|----------|--------|------------|
| 1 | M0 | `unspecified-high` | `[]` | NO (foundational — must succeed first) |
| 2 | M1 | `unspecified-high` | `[]` | NO |
| 3 | M2 | `unspecified-high` | `[]` | NO |
| 4 | M3 | `unspecified-high` | `[]` | NO |
| 5 | M4 | `unspecified-high` | `[]` | NO |
| 6 | M5 | `ultrabrain` | `[]` | NO |
| 7 | M6 | `unspecified-high` | `[]` | NO |
| 8 | M7 | `unspecified-high` | `[]` | NO |
| 9 | M8 | `unspecified-high` | `[]` | NO |
| 10 | M9 | `unspecified-high` | `[]` | NO |
| 11 | M10 | `unspecified-high` | `[]` | NO |
| 12 | M11 | `unspecified-high` | `[]` | NO |
| 13 | M12 | `unspecified-high` | `[]` | NO |

---

## TODOs

---

### - [ ] 1. M0: Project Scaffolding

**What to do**:

1. Create the Rust workspace root `Cargo.toml` with:
   - Workspace members: `crates/strato_core`, `crates/strato_cache`, `crates/strato_cli`
   - `resolver = "2"`
   - All `[workspace.dependencies]` exactly as specified in Design Doc Section 18 lines 2709–2749
   - Note: `ruff_source_file` and `ruff_text_size` may not exist at the pinned rev — probe with `cargo build` and adapt:
     - If `ruff_source_file` is not a separate crate, use `ruff_python_ast::source_code` or equivalent
     - If `ruff_text_size` is internal, use `ruff_python_parser`'s re-export or define `TextRange` manually

2. Create `crates/strato_core/Cargo.toml`:
   - Package name `strato_core`, version `0.1.0`, edition `2021`
   - Dependencies: `serde = { workspace = true }`, `thiserror = { workspace = true }`
   - **Add `#[derive(Serialize, Deserialize)]` support from the start** (Guardrail G11)

3. Create `crates/strato_core/src/lib.rs`:
   - Empty `pub mod` declarations for future modules: `types`
   - Re-export key types from `types`

4. Create `crates/strato_core/src/types.rs`:
   - Define shared types: `QualifiedName`, `Location`, `ModulePath`
   - Exactly as specified in Design Doc Section 18 line 2623
   - Include `#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]`

5. Create `crates/strato_cache/Cargo.toml`:
   - Package name `strato_cache`, version `0.1.0`, edition `2021`
   - Minimal dependencies (full deps added in M10)

6. Create `crates/strato_cache/src/lib.rs`:
   - Stub: `// Cache subsystem — implemented in M10`

7. Create `crates/strato_cli/Cargo.toml`:
   - Package name `strato_cli`, version `0.1.0`, edition `2021`
   - `[[bin]]` section: name = `strato`
   - Dependencies: `strato_core = { path = "../strato_core" }`, `clap = { workspace = true }`
   - Dev-dependencies: `strato_core` (for integration tests later)

8. Create `crates/strato_cli/pyproject.toml`:
   - Maturin-based config for PyPI package `strato-cli`
   - As specified in Design Doc Section 17

9. Create `crates/strato_cli/src/main.rs`:
   - Stub: parse `--version` flag with clap, print version, exit
   - `fn main() { /* clap version stub */ }`

10. Create `python/strato/__init__.py`:
    - Import and re-export from `_annotations`: `from strato._annotations import blocking, non_blocking`

11. Create `python/strato/_annotations.py`:
    - Implement `@blocking` and `@non_blocking` decorators
    - Exactly as specified in Design Doc Section 12 (lines ~1880–1940)
    - Use `functools.wraps`, set `__strato_blocking__` / `__strato_non_blocking__` attributes
    - Type-safe: support both `@blocking` and `@blocking(reason="...")` syntax

12. Create `python/strato/py.typed`:
    - Empty file (PEP 561 marker)

13. Create `pyproject.toml` (workspace root — Python annotations package):
    - Package name `strato`, version `0.1.0`
    - Build system: `setuptools` or `hatchling`
    - Packages: `python/strato`

14. Create test infrastructure for integration tests:
    - Create directory `crates/strato_core/tests/integration/`
    - Create `crates/strato_core/tests/integration/main.rs` with `mod harness;` (empty for now, integration test modules added in M11)
    - Create `crates/strato_core/tests/integration/harness.rs` from Design Doc Appendix B (lines 3163–3245)
    - Note: The harness calls `strato_core::analyze()` which doesn't exist yet — make `harness.rs` compile but with the `run_fixture` function body commented out or behind a `#[cfg(feature = "full-pipeline")]` gate until M9 wires it up

15. Create smoke test fixture:
    - `tests/fixtures/smoke/test_smoke.py`:
      ```python
      import time

      async def handler():
          time.sleep(1)  # STRATO001: Direct blocking call
      ```
    - `tests/fixtures/smoke/expected.json`:
      ```json
      {
        "fixture": "smoke",
        "expected_diagnostics": [
          {
            "code": "STRATO001",
            "file": "test_smoke.py",
            "line": 5,
            "message_contains": "Direct blocking call",
            "chain_length": 2,
            "chain_root": "time.sleep"
          }
        ],
        "expected_diagnostic_count": 1,
        "expected_exit_code": 1
      }
      ```

16. **CRITICAL VALIDATION**: Run `cargo build` to verify all ruff crate dependencies resolve and compile at the pinned rev. If compilation fails:
    - Check if `ruff_source_file` needs a different crate path
    - Check if `ruff_text_size` is re-exported from another crate
    - If the pinned rev is incompatible, find the nearest working rev
    - Do NOT proceed to M1 until `cargo build` succeeds

**Must NOT do**:
- Do not implement any analysis logic — M0 is skeleton only
- Do not add unit tests beyond compilation checks
- Do not write `///` doc comments beyond module-level descriptions
- Do not add any blocking database entries
- Do not implement the `analyze()` function

**Recommended Agent Profile**:
- **Category**: `unspecified-high`
  - Reason: Project scaffolding requires careful file creation across multiple crates and languages (Rust + Python), with build system validation. Not purely code but not trivial.
- **Skills**: `[]`
  - No specialized skills needed — standard file creation and cargo commands
- **Skills Evaluated but Omitted**:
  - `git-master`: Not needed — commit is a simple operation
  - `playwright`: No browser interaction
  - `frontend-ui-ux`: No frontend work

**Parallelization**:
- **Can Run In Parallel**: NO
- **Parallel Group**: Sequential — first milestone, no dependencies
- **Blocks**: ALL subsequent TODOs (2–13)
- **Blocked By**: None (start here)

**References**:

**Pattern References**:
- Design Doc Section 18 (lines 2600–2750): Complete repository structure with all file paths
- Design Doc Section 18 (lines 2709–2749): Exact `Cargo.toml` workspace dependencies

**API/Type References**:
- Design Doc Section 12 (lines ~1880–1940): Python `@blocking`/`@non_blocking` decorator implementation
- Design Doc Section 17 (lines ~2450–2600): Distribution and packaging (maturin config)

**Test References**:
- Design Doc Appendix B (lines 3077–3292): Test harness specification with `run_fixture()` function
- Design Doc Section 21 M0 (lines 3320–3350): Exact M0 files and verification

**Documentation References**:
- Design Doc Section 14 (lines ~2200–2400): CLI interface spec (binary naming convention)

**WHY Each Reference Matters**:
- Section 18 contains the exact directory tree and Cargo.toml content to transcribe
- Section 12 has the complete Python decorator implementation including type signatures
- Appendix B has the Rust harness code to copy verbatim (with minor adjustments for compilation)
- Section 14 clarifies binary naming: Cargo package = `strato_cli`, binary name = `strato`

**Acceptance Criteria**:

```bash
# 1. Rust workspace compiles
cargo build
# Assert: exit code 0, no errors

# 2. All crates listed
cargo metadata --format-version=1 | python3 -c "
import sys, json
meta = json.load(sys.stdin)
names = {p['name'] for p in meta['packages'] if p['source'] is None}
assert 'strato_core' in names, 'strato_core missing'
assert 'strato_cache' in names, 'strato_cache missing'
assert 'strato_cli' in names, 'strato_cli missing'
print('All 3 crates present')
"

# 3. Python package importable
PYTHONPATH=python python3 -c "from strato import blocking, non_blocking; print('OK')"
# Assert: prints "OK"

# 4. CLI stub runs
cargo run -p strato_cli -- --version
# Assert: prints version string

# 5. Smoke fixture exists
test -f tests/fixtures/smoke/test_smoke.py && test -f tests/fixtures/smoke/expected.json && echo "Smoke fixture exists"

# 6. Integration test harness compiles (even if tests are gated)
cargo test -p strato_core --no-run
# Assert: exit code 0

# 7. No test failures
cargo test
# Assert: exit code 0 (no tests fail — there may be 0 tests or stub tests)
```

**Commit**: YES
- Message: `M0: project scaffolding — Rust workspace, Python package, test fixtures`
- Files: All created files
- Pre-commit: `cargo build && cargo test`

---

### - [ ] 2. M1: Parser Abstraction + File Discovery

**What to do**:

1. Create `crates/strato_core/src/discovery.rs`:
   - File discovery: walk directory tree, find `.py` files, respect exclude patterns
   - Source root detection: parse `pyproject.toml` for `[tool.setuptools.packages.find]` or detect `src/` layout
   - Auto-detect first-party packages from project layout
   - Implement as specified in Design Doc Section 4 Phase 1 and Section 5 (resolver depends on discovery)

2. Create `crates/strato_core/src/parser.rs`:
   - Define `trait PythonParser` (abstraction over ruff):
     ```rust
     pub trait PythonParser {
         fn parse(&self, source: &str) -> Result<ParsedModule, ParseError>;
     }
     ```
   - Implement `RuffParser` using `ruff_python_parser::parse_module()`
   - Extract `FileSymbols`: function defs (name, is_async, decorators, location), class defs, import statements
   - Handle parse errors gracefully (non-fatal — continue on valid files)
   - As specified in Design Doc Section 4 Phase 2

3. Update `crates/strato_core/src/lib.rs`:
   - Add `pub mod discovery; pub mod parser;`

4. Update `crates/strato_core/Cargo.toml`:
   - Add dependencies: `ruff_python_parser`, `ruff_python_ast`, `rayon`, `toml`, `globset` (all from workspace)

5. Write unit tests (exactly as specified in Design Doc Section 21 M1, lines 3371–3378):
   - `parser::test_parse_simple_function` — Parse a function def, verify AST
   - `parser::test_parse_async_function` — Parse async def, verify `is_async` flag
   - `parser::test_parse_error_non_fatal` — Invalid syntax produces error, doesn't panic
   - `discovery::test_detect_src_layout` — Detect `src/` layout from pyproject.toml
   - `discovery::test_detect_flat_layout` — Detect flat layout
   - `discovery::test_exclude_patterns` — Glob exclusion works

**Must NOT do**:
- Do not implement module resolution (that's M2)
- Do not build a symbol table (that's M2)
- Do not extract call edges from function bodies (that's M3)
- Do not implement parallel parsing yet — sequential is fine for M1

**Recommended Agent Profile**:
- **Category**: `unspecified-high`
  - Reason: Core Rust development with ruff crate integration — requires understanding AST node types and parser API
- **Skills**: `[]`
- **Skills Evaluated but Omitted**:
  - `playwright`: No browser work

**Parallelization**:
- **Can Run In Parallel**: NO
- **Parallel Group**: Sequential
- **Blocks**: TODO 3 (M2: resolver needs parser + discovery)
- **Blocked By**: TODO 1 (M0: workspace must exist)

**References**:

**Pattern References**:
- Design Doc Section 4 (lines ~310–500): Analysis pipeline Phase 1 (Discovery) and Phase 2 (Parse)
- Design Doc Section 5 (lines ~501–700): Module resolver (for understanding what discovery feeds into)

**API/Type References**:
- Design Doc Section 18 lines 2615–2617: File locations for `discovery.rs` and `parser.rs`
- `ruff_python_parser::parse_module()`: The ruff parsing entry point
- `ruff_python_ast::Stmt`, `StmtFunctionDef`, `StmtAsyncFunctionDef`, `StmtClassDef`, `StmtImport`, `StmtImportFrom`: AST node types

**Test References**:
- Design Doc Section 21 M1 (lines 3371–3378): Exact test names and descriptions

**WHY Each Reference Matters**:
- Section 4 defines the pipeline phases — parser.rs implements Phase 2, discovery.rs implements Phase 1
- The ruff AST types are needed to understand what node types to match against
- M1 test names are specified exactly — implement them verbatim

**Acceptance Criteria**:

```bash
# 1. New modules compile
cargo build -p strato_core
# Assert: exit code 0

# 2. All 6 unit tests pass
cargo test -p strato_core parser discovery
# Assert: exit code 0, 6 tests pass

# 3. Parser can handle the smoke fixture
cargo test -p strato_core parser::test_parse_simple_function
cargo test -p strato_core parser::test_parse_async_function
cargo test -p strato_core parser::test_parse_error_non_fatal
# Assert: all pass

# 4. Discovery tests pass
cargo test -p strato_core discovery::test_detect_src_layout
cargo test -p strato_core discovery::test_detect_flat_layout
cargo test -p strato_core discovery::test_exclude_patterns
# Assert: all pass

# 5. Full build still works
cargo build && cargo test
# Assert: exit code 0
```

**Commit**: YES
- Message: `M1: parser abstraction + file discovery — ruff integration, Python file discovery`
- Files: `crates/strato_core/src/discovery.rs`, `crates/strato_core/src/parser.rs`, modified `lib.rs` and `Cargo.toml`
- Pre-commit: `cargo build && cargo test`

---

### - [ ] 3. M2: Module Resolver

**What to do**:

1. Create `crates/strato_core/src/resolver.rs`:
   - Implement `ModuleMap`: maps module paths to file system paths
   - Implement `SymbolTable`: cross-file symbol lookup
   - Resolution algorithm for:
     - Absolute imports (`import myapp.utils`)
     - From imports (`from myapp.utils import helper`)
     - Relative imports (`from . import sibling`, `from ..utils import helper`)
     - Package imports (`from myapp import subpackage` → `__init__.py`)
     - `.pyi` stub resolution (alongside `.py`)
   - Source root ordering: try roots in order, first match wins
   - Unresolvable imports return `None` (not errors)
   - As specified in Design Doc Section 5 (lines ~501–700)

2. Create test fixtures:
   - `tests/fixtures/resolver_basic/` — Simple project with absolute imports
   - `tests/fixtures/resolver_relative/` — Relative imports across packages
   - `tests/fixtures/resolver_init_package/` — `__init__.py` package imports

3. Update `crates/strato_core/src/lib.rs`:
   - Add `pub mod resolver;`

4. Write unit tests (exactly as specified in Design Doc Section 21 M2, lines 3403–3412):
   - `resolver::test_absolute_import`
   - `resolver::test_from_import`
   - `resolver::test_relative_import`
   - `resolver::test_relative_parent_import`
   - `resolver::test_init_package`
   - `resolver::test_unresolvable_returns_none`
   - `resolver::test_source_root_ordering`
   - `resolver::test_pyi_stub_resolution`

**Must NOT do**:
- Do not resolve star imports (`from x import *`) — treat as unresolvable
- Do not handle namespace packages (PEP 420) — require `__init__.py`
- Do not follow circular imports — each module resolved independently based on file paths
- Do not implement conditional imports (try/except) — take first branch only in M2
- Do not build call graph edges (that's M3)

**Recommended Agent Profile**:
- **Category**: `unspecified-high`
  - Reason: Complex module resolution logic with filesystem interactions and multiple edge cases
- **Skills**: `[]`

**Parallelization**:
- **Can Run In Parallel**: NO
- **Parallel Group**: Sequential
- **Blocks**: TODO 4 (M3: call graph needs symbol table)
- **Blocked By**: TODO 2 (M1: resolver needs parser output)

**References**:

**Pattern References**:
- Design Doc Section 5 (lines ~501–700): Complete module resolution algorithm
- Design Doc Section 4 Phase 3 (lines ~380–420): Resolution phase in pipeline

**API/Type References**:
- `crates/strato_core/src/types.rs` (created in M0): `QualifiedName`, `ModulePath` types
- `crates/strato_core/src/discovery.rs` (created in M1): Source root detection used by resolver

**Test References**:
- Design Doc Section 21 M2 (lines 3394–3412): Exact test names and fixture descriptions

**WHY Each Reference Matters**:
- Section 5 IS the resolver specification — it contains the resolution algorithm, lookup order, and edge cases
- Types from M0 are the foundation (QualifiedName is how resolved symbols are keyed)
- M1's discovery provides source roots that the resolver needs to probe

**Acceptance Criteria**:

```bash
# 1. All 8 resolver tests pass
cargo test -p strato_core resolver
# Assert: exit code 0, 8 tests pass

# 2. Resolver fixtures exist
test -d tests/fixtures/resolver_basic && test -d tests/fixtures/resolver_relative && test -d tests/fixtures/resolver_init_package
# Assert: all exist

# 3. Full build still works
cargo build && cargo test
# Assert: exit code 0
```

**Commit**: YES
- Message: `M2: module resolver — cross-file import resolution, symbol table`
- Files: `crates/strato_core/src/resolver.rs`, test fixtures, modified `lib.rs`
- Pre-commit: `cargo build && cargo test`

---

### - [ ] 4. M3: Call Graph Data Structures + Construction

**What to do**:

1. Create `crates/strato_core/src/graph.rs`:
   - Define `CallGraph` (wrapping `petgraph::DiGraph`)
   - Define `CallGraphNode`: `FunctionNode`, `PhantomNode` (for external blocking functions)
   - Define `CallEdge`: `DirectCall`, `PropertyAccess`, `ImplicitDunder`, with `in_executor: bool`
   - Define `BlockingStatus`: `Unknown`, `KnownBlocking`, `KnownNonBlocking`, `PropagatedBlocking`
   - Define `BlockingReason`: `root_cause`, `call_chain: Vec<ChainLink>`
   - Define `ChainLink`: `function_name`, `function_location`, `call_site_location`
   - All types as specified in Design Doc Section 6 (lines ~700–920)

2. Create `crates/strato_core/src/graph_builder.rs`:
   - Implement `CallEdgeVisitor`: AST walker that extracts call edges from function bodies
   - Callee resolution: resolve `foo()`, `obj.method()`, `self.method()` to graph nodes
   - Simple type inference (Design Doc Section 6 lines 888–914):
     - `self` → current class
     - `cls` → current class
     - `MyClass()` → `MyClass`
     - Direct imports → resolved symbol
     - Everything else → Unknown (skip)
   - `ScopeBindings`: per-function bindings tracking
   - Pre-seed phantom nodes from blocking database stub (empty for now — populated in M4)

3. Update `crates/strato_core/src/lib.rs`:
   - Add `pub mod graph; pub mod graph_builder;`

4. Update `crates/strato_core/Cargo.toml`:
   - Add `petgraph = { workspace = true }`

5. Write unit tests (Design Doc Section 21 M3, lines 3434–3441):
   - `graph_builder::test_direct_call_edge`
   - `graph_builder::test_method_call_edge`
   - `graph_builder::test_self_method_call`
   - `graph_builder::test_unresolvable_call_skipped`
   - `graph_builder::test_lambda_node`
   - `graph_builder::test_simple_type_inference_constructor`
   - `graph_builder::test_simple_type_inference_self`

**Must NOT do**:
- Do not implement executor detection (that's M6)
- Do not implement property/dunder detection (that's M7)
- Do not implement blocking propagation (that's M5)
- Do not implement type inference beyond the 4 simple patterns
- Do not use `HashMap` for any collection that affects output ordering (use `BTreeMap`)

**Recommended Agent Profile**:
- **Category**: `unspecified-high`
  - Reason: Complex data structure design + AST walking — core of the analysis engine
- **Skills**: `[]`

**Parallelization**:
- **Can Run In Parallel**: NO
- **Parallel Group**: Sequential
- **Blocks**: TODO 5 (M4: database needs graph types)
- **Blocked By**: TODO 3 (M2: graph builder needs resolved symbols)

**References**:

**Pattern References**:
- Design Doc Section 6 (lines ~700–920): Complete call graph specification — node types, edge types, type inference rules
- Design Doc Section 4 Phase 4 (lines ~420–460): Build phase in pipeline

**API/Type References**:
- `petgraph::DiGraph`: Underlying graph data structure
- `petgraph::graph::NodeIndex`: Node handle type
- `crates/strato_core/src/resolver.rs` (M2): `SymbolTable` for callee resolution
- `crates/strato_core/src/types.rs` (M0): `QualifiedName`, `Location`

**Test References**:
- Design Doc Section 21 M3 (lines 3418–3441): Exact test names

**WHY Each Reference Matters**:
- Section 6 is THE call graph spec — every type, every field, every edge variant is defined there
- `petgraph` is the graph library; nodes are `CallGraphNode`, edges are `CallEdge`
- The resolver provides `SymbolTable` which the graph builder queries to resolve callees

**Acceptance Criteria**:

```bash
# 1. All 7 graph tests pass
cargo test -p strato_core graph
# Assert: exit code 0, 7 tests pass

# 2. Graph builder tests specifically
cargo test -p strato_core graph_builder
# Assert: all pass

# 3. Full build still works
cargo build && cargo test
# Assert: exit code 0
```

**Commit**: YES
- Message: `M3: call graph construction — petgraph-based graph, AST edge extraction, simple type inference`
- Files: `crates/strato_core/src/graph.rs`, `crates/strato_core/src/graph_builder.rs`, modified `lib.rs`, `Cargo.toml`
- Pre-commit: `cargo build && cargo test`

---

### - [ ] 5. M4: Blocking Database + Annotation Detection

**What to do**:

1. Create `crates/strato_core/src/annotator.rs`:
   - Detect `@blocking` and `@non_blocking` decorators on functions
   - Match by decorator name pattern (NOT by import resolution):
     - `@blocking`, `@strato.blocking`
     - `@non_blocking`, `@strato.non_blocking`
   - Set `BlockingStatus::KnownBlocking` or `KnownNonBlocking` on matching nodes
   - As specified in Design Doc Section 12 (lines ~1880–1960)

2. Create `crates/strato_core/src/database/mod.rs`:
   - Define `BlockingDatabase` struct: lookup by `QualifiedName`
   - `BlockingEntry`: qualified name, reason category, help text, async alternative
   - Methods: `is_blocking()`, `get_entry()`, `add_user_entry()`, `remove_entry()`

3. Create `crates/strato_core/src/database/stdlib.rs`:
   - Built-in entries for stdlib blocking functions
   - Exactly from Design Doc Section 9 tables:
     - `time.sleep`, `time.time` (blocking variant), etc.
     - `builtins.open`, `builtins.input`, `builtins.print` (when blocking)
     - `os.*` blocking calls, `shutil.*`, `pathlib.*`

4. Create `crates/strato_core/src/database/network.rs`:
   - `requests.*`, `urllib.*`, `http.client.*`, `socket.*`

5. Create `crates/strato_core/src/database/database.rs`:
   - `psycopg2.*`, `sqlite3.*`, `pymysql.*`

6. Create `crates/strato_core/src/database/subprocess.rs`:
   - `subprocess.*`, `os.system`, `os.popen`

7. Update `crates/strato_core/src/lib.rs`:
   - Add `pub mod annotator; pub mod database;`

8. Wire phantom nodes: Update `graph_builder.rs` to pre-seed `CallGraph` with phantom nodes for all blocking database entries

9. Write unit tests (Design Doc Section 21 M4, lines 3468–3477):
   - `database::test_builtin_entries_complete` — verify ALL Section 9 entries are present
   - `database::test_fixture_required_entries` — `time.sleep`, `requests.get` present
   - `database::test_time_sleep_help_message` — help text exists
   - `database::test_user_config_add` — custom entry addable
   - `database::test_user_config_remove` — built-in entry removable
   - `annotator::test_detect_blocking_decorator`
   - `annotator::test_detect_non_blocking_decorator`
   - `annotator::test_detect_strato_dot_blocking`
   - `annotator::test_ignore_unrelated_decorator`

**Must NOT do**:
- Do not add blocking entries beyond Section 9's tables (Guardrail G9)
- Do not resolve `from strato import blocking` as an import — match by decorator name pattern only
- Do not implement propagation logic (that's M5)

**Recommended Agent Profile**:
- **Category**: `unspecified-high`
  - Reason: Data-entry heavy (80+ blocking entries) plus decorator pattern matching
- **Skills**: `[]`

**Parallelization**:
- **Can Run In Parallel**: NO
- **Parallel Group**: Sequential
- **Blocks**: TODO 6 (M5: propagation needs blocking annotations)
- **Blocked By**: TODO 4 (M3: needs graph types for phantom nodes)

**References**:

**Pattern References**:
- Design Doc Section 9 (lines ~1200–1450): Complete blocking database tables — EVERY entry listed
- Design Doc Section 12 (lines ~1880–1960): Annotation API and decorator detection

**API/Type References**:
- `crates/strato_core/src/graph.rs` (M3): `CallGraphNode::PhantomNode`, `BlockingStatus::KnownBlocking`
- `crates/strato_core/src/types.rs` (M0): `QualifiedName` for database keys

**Test References**:
- Design Doc Section 21 M4 (lines 3466–3477): Exact test names

**WHY Each Reference Matters**:
- Section 9 is the ONLY source of blocking database entries — transcribe exactly, don't add extras
- Section 12 specifies decorator matching semantics — not import-based, pattern-based
- Graph types from M3 define how phantom nodes are represented

**Acceptance Criteria**:

```bash
# 1. Annotator tests pass
cargo test -p strato_core annotator
# Assert: exit code 0, 4 tests pass

# 2. Database tests pass
cargo test -p strato_core database
# Assert: exit code 0, 5 tests pass

# 3. Full build still works
cargo build && cargo test
# Assert: exit code 0
```

**Commit**: YES
- Message: `M4: blocking database + annotation detection — 80+ entries, decorator matching`
- Files: `crates/strato_core/src/annotator.rs`, `crates/strato_core/src/database/` (all files), modified `lib.rs`, `graph_builder.rs`
- Pre-commit: `cargo build && cargo test`

---

### - [ ] 6. M5: Blocking Propagation (SCC Algorithm)

**What to do**:

1. Create `crates/strato_core/src/propagator.rs`:
   - Implement Tarjan's SCC decomposition using `petgraph::algo::tarjan_scc()`
   - Build condensation graph (DAG of SCCs)
   - Topological sort the condensation DAG
   - Propagate blocking status in reverse topological order:
     - Within each SCC: if ANY member is `KnownBlocking`, ALL members become `PropagatedBlocking` (unless `@non_blocking`)
     - Between SCCs: if callee SCC is blocking AND edge is NOT `in_executor`, caller SCC becomes `PropagatedBlocking`
   - Build `BlockingReason` with complete `call_chain: Vec<ChainLink>` for each propagated node
   - **SACRED INVARIANT**: `Unknown` nodes MUST stay `Unknown`. Never reclassify to `NotBlocking`. (Guardrail G1)
   - `@non_blocking` within SCC: shields entire SCC from propagation (user assertion)
   - O(V+E) guaranteed — single pass, no iteration
   - As specified in Design Doc Section 7 (lines ~920–1200)

2. Update `crates/strato_core/src/lib.rs`:
   - Add `pub mod propagator;`

3. Write unit tests (Design Doc Section 21 M5, lines 3498–3507):
   - `propagator::test_direct_blocking_propagation` — A calls B(blocking) → A PropagatedBlocking
   - `propagator::test_transitive_propagation` — A→B→C(blocking) → A,B PropagatedBlocking
   - `propagator::test_executor_edge_blocks_propagation` — executor edge stops propagation
   - `propagator::test_non_blocking_stops_propagation` — @non_blocking stops propagation
   - `propagator::test_cycle_handling` — Mutual recursion with blocking → both blocking
   - `propagator::test_cycle_no_blocking` — Mutual recursion, no blocking → both Unknown
   - `propagator::test_unknown_stays_unknown` — Unknown remains Unknown
   - `propagator::test_blocking_reason_path` — call_chain is correct

**Must NOT do**:
- Do not implement iterative fixpoint — SCC decomposition ensures single-pass
- Do not reclassify `Unknown` nodes (SACRED INVARIANT — Guardrail G1)
- Do not optimize performance (Guardrail G7)
- Do not handle executor edge detection (that was M3 edge type, M6 adds detection logic)

**Recommended Agent Profile**:
- **Category**: `ultrabrain`
  - Reason: SCC algorithm + topological propagation is the most algorithmically complex task. Requires deep understanding of graph theory, Tarjan's algorithm, and the design's specific propagation rules. A single bug here breaks the entire analysis.
- **Skills**: `[]`

**Parallelization**:
- **Can Run In Parallel**: NO
- **Parallel Group**: Sequential
- **Blocks**: TODO 7 (M6: escape hatches interact with propagation)
- **Blocked By**: TODO 5 (M4: needs annotated graph with blocking status)

**References**:

**Pattern References**:
- Design Doc Section 7 (lines ~920–1200): COMPLETE propagation algorithm — SCC decomposition, condensation, topological sort, propagation rules, @non_blocking in SCC handling
- Design Doc Section 6 (lines ~860–880): `BlockingStatus` enum definition

**API/Type References**:
- `petgraph::algo::tarjan_scc()`: Returns `Vec<Vec<NodeIndex>>` — SCCs
- `crates/strato_core/src/graph.rs` (M3): `CallGraph`, `BlockingStatus`, `BlockingReason`, `ChainLink`

**Test References**:
- Design Doc Section 21 M5 (lines 3498–3507): Exact test names and expected behaviors

**WHY Each Reference Matters**:
- Section 7 IS the propagation algorithm — every rule, every edge case, every SCC handling detail
- `petgraph::algo::tarjan_scc` provides the SCC decomposition; must understand its return format
- The `BlockingStatus` and `BlockingReason` types from M3 are what the propagator reads and writes

**Acceptance Criteria**:

```bash
# 1. All 8 propagation tests pass
cargo test -p strato_core propagator
# Assert: exit code 0, 8 tests pass

# 2. Specifically verify the sacred invariant
cargo test -p strato_core propagator::test_unknown_stays_unknown
# Assert: pass

# 3. Full build still works
cargo build && cargo test
# Assert: exit code 0
```

**Commit**: YES
- Message: `M5: blocking propagation — SCC-based single-pass algorithm, O(V+E)`
- Files: `crates/strato_core/src/propagator.rs`, modified `lib.rs`
- Pre-commit: `cargo build && cargo test`

---

### - [ ] 7. M6: Escape Hatch Recognition

**What to do**:

1. Modify `crates/strato_core/src/graph_builder.rs`:
   - Add `is_executor_call()` function: detect `run_in_executor` and `asyncio.to_thread` patterns
   - Add `is_likely_event_loop()`: recognize `asyncio.get_running_loop()`, `asyncio.get_event_loop()` assignments
   - Mark `CallEdge.in_executor = true` for protected callable arguments:
     - `run_in_executor(executor, func)` → position 1 is protected
     - `asyncio.to_thread(func)` → position 0 is protected
   - Handle `functools.partial(func, ...)` wrapping
   - High precision rule: if receiver is unknown variable (not proven to be event loop), do NOT mark as executor call
   - As specified in Design Doc Section 11 (lines ~1700–1870)

2. Write unit tests (Design Doc Section 21 M6, lines 3525–3531):
   - `graph_builder::test_executor_run_in_executor`
   - `graph_builder::test_executor_to_thread`
   - `graph_builder::test_executor_only_callable_arg_protected`
   - `graph_builder::test_executor_partial_wrapping`
   - `graph_builder::test_is_likely_event_loop_direct`
   - `graph_builder::test_is_likely_event_loop_variable`
   - `graph_builder::test_is_likely_event_loop_unknown_var`

**Must NOT do**:
- Do not recognize trio/anyio escape hatches (Guardrail G3 — v2 feature)
- Do not recognize `x.run_in_executor()` if `x` is not proven to be an event loop (high precision)
- Do not add framework-specific escape hatches (Django `sync_to_async`, etc.)

**Recommended Agent Profile**:
- **Category**: `unspecified-high`
  - Reason: Pattern matching logic within AST walker — extends M3's graph_builder with executor detection
- **Skills**: `[]`

**Parallelization**:
- **Can Run In Parallel**: NO
- **Parallel Group**: Sequential
- **Blocks**: TODO 8 (M7: property/dunder detection extends same file)
- **Blocked By**: TODO 6 (M5: executor edges affect propagation which must work first)

**References**:

**Pattern References**:
- Design Doc Section 11 (lines ~1700–1870): Escape hatch recognition patterns, synthetic edge rules

**API/Type References**:
- `crates/strato_core/src/graph_builder.rs` (M3): Existing `CallEdgeVisitor` to extend
- `crates/strato_core/src/graph.rs` (M3): `CallEdge.in_executor` field

**Test References**:
- Design Doc Section 21 M6 (lines 3516–3531): Exact test names

**WHY Each Reference Matters**:
- Section 11 specifies exactly which patterns to recognize and the precision rules for unknown receivers
- The graph_builder from M3 is the file being modified — must understand its visitor pattern
- `CallEdge.in_executor` is the field that M5's propagator checks

**Acceptance Criteria**:

```bash
# 1. All 7 executor tests pass
cargo test -p strato_core graph_builder::test_executor
# Assert: exit code 0, 7 tests pass

# 2. Full build still works
cargo build && cargo test
# Assert: exit code 0
```

**Commit**: YES
- Message: `M6: escape hatch recognition — run_in_executor, asyncio.to_thread`
- Files: modified `crates/strato_core/src/graph_builder.rs`
- Pre-commit: `cargo build && cargo test`

---

### - [ ] 8. M7: Properties and Dunder Methods

**What to do**:

1. Modify `crates/strato_core/src/graph_builder.rs`:
   - **Property detection**: When encountering `obj.attr` (attribute access), check if `attr` is a `@property` getter on the resolved type. If so, create `CallEdge::PropertyAccess` edge.
   - **Dunder method mapping**: Map Python syntax to implicit dunder calls:
     - `str(obj)` → `obj.__str__()`
     - `a == b` → `a.__eq__(b)`
     - `x[k]` → `x.__getitem__(k)`
     - `with x:` → `x.__enter__()` + `x.__exit__()`
     - `for i in x:` → `x.__iter__()`
     - `f"{x}"` → `x.__format__("")`
     - Full table in Design Doc Section 10
   - **Context manager detection**: `StmtWith` → edges to `__enter__` and `__exit__`
   - **High precision**: If type is unknown, do NOT create dunder edge (skip silently)
   - As specified in Design Doc Section 10 (lines ~1450–1700)

2. Write unit tests (Design Doc Section 21 M7, lines 3551–3559):
   - `graph_builder::test_property_access_creates_edge`
   - `graph_builder::test_property_non_property_attribute_no_edge`
   - `graph_builder::test_dunder_str_builtin`
   - `graph_builder::test_dunder_eq_operator`
   - `graph_builder::test_dunder_getitem`
   - `graph_builder::test_dunder_with_statement`
   - `graph_builder::test_dunder_for_loop`
   - `graph_builder::test_dunder_fstring`
   - `graph_builder::test_dunder_unknown_type_skipped`

**Must NOT do**:
- Do not detect `@cached_property` (only `@property`)
- Do not resolve inherited methods (no MRO traversal)
- Do not create dunder edges for unknown types (Guardrail G2 — high precision)
- Do not handle `__aenter__`/`__aexit__` (these are async context managers, not blocking)

**Recommended Agent Profile**:
- **Category**: `unspecified-high`
  - Reason: Complex AST pattern matching with many edge cases (9 dunder patterns + property detection)
- **Skills**: `[]`

**Parallelization**:
- **Can Run In Parallel**: NO
- **Parallel Group**: Sequential
- **Blocks**: TODO 9 (M8: reporter needs all edge types)
- **Blocked By**: TODO 7 (M6: must be after escape hatches in same file)

**References**:

**Pattern References**:
- Design Doc Section 10 (lines ~1450–1700): Properties and dunder method specification — full mapping table

**API/Type References**:
- `crates/strato_core/src/graph.rs` (M3): `CallEdge::PropertyAccess`, `CallEdge::ImplicitDunder`
- `crates/strato_core/src/graph_builder.rs` (M3/M6): Existing visitor to extend

**Test References**:
- Design Doc Section 21 M7 (lines 3540–3559): Exact test names

**WHY Each Reference Matters**:
- Section 10 has the complete dunder mapping table — every Python syntax → dunder function
- The edge types `PropertyAccess` and `ImplicitDunder` are what M8's reporter checks for error code classification

**Acceptance Criteria**:

```bash
# 1. Property tests pass
cargo test -p strato_core graph_builder::test_property
# Assert: exit code 0, 2 tests pass

# 2. Dunder tests pass
cargo test -p strato_core graph_builder::test_dunder
# Assert: exit code 0, 7 tests pass

# 3. Full build still works
cargo build && cargo test
# Assert: exit code 0
```

**Commit**: YES
- Message: `M7: property + dunder detection — @property getters, 6 dunder patterns`
- Files: modified `crates/strato_core/src/graph_builder.rs`
- Pre-commit: `cargo build && cargo test`

---

### - [ ] 9. M8: Diagnostic Reporting

**What to do**:

1. Create `crates/strato_core/src/reporter.rs`:
   - Define `Diagnostic`: code, severity, message, primary_location, blocking_chain, help text
   - Define `DiagnosticSet`: collection of diagnostics with deterministic ordering
   - **Intervention point strategy**:
     - `first-party-deepest` (default): Select deepest first-party function in the chain as primary location
     - `async-boundary`: Select the async→sync transition point
   - **Error code classification** (Guardrail G6):
     - STRATO001: `chain_length == 1` AND caller is async (direct blocking in async)
     - STRATO002: `chain_length > 1` (indirect via intermediaries)
     - STRATO003: Last edge is `PropertyAccess`
     - STRATO004: Last edge is `ImplicitDunder`
   - **Deterministic output**: Sort diagnostics by (file path, line, column, code) using `BTreeMap` or explicit sort (Guardrail G5)
   - **Help text**: Pull from `BlockingDatabase` entry for root cause, `None` if not in DB
   - As specified in Design Doc Section 8 (lines ~1200–1450)

2. Update `crates/strato_core/src/lib.rs`:
   - Add `pub mod reporter;`

3. Write unit tests (Design Doc Section 21 M8, lines 3581–3589):
   - `reporter::test_first_party_deepest_strategy`
   - `reporter::test_async_boundary_strategy`
   - `reporter::test_all_third_party_fallback`
   - `reporter::test_error_code_strato001`
   - `reporter::test_error_code_strato002`
   - `reporter::test_error_code_strato003`
   - `reporter::test_error_code_strato004`
   - `reporter::test_diagnostic_message_format`

**Must NOT do**:
- Do not format output (that's M9 — text/JSON/SARIF formatters)
- Do not implement the CLI (that's M9)
- Do not add colors or pretty-printing (that's M9's miette integration)

**Recommended Agent Profile**:
- **Category**: `unspecified-high`
  - Reason: Diagnostic generation logic with strategy pattern and error code classification
- **Skills**: `[]`

**Parallelization**:
- **Can Run In Parallel**: NO
- **Parallel Group**: Sequential
- **Blocks**: TODO 10 (M9: CLI needs diagnostics)
- **Blocked By**: TODO 8 (M7: needs all edge types for error code classification)

**References**:

**Pattern References**:
- Design Doc Section 8 (lines ~1200–1450): Complete error reporting model — intervention strategies, error codes, diagnostic format
- Design Doc Section 21 M8 (lines 3565–3595): Implementation notes for M8

**API/Type References**:
- `crates/strato_core/src/graph.rs` (M3): `BlockingReason`, `ChainLink`, `CallEdge` variants
- `crates/strato_core/src/database/mod.rs` (M4): `BlockingDatabase` for help text lookup

**Test References**:
- Design Doc Section 21 M8 (lines 3581–3589): Exact test names

**WHY Each Reference Matters**:
- Section 8 defines exactly how diagnostics are generated, including the intervention strategy algorithm
- Error code classification depends on `CallEdge` variants from M3 and chain length from M5
- Database help text from M4 is included in diagnostics

**Acceptance Criteria**:

```bash
# 1. All 8 reporter tests pass
cargo test -p strato_core reporter
# Assert: exit code 0, 8 tests pass

# 2. Full build still works
cargo build && cargo test
# Assert: exit code 0
```

**Commit**: YES
- Message: `M8: diagnostic reporting — intervention strategies, STRATO001-004 error codes`
- Files: `crates/strato_core/src/reporter.rs`, modified `lib.rs`
- Pre-commit: `cargo build && cargo test`

---

### - [ ] 10. M9: CLI + Output Formats

**What to do**:

1. Create `crates/strato_cli/src/args.rs`:
   - CLI argument parsing with clap (derive API):
     - `strato check <paths>` — main command
     - `--format text|json|sarif` (default: text)
     - `--config <path>` (default: auto-detect pyproject.toml)
     - `--first-party <module>` (override first-party detection)
     - `--severity error|warning`
     - `--intervention first-party-deepest|async-boundary`
     - `--no-cache`, `--clear-cache`
     - `--stats` (show analysis statistics)
     - `--version`
   - As specified in Design Doc Section 14 (lines ~2200–2400)

2. Create `crates/strato_cli/src/config.rs`:
   - Parse `[tool.strato]` from pyproject.toml
   - Map config keys to `strato_core::Config`
   - CLI flags override config file values
   - As specified in Design Doc Section 13 (lines ~2100–2200)

3. Create `crates/strato_cli/src/output/mod.rs`:
   - `trait OutputFormatter { fn format(&self, result: &AnalysisResult) -> String; }`

4. Create `crates/strato_cli/src/output/text.rs`:
   - Text formatter using miette for pretty error display
   - Show chain paths, help text, underline primary location
   - As specified in Design Doc Section 15 (lines ~2400–2500)

5. Create `crates/strato_cli/src/output/json.rs`:
   - JSON formatter: `{ "version": "1.0", "diagnostics": [...], "stats": {...} }`
   - 0-based columns in JSON output (Design Doc convention)
   - As specified in Design Doc Section 15

6. Create `crates/strato_cli/src/output/sarif.rs`:
   - SARIF v2.1.0 formatter
   - Required fields: `version`, `runs[].tool.driver`, `runs[].results`
   - Rule definitions: STRATO001–004
   - As specified in Design Doc Section 15

7. **CRITICAL**: Create `pub fn analyze()` orchestrator in `crates/strato_core/src/lib.rs`:
   - Wire up the full 7-phase pipeline: Discovery → Parse → Resolve → Build Call Graph → Annotate → Propagate → Report
   - Signature: `pub fn analyze(project_path: &Path, config: &Config) -> Result<AnalysisResult, AnalysisError>`
   - Define `Config` struct and `AnalysisResult` struct
   - This is the library API that integration tests (M11) call directly

8. Update `crates/strato_cli/src/main.rs`:
   - Wire: parse args → load config → call `strato_core::analyze()` → format output → set exit code
   - Exit codes: 0 (clean), 1 (issues found), 2 (config error), 3 (all files failed to parse)

9. Update `crates/strato_cli/Cargo.toml`:
   - Add dependencies: `strato_core`, `clap`, `serde_json`, `miette`

**Must NOT do**:
- Do not implement `strato init` or any other subcommand (Guardrail G10)
- Do not implement caching integration (that's M10)
- Do not implement watch mode (Guardrail G3)
- Do not add autofix (Guardrail G3)

**Recommended Agent Profile**:
- **Category**: `unspecified-high`
  - Reason: Multi-file CLI assembly + 3 output formatters + pipeline orchestration — largest single milestone
- **Skills**: `[]`

**Parallelization**:
- **Can Run In Parallel**: NO
- **Parallel Group**: Sequential
- **Blocks**: TODO 11 (M10: caching integrates into CLI pipeline)
- **Blocked By**: TODO 9 (M8: needs diagnostic types)

**References**:

**Pattern References**:
- Design Doc Section 14 (lines ~2200–2400): CLI interface specification — all flags, exit codes
- Design Doc Section 15 (lines ~2400–2500): Output format specifications (text, JSON, SARIF)
- Design Doc Section 13 (lines ~2100–2200): Configuration (pyproject.toml `[tool.strato]`)
- Design Doc Section 4 (lines ~310–500): Pipeline overview (phases to wire together)

**API/Type References**:
- `crates/strato_core/src/*.rs` (M1–M8): All phases to orchestrate
- `clap::Parser` derive macro: CLI argument parsing
- `miette::Report`: Pretty error display
- `serde_json::to_string_pretty()`: JSON formatting

**Test References**:
- Design Doc Section 21 M9 (lines 3600–3621): Verification commands

**WHY Each Reference Matters**:
- Section 14 is the CLI spec — flags, exit codes, binary naming
- Section 15 is the output format spec — exact JSON schema, SARIF structure
- Section 13 defines config file parsing
- Section 4 defines the pipeline order that `analyze()` must follow

**Acceptance Criteria**:

```bash
# 1. Binary compiles
cargo build -p strato_cli
# Assert: exit code 0

# 2. Help output
cargo run -p strato_cli -- check --help
# Assert: shows all options (--format, --first-party, --severity, etc.)

# 3. Smoke test produces output (JSON)
cargo run -p strato_cli -- check tests/fixtures/smoke/ --format json 2>/dev/null | python3 -c "
import sys, json
d = json.load(sys.stdin)
assert 'diagnostics' in d, 'missing diagnostics key'
assert len(d['diagnostics']) > 0, 'no diagnostics found'
assert d['diagnostics'][0]['code'] == 'STRATO001', 'wrong code'
print('Smoke test passed')
"
# Assert: prints "Smoke test passed"

# 4. Version flag
cargo run -p strato_cli -- --version
# Assert: prints version

# 5. Full build still works
cargo build && cargo test
# Assert: exit code 0
```

**Commit**: YES
- Message: `M9: CLI + output formats — strato check with text/JSON/SARIF, full pipeline wired`
- Files: `crates/strato_cli/src/args.rs`, `config.rs`, `output/mod.rs`, `output/text.rs`, `output/json.rs`, `output/sarif.rs`, modified `main.rs`, `Cargo.toml`; modified `crates/strato_core/src/lib.rs` (analyze function)
- Pre-commit: `cargo build && cargo test`

---

### - [ ] 11. M10: Caching System

**What to do**:

1. Create `crates/strato_cache/src/manifest.rs`:
   - Cache manifest: maps file paths to content hashes (SHA-256)
   - Persisted to `.strato-cache/manifest.json` (or binary format)

2. Create `crates/strato_cache/src/storage.rs`:
   - Binary cache read/write using bincode
   - Cached data per file: `FileSymbols`, import statements, call edges
   - Cache location: `.strato-cache/` directory in project root

3. Create `crates/strato_cache/src/invalidation.rs`:
   - Cache invalidation logic:
     - File content hash changed → re-parse that file
     - File added/deleted → invalidate affected modules
     - Config changed → invalidate all

4. Update `crates/strato_cache/src/lib.rs`:
   - Public API: `Cache::load()`, `Cache::save()`, `Cache::is_fresh()`

5. Update `crates/strato_cache/Cargo.toml`:
   - Add dependencies: `bincode`, `serde`, `sha2`, `serde_json` (all from workspace)

6. Integrate cache into CLI pipeline (`crates/strato_cli/src/main.rs`):
   - Before parse: check cache, skip cached files
   - After analysis: save updated cache
   - `--no-cache`: ignore cache, run fresh
   - `--clear-cache`: delete cache, then run fresh
   - `--stats`: show cache hit/miss counts
   - As specified in Design Doc Section 16 (lines ~2500–2600)

7. Add `strato_cache` dependency to `strato_cli`'s `Cargo.toml`

8. Write unit tests:
   - `cache::test_fresh_creates_cache` — first run creates cache directory
   - `cache::test_cached_run_hits` — second run shows cache hits
   - `cache::test_modified_file_invalidates` — changing file content invalidates its cache
   - `cache::test_no_cache_flag` — `--no-cache` skips cache
   - `cache::test_clear_cache_flag` — `--clear-cache` deletes existing cache
   - **Cache correctness test**: fresh run diagnostics == cached run diagnostics (byte-identical)

**Must NOT do**:
- Do not implement incremental graph updates (v2 feature)
- Do not cache across different config versions (invalidate on config change)
- Do not optimize cache format for performance (correctness first — G7)

**Recommended Agent Profile**:
- **Category**: `unspecified-high`
  - Reason: Cache system with serialization, hashing, and invalidation logic across 2 crates
- **Skills**: `[]`

**Parallelization**:
- **Can Run In Parallel**: NO
- **Parallel Group**: Sequential
- **Blocks**: TODO 12 (M11: integration tests need full pipeline including cache)
- **Blocked By**: TODO 10 (M9: CLI pipeline to integrate cache into)

**References**:

**Pattern References**:
- Design Doc Section 16 (lines ~2500–2600): Caching strategy specification

**API/Type References**:
- `crates/strato_cli/src/main.rs` (M9): Pipeline to add cache checks
- `bincode::serialize()` / `bincode::deserialize()`: Binary serialization
- `sha2::Sha256`: Content hashing

**Test References**:
- Design Doc Section 21 M10 (lines 3626–3654): Verification commands

**WHY Each Reference Matters**:
- Section 16 defines cache format, location, invalidation rules
- M9's CLI main.rs is where cache checks are inserted into the pipeline
- Types from M1–M8 need `Serialize`/`Deserialize` derives (added progressively — G11)

**Acceptance Criteria**:

```bash
# 1. Cache crate compiles
cargo build -p strato_cache
# Assert: exit code 0

# 2. First run creates cache
cargo run -p strato_cli -- check tests/fixtures/smoke/ --stats 2>&1 | grep -i "cache"
# Assert: shows cache miss or cache created

# 3. Second run hits cache
cargo run -p strato_cli -- check tests/fixtures/smoke/ --stats 2>&1 | grep -i "cache"
# Assert: shows cache hit

# 4. --no-cache ignores cache
cargo run -p strato_cli -- check tests/fixtures/smoke/ --no-cache --stats 2>&1 | grep -i "cache"
# Assert: shows no cache used

# 5. Cache correctness: fresh == cached diagnostics
diff <(cargo run -p strato_cli -- check tests/fixtures/smoke/ --no-cache --format json 2>/dev/null) <(cargo run -p strato_cli -- check tests/fixtures/smoke/ --format json 2>/dev/null)
# Assert: no diff (identical output)

# 6. Full build still works
cargo build && cargo test
# Assert: exit code 0
```

**Commit**: YES
- Message: `M10: caching system — incremental analysis with SHA-256 content hashing`
- Files: `crates/strato_cache/src/manifest.rs`, `storage.rs`, `invalidation.rs`, modified `lib.rs`, `Cargo.toml`; modified CLI `main.rs`
- Pre-commit: `cargo build && cargo test`

---

### - [ ] 12. M11: Integration Tests (Appendix A)

**What to do**:

1. Create ALL 13 fixture directories with Python source files and `expected.json`:
   - `tests/fixtures/a01_direct_blocking/` — A1 (Design Doc lines 2865–2875)
   - `tests/fixtures/a02_indirect_blocking/` — A2 (lines 2877–2890)
   - `tests/fixtures/a03_executor_safe/` — A3 (lines 2892–2904)
   - `tests/fixtures/a04_to_thread_safe/` — A4 (lines 2906–2917)
   - `tests/fixtures/a05_sync_only_safe/` — A5 (lines 2919–2929)
   - `tests/fixtures/a06_blocking_annotation/` — A6 (lines 2931–2945)
   - `tests/fixtures/a07_non_blocking_override/` — A7 (lines 2947–2962)
   - `tests/fixtures/a08_property_blocking/` — A8 (lines 2964–2980)
   - `tests/fixtures/a09_dunder_blocking/` — A9 (lines 2982–2997)
   - `tests/fixtures/a10_cross_file/` — A10 (lines 2999–3017) — multi-file fixture
   - `tests/fixtures/a11_deep_transitive/` — A11 (lines 3019–3038)
   - `tests/fixtures/a12_multiple_callers/` — A12 (lines 3040–3056)
   - `tests/fixtures/a13_mixed_safe_unsafe/` — A13 (lines 3058–3073)

2. For each fixture, create `expected.json` following the golden output format in Design Doc Appendix B (lines 3127–3156)

3. Update integration test harness `crates/strato_core/tests/integration/harness.rs`:
   - Uncomment/enable the `run_fixture()` function (gated in M0)
   - Wire to `strato_core::analyze()`

4. Create integration test files under `crates/strato_core/tests/integration/`:
   - `test_direct_blocking.rs` → `run_fixture("a01_direct_blocking")`
   - `test_indirect_blocking.rs` → `run_fixture("a02_indirect_blocking")`
   - `test_executor.rs` → A3 + A4
   - `test_sync_only.rs` → A5
   - `test_annotations.rs` → A6 + A7
   - `test_property.rs` → A8
   - `test_dunder.rs` → A9
   - `test_cross_file.rs` → A10
   - `test_deep_transitive.rs` → A11
   - `test_multiple_callers.rs` → A12
   - `test_mixed.rs` → A13
   - `test_output_formats.rs` → JSON schema + SARIF schema + exit code tests (Design Doc M12 lines 3757–3808)

5. Update `crates/strato_core/tests/integration/main.rs` to include all test modules

6. Verify ALL 13 tests pass. If a test fails, fix the implementation (M1–M10 code), NOT the test expectations.

**Must NOT do**:
- Do not modify expected.json to match broken behavior — fix the code instead
- Do not add extra test cases beyond the 13 in Appendix A (Guardrail G8)
- Do not add test infrastructure beyond what Appendix B specifies

**Recommended Agent Profile**:
- **Category**: `unspecified-high`
  - Reason: Many fixture files + integration test wiring + potential implementation bug fixes
- **Skills**: `[]`

**Parallelization**:
- **Can Run In Parallel**: NO
- **Parallel Group**: Sequential
- **Blocks**: TODO 13 (M12: performance tests need working pipeline)
- **Blocked By**: TODO 11 (M10: full pipeline with caching must be operational)

**References**:

**Pattern References**:
- Design Doc Appendix A (lines 2840–3073): ALL 13 test case specifications — exact Python code, expected diagnostics, chain lengths
- Design Doc Appendix B (lines 3077–3292): Test harness specification — fixture structure, golden output format, `run_fixture()` implementation

**API/Type References**:
- `crates/strato_core::analyze()` (M9): Library entry point called by harness
- `crates/strato_core/tests/integration/harness.rs` (M0): Shared test infrastructure

**Test References**:
- Design Doc Section 21 M11 (lines 3657–3683): Verification commands
- Design Doc Section 21 M12 (lines 3757–3808): Output format tests (`test_json_output_schema`, `test_sarif_output_schema`, `test_text_output_exit_codes`)

**WHY Each Reference Matters**:
- Appendix A has the EXACT Python snippets and expected diagnostic counts — transcribe verbatim
- Appendix B has the `run_fixture()` code and `ExpectedOutput` struct — use as-is
- M12's output format tests are included here because they use the same harness

**Acceptance Criteria**:

```bash
# 1. All fixtures exist
for d in a01_direct_blocking a02_indirect_blocking a03_executor_safe a04_to_thread_safe a05_sync_only_safe a06_blocking_annotation a07_non_blocking_override a08_property_blocking a09_dunder_blocking a10_cross_file a11_deep_transitive a12_multiple_callers a13_mixed_safe_unsafe; do
  test -d "tests/fixtures/$d" && test -f "tests/fixtures/$d/expected.json" || echo "MISSING: $d"
done
# Assert: no MISSING output

# 2. All 13 integration tests pass
cargo test --tests
# Assert: exit code 0, 13+ tests pass

# 3. Output format tests pass
cargo test test_json_output_schema test_sarif_output_schema test_text_output_exit_codes
# Assert: all pass

# 4. Full build still works
cargo build && cargo test
# Assert: exit code 0
```

**Commit**: YES
- Message: `M11: integration tests — all 13 acceptance test fixtures pass`
- Files: All fixture directories + expected.json files, integration test .rs files, modified harness
- Pre-commit: `cargo build && cargo test`

---

### - [ ] 13. M12: Performance Testing + Polish

**What to do**:

1. Create `tests/fixtures/generate_large_project.py`:
   - Deterministic script (seeded RNG) that generates a 500-file Python project
   - Mix of: async functions, sync functions, blocking calls, cross-file imports, executor-wrapped calls
   - Purpose: benchmark target for performance assertions

2. Run the generator to create `tests/fixtures/large_project/`

3. Create `crates/strato_core/tests/integration/test_performance.rs`:
   - `test_fresh_run_500_files`: delete cache, run analysis, assert < 6.5s (release mode CI tolerance)
   - `test_cached_run_500_files`: run twice, assert second run < 650ms (release mode CI tolerance)
   - As specified in Design Doc M12 (lines 3726–3754)

4. Vendore SARIF schema:
   - Download `sarif-schema-2.1.0.json` to `tests/schemas/`
   - `curl -o tests/schemas/sarif-schema-2.1.0.json https://raw.githubusercontent.com/oasis-tcs/sarif-spec/main/sarif-2.1/schema/sarif-schema-2.1.0.json`

5. Create `stubs/examples/redis.pyi` — example stub file

6. Profile with `cargo flamegraph` on `tests/fixtures/large_project/`:
   - Optimization is **done** when: (a) performance targets pass, OR (b) top 3 hotspots documented as inherent to algorithm

7. Update `README.md`:
   - Project description
   - Installation commands (`pip install strato`, `pip install strato-cli`)
   - Basic usage (`strato check src/`)
   - Configuration section (pyproject.toml example)
   - Link to rule docs (STRATO001–004)

8. Set up maturin build for `strato-cli` PyPI package:
   - Verify `maturin build -m crates/strato_cli/Cargo.toml` produces a `.whl` file

9. **Optional SARIF validation** (if `npx` available):
   - `cargo run --release -p strato_cli -- check tests/fixtures/smoke/ --format sarif > /tmp/test.sarif`
   - `npx ajv-cli validate -s tests/schemas/sarif-schema-2.1.0.json -d /tmp/test.sarif`

**Must NOT do**:
- Do not implement watch mode, autofix, or any v2 feature (Guardrail G3)
- Do not run performance tests in debug mode (release only)
- Do not add features to make benchmarks look better — optimize hotspots only

**Recommended Agent Profile**:
- **Category**: `unspecified-high`
  - Reason: Mixed tasks — scripting, profiling, documentation, build system
- **Skills**: `[]`

**Parallelization**:
- **Can Run In Parallel**: NO
- **Parallel Group**: Sequential (final milestone)
- **Blocks**: None (this is the final milestone)
- **Blocked By**: TODO 12 (M11: all tests must pass first)

**References**:

**Pattern References**:
- Design Doc Section 19 (lines 2753–2806): Performance targets and optimization strategies
- Design Doc Section 21 M12 (lines 3686–3821): Complete M12 specification

**API/Type References**:
- `hyperfine`: CLI benchmarking tool
- `cargo flamegraph`: CPU profiling
- `maturin build`: Rust→Python wheel builder

**Test References**:
- Design Doc M12 (lines 3726–3754): Performance test assertions with exact thresholds
- Design Doc M12 (lines 3757–3808): Output format validation tests

**WHY Each Reference Matters**:
- Section 19 defines performance targets — <5s fresh, <500ms cached
- M12 spec has the exact `test_performance.rs` implementation including timing assertions
- Maturin config from Section 17 is needed for the PyPI build step

**Acceptance Criteria**:

```bash
# 1. Performance tests pass (RELEASE MODE ONLY)
cargo test --tests --release test_performance
# Assert: exit code 0

# 2. Large project fixture exists
test -d tests/fixtures/large_project && python3 -c "
import os
count = sum(1 for f in os.listdir('tests/fixtures/large_project') if f.endswith('.py'))
assert count >= 500, f'Only {count} files, need 500'
print(f'{count} Python files generated')
"
# Assert: 500+ files

# 3. README updated
test -f README.md && grep -q 'strato check' README.md && grep -q 'pyproject.toml' README.md && echo "README OK"
# Assert: prints "README OK"

# 4. SARIF schema vendored
test -f tests/schemas/sarif-schema-2.1.0.json && echo "SARIF schema exists"
# Assert: prints message

# 5. All tests pass (full suite)
cargo test && cargo test --tests --release
# Assert: exit code 0

# 6. Maturin build (if maturin installed)
which maturin && maturin build -m crates/strato_cli/Cargo.toml || echo "maturin not installed — skip"
# Assert: .whl file created OR skip message
```

**Commit**: YES
- Message: `M12: performance testing + polish — benchmarks, SARIF schema, README, release build`
- Files: `tests/fixtures/generate_large_project.py`, `tests/fixtures/large_project/`, `tests/integration/test_performance.rs`, `tests/schemas/sarif-schema-2.1.0.json`, `stubs/examples/redis.pyi`, updated `README.md`
- Pre-commit: `cargo build && cargo test`

---

## Commit Strategy

| After TODO | Milestone | Message | Verification |
|------------|-----------|---------|-------------|
| 1 | M0 | `M0: project scaffolding — Rust workspace, Python package, test fixtures` | `cargo build && cargo test` |
| 2 | M1 | `M1: parser abstraction + file discovery — ruff integration, Python file discovery` | `cargo test -p strato_core` |
| 3 | M2 | `M2: module resolver — cross-file import resolution, symbol table` | `cargo test -p strato_core resolver` |
| 4 | M3 | `M3: call graph construction — petgraph-based graph, AST edge extraction, simple type inference` | `cargo test -p strato_core graph` |
| 5 | M4 | `M4: blocking database + annotation detection — 80+ entries, decorator matching` | `cargo test -p strato_core annotator database` |
| 6 | M5 | `M5: blocking propagation — SCC-based single-pass algorithm, O(V+E)` | `cargo test -p strato_core propagator` |
| 7 | M6 | `M6: escape hatch recognition — run_in_executor, asyncio.to_thread` | `cargo test -p strato_core graph_builder::test_executor` |
| 8 | M7 | `M7: property + dunder detection — @property getters, 6 dunder patterns` | `cargo test -p strato_core graph_builder::test_property graph_builder::test_dunder` |
| 9 | M8 | `M8: diagnostic reporting — intervention strategies, STRATO001-004 error codes` | `cargo test -p strato_core reporter` |
| 10 | M9 | `M9: CLI + output formats — strato check with text/JSON/SARIF, full pipeline wired` | `cargo run -p strato_cli -- check tests/fixtures/smoke/ --format json` |
| 11 | M10 | `M10: caching system — incremental analysis with SHA-256 content hashing` | `cargo test -p strato_cache` |
| 12 | M11 | `M11: integration tests — all 13 acceptance test fixtures pass` | `cargo test --tests` |
| 13 | M12 | `M12: performance testing + polish — benchmarks, SARIF schema, README, release build` | `cargo test --tests --release` |

---

## Success Criteria

### Verification Commands

```bash
# Full pipeline test
cargo build && cargo test && cargo test --tests --release
# Expected: all pass

# Smoke test
cargo run -p strato_cli -- check tests/fixtures/smoke/ --format json | python3 -c "
import sys, json
d = json.load(sys.stdin)
assert d['diagnostics'][0]['code'] == 'STRATO001'
print('STRATO001 detected')
"
# Expected: prints "STRATO001 detected"

# Python package
PYTHONPATH=python python3 -c "from strato import blocking, non_blocking; print('annotations OK')"
# Expected: prints "annotations OK"

# Integration tests
cargo test --tests
# Expected: 13+ tests pass

# Performance (release mode)
cargo test --tests --release test_performance
# Expected: fresh < 6.5s, cached < 650ms
```

### Final Checklist

- [ ] All "Must Have" features present (7-phase pipeline, SCC propagation, executor detection, property/dunder, annotations, CLI with 3 formats, caching, 4 error codes)
- [ ] All "Must NOT Have" guardrails respected (no v2 features, no over-engineering, no premature optimization)
- [ ] All 13 acceptance tests pass
- [ ] Performance targets met in release mode
- [ ] Python annotations package importable
- [ ] Deterministic output (run twice → identical)
- [ ] README with installation and usage docs
