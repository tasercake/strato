# 3. Design Decisions

This section presents the core architectural and implementation choices that define Strato's approach to detecting blocking calls in async Python code. Each decision is structured as a tradeoff analysis: the context that forced a choice, the options considered, the selection made, the rationale for that selection, and the risks that remain. These decisions are presented for expert review — scrutiny from practitioners in Python async, static analysis/PL, and Rust/tooling domains.

### 3.1 Transitive Call Graph vs Pattern Matching

*Tags: async, analysis*
**Context:** Existing async linters (flake8-async, ruff ASYNC2XX) use pattern matching to detect direct blocking calls inside async functions — they scan for `time.sleep()`, `requests.get()`, etc. within `async def` bodies. This catches obvious cases but fails when blocking code hides behind intermediate function calls. The motivating example is `async def handler(): helper()` where `helper()` internally calls `time.sleep()` — no existing tool detects this because the blocking call is not syntactically visible at the async boundary. The question: should Strato use the same pattern-matching approach (fast, simple, proven) or build a full call graph to trace blocking through function call chains (complex, novel, higher ambition)?

**Options considered:**

1. **Pattern matching (like existing tools)** — Scan async function bodies for direct calls to known blocking functions. Pros: Simple to implement, fast, well-understood failure modes. Cons: Misses transitive blocking (the core problem Strato aims to solve), provides no value over existing tools.

2. **Full transitive call graph** — Build a project-wide directed graph of function calls, propagate "blocking" status through edges, report when async functions can reach blocking nodes. Pros: Catches hidden blocking through arbitrarily deep call chains, provides unique value. Cons: Complex implementation (module resolution, type inference, SCC decomposition), higher false negative rate (unresolvable calls are skipped), performance risk (graph construction + propagation on large codebases).

3. **Hybrid: pattern matching + one-level call depth** — Check direct calls in async functions, plus check the immediate callees of those functions (one level of indirection). Pros: Catches the most common case (async → sync helper → blocking) without full graph complexity. Cons: Arbitrary depth limit (why stop at one level?), still misses deeper chains, implementation complexity approaches full graph anyway (need symbol resolution).

**Choice:** Full transitive call graph (Option 2).

**Rationale:** The entire value proposition of Strato is catching blocking calls that existing tools miss. Pattern matching (Option 1) provides zero incremental value — users already have flake8-async and ruff. The hybrid approach (Option 3) is a half-measure that still requires most of the infrastructure of a full graph (module resolution, symbol tables, call edge extraction) but arbitrarily limits the analysis depth. The full graph approach is the only option that delivers on the promise: if a blocking call is reachable from an async context through any chain of function calls, Strato finds it. The complexity cost is justified by the unique capability. The design mitigates performance risk through SCC-based propagation (O(V+E), not iterative fixpoint) and incremental caching. The false negative risk (unresolvable calls are skipped) is addressed by the precision policy (Decision 3.2) — better to miss some cases than flood users with false positives.

**Risk:** The call graph approach is unproven in the Python async linting domain. If real-world codebases have too many unresolvable calls (dynamic imports, heavy metaprogramming, complex type flows), the false negative rate could be so high that the tool provides little practical value. The acceptance test suite (Appendix B) is designed to validate coverage on realistic patterns, but production validation will be critical. If the approach fails, there is no fallback — the entire architecture is predicated on the call graph.

---

### 3.2 Precision Policy: Unknown ≠ Not Blocking

