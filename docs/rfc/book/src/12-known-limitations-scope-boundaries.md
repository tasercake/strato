# 12. Known Limitations & Scope Boundaries

**Tags**: everyone

### 12.1 Type System Limitations

Strato's type resolution depends on the `ty` crate for inference. When `ty` cannot determine a type, the call is skipped silently per [Decision 3.2](./03-design-decisions.md#32-precision-policy-unknown--not-blocking).

| Limitation | Impact | Mitigation | Status |
|-----------|--------|------------|--------|
| **No-annotation dynamic types** | Variables without type hints or obvious constructors (e.g., `x = get_loader()`) have unknown type. Method calls on `x` are unresolvable. | Add type hints or use `@blocking` decorator. | v1 — by design |
| **Heavily metaprogrammed code** | Classes generated via metaclasses, `type()`, or `__init_subclass__` are invisible to static analysis. | Annotate generated methods with `@blocking`. | v1 — out of scope |
| **Runtime type construction** | `type(name, bases, dict)` creates classes at runtime. Strato cannot resolve calls to methods defined this way. | Avoid runtime class construction in async contexts. | v1 — out of scope |
| **Plugin-based systems** | Frameworks loading callables via entry points or plugin registries are invisible. | Manually annotate plugin callables with `@blocking`. | v1 — out of scope |
| **Generic type parameters** | `T` in `def process(x: T) -> T` is not resolved. Method calls on `x` are unresolvable. | Use concrete types or `@blocking` annotations. | v1 — no generics support |
| **Union types** | `x: Union[A, B]` — Strato does not track which branch is active. | Refactor to avoid unions in async contexts. | v1 — no union tracking |

### 12.2 Import System Limitations

Strato's import resolver handles standard Python import forms but does not support dynamic or runtime-modified imports.

| Limitation | Impact | Mitigation | Status |
|-----------|--------|------------|--------|
| **Dynamic imports** | `importlib.import_module(name)` where `name` is computed at runtime. | Use static imports in async contexts. | v1 — unresolvable |
| **`importlib.import_module` with literal strings** | Even `importlib.import_module("myapp.utils")` is not resolved. | Refactor to `import myapp.utils`. | v1 — not implemented |
| **`.pth` files** | `site-packages/*.pth` files modify `sys.path` at runtime. | Use explicit source roots in config. | v1 — out of scope |
| **Import hooks** | Custom `sys.meta_path` or `sys.path_hooks` importers. | Use standard filesystem-based imports. | v1 — out of scope |
| **Conditional imports (beyond first branch)** | `try: import A; except: import B` — Strato takes the **first branch only**. | Use a single canonical import. | v1 — best-effort |
| **Star imports (transitive)** | `from x import *` is resolved **one level only**. Transitive star imports not followed. | Use explicit imports. | v1 — one level only |
| **Namespace packages (PEP 420)** | Basic support within configured source roots only. External namespace packages not supported. | Add `__init__.py` to all package directories. | v1 — partial |
| **Circular imports** | Symbols registered before bodies walked, but runtime `ImportError` not detected. | Refactor to eliminate circular imports. | v1 — no runtime validation |

### 12.3 Call Graph Limitations

Strato builds a **static call graph** by analyzing function bodies. It cannot resolve calls that depend on runtime state or higher-order function patterns.

| Limitation | Impact | Mitigation | Status |
|-----------|--------|------------|--------|
| **Callbacks passed as arguments** | `def process(callback): callback()` — `callback` unresolvable. | Use `@blocking` on functions that invoke callbacks. | v1 — unresolvable |
| **Higher-order functions returning callables** | `handler = get_handler(); handler()` — unresolvable. | Annotate returned callables with `@blocking`. | v1 — unresolvable |
| **Decorator chains that transform signatures** | Decorators that replace functions with wrappers — Strato analyzes the original function. | Annotate wrappers with `@blocking`. | v1 — original function only |
| **Monkey-patching** | `MyClass.method = some_other_function` — runtime reassignment invisible. | Avoid monkey-patching in async contexts. | v1 — original definition only |
| **Generators and `yield`** | Generator bodies visited, but generator **consumption** (`next(gen())`) does not create call edge to body. | Annotate blocking generators with `@blocking`. | v1 — partial support |
| **`eval()` / `exec()`** | String-based code execution invisible. | Avoid in async contexts. | v1 — out of scope |
| **`getattr()` / `setattr()`** | Dynamic attribute access unresolvable. | Use explicit attribute access. | v1 — unresolvable |
| **`functools.partial`** | Partial application not tracked. | Annotate partial-wrapped functions. | v1 — unresolvable |

