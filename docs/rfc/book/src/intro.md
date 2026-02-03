# Strato: Async Blocking Call Detector for Python

> **Status**: Draft – Seeking feedback & expert review

> **How to review this document**: This book is structured in layers. Sections 1-2 give you enough context to understand the problem and our approach. Sections 3-11 are the detailed design. Section 12 documents known limitations. Section 13 collects open questions. Each section is tagged with the expertise most relevant to it: **[async]** for Python async experts, **[analysis]** for static analysis / PL experts, **[tooling]** for Rust/tooling experts.
>
> You don't need to read everything – focus on the sections tagged with your expertise, and especially **Section 13 (Open Questions)** where we most need your input.

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
