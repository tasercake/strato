# 7. Blocking Propagation

> **Decision recap**: [Decision 3.3](./03-design-decisions.md#33-scc-based-propagation-vs-iterative-fixpoint) — Use Tarjan's algorithm for strongly connected component decomposition, followed by topological propagation over the condensation graph. This eliminates cycles and enables single-pass O(V+E) propagation without iterative fixpoint computation.

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
