# Strato: Async Blocking Call Detector for Python

> **Status**: Draft – Seeking feedback & expert review

**Strato** is a static analysis tool that detects blocking function calls inside Python async contexts — including the ones hidden behind layers of ordinary function calls that existing linters miss entirely.

### The Problem

Async code that blocks looks correct but silently destroys the concurrency benefits `async` was supposed to provide. When a blocking operation runs inside an async function, it freezes the entire event loop — preventing **all** other async tasks from making progress. These bugs are particularly insidious because the code still *works*, it just performs terribly under load.

Existing tools like flake8-async and ruff's `ASYNC` rules catch **direct** blocking calls. Strato catches **indirect** ones too:

```python
import asyncio
import requests

async def foo():
    loop = asyncio.get_running_loop()
    return await loop.run_in_executor(None, requests.get, "https://example.com")

async def bar():
    return requests.get("https://example.com")

def baz():
    return requests.get("https://example.com")

def qux():
    return

async def main():
    await foo()  # ✅ OK — blocking call properly offloaded to executor
    await bar()  # ⚠️ Direct blocking call — existing tools catch this
    baz()        # ⚠️ Indirect blocking call — only Strato catches this
    qux()        # ✅ OK — qux doesn't block
```

The key insight: `baz()` is a perfectly valid synchronous function. But calling it from an async context is a bug — it will block the event loop. Existing linters don't catch this because they only look at direct calls to known blocking functions, not at the transitive blocking behavior of user-defined functions.

Strato traces through your entire codebase to find these hidden blocking paths and reports them with clear diagnostics pointing at *your* code, not deep in third-party libraries.

