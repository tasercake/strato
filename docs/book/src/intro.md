# Strato: transitive blocking-call detection for async Python

> **Status**: Early alpha — seeking real-world test cases, false positives, and missed blockers.

**Strato** is a static analysis tool for one specific async Python failure mode: a coroutine calls an ordinary synchronous helper, and that helper eventually performs blocking I/O.

In shorthand:

```text
async handler -> sync helper -> blocking I/O
```

That is the case direct async linters usually miss.

## The problem

Async Python only delivers concurrency while the event loop can keep making progress. A blocking call such as `requests.get`, `time.sleep`, synchronous file I/O, a sync database driver, or a subprocess call can freeze the loop and make every other coroutine wait.

The direct version is easy to see:

```python
async def handler():
    time.sleep(1)  # direct blocking call
```

Tools like Ruff's `ASYNC` rules and `flake8-async` are useful here. Strato is aimed at the version that tends to survive into production:

```python
import requests

async def handler():
    load_profile()  # looks harmless, but blocks the event loop

def load_profile():
    return requests.get("https://api.example.com/profile").json()
```

`load_profile()` is valid synchronous Python. The bug is calling it from an async context without offloading it. Strato follows the call graph and reports the async call site that introduced the blocking path:

```text
STRATO002: transitive blocking call reachable from async context
  chain: handler -> load_profile -> requests.get
  help: use an async client, or offload with asyncio.to_thread / run_in_executor
```

## What Strato does

Strato uses semantic resolution from Astral's [`ty`](https://docs.astral.sh/ty/) to understand which functions and methods a call expression refers to. On top of that, Strato:

1. marks known blocking functions from a curated database,
2. builds a call graph for first-party project code,
3. propagates a blocking effect through ordinary sync helpers,
4. stops propagation at known offload boundaries such as `asyncio.to_thread`, `loop.run_in_executor`, and `anyio.to_thread.run_sync`,
5. reports async call sites that reach blocking I/O without an offload boundary.

The goal is not to replace Ruff, `flake8-async`, runtime `asyncio` debug mode, or APM. Those tools cover adjacent layers. Strato targets the transitive static layer: the hidden sync call path that makes async code look correct while silently destroying concurrency.

## Why this matters

Teams often compensate for blocked event loops by increasing workers, replicas, and CPU. That can be the right emergency move, but it also hides the underlying bug. Strato is meant to make that hidden sync-call tax visible before it turns into production latency or infrastructure spend.

The rest of this book describes the analysis pipeline, blocking-function database, executor wrappers, annotations, escape hatches, diagnostics, and output formats.
