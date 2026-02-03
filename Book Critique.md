# Book Critique: Strato RFC

## Grading Criteria

| Criterion | Weight | What It Measures |
|-----------|--------|------------------|
| **Structural Clarity** | 20% | Logical organization; can the reader find what they need? |
| **Audience Calibration** | 20% | Is the complexity level right for the stated audience? |
| **Internal Consistency** | 15% | Do sections agree with each other? Are terms stable? |
| **Pedagogical Effectiveness** | 15% | Does the book teach what the reader needs to evaluate the design? |
| **Completeness** | 15% | Are there gaps that would block implementation or meaningful review? |
| **Conciseness** | 15% | Signal-to-noise ratio; is repetition purposeful or wasteful? |

---

## Scores

| Criterion | Score | Notes |
|-----------|-------|-------|
| Structural Clarity | **B+** | Strong linear flow, but Chapter 3 is a monolith and the deep-dive chapters (6-7) need better on-ramps |
| Audience Calibration | **B-** | Excellent for [async] and [tooling] readers; significantly overestimates [analysis] literacy of the target audience |
| Internal Consistency | **B** | Mostly tight, but several specific contradictions (column conventions, help text policy, version labeling) |
| Pedagogical Effectiveness | **B-** | Great decision-log format; missing an end-to-end worked example and scaffolding for graph theory concepts |
| Completeness | **A-** | Remarkably thorough for a draft RFC; only meaningful gap is the running-example walkthrough |
| Conciseness | **B** | Some beneficial repetition, some wasteful; the motivating examples need deduplication |

**Overall: B / B+** – A strong technical specification that would significantly benefit from three targeted improvements: (1) audience-aware scaffolding for Sections 6-7, (2) an end-to-end worked example, and (3) consistency cleanup passes on a handful of contradictions.

---

## Detailed Findings

### 1. Chapter 3 (Design Decisions) Is a Monolith

**Severity: Moderate**

Chapter 3 packs 16 design decisions (~336 lines) into a single file with no structural preamble. This is the longest chapter by far and serves dual duty as both a narrative progression and a reference section. The individual decisions are well-structured (Context → Options → Choice → Rationale → Risk), but the chapter as a whole lacks:

- **A summary table up front.** A reader scanning for "what did they decide about caching?" must scroll through 16 decisions sequentially. A table mapping decision number → topic → one-line choice would save significant navigation time.
- **Thematic grouping.** Decisions 3.1-3.5 are analysis-related, 3.6-3.9 are async/user-facing, 3.10-3.15 are tooling, 3.16 is scope. These clusters are not delineated with headers or visual separation.
- **Priority signaling.** Not all 16 decisions carry equal weight for a non-specialist reader. Decisions 3.1 (call graph vs pattern matching), 3.2 (precision policy), and 3.4 (ty integration) are load-bearing architectural choices. Decisions 3.14 (determinism) and 3.15 (failure policy) are implementation details. A "start here" marker or tiered reading guide would help.

**Recommendation:** Add a decision summary table at the top of Chapter 3. Consider splitting into sub-pages by theme (Analysis Decisions, User-Facing Decisions, Tooling Decisions) for the mdbook rendering, or at minimum add `---` separators and H2 headers between thematic clusters.

---

### 2. Sections 6-7 Assume Graph Theory Expertise the Audience Doesn't Have

**Severity: Serious**

This is the single most significant audience calibration problem in the book. Sections 6 (Call Graph & Type Resolution) and 7 (Blocking Propagation) are the intellectual core of the design, but they deploy technical vocabulary that a Python developer – even an experienced one – likely hasn't encountered:

- **SCC (Strongly Connected Component)** – defined in the glossary, but the glossary definition ("maximal set of nodes where every node is reachable from every other") doesn't build intuition for *why* this matters.
- **Condensation graph** – mentioned without a diagram. The concept is "collapse each cycle into a single dot, now you have a tree-like structure you can walk in one pass." This is not explained at intuition level.
- **Topological sort** – used as if common knowledge. The non-SA reader needs to know: "process leaves first, then work upward."
- **Tarjan's algorithm** – named but not explained. It could be treated as an implementation detail ("we use a standard graph algorithm for SCC detection") rather than foregrounded.

The glossary helps but does not solve this. Glossaries are reference tools; they don't build working understanding. A reader encountering "SCC-based propagation" for the first time needs a 3-sentence intuition primer *at the point of use*, not a lookup in a different chapter.

**Recommendation:** Add a short "Graph Concepts for This Book" subsection (5-10 paragraphs) at the start of Section 6 or as a standalone interlude between Sections 5 and 6. Cover: what a call graph is (with a tiny visual), what cycles mean in practice (mutual recursion), why cycles are problematic for propagation, and how SCC decomposition solves this. Use a concrete Python example:

