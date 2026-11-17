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
| [#327](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/327) | `db16bfd889637ac23826b66426163cfa2ae88b96` (head immediately before this snapshot refresh) | Draft / Open | Automatic checkpoint policy + combined activate-and-continue `HQ RESUME`; documentation-only. Live PR metadata: mergeable=true, mergeable_state=blocked, no submitted reviews. Exact-head checks at recheck: 10 total, 7 IN_PROGRESS and 3 QUEUED; no CI PASS claimed. This snapshot refresh advances the branch; refetch exact head/checks after write. |
| [#171](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/171) | `e64ea2fca5b3dd17b709ecf9c92a2cf933da32cb` | Draft / Open | Central Fleet architecture only; deferred after 0.4.0. |

PR base refs and statuses can change; re-fetch them before action. PR #327 was opened from the recorded main SHA and was mergeable=true at the last check, but merge remained blocked by required checks. This checkpoint itself advances its head, so fetch the live PR head/checks before any review or merge decision. PR #169 is the separate HQ playbook and is not BAFT-TUNNEL product scope.

## Active task — P0-A raw TCP vs TLS aged-flow diagnosis

**Status: IN PROGRESS / NOT VERIFIED.**

- Diagnostic workflow: [run 44101542573](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/44101542573), head SHA `65b50dea5562a15f8299a13afcec64d34a93bf58`, job `134102574533`; last observed step `Run control transfers (tls and tcp, both directions)` was `in_progress`.
- Queued workflow: [run 44101769313](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/44101769313), same head SHA; status `pending`, no job had started.
- The workflow is explicitly **non-tunnel network control**; it runs a temporary Python transfer endpoint and does not execute BAFT-TUNNEL. It can perform a full TLS/TCP up/down bulk matrix when `AGES` is empty, or a single selected `AGE_MODE` aged-flow series when `AGES` is supplied.
- The dispatch inputs for these two runs were not available in the inspected API response. Job-log retrieval for job `134102574533` still returns `BlobNotFound`; run `44101542573` has no listed artifacts. Therefore the actual mode/age matrix and any transfer outcome are **UNKNOWN**. Do not call either run PASS or FAIL yet.
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

## Latest HQ RESUME reconciliation — 2026-11-17 14:42 UTC

- Live `main` remains `c3e2bda8cc4b598c410a665f3c18fb6cc2d944c2` (PR #325); roadmap snapshot still identifies `v0.2.1` as latest Stable.
- PR #327 before this snapshot refresh: OPEN / Draft, head `db16bfd889637ac23826b66426163cfa2ae88b96`, base unchanged at the current `main`, `mergeable=true`, `mergeable_state=blocked`; no submitted reviews. Exact-head Check Runs: 10 total (7 IN_PROGRESS, 3 QUEUED), no completed result observed at this recheck.
- P0-A run `44101542573` remains IN_PROGRESS on `65b50dea5562a15f8299a13afcec64d34a93bf58`; job `134102574533` is still in the transfer step. Direct job listing confirms this, but log retrieval again returns HTTP 404 `BlobNotFound`; no artifacts are listed.
- P0-A run `44101769313` remains PENDING with no jobs and is queued behind the shared concurrency group. No inputs or transfer outcome were exposed by the inspected API. Both runs remain UNKNOWN / NOT VERIFIED; no PASS/FAIL classification is justified.
- Read-only inspection of the workflow confirms a 45-minute job timeout and that the run measures a non-tunnel control endpoint. It is not product evidence. No new LAB run was dispatched, no LAB host was contacted by this resume, and no host/network mutation occurred.
- This entry is written on the PR #327 branch; its commit advances the PR head. The resulting exact head and check runs must be fetched after the write. No merge, release, tag, deploy or production operation was performed.

## Next action

Continue the combined `HQ RESUME` flow from the active P0-A handoff: recheck runs **44101542573** and **44101769313** and the shared LAB concurrency state. Do not dispatch another LAB workflow until the first run releases the lock and its result/log availability is understood. Then classify the evidence strictly as PASS, FAIL or UNKNOWN and update this file plus Issue #2.

## Resume checklist

1. Read [AGENTS.md](../AGENTS.md), [PROJECT_PROFILE.md](../PROJECT_PROFILE.md), [HQ_USAGE.md](HQ_USAGE.md), [HQ_CHECKPOINT_PROTOCOL.md](HQ_CHECKPOINT_PROTOCOL.md), and this file.
2. Read the newest comments on [Issue #2](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/2) (paginate to the newest page).
3. Verify live main SHA, open PR heads/reviews/checks, active runs/jobs/logs/artifacts and Roadmap priority.
4. Confirm applicable authorization and safety gates.
5. Continue from **Next action**; do not repeat completed work or trust stale snapshot fields without live verification.
