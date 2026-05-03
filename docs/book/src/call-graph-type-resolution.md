# Call Graph & Type Resolution

> **Decision recap:** The call graph is the central data structure for propagation analysis. We chose a node-per-callable model (rather than node-per-statement) to keep graph size manageable and enable efficient traversal. Module, name, and type semantics come from Astral's `ty` crate; Strato consumes only normalized semantic facts and owns the blocking-specific graph, annotation, propagation, and reporting layers. See [Semantic Substrate](./design-overview.md#semantic-substrate) for the full tradeoff analysis.

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

Walk the AST of every file and register a `CallGraphNode` for each function/method definition. This creates the node set before analyzing call edges.

**Phase B: Walk function bodies**

For each function, walk its AST and record call edges using `CallEdgeVisitor`.

#### `CallEdgeVisitor` Pseudocode

```rust
struct CallEdgeVisitor {
    current_function: NodeId,
    call_graph: &mut CallGraph,
    semantic_facts: &SemanticFactSet,
}

impl Visitor for CallEdgeVisitor {
    fn visit_expr_call(&mut self, call: &ExprCall) {
        let callee = self.semantic_facts.target_for_call(&call.func);
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
        if let Some(prop_node) = self.semantic_facts.property_getter_for(&attr) {
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
        if let Some(dunder_node) = self.semantic_facts.dunder_for(&binop, dunder) {
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

The names in this pseudocode represent Strato-owned lookups over normalized semantic facts. They are not ty API names.

#### Callee Resolution

Determining the target of a call requires resolving the callee expression:

| Callee Expression | Resolution Strategy |
|-------------------|---------------------|
| `Name` (e.g., `foo()`) | Ask the semantic layer for the resolved callable target |
| `Attribute` (e.g., `obj.method()`) | Ask the semantic layer for the resolved attribute target |
| `Subscript` (e.g., `funcs[0]()`) | Skip (requires runtime information) |
| `Lambda` | Create anonymous node for lambda |
| Unresolvable | Skip silently (no edge created) |

**Key principle:** When callee resolution fails, skip the edge rather than guessing. This maintains high precision at the cost of some recall.

### Semantic Resolution via `ty`

Strato uses Astral's `ty` crate as the semantic substrate. There is no Strato-owned resolver API with invented methods, and no parallel local-binding fallback. The graph builder consumes normalized facts from the ty-backed semantic layer and converts them into Strato `NodeId`s or known external qualified names.

#### Facts consumed by graph construction

| Needed Fact | Graph Use |
|-------------|-----------|
| Direct callable target for `foo()` or an alias call | `DirectCall` edge |
| Attribute target for `obj.method()` | `MethodCall` edge |
| Property getter target for `obj.prop` | `PropertyAccess` edge |
| Dunder target for operations like `str(obj)` or `obj + other` | `ImplicitDunder` edge |
| First-party definition identity | Node lookup or node registration |
| External qualified name | Phantom-node lookup in the blocking database |

The exact ty APIs used to obtain these facts are an implementation detail of the pinned ty revision and must be validated in the ty integration spike. This document describes Strato's semantic requirements, not ty's public API.

#### What Strato does not consume from ty

| Capability | v1 Position |
|------------|-------------|
| Generic instantiation details | Not needed unless they affect callable target identity |
| Union branch narrowing as diagnostics | Not surfaced as warnings or uncertain findings |
| Literal value reasoning | Not part of blocking detection |
| TypedDict field modeling | Not relevant to callable graph construction |
| Serialized Salsa query state | Not cacheable cross-run |

#### Graceful Degradation

`ty` is best-effort for Strato's purposes. When the semantic layer cannot provide a needed fact:

1. The graph builder receives no target for that expression.
2. The corresponding edge is skipped.
3. Analysis continues with reduced recall and no speculative diagnostic.

**Example:**

```python
def foo(x):  # x has no type annotation
    x.method()  # ty cannot infer type of x
```

Result: No edge created for `x.method()` call. This is **by design** – we prefer false negatives over false positives.

#### ty Failures and Panics

If ty cannot initialize for the project or a semantic query fails, Strato emits a warning and skips semantic facts from the affected scope. Recoverable Rust panics at the ty boundary are caught on a best-effort basis where unwinding is available. Strato does not claim to recover from aborting panics or process-level failures.

### External Symbol Modeling (Phantom Nodes)

External symbols (from third-party libraries or stdlib) are not parsed by Strato. However, they must be represented in the call graph if they are blocking.

> **Decision recap:** See [Phantom Nodes](./design-overview.md#phantom-nodes) for why we model externals as phantom nodes rather than parsing third-party source.

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

#### External Qualification

Strato does not maintain separate import binding rules for external symbols. The ty-backed semantic layer provides a resolved first-party definition identity or, when available, an external qualified name. Strato uses that normalized name only to match a phantom node from the blocking database.

#### Invisible Externals

Calls to external symbols **not in the blocking database** are invisible to analysis:

```python
import some_library

def foo():
    some_library.unknown_function()  # No edge created (not in DB)
```

This is **by design**: Strato only tracks blocking behavior for known-blocking functions. Unknown externals remain `Unknown` and are skipped rather than assumed blocking.

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
2. Ask the semantic layer whether `foo.bar` resolves to a property getter
3. Normalize the getter target to a Strato `NodeId`
4. If the target is a property getter, create a `PropertyAccess` edge

**Unknown semantic target:** If the semantic layer cannot resolve the property getter, no property edge is created (high precision).

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
3. Ask the semantic layer whether the operation resolves to `__add__`
4. Normalize the dunder target to a Strato `NodeId`
5. If found, create `ImplicitDunder` edge

**Unknown semantic target:** If the semantic layer cannot resolve the dunder target, no dunder edge is created.

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
