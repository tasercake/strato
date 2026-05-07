# Strato Agent Notes

## Acceptance Fixtures
- Production-style fixture cases live in `tests/fixtures/a01_*` through `a39_*`; each fixture has one `.py` file plus `expected.json`.
- Some fixtures include local `pyproject.toml` with `[tool.strato.executor-wrappers]`; keep fixture config paths relative to the fixture root.

## Design Constraints
- Core value is transitive call-graph analysis, not direct pattern matching alone.
- Precision policy: unresolved/unknown calls should be skipped rather than flagged; avoid false positives.
- Blocking propagation is specified as SCC/Tarjan-based, not iterative fixpoint.
- Output-affecting data and diagnostics must be deterministic; prefer sorted collections/order (`BTreeMap`-style behavior) where output order can change.
- Strato's cross-run cache boundary is Strato-owned discovery/syntax artifacts only; Ruff parsed modules, ty's Salsa database, semantic facts, call graph, propagation results, and diagnostics are not serialized.
- Individual syntax errors, unresolvable imports, facade failures, and recoverable facade-boundary panics are warnings; config errors, I/O errors, and no analyzable source files are fatal.
- Performance targets in the docs are <5s fresh analysis and <500ms cached on 500 files; treat them as validation targets until implementation exists.
- v1 scope is asyncio-first; trio/anyio support is explicitly out of scope in the design docs.

## Docs
- Design docs live under `docs/book/src`; that subtree has its own `docs/book/AGENTS.md` with chapter conventions.
- For design context, start with `docs/book/src/intro.md`, `architecture-overview.md`, `analysis-pipeline.md`, and `design-overview.md`; acceptance-case details live in `appendix-b-acceptance-test-cases.md`.
- Never edit generated `docs/book/book/` by hand.
- Custom mdBook theme assets live in `docs/book/theme/` (`page-toc`, syntax highlighting, Mermaid assets); edit those rather than generated HTML.
- `docs/book/book.toml` has `create-missing = false`; adding a chapter requires both the `.md` file and a `SUMMARY.md` entry.
- Local `mdbook build docs/book` requires both `mdbook` and `mdbook-mermaid`. Verified failure mode when missing: `mdbook-mermaid` preprocessor not found.
- Serve docs locally with `mdbook serve docs/book` with the same `mdbook-mermaid` requirement.
- Vercel config downloads only `mdbook v0.5.2` and runs `./mdbook build docs/book`, outputting `docs/book/book/`.
