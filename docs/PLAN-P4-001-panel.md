# P4-001: local panel and wizard — execution plan

Status: in progress. Part of [PRODUCT_ROADMAP.md](PRODUCT_ROADMAP.md). Builds on the agent API of [PLAN-P3-002-agent-api.md](PLAN-P3-002-agent-api.md).

## Decision: dependency-free static panel, not React/Vite (for the MVP)
The roadmap named React/TS/Vite. The MVP panel is a status view plus a four-field wizard, so a Node build chain would add a large dependency tree (supply-chain audit, lockfile, CI toolchain) for no capability the MVP needs. The panel is three static files (`panel/`) compiled into the binary and served by the agent: no build step, no CDN, works offline, strict CSP (`script-src 'self'`, no inline script, no `innerHTML`). Persian (RTL) and English are built in. A framework can replace it later behind the same API; the mockup design (PR #17) stays the visual reference.

## Slices
| Slice | Content | Acceptance |
|---|---|---|
| W1 | Agent serves `/`, `/panel.js`, `/panel.css` without a token; the page asks for the API token (kept in `sessionStorage` only) and shows status and stored desired state. | Unit tests (served with CSP, no `innerHTML`); headless browser run: wrong token refused, status shown, language switch flips RTL/LTR. |
| W2 | Wizard: engine, direct/reverse, role, one service port (ids and tunnel port are advanced defaults). `POST /v1/setup/plan` runs the same validated `setup::plan` as the CLI and returns config, files, pending items and the exact `setup init`, `join`, `apply` commands. Writes nothing, runs nothing. | Token required; bad input and shell metacharacters rejected (422); nothing written; browser run shows the commands. |
| W3 | LAB: the panel is fetched from the agent on a real host and the wizard plan matches the commands used by the setup stage. | Run ID and SHA recorded. |

## Out of scope
Executing `setup init` from the browser (keys are created by the CLI on the host), remote access, multi-user login, engine switching from the panel (the agent never applies a config), React/Vite.
