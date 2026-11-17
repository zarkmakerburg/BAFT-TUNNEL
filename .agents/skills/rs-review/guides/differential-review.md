# Differential review — reviewing a change, not a codebase

> **rs-review · guide** — host-neutral (PR, MR, branch, commit range, patch).

Use when the unit of review is a **change set**: a merge request, a release diff, a fix commit. Run `scripts/diff-review.py --base <ref>` (or `review-report.sh --diff <ref>`); it nominates, you decide.

## Why a change is not "just a diff"

A refactor breaks invariants; a two-line change can disable a check (the classic is a removed validation that an earlier security fix added). Classify by **risk, not size**.

## Phases

| Phase | Do | Tool |
|---|---|---|
| 0 Triage | list files, classify HIGH/MEDIUM/LOW (auth, crypto, policy, unsafe/FFI, process spawn, deserialisation of input, dependency/`build.rs` changes ⇒ HIGH) | `diff-review.py` table |
| 1 Changed code | for each hunk: BEFORE / AFTER / behavioural change / security implication | read both versions |
| 2 History of removed code | was a removed check added by a fix? `git log -S'<snippet>' -- <file>`, `git blame`; a re-added snippet that was previously removed for security is a **regression** | `diff-review.py` red flags + `git` |
| 3 Blast radius | how many callers does a changed function have? ≥ 20 = wide; changed public API = semver (rs-api-design) | `diff-review.py` |
| 4 Test coverage | which changed functions are mentioned by no test; source changed with no test change ⇒ raise severity one level until covered | `diff-review.py` + read assertions |
| 5 Adversarial | for each HIGH change: attacker, entry point, trigger, impact — concrete steps, not generic worries (rs-security-review `guides/triage.md`) | by hand |
| 6 Report | cards in the day report with commit SHAs and line numbers; state coverage limits honestly | rs-review |

## Depth by size

Small (<20 files): deep, read callees. Medium (20–200): HIGH files fully, one hop out. Large (200+): HIGH paths only and say what was not read.

## Reading order

`diff-review.py` prints a reading order — definitions (model/ports/errors) → implementations → adapters → wiring/config → tests, grouped by feature module — so the reviewer reads the change as a story, not alphabetically. Post the same order in the MR description (rs-git `change-summary.md`).

## Red flags (escalate even in a quick pass)

Removed code from "fix"/"security" commits · validation removed without replacement · a deny path changed to continue · new `unwrap`/cast/allocation on input · new `unsafe`/process spawn · new dependency with `build.rs` · high blast radius + HIGH risk.

## Don't use for

Greenfield code (no baseline), documentation-only or formatting-only changes, or when the requester explicitly accepts a quick summary (state the risk).