*Tags: analysis*
**Context:** When Strato encounters a call it cannot resolve (e.g., `obj.method()` where `obj`'s type is unknown, or a dynamic import), it must decide: treat the call as potentially blocking (emit a diagnostic) or treat it as unknown (skip silently). This is the classic precision vs. recall tradeoff in static analysis. High recall (flag everything uncertain) maximizes detection but floods users with false positives. High precision (only flag proven cases) minimizes false positives but misses real bugs.

**Options considered:**

1. **Unknown = Blocking (high recall)** — Any unresolvable call is assumed blocking. Emit diagnostics for all uncertain cases. Pros: Catches more real bugs, forces users to annotate or refactor unclear code. Cons: High false positive rate, noisy output, users will ignore or disable the tool.

2. **Unknown = Not Blocking (optimistic)** — Any unresolvable call is assumed safe. Only emit diagnostics for proven blocking calls. Pros: Clean output, no false positives. Cons: Misses real bugs when resolution fails, users may have false confidence.

3. **Unknown = Unknown (high precision)** — Unresolvable calls are neither blocking nor non-blocking — they are skipped. Only emit diagnostics when blocking status is definitively proven. Pros: Zero false positives, users trust the tool's output. Cons: False negatives when resolution fails, tool may miss bugs in complex codebases.

**Choice:** Unknown = Unknown (Option 3).

**Rationale:** Strato is designed for expert review and CI integration. In these contexts, false positives are more damaging than false negatives. A false positive (flagging safe code as blocking) wastes developer time, erodes trust, and leads to tool abandonment. A false negative (missing a real blocking call) is unfortunate but does not actively harm — the bug may be caught by other means (testing, profiling, manual review). The design prioritizes trust: when Strato reports an error, it is confident the error is real. This is reflected in the `BlockingStatus` enum: `Unknown` is a permanent terminal state, never reclassified to `NotBlocking` or `Blocking`. The propagation algorithm (Section 7) explicitly skips `Unknown` nodes — they do not participate in blocking propagation. This policy is consistent with the call graph approach (Decision 3.1): if we can't prove a call is blocking, we don't report it.

**Risk:** The false negative rate could be unacceptably high in codebases with heavy use of dynamic typing, metaprogramming, or third-party libraries without type stubs. If Strato misses too many real bugs, users will perceive it as incomplete or unreliable. The mitigation is twofold: (1) ty integration (Decision 3.4) improves type resolution, reducing the `Unknown` rate; (2) user annotations (`@blocking`, `@non_blocking`) allow manual override when Strato's analysis is insufficient.

---

### 3.3 SCC-Based Propagation vs Iterative Fixpoint

*Tags: analysis*
**Context:** After the call graph is constructed and initial blocking annotations are applied, the propagation phase must spread "blocking" status through the graph. If function A calls function B, and B is blocking, then A is also blocking (unless the call is wrapped in an executor). The challenge: call graphs contain cycles (mutual recursion). Naive iterative propagation (repeatedly scan the graph until no changes occur) works but is inefficient — it may require multiple passes over the same nodes, and the number of iterations is unbounded in the presence of complex cycles.

**Options considered:**

1. **Iterative fixpoint** — Repeatedly scan all nodes, propagating blocking status from callees to callers, until no node's status changes. Pros: Simple to implement, easy to understand. Cons: O(V × E) worst case (V iterations, each scanning E edges), slow on large graphs with deep cycles, non-deterministic iteration order complicates testing.

2. **SCC-based propagation (Tarjan's algorithm)** — Decompose the call graph into Strongly Connected Components (SCCs), build a condensation graph (DAG of SCCs), topologically sort the condensation, propagate in topological order (leaves first). Pros: O(V + E) single-pass algorithm, deterministic, handles cycles elegantly (entire SCC is treated as a unit). Cons: More complex implementation (Tarjan's algorithm, condensation graph construction), harder to debug.

3. **Worklist algorithm** — Maintain a worklist of nodes whose blocking status has changed. When a node's status changes, add its callers to the worklist. Repeat until worklist is empty. Pros: More efficient than naive iteration (only revisits affected nodes), easier to implement than SCC decomposition. Cons: Still requires multiple passes in the presence of cycles, worst-case complexity is O(V × E), non-deterministic worklist ordering.

**Choice:** SCC-based propagation (Option 2).

**Rationale:** The SCC approach is the only option that guarantees O(V + E) complexity — a single pass over the graph, regardless of cycle structure. This is critical for performance on large codebases (the 500-file benchmark targets sub-5-second fresh analysis). The iterative fixpoint (Option 1) and worklist (Option 3) approaches both degrade to O(V × E) in the presence of deep cycles, which are common in real-world code (e.g., mutually recursive validation functions, circular imports). The implementation complexity of Tarjan's algorithm is justified by the performance guarantee. The deterministic topological ordering also simplifies testing — the propagation order is reproducible, making it easier to write unit tests and debug failures.

**Risk:** The SCC decomposition adds a dependency on a correct implementation of Tarjan's algorithm. If the implementation has bugs (e.g., incorrect handling of self-loops, off-by-one errors in the DFS stack), the propagation results will be wrong, and the bugs will be hard to diagnose. The mitigation is thorough unit testing of the SCC decomposition in isolation and integration tests that validate end-to-end propagation on known-good fixtures.

---

### 3.4 Type Inference Strategy: ty Integration vs Hand-Rolled

*Tags: analysis, tooling*
**Context:** To resolve method calls (`obj.method()`), property accesses (`obj.prop`), and dunder invocations (`str(obj)`), Strato needs to infer the type of `obj`. The v1.0 design used a hand-rolled `ScopeBindings` system that tracked simple cases: `self`/`cls` in methods, constructor calls (`x = MyClass()`), and direct imports. This was sufficient for basic call graph construction but missed common patterns like alias tracking (`x = requests.get; x()`) and return type inference (`loader = get_loader(); loader.load()`). Astral's `ty` crate provides full type inference for Python, including these cases, but integrating it requires adopting Salsa (a query-based incremental computation framework) and accepting the complexity of a pre-1.0 external dependency.

**Options considered:**

1. **Hand-rolled ScopeBindings (v1.0 baseline)** — Implement a minimal type inference system that tracks local variable bindings within function scopes. Resolve `self`, `cls`, constructors, and imports. Skip everything else. Pros: Full control, no external dependencies, simple implementation. Cons: Misses common patterns (alias tracking is critical for executor wrapper detection), limited by what we're willing to implement, reinventing the wheel.

2. **ty integration (v1.1)** — Use Astral's `ty_python_semantic` crate for type inference. Wrap it in a `trait TypeResolver` abstraction to isolate Strato from ty's API. Pros: Full type inference including aliases, return types, MRO, attribute resolution; leverages Astral's investment in Python type system; Salsa provides in-run memoization. Cons: Pre-1.0 dependency (API instability, potential panics), Salsa adds complexity, double parse (ruff AST for Strato + ty's internal parse), ty results are not cacheable cross-run (Salsa is in-memory only).

