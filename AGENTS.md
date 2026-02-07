# STRATO — PROJECT KNOWLEDGE BASE

**Generated:** 2026-02-07  
**Commit:** 35b962c  
**Branch:** vibes

## OVERVIEW

Strato is a Rust-based static analysis tool that detects blocking function calls inside Python async contexts via full transitive call-graph analysis. **Currently in research/design phase — no implementation code exists.** The repo contains only an mdBook design specification deployed to Vercel.

## STRUCTURE

```
strato/
├── docs/book/           # mdBook design specification (THE core content)
│   ├── src/             # 20 markdown chapters — proposal, architecture, appendices
│   ├── theme/           # Custom mdBook theme (page-toc, highlight overrides)
│   ├── book/            # Generated HTML output (DO NOT edit)
│   └── book.toml        # mdBook config
├── README.md            # Project summary + motivating example
├── Book Critique.md     # External feedback on the design document
├── vercel.json          # Vercel deployment: mdBook build + serve
└── .ruff_cache/         # Artifact from ruff usage (ignorable)
```

## WHERE TO LOOK

| Task | Location | Notes |
|------|----------|-------|
| Understand the tool | `docs/book/src/intro.md` | Start here |
| Architecture & pipeline | `docs/book/src/architecture-overview.md` | 7-phase pipeline diagram |
| Design tradeoffs | `docs/book/src/design-decisions.md` | 16 decisions with alternatives |
| Planned repo layout | `docs/book/src/appendix-e-repository-structure-implementation-plan.md` | Rust workspace + Python pkg |
| Acceptance tests | `docs/book/src/appendix-b-acceptance-test-cases.md` | 19 fixture definitions |
| Config schema | `docs/book/src/appendix-d-configuration-schema.md` | `[tool.strato]` spec |
| Output formats | `docs/book/src/appendix-c-output-format-specifications.md` | Text/JSON/SARIF |
| Open questions | `docs/book/src/open-questions-reviewers.md` | Unresolved design issues |
| mdBook config | `docs/book/book.toml` | Title, theme, search settings |
| Deployment | `vercel.json` | Downloads mdBook binary, builds, serves `docs/book/book/` |

## PLANNED TECH STACK (not yet implemented)

| Component | Technology | Purpose |
|-----------|------------|---------|
| Core analysis | Rust (`strato_core` crate) | 7-phase pipeline: discovery → parse → resolve → graph → annotate → propagate → report |
| CLI | Rust (`strato_cli` crate) | `strato check <path>` with text/JSON/SARIF output |
| Caching | Rust (`strato_cache` crate) | Content-hash incremental cache for phases 1-3 |
| Python parser | `ruff_python_parser` (pinned rev) | AST parsing |
| Type inference | `ty_python_semantic` (pinned rev) | Alias tracking, return types, MRO |
| Call graph | `petgraph` | Directed graph + SCC decomposition |
| Python pkg | `strato` on PyPI | `@blocking`, `@non_blocking`, `@unblocker` decorators |

## CONVENTIONS

- **Documentation style**: Technical specification prose, not tutorial. Each design decision has: problem → decision → alternatives → risks.
- **mdBook chapter naming**: kebab-case filenames, no numeric prefixes in filenames (section numbers in headings only).
- **Book theme**: Custom `page-toc.js`/`page-toc.css` for floating right-side TOC; custom `highlight.css` for code blocks. Edits go in `docs/book/theme/`.
- **No code yet**: All architecture, error codes, data structures, and API contracts exist only as markdown specs in `docs/book/src/`.

## ANTI-PATTERNS (THIS PROJECT)

### Design constraints — violating these invalidates the specification:

1. **NEVER pattern-match only** — full transitive call graph is the core value prop. Pattern matching = existing tools (ruff, flake8-async).
2. **NEVER emit false positives** — Unknown calls are skipped, not flagged. Precision > recall. `BlockingStatus::Unknown` is terminal.
3. **NEVER use iterative fixpoint** — SCC-based propagation (Tarjan's) guarantees O(V+E). Iterative is O(V×E).
4. **NEVER use HashMap for output-affecting data** — BTreeMap everywhere. Diagnostics sorted: file → line → col → error code. Byte-for-byte determinism.
5. **NEVER support trio/anyio in v1** — asyncio only. Documented scope boundary.
6. **NEVER merge the two PyPI packages** — `strato` (annotations, pure Python, zero deps) and `strato-cli` (Rust binary via maturin) version independently.
7. **NEVER cache phases 4-7** — only phases 1-3 (parse + imports) are cacheable. `ty`'s Salsa is not serializable.
8. **NEVER abort on single-file errors** — collect warnings, continue. Only fail on: config error, I/O error, all files failed to parse.

### Error codes (do not conflate):

| Code | Meaning |
|------|---------|
| STRATO001 | Direct blocking call in async function |
| STRATO002 | Indirect blocking via sync intermediary (2+ links) |
| STRATO003 | Blocking `@property` accessed in async context |
| STRATO004 | Blocking dunder method invoked in async context |

## COMMANDS

```bash
# Build documentation locally (requires mdBook installed)
mdbook build docs/book
mdbook serve docs/book          # Local dev server

# Vercel does this automatically on push:
#   install: curl mdBook v0.5.2 binary
#   build:   ./mdbook build docs/book
#   output:  docs/book/book/
```

## NOTES

- `docs/book/book/` is the **generated** HTML output. It's tracked in git but should not be manually edited.
- The `.ruff_cache/` directory is an artifact — no Python source exists in the repo yet.
- `scripts/` directory referenced in early commits was removed (`df78036`).
- GitHub repo: `tasercake/strato` (see `book.toml` git-repository-url).
- Performance targets: <5s fresh analysis (500 files), <500ms cached.
- Implementation plan: 13 sequential milestones (M-1 through M12) defined in Appendix E.
