# Design Overview

This section presents the core architectural and implementation choices that define Strato's approach to detecting blocking calls in async Python code. Each decision is structured as a tradeoff analysis: the problem that forced a choice, the decision made, the alternatives considered, and the risks that remain. These decisions are presented for expert review – scrutiny from practitioners in Python async, static analysis/PL, and Rust/tooling domains.

## Transitive Call Graph

Existing async linters (flake8-async, ruff ASYNC2XX) use pattern matching to detect direct blocking calls inside async functions – they scan for `time.sleep()`, `requests.get()`, etc. within `async def` bodies. This catches obvious cases but fails when blocking code hides behind intermediate function calls. The motivating example is `async def handler(): helper()` where `helper()` internally calls `time.sleep()` – no existing tool detects this because the blocking call is not syntactically visible at the async boundary.

Strato builds a full transitive call graph: a project-wide directed graph of function calls where "blocking" status propagates through edges, reporting when async functions can reach blocking nodes. This catches hidden blocking through arbitrarily deep call chains and provides unique value over existing tools. Pattern matching provides zero incremental value – users already have flake8-async and ruff – and the hybrid approach (one level of indirection) is a half-measure that still requires most of the infrastructure of a full graph (module resolution, symbol tables, call edge extraction) but arbitrarily limits the analysis depth. The full graph is the only option that delivers on the promise: if a blocking call is reachable from an async context through any chain of function calls, Strato finds it. Performance risk is mitigated through SCC-based propagation (O(V+E)) and incremental caching. The false negative risk (unresolvable calls are skipped) is addressed by the Precision Policy – better to miss some cases than flood users with false positives.

**Risk:** The call graph approach is unproven in the Python async linting domain. If real-world codebases have too many unresolvable calls (dynamic imports, heavy metaprogramming, complex type flows), the false negative rate could be so high that the tool provides little practical value. The acceptance test suite (Appendix B) is designed to validate coverage on realistic patterns, but production validation will be critical. If the approach fails, there is no fallback – the entire architecture is predicated on the call graph.

## Precision Policy

