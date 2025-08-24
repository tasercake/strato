# Building a Rust-based Python Asyncio Linter

Based on comprehensive analysis of ruff's architecture, asyncio linting patterns, and the Rust ecosystem, here's a detailed technical guide for building a production-quality Python asyncio linter in Rust. **The key insight is that ruff's architecture provides an excellent foundation, requiring specialized extension rather than complete reimplementation.**

## Ruff architecture analysis reveals critical building blocks

Ruff uses a **sophisticated multi-crate architecture** that can be directly leveraged for asyncio-specific analysis. The core components most relevant for asyncio linting include:

**Essential crates for asyncio linting**:
- `ruff_python_parser`: Hand-written recursive descent parser supporting Python 3.12+ syntax including all async/await constructs
- `ruff_python_semantic`: **Critical for asyncio analysis** - provides semantic model for symbol resolution, cross-file analysis, and type inference
- `ruff_linter`: Rule engine and AST visitor pattern implementation
- `ruff_python_ast`: Custom AST types with location tracking and semantic metadata

**Multi-file analysis capabilities**: Ruff's **Red Knot type checker integration** uses Salsa-based incremental computation for cross-file semantic analysis. This is essential for detecting asyncio patterns that span multiple modules, such as:
- Import analysis to identify blocking libraries vs async alternatives
- Cross-module async function call validation  
- Resource lifecycle tracking across files

**Performance optimization strategies**: Ruff achieves 10-100x performance improvements through parallel file processing with Rayon, efficient caching with modification-time checks, and hand-written parsers optimized for Python syntax.

## Critical asyncio anti-patterns requiring detection

Research identifies **five high-impact categories** of asyncio issues that static analysis can effectively catch:

### Blocking operations in async functions (highest severity)
The most critical anti-pattern involves synchronous blocking calls within async functions that freeze the entire event loop:

```python
async def bad_function():
    time.sleep(1)  # BLOCKS entire event loop
    with open('file.txt') as f:  # BLOCKS 
        data = f.read()
    response = requests.get('http://api.com')  # BLOCKS
```

**Detection strategy**: Maintain a comprehensive database of blocking function calls and their async alternatives. The linter should flag usage of `time.sleep`, `requests.*`, synchronous database drivers, and file I/O operations within async contexts.

### Resource management violations 
**Unclosed async resources** represent the second most severe category, causing memory leaks and connection exhaustion:

```python
async def resource_leak():
    session = aiohttp.ClientSession()  # Never closed
    response = await session.get('http://example.com')
    return await response.text()
```

**Detection approach**: Track resource creation patterns and verify proper cleanup through async context managers or explicit `.close()` calls.

### Performance anti-patterns in concurrent execution
Sequential processing where concurrent execution is possible represents a major performance issue:

```python
async def slow_sequential():
    results = []
    for url in urls:
        response = await fetch(url)  # Sequential - inefficient
        results.append(response)
```

**Detection methodology**: Identify `await` statements within loops that could be converted to `asyncio.gather()` or similar concurrent patterns.

## Rust ecosystem provides mature libraries for implementation

The Rust ecosystem offers **production-ready alternatives** that enable building high-performance Python analysis tools:

### Python parsing and AST analysis
**RustPython parser** (crate: `rustpython-parser`) provides the most mature Python parsing solution:
- Full Python 3.8+ compatibility including async/await syntax
- 2-3x performance improvement over Python-based parsers
- Clean API with complete AST generation and source location tracking
- Battle-tested through RustPython interpreter and ruff integration

### Code analysis framework
**Tree-sitter integration** offers exceptional performance for pattern matching:
- Incremental parsing with sub-millisecond updates after initial parse
- Built-in query system for complex pattern detection
- Python grammar available through `tree-sitter-python`

### Performance and concurrency libraries
**Rayon** provides essential parallel processing capabilities:
- Drop-in replacement for iterators with `par_iter()`
- Work-stealing scheduler with automatic load balancing  
- Data-race prevention through Rust's type system
- Typical 2x+ speedups on multi-core systems

