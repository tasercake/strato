# Design Overview

This section presents the core architectural and implementation choices that define Strato's approach to detecting blocking calls in async Python code. Each decision is structured as a tradeoff analysis: the problem that forced a choice, the decision made, the alternatives considered, and the risks that remain. These decisions are presented for expert review – scrutiny from practitioners in Python async, static analysis/PL, and Rust/tooling domains.

## Transitive Call Graph

Existing async linters (flake8-async, ruff ASYNC2XX) use pattern matching to detect direct blocking calls inside async functions – they scan for `time.sleep()`, `requests.get()`, etc. within `async def` bodies. This catches obvious cases but fails when blocking code hides behind intermediate function calls. The motivating example is `async def handler(): helper()` where `helper()` internally calls `time.sleep()` – no existing tool detects this because the blocking call is not syntactically visible at the async boundary.

Strato builds a full transitive call graph: a project-wide directed graph of function calls where "blocking" status propagates through edges, reporting when async functions can reach blocking nodes. This catches hidden blocking through arbitrarily deep call chains and provides unique value over existing tools. Pattern matching provides zero incremental value – users already have flake8-async and ruff – and the hybrid approach (one level of indirection) is a half-measure that still requires most of the infrastructure of a full graph (module resolution, symbol tables, call edge extraction) but arbitrarily limits the analysis depth. The full graph is the only option that delivers on the promise: if a blocking call is reachable from an async context through any chain of function calls, Strato finds it. Performance risk is mitigated through SCC-based propagation (O(V+E)) and incremental caching. The false negative risk (unresolvable calls are skipped) is addressed by the Precision Policy – better to miss some cases than flood users with false positives.

**Risk:** The call graph approach is unproven in the Python async linting domain. If real-world codebases have too many unresolvable calls (dynamic imports, heavy metaprogramming, complex type flows), the false negative rate could be so high that the tool provides little practical value. The acceptance test suite (Appendix B) is designed to validate coverage on realistic patterns, but production validation will be critical. If the approach fails, there is no fallback – the entire architecture is predicated on the call graph.

## Precision Policy

