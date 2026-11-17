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

- **Verified at:** 2026-11-17 15:24 UTC.
- **main HEAD:** `c3e2bda8cc4b598c410a665f3c18fb6cc2d944c2` (PR #325); 13/13 Check Runs SUCCESS. Required branch-protection contexts are `rust`, `ubuntu-2204-baseline`, and `lab-config`, all SUCCESS on this exact SHA. This is hosted CI, not LAB evidence.
- **Open product PRs (live list):**

| PR | Head SHA | State | Exact-head CI | Review / note |
|---|---|---|---|---|
| [#323](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/323) | `7177bb4566d5dd5b4fddc45d69a325a82729d68e` | Draft / Open | 11/11 SUCCESS; all required contexts SUCCESS | No submitted reviews/requested reviewers. Candidate guard requires merged PR #315 and its merge commit ancestor. Noise-TCP Direct harness only; no LAB dispatch. |
| [#324](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/324) | `2ca82c8b86d75ffe4db9aae9fc8e1025635e1f7d` | Draft / Open | 11/11 SUCCESS | Skills/governance documentation; non-blocking parallel track; no review recorded. |
| [#326](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/326) | `ab8095d59cf77bf16bd13e34a81acf77adc463c5` | Draft / Open | 12/12 SUCCESS | Fleet F1 pure rules / ADR-0045; base is older than current main; no review recorded. |
| [#327](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/327) | `b67631acec69472927bb7c99394aa3fbf98e2553` | Draft / Open | 10/11 SUCCESS, 1 IN_PROGRESS (`rust`) at last exact-head read | HQ checkpoint protocol/snapshot docs; this checkpoint update advances its head and restarts CI. |
| [#328](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/328) | `c2a92a2270c93d3b495ecd38da4fcbe24d7aaeb5` | Draft / Open | 11/11 SUCCESS | Roadmap §9.7 raw TCP/TLS aged-flow correction; documentation-only; no review recorded. |
| [#171](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/171) | `e64ea2fca5b3dd17b709ecf9c92a2cf933da32cb` | Draft / Open | Not rechecked in this resume | Central Fleet architecture; deferred until after 0.4.0. |

All PRs above remain Draft. CI status is exact-head and time-sensitive; re-fetch before any landing decision. PR #169 is the separate HQ playbook and is not BAFT-TUNNEL product scope.

- **Release refs:** `v0.2.1` is the latest Stable tag. `v0.2.2` tag does not exist. `v0.2.2-rc.3` is a published prerelease whose tag object resolves to commit `5dd0890cac9deddcac66c3105d3fbaf9164e391e`; `v0.3.0-rc.1` is also a prerelease. No Stable tag/release was created in this resume.
- **Worktrees on Mac:** HQ takeover worktree `hq/agent-skills-roadmap-20261117` at `bc1e2ebedea96ed61e183675a30fa5720a35764f`, clean; PR #323 review worktree detached at `d1897c5cb3a6af89868ce57255136a6087d908d3`, clean; roadmap worktree detached at `2ca82c8b86d75ffe4db9aae9fc8e1025635e1f7d`, clean. Preserve all three.
- **Mac Agent Skills accessed in this resume:** `awesome-code-review`, `awesome-security-audit`, `awesome-test-gap-audit`, `verification-before-completion`, and `using-git-worktrees`. The previously referenced Rust-specific `rs-*` skill files were not found in the checked Mac skill roots in this session; do not claim they were loaded here.

PR #327 remains a documentation-only Draft. Its CI may be restarted by each snapshot update. PR #169 remains outside product scope.

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


## Latest HQ continuation reconciliation — 2026-11-17 15:24 UTC

- P0-A paired raw TCP/TLS non-tunnel controls are complete. Both 18/18 client transfers succeeded at all six ages; both modes show fast 0/15s and slow 20s onward. TLS does not remove the correlation; age-25 TLS is noisy. Root cause and product-path impact remain unverified.
- PR #323 head `7177bb4566d5dd5b4fddc45d69a325a82729d68e`: candidate validation now requires PR #315 merged and its merge commit to be ancestor of candidate SHA; static test added. Exact-head checks latest: 1 SUCCESS, 7 IN_PROGRESS, 2 QUEUED. OPEN / Draft, blocked, no independent review. No LAB dispatch.
- PR #328 created from main `c3e2bda8cc4b598c410a665f3c18fb6cc2d944c2`, head `c2a92a2270c93d3b495ecd38da4fcbe24d7aaeb5`, documentation-only update to Roadmap §9.7 correcting stale raw-vs-TLS status and recording the evidence/caveats. 5 exact-head checks QUEUED at last recheck; OPEN / Draft.
- PR #327 latest head `38c909db4ab97d1e4142393c3b50d93b959f799e`: 0 SUCCESS, 1 IN_PROGRESS, 9 QUEUED; OPEN / Draft, blocked.
- No independent review, no merge, no LAB dispatch, no host mutation, no firewall/route/sysctl/CAP_NET_ADMIN change, no release/deploy.
- Next: wait for exact-head checks on #323/#328/#327; arrange independent review for #323 before any LAB. Keep product-specific root-cause isolation separate and non-privileged.

## Latest HQ continuation reconciliation — 2026-11-17 15:24 UTC

- HQ RESUME re-read the HQ playbook, BAFT-TUNNEL entry point, project profile, usage guide, checkpoint protocol, current snapshot, latest Issue #2 comments, roadmap, remaining-work plan, release policy, merge policy and LAB access contract.
- Live main is `c3e2bda8cc4b598c410a665f3c18fb6cc2d944c2`; 13/13 exact-SHA Check Runs SUCCESS. Required branch-protection contexts are all green.
- PR #323 exact head `7177bb4566d5dd5b4fddc45d69a325a82729d68e`: 11/11 exact-head checks SUCCESS, including all three required contexts. PR remains OPEN / Draft, mergeable=true, with no submitted reviews and no requested reviewers. The candidate-selection guard for merged PR #315 is present. No LAB was dispatched.
- PR #324: 11/11 SUCCESS; PR #326: 12/12 SUCCESS; PR #328: 11/11 SUCCESS. All remain Draft. PR #327 before this snapshot write: 10/11 SUCCESS, with `rust` still IN_PROGRESS; its head advances because this snapshot is being refreshed. PR #171 remains Draft and deferred; its CI was not rechecked.
- Release verification: latest Stable tag is `v0.2.1`; no `v0.2.2` tag exists. `v0.2.2-rc.3` remains a prerelease on `5dd0890cac9deddcac66c3105d3fbaf9164e391e`. No tag or release action was taken.
- Mac is online. Three known BAFT-TUNNEL worktrees were checked and are clean at their recorded commits. Relevant Mac skills were read: code review, security audit, test-gap audit, verification-before-completion, and worktree safety. Rust-specific `rs-*` skills were not located in the inspected roots.
- **Next safe action:** obtain an independent agent review of PR #323 at its exact current head. Keep it Draft; no merge and no LAB dispatch until review findings are read, the head remains unchanged, and the separate owner-authorized LAB gate is reconfirmed. Do not conflate this Noise-TCP Direct harness with the P0-A raw TCP/TLS control finding.

## Latest HQ continuation reconciliation — 2026-11-17 15:25 UTC

- Attempted to start an independent CodeAgent review for PR #323 using the connected CodeAgent Mobile integration. The request failed with `AGENT_NOT_AVAILABLE`: no linked agent can run the review.
- `list_agents` returned only managed wallet-based agents (DeepSeek V4 Flash, DeepSeek V4, Qwen3 Coder, Codex, Claude); the integration's PR-review action supports the default linked agent only. `list_sessions(status=online)` returned no sessions. No agent was started and no wallet spend occurred.
- The Mac has no `codex` or `claude` CLI available. GitHub CLI is authenticated. The repository collaborator list contains only `zarkmakerburg`; no independent human collaborator is currently available to request.
- PR #323 remains OPEN / Draft at `7177bb4566d5dd5b4fddc45d69a325a82729d68e`, 11/11 exact-head checks SUCCESS, no submitted reviews, no requested reviewers. The candidate-selection guard is present. No LAB dispatch.
- The checkpoint snapshot and Issue #2 board must record this review blocker. No merge, LAB, tag, release, deploy or host operation was performed.

## Live release and repository reconciliation — 2026-11-17 15:50 UTC

- **main:** `c3e2bda8cc4b598c410a665f3c18fb6cc2d944c2`; 15/15 exact-SHA Check Runs SUCCESS; required `rust`, `ubuntu-2204-baseline`, `lab-config` contexts SUCCESS. Branch protection remains enabled.
- **Stable v0.2.2 was published after the previous checkpoint:** tag `v0.2.2` points to `5dd0890cac9deddcac66c3105d3fbaf9164e391e`; annotated tag message `BAFT-TUNNEL v0.2.2`; tagger Amirm... at 15:36:21 UTC. GitHub Release is public, non-draft, `prerelease=false`, published 15:39:08 UTC. Release workflow, phase0 and same-SHA package/signature verification workflows all SUCCESS. Eight assets uploaded: engine licenses, engine binary, tunnel binary, tunnel tar.gz, SBOM, engine manifest, SHA256SUMS and cosign bundle. This is a real Stable publication, not merely a tag candidate.
- **Important release-policy conflict / acceptance risk:** `docs/RELEASE.md` says releases stay pre-release until Phase 1 gates close. On the same target SHA `5dd0890...`, two historical trusted two-host U7 upgrade acceptance runs failed:
  - [Run 43897433228](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/43897433228): soak payload mismatches (expected 2,097,289 bytes; observed 851,860 / 982,932 / 868,244) and subsequent mTLS `CertificateRevoked` failures.
  - [Run 43910396414](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/43910396414): probes failed during soak, Yamux session timeout, shutdown deadline exceeded and subsequent mTLS `CertificateRevoked` failures.
  - In both, release asset verification/tag validation jobs succeeded; the two-host U7 LAB job failed. The current tag-push release pipeline itself succeeded but does not erase these failed acceptance records. Stable release was not reverted or modified; flag for owner review and release-policy reconciliation.
- **Open Draft PRs (live list):**
  - #323 head `7177bb4566d5dd5b4fddc45d69a325a82729d68e`, 11/11 SUCCESS, no review, no LAB. Noise-TCP Direct rotation harness.
  - #324 head `2ca82c8b86d75ffe4db9aae9fc8e1025635e1f7d`, 11/11 SUCCESS.
  - #326 head `ab8095d59cf77bf16bd13e34a81acf77adc463c5`, 12/12 SUCCESS.
  - #327 head `e86a1ebe2f1fb0c58fd166ed45bcb55f203dc041`, 11/11 SUCCESS.
  - #328 head `c2a92a2270c93d3b495ecd38da4fcbe24d7aaeb5`, 11/11 SUCCESS.
  - #329 head `dfecce03ec37e1e603b2d5314170b7a54ca349f3`, README install pin v0.2.2, 11 checks with `rust` IN_PROGRESS; CodSpeed gate passed with warning that 3 benchmarks are dominated by system calls.
  - #330 head `c83b3ac3a2a9a74fbfe461367744fbb6043c0bea`, repeat E2 blackhole scenario per tool, 12 checks with `ubuntu-2204-baseline`, `rust`, and `e2` IN_PROGRESS; CodSpeed gate passed with same system-call warning.
  - #171 remains Draft/deferred; not rechecked in this live pass.
- **PR #323 review gate:** still Draft, exact-head CI green, no submitted reviews. CodeAgent has no linked agent; no independent collaborator exists in repo collaborator list. No LAB dispatch.
- **v0.3.0-rc.1** remains prerelease; no rc.2 created.
- No merges, new LAB dispatch, host mutation, firewall/route/sysctl changes, CAP_NET_ADMIN, new tags, or deployment were performed in this report.

## Next action

Independent review of PR #323 is BLOCKED because there is no linked CodeAgent provider or independent GitHub collaborator available. To proceed, the owner must either link an agent provider in CodeAgent Mobile or add/name an independent GitHub collaborator reviewer. After that, request the review against exact HEAD `7177bb4566d5dd5b4fddc45d69a325a82729d68e`, read and resolve findings, revalidate exact-head CI, and only then consider the separate owner-authorized two-host Noise-TCP Direct LAB. Do not merge or dispatch LAB while this gate is open.


## Live HQ continuation reconciliation — 2026-11-17 (after PR #323 rotation LAB)

- Bound project: `zarkmakerburg/BAFT-TUNNEL` only. HQ playbook, project `AGENTS.md`, `PROJECT_PROFILE.md`, `docs/HQ_USAGE.md`, command/evidence policy and checkpoint protocol were read. The BAFT-TUNNEL profile binds Issue #2 as the append-only operational board and `docs/PRODUCT_ROADMAP.md` as the product roadmap.
- Live `main`: `3eceebf28263a6be17582c37725f7b38627e8e67`. Branch protection is enabled. At the latest exact-SHA poll, `lab-config`, `panel-ui`, `ubuntu-2404-pipe-install`, `offline-ca-container`, `package-and-verify`, CodSpeed and benchmarks were SUCCESS; `rust` and `ubuntu-2204-baseline` were still IN_PROGRESS. Do not report required CI green until both complete on this exact SHA.
- Releases: Stable `v0.2.2` remains published at `5dd0890cac9deddcac66c3105d3fbaf9164e391e`; `v0.3.0-rc.1` remains a prerelease. No tag/release modification was made.
- **PR #323 state changed:** merged by owner `zarkmakerburg` at 2026-11-17 16:01:34Z; merge commit `c2c64bc6bbe3077286e616f128199c8969ab97fd`. GitHub has no submitted PR reviews. The previously open review/LAB plan is therefore superseded by the observed owner merge and subsequent run; absence of a review record remains a governance fact, not evidence that review occurred.
- **Real two-host LAB result now verified:** workflow run [44121043530](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/44121043530), job `134173734351`, completed SUCCESS. Candidate source SHA `c3e2bda8cc4b598c410a665f3c18fb6cc2d944c2`; candidate binary SHA-256 `29bd61a2c64a1ccaacad080f4104552a84abeefc65450ea4e711baef25a1aef9`, independently verified on both LAB hosts. Three 2 MiB probes after 60 s aging with rotation OFF all failed (0/3; payloads 819,092 / 753,583 / 769,940 bytes). With `session_rotate_secs=10`, all three passed (3/3, 3.5–4.1 s) with exact 2,097,152-byte payloads and matching SHA-256; 12 completed rotation events were observed. Final marker: `ROT_ACCEPTANCE=PASS order=rotation_off_then_rotation_on off=0/3 on=3/3 rotation_events=12`.
- Scope limitation: this is **Noise-TCP Direct** on the two-host LAB path. It supports session rotation as a mitigation under this tested configuration. It does **not** prove TCP-TLS mitigation, establish the root cause of aged-flow throttling, prove general stream resume, or establish Production readiness. Do not extrapolate it to the v0.2.2 TCP-TLS release issue.
- PR #329 merged into main at `3eceebf28263a6be17582c37725f7b38627e8e67`; PR #330 merged at `b7172bc84bb998b0ee03c8a460e69132a09d411f`. PR #331 and #332 are new Draft PRs.
- PR #331 `docs: reference projects review and roadmap reconciliation`, head `b316ff52a6c4cbb43a51ad1cda25fd66b85e875c`, OPEN/Draft, 11/11 exact-head checks SUCCESS. Its reference review correctly identifies Rakhsh as `CluvexStudio/rakhsh` and BackPack as `AminMGMT/BackPack`. However, the document is stale after recent events: it says #323 was not dispatched and that session rotation has no LAB before/after evidence; its open-PR inventory also lists #323/#329/#330 as open. These claims must be reconciled before merge. This session re-read upstream repository metadata and LICENSE files for Rakhsh and BackPack; both identify AGPL-3.0. This does not replace legal review or verify every other reference licence.
- PR #332 `bench: add LAB E3 workflow`, head `8283938ce4ac8ca0c9304851d6e909a2171894a1`, OPEN/Draft, 13/13 checks SUCCESS including E1/E2 hosted-runner jobs. E2 is simulation, not LAB. E3 is a manual two-host TCP-TLS benchmark with a maximum 5.4-hour window and has **not** been dispatched. A task-specific owner authorization for that exact long LAB run was not independently verified in the canonical board during this pass; do not dispatch until verified.
- PR #327 checkpoint branch remains OPEN/Draft at prior head `838fc0a8e2f8f41f2918c4e68bfa49d04ec2a244`, 11/11 checks SUCCESS before this snapshot refresh. This update will create a new head and restart its checks. PRs #324, #326 and #328 remain OPEN/Draft; #171 remains deferred.
- Mac worktrees were inspected and are clean: `BAFT-TUNNEL-hq-takeover-20261117` at `bc1e2ebedea96ed61e183675a30fa5720a35764f`; `BAFT-TUNNEL-pr323-review-20261117` detached at `d1897c5cb3a6af89868ce57255136a6087d908d3`; `BAFT-TUNNEL-roadmap-update-20261117` detached at `2ca82c8b86d75ffe4db9aae9fc8e1025635e1f7d`. All were preserved.
- Mac skills read from `~/.agents/skills`: `awesome-code-review`, `awesome-security-audit`, `awesome-test-gap-audit`, `verification-before-completion`, `using-git-worktrees`. No Rust-specific `rs-*` skill was found in the inspected skill roots.
- No new LAB dispatch, persistent host mutation, firewall/route/sysctl change, CAP_NET_ADMIN use, merge, tag, release, or deploy was initiated by this HQ continuation. The PR #323 merge and run above were observed as already completed GitHub events.

## Next action

1. Review PR #332's E3 workflow and helper scripts for correctness, safety, cleanup, and truthful evidence before any long LAB execution. Do not dispatch until the exact 5.4-hour LAB authorization is verified.
2. Request corrections to PR #331's stale #323/LAB/PR inventory statements before merge.
3. Keep the v0.2.2 aged-flow investigation separate: current rotation LAB evidence is Noise-TCP Direct only; TCP-TLS cause and product impact remain unproven.


## U7 evidence clarification — 2026-11-17

This section supersedes any earlier snapshot wording that treated every `CertificateRevoked` line as a failure or described the third run as a workflow PASS.

- Workflow runs #43897433228 and #43910396414 both used the signed `v0.2.2-rc.3` candidate, binary SHA-256 `5f4ef12063c9d8a7a55ce993380c84d42ec648ad8121d3c4dd02cea43ac54247`. Their workflow-run `head_sha` is `5dd0890cac9deddcac66c3105d3fbaf9164e391e`. Their two-host soak gates failed (5 and 7 failed probes respectively).
- Run #43945271865 used the same rc.3 candidate binary and binary SHA-256, but its workflow-run `head_sha` was `2f48138ddd8904063fcdb91c8cda945324d24ca4`. GitHub's workflow conclusion is **FAILURE**. `docs/STABLE_0.2.2_EVIDENCE.md` records a coordinator decision delegated by the owner at 2026-11-16 21:15Z applying the 2026-11-15 outage rule: 2 of 240 probes were accepted as an external path outage because both nodes recorded a Yamux timeout in the same second, incoming SSH KEX connections closed, recovery was automatic, restarts were zero, and fd/RSS stayed flat. This is **manual outage-rule acceptance**, not a successful workflow run. The evidence document explicitly says post-soak reconnect and backup steps did not execute in that run.
- The `CertificateRevoked` log lines at the end of these runs correspond to the script's explicit final TLS negative tests: `lab/real-hosts/phase1-two-host-e2e.sh` lines 1048–1076 revoke the client and server certificates and require refusal. These lines are expected negative-test evidence, not by themselves product failures.
- Do not re-label runs #43897433228 or #43910396414 as PASS; the outage exception is documented for #43945271865 only. The Stable v0.2.2 release decision therefore relied on one documented manual exception for the rc.3 candidate, with the post-soak coverage limitation above. Do not claim the release was necessarily policy-violating; retain the policy question of whether the human exception provided sufficient acceptance coverage and whether R-6 should make the rule machine-checkable.
- PR #331 received follow-up evidence notes in comments [6953905789](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/331#issuecomment-6953905789) and [6953945625](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/331#issuecomment-6953945625). PR #332 received pre-LAB review findings in [6953906564](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/332#issuecomment-6953906564). No branch content was modified by those comments.
- PR #332 review blockers before E3 dispatch: missing cosign verification of the release checksum bundle; candidate runs as the LAB SSH account (last observed values were root); no per-host binary digest verification after transfer; remote `nohup` processes lack an independent host-side TTL; 5.4-hour cap omits the per-send 1500-second timeout; LAB host/user validation is absent; and the harness performs application-level retries, which must not be described as tunnel stream resume.
