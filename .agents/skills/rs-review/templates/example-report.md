---
kind: crate
scope: orders
crate: orders
version: 0.4.0
date: 2026-01-15
commit: a1b2c3d
reviewer: rs-skills (rs-review)
verdict: Fix before merge
critical: 0
high: 2
medium: 3
low: 1
---

# Review: `orders` — orders

> **Fix before merge** · 🔴 0 critical · 🟠 2 high · 🟡 3 medium · 🔵 1 low
> 2026-01-15 · commit `a1b2c3d` · scope: whole crate · previous: [../2025-12-01/orders.md](../2025-12-01/orders.md)
> Day index: [README](README.md) · Agent work log: [work-log.md](work-log.md)

## TL;DR

**What it is.** The order service: an HTTP API over Postgres that prices, charges and stores orders.
**Verdict.** Fix before merge — the checkout path panics on an unknown order id and holds a lock across a payment call.
**Start here.** 1. [P1-1](#p1-1) panic on unknown id · 2. [P1-2](#p1-2) lock across `.await` · 3. [P3-1](#p3-1) split the 900-line `service.rs`
**Good news.** Clean domain types and good doc coverage — keep both.
**Effort.** M (about 4 commits, one per non-empty phase).

<details>
<summary>How to read this report</summary>

- Phases are an order of work: fix P1 first, then P2 … P6; each phase is one commit/MR and leaves the crate green.
- Severity: 🔴 Critical blocks merge · 🟠 High fix before merge · 🟡 Medium design debt · 🔵 Low polish.
- Each finding is a card: what is wrong → why it matters → the fix → a tick-box. Tick it and add the commit when fixed.
- Evidence is at the bottom; the reviewer's steps are in [work-log.md](work-log.md).

</details>

## Fix plan

| Phase | Goal | 🔴 | 🟠 | 🟡 | 🔵 | Effort | Done when |
|---|---|---:|---:|---:|---:|---|---|
| [P1 Safety & correctness](#p1--safety--correctness) | nothing unsafe, panicking or fail-open | 0 | 2 | 0 | 0 | S | every P1 item fixed + a test that proves it |
| [P2 Hygiene & gates](#p2--hygiene--gates) | the tools can be trusted | 0 | 0 | 0 | 0 | — | — |
| [P3 Structure & contracts](#p3--structure--contracts) | feature modules with Design Contracts | 0 | 0 | 2 | 0 | M | `structure-check --strict` + `contract-check` pass |
| [P4 Design & idioms](#p4--design--idioms) | rules followed | 0 | 0 | 1 | 1 | S | no depth-rule finding left open |
| [P5 Tests](#p5--tests) | Invariants proven | 0 | 0 | 0 | 0 | — | — |
| [P6 Release](#p6--release) | ready to ship | 0 | 0 | 0 | 0 | — | — |

**Progress:** 1 of 6 findings fixed

## P1 — Safety & correctness

| ID | Sev | Where | Title | Fixed |
|---|---|---|---|---|
| [P1-1](#p1-1) | 🟠 High | `src/checkout/service.rs:42` | `unwrap()` on DB result panics the worker | ☐ |
| [P1-2](#p1-2) | 🟠 High | `src/checkout/service.rs:118` | `MutexGuard` held across `.await` | ☐ |

<a id="p1-1"></a>
### P1-1 · 🟠 High · `src/checkout/service.rs:42`
- [ ] **Fixed** — commit: _ · test: _
- **Rule:** `err-no-unwrap-prod` · **Area:** error handling

**What's wrong.** `find_order(id).unwrap()` runs with an id that comes straight from the request.
**Why it matters.** An unknown id panics the worker thread; one bad request takes capacity away from everyone.
**Fix.**
```rust
// before
let order = repo.find_order(id).unwrap();
// after
let order = repo.find_order(id).await?.ok_or(CheckoutError::NotFound(id))?;
```
Map `NotFound` to HTTP 404 in `api.rs`, and add a test that posts an unknown id.

<a id="p1-2"></a>
### P1-2 · 🟠 High · `src/checkout/service.rs:118`
- [ ] **Fixed** — commit: _ · test: _
- **Rule:** `anti-lock-across-await` · **Area:** concurrency

**What's wrong.** The `MutexGuard` for `self.cart` is still alive while `payment.charge(...).await` runs.
**Why it matters.** Every other request for that cart blocks on the payment provider's latency; under load this is a deadlock risk.
**Fix.** Copy what you need, drop the guard, then await:
```rust
let amount = { self.cart.lock().expect("cart lock").total() };   // guard dropped here
payment.charge(amount).await?;
```

## P2 — Hygiene & gates

Nothing to do.

## P3 — Structure & contracts

| ID | Sev | Where | Title | Fixed |
|---|---|---|---|---|
| [P3-1](#p3-1) | 🟡 Medium | `src/checkout/service.rs` | 900 lines: pricing, payment and persistence in one file | ☐ |
| [P3-2](#p3-2) | 🟡 Medium | `src/config.rs` | `dry_run: bool` added against the contract's "no bool flags" | ☑ |

<a id="p3-1"></a>
### P3-1 · 🟡 Medium · `src/checkout/service.rs`
- [ ] **Fixed** — commit: _
- **Rule:** rs-structure size ladder (file > 400)

**What's wrong.** One 900-line file mixes pricing rules, payment calls and SQL.
**Why it matters.** Every change in checkout touches this file; the payment code cannot be tested without the SQL.
**Fix.** Promote to a directory (layout below); each move compiles on its own.

<a id="p3-2"></a>
### P3-2 · 🟡 Medium · `src/config.rs`
- [x] **Fixed** — commit: `9f8e7d6` · test: `config_run_mode`
- **Rule:** Design Contract **Extend by**

**What's wrong.** The contract says "no bool flags"; `dry_run: bool` was added.
**Fix.** Modelled as a `RunMode` enum (done).

### Proposed layout

```text
src/checkout/
├── mod.rs        # orchestrator + contract
├── pricing.rs
├── payment.rs
└── repo.rs
```

### Design Contract drift

| File | Contract says | Code does | Change |
|---|---|---|---|
| `src/config.rs` | Extend by: no bool flags | `dry_run: bool` | `RunMode` enum (P3-2, done) |

## P4 — Design & idioms

| ID | Sev | Where | Title | Fixed |
|---|---|---|---|---|
| [P4-1](#p4-1) | 🟡 Medium | `src/checkout/pricing.rs:30` | Clones the cart per line item | ☐ |
| P4-2 | 🔵 Low | `src/api.rs:12` | Capitalised error message (`err-lowercase-msg`) — lower-case it | ☐ |

<a id="p4-1"></a>
### P4-1 · 🟡 Medium · `src/checkout/pricing.rs:30`
- [ ] **Fixed** — commit: _
- **Rule:** `anti-clone-excessive`

**What's wrong.** `cart.clone()` inside the per-line loop.
**Fix.** Take `&Cart`; nothing mutates it.

## P5 — Tests

Nothing to do.

## P6 — Release

Nothing to do.

## Scores

| Area | Score (0–5) | Note |
|---|---:|---|
| Design Contracts | 4 | one drifted contract (fixed) |
| Structure | 3 | `service.rs` 900 lines |
| Correctness & safety | 3 | panic on unknown id |
| Error handling | 4 | |
| Concurrency | 3 | lock across await |
| API & docs | 5 | |
| Tests | 4 | |
| Lints | 5 | |

## Since last review

| Previous ID | Status | Note |
|---|---|---|
| P3-4 | fixed | moved to `orders/repo.rs` |

## Notes

—

## Appendix — Automated checks

_(filled by `review-report.sh`)_
