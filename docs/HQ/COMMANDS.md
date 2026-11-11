# HQ Conversation Command Protocol

These are plain-language conversation shorthands for work in a project governed by this HQ methodology. They are not shell commands, GitHub slash commands, API endpoints, or guaranteed ChatGPT account-level settings. The agent must read `AGENTS.md` and verify live state in every new session; repository instructions do not automatically configure a ChatGPT account or every unrelated conversation.

| User message | Intended behavior | Minimum output / action |
|---|---|---|
| `.` or `..` | Continue from the latest verified project state and perform the next safe, authorized action. Do not substitute a status recap for execution. | Execute until a real blocker, permission gate or completed task; then give a concise factual handoff. |
| `status` / `وضعیت` | Read-only live status check. | Report current phase/next gate from the canonical board declared in PROJECT_PROFILE.md, main SHA, relevant PR/CI state, active blockers and evidence freshness. |
| `plan: <goal>` | Explore/plan the requested goal without implementation. | Clarify constraints only when essential; produce a scoped plan, dependencies and acceptance criteria. |
| `implement: <task>` | Implement an already authorized, sufficiently specified task. | Verify ownership/baseline, use an isolated workspace, follow the appropriate skill and commit coherent changes. |
| `verify: <sha or PR>` | Validate a specific commit or PR without changing its implementation unless separately requested. | Inspect exact diff, required checks, review threads and available test/LAB evidence; UNKNOWN stays UNKNOWN. |
| `handoff` | Produce a concise, resumable transfer record. | Exact branch/SHA, changed files, tests/evidence, PR, blockers and next safe action. |

## Default execution contract

- A continuation request does not override security boundaries, repository protections or a required owner approval.
- Ask only for information or authorization that is genuinely necessary. Do not repeatedly ask for routine engineering decisions already covered by accepted project policy.
- If the task is clear and safe, proceed without asking the user to re-approve each ordinary read, test or isolated code edit.
- Never merge, release, sign, publish, deploy, change repository protection or mutate persistent hosts merely because `.` / `..` was sent. Those actions require their own explicit gate.
- If execution is blocked, report the exact missing permission, dependency or decision and preserve the current workspace.
- Keep reports short: stage, actual result, exact SHA/PR/run, tests actually executed, and remaining blocker.
