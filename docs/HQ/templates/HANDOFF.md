# Engineering Handoff: <task / workstream>

- **Project:** BAFT-TUNNEL
- **Task / Issue:** <identifier and link>
- **State:** active | blocked | review | accepted
- **Handoff time (UTC):** <YYYY-MM-DD HH:MM>
- **From → To:** <role/worker> → <role/worker>

## Exact repository state

- **Repository:** `zarkmakerburg/BAFT-TUNNEL`
- **Branch:** `<branch>`
- **Base SHA:** `<full SHA>`
- **Current HEAD:** `<full SHA>`
- **Workspace:** `<isolated path>`
- **Working tree:** clean | dirty (<list exact changed paths>)
- **PR:** <URL and draft/open/closed state, or none>

## What changed

- <Changed file / behavior and commit link>

## Verification actually performed

| Type | Exact SHA | Command / run | Observed result | Limitation |
|---|---|---|---|---|
| Local test | `<SHA>` | `<command>` | PASS / FAIL / UNKNOWN / NOT RUN | <scope> |
| Hosted CI | `<SHA>` | <run URL and jobs> | PASS / FAIL / UNKNOWN / NOT RUN | <scope> |
| Simulation | `<SHA>` | <run reference> | PASS / FAIL / UNKNOWN / NOT RUN | Simulation only; not real-host evidence |
| Real two-host LAB | `<SHA>` | <approved run ID/link> | PASS / FAIL / UNKNOWN / NOT RUN | <hosts by role only; no addresses> |

Do not list unexecuted checks as PASS. Preserve failed runs and explain any rerun delta.

## Decisions, risks and blockers

- **Accepted decisions / rulings:** <decision, reason, cost if wrong>
- **Open risks:** <risk and mitigation>
- **Blocker:** <exact missing dependency/permission or `none`>
- **Protected concurrent work:** <PRs, branches, worktrees or LAB runs intentionally left untouched>

## Next safe action

1. <One concrete, non-destructive next step>

## Canonical record

Append operationally significant changes to [HQ Issue #2](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/2). This handoff summarizes the transition; it does not replace the append-only board.
