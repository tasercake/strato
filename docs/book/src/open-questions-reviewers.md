# Open Questions for Reviewers

### For Python Async Experts

*Tags: async*
**Asyncio scope limitation ([Async Library Support](./design-overview.md#async-library-support)):** We chose to support asyncio only in v1, excluding trio, curio, and anyio. The rationale is that asyncio is the stdlib framework and most widely used, and supporting multiple frameworks would require tracking each framework's distinct APIs for escape hatches. The architecture is designed for future expansion – the executor wrapper registry ([Escape Hatches](./design-overview.md#escape-hatches)) is already generalized, and adding trio/anyio patterns is straightforward.

**Question:** Is the asyncio-only scope the right call for v1? Should we attempt trio support from the start, or is the incremental approach (asyncio first, trio in v2) more pragmatic? What are the adoption barriers for teams using trio or anyio if Strato doesn't support their framework?

**Blocking database completeness ([Blocking Function Database & Annotations](./blocking-function-database-annotations.md#blocking-function-database--annotations), [Blocking Database](./design-overview.md#blocking-database)):** We curated 61 blocking functions covering I/O, synchronization, sleep/wait, subprocess, and database drivers. Fast blocking functions (e.g., `os.getpid()`) are excluded because they block for microseconds and are rarely problematic. The database is user-extensible via config and `@blocking` decorator.

**Question:** What common blocking functions are we missing? Are there domain-specific blocking patterns (e.g., scientific computing, data processing) that should be in the built-in database? Is the exclusion of fast blocking functions (microsecond-scale) the right policy, or should we flag them with a lower severity?

**Executor wrapper coverage ([Escape Hatches](./design-overview.md#escape-hatches)):** We specify a generalized registry for executor wrappers populated from: (a) built-in patterns (`run_in_executor`, `to_thread`), (b) user config (`[tool.strato.executor-wrappers]`), (c) `@unblocker` decorator. Each entry specifies which parameter receives the callable being offloaded.

**Question:** What common asyncio-compatible executor wrapper patterns are we missing? Are there third-party libraries (e.g., `asgiref.sync.sync_to_async`) that should be in the built-in registry? Does the parameter-based model (specify which argument is the callable) cover all real-world wrapper patterns, or are there wrappers that don't fit this model?

**False negative tolerance ([Precision Policy](./design-overview.md#precision-policy)):** We chose "Unknown = Unknown" (high precision) – unresolvable calls are skipped silently. Only emit diagnostics when blocking status is definitively proven. The rationale is that false positives (flagging safe code) are more damaging than false negatives (missing real bugs) in CI and expert review contexts.

**Question:** Is this precision-over-recall policy correct for async bugs? Async bugs can be subtle and hard to debug – should we be more aggressive about flagging uncertain cases, even at the cost of false positives? Would a configurable policy (strict mode vs. permissive mode) be more useful?

---

### For Static Analysis / PL Experts

*Tags: analysis*
**SCC-based propagation correctness ([Blocking Propagation decision](./design-overview.md#blocking-propagation), [Blocking Propagation](./blocking-propagation.md#blocking-propagation)):** We use Tarjan's algorithm to decompose the call graph into Strongly Connected Components (SCCs), build a condensation graph (DAG of SCCs), topologically sort, and propagate in topological order (leaves first). This guarantees O(V+E) single-pass propagation.

**Question:** Are there edge cases in cycle handling that this approach misses? For example, if an SCC contains both blocking and non-blocking nodes, we mark the entire SCC as blocking – is this sound? What about self-loops (a function calling itself) – are they handled correctly by the planned Tarjan implementation?

**Precision policy ([Precision Policy](./design-overview.md#precision-policy)):** We chose "Unknown = Unknown" – any unresolvable call is neither blocking nor non-blocking, it's skipped. The propagation algorithm explicitly skips `Unknown` nodes – they do not participate in blocking propagation. This is a permanent terminal state, never reclassified.

**Question:** Is the "Unknown = Unknown" policy too aggressive? In practice, does this lead to an unacceptably high false negative rate in codebases with heavy dynamic typing or metaprogramming? Should we have a middle ground (e.g., "Unknown = Warning" – flag uncertain cases with a lower severity)?

**Semantic resolution gaps ([Semantic Substrate](./design-overview.md#semantic-substrate)):** We use a Strato facade over vendored Ruff/ty as the semantic substrate for module, name, type, call, property, and dunder facts. We rely on facade-backed facts to resolve direct calls, method calls (`obj.method()`), property accesses (`obj.prop`), and dunder invocations (`str(obj)`).

**Question:** What common patterns defeat the vendored Ruff/ty facade's semantic resolution? Are there cases where the facade fails to resolve call targets that a human reviewer would consider obvious? How does Ruff/ty handle complex patterns like conditional assignments, exception handlers, or context managers? Should Strato add facade-backed support for common cases that are not exposed upstream?

**Call graph completeness ([Analysis Pipeline](./analysis-pipeline.md#analysis-pipeline), [Transitive Call Graph](./design-overview.md#transitive-call-graph)):** We build a project-wide call graph by extracting call edges from AST nodes (`ExprCall`, `ExprAttribute`, operators, `with` statements, `for` loops). Unresolvable calls (dynamic imports, `getattr()`, monkey patching) are skipped silently.

**Question:** What call patterns do we miss? Are there common Python idioms (e.g., decorators that modify function signatures, metaclasses, descriptor protocol) that defeat call graph construction? Should we attempt heuristic detection for common dynamic patterns (e.g., `getattr(obj, "method_name")()` where `method_name` is a string literal)?

**Phantom node model ([Phantom Nodes](./design-overview.md#phantom-nodes)):** For every entry in the blocking function database, we create a call graph node with no source location (`location: None`, `blocking_status: KnownBlocking`). When the facade resolves a call to external qualified aliases such as `"time.sleep"`, Strato matches any alias to the phantom node and creates an edge.

**Question:** Is the phantom node model sound? Are there cases where a phantom node could be confused with a user-defined function of the same name (e.g., a project defines its own `time.sleep`)? Should phantom nodes have a distinct type or marker to prevent this? How do we handle overloaded functions (e.g., `open()` is both a builtin and a method on file objects)?

---

### For Rust / Tooling Experts

*Tags: tooling*
**Vendored Ruff/ty integration risk ([Semantic Substrate](./design-overview.md#semantic-substrate)):** We vendor Astral's Ruff monorepo and depend on patched Ruff/ty crates for semantic facts. This introduces Salsa (a query-based incremental computation framework), a pinned vendored revision, and a Strato-maintained patch set for facade APIs not exposed upstream. We mitigate API instability by isolating direct Ruff/ty usage in `strato_ty_adapter`, keeping a patch ledger, running facade conformance tests, isolating recoverable panics at the facade boundary where unwinding permits it, and using Ruff parsed modules from the same ty project database rather than maintaining a separate parse.

**Question:** Is the vendored Ruff/ty strategy acceptable for a v1 release? Is the maintenance burden of replaying Strato's patches on new Ruff revisions manageable? Which facade APIs should be proposed upstream to reduce the long-term fork delta?

**Performance targets ([Performance Targets](./supporting-systems.md#performance-targets)):** We target <5s fresh analysis and <500ms cached on 500 files. The measurement protocol uses `hyperfine` with 3 warmup runs and 5 timed runs (report median). CI tests use a +/-30% tolerance band.

**Question:** Are these targets achievable given the architecture (Ruff parsed modules + Strato syntax extraction + facade semantic queries + SCC propagation)? What are the likely bottlenecks – Ruff/ty project setup, parsing, facade queries, graph construction, or propagation? Should we have separate targets for different project sizes (e.g., <1s for 100 files, <10s for 1000 files)? Is the +/-30% CI tolerance too loose?

**Caching strategy ([Caching Strategy](./design-overview.md#caching-strategy)):** We cache Strato-owned per-file extraction artifacts keyed by file content hash. Vendored Ruff parsed modules, ty's Salsa database, semantic facade facts, call edges, call graph construction, propagation, and diagnostics re-run every time. Salsa state is in-memory only and not serialized by Strato, so Ruff/ty results are not cached cross-run.

**Question:** Is per-file caching sufficient, or will the lack of cross-run Ruff/ty caching be a performance bottleneck? Should we explore a broader Strato-owned cache only after measuring the facade cost? Are there other caching strategies (e.g., caching selected semantic facts or call edges) that would be more effective without violating determinism or invalidation safety?

**maturin distribution ([Distribution & Packaging](./supporting-systems.md#distribution--packaging)):** We use maturin to build a PyPI wheel for `strato-cli` (the Rust binary). The wheel is platform-specific (separate builds for Linux, macOS, Windows).

**Question:** What is the platform coverage we should target? Should we support ARM (Apple Silicon, ARM Linux) from v1, or is x86_64 sufficient? What is the CI burden of building wheels for multiple platforms? Are there distribution challenges (e.g., glibc version compatibility on Linux) we should anticipate?

**Determinism contract ([Determinism Contract](./design-overview.md#determinism-contract)):** We enforce determinism at multiple levels: (1) `BTreeMap` for output-affecting collections, (2) diagnostics sorted by file path -> line -> column -> error code, (3) blocking path selection uses shortest-path with lexicographic tie-breaking, (4) cache keys use SHA-256 content hashes.

**Question:** Is `BTreeMap` sufficient to guarantee determinism, or are there other sources of non-determinism (e.g., rayon's parallel iteration order, filesystem traversal order, Ruff/ty's internal query order)? Should we have a determinism regression test that runs the same fixture multiple times and asserts identical output? What is the performance cost of determinism – is the O(log n) overhead of `BTreeMap` negligible, or does it add up at scale?

---

### For Everyone

**Overall scope ([Transitive Call Graph](./design-overview.md#transitive-call-graph), [Known Limitations & Scope Boundaries](./known-limitations-scope-boundaries.md#known-limitations--scope-boundaries)):** Strato v1 aims to detect blocking calls in asyncio code through transitive call graph analysis. Committed v1 scope includes the vendored Ruff/ty facade, executor wrapper registry, annotations, property/dunder detection, and deterministic output. Known limitations include unresolved dynamic semantic patterns, no dynamic dispatch, asyncio-only, first-party focus, and no cross-package analysis.

**Question:** Are the committed v1 boundaries correct for production usefulness, or should future work prioritize expansion after v1 (e.g., trio support, autofix suggestions, broader third-party stubs)? Which v1 risks need the strongest validation before implementation starts?

**Error reporting UX ([Error Reporting](./design-overview.md#error-reporting)):** We default to `first-party-deepest` intervention strategy – point the diagnostic to the deepest first-party call site in the blocking call chain. The rationale is that this is the most actionable expression (user can refactor or offload this call). The full chain is always included in diagnostics for context.

**Question:** Given `first-party-deepest` is the v1 default, are the fallback and secondary-location rules sufficient? Should `async-boundary` remain only an explicit opt-in strategy, and should diagnostics always include the async boundary as a related location for context? How do we handle cases where the entire chain is third-party code (e.g., `async def handler(): requests.get(...)`) – should we fall back to the async boundary?

**Adoption barriers:** Strato requires: (1) installing `strato-cli` (Rust binary via PyPI), (2) optionally installing `strato` (Python annotations package), (3) running `strato check src/` in CI, (4) configuring `pyproject.toml` for custom blocking functions or executor wrappers.

**Question:** What would prevent you from using this tool? Is the Rust binary a barrier (e.g., platform compatibility, binary size, security concerns)? Is the configuration burden too high? Would you trust a pre-1.0 tool in CI, or would you wait for 1.0? What documentation or examples would you need to adopt Strato?

**Tags**: everyone
