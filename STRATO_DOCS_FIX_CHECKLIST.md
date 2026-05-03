# Strato docs fix checklist

Step-by-step checklist for turning the current Strato design docs into a ty-accurate, internally consistent spec.

Source material:
- `/home/exedev/repos/strato/STRATO_DOCS_REVIEW_VS_TY.md`
- `docs/book/src/*`
- reference repo: `/home/exedev/repos/ty`

---

## Phase 0 — Freeze the intended architecture

### 0.1 Decide the `ty` integration model
- [x] Decide whether Strato will use `ty` as the **source of truth** for:
  - [x] project/file database
  - [x] parsing
  - [x] module resolution
  - [x] semantic/type lookup
- [x] If yes, update all docs to describe Strato as building its blocking analysis **on top of** `ty`.
- [x] If no, rewrite all `ty` references as **optional / best-effort / adapter-based**, and document the exact pinned APIs actually used. (Not applicable; architecture explicitly chose `ty` as the semantic substrate.)
- [x] Add one explicit sentence to the design overview stating which system owns:
  - [x] AST identity
  - [x] module resolution
  - [x] type inference
  - [x] call edge construction

### 0.2 Remove fake API assumptions
- [x] Audit every reference to a conceptual `TypeResolver`.
- [x] Remove or rewrite any claims that `ty` cleanly exposes:
  - [x] `resolve_type(&Expr)`
  - [x] `resolve_callee(&Expr)`
  - [x] `resolve_attribute(...)`
  - [x] public MRO traversal API
- [x] Replace with the real abstraction boundary you want to depend on.
- [x] Add a note that `ty` APIs are file/model-bound and not arbitrary-AST-oracle APIs.

---

## Phase 1 — Fix the biggest correctness errors first

### 1.1 Fix property/dunder error classification
Files:
- `docs/book/src/error-reporting-diagnostics.md`
- `docs/book/src/appendix-b-acceptance-test-cases.md`

- [x] Rewrite the STRATO003/004 classification algorithm.
- [x] Stop classifying by the **last edge kind** in the chain.
- [x] Classify using the **first special semantic edge** from the async context, or explicit path metadata.
- [x] Update examples so they reflect the corrected rule.
- [x] Add acceptance tests where:
  - [x] property access is intermediate, not leaf
  - [x] implicit dunder call is intermediate, not leaf

### 1.2 Fix `@non_blocking` SCC semantics
Files:
- `docs/book/src/blocking-propagation.md`
- `docs/book/src/blocking-function-database-annotations.md`
- `docs/book/src/appendix-b-acceptance-test-cases.md`

- [x] Remove the rule that one `KnownNonBlocking` function makes an entire SCC non-blocking.
- [x] Replace it with a narrower rule, e.g. reporting suppression at the annotated node/intervention point only.
- [x] Explicitly document whether `@non_blocking` is:
  - [x] a semantic truth claim
  - [x] a reporting suppression
  - [x] an override with accepted unsoundness
- [x] Add an SCC regression test:
  - [x] mutually recursive functions
  - [x] one annotated `@non_blocking`
  - [x] another reaches a real blocking root

### 1.3 Fix the false-positive claim
Files:
- `docs/book/src/intro.md`
- `docs/book/src/design-overview.md`
- `docs/book/src/escape-hatches-executor-wrappers.md`

- [x] Remove or soften “zero false positives”.
- [x] Replace with a precise claim like:
  - [x] proven-blocking oriented
  - [x] low false positive by design
  - [x] unresolved wrappers may require configuration
- [x] Ensure the intro and design overview say the same thing.

---

## Phase 2 — Eliminate architectural contradictions

### 2.1 Stop duplicating `ty`’s import resolver in the docs
Files:
- `docs/book/src/analysis-pipeline.md`
- `docs/book/src/design-overview.md`
- `docs/book/src/appendix-e-repository-structure-implementation-plan.md`
- `docs/book/src/known-limitations-scope-boundaries.md`

- [x] Remove or shrink the custom module-resolution design if Strato will rely on `ty`.
- [x] If keeping a Strato-side resolver, explain exactly why it exists despite `ty_module_resolver`.
- [x] Remove prose that sounds like Strato must fully recreate Python import semantics on its own.
- [x] Rewrite limitations accordingly.

### 2.2 Fix cache-boundary contradictions
Files:
- `docs/book/src/supporting-systems.md`
- `docs/book/src/design-overview.md`
- `docs/book/src/architecture-overview.md`

- [x] Decide whether cached `call_edges` are allowed.
- [x] If yes: (Not applicable; cache design explicitly chooses not to cache resolved call edges.)
  - [x] explicitly say resolved edge facts are cached (Not applicable; resolved edge facts are intentionally not cached.)
  - [x] define cache invalidation inputs (Not applicable; resolved edge facts are intentionally not cached.)
