# Strato

Strato is a linter designed to detect common pitfalls in Python's asynchronous (asyncio) code.

> 🔎 Strato is currently in the research & design phase. Nothing has been implemented yet. Open to comments and feedback 🙂

The core problem it addresses: async code that looks correct but actually defeats the purpose of async programming. When a blocking operation (like reading a file synchronously, making a blocking HTTP request, or calling time.sleep()) runs inside an async function, it freezes the entire event loop – preventing all other async tasks from making progress. These bugs are particularly insidious because the code still works, it just silently destroys the concurrency benefits async was supposed to provide.

The project's key ambition goes beyond detecting direct blocking calls (which existing tools already catch). It aims to detect indirect blocking – when an async function calls a regular function that internally makes blocking calls. This requires cross-function and potentially cross-file analysis to trace call chains and determine whether a seemingly innocent function call will ultimately block the event loop.

---

## Motivating Example

```python
import asyncio
import requests
async def foo():
    loop = asyncio.get_running_loop()
    return await loop.run_in_executor(None, requests.get, "https://example.com")  # OK - properly offloaded
async def bar():
    return requests.get("https://example.com")  # Error! Direct blocking call (existing tools catch this)
def baz():
    return requests.get("https://example.com")  # OK in isolation - it's a sync function
def qux():
    return  # OK - no blocking
async def main():
    await foo()  # OK
    await bar()  # OK (the error is in bar's body, not here)
    baz()        # Error! Calling a blocking sync function from async context
    qux()        # OK - qux doesn't block
```

The key insight: baz() itself is a perfectly valid synchronous function. But calling it from an async context is a bug – it will block the event loop. Existing linters don't catch this because they only look at direct calls to known blocking functions, not at the transitive blocking behavior of user-defined functions.
