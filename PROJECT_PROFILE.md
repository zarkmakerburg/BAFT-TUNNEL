# Project Profile — BAFT-TUNNEL (HQ binding)

Binds the reusable [Project HQ](https://github.com/zarkmakerburg/project-hq) playbook to this repository. A profile binds context; it grants no permissions. Usage guide: [docs/HQ_USAGE.md](docs/HQ_USAGE.md).

```yaml
project:
  id: "baft-tunnel"
  name: "BAFT-TUNNEL"
  repository: "https://github.com/zarkmakerburg/BAFT-TUNNEL"
  default_branch: "main"
  # Declared in Issue #2 itself ("This board belongs only to BAFT-TUNNEL").
  # NOT project-hq Issue #1 (that is HQ's own board).
  canonical_operations_board: "https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/2"
  # Owner approved docs/PRODUCT_ROADMAP.md on 2026-11-07 (PR #18); PROJECT_BOARD.md calls it the single reference for phases and priorities.
  roadmap: "docs/PRODUCT_ROADMAP.md"

continuation:
  commands:
    ".": "Continue the current task from its latest verified state; perform the next safe, authorized action; if no task is active, follow the .. behavior."
    "..": "Reinspect open tasks on Issue #2 (and its latest comments), prioritize by dependencies, risks and acceptance gates, then execute the highest-priority safe task."
    "...": "Reassess position in docs/PRODUCT_ROADMAP.md, reclassify open tasks against its phases and dependencies, then start the highest-priority safe task. The roadmap is not the board, and the board is not the roadmap."
  behavior: "Do not replace execution with a status-only recap. All commands remain subordinate to security and approval gates."

environments:
  registered_lab_hosts: "Two hosts (Iran, abroad) held as secrets/variables in the private GitHub Environment `baft-tunnel-lab`. No addresses or credentials appear in this repository."
  production_hosts: []   # none registered; anything unidentified is production / out of scope
  access_procedure: "docs/LAB_AGENT_ACCESS.md (one real-host workflow at a time; announce on Issue #2 before a run)"

permissions:
  merge: "EXPLICIT_GATE — per docs/MERGE_POLICY.md (PR, required checks on exact head SHA)"
  release: "EXPLICIT_GATE — docs/RELEASE.md; signing only from a v* tag"
  deploy: "EXPLICIT_GATE"
  real_host_operations: "TASK_SPECIFIC_OWNER_AUTHORIZATION_REQUIRED"
  note: "Standing delegations by the owner apply only within the conditions recorded on Issue #2 or stated by the owner in the thread. HQ ON never grants any of the above."

rules:
  - "Never copy credentials or secret values into this profile."
  - "Unlisted or unidentified hosts are production / out of scope."
  - "BAFT-TUNNEL is independent of BAFT. Rust is the core language; Go must not enter the core (the BAFT engine is a separate pinned process)."
  - "CAP_NET_ADMIN is not accepted in any component (ADR-0030); firewall status stays Unknown."
  - "Reverse and BAFT are Preview; no Stable / production-ready claim without the matching acceptance evidence."
  - "Architecture and acceptance criteria stay in this repository (docs/)."
```
