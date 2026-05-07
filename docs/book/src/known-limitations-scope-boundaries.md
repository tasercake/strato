# Known Limitations & Scope Boundaries

**Tags**: everyone

### Type System Limitations

Strato's semantic resolution depends on the Strato facade over vendored Ruff/ty for module, name, type, call, property, and dunder facts. When a supported facade query returns `Unknown` for an individual expression, the call is skipped silently per the [Precision Policy](./design-overview.md#precision-policy).

| Limitation | Impact | Mitigation | Status |
|-----------|--------|------------|--------|
| **No-annotation dynamic types** | Variables whose type cannot be inferred by ty have unknown type. Method calls on those values are unresolvable. | Add type hints or use `@blocking` decorator. | v1 – by design |
| **Heavily metaprogrammed code** | Classes generated via metaclasses, `type()`, or `__init_subclass__` are invisible to static analysis. | Annotate generated methods with `@blocking`. | v1 – out of scope |
| **Runtime type construction** | `type(name, bases, dict)` creates classes at runtime. Strato cannot resolve calls to methods defined this way. | Avoid runtime class construction in async contexts. | v1 – out of scope |
| **Plugin-based systems** | Frameworks loading callables via entry points or plugin registries are invisible. | Manually annotate plugin callables with `@blocking`. | v1 – out of scope |
| **Generic type parameters** | `T` in `def process(x: T) -> T` is not resolved. Method calls on `x` are unresolvable. | Use concrete types or `@blocking` annotations. | v1 – no generics support |
| **Union types** | `x: Union[A, B]` – Strato does not track which branch is active. | Refactor to avoid unions in async contexts. | v1 – no union tracking |

### Import System Limitations

Strato relies on vendored Ruff/ty for static import semantics under configured source roots, Python version, and stub paths mapped to ty `environment.extra-paths`. Dynamic or runtime-modified imports remain outside Strato's guarantees.

| Limitation | Impact | Mitigation | Status |
|-----------|--------|------------|--------|
| **Dynamic imports** | `importlib.import_module(name)` where `name` is computed at runtime. | Use static imports in async contexts. | v1 – unresolvable |
| **Runtime import calls even with literal strings** | Strato does not special-case `importlib.import_module("myapp.utils")` as a static import. | Refactor to `import myapp.utils`. | v1 – not implemented |
| **`.pth` files** | `site-packages/*.pth` files modify `sys.path` at runtime. | Use explicit source roots in config. | v1 – out of scope |
| **Import hooks** | Custom `sys.meta_path` or `sys.path_hooks` importers. | Use standard filesystem-based imports. | v1 – out of scope |
| **Conditional imports** | Resolution follows ty's static semantics; Strato does not execute both runtime branches. | Use a single canonical import. | v1 – best-effort |
| **Star imports** | Supported only in happy-path cases where ty can statically enumerate the exported names. | Use explicit imports. | v1 – best-effort |
| **Namespace packages (PEP 420)** | Happy-path first-party namespace packages can resolve under configured source roots; support otherwise depends on ty and stub/source-root configuration. External namespace packages are not a Strato guarantee. | Add `__init__.py` or explicit source roots where possible. | v1 – partial |
| **Circular imports** | Symbols registered before bodies walked, but runtime `ImportError` not detected. | Refactor to eliminate circular imports. | v1 – no runtime validation |

### Call Graph Limitations

Strato builds a **static call graph** by analyzing function bodies. It cannot resolve calls that depend on runtime state or higher-order function patterns.

| Limitation | Impact | Mitigation | Status |
|-----------|--------|------------|--------|
| **Callbacks passed as arguments** | `def process(callback): callback()` – `callback` unresolvable. | Use `@blocking` on functions that invoke callbacks. | v1 – unresolvable |
| **Higher-order functions returning callables** | `handler = get_handler(); handler()` – unresolvable. | Annotate returned callables with `@blocking`. | v1 – unresolvable |
| **Decorator chains that transform signatures** | Decorators that replace functions with wrappers – Strato analyzes the original function. | Annotate wrappers with `@blocking`. | v1 – original function only |
| **Monkey-patching** | `MyClass.method = some_other_function` – runtime reassignment invisible. | Avoid monkey-patching in async contexts. | v1 – original definition only |
| **Generators and `yield`** | Generator bodies visited, but generator **consumption** (`next(gen())`) does not create call edge to body. | Annotate blocking generators with `@blocking`. | v1 – partial support |
| **`eval()` / `exec()`** | String-based code execution invisible. | Avoid in async contexts. | v1 – out of scope |
| **`getattr()` / `setattr()`** | Dynamic attribute access unresolvable. | Use explicit attribute access. | v1 – unresolvable |
| **General `functools.partial` flow** | Partial application is not tracked as a general callable value outside recognized executor-wrapper arguments. | Use direct calls or annotate/configure the wrapper that receives the callable. | v1 – limited support |

### Scope Limitations

| Limitation | Impact | Mitigation | Status |
|-----------|--------|------------|--------|
| **asyncio-only** | trio, curio, and anyio framework semantics are not modeled in v1. Built-in escape hatches are asyncio-only. | Use asyncio for v1, or mark project-specific safe boundaries explicitly. | v1 – asyncio only ([Async Library Support](./design-overview.md#async-library-support)) |
| **No runtime analysis** | Cannot detect blocking calls conditionally skipped at runtime. | Use runtime profiling tools to complement. | v1 – static only |
| **No inter-process analysis** | Blocking calls in subprocesses invisible. | Subprocess code is isolated from event loop. | v1 – out of scope |
| **Single-project only** | Does not traverse into installed third-party packages. | Extend blocking database via config. | v1 – first-party focus |
| **No cross-package analysis** | Monorepo packages analyzed separately. | Run Strato on each package independently. | v1 – single-project only |

### "Skip Silently" Behavior

Strato follows a **high-precision policy** ([Precision Policy](./design-overview.md#precision-policy)): when it cannot definitively prove a call is blocking, it skips silently. This section documents every such case.

| Case | Behavior | Rationale |
|------|----------|-----------|
| **Unresolvable callee** | Facade has no callable target → no call edge created | Unknown != Blocking |
| **Unknown semantic target → no property/dunder edge** | The facade cannot resolve the property or dunder target → access not checked | Cannot prove which callable would run |
| **External symbol not in DB** | Third-party symbol without database entry → no phantom node | Only known-blocking third-party functions tracked |
| **Unresolvable import** | Dynamic import, import hook, runtime path mutation, or missing module → no binding | Cannot analyze what is not available through static filesystem-backed import semantics |
| **Star import with severe syntax errors** | Target module cannot provide a safe export set → no bindings from star import | Cannot enumerate symbols without reliable declarations |
| **Decorator replacing function** | Original function analyzed, not wrapper | Decorators not executed statically |
| **Callback parameter invoked** | `callback()` inside function → unresolvable | Higher-order requires interprocedural analysis |
| **Conditional import branch not resolved by ty** | Binding unavailable to Strato | Best-effort static semantics |
| **Star import not statically enumerable** | Exported names unavailable to Strato | Avoid guessing imported names |
| **Monkey-patched method** | Original method analyzed, not patched replacement | Runtime reassignments invisible to static analysis |
| **`eval()` / `exec()` / `getattr()`** | String-based execution/access invisible | Cannot statically analyze runtime-constructed code |

**User guidance:** If Strato misses a blocking call, users can: (1) add type hints to improve resolution, (2) use `@blocking` to manually annotate, (3) refactor dynamic patterns to explicit calls.

### Future Work (v2+)

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
