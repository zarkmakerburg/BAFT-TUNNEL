---
kind: overview
scope: {{SCOPE}}
crate: (all)
date: {{DATE}}
commit: {{COMMIT}}
reviewer: rs-skills {{RS_VERSION}} (rs-review)
verdict: pending
critical: 0
high: 0
medium: 0
low: 0
---

# Review overview: {{SCOPE}}

> **pending** · 🔴 _ critical · 🟠 _ high · 🟡 _ medium · 🔵 _ low (including _ cross-crate)
> {{DATE}} · commit `{{COMMIT}}` · `{{PATH}}` — {{N_CRATES}} crates (≈ {{LINES}} lines)
> Day index: [README](README.md) · Agent work log: [work-log.md](work-log.md)

## TL;DR

<!-- 5–7 lines.
**What this is.** <the crates and what they do together>
**Verdict.** <Block | Fix before merge | Ship> — <why, in one sentence>
**Common weaknesses.** <the 2–3 patterns that repeat across crates>
**Start here.** 1. <X-1 or the worst P1> 2. … 3. …
**Good news.** <what is consistently good>
**Effort.** <S|M|L> total; <order of crates> -->

<details>
<summary>How to read this overview</summary>

- This page is the **map**; every crate has its own report with the full finding cards (links below).
- Phases are an order of work: P1 → P6. Inside a phase fix one crate per MR (worst first); cross-crate items (`X-n`) are one MR for all crates.
- Boxes (`- [ ]`) in a crate report are counted here; tick them there.
- Severity: 🔴 Critical blocks · 🟠 High fix before merge · 🟡 Medium debt · 🔵 Low polish.

</details>

## Crates at a glance

| Crate | Report | Lines | Verdict | 🔴 | 🟠 | 🟡 | 🔵 | Fixed | Biggest function |
|---|---|---:|---|---:|---:|---:|---:|---|---|

### Scores (0–5)

| Area | <!-- one column per crate --> |
|---|---:|
| Design Contracts | |
| Structure | |
| Correctness & safety | |
| Error handling | |
| Concurrency | |
| API & docs | |
| Tests | |
| Lints | |

## Fix plan

Phases run in order. Inside a phase, one crate per MR (worst first); cross-crate items are one MR for all crates.

| Phase | Goal | <!-- crate columns: "1🟠 2🟡" --> | Cross-crate | Effort | Done when |
|---|---|---|---|---|---|
| [P1 Safety & correctness](#p1--safety--correctness) | nothing unsafe, panicking or fail-open | | | S | each P1 item fixed + a test proving it |
| [P2 Hygiene & gates](#p2--hygiene--gates) | the tools can be trusted | | | S | fmt, clippy, doc, test green in every crate |
| [P3 Structure & contracts](#p3--structure--contracts) | feature modules + Design Contracts | | | L | `structure-check --strict` + `contract-check` pass |
| [P4 Design & idioms](#p4--design--idioms) | errors, types, logging follow the rules | | | M | no depth-rule item left open |
| [P5 Tests](#p5--tests) | every Invariant proven by a test | | | M | one test per contract Invariant |
| [P6 Release](#p6--release) | ready to ship | | | S | `/rs-release-check --strict` passes |

## Progress

| Phase | Status | Commits / MR | Notes |
|---|---|---|---|
| P1 | open | | |
| P2 | open | | |
| P3 | open | | |
| P4 | open | | |
| P5 | open | | |
| P6 | open | | |

<!-- status: open / in progress / done. Findings discovered while fixing get a card in their phase marked "(found during Pn)". -->

## P1 — Safety & correctness

<!-- Every P1 item from every crate (one line each: crate · id link · severity · where · title), then cross-crate cards. For P2–P4: at most 3 lines per crate + "+n more in <crate report>". A phase with nothing: "Nothing to do." -->

| Crate | ID | Sev | Where | Title |
|---|---|---|---|---|

### Cross-crate items

<!-- Cards for X-n items: duplication, shared crate, workspace lints, one decision that applies to all crates. Same card format as a crate report, with **Crates:** all. -->

## P2 — Hygiene & gates

| Crate | ID | Sev | Where | Title |
|---|---|---|---|---|

## P3 — Structure & contracts

| Crate | ID | Sev | Where | Title |
|---|---|---|---|---|

<!-- Target tree for the whole folder (workspace, shared crate, per-crate feature modules). -->

## P4 — Design & idioms

| Crate | ID | Sev | Where | Title |
|---|---|---|---|---|

## P5 — Tests

| Crate | ID | Sev | Where | Title |
|---|---|---|---|---|

## P6 — Release

| Crate | ID | Sev | Where | Title |
|---|---|---|---|---|

## Notes
