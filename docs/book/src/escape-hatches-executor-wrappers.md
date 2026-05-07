# Escape Hatches & Executor Wrappers

### Built-In Patterns

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

### Detection Mechanism

During call edge construction (Phase 4), the visitor checks if the current call expression matches an escape hatch pattern.

#### Pattern Recognition

```
FUNCTION is_executor_call(call: &ExprCall) -> bool:
  MATCH call.func:
    // asyncio.to_thread(func, ...)
    Attribute(value=Name("asyncio"), attr="to_thread"):
      IF configured_python_version >= 3.9:
        RETURN true
      ELSE:
        warnings.push("asyncio.to_thread is unavailable for configured python_version; executor protection was not applied")
        RETURN false

    // loop.run_in_executor(executor, func, ...)
    Attribute(value, attr="run_in_executor"):
      RETURN facade.resolves_to_event_loop_run_in_executor(current_file, call)

    _:
      RETURN false

// The facade performs the semantic check. Strato does not maintain a local
// assignment resolver for event-loop variables.
```

#### Synthetic Edge Rule

When an escape hatch is detected, the **callable argument** is protected. However, passing a callable as an argument (e.g., `run_in_executor(None, time.sleep, 1)`) is NOT a call expression in the AST – it's a `Name` reference. Strato creates a **synthetic call edge** to model the offloading:

```
WHEN is_executor_call(call) is true:

  callable_arg = call.args[get_executor_callable_arg_position(call)]

  MATCH callable_arg:
    // Case 1: Direct name reference – time.sleep, my_func
    Name(name) | Attribute(value, attr):
      callee = facade.resolve_callable_reference(current_file, callable_arg)
      IF callee is Some:
        // Create SYNTHETIC edge with in_executor=true
        graph.add_edge(current_function, callee, DirectCall, in_executor=true)

    // Case 2: functools.partial(func, arg1, ...) – unwrap to the underlying callable.
    // This is semantic, not syntax-only: both `functools.partial(...)` and
    // `from functools import partial; partial(...)` must resolve to functools.partial.
    Call(func=partial_ref, args=[real_func, ...])
      IF facade.resolve_call_target(current_file, partial_ref) == "functools.partial":
      callee = facade.resolve_callable_reference(current_file, real_func)
      IF callee is Some:
        graph.add_edge(current_function, callee, DirectCall, in_executor=true)

    // Case 3: lambda: func(arg1) – connect and walk the lambda under executor protection
    Lambda(body):
      lambda_node = graph.node_for_lambda(callable_arg)
      graph.add_edge(current_function, lambda_node, DirectCall, in_executor=true)
      previous_function = current_function
      current_function = lambda_node
      in_executor_context = true
      visit(body)  // Any edges found inside are marked in_executor=true
      in_executor_context = false
      current_function = previous_function

    // Case 4: Anything else – unresolvable, skip
    _:
      PASS
```

After the executor wrapper has handled the callable argument, normal AST traversal must skip that same argument. Otherwise lambda or partial bodies would be visited twice: once with `in_executor=true` and once as an ordinary unprotected call path.

**Key invariant**: The synthetic edge ensures that `time.sleep` (a phantom node with `KnownBlocking`) is connected to the calling function but with `in_executor=true`, so blocking status does NOT propagate backward through this edge.

Synthetic executor edges are still counted as call-graph edges in analysis stats, and resolved blocking roots behind those edges are still counted as blocking functions found. The suppression rule affects propagation and diagnostics only; it does not erase graph facts.

**Executor scope rule**: Only the CALLABLE ARGUMENT position gets `in_executor=true` protection. In `loop.run_in_executor(executor, func, arg1, arg2)`: arg[0] (executor) is NOT protected, arg[1] (func) IS protected, arg[2..] (data arguments) are NOT protected.

### Generalized Wrapper Registry

> **Decision recap ([Escape Hatches](./design-overview.md#escape-hatches))**: Strato v1 uses a generalized registry for executor wrappers. Built-in asyncio patterns, configured wrappers, and `@unblocker` annotations all feed the same model: identify the wrapper call and mark only the offloaded callable argument with `in_executor=true`.

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
    callable_param: CallableParam,
}

enum CallableParam {
    Position(usize),
    Keyword(String),
}
```

**Built-in patterns:**

```rust
vec![
    EscapeHatchPattern { function_name: "asyncio.to_thread", callable_param: Position(0) },
    // run_in_executor is detected structurally (method on event loop)
    // rather than by qualified name, since the loop variable name varies
]
```

**Note**: `run_in_executor` is detected through the facade rather than by configured qualified name, since the loop expression may be a variable, method return, or other value whose event-loop type is known only semantically. This detection is a special case outside the registry, but it still goes through `StratoTyFacade` rather than a Strato-owned local resolver.

### Configuration Schema

Users can add custom escape hatches in `pyproject.toml`:

```toml
[tool.strato.executor-wrappers]
"asgiref.sync.sync_to_async" = { callable_param = 0 }
"myproject.utils.offload" = { callable_param = 0 }
"custom.wrapper" = { callable_param = "func" }  # Keyword argument
```

**Configuration semantics**:

- **Key**: Qualified name of the wrapper function
- **Value**: Object with `callable_param` field – integer (positional index, 0-based) or string (keyword argument name)
- Duplicate keys are rejected as a configuration error; Strato does not apply last-key-wins semantics
- Configured wrappers are trusted call-site policy. If the wrapper implementation is first-party code and should also be analyzed as ordinary source, list it in `source_files`; if it exists only to make imports resolvable for the fixture or project, keep it outside analyzed sources. Strato does not inspect a configured wrapper body to prove it truly offloads work.

**The `@unblocker` decorator** provides an annotation-based alternative to configuration for first-party wrappers (see [Annotations API](./blocking-function-database-annotations.md#annotations-api-blocking-non_blocking-unblocker)).

**Precedence**: Annotations take precedence over configuration. If a function has both `@unblocker` and a `[tool.strato.executor-wrappers]` entry, the annotation wins.
