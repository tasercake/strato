# Analysis Pipeline

Strato's analysis runs as a seven-phase pipeline, each phase consuming the outputs of the previous:

```
Discovery → Parse → Semantics → Build → Annotate → Propagate → Report
```

Each phase is designed for isolation, testability, and graceful degradation. Failures in early phases (syntax errors, semantic resolution failures) are collected as warnings but do not halt analysis.

### Phase 1: Discovery

**Objective:** Enumerate all Python files in the project and classify them as first-party or third-party.

**Steps:**

1. **Load configuration** from `pyproject.toml` under `[tool.strato]`:
   - `src_roots`: explicit list of directories containing first-party code
   - `exclude`: glob patterns for files/directories to skip
   - blocking database extensions and removals from `[tool.strato.blocking]`
   - executor-wrapper configuration from `[tool.strato.executor-wrappers]`

2. **Auto-detect source roots** if `src_roots` is not explicitly configured:
   - Check `[tool.setuptools.packages.find]` for `where` directive
   - Fall back to common layouts: `src/` directory if present, otherwise project root
   - Scan for top-level `__init__.py` files to identify package roots

3. **Build file manifest:**
   - Recursively walk all source roots and collect `.py` and `.pyi` files
   - Recursively walk `stub_paths` and collect `.pyi` files
   - Compute SHA-256 content hash for each file (used for incremental analysis caching)
   - Classify each file:
     - **Source:** `.py` file used for body analysis and call graph construction
     - **Stub:** `.pyi` file used for annotation/declaration metadata only
     - **First-party:** file path is under any configured source root
     - **Third-party stub:** `.pyi` file under `stub_paths`
    - Load and normalize the effective blocking database from built-ins plus user config so Phase 4 can materialize external phantom nodes deterministically when resolved calls reference them.

**Output:** `FileManifest` containing:
- `files: Vec<FileEntry>` where `FileEntry = { path, content_hash, kind: FileKind, is_first_party }`
- `source_roots: Vec<PathBuf>` (internal derived value from configured `src_roots` or auto-detection)
- `blocking_database: BlockingDatabase` (built-ins plus config additions/removals)
- `escape_hatch_config: EscapeHatchConfig`

### Phase 2: Parse

**Objective:** Load Ruff parsed modules from the vendored Ruff/ty project database and extract Strato-owned syntactic declarations needed by later phases.

**Steps:**

1. **Initialize the vendored Ruff/ty project database** using `ty_project::ProjectDatabase`:
   - Ruff is vendored as a pinned monorepo submodule under `vendor/ruff`.
   - Strato may apply surgical patches to vendored Ruff/ty to expose semantic facts required by the facade.
   - Project settings, source roots, Python version, and include/exclude patterns are translated into ty project metadata.
   - `stub_paths` are translated to ty `environment.extra-paths` so they participate in module resolution without becoming first-party project roots.

2. **Load parsed modules** through `ruff_db::parsed::parsed_module`:
    - Ruff's parser remains the parser of record, but Strato does not maintain a second independent AST parse when the ty database can provide one.
   - Ruff's parser is error-resilient: syntax errors are exposed on the parsed module rather than treated as ordinary parser failure.
   - Syntax errors are **non-fatal**: collected as `AnalysisWarning::SyntaxError { path, error }`
   - Analysis continues on every module from which Strato can safely extract declarations.

3. **Extract `FileSyntax`** from each parsed module:
    - **Function/method definitions:** name, qualified path, `is_async` flag, location
    - **Class definitions:** name, base classes, location
    - **Import statements:** module, imported names, aliases, relative level
    - **Decorator syntax:** raw decorator expressions applied to functions/classes (semantic classification happens after facade resolution)
   - `.py` source files contribute declarations and bodies for graph construction.
   - `.pyi` stub files contribute declarations and decorator syntax only; stub bodies are never walked for call edges.

4. **Adapter boundary:**
   - All direct Ruff/ty access goes through `strato_ty_adapter`.
   - `strato_core` receives Strato-owned syntax and semantic target types, not raw ty internals except where an opaque handle is required.
   - Tests mock the adapter facade rather than inventing a separate parser abstraction.

**Output:** `ParsedFiles = BTreeMap<PathBuf, ParsedModuleRef>` plus `FileSyntax` extracted from source and stub modules. The ordered map is part of the determinism contract for Strato-owned iteration.

### Phase 3: Semantics (Strato ty Facade)

**Objective:** Query a narrow Strato-owned facade over vendored Ruff/ty and expose only the semantic facts Strato needs for blocking analysis.

**Risk:** This is the **highest-risk integration point** of the pipeline. Python's import and type semantics are complex, ty is pre-1.0, and Strato intentionally vendors and patches Ruff/ty APIs where needed.

#### Strato-owned setup

1. Keep `vendor/ruff` pinned to an audited commit and record local Strato patches.
2. Configure `ty_project::ProjectDatabase` with Strato's source roots, Python version, include/exclude patterns, and stub paths mapped as ty `environment.extra-paths`.
3. Use `ty_module_resolver` for module and search-path semantics; Strato does not implement a parallel module resolver.
4. Expose required facts through `strato_ty_adapter::StratoTyFacade`.
5. Normalize facts consumed from the facade into deterministic Strato identifiers before graph construction.

#### Facade API surface

The facade is allowed to call patched vendored Ruff/ty internals. `strato_core` is not.