### 12.4 Scope Limitations

| Limitation | Impact | Mitigation | Status |
|-----------|--------|------------|--------|
| **asyncio-only** | trio, curio, anyio escape hatches not recognized. Blocking calls wrapped in these are flagged as errors. | Use asyncio, or annotate wrapped functions with `@non_blocking`. | v1 — asyncio only ([Decision 3.16](./03-design-decisions.md#316-async-scope-boundary-asyncio-only)) |
| **No runtime analysis** | Cannot detect blocking calls conditionally skipped at runtime. | Use runtime profiling tools to complement. | v1 — static only |
| **No inter-process analysis** | Blocking calls in subprocesses invisible. | Subprocess code is isolated from event loop. | v1 — out of scope |
| **Single-project only** | Does not traverse into installed third-party packages. | Extend blocking database via config. | v1 — first-party focus |
| **No cross-package analysis** | Monorepo packages analyzed separately. | Run Strato on each package independently. | v1 — single-project only |

### 12.5 "Skip Silently" Behavior

Strato follows a **high-precision policy** ([Decision 3.2](./03-design-decisions.md#32-precision-policy-unknown--not-blocking)): when it cannot definitively prove a call is blocking, it skips silently. This section documents every such case.

| Case | Behavior | Rationale |
|------|----------|-----------|
| **Unresolvable callee** | `resolve_callee()` returns `None` → no call edge created | Unknown != Blocking |
| **Unknown type → no property/dunder edge** | Type inference fails → property/dunder access not checked | Cannot determine if attribute is `@property` without type |
| **External symbol not in DB** | Third-party symbol without database entry → no phantom node | Only known-blocking third-party functions tracked |
| **Unresolvable import** | Dynamic import, missing `__init__.py` → no binding | Cannot analyze what doesn't exist on filesystem |
| **Star import with parse error** | Target module unparseable → no bindings from star import | Cannot enumerate symbols without parsing |
| **Decorator replacing function** | Original function analyzed, not wrapper | Decorators not executed statically |
| **Callback parameter invoked** | `callback()` inside function → unresolvable | Higher-order requires interprocedural analysis |
| **Conditional import (non-first branch)** | Only first branch analyzed | Best-effort: most likely branch |
| **Transitive star import** | `from x import *` where `x` has `from y import *` → `y`'s symbols invisible | One level only — prevents infinite recursion |
| **Monkey-patched method** | Original method analyzed, not patched replacement | Runtime reassignments invisible to static analysis |
| **`eval()` / `exec()` / `getattr()`** | String-based execution/access invisible | Cannot statically analyze runtime-constructed code |

**User guidance:** If Strato misses a blocking call, users can: (1) add type hints to improve resolution, (2) use `@blocking` to manually annotate, (3) refactor dynamic patterns to explicit calls.

### 12.6 Future Work (v2+)

| Feature | Description | Priority | Complexity |
|---------|-------------|----------|------------|
| **trio/anyio/curio support** | Recognize framework-specific escape hatches | High | Medium |
| **Framework integration** | Django `sync_to_async`, FastAPI thread offloading, Celery task dispatch | High | High |
| **Dynamic analysis integration** | Runtime profiling + static call graph correlation | Medium | High |
| **Auto-fix suggestions** | Generate `asyncio.to_thread` wrapping or suggest async alternatives | Medium | Medium |
| **IDE plugin / LSP server** | Real-time diagnostics in editors | Medium | High |
| **Cross-package analysis** | Traverse into installed third-party packages | Medium | High |
| **Incremental graph updates** | Only rebuild affected subgraph on file change | Low | High |
| **Watch mode** | Continuous analysis on file save | Low | Low |
| **GitHub Action** | Pre-built CI integration: `uses: strato-linter/strato-action@v1` | Low | Low |
| **Full trace visualization** | Interactive HTML report with call graph | Low | Medium |
