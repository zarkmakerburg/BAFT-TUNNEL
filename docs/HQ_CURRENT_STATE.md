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

## Automatic checkpoint policy

- A lightweight in-thread ledger is refreshed after every user message and every tool operation: intent, decisions/constraints, task state, exact refs, observed result, valid approvals, blocker and next safe action.
- A durable checkpoint updates this file and appends Issue #2 after material decisions, instruction completion/failure/blockage, repository/PR/CI/LAB state changes, new evidence, changed priorities/blockers, handoffs/task switches, and before long-running operations (then again after outcome).
- A durable checkpoint is also due every 3 substantive user turns or 10 meaningful tool operations, whichever comes first.
- No-op conversational turns do not create empty Issue #2 comments. Turn-level tracking still applies.
- Verify both snapshot and Issue #2 read-back before calling a checkpoint complete. A failed write is **CHECKPOINT INCOMPLETE**.
- This is best-effort while the chat and tools are available; it cannot run after abrupt client termination or in the background. Checkpoint during work, not at an assumed context limit.
- A checkpoint is not an approval and does not transfer task-specific permissions. Full rules: [HQ_CHECKPOINT_PROTOCOL.md](HQ_CHECKPOINT_PROTOCOL.md).

## HQ RESUME behavior

`HQ RESUME` is one combined command, not a two-step instruction. If HQ is OFF in a new chat, it activates HQ within that same command, binds only this project, reads the profile/protocol/snapshot and newest Issue #2 history, verifies mutable facts live, and immediately continues the first safe, authorized, unfinished action from the latest checkpoint. If HQ is already ON, it skips activation and resumes directly. The user-facing response must report continuation progress (action resumed/performed, evidence/result, blockers and next action), not an activation-only announcement. If no action is safe, state the exact gate and the nearest safe action. It must not stop after reading documents or displaying status.

## Live repository snapshot

- **main HEAD at last verification:** `c3e2bda8cc4b598c410a665f3c18fb6cc2d944c2` — PR #325, documentation update for advisor-panel UX captures.
- **main exact-SHA checks:** 11 Check Runs observed on `c3e2bda8cc4b598c410a665f3c18fb6cc2d944c2`; all 11 completed with `success` at this verification. This describes CI only, not LAB acceptance.
- **Open PRs at last verification:**

