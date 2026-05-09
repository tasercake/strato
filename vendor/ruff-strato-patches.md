# Strato Ruff Patch Ledger

## Pinned Vendor State

- Submodule: `vendor/ruff`
- Upstream URL: `https://github.com/astral-sh/ruff.git`
- Pinned commit: `767df43e00729f875bd6b8ac3632b933f8b066ce`
- Workspace membership: root `Cargo.toml` excludes `vendor/ruff` from the workspace.

## Current Patch State

No patches required yet for Task 2 initialization/audit.

No files under `vendor/ruff` were modified for this task, and no Strato blocking policy has been placed in vendored Ruff/ty. Future changes under `vendor/ruff` must be narrow fact-exposure patches for `strato_ty_adapter` only, with file, rationale, upstreamability, and test coverage recorded here before or alongside the vendored diff.

Task 5 implemented the initial `strato_ty_adapter` facade without vendored source changes. The adapter consumes public `ruff_db::parsed::parsed_module`, `ty_project::ProjectDatabase`, `ty_python_semantic::SemanticModel`, and public IDE fact helpers for name, generic attribute, and binary/unary operator definitions. Exact public APIs for descriptor-aware `property.fget` identity and event-loop `run_in_executor` identity remain absent at the pinned commit, so `resolve_property_getter` returns `Unknown` and `resolves_to_event_loop_run_in_executor` returns `false` rather than fabricating targets.

## Audited Required API Surface

| Required fact/API | Audit result at pinned commit | Patch status |
|---|---|---|
| `definitions_for_call` for `ExprCall` callees | No exact public API with this name is present. ty has call/type internals such as `Type::bindings`, callable bindings, and callable `Type` variants, but definition extraction from call expressions is not exposed as a Strato-ready public fact. | No patch applied yet; Task 5 adapter work should add a surgical fact API if existing internals cannot be consumed cleanly. |
| `definitions_for_callable_reference` for expressions passed as values | No exact public API with this name is present. `SemanticIndex::try_expression` exposes expression ingredients and ty has function/callable types, but a direct callable-reference-to-definition fact is not exposed. | No patch applied yet; Task 5 should validate and patch only fact exposure if needed. |
| Descriptor-aware property getter target for `ExprAttribute` | Descriptor machinery exists: `Type::member`, `Type::member_lookup_with_policy`, `Type::try_call_dunder_get`, `PropertyInstanceType::getter`, and `KnownBoundMethodType::PropertyDunderGet`. Public `definitions_for_attribute` is generic attribute resolution and is not a `property.fget` identity fact. | No patch applied; Task 5 facade returns `Unknown` for property getter queries pending a narrow fact-exposure patch. |
| `definitions_for_dunder_operation` for Strato operations | Dunder-call internals exist, including `Type::try_call_dunder`/policy variants used by iteration, subscript, diagnostics, and type operations. No exact operation-enum-to-definition fact API is present. | No patch applied yet; Task 5/7 should patch fact exposure rather than implement policy in Ruff. |
| Event-loop `run_in_executor` target identity | `KnownFunction` does not include `asyncio` event-loop `run_in_executor` at this commit. No exact public identity helper or documented qualified-alias fact was found. | No patch applied; Task 5 facade returns `false` for executor identity queries pending a narrow fact-exposure patch. |
| Deterministic qualified display name for `Definition` | `Definition::name` is public and stable for simple names; class/type-alias qualified-name helpers exist in ty display internals, but a deterministic qualified name for arbitrary `Definition` is not exposed as a public API. | No patch applied yet; likely requires a narrow public helper for adapter normalization. |
| External qualified aliases for non-first-party calls | Import/re-export metadata exists (`Definition::is_reexported`, import definition kinds, public type/re-export handling), but an external alias set for resolved calls is not exposed as a dedicated public fact. | No patch applied yet; Task 5 should add the smallest alias fact API needed by the facade. |

## Validation Notes

- `.gitmodules:1` defines the Ruff submodule URL.
- `Cargo.toml:7` keeps `vendor/ruff` excluded from workspace membership.
- `docs/book/src/appendix-e-repository-structure-implementation-plan.md:64` requires the vendor location and this ledger.
- `docs/book/src/appendix-e-repository-structure-implementation-plan.md:211` limits vendored changes to semantic fact exposure.
- `docs/book/src/appendix-e-repository-structure-implementation-plan.md:227` lists the required fact surface audited above.
