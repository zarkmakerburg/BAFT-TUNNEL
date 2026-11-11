# BAFT-TUNNEL Agent Entry Point

This repository is the independent, owner-controlled **BAFT-TUNNEL** project. Keep it independent from BAFT: transfer only explicitly accepted lessons and requirements; do not import BAFT source, history, credentials, deployment state, or secrets.

## Required start-of-task checks

1. Read [`docs/HQ/README.md`](docs/HQ/README.md), then the task-relevant HQ policy.
2. Inspect live GitHub state: repository visibility/default branch, current branch and exact HEAD, relevant Issues/PRs, CI status, and the canonical [HQ Issue #2](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/2).
3. Inspect the actual checkout/worktree and `git status`; identify ownership of any existing branch, uncommitted changes, stashes, and concurrent work before editing.
4. Read [`docs/HQ/DESIGN.md`](docs/HQ/DESIGN.md) and the relevant roadmap/architecture decision when the task touches their scope. Resolve conflicts in favor of accepted decisions and record the ruling.
5. Define acceptance criteria and evidence before implementation. Use the relevant installed engineering skill (planning, TDD, debugging, verification, worktree, or review) rather than loading unrelated skills.

## Operating constraints

- GitHub is authoritative for code, branches, commits, PRs and CI. Issue #2 is the single operational/evidence board for BAFT-TUNNEL; append dated facts, do not silently rewrite history or create a competing board.
- Preserve all existing branches, worktrees, stashes, PRs and active LAB runs. Work in an isolated branch/worktree. Never reset another worker's changes, force-push, or delete work without explicit authorization.
- Do not publish secrets, tokens, private keys, credential values, host addresses, or sensitive operational details. Secret variable names may be referenced when necessary; never include values.
- Distinguish local tests, hosted CI, simulation and real two-host LAB. A result is PASS only when tied to the exact tested SHA and observed run/evidence. Missing or ambiguous evidence is UNKNOWN / NOT VERIFIED.
- Do not mutate production or persistent LAB hosts as part of ordinary development. Use only reviewed, scoped LAB procedures.
- Merge, release, signing, publish, deploy and phase transitions are separate gates. A green test suite or open PR does not itself authorize them.
- Do not claim an action, test, review or tool connection succeeded without observing its result.

## Continuation shorthand

In BAFT-TUNNEL conversations, `.` or `..` means continue from the latest verified project state and perform the next safe, authorized engineering action. Do not replace execution with a status recap. If blocked by a real permission, security or irreversible-action gate, report the exact blocker.

For role definitions, task lifecycle, tool selection, commands and handoff formats, follow [`docs/HQ/README.md`](docs/HQ/README.md).
