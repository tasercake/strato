# Strato v1.1: Full Implementation Plan (M-1 through M12)

## TL;DR

> **Quick Summary**: Implement the complete Strato async blocking call detector from the approved design. 14 milestones (M-1 through M12), strictly sequential, converting the architecture spec into a working Rust CLI tool with Python annotations package. Integrates Astral's `ty` crate for full type inference, replacing the v1.0 hand-rolled `ScopeBindings`. **This plan is fully self-contained — all v1.1 amendment content is embedded directly inline. No cross-referencing of external documents is needed.**
>
> **Deliverables**:
> - Rust workspace with 3 crates (`strato_core`, `strato_cache`, `strato_cli`)
> - Python annotations package (`strato`) with `@blocking`/`@non_blocking`/`@unblocker`
> - CLI binary (`strato check`) with text, JSON, and SARIF output
> - 19 acceptance test fixtures with golden output
> - Generalized executor wrapper registry (config + `@unblocker` decorator)
> - Multi-span diagnostics with related locations
> - Syntax error/warning reporting
> - Performance-validated on 500-file benchmark
>
> **Estimated Effort**: XL (14 milestones, ~70 TODOs)
> **Parallel Execution**: NO — strictly sequential (each milestone depends on previous)
> **Critical Path**: M-1 → M0 → M1 → M2 → M3 → M4 → M5 → M6 → M7 → M8 → M9 → M10 → M11 → M12

---

## Context

### Design Document

**One authoritative specification governs this plan**: `.sisyphus/plans/strato-design.md` (v1.0) — 3,850 lines, 21 sections, 2 appendices. The foundational architecture.

All v1.1 amendments are **embedded directly in the TODOs below**. Wherever v1.0 and v1.1 content conflict, the inline v1.1 content takes precedence. The executor should never need to open any supplementary document — everything needed is in this file.

### Key Decision: ty Integration

The v1.0 design used hand-rolled `ScopeBindings` for type inference (resolving `self`, `cls`, constructors, direct imports). v1.1 replaces this entirely with Astral's `ty` crate (`ty_python_semantic`), providing full type inference including alias tracking, return types, MRO, and attribute resolution.

**Critical technical parameters**:
- **Ruff git rev**: `a89bcfa0aa1f261d21b03d0c00a11e9093990fdd` (Jan 26, 2026) — this rev has BOTH parser crates AND ty crates
- **Salsa rev**: `0946cbd6478cf2bddfc9ac65b3c254c1f1b1bf95` from `salsa-rs/salsa`
- **Required ty crates**: `ty_python_semantic`, `ty_module_resolver`, `ty_site_packages`, `ty_vendored`, `ruff_db`
- **Integration pattern**: "Shallow+" — Strato keeps its own pipeline, queries ty via `trait TypeResolver` as a type oracle
- **Double parse is acceptable**: ruff AST for Strato's walk + ty's internal parse for type inference (<100ms for 500 files)
- **ScopeBindings fallback**: NONE. Full ty commitment per user decision.

### Metis Review — Addressed Gaps

The following issues were identified by Metis and are incorporated into this plan:

| Gap | Resolution | Affected TODO |
|-----|-----------|---------------|
| Caching + ty conflict: ty's cross-file resolution invalidates per-file caching | M10 redesigned: ty results are NOT cached cross-run. Cache parse results + imports only. Salsa's in-run memoization handles performance. | TODO 12 (M10) |
| A3/A6 contradiction: unblocker alias tracking has no ScopeBindings fallback | Decided: if ty can't resolve `safe = sync_to_async(func)`, the unblocker protection is silently lost → false positive on `safe()`. Same "Unknown = skip" principle. Document as known limitation. | TODO 8 (M6) |
| Parser API migration risk at new rev (103 commits gap) | M-1 validates parser API alongside ty. If `parse_module()` signature changed, M-1 documents the adaptation needed. | TODO 1 (M-1) |
| ty initialization cost unknown | M-1 includes rough timing measurement on sample project. If >2s, flag as risk. | TODO 1 (M-1) |
| Two module resolvers (Strato's vs ty's) conflict potential | Strato's resolver feeds source roots into ty's resolver. For call graph edges, ty's resolution is authoritative. For first-party/third-party classification, Strato's resolver is authoritative. | TODO 4 (M2), TODO 5 (M3) |
| `functools.partial` with unblockers unvalidated via ty | Deferred validation to M6. If ty doesn't handle partial, document as known limitation. | TODO 8 (M6) |
| Exit code for warnings-only scenarios | Warnings do NOT affect exit code. Exit 0 if no errors, regardless of warnings. Warnings are informational. | TODO 11 (M9) |
| Missing acceptance criteria for new v1.1 features | 6 new integration test fixtures added to M11 (a14-a19). Total: 19 fixtures. | TODO 13 (M11) |
| ty panic safety | G18 added: catch panics from ty crate calls. Downgrade to NullTypeResolver per-file on panic. | Global guardrail |
| Related locations undefined per error code | Explicit spec for each STRATO code added to M8 TODO. | TODO 10 (M8) |
| Induced edges from unblockers must participate in edge aggregation | Documented in M5/M6: induced edges follow same `all_calls_in_executor` aggregation rule. | TODO 7 (M5), TODO 8 (M6) |
| Star import recursive resolution risk | G16 added: one level only, no transitive star imports. | Global guardrail |

---

## Work Objectives

### Core Objective

Build strato v1.1: a Rust CLI tool that performs transitive call-graph analysis on Python projects to detect blocking calls reachable from async contexts, with full type inference via ty and a generalized executor wrapper system.

### Concrete Deliverables

- `strato` CLI binary (Rust, via `strato_cli` crate)
- `strato` Python package (annotations: `@blocking`, `@non_blocking`, `@unblocker`)
- 80+ built-in blocking function entries with policy-compliant help text
- Text, JSON, and SARIF output formats with multi-span diagnostics
- Generalized executor wrapper registry (built-in + config + `@unblocker` decorator)
- Syntax error and unresolvable import warnings
- Star import resolution and basic namespace package support
- Incremental caching system (parse results cached; ty results per-run only)
- 19 acceptance test fixtures with golden output
- Performance: <5s fresh, <500ms cached on 500 files

### Definition of Done

- [ ] `cargo build` succeeds (all 3 crates compile)
- [ ] `cargo test` passes (all unit + integration tests)
- [ ] `cargo test --tests --release` passes (performance tests within targets)
- [ ] `strato check tests/fixtures/smoke/` produces correct diagnostic output with related locations
- [ ] All 19 acceptance test fixtures pass golden output comparison
- [ ] Python annotations package importable: `from strato import blocking, non_blocking, unblocker`
- [ ] Warnings (parse errors, unresolvable imports) surfaced in all output formats
- [ ] `[tool.strato.executor-wrappers]` config parsed and applied

### Must Have