```python
# These two functions form a cycle (SCC):
def validate(data):
    if complex: return check(data)

def check(data):
    return validate(data.subset)
```

After this primer, Sections 6-7 can reference these concepts freely. This single addition would make the most technically dense chapters accessible to the target audience.

---

### 3. No End-to-End Worked Example Through All 7 Phases

**Severity: Serious**

The pipeline (Discovery → Parse → Resolve → Build → Annotate → Propagate → Report) is introduced in Section 4 with a clear ASCII diagram and explained phase-by-phase in Section 5. But no single example is traced through all 7 phases to show how a concrete blocking call is discovered, tracked, and reported.

This matters because:
- Non-SA readers understand systems by watching them work on real inputs, not by reading abstract phase descriptions.
- The phases have data dependencies (Phase 3's symbol table feeds Phase 4's call graph, which feeds Phase 6's propagation). Without a concrete trace, readers can't verify their understanding of how data flows.
- The acceptance test cases in Appendix B show inputs and expected outputs but skip the middle: *how* does the tool get from input to output?

**Recommendation:** Add a "Worked Example" chapter (or a prominent subsection in Chapter 5) that traces this code through every phase:

```python
# utils.py
import time
def helper():
    time.sleep(1)

# main.py
from utils import helper
async def handler():
    helper()
```

Show, for each phase:
1. **Discovery**: Files found: `main.py`, `utils.py`. Config: default.
2. **Parse**: ASTs extracted. Symbols: `handler` (async), `helper` (sync).
3. **Resolve**: `from utils import helper` → `utils.helper` in symbol table.
4. **Build**: Call graph edge: `handler → helper → time.sleep` (phantom).
5. **Annotate**: `time.sleep` marked `KnownBlocking` from database.
6. **Propagate**: `helper` becomes `PropagatedBlocking` (calls `time.sleep`).
7. **Report**: STRATO002 at `main.py:4:5` with chain `handler → helper → time.sleep`.

A table format would work well. This would be the single highest-impact addition to the book.

---

### 4. Version Labeling Creates Ambiguity

**Severity: Moderate**

The book uses "v1.0" and "v1.1" to refer to design iterations (e.g., Section 3.4: "v1.0 used ScopeBindings, v1.1 uses ty"; Section 3.12: "v1.0 was too restrictive... v1.1 extensions address the most common gaps"). However, it never explicitly states which version the book documents.

A reader encountering "the v1.0 design used a hand-rolled `ScopeBindings` system" doesn't know whether they're reading current design or historical context. This ambiguity compounds when decisions reference both versions in the same paragraph:

> "The v1.0 design used a hand-rolled ScopeBindings system... v1.1 approach: Replaced with Astral's ty crate"

Is this book v1.0 or v1.1? The reader must infer from context that v1.1 is the current design, but this is never stated.

**Recommendation:** Add a single sentence to the intro: "This book documents the v1.1 design. Where v1.0 behavior is mentioned, it is historical context." Then visually distinguish historical notes (e.g., with an admonition block or italic preamble: "*Historical note: v1.0 used...*").

---

### 5. Help Text Policy Contradicts the Blocking Database

**Severity: Moderate**

Decision 3.9 establishes the policy: **"No Third-Party Recommendations"** in help text. The rationale is explicit: "Strato is a linting tool, not a library recommendation engine. Help text lists multiple alternatives neutrally... without prescribing one."

But the actual help text in Appendix A and Section 8.2 does recommend specific third-party libraries:

| Function | Help Text |
|----------|-----------|
| `requests.get` | "Use `aiohttp` or `httpx`" |
| `psycopg2.connect` | "Use `asyncpg`" |
| `sqlite3.connect` | "Use `aiosqlite`" |

`aiohttp`, `httpx`, `asyncpg`, and `aiosqlite` are all third-party packages. The policy says "no single library endorsement," but recommending a shortlist of 1-2 libraries *is* endorsement. The distinction between "recommending one library" and "recommending two libraries" is not meaningful – Strato is still choosing which libraries to name.

**Recommendation:** Resolve this one of two ways:
1. **Tighten the policy**: Change 3.9 to say "Help text may name well-known async alternatives as examples, but does not endorse any specific library." This acknowledges the practice and removes the contradiction.
2. **Enforce the policy**: Change help text to generic advice: "Use an async HTTP library" instead of "Use `aiohttp` or `httpx`". This is less actionable but more consistent.

Option 1 is more practical – naming specific libraries is genuinely helpful to users.

---

### 6. Reviewer Routing Tags Are Inconsistently Applied

**Severity: Minor to Moderate**

The intro establishes a tag system: `[async]`, `[analysis]`, `[tooling]`, `everyone`. The reviewer routing table maps expertise to sections. This is a genuinely good idea for a multi-audience document.

However, the tagging is applied inconsistently:

- Chapter 3 has inline tags on individual decisions (`*Tags: async, analysis*`), but the chapter heading itself has no tag.
- Chapter 10 has `[async] [tooling]` as a standalone line between the chapter heading and the first subsection – easy to miss.
- Chapter 12 has `**Tags**: everyone` as the first line.
- Chapters 4, 5, 6, 7 have tags in prose form or not at all.
- The intro's routing table uses bracketed format `[async]`, but the chapters use various formats.

**Recommendation:** Standardize on one format (e.g., `**Tags**: async, analysis` as the first line after each chapter heading). Add tags to every chapter file. The mdbook build could also be extended with a tag index page if the book grows.

---

### 7. Motivating Examples Are Repeated Without Distinct Purpose

**Severity: Minor to Moderate**

The `handler → helper → time.sleep` blocking-via-intermediary example appears in:
- The project README (referenced by the intro)
- Section 2.5.1 (Problem Statement – motivating the need)
- Section 10.5 (Error Reporting – showing diagnostic output)
- Appendix B, Test Case A2 (specifying expected behavior)

Repetition is beneficial when each appearance has a distinct job. Here:
- Section 2.5.1 and the README serve the same purpose (sell the problem).
- Section 10.5 and Appendix B serve overlapping purposes (show what the output looks like).

The property and dunder examples (Sections 2.5.2, 2.5.3) also reappear in Appendix B (A8, A9) with slightly different framing.

**Recommendation:** Designate Section 2.5 as the canonical home for motivating examples. In Section 10 and Appendix B, reference them: "Using the example from Section 2.5.1, Strato produces..." This reduces redundancy while maintaining context.

---

### 8. Column Convention Table Has an Internal Contradiction

**Severity: Minor (but affects implementors)**

Section 10.4's "Column Convention" table states:

| Context | Convention |
|---------|-----------|
| JSON output | 0-based (matches internal, LSP convention) |

But the JSON example in Appendix C (Section C.2) uses 1-indexed columns:

```json
"primary_location": {
    "column": 5,    // This is 1-indexed (the call starts at the 5th character)
}
```

If `column` were 0-based, a call at the 5th character position would be `column: 4`. The example shows `column: 5`, matching 1-based convention. The schema description in C.2 also says `"column": "integer (1-indexed)"`.

So the convention table says 0-based for JSON, but the actual JSON spec and examples use 1-based. One of these is wrong.

**Recommendation:** Decide which is correct and fix the other. Given that the JSON schema explicitly says "1-indexed" and the examples are 1-indexed, the convention table in Section 10.4 likely needs correction.

---

### 9. Appendix B Is Normative But Treated as Supplementary

**Severity: Moderate**

Appendix B (Acceptance Test Cases) defines the tool's behavior more precisely than the prose in many cases. For example:

- A6 clarifies that `@blocking` marks a function as blocking *regardless of its implementation* (even if the body is `pass`). This is a behavioral specification, not a supplementary detail.
- A13 clarifies that `asyncio.to_thread(helper)` protects the wrapper but `helper()` direct does not. The prose mentions this, but A13 makes it unambiguous.
- A15's config schema (`"mylib.offload" = true`) differs from Section 9.4's schema (`{ callable_param = 0 }`). This is a specification conflict, not just supplementary detail.

The appendix is de facto normative (it's where the behavioral contract is most precisely defined) but is positioned as supplementary material that readers might skip.

**Recommendation:** Either:
1. Promote Appendix B to a main chapter ("Chapter 14: Behavioral Specification") and explicitly state that test cases are the source of truth for edge cases.
2. Or add a note to the main text: "Appendix B defines the executable behavioral contract. When prose descriptions and test cases disagree, the test cases are authoritative."

Also fix the A15 config schema inconsistency: `true` (A15) vs `{ callable_param = 0 }` (Section 9.4).

---

### 10. Missing "How to Read This Book" Path for Non-Experts

**Severity: Moderate**

The Reviewer Routing Guide in the intro maps expertise to sections, which is excellent. But it assumes the reader already identifies as one of three expert types (async, analysis, tooling). The stated target audience – "technical and proficient in Python, but not necessarily experts in static analysis" – may not fit neatly into any category.

A Python web developer wanting to evaluate whether Strato is useful for their FastAPI project would need:
1. Sections 1-2 (what and why)
2. Section 8 (what does it know about blocking functions?)
3. Section 9 (will it understand my `sync_to_async` wrappers?)
4. Section 11.1 (how do I run it?)
5. Section 12 (what can't it do?)
6. Appendix D (how do I configure it?)

This reading path is not documented. The routing table pushes this reader toward the "Everyone" row, which lists only Sections 1, 2, 12, and 13 – missing the most practically relevant sections.

**Recommendation:** Add a "Getting Started" reading path for the "practitioner who wants to evaluate Strato for their project" persona. This is likely the majority audience, not the expert reviewers the routing table targets.

---

### 11. The Implementation Plan Is Strictly Sequential Without Justification

**Severity: Minor**

Appendix E.3 declares the critical path as strictly sequential: M-1 → M0 → M1 → ... → M12. But the modular architecture suggests parallelism opportunities:

- M4 (Blocking Database) has no dependency on M3 (Call Graph) – it's a static data file.
- M8 (Diagnostics) has structural dependency on M5 (Propagation) but its *formatting* layer could be developed in parallel.
- M9 (CLI + Output) could begin before M8 completes – the CLI framework, argument parsing, and output formatters are independent of diagnostic content.

A strictly sequential plan for 13 milestones suggests either (a) the author hasn't considered parallelism, or (b) there's a workforce constraint (single developer) that isn't stated.

**Recommendation:** If single-developer, state that explicitly. If not, identify which milestones can overlap and annotate the dependency graph.

---

### 12. The `test_strategy` Design Decision (3.15) Is Misnamed

**Severity: Nitpick**

The SUMMARY.md and intro reference the design decisions by number, and Section 3.15 is titled "Failure and Warning Policy." However, the intro's routing table references `3.15` as part of the "Rust/tooling" block labeled `3.10-3.15`, and the section numbering skips a "Test Strategy" decision that the intro suggests should exist (referenced as `3.15` in the routing guide with the description implying test-related content). The actual Decision 3.15 is about failure/warning policy, not test strategy. The test strategy appears only implicitly in the acceptance test cases (Appendix B) and the golden-output comparison mention in Section 10.6.

**Recommendation:** Either add an explicit "Test Strategy" design decision (golden output comparison, fixture-based testing, etc.) or ensure the intro's routing table labels match actual section titles.

---

## Summary of Recommendations by Priority

### High Priority (structural/audience issues)
1. **Add graph theory primer** before Sections 6-7 – the single most impactful change for audience calibration
2. **Add end-to-end worked example** tracing a blocking call through all 7 pipeline phases
3. **Fix column convention contradiction** between Section 10.4 and Appendix C JSON spec
4. **Fix A15 config schema inconsistency** (`true` vs `{ callable_param = 0 }`)

### Medium Priority (navigability/consistency)
5. **Add decision summary table** at the top of Chapter 3
6. **Resolve help text policy contradiction** (Decision 3.9 vs Appendix A)
7. **Clarify version labeling** (state that the book documents v1.1)
8. **Acknowledge Appendix B's normative status** in the main text
9. **Add practitioner reading path** for the "evaluate Strato for my project" persona

### Low Priority (polish)
10. **Standardize reviewer routing tags** across all chapters
11. **Deduplicate motivating examples** across sections
12. **Annotate parallelism** in the implementation milestones
13. **Fix Decision 3.15 naming/reference alignment**

---

## What the Book Does Well

This critique focuses on improvements, but the book has several genuine strengths worth preserving:

1. **The decision-log format is exemplary.** Context → Options → Choice → Rationale → Risk is a model for technical design documentation. Every decision explicitly states what was rejected and why. This is rare and valuable.

2. **The precision policy (Decision 3.2) is clearly articulated and consistently applied.** The "Unknown = Unknown" principle is threaded through Sections 5, 6, 7, 10, and 12 without contradiction. This is the backbone of the design and it reads as a coherent philosophy.

3. **The error code design is intuitive.** Four error codes covering direct, indirect, property, and dunder cases is a clean taxonomy. The classification algorithm in Section 10.2 is precise and testable.

4. **The blocking database curation is well-reasoned.** ~80 entries with explicit categories, extensibility via config and decorators, and clear precedence rules. Appendix A is a complete, usable reference.

5. **The acceptance test suite (Appendix B) is comprehensive.** 19 test cases covering happy paths, edge cases, cross-file detection, and error handling. Each test has explicit expected output. This is implementation-ready.

6. **The known limitations section (Chapter 12) is honest and exhaustive.** Every skip-silently case is documented. Every unsupported import form is listed. This builds trust – the authors clearly understand what they can't do.

7. **The reviewer routing guide is a thoughtful accessibility feature.** Not many RFCs consider that different readers have different needs. The expertise-tagged sections, even with inconsistent application, are a step above most technical documents.

8. **The output format specifications (Appendix C) are production-quality.** Text, JSON, and SARIF formats are fully specified with examples. The SARIF mapping table and code-flow threading are particularly thorough.
