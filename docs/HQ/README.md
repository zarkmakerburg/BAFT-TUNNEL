# HQ Command Center

Repository-native operating guide for BAFT-TUNNEL. This is a shared contract for human operators and AI workers; it is not a runtime service and does not grant permissions that GitHub, connected tools, or the project owner have not granted.

## Canonical sources of truth

| Information | Authoritative source | Rule |
|---|---|---|
| Code, branches, commits, PRs and CI | Live GitHub repository and exact commit/run | Inspect current state; never rely on stale chat summaries. |
| Operations, decisions, blockers, evidence and handoffs | [HQ Issue #2](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/2) | Single BAFT-TUNNEL board; append dated entries. |
| Product phases, priorities and next product work | [`../PRODUCT_ROADMAP.md`](../PRODUCT_ROADMAP.md), with the current snapshot in [`../PROJECT_BOARD.md`](../PROJECT_BOARD.md) | Roadmap sequences product work; project board is an entry summary, not a second operations log. |
| Architecture and accepted design constraints | Relevant ADRs and [`DESIGN.md`](DESIGN.md) | Accepted decisions outrank proposals and stale plans. |
| Roles, workflow, tool/skill selection and command conventions | Documents in this directory | These are operating rules, not live project status. |

If sources disagree, do not silently choose whichever is convenient. Identify the conflicting facts, inspect their timestamps/SHAs, follow the accepted decision, and record a concise ruling on Issue #2 when it changes project operations.

## Required reading order

1. Root [`AGENTS.md`](../../AGENTS.md) — universal repository guardrails.
2. This README — source-of-truth hierarchy and navigation.
3. [`OPERATING_MODEL.md`](OPERATING_MODEL.md) — roles, task states, isolation and gates.
4. The task-relevant sections of [`TOOLCHAIN.md`](TOOLCHAIN.md), [`SKILL-MATRIX.md`](SKILL-MATRIX.md), and [`COMMANDS.md`](COMMANDS.md).
5. [`DESIGN.md`](DESIGN.md), the relevant ADR, roadmap entry and live Issue/PR evidence for the task.

## Operating documents

- [`OPERATING_MODEL.md`](OPERATING_MODEL.md): HQ, Worker and QA/Integrator responsibilities; lifecycle and approval boundaries.
- [`TOOLCHAIN.md`](TOOLCHAIN.md): connected execution tools, evidence expectations and limitations.
- [`SKILL-MATRIX.md`](SKILL-MATRIX.md): which engineering skill to load for which kind of work.
- [`COMMANDS.md`](COMMANDS.md): concise command semantics, including `.` / `..` continuation.
- [`templates/`](templates/): reusable task, handoff and evidence records.

## Project boundary

This HQ structure may be reused as a pattern by future projects, but each project must have its own repository context, canonical board, evidence, permissions and operational state. BAFT-TUNNEL Issue #2 is not a board for other projects. Do not mix secrets, host inventory, release state or credentials across projects.

## Current project state

Do not copy current phase, release, LAB or PR status into these policy documents. Read the live [Issue #2](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/2), [project board](../PROJECT_BOARD.md), roadmap and GitHub checks at the start of each task.