3. **Hybrid: ScopeBindings + ty fallback** — Use ScopeBindings for simple cases, query ty for complex cases. Pros: Graceful degradation if ty fails. Cons: Two type inference systems to maintain, unclear boundary between "simple" and "complex", added complexity for minimal benefit.

**Choice:** ty integration (Option 2), with no ScopeBindings fallback.

**Rationale:** The key capabilities ty provides — alias tracking and return type inference — are critical for Strato's core use cases. Alias tracking is essential for executor wrapper detection: the pattern `safe = sync_to_async(func); await safe()` requires resolving `safe` back to a callable, which ScopeBindings cannot do. Return type inference enables resolving indirect calls like `get_loader().load()`. The risks (API instability, panics, double parse) are mitigated by: (1) pinning to a specific ruff rev, (2) panic isolation (catch panics, downgrade to `NullTypeResolver` per-file), (3) accepting the double parse cost (<100ms for 500 files). The caching limitation (ty results not cached cross-run) is addressed in Decision 3.13.

**Risk:** ty is pre-1.0 and may have bugs, panics, or API changes. If ty fails on a file, Strato degrades gracefully (emit a warning, skip type-dependent analysis for that file). The pinned rev strategy means Strato is frozen at a specific ruff version — upgrading requires a dedicated compatibility spike.

---

### 3.5 Phantom Nodes for External Symbols

