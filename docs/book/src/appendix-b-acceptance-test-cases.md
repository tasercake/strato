# Appendix B: Acceptance Test Cases

Each executable fixture directory contains a `fixture.toml` manifest. The manifest is the source of truth for how Strato is invoked and what the test asserts:

- Every fixture input must be accounted for exactly once by `source_files`, `config_files`, `extra_files`, or a run's expectation path.
- `source_files` lists Python source files walked for body analysis.
- `config_files` lists fixture-relative configuration files.
- `extra_files` lists non-source fixture inputs such as `.pyi` stubs.
- each `[[runs]]` entry declares CLI arguments, config source (`defaults` or a fixture-relative config path), cache mode, expected exit code, and expectation path.
- expectation `mode = "full_json"` is reserved for output-contract cases where every JSON field matters.
- expectation `mode = "partial_json"` is used for semantic cases; the `assert` list names the top-level JSON sections that protect the behavior under test. Partial JSON expectations are object-subset assertions: fields present in expected objects must match, but unrelated fields in actual objects may evolve without breaking semantic fixtures. Arrays still require the same length and order so fixtures cannot silently ignore extra diagnostics or warnings.

Do not infer config from fixture names or global harness defaults. If a case depends on `intervention_strategy`, cache behavior, output format, or CLI precedence, encode that as a named run in `fixture.toml`. JSON output always contains top-level `version`, `diagnostics`, `warnings`, and `stats`; semantic fixtures should not assert exact message text or stats unless that is their explicit purpose. `source_files` is the source of truth for body analysis; `.py` helper files that exist only to make imports resolvable must be listed in `extra_files`, not silently analyzed.

### A1: Direct Blocking in Async (STRATO001)

**Code:**

```python
import time

async def handler():
    time.sleep(1)
```

**Expected:**
- 1 diagnostic
- Error code: STRATO001
- Direct-call classification; exact message text is covered by output-contract fixtures

---

### A2: Transitive Blocking (STRATO002)

**Code:**

```python
import time

async def handler():
    helper()

def helper():
    time.sleep(1)
```

**Expected:**
- 1 diagnostic
- Error code: STRATO002
- Chain length: 3 (handler -> helper -> time.sleep)
- With the default `first-party-deepest` strategy, primary location is the `time.sleep(1)` call inside `helper`

---

### A3: Executor Wrapping is Safe

**Code:**

```python
import asyncio
import time

async def handler():
    loop = asyncio.get_event_loop()
    await loop.run_in_executor(None, time.sleep, 1)
```

**Expected:**
- 0 diagnostics
- Executor wrapping offloads blocking call to thread pool

---

### A4: `asyncio.to_thread` is Safe

**Code:**

```python
import asyncio
import time

async def handler():
    await asyncio.to_thread(time.sleep, 1)
```

**Expected:**
- 0 diagnostics
- `asyncio.to_thread` is a recognized executor wrapper

---

### A5: Sync-Only Code is Safe

**Code:**

```python
import time

def handler():
    time.sleep(1)
```

**Expected:**
- 0 diagnostics
- `handler` is not async and not called from async context

---

### A6: `@blocking` Decorator

**Code:**

```python
import time
from strato import blocking

@blocking
def custom_slow():
    pass

async def handler():
    custom_slow()
```

**Expected:**
- 1 diagnostic
- Error code: STRATO001
- `@blocking` decorator marks function as blocking regardless of implementation
- A direct async call to an `@blocking` function is still a direct blocking call

---

### A7: `@non_blocking` Override

**Code:**

```python
import time
from strato import non_blocking

@non_blocking
def actually_safe():
    time.sleep(1)

async def handler():
    actually_safe()
```

**Expected:**
- 0 diagnostics
- `@non_blocking` decorator overrides blocking detection for `actually_safe` only

---

### A8: Blocking Property (STRATO003)

**Code:**

```python
import requests

class DataFetcher:
    @property
    def data(self):
        return requests.get("https://api.example.com/data").json()

async def handler():
    fetcher = DataFetcher()
    result = fetcher.data
```

**Expected:**
- 1 diagnostic
- Error code: STRATO003
- Primary location is the property access that introduces the blocking path

---

### A9: Blocking Dunder (STRATO004)

**Code:**

```python
import requests

class RemoteObject:
    def __str__(self):
        return requests.get("https://api.example.com/status").text

async def handler():
    obj = RemoteObject()
    print(str(obj))
```

**Expected:**
- 1 diagnostic
- Error code: STRATO004
- Primary location is the implicit dunder invocation, e.g. `str(obj)`

