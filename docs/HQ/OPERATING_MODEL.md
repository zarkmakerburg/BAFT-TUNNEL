# HQ Operating Model

## Roles and authority

### HQ / Controller

- Decomposes approved goals into scoped tasks, assigns owners, selects tools/skills and coordinates dependencies.
- Reads live repository, issue, PR, CI and LAB evidence before deciding what is next.
- Maintains the operational narrative on Issue #2 and ensures every handoff is resumable.
- Does not invent tool results, test outcomes, permissions or acceptance. It does not bypass project-owner gates.

### Worker / Implementer

- Owns only the explicitly assigned task and its isolated branch/worktree.
- Reads the task brief, accepted design/ADRs and current baseline before editing.
- Implements the smallest complete change, adds/updates tests where behavior changes, runs the required checks and commits coherent units.
- Reports exact base/head SHAs, changed paths, commands/results and any deviations. It does not merge or claim acceptance on its own.

### QA / Integrator

- Is the designated acceptance owner for a delivery; it may be a separate worker or an explicitly assigned HQ role.
- Reviews the complete diff against scope, architecture, security, compatibility and acceptance criteria.
- Independently checks exact-SHA test/CI/LAB evidence, verifies that UNKNOWN remains UNKNOWN, and records findings.
- Accepts, rejects or requests a bounded fix. Merge permission remains a separate gate.

One person/agent may perform more than one role for a small task, but implementation and acceptance responsibilities must remain explicit. A self-review is weaker than a fresh independent review and must be labelled honestly.

## Task lifecycle

| State | Meaning | Entry condition / exit evidence |
|---|---|---|
| `proposed` | Candidate work, not yet authorized or scheduled | Goal and rationale captured. |
| `ready` | Scope and acceptance are clear | Owner, base SHA, branch/worktree, dependencies, risks and test plan recorded. |
| `active` | Implementation or investigation is underway | Worker has verified the baseline and owns the isolated workspace. |
| `blocked` | A real dependency, access, security or decision prevents safe progress | Exact blocker, impact, required owner/action and last safe SHA recorded. |
| `review` | Implementation is complete and submitted for evaluation | Commit/PR plus reproducible validation evidence are available. |
| `accepted` | QA/Integrator has verified the defined acceptance criteria | Review disposition and exact accepted SHA/evidence recorded. |

A task may return from `review` to `active` for a bounded fix, or from any active state to `blocked` when evidence or authority is missing. “Merged” and “released” are delivery outcomes, not substitutes for technical acceptance.

## Isolation and ownership

1. Before editing, inspect branch, HEAD, worktree path, dirty files, stashes and open PR ownership.
2. Use a dedicated branch and isolated worktree/clone for each concurrent implementation stream. Never share a writable checkout between workers.
3. Do not reuse a stale checkout merely because its directory name matches the repository. Verify its remote, branch, upstream and exact SHA.
4. Preserve existing branches, worktrees, stashes and active PRs. No destructive reset, force-push, branch deletion or overwriting another worker's files.
5. A worker pushes only its own task branch when authorized by the task workflow. Shared branch writes, merge, release and deploy require their own explicit gates.

## Evidence and handoff contract

Every implementation handoff must include:

- Task identifier and concise scope; owner/role.
- Base SHA, head SHA, branch and changed paths.
- Acceptance criteria and each check's exact command, observed result and limitations.
- Hosted CI workflow/run links and real LAB run identifiers where applicable.
- Review disposition, unresolved risks, blockers and the next safe action.

Evidence types are not interchangeable: local test, static check, hosted CI, simulation and real two-host LAB must be labelled separately. Do not infer real network behavior from mocks or simulation. Do not report a test as passing unless it was actually executed and its output inspected.

## Approval and delivery gates

- **Implementation gate:** scoped task, owner, isolated workspace and acceptance criteria exist.
- **Technical acceptance gate:** QA/Integrator reviews the diff and exact-SHA evidence.
- **Merge gate:** required checks/reviews are green and repository rules are satisfied; merge is separately authorized.
- **Release/signing gate:** artifact provenance, signing identity, reproducibility and release criteria are independently verified.
- **Deploy/phase gate:** deployment target, rollback, operational impact and phase acceptance are reviewed and authorized.

Passing an earlier gate never implies approval for a later one. This document does not authorize production changes, LAB host mutation, merge, release or deployment.

## Operational record

Use BAFT-TUNNEL [HQ Issue #2](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/2) as the append-only operational record. Add dated entries for decisions, state changes, test failures, approvals, risks and handoffs. Link exact GitHub artifacts. Keep policy in `docs/HQ/`; keep live status and history in Issue #2. Do not create parallel boards or duplicate a changing status snapshot in static docs.
