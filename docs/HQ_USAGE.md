# Using HQ in BAFT-TUNNEL

Guide for agents and new threads. The generic rules live in [project-hq](https://github.com/zarkmakerburg/project-hq) (`AGENTS.md`, `CORE/command-protocol.md`, `CORE/evidence-policy.md`); this file only adds what is specific to BAFT-TUNNEL and does not override HQ. Binding: [PROJECT_PROFILE.md](../PROJECT_PROFILE.md).

## 1. Two repositories, two roles

| | `zarkmakerburg/BAFT-TUNNEL` (this repo) | `zarkmakerburg/project-hq` |
|---|---|---|
| What it is | The product | The generic HQ method (no project data) |
| Owns | Code, PRs, commits/SHAs, CI and workflow runs, real two-host LAB evidence, tests/failures/limits, ADRs and security decisions, RC/release state, acceptance | Command semantics, evidence policy, templates, integration guide |
| Project coordination | **Issue #2** (canonical board: operations, decisions, evidence, handoffs, blockers) and `docs/PRODUCT_ROADMAP.md` | Not used for BAFT-TUNNEL. HQ's own Issue #1 is HQ's board only |

Rules: keep one source of truth. Record a fact where it is owned and link to it by PR / SHA / run ID elsewhere; do not copy technical detail into both places. HQ's isolation rule forbids project identifiers, hosts and operational state in `project-hq`, so BAFT-TUNNEL's profile, board and this guide live here.

## 2. Commands

Conversational shorthands, not executables; state does not carry to a new chat. Bind exactly one project (this one) and read `AGENTS.md`, the profile and the latest comments on Issue #2 first.

| Command | Meaning here |
|---|---|
| `HQ ON` | Activate HQ in the current thread: load the HQ playbook and this profile, check live state (main SHA, open PRs, CI, latest Issue #2 comments), then continue authorized work. Grants nothing. |
| `HQ OFF` | Stop HQ-directed operations in this thread. Does not undo earlier actions. |
| `HQ STATUS` | Report: active or not, bound project, last verified progress (with SHA/run), blockers. Read-only. |
| `.` | Continue the current task from its latest verified state; if none, behave as `..`. |
| `..` | Re-read open tasks on Issue #2, prioritize by dependency/risk/gate, run the top safe one. |
| `...` | Reassess position in `docs/PRODUCT_ROADMAP.md`, reprioritize, run the top safe one. The roadmap is a separate document from the board; never treat one as the other. If the roadmap file is missing or unreadable, say so and fall back to `..`. |

Persian: `HQ ON` فعال‌سازی در همین Thread · `HQ OFF` توقف (بدون برگرداندن کارهای قبلی) · `HQ STATUS` گزارش وضعیت · `.` ادامهٔ کار جاری · `..` اولویت‌بندی وظایف باز بُرد و اجرای مهم‌ترین · `...` ارزیابی مجدد در Roadmap.

Commands never authorize merge, tag, release, signing, deploy, repo-protection changes or host changes; each is a separate gate (profile `permissions`).

## 3. Evidence rules

Every important report states, as far as possible: date and scope; branch and exact SHA; PR/Issue number; workflow run and job ID; result **PASS / FAIL / UNKNOWN (NOT VERIFIED)**; evidence level; limits, risks, next action.

- Label the level separately: CI (hosted), local test, mock, simulation, real two-host LAB. CI green is not LAB proof.
- A successful rerun does not erase an earlier failure; list both and what changed.
- Missing, stale or ambiguous evidence is UNKNOWN, never PASS.
- Append dated entries to Issue #2 (format in its section 9); do not rewrite history. Link evidence, never paste secrets, host addresses or keys.

## 4. Safety

- Work only on the explicitly bound repositories; never import other projects' hosts, secrets or state.
- Unregistered hosts are out of scope. One real-host LAB workflow at a time; announce on Issue #2 first.
- Preserve other agents' branches, worktrees and stashes; check for concurrent edits; no destructive reset, no force-push.
- Never claim a result without live evidence. Enabling HQ is never approval for a risky action.

## 5. BAFT-TUNNEL-specific constraints

- BAFT-TUNNEL is independent of BAFT. Rust is the core; Go does not enter the core (the BAFT engine is a separate pinned process).
- `CAP_NET_ADMIN` is not accepted in any component; use a safer design (see ADR-0030). Firewall status stays Unknown.
- Reverse is not public or production-ready until a real implementation passes valid two-host acceptance (`docs/REVERSE_ACCEPTANCE.md`); BAFT is a separate Preview product.
- Record real-LAB results and their limits honestly. Integrity needs protocol v2 on both nodes; with default v1 it shows Unknown.
- No claim of Stable or production readiness without the matching acceptance evidence. Read live status from Issue #2 and `docs/PRODUCT_ROADMAP.md`; this guide holds no current status.

Example of time-stamped reporting (illustrative only): "2026-11-15 — `main` at `<sha>`: CI PASS (hosted). Real LAB: UNKNOWN for this SHA. Blocker: none recorded."

## 6. Starting a new thread

1. `HQ ON`. 2. Read `AGENTS.md` → profile → this file → latest Issue #2 comments. 3. State the task, its gate and the evidence you will produce. 4. Do only that; report SHA/PR/run and UNKNOWNs; log on Issue #2.
