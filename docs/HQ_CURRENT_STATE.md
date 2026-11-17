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
| [#323](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/323) | `7177bb4566d5dd5b4fddc45d69a325a82729d68e` | Draft / Open | Candidate gate now queries merged PR #315 and requires its merge commit to be an ancestor of candidate SHA; static contract test added. New exact-head checks started (2 IN_PROGRESS at recheck); no independent review. Noise-TCP Direct only. No LAB dispatch. |
| [#324](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/324) | `2ca82c8b86d75ffe4db9aae9fc8e1025635e1f7d` | Draft / Open | 11 Check Runs green on this head; no review recorded. Skills/governance + roadmap documentation; parallel, non-blocking track. |
| [#326](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/326) | `ab8095d59cf77bf16bd13e34a81acf77adc463c5` | Draft / Open | F1 pure rules + ADR-0045 accepted with conditions; mergeability/check state not yet verified. |
| [#327](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/327) | `225e606135e3225faeaa14d8f03d8a504e7433d9` | Draft / Open | Automatic checkpoint policy + combined activate-and-continue `HQ RESUME`; documentation-only. Live metadata: mergeable=true, `mergeable_state=blocked`; exact-head checks 2/10 SUCCESS, 8 IN_PROGRESS. No submitted reviews. |
| [#171](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/171) | `e64ea2fca5b3dd17b709ecf9c92a2cf933da32cb` | Draft / Open | Central Fleet architecture only; deferred after 0.4.0. |

PR base refs and statuses can change; re-fetch them before action. PR #327 was opened from the recorded main SHA and was mergeable=true at the last check, but merge remained blocked by required checks. This checkpoint itself advances its head, so fetch the live PR head/checks before any review or merge decision. PR #169 is the separate HQ playbook and is not BAFT-TUNNEL product scope.

## Active task — P0-A raw TCP vs TLS aged-flow diagnosis

**Status: PAIRED RAW-TCP/TLS CONTROL COMPARISON COMPLETE; ROOT CAUSE AND PRODUCT IMPACT NOT ESTABLISHED.**

- Completed [run 44101542573](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/44101542573), head SHA `65b50dea5562a15f8299a13afcec64d34a93bf58`, job `134102574533`: conclusion `success`. Job logs are now retrievable. Inputs in the log: `AGE_MODE=tcp`, `AGES=0,15,20,25,30,60`, `SIZE_KB=2048`, `ITER=3`, port 8443. This is raw TCP, not TLS and not BAFT-TUNNEL product traffic.
- Client logs show all 18/18 transfers completed with `ok=true`, `bytes=2097152`, no reported error. Per-attempt client durations: age 0 = 1.40/0.87/0.86s; age 15 = 0.88/0.83/0.88s; age 20 = 94.14/24.40/98.41s; age 25 = 96.11/97.40/109.52s; age 30 = 100.86/85.30/93.72s; age 60 = 93.59/84.49/108.51s. Server logs also report 2,097,152 bytes for each transfer and no reported error; server durations include 5.54/1.88/1.96s (age 0), 20.24/17.22/17.28s (age 15), 119.42/45.92/119.92s (age 20), 126.31/123.99/136.14s (age 25), 136.40/116.97/125.44s (age 30), and 159.38/147.04/171.44s (age 60). The pronounced slowdown appears from age 20 onward in this raw-TCP control. This is diagnostic evidence only; it does not identify root cause or prove tunnel behavior.
- Completed [run 44101769313](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/44101769313), same workflow head, job `134120451601`: conclusion SUCCESS. Recovered log inputs: `AGE_MODE=tls`, `AGES=0,15,20,25,30,60`, `SIZE_KB=2048`, `ITER=3`, port 8443, empty SNI. Client: 18/18 transfers `ok=true`, exactly 2,097,152 bytes each, no errors. Client durations: age 0 = 1.03/0.86/0.92s; 15 = 0.84/0.92/0.89s; 20 = 96.70/95.16/89.42s; 25 = 23.75/0.83/79.40s (high variance); 30 = 99.05/98.59/82.20s; 60 = 90.76/100.32/100.66s. Server also logged 18 matching 2,097,152-byte records with no errors.
- Script inspection confirms 5-second application ping/pong while aging the same TCP/TLS flow, then a 2 MiB echo transfer; client transfer timing is reset after aging. Server `secs` starts before accept and therefore includes connection/age waiting; do not treat server seconds as pure transfer duration.
- Paired comparison: both raw TCP and TLS are fast at ages 0/15, then both show severe client-side slowdown from age 20 onward. TLS does not remove the observed aged-flow degradation. Age 25 TLS is noisy (0.83–79.40s), so avoid overclaiming an exact threshold or a smooth monotonic curve. Raw TCP and TLS runs were sequential, not simultaneous randomized trials.
- The workflow is explicitly **non-tunnel network control**; it runs a temporary Python endpoint and does not execute BAFT-TUNNEL. The evidence supports a shared path/aged-flow correlation, not a proven network root cause, and does not prove BAFT-TUNNEL product behavior.
- P0-A's requested raw-vs-TLS control matrix is complete. Root-cause isolation and product impact remain unverified. Because TLS showed no mitigation, current evidence does not justify starting TCP-TLS rotation as a mitigation; P0-C remains conditional/parked pending a separate technical rationale.

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


## Latest HQ continuation reconciliation — 2026-11-17 15:12 UTC

- Run **44101769313** completed SUCCESS at 15:11:10 UTC. Recovered inputs establish TLS aged-flow control: ages 0/15/20/25/30/60, 2 MiB x3, port 8443, 5-second app ping/pong; 18/18 client transfers and 18 server records completed with exact byte counts and no reported errors.
- Paired with raw-TCP run **44101542573**, both modes are fast at age 0/15 and both degrade sharply from age 20 onward. TLS does not remove the effect. TLS age 25 is highly variable (client 0.83/23.75/79.40s); no smooth threshold claim. The server `secs` metric includes accept/aged waiting time, so client timings are the meaningful transfer comparison.
- Classification: P0-A requested non-tunnel raw-TCP/TLS control comparison COMPLETE; root cause and BAFT-TUNNEL product impact NOT ESTABLISHED. This is evidence of a shared aged-flow/path correlation, not proof of a particular network mechanism. Do not claim tunnel behavior. Current evidence does not justify TCP-TLS rotation as mitigation; P0-C stays conditional/parked absent a separate rationale.
- PR #327 remains OPEN / Draft; current head must be re-fetched after this snapshot write. No merge/release/deploy. The prior exact head `f86b671ed1b021990c4962a200e5a89c60804208` had 11/11 Check Runs SUCCESS; this snapshot update will create a new head.
- No third LAB dispatch or host configuration change was performed.

## Next action

Proceed to P0-B on PR #323: refresh exact head, required checks and review state; if still eligible, request/obtain independent review before any owner-authorized real two-host Noise-TCP Direct LAB. Keep this separate from the TCP/TLS control finding. In parallel, design a safe root-cause isolation plan that does not change firewall, routes, sysctls, or CAP_NET_ADMIN settings.


## Latest HQ continuation reconciliation — 2026-11-17 15:18 UTC

- P0-A paired non-tunnel control comparison is complete: raw TCP run 44101542573 and TLS run 44101769313, both SUCCESS, 18/18 2 MiB transfers per run, ages 0/15/20/25/30/60, 5-second application ping/pong. Both modes are fast at 0/15 and severely slower from age 20 onward; TLS does not remove the effect. Age-25 TLS variance is high. Server timing includes pre-accept/aging wait; use client timings. Root cause and product impact remain unproven. P0-C TCP-TLS rotation remains parked absent a separate rationale.
- P0-B PR #323 live: OPEN / Draft, head `d1897c5cb3a6af89868ce57255136a6087d908d3`, base `a0e759d208c93e7ccda9f36ad3b9fc2fa9cd3de3`, mergeable=true / clean, 11/11 exact-head checks SUCCESS, zero submitted reviews and no requested reviewers. It is a Noise-TCP Direct harness, not TCP-TLS proof.
- Pre-review audit found a harness candidate-selection gap: workflow checks requested SHA is an ancestor of current main and required contexts pass, but does not assert that it contains PR #315's merged rotation implementation. PR #315 merge commit is `a0e759d208c93e7ccda9f36ad3b9fc2fa9cd3de3`. An older green main SHA could be selected. Posted pre-review note on PR #323 (comment 6952163991), asking for a merged-PR/merge-commit ancestry guard and static contract test. This is a reproducibility/correctness gate, not an unsafe-code finding.
- Main HEAD remains `c3e2bda8cc4b598c410a665f3c18fb6cc2d944c2`. PR #327 exact head `225e606135e3225faeaa14d8f03d8a504e7433d9`: 2/10 checks SUCCESS, 8 IN_PROGRESS; OPEN / Draft, mergeable_state=blocked.
- Next: fix/verify PR #323 candidate gate, rerun exact-head CI, obtain independent review, then re-evaluate explicit owner authorization for the real two-host Noise-TCP Direct LAB. No LAB dispatch, merge, release, deployment, firewall/route/sysctl change or CAP_NET_ADMIN use.

## Next action

P0-B: close the PR #323 candidate ancestry guard gap (must prove PR #315 is merged and its merge commit is an ancestor of the candidate SHA), add static contract coverage, then recheck all required exact-head CI and independent review. Only after those gates should the owner-authorized LAB dispatch be considered.

## Latest HQ continuation reconciliation — 2026-11-17 15:22 UTC

- P0-B PR #323 candidate-selection gap was fixed on the PR branch. Workflow now queries PR #315, requires `.merged == true`, extracts its exact `merge_commit_sha`, and requires that commit to be an ancestor of `candidate_sha`; static contract test asserts these guards. Read-back verified both files.
- New PR #323 head: `7177bb4566d5dd5b4fddc45d69a325a82729d68e`; OPEN / Draft, mergeable=true, mergeable_state=blocked. At first exact-head check: 2 Check Runs IN_PROGRESS (`package-and-verify`, `lab-config`); remaining checks may still be queued/not created. No submitted reviews. Await all exact-head checks and independent review.
- P0-B harness still Noise-TCP Direct only. No real LAB dispatch. Do not infer TCP-TLS mitigation from this harness.
- PR #327 remains OPEN / Draft, head `225e606135e3225faeaa14d8f03d8a504e7433d9`, exact-head checks 2/10 SUCCESS + 8 IN_PROGRESS at last verification, mergeable_state=blocked.
- P0-A paired non-tunnel raw-TCP/TLS control comparison is complete; root cause/product impact remain unverified. P0-C remains parked. No host mutation, firewall/route/sysctl change, merge, release or deploy.

## Next action

Recheck PR #323 exact-head required workflows until terminal. If green, obtain an independent review (none is currently recorded) before considering any owner-authorized real two-host Noise-TCP Direct LAB. In parallel, prepare a read-only, no-CAP_NET_ADMIN root-cause isolation plan for the aged-flow correlation.
