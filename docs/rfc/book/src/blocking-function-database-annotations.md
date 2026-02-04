# 8. Blocking Function Database & Annotations

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

> **Decision recap ([3.8](./03-design-decisions.md#38-blocking-database-curated-list-vs-exhaustive))**: Strato ships a curated database of ~80 entries covering the most common and impactful blocking functions, rather than attempting exhaustive coverage. User extension via config and `@blocking` decorator fills gaps.

Strato ships with 80+ built-in blocking function entries across six categories. The complete database is provided in [Appendix A](./appendix-a-blocking-function-database.md#appendix-a-blocking-function-database-complete). Representative examples by category:

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

> **Decision recap ([3.9](./03-design-decisions.md#39-help-text-policy-no-third-party-recommendations))**: Help text suggests async alternatives generically, never recommending one third-party library over another. When multiple options exist, all are listed neutrally (e.g., "Use `aiohttp` or `httpx`").

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

The `strato` Python package provides three decorators for annotating function blocking behavior. The package has zero dependencies and zero runtime impact – decorators are transparent wrappers.

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

> **Decision recap ([3.6](./03-design-decisions.md#36-generalized-executor-wrapper-system))**: The `@unblocker` decorator is a v1.1 addition enabling user-defined executor wrappers. It generalizes the hardcoded `run_in_executor`/`to_thread` patterns.

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

1. `@non_blocking` annotation (highest – explicit override)
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
