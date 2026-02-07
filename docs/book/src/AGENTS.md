# docs/book/src — DESIGN SPECIFICATION

## OVERVIEW

20 markdown chapters comprising Strato's complete design specification. This is the authoritative source for all architecture, algorithms, constraints, and scope decisions.

## STRUCTURE

```
src/
├── SUMMARY.md                                    # mdBook navigation (edit this to reorder/add chapters)
├── intro.md                                      # Tool overview, error codes, performance targets
├── problem-statement-motivation.md               # Why Strato exists
├── design-overview.md                           # 16 tradeoff analyses (LARGEST file, ~27KB)
├── architecture-overview.md                      # 7-phase pipeline, component map, public API
├── analysis-pipeline.md                          # Phase-by-phase detail
├── call-graph-type-resolution.md                 # Graph data model, edge extraction, ty integration
├── blocking-propagation.md                       # SCC decomposition, propagation algorithm
├── blocking-function-database-annotations.md     # ~80 built-in entries, @blocking/@non_blocking
├── escape-hatches-executor-wrappers.md           # run_in_executor, to_thread, @unblocker, custom wrappers
├── error-reporting-diagnostics.md                # Error codes, intervention strategies, output format
├── supporting-systems.md                         # Caching, config, testing infrastructure
├── known-limitations-scope-boundaries.md         # What Strato explicitly does NOT handle
├── open-questions-reviewers.md                   # Unresolved design questions for expert review
├── appendix-a-blocking-function-database.md      # Complete ~80-entry blocking function list
├── appendix-b-acceptance-test-cases.md           # 19 test fixture definitions (A1–A19)
├── appendix-c-output-format-specifications.md    # Text, JSON, SARIF format specs
├── appendix-d-configuration-schema.md            # [tool.strato] pyproject.toml schema
├── appendix-e-repository-structure-implementation-plan.md  # Planned Rust workspace layout + milestones
└── glossary.md                                   # Term definitions
```

## WHERE TO LOOK

| Task | Start here | Then read |
|------|-----------|-----------|
| Quick orientation | `intro.md` | `architecture-overview.md` |
| Understand a design choice | `design-overview.md` | Section number matches chapter number |
| Add/modify a chapter | `SUMMARY.md` | Update nav, then create/edit `.md` file |
| Check what's out of scope | `known-limitations-scope-boundaries.md` | `open-questions-reviewers.md` |
| Define test fixtures | `appendix-b-acceptance-test-cases.md` | Fixtures are named `a01_` through `a19_` |
| Check blocking DB entries | `appendix-a-blocking-function-database.md` | `blocking-function-database-annotations.md` for design |

## CONVENTIONS

- **Chapter headings**: `# N. Title` format (e.g., `# 3. Architecture Overview`). Section numbers in prose, NOT in filenames.
- **Cross-references**: Use relative links with `./filename.md#anchor` format. Existing refs use `[Section N.M](./filename.md#nm-section-title)`.
- **Code blocks**: Python for examples, Rust for planned implementation, TOML for config. Always annotate language.
- **Tables**: Used extensively for tradeoff matrices, limitation lists, config schemas. Keep column alignment.
- **Design decision structure**: Problem → Decision (with rationale) → Alternatives considered (with pros/cons) → Risk paragraph.
- **`SUMMARY.md`**: Three sections separated by `---`: Introduction, Proposal (11 chapters), Appendices (6 items). Add new chapters in the appropriate section.
- **`book.toml` setting**: `create-missing = false` — mdBook will NOT auto-create files listed in SUMMARY.md. You must create the `.md` file yourself.

## ANTI-PATTERNS

- NEVER edit files in `../book/` (the build output directory). Only edit files in this `src/` directory.
- NEVER add numeric prefixes to filenames (e.g., `01-intro.md`). The `no-section-label = true` setting in `book.toml` already suppresses auto-numbering.
- NEVER break cross-reference anchors without updating all linking chapters.
- NEVER add chapters to `src/` without adding them to `SUMMARY.md` — they won't appear in navigation.