- Full 7-phase analysis pipeline (Discovery → Parse → Resolve → Build → Annotate → Propagate → Report)
- SCC-based blocking propagation (Tarjan's algorithm)
- **ty-backed type inference** via `trait TypeResolver`
- **Generalized executor wrapper registry** (replaces hardcoded `run_in_executor`/`to_thread`)
- **`@unblocker` decorator** for first-party wrappers
- **Multi-span diagnostics** with `related_locations` per error code
- **Syntax error warnings** via `AnalysisWarning`
- **Star import resolution** via literal `__all__` + public names fallback
- **Basic namespace package support** within configured source roots
- Property and dunder method detection (enhanced by ty)
- Cross-file analysis
- `@blocking` / `@non_blocking` / `@unblocker` annotation support
- Configurable intervention strategy (`first-party-deepest`, `async-boundary`)
- 4 error codes: STRATO001–STRATO004
- `[tool.strato.executor-wrappers]` config section

### Must NOT Have (Guardrails)

- **G1: Unknown stays Unknown** — MUST NOT reclassify `Unknown` `BlockingStatus` nodes to `NotBlocking` or any other state after propagation. Unknown is a permanent terminal state. (Design doc lines 460–462)
- **G2: ty Graceful Degradation** — ty provides type inference, but Unknown is STILL the default. If ty returns `None`/`Unknown`/`TodoType` for any expression, skip silently. MUST NOT panic, MUST NOT fall back to heuristics, MUST NOT emit a diagnostic about the failure. The `trait TypeResolver` must always have `Option<>` return types. *(Replaces v1.0 G2 "no full type inference")*
- **G3: No v2 features** — MUST NOT implement any feature from Section 20 (trio/anyio native support, framework plugins, autofix, watch mode, LSP). **Exception**: Basic namespace package support within configured source roots is v1.1 scope. Cross-root namespace merging remains v2.
- **G4: Each milestone must compile** — MUST run `cargo build` and `cargo test` at end of each milestone with zero failures
- **G5: Deterministic output** — MUST use `BTreeMap` or explicit sorting for all collections that affect diagnostic output order. `HashMap` is acceptable for internal lookups only.
- **G6: Exact error code semantics** — STRATO001 (chain_len=1 + async caller), STRATO002 (chain_len>1), STRATO003 (PropertyAccess edge), STRATO004 (ImplicitDunder edge). No merging or reinterpreting.
- **G7: No premature optimization** — MUST NOT profile or benchmark before M12. Correctness first.
- **G8: Only design-specified tests + v1.1 additions** — Write ONLY the tests listed in each milestone plus the 6 new v1.1 test fixtures. No extra edge case tests.
- **G9: Only Section 9 DB entries** — MUST NOT add blocking database entries beyond what's in the design doc's Section 9 tables.
- **G10: Only `strato check`** — No other CLI commands. No `strato init`, `strato fix`, etc.
- **G11: Serde derives progressive** — Add `#[derive(Serialize, Deserialize)]` to types as they're created (M0 onwards), so M10 caching doesn't require mass refactoring.
- **G12: No over-documentation** — Minimal `///` doc comments. Code is self-documenting per design doc. No README changes until M12.
- **G13: ty Query Safety** — MUST NOT call `TypeResolver::resolve_type()` inside tight loops without result caching. ty Salsa queries may trigger expensive recomputation. All ty queries in hot paths (call edge visitor, dunder resolution) must be batched or memoized per-function.
- **G14: ty Upgrade Protocol** — MUST NOT upgrade the ty/ruff pinned rev without a dedicated compatibility spike (equivalent to M-1). Any rev change may break the Salsa DB setup, parser API, or type query API.
- **G15: Unblocker Simplicity** — MUST NOT add support for unblocker decorator parameters beyond `callable_param`. No `thread_sensitive`, no `executor_class`, no return type mapping. The unblocker system recognizes ONE thing: which argument is the callable being offloaded.
- **G16: Star Import Depth Limit** — MUST NOT resolve star imports recursively. `from a import *` where `a` itself has `from b import *` — the transitive star import from `b` is NOT resolved through `a`. One level only.
- **G17: Warning System Scope** — MUST NOT add `AnalysisWarning` variants beyond `ParseError` and `UnresolvableImport` in v1.1. No `TypeResolutionFailure`, no `CyclicImportDetected`, no `DeprecatedPattern`. **Exception**: If ty integration requires a `TyInitializationFailure` warning, that is the ONLY additional variant permitted.
- **G18: ty Panic Isolation** — MUST catch panics from ty crate calls using `std::panic::catch_unwind()`. ty is pre-1.0 and may panic on unexpected input. A ty panic MUST NOT crash strato — downgrade to `NullTypeResolver` for the affected file and emit `AnalysisWarning::TyPanic { file, message }` (this is the G17 exception).

### ty Feature Budget

> Explicitly scopes which ty capabilities are used in v1.1. This prevents scope creep from ty's rich type system.

| ty Capability | v1.1 Status | Usage |
|---|---|---|
| `self`/`cls` type resolution | ✅ USE | Same as v1.0 but via ty instead of ScopeBindings |
| Constructor return type (`MyClass()` → `MyClass`) | ✅ USE | Same as v1.0 but via ty |
| Import resolution / qualified name lookup | ✅ USE | Same as v1.0 but via ty |
| **Alias tracking** (`x = requests.get; x()`) | ✅ USE | **NEW** — core value of ty integration |
| **Return type inference** (`get_loader()` → `Loader`) | ✅ USE | **NEW** — resolves indirect calls through return types |
| **Attribute type resolution** (`obj.method` → resolved method) | ✅ USE | **NEW** — more accurate method/property resolution |
| **MRO / inherited methods** | ⚠️ LIMITED | ONLY for property getter and dunder method lookup. MUST NOT implement full MRO-aware blocking propagation. |
| Generic instantiation (`List[int]`, `Dict[str, Any]`) | ❌ SKIP | v2. Too complex for v1.1. |
| Type narrowing (`isinstance` guards) | ❌ SKIP | v2. Requires control flow analysis integration. |
| Protocol / structural typing | ❌ SKIP | v2. |
| Overloaded function resolution | ❌ SKIP | v2. |
| Dataclass / attrs field inference | ❌ SKIP | Not relevant to blocking analysis. |

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
| M-1 | `cargo build -p strato_core && cargo test -p strato_core spike` | ty + ruff deps compile, type queries work |
| M0 | `cargo build && PYTHONPATH=python python3 -c "from strato import blocking, non_blocking, unblocker; print('OK')"` | Workspace compiles, Python pkg works |
| M1 | `cargo test -p strato_core parser discovery` | Parser + discovery + warnings unit tests pass |
| M2 | `cargo test -p strato_core resolver` | Module resolver + star imports + namespace tests pass |
| M3 | `cargo test -p strato_core graph` | Call graph construction + TypeResolver tests pass |
| M4 | `cargo test -p strato_core annotator && cargo test -p strato_core database` | Blocking DB + annotations + @unblocker pass |
| M5 | `cargo test -p strato_core propagator` | SCC propagation tests pass |
| M6 | `cargo test -p strato_core graph_builder::test_wrapper` | Wrapper registry + induced edge tests pass |
| M7 | `cargo test -p strato_core graph_builder::test_property && cargo test -p strato_core graph_builder::test_dunder` | ty-enhanced property + dunder tests pass |
| M8 | `cargo test -p strato_core reporter` | Diagnostic reporter + related locations tests pass |
| M9 | `cargo build -p strato_cli && cargo run -p strato_cli -- check --help` | CLI binary works, all formatters + warnings |
| M10 | `cargo test -p strato_cache` | Caching tests pass |
| M11 | `cargo test --tests` | All 19 integration tests pass |
| M12 | `cargo test --tests --release` | Performance + schema tests pass |

---

## Execution Strategy

### Sequential Execution (No Parallelization)

All 14 milestones are strictly sequential. Each builds on the previous. The dependency chain is inherent to the architecture — each phase of the analysis pipeline requires the previous phase to be implemented and compiling.

```
M-1: ty Integration Spike
 └─> M0: Project Scaffolding (+ ty deps + @unblocker)
      └─> M1: Parser + Discovery (+ AnalysisWarning)
           └─> M2: Module Resolver (+ star imports + namespace pkgs)
                └─> M3: Call Graph Construction (+ trait TypeResolver via ty)
                     └─> M4: Blocking Database + Annotations (+ @unblocker + help text policy)
                          └─> M5: Blocking Propagation (SCC — unchanged algorithm)
                               └─> M6: Wrapper Registry (generalized escape hatches)
                                    └─> M7: Properties + Dunders (ty-enhanced)
                                         └─> M8: Diagnostic Reporting (+ related_locations)
                                              └─> M9: CLI + Output Formats (+ warnings + wrapper config)
                                                   └─> M10: Caching System (ty-aware: parse-only caching)
                                                        └─> M11: Integration Tests (19 fixtures)
                                                             └─> M12: Performance + Polish + Docs
```

### Agent Dispatch Summary

| TODO | Milestone | Category | Skills | Background |
|------|-----------|----------|--------|------------|
| 1 | M-1 | `deep` | `[]` | NO (foundational research — must succeed first) |
| 2 | M0 | `unspecified-high` | `[]` | NO (foundational — must compile) |
| 3 | M1 | `unspecified-high` | `[]` | NO |
| 4 | M2 | `unspecified-high` | `[]` | NO |
| 5 | M3 | `unspecified-high` | `[]` | NO |
| 6 | M4 | `unspecified-high` | `[]` | NO |
| 7 | M5 | `ultrabrain` | `[]` | NO |
| 8 | M6 | `unspecified-high` | `[]` | NO |
| 9 | M7 | `unspecified-high` | `[]` | NO |
| 10 | M8 | `unspecified-high` | `[]` | NO |
| 11 | M9 | `unspecified-high` | `[]` | NO |
| 12 | M10 | `unspecified-high` | `[]` | NO |
| 13 | M11 | `unspecified-high` | `[]` | NO |
| 14 | M12 | `unspecified-high` | `[]` | NO |

---

## TODOs

---

### - [ ] 1. M-1: ty Integration Spike

**What to do**:

> **Purpose**: Validate that ty crate integration is feasible at the chosen ruff rev. This is a research spike — the code produced here is **exploratory** and will be refined in M0/M3. The spike SUCCEEDS when all acceptance criteria pass. The spike FAILS when any criterion cannot be made to work after reasonable effort.
>
> **Time box**: If this spike requires more than 2 serious attempts (full build cycles), escalate — the plan may need a fallback to ScopeBindings.

1. Create a minimal `Cargo.toml` in the workspace root with:
   - Ruff crates pinned to rev `a89bcfa0aa1f261d21b03d0c00a11e9093990fdd`:
     - `ruff_python_parser`, `ruff_python_ast`, `ruff_text_size` (verify crate name at this rev)
   - ty crates pinned to the SAME rev:
     - `ty_python_semantic`, `ty_module_resolver`, `ty_site_packages`, `ty_vendored`, `ruff_db`
   - Salsa from `salsa-rs/salsa` at rev `0946cbd6478cf2bddfc9ac65b3c254c1f1b1bf95`
   - Verify all crates compile together: `cargo build`

2. Create `crates/strato_core/src/spike.rs` (temporary module — will be replaced in M3):
   - **Test 1: Parse at new rev** — Call `ruff_python_parser::parse_module()` on a simple Python snippet. Verify the function signature matches expectations. Document any API changes from the old rev.
   - **Test 2: Salsa DB initialization** — Create a ty Salsa database. Implement the required traits (`ruff_db::Db`, `ty_module_resolver::Db`, `ty_python_semantic::Db`). Verify `Db::default()` or equivalent works.
   - **Test 3: Source file ingestion** — Feed a Python source file into the ty database. Verify the file is registered and parseable by ty.
   - **Test 4: Type query** — Given `import time; time.sleep(1)`, query ty for the type of `time.sleep` expression. Verify it returns something that can be converted to a qualified name (`"time.sleep"`).
   - **Test 5: Attribute resolution** — Given `import requests; requests.get("url")`, query ty for the resolved callee of the `get()` call. Verify it resolves to `"requests.get"` or equivalent.
   - **Test 6: Alias tracking** — Given `from requests import get as fetch; fetch("url")`, query ty for the resolved callee. Verify it resolves to `"requests.get"`.
   - **Test 7: Module resolver integration** — Configure ty's module resolver with a source root path. Verify it can resolve imports from that root. Document whether Strato needs to feed its source roots into ty.
   - **Test 8: Performance sniff test** — Parse 50 simple Python files through both ruff parser and ty. Measure wall time. If >5s, flag as risk for the 500-file performance target.

3. Document findings in a `SPIKE_RESULTS.md` (temporary file, deleted after M3):
   - Exact Salsa DB boilerplate code needed
   - Any parser API changes observed
   - ty type query API surface: what methods, what return types
   - How to extract `QualifiedName` from ty's `Type` enum
   - Whether ty requires files on disk or accepts in-memory sources
   - Performance characteristics observed
   - Any surprises or risks

**Context — what the spike is validating**: The spike validates that the following `trait TypeResolver` architecture can be implemented via ty. This trait will be formally implemented in M3, but the spike must demonstrate the underlying ty API supports it:

```rust
// The abstraction Strato will use (implemented in M3)
trait TypeResolver {
    /// Given an expression, return the qualified name of its type (if known)
    fn resolve_type(&self, expr: &Expr, file: &Path) -> Option<QualifiedName>;

    /// Given a call expression, return the qualified name of the callee (if known)
    fn resolve_callee(&self, call: &ExprCall, file: &Path) -> Option<QualifiedName>;

    /// Given an attribute access, return the resolved method/property (if known)
    fn resolve_attribute(&self, obj_type: &QualifiedName, attr: &str) -> Option<QualifiedName>;

    /// Compute MRO for a class
    fn mro(&self, class: &QualifiedName) -> Vec<QualifiedName>;
}
```

The spike must also validate that ty can support executor wrapper alias tracking — the pattern `safe = sync_to_async(func); await safe()` where ty needs to resolve `safe` back to a callable. This is critical because the executor wrapper config schema depends on ty's ability to resolve the callable argument:

```toml
# Config that depends on ty resolving the callable at the configured param position
[tool.strato.executor-wrappers]
"asgiref.sync.sync_to_async" = { callable_param = 0 }
"anyio.to_thread.run_sync" = { callable_param = 0 }
```

**Must NOT do**:
- Do not implement the full `TypeResolver` trait (that's M3)
- Do not implement the call graph builder (that's M3)
- Do not create the final workspace structure (that's M0)
- Do not write production-quality code — this is exploration

**Recommended Agent Profile**:
- **Category**: `deep`
  - Reason: Research spike requiring deep investigation of unfamiliar crate APIs. Must understand Salsa's database model, ty's type system, and ruff's parser changes. Goal-oriented autonomous problem-solving is essential — the agent must probe, fail, adapt, and document.
- **Skills**: `[]`
- **Skills Evaluated but Omitted**:
  - `playwright`: No browser work
  - `git-master`: No git operations beyond basic

**Parallelization**:
- **Can Run In Parallel**: NO
- **Parallel Group**: Sequential — first milestone, gating all subsequent work
- **Blocks**: ALL subsequent TODOs (2–14)
- **Blocked By**: None (start here)

**References**:

**Pattern References**:
- Design Doc Section 6 (lines ~607–701): The ScopeBindings subsection that ty REPLACES — understanding what ScopeBindings did informs what ty must do
- Design Doc Section 11 (lines ~1700–1870): Escape hatch patterns — the spike validates ty can track aliases for wrapped callables

**API/Type References**:
- `ruff_python_parser::parse_module()`: Parser entry point — verify signature at new rev
- `ty_python_semantic::SemanticModel`: Main ty query interface (expected, verify)
- `ruff_db::Db`: Database trait that Strato's Salsa DB must implement
- `salsa::Database`: Salsa 2022 database derive macro

**External References**:
- Ruff repo at rev `a89bcfa`: `https://github.com/astral-sh/ruff/tree/a89bcfa0aa1f261d21b03d0c00a11e9093990fdd`
- Salsa repo at rev `0946cbd`: `https://github.com/salsa-rs/salsa/tree/0946cbd6478cf2bddfc9ac65b3c254c1f1b1bf95`
- ty crate source: `https://github.com/astral-sh/ruff/tree/a89bcfa/crates/ty_python_semantic`

**WHY Each Reference Matters**:
- Section 6 ScopeBindings shows what capabilities must be replicated by ty — the spike validates this
- The ruff repo at the specific rev is where the agent must look for actual API signatures
- Salsa's database model is the core setup challenge — the agent needs the actual trait requirements

**Acceptance Criteria**:

```bash
# 1. Ruff crates at new rev compile
cargo build -p strato_core
# Assert: exit code 0

# 2. ty crates compile alongside ruff crates
cargo build -p strato_core  # with ty_python_semantic in deps
# Assert: exit code 0

# 3. Parser at new rev works
cargo test -p strato_core spike::test_parse_at_new_rev
# Assert: parse_module() succeeds on simple Python snippet

# 4. Salsa DB initializes
cargo test -p strato_core spike::test_salsa_db_init
# Assert: Database construction succeeds

# 5. Type query returns a result
cargo test -p strato_core spike::test_type_query_basic
# Assert: Given known-type expression, ty returns non-None type

# 6. Qualified name extraction works
cargo test -p strato_core spike::test_qualified_name_from_ty
# Assert: Given `import time; time.sleep()`, resolve_callee returns something containing "time.sleep"

# 7. Alias tracking works
cargo test -p strato_core spike::test_alias_tracking
# Assert: `from x import y as z; z()` resolves to `x.y`

# 8. Performance is within budget
cargo test -p strato_core spike::test_performance_sniff -- --ignored
# Assert: 50-file parse + type query completes in <2s (not the final benchmark, just a sanity check)

# 9. Full workspace compiles
cargo build
# Assert: exit code 0
```

**Commit**: YES
- Message: `M-1: ty integration spike — validate Salsa DB setup, type queries, parser compatibility`
- Files: `Cargo.toml` (workspace deps), `crates/strato_core/Cargo.toml`, `crates/strato_core/src/spike.rs`, `SPIKE_RESULTS.md`
- Pre-commit: `cargo build && cargo test -p strato_core spike`

---

### - [ ] 2. M0: Project Scaffolding

**What to do**:

1. **Finalize workspace `Cargo.toml`** based on M-1 findings:
   - Workspace members: `crates/strato_core`, `crates/strato_cache`, `crates/strato_cli`
   - `resolver = "2"`
   - All `[workspace.dependencies]` as specified in Design Doc Section 18 lines 2709–2749, **UPDATED** with:
     - Ruff crates pinned to rev `a89bcfa0aa1f261d21b03d0c00a11e9093990fdd`
     - ty crates: `ty_python_semantic`, `ty_module_resolver`, `ty_site_packages`, `ty_vendored`, `ruff_db`
     - Salsa from `salsa-rs/salsa` at rev `0946cbd6478cf2bddfc9ac65b3c254c1f1b1bf95`
   - Apply any dependency adjustments discovered in M-1 (e.g., crate names that differ at this rev)

2. Create `crates/strato_core/Cargo.toml`:
   - Package name `strato_core`, version `0.1.0`, edition `2021`
   - Dependencies: `serde`, `thiserror`, ruff crates, ty crates, `salsa`, `petgraph`
   - **Add `#[derive(Serialize, Deserialize)]` support from the start** (Guardrail G11)

3. Create `crates/strato_core/src/lib.rs`:
   - Empty `pub mod` declarations for future modules: `types`
   - Re-export key types from `types`
   - **Remove `spike` module** (was temporary for M-1)

4. Create `crates/strato_core/src/types.rs`:
   - Define shared types: `QualifiedName`, `Location`, `ModulePath`
   - Include `#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]` on all types
   - Define `RelatedLocation` — a secondary location with a descriptive label for multi-span diagnostics:

     ```rust
     /// A secondary location with a descriptive label.
     struct RelatedLocation {
         location: Location,
         /// Human-readable label explaining this location's role.
         /// Examples: "blocking property accessed here", "blocking call executes here"
         label: String,
     }
     ```

     This enables multi-span diagnostics where the primary location follows the intervention strategy, and related locations provide supplementary context (e.g., for STRATO003, the property access site vs the blocking call inside the getter).

   - Define `AnalysisWarning` — non-fatal conditions tracked during analysis:

     ```rust
     enum AnalysisWarning {
         /// A file could not be parsed due to syntax errors.
         ParseError {
             file: String,
             error: String,
             line: Option<usize>,
         },
         /// An import could not be resolved (informational).
         UnresolvableImport {
             file: String,
             line: usize,
             import_path: String,
         },
         // Future: other non-fatal conditions
     }
     ```

     Warnings are surfaced in all output formats but do NOT affect exit codes. See the output behavior per format:

     | Format | Behavior |
     |--------|----------|
     | **Text** | Warnings printed after diagnostics, dimmed, prefixed with `warning:` |
     | **JSON** | Included in `"warnings"` array alongside `"diagnostics"` |
     | **SARIF** | Included as results with `"level": "note"` |
     | **`--stats`** | Shows count of files skipped due to parse errors |

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

10. Create `python/strato/__init__.py`:
    - Import and re-export: `from strato._annotations import blocking, non_blocking, unblocker`
    - `__all__ = ["blocking", "non_blocking", "unblocker"]`

11. Create `python/strato/_annotations.py` — the complete annotations module with `@blocking`, `@non_blocking`, AND `@unblocker`:

    ```python
    """Strato annotations for marking function blocking behavior."""

    import functools
    from typing import TypeVar, Callable, Any, overload

    F = TypeVar("F", bound=Callable[..., Any])


    def blocking(func: F) -> F:
        """Mark a function as blocking the event loop.

        Use this to annotate functions that Strato's built-in database
        doesn't cover but that you know are blocking.
        """
        @functools.wraps(func)
        def wrapper(*args: Any, **kwargs: Any) -> Any:
            return func(*args, **kwargs)
        wrapper.__strato_blocking__ = True  # type: ignore[attr-defined]
        return wrapper  # type: ignore[return-value]


    def non_blocking(func: F) -> F:
        """Mark a function as NOT blocking, overriding Strato's analysis.

        Use this when Strato incorrectly flags a function as blocking
        (e.g., CPU-bound work that completes quickly).
        """
        @functools.wraps(func)
        def wrapper(*args: Any, **kwargs: Any) -> Any:
            return func(*args, **kwargs)
        wrapper.__strato_non_blocking__ = True  # type: ignore[attr-defined]
        return wrapper  # type: ignore[return-value]


    @overload
    def unblocker(func: F) -> F: ...
    @overload
    def unblocker(*, callable_param: int | str = 0) -> Callable[[F], F]: ...

    def unblocker(func: F | None = None, *, callable_param: int | str = 0) -> F | Callable[[F], F]:
        """Mark a function as an executor wrapper that offloads blocking work.

        Use this to annotate wrapper functions that execute their callable
        argument in a thread pool or other non-blocking context.

        Args:
            callable_param: Which parameter receives the callable to offload.
                Can be a positional index (int) or parameter name (str).
                Default: 0 (first positional argument).

        Example:
            @unblocker
            def my_thread_wrapper(func):
                return asyncio.to_thread(func)

            @unblocker(callable_param="target")
            def custom_offload(*, target, timeout=30):
                return background.submit(target, timeout=timeout)
        """
        def decorator(f: F) -> F:
            @functools.wraps(f)
            def wrapper(*args: Any, **kwargs: Any) -> Any:
                return f(*args, **kwargs)
            wrapper.__strato_unblocker__ = True  # type: ignore[attr-defined]
            wrapper.__strato_callable_param__ = callable_param  # type: ignore[attr-defined]
            return wrapper  # type: ignore[return-value]

        if func is not None:
            return decorator(func)
        return decorator  # type: ignore[return-value]
    ```

    Key implementation details for `@unblocker`:
    - Supports `@unblocker` (bare — default: `callable_param=0`) and `@unblocker(callable_param=...)` (parameterized)
    - Sets `__strato_unblocker__ = True` and `__strato_callable_param__` on the wrapped function
    - `callable_param` can be an `int` (positional index, 0-based) or `str` (keyword argument name)
    - Decorator detection in `annotator.rs` matches by decorator name, not import resolution

12. Create `python/strato/py.typed`:
    - Empty file (PEP 561 marker)

13. Create `pyproject.toml` (workspace root — Python annotations package):
    - Package name `strato`, version `0.1.0`
    - Build system: `setuptools` or `hatchling`
    - Packages: `python/strato`

14. Create test infrastructure for integration tests:
    - Create directory `crates/strato_core/tests/integration/`
    - Create `crates/strato_core/tests/integration/main.rs` with `mod harness;` (gated)
    - Create `crates/strato_core/tests/integration/harness.rs` from Design Doc Appendix B (lines 3163–3245)
    - Harness `run_fixture()` function body gated behind `#[cfg(feature = "full-pipeline")]` until M9

15. Create smoke test fixture:
    - `tests/fixtures/smoke/test_smoke.py` and `tests/fixtures/smoke/expected.json` as in v1.0 plan

16. **Delete M-1 spike artifacts**: Remove `SPIKE_RESULTS.md` and `crates/strato_core/src/spike.rs` (findings are incorporated into M0 setup)

17. **CRITICAL VALIDATION**: Run `cargo build` to verify everything compiles cleanly at the new ruff rev.

**Must NOT do**:
- Do not implement any analysis logic — M0 is skeleton only
- Do not add unit tests beyond compilation checks
- Do not write `///` doc comments beyond module-level descriptions
- Do not add any blocking database entries
- Do not implement the `analyze()` function
- Do not implement the Salsa DB setup (that's wired in M3 through the TypeResolver)

**Recommended Agent Profile**:
- **Category**: `unspecified-high`
  - Reason: Project scaffolding with many files across Rust + Python + TOML configs, incorporating M-1 findings
- **Skills**: `[]`

**Parallelization**:
- **Can Run In Parallel**: NO
- **Parallel Group**: Sequential
- **Blocks**: ALL subsequent TODOs (3–14)
- **Blocked By**: TODO 1 (M-1: spike must succeed first)

**References**:

**Pattern References**:
- Design Doc Section 18 (lines 2600–2750): Complete repository structure with all file paths
- Design Doc Section 18 (lines 2709–2749): Exact `Cargo.toml` workspace dependencies (adapt for new rev)
- `SPIKE_RESULTS.md` (from M-1): Exact dependency versions and crate names validated

**API/Type References**:
- Design Doc Section 12 (lines ~1880–1940): Python `@blocking`/`@non_blocking` decorator implementation
- Design Doc Section 17 (lines ~2450–2600): Distribution and packaging (maturin config)

**Test References**:
- Design Doc Appendix B (lines 3077–3292): Test harness specification
- Design Doc Section 21 M0 (lines 3320–3350): Exact M0 files and verification

**WHY Each Reference Matters**:
- Section 18 contains the directory tree to create; now enhanced with ty crate deps from M-1
- Section 12 has the `@blocking`/`@non_blocking` baseline — `@unblocker` is specified inline above
- Appendix B has the harness code to copy

**Acceptance Criteria**:

```bash
# 1. Rust workspace compiles (including ty + ruff deps)
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

# 3. Python package importable (with @unblocker)
PYTHONPATH=python python3 -c "from strato import blocking, non_blocking, unblocker; print('OK')"
# Assert: prints "OK"

# 4. @unblocker works correctly
PYTHONPATH=python python3 -c "
from strato import unblocker
@unblocker
def wrap(f): return f
assert hasattr(wrap, '__strato_unblocker__')
assert wrap.__strato_callable_param__ == 0

@unblocker(callable_param='func')
def wrap2(func=None): return func
assert wrap2.__strato_callable_param__ == 'func'
print('unblocker OK')
"

# 5. CLI stub runs
cargo run -p strato_cli -- --version
# Assert: prints version string

# 6. Spike artifacts cleaned up
test ! -f SPIKE_RESULTS.md && test ! -f crates/strato_core/src/spike.rs && echo "Spike cleaned"
# Assert: prints "Spike cleaned"

# 7. No test failures
cargo test
# Assert: exit code 0
```

**Commit**: YES
- Message: `M0: project scaffolding — Rust workspace with ty deps, Python package with @unblocker, test fixtures`
- Files: All created files
- Pre-commit: `cargo build && cargo test`

---

### - [ ] 3. M1: Parser Abstraction + File Discovery + Warning Collection

**What to do**:

1. Create `crates/strato_core/src/discovery.rs`:
   - File discovery: walk directory tree, find `.py` files, respect exclude patterns
   - Source root detection: parse `pyproject.toml` for `[tool.setuptools.packages.find]` or detect `src/` layout
   - Auto-detect first-party packages from project layout
   - As specified in Design Doc Section 4 Phase 1 and Section 5

2. Create `crates/strato_core/src/parser.rs`:
   - Define `trait PythonParser` (abstraction over ruff):
     ```rust
     pub trait PythonParser {
         fn parse(&self, source: &str) -> ParseResult;
     }

     pub struct ParseResult {
         pub module: Option<ParsedModule>,  // None if parse failed entirely
         pub warnings: Vec<AnalysisWarning>,  // Parse errors collected here
     }
     ```
   - Implement `RuffParser` using `ruff_python_parser::parse_module()` **at the new rev**
   - Collect parse errors as `AnalysisWarning::ParseError` instead of silently dropping them. When a file fails to parse, create a warning:
     ```
     AnalysisWarning::ParseError {
         file: file_path.to_string(),
         error: parse_error.to_string(),
         line: Some(error_line),
     }
     ```
   - Extract `FileSymbols`: function defs (name, is_async, decorators, location), class defs, import statements
   - Handle parse errors gracefully: partial parse results preserved where possible, error becomes `AnalysisWarning`
   - Adapt to any parser API changes discovered in M-1 (e.g., changed function signatures, renamed types)

   Example of how warnings appear in text output:
   ```
   error[STRATO002]: Indirect blocking call reachable from async context
     --> src/services/email.py:23:5
      ...

   warning: failed to parse src/legacy/broken.py: unexpected token at line 42
   warning: failed to parse src/generated/proto.py: invalid syntax at line 1

   Found 1 error, 2 warnings in 1.8s (analyzed 245 of 247 files)
   ```

3. Update `crates/strato_core/src/lib.rs`:
   - Add `pub mod discovery; pub mod parser;`

4. Update `crates/strato_core/Cargo.toml`:
   - Add dependencies: `ruff_python_parser`, `ruff_python_ast`, `rayon`, `toml`, `globset` (all from workspace)

5. Write unit tests (Design Doc Section 21 M1 + new warning tests):
   - `parser::test_parse_simple_function` — Parse a function def, verify AST
   - `parser::test_parse_async_function` — Parse async def, verify `is_async` flag
   - `parser::test_parse_error_produces_warning` — Invalid syntax produces `AnalysisWarning::ParseError`, doesn't panic
   - `parser::test_parse_error_includes_location` — Warning includes file path and line number
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
  - Reason: Core Rust development with ruff crate integration at new rev — requires adapting to potentially changed parser APIs
- **Skills**: `[]`

**Parallelization**:
- **Can Run In Parallel**: NO
- **Parallel Group**: Sequential
- **Blocks**: TODO 4 (M2: resolver needs parser + discovery)
- **Blocked By**: TODO 2 (M0: workspace must exist)

**References**:

**Pattern References**:
- Design Doc Section 4 (lines ~310–500): Analysis pipeline Phase 1 (Discovery) and Phase 2 (Parse)
- `SPIKE_RESULTS.md` findings (from M-1): Any parser API changes documented

**API/Type References**:
- `ruff_python_parser::parse_module()`: Parser entry point (verify signature from M-1)
- `ruff_python_ast::Stmt`, `StmtFunctionDef`, `StmtClassDef`, etc.: AST node types
- `crates/strato_core/src/types.rs` (M0): `AnalysisWarning::ParseError` variant

**Test References**:
- Design Doc Section 21 M1 (lines 3371–3378): Original test names

**WHY Each Reference Matters**:
- Section 4 defines pipeline phases — parser.rs implements Phase 2
- M-1 findings tell you exactly what parser API looks like at the new rev
- The AnalysisWarning type defined in M0 types.rs is what parse errors produce

**Acceptance Criteria**:

```bash
# 1. New modules compile
cargo build -p strato_core
# Assert: exit code 0

# 2. All 7 unit tests pass
cargo test -p strato_core parser discovery
# Assert: exit code 0, 7 tests pass

# 3. Parse error warning test specifically
cargo test -p strato_core parser::test_parse_error_produces_warning
# Assert: pass — returns AnalysisWarning, not panic

# 4. Full build still works
cargo build && cargo test
# Assert: exit code 0
```

**Commit**: YES
- Message: `M1: parser abstraction + file discovery — ruff integration at new rev, parse error warnings`
- Files: `crates/strato_core/src/discovery.rs`, `crates/strato_core/src/parser.rs`, modified `lib.rs`, `Cargo.toml`
- Pre-commit: `cargo build && cargo test`

---

### - [ ] 4. M2: Module Resolver + Star Imports + Namespace Packages

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
   - As specified in Design Doc Section 5

   - **Star import resolution** via `resolve_star_import()`. Algorithm:

     ```
     FUNCTION resolve_star_import(module_path: &str, symbol_table: &SymbolTable) -> Vec<String>:

       target_module = resolve_module(module_path)
       IF target_module is None:
         RETURN []  // Unresolvable module → treat as empty

       source = read_source(target_module.file_path)

       // Strategy 1: Look for literal __all__
       all_value = extract_literal_all(source)
       IF all_value is Some(names):
         RETURN names  // ["foo", "bar", "baz"]

       // Strategy 2: No __all__ → collect all public top-level definitions
       RETURN target_module.top_level_names()
              .filter(|name| !name.starts_with("_"))
              .collect()
     ```

     **`extract_literal_all()`**: Parses the target module's AST looking for:
     - `__all__ = ["name1", "name2", ...]` — literal list of strings
     - `__all__ = ("name1", "name2", ...)` — literal tuple of strings
     - `__all__ = ["a", "b"] + ["c", "d"]` — simple concatenation of literal lists
     If `__all__` is assigned from a non-literal expression (function call, variable, etc.), return `None` and fall through to Strategy 2.

     **Strategy 2 (no `__all__`)**: Collect all names defined at the module's top level that don't start with `_`. This includes:
     - Function definitions (`def foo():`)
     - Class definitions (`class Foo:`)
     - Variable assignments (`BAR = ...`)
     - Imported names (`from x import y` makes `y` available)

     **Rationale**: Star imports are pragmatically solvable for the vast majority of real-world cases. Packages that re-export via `__init__.py` (e.g., `from .models import *`) are extremely common, and skipping them causes false negatives in cross-module analysis.

     **G16**: One level only — no recursive star import resolution. `from a import *` where `a` does `from b import *` does NOT pull in `b`'s names.

   - **Basic namespace package support** within configured source roots (PEP 420):

     ```
     OLD: directory without __init__.py → not a package → resolution fails
     NEW: directory without __init__.py → namespace package portion → continue resolution into subdirectories
     ```

     **Constraints**:
     - Only within project source roots (no `sys.path` discovery)
     - A regular package (with `__init__.py`) always takes precedence over a namespace portion at the same path
     - No cross-root namespace merging in v1 (would require searching multiple roots and combining)

     **Rationale**: This is a pragmatic limitation that causes surprising "cannot resolve import" errors in monorepos and projects that follow newer Python packaging practices. Basic support within configured roots is low-effort and eliminates the most common failure mode.

   - Source root ordering: try roots in order, first match wins
   - Unresolvable imports return `None` (not errors)

2. Create test fixtures:
   - `tests/fixtures/resolver_basic/` — Simple project with absolute imports
   - `tests/fixtures/resolver_relative/` — Relative imports across packages
   - `tests/fixtures/resolver_init_package/` — `__init__.py` package imports
   - `tests/fixtures/resolver_star_import/` — Module with `__all__` and star imports
   - `tests/fixtures/resolver_namespace/` — Directory without `__init__.py`

3. Update `crates/strato_core/src/lib.rs`:
   - Add `pub mod resolver;`

4. Write unit tests (Design Doc Section 21 M2 + new v1.1 tests):
   - `resolver::test_absolute_import`
   - `resolver::test_from_import`
   - `resolver::test_relative_import`
   - `resolver::test_relative_parent_import`
   - `resolver::test_init_package`
   - `resolver::test_unresolvable_returns_none`
   - `resolver::test_source_root_ordering`
   - `resolver::test_pyi_stub_resolution`
   - `resolver::test_star_import_with_all` — `__all__ = ["foo", "bar"]` resolves correctly
   - `resolver::test_star_import_without_all` — No `__all__`, collects public names
   - `resolver::test_star_import_dynamic_all_fallback` — `__all__ = get_names()` falls through to public names
   - `resolver::test_namespace_package_basic` — Directory without `__init__.py` resolves as namespace
   - `resolver::test_namespace_regular_wins` — Regular package with `__init__.py` takes precedence over namespace

**Must NOT do**:
- Do not resolve star imports recursively (G16) — `from a import *` where `a` does `from b import *` does NOT pull in `b`'s names
- Do not handle cross-root namespace merging (v2)
- Do not follow circular imports — each module resolved independently
- Do not implement conditional imports (try/except) beyond first branch
- Do not build call graph edges (that's M3)

**Recommended Agent Profile**:
- **Category**: `unspecified-high`
  - Reason: Complex module resolution logic with multiple new features (star imports, namespace packages)
- **Skills**: `[]`

**Parallelization**:
- **Can Run In Parallel**: NO
- **Parallel Group**: Sequential
- **Blocks**: TODO 5 (M3: call graph needs symbol table)
- **Blocked By**: TODO 3 (M1: resolver needs parser output)

**References**:

**Pattern References**:
- Design Doc Section 5 (lines ~501–700): Complete module resolution algorithm

**API/Type References**:
- `crates/strato_core/src/types.rs` (M0): `QualifiedName`, `ModulePath`
- `crates/strato_core/src/discovery.rs` (M1): Source root detection
- `crates/strato_core/src/parser.rs` (M1): Parsed module for `__all__` extraction

**Test References**:
- Design Doc Section 21 M2 (lines 3394–3412): Original test names

**WHY Each Reference Matters**:
- Section 5 IS the resolver specification — the base algorithm
- The star import algorithm and namespace package support are specified inline above
- Parser from M1 is needed to extract `__all__` from target modules

**Acceptance Criteria**:

```bash
# 1. All 13 resolver tests pass
cargo test -p strato_core resolver
# Assert: exit code 0, 13 tests pass

# 2. Star import tests specifically
cargo test -p strato_core resolver::test_star_import
# Assert: all 3 star import tests pass

# 3. Namespace package tests
cargo test -p strato_core resolver::test_namespace
# Assert: both namespace tests pass

# 4. Full build still works
cargo build && cargo test
# Assert: exit code 0
```

**Commit**: YES
- Message: `M2: module resolver — cross-file imports, star import resolution, namespace packages`
- Files: `crates/strato_core/src/resolver.rs`, test fixtures, modified `lib.rs`
- Pre-commit: `cargo build && cargo test`

---

### - [ ] 5. M3: Call Graph Construction + TypeResolver via ty

**What to do**:

> This is the most architecturally significant milestone. It replaces v1.0's hand-rolled `ScopeBindings` with ty-backed type inference via a `trait TypeResolver` abstraction.

1. Create `crates/strato_core/src/graph.rs`:
   - Define `CallGraph` (wrapping `petgraph::DiGraph`)
   - Define `CallGraphNode`: `FunctionNode`, `PhantomNode` (for external blocking functions)
   - Define `CallEdge`: `DirectCall`, `PropertyAccess`, `ImplicitDunder`, with `in_executor: bool`, `via: Option<String>` (for unblocker attribution — when a call goes through an executor wrapper, this field names the wrapper)
   - Define `BlockingStatus`: `Unknown`, `KnownBlocking`, `KnownNonBlocking`, `PropagatedBlocking`
   - Define `BlockingReason`: `root_cause`, `call_chain: Vec<ChainLink>`
   - Define `ChainLink`: `function_name`, `function_location`, `call_site_location`, `via: Option<String>`
   - All types as specified in Design Doc Section 6 (lines ~700–920), with `via` field for wrapper attribution

2. Create `crates/strato_core/src/type_resolver.rs`:
   - Define `trait TypeResolver`:

     ```rust
     /// Abstract over type resolution source.
     /// v1.1: Implemented via ty (Astral's type inference engine).
     pub trait TypeResolver {
         /// Given an expression, return the qualified name of its type (if known)
         fn resolve_type(&self, expr: &Expr, file: &Path) -> Option<QualifiedName>;

         /// Given a call expression, return the qualified name of the callee (if known)
         fn resolve_callee(&self, call: &ExprCall, file: &Path) -> Option<QualifiedName>;

         /// Given an attribute access, return the resolved method/property (if known)
         fn resolve_attribute(&self, obj_type: &QualifiedName, attr: &str) -> Option<QualifiedName>;

         /// Compute MRO for a class
         fn mro(&self, class: &QualifiedName) -> Vec<QualifiedName>;
     }
     ```

   - Implement `TyTypeResolver`:
     - Contains ty's Salsa database (setup boilerplate from M-1 findings)
     - Implements all 4 trait methods by querying ty:
       ```rust
       struct TyTypeResolver {
           // ty's Salsa database + semantic model
           db: ty_python_semantic::Db,
       }

       impl TypeResolver for TyTypeResolver {
           fn resolve_type(&self, expr: &Expr, file: &Path) -> Option<QualifiedName> {
               // Query ty's type inference for this expression
               let ty = self.db.infer_expression_type(file, expr.range());
               ty.as_qualified_name()
           }
           // ... etc
       }
       ```
     - **G2**: All methods return `Option<>` — `None` means Unknown, skip silently
     - **G13**: Results cached within the resolver to avoid redundant Salsa queries in hot paths
     - **G18**: All ty calls wrapped in `catch_unwind()` — panic → return `None` + emit warning

   - Implement `NullTypeResolver`:
     - Returns `None` for everything — used as fallback if ty initialization fails

   - **Feed Strato's source roots into ty's module resolver** so ty resolves project-local imports correctly

   **Architecture (v1.1 — replaces ScopeBindings entirely)**:
   ```
   v1.0 (old):                        v1.1 (new):

     Parser (ruff) → AST                Parser (ruff) → AST
          |                                  |
     ScopeBindings (hand-rolled)         ty_python_semantic
          |                                  |
     infer_simple_type()                 ty type queries
          |                                  |
     Call graph builder                  Call graph builder
   ```

   **What ty gives us over ScopeBindings**:

   | Capability | ScopeBindings (v1.0) | ty (v1.1) |
   |-----------|---------------------|-----------|
   | `self`/`cls` type | Yes | Yes |
   | Constructor return type | Yes | Yes |
   | Import resolution | Yes | Yes |
   | Alias tracking (`x = requests.get`) | No | Yes |
   | Return type inference | No | Yes |
   | Attribute type resolution | No | Yes |
   | MRO / inherited methods | No | Yes |
   | Generic instantiation | No | Yes |
   | Type narrowing (`isinstance`) | No | Yes |
   | `functools.partial` semantics | No | Likely |
   | Protocol / structural typing | No | Yes |

   (Note: Not all ty capabilities are used — see ty Feature Budget for what's in-scope.)

3. Create `crates/strato_core/src/graph_builder.rs`:
   - Implement `CallEdgeVisitor`: AST walker that extracts call edges from function bodies
   - Callee resolution: resolve `foo()`, `obj.method()`, `self.method()` to graph nodes
   - **Uses `TypeResolver` instead of `ScopeBindings`** for all type queries:
     - `self` → `type_resolver.resolve_type(self_expr, file)`
     - `cls` → `type_resolver.resolve_type(cls_expr, file)`
     - `MyClass()` → `type_resolver.resolve_callee(call_expr, file)`
     - `obj.method()` → `type_resolver.resolve_attribute(obj_type, "method")`
     - `x = requests.get; x()` → `type_resolver.resolve_callee(x_call, file)` (ty handles alias tracking)
   - Pre-seed phantom nodes from blocking database stub (empty for now — populated in M4)

4. Update `crates/strato_core/src/lib.rs`:
   - Add `pub mod graph; pub mod graph_builder; pub mod type_resolver;`

5. Update `crates/strato_core/Cargo.toml`:
   - Add `petgraph = { workspace = true }`
   - Ensure ty crates are properly referenced

6. Write unit tests (Design Doc Section 21 M3 + new TypeResolver tests):
   - `graph_builder::test_direct_call_edge`
   - `graph_builder::test_method_call_edge`
   - `graph_builder::test_self_method_call`
   - `graph_builder::test_unresolvable_call_skipped` — ty returns None → no edge created
   - `graph_builder::test_lambda_node`
   - `graph_builder::test_type_resolver_constructor` — ty resolves `MyClass()` to `MyClass`
   - `graph_builder::test_type_resolver_self` — ty resolves `self.method()` to correct class method
   - `graph_builder::test_alias_tracking` — `x = requests.get; x()` → edge to `requests.get`
   - `graph_builder::test_return_type_inference` — `loader = get_loader(); loader.load()` → edge to `Loader.load` (if ty infers return type)
   - `type_resolver::test_null_resolver_returns_none` — NullTypeResolver returns None for everything
   - `type_resolver::test_ty_panic_caught` — ty panic → None returned, warning emitted (G18)

**Must NOT do**:
- Do not implement executor detection (that's M6)
- Do not implement property/dunder detection (that's M7)
- Do not implement blocking propagation (that's M5)
- Do not use ty's generic instantiation, type narrowing, or protocol typing (ty Feature Budget)
- Do not use `HashMap` for any collection that affects output ordering (use `BTreeMap`)
- Do not implement `ScopeBindings` — it is replaced entirely by `TypeResolver`

**Recommended Agent Profile**:
- **Category**: `unspecified-high`
  - Reason: Core architecture milestone — wiring ty Salsa DB + TypeResolver trait + AST walking. Requires understanding both ty's API (from M-1 findings) and the design doc's call graph spec.
- **Skills**: `[]`

**Parallelization**:
- **Can Run In Parallel**: NO
- **Parallel Group**: Sequential
- **Blocks**: TODO 6 (M4: database needs graph types)
- **Blocked By**: TODO 4 (M2: graph builder needs resolved symbols)

**References**:

**Pattern References**:
- Design Doc Section 6 (lines ~700–920): Complete call graph specification
- `SPIKE_RESULTS.md` (from M-1): Exact Salsa DB boilerplate, ty API surface documented

**API/Type References**:
- `ty_python_semantic::SemanticModel`: Main type query interface (from M-1 validation)
- `ruff_db::Db`: Database trait for Salsa integration
- `petgraph::DiGraph`: Graph data structure
- `crates/strato_core/src/resolver.rs` (M2): `SymbolTable` for callee resolution
- `crates/strato_core/src/types.rs` (M0): `QualifiedName`, `Location`, `AnalysisWarning`

**Test References**:
- Design Doc Section 21 M3 (lines 3418–3441): Original test names (adapted for TypeResolver)

**WHY Each Reference Matters**:
- Section 6 defines call graph types — every node, edge, status variant
- The TypeResolver trait specified inline above REPLACES ScopeBindings — this is the core change
- M-1 SPIKE_RESULTS.md has the actual Salsa DB boilerplate code to use

**Acceptance Criteria**:

```bash
# 1. All 11 graph/type_resolver tests pass
cargo test -p strato_core graph type_resolver
# Assert: exit code 0, 11 tests pass

# 2. Alias tracking works (core ty value)
cargo test -p strato_core graph_builder::test_alias_tracking
# Assert: pass

# 3. ty panic safety (G18)
cargo test -p strato_core type_resolver::test_ty_panic_caught
# Assert: pass — no crash

# 4. Full build still works
cargo build && cargo test
# Assert: exit code 0
```

**Commit**: YES
- Message: `M3: call graph construction — ty-backed TypeResolver, petgraph graph, AST edge extraction`
- Files: `crates/strato_core/src/graph.rs`, `graph_builder.rs`, `type_resolver.rs`, modified `lib.rs`, `Cargo.toml`
- Pre-commit: `cargo build && cargo test`

---

### - [ ] 6. M4: Blocking Database + Annotation Detection (+ @unblocker + Help Text Policy)

**What to do**:

1. Create `crates/strato_core/src/annotator.rs`:
   - Detect `@blocking`, `@non_blocking`, AND `@unblocker` decorators on functions
   - Match by decorator name pattern (NOT by import resolution):
     - `@blocking`, `@strato.blocking`
     - `@non_blocking`, `@strato.non_blocking`
     - `@unblocker`, `@strato.unblocker`, `@unblocker(callable_param=...)`, `@strato.unblocker(callable_param=...)`
   - For `@blocking` / `@non_blocking`: set `BlockingStatus::KnownBlocking` or `KnownNonBlocking`
   - For `@unblocker`: record in `UnblockerRegistry` with `callable_param` value
   - `@unblocker` without arguments → `callable_param = 0`
   - `@unblocker(callable_param=N)` → extract `N` (integer or string literal)
   - As specified in Design Doc Section 12

   **How `@unblocker` differs from `@non_blocking`** (important — both are needed, they are orthogonal):

   | Aspect | `@non_blocking` | `@unblocker` |
   |--------|-----------------|-------------|
   | **Claim** | "This function itself does not block" | "This function offloads its callable argument to another thread" |
   | **Effect** | Sets the function's status to `KnownNonBlocking` | Creates `in_executor=true` induced edges for wrapped callables |
   | **Scope** | The function's own behavior | The wrapped callable's execution context |
   | **Use case** | CPU-bound work, cached I/O, false positive suppression | `sync_to_async`, custom thread pool wrappers |
   | **Composability** | Stops propagation AT this node | Stops propagation THROUGH this wrapper for the wrapped callable |

2. Create `crates/strato_core/src/database/mod.rs`:
   - Define `BlockingDatabase` struct: lookup by `QualifiedName`
   - `BlockingEntry`: qualified name, reason category, help text
   - Methods: `is_blocking()`, `get_entry()`, `add_user_entry()`, `remove_entry()`

3. Create database entry files (`database/stdlib.rs`, `database/network.rs`, `database/database.rs`, `database/subprocess.rs`):
   - Exactly from Design Doc Section 9 tables
   - **ALL help text MUST follow this policy**:

     **Policy**: Help text MUST NOT suggest specific third-party libraries by name. It should describe the *problem pattern* and *solution patterns* generically.

     **Allowed**:
     - Stdlib alternatives: "Use `asyncio.sleep()` instead" (stdlib is always available)
     - Pattern descriptions: "Offload to a thread with `asyncio.to_thread()`, or use an async alternative"
     - General guidance: "Move I/O out of the property, or convert to an async method"

     **Forbidden**:
     - "Use `httpx` instead of `requests`"
     - "Use `aiofiles.open()` instead"
     - "Consider switching to `asyncpg`"

     **Rationale**: Strato should not be opinionated about which async libraries users adopt. It should identify problems clearly and let users choose their own solutions. Recommending specific libraries creates an implied endorsement, risks going stale as the ecosystem evolves, and doesn't serve users who have already chosen different async libraries.

     **Revised Help Text Examples**:

     | Blocking Function | Help Text |
     |-------------------|-----------|
     | `time.sleep` | "Use `asyncio.sleep()` instead" (stdlib) |
     | `requests.get` | "Offload to a thread with `asyncio.to_thread()`, or use an async HTTP client" |
     | `builtins.open` | "Offload to a thread with `asyncio.to_thread()`, or use an async file API" |
     | `subprocess.run` | "Use `asyncio.create_subprocess_exec()` or offload with `asyncio.to_thread()`" |
     | `psycopg2.connect` | "Use an async database driver, or offload with `asyncio.to_thread()`" |
     | `socket.connect` | "Use `asyncio` socket APIs or an async networking library" |

     **General template**: "Use `{stdlib_async_alternative}` or offload with `asyncio.to_thread()`" — where the stdlib alternative exists. Otherwise: "Offload with `asyncio.to_thread()`, or use an async alternative"

4. Update `crates/strato_core/src/lib.rs`:
   - Add `pub mod annotator; pub mod database;`

5. Wire phantom nodes: Update `graph_builder.rs` to pre-seed `CallGraph` with phantom nodes for all blocking database entries

6. Write unit tests (Design Doc Section 21 M4 + new v1.1 tests):
   - `database::test_builtin_entries_complete` — verify ALL Section 9 entries present
   - `database::test_fixture_required_entries` — `time.sleep`, `requests.get` present
   - `database::test_help_text_no_third_party` — scan ALL help texts, assert none contain known third-party names (`httpx`, `aiofiles`, `asyncpg`, etc.)
   - `database::test_help_text_has_suggestion` — all entries have non-empty help text
   - `database::test_user_config_add` — custom entry addable
   - `database::test_user_config_remove` — built-in entry removable
   - `annotator::test_detect_blocking_decorator`
   - `annotator::test_detect_non_blocking_decorator`
   - `annotator::test_detect_strato_dot_blocking`
   - `annotator::test_ignore_unrelated_decorator`
   - `annotator::test_detect_unblocker_bare` — `@unblocker` detected, callable_param=0
   - `annotator::test_detect_unblocker_with_param` — `@unblocker(callable_param=1)` detected
   - `annotator::test_detect_unblocker_with_string_param` — `@unblocker(callable_param="func")` detected

**Must NOT do**:
- Do not add blocking entries beyond Section 9's tables (G9)
- Do not resolve `from strato import blocking` as an import — match by decorator name only
- Do not implement propagation logic (that's M5)
- Do not suggest third-party libraries in help text (policy above)

**Recommended Agent Profile**:
- **Category**: `unspecified-high`
  - Reason: Data-entry heavy (80+ blocking entries with rewritten help text) plus decorator pattern matching for 3 decorator types
- **Skills**: `[]`

**Parallelization**:
- **Can Run In Parallel**: NO
- **Parallel Group**: Sequential
- **Blocks**: TODO 7 (M5: propagation needs blocking annotations)
- **Blocked By**: TODO 5 (M3: needs graph types for phantom nodes)

**References**:

**Pattern References**:
- Design Doc Section 9 (lines ~1200–1450): Complete blocking database tables
- Design Doc Section 12 (lines ~1880–1960): Annotation API and decorator detection

**API/Type References**:
- `crates/strato_core/src/graph.rs` (M3): `CallGraphNode::PhantomNode`, `BlockingStatus`
- `crates/strato_core/src/types.rs` (M0): `QualifiedName`

**Test References**:
- Design Doc Section 21 M4 (lines 3466–3477): Original test names

**WHY Each Reference Matters**:
- Section 9 has EVERY blocking entry — transcribe exactly
- The help text policy specified inline above must be applied to every entry
- The `@unblocker` decorator detection pattern is specified inline — match by name, extract `callable_param`

**Acceptance Criteria**:

```bash
# 1. Annotator tests pass (including @unblocker)
cargo test -p strato_core annotator
# Assert: exit code 0, 7 tests pass

# 2. Database tests pass (including help text policy)
cargo test -p strato_core database
# Assert: exit code 0, 6 tests pass

# 3. Help text policy specifically
cargo test -p strato_core database::test_help_text_no_third_party
# Assert: pass — no third-party library names in any help text

# 4. Full build still works
cargo build && cargo test
# Assert: exit code 0
```

**Commit**: YES
- Message: `M4: blocking database + annotations — 80+ entries, @unblocker detection, help text policy`
- Files: `crates/strato_core/src/annotator.rs`, `database/` (all files), modified `lib.rs`, `graph_builder.rs`
- Pre-commit: `cargo build && cargo test`

---

### - [ ] 7. M5: Blocking Propagation (SCC Algorithm)

**What to do**:

> The propagation algorithm itself is UNCHANGED from v1.0. The SCC-based single-pass approach works identically. What changes is that **induced edges** from the wrapper registry (M6) will participate in propagation — but those edges don't exist yet. M5 implements the propagation engine; M6 adds the wrapper edges that feed into it.

1. Create `crates/strato_core/src/propagator.rs`:
   - Implement Tarjan's SCC decomposition using `petgraph::algo::tarjan_scc()`
   - Build condensation graph (DAG of SCCs)
   - Topological sort the condensation DAG
   - Propagate blocking status in reverse topological order:
     - Within each SCC: if ANY member is `KnownBlocking`, ALL become `PropagatedBlocking` (unless `@non_blocking`)
     - Between SCCs: if callee SCC is blocking AND edge is NOT `in_executor`, caller SCC becomes `PropagatedBlocking`
   - **Edge aggregation rule** (Design Doc line 1055-1059): When condensing multiple edges between the same SCC pair, `all_calls_in_executor` is `true` ONLY if ALL edges have `in_executor=true`. A single non-executor edge means the condensed edge is `in_executor=false`. **This rule applies equally to induced edges from the wrapper registry** (added in M6).
   - Build `BlockingReason` with complete `call_chain: Vec<ChainLink>` including `via` field for wrapper attribution
   - **SACRED INVARIANT**: `Unknown` nodes MUST stay `Unknown` (G1)
   - `@non_blocking` within SCC: shields entire SCC from propagation
   - O(V+E) guaranteed — single pass
   - As specified in Design Doc Section 7 (lines ~920–1200)

2. Update `crates/strato_core/src/lib.rs`:
   - Add `pub mod propagator;`

3. Write unit tests (Design Doc Section 21 M5):
   - `propagator::test_direct_blocking_propagation`
   - `propagator::test_transitive_propagation`
   - `propagator::test_executor_edge_blocks_propagation`
   - `propagator::test_non_blocking_stops_propagation`
   - `propagator::test_cycle_handling`
   - `propagator::test_cycle_no_blocking`
   - `propagator::test_unknown_stays_unknown`
   - `propagator::test_blocking_reason_path`
   - `propagator::test_via_field_in_chain` — `ChainLink.via` populated when present on edge

**Must NOT do**:
- Do not implement iterative fixpoint — SCC decomposition ensures single-pass
- Do not reclassify `Unknown` nodes (G1)
- Do not optimize performance (G7)
- Do not implement wrapper registry (that's M6 — M5 just handles the `in_executor` flag on edges)

**Recommended Agent Profile**:
- **Category**: `ultrabrain`
  - Reason: SCC algorithm + topological propagation is algorithmically complex. Tarjan's, condensation, reverse topological propagation, edge aggregation rule. A single bug breaks entire analysis.
- **Skills**: `[]`

**Parallelization**:
- **Can Run In Parallel**: NO
- **Parallel Group**: Sequential
- **Blocks**: TODO 8 (M6: wrapper edges feed into propagation)
- **Blocked By**: TODO 6 (M4: needs annotated graph)

**References**:

**Pattern References**:
- Design Doc Section 7 (lines ~920–1200): COMPLETE propagation algorithm
- Design Doc lines 1055-1059: Edge aggregation rule for SCC condensation

**API/Type References**:
- `petgraph::algo::tarjan_scc()`: Returns `Vec<Vec<NodeIndex>>`
- `crates/strato_core/src/graph.rs` (M3): `CallGraph`, `BlockingStatus`, `CallEdge.in_executor`, `CallEdge.via`, `ChainLink.via`

**Test References**:
- Design Doc Section 21 M5 (lines 3498–3507): Original test names

**WHY Each Reference Matters**:
- Section 7 IS the propagation algorithm — every rule for SCC handling
- The edge aggregation rule (lines 1055-1059) is critical — one non-executor edge taints the whole condensed edge

**Acceptance Criteria**:

```bash
# 1. All 9 propagation tests pass
cargo test -p strato_core propagator
# Assert: exit code 0, 9 tests pass

# 2. Sacred invariant
cargo test -p strato_core propagator::test_unknown_stays_unknown
# Assert: pass

# 3. Via field propagation
cargo test -p strato_core propagator::test_via_field_in_chain
# Assert: pass

# 4. Full build
cargo build && cargo test
# Assert: exit code 0
```

**Commit**: YES
- Message: `M5: blocking propagation — SCC-based single-pass algorithm, O(V+E), via field support`
- Files: `crates/strato_core/src/propagator.rs`, modified `lib.rs`
- Pre-commit: `cargo build && cargo test`

---

### - [ ] 8. M6: Generalized Wrapper Registry (Escape Hatches)

**What to do**:

> **Major change from v1.0**: The v1.0 plan hardcoded two escape hatch patterns (`run_in_executor`, `asyncio.to_thread`). v1.1 replaces this with a **generalized executor wrapper registry**. Built-in patterns become entries in the registry, not special-cased code.

#### Concept: Executor Wrapper

An **executor wrapper** is a function that takes a callable argument and arranges for it to execute off the event loop thread. The wrapper *removes the blocking taint* from calls to the wrapped callable.

Built-in wrappers (always active, not configurable):
- `asyncio.loop.run_in_executor` (callable at position 1)
- `asyncio.to_thread` (callable at position 0)

User-configurable wrappers (via `[tool.strato.executor-wrappers]` config):
- `asgiref.sync.sync_to_async` (callable at position 0)
- `anyio.to_thread.run_sync` (callable at position 0)
- Any user-defined wrapper

First-party wrappers (via `@unblocker` decorator — detected in M4):
- Any function decorated with `@unblocker` or `@unblocker(callable_param=...)`

#### Implementation

1. Create `crates/strato_core/src/wrapper_registry.rs`:

   ```rust
   pub struct WrapperRegistry {
       entries: BTreeMap<QualifiedName, WrapperEntry>,  // G5: deterministic
   }

   pub struct WrapperEntry {
       callable_param: CallableParam,
       source: WrapperSource,  // BuiltIn, Config, Decorator
   }

   /// Which parameter receives the callable to offload.
   /// Can be positional (usize) or keyword (String).
   /// Both are tried at call sites — positional first, then keyword.
   pub enum CallableParam {
       Position(usize),
       Name(String),
   }
   ```

   - **Built-in entries** (always active, not configurable):
     - `asyncio.loop.run_in_executor` → `callable_param = Position(1)`
     - `asyncio.to_thread` → `callable_param = Position(0)`
   - **Config entries**: Loaded from `[tool.strato.executor-wrappers]` (parsed in M9, passed into registry):
     ```toml
     # pyproject.toml
     [tool.strato.executor-wrappers]
     "asgiref.sync.sync_to_async" = { callable_param = 0 }
     "anyio.to_thread.run_sync" = { callable_param = 0 }
     "starlette.concurrency.run_in_threadpool" = { callable_param = 0 }
     "myutils.offload" = { callable_param = "target_func" }
     ```

     Config schema in Rust:
     ```rust
     struct ExecutorWrapperConfig {
         /// Qualified name of the wrapper function
         name: QualifiedName,
         /// Which parameter receives the callable to offload.
         callable_param: CallableParam,
     }
     ```

   - **Decorator entries**: Loaded from `@unblocker` annotations (detected in M4, passed into registry)

2. Modify `crates/strato_core/src/graph_builder.rs`:

   - **Replace** `is_executor_call()` / `is_likely_event_loop()` with registry-based lookup:
     - When encountering a call expression, check if callee is in `WrapperRegistry`
     - If yes: extract the callable argument at the configured `callable_param` position/name
     - If the callable argument resolves to a known function `f`:
       - Create an **induced edge** from caller to `f` with `in_executor = true` and `via = Some(wrapper_name)`
     - The wrapper function itself is also called normally (its body is analyzed)

   **Graph semantics for wrapper calls**:

   When a call matches a known executor wrapper:
   1. Identify the callable argument at the configured parameter position
   2. If the callable resolves to a known function `f`:
      - Create an **induced edge** from the caller to `f` with `in_executor = true`
      - The edge carries `via: Some("sync_to_async")` for diagnostic clarity
   3. The wrapper function itself is treated normally (its own body is analyzed)

   **Example**:
   ```python
   async def handler():
       safe_func = sync_to_async(blocking_db_query)  # wrapper recognized
       result = await safe_func()                      # safe — induced edge has in_executor=true
   ```

   Graph edges:
   - `handler -> sync_to_async`: DirectCall (normal)
   - `handler -> blocking_db_query`: InducedEdge (in_executor=true, via="sync_to_async")

   The InducedEdge prevents blocking propagation from `blocking_db_query` to `handler`.

   **Resolution at call sites**:
   ```
   call = sync_to_async(my_blocking_func, thread_sensitive=False)
                        ^^^^^^^^^^^^^^^^^
                        callable_param = 0 → this argument
   ```
   1. Look up the callee in the executor wrapper registry
   2. Extract the argument at the configured position/name
   3. If the argument resolves to a known function, create an induced edge with `in_executor=true`

   - **Alias tracking for wrapped callables**: Use `TypeResolver` to resolve `safe = sync_to_async(func); safe()`:
     - `sync_to_async(func)` → wrapper call recognized, records `func` as the wrapped callable
     - `safe()` → `TypeResolver` resolves `safe` (ty handles the value flow)
     - **If ty can't resolve the alias**: the unblocker protection is silently lost. The direct call to `safe()` creates a normal edge (not `in_executor`). This is a **known limitation** — following the "Unknown = skip" principle. The result is a false positive on `safe()`, which is safe (over-reporting, not under-reporting).

   - **`functools.partial` support**: If ty resolves `partial(blocking_func, arg1)` → extract the first argument. If ty can't → skip (known limitation).

3. Update `crates/strato_core/src/lib.rs`:
   - Add `pub mod wrapper_registry;`

4. Write unit tests:
   - `wrapper_registry::test_builtin_entries_exist` — `run_in_executor` and `to_thread` always present
   - `wrapper_registry::test_config_entry_added` — Custom wrapper addable via config
   - `wrapper_registry::test_decorator_entry_added` — `@unblocker` result addable
   - `graph_builder::test_wrapper_run_in_executor` — `run_in_executor` creates induced edge
   - `graph_builder::test_wrapper_to_thread` — `to_thread` creates induced edge
   - `graph_builder::test_wrapper_config_entry` — Config-defined wrapper creates induced edge
   - `graph_builder::test_wrapper_unblocker_decorator` — `@unblocker` function creates induced edge
   - `graph_builder::test_wrapper_only_callable_arg_protected` — Non-callable args not affected
   - `graph_builder::test_wrapper_via_field` — Induced edge has `via: Some("sync_to_async")`
   - `graph_builder::test_wrapper_alias_resolved_by_ty` — `safe = sync_to_async(func); safe()` → induced edge if ty resolves
   - `graph_builder::test_wrapper_alias_unresolved_by_ty` — If ty can't resolve alias → normal edge (known limitation)
   - `graph_builder::test_wrapper_induced_plus_direct_edge` — Same function called directly AND through wrapper → mixed `in_executor` flags

**Must NOT do**:
- Do not add wrapper parameters beyond `callable_param` (G15)
- Do not implement conflict resolution between config and decorator for same qualified name — last-registered wins
- Do not implement recursive wrapper unwrapping (wrapper wrapping a wrapper)
- Do not hardcode `is_likely_event_loop()` — the registry handles everything

**Recommended Agent Profile**:
- **Category**: `unspecified-high`
  - Reason: New registry system replacing hardcoded patterns + induced edge creation + alias tracking interaction with ty
- **Skills**: `[]`

**Parallelization**:
- **Can Run In Parallel**: NO
- **Parallel Group**: Sequential
- **Blocks**: TODO 9 (M7: properties need wrapper system to not interfere)
- **Blocked By**: TODO 7 (M5: propagation must handle `in_executor` edges)

**References**:

**Pattern References**:
- Design Doc Section 11 (lines ~1700–1870): Original escape hatch patterns (now registry entries)

**API/Type References**:
- `crates/strato_core/src/graph_builder.rs` (M3): Existing visitor to modify
- `crates/strato_core/src/graph.rs` (M3): `CallEdge.in_executor`, `CallEdge.via`
- `crates/strato_core/src/annotator.rs` (M4): `@unblocker` detection results

**Test References**:
- Design Doc Section 21 M6 (lines 3516–3531): Original escape hatch test names (adapted for registry)

**WHY Each Reference Matters**:
- Section 11 defines the patterns that become built-in registry entries
- The full wrapper registry spec, graph semantics, config schema, and alias tracking rules are specified inline above

**Acceptance Criteria**:

```bash
# 1. Registry tests pass
cargo test -p strato_core wrapper_registry
# Assert: exit code 0, 3 tests pass

# 2. Wrapper integration tests pass
cargo test -p strato_core graph_builder::test_wrapper
# Assert: exit code 0, 8+ tests pass

# 3. Via field populated
cargo test -p strato_core graph_builder::test_wrapper_via_field
# Assert: pass

# 4. Full build
cargo build && cargo test
# Assert: exit code 0
```

**Commit**: YES
- Message: `M6: generalized wrapper registry — run_in_executor/to_thread as entries, config + @unblocker support`
- Files: `crates/strato_core/src/wrapper_registry.rs`, modified `graph_builder.rs`, `lib.rs`
- Pre-commit: `cargo build && cargo test`

---

### - [ ] 9. M7: Properties and Dunder Methods (ty-Enhanced)

**What to do**:

> ty integration improves property and dunder detection. Where v1.0 could only detect properties on `self` and direct imports, ty can resolve attribute types through return values, aliases, and (limited) MRO traversal.

1. Modify `crates/strato_core/src/graph_builder.rs`:
   - **Property detection**: When encountering `obj.attr` (attribute access):
     - Query `type_resolver.resolve_type(obj, file)` to get `obj`'s type
     - Query `type_resolver.resolve_attribute(obj_type, "attr")` to check if it's a `@property`
     - If yes, create `CallEdge::PropertyAccess` edge to the getter
   - **Dunder method mapping** (unchanged from v1.0): Map Python syntax to implicit dunder calls:
     - `str(obj)` → `obj.__str__()`
     - `a == b` → `a.__eq__(b)`
     - `x[k]` → `x.__getitem__(k)`
     - `with x:` → `x.__enter__()` + `x.__exit__()`
     - `for i in x:` → `x.__iter__()`
     - `f"{x}"` → `x.__format__("")`
     - Full table in Design Doc Section 10
   - For dunder resolution: query `type_resolver.resolve_type(obj, file)` then `type_resolver.resolve_attribute(obj_type, "__dunder__")`
   - **ty enables MRO-based dunder lookup**: `type_resolver.mro(class)` resolves inherited `__enter__` etc. **But**: only used for property/dunder lookup, NOT for blocking propagation (ty Feature Budget).
   - **High precision**: If `type_resolver` returns `None`, do NOT create dunder/property edge (skip silently, G2)

2. Write unit tests (Design Doc Section 21 M7 + ty-enhanced tests):
   - `graph_builder::test_property_access_creates_edge`
   - `graph_builder::test_property_non_property_attribute_no_edge`
   - `graph_builder::test_dunder_str_builtin`
   - `graph_builder::test_dunder_eq_operator`
   - `graph_builder::test_dunder_getitem`
   - `graph_builder::test_dunder_with_statement`
   - `graph_builder::test_dunder_for_loop`
   - `graph_builder::test_dunder_fstring`
   - `graph_builder::test_dunder_unknown_type_skipped`
   - `graph_builder::test_property_via_return_type` — `get_loader()` returns `Loader`, `loader.data` is `@property` → edge created (ty resolves return type)
   - `graph_builder::test_dunder_inherited` — `SubClass` inherits `__enter__` from `BaseClass` → edge created (ty MRO)

**Must NOT do**:
- Do not detect `@cached_property` (only `@property`)
- Do not use MRO for blocking propagation — only for property/dunder lookup (ty Feature Budget)
- Do not handle `__aenter__`/`__aexit__` (async context managers, not blocking)
- Do not create edges for unknown types (G2)

**Recommended Agent Profile**:
- **Category**: `unspecified-high`
  - Reason: Complex AST pattern matching enhanced by ty type resolution
- **Skills**: `[]`

**Parallelization**:
- **Can Run In Parallel**: NO
- **Parallel Group**: Sequential
- **Blocks**: TODO 10 (M8: reporter needs all edge types)
- **Blocked By**: TODO 8 (M6: must be after wrapper registry)

**References**:

**Pattern References**:
- Design Doc Section 10 (lines ~1450–1700): Properties and dunder method specification

**API/Type References**:
- `crates/strato_core/src/graph.rs` (M3): `CallEdge::PropertyAccess`, `CallEdge::ImplicitDunder`
- `crates/strato_core/src/type_resolver.rs` (M3): `TypeResolver` trait — `resolve_type`, `resolve_attribute`, `mro`

**Test References**:
- Design Doc Section 21 M7 (lines 3540–3559): Original test names

**WHY Each Reference Matters**:
- Section 10 has the complete dunder mapping table
- `TypeResolver` from M3 is how property/dunder resolution happens — replaces the old simple inference

**Acceptance Criteria**:

```bash
# 1. Property tests pass
cargo test -p strato_core graph_builder::test_property
# Assert: exit code 0, 3 tests pass (including return type test)

# 2. Dunder tests pass
cargo test -p strato_core graph_builder::test_dunder
# Assert: exit code 0, 8 tests pass (including inherited test)

# 3. Full build
cargo build && cargo test
# Assert: exit code 0
```

**Commit**: YES
- Message: `M7: property + dunder detection — ty-enhanced resolution, inherited dunders via MRO`
- Files: modified `crates/strato_core/src/graph_builder.rs`
- Pre-commit: `cargo build && cargo test`

---

### - [ ] 10. M8: Diagnostic Reporting + Related Locations

**What to do**:

1. Create `crates/strato_core/src/reporter.rs`:
   - Define `Diagnostic`:

     ```rust
     pub struct Diagnostic {
         pub code: ErrorCode,
         pub severity: Severity,
         pub primary_location: Location,
         pub message: String,
         pub blocking_chain: Vec<ChainLink>,
         pub strategy: InterventionStrategy,
         pub help: Option<String>,
         /// Secondary locations that provide additional context.
         /// Always includes both the "trigger site" and the "blocking site"
         /// when they differ from the primary location.
         pub related_locations: Vec<RelatedLocation>,
     }
     ```

   - Define `DiagnosticSet`: deterministic ordering (G5)

   - **Intervention point strategy** (unchanged):
     - `first-party-deepest`: Deepest first-party function in chain as primary
     - `async-boundary`: Async→sync transition point as primary

   - **Error code classification** (unchanged, G6):
     - STRATO001: `chain_length == 1` AND caller is async
     - STRATO002: `chain_length > 1`
     - STRATO003: Last edge is `PropertyAccess`
     - STRATO004: Last edge is `ImplicitDunder`

   - **Related locations per error code** — the primary location is determined by the intervention strategy (consistent across all error codes). Related locations provide supplementary context:

     | Code | Primary Location | Related Locations |
     |------|-----------------|-------------------|
     | STRATO001 | The blocking call (per strategy) | `["async context here" → the async function definition]` |
     | STRATO002 | first-party-deepest in chain (per strategy) | `["blocking call executes here" → leaf blocking call, "called from async context here" → async function's call site]` |
     | STRATO003 | Property getter body if first-party, else access site | `["blocking property accessed here" → the obj.prop expression, "blocking call executes here" → the call inside the getter]` |
     | STRATO004 | Dunder method body if first-party, else syntax site | `["implicit dunder invoked here" → the syntax triggering it, "blocking call executes here" → the call inside the dunder]` |

     **Example for STRATO003**:

     Before (v1.0 — no related locations):
     ```
     error[STRATO003]: Blocking property access in async context
       --> src/models/user.py:34:9
        |
     34 |         return requests.get(self.avatar_url).content
        |                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `requests.get` blocks
     ```

     After (v1.1 — with related locations):
     ```
     error[STRATO003]: Blocking property access in async context
       --> src/models/user.py:34:9
        |
     34 |         return requests.get(self.avatar_url).content
        |                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `requests.get` blocks
        |
       ::: src/api/users.py:52:14
        |
     52 |     result = loader.data
        |              ----------- blocking property accessed here
     ```

     The primary location follows `first-party-deepest` (here: the getter is first-party, so it points there). The related location shows the access site. If the getter were third-party, the primary would shift to the access site instead.

   - **Impact on output formats**:
     - **Text**: Related locations shown as secondary spans (`::: file:line`)
     - **JSON**: `"related_locations": [{"file": "...", "line": N, "label": "..."}]`
     - **SARIF**: Mapped to `relatedLocations` array in SARIF result objects (native SARIF concept)

   - **Wrapper attribution in chain**: When a `ChainLink` has `via: Some("sync_to_async")`, include it in the message: "through `sync_to_async` (executor wrapper)"

2. Update `crates/strato_core/src/lib.rs`:
   - Add `pub mod reporter;`

3. Write unit tests (Design Doc Section 21 M8 + new related location tests):
   - `reporter::test_first_party_deepest_strategy`
   - `reporter::test_async_boundary_strategy`
   - `reporter::test_all_third_party_fallback`
   - `reporter::test_error_code_strato001`
   - `reporter::test_error_code_strato002`
   - `reporter::test_error_code_strato003`
   - `reporter::test_error_code_strato004`
   - `reporter::test_diagnostic_message_format`
   - `reporter::test_related_locations_strato001` — includes "async context here"
   - `reporter::test_related_locations_strato002` — includes blocking call + async context
   - `reporter::test_related_locations_strato003` — includes property access site
   - `reporter::test_related_locations_strato004` — includes dunder invocation syntax site
   - `reporter::test_via_attribution_in_message` — wrapper name appears in chain description

**Must NOT do**:
- Do not format output (that's M9)
- Do not implement the CLI (that's M9)
- Do not add colors or pretty-printing (M9's miette)

**Recommended Agent Profile**:
- **Category**: `unspecified-high`
  - Reason: Diagnostic generation with per-error-code related location logic + wrapper attribution
- **Skills**: `[]`

**Parallelization**:
- **Can Run In Parallel**: NO
- **Parallel Group**: Sequential
- **Blocks**: TODO 11 (M9: CLI needs diagnostics)
- **Blocked By**: TODO 9 (M7: needs all edge types for classification)

**References**:

**Pattern References**:
- Design Doc Section 8 (lines ~1200–1450): Error reporting model

**API/Type References**:
- `crates/strato_core/src/graph.rs` (M3): `BlockingReason`, `ChainLink`, `CallEdge` variants
- `crates/strato_core/src/database/mod.rs` (M4): `BlockingDatabase` for help text
- `crates/strato_core/src/types.rs` (M0): `RelatedLocation`

**Test References**:
- Design Doc Section 21 M8 (lines 3581–3589): Original test names

**WHY Each Reference Matters**:
- Section 8 defines diagnostic generation — intervention strategies, error codes
- The related location specs per error code are defined inline above

**Acceptance Criteria**:

```bash
# 1. All 13 reporter tests pass
cargo test -p strato_core reporter
# Assert: exit code 0, 13 tests pass

# 2. Related locations tests specifically
cargo test -p strato_core reporter::test_related_locations
# Assert: all 4 pass

# 3. Full build
cargo build && cargo test
# Assert: exit code 0
```

**Commit**: YES
- Message: `M8: diagnostic reporting — STRATO001-004, related locations, wrapper attribution`
- Files: `crates/strato_core/src/reporter.rs`, modified `lib.rs`
- Pre-commit: `cargo build && cargo test`

---

### - [ ] 11. M9: CLI + Output Formats + Warnings + Wrapper Config

**What to do**:

1. Create `crates/strato_cli/src/args.rs`:
   - CLI argument parsing with clap (all flags from v1.0 + no new flags)
   - Exit codes: 0 (clean), 1 (issues found), 2 (config error), 3 (all files failed to parse)
   - **Warnings do NOT affect exit code** — exit 0 if no errors, regardless of warnings
   - As specified in Design Doc Section 14

2. Create `crates/strato_cli/src/config.rs`:
   - Parse `[tool.strato]` from pyproject.toml
   - Parse `[tool.strato.executor-wrappers]` config section:

     ```toml
     # pyproject.toml
     [tool.strato]
     severity = "error"
     first-party = ["myapp"]

     # Built-in wrappers (always active):
     #   asyncio.loop.run_in_executor (callable_param = 1)
     #   asyncio.to_thread (callable_param = 0)
     #
     # Additional wrappers:
     [tool.strato.executor-wrappers]
     "asgiref.sync.sync_to_async" = { callable_param = 0 }
     "anyio.to_thread.run_sync" = { callable_param = 0 }
     "starlette.concurrency.run_in_threadpool" = { callable_param = 0 }
     "myutils.offload" = { callable_param = "target_func" }
     ```

     The config parser must convert each entry into an `ExecutorWrapperConfig`:
     ```rust
     struct ExecutorWrapperConfig {
         name: QualifiedName,
         callable_param: CallableParam,
     }
     enum CallableParam {
         Position(usize),
         Name(String),
     }
     ```

   - Convert config entries to `WrapperEntry` objects for the registry
   - CLI flags override config file values

3. Create output formatters (`output/mod.rs`, `output/text.rs`, `output/json.rs`, `output/sarif.rs`):
   - `trait OutputFormatter { fn format(&self, result: &AnalysisResult) -> String; }`

   - **Text formatter**: miette for pretty errors
     - Related locations rendered as secondary spans (`::: file:line`)
     - Warnings printed after diagnostics, dimmed, prefixed with `warning:`

   - **JSON formatter**: `{ "version": "1.0", "diagnostics": [...], "warnings": [...], "stats": {...} }`
     - Each diagnostic includes `"related_locations": [{"file", "line", "column", "label"}]`
     - Top-level `"warnings"` array: `[{"type": "ParseError", "file": "...", "error": "...", "line": N}]`

   - **SARIF formatter**: SARIF v2.1.0
     - `relatedLocations` mapped to native SARIF concept (SARIF natively supports this)
     - Warnings as results with `"level": "note"`

   **Warning output example** (text format):
   ```
   error[STRATO002]: Indirect blocking call reachable from async context
     --> src/services/email.py:23:5
      ...

   warning: failed to parse src/legacy/broken.py: unexpected token at line 42
   warning: failed to parse src/generated/proto.py: invalid syntax at line 1

   Found 1 error, 2 warnings in 1.8s (analyzed 245 of 247 files)
   ```

4. **CRITICAL**: Create `pub fn analyze()` orchestrator in `crates/strato_core/src/lib.rs`:
   - Wire up the full 7-phase pipeline
   - `AnalysisResult` includes `warnings: Vec<AnalysisWarning>`
   - Pipeline receives `WrapperRegistry` (populated from config + annotations)
   - Signature: `pub fn analyze(project_path: &Path, config: &Config) -> Result<AnalysisResult, AnalysisError>`
   - `Config` includes `executor_wrappers: Vec<ExecutorWrapperConfig>` from parsed config

5. Update `crates/strato_cli/src/main.rs`:
   - Wire: parse args → load config → build wrapper registry (built-ins + config entries) → call `analyze()` → format output → set exit code
   - Stats summary includes warning counts: "Found N errors, M warnings in Xs (analyzed Y of Z files)"

**Must NOT do**:
- Do not implement `strato init` or any other subcommand (G10)
- Do not implement caching integration (that's M10)
- Do not implement watch mode (G3)
- Do not add autofix (G3)
- Do not make warnings affect exit code

**Recommended Agent Profile**:
- **Category**: `unspecified-high`
  - Reason: Largest milestone — CLI assembly + 3 formatters + pipeline orchestration + config parsing + warning rendering
- **Skills**: `[]`

**Parallelization**:
- **Can Run In Parallel**: NO
- **Parallel Group**: Sequential
- **Blocks**: TODO 12 (M10: caching integrates into CLI pipeline)
- **Blocked By**: TODO 10 (M8: needs diagnostic types)

**References**:

**Pattern References**:
- Design Doc Section 14 (lines ~2200–2400): CLI interface spec
- Design Doc Section 15 (lines ~2400–2500): Output format specs
- Design Doc Section 13 (lines ~2100–2200): Configuration

**API/Type References**:
- `crates/strato_core/src/*.rs` (M1–M8): All phases to orchestrate
- `crates/strato_core/src/wrapper_registry.rs` (M6): `WrapperRegistry` to populate from config
- `clap::Parser`, `miette::Report`, `serde_json`

**Test References**:
- Design Doc Section 21 M9 (lines 3600–3621): Verification commands

**WHY Each Reference Matters**:
- Section 14/15 define CLI and output format contracts
- The related location rendering, warning output, and config parsing specs are all inline above

**Acceptance Criteria**:

```bash
# 1. Binary compiles
cargo build -p strato_cli
# Assert: exit code 0

# 2. Help output
cargo run -p strato_cli -- check --help
# Assert: shows all options

# 3. Smoke test produces JSON with related_locations and warnings
cargo run -p strato_cli -- check tests/fixtures/smoke/ --format json 2>/dev/null | python3 -c "
import sys, json
d = json.load(sys.stdin)
assert 'diagnostics' in d, 'missing diagnostics'
assert 'warnings' in d, 'missing warnings key'
assert len(d['diagnostics']) > 0, 'no diagnostics'
diag = d['diagnostics'][0]
assert diag['code'] == 'STRATO001', 'wrong code'
assert 'related_locations' in diag, 'missing related_locations'
print('Smoke test passed')
"

# 4. Exit code 0 when clean (no errors, even with warnings)
cargo run -p strato_cli -- check tests/fixtures/smoke_clean/ 2>/dev/null
echo "Exit code: $?"
# Assert: exit code 0

# 5. Version flag
cargo run -p strato_cli -- --version
# Assert: prints version

# 6. Full build
cargo build && cargo test
# Assert: exit code 0
```

**Commit**: YES
- Message: `M9: CLI + output formats — text/JSON/SARIF with related locations, warnings, wrapper config`
- Files: `crates/strato_cli/src/args.rs`, `config.rs`, `output/*.rs`, modified `main.rs`, `Cargo.toml`; modified `crates/strato_core/src/lib.rs` (analyze function)
- Pre-commit: `cargo build && cargo test`

---

### - [ ] 12. M10: Caching System (ty-Aware)

**What to do**:

> **Key design decision (from Metis review)**: ty's type resolution is cross-file — changing file B might change ty's resolution in file A. Therefore, **ty results are NOT cached across runs**. Only parse results and import statements are cached. Salsa's internal memoization handles within-run performance. This is the simplest approach that preserves correctness.

1. Create `crates/strato_cache/src/manifest.rs`:
   - Cache manifest: maps file paths to content hashes (SHA-256)

2. Create `crates/strato_cache/src/storage.rs`:
   - Binary cache read/write using bincode
   - **Cached per file**: `FileSymbols` (parsed function/class defs), import statements
   - **NOT cached**: Call edges (depend on type resolution), blocking annotations (depend on graph), propagation results
   - Cache location: `.strato-cache/` directory in project root

3. Create `crates/strato_cache/src/invalidation.rs`:
   - File content hash changed → re-parse that file
   - File added/deleted → invalidate affected modules
   - Config changed → invalidate all
   - **Note**: Since ty results aren't cached, type resolution always runs fresh

4. Update `crates/strato_cache/src/lib.rs`:
   - Public API: `Cache::load()`, `Cache::save()`, `Cache::is_fresh()`

5. Integrate into CLI pipeline:
   - Before parse: check cache, skip re-parsing cached files
   - After analysis: save updated cache
   - `--no-cache`, `--clear-cache`, `--stats` flags

6. Write unit tests:
   - `cache::test_fresh_creates_cache`
   - `cache::test_cached_run_skips_parse`
   - `cache::test_modified_file_invalidates`
   - `cache::test_no_cache_flag`
   - `cache::test_clear_cache_flag`
   - **Cache correctness**: fresh run diagnostics == cached run diagnostics

**Must NOT do**:
- Do not cache ty type resolution results across runs (correctness risk from cross-file dependencies)
- Do not cache call graph edges (they depend on ty resolution)
- Do not implement incremental graph updates (v2)
- Do not optimize cache format (G7)

**Recommended Agent Profile**:
- **Category**: `unspecified-high`
  - Reason: Cache system with specific ty-aware design constraints
- **Skills**: `[]`

**Parallelization**:
- **Can Run In Parallel**: NO
- **Parallel Group**: Sequential
- **Blocks**: TODO 13 (M11: integration tests need full pipeline)
- **Blocked By**: TODO 11 (M9: CLI pipeline to integrate into)

**References**:

**Pattern References**:
- Design Doc Section 16 (lines ~2500–2600): Caching strategy (adapted for ty-aware design)

**API/Type References**:
- `crates/strato_cli/src/main.rs` (M9): Pipeline to add cache checks
- `bincode`, `sha2`: Serialization and hashing

**WHY Each Reference Matters**:
- Section 16 defines cache format and invalidation rules — adapted because ty resolution is NOT cached

**Acceptance Criteria**:

```bash
# 1. Cache crate compiles
cargo build -p strato_cache
# Assert: exit code 0

# 2. First run creates cache
cargo run -p strato_cli -- check tests/fixtures/smoke/ --stats 2>&1 | grep -i "cache"
# Assert: shows cache miss or cache created

# 3. Second run shows cache benefit
cargo run -p strato_cli -- check tests/fixtures/smoke/ --stats 2>&1 | grep -i "cache"
# Assert: shows cache hit for parse phase

# 4. Cache correctness
diff <(cargo run -p strato_cli -- check tests/fixtures/smoke/ --no-cache --format json 2>/dev/null) <(cargo run -p strato_cli -- check tests/fixtures/smoke/ --format json 2>/dev/null)
# Assert: no diff

# 5. Full build
cargo build && cargo test
# Assert: exit code 0
```

**Commit**: YES
- Message: `M10: caching system — parse-level caching, ty results always fresh (cross-file safety)`
- Files: `crates/strato_cache/src/*.rs`, modified CLI `main.rs`
- Pre-commit: `cargo build && cargo test`

---

### - [ ] 13. M11: Integration Tests (19 Fixtures)

**What to do**:

1. Create ALL 13 original fixture directories (Appendix A):
   - `tests/fixtures/a01_direct_blocking/` through `tests/fixtures/a13_mixed_safe_unsafe/`
   - Python source files and `expected.json` exactly as specified in Design Doc Appendix A (lines 2840–3073)
   - `expected.json` format includes `related_locations` array for each diagnostic

2. Create 6 NEW v1.1 fixture directories:

   - **`tests/fixtures/a14_unblocker_basic/`**:
     ```python
     from strato import unblocker
     import time

     @unblocker
     def my_offload(func):
         return func()  # pretend this offloads

     async def handler():
         await my_offload(time.sleep)  # Should NOT emit — offloaded via @unblocker
         time.sleep(1)  # STRATO001 — direct blocking
     ```
     Expected: 1 diagnostic (only the direct call), 0 for the wrapped call

   - **`tests/fixtures/a15_executor_wrapper_config/`**:
     ```python
     # pyproject.toml: [tool.strato.executor-wrappers]
     # "mylib.offload" = { callable_param = 0 }
     import time
     from mylib import offload

     async def handler():
         await offload(time.sleep)  # Should NOT emit — config-defined wrapper
     ```
     Expected: 0 diagnostics (wrapper config suppresses)

   - **`tests/fixtures/a16_star_import/`**:
     ```python
     # module_a.py: __all__ = ["blocking_func"]
     # module_a.py: def blocking_func(): time.sleep(1)
     # main.py: from module_a import *; async def handler(): blocking_func()
     ```
     Expected: 1 diagnostic — STRATO002 (indirect via star-imported function)

   - **`tests/fixtures/a17_namespace_package/`**:
     ```python
     # myns/utils.py (no __init__.py in myns/)
     # main.py: from myns.utils import helper; async def handler(): helper()
     ```
     Expected: Depends on whether `helper` is blocking. Tests namespace resolution works.

   - **`tests/fixtures/a18_related_locations/`**:
     ```python
     # Fixture designed to verify related_locations in JSON output
     # Should produce STRATO002 with related locations for both blocking site and async context
     ```
     Expected: Diagnostic with non-empty `related_locations` array

   - **`tests/fixtures/a19_parse_warnings/`**:
     ```python
     # Contains one valid .py file and one with syntax errors
     # valid.py: async def handler(): time.sleep(1)
     # broken.py: def foo( <- syntax error
     ```
     Expected: 1 diagnostic from valid.py + 1 warning about broken.py

3. Update `expected.json` format:
   - Each diagnostic now has `"related_locations": [{"file": "...", "line": N, "label": "..."}]`
   - Top-level `"warnings": [{"type": "ParseError", "file": "...", "error": "..."}]` for fixtures with parse errors

4. Update integration test harness and create test modules under `crates/strato_core/tests/integration/`

5. Verify ALL 19 tests pass. Fix implementation if needed, NOT test expectations.

**Must NOT do**:
- Do not modify expected.json to match broken behavior (G8)
- Do not add extra fixtures beyond the 19 specified

**Recommended Agent Profile**:
- **Category**: `unspecified-high`
  - Reason: Many fixture files + harness wiring + potential implementation fixes
- **Skills**: `[]`

**Parallelization**:
- **Can Run In Parallel**: NO
- **Parallel Group**: Sequential
- **Blocks**: TODO 14 (M12: needs working pipeline)
- **Blocked By**: TODO 12 (M10: full pipeline must be operational)

**References**:

**Pattern References**:
- Design Doc Appendix A (lines 2840–3073): ALL 13 original test case specifications
- Design Doc Appendix B (lines 3077–3292): Test harness specification

**API/Type References**:
- `crates/strato_core::analyze()` (M9): Library entry point

**WHY Each Reference Matters**:
- Appendix A has EXACT Python snippets and expected counts
- Appendix B has `run_fixture()` code
- The 6 new v1.1 fixtures are specified inline above

**Acceptance Criteria**:

```bash
# 1. All 19 fixtures exist
for d in a01_direct_blocking a02_indirect_blocking a03_executor_safe a04_to_thread_safe a05_sync_only_safe a06_blocking_annotation a07_non_blocking_override a08_property_blocking a09_dunder_blocking a10_cross_file a11_deep_transitive a12_multiple_callers a13_mixed_safe_unsafe a14_unblocker_basic a15_executor_wrapper_config a16_star_import a17_namespace_package a18_related_locations a19_parse_warnings; do
  test -d "tests/fixtures/$d" && test -f "tests/fixtures/$d/expected.json" || echo "MISSING: $d"
done
# Assert: no MISSING output

# 2. All 19 integration tests pass
cargo test --tests
# Assert: exit code 0, 19+ tests pass

# 3. Warning fixture
cargo test test_parse_warnings
# Assert: pass — warnings present in output

# 4. Related locations fixture
cargo test test_related_locations
# Assert: pass — related_locations non-empty

# 5. Full build
cargo build && cargo test
# Assert: exit code 0
```

**Commit**: YES
- Message: `M11: integration tests — 19 acceptance test fixtures (13 original + 6 v1.1)`
- Files: All fixture directories, integration test .rs files, modified harness
- Pre-commit: `cargo build && cargo test`

---

### - [ ] 14. M12: Performance Testing + Polish + Documentation

**What to do**:

1. Create `tests/fixtures/generate_large_project.py`:
   - Deterministic script (seeded RNG) generating 500-file Python project
   - Mix of: async functions, sync functions, blocking calls, cross-file imports, executor-wrapped calls, `@unblocker` usage, star imports

2. Run generator to create `tests/fixtures/large_project/`

3. Create `crates/strato_core/tests/integration/test_performance.rs`:
   - `test_fresh_run_500_files`: delete cache, run analysis, assert < 6.5s (release mode)
   - `test_cached_run_500_files`: run twice, assert second run < 650ms (release mode)

4. Vendor SARIF schema to `tests/schemas/sarif-schema-2.1.0.json`

5. **Update `README.md`**:
   - Project description
   - Installation commands
   - Basic usage (`strato check src/`)
   - Configuration section (pyproject.toml with `[tool.strato.executor-wrappers]`)
   - Error codes STRATO001–004
   - `@blocking`, `@non_blocking`, `@unblocker` decorator documentation

6. **Exhaustive known limitations documentation** — include the following categorized tables in README or separate LIMITATIONS.md:

   #### Type System Limitations

   | Limitation | Type | Workaround | Future |
   |-----------|------|-----------|--------|
   | Dynamic dispatch (`getattr()`, `__getattr__`) | Fundamental | `@blocking` annotation | No — requires runtime info |
   | Monkey patching | Fundamental | None | No — requires runtime info |
   | `eval()`/`exec()` generated callables | Fundamental | None | No — Halting Problem adjacent |
   | Generic type parameter resolution (with ty) | Pragmatic | Depends on ty's coverage | Improves as ty matures |
   | Metaclass `__call__` | Pragmatic | `@blocking` annotation | Possible in v2 |

   #### Import System Limitations

   | Limitation | Type | Workaround | Future |
   |-----------|------|-----------|--------|
   | Dynamic imports (`importlib.import_module()`) | Fundamental | `@blocking` on the resulting callable | No |
   | Conditional imports beyond first branch | Pragmatic | Place preferred import first in `try` | Possible: analyze all branches |
   | `__import__()` builtin | Fundamental | None | No |
   | Circular import edge cases | Pragmatic | Restructure imports | Possible in v2 |

   #### Call Graph Limitations

   | Limitation | Type | Workaround | Future |
   |-----------|------|-----------|--------|
   | Callbacks passed to non-wrapper functions | Pragmatic | `@non_blocking` on the caller | Possible with deeper value flow |
   | Decorator chains (beyond `@property`/`@blocking`/etc.) | Pragmatic | `@blocking`/`@non_blocking` annotation | Possible: decorator semantic registry |
   | `@cached_property` | Pragmatic | Treated same as regular attribute access | v2: recognize as property |
   | Generator/coroutine `send()` edges | Pragmatic | Not tracked | Possible in v2 |
   | `async for` / `async with` on custom types | Pragmatic | Only async versions tracked; sync `__aiter__` etc. handled | Improve with ty |
   | Comprehension-local function calls | Supported | N/A — comprehension bodies are walked | N/A |

   #### Scope Limitations

   | Limitation | Type | Workaround | Future |
   |-----------|------|-----------|--------|
   | asyncio only (no trio/anyio) | Pragmatic | Config executor-wrappers for trio/anyio escape hatches | v2: native trio/anyio support |
   | No framework-specific knowledge (Django ORM, etc.) | Pragmatic | Config blocking-functions + executor-wrappers | v2: framework plugins |
   | No cross-process analysis | Fundamental | `@blocking` annotations | No |
   | Single-project scope (no installed package analysis) | Pragmatic | Blocking database covers common packages | v2: venv traversal |

   #### "Skip Silently" Behavior Documentation

   Every case where Strato silently skips analysis (no diagnostic, no warning):

   | Situation | Behavior | Justification |
   |----------|----------|--------------|
   | Unresolvable callee | No edge created, no diagnostic | High precision: don't guess |
   | Unknown type for attribute access | No dunder/property edge | High precision: don't guess |
   | Unknown type for method call | No edge created | High precision: don't guess |
   | `from x import *` with dynamic `__all__` | Imported names treated as unknown | Can't statically determine |
   | `getattr(obj, "method")()` | Not tracked | Dynamic attribute name |
   | Variable assigned from function return | Type depends on ty resolution | ty handles what it can |

7. Set up maturin build: verify `maturin build -m crates/strato_cli/Cargo.toml` produces `.whl`

8. Create `stubs/examples/redis.pyi` — example stub file

**Must NOT do**:
- Do not implement watch mode, autofix, or v2 features (G3)
- Do not run performance tests in debug mode (release only)

**Recommended Agent Profile**:
- **Category**: `unspecified-high`
  - Reason: Mixed tasks — scripting, profiling, documentation
- **Skills**: `[]`

**Parallelization**:
- **Can Run In Parallel**: NO
- **Parallel Group**: Sequential (final milestone)
- **Blocks**: None (final)
- **Blocked By**: TODO 13 (M11: all tests must pass)

**References**:

**Pattern References**:
- Design Doc Section 19 (lines 2753–2806): Performance targets
- Design Doc Section 21 M12 (lines 3686–3821): Complete M12 spec

**Test References**:
- Design Doc M12 (lines 3726–3754): Performance test assertions

**WHY Each Reference Matters**:
- Section 19 defines performance targets
- The complete known limitations documentation is specified inline above

**Acceptance Criteria**:

```bash
# 1. Performance tests pass (RELEASE)
cargo test --tests --release test_performance
# Assert: exit code 0

# 2. Large project exists (500+ files)
python3 -c "
import os
count = sum(1 for f in os.listdir('tests/fixtures/large_project') if f.endswith('.py'))
assert count >= 500, f'Only {count} files'
print(f'{count} Python files')
"

# 3. README updated
grep -q 'strato check' README.md && grep -q 'unblocker' README.md && grep -q 'executor-wrappers' README.md && echo "README OK"

# 4. Limitations documented
grep -q 'Known Limitations' README.md && echo "Limitations OK"

# 5. Full suite
cargo test && cargo test --tests --release
# Assert: exit code 0
```

**Commit**: YES
- Message: `M12: performance testing + polish — benchmarks, documentation, known limitations`
- Files: Generator script, large_project/, test_performance.rs, SARIF schema, README.md, stubs/
- Pre-commit: `cargo build && cargo test`

---

## Commit Strategy

| After TODO | Milestone | Message | Verification |
|------------|-----------|---------|-------------|
| 1 | M-1 | `M-1: ty integration spike — validate Salsa DB, type queries, parser compatibility` | `cargo test -p strato_core spike` |
| 2 | M0 | `M0: project scaffolding — Rust workspace with ty deps, Python package with @unblocker` | `cargo build && cargo test` |
| 3 | M1 | `M1: parser + file discovery — ruff at new rev, parse error warnings` | `cargo test -p strato_core parser discovery` |
| 4 | M2 | `M2: module resolver — star imports, namespace packages` | `cargo test -p strato_core resolver` |
| 5 | M3 | `M3: call graph — ty-backed TypeResolver, petgraph graph` | `cargo test -p strato_core graph type_resolver` |
| 6 | M4 | `M4: blocking database + annotations — @unblocker, help text policy` | `cargo test -p strato_core annotator database` |
| 7 | M5 | `M5: blocking propagation — SCC single-pass, via field` | `cargo test -p strato_core propagator` |
| 8 | M6 | `M6: wrapper registry — generalized escape hatches` | `cargo test -p strato_core wrapper_registry graph_builder::test_wrapper` |
| 9 | M7 | `M7: property + dunder — ty-enhanced, inherited dunders` | `cargo test -p strato_core graph_builder::test_property graph_builder::test_dunder` |
| 10 | M8 | `M8: diagnostics — related locations, wrapper attribution` | `cargo test -p strato_core reporter` |
| 11 | M9 | `M9: CLI + output — text/JSON/SARIF, warnings, wrapper config` | `cargo run -p strato_cli -- check tests/fixtures/smoke/ --format json` |
| 12 | M10 | `M10: caching — parse-level, ty always fresh` | `cargo test -p strato_cache` |
| 13 | M11 | `M11: integration tests — 19 fixtures` | `cargo test --tests` |
| 14 | M12 | `M12: performance + polish — benchmarks, docs, limitations` | `cargo test --tests --release` |

---

## Success Criteria

### Verification Commands

```bash
# Full pipeline test
cargo build && cargo test && cargo test --tests --release
# Expected: all pass

# Smoke test with related locations
cargo run -p strato_cli -- check tests/fixtures/smoke/ --format json | python3 -c "
import sys, json
d = json.load(sys.stdin)
assert d['diagnostics'][0]['code'] == 'STRATO001'
assert 'related_locations' in d['diagnostics'][0]
assert 'warnings' in d
print('STRATO001 with related locations detected')
"

# Python package (with @unblocker)
PYTHONPATH=python python3 -c "from strato import blocking, non_blocking, unblocker; print('annotations OK')"

# Integration tests (19 fixtures)
cargo test --tests
# Expected: 19+ tests pass

# Performance (release mode)
cargo test --tests --release test_performance
# Expected: fresh < 6.5s, cached < 650ms
```

### Final Checklist

- [ ] All "Must Have" features present
- [ ] All "Must NOT Have" guardrails respected (G1-G18)
- [ ] ty Feature Budget respected (no generic/narrowing/protocol usage)
- [ ] All 19 acceptance tests pass
- [ ] Performance targets met in release mode
- [ ] Python annotations package importable (blocking, non_blocking, unblocker)
- [ ] Help text contains no third-party library names
- [ ] Related locations present in all diagnostic output formats
- [ ] Warnings present in all output formats
- [ ] `[tool.strato.executor-wrappers]` config works
- [ ] Deterministic output (run twice → identical)
- [ ] README with installation, usage, configuration, and known limitations