*Tags: analysis*
**Context:** Strato's call graph includes nodes for user-defined functions (parsed from source) and nodes for external blocking functions (stdlib, third-party libraries). How do external symbols like `time.sleep`, `requests.get` become resolvable call graph nodes when their source files are not in the project's source roots?

**Options considered:**

1. **Parse external libraries** — Include stdlib and third-party packages in the source roots. Pros: Uniform treatment. Cons: Massive performance cost (parsing thousands of files), many libraries are C extensions (no Python source), version skew.

2. **Stub files (.pyi)** — Provide hand-written `.pyi` stubs for known blocking functions. Pros: Lightweight. Cons: Must be maintained separately, still requires parsing.

3. **Phantom nodes (pre-seeded from database)** — For every entry in the blocking function database, create a call graph node with no source location. Pros: Zero parsing cost, no version skew, database is the single source of truth. Cons: Only works for functions in the database.

**Choice:** Phantom nodes (Option 3).

**Rationale:** The phantom node approach is the simplest and most performant. It aligns with Strato's precision policy (Decision 3.2): only known blocking functions are tracked. External calls not in the database are treated as `Unknown` and skipped. During Phase 4 initialization, iterate over the blocking database and create a `CallGraphNode` for each entry with `location: None` and `blocking_status: KnownBlocking`. When the call graph builder encounters `time.sleep(1)`, the symbol resolution constructs the qualified name `"time.sleep"`, finds the phantom node, and creates an edge. The phantom node participates in propagation like any other node.

**Risk:** Tightly coupled to the blocking database. If the database is incomplete, calls to unlisted blocking functions will be unresolvable and skipped. The mitigation is a comprehensive database (~80+ entries) and user extensibility (config allows adding custom entries, `@blocking` decorator allows per-function annotation).

---

### 3.6 Generalized Executor Wrapper System

*Tags: async, analysis*
**Context:** Python's asyncio provides `loop.run_in_executor()` and `asyncio.to_thread()` to offload blocking work to a thread pool. But real-world codebases use custom wrappers (e.g., `asgiref.sync.sync_to_async`, `anyio.to_thread.run_sync`) and project-specific helpers. Hardcoding every possible wrapper is unmaintainable.

**Options considered:**

1. **Hardcoded list (v1.0 baseline)** — Recognize `run_in_executor` and `to_thread` by name. Pros: Simple. Cons: Not extensible, misses third-party wrappers.

2. **Heuristic detection** — Analyze function bodies to detect patterns like "creates a thread". Pros: Automatic. Cons: Unreliable, doesn't work for C extensions.

3. **Generalized registry (built-in + config + decorator)** — Maintain a registry of known executor wrappers populated from: (a) built-in patterns, (b) user config (`[tool.strato.executor-wrappers]`), (c) `@unblocker` decorator. Each entry specifies which parameter receives the callable being offloaded. Pros: Extensible, covers common cases, user-controllable. Cons: Requires configuration for third-party wrappers.

**Choice:** Generalized registry (Option 3).

**Rationale:** The registry approach balances coverage and extensibility. Built-in patterns cover the most common cases with zero configuration. Config allows adding third-party wrappers without modifying Strato's code. The `@unblocker` decorator allows annotating project-specific wrappers. The call graph builder checks the registry when visiting call expressions; if the callee matches, the edge to the callable argument is marked `in_executor: true`, suppressing blocking propagation.

**Risk:** Users must configure third-party wrappers not in the built-in list. If unconfigured, Strato will flag safe code as blocking (false positive). The registry also depends on ty's ability to resolve the callable argument — if ty can't resolve `safe = sync_to_async(func); await safe()`, the protection is lost.

---

### 3.7 Intervention Strategy for Error Reporting

*Tags: async, tooling*
**Context:** When Strato detects a blocking call chain like `async handler() → helper() → db_query() → psycopg2.connect()`, where should it point the diagnostic? The blocking call is in `psycopg2.connect()` (third-party), but the user can't fix that.

**Options considered:**

