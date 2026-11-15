# Panel redesign: capability parity matrix

Baseline: `d47171a` (main). Classification: **Preserve** (unchanged behaviour), **Improve** (same function, better UX), **Migrate** (moved), **Remove** (none). Evidence column names the check that exercised it; "real agent" = `baft-tunnel agent` running locally with a loopback noise-tcp tunnel (not a two-host LAB).

| # | Capability (old location → new) | Class | Backend | Evidence |
|---|---|---|---|---|
| 1 | Token sign-in, sessionStorage only, 401 → sign-out | Improve (empty/offline messages) | `GET /v1/status` | p1-real-agent flow logic unchanged; screenshots QA |
| 2 | Overview KPIs, health strip, latency, traffic, warnings, recent ops | Improve | status, history, traffic, security, audit | screenshots QA, p1-ops (mock) |
| 3 | 24 h history + alerts + diagnostics runs (Overview → Monitoring) | Migrate/Improve (range switch: last hour / 24 h, only real ranges) | `/v1/history` | screenshots QA |
| 4 | Servers: this node, capabilities, trust (pin/CA/cert/CRL), peer Unknown | Preserve | status, security, engines | screenshots QA |
| 5 | Tunnel state tiers, peer, data path, integrity, last failure (Tunnels) | Preserve | status | p1-ops, screenshots QA |
| 6 | Diagnostics run + tier table + next action (Tunnels → Diagnostics) | Migrate | `POST/GET /v1/diagnostics` | p1-ops (mock) |
| 7 | Integrity probe (Tunnels → Diagnostics); never shown healthy unless sha256 match | Migrate | `POST /v1/verify` | NOT VERIFIED in browser on a v2 pair (needs protocol v2 two-node) |
| 8 | Lifecycle Start/Stop/Restart with receipt (confirm dialog replaces `window.confirm`) | Improve | `POST /v1/tunnel/lifecycle` | NOT VERIFIED (agent had no `--instance`; control shows "unavailable"). Contract test in `cargo test agent::` |
| 9 | Create-tunnel wizard (6 steps, Direct/Reverse-preview, engines, protocol, preflight, plan) | Preserve (restyled; same code) | engines, setup/preflight, setup/plan | browser-flows, wizard QA |
| 10 | BAFT Direct guided setup (B4) | Preserve | engines (`guided_setup`) | wizard-baft: NOT RUN here (local host has no engine store; BAFT correctly shows not selectable) |
| 11 | Panel pairing (init, export bundle, inspect, join, reload) | Preserve | setup/apply, setup/bundle(/inspect) | pairing QA needs setup-mode root helper: NOT RUN |
| 12 | tcp-tls CSR create/recover/download, PEM upload (3 files, key refused), install | Preserve | pki/csr/*, pki/certificate/install | pki-enrollment (mock), 2 langs |
| 13 | Activity (audit) table + result filter | Preserve | `/v1/audit` | screenshots QA |
| 14 | Agent health check + capabilities + ops guidance (read-only) | Preserve | `/v1/health` | p1-ops (mock) |
| 15 | Settings: language, theme, version, sign-out | Preserve (+ sidebar mode) | none | language-theme-unknown |
| 16 | Setup-mode gating (only wizard + settings while pairing pending) | Preserve | status `setup_pending` | pki-enrollment (setup mode) |
| 17 | axe accessibility audit incl. dark-theme contrast (B6) | Preserve | CI | a11y-axe |
| New | Command palette, notifications, breadcrumb, drawer, icon rail, skeletons, partial/stale/error states | New (no backend change) | none | screenshots QA |
| Removed | none | | | |

Deliberately not built (no backend): users/access management, tunnel editing, multi-tunnel list, peer health, maps, historical audit search.

## Independent review follow-up (commit after 1d6140e)

An independent read-only review of 1d6140e found 15 items. Fixed in code: integrity probe badge kind, invented "not listening" warning when status is unknown, render-sequence stranding on skeleton, periodic refresh no longer replaces the page while an interactive control inside it has focus, render errors shown as an alert instead of an endless skeleton, shortcuts use physical key codes (work on Persian layouts), chart legend kept LTR, stale state shown as a topbar warning line instead of dimming below AA, logout clears wizard/data. Still OPEN (not fixed in this PR, listed honestly): hardcoded capability pills on Agent/Servers, `|| 0` defaults in some KPIs, "no samples" shown for unavailable history, chart legend colour-only up/down, 10px SVG text on narrow cards, drawer aria-current and RTL border/rail icon, toast live region created lazily, language switch rebuilds shell. Screenshot QA and axe results in this document refer to the earlier build unless re-run; re-run status is stated in the PR.