- [x] If no:
  - [x] remove `call_edges` from cached per-file results
  - [x] limit cached artifacts to parse/symbol/import summaries
- [x] Add an explicit cache key section covering:
  - [x] ty revision / pinned commit
  - [x] Python version
  - [x] config
  - [x] blocking DB contents/version
  - [x] stub paths
  - [x] source roots / search paths
- [x] State whether fresh and cached runs must be diagnostically identical.

### 2.3 Fix the “double parse” story
Files:
- `docs/book/src/design-overview.md`
- `docs/book/src/call-graph-type-resolution.md`

- [x] Remove overconfident timing claims unless benchmarked.
- [x] Reframe the real issue as **ownership of the source database / AST identity**, not just parse cost.
- [x] Clarify whether Strato:
  - [x] uses ty-owned parsed modules
  - [x] owns its own ASTs and bridges carefully
- [x] If separate ASTs are kept, document node mapping / identity risks.

---

## Phase 3 — Make the docs internally consistent

### 3.1 Normalize config keys
Files:
- `docs/book/src/analysis-pipeline.md`
- `docs/book/src/appendix-d-configuration-schema.md`
- any other config references

- [x] Pick one spelling for source roots:
  - [x] `src_roots`
  - [x] or `source_roots` (Rejected; standardized on `src_roots`.)
- [x] Use that spelling everywhere.
- [x] Decide whether `blocking_db_path` exists.
- [x] If it exists, add it to the schema. (Not applicable; `blocking_db_path` was removed as a schema concept.)
- [x] If it does not, remove stray references.

### 3.2 Normalize JSON schema terms
Files:
- `docs/book/src/appendix-c-output-format-specifications.md`
- `docs/book/src/appendix-b-acceptance-test-cases.md`
- `docs/book/src/error-reporting-diagnostics.md`

- [x] Pick one field name:
  - [x] `primary_location`
  - [x] or `location` (Rejected; standardized on `primary_location`.)
- [x] Use it consistently in schema, examples, and test fixtures.
- [x] Normalize all example payloads to the final schema.

### 3.3 Normalize column indexing
Files:
- `docs/book/src/error-reporting-diagnostics.md`
- `docs/book/src/appendix-c-output-format-specifications.md`

- [x] Pick one indexing convention for emitted JSON columns.
- [x] Prefer 1-indexed unless there is a strong protocol reason not to.
- [x] State the convention once and reference it consistently.

### 3.4 Normalize stub annotation format
Files:
- `docs/book/src/analysis-pipeline.md`
- `docs/book/src/blocking-function-database-annotations.md`
- `docs/book/src/appendix-d-configuration-schema.md`

- [x] Choose one stub annotation mechanism.
- [x] Prefer decorator-based annotation over a custom comment syntax unless there is a concrete need.
- [x] Delete references to the losing format.

### 3.5 Normalize version/scope language
Files:
- `docs/book/src/intro.md`
- `docs/book/src/escape-hatches-executor-wrappers.md`
- `docs/book/src/blocking-function-database-annotations.md`
- `docs/book/src/appendix-b-acceptance-test-cases.md`

- [x] Decide whether `@unblocker` / generalized wrappers are:
  - [x] in v1 (Rejected; generalized wrappers remain documented as v1.1+.)
  - [x] or in v1.1+
- [x] Make every file agree.
- [x] Make acceptance tests match the declared scope.

### 3.6 Normalize determinism claims
Files:
- `docs/book/src/design-overview.md`
- `docs/book/src/analysis-pipeline.md`
- `docs/book/src/error-reporting-diagnostics.md`

- [x] Audit every data structure mentioned in output-affecting paths.
- [x] If `HashMap` remains anywhere relevant, document the explicit sorting boundary.
- [x] Otherwise switch described structures to ordered equivalents.
- [x] Ensure the determinism contract is technically defensible.

### 3.7 Fix blocking DB count claims
Files:
- `docs/book/src/blocking-function-database-annotations.md`
- `docs/book/src/appendix-a-blocking-function-database.md`
- `docs/book/src/intro.md`

- [x] Recount entries by category.
- [x] Fix per-category counts.
- [x] Fix total count claims.
- [x] If the DB is still evolving, use approximate language only where justified.

### 3.8 Remove impossible TOML duplicate-key semantics
Files:
- `docs/book/src/escape-hatches-executor-wrappers.md`
- `docs/book/src/appendix-d-configuration-schema.md`

- [x] Remove “duplicate keys are rejected (last one wins)” wording.
- [x] Replace with behavior that matches the actual parser/tooling.

