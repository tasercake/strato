# Problem Statement & Motivation

### The Core Problem

Blocking function calls inside Python async contexts silently destroy concurrency. When an `async def` function calls a blocking operation – such as `time.sleep()`, `requests.get()`, or any synchronous I/O – the entire event loop freezes. No other coroutines can execute until the blocking call completes. The application appears to work correctly in isolation but fails catastrophically under load.

This is an insidious class of bug because:

1. **The code runs without errors.** Python does not raise exceptions or warnings when blocking calls occur in async contexts.
2. **Tests pass in isolation.** A single request completes successfully, masking the concurrency failure.
3. **Production failures are mysterious.** Under concurrent load, the application becomes unresponsive, timeouts cascade, and the root cause is non-obvious.
4. **The bug propagates transitively.** A blocking call buried five levels deep in the call stack poisons every async caller above it.

### Why Existing Tools Fail

Current linters detect only **direct** blocking calls. They cannot trace blocking behavior through call chains.

**Example from the Strato README:**

```python
import time

async def handler():
    baz()  # Looks innocent – no linter flags this

def baz():
    time.sleep(1)  # The actual blocking call
```

- **flake8-async** and **ruff ASYNC2XX** flag `time.sleep(1)` if it appears directly in an `async def`, but they do not flag `baz()` when called from `handler()`.
- **PyCG** builds call graphs but does not understand async semantics or blocking behavior.

The result: developers must manually audit every function in the call chain to determine if it eventually blocks. This is infeasible in large codebases.

### Detection Case Matrix

| Case | Description | Expected Result | Difficulty |
|------|-------------|-----------------|------------|
| Direct blocking | `async def handler(): time.sleep(1)` | STRATO001 diagnostic | Easy (existing tools catch this) |
| Indirect blocking | `async def handler(): helper()` where `helper()` calls `time.sleep(1)` | STRATO002 diagnostic | Hard (requires call graph + taint analysis) |
| Executor-wrapped | `await loop.run_in_executor(None, time.sleep, 1)` | 0 diagnostics (safe) | Medium (requires executor detection) |
| `asyncio.to_thread` | `await asyncio.to_thread(time.sleep, 1)` | 0 diagnostics (safe) | Medium (requires stdlib knowledge) |
| Sync standalone | `def handler(): time.sleep(1)` (not called from async) | 0 diagnostics (safe) | Easy (context-aware analysis) |
| Sync called from async | `async def handler(): sync_helper()` where `sync_helper()` blocks | STRATO002 diagnostic | Hard (requires cross-context tracing) |
| Blocking property | `@property def data(self): return requests.get(...)` accessed in async | STRATO003 diagnostic | Very hard (implicit call via attribute access) |
| Blocking dunder | `def __str__(self): return requests.get(...).text` with `str(obj)` in async | STRATO004 diagnostic | Very hard (implicit call via operators/builtins) |
| Cross-file blocking | `from utils import slow_util; async def handler(): slow_util()` | STRATO002 diagnostic | Hard (requires project-wide analysis) |
| Deep transitive chain | `handler() -> level_1() -> level_2() -> level_3() -> time.sleep()` | STRATO002 diagnostic with chain_length=5 | Very hard (requires deep graph traversal) |

### Tool Comparison

| Tool | Direct Blocking | Indirect Blocking | Properties | Dunders | Cross-File | Deep Chains | Executor Detection |
|------|----------------|-------------------|------------|---------|------------|-------------|-------------------|
| flake8-async | ✓ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ |
| ruff ASYNC2XX | ✓ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ |
| PyCG | N/A (call graph only) | N/A | N/A | N/A | ✓ | ✓ | N/A |
| **Strato** | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |

### Motivating Examples

#### Indirect Blocking (README Example)

```python
import time

async def handler():
    baz()  # No existing tool flags this line

def baz():
    time.sleep(1)  # Blocks the event loop
```

**Strato output:**

```
STRATO002: Transitive blocking call reachable from async context
  --> example.py:4:5
   |
 4 |     baz()
   |     ^^^^^ blocking call here
   |
   = note: call chain: handler -> baz -> time.sleep (length: 3)
```

#### Blocking Property

```python
import requests

class DataFetcher:
    @property
    def data(self):
        return requests.get("https://api.example.com/data").json()

async def process():
    fetcher = DataFetcher()
    result = fetcher.data  # Looks like attribute access, actually blocks
```

**Problem:** The property getter performs synchronous HTTP I/O. The access `fetcher.data` appears to be a simple attribute read but blocks the event loop.

**Strato output:**

```
STRATO003: Async function 'process' accesses blocking property 'DataFetcher.data'
  --> example.py:10:14
   |
10 |     result = fetcher.data
   |              ^^^^^^^^^^^^ blocking property access
   |
   = note: property getter calls requests.get (blocking)
```

#### Blocking Dunder Method

```python
import requests

class RemoteObject:
    def __str__(self):
        return requests.get("https://api.example.com/status").text

async def log_status(obj):
    print(str(obj))  # Implicit __str__ call blocks
```

**Problem:** The `__str__` method performs network I/O. The `str(obj)` call is implicit and blocks the event loop.

**Strato output:**

```
STRATO004: Async function 'log_status' calls blocking dunder method 'RemoteObject.__str__'
  --> example.py:8:11
   |
 8 |     print(str(obj))
   |           ^^^^^^^^ blocking dunder call
   |
   = note: __str__ method calls requests.get (blocking)
```

#### Cross-File Blocking

**utils.py:**

```python
import time

def slow_util():
    time.sleep(2)
```

**main.py:**

```python
from utils import slow_util

async def handler():
    slow_util()  # Blocking call hidden in another file
```

**Strato output:**

```
STRATO002: Transitive blocking call reachable from async context
  --> main.py:4:5
   |
 4 |     slow_util()
   |     ^^^^^^^^^^^ blocking call here
   |
   = note: call chain: handler -> slow_util -> time.sleep (length: 3)
   = note: slow_util defined in utils.py:3
```

#### Deep Transitive Chain

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

**Problem:** The blocking call is four levels deep. Manual inspection requires tracing through multiple function definitions.

**Strato output:**

```
STRATO002: Transitive blocking call reachable from async context
  --> example.py:4:5
   |
 4 |     level_1()
   |     ^^^^^^^^^ blocking call here
   |
   = note: call chain: handler -> level_1 -> level_2 -> level_3 -> time.sleep (length: 5)
```
