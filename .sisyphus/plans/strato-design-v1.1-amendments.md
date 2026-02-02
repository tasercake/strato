# Strato Design Amendments v1.1

> **Amends**: `.sisyphus/plans/strato-design.md` (v1.0)
> **Date**: 2026-02-01
> **Status**: Under Review
> **Trigger**: Design review feedback on UX, extensibility, and type inference

This document describes changes to the approved v1.0 design. Each amendment references the original section it modifies. When the original and this document conflict, **this document takes precedence**.

---

## Table of Amendments

| # | Section Affected | Summary |
|---|-----------------|---------|
| A1 | Section 8 (Error Reporting) | Related locations in diagnostics |
| A2 | Section 8 (Error Reporting) | Help text policy: no prescriptive third-party suggestions |
| A3 | Section 9 (Blocking Database) | Help text rewrites for all entries |
| A4 | Section 11 (Escape Hatches) | Generalized executor wrappers / unblocker system |
| A5 | Section 12 (Annotations API) | New `@unblocker` decorator |
| A6 | Section 5 (Module Resolver) | Star import resolution via `__all__` |
| A7 | Section 5 (Module Resolver) | Basic namespace package support |
| A8 | Section 6 (Call Graph) | Replace ScopeBindings with ty type inference |
| A9 | Section 4 (Pipeline) | Syntax error tracking via warnings |
| A10 | Section 13 (Configuration) | New `[tool.strato.executor-wrappers]` config |
| A11 | Section 20 (Limitations) | Exhaustive known-limitations documentation |

---

## A1: Related Locations in Diagnostics

**Amends**: Section 8, `Diagnostic` struct (line ~1227)

### Change

Add `related_locations` field to `Diagnostic`:

```rust
struct Diagnostic {
    code: ErrorCode,
    severity: Severity,
    primary_location: Location,
    message: String,
    blocking_chain: Vec<ChainLink>,
    strategy: InterventionStrategy,
    help: Option<String>,
    /// Secondary locations that provide additional context.
    /// Always includes both the "trigger site" and the "blocking site"
    /// when they differ from the primary location.
    related_locations: Vec<RelatedLocation>,  // NEW
}

/// A secondary location with a descriptive label.
struct RelatedLocation {
    location: Location,
    /// Human-readable label explaining this location's role.
    /// Examples: "blocking property accessed here", "blocking call executes here"
    label: String,
}
```

### Rationale

The user pointed out that STRATO003 (blocking property) diagnostics should show *where the property was accessed*, not just where the blocking call lives inside the getter. Rather than special-casing STRATO003, we add a general-purpose `related_locations` mechanism:

