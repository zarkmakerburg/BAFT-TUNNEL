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
- **`HQ RESUME`** — read `AGENTS.md`, `PROJECT_PROFILE.md`, this protocol, `HQ_CURRENT_STATE.md`, the latest Issue #2 comments and the relevant roadmap/work-plan sections; then verify every mutable fact live before continuing the next safe, authorized action.
- **`HQ STATUS`** — read-only live reconciliation; do not edit the snapshot or board.
- **`.`, `..`, `...`** — retain the project profile's continuation semantics. Each command must verify the current state before action; the snapshot is a starting index, not proof.

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

1. User activates `HQ ON`; HQ remains chat-scoped.
2. Bind only BAFT-TUNNEL and read `AGENTS.md` → profile → `HQ_USAGE.md` → this protocol → current snapshot.
3. Read the newest Issue #2 comments (paginate to the newest page; do not assume page 1 is newest).
4. Verify `main`, PR heads, checks, run statuses and relevant evidence live. A snapshot may be stale by the time a new chat starts.
5. Reconfirm any permission that is chat-scoped, task-specific, expired, or not explicitly recorded in the project's approved process. `HQ ON` and a checkpoint do not grant permissions.
6. Continue from the recorded next action without repeating completed work. If evidence conflicts, trust the newer authoritative source and record the discrepancy.

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
