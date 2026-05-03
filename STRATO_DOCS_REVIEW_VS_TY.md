# Strato docs review vs `ty`

This file captures the feedback from the OpenCode `codereview-roasted` review of `docs/book/src`, plus the synthesized follow-up notes.

Context:
- Strato repo: `/home/exedev/repos/strato`
- Branch reviewed: `review-vibes` (tracking `origin/vibes`)
- Reference repo: `/home/exedev/repos/ty`
- `ty`'s `ruff` submodule was initialized so the review could inspect live internals

---

## OpenCode codereview-roasted output

🔴 **Taste Rating: Needs improvement**

**Linus-Style Analysis**

**[CRITICAL ISSUES]**
- [`docs/book/src/call-graph-type-resolution.md:181-191`, `docs/book/src/design-overview.md:58-62`] **ty Integration Fantasy API**: The proposed `TypeResolver` API assumes ty can answer `resolve_type(&Expr)`, `resolve_callee(&Expr)`, `resolve_attribute(...)`, and `mro(...)` over Strato-owned Ruff AST nodes. Latest ty does not expose that clean API. The public surface is `SemanticModel` plus traits like `HasType`, and it explicitly warns that AST nodes from the wrong file/model may panic (`/home/exedev/repos/ty/ruff/crates/ty_python_semantic/src/semantic_model.rs:26-36`, `:580-585`). Also, ty’s MRO machinery is `pub(crate)` (`/home/exedev/repos/ty/ruff/crates/ty_python_semantic/src/types/mro.rs:37`, `:54`), not a stable public resolver API. This design is building on sand. Simpler architecture: either make ty’s `ProjectDatabase`/`SemanticModel` the source of truth for parsed files and types, or keep Strato’s AST-only resolver and treat ty integration as a spike-backed optional adapter with exact APIs listed from the pinned rev.

- [`docs/book/src/analysis-pipeline.md:67-150`, `docs/book/src/appendix-e-repository-structure-implementation-plan.md:132-136`] **Duplicate Import Resolver**: The docs plan a large custom module resolver even though ty already has `ty_module_resolver` with `SearchPaths`, `resolve_module`, `resolve_real_module`, namespace/package handling, stub modes, and cached Salsa queries (`/home/exedev/repos/ty/ruff/crates/ty_module_resolver/src/lib.rs:10-14`, `/home/exedev/repos/ty/ruff/crates/ty_module_resolver/src/resolve.rs:55-64`, `:133-154`). Reimplementing Python import resolution is not “architecture”, it’s volunteering for years of bug-for-bug compatibility pain. Use ty’s resolver/project setup, then layer Strato’s blocking graph on top.

- [`docs/book/src/supporting-systems.md:127-141`, `docs/book/src/design-overview.md:212-225`] **Cache Boundary Contradiction**: `CachedFileResult` includes `call_edges` (`supporting-systems.md:132`) while the design says ty results are not cached and phases 4-7, including call graph construction, rerun every time. Call edges are exactly where callee/type resolution happens, so caching them either caches ty-derived facts or makes cached runs less accurate than fresh runs. Pick one. Better: cache only source hash plus parsed symbol/import summaries, or cache fully resolved call edges with a config+ty-version+resolver-input cache key and admit you are caching resolution output.

- [`docs/book/src/error-reporting-diagnostics.md:90-139`] **Property/Dunder Classification Is Wrong**: The algorithm checks only the last edge kind. The doc’s own example says `async handler() -> loader.data [PropertyAccess] -> requests.get()` (`error-reporting-diagnostics.md:133`), but the last edge is getter-to-`requests.get`, usually `DirectCall`, not `PropertyAccess`. Same bug for dunders: `str(obj)` points to `__str__`, then `__str__` calls `requests.get`; the last edge is not the implicit dunder edge. STRATO003/004 will be misclassified as STRATO002. Classify by the first special edge on the path from async context, not the last edge to the blocking root.

- [`docs/book/src/blocking-propagation.md:43-58`] **`@non_blocking` Nukes Whole SCCs**: “If any function in the SCC is KnownNonBlocking, the entire SCC is treated as non-blocking” is unsound. One annotated function in a mutual-recursion cycle can hide another function in the same SCC that directly calls `time.sleep`. That violates the high-precision story in the worst possible direction: silent false negatives caused by one decorator. Simpler rule: `@non_blocking` suppresses diagnostics for that function as an entry/intervention point, but it must not erase blocking facts for other SCC members.

- [`docs/book/src/intro.md:26`, `docs/book/src/design-overview.md:102`] **“Zero False Positives” Is Already False**: The intro promises “Zero false positives”, then Escape Hatches says unconfigured safe third-party wrappers will be flagged as blocking. That is a false positive. Don’t make impossible marketing claims. Say “low false positive, proven-blocking policy, except unresolved escape hatches require configuration.”

