# Analysis Pipeline

Strato's analysis runs as a seven-phase pipeline, each phase consuming the outputs of the previous:

```
Discovery → Parse → Semantics → Build → Annotate → Propagate → Report
```

Each phase is designed for isolation, testability, and graceful degradation. Failures in early phases (parse errors, semantic resolution failures) are collected as warnings but do not halt analysis.

### Phase 1: Discovery

**Objective:** Enumerate all Python files in the project and classify them as first-party or third-party.

**Steps:**

1. **Load configuration** from `pyproject.toml` under `[tool.strato]`:
   - `src_roots`: explicit list of directories containing first-party code
   - `exclude`: glob patterns for files/directories to skip

2. **Auto-detect source roots** if `src_roots` is not explicitly configured:
   - Check `[tool.setuptools.packages.find]` for `where` directive
   - Fall back to common layouts: `src/` directory if present, otherwise project root
   - Scan for top-level `__init__.py` files to identify package roots

3. **Build file manifest:**
   - Recursively walk all source roots and collect `.py` files
   - Compute SHA-256 content hash for each file (used for incremental analysis caching)
   - Classify each file:
     - **First-party:** file path is under any configured source root
     - **Third-party:** everything else (site-packages, stdlib, external dependencies)

**Output:** `FileManifest` containing:
- `files: Vec<FileEntry>` where `FileEntry = { path, content_hash, is_first_party }`
- `source_roots: Vec<PathBuf>` (internal derived value from configured `src_roots` or auto-detection)

### Phase 2: Parse

**Objective:** Parse all Python files into Strato-owned ASTs and extract syntactic declarations needed by later phases.

**Steps:**

1. **Parse all files in parallel** using `ruff_python_parser`:
   - Parallelized via `rayon::par_iter()` (embarrassingly parallel workload)
   - Parse errors are **non-fatal**: collected as `AnalysisWarning::ParseError { path, error }`
   - Analysis continues on all successfully parsed files

2. **Extract `FileSyntax`** from each AST:
   - **Function/method definitions:** name, qualified path, `is_async` flag, location
   - **Class definitions:** name, base classes, location
   - **Import statements:** module, imported names, aliases, relative level
   - **Decorators:** applied to functions/classes (e.g., `@blocking`, `@property`)

3. **Parser abstraction layer:**
   - All ruff parser access goes through `trait PythonParser`:
     ```
     trait PythonParser {
         fn parse(&self, source: &str) -> Result<ParsedModule, ParseError>;
     }
     ```
   - Isolates analysis logic from ruff API changes
   - Enables test mocking with synthetic ASTs

**Output:** `ParsedFiles = BTreeMap<PathBuf, ParsedModule>` where `ParsedModule = { ast, syntax }`. The ordered map is part of the determinism contract for Strato-owned iteration.

### Phase 3: Semantics (ty Semantic Context)

**Objective:** Initialize ty over the project and expose only the stable semantic facts Strato needs for blocking analysis.

**Risk:** This is the **highest-risk integration point** of the pipeline. Python's import and type semantics are complex, and ty is pre-1.0.

#### Strato-owned setup

1. Configure ty with the same source roots, Python version, and stub paths used by Strato discovery.
2. Provide the discovered file set and source text to ty's semantic database.
3. Run Strato syntactic extraction from Phase 2 in parallel with, but not as a replacement for, ty's own semantic model.
4. Normalize facts consumed from ty into deterministic Strato identifiers before graph construction.

#### Facts Strato consumes from ty

| Fact | Used For |
|------|----------|
| Resolved callable target for a call expression | Direct call and alias edge construction |
| Resolved first-party definition identity | Mapping semantic targets to `CallGraphNode`s |
| External qualified name when available | Matching calls against blocking database phantom nodes |
| Attribute target and owning class | Method/property edge construction |
| Class hierarchy lookup for an operation | Dunder edge construction |

Strato does not serialize ty facts, expose ty's internal types in public APIs, or maintain a parallel import resolver. If ty cannot provide a fact, Strato treats the expression as unknown and creates no edge.

**Output:** `SemanticContext` plus a deterministic `SemanticFactSet` for facts already queried during this run. The semantic context is in-memory only.

### Phase 4: Build (Call Graph Construction)

**Objective:** Construct a directed graph of all function calls in the codebase.

This phase walks the AST of every function body and records call edges. Callee resolution uses the ty-backed semantic context from Phase 3 and normalized Strato identifiers, not a separate Strato module resolver.

**Detailed algorithm in [Call Graph & Type Resolution](./call-graph-type-resolution.md#call-graph--type-resolution).**

**Output:** `CallGraph = { nodes: Vec<CallGraphNode>, edges: Vec<CallEdge> }`

### Phase 5: Annotate

**Objective:** Mark known blocking functions using the blocking database and decorator annotations.

**Steps:**

1. **Load blocking database:** JSON file mapping qualified names to blocking status:
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

3. **Scan `.pyi` stub files** for type annotations:
   - Look for `# strato: blocking` comments in stubs
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
2. **Walk outgoing edges** from each async function
3. **Report violations** where:
   - Edge target has `blocking_status = KnownBlocking | PropagatedBlocking`
   - Edge does **not** have `in_executor = true`
4. **Format reports** with location, call chain, and suggested fixes

**Detailed output format in [Error Reporting & Diagnostics](./error-reporting-diagnostics.md#error-reporting--diagnostics).**

**Output:** `Vec<Violation>`
