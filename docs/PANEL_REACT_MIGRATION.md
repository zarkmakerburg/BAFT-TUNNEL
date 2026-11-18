# React local Agent panel: implementation and verification

Status: **Draft, preview beside the classic panel.** Classic panel stays the default at `/`; the React panel is served at `/next/`. Same API, same bearer token (shared `sessionStorage`), same CSP. No Rust engine, API or auth change.

Decision context: React for the local panel is recorded in the ADR on PR #344 (file name `ADR-0051-react-local-agent-panel.md`). PR #341 also uses number 0051 for the Fleet add-server design, so one of the two must be renumbered before merge; this change does not touch either.

## Layout

| Path | Role |
|---|---|
| `panel-react/src/api.ts`, `app.tsx`, `derive.ts` | API client (same endpoints), session/theme/lang state, polling feed, pure state derivation |
| `panel-react/src/components/` | design system: Pill, Card, Kpi, KV, Empty, Banner, Skeleton, Segmented, Field, Tip, Dialog, Confirm, Toasts, Table, charts, icons |
| `panel-react/src/shell/` | login, sidebar/rail, top bar, drawer, tab bar, command palette, notifications |
| `panel-react/src/pages/` | the nine routes |
| `panel-react/src/styles.css` | tokens + components (Weave identity, Clarity forms/spacing, Command density) |
| `panel-react/dist/` | **committed** build output; the Agent embeds exactly these 3 files (`src/agent.rs`) |
| strings | reused from the Agent's own `/i18n.js` so both panels never drift; React-only strings live in `src/i18n.ts` |

## Build

```sh
cd panel-react
npm ci
npm run typecheck
npm run build        # writes dist/ (fixed file names, no hashes)
npm run check:dist   # no inline script/style, no remote URL
```
Customer hosts need no Node. After changing `src/`, rebuild and commit `dist/`, then `cargo build`.
Dev: run an agent on 127.0.0.1:18090, then `npm run dev` (proxies `/v1`, `/i18n.js`, `/fonts`).

## Dependencies (pinned, exact)

Runtime in the bundle: `react` 19.2.9, `react-dom` 19.2.9 (MIT; `scheduler` MIT). Build only: `vite` 7.3.8, `@vitejs/plugin-react` 5.0.0, `typescript` 5.9.2, `@types/*`. `npm audit --omit=dev`: 0 vulnerabilities. No UI framework, no CDN, no remote fonts (Vazirmatn is served by the Agent).

## Route parity

| Route | React | Notes |
|---|---|---|
| Overview | full | health strip, latency, traffic, alerts, recent ops, KPIs |
| Tunnels | full | state tiers, Direct/Reverse badge (Reverse always labelled Preview), lifecycle start/stop/restart via `/v1/tunnel/lifecycle` with confirm + receipt, sibling instances |
| Servers | full | this node, peer (never observed remotely), capabilities, trust, Fleet note |
| Monitoring | full | 1 h / 24 h real samples only |
| Setup | partial | 3-step wizard (Install node, Connect, Create tunnel): real `/v1/setup/preflight` and `/v1/setup/plan` (read-only). **Apply and panel pairing are not ported**; the wizard hands off to the classic panel |
| Diagnostics | full | run diagnostics, integrity probe, copy technical details |
| Activity | full | result filter + text search |
| Agent | full | health check, runtime, config summary, capabilities |
| Settings | full | language, theme, nav, support bundle, confirmed sign-out |

Honest-state rules kept: unknown/unavailable/error are distinct; no value without an endpoint; Reverse selectable only if `/v1/engines` lists it; "ready" never derived from a process or HTTP 200.

## Verification actually run (2026-11-18, debug agent on 127.0.0.1, `examples/forward.json`)

- `tsc` strict: clean. `vite build`: OK. `check:dist`: OK.
- `cargo test panel_`: 4 passed (CSP header, `/next` served, `/` stays classic, no `innerHTML` in React sources, bundle has no `eval`).
- Playwright, 9 routes x (desktop fa dark, desktop en light, tablet fa, mobile fa, mobile en): all rendered, no render-error banner, **no horizontal overflow**, **0 CSP violations**. Raw report: `verification-report.json`. Console 404s are endpoints this agent mode does not offer (shown as Unavailable, same as classic).
- RTL/LTR: `dir` follows language; identifiers render in `bdi dir=ltr`.
- Keyboard: skip link first, Ctrl+K palette, Enter navigates, Esc closes and restores focus, visible focus ring, confirm dialog opens and Cancel keeps the session.
- Auth: no token shows login; wrong token shows "Wrong token" and stays; right token enters; `/v1/status` without token is 401.
- States: aborted `/v1` shows the unreachable banner; `/v1/audit` 500 shows a partial-data banner plus an error empty state; loading skeleton present.

## Not verified / limitations

- No LAB, no two-host run, no real tunnel traffic: Tunnels/Monitoring were checked only against an idle local agent (state Unknown, no samples), so populated charts and the Reverse path are untested here.
- Lifecycle actions and the integrity probe were not executed (they act on a service unit).
- Manual screen reader and real-phone checks not done; no automated axe run.
- Setup Apply/pairing, Fleet panel, update/rollback UI remain classic-only.
- Committed `dist/` must be rebuilt by hand; a CI freshness check is a follow-up.
- Existing ADR-0051 number clash (above).