**[IMPROVEMENT OPPORTUNITIES]**
- [`docs/book/src/design-overview.md:60`, `docs/book/src/open-questions-reviewers.md:52`] **Panic Isolation Is Hand-Wavy**: ty already catches panics around file checking in `ty_project` (`/home/exedev/repos/ty/ruff/crates/ty_project/src/lib.rs:649-656`, `:719-747`). Strato’s “catch panics, downgrade per-file” claim needs to explain whether it uses ty_project’s checked entry points or wraps arbitrary semantic queries itself. Catching around random Salsa queries can interact badly with cancellation and propagated panics.

- [`docs/book/src/design-overview.md:60`] **Double Parse Claim Is Overconfident**: The docs say Strato will parse with Ruff and ty will internally parse again, with “<100ms for 500 files”. Latest ty’s semantic model is tied to `ruff_db::parsed::parsed_module` (`/home/exedev/repos/ty/ruff/crates/ty_python_semantic/src/semantic_model.rs:1-7`, `:46-52`). If Strato keeps separate ASTs, you have node identity/mapping problems. If Strato uses ty’s parsed files, there is no inherent double parse. The real decision is not cost; it is ownership of the source database.

- [`docs/book/src/call-graph-type-resolution.md:199-225`, `docs/book/src/known-limitations-scope-boundaries.md:15-16`] **Type Capability Claims Contradict Each Other**: One table says ty gives generic instantiation and union narrowing (`call-graph-type-resolution.md:208-209`), the next table says Strato will not use them (`:221-222`), and limitations say no generics/union tracking (`known-limitations-scope-boundaries.md:15-16`). That is not a spec, it is three positions in a trench coat. State what Strato consumes from ty: expression type, static attribute definitions, callable/function literal identity. Everything else is unsupported.

- [`docs/book/src/error-reporting-diagnostics.md:313-320`, `docs/book/src/appendix-c-output-format-specifications.md:51-56`] **Column Convention Contradiction**: Appendix C says JSON columns are 1-indexed; Error Reporting says JSON is 0-based. Pick one. For external tools, use 1-indexed in JSON too unless you are explicitly making an LSP protocol. Internal byte offsets should not leak into a generic JSON report.

- [`docs/book/src/appendix-b-acceptance-test-cases.md:420-449`, `docs/book/src/appendix-c-output-format-specifications.md:51-57`] **JSON Schema Inconsistency**: Acceptance test A18 uses `"location"`, while Appendix C defines `"primary_location"`. That will produce useless golden tests because nobody knows which contract is real.

- [`docs/book/src/analysis-pipeline.md:17-21`, `docs/book/src/appendix-d-configuration-schema.md:9`] **Config Names Drifted**: Phase 1 says `source_roots` and `blocking_db_path`; Appendix D defines `src_roots` and no `blocking_db_path`. Specs that can’t agree on key names become broken CLIs.

- [`docs/book/src/analysis-pipeline.md:182-184`, `docs/book/src/blocking-function-database-annotations.md:237-243`] **Stub Annotation Format Contradiction**: Phase 5 says `.pyi` stubs use `# strato: blocking` comments; the annotations chapter says stubs use `@blocking` decorators. Pick decorators. Comments are another parser and another mini-language for no gain.

- [`docs/book/src/blocking-function-database-annotations.md:40-49`, `docs/book/src/appendix-a-blocking-function-database.md:5-93`] **Blocking DB Counts Are Fiction**: The docs repeatedly claim 80+ entries. Appendix A lists 60 by my count, and the File I/O category claims 23 but lists 20. If the “complete database” can’t count itself, users should not trust its coverage claims.

- [`docs/book/src/escape-hatches-executor-wrappers.md:156`] **Duplicate Key Semantics Are Nonsense**: “Duplicate keys are rejected (last one wins, with a warning)” is two mutually exclusive behaviors. TOML duplicate keys are generally invalid before your config loader sees them. Remove this.

- [`docs/book/src/blocking-function-database-annotations.md:189`, `docs/book/src/escape-hatches-executor-wrappers.md:111`, `docs/book/src/intro.md:32-35`] **v1 vs v1.1 Scope Is Muddy**: `@unblocker` and generalized wrappers are called v1.1 additions, but the intro lists executor wrapper recognition and built-in scope as v1. Appendix B also requires `@unblocker` in acceptance test A14. This is not a roadmap; it is scope leakage. Keep the feature if needed, but call it v1 consistently.