1. **Async boundary** — Always point to the async function. Pros: Clear context. Cons: May be far from the fix point, less actionable.

2. **First-party deepest** — Point to the deepest first-party function in the chain. Pros: Most actionable (user can fix this function). Cons: May be in a utility far from the async context.

3. **Configurable (default: first-party deepest)** — Allow users to choose via config. Pros: Flexibility. Cons: More complexity.

**Choice:** Configurable, default `first-party-deepest` (Option 3).

**Rationale:** Different teams have different workflows. The default `first-party-deepest` is more actionable — pointing to `helper()` tells the user "fix it here" rather than "figure out where". The full chain is always included in diagnostics for context.

**Risk:** `first-party-deepest` may be confusing if the deepest first-party function is a low-level utility far from the async context. The `async-boundary` strategy is available as a fallback.

---

### 3.8 Blocking Database: Curated List vs Exhaustive

*Tags: async*
**Context:** Strato needs a database of known blocking functions to seed phantom nodes. Should it be exhaustive (every blocking function in stdlib and popular libraries) or curated?

**Options considered:**

1. **Exhaustive** — Every blocking function. Pros: Maximum coverage. Cons: Massive maintenance burden, high risk of false positives (some functions are blocking but fast, e.g., `os.getpid()`).

2. **Curated (~80 entries)** — Focus on common, impactful blocking functions: I/O, synchronization, sleep/wait, subprocess. Pros: Manageable size, low false positive rate, user-extensible. Cons: Misses less common blocking functions.

3. **Minimal (~20 entries)** — Only the most egregious offenders. Pros: Very low false positive rate. Cons: Incomplete, misses many real bugs.

**Choice:** Curated (~80 entries) (Option 2).

**Rationale:** The curated list covers the most common blocking patterns (`time.sleep`, `requests.*`, `urllib.*`, `socket.*`, `subprocess.*`, `os.read`, `open()`, database drivers). Fast blocking functions (e.g., `os.getpid()`) are excluded — they block for microseconds and are rarely problematic. The database is user-extensible via config and `@blocking` decorator.

**Risk:** May miss blocking functions common in specific domains (e.g., scientific computing). Users must extend via config.

---

### 3.9 Help Text Policy: No Third-Party Recommendations

*Tags: async, tooling*
**Context:** Diagnostics include help text suggesting how to fix the issue. Should help text recommend specific third-party libraries?

**Options considered:**

1. **Specific recommendations** — "use `httpx` instead of `requests`". Pros: Actionable. Cons: Strato becomes a kingmaker, recommendations may become outdated.

2. **Generic recommendations** — "use an async HTTP library" or "offload to `asyncio.to_thread()`". Pros: Neutral, timeless. Cons: Less actionable.

3. **No help text** — Only report the problem. Pros: Minimal. Cons: Unhelpful.

**Choice:** Generic recommendations (Option 2).

**Rationale:** Strato is a linting tool, not a library recommendation engine. Help text lists multiple alternatives neutrally (e.g., "Use `aiohttp` or `httpx`") without prescribing one. This avoids implicit endorsement and keeps help text maintainable.

**Risk:** Generic text may be too vague for novice users. Mitigation: include multiple examples without recommending one.

---

### 3.10 Language Choice: Rust

*Tags: tooling*
**Context:** Strato is a static analysis tool that must parse Python code, build a call graph, and propagate blocking status.

**Options considered:**

1. **Python** — Using `ast` module. Pros: Familiar to target audience. Cons: Performance (Python is slow for graph algorithms), packaging complexity.

2. **Rust** — Using ruff's parser crates. Pros: Performance, ruff parser is the fastest Python parser, strong type safety, single-binary distribution. Cons: Steeper learning curve, smaller contributor pool.

3. **Go** — Pros: Fast, single-binary. Cons: No existing Python parser ecosystem.

**Choice:** Rust (Option 2).

