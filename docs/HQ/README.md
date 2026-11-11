# HQ Command Center

Reusable, repository-native operating methodology for coordinating humans, ChatGPT sessions, AI agents and connected tools. It is not part of the product's runtime or architecture and does not grant permissions that connected systems or the project owner have not granted. This repository is its first project-specific deployment; future projects should reuse the methodology with their own profile, board, evidence and access boundaries.

## Canonical sources of truth

| Information | Authoritative source | Rule |
|---|---|---|
| Code, branches, commits, PRs and CI | Live GitHub repository and exact commit/run | Inspect current state; never rely on stale chat summaries. |
| Operations, decisions, blockers, evidence and handoffs | the canonical board declared in [PROJECT_PROFILE.md](PROJECT_PROFILE.md) | Single project board; append dated entries. |
| Product phases, priorities and next product work | [`../PRODUCT_ROADMAP.md`](../PRODUCT_ROADMAP.md), with the current snapshot in [`../PROJECT_BOARD.md`](../PROJECT_BOARD.md) | Roadmap sequences product work; project board is an entry summary, not a second operations log. |
| Architecture and accepted design constraints | Relevant ADRs and [`DESIGN.md`](DESIGN.md) | Accepted decisions outrank proposals and stale plans. |
| Roles, workflow, tool/skill selection and command conventions | Documents in this directory | These are operating rules, not live project status. |

If sources disagree, do not silently choose whichever is convenient. Identify the conflicting facts, inspect their timestamps/SHAs, follow the accepted decision, and record a concise ruling on the project's canonical board when it changes project operations.

## Required reading order

1. Root [AGENTS.md](../../AGENTS.md) — this repository's entry point and guardrails.
2. This README — reusable methodology, source-of-truth hierarchy and navigation.
3. [PROJECT_PROFILE.md](PROJECT_PROFILE.md) — project-specific repository, board, environment and policy bindings.
4. [OPERATING_MODEL.md](OPERATING_MODEL.md) — roles, task states, isolation and gates.
5. The task-relevant sections of [TOOLCHAIN.md](TOOLCHAIN.md), [SKILL-MATRIX.md](SKILL-MATRIX.md), and [COMMANDS.md](COMMANDS.md).
6. [DESIGN.md](DESIGN.md), the relevant ADR, roadmap entry and live Issue/PR evidence for the task.

## Operating documents

- [`OPERATING_MODEL.md`](OPERATING_MODEL.md): HQ, Worker and QA/Integrator responsibilities; lifecycle and approval boundaries.
- [`TOOLCHAIN.md`](TOOLCHAIN.md): connected execution tools, evidence expectations and limitations.
- [`SKILL-MATRIX.md`](SKILL-MATRIX.md): which engineering skill to load for which kind of work.
- [`COMMANDS.md`](COMMANDS.md): concise command semantics, including `.` / `..` continuation.
- [`templates/`](templates/): reusable task, handoff and evidence records.

## Reuse boundary

The HQ documents define a reusable operating method, not BAFT-TUNNEL product functionality. Reuse them in another project only with a separate project profile and project-local board, evidence, permissions, host registry and operational state. Never carry over project identifiers, credentials, host inventory, release state or approvals. See [PROJECT_PROFILE.md](PROJECT_PROFILE.md) for this repository's current binding.

## Current project state

Do not copy changing phase, release, LAB or PR status into reusable policy documents. Read the live board, project board, roadmap and GitHub checks identified by PROJECT_PROFILE.md at the start of each task.
