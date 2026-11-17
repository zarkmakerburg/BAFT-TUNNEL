# Triage — is it real, who can trigger it, how bad

> **rs-security-review · guide** — rs-skills original.

Run this on **every** candidate (scanner hit, reviewer hunch, external report). It is cheap, and it is what keeps a report credible.

## Step 0 — Restate the claim

Write it in one breath: *"an {attacker} sends {input} to {entry point}; the code at {file:line} {does X}; the result is {impact}."* If you cannot, the finding is not ready — half of false positives dissolve here.

## Step 1 — Verdict

| Verdict | Meaning |
|---|---|
| `TRUE_POSITIVE` | real, reachable by the defined attacker, impact shown |
| `LIKELY_TP` | real; reachability plausible but not fully traced (say what is missing) |
| `LIKELY_FP` | bug-shaped, probably unreachable or mitigated |
| `FALSE_POSITIVE` | the premise is wrong (misread code, guarded upstream) |
| `OUT_OF_SCOPE` | real, but needs capabilities outside the threat model |
| `HARDENING` | valid latent weakness with no attacker path today → report as **Low** |

When torn between `LIKELY_TP` and `LIKELY_FP`, prefer `LIKELY_TP` and list what would settle it. Hardening gaps are **not** false positives: a missing lint table is a true, low-severity observation.

## Step 2 — The seven dismissal tests

A candidate that fails one is dismissed (write which, and why); one that passes all seven is escalated.

| # | Test | Dismiss when |
|---|---|---|
| 1 | **Threat model** | the attacker already has the power the bug would grant (e.g. local shell needed to reach a bug that only gives a local shell) |
| 2 | **Exploit from the heavens** | triggering needs conditions no attacker can arrange (impossible state, root already, contradiction in the data flow) |
| 3 | **Outside of usage** | no real caller reaches the path; for a library, check downstream usage before dismissing |
| 4 | **Standard behaviour** | the code correctly implements a spec that permits it (unless you voluntarily promise more) |
| 5 | **Documented behaviour** | documented, with the caveat — then misuse downstream is the downstream's bug |
| 6 | **Cure worse than the disease** | the fix would break more than the issue costs; downgrade or accept with a note |
| 7 | **Report is not evidence** | a CVE number or a scanner label proves nothing; judge the technical description alone (and absence of a report proves nothing either) |

## Step 3 — Gates for a true positive

| Gate | Passes when |
|---|---|
| Process | every step has documented evidence (file:line, the trace) |
| Reachability | an attacker-controlled path from entry point to sink is shown, with the validation in between read |
| Real impact | crash/DoS, disclosure, forgery, escalation, data loss — not "robustness in an impossible case" |
| Reproduction | a failing test, PoC input, or fuzz crash shows trigger and impact (write it as a regression test — rs-bugfix) |
| Bounds | for arithmetic/size claims, the math shows the condition is possible (a prior check may make it impossible) |
| Environment | mitigations (sandbox, limits, framework caps, `panic = "unwind"` + catch layer) do not neutralise it |

A candidate failing a gate is not reported as a vulnerability; record the reason. Check **chains**: findings that fail alone may combine.

## Step 4 — Severity (only for survivors) — depends on the threat model

| | REMOTE | LOCAL_UNPRIVILEGED |
|---|---|---|
| **Critical** | RCE or memory corruption reachable from the network; authentication/authorisation bypass; sandbox escape | privilege escalation to root/kernel; container escape |
| **High** | remote DoS via reachable panic/abort/unbounded resource on attacker input; remote disclosure of memory or secrets; forged signatures/tokens | access to other users' data; arbitrary file read/write across a privilege boundary |
| **Medium** | DoS needing narrow conditions (race, large input); discarded error degrading correctness on a hot path; cancellation-unsafe sequence corrupting shared state | local DoS; disclosure of system data; limited boundary crossing |
| **Low** | theoretical UB, hardening gaps (no `[lints]`, no MSRV, missing SAFETY comment on correct code), negligible impact | boundary crossing with minimal impact |

Adjust one level: **down** if a real invariant guards an `unsafe` site, a race must be won, or a non-default feature is needed; **up** if it touches authentication/crypto/deserialization of attacker bytes or sits in a widely used public API. A panic inside `Drop` is at least Medium (double panic aborts). Local-only triggers under `REMOTE` are `OUT_OF_SCOPE`, and same-user bugs crossing no boundary under `LOCAL_UNPRIVILEGED` are `LIKELY_FP` — they are not "Low".

Keep it coarse: Critical/High/Medium/Low is enough. (CVSS vectors are for CVE publication, not for fixing your own code.)

## Step 5 — Merge duplicates

Two findings are the same defect if fixing one fixes the other (same root cause, different symptom or class label). Merge them into one finding listing every manifestation; the verdict is for the group (real under any framing ⇒ real).

## False-positive patterns to check before reporting

1. Validation upstream (trace backwards to the entry point, read the whole chain).
2. Conditions that make the path unreachable or the arithmetic impossible (`len >= 12` guard before `len - 4`).
3. Defensive assertions mistaken for vulnerabilities.
4. "Attacker controlled" data that is actually a constant, config, or output of a trusted component.
5. API contracts with built-in bounds (a function that cannot write past the buffer).
6. TOCTOU claims where nothing can change the value between check and use.
7. Concurrency claims where concurrent access is not actually possible (single-threaded VM, `!Send` type, one owner).
8. Pattern match without context ("`unwrap` ⇒ bug") — the value may be proven `Some`.
9. Impact that is operational robustness, not security (an internal tool crashing on its own bad config).
