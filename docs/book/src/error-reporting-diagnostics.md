# 9. Error Reporting & Diagnostics

> **Decision recap:** [Decision 2.7](./02-design-overview.md#27-intervention-strategy-for-error-reporting) established the intervention point strategy (first-party-deepest vs async-boundary) to guide users to the most actionable fix location. [Decision 2.14](./02-design-overview.md#214-determinism-contract) mandates deterministic output ordering for test stability and reproducible CI runs.

[async] [tooling]

### 9.1 Error Codes

Strato emits four error codes, each corresponding to a distinct pattern of blocking call reachability from async contexts:

| Code | Meaning | Severity | Trigger Condition |
|------|---------|----------|-------------------|
| `STRATO001` | Direct blocking call in async function | Error | Async function directly calls a blocking function with no intermediary sync functions |
| `STRATO002` | Indirect blocking call via sync intermediary | Error | Async function calls sync function(s) that transitively reach a blocking function |
| `STRATO003` | Blocking `@property` accessed in async context | Error | Property getter (decorated with `@property`) is accessed and transitively blocks |
| `STRATO004` | Blocking dunder method invoked in async context | Error | Implicit dunder method call (e.g., `str(obj)`, `x + y`) transitively blocks |

#### Message Templates

**STRATO001:**
```
STRATO001: Direct blocking call in async function

  --> {file}:{line}:{column}
   |
{line} | {source_line}
   | {underline} blocks the event loop
   |
   = call chain: {chain}
   = help: {help_text}
```

**STRATO002:**
```
STRATO002: Blocking call reachable from async context

  --> {file}:{line}:{column}
   |
{line} | {source_line}
   | {underline} calls sync chain that blocks the event loop
   |
   = call chain: {chain}
   = help: {help_text}
```

**STRATO003:**
```
STRATO003: Blocking property access in async context

  --> {file}:{line}:{column}
   |
{line} | {source_line}
   | {underline} property getter blocks the event loop
   |
   = call chain: {chain}
   = help: {help_text}
```

**STRATO004:**
```
STRATO004: Blocking dunder method in async context

  --> {file}:{line}:{column}
   |
{line} | {source_line}
   | {underline} implicit dunder call blocks the event loop
   |
   = call chain: {chain}
   = help: {help_text}
```

#### Wrapper Attribution

When a diagnostic fires because an `@unblocker` wrapper could not be resolved (type inference failed to track the alias), the diagnostic includes wrapper attribution:

```
   = note: This call may be wrapped by an @unblocker decorator, but type inference
           could not confirm the wrapper. If this is a false positive, ensure the
           wrapper alias is directly assigned (e.g., `safe = sync_to_async(func)`)
           without intermediate reassignments.
```

This note is appended to the diagnostic message when:
1. The call site is to a name that was assigned from an `@unblocker`-decorated function
2. Type inference (`ty`) could not resolve the alias chain
3. The call was not marked `in_executor` due to resolution failure

### 9.2 Error Code Classification Algorithm

The error code is determined by inspecting the `BlockingReason.chain_links` and the edge kind of the last link in the chain:

```rust
fn classify_error_code(chain: &BlockingReason, graph: &CallGraph) -> ErrorCode {
    // The first link is always from the async function.
    // The last link's callee is the blocking root cause.
    let first_link = &chain.chain_links[0];
    let last_link = chain.chain_links.last().unwrap();

    // Check the edge kind of the last link to the blocking root
    let last_edge_kind = graph.edge_kind(
        &last_link.function_name,
        &last_link.callee_name
    );

    // STRATO003: Property access to a blocking getter
    if last_edge_kind == EdgeKind::PropertyAccess {
        return ErrorCode::STRATO003;
    }

    // STRATO004: Implicit dunder call that blocks
    if last_edge_kind == EdgeKind::ImplicitDunder {
        return ErrorCode::STRATO004;
    }

    // STRATO001 vs STRATO002: Is the blocking call directly in an async function?
    // "Direct" means: chain has exactly 1 link AND the caller is async.
    // That means: async_func directly calls blocking_func with no intermediaries.
    if chain.chain_links.len() == 1 && first_link.is_async {
        return ErrorCode::STRATO001;  // Direct blocking call in async function
    }

    // Otherwise: there are intermediary sync functions between async and blocker
    ErrorCode::STRATO002
}
```

**Classification Examples:**

| Scenario | Chain | Edge Kind | Result |
|----------|-------|-----------|--------|
| `async handler() -> time.sleep()` | 1 link, caller is async | `DirectCall` | **STRATO001** |
| `async handler() -> helper() -> time.sleep()` | 2 links | `DirectCall` | **STRATO002** |
| `async handler() -> loader.data [PropertyAccess] -> requests.get()` | 2+ links | `PropertyAccess` (last edge) | **STRATO003** |
| `async handler() -> str(obj) [ImplicitDunder] -> __str__() -> requests.get()` | 2+ links | `ImplicitDunder` (last edge) | **STRATO004** |

**Key invariants:**
- The first link's `is_async` field is always `true` (the chain starts from an async function)
- The last link's callee is always a `KnownBlocking` node (the blocking root cause)
- Edge kind is checked only for the **last link** (the edge leading to the blocking root)

### 9.3 Intervention Point Strategy

The "intervention point" is the primary location shown in the diagnostic – the place in the user's code where they should make a change. Strato supports two strategies for selecting this location:

#### Strategy: `first-party-deepest` (Default)

Select the **deepest function in first-party code** on the call chain between the async context and the blocking call. This points users to the lowest-level first-party function that could be refactored to be async.

```rust
fn select_intervention_point(
    chain: &[ChainLink],
    strategy: InterventionStrategy
) -> &ChainLink {
    match strategy {
        InterventionStrategy::FirstPartyDeepest => {
            // Walk the chain from the blocking end toward the async end
            // Find the deepest first-party function
            for link in chain.iter().rev() {
                if link.is_first_party {
                    return link;
                }
            }
            // Fallback: if no first-party code on path, use async boundary
            select_async_boundary(chain)
        }
        InterventionStrategy::AsyncBoundary => {
            select_async_boundary(chain)
        }
    }
}

fn select_async_boundary(chain: &[ChainLink]) -> &ChainLink {
    // Find the transition: last async function before sync code that leads to blocking
    for i in 0..chain.len() - 1 {
        if chain[i].is_async && !chain[i + 1].is_async {
            return &chain[i];
        }
    }
    // Fallback: first element
    &chain[0]
}
```

#### Strategy: `async-boundary`

Select the **async-to-sync transition point** – the last async function before the sync call chain that leads to blocking. This points users to the boundary where they should consider offloading the sync work.

#### Example Comparison

```python
# src/myapp/handler.py
async def handle_request():          # [0] async, first-party
    await process()                   # [1] async, first-party

# src/myapp/processor.py
async def process():                  # [1] async, first-party
    validate(data)                    # [2] sync, first-party   <-- async-boundary

# src/myapp/validator.py
def validate(data):                   # [2] sync, first-party
    check_db(data)                    # [3] sync, first-party   <-- first-party-deepest

# src/myapp/db.py
def check_db(data):                   # [3] sync, first-party
    psycopg2.connect(...)             # [4] sync, third-party, BLOCKING
```

**`first-party-deepest`** reports at `check_db()` in `db.py`:
```
STRATO002: Blocking call reachable from async context

  --> src/myapp/db.py:15:5
   |
15 |     psycopg2.connect(dsn)
   |     ^^^^^^^^^^^^^^^^^^^^ calls sync chain that blocks the event loop
   |
   = call chain: process() -> validate() -> check_db() -> psycopg2.connect()
   = help: Use `asyncpg` or wrap in `await loop.run_in_executor(None, psycopg2.connect, dsn)`
```

**`async-boundary`** reports at `process()` calling `validate()`:
```
STRATO002: Blocking call reachable from async context

  --> src/myapp/processor.py:8:5
   |
 8 |     validate(data)
   |     ^^^^^^^^^^^^^^ calls sync chain that blocks the event loop
   |
   = call chain: process() -> validate() -> check_db() -> psycopg2.connect()
   = help: Use `asyncpg` or wrap in `await loop.run_in_executor(None, psycopg2.connect, dsn)`
```

#### Tie-Breaking Rules

When the `first-party-deepest` strategy finds **multiple first-party functions at the same depth**, select the one with the lexicographically smallest `qualified_name`. If still tied (same function called from multiple sites), select the call site with the smallest `(line, column)` pair.

### 9.4 Diagnostic Structure

The `Diagnostic` struct is the core data structure for error reporting. It contains all information needed to render a diagnostic in any output format (text, JSON, SARIF).

```rust
/// A single diagnostic emitted by Strato.
struct Diagnostic {
    /// Unique error code (e.g., "STRATO001")
    code: ErrorCode,

    /// Severity level
    severity: Severity,  // Error, Warning

    /// The "intervention point" – where the user should look
    primary_location: Location,

    /// Human-readable message
    message: String,

    /// The call chain from async context to blocking call
    blocking_chain: Vec<ChainLink>,

    /// Which intervention strategy was used
    strategy: InterventionStrategy,

    /// Static suggestion for fixing the issue (from BlockingDatabase).
    help: Option<String>,

    /// Related locations (additional context for the diagnostic)
    related_locations: Vec<RelatedLocation>,

    /// Wrapper attribution note (if applicable)
    wrapper_attribution: Option<String>,
}

/// Source location with range information.
struct Location {
    /// File path (relative to project root, `/`-normalized)
    file: String,
    /// Start line (1-based)
    line: usize,
    /// Start column (0-based, UTF-8 byte offset within line)
    column: usize,
    /// End line (1-based)
    end_line: usize,
    /// End column (0-based, UTF-8 byte offset)
    end_column: usize,
}

/// A related location providing additional context.
struct RelatedLocation {
    location: Location,
    message: String,
}

/// Error code enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ErrorCode {
    STRATO001,
    STRATO002,
    STRATO003,
    STRATO004,
}
```

#### Location Derivation from Ruff AST

Ruff AST nodes provide `TextRange` (byte-offset range from the start of the source file). Conversion to `(line, column)` uses `ruff_source_file::SourceCode` and `ruff_source_file::LineIndex` for O(log n) lookup.

**Which AST span to use:**
- **Function definitions:** Use the `name` identifier range (not the entire `def`)
- **Call sites:** Use the full `ExprCall` range (includes parentheses)
- **Property access:** Use the `Attribute.attr` identifier range
- **Dunder operations:** Use the operator/builtin call range

#### Column Convention (End-to-End)

| Context | Convention |
|---------|-----------|
| Internal (`Location` struct) | 0-based byte offset (matches ruff) |
| Text output display | 1-based column (add 1 when formatting) |
| JSON output | 0-based (matches internal, LSP convention) |
| SARIF output | 1-based column (SARIF spec requires 1-based) |

### 9.5 Related Locations

Related locations provide additional context for diagnostics. They are attached based on the error code and call chain structure.

#### Related Location Rules by Error Code

| Error Code | Related Locations Attached | Purpose |
|------------|---------------------------|---------|
| `STRATO001` | 1. Async function definition<br>2. Blocking root definition (if available) | Show where the async context starts and the blocking root |
| `STRATO002` | 1. Async function definition<br>2. All intermediary sync function definitions<br>3. Blocking root definition (if available) | Show full call chain |
| `STRATO003` | 1. Async function definition<br>2. Property definition<br>3. Blocking root definition (if available) | Show property and blocking root |
| `STRATO004` | 1. Async function definition<br>2. Dunder method definition<br>3. Blocking root definition (if available) | Show dunder method and blocking root |

#### Example: STRATO002 with Related Locations

```python
# src/myapp/handler.py
async def handle_request():          # Related location 1
    process()

# src/myapp/processor.py
def process():                        # Related location 2
    validate()

# src/myapp/validator.py
def validate():                       # Related location 3 (intervention point)
    time.sleep(1)                     # Primary location
```

**Text output:**
```
STRATO002: Blocking call reachable from async context

  --> src/myapp/validator.py:8:5
   |
 8 |     time.sleep(1)
   |     ^^^^^^^^^^^^^ calls sync chain that blocks the event loop
   |
   = call chain: handle_request() -> process() -> validate() -> time.sleep()
   = help: Use `asyncio.sleep()` instead
   |
note: async function `handle_request` defined here
  --> src/myapp/handler.py:3:1
   |
 3 | async def handle_request():
   | ^^^^^^^^^^^^^^^^^^^^^^^^^^^

note: sync function `process` defined here
  --> src/myapp/processor.py:5:1
   |
 5 | def process():
   | ^^^^^^^^^^^^^^

note: blocking function `time.sleep` is a known blocking stdlib function
```

### 9.6 Deterministic Output Rules

For test stability and reproducible CI runs, all outputs must be deterministic.

#### Diagnostic Ordering

When multiple diagnostics are emitted, they are sorted by this key (lexicographic, ascending):

1. **File path** (string comparison, using `/`-normalized relative paths)
2. **Line number** (numeric, ascending)
3. **Column number** (numeric, ascending)
4. **Error code** (string comparison: STRATO001 < STRATO002 < STRATO003 < STRATO004)

```rust
impl Ord for Diagnostic {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.primary_location.file.cmp(&other.primary_location.file)
            .then(self.primary_location.line.cmp(&other.primary_location.line))
            .then(self.primary_location.column.cmp(&other.primary_location.column))
            .then(self.code.cmp(&other.code))
    }
}

// Sort diagnostics before output
diagnostics.sort();
```

#### Blocking Reason Path Selection

When a function has **multiple paths** to different blocking roots, store the **shortest path**. If multiple paths have the same length, select the path whose root cause has the lexicographically smallest `qualified_name`.

#### BTreeMap Usage

All internal maps that affect output order use `BTreeMap` instead of `HashMap`:

```rust
use std::collections::BTreeMap;

type SymbolTable = BTreeMap<String, SymbolDef>;
type ModuleMap = BTreeMap<String, PathBuf>;
type BlockingDatabase = BTreeMap<String, BlockingEntry>;
```

#### Determinism Contract

**Guarantee:** Given the same input files, configuration, and Strato version, the tool produces **byte-for-byte identical output** across runs, regardless of parallel processing order, hash map iteration order, file system traversal order, or operating system.

**Enforcement:** All diagnostic lists sorted before output; all maps use `BTreeMap`; all tie-breaking rules explicitly specified; integration tests include golden output comparison.

### 9.7 Output Formats

Strato supports three output formats:

| Format | Use Case | Audience |
|--------|----------|----------|
| **Text** | Terminal output, CI logs | Developers reading diagnostics directly |
| **JSON** | Programmatic consumption, IDE integration | Tools parsing Strato output |
| **SARIF** | GitHub Code Scanning, IDE integration | Security/quality platforms |

Format is controlled by the `--format` CLI flag:

```bash
strato check --format=text    # Default
strato check --format=json
strato check --format=sarif
```

Full specifications for each output format are provided in [Appendix C: Output Format Specifications](./appendix-c-output-format-specifications.md#appendix-c-output-format-specifications).