When Strato encounters a call it cannot resolve (e.g., `obj.method()` where `obj`'s type is unknown, or a dynamic import), it must decide: treat the call as potentially blocking (emit a diagnostic) or treat it as unknown (skip silently). This is the classic precision vs. recall tradeoff in static analysis. High recall (flag everything uncertain) maximizes detection but floods users with false positives. High precision (only flag proven cases) minimizes false positives but misses real bugs.

Strato treats unknown as unknown – unresolvable calls are neither blocking nor non-blocking, and are skipped. Diagnostics are only emitted when blocking status is definitively proven, yielding zero false positives so users trust the tool's output. This reflects the `BlockingStatus` enum design: `Unknown` is a permanent terminal state, never reclassified to `NotBlocking` or `Blocking`, and the propagation algorithm (Section 6) explicitly skips `Unknown` nodes. Strato is designed for expert review and CI integration, where false positives are more damaging than false negatives – a false positive wastes developer time, erodes trust, and leads to tool abandonment, while a false negative may be caught by other means (testing, profiling, manual review). The tradeoff is false negatives when resolution fails, meaning the tool may miss bugs in complex codebases.

**Risk:** The false negative rate could be unacceptably high in codebases with heavy use of dynamic typing, metaprogramming, or third-party libraries without type stubs. If Strato misses too many real bugs, users will perceive it as incomplete or unreliable. The mitigation is twofold: (1) ty integration (see Type Inference) improves type resolution, reducing the `Unknown` rate; (2) user annotations (`@blocking`, `@non_blocking`) allow manual override when Strato's analysis is insufficient.

<details>
<summary><strong>Alternatives considered</strong></summary>

**1. Unknown = Blocking**

Any unresolvable call is assumed blocking, and diagnostics are emitted for all uncertain cases.
This is the high-recall option but is likely to have a higher false-positive rate and noisier output.
On the other hand, this could be used as a forcing function to annotate unclear code.
    
**2. Unknown = Not Blocking (optimistic)**

Any unresolvable call is assumed safe. Only emit diagnostics for proven blocking calls.
This results in cleaner output and fewer false-positives, but can miss real bugs when blocking resolution fails.
</details>

## Blocking propagation

After the call graph is constructed and initial blocking annotations are applied, the propagation phase must spread "blocking" status through the graph: if function A calls function B, and B is blocking, then A is also blocking (unless the call is wrapped in an executor). The challenge is that call graphs contain cycles (mutual recursion). Naive iterative propagation (repeatedly scan the graph until no changes occur) works but is inefficient – it may require multiple passes over the same nodes, and the number of iterations is unbounded in the presence of complex cycles.

Strato uses SCC-based propagation via Tarjan's algorithm: decompose the call graph into Strongly Connected Components (SCCs), build a condensation graph (DAG of SCCs), topologically sort the condensation, and propagate in topological order (leaves first). This is the only approach that guarantees O(V+E) single-pass complexity regardless of cycle structure – critical for performance on large codebases (the 500-file benchmark targets sub-5-second fresh analysis). Both the iterative fixpoint and worklist alternatives degrade to O(V × E) in the presence of deep cycles, which are common in real-world code (e.g., mutually recursive validation functions, circular imports). The deterministic topological ordering also simplifies testing – the propagation order is reproducible, making it easier to write unit tests and debug failures.

**Risk:** The SCC decomposition adds a dependency on a correct implementation of Tarjan's algorithm. If the implementation has bugs (e.g., incorrect handling of self-loops, off-by-one errors in the DFS stack), the propagation results will be wrong, and the bugs will be hard to diagnose. The mitigation is thorough unit testing of the SCC decomposition in isolation and integration tests that validate end-to-end propagation on known-good fixtures.

<details>
<summary><strong>Alternatives considered</strong></summary>

**1. Iterative fixpoint**

Repeatedly scan all nodes, propagating blocking status from callees to callers, until no node's status changes. Simple to implement and easy to understand, but has O(V × E) worst-case complexity (V iterations, each scanning E edges), is slow on large graphs with deep cycles, and non-deterministic iteration order complicates testing.

**2. Worklist algorithm**

Maintain a worklist of nodes whose blocking status has changed. When a node's status changes, add its callers to the worklist. Repeat until worklist is empty. More efficient than naive iteration (only revisits affected nodes) and easier to implement than SCC decomposition, but still requires multiple passes in the presence of cycles with worst-case complexity of O(V × E) and non-deterministic worklist ordering.
</details>

## Type Inference

To resolve method calls (`obj.method()`), property accesses (`obj.prop`), and dunder invocations (`str(obj)`), Strato needs to infer the type of `obj`. A hand-rolled `ScopeBindings` system could track simple cases – `self`/`cls` in methods, constructor calls (`x = MyClass()`), and direct imports – but this misses common patterns like alias tracking (`x = requests.get; x()`) and return type inference (`loader = get_loader(); loader.load()`). These capabilities are critical: alias tracking is essential for executor wrapper detection (the pattern `safe = sync_to_async(func); await safe()` requires resolving `safe` back to a callable), and return type inference enables resolving indirect calls like `get_loader().load()`.

Strato integrates Astral's `ty_python_semantic` crate for full type inference, wrapped in a `trait TypeResolver` abstraction to isolate Strato from ty's API. This leverages Astral's investment in Python's type system and Salsa's in-run memoization. The tradeoffs are real: ty is a pre-1.0 dependency (API instability, potential panics), Salsa adds complexity, there's a double parse (ruff AST for Strato + ty's internal parse), and ty results are not cacheable cross-run (Salsa is in-memory only). These risks are mitigated by pinning to a specific ruff rev, panic isolation (catch panics, downgrade to `NullTypeResolver` per-file), and accepting the double parse cost (<100ms for 500 files). The caching limitation is addressed in Caching Strategy.

**Risk:** ty is pre-1.0 and may have bugs, panics, or API changes. If ty fails on a file, Strato degrades gracefully (emit a warning, skip type-dependent analysis for that file). The pinned rev strategy means Strato is frozen at a specific ruff version – upgrading requires a dedicated compatibility spike.

<details>
<summary><strong>Alternatives considered</strong></summary>

**1. Hand-rolled ScopeBindings**

Implement a minimal type inference system that tracks local variable bindings within function scopes. Resolve `self`, `cls`, constructors, and imports. Skip everything else. Offers full control with no external dependencies and a simple implementation, but misses common patterns (alias tracking is critical for executor wrapper detection), is limited by what we're willing to implement, and reinvents the wheel.

**2. Hybrid: ScopeBindings + ty fallback**

Use ScopeBindings for simple cases, query ty for complex cases. Provides graceful degradation if ty fails, but requires maintaining two type inference systems with an unclear boundary between "simple" and "complex", adding complexity for minimal benefit.
</details>

## Phantom Nodes

Strato's call graph includes nodes for user-defined functions (parsed from source) and nodes for external blocking functions (stdlib, third-party libraries). External symbols like `time.sleep` and `requests.get` need to become resolvable call graph nodes even though their source files are not in the project's source roots.

Strato pre-seeds phantom nodes from its blocking function database: for every entry, a call graph node is created with no source location, zero parsing cost, and no version skew. The database is the single source of truth. During Phase 4 initialization, the system iterates over the blocking database and creates a `CallGraphNode` for each entry with `location: None` and `blocking_status: KnownBlocking`. When the call graph builder encounters `time.sleep(1)`, the symbol resolution constructs the qualified name `"time.sleep"`, finds the phantom node, and creates an edge. This aligns with Strato's Precision Policy: only known blocking functions are tracked, and external calls not in the database are treated as `Unknown` and skipped.

**Risk:** Tightly coupled to the blocking database. If the database is incomplete, calls to unlisted blocking functions will be unresolvable and skipped. The mitigation is a comprehensive database (~80+ entries) and user extensibility (config allows adding custom entries, `@blocking` decorator allows per-function annotation).

<details>
<summary><strong>Alternatives considered</strong></summary>

**1. Parse external libraries**

Include stdlib and third-party packages in the source roots. Provides uniform treatment of all code, but at massive performance cost (parsing thousands of files), and many libraries are C extensions with no Python source, introducing version skew.

**2. Stub files (.pyi)**

Provide hand-written `.pyi` stubs for known blocking functions. Lightweight, but must be maintained separately and still requires parsing.
</details>

## Escape Hatches

Python's asyncio provides `loop.run_in_executor()` and `asyncio.to_thread()` to offload blocking work to a thread pool, but real-world codebases use custom wrappers (e.g., `asgiref.sync.sync_to_async`, `anyio.to_thread.run_sync`) and project-specific helpers. Hardcoding every possible wrapper is unmaintainable.

Strato maintains a generalized registry of known executor wrappers populated from three sources: (a) built-in patterns, (b) user config (`[tool.strato.executor-wrappers]`), (c) `@unblocker` decorator. Each entry specifies which parameter receives the callable being offloaded. The call graph builder checks the registry when visiting call expressions; if the callee matches, the edge to the callable argument is marked `in_executor: true`, suppressing blocking propagation. Built-in patterns cover the most common cases with zero configuration, config allows adding third-party wrappers without modifying Strato's code, and the `@unblocker` decorator allows annotating project-specific wrappers.

**Risk:** Users must configure third-party wrappers not in the built-in list. If unconfigured, Strato will flag safe code as blocking (false positive). The registry also depends on ty's ability to resolve the callable argument – if ty can't resolve `safe = sync_to_async(func); await safe()`, the protection is lost.

<details>
<summary><strong>Alternatives considered</strong></summary>

**1. Hardcoded list**

Recognize `run_in_executor` and `to_thread` by name. Simple, but not extensible and misses third-party wrappers.

**2. Heuristic detection**

Analyze function bodies to detect patterns like "creates a thread". Automatic, but unreliable and doesn't work for C extensions.
</details>

## Error reporting

When Strato detects a blocking call chain like `async handler() → helper() → db_query() → psycopg2.connect()`, the diagnostic must point somewhere actionable. The blocking call is in `psycopg2.connect()` (third-party, unfixable), so the question is where to direct the user's attention.

Strato makes this configurable, defaulting to `first-party-deepest`: point the diagnostic at the deepest first-party function in the chain, which is the most actionable location. Different teams have different workflows – the `async-boundary` strategy is available as an alternative for those who prefer the async function as the anchor point. The full chain is always included in diagnostics for context.

**Risk:** `first-party-deepest` may be confusing if the deepest first-party function is a low-level utility far from the async context. The `async-boundary` strategy is available as a fallback.

<details>
<summary><strong>Alternatives considered</strong></summary>

**1. Async boundary**

Always point to the async function. Provides clear context, but may be far from the fix point and less actionable.

**2. First-party deepest (non-configurable)**

Always point to the deepest first-party function in the chain. Most actionable since the user can fix this function directly, but may be in a utility far from the async context with no flexibility for different workflows.
</details>

## Blocking Database

Strato needs a database of known blocking functions to seed phantom nodes (see Phantom Nodes). An exhaustive database covering every blocking function in stdlib and popular libraries would maximize coverage but create a massive maintenance burden and high risk of false positives for functions that are technically blocking but fast (e.g., `os.getpid()`).

Strato uses a curated database of ~80 entries focused on common, impactful blocking functions: I/O, synchronization, sleep/wait, and subprocess. The list covers the most common blocking patterns (`time.sleep`, `requests.*`, `urllib.*`, `socket.*`, `subprocess.*`, `os.read`, `open()`, database drivers) while excluding fast blocking functions that rarely cause problems. The database is user-extensible via config and the `@blocking` decorator.

**Risk:** May miss blocking functions common in specific domains (e.g., scientific computing). Users must extend via config.

<details>
<summary><strong>Alternatives considered</strong></summary>

**1. Exhaustive**

Every blocking function. Maximum coverage, but creates a massive maintenance burden with high risk of false positives for functions that are blocking but fast (e.g., `os.getpid()`).

**2. Minimal (~20 entries)**

Only the most egregious offenders. Very low false positive rate, but incomplete and misses many real bugs.
</details>

## Help Text Policy

Diagnostics include help text suggesting how to fix the issue. Strato uses generic recommendations – "use an async HTTP library" or "offload to `asyncio.to_thread()`" – listing multiple alternatives without prescribing one. This keeps help text neutral and timeless: Strato is a linting tool, not a library recommendation engine. Where alternatives exist (e.g., `aiohttp` or `httpx`), they are listed neutrally without implicit endorsement.

**Risk:** Generic text may be too vague for novice users. Mitigation: include multiple examples without recommending one.

<details>
<summary><strong>Alternatives considered</strong></summary>

**1. Specific recommendations**

"use `httpx` instead of `requests`". Actionable, but makes Strato a kingmaker and recommendations may become outdated.

**2. No help text**

Only report the problem. Minimal, but unhelpful.
</details>

## Distribution

Strato consists of a Rust binary (analysis tool) and a Python package (`@blocking`/`@non_blocking`/`@unblocker` decorators). These are distributed as two packages: `strato` (pure Python, annotations only, zero deps, <10KB) and `strato-cli` (Rust binary via maturin). This achieves "zero binary footprint in production" – the `strato` package can be added to production dependencies with no overhead, while `strato-cli` is installed only in dev/CI environments. Independent versioning means annotations (stable API) can evolve separately from the analysis tool (frequent updates).

**Risk:** Users may be confused about which package to install. Mitigated by clear documentation and the rule: "`strato` for annotations, `strato-cli` for the analysis tool."

<details>
<summary><strong>Alternatives considered</strong></summary>

**1. Single package**

Binary + annotations together. Simple, but results in a large package (~10MB) and forces users who only want annotations to install the binary.

**2. Binary-only**

No annotations package. Simplest approach, but poor UX with no type checking for decorators.
</details>

## Import Resolution

Python's import system is extremely flexible – dynamic imports, import hooks, `.pth` files, namespace packages, conditional imports – and Strato must decide which forms to support. Strato supports static imports plus pragmatic extensions (v1.1): (a) star imports via literal `__all__` + public names fallback (one level only), (b) basic namespace packages within configured source roots, (c) conditional imports (first branch only). Dynamic imports, import hooks, and `.pth` files are excluded. Star imports and namespace packages are common in real-world code, and addressing them avoids a major source of false negatives without crossing into intractable territory. Unresolvable imports are treated as `Unknown` (see Precision Policy) and skipped silently.

**Risk:** Codebases using `importlib.import_module()` extensively will have many unresolvable imports, leading to false negatives. Mitigated by `@blocking` decorator for manual annotation.

<details>
<summary><strong>Alternatives considered</strong></summary>

**1. Full Python import semantics**

Support everything including dynamic imports, import hooks, and `.pth` files. Maximum compatibility, but intractable (dynamic imports require runtime execution), extremely complex, and slow.

**2. Static imports only**

Absolute, from-import, and relative only. Simple and fast, but misses star imports and namespace packages which are common in real code.
</details>

## Caching Strategy

Strato's seven-phase pipeline has cacheable per-file phases (Parse, Resolve) and cross-file phases (Build, Propagate, Report). ty's Salsa database is in-memory only and not serializable, which constrains the caching design. Strato caches Phases 1-3 results (parse + imports) keyed by file content hash and re-runs Phases 4-7 (call graph construction + propagation) every time. This is the only option compatible with ty – Salsa's in-run memoization handles repeated queries within a single analysis run, but cross-run persistence is not supported. Per-file caching skips parsing (the expensive phase) while accepting that graph construction and propagation re-run at O(V+E). Target: <500ms cached on 500 files.

**Risk:** If graph construction or ty queries are slower than expected, cached runs may not meet the <500ms target. Requires performance validation.

<details>
<summary><strong>Alternatives considered</strong></summary>

**1. No caching**

Re-run everything. Simple, but slow on large codebases.

**2. Full pipeline caching**

Cache the entire call graph and propagation results. Maximum performance, but complex invalidation and incompatible with ty (Salsa is not serializable).
</details>

## Determinism Contract

Strato is designed for CI integration, where non-deterministic output causes flaky builds and erodes trust. Strato enforces determinism at multiple levels: `BTreeMap` for all output-affecting collections, diagnostics sorted by file path → line → column → error code, blocking path selection using shortest-path with lexicographic tie-breaking, and cache keys using SHA-256 content hashes. The O(log n) overhead of `BTreeMap` versus `HashMap` is negligible compared to parsing and type inference.

**Risk:** Accidentally using `HashMap` in an output-affecting code path breaks the contract silently. Mitigated by determinism regression tests (run same fixture twice, assert identical output).

<details>
<summary><strong>Alternatives considered</strong></summary>

**1. Non-deterministic**

Use `HashMap`, accept varying output order. Simpler and slightly faster, but leads to flaky CI and is hard to test.
</details>

## Failure and Warning Policy

The analysis pipeline can encounter parse errors, unresolvable imports, ty panics, and I/O errors. Real-world codebases have files with parse errors (generated code, legacy syntax) and unresolvable imports (optional dependencies), so aborting analysis for one bad file is unacceptable – but if all files fail to parse, the user should be alerted.

Strato uses a tiered failure policy: fatal errors (config errors, I/O errors, all files failed to parse) produce a non-zero exit, while non-fatal warnings (individual parse errors, unresolvable imports, ty panics) are collected but don't affect exit code. Exit codes: 0 = no blocking issues, 1 = blocking issues found, 2 = config error, 3 = all files failed to parse. Warnings never affect exit code.

**Risk:** Users must understand which errors are fatal vs. warnings. Mitigated by clear error messages and documentation.

<details>
<summary><strong>Alternatives considered</strong></summary>

**1. Fail fast**

Any error aborts analysis. Simple, but unusable on real codebases where most projects have at least one file with issues.

**2. Warnings only (exit 0)**

All errors become warnings. Permissive, but provides no signal for serious failures.
</details>

## Async Library Support

Python has multiple async frameworks – asyncio (stdlib), trio, curio, anyio – each with its own event loop, task model, and blocking semantics. Strato targets asyncio only in v1: it is the stdlib framework and the most widely used. Supporting multiple frameworks would require tracking each framework's distinct APIs, which adds complexity without proportionate value at launch. The architecture supports future expansion – the executor wrapper registry (see Escape Hatches) is already generalized, and adding trio/anyio patterns is straightforward in v2.

**Risk:** Users of trio, curio, or anyio cannot use Strato in v1. Mitigated by clear scope documentation and a v2 roadmap.

<details>
<summary><strong>Alternatives considered</strong></summary>

**1. All frameworks (v1)**

Support asyncio, trio, curio, and anyio. Maximum coverage, but complex since each framework has different APIs, with a high maintenance burden.

**2. Framework-agnostic**

Detect blocking in any `async def` without recognizing framework-specific escape hatches. Simple and works for all frameworks, but results in a high false positive rate since escape hatches are not recognized.
</details>
