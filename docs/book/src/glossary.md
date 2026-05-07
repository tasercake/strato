| Term | Definition |
|------|-----------|
| **Blocking call** | A function call that performs synchronous I/O or waits, stalling the event loop (e.g., `time.sleep()`, `requests.get()`) |
| **Transitive blocking** | A function that is not itself blocking but calls a blocking function through one or more intermediary calls |
| **Event loop** | The asyncio mechanism that schedules and runs coroutines concurrently on a single thread |
| **Call graph** | A directed graph where nodes represent functions and edges represent call relationships |
| **SCC (Strongly Connected Component)** | A maximal set of nodes in a directed graph where every node is reachable from every other node (mutual recursion) |
| **Phantom node** | A call graph node for an external symbol (e.g., `time.sleep`) with no source location, materialized from the blocking database when a resolved call references it |
| **Escape hatch** | A pattern that correctly offloads blocking work to a thread pool (e.g., `asyncio.to_thread()`, `loop.run_in_executor()`) |
| **Intervention point** | The source location shown in a diagnostic – where the user should make a change |
| **First-party code** | Code in the user's project (under configured source roots) |
| **Third-party code** | Code from external packages (stdlib, site-packages) |
| **ty** | Astral's Python type checker within the vendored Ruff monorepo, used through `strato_ty_adapter` for resolving method calls, properties, and dunder invocations |
| **StratoTyFacade** | Strato-owned compatibility boundary over patched vendored Ruff/ty APIs. It exposes semantic facts needed by graph construction without leaking ty internals into `strato_core` |
| **Salsa** | A query-based incremental computation framework used by ty for in-memory memoization |
| **Propagation** | The process of spreading "blocking" status through the call graph from known blocking functions to their callers |
| **Condensation graph** | A DAG formed by collapsing each SCC into a single node – enables single-pass topological propagation |
