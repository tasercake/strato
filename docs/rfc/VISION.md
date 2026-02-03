# Strato: Catching the Async Bugs Nobody Else Can

## The Problem

Python's `async`/`await` promises concurrency — but a single misplaced blocking call can silently freeze your entire application.

```python
async def handle_request(user_id):
    user = get_user(user_id)      # Looks fine...
    return {"name": user.name}

def get_user(user_id):
    return db.query(user_id)      # ...but this blocks the entire event loop
```

This code works. It passes tests. It deploys successfully. But in production, every call to `handle_request` stalls all concurrent operations — defeating the entire purpose of async. The server that should handle thousands of requests simultaneously grinds to a crawl.

These bugs are **invisible to existing tools**. Linters like flake8-async and ruff's ASYNC rules catch obvious cases — `time.sleep()` directly inside an `async def`. But the moment blocking code hides behind a regular function call, every existing tool goes blind.

## The Insight

The real danger isn't `time.sleep()` in an async function — any developer can spot that. The real danger is a perfectly innocent-looking helper function that, three calls deep, reads a file, queries a database, or makes a synchronous HTTP request. The blocking call is invisible at the point where it matters.

Catching this requires something no existing Python linter does: **tracing call chains across functions and files** to determine whether a seemingly safe function call will ultimately block the event loop.

## What Strato Does

Strato is a static analysis tool that builds a project-wide call graph of your Python code and traces the "blocking" property through function call chains. If an async function calls a sync function, which calls another sync function, which eventually calls `requests.get()` — Strato finds it and tells you exactly where the problem is.

```
error[STRATO002]: Blocking call reachable from async context

  --> src/api/handlers.py:12:5
   |
12 |     user = get_user(user_id)
   |     ^^^^^^^^^^^^^^^^^^^^^^^^ leads to blocking call
   |
   = chain: handle_request() → get_user() → db.query() → psycopg2.connect()
   = help: Offload to a thread with `asyncio.to_thread()`, or use an async database driver
```

No other tool does this.

## Key Properties

- **High precision over high recall**: Strato only reports issues it can prove. If it can't trace a call chain, it stays silent rather than guessing. Zero false positives is the goal.
- **Understands executor patterns**: `run_in_executor`, `asyncio.to_thread`, and custom wrappers are recognized as safe — blocking calls properly offloaded to a thread pool don't trigger errors.
- **Extensible**: A built-in database of 80+ known blocking functions (stdlib + popular libraries) covers common cases. User annotations (`@blocking`, `@non_blocking`) and configuration handle the rest.
- **Fast**: Built in Rust, using the same parser as ruff. Designed for sub-second cached runs in CI.

## Current Status

Strato is in the design phase. The architecture is specified, the algorithms are defined, and the implementation plan is ready. We are seeking expert feedback on the design before building.

## We Want Your Feedback

We're particularly interested in:

1. **Detection coverage**: Are there important blocking patterns we're missing?
2. **Precision tradeoffs**: Is our "silent on uncertainty" approach the right default?
3. **The call graph approach**: Is full transitive analysis the right level of ambition, or is there a simpler approach that captures 90% of the value?
4. **Practical usability**: Will the error messages and intervention strategies actually help developers fix their code?
