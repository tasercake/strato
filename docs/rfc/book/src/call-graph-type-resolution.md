# 5. Call Graph & Type Resolution

> **Decision recap:** The call graph is the central data structure for propagation analysis. We chose a node-per-callable model (rather than node-per-statement) to keep graph size manageable and enable efficient traversal. Type resolution uses Astral's `ty` crate for full inference; an earlier hand-rolled `ScopeBindings` approach was dropped because it failed on aliased imports, return-type inference, and attribute resolution. See [Decision 2.4](./02-design-decisions.md#24-type-inference-strategy-ty-integration-vs-hand-rolled) for the full tradeoff analysis.

### 5.1 Graph Data Model

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

### 5.2 Call Edge Visitor

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

### 5.3 Type Resolution via `ty`

#### Type resolution: `ty` (not hand-rolled `ScopeBindings`)

Strato uses Astral's `ty` crate for type inference. A hand-rolled `ScopeBindings`-style approach (scope chain of variable bindings) was considered but is insufficient: it fails on aliased imports (`x = requests.get; x()`), return type inference (`factory().method()`), and attribute resolution (`obj.attr.method()`). The current design relies on `ty` for full inference.

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

Result: No edge created for `x.method()` call. This is **by design** – we prefer false negatives over false positives.

#### Fallback: `NullTypeResolver`

If `ty` initialization fails (e.g., due to malformed AST or internal error), Strato falls back to `NullTypeResolver`, which always returns `None`. This degrades analysis to name-based resolution only (no attribute or method resolution).

### 5.4 External Symbol Modeling (Phantom Nodes)

External symbols (from third-party libraries or stdlib) are not parsed by Strato. However, they must be represented in the call graph if they are blocking.

> **Decision recap:** See [Decision 2.5](./02-design-decisions.md#25-phantom-nodes-for-external-symbols) for why we model externals as phantom nodes rather than parsing third-party source.

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

### 5.5 Properties & Dunder Methods

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

### 5.6 Qualified Name Conventions

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
