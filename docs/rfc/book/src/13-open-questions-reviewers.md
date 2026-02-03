# 13. Open Questions for Reviewers

### For Python Async Experts

*Tags: async*
**Asyncio scope limitation ([Decision 3.16](./03-design-decisions.md#316-async-scope-boundary-asyncio-only)):** We chose to support asyncio only in v1, excluding trio, curio, and anyio. The rationale is that asyncio is the stdlib framework and most widely used, and supporting multiple frameworks would require tracking each framework's distinct APIs for escape hatches. The architecture is designed for future expansion — the executor wrapper registry ([Decision 3.6](./03-design-decisions.md#36-generalized-executor-wrapper-system)) is already generalized, and adding trio/anyio patterns is straightforward.

**Question:** Is the asyncio-only scope the right call for v1? Should we attempt trio support from the start, or is the incremental approach (asyncio first, trio in v2) more pragmatic? What are the adoption barriers for teams using trio or anyio if Strato doesn't support their framework?

**Blocking database completeness ([Section 8](./08-blocking-function-database-annotations.md#8-blocking-function-database--annotations), [Decision 3.8](./03-design-decisions.md#38-blocking-database-curated-list-vs-exhaustive)):** We curated ~80 blocking functions covering I/O, synchronization, sleep/wait, subprocess, and database drivers. Fast blocking functions (e.g., `os.getpid()`) are excluded because they block for microseconds and are rarely problematic. The database is user-extensible via config and `@blocking` decorator.

**Question:** What common blocking functions are we missing? Are there domain-specific blocking patterns (e.g., scientific computing, data processing) that should be in the built-in database? Is the exclusion of fast blocking functions (microsecond-scale) the right policy, or should we flag them with a lower severity?

**Executor wrapper coverage ([Decision 3.6](./03-design-decisions.md#36-generalized-executor-wrapper-system)):** We implemented a generalized registry for executor wrappers populated from: (a) built-in patterns (`run_in_executor`, `to_thread`), (b) user config (`[tool.strato.executor-wrappers]`), (c) `@unblocker` decorator. Each entry specifies which parameter receives the callable being offloaded.

**Question:** What common executor wrapper patterns are we missing? Are there third-party libraries (e.g., `asgiref.sync.sync_to_async`, `anyio.to_thread.run_sync`) that should be in the built-in registry? Does the parameter-based model (specify which argument is the callable) cover all real-world wrapper patterns, or are there wrappers that don't fit this model?

**False negative tolerance ([Decision 3.2](./03-design-decisions.md#32-precision-policy-unknown--not-blocking)):** We chose "Unknown = Unknown" (high precision) — unresolvable calls are skipped silently. Only emit diagnostics when blocking status is definitively proven. The rationale is that false positives (flagging safe code) are more damaging than false negatives (missing real bugs) in CI and expert review contexts.

**Question:** Is this precision-over-recall policy correct for async bugs? Async bugs can be subtle and hard to debug — should we be more aggressive about flagging uncertain cases, even at the cost of false positives? Would a configurable policy (strict mode vs. permissive mode) be more useful?

---

### For Static Analysis / PL Experts

*Tags: analysis*
**SCC-based propagation correctness ([Decision 3.3](./03-design-decisions.md#33-scc-based-propagation-vs-iterative-fixpoint), [Section 7](./07-blocking-propagation.md#7-blocking-propagation)):** We use Tarjan's algorithm to decompose the call graph into Strongly Connected Components (SCCs), build a condensation graph (DAG of SCCs), topologically sort, and propagate in topological order (leaves first). This guarantees O(V+E) single-pass propagation.

**Question:** Are there edge cases in cycle handling that this approach misses? For example, if an SCC contains both blocking and non-blocking nodes, we mark the entire SCC as blocking — is this sound? What about self-loops (a function calling itself) — are they handled correctly by Tarjan's algorithm as implemented?

**Precision policy ([Decision 3.2](./03-design-decisions.md#32-precision-policy-unknown--not-blocking)):** We chose "Unknown = Unknown" — any unresolvable call is neither blocking nor non-blocking, it's skipped. The propagation algorithm explicitly skips `Unknown` nodes — they do not participate in blocking propagation. This is a permanent terminal state, never reclassified.

**Question:** Is the "Unknown = Unknown" policy too aggressive? In practice, does this lead to an unacceptably high false negative rate in codebases with heavy dynamic typing or metaprogramming? Should we have a middle ground (e.g., "Unknown = Warning" — flag uncertain cases with a lower severity)?

**Type inference gaps ([Decision 3.4](./03-design-decisions.md#34-type-inference-strategy-ty-integration-vs-hand-rolled)):** We integrated Astral's `ty` crate for type inference, which provides alias tracking, return type inference, MRO, and attribute resolution. We rely on ty to resolve method calls (`obj.method()`), property accesses (`obj.prop`), and dunder invocations (`str(obj)`).

**Question:** What common patterns defeat ty's type inference? Are there cases where ty fails to resolve types that a human reviewer would consider obvious? How does ty handle complex patterns like conditional assignments, exception handlers, or context managers? Should we have a fallback heuristic for common cases where ty fails?

**Call graph completeness ([Section 5](./05-analysis-pipeline.md#5-analysis-pipeline), [Decision 3.1](./03-design-decisions.md#31-transitive-call-graph-vs-pattern-matching)):** We build a project-wide call graph by extracting call edges from AST nodes (`ExprCall`, `ExprAttribute`, operators, `with` statements, `for` loops). Unresolvable calls (dynamic imports, `getattr()`, monkey patching) are skipped silently.

**Question:** What call patterns do we miss? Are there common Python idioms (e.g., decorators that modify function signatures, metaclasses, descriptor protocol) that defeat call graph construction? Should we attempt heuristic detection for common dynamic patterns (e.g., `getattr(obj, "method_name")()` where `method_name` is a string literal)?

**Phantom node model ([Decision 3.5](./03-design-decisions.md#35-phantom-nodes-for-external-symbols)):** For every entry in the blocking function database, we create a call graph node with no source location (`location: None`, `blocking_status: KnownBlocking`). When the call graph builder encounters `time.sleep(1)`, it constructs the qualified name `"time.sleep"`, finds the phantom node, and creates an edge.

**Question:** Is the phantom node model sound? Are there cases where a phantom node could be confused with a user-defined function of the same name (e.g., a project defines its own `time.sleep`)? Should phantom nodes have a distinct type or marker to prevent this? How do we handle overloaded functions (e.g., `open()` is both a builtin and a method on file objects)?

---

### For Rust / Tooling Experts

*Tags: tooling*
**ty integration risk ([Decision 3.4](./03-design-decisions.md#34-type-inference-strategy-ty-integration-vs-hand-rolled)):** We depend on Astral's `ty_python_semantic` crate (pre-1.0) for type inference. This introduces Salsa (a query-based incremental computation framework) and requires pinning to a specific ruff rev. We mitigate API instability by: (1) pinning to a specific rev, (2) panic isolation (catch panics, downgrade to `NullTypeResolver` per-file), (3) accepting the double parse cost (ruff AST for Strato + ty's internal parse).

**Question:** Is the ty integration risk acceptable for a v1 release? Should we wait for ty to reach 1.0, or is the pinned-rev strategy sufficient? What is the maintenance burden of upgrading to new ruff revs — is this a one-time spike or an ongoing tax? Are there alternative type inference libraries (e.g., pyright's type checker, mypy's internals) that would be more stable?

**Performance targets ([Section 11.4](./11-supporting-systems.md#114-performance-targets)):** We target <5s fresh analysis and <500ms cached on 500 files. The measurement protocol uses `hyperfine` with 3 warmup runs and 5 timed runs (report median). CI tests use a +/-30% tolerance band.

**Question:** Are these targets achievable given the architecture (ruff parsing + ty type inference + SCC propagation)? What are the likely bottlenecks — parsing, type inference, graph construction, or propagation? Should we have separate targets for different project sizes (e.g., <1s for 100 files, <10s for 1000 files)? Is the +/-30% CI tolerance too loose?

**Caching strategy ([Decision 3.13](./03-design-decisions.md#313-caching-strategy-and-ty-boundary)):** We cache per-file parse results and imports (Phases 1-3) keyed by file content hash. Call graph construction and propagation (Phases 4-7) re-run every time. ty's Salsa database is in-memory only, not serializable, so ty results are not cached cross-run.

**Question:** Is per-file caching sufficient, or will the lack of cross-run ty caching be a performance bottleneck? Should we explore serializing ty's results (e.g., by extracting only the type information we need and caching that)? Are there other caching strategies (e.g., caching the call graph itself) that would be more effective?

**maturin distribution ([Section 11.5](./11-supporting-systems.md#115-distribution--packaging)):** We use maturin to build a PyPI wheel for `strato-cli` (the Rust binary). The wheel is platform-specific (separate builds for Linux, macOS, Windows).

**Question:** What is the platform coverage we should target? Should we support ARM (Apple Silicon, ARM Linux) from v1, or is x86_64 sufficient? What is the CI burden of building wheels for multiple platforms? Are there distribution challenges (e.g., glibc version compatibility on Linux) we should anticipate?

**Determinism contract ([Decision 3.14](./03-design-decisions.md#314-determinism-contract)):** We enforce determinism at multiple levels: (1) `BTreeMap` for output-affecting collections, (2) diagnostics sorted by file path -> line -> column -> error code, (3) blocking path selection uses shortest-path with lexicographic tie-breaking, (4) cache keys use SHA-256 content hashes.

**Question:** Is `BTreeMap` sufficient to guarantee determinism, or are there other sources of non-determinism (e.g., rayon's parallel iteration order, filesystem traversal order, ty's internal query order)? Should we have a determinism regression test that runs the same fixture multiple times and asserts identical output? What is the performance cost of determinism — is the O(log n) overhead of `BTreeMap` negligible, or does it add up at scale?

---

### For Everyone

**Overall scope ([Decision 3.1](./03-design-decisions.md#31-transitive-call-graph-vs-pattern-matching), [Section 12](./12-known-limitations-scope-boundaries.md#12-known-limitations--scope-boundaries)):** Strato v1 aims to detect blocking calls in asyncio code through transitive call graph analysis. Known limitations include: no type inference for complex patterns, no dynamic dispatch, asyncio-only, first-party focus, no cross-package analysis.

**Question:** Is the v1 scope too ambitious, or not ambitious enough? Should we cut features to ship faster (e.g., drop ty integration, drop executor wrapper detection), or should we expand scope (e.g., add trio support, add autofix suggestions)? What is the minimum viable feature set that would make Strato useful in production?

**Error reporting UX ([Decision 3.7](./03-design-decisions.md#37-intervention-strategy-for-error-reporting)):** We default to `first-party-deepest` intervention strategy — point the diagnostic to the deepest first-party function in the blocking call chain. The rationale is that this is the most actionable location (user can fix this function). The full chain is always included in diagnostics for context.

**Question:** Is `first-party-deepest` the right default? Would `async-boundary` (always point to the async function) be more intuitive for users? Should we provide both locations (primary + secondary) in the diagnostic? How do we handle cases where the entire chain is third-party code (e.g., `async def handler(): requests.get(...)`) — should we fall back to the async boundary?

**Adoption barriers:** Strato requires: (1) installing `strato-cli` (Rust binary via PyPI), (2) optionally installing `strato` (Python annotations package), (3) running `strato check src/` in CI, (4) configuring `pyproject.toml` for custom blocking functions or executor wrappers.

**Question:** What would prevent you from using this tool? Is the Rust binary a barrier (e.g., platform compatibility, binary size, security concerns)? Is the configuration burden too high? Would you trust a pre-1.0 tool in CI, or would you wait for 1.0? What documentation or examples would you need to adopt Strato?

**Tags**: everyone
