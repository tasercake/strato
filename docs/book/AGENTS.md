# docs/book/src — DESIGN SPECIFICATION

## OVERVIEW

20 markdown chapters comprising Strato's complete design specification. This is the authoritative source for all architecture, algorithms, constraints, and scope decisions.

## WHERE TO LOOK

| Task | Start here | Then read |
|------|-----------|-----------|
| Quick orientation | `intro.md` | `architecture-overview.md` |
| Understand a design choice | `design-overview.md` | Section number matches chapter number |
| Add/modify a chapter | `SUMMARY.md` | Update nav, then create/edit `.md` file |
| Check what's out of scope | `known-limitations-scope-boundaries.md` | `open-questions-reviewers.md` |
| Define test fixtures | `appendix-b-acceptance-test-cases.md` | Fixtures are named `a01_` through `a31_` |
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
