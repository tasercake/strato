# Appendix D: Configuration Schema

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