---

### A10: Cross-File Detection

**utils.py:**

```python
import time

def slow_util():
    time.sleep(1)
```

**main.py:**

```python
from utils import slow_util

async def handler():
    slow_util()
```

**Expected:**
- 1 diagnostic for the call chain entered from `main.py`
- Error code: STRATO002
- Related location: utils.py:3 (definition of slow_util)
- With `first-party-deepest`, primary location is the `time.sleep(1)` call in `utils.py`

---

### A11: Deep Transitive Chain

**Code:**

```python
import time

async def handler():
    level_1()

def level_1():
    level_2()

def level_2():
    level_3()

def level_3():
    time.sleep(1)
```

**Expected:**
- 1 diagnostic
- Error code: STRATO002
- Chain length: 5 (handler -> level_1 -> level_2 -> level_3 -> time.sleep)

---

### A12: Multiple Async Callers

**Code:**

```python
import time

def helper():
    time.sleep(1)

async def handler_a():
    helper()

async def handler_b():
    helper()
```

**Expected:**
- 2 diagnostics
- Both `handler_a` and `handler_b` flagged for calling blocking `helper`

---

### A13: Mixed Safe and Unsafe

**Code:**

```python
import asyncio
import time

def helper():
    time.sleep(1)

async def safe_caller():
    await asyncio.to_thread(helper)

async def unsafe_caller():
    helper()
```

**Expected:**
- 1 diagnostic
- Only `unsafe_caller` flagged
- `safe_caller` uses executor wrapper (safe)

---

### A14: @unblocker Basic

This is a v1 acceptance case for generalized first-party wrappers.

**Code:**

```python
import asyncio
import time
from strato import unblocker

@unblocker
def my_offload(func):
    return asyncio.to_thread(func)

async def safe_handler():
    await my_offload(lambda: time.sleep(1))

async def unsafe_handler():
    time.sleep(1)
```

**Expected:**
- 1 diagnostic
- Only `unsafe_handler` flagged
- `my_offload` is recognized as executor wrapper via `@unblocker`
- This fixture also asserts stats to protect lambda executor graph accounting: the protected lambda and its `in_executor=true` edge are graph facts even though they do not propagate a diagnostic

---

### A15: Executor Wrapper Config

This is a v1 acceptance case for generalized configured wrappers.

**pyproject.toml:**

```toml
[tool.strato.executor-wrappers]
"mylib.offload" = { callable_param = 0 }
```

**Code:**

```python
import time
from mylib import offload

async def handler():
    offload(time.sleep, 1)
```

**Expected:**
- 0 diagnostics
- `mylib.offload` configured as executor wrapper

---

### A16: Intermediate Property Edge Classifies as STRATO003

**Code:**

```python
import requests

class DataFetcher:
    @property
    def data(self):
        return load_remote()

def load_remote():
    return requests.get("https://api.example.com/data").json()

def helper(fetcher):
    return fetcher.data

async def handler():
    fetcher = DataFetcher()
    helper(fetcher)
```

**Expected:**
- 1 diagnostic
- Error code: STRATO003
- Classification is based on the intermediate `PropertyAccess` edge, not the final `requests.get` edge

---

### A17: Intermediate Dunder Edge Classifies as STRATO004

**Code:**

```python
import requests

class RemoteObject:
    def __str__(self):
        return load_remote()

def load_remote():
    return requests.get("https://api.example.com/status").text

def helper(obj):
    return str(obj)

async def handler():
    obj = RemoteObject()
    helper(obj)
```

**Expected:**
- 1 diagnostic
- Error code: STRATO004
- Classification is based on the intermediate `ImplicitDunder` edge, not the final `requests.get` edge

---

### A18: `@non_blocking` Does Not Shield SCC Peers

**Code:**

```python
import time
from strato import non_blocking

@non_blocking
def safe_entry(flag):
    if flag:
        unsafe_peer()

def unsafe_peer():
    safe_entry(False)
    time.sleep(1)

async def safe_handler():
    safe_entry(True)

async def unsafe_handler():
    unsafe_peer()
```

**Expected:**
- 1 diagnostic
- Only `unsafe_handler` is flagged
- `safe_entry` remains non-blocking, but its annotation does not erase the blocking fact for `unsafe_peer` in the same SCC

---

### A19: Alias-Based Wrapper Path is Safe

This is a v1 acceptance case for generalized configured wrappers.

**pyproject.toml:**

```toml
[tool.strato.executor-wrappers]
"mylib.offload" = { callable_param = 0 }
```

**Code:**

