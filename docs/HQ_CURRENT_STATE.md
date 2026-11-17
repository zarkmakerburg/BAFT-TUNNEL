# BAFT-TUNNEL — HQ Current State

> **Snapshot date:** 2026-11-17  
> **Purpose:** compact recovery index for long threads and new chats. This file is a replaceable snapshot, not an append-only history. Verify every mutable fact against GitHub before acting. Historical decisions and evidence remain in [Issue #2](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/2).

## Binding and guardrails

- **Bound repository:** [zarkmakerburg/BAFT-TUNNEL](https://github.com/zarkmakerburg/BAFT-TUNNEL) only.
- **Default branch:** `main`.
- **Canonical board:** [Issue #2](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/2), append-only.
- **Roadmap:** [docs/PRODUCT_ROADMAP.md](PRODUCT_ROADMAP.md); detailed queue: [docs/REMAINING_WORK_PLAN.md](REMAINING_WORK_PLAN.md).
- **Current Stable:** `v0.2.1` (per latest roadmap/board checkpoint).
- **Non-negotiable constraints:** Rust core; no Go in core; no `CAP_NET_ADMIN`; no firewall/route/sysctl changes; do not advertise unaccepted Reverse/Noise capabilities; preserve all worktrees and branches; no destructive reset or force-push.
- **Permission rule:** permissions do not transfer merely because this file exists or `HQ ON` is active. Recheck task-specific/ chat-scoped authorization and the project's merge, release, deployment and LAB gates before acting.

## Live repository snapshot

- **main HEAD at last verification:** `c3e2bda8cc4b598c410a665f3c18fb6cc2d944c2` — PR #325, documentation update for advisor-panel UX captures.
- **main exact-SHA checks:** 9 Check Runs observed; 7 completed successfully and 2 were still `in_progress` (`rust`, `ubuntu-2204-baseline`). This is **not** an all-green main result at this checkpoint.
- **Open PRs at last verification:**

| PR | Head SHA | State | Verified note |
|---|---|---|---|
| [#323](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/323) | `d1897c5cb3a6af89868ce57255136a6087d908d3` | Draft / Open | 11 Check Runs green on this head; no review recorded. Noise-TCP Direct rotation harness only; not TCP-TLS evidence. |
| [#324](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/324) | `2ca82c8b86d75ffe4db9aae9fc8e1025635e1f7d` | Draft / Open | 11 Check Runs green on this head; no review recorded. Skills/governance + roadmap documentation; parallel, non-blocking track. |
| [#326](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/326) | `ab8095d59cf77bf16bd13e34a81acf77adc463c5` | Draft / Open | F1 pure rules + ADR-0045 accepted with conditions; mergeability/check state not yet verified. |
| [#327](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/327) | `dccd060b926648ea29b26a80e1c3b58e255a2f4a` | Draft / Open | This checkpoint implementation; documentation-only. 10 exact-head Check Runs were in progress at PR creation; no pass claimed. |
| [#171](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/171) | `e64ea2fca5b3dd17b709ecf9c92a2cf933da32cb` | Draft / Open | Central Fleet architecture only; deferred after 0.4.0. |

PR base refs and statuses can change; re-fetch them before action. PR #327 was opened from the recorded main SHA and its initial compare showed 3 commits ahead / 0 behind; its mergeability was subsequently confirmed true, with merge blocked while checks run. PR #169 is the separate HQ playbook and is not BAFT-TUNNEL product scope.

## Active task — P0-A raw TCP vs TLS aged-flow diagnosis

**Status: IN PROGRESS / NOT VERIFIED.**

- Diagnostic workflow: [run 44101542573](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/44101542573), head SHA `65b50dea5562a15f8299a13afcec64d34a93bf58`, job `134102574533`; last observed step `Run control transfers (tls and tcp, both directions)` was `in_progress`.
- Queued workflow: [run 44101769313](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/44101769313), same head SHA; status `pending`, no job had started.
- The workflow is explicitly **non-tunnel network control**; it runs a temporary Python transfer endpoint and does not execute BAFT-TUNNEL. It can perform a full TLS/TCP up/down bulk matrix when `AGES` is empty, or a single selected `AGE_MODE` aged-flow series when `AGES` is supplied.
- The dispatch inputs for these two runs were not available in the inspected API response. Job-log retrieval returned `BlobNotFound`; no artifacts were listed. Therefore the actual mode/age matrix and any transfer outcome are **UNKNOWN**. Do not call either run PASS or FAIL yet.
- Do not dispatch a duplicate run while the first holds the shared LAB concurrency group. Fetch status/jobs again; once finished, retrieve and validate client + server output, parameters, per-attempt bytes/timing/errors, and cleanup evidence. If logs remain inaccessible, record the precise evidence gap and use a supported alternate retrieval path; do not infer results.

### P0 order

1. **P0-A:** complete raw TCP vs TLS aged-flow diagnosis on the two registered LAB hosts; required comparison is ages `0,15,20,25,30,60` seconds, 2 MiB, 3 iterations, 5-second keepalive, with client/server evidence. Keep the network-control result separate from product evidence.
2. **P0-B:** PR #323 exact-head CI → independent review → owner-authorized Noise-TCP Direct LAB. Its result cannot prove TCP-TLS mitigation.
3. **P0-C:** only if P0-A supports rotation as a TCP-TLS mitigation, design and prove a separate TCP-TLS/PKI-specific rotation path.
4. **GOV-1:** PR #324 review/CI is parallel and non-blocking for product P0.

## Release and maturity notes

- `v0.2.1` remains the latest Stable according to the latest roadmap/board checkpoint.
- `v0.2.2-rc.3` has recorded LAB evidence, but Stable tagging is an owner/admin-machine dependency; do not create a Stable tag without satisfying release/signing policy.
- `v0.3.0-rc.1` is not accepted because aged-flow upgrade soak remains unresolved.
- A CI-green result does not change release maturity or imply production readiness.

## Next action

Recheck runs **44101542573** and **44101769313** and the shared LAB concurrency state. Do not dispatch another LAB workflow until the first run releases the lock and its result/log availability is understood. Then classify the evidence strictly as PASS, FAIL or UNKNOWN and update this file plus Issue #2.

## Resume checklist

1. Read [AGENTS.md](../AGENTS.md), [PROJECT_PROFILE.md](../PROJECT_PROFILE.md), [HQ_USAGE.md](HQ_USAGE.md), [HQ_CHECKPOINT_PROTOCOL.md](HQ_CHECKPOINT_PROTOCOL.md), and this file.
2. Read the newest comments on [Issue #2](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/2) (paginate to the newest page).
3. Verify live main SHA, open PR heads/reviews/checks, active runs/jobs/logs/artifacts and Roadmap priority.
4. Confirm applicable authorization and safety gates.
5. Continue from **Next action**; do not repeat completed work or trust stale snapshot fields without live verification.