---

## Phase 4 — Clarify what Strato actually uses from `ty`

### 4.1 Tighten the capability story
Files:
- `docs/book/src/call-graph-type-resolution.md`
- `docs/book/src/known-limitations-scope-boundaries.md`
- `docs/book/src/design-overview.md`
- `docs/book/src/open-questions-reviewers.md`

- [x] Replace vague “ty gives X/Y/Z” tables with a precise statement of which facts Strato consumes.
- [x] Decide whether Strato relies on:
  - [x] expression type lookup
  - [x] static attribute/member resolution
  - [x] callable identity / alias tracking
  - [x] return-type inference
  - [x] generic instantiation (documented as available from ty, but not consumed by Strato for blocking analysis)
  - [x] union narrowing (documented as available from ty, but not consumed by Strato for blocking analysis)
- [x] Remove claims about capabilities Strato will not actually consume.
- [x] Ensure limitations match the chosen capability set.

### 4.2 Tighten panic-handling claims
Files:
- `docs/book/src/design-overview.md`
- `docs/book/src/open-questions-reviewers.md`
- `docs/book/src/supporting-systems.md`

- [x] Document the exact failure boundary for `ty` interaction.
- [x] Say whether Strato uses `ty_project` checked entry points.
- [x] Avoid hand-wavy “catch panics per-file” wording unless you describe the actual mechanism.

---

## Phase 5 — Improve the architecture sections without shrinking ambition

### 5.1 Rewrite the architecture around two layers
Files:
- `docs/book/src/architecture-overview.md`
- `docs/book/src/analysis-pipeline.md`
- `docs/book/src/call-graph-type-resolution.md`

- [x] Introduce an explicit **semantic layer** and **blocking layer**.
- [x] Move file/project/type/module concerns into the semantic layer.
- [x] Move blocking DB, escape hatches, propagation, and diagnostics into the blocking layer.
- [x] Update diagrams to match.

### 5.2 Rewrite special-edge modeling
Files:
- `docs/book/src/call-graph-type-resolution.md`
- `docs/book/src/error-reporting-diagnostics.md`

- [x] Describe property/dunder handling as explicit semantic edge/path metadata.
- [x] Stop relying on post-hoc leaf-edge inspection.
- [x] Show one worked example per special edge kind.

### 5.3 Reframe annotations
Files:
- `docs/book/src/blocking-function-database-annotations.md`
- `docs/book/src/blocking-propagation.md`

- [x] Describe `@blocking`, `@non_blocking`, and `@unblocker` in terms of graph semantics and reporting semantics.
- [x] Be explicit about which ones are trusted overrides vs evidence-producing annotations.
- [x] Add a “pitfalls” subsection for annotation misuse.

---

## Phase 6 — Expand test coverage in the docs

### 6.1 Add missing acceptance tests
File:
- `docs/book/src/appendix-b-acceptance-test-cases.md`

Add cases for:
- [x] property access as intermediate edge before blocking root
- [x] implicit dunder as intermediate edge before blocking root
- [x] mutually recursive SCC with one `@non_blocking` member and another blocker
- [x] alias-based executor wrapper path (`safe = sync_to_async(func); await safe()` style)
- [x] determinism regression expectation
- [x] fresh vs cached parity expectation

### 6.2 Make tests match schema and scope
- [x] Ensure all expected JSON payloads use the final schema names.
- [x] Ensure all tests refer only to features that are in the declared version scope.

---

## Phase 7 — Final consistency pass

### 7.1 Cross-file terminology sweep
- [x] Sweep for `TypeResolver`, `SemanticModel`, `ProjectDatabase`, `call edge`, `primary_location`, `src_roots`, `source_roots`, `@unblocker`, `v1.1`, `HashMap`, `BTreeMap`.
- [x] Make each term consistent across all chapters.

### 7.2 Cross-reference verification
- [x] Verify every internal link and section reference still points to the right chapter/anchor after edits.

### 7.3 Claims-vs-reality verification
- [x] Re-check every concrete claim about `ty` against `/home/exedev/repos/ty`.
- [x] Remove any prose that depends on non-public or speculative APIs unless clearly labeled as pinned-rev assumptions.

---

## Done criteria

Only mark this checklist done when:
- [x] The docs no longer describe a fake `ty` API.
- [x] The docs no longer duplicate `ty`’s resolver without justification.
- [x] Property/dunder classification is correct.
- [x] `@non_blocking` semantics are no longer unsound at SCC level.
- [x] Cache behavior is internally consistent.
- [x] Config/schema/output terms are consistent across all files.
- [x] Acceptance tests cover the previously missing failure modes.
- [x] Every claim about `ty` matches the pinned reality being referenced.
