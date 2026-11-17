---
name: rs-security-review
description: "Use to hunt and triage security and robustness bugs in Rust code — threat-model-first review of crates and services: panic/DoS on untrusted input, resource exhaustion, recursion stack overflow, path traversal, TOCTOU, drop/exit hazards, lock and atomic hazards, serialization integrity, lossy conversions, secrets in memory, supply-chain build-time code — plus the triage protocol (verdicts, gates, severity by threat model). Triggers: security review, security audit, vulnerability, threat model, attacker, untrusted input, denial of service, DoS, panic on input, stack overflow, OOM, path traversal, zip slip, TOCTOU, race condition, deadlock, integer overflow, lossy conversion, zeroize, constant time, timing attack, supply chain, build.rs, proc macro, false positive, triage, is this exploitable, severity, CVSS, RustSec, variant analysis, attack surface, entry points, /rs-security-review"
user-invocable: true
argument-hint: "[path] [--threat REMOTE|LOCAL|BOTH] [--diff <base>]"
---

# Security Review — find what an attacker (or a bad day) can break

> **Layer 4: Delivery** · the security half of review: rs-review owns the report and the phases; this skill owns *what to hunt* and *how to decide a finding is real*
>
> Tools: `${CLAUDE_SKILL_DIR}/scripts/rust-caps.sh` (which passes apply) · `rules/sec-*` (16 bug classes with gates, fixes and false-positive notes) · `guides/triage.md` (verdicts, gates, severity) · memory-safety classes live in **rs-unsafe** · diff-focused review in **rs-review** (`diff-risk.py`)

## Core Question

**Within the stated threat model, can a party who is not supposed to be able to do so make this code crash, stall, leak, forge or escalate — and can I show how?**

Rust removes whole bug classes (use-after-free, data races in safe code) and leaves the rest: availability bugs (panics, aborts, unbounded work), logic and integrity bugs, unsafe boundaries, concurrency hazards beyond races, secrets handling, and the supply chain.

---

## Procedure

1. **Threat model first** (ask if unknown): `REMOTE` (network attacker, no shell) · `LOCAL_UNPRIVILEGED` (shell as an unprivileged user — does it cross a privilege boundary?) · `BOTH`. Write down entry points (network, files, CLI/env, IPC, FFI, deserialised data) and trust boundaries. A finding must cross a boundary the attacker does not already own.
2. **Build context before hunting** — what each function assumes and guarantees; follow the calls (`rs-review/guides/context-building.md`). Spec or GCL present? `rs-review/guides/spec-compliance.md`.
3. **Probe capabilities**: `${CLAUDE_SKILL_DIR}/scripts/rust-caps.sh <crate>` → which passes apply (a pure-safe crate skips memory-safety classes; a service with deserialization adds recursion/exhaustion; filesystem adds path/TOCTOU…). Wide on purpose.
4. **Run the passes** for the switched-on capabilities (table below). Start from rs-review's machine pass and `risk-scan.py` candidates; for a change-set run `diff-risk.py` first.
5. **Triage every candidate** with `guides/triage.md` — verdict → severity. Don't report patterns; report **traced** data flows with a trigger and an impact.
6. **Variants**: one confirmed bug → hunt its siblings (`rs-bugfix/guides/variant-analysis.md`).
7. **Write it up** through rs-review (cards, P-phases, work log); security findings are usually **P1**.

## Passes (class → where the depth lives)

| Pass | Switched on by | Depth |
|---|---|---|
| panic / DoS on input | always | `sec-panic-on-input`, `sec-recursion-depth-dos`, `sec-resource-exhaustion` |
| error handling & cleanup | always | `sec-unflushed-writer`, `sec-drop-panic-and-skip`, rs-error-handling `err-fail-closed` |
| logic & integrity | always | `sec-ord-eq-hash-contract`, `sec-lossy-conversions`, `sec-serialization-integrity` |
| hygiene & supply chain | always | `sec-supply-chain-build-time-code`, rs-release (lints, MSRV, `cargo deny`, `dep-risk.py`) |
| memory safety & unsafe boundary | `has_unsafe` | rs-unsafe checklists (`review-unsafe`, `common-pitfalls`) and its ffi/safety/mem rules |
| FFI | `has_ffi` | rs-unsafe `ffi-*` (panic across `extern "C"`, `CString` lifetime, `repr(C)`, ownership) |
| concurrency | `has_concurrency` | `sec-lock-order-and-reentrancy`, `sec-atomic-check-then-act`, rs-unsafe `safety-05-send-sync` |
| async runtime | `has_async` | rs-concurrency `async-*` (blocking, cancel safety, `select!` bias), rs-async-patterns |
| filesystem | `has_fs_io` | `sec-path-traversal`, `sec-toctou-filesystem` |
| network edge | `has_network` | rs-domain-auth, rs-domain-policy, rs-domain-gateway (fail closed), `sec-pointer-exposure` |
| secrets | `has_secrets` | `sec-zeroize-secrets`, `sec-constant-time-compare`, rs-observability `obs-no-sensitive-data` |

