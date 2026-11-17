# HQ checkpoint and resume protocol

This protocol makes BAFT-TUNNEL work recoverable across long threads and new chats. It supplements, and never overrides, `AGENTS.md`, `PROJECT_PROFILE.md`, `docs/HQ_USAGE.md`, the canonical board (Issue #2), the Product Roadmap, and the project's safety/acceptance policies.

## Sources of truth

| Information | Canonical source |
|---|---|
| Product code, branch and exact commit | GitHub repository refs and commit SHAs |
| Current task handoff / compact live snapshot | [HQ_CURRENT_STATE.md](HQ_CURRENT_STATE.md) |
| Historical operations, decisions, approvals, evidence and blockers | [Issue #2](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/2), append-only comments |
| Product priority and sequencing | [PRODUCT_ROADMAP.md](PRODUCT_ROADMAP.md) |
| Detailed open work | [REMAINING_WORK_PLAN.md](REMAINING_WORK_PLAN.md) |
| Permissions and safety | `PROJECT_PROFILE.md`, `MERGE_POLICY.md`, `RELEASE.md`, `LAB_AGENT_ACCESS.md`, relevant ADRs |

The current-state file is a replaceable snapshot, not a historical record. Issue #2 remains append-only. Do not duplicate full test logs or technical findings in the snapshot; link to the source run, PR, commit, or board entry.

## Commands

These are conversational HQ commands, not shell commands:

- **`HQ CHECKPOINT`** — refresh live GitHub state first; update the current snapshot with exact refs, PRs, checks, runs, blockers and one next action; append a concise dated handoff to Issue #2. Never record an unverified result as PASS.
- **`HQ RESUME`** — a combined activate-and-continue command. If HQ is OFF in this chat, activate it first and bind only the project explicitly specified by the active project profile; do not stop at an activation announcement. Read `AGENTS.md`, `PROJECT_PROFILE.md`, this protocol, `HQ_CURRENT_STATE.md`, the newest Issue #2 comments and relevant roadmap/work-plan sections; verify every mutable fact live; reconcile the last checkpoint; then execute the first safe, authorized, unfinished next action. If HQ is already ON, do not repeat activation work or give an activation report—resume directly from the verified handoff. The response must be a **continuation progress report**: what was resumed, what action was performed, evidence/result, remaining blocker and next action. Report activation only as a brief note if it was necessary. If the next action is blocked or requires missing approval, perform the safest useful alternative and report the exact blocker; never fabricate progress.
- **`HQ STATUS`** — read-only live reconciliation; do not edit the snapshot or board.
- **`.`, `..`, `...`** — retain the project profile's continuation semantics. Each command must verify the current state before action; the snapshot is a starting index, not proof.

## Automatic checkpoint policy

HQ must not wait for the user to say that a thread is ending. Apply a two-layer checkpoint after **every user message** and after **every executed instruction / tool operation**:

### 1. Turn-level checkpoint (every message)

After each user message, update the in-thread working ledger before proceeding:
- current user intent and the instruction being handled;
- decisions and constraints newly stated or changed;
- active task, exact branch/SHA/run where known;
- operation state: NOT STARTED / IN PROGRESS / PASS / FAIL / UNKNOWN / BLOCKED;
- approvals that are still valid for this exact task;
- the next safe action and any blocker.

After each tool operation, record its observed result in that ledger before making a dependent call. A planned action is never recorded as completed. If a user message is conversational and changes no project state, this lightweight ledger is sufficient; do not create a noisy, empty Issue #2 comment.

### 2. Durable checkpoint (automatic triggers)

Update `HQ_CURRENT_STATE.md`, append a dated Issue #2 comment, and verify both writes whenever any of these occurs:
- a user decision, scope, priority, constraint, or task authorization materially changes;
- a repository/worktree/branch/commit/PR/review/CI/workflow state changes;
- an instruction completes, fails, or becomes blocked after performing project work;
- new test, CI, security-review, or LAB evidence is produced or its status changes;
- a blocker, risk, acceptance gap, or next action materially changes;
- work is handed off, paused, switched to another task, or a long-running operation is about to be started.

Also create a durable checkpoint after every **3 substantive user turns or 10 meaningful tool operations, whichever comes first**, even if the work is still in progress. Before a long-running or asynchronous operation, first persist its intended scope, exact target and current authorization; checkpoint again when its outcome is known.

### 3. Checkpoint quality and anti-spam rules

- Refresh mutable facts from GitHub before writing; include exact SHAs, PR/run IDs, evidence class, limitations, blockers and one next action.
- Keep Issue #2 append-only. The snapshot is replaceable and must link to source evidence; never duplicate raw logs or secrets.
- Do not write an Issue #2 entry for a no-op conversational turn unless it changes the recovery state. The turn-level ledger still applies.
- Never call a checkpoint complete until the snapshot and Issue #2 entry have both been written and read back successfully.
- If a write or verification fails, report **CHECKPOINT INCOMPLETE**, preserve the exact failure, and retry only when safe.
- This is best-effort within an active ChatGPT session. It cannot run after the client terminates the thread, in the background, or when tools are unavailable; therefore checkpoints must be created during work, not deferred until a guessed context limit.
- A checkpoint is not an approval and does not extend or transfer task-specific permissions.

## Checkpoint procedure

1. Confirm the bound repository is exactly `zarkmakerburg/BAFT-TUNNEL`.
2. Read the project entry files and the latest Issue #2 comments. Do not scan unrelated repositories or import context from another project.
3. Fetch live `main` SHA and status; fetch all open PRs, their head/base SHAs, draft/review/mergeability state and exact-head checks.
4. For active workflows, fetch run metadata, jobs, step status, logs and artifacts. Record the tested SHA and evidence level. If logs/artifacts are unavailable, say so explicitly.
5. Reconcile against the current Roadmap and Remaining Work Plan. Keep CI, local tests, mocks/simulations and real two-host LAB as separate evidence classes.
6. Rewrite `HQ_CURRENT_STATE.md` with the new verified snapshot and one next action. Preserve older history in Issue #2; do not rewrite or delete old comments.
7. Append a concise dated Issue #2 comment linking the snapshot commit/PR and recording material changes, evidence, limitations and next action.
8. Verify the committed snapshot and board comment after writing. A failed write is not success.

## Resume procedure for a new chat

1. On `HQ RESUME`, if HQ is OFF, activate HQ in this chat as part of the same command; do not require a separate `HQ ON` message. HQ remains chat-scoped.
2. Bind only BAFT-TUNNEL and read `AGENTS.md` → profile → `HQ_USAGE.md` → this protocol → current snapshot.
3. Read the newest Issue #2 comments (paginate to the newest page; do not assume page 1 is newest).
4. Verify `main`, PR heads, checks, run statuses and relevant evidence live. A snapshot may be stale by the time a new chat starts.
5. Reconfirm any permission that is chat-scoped, task-specific, expired, or not explicitly recorded in the project's approved process. `HQ RESUME` and a checkpoint do not grant permissions.
6. Reconcile the checkpoint against live state and continue the first safe, authorized, unfinished action. Do not stop after loading documents or announcing activation.
7. Report resumed work, actual actions and evidence, blockers, and the next action. Do not make the response primarily an HQ activation/status announcement. If no action can safely proceed, state exactly why and identify the closest safe next step.

## Status vocabulary

- **PASS** — the required acceptance condition completed and its exact evidence is accessible.
- **FAIL** — the required condition ran and failed; preserve the failure.
- **IN PROGRESS** — a verified run/job is active.
- **PENDING / QUEUED** — verified run exists but has not started.
- **UNKNOWN / NOT VERIFIED** — evidence is missing, inaccessible, stale, ambiguous or insufficient.
- **NOT APPLICABLE / DEFERRED** — explicitly out of current scope or sequenced later.

Never convert UNKNOWN into PASS based on a green CI check, a mock, a static test, an API status alone, or a run title.

## Safety and scope

- Preserve all branches, worktrees, stashes and concurrent agent changes. No destructive reset, force-push, branch deletion or history rewrite.
- Never include credentials, secret values, host addresses, private keys or raw sensitive logs in the snapshot or Issue #2.
- Use only the two registered LAB hosts and approved workflows. One real-host workflow at a time. No firewall, route, sysctl or `CAP_NET_ADMIN` changes.
- No merge, tag, release, signing, deployment, production or host mutation unless the relevant project policy and current owner authorization explicitly permit that exact action.
- Do not infer that a successful control-network test proves BAFT-TUNNEL product behavior.
- Do not claim a checkpoint is complete until both the repository snapshot and its post-write verification exist.
