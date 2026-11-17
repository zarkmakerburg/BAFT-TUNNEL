---
kind: crate
scope: {{SCOPE}}
crate: {{CRATE}}
version: {{VERSION}}
date: {{DATE}}
commit: {{COMMIT}}
reviewer: rs-skills {{RS_VERSION}} (rs-review)
verdict: pending
critical: 0
high: 0
medium: 0
low: 0
---

# Review: `{{CRATE}}` — {{SCOPE}}

> **{{VERDICT}}** · 🔴 {{N_CRIT}} critical · 🟠 {{N_HIGH}} high · 🟡 {{N_MED}} medium · 🔵 {{N_LOW}} low
> {{DATE}} · commit `{{COMMIT}}` · scope: {{SCOPE_DESC}} · previous: {{PREVIOUS}}
> Day index: [README](README.md) · Agent work log: [work-log.md](work-log.md)

## TL;DR

<!-- 4–6 lines a busy developer can act on. No jargon.
**What it is.** <one sentence: what the crate does>
**Verdict.** <Block | Fix before merge | Ship> because <one sentence>.
**Start here.** 1. [P1-1](#p1-1) <title> · 2. [P1-2](#p1-2) <title> · 3. [P3-1](#p3-1) <title>
**Good news.** <1–2 things done well — keep them>
**Effort.** <S | M | L> (~<n> commits, one per phase) -->

<details>
<summary>How to read this report</summary>

- **Phases are an order of work, not a topic list.** Fix P1 first, then P2 … P6; each phase is one commit/MR and leaves the crate green.
- **Severity:** 🔴 Critical blocks merge · 🟠 High fix before merge · 🟡 Medium design debt · 🔵 Low polish.
- **Each finding is a card:** what is wrong → why it matters → the fix (with code) → a tick-box. Tick it and add the commit when fixed; the day index and the overview count the boxes.
- **Evidence** (what the tools said) is at the bottom: [Automated checks](#appendix--automated-checks). **What the reviewer did, in order,** is in [work-log.md](work-log.md).
- Every finding cites a rule id (`err-no-unwrap-prod`, `Extend by`, …): open `skills/<skill>/rules/<id>.md` for the long explanation.

</details>

## Fix plan

| Phase | Goal | 🔴 | 🟠 | 🟡 | 🔵 | Effort | Done when |
|---|---|---:|---:|---:|---:|---|---|
| [P1 Safety & correctness](#p1--safety--correctness) | nothing unsafe, panicking or fail-open | {{P1_COUNTS}} | every P1 item fixed + a test that proves it |
| [P2 Hygiene & gates](#p2--hygiene--gates) | the tools can be trusted | {{P2_COUNTS}} | fmt, clippy, doc, test green locally and in CI |
| [P3 Structure & contracts](#p3--structure--contracts) | feature modules with Design Contracts | {{P3_COUNTS}} | `structure-check --strict` + `contract-check` pass |
| [P4 Design & idioms](#p4--design--idioms) | errors, types, APIs, logging follow the rules | {{P4_COUNTS}} | no depth-rule finding left open |
| [P5 Tests](#p5--tests) | every Invariant proven by a test | {{P5_COUNTS}} | one test per contract Invariant |
| [P6 Release](#p6--release) | ready to ship | {{P6_COUNTS}} | `/rs-release-check --strict` passes |

<!-- Phase rows: replace {{Pn_COUNTS}} with "c | h | m | l | S/M/L". A phase with no findings: "0 | 0 | 0 | 0 | —". -->

**Progress:** 0 of 0 findings fixed <!-- update when ticking boxes: `scripts/review-index.py` recounts -->

## P1 — Safety & correctness

> UB / unsound `unsafe`, data races, panics on input, fail-open security paths, secrets in logs, violated contract Invariants, wrong results.

<!-- At-a-glance table (one line per finding), then a card for each Critical/High/Medium. Low findings can stay as table lines only.

| ID | Sev | Where | Title | Fixed |
|---|---|---|---|---|
| [P1-1](#p1-1) | 🟠 High | `src/orders/service.rs:42` | `unwrap()` on DB result panics the worker | ☐ |

<a id="p1-1"></a>
### P1-1 · 🟠 High · `src/orders/service.rs:42`
- [ ] **Fixed** — commit: _ · test: _
- **Rule:** `err-no-unwrap-prod` · **Area:** error handling

**What's wrong.** `find_order(id).unwrap()` is reached with an id taken from the request.
**Why it matters.** An unknown id panics the worker thread; one request can take the service down.
**Fix.**
```rust
// before
let order = repo.find_order(id).unwrap();
// after
let order = repo.find_order(id).await?.ok_or(OrderError::NotFound(id))?;
```
-->

## P2 — Hygiene & gates

> rustfmt, `[lints]` policy, clippy warnings, `cargo doc -D warnings`, failing or environment-bound tests, CI gates.

## P3 — Structure & contracts

> rs-structure split ladder (fn > 60, file > 400), group by feature, dumping grounds (`utils.rs`), duplication, visibility, missing or drifted Design Contracts. Ports & adapters leaks (`port-check.py`) and hand-rolled standards (rs-reuse-first) are reported here too.

### Proposed layout

<!-- Current vs proposed tree (rs-structure). Order the moves; each move compiles. -->

### Design Contract drift

| File | Contract says | Code does | Change |
|---|---|---|---|

## P4 — Design & idioms

> Depth rules: error handling (`err-*`), types (`type-*`, `api-parse-dont-validate`), ownership (`own-*`), API & docs (`api-*`, `doc-*`), performance (`perf-*`, `mem-*`), observability (`obs-*`), patterns (rs-patterns).

## P5 — Tests

> Missing tests for Invariants and edge cases, test layout (`test-*`), property / snapshot / concurrency tests where they pay off.

## P6 — Release

> MSRV, edition, semver, CHANGELOG, release profile, supply chain (`cargo-deny`), git hygiene (`git-check.sh audit`), image/target hygiene (rs-cleanup).

## Scores

| Area | Score (0–5) | Note |
|---|---:|---|
| Design Contracts | | |
| Structure | | |
| Correctness & safety | | |
| Error handling | | |
| Concurrency | | |
| API & docs | | |
| Tests | | |
| Lints | | |

## Since last review

<!-- If a previous report exists: mark each of its findings fixed / open / wontfix. -->

| Previous ID | Status | Note |
|---|---|---|

## Notes

<!-- Anything out of scope, follow-ups, questions for the author. -->

## Appendix — Automated checks

{{AUTOMATED}}