- [`docs/book/src/analysis-pipeline.md:65`, `docs/book/src/analysis-pipeline.md:145-146`, `docs/book/src/error-reporting-diagnostics.md:409-425`] **Determinism Contract Violated By Data Structures**: The determinism section says output-affecting maps use `BTreeMap`, but the pipeline still specifies `HashMap` for parsed files, module map, and symbol table. If those feed graph order, path selection, or diagnostics, they are output-affecting. Either change them to ordered maps or explicitly sort at every boundary.

**[TESTING GAPS]**
- [`docs/book/src/appendix-b-acceptance-test-cases.md:143-187`] **Property/Dunder Tests Are Too Shallow**: A8 and A9 only test the happy path. Add tests where the property/dunder is an intermediate edge before the blocking root, because the current classification algorithm would fail exactly there.

- [`docs/book/src/appendix-b-acceptance-test-cases.md:121-140`] **No SCC Decorator Test**: There is no test for a mutually recursive SCC where one member is `@non_blocking` and another reaches a blocking root. That is where the current propagation rule hides real bugs.

- [`docs/book/src/appendix-b-acceptance-test-cases.md:319-340`] **Executor Wrapper Tests Don’t Cover Alias Reality**: The docs lean on ty for `safe = sync_to_async(func); await safe()`, but no acceptance case proves alias-created wrappers. Add it or stop claiming alias tracking as critical functionality.

**VERDICT:**
❌ **Needs rework**: The high-level goal is good, but the design currently depends on a ty API that does not exist in the shape described, duplicates ty’s resolver, contradicts its own cache and output contracts, and has an actual bug in diagnostic classification.

**KEY INSIGHT:**
Use ty as the project/source/type database instead of pretending it is a simple expression-to-type oracle; then build the smallest deterministic blocking graph on top of ty-resolved definitions.

---

## Synthesized follow-up notes

### Overall read
The design goal is strong, but the docs are currently too eager to describe a clean-room semantic stack when the better architecture is to lean much harder on `ty` for source/project/type semantics and keep Strato focused on blocking-specific graph construction, propagation, and reporting.

### Main themes

#### 1. `ty` should be the semantic substrate
Instead of:
- Strato parses files
- Strato resolves imports
- Strato builds symbol tables
- Strato asks `ty` ad hoc questions

Prefer:
- `ty` / Ruff DB owns source files, parsing, module resolution, and semantic lookup
- Strato consumes:
  - resolved defs/callees
  - type/class/member lookup
  - package/source-root semantics
- Strato adds:
  - blocking DB
  - escape-hatch modeling
  - blocking propagation
  - diagnostics/reporting

This removes both the fake adapter problem and the duplicate-resolver problem.

#### 2. Separate semantic facts from blocking facts
Use two explicit layers:

- **Semantic layer**
  - file graph
  - imports/modules
  - callable definitions
  - resolved edges
  - property/dunder lowering

- **Blocking layer**
  - known blocking roots
  - executor-protected edges
  - SCC propagation
  - intervention-point selection

This makes caching and correctness easier to reason about.

#### 3. Model path semantics directly
Instead of inferring STRATO003/004 from the leaf edge, record path metadata such as:
- `contains_property_access`
- `contains_implicit_dunder`
- first special edge location/kind

Then classify from path semantics rather than a brittle last-edge heuristic.

#### 4. Treat annotations as reporting overrides, not truth bombs
Especially for `@non_blocking`:
- `@blocking` can seed a blocking root
- `@non_blocking` can suppress diagnostics for that callable or mark “don’t report here”
- but propagation facts should remain structurally derived unless the design explicitly accepts unsoundness

This avoids letting one decorator falsify the graph.

#### 5. Make cache keys honest
If cached call edges depend on:
- ty revision
- Python version
- config
- blocking DB
- stub paths
- source roots / search paths

then the cache key needs those inputs. Otherwise the design should cache only earlier artifacts and rebuild resolved edges every run.

### Internal consistency issues to fix
- `source_roots` vs `src_roots`
- `blocking_db_path` present in one place, absent in schema
- `location` vs `primary_location`
- 0-based vs 1-based JSON columns
- stub comments vs decorator-based annotation format
- v1 vs v1.1 treatment of `@unblocker` / generalized wrappers
- deterministic output claims vs `HashMap` usage in key structures
- blocking DB entry counts
- contradictory TOML duplicate-key behavior

### Testing gaps to add
- Property/dunder cases where the special edge is not the leaf edge
- SCC with one `@non_blocking` function and another real blocker
- Alias-based wrapper cases, since the docs rely on ty alias tracking
- Determinism regression tests across repeated runs
- Cache parity tests: fresh vs cached should produce identical diagnostics

### Suggested one-sentence framing
Use `ty` as the project/type database, not as a tiny expression oracle; then keep Strato’s custom logic narrowly focused on blocking-specific graph semantics and diagnostics.