When Strato encounters a call it cannot resolve (e.g., `obj.method()` where `obj`'s type is unknown, or a dynamic import), it must decide: treat the call as potentially blocking (emit a diagnostic) or treat it as unknown (skip silently). This is the classic precision vs. recall tradeoff in static analysis. High recall (flag everything uncertain) maximizes detection but floods users with false positives. High precision (only flag proven cases) minimizes false positives but misses real bugs.

Strato treats unknown as unknown – unresolvable calls are neither blocking nor non-blocking, and are skipped. Diagnostics are only emitted when blocking status is definitively proven, yielding a high-trust, low-false-positive report surface for proven-blocking findings. This reflects the `BlockingStatus` enum design: `Unknown` is a permanent terminal state, never reclassified to `NotBlocking` or `Blocking`, and the propagation algorithm (Section 6) explicitly skips `Unknown` nodes. Strato is designed for expert review and CI integration, where false positives are more damaging than false negatives – a false positive wastes developer time, erodes trust, and leads to tool abandonment, while a false negative may be caught by other means (testing, profiling, manual review). The tradeoff is false negatives when resolution fails, meaning the tool may miss bugs in complex codebases.

**Risk:** The false negative rate could be unacceptably high in codebases with heavy use of dynamic typing, metaprogramming, or third-party libraries without type stubs. If Strato misses too many real bugs, users will perceive it as incomplete or unreliable. The mitigation is twofold: (1) the vendored Ruff/ty facade (see Semantic Substrate) improves semantic resolution, reducing the `Unknown` rate; (2) user annotations (`@blocking`, `@non_blocking`) allow manual override when Strato's analysis is insufficient.

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

## Semantic Substrate

To resolve direct calls, method calls (`obj.method()`), property accesses (`obj.prop`), and dunder invocations (`str(obj)`), Strato needs more than a local symbol table. It needs Python-aware module resolution, import aliasing, name binding, class hierarchy lookup, and inferred expression types. A hand-rolled local binding resolver could track simple cases like `self`/`cls`, constructor calls (`x = MyClass()`), and direct imports, but it would duplicate a large part of Python's static semantics and still miss common patterns like `x = requests.get; x()` or `loader = get_loader(); loader.load()`.

Strato vendors Astral's full Ruff monorepo as a pinned source dependency and uses Ruff/ty as the semantic substrate. Key consumed crates include `ruff_db`, `ruff_python_parser`, `ruff_python_ast`, `ty_project`, `ty_module_resolver`, `ty_python_core`, and `ty_python_semantic`. Strato does not define an independent module resolver or semantic resolver with parallel semantics. Instead, `strato_ty_adapter` exposes a small facade over vendored Ruff/ty for the facts needed by blocking analysis: what callable an expression refers to, whether an attribute resolves to a method or property, what class hierarchy lookup says about an implicit dunder, and which imports/names resolve to first-party definitions or known external qualified aliases. Strato then owns the blocking layer: call graph nodes and edges, phantom nodes from the blocking database, executor-wrapper edge suppression, SCC propagation, and diagnostics.

The tradeoffs are real: ty is pre-1.0, Ruff/ty's library crates are not a stable public API, Salsa adds an in-memory query system, and ty's semantic database is not serializable for Strato's cross-run cache. Strato accepts these costs by vendoring Ruff as source and applying surgical patches when the public API does not expose required facts. Those patches must expose semantic facts only; they must not embed Strato's blocking policy into Ruff/ty. Strato uses Ruff's parsed modules from the ty database where possible rather than maintaining an intentional double parse. Panic handling is best-effort: Strato will isolate calls into the facade where Rust unwinding can be caught, emit a warning, and skip semantic facts for the affected file or query. This does not protect against aborting panics or process-level failures.

**Risk:** The vendored Ruff/ty strategy creates an explicit fork-maintenance burden. Upgrades require replaying Strato's facade patches, rerunning facade conformance tests, and validating every acceptance fixture. If a supported facade query returns `Unknown` for an individual expression, Strato skips the corresponding call edge or attribute/dunder edge per the precision policy. If the facade lacks a required query category entirely, v1 scope is not reduced; that gap is an implementation blocker rather than a planned feature cut.

<details>
<summary><strong>Alternatives considered</strong></summary>

**1. Hand-rolled local binding resolver**

Implement a minimal semantic system that tracks local variable bindings within function scopes. Resolve `self`, `cls`, constructors, and imports. Skip everything else. Offers full control with no external dependencies and a simple implementation, but misses common patterns (alias tracking is critical for executor wrapper detection), is limited by what we're willing to implement, and reinvents the wheel.

**2. Hybrid: local resolver + ty fallback**

Use local rules for simple cases and query ty for complex cases. Provides apparent graceful degradation if ty fails, but requires maintaining two semantic systems with an unclear boundary between "simple" and "complex", adding complexity and inconsistency risk for minimal benefit.

**3. Depend only on upstream public Ruff/ty APIs**

Avoid vendoring and use only APIs already exposed by Ruff/ty. This reduces fork maintenance but leaves Strato blocked on facts that are currently internal to ty, especially precise call-target, property, and dunder resolution. Since Strato's v1 scope requires those facts, source vendoring with a narrow facade is the more realistic option.
</details>

## Phantom Nodes

Strato's call graph includes nodes for user-defined functions (parsed from source) and nodes for external blocking functions (stdlib, third-party libraries). External symbols like `time.sleep` and `requests.get` need to become resolvable call graph nodes even though their source files are not in the project's source roots.

Strato loads the effective blocking database during Phase 1 and builds a deterministic phantom-node index from it during Phase 4. Exact database entries are materialized as call graph nodes only when a resolved call target actually references them. Configured `blocking_modules` entries are applied as module-boundary prefix matches during graph construction; when any resolved external alias falls under such a module, Strato creates a phantom node for the matched call target on demand and marks it `KnownBlocking`. When the call graph builder encounters `time.sleep(1)`, the facade provides aliases including `"time.sleep"`, Strato materializes the matching phantom node and creates an edge. Alias sets are required because ty/typeshed may resolve public APIs through re-exports, inherited base classes, or implementation modules such as `_socket`. This aligns with Strato's Precision Policy: only known blocking functions and explicitly configured blocking module prefixes are tracked, and other external calls are treated as `Unknown` and skipped.

**Risk:** Tightly coupled to the blocking database. If the database is incomplete, calls to unlisted blocking functions will be unresolvable and skipped. The mitigation is a curated database (currently 61 entries) and user extensibility (config allows adding custom entries, `@blocking` decorator allows per-function annotation).

<details>
<summary><strong>Alternatives considered</strong></summary>

**1. Parse external libraries**

Include stdlib and third-party packages in the source roots. Provides uniform treatment of all code, but at massive performance cost (parsing thousands of files), and many libraries are C extensions with no Python source, introducing version skew.

**2. Stub files (.pyi)**

Provide hand-written `.pyi` stubs for known blocking functions. Lightweight, but must be maintained separately and still requires parsing.
</details>

## Escape Hatches

Python's asyncio provides `loop.run_in_executor()` and `asyncio.to_thread()` to offload blocking work to a thread pool, but real-world codebases use custom wrappers (e.g., `asgiref.sync.sync_to_async`) and project-specific helpers. Hardcoding every possible wrapper is unmaintainable.

Strato maintains a generalized registry of known executor wrappers populated from three sources: (a) built-in patterns, (b) user config (`[tool.strato.executor-wrappers]`), (c) `@unblocker` decorator. Each entry specifies which parameter receives the callable being offloaded. The call graph builder checks the registry when visiting call expressions; if the facade resolves the callee to an executor wrapper, the edge to the callable argument is marked `in_executor: true`, suppressing blocking propagation. `run_in_executor` is recognized through a facade query for event-loop method identity rather than through Strato-owned local assignment heuristics. Built-in patterns cover the most common cases with zero configuration, config allows adding third-party wrappers without modifying Strato's code, and the `@unblocker` decorator allows annotating project-specific wrappers.

**Risk:** Users must configure third-party wrappers not in the built-in list. If unconfigured, Strato will flag safe code as blocking (false positive). The registry also depends on the facade's ability to resolve the wrapper and callable argument – if the facade cannot resolve `safe = sync_to_async(func); await safe()`, the protection is lost.

<details>
<summary><strong>Alternatives considered</strong></summary>

**1. Hardcoded list**

Recognize `run_in_executor` and `to_thread` by name. Simple, but not extensible and misses third-party wrappers.

**2. Heuristic detection**

Analyze function bodies to detect patterns like "creates a thread". Automatic, but unreliable and doesn't work for C extensions.
</details>

## Error reporting

When Strato detects a blocking call chain like `async handler() → helper() → db_query() → psycopg2.connect()`, the diagnostic must point somewhere actionable. The blocking call is in `psycopg2.connect()` (third-party, unfixable), so the question is where to direct the user's attention.

Strato makes this configurable, defaulting to `first-party-deepest`: point the diagnostic at the deepest first-party call site in the chain, which is the most actionable location. Different teams have different workflows – the `async-boundary` strategy is available as an alternative for those who prefer the async function as the anchor point. The full chain is always included in diagnostics for context.

**Risk:** `first-party-deepest` may be confusing if the deepest first-party call site is in a low-level utility far from the async context. The `async-boundary` strategy is available as a fallback.

<details>
<summary><strong>Alternatives considered</strong></summary>

**1. Async boundary**

Always point to the async function. Provides clear context, but may be far from the fix point and less actionable.

**2. First-party deepest (non-configurable)**

Always point to the deepest first-party call site in the chain. Most actionable since the user can fix or offload this expression directly, but may be in a utility far from the async context with no flexibility for different workflows.
</details>

## Blocking Database

Strato needs a database of known blocking functions to materialize phantom nodes (see Phantom Nodes). An exhaustive database covering every blocking function in stdlib and popular libraries would maximize coverage but create a massive maintenance burden and high risk of false positives for functions that are technically blocking but fast (e.g., `os.getpid()`).

Strato uses a curated database of 61 entries focused on common, impactful blocking functions: I/O, synchronization, sleep/wait, and subprocess. The list covers the most common blocking patterns (`time.sleep`, `requests.*`, `urllib.*`, `socket.*`, `subprocess.*`, `os.read`, `open()`, database drivers) while excluding fast blocking functions that rarely cause problems. The database is user-extensible via config and the `@blocking` decorator.

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

Python's import system is extremely flexible – dynamic imports, import hooks, `.pth` files, namespace packages, conditional imports – and Strato must define the semantic scope it expects from vendored Ruff/ty rather than reimplementing Python imports itself. For v1, Strato configures the vendored Ruff/ty project database with the project source roots, Python version, and stub paths mapped to ty `environment.extra-paths`, then consumes facade-normalized module/name facts for static filesystem-backed imports. Dynamic imports, import hooks, and runtime `sys.path` mutation remain outside Strato's guarantees. Star imports, namespace packages, and conditional imports are documented as best-effort only to the extent Ruff/ty can resolve them under the configured source roots and extra paths. Unresolvable imports are treated as `Unknown` (see Precision Policy) and skipped silently.

**Risk:** Codebases using `importlib.import_module()` or runtime import customization extensively will have many unresolvable imports, leading to false negatives. Mitigated by explicit imports, type information where possible, and `@blocking` decorator for manual annotation.

<details>
<summary><strong>Alternatives considered</strong></summary>

**1. Full Python import semantics**

Support everything including dynamic imports, import hooks, and `.pth` files. Maximum compatibility, but intractable (dynamic imports require runtime execution), extremely complex, and slow.

**2. Static imports only**

Absolute, from-import, and relative only. Simple and fast, but misses star imports and namespace packages which are common in real code.
</details>

## Caching Strategy

Strato's cache boundary is limited to Strato-owned Phase 1 and Phase 2 artifacts from discovery and syntactic extraction: file manifests, content hashes, parsed-module-derived declarations, import statements as syntax, and decorator annotations. Ruff parsed modules, ty's Salsa database, Phase 3 semantic facade facts, call edges that depend on semantic resolution, the project call graph, propagation results, and diagnostics are not serialized by Strato. Salsa's in-run memoization handles repeated parse and semantic queries within one analysis run, but cross-run persistence belongs only to Strato's own stable artifacts. Target: <500ms cached on 500 files, subject to validation because vendored Ruff/ty setup and call graph construction still run each time.

**Risk:** If vendored Ruff/ty setup, facade queries, or graph construction are slower than expected, cached runs may not meet the <500ms target. Requires performance validation before adding any broader cache boundary.

<details>
<summary><strong>Alternatives considered</strong></summary>

**1. No caching**

Re-run everything. Simple, but slow on large codebases.

**2. Full pipeline caching**

Cache the entire call graph and propagation results. Maximum performance, but complex invalidation and unsafe across semantic changes because call edges depend on Ruff/ty facade facts that Strato does not serialize.
</details>

## Determinism Contract

Strato is designed for CI integration, where non-deterministic output causes flaky builds and erodes trust. Strato enforces determinism at the blocking-analysis boundary: output-affecting collections use ordered data structures, filesystem inputs are normalized and sorted, diagnostics are sorted by file path → line → column → error code, blocking path selection uses shortest-path with lexicographic tie-breaking, and cache keys use SHA-256 content hashes. Ruff/ty's internal query order is not part of Strato's output contract; any semantic facts consumed from the facade must be normalized before they affect graph insertion or diagnostics. The O(log n) overhead of ordered maps versus hash maps is negligible compared to parsing and semantic analysis.

**Risk:** Accidentally using unordered iteration in an output-affecting code path breaks the contract silently. Mitigated by determinism regression tests (run the same fixture multiple times, with cache cold and warm, and assert identical output).

<details>
<summary><strong>Alternatives considered</strong></summary>

**1. Non-deterministic**

Use `HashMap`, accept varying output order. Simpler and slightly faster, but leads to flaky CI and is hard to test.
</details>

## Failure and Warning Policy

The analysis pipeline can encounter syntax errors, unresolvable imports, Ruff/ty facade failures, recoverable facade-boundary panics, and I/O errors. Ruff's parser is error-resilient and usually returns an AST plus syntax diagnostics, so aborting analysis for one bad file is unacceptable – but if no source file can be analyzed, the user should be alerted.

Strato uses a tiered failure policy: fatal errors (config errors, I/O errors, no analyzable source files) produce a non-zero exit, while non-fatal warnings (individual syntax errors, unresolvable imports, Ruff/ty facade failures, recoverable facade-boundary panics) are collected but don't affect exit code. Exit codes: 0 = no blocking issues, 1 = blocking issues found, 2 = config error, 3 = no analyzable source files. Warnings never affect exit code.

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