## Rationalizations to reject

| Excuse | Reality |
|---|---|
| "It's safe Rust, so it can't be exploited." | Safe Rust still aborts on stack overflow/OOM, panics on input, deadlocks, leaks timing and mis-parses. Availability is a security property. |
| "The compiler would have caught it." | The compiler proves memory and thread safety of safe code only — not panic reachability, resource limits, logic, or `unsafe`. |
| "`unwrap()` is fine, it's documented infallible." | `// SAFETY:` documents unsafe operations; an `unwrap` is a claim the input can break. File it low if the claim is real, high if input reaches it. |
| "No `unsafe`, skip the review." | Always-on passes (DoS, logic, supply chain, secrets) don't need `unsafe`. |
| "This pattern looks dangerous, so it's a vulnerability." | Pattern recognition is not analysis. Trace source → validation → sink. |
| "Similar code was vulnerable elsewhere." | Each site has its own validation and callers. Verify this instance. |
| "Everything is critical." | Severity depends on the threat model; reviewers over-rate. Prove impact or downgrade. |
| "I found nothing, so it's secure." | State coverage honestly: which passes ran, what was out of scope, what was not read. |

## Output discipline

Findings carry: id, `file:line`, bug class (`sec-*` rule), the **trigger** (who sends what), the **impact** in the threat model, the traced path, the fix. Hardening gaps without an attacker path (missing `forbid(unsafe_code)`, no `[lints]`, no MSRV) are **Low**, not "false positives". Report what you could *not* verify under *Coverage*.

## Trace

| Direction | Go to |
|---|---|
| ↑ Up | rs-domain-gateway / -web / -auth / -policy (what the code defends), rs-domain-distributed (network assumptions) |
| ↓ Down | rs-unsafe, rs-concurrency, rs-error-handling, rs-type-driven (make the invalid state unrepresentable) |
| → Delivery | rs-review (report), rs-fuzzing (find what review missed), rs-bugfix (fix + variants + proof), rs-release (supply chain gates) |

<!-- rules-index:start -->
## Rules (16)

Depth rules under `rules/` — open the one the question needs, not all of them.

**sec-**

- [`sec-atomic-check-then-act`](rules/sec-atomic-check-then-act.md) — Atomic operations are atomic one at a time — compound invariants need one operation or a lock
- [`sec-constant-time-compare`](rules/sec-constant-time-compare.md) — Compare secrets and MACs in constant time
- [`sec-drop-panic-and-skip`](rules/sec-drop-panic-and-skip.md) — Drop must not panic, and cleanup must not depend on a Drop that process::exit, mem::forget or abort will skip
- [`sec-lock-order-and-reentrancy`](rules/sec-lock-order-and-reentrancy.md) — Lock scopes, ordering and re-entrancy: double locks, ABBA, Condvar, call_once, signal handlers
- [`sec-lossy-conversions`](rules/sec-lossy-conversions.md) — Lossy casts and lossy string conversions turn two different inputs into the same value
- [`sec-ord-eq-hash-contract`](rules/sec-ord-eq-hash-contract.md) — Eq, Ord and Hash must agree — and keys must not change while they are in a collection
- [`sec-panic-on-input`](rules/sec-panic-on-input.md) — A panic reachable from input is a denial of service: no unwrap/index/slice/assert/overflow on untrusted data
- [`sec-path-traversal`](rules/sec-path-traversal.md) — Join untrusted path components safely: Path::join with an absolute path replaces the base
- [`sec-pointer-exposure`](rules/sec-pointer-exposure.md) — Don't print addresses: pointers in logs, errors or responses defeat ASLR
- [`sec-recursion-depth-dos`](rules/sec-recursion-depth-dos.md) — Bound recursion over attacker-shaped data: deserialization, formatting, traversal, Drop
- [`sec-resource-exhaustion`](rules/sec-resource-exhaustion.md) — Never size memory, time or connections from an unvalidated number the peer controls
- [`sec-serialization-integrity`](rules/sec-serialization-integrity.md) — Serialization must be total, deterministic and agree with its own length claims
- [`sec-supply-chain-build-time-code`](rules/sec-supply-chain-build-time-code.md) — Dependencies run code on your machine at build time: build.rs, proc-macros, git deps, patches
- [`sec-toctou-filesystem`](rules/sec-toctou-filesystem.md) — Check-then-use on the filesystem is a race: open first, then inspect the handle
- [`sec-unflushed-writer`](rules/sec-unflushed-writer.md) — Write errors vanish unless you flush: BufWriter, File and discarded Results
- [`sec-zeroize-secrets`](rules/sec-zeroize-secrets.md) — Wipe secrets from memory with a mechanism the optimizer cannot remove — and know its limits

<!-- rules-index:end -->

---

## Related Skills

| When | See |
|---|---|
| The report, phases, work log | rs-review |
| Memory-safety classes and `unsafe` rules | rs-unsafe |
| Fuzzing the parsers and decoders you reviewed | rs-fuzzing |
| Fixing a confirmed bug and its variants | rs-bugfix |
| Dependency and CI risk | rs-release |
