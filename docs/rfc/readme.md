# RFC: Strato — Async Blocking Call Detector for Python

> **Status**: Draft — seeking expert review
> **Authors**: [TBD]
> **Date**: 2026-02-04
> **Review period**: [TBD]

> **How to review this document**: This RFC is structured in layers. Sections 1-2 give you enough context to understand the problem and our approach. Sections 3-11 are the detailed design. Section 12 documents known limitations. Section 13 collects open questions. Each section is tagged with the expertise most relevant to it: **[async]** for Python async experts, **[analysis]** for static analysis / PL experts, **[tooling]** for Rust/tooling experts.
>
> You don't need to read everything — focus on the sections tagged with your expertise, and especially **Section 13 (Open Questions)** where we most need your input.

### Reviewer Routing Guide

| Your expertise | Read these sections | Skip these |
|----------------|--------------------|----|
| **Python async** [async] | [1](#1-executive-summary), [2](#2-problem-statement--motivation), [3.1](#31-transitive-call-graph-vs-pattern-matching)-[3.2](#32-precision-policy-unknown--not-blocking), [3.6](#36-generalized-executor-wrapper-system), [3.8](#38-blocking-database-curated-list-vs-exhaustive)-[3.9](#39-help-text-policy-no-single-library-endorsement), [3.16](#316-async-scope-boundary-asyncio-only), [8](#8-blocking-function-database--annotations), [9](#9-escape-hatches--executor-wrappers), [12](#12-known-limitations--scope-boundaries), [13](#13-open-questions-for-reviewers) | [6](#6-call-graph--type-resolution) (call graph internals), [11](#11-supporting-systems) (tooling) |
| **Static analysis / PL** [analysis] | [1](#1-executive-summary), [2](#2-problem-statement--motivation), [3.1](#31-transitive-call-graph-vs-pattern-matching)-[3.5](#35-phantom-nodes-for-external-symbols), [5](#5-analysis-pipeline), [6](#6-call-graph--type-resolution), [7](#7-blocking-propagation), [12](#12-known-limitations--scope-boundaries), [13](#13-open-questions-for-reviewers) | [8](#8-blocking-function-database--annotations) (blocking database), [11](#11-supporting-systems) (tooling) |
| **Rust / tooling** [tooling] | [1](#1-executive-summary), [3.10](#310-rust-with-ruff-crates)-[3.15](#315-test-strategy-golden-output-comparison), [4](#4-architecture-overview), [10](#10-error-reporting--diagnostics), [11](#11-supporting-systems), [Appendix C](#appendix-c-output-format-specifications)-[E](#appendix-e-repository-structure--implementation-plan), [13](#13-open-questions-for-reviewers) | [7](#7-blocking-propagation) (propagation algorithm), [9](#9-escape-hatches--executor-wrappers) (escape hatches) |
| **Everyone** | [1](#1-executive-summary), [2](#2-problem-statement--motivation), [12](#12-known-limitations--scope-boundaries), [13](#13-open-questions-for-reviewers) | — |

### Glossary

| Term | Definition |
|------|-----------|
| **Blocking call** | A function call that performs synchronous I/O or waits, stalling the event loop (e.g., `time.sleep()`, `requests.get()`) |
| **Transitive blocking** | A function that is not itself blocking but calls a blocking function through one or more intermediary calls |
| **Event loop** | The asyncio mechanism that schedules and runs coroutines concurrently on a single thread |
| **Call graph** | A directed graph where nodes represent functions and edges represent call relationships |
| **SCC (Strongly Connected Component)** | A maximal set of nodes in a directed graph where every node is reachable from every other node (mutual recursion) |
| **Phantom node** | A call graph node for an external symbol (e.g., `time.sleep`) with no source location, pre-seeded from the blocking database |
| **Escape hatch** | A pattern that correctly offloads blocking work to a thread pool (e.g., `asyncio.to_thread()`, `loop.run_in_executor()`) |
| **Intervention point** | The source location shown in a diagnostic — where the user should make a change |
| **First-party code** | Code in the user's project (under configured source roots) |
| **Third-party code** | Code from external packages (stdlib, site-packages) |
| **ty** | Astral's Python type inference crate, used for resolving method calls, properties, and dunder invocations |
| **Salsa** | A query-based incremental computation framework used by ty for in-memory memoization |
| **Propagation** | The process of spreading "blocking" status through the call graph from known blocking functions to their callers |
| **Condensation graph** | A DAG formed by collapsing each SCC into a single node — enables single-pass topological propagation |

---

## Table of Contents

1. [Executive Summary](#1-executive-summary)
2. [Problem Statement & Motivation](#2-problem-statement--motivation)
3. [Design Decisions](#3-design-decisions)
4. [Architecture Overview](#4-architecture-overview)
5. [Analysis Pipeline](#5-analysis-pipeline)
6. [Call Graph & Type Resolution](#6-call-graph--type-resolution)
7. [Blocking Propagation](#7-blocking-propagation)
8. [Blocking Function Database & Annotations](#8-blocking-function-database--annotations)
9. [Escape Hatches & Executor Wrappers](#9-escape-hatches--executor-wrappers)
10. [Error Reporting & Diagnostics](#10-error-reporting--diagnostics)
11. [Supporting Systems](#11-supporting-systems)
12. [Known Limitations & Scope Boundaries](#12-known-limitations--scope-boundaries)
13. [Open Questions for Reviewers](#13-open-questions-for-reviewers)

**Appendices**
- [A: Blocking Function Database (Complete)](#appendix-a-blocking-function-database-complete)
- [B: Acceptance Test Cases](#appendix-b-acceptance-test-cases)
- [C: Output Format Specifications](#appendix-c-output-format-specifications)
- [D: Configuration Schema](#appendix-d-configuration-schema)
- [E: Repository Structure & Implementation Plan](#appendix-e-repository-structure--implementation-plan)

---

## 1. Executive Summary

**Strato** is a Rust-based static analysis tool that detects blocking function calls inside Python async contexts. Unlike existing linters (flake8-async, ruff ASYNC2XX) which only catch **direct** blocking calls, Strato performs **full transitive call-graph analysis** — tracing through intermediary sync functions to find hidden blocking calls that would stall the event loop.

### The Novel Contribution

No existing tool catches this:

```python
def sync_helper():
    time.sleep(1)          # Blocking call hidden here

async def handler():
    sync_helper()          # Strato catches this. No other tool does.
```

Strato builds a project-wide call graph, propagates "blocking" status through function call chains using SCC-based linear-time analysis, and reports when blocking code is reachable from async contexts — with configurable error reporting that shows diagnostics in the user's own code, not deep in third-party libraries.

### Key Design Choices

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Analysis approach | Full transitive call graph | Only way to catch indirect blocking (the core value proposition) |
| Precision policy | High precision (Unknown = skip) | Zero false positives — users trust every diagnostic |
| Propagation algorithm | SCC-based (Tarjan's) | O(V+E) linear time, handles cycles elegantly |
| Type inference | Astral's `ty` crate | Full alias tracking, return types, MRO — critical for method/property resolution |
| Language | Rust | Performance parity with ruff; access to ruff parser crates |
| Distribution | Dual PyPI packages | Zero binary footprint in production (`strato` annotations + `strato-cli` binary) |
| Error reporting | Configurable intervention point | Default: deepest first-party function (most actionable) |
| Blocking database | Curated ~80 entries + user-extensible | High signal, low noise; extensible via config and `@blocking` decorator |
| Executor wrappers | Generalized registry | Built-in + config + `@unblocker` decorator for custom wrappers |
| v1 scope | asyncio only | Bounded complexity; architecture supports future framework expansion |

### v1 Scope Boundaries

**In scope:** asyncio blocking detection, transitive call graph, SCC propagation, property/dunder detection, executor wrapper recognition, 80+ built-in blocking functions, text/JSON/SARIF output, incremental caching.

**Out of scope:** trio/curio/anyio, dynamic imports, runtime analysis, cross-package analysis, auto-fix, IDE integration. See [Section 12](#12-known-limitations--scope-boundaries) for the full limitations matrix.

### Error Codes

| Code | What it catches |
|------|----------------|
| STRATO001 | Direct blocking call in async function |
| STRATO002 | Indirect blocking via sync intermediary (the novel case) |
| STRATO003 | Blocking `@property` accessed in async context |
| STRATO004 | Blocking dunder method invoked in async context |

### Performance Targets

| Scenario | Target |
|----------|--------|
| Fresh analysis (500 files) | < 5 seconds |
| Cached analysis (no changes) | < 500 milliseconds |

**Tags**: everyone

---

[async] [analysis]

## 2. Problem Statement & Motivation

### 2.1 The Core Problem

Blocking function calls inside Python async contexts silently destroy concurrency. When an `async def` function calls a blocking operation—such as `time.sleep()`, `requests.get()`, or any synchronous I/O—the entire event loop freezes. No other coroutines can execute until the blocking call completes. The application appears to work correctly in isolation but fails catastrophically under load.

This is an insidious class of bug because:

1. **The code runs without errors.** Python does not raise exceptions or warnings when blocking calls occur in async contexts.
2. **Tests pass in isolation.** A single request completes successfully, masking the concurrency failure.
3. **Production failures are mysterious.** Under concurrent load, the application becomes unresponsive, timeouts cascade, and the root cause is non-obvious.
4. **The bug propagates transitively.** A blocking call buried five levels deep in the call stack poisons every async caller above it.

### 2.2 Why Existing Tools Fail

Current linters detect only **direct** blocking calls. They cannot trace blocking behavior through call chains.

**Example from the Strato README:**

```python
import time

async def handler():
    baz()  # Looks innocent—no linter flags this

def baz():
    time.sleep(1)  # The actual blocking call
```

- **flake8-async** and **ruff ASYNC2XX** flag `time.sleep(1)` if it appears directly in an `async def`, but they do not flag `baz()` when called from `handler()`.
- **PyCG** builds call graphs but does not understand async semantics or blocking behavior.

The result: developers must manually audit every function in the call chain to determine if it eventually blocks. This is infeasible in large codebases.

### 2.3 Detection Case Matrix

| Case | Description | Expected Result | Difficulty |
|------|-------------|-----------------|------------|
| Direct blocking | `async def handler(): time.sleep(1)` | STRATO001 diagnostic | Easy (existing tools catch this) |
| Indirect blocking | `async def handler(): helper()` where `helper()` calls `time.sleep(1)` | STRATO002 diagnostic | Hard (requires call graph + taint analysis) |
| Executor-wrapped | `await loop.run_in_executor(None, time.sleep, 1)` | 0 diagnostics (safe) | Medium (requires executor detection) |
| `asyncio.to_thread` | `await asyncio.to_thread(time.sleep, 1)` | 0 diagnostics (safe) | Medium (requires stdlib knowledge) |
| Sync standalone | `def handler(): time.sleep(1)` (not called from async) | 0 diagnostics (safe) | Easy (context-aware analysis) |
| Sync called from async | `async def handler(): sync_helper()` where `sync_helper()` blocks | STRATO002 diagnostic | Hard (requires cross-context tracing) |
| Blocking property | `@property def data(self): return requests.get(...)` accessed in async | STRATO003 diagnostic | Very hard (implicit call via attribute access) |
| Blocking dunder | `def __str__(self): return requests.get(...).text` with `str(obj)` in async | STRATO004 diagnostic | Very hard (implicit call via operators/builtins) |
| Cross-file blocking | `from utils import slow_util; async def handler(): slow_util()` | STRATO002 diagnostic | Hard (requires project-wide analysis) |
| Deep transitive chain | `handler() -> level_1() -> level_2() -> level_3() -> time.sleep()` | STRATO002 diagnostic with chain_length=5 | Very hard (requires deep graph traversal) |

### 2.4 Tool Comparison

| Tool | Direct Blocking | Indirect Blocking | Properties | Dunders | Cross-File | Deep Chains | Executor Detection |
|------|----------------|-------------------|------------|---------|------------|-------------|-------------------|
| flake8-async | ✓ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ |
| ruff ASYNC2XX | ✓ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ |
| PyCG | N/A (call graph only) | N/A | N/A | N/A | ✓ | ✓ | N/A |
| **Strato** | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |

### 2.5 Motivating Examples

#### 2.5.1 Indirect Blocking (README Example)

```python
import time

async def handler():
    baz()  # No existing tool flags this line

def baz():
    time.sleep(1)  # Blocks the event loop
```

**Strato output:**

```
STRATO002: Async function 'handler' calls blocking function 'baz'
  --> example.py:4:5
   |
 4 |     baz()
   |     ^^^^^ blocking call here
   |
   = note: call chain: handler -> baz -> time.sleep (length: 3)
```

#### 2.5.2 Blocking Property

```python
import requests

class DataFetcher:
    @property
    def data(self):
        return requests.get("https://api.example.com/data").json()

async def process():
    fetcher = DataFetcher()
    result = fetcher.data  # Looks like attribute access, actually blocks
```

**Problem:** The property getter performs synchronous HTTP I/O. The access `fetcher.data` appears to be a simple attribute read but blocks the event loop.

**Strato output:**

```
STRATO003: Async function 'process' accesses blocking property 'DataFetcher.data'
  --> example.py:10:14
   |
10 |     result = fetcher.data
   |              ^^^^^^^^^^^^ blocking property access
   |
   = note: property getter calls requests.get (blocking)
```

#### 2.5.3 Blocking Dunder Method

```python
import requests

class RemoteObject:
    def __str__(self):
        return requests.get("https://api.example.com/status").text

async def log_status(obj):
    print(str(obj))  # Implicit __str__ call blocks
```

**Problem:** The `__str__` method performs network I/O. The `str(obj)` call is implicit and blocks the event loop.

**Strato output:**

```
STRATO004: Async function 'log_status' calls blocking dunder method 'RemoteObject.__str__'
  --> example.py:8:11
   |
 8 |     print(str(obj))
   |           ^^^^^^^^ blocking dunder call
   |
   = note: __str__ method calls requests.get (blocking)
```

#### 2.5.4 Cross-File Blocking

**utils.py:**

```python
import time

def slow_util():
    time.sleep(2)
```

**main.py:**

```python
from utils import slow_util

async def handler():
    slow_util()  # Blocking call hidden in another file
```

**Strato output:**

```
STRATO002: Async function 'handler' calls blocking function 'slow_util'
  --> main.py:4:5
   |
 4 |     slow_util()
   |     ^^^^^^^^^^^ blocking call here
   |
   = note: call chain: handler -> slow_util -> time.sleep (length: 3)
   = note: slow_util defined in utils.py:3
```

#### 2.5.5 Deep Transitive Chain

```python
import time

async def handler():
    level_1()

def level_1():
    level_2()

def level_2():
    level_3()

def level_3():
    time.sleep(1)
```

**Problem:** The blocking call is four levels deep. Manual inspection requires tracing through multiple function definitions.

**Strato output:**

```
STRATO002: Async function 'handler' calls blocking function 'level_1'
  --> example.py:4:5
   |
 4 |     level_1()
   |     ^^^^^^^^^ blocking call here
   |
   = note: call chain: handler -> level_1 -> level_2 -> level_3 -> time.sleep (length: 5)
```

---

## 3. Design Decisions

This section presents the core architectural and implementation choices that define Strato's approach to detecting blocking calls in async Python code. Each decision is structured as a tradeoff analysis: the context that forced a choice, the options considered, the selection made, the rationale for that selection, and the risks that remain. These decisions are presented for expert review — scrutiny from practitioners in Python async, static analysis/PL, and Rust/tooling domains.

### 3.1 Transitive Call Graph vs Pattern Matching [async] [analysis]

**Context:** Existing async linters (flake8-async, ruff ASYNC2XX) use pattern matching to detect direct blocking calls inside async functions — they scan for `time.sleep()`, `requests.get()`, etc. within `async def` bodies. This catches obvious cases but fails when blocking code hides behind intermediate function calls. The motivating example is `async def handler(): helper()` where `helper()` internally calls `time.sleep()` — no existing tool detects this because the blocking call is not syntactically visible at the async boundary. The question: should Strato use the same pattern-matching approach (fast, simple, proven) or build a full call graph to trace blocking through function call chains (complex, novel, higher ambition)?

**Options considered:**

1. **Pattern matching (like existing tools)** — Scan async function bodies for direct calls to known blocking functions. Pros: Simple to implement, fast, well-understood failure modes. Cons: Misses transitive blocking (the core problem Strato aims to solve), provides no value over existing tools.

2. **Full transitive call graph** — Build a project-wide directed graph of function calls, propagate "blocking" status through edges, report when async functions can reach blocking nodes. Pros: Catches hidden blocking through arbitrarily deep call chains, provides unique value. Cons: Complex implementation (module resolution, type inference, SCC decomposition), higher false negative rate (unresolvable calls are skipped), performance risk (graph construction + propagation on large codebases).

3. **Hybrid: pattern matching + one-level call depth** — Check direct calls in async functions, plus check the immediate callees of those functions (one level of indirection). Pros: Catches the most common case (async → sync helper → blocking) without full graph complexity. Cons: Arbitrary depth limit (why stop at one level?), still misses deeper chains, implementation complexity approaches full graph anyway (need symbol resolution).

**Choice:** Full transitive call graph (Option 2).

**Rationale:** The entire value proposition of Strato is catching blocking calls that existing tools miss. Pattern matching (Option 1) provides zero incremental value — users already have flake8-async and ruff. The hybrid approach (Option 3) is a half-measure that still requires most of the infrastructure of a full graph (module resolution, symbol tables, call edge extraction) but arbitrarily limits the analysis depth. The full graph approach is the only option that delivers on the promise: if a blocking call is reachable from an async context through any chain of function calls, Strato finds it. The complexity cost is justified by the unique capability. The design mitigates performance risk through SCC-based propagation (O(V+E), not iterative fixpoint) and incremental caching. The false negative risk (unresolvable calls are skipped) is addressed by the precision policy (Decision 3.2) — better to miss some cases than flood users with false positives.

**Risk:** The call graph approach is unproven in the Python async linting domain. If real-world codebases have too many unresolvable calls (dynamic imports, heavy metaprogramming, complex type flows), the false negative rate could be so high that the tool provides little practical value. The acceptance test suite (Appendix B) is designed to validate coverage on realistic patterns, but production validation will be critical. If the approach fails, there is no fallback — the entire architecture is predicated on the call graph.

---

### 3.2 Precision Policy: Unknown ≠ Not Blocking [analysis]

**Context:** When Strato encounters a call it cannot resolve (e.g., `obj.method()` where `obj`'s type is unknown, or a dynamic import), it must decide: treat the call as potentially blocking (emit a diagnostic) or treat it as unknown (skip silently). This is the classic precision vs. recall tradeoff in static analysis. High recall (flag everything uncertain) maximizes detection but floods users with false positives. High precision (only flag proven cases) minimizes false positives but misses real bugs.

**Options considered:**

1. **Unknown = Blocking (high recall)** — Any unresolvable call is assumed blocking. Emit diagnostics for all uncertain cases. Pros: Catches more real bugs, forces users to annotate or refactor unclear code. Cons: High false positive rate, noisy output, users will ignore or disable the tool.

2. **Unknown = Not Blocking (optimistic)** — Any unresolvable call is assumed safe. Only emit diagnostics for proven blocking calls. Pros: Clean output, no false positives. Cons: Misses real bugs when resolution fails, users may have false confidence.

3. **Unknown = Unknown (high precision)** — Unresolvable calls are neither blocking nor non-blocking — they are skipped. Only emit diagnostics when blocking status is definitively proven. Pros: Zero false positives, users trust the tool's output. Cons: False negatives when resolution fails, tool may miss bugs in complex codebases.

**Choice:** Unknown = Unknown (Option 3).

**Rationale:** Strato is designed for expert review and CI integration. In these contexts, false positives are more damaging than false negatives. A false positive (flagging safe code as blocking) wastes developer time, erodes trust, and leads to tool abandonment. A false negative (missing a real blocking call) is unfortunate but does not actively harm — the bug may be caught by other means (testing, profiling, manual review). The design prioritizes trust: when Strato reports an error, it is confident the error is real. This is reflected in the `BlockingStatus` enum: `Unknown` is a permanent terminal state, never reclassified to `NotBlocking` or `Blocking`. The propagation algorithm (Section 7) explicitly skips `Unknown` nodes — they do not participate in blocking propagation. This policy is consistent with the call graph approach (Decision 3.1): if we can't prove a call is blocking, we don't report it.

**Risk:** The false negative rate could be unacceptably high in codebases with heavy use of dynamic typing, metaprogramming, or third-party libraries without type stubs. If Strato misses too many real bugs, users will perceive it as incomplete or unreliable. The mitigation is twofold: (1) ty integration (Decision 3.4) improves type resolution, reducing the `Unknown` rate; (2) user annotations (`@blocking`, `@non_blocking`) allow manual override when Strato's analysis is insufficient.

---

### 3.3 SCC-Based Propagation vs Iterative Fixpoint [analysis]

**Context:** After the call graph is constructed and initial blocking annotations are applied, the propagation phase must spread "blocking" status through the graph. If function A calls function B, and B is blocking, then A is also blocking (unless the call is wrapped in an executor). The challenge: call graphs contain cycles (mutual recursion). Naive iterative propagation (repeatedly scan the graph until no changes occur) works but is inefficient — it may require multiple passes over the same nodes, and the number of iterations is unbounded in the presence of complex cycles.

**Options considered:**

1. **Iterative fixpoint** — Repeatedly scan all nodes, propagating blocking status from callees to callers, until no node's status changes. Pros: Simple to implement, easy to understand. Cons: O(V × E) worst case (V iterations, each scanning E edges), slow on large graphs with deep cycles, non-deterministic iteration order complicates testing.

2. **SCC-based propagation (Tarjan's algorithm)** — Decompose the call graph into Strongly Connected Components (SCCs), build a condensation graph (DAG of SCCs), topologically sort the condensation, propagate in topological order (leaves first). Pros: O(V + E) single-pass algorithm, deterministic, handles cycles elegantly (entire SCC is treated as a unit). Cons: More complex implementation (Tarjan's algorithm, condensation graph construction), harder to debug.

3. **Worklist algorithm** — Maintain a worklist of nodes whose blocking status has changed. When a node's status changes, add its callers to the worklist. Repeat until worklist is empty. Pros: More efficient than naive iteration (only revisits affected nodes), easier to implement than SCC decomposition. Cons: Still requires multiple passes in the presence of cycles, worst-case complexity is O(V × E), non-deterministic worklist ordering.

**Choice:** SCC-based propagation (Option 2).

**Rationale:** The SCC approach is the only option that guarantees O(V + E) complexity — a single pass over the graph, regardless of cycle structure. This is critical for performance on large codebases (the 500-file benchmark targets sub-5-second fresh analysis). The iterative fixpoint (Option 1) and worklist (Option 3) approaches both degrade to O(V × E) in the presence of deep cycles, which are common in real-world code (e.g., mutually recursive validation functions, circular imports). The implementation complexity of Tarjan's algorithm is justified by the performance guarantee. The deterministic topological ordering also simplifies testing — the propagation order is reproducible, making it easier to write unit tests and debug failures.

**Risk:** The SCC decomposition adds a dependency on a correct implementation of Tarjan's algorithm. If the implementation has bugs (e.g., incorrect handling of self-loops, off-by-one errors in the DFS stack), the propagation results will be wrong, and the bugs will be hard to diagnose. The mitigation is thorough unit testing of the SCC decomposition in isolation and integration tests that validate end-to-end propagation on known-good fixtures.

---

### 3.4 Type Inference Strategy: ty Integration vs Hand-Rolled [analysis] [tooling]

**Context:** To resolve method calls (`obj.method()`), property accesses (`obj.prop`), and dunder invocations (`str(obj)`), Strato needs to infer the type of `obj`. The v1.0 design used a hand-rolled `ScopeBindings` system that tracked simple cases: `self`/`cls` in methods, constructor calls (`x = MyClass()`), and direct imports. This was sufficient for basic call graph construction but missed common patterns like alias tracking (`x = requests.get; x()`) and return type inference (`loader = get_loader(); loader.load()`). Astral's `ty` crate provides full type inference for Python, including these cases, but integrating it requires adopting Salsa (a query-based incremental computation framework) and accepting the complexity of a pre-1.0 external dependency.

**Options considered:**

1. **Hand-rolled ScopeBindings (v1.0 baseline)** — Implement a minimal type inference system that tracks local variable bindings within function scopes. Resolve `self`, `cls`, constructors, and imports. Skip everything else. Pros: Full control, no external dependencies, simple implementation. Cons: Misses common patterns (alias tracking is critical for executor wrapper detection), limited by what we're willing to implement, reinventing the wheel.

2. **ty integration (v1.1)** — Use Astral's `ty_python_semantic` crate for type inference. Wrap it in a `trait TypeResolver` abstraction to isolate Strato from ty's API. Pros: Full type inference including aliases, return types, MRO, attribute resolution; leverages Astral's investment in Python type system; Salsa provides in-run memoization. Cons: Pre-1.0 dependency (API instability, potential panics), Salsa adds complexity, double parse (ruff AST for Strato + ty's internal parse), ty results are not cacheable cross-run (Salsa is in-memory only).

3. **Hybrid: ScopeBindings + ty fallback** — Use ScopeBindings for simple cases, query ty for complex cases. Pros: Graceful degradation if ty fails. Cons: Two type inference systems to maintain, unclear boundary between "simple" and "complex", added complexity for minimal benefit.

**Choice:** ty integration (Option 2), with no ScopeBindings fallback.

**Rationale:** The key capabilities ty provides — alias tracking and return type inference — are critical for Strato's core use cases. Alias tracking is essential for executor wrapper detection: the pattern `safe = sync_to_async(func); await safe()` requires resolving `safe` back to a callable, which ScopeBindings cannot do. Return type inference enables resolving indirect calls like `get_loader().load()`. The risks (API instability, panics, double parse) are mitigated by: (1) pinning to a specific ruff rev, (2) panic isolation (catch panics, downgrade to `NullTypeResolver` per-file), (3) accepting the double parse cost (<100ms for 500 files). The caching limitation (ty results not cached cross-run) is addressed in Decision 3.13.

**Risk:** ty is pre-1.0 and may have bugs, panics, or API changes. If ty fails on a file, Strato degrades gracefully (emit a warning, skip type-dependent analysis for that file). The pinned rev strategy means Strato is frozen at a specific ruff version — upgrading requires a dedicated compatibility spike.

---

### 3.5 Phantom Nodes for External Symbols [analysis]

**Context:** Strato's call graph includes nodes for user-defined functions (parsed from source) and nodes for external blocking functions (stdlib, third-party libraries). How do external symbols like `time.sleep`, `requests.get` become resolvable call graph nodes when their source files are not in the project's source roots?

**Options considered:**

1. **Parse external libraries** — Include stdlib and third-party packages in the source roots. Pros: Uniform treatment. Cons: Massive performance cost (parsing thousands of files), many libraries are C extensions (no Python source), version skew.

2. **Stub files (.pyi)** — Provide hand-written `.pyi` stubs for known blocking functions. Pros: Lightweight. Cons: Must be maintained separately, still requires parsing.

3. **Phantom nodes (pre-seeded from database)** — For every entry in the blocking function database, create a call graph node with no source location. Pros: Zero parsing cost, no version skew, database is the single source of truth. Cons: Only works for functions in the database.

**Choice:** Phantom nodes (Option 3).

**Rationale:** The phantom node approach is the simplest and most performant. It aligns with Strato's precision policy (Decision 3.2): only known blocking functions are tracked. External calls not in the database are treated as `Unknown` and skipped. During Phase 4 initialization, iterate over the blocking database and create a `CallGraphNode` for each entry with `location: None` and `blocking_status: KnownBlocking`. When the call graph builder encounters `time.sleep(1)`, the symbol resolution constructs the qualified name `"time.sleep"`, finds the phantom node, and creates an edge. The phantom node participates in propagation like any other node.

**Risk:** Tightly coupled to the blocking database. If the database is incomplete, calls to unlisted blocking functions will be unresolvable and skipped. The mitigation is a comprehensive database (~80+ entries) and user extensibility (config allows adding custom entries, `@blocking` decorator allows per-function annotation).

---

### 3.6 Generalized Executor Wrapper System [async] [analysis]

**Context:** Python's asyncio provides `loop.run_in_executor()` and `asyncio.to_thread()` to offload blocking work to a thread pool. But real-world codebases use custom wrappers (e.g., `asgiref.sync.sync_to_async`, `anyio.to_thread.run_sync`) and project-specific helpers. Hardcoding every possible wrapper is unmaintainable.

**Options considered:**

1. **Hardcoded list (v1.0 baseline)** — Recognize `run_in_executor` and `to_thread` by name. Pros: Simple. Cons: Not extensible, misses third-party wrappers.

2. **Heuristic detection** — Analyze function bodies to detect patterns like "creates a thread". Pros: Automatic. Cons: Unreliable, doesn't work for C extensions.

3. **Generalized registry (built-in + config + decorator)** — Maintain a registry of known executor wrappers populated from: (a) built-in patterns, (b) user config (`[tool.strato.executor-wrappers]`), (c) `@unblocker` decorator. Each entry specifies which parameter receives the callable being offloaded. Pros: Extensible, covers common cases, user-controllable. Cons: Requires configuration for third-party wrappers.

**Choice:** Generalized registry (Option 3).

**Rationale:** The registry approach balances coverage and extensibility. Built-in patterns cover the most common cases with zero configuration. Config allows adding third-party wrappers without modifying Strato's code. The `@unblocker` decorator allows annotating project-specific wrappers. The call graph builder checks the registry when visiting call expressions; if the callee matches, the edge to the callable argument is marked `in_executor: true`, suppressing blocking propagation.

**Risk:** Users must configure third-party wrappers not in the built-in list. If unconfigured, Strato will flag safe code as blocking (false positive). The registry also depends on ty's ability to resolve the callable argument — if ty can't resolve `safe = sync_to_async(func); await safe()`, the protection is lost.

---

### 3.7 Intervention Strategy for Error Reporting [async] [tooling]

**Context:** When Strato detects a blocking call chain like `async handler() → helper() → db_query() → psycopg2.connect()`, where should it point the diagnostic? The blocking call is in `psycopg2.connect()` (third-party), but the user can't fix that.

**Options considered:**

1. **Async boundary** — Always point to the async function. Pros: Clear context. Cons: May be far from the fix point, less actionable.

2. **First-party deepest** — Point to the deepest first-party function in the chain. Pros: Most actionable (user can fix this function). Cons: May be in a utility far from the async context.

3. **Configurable (default: first-party deepest)** — Allow users to choose via config. Pros: Flexibility. Cons: More complexity.

**Choice:** Configurable, default `first-party-deepest` (Option 3).

**Rationale:** Different teams have different workflows. The default `first-party-deepest` is more actionable — pointing to `helper()` tells the user "fix it here" rather than "figure out where". The full chain is always included in diagnostics for context.

**Risk:** `first-party-deepest` may be confusing if the deepest first-party function is a low-level utility far from the async context. The `async-boundary` strategy is available as a fallback.

---

### 3.8 Blocking Database: Curated List vs Exhaustive [async]

**Context:** Strato needs a database of known blocking functions to seed phantom nodes. Should it be exhaustive (every blocking function in stdlib and popular libraries) or curated?

**Options considered:**

1. **Exhaustive** — Every blocking function. Pros: Maximum coverage. Cons: Massive maintenance burden, high risk of false positives (some functions are blocking but fast, e.g., `os.getpid()`).

2. **Curated (~80 entries)** — Focus on common, impactful blocking functions: I/O, synchronization, sleep/wait, subprocess. Pros: Manageable size, low false positive rate, user-extensible. Cons: Misses less common blocking functions.

3. **Minimal (~20 entries)** — Only the most egregious offenders. Pros: Very low false positive rate. Cons: Incomplete, misses many real bugs.

**Choice:** Curated (~80 entries) (Option 2).

**Rationale:** The curated list covers the most common blocking patterns (`time.sleep`, `requests.*`, `urllib.*`, `socket.*`, `subprocess.*`, `os.read`, `open()`, database drivers). Fast blocking functions (e.g., `os.getpid()`) are excluded — they block for microseconds and are rarely problematic. The database is user-extensible via config and `@blocking` decorator.

**Risk:** May miss blocking functions common in specific domains (e.g., scientific computing). Users must extend via config.

---

### 3.9 Help Text Policy: No Third-Party Recommendations [async] [tooling]

**Context:** Diagnostics include help text suggesting how to fix the issue. Should help text recommend specific third-party libraries?

**Options considered:**

1. **Specific recommendations** — "use `httpx` instead of `requests`". Pros: Actionable. Cons: Strato becomes a kingmaker, recommendations may become outdated.

2. **Generic recommendations** — "use an async HTTP library" or "offload to `asyncio.to_thread()`". Pros: Neutral, timeless. Cons: Less actionable.

3. **No help text** — Only report the problem. Pros: Minimal. Cons: Unhelpful.

**Choice:** Generic recommendations (Option 2).

**Rationale:** Strato is a linting tool, not a library recommendation engine. Help text lists multiple alternatives neutrally (e.g., "Use `aiohttp` or `httpx`") without prescribing one. This avoids implicit endorsement and keeps help text maintainable.

**Risk:** Generic text may be too vague for novice users. Mitigation: include multiple examples without recommending one.

---

### 3.10 Language Choice: Rust [tooling]

**Context:** Strato is a static analysis tool that must parse Python code, build a call graph, and propagate blocking status.

**Options considered:**

1. **Python** — Using `ast` module. Pros: Familiar to target audience. Cons: Performance (Python is slow for graph algorithms), packaging complexity.

2. **Rust** — Using ruff's parser crates. Pros: Performance, ruff parser is the fastest Python parser, strong type safety, single-binary distribution. Cons: Steeper learning curve, smaller contributor pool.

3. **Go** — Pros: Fast, single-binary. Cons: No existing Python parser ecosystem.

**Choice:** Rust (Option 2).

**Rationale:** Performance is critical for CI. The 500-file benchmark targets sub-5-second fresh analysis and sub-500ms cached. Python cannot achieve this for graph algorithms at scale. Rust gives access to ruff's parser crates (fastest Python parser available) and the single-binary distribution model via maturin.

**Risk:** Steeper learning curve limits contributors. Mitigated by clear architecture documentation and modular codebase.

---

### 3.11 Distribution: Dual PyPI Packages [tooling]

**Context:** Strato consists of a Rust binary (analysis tool) and a Python package (`@blocking`/`@non_blocking`/`@unblocker` decorators).

**Options considered:**

1. **Single package** — Binary + annotations together. Pros: Simple. Cons: Large package (~10MB), users who only want annotations must install the binary.

2. **Dual packages** — `strato` (pure Python, annotations only, zero deps) and `strato-cli` (Rust binary via maturin). Pros: Lightweight annotations package, independent versioning. Cons: Two packages to maintain.

3. **Binary-only** — No annotations package. Pros: Simplest. Cons: Poor UX, no type checking for decorators.

**Choice:** Dual packages (Option 2).

**Rationale:** Achieves "zero binary footprint in production." The `strato` package (<10KB, pure Python) can be added to production dependencies with no overhead. `strato-cli` is installed only in dev/CI environments. Independent versioning means annotations (stable API) can evolve separately from the analysis tool (frequent updates).

**Risk:** Users may be confused about which package to install. Mitigated by clear documentation and the rule: "`strato` for annotations, `strato-cli` for the analysis tool."

---

### 3.12 Import Resolution: Scope Limits [analysis] [tooling]

**Context:** Python's import system is extremely flexible — dynamic imports, import hooks, `.pth` files, namespace packages, conditional imports. Strato must decide which import forms to support and which to exclude.

**Options considered:**

1. **Full Python import semantics** — Support everything including dynamic imports, import hooks, `.pth` files. Pros: Maximum compatibility. Cons: Intractable (dynamic imports require runtime execution), extremely complex, slow.

2. **Static imports only (v1.0)** — Absolute, from-import, relative only. Pros: Simple, fast. Cons: Misses star imports and namespace packages (common in real code).

3. **Static imports + pragmatic extensions (v1.1)** — Static imports plus: (a) star imports via literal `__all__` + public names fallback (one level only), (b) basic namespace packages within configured source roots, (c) conditional imports (first branch only). Exclude dynamic imports, import hooks, `.pth` files. Pros: Covers common patterns, manageable complexity. Cons: Still misses exotic cases.

**Choice:** Static imports + pragmatic extensions (Option 3).

**Rationale:** The v1.0 baseline was too restrictive. Star imports and namespace packages are common in real-world code. The v1.1 extensions address the most common gaps without crossing into intractable territory. Unresolvable imports are treated as `Unknown` (Decision 3.2) and skipped silently.

**Risk:** Codebases using `importlib.import_module()` extensively will have many unresolvable imports, leading to false negatives. Mitigated by `@blocking` decorator for manual annotation.

---

### 3.13 Caching Strategy and ty Boundary [tooling]

**Context:** Strato's seven-phase pipeline has cacheable per-file phases (Parse, Resolve) and cross-file phases (Build, Propagate, Report). ty's Salsa database is in-memory only, not serializable.

**Options considered:**

1. **No caching** — Re-run everything. Pros: Simple. Cons: Slow on large codebases.

2. **Per-file caching (parse + imports only)** — Cache Phases 1-3 results keyed by file content hash. Re-run Phases 4-7 every time. Pros: Fast cached runs, simple invalidation, compatible with ty. Cons: Call graph construction + propagation re-run every time (but these are fast at O(V+E)).

3. **Full pipeline caching** — Cache the entire call graph and propagation results. Pros: Maximum performance. Cons: Complex invalidation, incompatible with ty (Salsa is not serializable).

**Choice:** Per-file caching (Option 2).

**Rationale:** The only option compatible with ty. Salsa's in-run memoization handles repeated queries within a single analysis run, but cross-run persistence is not supported. Per-file caching skips parsing (the expensive phase) while accepting that graph construction and propagation are re-run (fast — O(V+E)). Target: <500ms cached on 500 files.

**Risk:** If graph construction or ty queries are slower than expected, cached runs may not meet the <500ms target. Requires performance validation.

---

### 3.14 Determinism Contract [tooling]

**Context:** Strato is designed for CI integration, where non-deterministic output causes flaky builds and erodes trust.

**Options considered:**

1. **Non-deterministic** — Use `HashMap`, accept varying output order. Pros: Simpler, slightly faster. Cons: Flaky CI, hard to test.

2. **Deterministic** — Use `BTreeMap`, explicit sorting for all output-affecting collections. Pros: Reproducible output, reliable CI. Cons: Slightly slower (O(log n) vs O(1)).

**Choice:** Deterministic (Option 2).

**Rationale:** Determinism is a hard requirement for CI. Enforced at multiple levels: (1) `BTreeMap` for output-affecting collections, (2) diagnostics sorted by file path → line → column → error code, (3) blocking path selection uses shortest-path with lexicographic tie-breaking, (4) cache keys use SHA-256 content hashes. The O(log n) overhead of `BTreeMap` is negligible compared to parsing and type inference.

**Risk:** Accidentally using `HashMap` in an output-affecting code path breaks the contract silently. Mitigated by determinism regression tests (run same fixture twice, assert identical output).

---

### 3.15 Failure and Warning Policy [tooling]

**Context:** The analysis pipeline can encounter parse errors, unresolvable imports, ty panics, and I/O errors.

**Options considered:**

1. **Fail fast** — Any error aborts analysis. Pros: Simple. Cons: Unusable on real codebases (most projects have at least one file with issues).

2. **Warnings only (exit 0)** — All errors become warnings. Pros: Permissive. Cons: No signal for serious failures.

3. **Tiered failure policy** — Fatal errors (config errors, I/O errors, all files failed to parse) → non-zero exit. Non-fatal warnings (individual parse errors, unresolvable imports, ty panics) → collected but don't affect exit code. Pros: Balances usability and reliability. Cons: More complex.

**Choice:** Tiered failure policy (Option 3).

**Rationale:** Real-world codebases have files with parse errors (generated code, legacy syntax) and unresolvable imports (optional dependencies). Aborting analysis for one bad file is unacceptable. But if all files fail to parse, the user should be alerted. Exit codes: 0 = no blocking issues, 1 = blocking issues found, 2 = config error, 3 = all files failed to parse. Warnings do NOT affect exit code.

**Risk:** Users must understand which errors are fatal vs. warnings. Mitigated by clear error messages and documentation.

---

### 3.16 Async Scope Boundary: asyncio Only [async]

**Context:** Python has multiple async frameworks: asyncio (stdlib), trio, curio, anyio. Each has its own event loop, task model, and blocking semantics.

**Options considered:**

1. **asyncio only (v1)** — Detect blocking in asyncio contexts only. Pros: Bounded scope, asyncio is the most common framework. Cons: Users of trio/curio/anyio can't use Strato.

2. **All frameworks (v1)** — Support asyncio, trio, curio, anyio. Pros: Maximum coverage. Cons: Complex (each framework has different APIs), high maintenance burden.

3. **Framework-agnostic** — Detect blocking in any `async def`. Don't recognize framework-specific escape hatches. Pros: Simple, works for all frameworks. Cons: High false positive rate (escape hatches not recognized).

**Choice:** asyncio only (Option 1).

**Rationale:** asyncio is the stdlib framework and the most widely used. Supporting multiple frameworks would require tracking each framework's APIs. The architecture supports future expansion — the executor wrapper registry (Decision 3.6) is already generalized, and adding trio/anyio patterns is straightforward in v2.

**Risk:** Users of trio, curio, or anyio cannot use Strato in v1. Mitigated by clear scope documentation and a v2 roadmap.

---

[analysis] [tooling]

## 4. Architecture Overview

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
| `ModuleMap` | Maps module paths to file paths | [Section 5.3](#53-phase-3-resolve-module-resolution) |
| `SymbolTable` | Maps qualified names to definitions | [Section 5.3](#53-phase-3-resolve-module-resolution) |
| `CallGraph` | Directed graph of function call relationships | [Section 6.1](#61-graph-data-model) |
| `BlockingDatabase` | Registry of known blocking functions | [Section 8.1](#81-database-structure) |
| `EscapeHatchRegistry` | Patterns recognized as safe executor wrapping | [Section 9.3](#93-generalized-wrapper-registry) |
| `Diagnostic` | Reported issue with location, chain, and help text | [Section 10.1](#101-error-codes) |
| `AnalysisCache` | Serialized per-file results for incremental analysis | [Section 11.3](#113-caching-strategy) |

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

---

[analysis] [tooling]

## 5. Analysis Pipeline

Strato's analysis runs as a seven-phase pipeline, each phase consuming the outputs of the previous:

```
Discovery → Parse → Resolve → Build → Annotate → Propagate → Report
```

Each phase is designed for isolation, testability, and graceful degradation. Failures in early phases (parse errors, resolution failures) are collected as warnings but do not halt analysis.

### 5.1 Phase 1: Discovery

**Objective:** Enumerate all Python files in the project and classify them as first-party or third-party.

**Steps:**

1. **Load configuration** from `pyproject.toml` under `[tool.strato]`:
   - `source_roots`: explicit list of directories containing first-party code
   - `exclude`: glob patterns for files/directories to skip
   - `blocking_db_path`: path to blocking function database

2. **Auto-detect source roots** if not explicitly configured:
   - Check `[tool.setuptools.packages.find]` for `where` directive
   - Fall back to common layouts: `src/` directory if present, otherwise project root
   - Scan for top-level `__init__.py` files to identify package roots

3. **Build file manifest:**
   - Recursively walk all source roots and collect `.py` files
   - Compute SHA-256 content hash for each file (used for incremental analysis caching)
   - Classify each file:
     - **First-party:** file path is under any configured source root
     - **Third-party:** everything else (site-packages, stdlib, external dependencies)

**Output:** `FileManifest` containing:
- `files: Vec<FileEntry>` where `FileEntry = { path, content_hash, is_first_party }`
- `source_roots: Vec<PathBuf>`

### 5.2 Phase 2: Parse

**Objective:** Parse all Python files into ASTs and extract symbol definitions.

**Steps:**

1. **Parse all files in parallel** using `ruff_python_parser`:
   - Parallelized via `rayon::par_iter()` (embarrassingly parallel workload)
   - Parse errors are **non-fatal**: collected as `AnalysisWarning::ParseError { path, error }`
   - Analysis continues on all successfully parsed files

2. **Extract `FileSymbols`** from each AST:
   - **Function/method definitions:** name, qualified path, `is_async` flag, location
   - **Class definitions:** name, base classes, location
   - **Import statements:** module, imported names, aliases, relative level
   - **Decorators:** applied to functions/classes (e.g., `@blocking`, `@property`)

3. **Parser abstraction layer:**
   - All ruff parser access goes through `trait PythonParser`:
     ```
     trait PythonParser {
         fn parse(&self, source: &str) -> Result<ParsedModule, ParseError>;
     }
     ```
   - Isolates analysis logic from ruff API changes
   - Enables test mocking with synthetic ASTs

**Output:** `ParsedFiles = HashMap<PathBuf, ParsedModule>` where `ParsedModule = { ast, symbols }`

### 5.3 Phase 3: Resolve (Module Resolution)

**Objective:** Map Python import statements to source files and build a global symbol table.

**Risk:** This is the **highest-risk component** of the pipeline. Python's import system is notoriously complex, and edge cases abound.

#### Supported Import Forms

| Import Form | Example | Resolution Strategy |
|-------------|---------|---------------------|
| Absolute | `import foo.bar` | Lookup `foo/bar.py` or `foo/bar/__init__.py` in source roots |
| From-import | `from foo.bar import baz` | Resolve `foo.bar` module, then lookup `baz` symbol |
| Relative | `from . import sibling` | Resolve relative to current module's parent |
| Relative-from | `from ..pkg import mod` | Walk up directory tree by relative level |
| Package `__init__.py` | `import pkg` | Resolve to `pkg/__init__.py` |
| Multi-level | `from a.b.c.d import e` | Iteratively resolve each component |
| `.pyi` stubs | `import foo` | Prefer `foo.pyi` over `foo.py` if present |

#### Unsupported Import Forms

| Import Form | Example | Why Unsupported |
|-------------|---------|-----------------|
| Star imports | `from foo import *` | Partially supported: see algorithm below |
| Conditional imports | `if sys.version_info >= (3, 10): import x` | Best-effort: analyze first branch only |
| Dynamic imports | `importlib.import_module(var)` | Requires runtime information |
| Namespace packages | `import namespace.pkg` | Partially supported: see below |
| `.pth` files | `site-packages/custom.pth` | Requires runtime sys.path manipulation |
| Import hooks | `sys.meta_path.append(...)` | Arbitrary code execution at import time |

#### Star Import Resolution Algorithm

Star imports (`from foo import *`) are resolved with limited scope:

1. Parse the target module (`foo`)
2. Look for a literal `__all__` assignment:
   - If `__all__ = ["a", "b", "c"]` exists, import only those names
   - If `__all__` is dynamically constructed, skip (treat as unresolvable)
3. If no `__all__`, collect all public top-level names (not starting with `_`)
4. **One level only:** do not recursively resolve star imports in the target module

#### Namespace Package Support

Basic support for PEP 420 namespace packages:

- Directories **without** `__init__.py` are treated as namespace packages **only within configured source roots**
- Resolution algorithm checks for regular packages first (with `__init__.py`), then falls back to namespace package lookup
- External namespace packages (e.g., in site-packages) are not supported

#### Resolution Algorithm Pseudocode

```
fn resolve_import(import_stmt, current_module_path, source_roots):
    if import_stmt.is_relative():
        base_path = walk_up(current_module_path, import_stmt.level)
        module_path = base_path.join(import_stmt.module)
    else:
        module_path = import_stmt.module
    
    for root in source_roots:
        candidates = [
            root / module_path.with_suffix(".pyi"),
            root / module_path.with_suffix(".py"),
            root / module_path / "__init__.pyi",
            root / module_path / "__init__.py",
        ]
        for candidate in candidates:
            if candidate.exists():
                return ResolvedModule { path: candidate, kind: File }
        
        # Namespace package fallback
        if (root / module_path).is_dir():
            return ResolvedModule { path: root / module_path, kind: NamespacePackage }
    
    return None  # Unresolved (external or missing)
```

#### Data Structures

- **`ModuleMap`:** `HashMap<ModulePath, FilePath>` — maps Python module paths (e.g., `foo.bar.baz`) to source files
- **`SymbolTable`:** `HashMap<QualifiedName, SymbolDef>` — maps fully qualified names (e.g., `foo.bar.MyClass.method`) to definitions
- **`ResolvedModule`:** `{ path: PathBuf, kind: ModuleKind }` where `ModuleKind = File | Package | NamespacePackage`
- **`SymbolDef`:** `enum { Function, Class, Variable, Import }`

**Output:** `ModuleMap` and `SymbolTable`

### 5.4 Phase 4: Build (Call Graph Construction)

**Objective:** Construct a directed graph of all function calls in the codebase.

This phase walks the AST of every function body and records call edges. Callee resolution uses the symbol table (Phase 3) and type inference (via `ty` crate).

**Detailed algorithm in [Section 6](#6-call-graph--type-resolution).**

**Output:** `CallGraph = { nodes: Vec<CallGraphNode>, edges: Vec<CallEdge> }`

### 5.5 Phase 5: Annotate

**Objective:** Mark known blocking functions using the blocking database and decorator annotations.

**Steps:**

1. **Load blocking database:** JSON file mapping qualified names to blocking status:
   ```json
   {
     "requests.get": "blocking",
     "time.sleep": "blocking",
     "asyncio.sleep": "non_blocking"
   }
   ```

2. **Scan for decorator annotations:**
   - `@blocking` decorator explicitly marks a function as blocking
   - `@non_blocking` decorator explicitly marks a function as non-blocking
   - Decorators override database entries

3. **Scan `.pyi` stub files** for type annotations:
   - Look for `# strato: blocking` comments in stubs
   - Useful for annotating third-party libraries without modifying source

4. **Update `CallGraphNode.blocking_status`:**
   - Set to `KnownBlocking` or `KnownNonBlocking` for annotated nodes
   - Leave as `Unknown` for unannotated nodes

**Output:** Updated `CallGraph` with annotated nodes

### 5.6 Phase 6: Propagate

**Objective:** Propagate blocking status through the call graph to infer blocking behavior of unannotated functions.

If function `f` calls blocking function `g`, then `f` is also blocking (unless the call is wrapped in `asyncio.to_thread` or similar executor).

**Detailed algorithm in [Section 7](#7-blocking-propagation).**

**Output:** Fully annotated `CallGraph` with `PropagatedBlocking` status

### 5.7 Phase 7: Report

**Objective:** Generate violation reports for blocking calls in async contexts.

**Steps:**

1. **Find all async functions** in the call graph
2. **Walk outgoing edges** from each async function
3. **Report violations** where:
   - Edge target has `blocking_status = KnownBlocking | PropagatedBlocking`
   - Edge does **not** have `in_executor = true`
4. **Format reports** with location, call chain, and suggested fixes

**Detailed output format in [Section 10](#10-error-reporting--diagnostics).**

**Output:** `Vec<Violation>`

---

[analysis] [tooling]

## 6. Call Graph & Type Resolution

> **Decision recap:** The call graph is the central data structure for propagation analysis. We chose a node-per-callable model (rather than node-per-statement) to keep graph size manageable and enable efficient traversal. Type resolution was initially hand-rolled (`ScopeBindings`) but replaced with Astral's `ty` crate in v1.1 for improved accuracy. See [Decision 3.4](#34-type-inference-strategy-ty-integration-vs-hand-rolled) for the full tradeoff analysis.

### 6.1 Graph Data Model

#### Nodes

Each callable in the codebase becomes a node in the call graph. Nodes are identified by qualified name and callable kind.

**`CallableKind` enum:**

| Variant | Description | Example |
|---------|-------------|---------|
| `Function` | Top-level or nested function | `def foo(): ...` |
| `AsyncFunction` | Async function | `async def foo(): ...` |
| `Method` | Instance method | `class C: def foo(self): ...` |
| `AsyncMethod` | Async instance method | `class C: async def foo(self): ...` |
| `Property` | Property getter | `@property def foo(self): ...` |
| `ClassMethod` | Class method | `@classmethod def foo(cls): ...` |
| `StaticMethod` | Static method | `@staticmethod def foo(): ...` |
| `Lambda` | Lambda expression | `lambda x: x + 1` |
| `DunderMethod` | Dunder method | `def __init__(self): ...` |

**`CallGraphNode` struct:**

```rust
struct CallGraphNode {
    id: NodeId,
    qualified_name: String,
    kind: CallableKind,
    is_async: bool,
    location: Option<Location>,  // None for phantom nodes
    blocking_status: BlockingStatus,
}
```

**`BlockingStatus` enum:**

| Variant | Semantics |
|---------|-----------|
| `Unknown` | No information about blocking behavior (default state) |
| `KnownBlocking` | Explicitly marked as blocking (database or decorator) |
| `KnownNonBlocking` | Explicitly marked as non-blocking (database or decorator) |
| `PropagatedBlocking` | Inferred as blocking via call graph propagation |

#### Edges

Edges represent call relationships between callables. Each edge has a kind indicating the call mechanism.

**Edge types:**

| Edge Kind | Description | Example |
|-----------|-------------|---------|
| `DirectCall` | Direct function call | `foo()` |
| `MethodCall` | Method invocation | `obj.method()` |
| `PropertyAccess` | Property getter access | `obj.prop` |
| `ImplicitDunder` | Implicit dunder method call | `str(obj)` → `__str__` |
| `SuperCall` | Super method call | `super().method()` |
| `DecoratorCall` | Decorator application | `@decorator def f(): ...` |

**`CallEdge` struct:**

```rust
struct CallEdge {
    from: NodeId,
    to: NodeId,
    kind: EdgeKind,
    location: Location,
    in_executor: bool,  // True if call is wrapped in asyncio.to_thread, etc.
    via: Option<NodeId>,  // For wrapper attribution (e.g., call via decorator)
}
```

### 6.2 Call Edge Visitor

Call graph construction happens in two phases:

**Phase A: Register all callable nodes**

Walk the AST of every file and register a `CallGraphNode` for each function/method definition. This creates the node set before analyzing call edges.

**Phase B: Walk function bodies**

For each function, walk its AST and record call edges using `CallEdgeVisitor`.

#### `CallEdgeVisitor` Pseudocode

```rust
struct CallEdgeVisitor {
    current_function: NodeId,
    call_graph: &mut CallGraph,
    symbol_table: &SymbolTable,
    type_resolver: &dyn TypeResolver,
}

impl Visitor for CallEdgeVisitor {
    fn visit_expr_call(&mut self, call: &ExprCall) {
        let callee = self.resolve_callee(&call.func);
        if let Some(target_node) = callee {
            let in_executor = self.is_wrapped_in_executor(call);
            self.call_graph.add_edge(CallEdge {
                from: self.current_function,
                to: target_node,
                kind: DirectCall,
                location: call.location,
                in_executor,
                via: None,
            });
        }
        // Continue visiting arguments
        walk_expr(self, &call.func);
        for arg in &call.args {
            walk_expr(self, arg);
        }
    }

    fn visit_expr_attribute(&mut self, attr: &ExprAttribute) {
        // Check if this is a property access
        let value_type = self.type_resolver.resolve_type(&attr.value);
        if let Some(prop_node) = self.lookup_property(value_type, &attr.attr) {
            self.call_graph.add_edge(CallEdge {
                from: self.current_function,
                to: prop_node,
                kind: PropertyAccess,
                location: attr.location,
                in_executor: false,
                via: None,
            });
        }
        walk_expr(self, &attr.value);
    }

    fn visit_expr_bin_op(&mut self, binop: &ExprBinOp) {
        // Map operator to dunder method
        let dunder = match binop.op {
            Add => "__add__",
            Sub => "__sub__",
            Mult => "__mul__",
            // ... etc
        };
        let left_type = self.type_resolver.resolve_type(&binop.left);
        if let Some(dunder_node) = self.lookup_dunder(left_type, dunder) {
            self.call_graph.add_edge(CallEdge {
                from: self.current_function,
                to: dunder_node,
                kind: ImplicitDunder,
                location: binop.location,
                in_executor: false,
                via: None,
            });
        }
        walk_expr(self, &binop.left);
        walk_expr(self, &binop.right);
    }

    // ... other visit methods for comparisons, context managers, etc.
}
```

#### Callee Resolution

Determining the target of a call requires resolving the callee expression:

| Callee Expression | Resolution Strategy |
|-------------------|---------------------|
| `Name` (e.g., `foo()`) | Lookup in symbol table via scope chain |
| `Attribute` (e.g., `obj.method()`) | Resolve `obj` type via type inference, then lookup `method` in type's MRO |
| `Subscript` (e.g., `funcs[0]()`) | Skip (requires runtime information) |
| `Lambda` | Create anonymous node for lambda |
| Unresolvable | Skip silently (no edge created) |

**Key principle:** When callee resolution fails, skip the edge rather than guessing. This maintains high precision at the cost of some recall.

### 6.3 Type Resolution via `ty`

#### Evolution: `ScopeBindings` → `ty`

**v1.0 approach:** Hand-rolled `ScopeBindings` struct tracked variable bindings in each scope:

```rust
struct ScopeBindings {
    bindings: HashMap<String, SymbolDef>,
    parent: Option<Box<ScopeBindings>>,
}
```

This worked for simple cases but failed on:
- Aliased imports: `x = requests.get; x()` (lost track of `requests.get`)
- Return type inference: `def factory() -> Foo: ...; factory().method()`
- Attribute resolution: `obj.attr.method()` (no type information for `obj`)

**v1.1 approach:** Replaced with Astral's `ty` crate, which provides full type inference for Python.

#### `TypeResolver` Trait

All type resolution goes through this abstraction:

```rust
trait TypeResolver {
    fn resolve_type(&self, expr: &Expr) -> Option<Type>;
    fn resolve_callee(&self, expr: &Expr) -> Option<NodeId>;
    fn resolve_attribute(&self, base_type: &Type, attr: &str) -> Option<NodeId>;
    fn mro(&self, type_: &Type) -> Vec<Type>;
}
```

**Implementations:**

- `TyTypeResolver`: Uses `ty` crate for full inference
- `NullTypeResolver`: Fallback that always returns `None` (used if `ty` initialization fails)

#### What `ty` Gives Over `ScopeBindings`

| Capability | `ScopeBindings` | `ty` |
|------------|-----------------|------|
| Variable bindings | ✓ | ✓ |
| Import alias tracking | ✗ | ✓ |
| Return type inference | ✗ | ✓ |
| Attribute resolution | ✗ | ✓ |
| Method resolution order (MRO) | ✗ | ✓ |
| Generic type instantiation | ✗ | ✓ |
| Union type narrowing | ✗ | ✓ |

#### `ty` Feature Budget

Strato uses a **subset** of `ty`'s capabilities to balance accuracy and performance:

| Feature | Used? | Why / Why Not |
|---------|-------|---------------|
| Type inference | ✓ | Core requirement for attribute resolution |
| MRO computation | ✓ | Needed for method lookup in inheritance hierarchies |
| Alias tracking | ✓ | Handles `x = foo.bar; x()` patterns |
| Return type inference | ✓ | Handles `factory().method()` patterns |
| Generic instantiation | ✗ | Adds complexity, low ROI for blocking detection |
| Union narrowing | ✗ | Requires control flow analysis, expensive |
| Literal types | ✗ | Not relevant for call graph construction |
| TypedDict | ✗ | Not relevant for call graph construction |

#### Graceful Degradation

`ty` is a best-effort system. When it cannot infer a type:

1. `resolve_type()` returns `None`
2. Caller skips the edge (no panic, no error)
3. Analysis continues with reduced precision

**Example:**

```python
def foo(x):  # x has no type annotation
    x.method()  # ty cannot infer type of x
```

Result: No edge created for `x.method()` call. This is **by design** — we prefer false negatives over false positives.

#### Fallback: `NullTypeResolver`

If `ty` initialization fails (e.g., due to malformed AST or internal error), Strato falls back to `NullTypeResolver`, which always returns `None`. This degrades analysis to name-based resolution only (no attribute or method resolution).

### 6.4 External Symbol Modeling (Phantom Nodes)

External symbols (from third-party libraries or stdlib) are not parsed by Strato. However, they must be represented in the call graph if they are blocking.

> **Decision recap:** See [Decision 3.5](#35-phantom-nodes-for-external-symbols) for why we model externals as phantom nodes rather than parsing third-party source.

#### Phantom Node Creation

External symbols become graph nodes **only if** they appear in the blocking database. These are called **phantom nodes** (nodes without source location).

**Pre-seeding at Phase 4 initialization:**

```rust
for (qualified_name, status) in blocking_database {
    if !call_graph.has_node(qualified_name) {
        call_graph.add_node(CallGraphNode {
            id: next_id(),
            qualified_name,
            kind: Function,  // Assume function unless known otherwise
            is_async: false,
            location: None,  // Phantom node
            blocking_status: status,
        })
    }
}
```

#### Import Binding Rules

When an import statement is encountered, the symbol table is updated with bindings:

| Import Form | Binding Created | Example |
|-------------|-----------------|---------|
| `import foo` | `foo` → `foo` module | `import requests` → `requests` |
| `import foo.bar` | `foo` → `foo` module | `import requests.adapters` → `requests` |
| `from foo import bar` | `bar` → `foo.bar` | `from requests import get` → `get` |
| `from foo import bar as baz` | `baz` → `foo.bar` | `from requests import get as g` → `g` |
| `from foo.bar import baz` | `baz` → `foo.bar.baz` | `from os.path import join` → `join` |

These bindings are used during callee resolution to map names to qualified names, which are then looked up in the call graph.

#### Invisible Externals

Calls to external symbols **not in the blocking database** are invisible to analysis:

```python
import some_library

def foo():
    some_library.unknown_function()  # No edge created (not in DB)
```

This is **by design**: Strato only tracks blocking behavior for known-blocking functions. Unknown externals are assumed non-blocking (optimistic assumption).

### 6.5 Properties & Dunder Methods

#### Property Detection

Property access triggers a call to the property getter:

```python
class Foo:
    @property
    def bar(self):
        time.sleep(1)  # Blocking!

foo = Foo()
x = foo.bar  # This is a call to bar(), not a field access
```

**Detection algorithm:**

1. Encounter `ExprAttribute` (e.g., `foo.bar`)
2. Resolve type of `foo` via `type_resolver.resolve_type()`
3. Lookup `bar` in type's class definition
4. Check if `bar` is decorated with `@property`
5. If yes, create `PropertyAccess` edge to `bar` getter

**Unknown types:** If type resolution fails, no property edge is created (high precision).

#### Dunder Method Mapping

Many Python operations implicitly call dunder methods. Strato models these as `ImplicitDunder` edges.

**Full dunder mapping table:**

| Operation | Dunder Method | Example |
|-----------|---------------|---------|
| `str(x)` | `__str__` | `str(obj)` |
| `repr(x)` | `__repr__` | `repr(obj)` |
| `bool(x)` | `__bool__` | `if obj: ...` |
| `int(x)` | `__int__` | `int(obj)` |
| `float(x)` | `__float__` | `float(obj)` |
| `len(x)` | `__len__` | `len(obj)` |
| `iter(x)` | `__iter__` | `for i in obj: ...` |
| `next(x)` | `__next__` | `next(iterator)` |
| `hash(x)` | `__hash__` | `hash(obj)` |
| `x + y` | `__add__` | `a + b` |
| `x - y` | `__sub__` | `a - b` |
| `x * y` | `__mul__` | `a * b` |
| `x / y` | `__truediv__` | `a / b` |
| `x == y` | `__eq__` | `a == b` |
| `x != y` | `__ne__` | `a != b` |
| `x < y` | `__lt__` | `a < b` |
| `x > y` | `__gt__` | `a > b` |
| `x <= y` | `__le__` | `a <= b` |
| `x >= y` | `__ge__` | `a >= b` |
| `x[k]` | `__getitem__` | `obj[key]` |
| `x[k] = v` | `__setitem__` | `obj[key] = val` |
| `del x[k]` | `__delitem__` | `del obj[key]` |
| `k in x` | `__contains__` | `key in obj` |
| `x(...)` | `__call__` | `callable(args)` |
| `f"{x}"` | `__format__` | `f"Value: {obj}"` |
| `with x` | `__enter__`, `__exit__` | `with obj: ...` |
| `for i in x` | `__iter__`, `__next__` | `for item in obj: ...` |

**Detection algorithm:**

1. Encounter operation (e.g., `ExprBinOp` with `Add`)
2. Map operation to dunder method (`__add__`)
3. Resolve type of left operand via `type_resolver.resolve_type()`
4. Lookup `__add__` in type's MRO via `type_resolver.mro()`
5. If found, create `ImplicitDunder` edge

**Unknown types:** If type resolution fails, no dunder edge is created.

#### Context Manager Detection

`with` statements call `__enter__` and `__exit__`:

```python
with obj:
    ...
```

**Detection algorithm:**

1. Encounter `StmtWith`
2. Resolve type of context expression (`obj`)
3. Lookup `__enter__` and `__exit__` in type's MRO
4. Create two `ImplicitDunder` edges: one to `__enter__`, one to `__exit__`

### 6.6 Qualified Name Conventions

Qualified names uniquely identify callables across the codebase. Strato uses a consistent naming convention:

| Callable Type | Convention | Example |
|---------------|------------|---------|
| Top-level function | `module.path.function_name` | `myapp.utils.helper` |
| Class | `module.path.ClassName` | `myapp.models.User` |
| Instance method | `module.path.ClassName.method_name` | `myapp.models.User.save` |
| Class method | `module.path.ClassName.method_name` | `myapp.models.User.from_dict` |
| Static method | `module.path.ClassName.method_name` | `myapp.models.User.validate` |
| Property getter | `module.path.ClassName.property_name` | `myapp.models.User.full_name` |
| Dunder method | `module.path.ClassName.__dunder__` | `myapp.models.User.__init__` |
| Lambda | `module.path.function_name.<lambda>@line:col` | `myapp.utils.helper.<lambda>@42:15` |
| Nested function | `module.path.outer.inner` | `myapp.utils.outer.inner` |
| External phantom | `library.module.function` | `requests.get` |

#### Module Path Derivation

Module path is derived from file path relative to source root:

```
Algorithm:
1. Strip source root prefix from file path
2. Remove .py extension
3. Replace path separators with dots
4. If file is __init__.py, use parent directory name

Examples:
  src/myapp/utils.py → myapp.utils
  src/myapp/models/__init__.py → myapp.models
  myapp/core/engine.py → myapp.core.engine (if source root is project root)
```

---

[analysis]

## 7. Blocking Propagation

> **Decision recap**: [Decision 3.3](#33-scc-based-propagation-vs-iterative-fixpoint) — Use Tarjan's algorithm for strongly connected component decomposition, followed by topological propagation over the condensation graph. This eliminates cycles and enables single-pass O(V+E) propagation without iterative fixpoint computation.

### 7.1 The Fixpoint Problem

Naive iterative propagation has a fundamental problem: **cycles in the call graph** (mutual recursion):

```python
def foo():
    bar()

def bar():
    foo()    # Cycle! Does foo block? Only if bar blocks, but bar blocks only if foo blocks...
```

A naive fixpoint algorithm would iterate: "Is foo blocking? Check if bar is blocking. Is bar blocking? Check if foo is blocking..." This can require multiple passes over the graph until no changes occur, and the termination condition is not obvious in the presence of complex cycles.

**Solution**: **Strongly Connected Component (SCC) decomposition** followed by **topological propagation**.

The key insight: cycles (mutual recursion) make naive fixpoint iteration risky. SCC decomposition eliminates cycles by collapsing each cycle into a single node, producing a directed acyclic graph (DAG) of SCCs. A topological ordering of this DAG ensures that when we process an SCC, all of its callees have already been processed. This guarantees single-pass propagation with no backtracking.

### 7.2 SCC-Based Algorithm

```
FUNCTION propagate_blocking(graph: &mut CallGraph):

  // Step 1: Decompose into Strongly Connected Components
  // Using Tarjan's algorithm: O(V + E)
  sccs = tarjan_scc(graph)

  // Step 2: Build condensation graph (DAG of SCCs)
  // Each SCC becomes a single node. Edges between SCCs are AGGREGATED
  // per the edge aggregation rule (Section 7.3).
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

### 7.3 Edge Aggregation Rules

When collapsing edges between SCCs during condensation graph construction, the aggregated edge's `all_calls_in_executor` property is computed as follows:

**Rule**: `condensed_edge.all_calls_in_executor = individual_edges.iter().all(|e| e.in_executor)`

When multiple individual call edges exist between functions in SCC_A and SCC_B, the condensed edge is marked `all_calls_in_executor = true` ONLY IF **every** individual edge from any function in SCC_A to any function in SCC_B has `in_executor = true`. If even ONE edge is NOT in an executor, the condensed edge has `all_calls_in_executor = false`, meaning blocking WILL propagate.

**Example**:

```python
# SCC_A contains: foo(), bar()
# SCC_B contains: baz()

def foo():
    await loop.run_in_executor(None, baz)  # in_executor = true

def bar():
    baz()  # in_executor = false (direct call)
```

The condensed edge from SCC_A to SCC_B has `all_calls_in_executor = false` because `bar → baz` is not in an executor. Therefore, if `baz` is blocking, SCC_A becomes blocking.

**Executor edge handling**: Edges marked with `in_executor: true` do not propagate blocking status. The whole purpose of `run_in_executor` (and other executor wrappers) is to offload blocking work to a thread pool, preventing event loop blocking.

**Induced edges from unblockers**: When an `@unblocker` decorator or configured executor wrapper induces an edge (e.g., `sync_to_async(blocking_func)` creates an edge from the wrapper call site to `blocking_func`), that induced edge participates in the same aggregation rule. If the induced edge is marked `in_executor = true` (which it should be, since the wrapper's purpose is to offload), it does not propagate blocking.

### 7.4 Blocking Path Tracing

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

**Multiple call sites between same nodes**: When function A calls function B at multiple locations, `BlockingReason` stores the **first** (smallest line, then column) call site. This is deterministic and ensures consistent output across runs.

**Blocking path selection rules**: The path is computed via BFS from the newly-blocked function toward any `KnownBlocking` callee, selecting the **shortest path**. If multiple shortest paths exist, prefer the path whose root cause has the lexicographically smallest `qualified_name`:

```
FUNCTION select_blocking_reason(func, graph) -> BlockingReason:
  all_paths = find_all_paths_to_blocking_roots(func, graph)

  // Sort by: (path_length ASC, root_cause.qualified_name ASC)
  all_paths.sort_by(|a, b|
    a.len().cmp(&b.len())
      .then(a.root_cause.qualified_name.cmp(&b.root_cause.qualified_name))
  )

  RETURN all_paths[0]  // Shortest path, lexicographically first root on ties
```

### 7.5 Complexity Analysis

| Step | Algorithm | Complexity |
|------|-----------|------------|
| SCC decomposition | Tarjan's | O(V + E) |
| Condensation | Graph contraction | O(V + E) |
| Topological sort | Kahn's/DFS | O(V + E) |
| Propagation | Single pass over DAG | O(V + E) |
| **Total** | **Single pass** | **O(V + E)** |

Where V = number of functions, E = number of call edges.

**This is linear time.** There is no iterative fixpoint — the SCC decomposition eliminates cycles, and the topological ordering ensures each node is processed exactly once. This is critical for performance on large codebases.

---

[async] [tooling]

## 8. Blocking Function Database & Annotations

### 8.1 Database Structure

The blocking database is a registry of functions known to block the event loop. It ships with Strato and is extended via configuration.

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
    UserInput,      // builtins.input
    Other,
}

enum EntrySource {
    BuiltIn,        // Ships with strato
    UserConfig,     // From pyproject.toml [tool.strato.blocking]
    Annotation,     // From @blocking decorator in source code
}
```

### 8.2 Built-In Entries

> **Decision recap ([3.8](#38-blocking-database-curated-list-vs-exhaustive))**: Strato ships a curated database of ~80 entries covering the most common and impactful blocking functions, rather than attempting exhaustive coverage. User extension via config and `@blocking` decorator fills gaps.

Strato ships with 80+ built-in blocking function entries across six categories. The complete database is provided in [Appendix A](#appendix-a-blocking-function-database-complete). Representative examples by category:

| Category | Count | Examples |
|----------|-------|----------|
| **Sleep** | 1 | `time.sleep` |
| **Network I/O** | 27 | `requests.get`, `requests.post`, `urllib.request.urlopen`, `socket.socket.connect`, `http.client.HTTPConnection.request` |
| **File I/O** | 23 | `builtins.open`, `os.read`, `os.write`, `pathlib.Path.read_text`, `glob.glob`, `shutil.copy` |
| **Subprocess** | 8 | `subprocess.run`, `subprocess.call`, `subprocess.Popen.wait`, `os.system` |
| **Database** | 3 | `psycopg2.connect`, `sqlite3.connect`, `pymysql.connect` |
| **User Input** | 1 | `builtins.input` |

**Representative entries with help text**:

| Function | Help Text |
|----------|-----------|
| `time.sleep` | Use `asyncio.sleep()` |
| `requests.get` | Use `aiohttp` or `httpx` |
| `requests.Session.get` | Use `aiohttp.ClientSession` |
| `socket.socket.connect` | Use `asyncio` streams |
| `builtins.open` | Use `aiofiles.open()` |
| `os.read` | Use `aiofiles` or `run_in_executor` |
| `subprocess.run` | Use `asyncio.create_subprocess_exec()` |
| `psycopg2.connect` | Use `asyncpg` |
| `sqlite3.connect` | Use `aiosqlite` |
| `builtins.input` | Use async input library or `run_in_executor` |

> **Decision recap ([3.9](#39-help-text-policy-no-third-party-recommendations))**: Help text suggests async alternatives generically, never recommending one third-party library over another. When multiple options exist, all are listed neutrally (e.g., "Use `aiohttp` or `httpx`").

### 8.3 User Configuration

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

**Configuration semantics**:

- **`add`**: Extends the built-in database with project-specific blocking functions. Each entry requires `name` (qualified name), `help` (suggestion text), and `category` (one of: `sleep`, `network-io`, `file-io`, `subprocess`, `database-io`, `user-input`, `other`).
- **`remove`**: Excludes built-in entries that are false positives for the project (e.g., monkeypatched functions).
- **`blocking_modules`**: Treats all functions in the specified modules as blocking, without enumerating them individually.

### 8.4 Annotations API (@blocking, @non_blocking, @unblocker)

The `strato` Python package provides three decorators for annotating function blocking behavior. The package has zero dependencies and zero runtime impact — decorators are transparent wrappers.

#### Decorator Definitions

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


def unblocker(func: F = None, *, callable_param: int | str = 0) -> F | Callable[[F], F]:
    """Mark a function as an executor wrapper that offloads blocking work.

    Use this to annotate wrapper functions that execute their callable
    argument in a thread pool or other non-blocking context.

    Args:
        callable_param: Which parameter receives the callable to offload.
            Can be a positional index (int) or parameter name (str).
            Default: 0 (first positional argument).

    Usage:
        from strato import unblocker

        @unblocker
        def my_thread_wrapper(func):
            return asyncio.to_thread(func)

        @unblocker(callable_param="target")
        def custom_offload(*, target, timeout=30):
            return background.submit(target, timeout=timeout)
    """
    def decorator(f: F) -> F:
        f.__strato_unblocker__ = True  # type: ignore[attr-defined]
        f.__strato_callable_param__ = callable_param  # type: ignore[attr-defined]
        return f

    if func is not None:
        return decorator(func)
    return decorator
```

> **Decision recap ([3.6](#36-generalized-executor-wrapper-system))**: The `@unblocker` decorator is a v1.1 addition enabling user-defined executor wrappers. It generalizes the hardcoded `run_in_executor`/`to_thread` patterns.

#### Annotation Detection Algorithm

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

      // @unblocker or @unblocker(callable_param=...)
      Name("unblocker") | Call(func=Name("unblocker")):
        IF is_imported_from_strato("unblocker"):
          callable_param = extract_callable_param_arg(decorator)  // Default: 0
          RETURN Some(AnnotationType::Unblocker { callable_param })

  RETURN None
```

**Import resolution**: `is_imported_from_strato()` checks whether the decorator name was imported from the `strato` package, preventing false positives from unrelated decorators with the same name.

### 8.5 Stub File Support (.pyi)

Strato supports `.pyi` stub files for annotating third-party libraries without modifying their source code.

#### Resolution Data Flow

1. **Phase 1 (Discovery)**: The file manifest includes `.pyi` files found in source roots (alongside `.py` files) and `stub_paths` (from config).

2. **Phase 3 (Resolve)**: When both `foo.py` and `foo.pyi` exist:
   - The `.py` file is used for call graph construction (body analysis)
   - The `.pyi` file is used for annotation extraction (`@blocking`/`@non_blocking`/`@unblocker` only)
   - If only `.pyi` exists (no `.py`), it is used solely for annotations (no body analysis possible)

3. **Phase 5 (Annotate)**: `.pyi` files are scanned for decorators. Their annotations override or supplement database entries for the same qualified name.

4. **First-party classification**: `.pyi` files in `stub_paths` are classified as **third-party**. `.pyi` files in source roots follow normal classification.

#### Override Precedence

When a function has multiple sources of blocking information:

1. `@non_blocking` annotation (highest — explicit override)
2. `@blocking` annotation
3. User configuration (`[tool.strato.blocking]`)
4. Built-in database entry (lowest)

#### Example Stub

```python
# stubs/redis.pyi
from strato import blocking

class Redis:
    @blocking
    def get(self, key: str) -> bytes: ...

    @blocking
    def set(self, key: str, value: bytes) -> None: ...

    @blocking
    def delete(self, *keys: str) -> int: ...
```

---

[async] [analysis]

## 9. Escape Hatches & Executor Wrappers

### 9.1 Built-In Patterns

An "escape hatch" is a pattern that correctly offloads a blocking call to a thread pool, making it safe to use in async contexts. Strato recognizes four built-in patterns (asyncio only in v1):

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

**Key property**: When an escape hatch is detected, the callable argument (the function being offloaded) is protected. Blocking status does NOT propagate backward through edges marked `in_executor=true`.

### 9.2 Detection Mechanism

During call edge construction (Phase 4), the visitor checks if the current call expression matches an escape hatch pattern.

#### Pattern Recognition

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

// Syntactic heuristic — no type inference required.
FUNCTION is_likely_event_loop(value: &Expr) -> bool:

  MATCH value:
    // Case 1: Direct call result
    // e.g., asyncio.get_running_loop().run_in_executor(...)
    Call(func=Attribute(value=Name("asyncio"), attr)):
      RETURN attr IN ["get_running_loop", "get_event_loop"]

    // Case 2: Variable previously assigned from asyncio loop getter
    // e.g., loop = asyncio.get_running_loop() ... loop.run_in_executor(...)
    Name(name):
      binding = lookup_assignment_in_scope(name, current_function)
      MATCH binding:
        Assign(value=Call(func=Attribute(value=Name("asyncio"), attr))):
          RETURN attr IN ["get_running_loop", "get_event_loop"]
        _:
          RETURN false

    // Case 3: Anything else — not provably an event loop
    _:
      RETURN false
```

#### Synthetic Edge Rule

When an escape hatch is detected, the **callable argument** is protected. However, passing a callable as an argument (e.g., `run_in_executor(None, time.sleep, 1)`) is NOT a call expression in the AST — it's a `Name` reference. Strato creates a **synthetic call edge** to model the offloading:

```
WHEN is_executor_call(call) is true:

  callable_arg = call.args[get_executor_callable_arg_position(call)]

  MATCH callable_arg:
    // Case 1: Direct name reference — time.sleep, my_func
    Name(name) | Attribute(value, attr):
      callee = resolve_callee(callable_arg)
      IF callee is Some:
        // Create SYNTHETIC edge with in_executor=true
        graph.add_edge(current_function, callee, DirectCall, in_executor=true)

    // Case 2: functools.partial(func, arg1, ...) — unwrap to the underlying callable
    Call(func=Attribute(value=Name("partial"|"functools"), attr="partial"),
         args=[real_func, ...]):
      callee = resolve_callee(real_func)
      IF callee is Some:
        graph.add_edge(current_function, callee, DirectCall, in_executor=true)

    // Case 3: lambda: func(arg1) — walk the lambda body with in_executor_context=true
    Lambda(body):
      in_executor_context = true
      visit(body)  // Any edges found inside are marked in_executor=true
      in_executor_context = false

    // Case 4: Anything else — unresolvable, skip
    _:
      PASS
```

**Key invariant**: The synthetic edge ensures that `time.sleep` (a phantom node with `KnownBlocking`) is connected to the calling function but with `in_executor=true`, so blocking status does NOT propagate backward through this edge.

**Executor scope rule**: Only the CALLABLE ARGUMENT position gets `in_executor=true` protection. In `loop.run_in_executor(executor, func, arg1, arg2)`: arg[0] (executor) is NOT protected, arg[1] (func) IS protected, arg[2..] (data arguments) are NOT protected.

### 9.3 Generalized Wrapper Registry

> **Decision recap ([3.6](#36-generalized-executor-wrapper-system))**: Strato v1.1 generalizes the hardcoded `run_in_executor`/`to_thread` patterns into a configurable registry, enabling user-defined executor wrappers.

```rust
struct EscapeHatchRegistry {
    patterns: Vec<EscapeHatchPattern>,
}

struct EscapeHatchPattern {
    /// Qualified name of the escape function (e.g., "asyncio.to_thread")
    function_name: QualifiedName,
    /// Which argument position contains the callable being offloaded
    /// For run_in_executor: position 1 (0=executor, 1=func)
    /// For to_thread: position 0 (0=func)
    callable_arg_position: usize,
}
```

**Built-in patterns (v1)**:

```rust
vec![
    EscapeHatchPattern { function_name: "asyncio.to_thread", callable_arg_position: 0 },
    // run_in_executor is detected structurally (method on event loop)
    // rather than by qualified name, since the loop variable name varies
]
```

**Note**: `run_in_executor` is detected structurally via `is_likely_event_loop()` rather than by qualified name. This structural detection is a special case outside the registry.

### 9.4 Configuration Schema

Users can add custom escape hatches in `pyproject.toml`:

```toml
[tool.strato.executor-wrappers]
"asgiref.sync.sync_to_async" = { callable_param = 0 }
"anyio.to_thread.run_sync" = { callable_param = 0 }
"myproject.utils.offload" = { callable_param = 0 }
"custom.wrapper" = { callable_param = "func" }  # Keyword argument
```

**Configuration semantics**:

- **Key**: Qualified name of the wrapper function
- **Value**: Object with `callable_param` field — integer (positional index, 0-based) or string (keyword argument name)
- Duplicate keys are rejected (last one wins, with a warning)

**The `@unblocker` decorator** provides an alternative to configuration for first-party wrappers (see [Section 8.4](#84-annotations-api-blocking-non_blocking-unblocker)).

**Precedence**: Annotations take precedence over configuration. If a function has both `@unblocker` and a `[tool.strato.executor-wrappers]` entry, the annotation wins.

---

## 10. Error Reporting & Diagnostics

> **Decision recap:** [Decision 3.7](#37-intervention-strategy-for-error-reporting) established the intervention point strategy (first-party-deepest vs async-boundary) to guide users to the most actionable fix location. [Decision 3.14](#314-determinism-contract) mandates deterministic output ordering for test stability and reproducible CI runs.

[async] [tooling]

### 10.1 Error Codes

Strato emits four error codes, each corresponding to a distinct pattern of blocking call reachability from async contexts:

| Code | Meaning | Severity | Trigger Condition |
|------|---------|----------|-------------------|
| `STRATO001` | Direct blocking call in async function | Error | Async function directly calls a blocking function with no intermediary sync functions |
| `STRATO002` | Indirect blocking call via sync intermediary | Error | Async function calls sync function(s) that transitively reach a blocking function |
| `STRATO003` | Blocking `@property` accessed in async context | Error | Property getter (decorated with `@property`) is accessed and transitively blocks |
| `STRATO004` | Blocking dunder method invoked in async context | Error | Implicit dunder method call (e.g., `str(obj)`, `x + y`) transitively blocks |

#### Message Templates

**STRATO001:**
```
STRATO001: Direct blocking call in async function

  --> {file}:{line}:{column}
   |
{line} | {source_line}
   | {underline} blocks the event loop
   |
   = call chain: {chain}
   = help: {help_text}
```

**STRATO002:**
```
STRATO002: Blocking call reachable from async context

  --> {file}:{line}:{column}
   |
{line} | {source_line}
   | {underline} calls sync chain that blocks the event loop
   |
   = call chain: {chain}
   = help: {help_text}
```

**STRATO003:**
```
STRATO003: Blocking property access in async context

  --> {file}:{line}:{column}
   |
{line} | {source_line}
   | {underline} property getter blocks the event loop
   |
   = call chain: {chain}
   = help: {help_text}
```

**STRATO004:**
```
STRATO004: Blocking dunder method in async context

  --> {file}:{line}:{column}
   |
{line} | {source_line}
   | {underline} implicit dunder call blocks the event loop
   |
   = call chain: {chain}
   = help: {help_text}
```

#### Wrapper Attribution

When a diagnostic fires because an `@unblocker` wrapper could not be resolved (type inference failed to track the alias), the diagnostic includes wrapper attribution:

```
   = note: This call may be wrapped by an @unblocker decorator, but type inference
           could not confirm the wrapper. If this is a false positive, ensure the
           wrapper alias is directly assigned (e.g., `safe = sync_to_async(func)`)
           without intermediate reassignments.
```

This note is appended to the diagnostic message when:
1. The call site is to a name that was assigned from an `@unblocker`-decorated function
2. Type inference (`ty`) could not resolve the alias chain
3. The call was not marked `in_executor` due to resolution failure

### 10.2 Error Code Classification Algorithm

The error code is determined by inspecting the `BlockingReason.chain_links` and the edge kind of the last link in the chain:

```rust
fn classify_error_code(chain: &BlockingReason, graph: &CallGraph) -> ErrorCode {
    // The first link is always from the async function.
    // The last link's callee is the blocking root cause.
    let first_link = &chain.chain_links[0];
    let last_link = chain.chain_links.last().unwrap();

    // Check the edge kind of the last link to the blocking root
    let last_edge_kind = graph.edge_kind(
        &last_link.function_name,
        &last_link.callee_name
    );

    // STRATO003: Property access to a blocking getter
    if last_edge_kind == EdgeKind::PropertyAccess {
        return ErrorCode::STRATO003;
    }

    // STRATO004: Implicit dunder call that blocks
    if last_edge_kind == EdgeKind::ImplicitDunder {
        return ErrorCode::STRATO004;
    }

    // STRATO001 vs STRATO002: Is the blocking call directly in an async function?
    // "Direct" means: chain has exactly 1 link AND the caller is async.
    // That means: async_func directly calls blocking_func with no intermediaries.
    if chain.chain_links.len() == 1 && first_link.is_async {
        return ErrorCode::STRATO001;  // Direct blocking call in async function
    }

    // Otherwise: there are intermediary sync functions between async and blocker
    ErrorCode::STRATO002
}
```

**Classification Examples:**

| Scenario | Chain | Edge Kind | Result |
|----------|-------|-----------|--------|
| `async handler() -> time.sleep()` | 1 link, caller is async | `DirectCall` | **STRATO001** |
| `async handler() -> helper() -> time.sleep()` | 2 links | `DirectCall` | **STRATO002** |
| `async handler() -> loader.data [PropertyAccess] -> requests.get()` | 2+ links | `PropertyAccess` (last edge) | **STRATO003** |
| `async handler() -> str(obj) [ImplicitDunder] -> __str__() -> requests.get()` | 2+ links | `ImplicitDunder` (last edge) | **STRATO004** |

**Key invariants:**
- The first link's `is_async` field is always `true` (the chain starts from an async function)
- The last link's callee is always a `KnownBlocking` node (the blocking root cause)
- Edge kind is checked only for the **last link** (the edge leading to the blocking root)

### 10.3 Intervention Point Strategy

The "intervention point" is the primary location shown in the diagnostic — the place in the user's code where they should make a change. Strato supports two strategies for selecting this location:

#### Strategy: `first-party-deepest` (Default)

Select the **deepest function in first-party code** on the call chain between the async context and the blocking call. This points users to the lowest-level first-party function that could be refactored to be async.

```rust
fn select_intervention_point(
    chain: &[ChainLink],
    strategy: InterventionStrategy
) -> &ChainLink {
    match strategy {
        InterventionStrategy::FirstPartyDeepest => {
            // Walk the chain from the blocking end toward the async end
            // Find the deepest first-party function
            for link in chain.iter().rev() {
                if link.is_first_party {
                    return link;
                }
            }
            // Fallback: if no first-party code on path, use async boundary
            select_async_boundary(chain)
        }
        InterventionStrategy::AsyncBoundary => {
            select_async_boundary(chain)
        }
    }
}

fn select_async_boundary(chain: &[ChainLink]) -> &ChainLink {
    // Find the transition: last async function before sync code that leads to blocking
    for i in 0..chain.len() - 1 {
        if chain[i].is_async && !chain[i + 1].is_async {
            return &chain[i];
        }
    }
    // Fallback: first element
    &chain[0]
}
```

#### Strategy: `async-boundary`

Select the **async-to-sync transition point** — the last async function before the sync call chain that leads to blocking. This points users to the boundary where they should consider offloading the sync work.

#### Example Comparison

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

**`first-party-deepest`** reports at `check_db()` in `db.py`:
```
STRATO002: Blocking call reachable from async context

  --> src/myapp/db.py:15:5
   |
15 |     psycopg2.connect(dsn)
   |     ^^^^^^^^^^^^^^^^^^^^ calls sync chain that blocks the event loop
   |
   = call chain: process() -> validate() -> check_db() -> psycopg2.connect()
   = help: Use `asyncpg` or wrap in `await loop.run_in_executor(None, psycopg2.connect, dsn)`
```

**`async-boundary`** reports at `process()` calling `validate()`:
```
STRATO002: Blocking call reachable from async context

  --> src/myapp/processor.py:8:5
   |
 8 |     validate(data)
   |     ^^^^^^^^^^^^^^ calls sync chain that blocks the event loop
   |
   = call chain: process() -> validate() -> check_db() -> psycopg2.connect()
   = help: Use `asyncpg` or wrap in `await loop.run_in_executor(None, psycopg2.connect, dsn)`
```

#### Tie-Breaking Rules

When the `first-party-deepest` strategy finds **multiple first-party functions at the same depth**, select the one with the lexicographically smallest `qualified_name`. If still tied (same function called from multiple sites), select the call site with the smallest `(line, column)` pair.

### 10.4 Diagnostic Structure

The `Diagnostic` struct is the core data structure for error reporting. It contains all information needed to render a diagnostic in any output format (text, JSON, SARIF).

```rust
/// A single diagnostic emitted by Strato.
struct Diagnostic {
    /// Unique error code (e.g., "STRATO001")
    code: ErrorCode,

    /// Severity level
    severity: Severity,  // Error, Warning

    /// The "intervention point" — where the user should look
    primary_location: Location,

    /// Human-readable message
    message: String,

    /// The call chain from async context to blocking call
    blocking_chain: Vec<ChainLink>,

    /// Which intervention strategy was used
    strategy: InterventionStrategy,

    /// Static suggestion for fixing the issue (from BlockingDatabase).
    help: Option<String>,

    /// Related locations (additional context for the diagnostic)
    related_locations: Vec<RelatedLocation>,

    /// Wrapper attribution note (if applicable)
    wrapper_attribution: Option<String>,
}

/// Source location with range information.
struct Location {
    /// File path (relative to project root, `/`-normalized)
    file: String,
    /// Start line (1-based)
    line: usize,
    /// Start column (0-based, UTF-8 byte offset within line)
    column: usize,
    /// End line (1-based)
    end_line: usize,
    /// End column (0-based, UTF-8 byte offset)
    end_column: usize,
}

/// A related location providing additional context.
struct RelatedLocation {
    location: Location,
    message: String,
}

/// Error code enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ErrorCode {
    STRATO001,
    STRATO002,
    STRATO003,
    STRATO004,
}
```

#### Location Derivation from Ruff AST

Ruff AST nodes provide `TextRange` (byte-offset range from the start of the source file). Conversion to `(line, column)` uses `ruff_source_file::SourceCode` and `ruff_source_file::LineIndex` for O(log n) lookup.

**Which AST span to use:**
- **Function definitions:** Use the `name` identifier range (not the entire `def`)
- **Call sites:** Use the full `ExprCall` range (includes parentheses)
- **Property access:** Use the `Attribute.attr` identifier range
- **Dunder operations:** Use the operator/builtin call range

#### Column Convention (End-to-End)

| Context | Convention |
|---------|-----------|
| Internal (`Location` struct) | 0-based byte offset (matches ruff) |
| Text output display | 1-based column (add 1 when formatting) |
| JSON output | 0-based (matches internal, LSP convention) |
| SARIF output | 1-based column (SARIF spec requires 1-based) |

### 10.5 Related Locations

Related locations provide additional context for diagnostics. They are attached based on the error code and call chain structure.

#### Related Location Rules by Error Code

| Error Code | Related Locations Attached | Purpose |
|------------|---------------------------|---------|
| `STRATO001` | 1. Async function definition<br>2. Blocking root definition (if available) | Show where the async context starts and the blocking root |
| `STRATO002` | 1. Async function definition<br>2. All intermediary sync function definitions<br>3. Blocking root definition (if available) | Show full call chain |
| `STRATO003` | 1. Async function definition<br>2. Property definition<br>3. Blocking root definition (if available) | Show property and blocking root |
| `STRATO004` | 1. Async function definition<br>2. Dunder method definition<br>3. Blocking root definition (if available) | Show dunder method and blocking root |

#### Example: STRATO002 with Related Locations

```python
# src/myapp/handler.py
async def handle_request():          # Related location 1
    process()

# src/myapp/processor.py
def process():                        # Related location 2
    validate()

# src/myapp/validator.py
def validate():                       # Related location 3 (intervention point)
    time.sleep(1)                     # Primary location
```

**Text output:**
```
STRATO002: Blocking call reachable from async context

  --> src/myapp/validator.py:8:5
   |
 8 |     time.sleep(1)
   |     ^^^^^^^^^^^^^ calls sync chain that blocks the event loop
   |
   = call chain: handle_request() -> process() -> validate() -> time.sleep()
   = help: Use `asyncio.sleep()` instead
   |
note: async function `handle_request` defined here
  --> src/myapp/handler.py:3:1
   |
 3 | async def handle_request():
   | ^^^^^^^^^^^^^^^^^^^^^^^^^^^

note: sync function `process` defined here
  --> src/myapp/processor.py:5:1
   |
 5 | def process():
   | ^^^^^^^^^^^^^^

note: blocking function `time.sleep` is a known blocking stdlib function
```

### 10.6 Deterministic Output Rules

For test stability and reproducible CI runs, all outputs must be deterministic.

#### Diagnostic Ordering

When multiple diagnostics are emitted, they are sorted by this key (lexicographic, ascending):

1. **File path** (string comparison, using `/`-normalized relative paths)
2. **Line number** (numeric, ascending)
3. **Column number** (numeric, ascending)
4. **Error code** (string comparison: STRATO001 < STRATO002 < STRATO003 < STRATO004)

```rust
impl Ord for Diagnostic {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.primary_location.file.cmp(&other.primary_location.file)
            .then(self.primary_location.line.cmp(&other.primary_location.line))
            .then(self.primary_location.column.cmp(&other.primary_location.column))
            .then(self.code.cmp(&other.code))
    }
}

// Sort diagnostics before output
diagnostics.sort();
```

#### Blocking Reason Path Selection

When a function has **multiple paths** to different blocking roots, store the **shortest path**. If multiple paths have the same length, select the path whose root cause has the lexicographically smallest `qualified_name`.

#### BTreeMap Usage

All internal maps that affect output order use `BTreeMap` instead of `HashMap`:

```rust
use std::collections::BTreeMap;

type SymbolTable = BTreeMap<String, SymbolDef>;
type ModuleMap = BTreeMap<String, PathBuf>;
type BlockingDatabase = BTreeMap<String, BlockingEntry>;
```

#### Determinism Contract

**Guarantee:** Given the same input files, configuration, and Strato version, the tool produces **byte-for-byte identical output** across runs, regardless of parallel processing order, hash map iteration order, file system traversal order, or operating system.

**Enforcement:** All diagnostic lists sorted before output; all maps use `BTreeMap`; all tie-breaking rules explicitly specified; integration tests include golden output comparison.

### 10.7 Output Formats

Strato supports three output formats:

| Format | Use Case | Audience |
|--------|----------|----------|
| **Text** | Terminal output, CI logs | Developers reading diagnostics directly |
| **JSON** | Programmatic consumption, IDE integration | Tools parsing Strato output |
| **SARIF** | GitHub Code Scanning, IDE integration | Security/quality platforms |

Format is controlled by the `--format` CLI flag:

```bash
strato check --format=text    # Default
strato check --format=json
strato check --format=sarif
```

Full specifications for each output format are provided in [Appendix C: Output Format Specifications](#appendix-c-output-format-specifications).

---

## 11. Supporting Systems

> **Decision recap:** [Decision 3.13](#313-caching-strategy-and-ty-boundary) — file-level caching with SHA-256 content hashing, excluding ty results and propagation from the cache. [Decision 3.11](#311-distribution-dual-pypi-packages) — dual PyPI packages with zero production footprint.

[tooling]

### 11.1 CLI Interface

The `strato` command provides a single primary subcommand for analysis:

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
                             Comma-separated top-level package names.

  --python-version <VER>     Override Python version.
                             Values: 3.7, 3.8, ..., 3.13

  --stats                    Show analysis statistics after run.

  -q, --quiet                Suppress non-diagnostic output.
  -v, --verbose              Show detailed analysis progress.
  --help                     Show help message.
  --version                  Show strato version.
```

#### Binary Naming Convention

| Context | Name |
|---------|------|
| Cargo package name | `strato_cli` |
| Compiled binary | `strato` (via `[[bin]] name = "strato"`) |
| PyPI package name | `strato-cli` |
| User-facing command | `strato check src/` |

#### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | No blocking issues found (some files may have parse warnings) |
| 1 | Blocking issues detected |
| 2 | Configuration error (invalid config, missing source roots) |
| 3 | All files failed to parse (no analysis possible) |

**Parse error policy**: Individual file parse errors are **non-fatal** — strato emits a warning for each unparseable file and continues on remaining files. Exit code 3 is returned **only** when every file fails to parse. Warnings do NOT affect exit code.

#### Example Usage

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

### 11.2 Configuration Loading

Strato loads configuration from `pyproject.toml` under the `[tool.strato]` section. Configuration precedence:

**CLI flags > config file > defaults**

#### Configuration Discovery

The `--config` flag accepts an explicit path to `pyproject.toml`. If omitted, strato walks up the directory tree from the current working directory until it finds a `pyproject.toml` containing a `[tool.strato]` section. If no config is found, all settings use defaults.

#### Configuration Validation

Strato validates the config at startup and exits with code 2 on error:

| Check | Error Message |
|-------|--------------|
| `src_roots` path doesn't exist | `Source root '{path}' does not exist` |
| `src_roots` path has no `.py` files | `Source root '{path}' contains no Python files` |
| Invalid `python_version` | `Invalid python_version: must be '3.7'...'3.13'` |
| Invalid `intervention_strategy` | `Invalid strategy: must be 'first-party-deepest' or 'async-boundary'` |
| `blocking.add` entry missing `name` | `Blocking entry missing required field 'name'` |
| Invalid `category` in blocking entry | `Unknown category '{cat}'. Valid: sleep, network-io, file-io, subprocess, database-io, user-input, other` |

For the complete configuration schema with all available options, see [Appendix D: Configuration Schema](#appendix-d-configuration-schema).

### 11.3 Caching Strategy

Strato implements file-level caching to accelerate incremental analysis. The cache stores per-file parse results and symbol extraction, keyed by SHA-256 content hash.

#### What Is Cached

Each file produces a **per-file analysis result** that can be cached:

```rust
struct CachedFileResult {
    content_hash: [u8; 32],          // SHA-256 of file contents
    symbols: Vec<SymbolDef>,         // Symbols defined in this file
    imports: Vec<ImportStatement>,   // Import statements
    call_edges: Vec<CallEdge>,       // Call edges from functions in this file
    annotations: Vec<AnnotationEntry>, // @blocking, @non_blocking found
}
```

#### What Is NOT Cached

- **Type inference results**: The `ty` crate uses Salsa for incremental computation, which maintains its own in-memory cache. Salsa's cache is not serializable and is designed for single-session use.
- **Call graph structure**: Rebuilt from cached (or fresh) per-file call edges. This is fast (inserting edges into the graph structure).
- **Blocking propagation**: Always rerun. Linear-time O(V+E), completes in milliseconds.

#### Cache Location and Format

Default: `.strato_cache/` in the project root. Binary format using `bincode` for fast serialization.

```
.strato_cache/
├── manifest.bin         # Maps file paths to content hashes
├── files/
│   ├── {hash1}.bin      # CachedFileResult for file 1
│   ├── {hash2}.bin      # CachedFileResult for file 2
│   └── ...
└── version              # Cache format version (invalidate on upgrade)
```

#### Cache Invalidation

| Trigger | Action |
|---------|--------|
| File content changed (hash mismatch) | Re-parse that file |
| File added | Parse new file, merge into call graph |
| File deleted | Remove from call graph, re-propagate |
| Config changed | Full re-analysis |
| strato version changed | Full invalidation |
| `--clear-cache` flag | Delete cache directory |

#### Caching Flow

```
For each file in project:
  1. Compute SHA-256 hash
  2. Check manifest for matching hash
     ├─ Hit:  Load cached CachedFileResult
     └─ Miss: Parse → extract → serialize to cache
  3. Merge file's call edges into project call graph

Always recompute (not cached):
  - Call graph structure (rebuilt from edges)
  - Blocking propagation (SCC + topological)
  - Diagnostics (generated from propagated graph)
```

### 11.4 Performance Targets

| Scenario | Target | Rationale |
|----------|--------|-----------|
| Cached run (no changes) | < 500ms for 500 files | Hash comparison + graph rebuild + propagation |
| Fresh run (first analysis) | < 5s for 500 files | Full parse + resolve + build + propagate |
| Incremental (1 file changed) | < 1s for 500 files | Re-parse 1 file + full graph rebuild |

#### Time Distribution (Fresh Run)

| Phase | Percentage | Optimization Strategy |
|-------|-----------|----------------------|
| Parse | ~60% | Parallel parsing with `rayon` |
| Type queries (ty) | ~25% | Salsa incremental computation |
| Propagation | ~10% | SCC-based linear-time algorithm |
| Reporting | ~5% | Minimal graph traversal |

#### Performance Complicating Factors

Ruff-level performance (200ms for 630 files) is difficult for **fresh** runs because:
1. **Cross-file coordination**: ruff analyzes files independently; Strato merges results for the call graph
2. **Module resolution**: Every import requires filesystem lookups
3. **Graph construction**: Visiting every function body and resolving callees
4. **Propagation**: Even at O(V+E), thousands of functions with tens of thousands of edges

However, **cached runs** approach ruff-level speed because: no parsing, no AST walking, graph rebuild from cached edges is fast, propagation is a single linear pass.

### 11.5 Distribution & Packaging

Strato is distributed as **two separate PyPI packages** to maintain zero binary footprint in production.

#### Package 1: `strato` (Pure Python Annotations)

- **Size**: ~5 KB (pure Python, no dependencies)
- **Runtime cost**: Zero (decorators are identity functions)
- **Install**: `pip install strato`

Contains: `__init__.py`, `_annotations.py` (`@blocking`, `@non_blocking`, `@unblocker`), `py.typed` (PEP 561 marker)

#### Package 2: `strato-cli` (Rust Binary)

- Built with `maturin` using `bindings = "bin"` (binary distribution)
- Platform-specific wheels: Linux x86_64/aarch64, macOS x86_64/aarch64, Windows x86_64
- **Install**: `pip install strato-cli`

#### Zero Binary Footprint Principle

**Production**: `pip install strato` (~5 KB, zero dependencies)
**Development**: `pip install strato[cli]` (includes strato-cli binary)

---

## 12. Known Limitations & Scope Boundaries

**Tags**: everyone

### 12.1 Type System Limitations

Strato's type resolution depends on the `ty` crate for inference. When `ty` cannot determine a type, the call is skipped silently per [Decision 3.2](#32-precision-policy-unknown--not-blocking).

| Limitation | Impact | Mitigation | Status |
|-----------|--------|------------|--------|
| **No-annotation dynamic types** | Variables without type hints or obvious constructors (e.g., `x = get_loader()`) have unknown type. Method calls on `x` are unresolvable. | Add type hints or use `@blocking` decorator. | v1 — by design |
| **Heavily metaprogrammed code** | Classes generated via metaclasses, `type()`, or `__init_subclass__` are invisible to static analysis. | Annotate generated methods with `@blocking`. | v1 — out of scope |
| **Runtime type construction** | `type(name, bases, dict)` creates classes at runtime. Strato cannot resolve calls to methods defined this way. | Avoid runtime class construction in async contexts. | v1 — out of scope |
| **Plugin-based systems** | Frameworks loading callables via entry points or plugin registries are invisible. | Manually annotate plugin callables with `@blocking`. | v1 — out of scope |
| **Generic type parameters** | `T` in `def process(x: T) -> T` is not resolved. Method calls on `x` are unresolvable. | Use concrete types or `@blocking` annotations. | v1 — no generics support |
| **Union types** | `x: Union[A, B]` — Strato does not track which branch is active. | Refactor to avoid unions in async contexts. | v1 — no union tracking |

### 12.2 Import System Limitations

Strato's import resolver handles standard Python import forms but does not support dynamic or runtime-modified imports.

| Limitation | Impact | Mitigation | Status |
|-----------|--------|------------|--------|
| **Dynamic imports** | `importlib.import_module(name)` where `name` is computed at runtime. | Use static imports in async contexts. | v1 — unresolvable |
| **`importlib.import_module` with literal strings** | Even `importlib.import_module("myapp.utils")` is not resolved. | Refactor to `import myapp.utils`. | v1 — not implemented |
| **`.pth` files** | `site-packages/*.pth` files modify `sys.path` at runtime. | Use explicit source roots in config. | v1 — out of scope |
| **Import hooks** | Custom `sys.meta_path` or `sys.path_hooks` importers. | Use standard filesystem-based imports. | v1 — out of scope |
| **Conditional imports (beyond first branch)** | `try: import A; except: import B` — Strato takes the **first branch only**. | Use a single canonical import. | v1 — best-effort |
| **Star imports (transitive)** | `from x import *` is resolved **one level only**. Transitive star imports not followed. | Use explicit imports. | v1 — one level only |
| **Namespace packages (PEP 420)** | Basic support within configured source roots only. External namespace packages not supported. | Add `__init__.py` to all package directories. | v1 — partial |
| **Circular imports** | Symbols registered before bodies walked, but runtime `ImportError` not detected. | Refactor to eliminate circular imports. | v1 — no runtime validation |

### 12.3 Call Graph Limitations

Strato builds a **static call graph** by analyzing function bodies. It cannot resolve calls that depend on runtime state or higher-order function patterns.

| Limitation | Impact | Mitigation | Status |
|-----------|--------|------------|--------|
| **Callbacks passed as arguments** | `def process(callback): callback()` — `callback` unresolvable. | Use `@blocking` on functions that invoke callbacks. | v1 — unresolvable |
| **Higher-order functions returning callables** | `handler = get_handler(); handler()` — unresolvable. | Annotate returned callables with `@blocking`. | v1 — unresolvable |
| **Decorator chains that transform signatures** | Decorators that replace functions with wrappers — Strato analyzes the original function. | Annotate wrappers with `@blocking`. | v1 — original function only |
| **Monkey-patching** | `MyClass.method = some_other_function` — runtime reassignment invisible. | Avoid monkey-patching in async contexts. | v1 — original definition only |
| **Generators and `yield`** | Generator bodies visited, but generator **consumption** (`next(gen())`) does not create call edge to body. | Annotate blocking generators with `@blocking`. | v1 — partial support |
| **`eval()` / `exec()`** | String-based code execution invisible. | Avoid in async contexts. | v1 — out of scope |
| **`getattr()` / `setattr()`** | Dynamic attribute access unresolvable. | Use explicit attribute access. | v1 — unresolvable |
| **`functools.partial`** | Partial application not tracked. | Annotate partial-wrapped functions. | v1 — unresolvable |

### 12.4 Scope Limitations

| Limitation | Impact | Mitigation | Status |
|-----------|--------|------------|--------|
| **asyncio-only** | trio, curio, anyio escape hatches not recognized. Blocking calls wrapped in these are flagged as errors. | Use asyncio, or annotate wrapped functions with `@non_blocking`. | v1 — asyncio only ([Decision 3.16](#316-async-scope-boundary-asyncio-only)) |
| **No runtime analysis** | Cannot detect blocking calls conditionally skipped at runtime. | Use runtime profiling tools to complement. | v1 — static only |
| **No inter-process analysis** | Blocking calls in subprocesses invisible. | Subprocess code is isolated from event loop. | v1 — out of scope |
| **Single-project only** | Does not traverse into installed third-party packages. | Extend blocking database via config. | v1 — first-party focus |
| **No cross-package analysis** | Monorepo packages analyzed separately. | Run Strato on each package independently. | v1 — single-project only |

### 12.5 "Skip Silently" Behavior

Strato follows a **high-precision policy** ([Decision 3.2](#32-precision-policy-unknown--not-blocking)): when it cannot definitively prove a call is blocking, it skips silently. This section documents every such case.

| Case | Behavior | Rationale |
|------|----------|-----------|
| **Unresolvable callee** | `resolve_callee()` returns `None` → no call edge created | Unknown != Blocking |
| **Unknown type → no property/dunder edge** | Type inference fails → property/dunder access not checked | Cannot determine if attribute is `@property` without type |
| **External symbol not in DB** | Third-party symbol without database entry → no phantom node | Only known-blocking third-party functions tracked |
| **Unresolvable import** | Dynamic import, missing `__init__.py` → no binding | Cannot analyze what doesn't exist on filesystem |
| **Star import with parse error** | Target module unparseable → no bindings from star import | Cannot enumerate symbols without parsing |
| **Decorator replacing function** | Original function analyzed, not wrapper | Decorators not executed statically |
| **Callback parameter invoked** | `callback()` inside function → unresolvable | Higher-order requires interprocedural analysis |
| **Conditional import (non-first branch)** | Only first branch analyzed | Best-effort: most likely branch |
| **Transitive star import** | `from x import *` where `x` has `from y import *` → `y`'s symbols invisible | One level only — prevents infinite recursion |
| **Monkey-patched method** | Original method analyzed, not patched replacement | Runtime reassignments invisible to static analysis |
| **`eval()` / `exec()` / `getattr()`** | String-based execution/access invisible | Cannot statically analyze runtime-constructed code |

**User guidance:** If Strato misses a blocking call, users can: (1) add type hints to improve resolution, (2) use `@blocking` to manually annotate, (3) refactor dynamic patterns to explicit calls.

### 12.6 Future Work (v2+)

| Feature | Description | Priority | Complexity |
|---------|-------------|----------|------------|
| **trio/anyio/curio support** | Recognize framework-specific escape hatches | High | Medium |
| **Framework integration** | Django `sync_to_async`, FastAPI thread offloading, Celery task dispatch | High | High |
| **Dynamic analysis integration** | Runtime profiling + static call graph correlation | Medium | High |
| **Auto-fix suggestions** | Generate `asyncio.to_thread` wrapping or suggest async alternatives | Medium | Medium |
| **IDE plugin / LSP server** | Real-time diagnostics in editors | Medium | High |
| **Cross-package analysis** | Traverse into installed third-party packages | Medium | High |
| **Incremental graph updates** | Only rebuild affected subgraph on file change | Low | High |
| **Watch mode** | Continuous analysis on file save | Low | Low |
| **GitHub Action** | Pre-built CI integration: `uses: strato-linter/strato-action@v1` | Low | Low |
| **Full trace visualization** | Interactive HTML report with call graph | Low | Medium |

---

## 13. Open Questions for Reviewers

### For Python Async Experts [async]

**Asyncio scope limitation ([Decision 3.16](#316-async-scope-boundary-asyncio-only)):** We chose to support asyncio only in v1, excluding trio, curio, and anyio. The rationale is that asyncio is the stdlib framework and most widely used, and supporting multiple frameworks would require tracking each framework's distinct APIs for escape hatches. The architecture is designed for future expansion — the executor wrapper registry ([Decision 3.6](#36-generalized-executor-wrapper-system)) is already generalized, and adding trio/anyio patterns is straightforward.

**Question:** Is the asyncio-only scope the right call for v1? Should we attempt trio support from the start, or is the incremental approach (asyncio first, trio in v2) more pragmatic? What are the adoption barriers for teams using trio or anyio if Strato doesn't support their framework?

**Blocking database completeness ([Section 8](#8-blocking-function-database--annotations), [Decision 3.8](#38-blocking-database-curated-list-vs-exhaustive)):** We curated ~80 blocking functions covering I/O, synchronization, sleep/wait, subprocess, and database drivers. Fast blocking functions (e.g., `os.getpid()`) are excluded because they block for microseconds and are rarely problematic. The database is user-extensible via config and `@blocking` decorator.

**Question:** What common blocking functions are we missing? Are there domain-specific blocking patterns (e.g., scientific computing, data processing) that should be in the built-in database? Is the exclusion of fast blocking functions (microsecond-scale) the right policy, or should we flag them with a lower severity?

**Executor wrapper coverage ([Decision 3.6](#36-generalized-executor-wrapper-system)):** We implemented a generalized registry for executor wrappers populated from: (a) built-in patterns (`run_in_executor`, `to_thread`), (b) user config (`[tool.strato.executor-wrappers]`), (c) `@unblocker` decorator. Each entry specifies which parameter receives the callable being offloaded.

**Question:** What common executor wrapper patterns are we missing? Are there third-party libraries (e.g., `asgiref.sync.sync_to_async`, `anyio.to_thread.run_sync`) that should be in the built-in registry? Does the parameter-based model (specify which argument is the callable) cover all real-world wrapper patterns, or are there wrappers that don't fit this model?

**False negative tolerance ([Decision 3.2](#32-precision-policy-unknown--not-blocking)):** We chose "Unknown = Unknown" (high precision) — unresolvable calls are skipped silently. Only emit diagnostics when blocking status is definitively proven. The rationale is that false positives (flagging safe code) are more damaging than false negatives (missing real bugs) in CI and expert review contexts.

**Question:** Is this precision-over-recall policy correct for async bugs? Async bugs can be subtle and hard to debug — should we be more aggressive about flagging uncertain cases, even at the cost of false positives? Would a configurable policy (strict mode vs. permissive mode) be more useful?

---

### For Static Analysis / PL Experts [analysis]

**SCC-based propagation correctness ([Decision 3.3](#33-scc-based-propagation-vs-iterative-fixpoint), [Section 7](#7-blocking-propagation)):** We use Tarjan's algorithm to decompose the call graph into Strongly Connected Components (SCCs), build a condensation graph (DAG of SCCs), topologically sort, and propagate in topological order (leaves first). This guarantees O(V+E) single-pass propagation.

**Question:** Are there edge cases in cycle handling that this approach misses? For example, if an SCC contains both blocking and non-blocking nodes, we mark the entire SCC as blocking — is this sound? What about self-loops (a function calling itself) — are they handled correctly by Tarjan's algorithm as implemented?

**Precision policy ([Decision 3.2](#32-precision-policy-unknown--not-blocking)):** We chose "Unknown = Unknown" — any unresolvable call is neither blocking nor non-blocking, it's skipped. The propagation algorithm explicitly skips `Unknown` nodes — they do not participate in blocking propagation. This is a permanent terminal state, never reclassified.

**Question:** Is the "Unknown = Unknown" policy too aggressive? In practice, does this lead to an unacceptably high false negative rate in codebases with heavy dynamic typing or metaprogramming? Should we have a middle ground (e.g., "Unknown = Warning" — flag uncertain cases with a lower severity)?

**Type inference gaps ([Decision 3.4](#34-type-inference-strategy-ty-integration-vs-hand-rolled)):** We integrated Astral's `ty` crate for type inference, which provides alias tracking, return type inference, MRO, and attribute resolution. We rely on ty to resolve method calls (`obj.method()`), property accesses (`obj.prop`), and dunder invocations (`str(obj)`).

**Question:** What common patterns defeat ty's type inference? Are there cases where ty fails to resolve types that a human reviewer would consider obvious? How does ty handle complex patterns like conditional assignments, exception handlers, or context managers? Should we have a fallback heuristic for common cases where ty fails?

**Call graph completeness ([Section 5](#5-analysis-pipeline), [Decision 3.1](#31-transitive-call-graph-vs-pattern-matching)):** We build a project-wide call graph by extracting call edges from AST nodes (`ExprCall`, `ExprAttribute`, operators, `with` statements, `for` loops). Unresolvable calls (dynamic imports, `getattr()`, monkey patching) are skipped silently.

**Question:** What call patterns do we miss? Are there common Python idioms (e.g., decorators that modify function signatures, metaclasses, descriptor protocol) that defeat call graph construction? Should we attempt heuristic detection for common dynamic patterns (e.g., `getattr(obj, "method_name")()` where `method_name` is a string literal)?

**Phantom node model ([Decision 3.5](#35-phantom-nodes-for-external-symbols)):** For every entry in the blocking function database, we create a call graph node with no source location (`location: None`, `blocking_status: KnownBlocking`). When the call graph builder encounters `time.sleep(1)`, it constructs the qualified name `"time.sleep"`, finds the phantom node, and creates an edge.

**Question:** Is the phantom node model sound? Are there cases where a phantom node could be confused with a user-defined function of the same name (e.g., a project defines its own `time.sleep`)? Should phantom nodes have a distinct type or marker to prevent this? How do we handle overloaded functions (e.g., `open()` is both a builtin and a method on file objects)?

---

### For Rust / Tooling Experts [tooling]

**ty integration risk ([Decision 3.4](#34-type-inference-strategy-ty-integration-vs-hand-rolled)):** We depend on Astral's `ty_python_semantic` crate (pre-1.0) for type inference. This introduces Salsa (a query-based incremental computation framework) and requires pinning to a specific ruff rev. We mitigate API instability by: (1) pinning to a specific rev, (2) panic isolation (catch panics, downgrade to `NullTypeResolver` per-file), (3) accepting the double parse cost (ruff AST for Strato + ty's internal parse).

**Question:** Is the ty integration risk acceptable for a v1 release? Should we wait for ty to reach 1.0, or is the pinned-rev strategy sufficient? What is the maintenance burden of upgrading to new ruff revs — is this a one-time spike or an ongoing tax? Are there alternative type inference libraries (e.g., pyright's type checker, mypy's internals) that would be more stable?

**Performance targets ([Section 11.4](#114-performance-targets)):** We target <5s fresh analysis and <500ms cached on 500 files. The measurement protocol uses `hyperfine` with 3 warmup runs and 5 timed runs (report median). CI tests use a +/-30% tolerance band.

**Question:** Are these targets achievable given the architecture (ruff parsing + ty type inference + SCC propagation)? What are the likely bottlenecks — parsing, type inference, graph construction, or propagation? Should we have separate targets for different project sizes (e.g., <1s for 100 files, <10s for 1000 files)? Is the +/-30% CI tolerance too loose?

**Caching strategy ([Decision 3.13](#313-caching-strategy-and-ty-boundary)):** We cache per-file parse results and imports (Phases 1-3) keyed by file content hash. Call graph construction and propagation (Phases 4-7) re-run every time. ty's Salsa database is in-memory only, not serializable, so ty results are not cached cross-run.

**Question:** Is per-file caching sufficient, or will the lack of cross-run ty caching be a performance bottleneck? Should we explore serializing ty's results (e.g., by extracting only the type information we need and caching that)? Are there other caching strategies (e.g., caching the call graph itself) that would be more effective?

**maturin distribution ([Section 11.5](#115-distribution--packaging)):** We use maturin to build a PyPI wheel for `strato-cli` (the Rust binary). The wheel is platform-specific (separate builds for Linux, macOS, Windows).

**Question:** What is the platform coverage we should target? Should we support ARM (Apple Silicon, ARM Linux) from v1, or is x86_64 sufficient? What is the CI burden of building wheels for multiple platforms? Are there distribution challenges (e.g., glibc version compatibility on Linux) we should anticipate?

**Determinism contract ([Decision 3.14](#314-determinism-contract)):** We enforce determinism at multiple levels: (1) `BTreeMap` for output-affecting collections, (2) diagnostics sorted by file path -> line -> column -> error code, (3) blocking path selection uses shortest-path with lexicographic tie-breaking, (4) cache keys use SHA-256 content hashes.

**Question:** Is `BTreeMap` sufficient to guarantee determinism, or are there other sources of non-determinism (e.g., rayon's parallel iteration order, filesystem traversal order, ty's internal query order)? Should we have a determinism regression test that runs the same fixture multiple times and asserts identical output? What is the performance cost of determinism — is the O(log n) overhead of `BTreeMap` negligible, or does it add up at scale?

---

### For Everyone

**Overall scope ([Decision 3.1](#31-transitive-call-graph-vs-pattern-matching), [Section 12](#12-known-limitations--scope-boundaries)):** Strato v1 aims to detect blocking calls in asyncio code through transitive call graph analysis. Known limitations include: no type inference for complex patterns, no dynamic dispatch, asyncio-only, first-party focus, no cross-package analysis.

**Question:** Is the v1 scope too ambitious, or not ambitious enough? Should we cut features to ship faster (e.g., drop ty integration, drop executor wrapper detection), or should we expand scope (e.g., add trio support, add autofix suggestions)? What is the minimum viable feature set that would make Strato useful in production?

**Error reporting UX ([Decision 3.7](#37-intervention-strategy-for-error-reporting)):** We default to `first-party-deepest` intervention strategy — point the diagnostic to the deepest first-party function in the blocking call chain. The rationale is that this is the most actionable location (user can fix this function). The full chain is always included in diagnostics for context.

**Question:** Is `first-party-deepest` the right default? Would `async-boundary` (always point to the async function) be more intuitive for users? Should we provide both locations (primary + secondary) in the diagnostic? How do we handle cases where the entire chain is third-party code (e.g., `async def handler(): requests.get(...)`) — should we fall back to the async boundary?

**Adoption barriers:** Strato requires: (1) installing `strato-cli` (Rust binary via PyPI), (2) optionally installing `strato` (Python annotations package), (3) running `strato check src/` in CI, (4) configuring `pyproject.toml` for custom blocking functions or executor wrappers.

**Question:** What would prevent you from using this tool? Is the Rust binary a barrier (e.g., platform compatibility, binary size, security concerns)? Is the configuration burden too high? Would you trust a pre-1.0 tool in CI, or would you wait for 1.0? What documentation or examples would you need to adopt Strato?

**Tags**: everyone

---

## Appendix A: Blocking Function Database (Complete)

This appendix lists every built-in entry in Strato's blocking function database. Users can extend this via `[tool.strato.blocking]` configuration ([Section 8.3](#83-user-configuration)) or `@blocking` decorator ([Section 8.4](#84-annotations-api-blocking-non_blocking-unblocker)).

### Sleep

| Qualified Name | Help Text | Notes |
|----------------|-----------|-------|
| `time.sleep` | Use `asyncio.sleep()` | Blocks the event loop for the specified duration |

### Network I/O

| Qualified Name | Help Text | Notes |
|----------------|-----------|-------|
| `requests.get` | Use `aiohttp` or `httpx` | Synchronous HTTP GET request |
| `requests.post` | Use `aiohttp` or `httpx` | Synchronous HTTP POST request |
| `requests.put` | Use `aiohttp` or `httpx` | Synchronous HTTP PUT request |
| `requests.delete` | Use `aiohttp` or `httpx` | Synchronous HTTP DELETE request |
| `requests.patch` | Use `aiohttp` or `httpx` | Synchronous HTTP PATCH request |
| `requests.head` | Use `aiohttp` or `httpx` | Synchronous HTTP HEAD request |
| `requests.options` | Use `aiohttp` or `httpx` | Synchronous HTTP OPTIONS request |
| `requests.request` | Use `aiohttp` or `httpx` | Generic synchronous HTTP request |
| `requests.Session.get` | Use `aiohttp.ClientSession` | Session-based HTTP GET |
| `requests.Session.post` | Use `aiohttp.ClientSession` | Session-based HTTP POST |
| `requests.Session.put` | Use `aiohttp.ClientSession` | Session-based HTTP PUT |
| `requests.Session.delete` | Use `aiohttp.ClientSession` | Session-based HTTP DELETE |
| `requests.Session.patch` | Use `aiohttp.ClientSession` | Session-based HTTP PATCH |
| `requests.Session.head` | Use `aiohttp.ClientSession` | Session-based HTTP HEAD |
| `requests.Session.options` | Use `aiohttp.ClientSession` | Session-based HTTP OPTIONS |
| `requests.Session.request` | Use `aiohttp.ClientSession` | Generic session-based HTTP request |
| `requests.Session.send` | Use `aiohttp.ClientSession` | Send prepared request via session |
| `urllib.request.urlopen` | Use `aiohttp` | Opens URL and reads response synchronously |
| `http.client.HTTPConnection.request` | Use `aiohttp` | Low-level HTTP connection request |
| `http.client.HTTPSConnection.request` | Use `aiohttp` | Low-level HTTPS connection request |
| `socket.socket.connect` | Use `asyncio` streams | Establishes socket connection |
| `socket.socket.recv` | Use `asyncio` streams | Receives data from socket |
| `socket.socket.send` | Use `asyncio` streams | Sends data through socket |
| `socket.socket.accept` | Use `asyncio.start_server()` | Accepts incoming socket connection |
| `socket.socket.sendall` | Use `asyncio` streams | Sends all data through socket |
| `socket.socket.recvfrom` | Use `asyncio` datagram | Receives data from datagram socket |
| `socket.create_connection` | Use `asyncio.open_connection()` | Creates and connects socket |

### File I/O

| Qualified Name | Help Text | Notes |
|----------------|-----------|-------|
| `builtins.open` | Use `aiofiles.open()` | Opens file for reading or writing |
| `io.open` | Use `aiofiles.open()` | Alternative file opening interface |
| `os.read` | Use `aiofiles` or `run_in_executor` | Low-level file descriptor read |
| `os.write` | Use `aiofiles` or `run_in_executor` | Low-level file descriptor write |
| `os.fdopen` | Use `aiofiles` | Opens file descriptor as file object |
| `pathlib.Path.read_text` | Use `aiofiles` | Reads entire file as text |
| `pathlib.Path.read_bytes` | Use `aiofiles` | Reads entire file as bytes |
| `pathlib.Path.write_text` | Use `aiofiles` | Writes text to file |
| `pathlib.Path.write_bytes` | Use `aiofiles` | Writes bytes to file |
| `os.listdir` | Use `run_in_executor` | Lists directory contents |
| `os.scandir` | Use `run_in_executor` | Scans directory with detailed info |
| `os.stat` | Use `run_in_executor` | Gets file status information |
| `os.path.exists` | Use `run_in_executor` | Checks if path exists |
| `os.path.isfile` | Use `run_in_executor` | Checks if path is a file |
| `os.path.isdir` | Use `run_in_executor` | Checks if path is a directory |
| `glob.glob` | Use `run_in_executor` | Finds files matching pattern |
| `glob.iglob` | Use `run_in_executor` | Iterator for files matching pattern |
| `shutil.copy` | Use `run_in_executor` | Copies file |
| `shutil.move` | Use `run_in_executor` | Moves file or directory |
| `shutil.rmtree` | Use `run_in_executor` | Recursively removes directory tree |

### Subprocess

| Qualified Name | Help Text | Notes |
|----------------|-----------|-------|
| `subprocess.run` | Use `asyncio.create_subprocess_exec()` | Runs command and waits for completion |
| `subprocess.call` | Use `asyncio.create_subprocess_exec()` | Runs command and returns exit code |
| `subprocess.check_call` | Use `asyncio.create_subprocess_exec()` | Runs command, raises on non-zero exit |
| `subprocess.check_output` | Use `asyncio.create_subprocess_exec()` | Runs command and captures output |
| `subprocess.Popen.wait` | Use `asyncio.create_subprocess_exec()` | Waits for subprocess to complete |
| `subprocess.Popen.communicate` | Use `asyncio.create_subprocess_exec()` | Sends input and reads output from subprocess |
| `os.system` | Use `asyncio.create_subprocess_shell()` | Executes shell command |
| `os.popen` | Use `asyncio.create_subprocess_shell()` | Opens pipe to/from shell command |

### Database

| Qualified Name | Help Text | Notes |
|----------------|-----------|-------|
| `psycopg2.connect` | Use `asyncpg` | Establishes PostgreSQL connection |
| `sqlite3.connect` | Use `aiosqlite` | Establishes SQLite connection |
| `pymysql.connect` | Use `aiomysql` | Establishes MySQL connection |

### User Input

| Qualified Name | Help Text | Notes |
|----------------|-----------|-------|
| `builtins.input` | Use async input library or `run_in_executor` | Waits for user input from stdin |

---

## Appendix B: Acceptance Test Cases

### A1: Direct Blocking in Async (STRATO001)

**Code:**

```python
import time

async def handler():
    time.sleep(1)
```

**Expected:**
- 1 diagnostic
- Error code: STRATO001
- Message: "Direct blocking call to 'time.sleep' in async function 'handler'"

---

### A2: Indirect Blocking via Sync Intermediary (STRATO002)

**Code:**

```python
import time

async def handler():
    helper()

def helper():
    time.sleep(1)
```

**Expected:**
- 1 diagnostic
- Error code: STRATO002
- Message: "Async function 'handler' calls blocking function 'helper'"
- Chain length: 3 (handler -> helper -> time.sleep)

---

### A3: Executor Wrapping is Safe

**Code:**

```python
import asyncio
import time

async def handler():
    loop = asyncio.get_event_loop()
    await loop.run_in_executor(None, time.sleep, 1)
```

**Expected:**
- 0 diagnostics
- Executor wrapping offloads blocking call to thread pool

---

### A4: `asyncio.to_thread` is Safe

**Code:**

```python
import asyncio
import time

async def handler():
    await asyncio.to_thread(time.sleep, 1)
```

**Expected:**
- 0 diagnostics
- `asyncio.to_thread` is a recognized executor wrapper

---

### A5: Sync-Only Code is Safe

**Code:**

```python
import time

def handler():
    time.sleep(1)
```

**Expected:**
- 0 diagnostics
- `handler` is not async and not called from async context

---

### A6: `@blocking` Decorator

**Code:**

```python
import time
from strato import blocking

@blocking
def custom_slow():
    pass

async def handler():
    custom_slow()
```

**Expected:**
- 1 diagnostic
- Error code: STRATO002
- Message: "Async function 'handler' calls blocking function 'custom_slow'"
- `@blocking` decorator marks function as blocking regardless of implementation

---

### A7: `@non_blocking` Override

**Code:**

```python
import time
from strato import non_blocking

@non_blocking
def actually_safe():
    time.sleep(1)

async def handler():
    actually_safe()
```

**Expected:**
- 0 diagnostics
- `@non_blocking` decorator overrides blocking detection

---

### A8: Blocking Property (STRATO003)

**Code:**

```python
import requests

class DataFetcher:
    @property
    def data(self):
        return requests.get("https://api.example.com/data").json()

async def handler():
    fetcher = DataFetcher()
    result = fetcher.data
```

**Expected:**
- 1 diagnostic
- Error code: STRATO003
- Message: "Async function 'handler' accesses blocking property 'DataFetcher.data'"

---

### A9: Blocking Dunder (STRATO004)

**Code:**

```python
import requests

class RemoteObject:
    def __str__(self):
        return requests.get("https://api.example.com/status").text

async def handler():
    obj = RemoteObject()
    print(str(obj))
```

**Expected:**
- 1 diagnostic
- Error code: STRATO004
- Message: "Async function 'handler' calls blocking dunder method 'RemoteObject.__str__'"

---

### A10: Cross-File Detection

**utils.py:**

```python
import time

def slow_util():
    time.sleep(1)
```

**main.py:**

```python
from utils import slow_util

async def handler():
    slow_util()
```

**Expected:**
- 1 diagnostic in main.py
- Error code: STRATO002
- Message: "Async function 'handler' calls blocking function 'slow_util'"
- Related location: utils.py:3 (definition of slow_util)

---

### A11: Deep Transitive Chain

**Code:**

```python
import time

async def handler():
    level_1()

def level_1():
    level_2()

def level_2():
    level_3()

def level_3():
    time.sleep(1)
```

**Expected:**
- 1 diagnostic
- Error code: STRATO002
- Chain length: 5 (handler -> level_1 -> level_2 -> level_3 -> time.sleep)

---

### A12: Multiple Async Callers

**Code:**

```python
import time

def helper():
    time.sleep(1)

async def handler_a():
    helper()

async def handler_b():
    helper()
```

**Expected:**
- 2 diagnostics
- Both `handler_a` and `handler_b` flagged for calling blocking `helper`

---

### A13: Mixed Safe and Unsafe

**Code:**

```python
import asyncio
import time

def helper():
    time.sleep(1)

async def safe_caller():
    await asyncio.to_thread(helper)

async def unsafe_caller():
    helper()
```

**Expected:**
- 1 diagnostic
- Only `unsafe_caller` flagged
- `safe_caller` uses executor wrapper (safe)

---

### A14: @unblocker Basic

**Code:**

```python
import time
from strato import unblocker

@unblocker
def my_offload(func):
    return func()

async def safe_handler():
    my_offload(lambda: time.sleep(1))

async def unsafe_handler():
    time.sleep(1)
```

**Expected:**
- 1 diagnostic
- Only `unsafe_handler` flagged
- `my_offload` is recognized as executor wrapper via `@unblocker`

---

### A15: Executor Wrapper Config

**pyproject.toml:**

```toml
[tool.strato.executor-wrappers]
"mylib.offload" = true
```

**Code:**

```python
import time
from mylib import offload

async def handler():
    offload(time.sleep, 1)
```

**Expected:**
- 0 diagnostics
- `mylib.offload` configured as executor wrapper

---

### A16: Star Import

**module_a.py:**

```python
import time

def blocking_func():
    time.sleep(1)
```

**main.py:**

```python
from module_a import *

async def handler():
    blocking_func()
```

**Expected:**
- 1 diagnostic
- Error code: STRATO002
- Star import resolved correctly

---

### A17: Namespace Package

**Directory structure:**

```
project/
  namespace_pkg/  # No __init__.py
    module.py
  main.py
```

**namespace_pkg/module.py:**

```python
import time

def blocking_func():
    time.sleep(1)
```

**main.py:**

```python
from namespace_pkg.module import blocking_func

async def handler():
    blocking_func()
```

**Expected:**
- 1 diagnostic
- Namespace package (directory without `__init__.py`) resolved correctly

---

### A18: Related Locations

**Code:**

```python
import time

def helper():
    time.sleep(1)

async def handler():
    helper()
```

**Expected JSON output:**

```json
{
  "diagnostics": [
    {
      "code": "STRATO002",
      "message": "Async function 'handler' calls blocking function 'helper'",
      "location": {
        "file": "example.py",
        "line": 7,
        "column": 5
      },
      "related_locations": [
        {
          "file": "example.py",
          "line": 3,
          "column": 1,
          "message": "helper defined here"
        },
        {
          "file": "example.py",
          "line": 4,
          "column": 5,
          "message": "blocking call: time.sleep"
        }
      ]
    }
  ]
}
```

---

### A19: Parse Warnings

**valid.py:**

```python
import time

async def handler():
    time.sleep(1)
```

**invalid.py:**

```python
def broken(
    # Missing closing parenthesis
```

**Expected:**
- 1 diagnostic from valid.py (STRATO001)
- 1 warning: "Failed to parse invalid.py: syntax error"
- Analysis continues despite parse failure

---

## Appendix C: Output Format Specifications

### C.1 Text Format

The text format is the default human-readable output, providing compiler-style diagnostics with source context.

**Format Specification:**

```
<CODE>: <MESSAGE>

  --> <FILE>:<LINE>:<COLUMN>
   |
<LINE_NUM> | <SOURCE_LINE>
   | <UNDERLINE_WITH_MESSAGE>
   |
   = chain: <FUNCTION_1> -> <FUNCTION_2> -> ... -> <BLOCKING_CALL>
   = help: <REMEDIATION_ADVICE>
```

**Example (A2 Test Case):**

```
STRATO002: Async function 'handler' calls blocking function 'helper'

  --> example.py:7:5
   |
 7 |     helper()
   |     ^^^^^^^^ calls blocking function
   |
   = chain: handler -> helper -> time.sleep
   = help: Wrap in `await asyncio.to_thread(...)` or use async alternative

Found 1 blocking issue in 1 file (2 functions analyzed)
```

### C.2 JSON Format

Machine-readable structured output for programmatic consumption and CI integration.

**Schema Definition:**

```json
{
  "version": "1.0",
  "diagnostics": [
    {
      "code": "string (STRATO001-STRATO004)",
      "severity": "string (error | warning)",
      "message": "string",
      "primary_location": {
        "file": "string (relative path)",
        "line": "integer (1-indexed)",
        "column": "integer (1-indexed)",
        "end_line": "integer (1-indexed, optional)",
        "end_column": "integer (1-indexed, optional)"
      },
      "related_locations": [
        {
          "file": "string",
          "line": "integer",
          "column": "integer",
          "message": "string"
        }
      ],
      "chain": [
        {
          "function": "string (fully qualified name)",
          "file": "string | null",
          "line": "integer | null",
          "is_async": "boolean",
          "is_first_party": "boolean"
        }
      ],
      "help": "string",
      "intervention_strategy": "string"
    }
  ],
  "warnings": [
    {
      "message": "string",
      "file": "string (optional)"
    }
  ],
  "stats": {
    "files_analyzed": "integer",
    "functions_analyzed": "integer",
    "call_graph_nodes": "integer",
    "call_graph_edges": "integer",
    "blocking_functions_found": "integer",
    "analysis_time_ms": "integer"
  }
}
```

**Required Fields:** `version`, `diagnostics`, `stats` always present. Within each diagnostic: `code`, `severity`, `message`, `primary_location` required. `related_locations`, `chain`, `help` optional.

**Example (A2 Test Case):**

```json
{
  "version": "1.0",
  "diagnostics": [
    {
      "code": "STRATO002",
      "severity": "error",
      "message": "Async function 'handler' calls blocking function 'helper'",
      "primary_location": {
        "file": "example.py",
        "line": 7,
        "column": 5,
        "end_line": 7,
        "end_column": 13
      },
      "related_locations": [
        {
          "file": "example.py",
          "line": 3,
          "column": 1,
          "message": "helper defined here"
        },
        {
          "file": "example.py",
          "line": 4,
          "column": 5,
          "message": "blocking call: time.sleep"
        }
      ],
      "chain": [
        {
          "function": "handler",
          "file": "example.py",
          "line": 6,
          "is_async": true,
          "is_first_party": true
        },
        {
          "function": "helper",
          "file": "example.py",
          "line": 3,
          "is_async": false,
          "is_first_party": true
        },
        {
          "function": "time.sleep",
          "file": null,
          "line": null,
          "is_async": false,
          "is_first_party": false
        }
      ],
      "help": "Wrap in `await asyncio.to_thread(...)` or use async alternative",
      "intervention_strategy": "first-party-deepest"
    }
  ],
  "warnings": [],
  "stats": {
    "files_analyzed": 1,
    "functions_analyzed": 2,
    "call_graph_nodes": 2,
    "call_graph_edges": 1,
    "blocking_functions_found": 1,
    "analysis_time_ms": 15
  }
}
```

**Ordering:** `diagnostics` sorted by file path, line, column. `chain` ordered from async entry to blocking call. Phantom node locations serialize as `null`.

### C.3 SARIF v2.1.0 Format

Compatible with GitHub Code Scanning, Azure DevOps, and CI/CD platforms supporting SARIF v2.1.0.

**Mapping to SARIF:**

| Strato Concept | SARIF Element |
|----------------|---------------|
| `primary_location` | `locations[0].physicalLocation` |
| `related_locations` | `relatedLocations` array |
| `chain` | `codeFlows[0].threadFlows[0].locations` |
| `severity` | `level` (error / warning / note) |

**Example (A2 Test Case):**

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
              "shortDescription": { "text": "Direct blocking call in async function" }
            },
            {
              "id": "STRATO002",
              "name": "IndirectBlockingInAsync",
              "shortDescription": { "text": "Blocking call reachable from async context via sync intermediary" }
            },
            {
              "id": "STRATO003",
              "name": "BlockingPropertyInAsync",
              "shortDescription": { "text": "Blocking @property getter accessed in async context" }
            },
            {
              "id": "STRATO004",
              "name": "BlockingDunderInAsync",
              "shortDescription": { "text": "Blocking dunder method invoked in async context" }
            }
          ]
        }
      },
      "results": [
        {
          "ruleId": "STRATO002",
          "level": "error",
          "message": { "text": "Async function 'handler' calls blocking function 'helper'" },
          "locations": [
            {
              "physicalLocation": {
                "artifactLocation": { "uri": "example.py" },
                "region": { "startLine": 7, "startColumn": 5, "endLine": 7, "endColumn": 13 }
              }
            }
          ],
          "relatedLocations": [
            {
              "id": 0,
              "message": { "text": "async context entry point" },
              "physicalLocation": {
                "artifactLocation": { "uri": "example.py" },
                "region": { "startLine": 6 }
              }
            },
            {
              "id": 1,
              "message": { "text": "helper defined here" },
              "physicalLocation": {
                "artifactLocation": { "uri": "example.py" },
                "region": { "startLine": 3 }
              }
            },
            {
              "id": 2,
              "message": { "text": "blocking call: time.sleep" },
              "physicalLocation": {
                "artifactLocation": { "uri": "example.py" },
                "region": { "startLine": 4 }
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
                        "message": { "text": "async function handler()" },
                        "physicalLocation": {
                          "artifactLocation": { "uri": "example.py" },
                          "region": { "startLine": 6 }
                        }
                      }
                    },
                    {
                      "location": {
                        "message": { "text": "calls helper()" },
                        "physicalLocation": {
                          "artifactLocation": { "uri": "example.py" },
                          "region": { "startLine": 7 }
                        }
                      }
                    },
                    {
                      "location": {
                        "message": { "text": "calls blocking time.sleep()" },
                        "physicalLocation": {
                          "artifactLocation": { "uri": "example.py" },
                          "region": { "startLine": 4 }
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

**SARIF-Specific Notes:**
- All four STRATO rules declared in tool driver, even if not all triggered
- `artifactLocation.uri` uses relative paths from project root
- Line and column numbers are 1-indexed per SARIF specification
- `codeFlows` ordered by execution sequence (async entry -> blocking call)
- Phantom nodes omit `physicalLocation`

---

## Appendix D: Configuration Schema

Strato is configured via `pyproject.toml` under the `[tool.strato]` namespace. All configuration is optional — Strato provides sensible defaults for zero-config operation.

### `[tool.strato]` — Core Configuration

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `src_roots` | `list[str]` | Auto-detected | Source roots for first-party code detection. Paths relative to `pyproject.toml`. |
| `python_version` | `str` | `"3.9"` | Minimum Python version. Affects escape hatch recognition (e.g., `asyncio.to_thread` requires 3.9+). Valid: `"3.7"`..`"3.13"`. |
| `exclude` | `list[str]` | `[]` | Glob patterns for paths to exclude (e.g., `"tests/**"`). |
| `intervention_strategy` | `str` | `"first-party-deepest"` | Error reporting strategy. Options: `"first-party-deepest"`, `"async-boundary"`. |
| `severity` | `str` | `"error"` | Diagnostic severity. Options: `"error"`, `"warning"`, `"info"`. |
| `cache_dir` | `str` | `".strato_cache"` | Cache directory (relative to `pyproject.toml`). |
| `cache_enabled` | `bool` | `true` | Enable/disable caching. |
| `stub_paths` | `list[str]` | `[]` | Additional directories to search for `.pyi` stubs with `@blocking` annotations. |
| `output_format` | `str` | `"text"` | Output format. Options: `"text"`, `"json"`, `"sarif"`. |

### `[tool.strato.blocking]` — Blocking Function Database

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `add` | `list[object]` | `[]` | Custom blocking functions. Each: `{ name, help, category }`. |
| `remove` | `list[str]` | `[]` | Remove built-in entries by qualified name. |
| `blocking_modules` | `list[str]` | `[]` | Mark entire modules as blocking. |

**`add` entry fields:**
- `name` (required): Fully qualified function name (e.g., `"redis.Redis.get"`)
- `help` (required): Human-readable fix suggestion
- `category` (required): One of: `"sleep"`, `"network-io"`, `"file-io"`, `"subprocess"`, `"database-io"`, `"user-input"`, `"other"`

### `[tool.strato.executor-wrappers]` — Custom Escape Hatches

Key-value pairs where the key is the qualified wrapper name and the value specifies which parameter receives the callable:

```toml
"qualified.wrapper.name" = { callable_param = <int | str> }
```

- **Integer**: Positional parameter index (0-based)
- **String**: Keyword argument name

**Precedence**: `@unblocker` annotation > config entry.

### Validation Rules

| Check | Error Message |
|-------|---------------|
| `src_roots` path missing | `Source root '{path}' does not exist` |
| Invalid `python_version` | `Invalid python_version: must be '3.7'...'3.13'` |
| Invalid `intervention_strategy` | `Invalid strategy: must be 'first-party-deepest' or 'async-boundary'` |
| Invalid `severity` | `Invalid severity: must be 'error', 'warning', or 'info'` |
| Missing `name` in `blocking.add` | `Blocking entry missing required field 'name'` |
| Invalid `category` | `Unknown category '{cat}'. Valid: sleep, network-io, file-io, subprocess, database-io, user-input, other` |
| Missing `callable_param` | `Executor wrapper '{name}' missing required field 'callable_param'` |

### Complete Annotated Example

```toml
[tool.strato]
src_roots = ["src", "lib"]
python_version = "3.11"
exclude = [
    "tests/**",
    "migrations/**",
    "**/conftest.py",
]
intervention_strategy = "first-party-deepest"
severity = "error"
cache_dir = ".strato_cache"
cache_enabled = true
stub_paths = ["stubs/"]
output_format = "text"


[tool.strato.blocking]
add = [
    { name = "redis.Redis.get", help = "Use aioredis", category = "network-io" },
    { name = "redis.Redis.set", help = "Use aioredis", category = "network-io" },
    { name = "mylib.slow_computation", help = "Use asyncio.to_thread()", category = "other" },
]
remove = [
    "builtins.open",  # Our open() is monkeypatched to be async-safe
]
blocking_modules = [
    "legacy_sync_module",
]


[tool.strato.executor-wrappers]
# Third-party wrappers
"asgiref.sync.sync_to_async" = { callable_param = 0 }
"anyio.to_thread.run_sync" = { callable_param = 0 }

# Project-specific wrappers
"myproject.utils.offload" = { callable_param = 0 }
"myproject.async_helpers.run_blocking" = { callable_param = "func" }
```

---

## Appendix E: Repository Structure & Implementation Plan

### E.1 Repository Layout

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
│   │       ├── resolver.rs          # Phase 3: module resolver
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

### E.2 Cargo Workspace

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
| `ty_python_semantic` | Pinned ruff git rev | Type inference |
| `petgraph` | `0.6` | Call graph data structure |
| `serde` | `1` (derive) | Serialization |
| `bincode` | `1` | Binary cache format |
| `clap` | `4` (derive) | CLI argument parsing |
| `rayon` | `1` | Parallel file processing |
| `sha2` | `0.10` | File content hashing |
| `miette` | `7` (fancy) | Beautiful error output |

### E.3 Implementation Milestones

| Milestone | Name | Key Deliverable | Effort |
|-----------|------|-----------------|--------|
| M-1 | ty Integration Spike | Validate ty crate API at pinned rev | Small |
| M0 | Project Scaffolding | Compiling workspace with stub modules | Small |
| M1 | Parser + Discovery | Parse Python files using ruff, discover project files | Medium |
| M2 | Module Resolver | Cross-file import resolution, symbol table | Large |
| M3 | Call Graph | Project-wide call graph construction | Large |
| M4 | Blocking Database | 80+ known blocking functions with help text | Medium |
| M5 | Propagation | SCC-based blocking propagation (Tarjan's algorithm) | Medium |
| M6 | Escape Hatches | `run_in_executor`, `to_thread`, `@unblocker` detection | Small |
| M7 | Properties + Dunders | Implicit call detection (`@property`, `__str__`, etc.) | Medium |
| M8 | Diagnostics | Error reporting with intervention strategies | Medium |
| M9 | CLI + Output | Working binary with text/JSON/SARIF output | Medium |
| M10 | Caching | Incremental analysis with content-based invalidation | Medium |
| M11 | Integration Tests | All 19 acceptance test fixtures pass | Medium |
| M12 | Performance + Polish | Performance validated, README, maturin build | Medium |

**Critical Path:** M-1 -> M0 -> M1 -> M2 -> M3 -> M4 -> M5 -> M6 -> M7 -> M8 -> M9 -> M10 -> M11 -> M12 (strictly sequential)

### E.4 Build & Test

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

---
