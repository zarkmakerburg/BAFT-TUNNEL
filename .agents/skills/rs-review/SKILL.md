---
name: rs-review
description: "Use to review Rust code and write a Markdown review report. Triggers: review, code review, review this crate, review my changes, review the diff, PR review, MR review, audit this code, is this production ready, quality report, review report, /rs-review"
user-invocable: true
argument-hint: "[path | --diff] [--scope <name>]"
---

# Rust Code Review → Markdown Report

> **Layer 4: Delivery** · every review ends in a saved report, not a chat message
>
> Reports are for **developers**: one folder per day, a TL;DR with three "start here" links, findings as readable cards with tick-boxes, an overview map for multi-crate scopes, a generated index, and an agent **work log** so the review can be followed and challenged.

## Core Question

**Can this code ship — and if not, exactly what must change, where, and why?**

## Procedure

1. **Scope.** `path` (default `.`) or `--diff <base>` (the change set `base…HEAD`: runs `${CLAUDE_SKILL_DIR}/scripts/diff-review.py` — risk triage, removed checks and their history, blast radius, test gaps, reading order; method in `guides/differential-review.md`). Name the scope (`crate`, `orders`, `mr-142`). **Understand before judging:** for unfamiliar code build context first (`guides/context-building.md`); when a spec, RFC or `gcl.yaml` exists, check code against it (`guides/spec-compliance.md`).
2. **Machine pass.** Run `${CLAUDE_PLUGIN_ROOT}/skills/rs-review/scripts/review-report.sh <crate-dir> --scope <name> [--files <list>]`. It creates today's folder `<docs>/rs-reviews/YYYY-MM-DD/` (docs folder found by `${CLAUDE_SKILL_DIR}/scripts/review-dir.sh`, see **Where reports go**), writes `<scope>.md` from `templates/report.md`, fills the **Automated checks** appendix (contracts, structure, fmt, clippy, tests, docs, release gate, **ports & adapters**, repository hygiene) and the **Risk scan** (`${CLAUDE_SKILL_DIR}/scripts/risk-scan.py`: fail-open shapes, discarded write results, casts, PII in logs, reflected content, hand-rolled standards, stream properties read but never written, config options the code never mentions), links the previous report of the same scope, appends the first row to `work-log.md` and refreshes the README indexes. For crates built for a non-host target (`.cargo/config.toml` `build.target`, or a `cdylib` whose Makefile targets `wasm32-wasip1`, e.g. Flex/proxy-wasm policies) it also runs the **release build for that target** — host `cargo test` passing does not prove the shipped artefact links. Override with `--target <triple>`. The builds leave artefacts behind: finish with rs-cleanup (the script warns when a budget is exceeded).
3. **Judgement pass** — for every file in scope, in this order, adding findings to the report:
   1. **Design Contract** — read the header; does the code honour **Invariants**, **Extend by**, **Avoid**? A mismatch is a finding (say which side should change). Skill(rs-design-contract)
   2. **Structure** — right feature module? split ladder respected? visibility minimal? Skill(rs-structure)
   3. **Anti-patterns** — Skill(rs-anti-pattern) checklist and its `rules/anti-*`.
   4. **Depth rules** — for each area touched, open the 2–3 relevant rules (rs-ownership, rs-error-handling, rs-concurrency, rs-api-design, rs-performance, rs-unsafe, rs-testing, rs-observability …) and cite them by id.
   5. **Patterns** — is a pattern from rs-patterns misapplied or missing?
   6. **Security & contract pass** — mandatory for anything that filters, authorises, inspects or audits traffic. Start from the report's *Risk scan* candidates, then walk the checklist below. In the first Flex review this pass did not exist and 14 High/Medium issues were found only later, during the restructure.
   7. **Reuse & decoupling** — (a) every *reinvention candidate* in the Risk scan: run the rs-reuse-first gate (`${CLAUDE_PLUGIN_ROOT}/skills/rs-reuse-first/scripts/sdk-scout.sh`); an official or de-facto SDK exists and none is used → finding (High; P1 when it is security-critical: JWT/OAuth/crypto by hand), unless a hard constraint is recorded (wasm sandbox, `no_std`) — then check the decision record and conformance fixtures; (b) the *ports & adapters* row and `port-check.py` output: vendor crates named outside `adapters/`, SQL in business code, vendor derives on domain types, errors leaking → P3 findings; run the **swap drill** from rs-ports-adapters for the main dependency (database, cache, queue, SDK); (c) is the dependency boundary visible in the Design Contract?
   8. **Housekeeping of the work itself** — the *Repository hygiene* section (`git-check.sh audit`: secrets, tracked artefacts, message style, attribution policy — rs-git) and disk (`target/`, images — rs-cleanup) go to P6; never fix history or delete data as part of a review without the owner's yes.
   9. **Security hunt** — for anything that parses untrusted input, filters, authorises or runs as a service: Skill(rs-security-review) — threat model first, `rust-caps.sh` capability probe, the `sec-*` bug classes, and its triage protocol (verdict → severity). In the report these are P1 unless proven otherwise.
