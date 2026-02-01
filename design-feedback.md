Some notes to iterate on the design:
- In help notes under errors, do not mention specific third-party libraries as alternatives (e.g. httpx instead of requests). As a general rule, point out the issue clearly but don't be overly prescriptive about the solution
- The error reporting for 'Blocking property access in async context' seems a little off. The error should have been reported at the site where `user.avatar` was accessed, not where `requests.get` was called.
- Is 'un-tainting' supported in the current design? By that I mean: if I wrap a blocking call in something like `asyncio.to_thread()`, it should remove the 'blocking' taint from any blocking functions. Having a built-in database of such functions is great but this needs to be extensible as well. Ideally, there would be a decorator to mark functions as 'un-blocking' (distinct from non-blocking), whose role is to *remove* the blocking taint from a wrapped function.
- files skipped due to syntax errors should be noted somewhere, even if we don't emit an error that's visible in the CLI or IDE.
- You said 'from x import * — opaque, skip it'. Is this a fundamental limitation of Python static analysis, or a pragmatic decision to reduce complexity?
- You said we won't handle module resolution for namespace packages. Is this a fundamental limitation of Python static analysis, or a pragmatic decision to reduce complexity?
- In general, all the known un-handled cases must be clearly documented - for clarity of scope, pre-empting bug reports, and to inspire future work.
- Type inference right now is minimal by design to keep it simple & precise, but is it possible to leverage an existing library to handle the bulk of the work here? This would give a huge boost to usefulness in lightly-typed codebases. Same for cases like Inheritance / MRO handling.
- Does blocking function recognition work with various import/naming patterns? For example, all of the following cases must be caught:
    - `import requests; requests.get(...)`
    - `from requsts import get; get(...)`
    - `import requests; req_get = requests.get; req_get(...)`
    - `from requsts import get as rget; rget(...)`
    - And any behaviorally equivalent patterns that don't involve dynamic access

Spin up parallel subagents to assess each of these notes in detail.
Ask me as many questions as needed to clear up ambiguity.
Update the design docs in .sisyphus/plans/ based on your new insights.