```python
import time
from mylib import offload as run_safe

async def handler():
    run_safe(time.sleep, 1)
```

**Expected:**
- 0 diagnostics
- Import alias resolution preserves the configured wrapper semantics

---

### A20: Deterministic Diagnostic Ordering Regression

**Code:**

```python
import time

async def handler_b():
    time.sleep(1)

async def handler_a():
    time.sleep(1)
```

**Expected:**
- 2 diagnostics
- Repeated runs produce identical normalized JSON output; volatile timing fields are normalized before comparison
- Diagnostics are ordered deterministically by file, line, column, and error code

---

### A21: Fresh and Cached Analysis Parity

**Code:**

```python
import time

def helper():
    time.sleep(1)

async def handler():
    helper()
```

**Expected:**
- Fresh analysis emits 1 diagnostic with error code STRATO002
- Cached analysis emits the same diagnostic with identical location, chain, and output ordering
- Cache state never changes diagnostic classification or suppression semantics

---

### A22: Star Import

**module_a.py:**

```python
import time

def blocking_func():
    time.sleep(1)
```

**main.py:**

```python
from module_a import *

async def handler():
    blocking_func()
```

**Expected:**
- 1 diagnostic
- Error code: STRATO002
- Star import resolved in this statically enumerable happy path. Dynamic or non-enumerable star imports remain best-effort.

---

### A23: Namespace Package

**Directory structure:**

```
project/
  namespace_pkg/  # No __init__.py
    module.py
  main.py
```

**namespace_pkg/module.py:**

```python
import time

def blocking_func():
    time.sleep(1)
```

**main.py:**

```python
from namespace_pkg.module import blocking_func

async def handler():
    blocking_func()
```

**Expected:**
- 1 diagnostic
- Namespace package (directory without `__init__.py`) resolved under the fixture source root. External or ambiguous namespace packages remain ty/source-root dependent.

---

### A24: Related Locations

**Code:**

```python
import time

def helper():
    time.sleep(1)

async def handler():
    helper()
```

**Expected JSON output:**
This fixture intentionally uses `full_json` because related-location shape and ordering are the behavior under test. A1, A8, and A9 provide the corresponding full-output contracts for STRATO001, STRATO003, and STRATO004.

```json
{
  "version": "1.0",
  "diagnostics": [
    {
      "code": "STRATO002",
      "severity": "error",
      "message": "Transitive blocking call reachable from async context",
      "primary_location": {
        "file": "main.py",
        "line": 4,
        "column": 5
      },
      "related_locations": [
        {
          "file": "main.py",
          "line": 6,
          "column": 11,
          "message": "async function handler defined here"
        },
        {
          "file": "main.py",
          "line": 3,
          "column": 1,
          "message": "helper defined here"
        },
        {
          "file": "main.py",
          "line": 4,
          "column": 5,
          "message": "blocking call: time.sleep"
        }
      ],
      "chain": [
        { "function": "handler", "file": "main.py", "line": 6, "is_async": true, "is_first_party": true },
        { "function": "helper", "file": "main.py", "line": 3, "is_async": false, "is_first_party": true },
        { "function": "time.sleep", "file": null, "line": null, "is_async": false, "is_first_party": false }
      ],
      "help": "Wrap the blocking call in `await asyncio.to_thread(...)` or use async alternative",
      "intervention_strategy": "first-party-deepest"
    }
  ],
  "warnings": [],
  "stats": { "files_analyzed": 1, "functions_analyzed": 2, "call_graph_nodes": 3, "call_graph_edges": 2, "blocking_functions_found": 1, "analysis_time_ms": 0 }
}
```

---

### A25: Syntax Warnings

**valid.py:**

```python
import time

async def handler():
    time.sleep(1)
```

**invalid.py:**

```python
def broken(
    # Missing closing parenthesis
```

**Expected:**
- 1 diagnostic from valid.py (STRATO001)
- 1 warning: "Syntax error in invalid.py"
- Analysis continues despite syntax errors when other source files remain analyzable

---

### A26: Stub Annotation Metadata

**pyproject.toml:**

```toml
[tool.strato]
stub_paths = ["stubs"]
```

**stubs/thirdparty.pyi:**

```python
from strato import blocking

@blocking
def slow() -> None: ...
```

**main.py:**

```python
from thirdparty import slow

async def handler():
    slow()
```

**Expected:**
- 1 diagnostic
- Error code: STRATO001
- The blocking fact comes from the configured `.pyi` stub, not from first-party source body analysis

---

### A27: Blocking Config Add

**pyproject.toml:**