- **Primary location**: Determined by the intervention strategy (consistent across all error codes)
- **Related locations**: Always include supplementary context:
  - For STRATO003: "blocking property accessed here" (the `obj.prop` expression)
  - For STRATO002: "called from async context here" (the async function's call site)
  - For all codes: "blocking call executes here" (the leaf blocking call)

### Impact on STRATO003 Specifically

Before (v1.0):
```
error[STRATO003]: Blocking property access in async context
  --> src/models/user.py:34:9
   |
34 |         return requests.get(self.avatar_url).content
   |                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `requests.get` blocks
```

After (v1.1):
```
error[STRATO003]: Blocking property access in async context
  --> src/models/user.py:34:9
   |
34 |         return requests.get(self.avatar_url).content
   |                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `requests.get` blocks
   |
  ::: src/api/users.py:52:14
   |
52 |     result = loader.data
   |              ----------- blocking property accessed here
```

The primary location follows `first-party-deepest` (here: the getter is first-party, so it points there). The related location shows the access site. If the getter were third-party, the primary would shift to the access site instead.

### Impact on Output Formats

- **Text**: Related locations shown as secondary spans (`::: file:line`)
- **JSON**: `"related_locations": [{"file": "...", "line": N, "label": "..."}]`
- **SARIF**: Mapped to `relatedLocations` array in SARIF result objects (native SARIF concept)

### Impact on Milestones

- **M8 (Reporter)**: Must generate `related_locations` for all diagnostic codes
- **M9 (Output Formats)**: All three formatters must render related locations
- **M11 (Integration Tests)**: `expected.json` format needs optional `related_locations` field

---

## A2: Help Text Policy

**Amends**: Section 8, `help` field description (line ~1240); Section 9, all help text entries

### Change

**Policy**: Help text MUST NOT suggest specific third-party libraries by name. It should describe the *problem pattern* and *solution patterns* generically.

**Allowed**:
- Stdlib alternatives: "Use `asyncio.sleep()` instead" (stdlib is always available)
- Pattern descriptions: "Offload to a thread with `asyncio.to_thread()`, or use an async alternative"
- General guidance: "Move I/O out of the property, or convert to an async method"

**Forbidden**:
- "Use `httpx` instead of `requests`"
- "Use `aiofiles.open()` instead"
- "Consider switching to `asyncpg`"

### Rationale

Strato should not be opinionated about which async libraries users adopt. It should identify problems clearly and let users choose their own solutions. Recommending specific libraries creates an implied endorsement, risks going stale as the ecosystem evolves, and doesn't serve users who have already chosen different async libraries.

### Revised Help Text Examples

| Blocking Function | Old Help (v1.0) | New Help (v1.1) |
|-------------------|-----------------|-----------------|
| `time.sleep` | "Use `asyncio.sleep()` instead" | "Use `asyncio.sleep()` instead" (unchanged — stdlib) |
| `requests.get` | (was unspecified) | "Offload to a thread with `asyncio.to_thread()`, or use an async HTTP client" |
| `builtins.open` | (was unspecified) | "Offload to a thread with `asyncio.to_thread()`, or use an async file API" |
| `subprocess.run` | (was unspecified) | "Use `asyncio.create_subprocess_exec()` or offload with `asyncio.to_thread()`" |
| `psycopg2.connect` | (was unspecified) | "Use an async database driver, or offload with `asyncio.to_thread()`" |
| `socket.connect` | (was unspecified) | "Use `asyncio` socket APIs or an async networking library" |

**General template**: "Use `{stdlib_async_alternative}` or offload with `asyncio.to_thread()`" — where the stdlib alternative exists. Otherwise: "Offload with `asyncio.to_thread()`, or use an async alternative"

---

## A3: Generalized Executor Wrappers (Unblocker System)

**Amends**: Section 11 (Escape Hatches), lines ~1700-1870

### Overview

The v1.0 design hardcodes two escape hatch patterns: `loop.run_in_executor()` and `asyncio.to_thread()`. This amendment generalizes the concept into an **executor wrapper** system that is:

1. **Extensible via config** — users can register third-party wrappers
2. **Extensible via decorator** — users can annotate their own wrappers
3. **Supports alias tracking** — `wrapped = sync_to_async(func); await wrapped()` is handled

### Concept: Executor Wrapper

An **executor wrapper** is a function that takes a callable argument and arranges for it to execute off the event loop thread. The wrapper *removes the blocking taint* from calls to the wrapped callable.

Built-in wrappers (v1.1):
- `asyncio.loop.run_in_executor` (callable at position 1)
- `asyncio.to_thread` (callable at position 0)

User-configurable wrappers:
- `asgiref.sync.sync_to_async` (callable at position 0)
- `anyio.to_thread.run_sync` (callable at position 0)
- Any user-defined wrapper

### Configuration

```toml
# pyproject.toml
[tool.strato.executor-wrappers]
# Format: "qualified.name" = { callable_param = <position_or_name> }
"asgiref.sync.sync_to_async" = { callable_param = 0 }
"anyio.to_thread.run_sync" = { callable_param = 0 }
"mylib.utils.run_in_thread" = { callable_param = "func" }
```

**Parameters**:
- `callable_param`: Which parameter receives the callable to offload. Can be:
  - An integer (positional index, 0-based)
  - A string (keyword argument name)
  - Both are tried at call sites — positional first, then keyword

### Decorator: `@unblocker`

For first-party wrapper functions:

```python
from strato import unblocker

@unblocker  # Default: callable_param=0
def my_thread_pool(func, *args, **kwargs):
    """Runs func in a thread pool."""
    return run_in_pool(func, *args, **kwargs)

@unblocker(callable_param="target")
def custom_offload(*, target, timeout=30):
    """Offloads target to background worker."""
    return background.submit(target, timeout=timeout)
```

**Decorator implementation** (in `python/strato/_annotations.py`):
```python
def unblocker(func=None, *, callable_param=0):
    """Mark a function as an executor wrapper that offloads its callable argument."""
    def decorator(f):
        @functools.wraps(f)
        def wrapper(*args, **kwargs):
            return f(*args, **kwargs)
        wrapper.__strato_unblocker__ = True
        wrapper.__strato_callable_param__ = callable_param
        return wrapper

    if func is not None:
        return decorator(func)
    return decorator
```

**Detection** (in `annotator.rs`): Same pattern as `@blocking`/`@non_blocking` — match by decorator name, not import resolution.

### Graph Semantics

When a call matches a known executor wrapper:

1. Identify the callable argument at the configured parameter position
2. If the callable resolves to a known function `f`:
   - Create an **induced edge** from the caller to `f` with `in_executor = true`
   - The edge carries `via: Some("sync_to_async")` for diagnostic clarity
3. The wrapper function itself is treated normally (its own body is analyzed)

**Example**:
```python
async def handler():
    safe_func = sync_to_async(blocking_db_query)  # wrapper recognized
    result = await safe_func()                      # safe — induced edge has in_executor=true
```

Graph edges:
- `handler -> sync_to_async`: DirectCall (normal)
- `handler -> blocking_db_query`: InducedEdge (in_executor=true, via="sync_to_async")

The InducedEdge prevents blocking propagation from `blocking_db_query` to `handler`.

### Alias Tracking for Wrapped Callables

The v1.0 `ScopeBindings` does NOT track this. With **ty integration** (see Amendment A8), the type inference engine handles value flow. Specifically:

For the pattern:
```python
safe = sync_to_async(func)
await safe()
```

ty resolves `safe` to a callable. Strato sees:
1. `sync_to_async(func)` — wrapper call with known callable at position 0
2. Records: "`safe` is a callable produced by unblocker `sync_to_async` wrapping `func`"
3. When `safe()` is called later, the induced edge to `func` has `in_executor=true`

**Without ty** (fallback, if ty integration fails): Expand `ScopeBindings` with:
```rust
enum LocalBinding {
    // ... existing variants ...
    /// Variable assigned from an executor wrapper call.
    /// `safe = sync_to_async(blocking_func)` → WrappedCallable { inner: "blocking_func", wrapper: "sync_to_async" }
    WrappedCallable { inner: QualifiedName, wrapper: QualifiedName },
}
```

### How Unblockers Differ from `@non_blocking`

| Aspect | `@non_blocking` | `@unblocker` |
|--------|-----------------|-------------|
| **Claim** | "This function itself does not block" | "This function offloads its callable argument to another thread" |
| **Effect** | Sets the function's status to `KnownNonBlocking` | Creates `in_executor=true` induced edges for wrapped callables |
| **Scope** | The function's own behavior | The wrapped callable's execution context |
| **Use case** | CPU-bound work, cached I/O, false positive suppression | `sync_to_async`, custom thread pool wrappers |
| **Composability** | Stops propagation AT this node | Stops propagation THROUGH this wrapper for the wrapped callable |

Both are needed. They are orthogonal concepts.

### Impact on Milestones

- **M4 (Annotations)**: Add `@unblocker` detection alongside `@blocking`/`@non_blocking`
- **M6 (Escape Hatches)**: Refactor to use the generalized wrapper system (built-in `run_in_executor` and `to_thread` become entries in the wrapper registry, not hardcoded patterns)
- **M9 (CLI/Config)**: Parse `[tool.strato.executor-wrappers]` config section

---

## A4: Star Import Resolution

**Amends**: Section 5 (Module Resolver)

### Change

Add static star import resolution to the module resolver:

**Algorithm**:
```
FUNCTION resolve_star_import(module_path: &str, symbol_table: &SymbolTable) -> Vec<String>:

  target_module = resolve_module(module_path)
  IF target_module is None:
    RETURN []  // Unresolvable module → treat as empty

  source = read_source(target_module.file_path)
  
  // Strategy 1: Look for literal __all__
  all_value = extract_literal_all(source)
  IF all_value is Some(names):
    RETURN names  // ["foo", "bar", "baz"]

  // Strategy 2: No __all__ → collect all public top-level definitions
  RETURN target_module.top_level_names()
         .filter(|name| !name.starts_with("_"))
         .collect()
```

**`extract_literal_all()`**: Parses the target module's AST looking for:
- `__all__ = ["name1", "name2", ...]` — literal list of strings
- `__all__ = ("name1", "name2", ...)` — literal tuple of strings
- `__all__ = ["a", "b"] + ["c", "d"]` — simple concatenation of literal lists

If `__all__` is assigned from a non-literal expression (function call, variable, etc.), return `None` and fall through to Strategy 2.

**Strategy 2 (no `__all__`)**: Collect all names defined at the module's top level that don't start with `_`. This includes:
- Function definitions (`def foo():`)
- Class definitions (`class Foo:`)
- Variable assignments (`BAR = ...`)
- Imported names (`from x import y` makes `y` available)

### Rationale

Star imports are pragmatically solvable for the vast majority of real-world cases. Packages that re-export via `__init__.py` (e.g., `from .models import *`) are extremely common, and skipping them causes false negatives in cross-module analysis.

### Impact on Milestones

- **M2 (Module Resolver)**: Add `resolve_star_import()` function with literal `__all__` support and public-names fallback
- **M2 tests**: Add `resolver::test_star_import_with_all`, `resolver::test_star_import_without_all`, `resolver::test_star_import_dynamic_all_fallback`

---

## A5: Basic Namespace Package Support

**Amends**: Section 5 (Module Resolver)

### Change

Within configured source roots, treat directories without `__init__.py` as namespace package portions (PEP 420).

**Resolution behavior change**:
```
OLD: directory without __init__.py → not a package → resolution fails
NEW: directory without __init__.py → namespace package portion → continue resolution into subdirectories
```

**Constraints**:
- Only within project source roots (no `sys.path` discovery)
- A regular package (with `__init__.py`) always takes precedence over a namespace portion at the same path
- No cross-root namespace merging in v1 (would require searching multiple roots and combining)

### Rationale

This is a pragmatic limitation that causes surprising "cannot resolve import" errors in monorepos and projects that follow newer Python packaging practices. Basic support within configured roots is low-effort and eliminates the most common failure mode.

### Impact on Milestones

- **M2 (Module Resolver)**: Modify `resolve_module()` to not require `__init__.py` for directory-as-package within source roots
- **M2 tests**: Add `resolver::test_namespace_package_basic`, `resolver::test_namespace_regular_wins`

---

## A6: Replace ScopeBindings with ty Type Inference

**Amends**: Section 6 (Call Graph Construction), specifically the `ScopeBindings` subsection (lines 607-701)

### Change

**Remove `ScopeBindings` entirely.** Replace all type inference with Astral's `ty` crate (`ty_python_semantic`).

### Architecture

```
v1.0 (old):                        v1.1 (new):
                                    
  Parser (ruff) → AST                Parser (ruff) → AST
       |                                  |
  ScopeBindings (hand-rolled)         ty_python_semantic
       |                                  |
  infer_simple_type()                 ty type queries
       |                                  |
  Call graph builder                  Call graph builder
```

**Integration pattern**:

```rust
// Abstract over type resolution source
trait TypeResolver {
    /// Given an expression, return the qualified name of its type (if known)
    fn resolve_type(&self, expr: &Expr, file: &Path) -> Option<QualifiedName>;
    
    /// Given a call expression, return the qualified name of the callee (if known)
    fn resolve_callee(&self, call: &ExprCall, file: &Path) -> Option<QualifiedName>;
    
    /// Given an attribute access, return the resolved method/property (if known)
    fn resolve_attribute(&self, obj_type: &QualifiedName, attr: &str) -> Option<QualifiedName>;
    
    /// Compute MRO for a class
    fn mro(&self, class: &QualifiedName) -> Vec<QualifiedName>;
}

// v1.1: Implemented via ty
struct TyTypeResolver {
    // ty's Salsa database + semantic model
    db: ty_python_semantic::Db,
}

impl TypeResolver for TyTypeResolver {
    fn resolve_type(&self, expr: &Expr, file: &Path) -> Option<QualifiedName> {
        // Query ty's type inference for this expression
        let ty = self.db.infer_expression_type(file, expr.range());
        ty.as_qualified_name()
    }
    // ... etc
}
```

### What ty Gives Us

| Capability | ScopeBindings (v1.0) | ty (v1.1) |
|-----------|---------------------|-----------|
| `self`/`cls` type | Yes | Yes |
| Constructor return type | Yes | Yes |
| Import resolution | Yes | Yes |
| Alias tracking (`x = requests.get`) | No | Yes |
| Return type inference | No | Yes |
| Attribute type resolution | No | Yes |
| MRO / inherited methods | No | Yes |
| Generic instantiation | No | Yes |
| Type narrowing (`isinstance`) | No | Yes |
| `functools.partial` semantics | No | Likely |
| Protocol / structural typing | No | Yes |

### Pre-Implementation Research Required

Before implementing this, a **deep analysis of ty's crate API** at a specific pinned commit is required:

1. Which crates to depend on (`ty_python_semantic`, `ty_module_resolver`, etc.)
2. How to construct ty's `Db` (Salsa database) and feed it source files
3. How to query expression types at specific AST positions
4. How to extract qualified names from ty's `Type` enum
5. Whether ty's module resolution conflicts with or replaces Strato's own resolver
6. Performance characteristics (ty processes 1M LOC in ~5s — should be fine for our use case)
7. API stability risk assessment

**This research must happen as a dedicated task before M0 begins.** Pin a ty commit, probe its public-ish API surface, and document the integration contract.

### Fallback Plan

If ty integration proves infeasible (API too unstable, build failures, performance overhead):
- Revert to `ScopeBindings` from v1.0 design
- Add the `trait TypeResolver` abstraction with a `ScopeBindingsResolver` implementation
- Plan for ty swap in v2

### Impact on Milestones

- **New M-1**: Research ty crate API at pinned commit (prerequisite for M0)
- **M0**: Add ty crates to workspace dependencies alongside ruff crates
- **M3**: Call graph builder uses `trait TypeResolver` instead of `ScopeBindings`
- **M5-M7**: Propagation, escape hatches, properties/dunders all benefit from better type resolution
- **M11**: Integration tests may catch more cases (fewer false negatives)

---

## A7: Syntax Error Tracking via Warnings

**Amends**: Section 4 (Pipeline), Section 8 (Error Reporting), Section 14 (CLI)

### Change

Add a `warnings` system to `AnalysisResult`:

```rust
struct AnalysisResult {
    diagnostics: Vec<Diagnostic>,
    warnings: Vec<AnalysisWarning>,  // NEW
    stats: AnalysisStats,
}

enum AnalysisWarning {
    /// A file could not be parsed due to syntax errors.
    ParseError {
        file: String,
        error: String,
        line: Option<usize>,
    },
    /// An import could not be resolved (informational).
    UnresolvableImport {
        file: String,
        line: usize,
        import_path: String,
    },
    // Future: other non-fatal conditions
}
```

### Output Behavior

| Format | Behavior |
|--------|----------|
| **Text** | Warnings printed after diagnostics, dimmed, prefixed with `warning:` |
| **JSON** | Included in `"warnings"` array alongside `"diagnostics"` |
| **SARIF** | Included as results with `"level": "note"` |
| **`--stats`** | Shows count of files skipped due to parse errors |

### Example Text Output

```
error[STRATO002]: Indirect blocking call reachable from async context
  --> src/services/email.py:23:5
   ...

warning: failed to parse src/legacy/broken.py: unexpected token at line 42
warning: failed to parse src/generated/proto.py: invalid syntax at line 1

Found 1 error, 2 warnings in 1.8s (analyzed 245 of 247 files)
```

### Impact on Milestones

- **M1 (Parser)**: Collect parse errors as `AnalysisWarning::ParseError` instead of silently dropping them
- **M8 (Reporter)**: Include warnings in output alongside diagnostics
- **M9 (Output Formats)**: All three formatters render warnings
- **M12 (Polish)**: Stats summary includes warning counts

---

## A8: Executor Wrapper Configuration

**Amends**: Section 13 (Configuration)

### Change

Add `[tool.strato.executor-wrappers]` to the configuration spec:

```toml
# pyproject.toml
[tool.strato]
severity = "error"
first-party = ["myapp"]

# Built-in wrappers (always active):
#   asyncio.loop.run_in_executor (callable_param = 1)
#   asyncio.to_thread (callable_param = 0)
#
# Additional wrappers:
[tool.strato.executor-wrappers]
"asgiref.sync.sync_to_async" = { callable_param = 0 }
"anyio.to_thread.run_sync" = { callable_param = 0 }
"starlette.concurrency.run_in_threadpool" = { callable_param = 0 }
"myutils.offload" = { callable_param = "target_func" }
```

### Config Schema

```rust
struct ExecutorWrapperConfig {
    /// Qualified name of the wrapper function
    name: QualifiedName,
    /// Which parameter receives the callable to offload.
    /// Can be positional (usize) or keyword (String).
    callable_param: CallableParam,
}

enum CallableParam {
    Position(usize),
    Name(String),
}
```

### Resolution at Call Sites

When the call graph builder encounters a call to a known wrapper:

```
call = sync_to_async(my_blocking_func, thread_sensitive=False)
                     ^^^^^^^^^^^^^^^^^
                     callable_param = 0 → this argument
```

1. Look up the callee in the executor wrapper registry
2. Extract the argument at the configured position/name
3. If the argument resolves to a known function, create an induced edge with `in_executor=true`

---

## A9: Exhaustive Known Limitations Documentation

**Amends**: Section 20 (Limitations and Future Work)

### Change

Replace the current limitations table with an exhaustive, categorized list. Each limitation includes:
- **What**: Precise description of what's not handled
- **Why**: Whether it's fundamental or pragmatic, and the design rationale
- **Workaround**: What users can do today
- **Future**: Whether it's planned for a future version

### Limitation Categories

#### Type System Limitations

| Limitation | Type | Workaround | Future |
|-----------|------|-----------|--------|
| Dynamic dispatch (`getattr()`, `__getattr__`) | Fundamental | `@blocking` annotation | No — requires runtime info |
| Monkey patching | Fundamental | None | No — requires runtime info |
| `eval()`/`exec()` generated callables | Fundamental | None | No — Halting Problem adjacent |
| Generic type parameter resolution (with ty) | Pragmatic | Depends on ty's coverage | Improves as ty matures |
| Metaclass `__call__` | Pragmatic | `@blocking` annotation | Possible in v2 |

#### Import System Limitations

| Limitation | Type | Workaround | Future |
|-----------|------|-----------|--------|
| Dynamic imports (`importlib.import_module()`) | Fundamental | `@blocking` on the resulting callable | No |
| Conditional imports beyond first branch | Pragmatic | Place preferred import first in `try` | Possible: analyze all branches |
| `__import__()` builtin | Fundamental | None | No |
| Circular import edge cases | Pragmatic | Restructure imports | Possible in v2 |

#### Call Graph Limitations

| Limitation | Type | Workaround | Future |
|-----------|------|-----------|--------|
| Callbacks passed to non-wrapper functions | Pragmatic | `@non_blocking` on the caller | Possible with deeper value flow |
| Decorator chains (beyond `@property`/`@blocking`/etc.) | Pragmatic | `@blocking`/`@non_blocking` annotation | Possible: decorator semantic registry |
| `@cached_property` | Pragmatic | Treated same as regular attribute access | v2: recognize as property |
| Generator/coroutine `send()` edges | Pragmatic | Not tracked | Possible in v2 |
| `async for` / `async with` on custom types | Pragmatic | Only async versions tracked; sync `__aiter__` etc. handled | Improve with ty |
| Comprehension-local function calls | Supported | N/A — comprehension bodies are walked | N/A |

#### Scope Limitations

| Limitation | Type | Workaround | Future |
|-----------|------|-----------|--------|
| asyncio only (no trio/anyio) | Pragmatic | Config executor-wrappers for trio/anyio escape hatches | v2: native trio/anyio support |
| No framework-specific knowledge (Django ORM, etc.) | Pragmatic | Config blocking-functions + executor-wrappers | v2: framework plugins |
| No cross-process analysis | Fundamental | `@blocking` annotations | No |
| Single-project scope (no installed package analysis) | Pragmatic | Blocking database covers common packages | v2: venv traversal |

### Behavior for Each "Skip Silently" Decision

Every case where Strato silently skips analysis (no diagnostic, no warning) is documented:

| Situation | Behavior | Justification |
|----------|----------|--------------|
| Unresolvable callee | No edge created, no diagnostic | High precision: don't guess |
| Unknown type for attribute access | No dunder/property edge | High precision: don't guess |
| Unknown type for method call | No edge created | High precision: don't guess |
| `from x import *` with dynamic `__all__` | Imported names treated as unknown | Can't statically determine |
| `getattr(obj, "method")()` | Not tracked | Dynamic attribute name |
| Variable assigned from function return | Type depends on ty resolution | ty handles what it can |

---

## A10: New Annotation — `@unblocker`

**Amends**: Section 12 (Annotations API)

### `python/strato/_annotations.py` — Updated

```python
"""Strato annotations for marking function blocking behavior."""

import functools
from typing import TypeVar, Callable, Any, overload

F = TypeVar("F", bound=Callable[..., Any])


def blocking(func: F) -> F:
    """Mark a function as blocking the event loop.
    
    Use this to annotate functions that Strato's built-in database
    doesn't cover but that you know are blocking.
    """
    @functools.wraps(func)
    def wrapper(*args: Any, **kwargs: Any) -> Any:
        return func(*args, **kwargs)
    wrapper.__strato_blocking__ = True  # type: ignore[attr-defined]
    return wrapper  # type: ignore[return-value]


def non_blocking(func: F) -> F:
    """Mark a function as NOT blocking, overriding Strato's analysis.
    
    Use this when Strato incorrectly flags a function as blocking
    (e.g., CPU-bound work that completes quickly).
    """
    @functools.wraps(func)
    def wrapper(*args: Any, **kwargs: Any) -> Any:
        return func(*args, **kwargs)
    wrapper.__strato_non_blocking__ = True  # type: ignore[attr-defined]
    return wrapper  # type: ignore[return-value]


@overload
def unblocker(func: F) -> F: ...
@overload
def unblocker(*, callable_param: int | str = 0) -> Callable[[F], F]: ...

def unblocker(func: F | None = None, *, callable_param: int | str = 0) -> F | Callable[[F], F]:
    """Mark a function as an executor wrapper that offloads blocking work.
    
    Use this to annotate wrapper functions that execute their callable
    argument in a thread pool or other non-blocking context.
    
    Args:
        callable_param: Which parameter receives the callable to offload.
            Can be a positional index (int) or parameter name (str).
            Default: 0 (first positional argument).
    
    Example:
        @unblocker
        def my_thread_wrapper(func):
            return asyncio.to_thread(func)
        
        @unblocker(callable_param="target")
        def custom_offload(*, target, timeout=30):
            return background.submit(target, timeout=timeout)
    """
    def decorator(f: F) -> F:
        @functools.wraps(f)
        def wrapper(*args: Any, **kwargs: Any) -> Any:
            return f(*args, **kwargs)
        wrapper.__strato_unblocker__ = True  # type: ignore[attr-defined]
        wrapper.__strato_callable_param__ = callable_param  # type: ignore[attr-defined]
        return wrapper  # type: ignore[return-value]

    if func is not None:
        return decorator(func)
    return decorator  # type: ignore[return-value]
```

### `python/strato/__init__.py` — Updated

```python
from strato._annotations import blocking, non_blocking, unblocker

__all__ = ["blocking", "non_blocking", "unblocker"]
```

---

## Summary of All Changes

### New Concepts
- `RelatedLocation` — secondary diagnostic spans
- `AnalysisWarning` — non-fatal conditions (parse errors, unresolvable imports)
- `ExecutorWrapper` / `@unblocker` — generalized escape hatch system
- `trait TypeResolver` — abstraction for type inference (ty-backed)
- Star import resolution via `__all__`
- Basic namespace package support

### Modified Data Structures
- `Diagnostic` gains `related_locations: Vec<RelatedLocation>`
- `AnalysisResult` gains `warnings: Vec<AnalysisWarning>`
- `ScopeBindings` removed (replaced by ty integration)
- `CallEdge` gains `via: Option<String>` for unblocker attribution
- Blocking database help texts rewritten per A2 policy

### New Configuration
- `[tool.strato.executor-wrappers]` section in pyproject.toml

### New Python Annotations
- `@unblocker` / `@unblocker(callable_param=...)` decorator

### Research Prerequisites
- ty crate API analysis at pinned commit (new pre-M0 task)

### Impact on Implementation Plan
The implementation plan (`.sisyphus/plans/strato-implementation.md`) needs regeneration to incorporate these amendments. Key changes:
- **New milestone M-1**: ty crate research
- **M0**: Add ty dependencies
- **M2**: Star import resolution + namespace packages
- **M3**: Replace ScopeBindings with TypeResolver trait
- **M4**: Add `@unblocker` detection
- **M6**: Generalize escape hatches to use wrapper registry
- **M8**: Related locations in diagnostics
- **M9**: Warnings in output + executor-wrapper config parsing
- **M11**: Updated test expectations
- **M12**: Updated documentation