**Rationale:** Performance is critical for CI. The 500-file benchmark targets sub-5-second fresh analysis and sub-500ms cached. Python cannot achieve this for graph algorithms at scale. Rust gives access to ruff's parser crates (fastest Python parser available) and the single-binary distribution model via maturin.

**Risk:** Steeper learning curve limits contributors. Mitigated by clear architecture documentation and modular codebase.

---

### 3.11 Distribution: Dual PyPI Packages

*Tags: tooling*
**Context:** Strato consists of a Rust binary (analysis tool) and a Python package (`@blocking`/`@non_blocking`/`@unblocker` decorators).

**Options considered:**

1. **Single package** — Binary + annotations together. Pros: Simple. Cons: Large package (~10MB), users who only want annotations must install the binary.

2. **Dual packages** — `strato` (pure Python, annotations only, zero deps) and `strato-cli` (Rust binary via maturin). Pros: Lightweight annotations package, independent versioning. Cons: Two packages to maintain.

3. **Binary-only** — No annotations package. Pros: Simplest. Cons: Poor UX, no type checking for decorators.

**Choice:** Dual packages (Option 2).

**Rationale:** Achieves "zero binary footprint in production." The `strato` package (<10KB, pure Python) can be added to production dependencies with no overhead. `strato-cli` is installed only in dev/CI environments. Independent versioning means annotations (stable API) can evolve separately from the analysis tool (frequent updates).

**Risk:** Users may be confused about which package to install. Mitigated by clear documentation and the rule: "`strato` for annotations, `strato-cli` for the analysis tool."

---

### 3.12 Import Resolution: Scope Limits

*Tags: analysis, tooling*
**Context:** Python's import system is extremely flexible — dynamic imports, import hooks, `.pth` files, namespace packages, conditional imports. Strato must decide which import forms to support and which to exclude.

**Options considered:**

1. **Full Python import semantics** — Support everything including dynamic imports, import hooks, `.pth` files. Pros: Maximum compatibility. Cons: Intractable (dynamic imports require runtime execution), extremely complex, slow.

2. **Static imports only (v1.0)** — Absolute, from-import, relative only. Pros: Simple, fast. Cons: Misses star imports and namespace packages (common in real code).

3. **Static imports + pragmatic extensions (v1.1)** — Static imports plus: (a) star imports via literal `__all__` + public names fallback (one level only), (b) basic namespace packages within configured source roots, (c) conditional imports (first branch only). Exclude dynamic imports, import hooks, `.pth` files. Pros: Covers common patterns, manageable complexity. Cons: Still misses exotic cases.

**Choice:** Static imports + pragmatic extensions (Option 3).

**Rationale:** The v1.0 baseline was too restrictive. Star imports and namespace packages are common in real-world code. The v1.1 extensions address the most common gaps without crossing into intractable territory. Unresolvable imports are treated as `Unknown` (Decision 3.2) and skipped silently.

**Risk:** Codebases using `importlib.import_module()` extensively will have many unresolvable imports, leading to false negatives. Mitigated by `@blocking` decorator for manual annotation.

---

### 3.13 Caching Strategy and ty Boundary

*Tags: tooling*
**Context:** Strato's seven-phase pipeline has cacheable per-file phases (Parse, Resolve) and cross-file phases (Build, Propagate, Report). ty's Salsa database is in-memory only, not serializable.

**Options considered:**

1. **No caching** — Re-run everything. Pros: Simple. Cons: Slow on large codebases.

2. **Per-file caching (parse + imports only)** — Cache Phases 1-3 results keyed by file content hash. Re-run Phases 4-7 every time. Pros: Fast cached runs, simple invalidation, compatible with ty. Cons: Call graph construction + propagation re-run every time (but these are fast at O(V+E)).

3. **Full pipeline caching** — Cache the entire call graph and propagation results. Pros: Maximum performance. Cons: Complex invalidation, incompatible with ty (Salsa is not serializable).

**Choice:** Per-file caching (Option 2).