```toml
[tool.strato.blocking]
add = [
    { name = "legacy.slow", help = "Offload legacy.slow or replace it with an async implementation", category = "other" },
]
```

**legacy.py:**

```python
def slow():
    pass
```

**main.py:**

```python
from legacy import slow

async def handler():
    slow()
```

**Expected:**
- Default run: 0 diagnostics; unannotated first-party sync helper remains unknown
- Configured run: 1 diagnostic with error code STRATO001
- User config can mark a resolvable first-party callable as blocking

---

### A28: Blocking Config Remove

**pyproject.toml:**

```toml
[tool.strato.blocking]
remove = ["time.sleep"]
```

**main.py:**

```python
import time

async def handler():
    time.sleep(1)
```

**Expected:**
- Default run: 1 diagnostic for built-in `time.sleep`
- Configured run: 0 diagnostics
- Removing a built-in blocking entry makes that external call invisible rather than speculatively blocking

---

### A29: Blocking Module Prefix

**pyproject.toml:**

```toml
[tool.strato]
stub_paths = ["stubs"]

[tool.strato.blocking]
blocking_modules = ["legacy_mod"]
```

**stubs/legacy_mod.pyi:**

```python
def slow() -> None: ...
```

**stubs/legacy_mod_extra.pyi:**

```python
def slow() -> None: ...
```

**main.py:**

```python
import legacy_mod
import legacy_mod_extra

async def handler():
    legacy_mod.slow()
    legacy_mod_extra.slow()
```

**Expected:**
- 1 diagnostic
- Error code: STRATO001
- Module-boundary prefix matching marks `legacy_mod.slow` blocking but does not match `legacy_mod_extra.slow`

---

### A30: Python Version Controls `asyncio.to_thread`

**pyproject.toml:**

```toml
[tool.strato]
python_version = "3.8"
```

**main.py:**

```python
import asyncio
import time

async def handler():
    await asyncio.to_thread(time.sleep, 1)
```

**Expected:**
- 0 diagnostics
- 1 warning that `asyncio.to_thread` is unavailable for Python 3.8 and executor protection was not applied
- The wrapped callable is not treated as a direct call merely because the escape hatch is unavailable

---

### A31: Unresolved Calls Stay Unknown

**main.py:**

```python
async def handler(callback):
    callback()
```

**Expected:**
- 0 diagnostics
- 0 warnings
- Unresolvable call targets are skipped rather than treated as blocking

---

### A32: `functools.partial` Executor Wrapping is Safe

**Code:**

```python
import asyncio
import time
from functools import partial

async def handler():
    loop = asyncio.get_running_loop()
    await loop.run_in_executor(None, partial(time.sleep, 1))
```

**Expected:**
- 0 diagnostics
- `partial` imported by name is semantically resolved to `functools.partial`
- The underlying `time.sleep` callable is recorded as protected by the executor wrapper

---

### A33: Method Call Resolution

**Code:**

```python
import time

class Worker:
    def instance_slow(self):
        time.sleep(1)

    @staticmethod
    def static_slow():
        time.sleep(1)

    @classmethod
    def class_slow(cls):
        time.sleep(1)

async def instance_handler():
    worker = Worker()
    worker.instance_slow()

async def static_handler():
    Worker.static_slow()

async def class_handler():
    Worker.class_slow()
```

**Expected:**
- 3 diagnostics
- Error code: STRATO002 for each diagnostic
- The facade resolves instance, static, and class method call targets to their first-party method definitions

---

### A34: Callable Object Dunder

**Code:**

```python
import time

class CallableWorker:
    def __call__(self):
        time.sleep(1)

async def handler():
    worker = CallableWorker()
    worker()
```

**Expected:**
- 1 diagnostic
- Error code: STRATO004
- Direct callable-object invocation resolves to `CallableWorker.__call__`

---

### A35: Representative Dunder Operations

**Code:**

```python
import time

class BlockingValue:
    def __add__(self, other):
        time.sleep(1)
        return self

    def __lt__(self, other):
        time.sleep(1)
        return False

    def __format__(self, spec):
        time.sleep(1)
        return "value"

    def __getitem__(self, key):
        time.sleep(1)
        return self

    def __enter__(self):
        time.sleep(1)
        return self

    def __exit__(self, exc_type, exc, tb):
        return False

    def __iter__(self):
        time.sleep(1)
        return iter(())
```

**Expected:**
- 6 diagnostics
- Error code: STRATO004 for binary addition, comparison, formatting, subscript, context-manager entry, and iteration
- Classification is based on the implicit dunder edge that introduces blocking behavior
