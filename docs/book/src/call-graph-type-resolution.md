# Call Graph & Type Resolution

> **Decision recap:** The call graph is the central data structure for propagation analysis. We chose a node-per-callable model (rather than node-per-statement) to keep graph size manageable and enable efficient traversal. Module, name, and type semantics come from a Strato facade over vendored Ruff/ty; Strato consumes only normalized semantic facts and owns the blocking-specific graph, annotation, propagation, and reporting layers. See [Semantic Substrate](./design-overview.md#semantic-substrate) for the full tradeoff analysis.

### Graph Data Model

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

### Call Edge Visitor

Call graph construction happens in two phases:

**Phase A: Register all callable nodes**

Walk the AST of every file and register a `CallGraphNode` for each function/method definition and lambda expression. Lambda nodes use deterministic synthetic names based on the enclosing callable plus the lambda expression's file position. This creates the node set before analyzing call edges. Lambdas passed as executor-wrapper callable arguments are still counted as callable nodes. The graph records a protected edge from the enclosing callable to the lambda node, then records edges discovered inside the lambda body as `in_executor=true`; those protected edges do not propagate diagnostics.

**Phase B: Walk function bodies**

For each function, walk its AST and record call edges using `CallEdgeVisitor`.

#### `CallEdgeVisitor` Pseudocode

```rust
struct CallEdgeVisitor {
    file: FileId,
    current_function: NodeId,
    call_graph: &mut CallGraph,
    semantics: &dyn StratoTyFacade,
}

impl Visitor for CallEdgeVisitor {
    fn visit_expr_call(&mut self, call: &ExprCall) {
        let is_executor_wrapper = self.is_executor_wrapper_call(call);
        if is_executor_wrapper {
            self.add_protected_callable_argument_edges(call);
        } else {
            let callee = self.semantics.resolve_call_target(self.file, call);
            if let Some(target_node) = self.call_graph.node_for_target(callee) {
                let edge_kind = self.edge_kind_for_call(call);
                self.call_graph.add_edge(CallEdge {
                    from: self.current_function,
                    to: target_node,
                    kind: edge_kind,
                    location: call.location,
                    in_executor: false,
                    via: None,
                });
            }
        }
        // Continue visiting arguments. Executor-wrapper handling owns traversal of
        // the protected callable argument; do not also descend into that argument
        // normally, or lambda bodies such as asyncio.to_thread(lambda: sleep())
        // would produce both protected and unprotected edges.
        walk_expr(self, &call.func);
        for (index, arg) in call.args.iter().enumerate() {
            if self.is_protected_executor_callable_arg(call, index) {
                continue;
            }
            walk_expr(self, arg);
        }
    }

    fn visit_expr_attribute(&mut self, attr: &ExprAttribute) {
        // Check if this is a property access
        let getter = self.semantics.resolve_property_getter(self.file, attr);
        if let Some(prop_node) = self.call_graph.node_for_target(getter) {
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
        let operation = DunderOperation::binary(binop, dunder);
        for target in self.semantics.resolve_dunder_target(self.file, operation) {
            if let Some(dunder_node) = self.call_graph.node_for_target(target) {
                self.call_graph.add_edge(CallEdge {
                    from: self.current_function,
                    to: dunder_node,
                    kind: ImplicitDunder,
                    location: binop.location,
                    in_executor: false,
                    via: None,
                });
            }
        }
        walk_expr(self, &binop.left);
        walk_expr(self, &binop.right);
    }

    // ... other visit methods for comparisons, context managers, etc.
}
```

The names in this pseudocode represent Strato-owned lookups over normalized semantic facts. They are not ty API names.

#### Callee Resolution

Determining the target of a call requires resolving the callee expression:

| Callee Expression | Resolution Strategy |
|-------------------|---------------------|
| `Name` (e.g., `foo()`) | Ask the facade for the resolved callable target |
| `Attribute` (e.g., `obj.method()`) | Ask the facade for the resolved attribute target |
| `Subscript` (e.g., `funcs[0]()`) | Skip (requires runtime information) |
| `Lambda` | Use the pre-registered deterministic lambda node |
| Unresolvable | Skip silently (no edge created) |

**Key principle:** When callee resolution fails, skip the edge rather than guessing. This maintains high precision at the cost of some recall.

### Semantic Resolution via Vendored Ruff/ty

Strato uses a source-vendored Ruff monorepo as the semantic substrate. There is no Strato-owned resolver with parallel Python semantics and no local-binding fallback. The graph builder consumes normalized facts from `strato_ty_adapter`, which wraps patched vendored Ruff/ty APIs and converts semantic answers into Strato `NodeId`s or known external qualified aliases.

The adapter modifies vendored Ruff/ty to expose the semantic facts Strato requires. Those patches are narrow and factual: `definitions_for_call`, `definitions_for_callable_reference`, descriptor-aware property getter resolution, `definitions_for_dunder_operation`, external qualified alias derivation, and deterministic definition qualified names. The vendored code must not know about Strato's blocking database, escape hatches, propagation rules, or diagnostic policy.

#### Facts consumed by graph construction

| Needed Fact | Graph Use |
|-------------|-----------|
| Direct callable target for `foo()` or an alias call | `DirectCall` edge |
| Attribute target for `obj.method()` | `MethodCall` edge |
| Property getter target for `obj.prop` | `PropertyAccess` edge |
| Dunder target for operations like `str(obj)` or `obj + other` | `ImplicitDunder` edge |
| First-party definition identity | Node lookup or node registration |
| External qualified aliases | Phantom-node lookup in the blocking database, including public aliases and implementation-definition names |

The exact vendored Ruff/ty APIs used to obtain these facts are an implementation detail of the pinned Ruff revision. The facade is Strato's stable boundary; vendored patches are allowed when the current Ruff/ty API does not expose a required fact.

#### Required facade-backed helpers

Strato's v1 scope requires facade support for all of the following categories:

| Helper | Required Coverage |
|--------|-------------------|
| `resolve_call_target` | Direct calls, imported aliases, method calls, static methods, class methods, direct callable-object invocation (`obj()` resolving to `type(obj).__call__`), and external known names, backed by patched `definitions_for_call` |
| `resolve_callable_reference` | Callable references passed to executor wrappers, including direct names, imported aliases, attributes, and configured wrapper callable arguments, backed by patched `definitions_for_callable_reference` |
| `resolve_attribute_target` | Attribute/member target identity for method and descriptor analysis |
| `resolve_property_getter` | `@property` access target resolution for STRATO003, backed by a descriptor-aware property getter query that returns `property.fget` |
| `resolve_dunder_target` | Unary, binary, comparison, conversion, formatting, subscript, iterator, context-manager, and `__call__` operations for STRATO004, backed by patched `definitions_for_dunder_operation` |
| `resolves_to_event_loop_run_in_executor` | Event-loop `run_in_executor` detection without local assignment heuristics |
| `definition_qualified_name` | Deterministic display/config matching name for first-party definitions |
| `external_qualified_names` | Deterministic set of normalized external aliases used to match phantom nodes |

If a helper cannot be implemented against upstream public APIs, the vendored Ruff/ty patch set must expose the needed fact before the corresponding Strato feature can ship. Scope is not reduced to fit the current upstream API surface.

Callable-object support is deliberately narrow: direct `obj()` syntax is in scope when ty can resolve the concrete `__call__` target. General callable values, callbacks stored in variables, callables returned from functions, and higher-order dataflow remain out of scope for v1 unless the user annotates/configures the relevant callable explicitly.

#### What Strato does not consume from ty

| Capability | v1 Position |
|------------|-------------|
| Generic instantiation details | Not needed unless they affect callable target identity |
| Union branch narrowing as diagnostics | Not surfaced as warnings or uncertain findings |
| Literal value reasoning | Not part of blocking detection |
| TypedDict field modeling | Not relevant to callable graph construction |
| Serialized Salsa query state | Not cacheable cross-run |

#### Graceful Degradation

The vendored Ruff/ty facade is best-effort for individual queries. When the facade cannot provide a needed fact:

1. The graph builder receives no target for that expression.
2. The corresponding edge is skipped.
3. Analysis continues with reduced recall and no speculative diagnostic.

**Example:**

```python
def foo(x):  # x has no type annotation
    x.method()  # facade cannot infer a callable target for x.method
```

Result: No edge created for `x.method()` call. This is **by design** – we prefer false negatives over false positives.

#### Ruff/ty Facade Failures and Panics

If the vendored Ruff/ty project cannot initialize or a facade query fails, Strato emits a warning and skips semantic facts from the affected scope. Recoverable Rust panics at the facade boundary are caught on a best-effort basis where unwinding is available. Strato does not claim to recover from aborting panics or process-level failures.

### External Symbol Modeling (Phantom Nodes)

External symbols (from third-party libraries or stdlib) are not parsed by Strato. However, they must be represented in the call graph if they are blocking.

> **Decision recap:** See [Phantom Nodes](./design-overview.md#phantom-nodes) for why we model externals as phantom nodes rather than parsing third-party source.

#### Phantom Node Creation

External symbols become graph nodes **only if** any facade-provided external alias appears in the effective blocking database or matches a configured `blocking_modules` prefix. These are called **phantom nodes** (nodes without source location).

**Materialization during Phase 4:**

```rust
for alias in facade.external_qualified_names(call_target) {
    let status = blocking_database
        .get(alias)
        .copied()
        .or_else(|| blocking_database.status_for_blocking_module_prefix(alias));
    if let Some(status) = status {
        call_graph.add_node(CallGraphNode {
            id: next_id(),
            qualified_name: alias,
            kind: Function,  // Assume function unless known otherwise
            is_async: false,
            location: None,  // Phantom node
            blocking_status: status,
        });
    }
}
```

Configured `blocking_modules` entries are applied during graph construction by prefix matching facade-provided external qualified aliases. If a call resolves to aliases including `legacy.module.func` and `legacy.module` is configured as blocking, Strato creates a phantom node for `legacy.module.func` on demand and marks it `KnownBlocking`. Prefix matching uses module-boundary semantics: `legacy.module` matches `legacy.module.func`, but does not match `legacy.module_extra.func`.

#### External Qualification

Strato does not maintain separate import binding rules for external symbols. The facade over vendored Ruff/ty provides a resolved first-party definition identity or, when available, a deterministic set of external qualified aliases. Strato uses those normalized aliases only to match a phantom node from the blocking database.

#### Invisible Externals

Calls to external symbols **not in the blocking database and not under a configured blocking module** are invisible to analysis:

```python
import some_library

def foo():
    some_library.unknown_function()  # No edge created (not in DB)
```

This is **by design**: Strato only tracks blocking behavior for known-blocking functions and configured blocking module prefixes. Unknown externals remain `Unknown` and are skipped rather than assumed blocking.

### Properties & Dunder Methods

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
2. Ask the facade whether `foo.bar` resolves to a property getter through descriptor-aware property semantics
3. Normalize the getter target to a Strato `NodeId`
4. If the target is a property getter, create a `PropertyAccess` edge

**Unknown semantic target:** If the facade cannot resolve the property getter, no property edge is created (high precision).

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
3. Ask the facade whether the operation resolves to `__add__`
4. Normalize the dunder target to a Strato `NodeId`
5. If found, create `ImplicitDunder` edge

**Unknown semantic target:** If the facade cannot resolve the dunder target, no dunder edge is created.

#### Context Manager Detection

`with` statements call `__enter__` and `__exit__`:

```python
with obj:
    ...
```

**Detection algorithm:**

1. Encounter `StmtWith`
2. Ask the facade for the dunder targets of the context expression (`obj`)
3. Resolve `__enter__` and `__exit__` through vendored Ruff/ty class hierarchy semantics
4. Create two `ImplicitDunder` edges: one to `__enter__`, one to `__exit__`

### Qualified Name Conventions

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