**Rationale:** The only option compatible with ty. Salsa's in-run memoization handles repeated queries within a single analysis run, but cross-run persistence is not supported. Per-file caching skips parsing (the expensive phase) while accepting that graph construction and propagation are re-run (fast — O(V+E)). Target: <500ms cached on 500 files.

**Risk:** If graph construction or ty queries are slower than expected, cached runs may not meet the <500ms target. Requires performance validation.

---

### 3.14 Determinism Contract

*Tags: tooling*
**Context:** Strato is designed for CI integration, where non-deterministic output causes flaky builds and erodes trust.

**Options considered:**

1. **Non-deterministic** — Use `HashMap`, accept varying output order. Pros: Simpler, slightly faster. Cons: Flaky CI, hard to test.

2. **Deterministic** — Use `BTreeMap`, explicit sorting for all output-affecting collections. Pros: Reproducible output, reliable CI. Cons: Slightly slower (O(log n) vs O(1)).

**Choice:** Deterministic (Option 2).

**Rationale:** Determinism is a hard requirement for CI. Enforced at multiple levels: (1) `BTreeMap` for output-affecting collections, (2) diagnostics sorted by file path → line → column → error code, (3) blocking path selection uses shortest-path with lexicographic tie-breaking, (4) cache keys use SHA-256 content hashes. The O(log n) overhead of `BTreeMap` is negligible compared to parsing and type inference.

**Risk:** Accidentally using `HashMap` in an output-affecting code path breaks the contract silently. Mitigated by determinism regression tests (run same fixture twice, assert identical output).

---

### 3.15 Failure and Warning Policy

*Tags: tooling*
**Context:** The analysis pipeline can encounter parse errors, unresolvable imports, ty panics, and I/O errors.

**Options considered:**

1. **Fail fast** — Any error aborts analysis. Pros: Simple. Cons: Unusable on real codebases (most projects have at least one file with issues).

2. **Warnings only (exit 0)** — All errors become warnings. Pros: Permissive. Cons: No signal for serious failures.

3. **Tiered failure policy** — Fatal errors (config errors, I/O errors, all files failed to parse) → non-zero exit. Non-fatal warnings (individual parse errors, unresolvable imports, ty panics) → collected but don't affect exit code. Pros: Balances usability and reliability. Cons: More complex.

**Choice:** Tiered failure policy (Option 3).

**Rationale:** Real-world codebases have files with parse errors (generated code, legacy syntax) and unresolvable imports (optional dependencies). Aborting analysis for one bad file is unacceptable. But if all files fail to parse, the user should be alerted. Exit codes: 0 = no blocking issues, 1 = blocking issues found, 2 = config error, 3 = all files failed to parse. Warnings do NOT affect exit code.

**Risk:** Users must understand which errors are fatal vs. warnings. Mitigated by clear error messages and documentation.

---

### 3.16 Async Scope Boundary: asyncio Only

*Tags: async*
**Context:** Python has multiple async frameworks: asyncio (stdlib), trio, curio, anyio. Each has its own event loop, task model, and blocking semantics.

**Options considered:**

1. **asyncio only (v1)** — Detect blocking in asyncio contexts only. Pros: Bounded scope, asyncio is the most common framework. Cons: Users of trio/curio/anyio can't use Strato.

2. **All frameworks (v1)** — Support asyncio, trio, curio, anyio. Pros: Maximum coverage. Cons: Complex (each framework has different APIs), high maintenance burden.

3. **Framework-agnostic** — Detect blocking in any `async def`. Don't recognize framework-specific escape hatches. Pros: Simple, works for all frameworks. Cons: High false positive rate (escape hatches not recognized).

**Choice:** asyncio only (Option 1).

**Rationale:** asyncio is the stdlib framework and the most widely used. Supporting multiple frameworks would require tracking each framework's APIs. The architecture supports future expansion — the executor wrapper registry (Decision 3.6) is already generalized, and adding trio/anyio patterns is straightforward in v2.

**Risk:** Users of trio, curio, or anyio cannot use Strato in v1. Mitigated by clear scope documentation and a v2 roadmap.
