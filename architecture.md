# Architecture Overview

This document serves as a living guide to Strato's architecture so humans and agents can quickly understand how the repository is organized, what the major components are, and where the important design decisions live. Strato is currently in the research and design phase: the repository does **not** yet contain the Rust implementation described in the spec. Today, the repo is primarily an mdBook-backed design document for a planned static-analysis tool.

## 1. Project Structure

This section describes the repository as it exists today, while also calling out the planned implementation structure captured in the design docs.

```text
strato/
├── README.md                                  # Project overview and motivating example
├── architecture.md                            # This document
├── vercel.json                                # Vercel build config for publishing the mdBook
├── .gitignore                                 # Ignore rules
├── docs/
│   └── book/
│       ├── book.toml                          # mdBook configuration
│       ├── .gitignore                         # mdBook-specific ignore rules
│       ├── src/                               # Authoritative design specification
│       │   ├── SUMMARY.md                     # Navigation for the book
│       │   ├── intro.md                       # High-level intro, goals, constraints
│       │   ├── problem-statement-motivation.md
│       │   ├── design-overview.md             # Core design decisions and tradeoffs
│       │   ├── architecture-overview.md       # 7-phase pipeline + component map
│       │   ├── analysis-pipeline.md           # Detailed per-phase analysis behavior
│       │   ├── call-graph-type-resolution.md  # Graph model + ty-based resolution
│       │   ├── blocking-propagation.md        # SCC-based propagation algorithm
│       │   ├── blocking-function-database-annotations.md
│       │   ├── escape-hatches-executor-wrappers.md
│       │   ├── error-reporting-diagnostics.md
│       │   ├── supporting-systems.md          # CLI, config, caching, packaging
│       │   ├── known-limitations-scope-boundaries.md
│       │   ├── open-questions-reviewers.md
│       │   ├── appendix-a-blocking-function-database.md
│       │   ├── appendix-b-acceptance-test-cases.md
│       │   ├── appendix-c-output-format-specifications.md
│       │   ├── appendix-d-configuration-schema.md
│       │   ├── appendix-e-repository-structure-implementation-plan.md
│       │   └── glossary.md
│       ├── theme/                             # Custom mdBook theme assets
│       │   ├── index.hbs
│       │   ├── page-toc.js
│       │   ├── page-toc.css
│       │   ├── highlight.css
│       │   ├── css/
│       │   └── fonts/
│       └── book/                              # Generated mdBook output (build artifact)
└── AGENTS.md                                  # Repo-specific guidance for agents working in this tree
```

### Planned future implementation structure

The intended implementation architecture is documented in `docs/book/src/appendix-e-repository-structure-implementation-plan.md`. That planned layout introduces:

- `crates/strato_core` — core Rust analysis engine
- `crates/strato_cache` — incremental cache subsystem
- `crates/strato_cli` — CLI binary and output formatters
- `python/strato` — pure-Python annotations package
- `tests/fixtures` + `tests/integration` — acceptance and integration test suites
- `docs/rules` — rule-specific diagnostic documentation

## 2. High-Level System Diagram

### Current repository architecture

```text
[Author / Reviewer]
        |
        v
[Markdown design chapters in docs/book/src]
        |
        v
[mdBook build]
        |
        v
[Static HTML book in docs/book/book]
        |
        v
[Vercel deployment]
```

### Planned product architecture

```text
[Python project source + pyproject.toml]
                |
                v
        [Strato CLI: `strato check`]
                |
                v
  Discovery -> Parse -> Semantics -> Build -> Annotate -> Propagate -> Report
                |         |           |         |           |            |
                |         |           |         |           |            +--> Text / JSON / SARIF diagnostics
                |         |           |         |           +--> SCC-based blocking inference
                |         |           |         +--> Blocking DB + decorators + stubs
                |         |           +--> Project-wide call graph
                |         +--> ty-backed semantic facts
                +--> Config loading + file discovery + cache keys
```

The key architectural boundary is between the **current repo-as-specification** and the **planned Strato analyzer implementation**. The current repository documents the latter in detail but does not yet contain executable analyzer code.

## 3. Core Components

### 3.1. Documentation / Specification System

**Name:** mdBook design specification

**Description:** This is the primary artifact in the repository today. It captures Strato's problem statement, architecture, tradeoffs, algorithms, output formats, configuration schema, test plan, and implementation roadmap. If you want to understand Strato, start here.

**Technologies:** Markdown, mdBook, custom Handlebars/CSS/JS theme assets

**Deployment:** Built locally with `mdbook build docs/book` and published via Vercel

### 3.2. Planned CLI Surface

**Name:** `strato` CLI

**Description:** Planned user-facing binary for analyzing Python projects. The main interface is `strato check [PATHS...] [OPTIONS]`, with support for text, JSON, and SARIF output plus cache/config controls.

**Technologies:** Rust, `clap`, `miette`, `serde_json`, TOML parsing

**Deployment:** Planned as a compiled Rust binary distributed via the `strato-cli` PyPI package

### 3.3. Planned Core Analysis Engine

**Name:** `strato_core`

**Description:** Planned Rust library that performs the seven-phase analysis pipeline: discovery, parsing, semantic resolution, call-graph construction, blocking annotation, blocking propagation, and diagnostics generation. This is the heart of the actual analyzer.

**Technologies:** Rust, `ruff_python_parser`, `ruff_python_ast`, `ty_python_semantic`, `petgraph`, `rayon`, `serde`

**Deployment:** Library crate inside the planned Rust workspace