**Memory-efficient collections** through `hashbrown` (now Rust's standard HashMap) deliver 2x performance improvements with 87% memory overhead reduction compared to previous implementations.

## Architecture recommendations based on successful patterns

Analysis of rust-analyzer and Clippy reveals **key architectural principles** for building scalable code analysis tools:

### Query-based incremental computation
Implement **Salsa-based architecture** for fine-grained dependency tracking and incremental recomputation. This pattern enables analyzing large codebases while only reprocessing what actually changed:

```rust
#[salsa::tracked]
fn analyze_async_function(db: &dyn AnalysisDatabase, func: &FunctionDef) -> Vec<Diagnostic> {
    // Analysis logic here - automatically cached and incrementally updated
}
```

### Layered crate organization  
Structure the project with clear separation of concerns:

```
asyncio-linter/
├── crates/
│   ├── asyncio-parser/     # Python parsing and AST generation
│   ├── asyncio-analysis/   # Core semantic analysis and rule engine  
│   ├── asyncio-rules/      # Asyncio-specific lint rule implementations
│   ├── asyncio-cli/        # Command-line interface
│   └── asyncio-server/     # Language Server Protocol implementation
```

### Performance optimization strategies
**Multi-threaded file processing** using Rayon's parallel iterators:
- Process multiple Python files concurrently
- Implement work-stealing for balanced load distribution
- Use Arc-based sharing for immutable semantic data

**Memory management** through arena allocation for AST processing:
- Reduces memory fragmentation for large codebases
- Enables efficient sharing of immutable AST nodes
- Supports incremental garbage collection

## Step-by-step development roadmap

### Phase 1: Foundation (8-12 weeks)
**Core parsing and basic rule engine**:

**Week 1-2**: Project setup and architecture
- Initialize multi-crate workspace structure
- Integrate RustPython parser for AST generation
- Implement basic CLI interface using Clap v4
- Set up testing framework with Criterion for benchmarking

**Week 3-6**: Semantic analysis foundation  
- Build symbol table construction during AST traversal
- Implement visitor pattern for rule dispatch
- Create configuration system using Serde + TOML
- Develop core diagnostic reporting infrastructure

**Week 7-10**: Essential asyncio rules
- **ASYNC-BLOCKING-CALL**: Detect blocking operations in async functions
- **ASYNC-MISSING-AWAIT**: Identify coroutine calls without await
- **ASYNC-RESOURCE-LEAK**: Flag unclosed async resources
- Implement fix suggestions for automatically correctable violations

**Week 11-12**: Performance optimization and testing
- Add parallel file processing with Rayon
- Create comprehensive test suite with real Python codebases  
- Implement performance benchmarks and regression testing
- Optimize memory usage for large project analysis

### Phase 2: Advanced analysis (6-10 weeks)
**Cross-file analysis and sophisticated patterns**:

**Week 1-4**: Multi-file semantic analysis
- Integrate Salsa for incremental computation
- Implement import resolution and cross-module analysis
- Add type-aware rule analysis for complex asyncio patterns
- Create project-wide resource lifecycle tracking

**Week 5-8**: Extended rule set
- **ASYNC-SEQUENTIAL-LOOP**: Detect sequential processing opportunities  
- **ASYNC-CPU-BOUND**: Flag CPU-intensive work in async functions
- **ASYNC-CONCURRENCY-LIMIT**: Identify unlimited concurrency patterns
- **ASYNC-EXCEPTION-HANDLING**: Verify proper async exception handling

**Week 9-10**: Integration and tooling
- Develop Language Server Protocol implementation
- Create editor plugins for VS Code and other editors
- Add CI/CD integration templates and examples
- Implement comprehensive documentation and usage guides

### Phase 3: Production hardening (4-8 weeks)
**Optimization, reliability, and ecosystem integration**:

**Week 1-3**: Performance and scalability
- Profile and optimize for codebases with 10,000+ files
- Implement smart caching strategies with disk persistence
- Add memory usage optimization for long-running processes
- Create benchmark suite comparing against existing Python linters

**Week 4-6**: Reliability and robustness  
- Implement comprehensive error recovery in parsing
- Add fuzzing with `cargo-fuzz` for edge case discovery
- Create extensive integration testing with popular Python projects
- Develop security audit procedures and vulnerability handling

**Week 7-8**: Ecosystem integration
- Build Python bindings using PyO3 for existing Python toolchains
- Create output format compatibility with existing linters
- Develop migration guides from flake8-async and pylint
- Launch community contribution guidelines and onboarding

### Critical implementation details

**Rule engine architecture**: Follow ruff's pattern with violation structs and analysis functions:

```rust
#[derive(ViolationMetadata)]
pub struct BlockingCallInAsync {
    pub function_name: String,
    pub async_alternative: Option<String>,
}

pub fn check_blocking_calls(func: &FunctionDef, semantic: &SemanticModel) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for call in extract_function_calls(func) {
        if is_blocking_call(&call.name, semantic) {
            diagnostics.push(Diagnostic::new(
                BlockingCallInAsync {
                    function_name: call.name.clone(),
                    async_alternative: suggest_async_alternative(&call.name),
                },
                call.range,
            ));
        }
    }
    diagnostics
}
```

**Semantic model integration**: Leverage ruff's semantic analysis for context-aware detection:

```rust
fn analyze_async_context(node: &AstNode, semantic: &SemanticModel) -> bool {
    semantic.in_async_function() && !semantic.in_executor_context()
}
```

**Performance measurement strategy**: Establish baseline comparisons against existing tools:
- Target 10x+ speed improvement over flake8-async
- Memory usage linear scaling with codebase size  
- Sub-second analysis for medium projects (1000-5000 files)

The combination of ruff's proven architecture, comprehensive asyncio pattern detection, and Rust's performance characteristics positions this approach to create a **production-quality tool that significantly advances the state of Python asyncio analysis**. The key differentiator lies in leveraging existing mature infrastructure while focusing specifically on the unique challenges of asyncio code patterns.
