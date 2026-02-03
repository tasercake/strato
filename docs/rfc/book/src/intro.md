# RFC: Strato — Async Blocking Call Detector for Python

> **Status**: Draft — seeking expert review
> **Authors**: [TBD]
> **Date**: 2026-02-04
> **Review period**: [TBD]

> **How to review this document**: This RFC is structured in layers. Sections 1-2 give you enough context to understand the problem and our approach. Sections 3-11 are the detailed design. Section 12 documents known limitations. Section 13 collects open questions. Each section is tagged with the expertise most relevant to it: **[async]** for Python async experts, **[analysis]** for static analysis / PL experts, **[tooling]** for Rust/tooling experts.
>
> You don't need to read everything — focus on the sections tagged with your expertise, and especially **Section 13 (Open Questions)** where we most need your input.

### Reviewer Routing Guide

| Your expertise | Read these sections | Skip these |
|----------------|--------------------|----|
| **Python async** [async] | [1](./01-executive-summary.md#1-executive-summary), [2](./02-problem-statement-motivation.md#2-problem-statement--motivation), [3.1](./03-design-decisions.md#31-transitive-call-graph-vs-pattern-matching)-[3.2](./03-design-decisions.md#32-precision-policy-unknown--not-blocking), [3.6](./03-design-decisions.md#36-generalized-executor-wrapper-system), [3.8](./03-design-decisions.md#38-blocking-database-curated-list-vs-exhaustive)-[3.9](./03-design-decisions.md#39-help-text-policy-no-third-party-recommendations), [3.16](./03-design-decisions.md#316-async-scope-boundary-asyncio-only), [8](./08-blocking-function-database-annotations.md#8-blocking-function-database--annotations), [9](./09-escape-hatches-executor-wrappers.md#9-escape-hatches--executor-wrappers), [12](./12-known-limitations-scope-boundaries.md#12-known-limitations--scope-boundaries), [13](./13-open-questions-reviewers.md#13-open-questions-for-reviewers) | [6](./06-call-graph-type-resolution.md#6-call-graph--type-resolution) (call graph internals), [11](./11-supporting-systems.md#11-supporting-systems) (tooling) |
| **Static analysis / PL** [analysis] | [1](./01-executive-summary.md#1-executive-summary), [2](./02-problem-statement-motivation.md#2-problem-statement--motivation), [3.1](./03-design-decisions.md#31-transitive-call-graph-vs-pattern-matching)-[3.5](./03-design-decisions.md#35-phantom-nodes-for-external-symbols), [5](./05-analysis-pipeline.md#5-analysis-pipeline), [6](./06-call-graph-type-resolution.md#6-call-graph--type-resolution), [7](./07-blocking-propagation.md#7-blocking-propagation), [12](./12-known-limitations-scope-boundaries.md#12-known-limitations--scope-boundaries), [13](./13-open-questions-reviewers.md#13-open-questions-for-reviewers) | [8](./08-blocking-function-database-annotations.md#8-blocking-function-database--annotations) (blocking database), [11](./11-supporting-systems.md#11-supporting-systems) (tooling) |
| **Rust / tooling** [tooling] | [1](./01-executive-summary.md#1-executive-summary), [3.10](./03-design-decisions.md#310-language-choice-rust)-[3.15](./03-design-decisions.md#315-failure-and-warning-policy), [4](./04-architecture-overview.md#4-architecture-overview), [10](./10-error-reporting-diagnostics.md#10-error-reporting--diagnostics), [11](./11-supporting-systems.md#11-supporting-systems), [Appendix C](./appendix-c-output-format-specifications.md#appendix-c-output-format-specifications)-[E](./appendix-e-repository-structure-implementation-plan.md#appendix-e-repository-structure--implementation-plan), [13](./13-open-questions-reviewers.md#13-open-questions-for-reviewers) | [7](./07-blocking-propagation.md#7-blocking-propagation) (propagation algorithm), [9](./09-escape-hatches-executor-wrappers.md#9-escape-hatches--executor-wrappers) (escape hatches) |
| **Everyone** | [1](./01-executive-summary.md#1-executive-summary), [2](./02-problem-statement-motivation.md#2-problem-statement--motivation), [12](./12-known-limitations-scope-boundaries.md#12-known-limitations--scope-boundaries), [13](./13-open-questions-reviewers.md#13-open-questions-for-reviewers) | — |

### Glossary

| Term | Definition |
|------|-----------|
| **Blocking call** | A function call that performs synchronous I/O or waits, stalling the event loop (e.g., `time.sleep()`, `requests.get()`) |
| **Transitive blocking** | A function that is not itself blocking but calls a blocking function through one or more intermediary calls |
| **Event loop** | The asyncio mechanism that schedules and runs coroutines concurrently on a single thread |
| **Call graph** | A directed graph where nodes represent functions and edges represent call relationships |
| **SCC (Strongly Connected Component)** | A maximal set of nodes in a directed graph where every node is reachable from every other node (mutual recursion) |
| **Phantom node** | A call graph node for an external symbol (e.g., `time.sleep`) with no source location, pre-seeded from the blocking database |
| **Escape hatch** | A pattern that correctly offloads blocking work to a thread pool (e.g., `asyncio.to_thread()`, `loop.run_in_executor()`) |
| **Intervention point** | The source location shown in a diagnostic — where the user should make a change |
| **First-party code** | Code in the user's project (under configured source roots) |
| **Third-party code** | Code from external packages (stdlib, site-packages) |
| **ty** | Astral's Python type inference crate, used for resolving method calls, properties, and dunder invocations |
| **Salsa** | A query-based incremental computation framework used by ty for in-memory memoization |
| **Propagation** | The process of spreading "blocking" status through the call graph from known blocking functions to their callers |
| **Condensation graph** | A DAG formed by collapsing each SCC into a single node — enables single-pass topological propagation |

---

## Table of Contents

1. [Executive Summary](./01-executive-summary.md#1-executive-summary)
2. [Problem Statement & Motivation](./02-problem-statement-motivation.md#2-problem-statement--motivation)
3. [Design Decisions](./03-design-decisions.md#3-design-decisions)
4. [Architecture Overview](./04-architecture-overview.md#4-architecture-overview)
5. [Analysis Pipeline](./05-analysis-pipeline.md#5-analysis-pipeline)
6. [Call Graph & Type Resolution](./06-call-graph-type-resolution.md#6-call-graph--type-resolution)
7. [Blocking Propagation](./07-blocking-propagation.md#7-blocking-propagation)
8. [Blocking Function Database & Annotations](./08-blocking-function-database-annotations.md#8-blocking-function-database--annotations)
9. [Escape Hatches & Executor Wrappers](./09-escape-hatches-executor-wrappers.md#9-escape-hatches--executor-wrappers)
10. [Error Reporting & Diagnostics](./10-error-reporting-diagnostics.md#10-error-reporting--diagnostics)
11. [Supporting Systems](./11-supporting-systems.md#11-supporting-systems)
12. [Known Limitations & Scope Boundaries](./12-known-limitations-scope-boundaries.md#12-known-limitations--scope-boundaries)
13. [Open Questions for Reviewers](./13-open-questions-reviewers.md#13-open-questions-for-reviewers)

**Appendices**
- [A: Blocking Function Database (Complete)](./appendix-a-blocking-function-database.md#appendix-a-blocking-function-database-complete)
- [B: Acceptance Test Cases](./appendix-b-acceptance-test-cases.md#appendix-b-acceptance-test-cases)
- [C: Output Format Specifications](./appendix-c-output-format-specifications.md#appendix-c-output-format-specifications)
- [D: Configuration Schema](./appendix-d-configuration-schema.md#appendix-d-configuration-schema)
- [E: Repository Structure & Implementation Plan](./appendix-e-repository-structure-implementation-plan.md#appendix-e-repository-structure--implementation-plan)
