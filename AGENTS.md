# Strato Agent Notes

## Current State
- This repository is a Rust workspace. `README.md` says nothing is implemented; trust `Cargo.toml`, code, and tests over that line.
- Workspace crates: `crates/strato_cli` exposes the `strato` binary, `crates/strato_core` holds analyzer scaffolding and fixture loading, and `crates/strato_cache` provides deterministic SHA-256 helpers.
- The CLI is scaffolded only: `cargo run -p strato_cli -- check <path> --output json` prints a not-implemented message. Use `--output`; some docs show non-working `--format` examples.
- `python/strato` is a tiny annotation package stub (`blocking`, `non_blocking`, `unblocker`) with no packaging manifest.
- The design keeps annotation decorators and the analyzer binary separate: `strato` is the pure Python annotation package, and `strato-cli` is the Rust binary package.
- `.analysis/` contains local Ruff/ty source snapshots for research; it is not part of the root Cargo workspace. Avoid broad searches or edits there unless the task is specifically about that vendored/reference code.
- Ignore local artifacts such as `target/`, `.vercel/`, and `.ruff_cache/` unless the task is specifically about build output or tool caches.

## Commands
- Rust toolchain is pinned in `rust-toolchain.toml` to `1.92.0` with `clippy` and `rustfmt`.
- Format: `cargo fmt --check`.
- Lint: `cargo clippy --workspace --all-targets`.
- Test all active Rust tests: `cargo test`.
- Focused fixture-loader test: `cargo test -p strato_core acceptance_fixtures_are_well_formed`.

## Acceptance Fixtures
- Production-style fixture cases live in `tests/fixtures/a01_*` through `a39_*`; each fixture has one or more `.py` files plus `expected.json`.
- The fixture loader sorts fixture directories and source paths for deterministic tests; preserve this when changing loader behavior.
- Some fixtures include local `pyproject.toml` with `[tool.strato.executor-wrappers]`; keep fixture config paths relative to the fixture root.
- Expected diagnostics use stable codes: `STRATO001` direct blocking, `STRATO002` transitive blocking, `STRATO003` blocking property access, `STRATO004` blocking dunder invocation.

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