```rust
enum ResolvedTarget {
    FirstPartyDefinition(DefinitionKey),
    ExternalQualifiedNames(BTreeSet<String>),
    Unknown,
}

trait StratoTyFacade {
    fn files(&self) -> Vec<FileId>;
    fn parsed_module(&self, file: FileId) -> Option<ParsedModuleRef>;
    fn callables_in_file(&self, file: FileId) -> Vec<CallableInfo>;
    fn resolve_call_target(&self, file: FileId, call: &ExprCall) -> ResolvedTarget;
    fn resolve_callable_reference(&self, file: FileId, expr: &Expr) -> ResolvedTarget;
    fn resolve_attribute_target(&self, file: FileId, attr: &ExprAttribute) -> ResolvedTarget;
    fn resolve_property_getter(&self, file: FileId, attr: &ExprAttribute) -> ResolvedTarget;
    fn resolve_dunder_target(&self, file: FileId, operation: DunderOperation) -> Vec<ResolvedTarget>;
    fn resolves_to_event_loop_run_in_executor(&self, file: FileId, call: &ExprCall) -> bool;
}
```

Vendored Ruff/ty patches should expose facts, not Strato policy. Blocking classification, executor suppression, propagation, and diagnostics remain outside the vendored tree.

The v1 adapter requires explicit vendored Ruff/ty patch APIs for `definitions_for_call`, `definitions_for_callable_reference`, descriptor-aware property getter resolution, `definitions_for_dunder_operation`, external qualified alias derivation, and deterministic definition qualified names. These patch APIs are milestone M-1 deliverables, not optional enhancements.

#### Facts Strato consumes from ty

| Fact | Used For |
|------|----------|
| Resolved callable target for a call expression | Direct call and alias edge construction |
| Resolved callable target for a callable reference | Executor-wrapper synthetic edges |
| Resolved first-party definition identity | Mapping semantic targets to `CallGraphNode`s |
| External qualified aliases when available | Matching calls against blocking database phantom nodes, including public aliases and implementation-definition names |
| Attribute target and owning class | Method/property edge construction |
| Class hierarchy lookup for an operation | Dunder edge construction |
| Decorator target identity | Classification of `@blocking`, `@non_blocking`, and `@unblocker` syntax |
| Event-loop `run_in_executor` target identity | Executor-wrapper detection without a Strato-owned local resolver |

Strato does not serialize ty facts, expose ty's internal types from `strato_core`, or maintain a parallel import resolver. If a supported facade query returns `Unknown` for an individual expression, Strato creates no edge for that expression. External targets carry a deterministic set of possible qualified aliases because ty/typeshed may resolve a public symbol through an implementation module, inherited base class, or re-exported definition.

**Output:** `StratoTyProject` plus deterministic normalized semantic targets queried during this run. The ty project database and facade state are in-memory only.

### Phase 4: Build (Call Graph Construction)

**Objective:** Construct a directed graph of all function calls in the codebase.

This phase starts by preparing a deterministic phantom-node index from the effective `BlockingDatabase` loaded in Phase 1. Phantom nodes are materialized into the call graph only when a resolved call target references them. It then walks the AST of every source-file function body and records call edges. Stub-file bodies are never walked. Callee resolution uses the Strato ty facade from Phase 3 and normalized Strato identifiers, not a separate Strato module resolver.

**Detailed algorithm in [Call Graph & Type Resolution](./call-graph-type-resolution.md#call-graph--type-resolution).**

**Output:** `CallGraph = { nodes: Vec<CallGraphNode>, edges: Vec<CallEdge> }`

### Phase 5: Annotate

**Objective:** Mark known blocking functions using the blocking database and decorator annotations.

**Steps:**

1. **Use the effective blocking database loaded in Phase 1:** built-ins plus project config mapping qualified names to blocking status:
   ```json
   {
     "requests.get": "blocking",
     "time.sleep": "blocking",
     "asyncio.sleep": "non_blocking"
   }
   ```

2. **Scan for decorator annotations:**
   - `@blocking` decorator explicitly marks a function as blocking
   - `@non_blocking` decorator explicitly marks a function as non-blocking
   - Decorators override database entries

3. **Apply `.pyi` stub annotations:**
   - Resolve decorators collected from stubs to `strato` annotations through the facade
   - Useful for annotating third-party libraries without modifying source

4. **Update `CallGraphNode.blocking_status`:**
   - Set to `KnownBlocking` or `KnownNonBlocking` for annotated nodes
   - Leave as `Unknown` for unannotated nodes

**Output:** Updated `CallGraph` with annotated nodes

### Phase 6: Propagate

**Objective:** Propagate blocking status through the call graph to infer blocking behavior of unannotated functions.

If function `f` calls blocking function `g`, then `f` is also blocking (unless the call is wrapped in `asyncio.to_thread` or similar executor).

**Detailed algorithm in [Blocking Propagation](./blocking-propagation.md#blocking-propagation).**

**Output:** Fully annotated `CallGraph` with `PropagatedBlocking` status

### Phase 7: Report

**Objective:** Generate violation reports for blocking calls in async contexts.

**Steps:**

1. **Find all async functions** in the call graph
2. **Read each async node's propagated `BlockingReason`**, which stores the selected shortest path from async context to blocking root
3. **Report violations** for async nodes with `blocking_status = PropagatedBlocking | KnownBlocking` when the selected path is not protected by executor edges
4. **Format reports** with primary call-site location, call chain, and suggested fixes

**Detailed output format in [Error Reporting & Diagnostics](./error-reporting-diagnostics.md#error-reporting--diagnostics).**

**Output:** `Vec<Violation>`
