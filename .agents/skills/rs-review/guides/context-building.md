# Build context before you hunt

> **rs-review · guide** — rs-skills original

A review that starts hunting on unfamiliar code produces confident, wrong findings. First build **understanding, not verdicts**: what each function assumes, what it guarantees, what it depends on elsewhere. Don't name vulnerabilities, rate severity or propose fixes in this step.

## Per function (the micro-analysis record)

| Slot | Content | Rule |
|---|---|---|
| **Purpose** | one sentence | cite the line that shows it |
| **Guarantees** | what is always true on return (`Ok` value is non-empty, tenant matches, sorted) | each with the line that establishes it |
| **Assumptions** | what it takes on faith (index in range, header already validated, caller holds the lock) | each with **what makes it true** — or the words `nothing found` |
| **Calls** | callee, and what this function needs from it | read the callee; follow **every** path through it, not just success |
| **Open questions** | what is still unclear | keep them; a confident wrong answer is worse than an honest list |

## Rules

1. **Follow the calls.** Correctness usually depends on another function: a limit "is enforced" because a helper with a reassuring name returned — read the helper. Say what makes each assumption true.
2. **`nothing found` is the most useful output.** An assumption nothing enforces goes straight to the hunting phase (rs-security-review) as a candidate.
3. **Cite or ask.** Every claim carries a `file:line`, or becomes an open question.
4. **Where two records disagree, quote both** — it is a fact about the code, not a flaw in the analysis.
5. **Context first, in its own pass.** For more than a function or two, write the records to files (`work-log.md` links them) instead of holding them in the conversation.

## Whole-system view (after the functions)

- **Invariants** that span functions ("every request carries an authenticated principal before `route()`").
- **Trust boundaries** and who can reach what (entry points → sinks).
- **Validation map**: what is checked where (defence in depth, or a single point of failure).
- **Where complexity clusters** (long functions, deep nesting, many callers) — review depth follows it.

Rust specifics to record: where `unsafe`/FFI begins and ends (the safe API's promises), which types are `Send`/`Sync` and why, ownership of shared state (who locks what, actor or mutex), panic strategy (`unwind`/`abort`), error-to-response mapping at boundaries, and which crates hold the parsing.

Tools that help: rs-code-navigator, rs-call-graph, rs-symbol-analyzer, rs-trait-explorer (LSP); `rust-caps.sh`; `cargo geiger` for unsafe counts. Optional external code-graph tools can add call-graph, taint and blast-radius queries — install separately if you want them.
