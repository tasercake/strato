# Analysis Pipeline

Strato's analysis runs as a seven-phase pipeline, each phase consuming the outputs of the previous:

```
Discovery → Parse → Resolve → Build → Annotate → Propagate → Report
```

Each phase is designed for isolation, testability, and graceful degradation. Failures in early phases (parse errors, resolution failures) are collected as warnings but do not halt analysis.

### Phase 1: Discovery

**Objective:** Enumerate all Python files in the project and classify them as first-party or third-party.

**Steps:**

1. **Load configuration** from `pyproject.toml` under `[tool.strato]`:
   - `source_roots`: explicit list of directories containing first-party code
   - `exclude`: glob patterns for files/directories to skip
   - `blocking_db_path`: path to blocking function database

2. **Auto-detect source roots** if not explicitly configured:
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
- `source_roots: Vec<PathBuf>`

### Phase 2: Parse

**Objective:** Parse all Python files into ASTs and extract symbol definitions.

**Steps:**

1. **Parse all files in parallel** using `ruff_python_parser`:
   - Parallelized via `rayon::par_iter()` (embarrassingly parallel workload)
   - Parse errors are **non-fatal**: collected as `AnalysisWarning::ParseError { path, error }`
   - Analysis continues on all successfully parsed files

2. **Extract `FileSymbols`** from each AST:
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

**Output:** `ParsedFiles = HashMap<PathBuf, ParsedModule>` where `ParsedModule = { ast, symbols }`

### Phase 3: Resolve (Module Resolution)

**Objective:** Map Python import statements to source files and build a global symbol table.

**Risk:** This is the **highest-risk component** of the pipeline. Python's import system is notoriously complex, and edge cases abound.

#### Supported Import Forms

| Import Form | Example | Resolution Strategy |
|-------------|---------|---------------------|
| Absolute | `import foo.bar` | Lookup `foo/bar.py` or `foo/bar/__init__.py` in source roots |
| From-import | `from foo.bar import baz` | Resolve `foo.bar` module, then lookup `baz` symbol |
| Relative | `from . import sibling` | Resolve relative to current module's parent |
| Relative-from | `from ..pkg import mod` | Walk up directory tree by relative level |
| Package `__init__.py` | `import pkg` | Resolve to `pkg/__init__.py` |
| Multi-level | `from a.b.c.d import e` | Iteratively resolve each component |
| `.pyi` stubs | `import foo` | Prefer `foo.pyi` over `foo.py` if present |

#### Unsupported Import Forms

| Import Form | Example | Why Unsupported |
|-------------|---------|-----------------|
| Star imports | `from foo import *` | Partially supported: see algorithm below |
| Conditional imports | `if sys.version_info >= (3, 10): import x` | Best-effort: analyze first branch only |
| Dynamic imports | `importlib.import_module(var)` | Requires runtime information |
| Namespace packages | `import namespace.pkg` | Partially supported: see below |
| `.pth` files | `site-packages/custom.pth` | Requires runtime sys.path manipulation |
| Import hooks | `sys.meta_path.append(...)` | Arbitrary code execution at import time |

#### Star Import Resolution Algorithm

Star imports (`from foo import *`) are resolved with limited scope:

1. Parse the target module (`foo`)
2. Look for a literal `__all__` assignment:
   - If `__all__ = ["a", "b", "c"]` exists, import only those names
   - If `__all__` is dynamically constructed, skip (treat as unresolvable)
3. If no `__all__`, collect all public top-level names (not starting with `_`)
4. **One level only:** do not recursively resolve star imports in the target module

#### Namespace Package Support

Basic support for PEP 420 namespace packages:

- Directories **without** `__init__.py` are treated as namespace packages **only within configured source roots**
- Resolution algorithm checks for regular packages first (with `__init__.py`), then falls back to namespace package lookup
- External namespace packages (e.g., in site-packages) are not supported

#### Resolution Algorithm Pseudocode

```
fn resolve_import(import_stmt, current_module_path, source_roots):
    if import_stmt.is_relative():
        base_path = walk_up(current_module_path, import_stmt.level)
        module_path = base_path.join(import_stmt.module)
    else:
        module_path = import_stmt.module
    
    for root in source_roots:
        candidates = [
            root / module_path.with_suffix(".pyi"),
            root / module_path.with_suffix(".py"),
            root / module_path / "__init__.pyi",
            root / module_path / "__init__.py",
        ]
        for candidate in candidates:
            if candidate.exists():
                return ResolvedModule { path: candidate, kind: File }
        
        # Namespace package fallback
        if (root / module_path).is_dir():
            return ResolvedModule { path: root / module_path, kind: NamespacePackage }
    
    return None  # Unresolved (external or missing)
```

#### Data Structures

- **`ModuleMap`:** `HashMap<ModulePath, FilePath>` – maps Python module paths (e.g., `foo.bar.baz`) to source files
- **`SymbolTable`:** `HashMap<QualifiedName, SymbolDef>` – maps fully qualified names (e.g., `foo.bar.MyClass.method`) to definitions
- **`ResolvedModule`:** `{ path: PathBuf, kind: ModuleKind }` where `ModuleKind = File | Package | NamespacePackage`
- **`SymbolDef`:** `enum { Function, Class, Variable, Import }`

**Output:** `ModuleMap` and `SymbolTable`

### Phase 4: Build (Call Graph Construction)

**Objective:** Construct a directed graph of all function calls in the codebase.

This phase walks the AST of every function body and records call edges. Callee resolution uses the symbol table (Phase 3) and type inference (via `ty` crate).

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
