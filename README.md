# Strato

Strato detects **transitive blocking calls in async Python**: the `async handler → sync helper → blocking I/O` bugs that direct async linters usually miss.

> 🔎 Strato is early alpha. Do not treat it as production-ready yet — please try it on real async Python code and send weird cases, false positives, and missed blockers.

## The bug Strato is built for

Async Python is efficient only while the event loop can keep making progress. A single blocking operation — `requests.get`, `time.sleep`, sync file I/O, sync DB drivers, subprocess calls — can freeze the loop and make every other coroutine wait.

Existing tools such as Ruff's `ASYNC` rules and `flake8-async` are good at the direct case:

```python
async def handler():
    time.sleep(1)  # direct blocking call
```

The production bug is often one or more functions away:

```python
import requests

async def handler():
    load_profile()  # looks harmless, but blocks the event loop

def load_profile():
    return requests.get("https://api.example.com/profile").json()
```

`load_profile()` is a perfectly valid synchronous function. The bug is calling it from an async context without offloading it. Strato follows the call graph and reports the async call site that introduced the blocking path:

```text
STRATO002: transitive blocking call reachable from async context
  chain: handler -> load_profile -> requests.get
  help: use an async client, or offload with asyncio.to_thread / run_in_executor
```

That is Strato's core job: find hidden event-loop blockers before teams compensate with extra workers, replicas, and CPU.

## What Strato does

Strato statically analyzes your first-party Python code and:

- marks known blocking functions from a curated database,
- builds a project call graph using semantic resolution from Astral's [`ty`](https://docs.astral.sh/ty/),
- propagates the blocking effect through ordinary sync helpers,
- suppresses propagation through known executor wrappers such as `asyncio.to_thread`, `loop.run_in_executor`, and `anyio.to_thread.run_sync`,
- reports async call sites that reach blocking I/O without an offload boundary.

It is complementary to Ruff and `flake8-async`: they catch direct async footguns; Strato targets the transitive call paths that require whole-project analysis.

## Quickstart

Run Strato against a Python project or directory:

```bash
cargo run -p strato_cli -- check path/to/project
```

For example, from this repository:

```bash
cargo run -p strato_cli -- check tests/fixtures/a02_transitive_blocking --no-cache
```

Machine-readable output is available for CI and code-scanning workflows:

```bash
cargo run -p strato_cli -- check path/to/project --output json
cargo run -p strato_cli -- check path/to/project --output sarif
```

## Safe offloading

Strato should not flag a blocking helper when it is explicitly moved off the event loop:

```python
import asyncio
import requests

def load_profile():
    return requests.get("https://api.example.com/profile").json()

async def handler():
    return await asyncio.to_thread(load_profile)  # OK: offloaded
```

Custom wrappers and project-specific knowledge can be modeled with configuration and decorators such as `@blocking`, `@non_blocking`, and `@unblocker`; see the docs for details.

## Documentation

The mdBook docs live under [`docs/book`](docs/book):

- motivation and problem statement
- analysis pipeline
- call graph and type resolution
- blocking propagation
- blocking database and annotations
- escape hatches / executor wrappers
- output formats including JSON and SARIF

## Status

Strato is currently focused on one narrow, high-value diagnostic: **transitive blocking-call detection for async Python**. The next credibility milestones are real-world scans, false-positive reduction, packaging, and workflow integration.