### 3.4. Planned Cache Subsystem

**Name:** `strato_cache`

**Description:** Planned incremental analysis cache storing Strato-owned per-file parse/extraction artifacts keyed by SHA-256 content hash. By design, it does **not** persist ty semantic state or derived call-graph / propagation outputs.

**Technologies:** Rust, `bincode`, `sha2`, filesystem-backed cache directory

**Deployment:** Library crate used by the CLI during local analysis runs

### 3.5. Planned Python Annotations Package

**Name:** `strato` (Python package)

**Description:** Planned tiny runtime package exporting decorators like `@blocking`, `@non_blocking`, and `@unblocker`. It exists to let users annotate code without adding meaningful runtime overhead.

**Technologies:** Pure Python, PEP 561 typing marker

**Deployment:** Separate PyPI package from the Rust CLI

## 4. Data Stores

### 4.1. Design-spec content store

**Name:** mdBook source tree

**Type:** Markdown files in git

**Purpose:** Stores the authoritative architecture and product design for Strato.

**Key files / collections:** `docs/book/src/*.md`, `docs/book/src/SUMMARY.md`, `docs/book/book.toml`

### 4.2. Planned blocking knowledge base

**Name:** Blocking function database

**Type:** Structured registry / JSON-like logical data source

**Purpose:** Maps qualified Python call targets to blocking or non-blocking behavior so Strato can seed analysis before propagation.

**Key schemas / collections:** stdlib entries, network library entries, database client entries, subprocess entries, user overrides

### 4.3. Planned local analysis cache

**Name:** `.strato_cache`

**Type:** Local binary on-disk cache

**Purpose:** Stores file content hashes and cached parse/extraction artifacts to speed up repeated runs.

**Key schemas / collections:** `manifest.bin`, `files/{hash}.bin`, cache version marker

## 5. External Integrations / APIs

**mdBook:** Builds the static documentation site from the markdown spec. Integration method: CLI build tool.

**Vercel:** Hosts the generated mdBook site. Integration method: `vercel.json` build configuration.

**ruff parser crates:** Planned dependency for Python parsing (`ruff_python_parser`, `ruff_python_ast`). Integration method: Rust crates.

**ty semantic engine:** Planned dependency for module/name/type resolution. Integration method: Rust crate API.

**PyPI / maturin:** Planned packaging path for distributing the CLI and annotations packages. Integration method: Python packaging + Rust build tooling.

## 6. Deployment & Infrastructure

**Cloud Provider:** Vercel (for the docs site)

**Key Services Used:** Vercel static deployment; git-backed repository content; mdBook build downloaded during deploy

**CI/CD Pipeline:** Lightweight Vercel-driven build pipeline defined in `vercel.json`

- Install command downloads mdBook v0.5.2
- Build command runs `./mdbook build docs/book`
- Output directory is `docs/book/book`

**Monitoring & Logging:** No explicit runtime monitoring stack is defined in the repository today because the current deliverable is a static documentation site, not a live service.

## 7. Security Considerations

**Authentication:** None in the current repo/site. The published artifact is static documentation.

**Authorization:** None at runtime for the docs site.

**Data Encryption:** Expected HTTPS/TLS in transit via Vercel-hosted delivery.

**Key Security Tools / Practices:**

- Planned analyzer design strongly prioritizes precision to avoid false positives
- Deterministic output is a deliberate design constraint (`BTreeMap`-style ordering in the planned implementation)
- Unknown semantic cases are intentionally treated conservatively by skipping unsupported inferences rather than fabricating certainty
- The Python annotations package is planned as zero-dependency / low-runtime-footprint to reduce operational risk

## 8. Development & Testing Environment

**Local Setup Instructions:**

- Read `README.md` for project framing
- Install `mdbook` if building locally
- Run `mdbook build docs/book` to build the docs
- Run `mdbook serve docs/book` for local preview

**Testing Frameworks:**

- Current repo: documentation review rather than a formal test harness
- Planned implementation: Rust unit/integration tests plus acceptance fixtures described in Appendix B

**Code Quality Tools:**

- Git for version control
- mdBook validation through successful build
- Planned Rust/Python toolchain validation once implementation begins

## 9. Future Considerations / Roadmap

- Move from a pure design repository to an implemented Rust workspace
- Validate the high-risk `ty` integration spike before committing to the full implementation
- Implement the sequential milestone plan from M-1 through M12
- Add the planned fixture corpus and integration tests for direct, indirect, cross-file, property, dunder, and executor-wrapped blocking behavior
- Ship dual-package distribution: `strato-cli` for the analyzer and `strato` for annotations
- Preserve the core product promises: transitive call-graph analysis, deterministic output, and no false positives by design

## 10. Project Identification

**Project Name:** Strato

**Repository URL:** https://github.com/tasercake/strato

**Primary Contact / Team:** Krishna Penukonda (author listed in `docs/book/book.toml`)

**Date of Last Update:** 2026-05-03 (creation/update date for this document)

## 11. Glossary / Acronyms

**AST:** Abstract Syntax Tree; parsed structural representation of Python source.

**SARIF:** Static Analysis Results Interchange Format; a standard machine-readable output format for code scanning tools.

**SCC:** Strongly Connected Component; used in the planned propagation phase to handle cycles in the call graph efficiently.

**ty:** The semantic analysis engine Strato plans to use for Python module/name/type facts.

**Blocking function database:** Registry of known blocking APIs used to seed the analysis before transitive propagation.

**First-party code:** Project-owned source code that Strato should analyze deeply and use for intervention-point suggestions.