4. **Log as you go.** After every step of the judgement pass append a row: `${CLAUDE_SKILL_DIR}/scripts/review-log.sh "<step>" "<what you read or ran>" "<result>" "<evidence>"` (scope chosen · each file group read · each tool run · each decision · each fix commit). One line each; this is how the developer follows your work.
5. **Write the report for a reader in a hurry.**
   - Fill the **TL;DR** (what it is · verdict · three "start here" links · what is good · effort) and the front matter (`verdict`, `critical`, `high`, `medium`, `low`) — the indexes read it.
   - Put every finding in exactly one phase, ordered by severity inside the phase. Per phase: the one-line-per-finding table, then a **card** for each Critical/High/Medium (*what's wrong · why it matters · fix with a before/after snippet · tick-box*). Low findings may stay as table lines. No six-column tables of prose.
   - Fill the **Fix plan** counts, the P3 proposed layout and contract drift, scores and verdict. Remove template comments.
6. **Fix loop discipline** — fixing review findings is a loop: pick the phase, fix, re-run the machine pass, re-review the **diff of your fix** (`--diff`), repeat. Keep a **ledger** (the cards' tick-boxes + work log), cap rounds (e.g. 5), stop and escalate to the user when fixes stop converging (the same finding reappears, or a fix creates a worse one), and keep a **scope guard**: a fix may only touch files the finding names (plus its tests) — anything else is a new finding.
   **Progress** — while phases are being fixed: tick each card's box and add the commit, keep the overview's Progress table current, add findings discovered during a fix to their phase marked *(found during Pn)*, log each fix commit, then run `${CLAUDE_SKILL_DIR}/scripts/review-index.py <rs-reviews dir>` to recount the day index.
7. **Follow-up** — if a previous report exists, mark each of its findings `fixed` / `open` / `wontfix` in **Since last review**.
8. **Reply** in chat with only: verdict, counts per phase and severity, top 3 findings, the report path and the day index path.

## Security & contract checklist (step 3.6)

| Check | How | Rule |
|---|---|---|
| **Every error path of a filter** — parse, I/O, host write, regex engine, missing config | list each early `return` / `?` / `unwrap_or` / `let _ =`; ask "is data forwarded unchecked if this runs?" | `err-fail-closed` |
| **Pass-through lists** — methods/routes exempt from checks | for each exempt item: can it read or change protected data directly? | `err-fail-closed` |
| **What the caller gets back** — error bodies, headers, HOLD texts | no blocked/matched content, no upstream body, no internals | `obs-no-reflect-untrusted` |
| **Logs** — every level, especially INFO/trace | no tokens, secrets, e-mails, full claim sets; ids are fine | `obs-no-sensitive-data` |
| **Cross-component contract** — properties, headers, context keys written by one component and read by another | match writer and reader key names *and* who writes on deny/hold paths; the scan lists keys read but never written | — |
| **Config surface** — every schema property read, every enum value implemented | the scan lists unmentioned options; never fix by narrowing the schema | `api-config-is-contract` |
| **Trust labels** — data marked `validated` / `verified` | confirm it really came from the verifier, in the order the components actually run | — |
| **Untrusted values in URLs, paths, queries** | percent-encode / validate before substitution | `api-parse-dont-validate` |
| **Privacy settings** (pseudonymise, mask, redact) | applied to the value that is **emitted**, on every source/branch | `obs-no-sensitive-data` |
| **Keys and digests** — caches, idempotency, dedup | collision-resistant (SHA-256), stable across versions | `type-newtype-ids` |

Every confirmed item is a finding (usually P1); every dismissed scan candidate needs no row.

## Rationalizations to reject (reviewer)

| Excuse | Why it fails | Do instead |
|---|---|---|
| "Small change, quick look." | Heartbleed was two lines; refactors break invariants. | Classify by **risk**, not size (`diff-review.py`). |
| "I know this codebase." | Familiarity breeds blind spots. | Build explicit context (`guides/context-building.md`). |
| "The tests pass." | Passing tests prove what they assert; a removed check often has no test. | Check what the removed/changed code *protected*; list untested changed functions. |
| "The scanner found nothing." | Scanners nominate patterns; they don't know your invariants. | Say which passes ran and what was not read (Coverage). |
| "This looks bad, so it's a finding." | Pattern ≠ analysis. | Trace source → validation → sink; run the triage (`rs-security-review/guides/triage.md`). |
| "I'll summarise verbally." | No artefact, no review. | Report file + work log, always. |
| "Zero findings — skip the report." | The empty report states coverage and limits; the indexes depend on it. | Write it. |

## Where reports go

Never create a bare `reviews/` folder in a random place. Reports belong with the project's documentation, **one folder per day**:

```
<docs>/rs-reviews/
  README.md                      ← all days, newest first (generated)
  2026-11-11/
    README.md                    ← the day's index: reports, verdicts, counts, fix progress (generated)
    flex-overview.md             ← multi-crate map (templates/overview.md)
    mcp-audit-logger.md          ← one report per crate (templates/report.md)
    mcp-response-inspector.md
    work-log.md                  ← what the agent did, step by step (review-log.sh)
  2026-11-10/ …
```

1. Search upward from the crate to the repository root for an existing docs folder, preferring internal docs: `docs/private` → `docs/internal` → `docs`. The nearest one wins.
2. Put reports in **`rs-reviews/`** inside it, then the day folder `YYYY-MM-DD/` (`RS_REVIEW_DATE` overrides the date). Several reviews on one day share the folder; a repeated scope gets `-2`, `-3`.
3. Only if the project has no docs folder: `docs/rs-reviews/` at the workspace root.
4. Multi-crate scopes: all crate reports and the overview go in the same day folder.
5. `${CLAUDE_SKILL_DIR}/scripts/review-dir.sh [dir]` prints the folder; `review-report.sh` uses it unless `--out` is given. Older flat files (`YYYY-MM-DD-<scope>.md`) are still found as the previous report. If the chosen docs folder is git-ignored (e.g. `docs/private/`), say so in the reply — the reports stay local.

## Phases — where each finding goes

The report is a fix plan: phases run in order, each is one commit/MR, each has an exit criterion. IDs are `P<phase>-<n>`.

| Phase | Put here | Typical rules |
|---|---|---|
| **P1 Safety & correctness** | UB, unsound `unsafe`, data race, panic on input, fail-open security path, secret in logs, violated Invariant, wrong result | `unsafe-*`, `err-no-unwrap-prod` (when reachable from input), `obs-no-sensitive-data`, contract Invariants |
| **P2 Hygiene & gates** | rustfmt drift, no `[lints]`, clippy warnings, rustdoc errors, failing / environment-bound tests, missing CI gate | `lint-*`, `lint-rustfmt-check` |
| **P3 Structure & contracts** | split ladder (fn > 60, file > 400), flat `src/`, `utils.rs`/`helpers.rs`, duplication across modules or crates, visibility, missing/drifted Design Contracts | `proj-*`, rs-structure ladder, rs-design-contract |
| **P4 Design & idioms** | swallowed errors, guarded `unwrap`, stringly types, too many args, needless clones, casts, log structure, misapplied patterns | `err-*`, `type-*`, `api-*`, `own-*`, `num-*`, `perf-*`, `mem-*`, `obs-*`, `anti-*` |
| **P5 Tests** | an Invariant or critical path without a test, poor test layout | `test-*` |
| **P6 Release** | MSRV, edition, semver, CHANGELOG, release profile, supply chain | `proj-msrv-declare`, rs-release |

Tie-break: a finding that is both a security hole and a design smell goes to **P1** (fix the hole first, refactor later). Severity and phase are independent — P3 can hold a High, P1 can hold a Medium.

## Multi-crate scope (workspace or folder of crates)

Run the machine pass per crate with the **same** `--out` (the folder `review-dir.sh` gives for the workspace root) → one report per crate in today's folder **plus** `<scope>-overview.md` from `templates/overview.md`: TL;DR, per-crate matrix, scores, the same P1–P6 sections listing each crate's top items (linking to the crate report) and **cross-crate** cards (`X-<n>`: duplication, shared crate, workspace lints). The overview is the map; the crate reports hold the cards.

## Severity

| Severity | Meaning | Examples |
|---|---|---|
| **Critical** | wrong or unsafe; blocks merge | UB or unsound `unsafe`, data race, panic on user input in a service, secret in logs, contract **Invariant** violated, failing tests |
| **High** | will hurt in production soon | `unwrap()` in prod path, lock held across `.await`, unbounded channel/allocation, error swallowed, public API break without major bump, missing contract on a new file |
| **Medium** | maintainability / design debt | flat `src/` or `utils.rs`, function > 60 lines, file > 400 lines, needless clone, stringly-typed API, missing `# Errors` docs, missing test for an Invariant |
| **Low** | polish | naming, minor clippy pedantic, doc wording |

**Verdict:** any Critical → **Block** · any High → **Fix before merge** · otherwise → **Ship** (Medium/Low tracked).

## Finding format

Each finding has an id (`P<phase>-<n>`; `X-<n>` across crates), a severity, a `file:line`, and a **cited rule or contract line** (`err-no-unwrap-prod`, `Extend by`, `rs-structure split ladder`). In a card: **What's wrong** (one or two sentences, concrete) · **Why it matters** (the consequence, not the rule's name) · **Fix** (the change, with a before/after snippet when code helps) · tick-box with commit and test. No finding without a citation. No vague advice ("consider improving error handling").

## Scores (0–5 per area, in the report)

Contracts · Structure · Correctness & safety · Error handling · Concurrency · API & docs · Tests · Lints. 5 = nothing to report; subtract per finding (Critical −3, High −2, Medium −1, Low −0).

## Related Skills

| When | See |
|---|---|
| Release gate | rs-release (`/rs-release-check`) |
| Unsafe deep dive | rs-unsafe (`/unsafe-review`) |
| Restructure suggested | rs-structure (`/rs-restructure`) |

Guides: `guides/context-building.md` · `guides/spec-compliance.md` · `guides/differential-review.md`. Templates: `templates/report.md` (crate) · `templates/overview.md` (multi-crate) · `templates/work-log.md` · Example: `templates/example-report.md` · Scripts: `review-report.sh` (machine pass) · `review-log.sh` (work log) · `review-index.py` (README indexes) · `review-dir.sh` · `risk-scan.py`.
