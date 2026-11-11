# Task: <short title>

- **Task ID:** <issue / plan / task identifier>
- **Project:** <project name / stable ID>
- **State:** proposed | ready | active | blocked | review | independent-review | accepted
- **Owner / role:** <worker or HQ role>
- **Created / updated (UTC):** <YYYY-MM-DD HH:MM>
- **Canonical board:** the project's canonical board defined in docs/HQ/PROJECT_PROFILE.md

## Goal and scope

<One outcome stated in user-visible terms.>

### In scope

- <Deliverable>

### Out of scope

- <Explicit exclusions and protected systems/PRs>

## Baseline and ownership

- **Base SHA:** `<full SHA>`
- **Branch:** `<task branch>`
- **Workspace:** `<isolated worktree/clone path; no credentials or host addresses>`
- **Related PRs / Issues / ADRs:** <canonical links>
- **Concurrent work checked:** <branches, worktrees, stashes, PRs and LAB runs inspected>

## Acceptance criteria

- [ ] <Observable behavior or artifact>
- [ ] <Required regression/compatibility condition>
- [ ] <Security / operational boundary>

## Risks and dependencies

- **Dependencies:** <owner and expected evidence>
- **Risks:** <impact and mitigation>
- **Blocked if:** <precise condition requiring escalation>

## Validation plan

| Check | Exact command / workflow | Expected evidence | Result |
|---|---|---|---|
| Local | `<command>` | Exit code and output tied to SHA | NOT RUN |
| Hosted CI | <workflow and link> | Exact-head run and required jobs | UNKNOWN |
| Simulation | <if applicable> | Clearly labelled simulation result | NOT RUN |
| Real LAB | <if applicable> | Approved run ID, two-host evidence, cleanup result | NOT RUN |

## Delivery

- **Commits:** <full SHAs and links>
- **PR:** <URL or none>
- **QA/Integrator:** <owner and disposition>
- **Blockers / deferred items:** <facts only>
- **Next safe action:** <one concrete step>
