# HQ Command Center Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Establish a lightweight, repository-native and multi-project-ready HQ operating contract for BAFT-TUNNEL without disturbing active engineering work.

**Architecture:** Keep GitHub Issues/PRs/CI as the operational evidence system and Issue #2 as the BAFT-TUNNEL command board. Add a root agent entrypoint and focused `docs/HQ/` documents for roles, tools, skills, commands and reusable handoffs; do not add a runtime service or database.

**Tech Stack:** Markdown, GitHub repository/Issues/PRs, existing connected GitHub and Mac terminal tools, existing CodSpeed and engineering skills.

**Spec:** `docs/HQ/DESIGN.md`

## Global Constraints

- Preserve the public repository visibility and all existing branches, worktrees, stashes and open PRs.
- Do not modify or take over PR #166 (CodSpeed), other active PRs, or active LAB runs.
- Do not write secrets, private keys, tokens, host addresses or secret values into repository files or logs.
- Do not claim unexecuted tests or unobserved CI/LAB outcomes as PASS.
- Do not merge, release, deploy, change branch protection, or mutate production/LAB hosts.
- Keep Issue #2 as the single BAFT-TUNNEL operations/evidence board; append operational history rather than creating a competing board.
- Use existing tools and load skills only when relevant to the task; do not install duplicate integrations.
- Keep future projects logically isolated even though the HQ document structure is reusable.

## Review Focus

- Stale or missing links: verify all relative links and referenced canonical Issue/PR entrypoints.
- Conflicting authority: verify Issue #2 remains operational source of truth and docs are policy/how-to only.
- Secret leakage: scan new files for credential-like values and host addresses; allow variable names only.
- False test claims: ensure templates distinguish local, CI, simulation and real LAB evidence.
- Scope creep: verify no app/service/database, production change, PR merge or release is introduced.

---

### Task 1: Establish the HQ entrypoint and operating model

**Files:**
- Create: `AGENTS.md`
- Create: `docs/HQ/README.md`
- Create: `docs/HQ/OPERATING_MODEL.md`
- Reference: `docs/HQ/DESIGN.md`, `docs/PROJECT_BOARD.md`, Issue #2

**Interfaces:**
- `AGENTS.md` is the short root-level instruction and points to `docs/HQ/README.md`.
- `docs/HQ/README.md` identifies the canonical project board and the required reading order.
- `OPERATING_MODEL.md` defines HQ, Worker, QA/Integrator, task lifecycle, isolation and phase gates.

- [ ] **Step 1: Write the root entrypoint** with project identity, independence boundary, mandatory initial status checks, safety constraints, and links to HQ operating documents.
- [ ] **Step 2: Write the HQ README** with source-of-truth hierarchy: GitHub state for code/PR/CI; Issue #2 for operations/evidence; roadmap for product sequencing; HQ docs for operating rules.
- [ ] **Step 3: Define the operating model** including task states (proposed, ready, active, blocked, review, accepted), worker isolation, QA ownership, handoff evidence and approval gates.
- [ ] **Step 4: Verify links and policy consistency** using a script or shell scan; confirm no existing project status is duplicated or silently rewritten.
- [ ] **Step 5: Commit** this coherent documentation unit.

### Task 2: Document toolchain, skill selection and command protocol

**Files:**
- Create: `docs/HQ/TOOLCHAIN.md`
- Create: `docs/HQ/SKILL-MATRIX.md`
- Create: `docs/HQ/COMMANDS.md`

**Interfaces:**
- Toolchain entries name the purpose, suitable task types, evidence returned and limitations of existing tools.
- Skill matrix maps task categories to relevant installed skills; it must not imply every skill is loaded for every task.
- Command protocol defines plain-text shorthand and expected behavior, without pretending that repository files automatically alter ChatGPT account settings.

- [ ] **Step 1: Document connected tools** (GitHub, Mac terminal/Remote Desktop Commander, CodSpeed, Context7, Figma and optional project tools) with safe usage and source-of-truth rules.
- [ ] **Step 2: Map skills to workflows** (design, planning, implementation/TDD, debugging, verification, worktree, PR completion, performance and UI) and identify when a skill is not applicable.
- [ ] **Step 3: Define command semantics** for continue, status, plan, implement, verify and handoff; preserve the user's BAFT-TUNNEL `.` / `..` continuation convention as a project instruction.
- [ ] **Step 4: Verify every named tool/skill is actually available or label it optional/unavailable**; do not claim plugin installation where only a connector/tool is present.
- [ ] **Step 5: Commit** this documentation unit.

### Task 3: Add reusable task, handoff and evidence templates

**Files:**
- Create: `docs/HQ/templates/TASK.md`
- Create: `docs/HQ/templates/HANDOFF.md`
- Create: `docs/HQ/templates/EVIDENCE.md`

**Interfaces:**
- Task template captures owner, scope, base SHA, branch/worktree, acceptance criteria, risks and validation.
- Handoff template captures exact current state, changed files, commit/PR, tests, blockers and next safe action.
- Evidence template captures evidence type, exact SHA, run/URL, result, limitations and timestamp; secret values are prohibited.

- [ ] **Step 1: Write the task template** with explicit scope and acceptance criteria.
- [ ] **Step 2: Write the handoff template** with concise, resumable engineering context and no duplicated status narrative.
- [ ] **Step 3: Write the evidence template** separating local tests, hosted CI, simulation and real LAB evidence.
- [ ] **Step 4: Verify templates contain no real credentials or host details and are usable as copyable Markdown.**
- [ ] **Step 5: Commit** this documentation unit.

### Task 4: Validate and prepare a reviewable PR

**Files:**
- Validate all files created in Tasks 1–3.
- Update only if required: internal links or wording in the new HQ docs.

- [ ] **Step 1: Run** `git diff --check`.
- [ ] **Step 2: Run** a Markdown relative-link validation over the new HQ documents and root `AGENTS.md`.
- [ ] **Step 3: Run** a secret/host-data scan over the changed files; manually inspect any matches to avoid both leaks and false positives.
- [ ] **Step 4: Review** the complete diff against `docs/HQ/DESIGN.md`, current `docs/PROJECT_BOARD.md`, Issue #2 and active PR state.
- [ ] **Step 5: Record exact commit SHA and validation outputs, then open a Draft PR** targeting `main`. Do not merge it.

## Execution Notes

- Work only on `docs/hq-command-center-foundation`, based on the verified main SHA used when this plan was created; refresh main and inspect divergence before opening the PR.
- Do not use the Mac checkout currently on `phase1/rust-tcp-transport`; it is not the workspace for this documentation change.
- Prefer repository-native Markdown and existing GitHub connector operations; no application code or dependency changes are expected.
- If any planned file already exists by execution time, inspect and reconcile rather than overwrite it.
