# Blocking Function Database & Annotations

### Database Structure

The blocking database is a registry of functions known to block the event loop. It ships with Strato and is extended via configuration.

```rust
struct BlockingDatabase {
    entries: BTreeMap<QualifiedName, BlockingEntry>,
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

### Built-In Entries

> **Decision recap ([Blocking Database](./design-overview.md#blocking-database))**: Strato ships a curated database of 61 entries covering the most common and impactful blocking functions, rather than attempting exhaustive coverage. User extension via config and `@blocking` decorator fills gaps.

Strato ships with 61 built-in blocking function entries across six categories. The complete database is provided in [Appendix A](./appendix-a-blocking-function-database.md#appendix-a-blocking-function-database-complete). Representative examples by category:

| Category | Count | Examples |
|----------|-------|----------|
| **Sleep** | 1 | `time.sleep` |
| **Network I/O** | 27 | `requests.get`, `requests.post`, `urllib.request.urlopen`, `socket.socket.connect`, `http.client.HTTPConnection.request` |
| **File I/O** | 21 | `builtins.open`, `os.read`, `os.write`, `pathlib.Path.read_text`, `glob.glob`, `shutil.copy` |
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

> **Decision recap ([Help Text Policy](./design-overview.md#help-text-policy))**: Help text suggests async alternatives generically, never recommending one third-party library over another. When multiple options exist, all are listed neutrally (e.g., "Use `aiohttp` or `httpx`").

External calls may match through public aliases or implementation-definition names. For example, vendored typeshed defines several public `socket.socket` methods on `_socket.socket`; Strato's built-in database stores alias sets so either facade result matches the same blocking concept without counting the implementation alias as a separate built-in entry.

### User Configuration

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
- **`blocking_modules`**: Treats all resolved call targets under the specified module prefixes as blocking, without enumerating them individually. Prefix matching uses module-boundary semantics.

### Annotations API (@blocking, @non_blocking, @unblocker)

The `strato` Python package provides three decorators for annotating function blocking behavior. The package has zero dependencies and zero runtime impact – decorators are transparent wrappers.

Annotation semantics are explicit and local:

- `@blocking` marks the decorated function as a blocking root, regardless of whether its body contains a built-in database call.
- `@non_blocking` marks only the decorated function as safe. It suppresses propagated blocking for that function but does not mark its callees safe, does not remove database entries, and does not shield other functions in the same SCC.
- `@unblocker` marks the decorated function as an executor wrapper. Calls to that wrapper protect only the configured callable argument by creating `in_executor=true` synthetic edges.
- If conflicting annotations are present on the same resolved function, `@non_blocking` wins because it is the explicit false-positive override.

**Pitfalls:**

- `@non_blocking` is a local override for the decorated function only; using it to silence a real blocker hides bugs rather than fixing them.
- `@unblocker` should only be used for wrappers that actually offload work; annotating an ordinary sync helper as an executor wrapper will suppress real findings.
- `@blocking` should be reserved for functions that are semantically blocking roots, not just functions that are slow for unrelated reasons.

#### Decorator Definitions

```python
# strato/_annotations.py

from typing import Callable, Optional, TypeVar, Union, overload

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


@overload
def unblocker(func: F) -> F: ...


@overload
def unblocker(*, callable_param: Union[int, str] = 0) -> Callable[[F], F]: ...


def unblocker(func: Optional[F] = None, *, callable_param: Union[int, str] = 0) -> Union[F, Callable[[F], F]]:
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

The annotations package supports Python 3.7+, so its public typing uses `typing.Optional`, `typing.Union`, and overloads rather than Python 3.10 `|` union syntax.

> **Decision recap ([Escape Hatches](./design-overview.md#escape-hatches))**: The `@unblocker` decorator is part of the v1 generalized executor-wrapper model. It enables first-party wrappers to declare which callable argument is offloaded.

#### Annotation Detection Algorithm

During Phase 2 (Parse), the AST walker records raw decorator applications. During Phase 3/5, the facade resolves decorator targets and classifies Strato annotations:

```
FUNCTION classify_annotations(func_def: &StmtFunctionDef, facade: &StratoTyFacade) -> Option<AnnotationType>:

  FOR decorator in func_def.decorator_list:
    MATCH decorator:
      // @blocking
      Name("blocking"):
        IF facade.resolves_to_strato_annotation(decorator, "blocking"):
          RETURN Some(AnnotationType::Blocking)

      // @strato.blocking
      Attribute(value=Name("strato"), attr="blocking"):
        IF facade.resolves_to_strato_annotation(decorator, "blocking"):
          RETURN Some(AnnotationType::Blocking)

      // @non_blocking
      Name("non_blocking"):
        IF facade.resolves_to_strato_annotation(decorator, "non_blocking"):
          RETURN Some(AnnotationType::NonBlocking)

      // @strato.non_blocking
      Attribute(value=Name("strato"), attr="non_blocking"):
        IF facade.resolves_to_strato_annotation(decorator, "non_blocking"):
          RETURN Some(AnnotationType::NonBlocking)

      // @unblocker or @unblocker(callable_param=...)
      Name("unblocker") | Call(func=Name("unblocker")):
        IF facade.resolves_to_strato_annotation(decorator, "unblocker"):
          callable_param = extract_callable_param_arg(decorator)  // Default: 0
          RETURN Some(AnnotationType::Unblocker { callable_param })

  RETURN None
```

**Import resolution**: decorator target identity comes from `StratoTyFacade`, preventing false positives from unrelated decorators with the same name while preserving the no-parallel-resolver rule.

### Stub File Support (.pyi)

Strato supports `.pyi` stub files for annotating third-party libraries without modifying their source code.

#### Resolution Data Flow

1. **Phase 1 (Discovery)**: The file manifest includes `.pyi` files found in source roots (alongside `.py` files) and `stub_paths` (from config). `stub_paths` are also passed to vendored ty as `environment.extra-paths`, not as project roots, so ty can resolve imports against those stubs without classifying them as first-party source files.

2. **Phase 3 (Semantics)**: When both `foo.py` and `foo.pyi` exist:
   - The `.py` file is used for call graph construction (body analysis)
   - The `.pyi` file is used for decorator syntax extraction and semantic annotation classification (`@blocking`/`@non_blocking`/`@unblocker` only)
   - If only `.pyi` exists (no `.py`), it is used solely for annotations (no body analysis possible)

3. **Phase 5 (Annotate)**: decorators collected from `.pyi` files are classified through the facade. Their annotations override or supplement database entries for the same qualified name.

4. **First-party classification**: `.pyi` files in `stub_paths` are classified as **third-party**. `.pyi` files in source roots follow normal classification.

#### Override Precedence

When a function has multiple sources of blocking information:

1. `@non_blocking` annotation (highest – explicit local override for that function)
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