| PR | Head SHA | State | Verified note |
|---|---|---|---|
| [#323](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/323) | `d1897c5cb3a6af89868ce57255136a6087d908d3` | Draft / Open | 11 Check Runs green on this head; no review recorded. Noise-TCP Direct rotation harness only; not TCP-TLS evidence. |
| [#324](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/324) | `2ca82c8b86d75ffe4db9aae9fc8e1025635e1f7d` | Draft / Open | 11 Check Runs green on this head; no review recorded. Skills/governance + roadmap documentation; parallel, non-blocking track. |
| [#326](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/326) | `ab8095d59cf77bf16bd13e34a81acf77adc463c5` | Draft / Open | F1 pure rules + ADR-0045 accepted with conditions; mergeability/check state not yet verified. |
| [#327](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/327) | `8526508820f5ee9db4d9f5aac3822ef3d9e6063c` | Draft / Open | Automatic checkpoint policy + combined activate-and-continue `HQ RESUME`; documentation-only. Live metadata: mergeable=true, `mergeable_state=clean`; no submitted reviews. Exact-head checks: 11/11 completed SUCCESS. Still Draft and not approved/merged. |
| [#171](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/171) | `e64ea2fca5b3dd17b709ecf9c92a2cf933da32cb` | Draft / Open | Central Fleet architecture only; deferred after 0.4.0. |

PR base refs and statuses can change; re-fetch them before action. PR #327 was opened from the recorded main SHA and was mergeable=true at the last check, but merge remained blocked by required checks. This checkpoint itself advances its head, so fetch the live PR head/checks before any review or merge decision. PR #169 is the separate HQ playbook and is not BAFT-TUNNEL product scope.

## Active task — P0-A raw TCP vs TLS aged-flow diagnosis

**Status: PARTIAL CONTROL EVIDENCE RECEIVED / P0-A NOT VERIFIED.**

- Completed [run 44101542573](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/44101542573), head SHA `65b50dea5562a15f8299a13afcec64d34a93bf58`, job `134102574533`: conclusion `success`. Job logs are now retrievable. Inputs in the log: `AGE_MODE=tcp`, `AGES=0,15,20,25,30,60`, `SIZE_KB=2048`, `ITER=3`, port 8443. This is raw TCP, not TLS and not BAFT-TUNNEL product traffic.
- Client logs show all 18/18 transfers completed with `ok=true`, `bytes=2097152`, no reported error. Per-attempt client durations: age 0 = 1.40/0.87/0.86s; age 15 = 0.88/0.83/0.88s; age 20 = 94.14/24.40/98.41s; age 25 = 96.11/97.40/109.52s; age 30 = 100.86/85.30/93.72s; age 60 = 93.59/84.49/108.51s. Server logs also report 2,097,152 bytes for each transfer and no reported error; server durations include 5.54/1.88/1.96s (age 0), 20.24/17.22/17.28s (age 15), 119.42/45.92/119.92s (age 20), 126.31/123.99/136.14s (age 25), 136.40/116.97/125.44s (age 30), and 159.38/147.04/171.44s (age 60). The pronounced slowdown appears from age 20 onward in this raw-TCP control. This is diagnostic evidence only; it does not identify root cause or prove tunnel behavior.
- [Run 44101769313](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/44101769313), same workflow head, is still `in_progress`; job `134120451601` remains in the transfer step. Its logs are not yet retrievable (404 `BlobNotFound`) and no artifacts are listed. Its actual dispatch inputs/mode remain UNKNOWN.
- The workflow is explicitly **non-tunnel network control**; it runs a temporary Python transfer endpoint and does not execute BAFT-TUNNEL. Preserve this distinction in all conclusions.
- Do not dispatch another LAB workflow while run `44101769313` is active. Recheck status/jobs/logs/artifacts after completion; validate its mode, parameters and both client/server records. P0-A remains open until the paired TLS-vs-raw-TCP evidence is available and interpreted.

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

## Latest HQ continuation reconciliation — 2026-11-17 14:47 UTC

- P0-A raw TCP control run `44101542573` completed SUCCESS. Its retrieved job log establishes raw TCP ages 0/15/20/25/30/60, 2 MiB x3, with 18/18 client transfers and matching server byte counts successful. Client timing is fast at ages 0 and 15, then degrades severely at ages 20–60 (roughly 24–109s per client transfer); server timing likewise rises. This is an observed correlation, not a root-cause finding.
- Run `44101769313` remains IN_PROGRESS, job `134120451601` at transfer step; logs currently return HTTP 404 `BlobNotFound`, no artifacts listed, dispatch inputs unknown. It must be identified and validated before further LAB dispatch.
- The experiment is non-tunnel control traffic; no BAFT-TUNNEL behavior is established. P0-A remains open pending the TLS comparator and careful interpretation.
- PR #327 live metadata: OPEN / Draft, head `a6b0c4fb4c6cd39bd2986c470819d70d82fb26fa`, base `c3e2bda8cc4b598c410a665f3c18fb6cc2d944c2`, mergeable=true, mergeable_state=blocked. Exact-head checks: 10 total, 7 completed SUCCESS, 3 IN_PROGRESS (Run benchmarks, rust, ubuntu-2204-baseline). No merge or review approval.
- No new LAB dispatch, no firewall/route/sysctl changes, and no product host mutation. No merge, release, tag, deploy or production operation.

## Next action

Continue P0-A from the partial raw-TCP result: wait for run **44101769313** to finish, recover its logs, determine its mode/ages/parameters, and validate client + server evidence. Do not dispatch a third LAB workflow while it is active. Then compare raw TCP with TLS and update the evidence-backed diagnosis; keep all results explicitly labeled non-tunnel control.

## Resume checklist

1. Read [AGENTS.md](../AGENTS.md), [PROJECT_PROFILE.md](../PROJECT_PROFILE.md), [HQ_USAGE.md](HQ_USAGE.md), [HQ_CHECKPOINT_PROTOCOL.md](HQ_CHECKPOINT_PROTOCOL.md), and this file.
2. Read the newest comments on [Issue #2](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/2) (paginate to the newest page).
3. Verify live main SHA, open PR heads/reviews/checks, active runs/jobs/logs/artifacts and Roadmap priority.
4. Confirm applicable authorization and safety gates.
5. Continue from **Next action**; do not repeat completed work or trust stale snapshot fields without live verification.


## Latest HQ continuation reconciliation — 2026-11-17 14:56 UTC

- P0-A run **44101769313** remains IN_PROGRESS; job **134120451601** is still in the transfer step. GitHub's last run update remains **14:45:27 UTC** despite rechecks at 14:55 UTC. Logs return 404 `BlobNotFound`; no artifacts exist. Actual dispatch inputs/mode remain UNKNOWN. Run began 14:19:03 UTC; its job timeout is 45 minutes. Do not infer the comparator mode or treat this as failure before terminal status.
- Do not dispatch another LAB workflow while this run is active. At the next safe recheck, inspect run/job status, logs and artifacts; if it reaches terminal state, recover inputs and validate both client/server evidence before any interpretation.
- PR #327 exact head `8526508820f5ee9db4d9f5aac3822ef3d9e6063c`: all 11/11 Check Runs completed SUCCESS. Live PR remains OPEN / Draft, mergeable=true, mergeable_state=clean, no submitted reviews. Green CI is not approval; do not merge while Draft and without review/owner gate.
- No third LAB dispatch, host mutation, firewall/route/sysctl change, merge, release or deploy.

## Next action

Continue P0-A: wait/recheck run **44101769313**. Determine its actual mode, ages and parameters from workflow logs/dispatch evidence, then validate client and server bytes/timings/errors. Do not launch another LAB run while it is active. Keep the evidence explicitly labeled non-tunnel control traffic.
