# Engineering Skill Matrix

Skills are selected per task, not loaded globally. Start with the smallest workflow that addresses the work. Read a skill's current instructions before invoking the associated workflow or tool.

| Task type | Required / recommended skill | When to load | Expected output |
|---|---|---|---|
| Any new project session governed by HQ | `superpowers:using-superpowers` | At session start, before project work | Relevant skill discovery and workflow discipline |
| Product/architecture exploration | `superpowers:brainstorming` | Before proposing new behavior, architecture or UX | Agreed problem, constraints, options and decision record |
| Multi-step implementation | `superpowers:writing-plans` | Before implementation when the work spans multiple tasks | Reviewed plan with task boundaries, interfaces and verification |
| Execute an approved plan inline | `superpowers:executing-plans` | When the owner chooses inline execution | Task-by-task implementation, ledger and final review |
| Execute independent tasks with workers | `superpowers:subagent-driven-development` | When independent workers/reviewers are available and selected | Isolated task execution, per-task review and integration |
| Feature, bug fix or behavior change | `superpowers:test-driven-development` | Before implementation code | RED → GREEN → REFACTOR with regression coverage |
| Unexpected failure or defect | `superpowers:systematic-debugging` | Before changing code to address a failure | Evidence-based root cause and verified fix |
| Before claiming completion | `superpowers:verification-before-completion` | Before reporting PASS, completion or readiness | Fresh command/output verification against acceptance criteria |
| Isolated parallel development | `superpowers:using-git-worktrees` | Before implementation that needs isolation | Verified isolated branch/worktree without disturbing other work |
| Request an independent review | `superpowers:requesting-code-review` | Mandatory before acceptance/merge consideration for every PR changing `AGENTS.md`, `docs/HQ/**`, security policy, permissions, or delivery gates; recommended for other substantial changes | Review findings against scope and failure modes, submitted by someone other than the author and tied to the exact head SHA |
| Respond to review feedback | `superpowers:receiving-code-review` | When review comments arrive | Evidence-backed disposition and fixes |
| Finish a development branch | `superpowers:finishing-a-development-branch` | After implementation and verification are complete | Safe integration/PR options; no automatic permission to merge |
| Benchmark/performance analysis | CodSpeed integration and relevant performance guidance | When measuring a defined performance question | Exact-SHA benchmark evidence and comparison |
| Figma UI work | Relevant Figma skill (for example `figma-use`, `figma-design-to-code`, or `figma-generate-design`) | Only when the task actually reads/writes Figma or translates design/code | Design-context evidence or Figma artifact |
| API/library research | Context7 integration | When current API behavior or version-specific usage is uncertain | Version-aware documentation reference |

## Non-applicable by default

- Do not load every skill at session start. Do not load TDD for pure copy edits or static documentation that does not change executable behavior; still run document/link/security validation.
- Do not use Figma skills for ordinary code changes without a Figma design dependency.
- Do not use CodSpeed for correctness tests or claim benchmark simulation as a real-host performance result.
- Do not create Linear/Notion/deployment workflows unless the task explicitly needs them and the user-authorized integration is available.
- Skills guide execution; they do not grant credentials, account permissions, merge rights, release approval or production access.
