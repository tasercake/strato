# Appendix B: Acceptance Test Cases

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
- Message: "Direct blocking call to 'time.sleep' in async function 'handler'"

---

### A2: Indirect Blocking via Sync Intermediary (STRATO002)

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
- Message: "Async function 'handler' calls blocking function 'helper'"
- Chain length: 3 (handler -> helper -> time.sleep)

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
- Error code: STRATO002
- Message: "Async function 'handler' calls blocking function 'custom_slow'"
- `@blocking` decorator marks function as blocking regardless of implementation

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
- `@non_blocking` decorator overrides blocking detection

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
- Message: "Async function 'handler' accesses blocking property 'DataFetcher.data'"

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
- Message: "Async function 'handler' calls blocking dunder method 'RemoteObject.__str__'"

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
- 1 diagnostic in main.py
- Error code: STRATO002
- Message: "Async function 'handler' calls blocking function 'slow_util'"
- Related location: utils.py:3 (definition of slow_util)

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

**Code:**

```python
import time
from strato import unblocker

@unblocker
def my_offload(func):
    return func()

async def safe_handler():
    my_offload(lambda: time.sleep(1))

async def unsafe_handler():
    time.sleep(1)
```

**Expected:**
- 1 diagnostic
- Only `unsafe_handler` flagged
- `my_offload` is recognized as executor wrapper via `@unblocker`

---

### A15: Executor Wrapper Config

**pyproject.toml:**

```toml
[tool.strato.executor-wrappers]
"mylib.offload" = true
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

### A16: Star Import

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
- Star import resolved correctly

---

### A17: Namespace Package

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
- Namespace package (directory without `__init__.py`) resolved correctly

---

### A18: Related Locations

**Code:**

```python
import time

def helper():
    time.sleep(1)

async def handler():
    helper()
```

**Expected JSON output:**

```json
{
  "diagnostics": [
    {
      "code": "STRATO002",
      "message": "Async function 'handler' calls blocking function 'helper'",
      "location": {
        "file": "example.py",
        "line": 7,
        "column": 5
      },
      "related_locations": [
        {
          "file": "example.py",
          "line": 3,
          "column": 1,
          "message": "helper defined here"
        },
        {
          "file": "example.py",
          "line": 4,
          "column": 5,
          "message": "blocking call: time.sleep"
        }
      ]
    }
  ]
}
```

---

### A19: Parse Warnings

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
- 1 warning: "Failed to parse invalid.py: syntax error"
- Analysis continues despite parse failure
