# React local Agent panel: implementation and verification

Status: **Draft, preview beside the classic panel.** Classic panel stays the default at `/`; the React panel is served at `/next/`. Same API, same bearer token (shared `sessionStorage`), same CSP. No Rust engine, API or auth change.

Decision context: React for the local panel is recorded in `docs/ADR-0053-react-local-agent-panel.md` (added here; supersedes the ADR-0051-numbered file in #344, because ADR-0051 is the merged Fleet add-server design).

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
node scripts/layout.cjs BASE_URL TOKEN [OUTDIR]   # layout audit against a running agent (see below)
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
| Setup | full (except bundle pairing) | Automatic pairing (ADR-0050): Iran/abroad first, one-time code, `/v1/pair/{offer,accept,status,approve}`, approval with key fingerprint, auto-start via `/v1/tunnel/lifecycle`. Manual/advanced wizard: real `/v1/setup/preflight`, `/v1/setup/plan` and confirmed `/v1/setup/apply` (stage `init`) with receipt. The older file/bundle pairing (`/v1/setup/bundle*`) stays classic-only. |
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

## Layout audit (added after owner feedback on sizing and scrolling)

`panel-react/scripts/layout.cjs` runs 9 routes x 9 viewports (1440, 1024, 1280x600, tablet 820 and 1024x768 landscape, phones 390, 360x640, 320, 844x390 landscape) x fa/en = 162 pages, with realistic mocked `/v1` data (300 samples, 40 audit rows, long identifiers, a full diagnostics report). It fails on: page-level horizontal scroll, any element outside the viewport, any nested scroll container (the page is the only scroller), last content hidden behind the bottom tab bar, sticky top bar not pinned, a stuck scroll lock, and touch form fields under 16px. Result on the current build: 0 findings in 162 pages. The first run on landscape phones did find content hidden behind the tab bar; fixed.

Fixes made: safe-area insets and `viewport-fit=cover`; `100vh` then `100dvh`; 16px touch inputs (stops iOS zooming the page on focus); compact sidebar on short windows; phone top bar no longer wraps or collides (language and theme live in Settings on phones); tables stay real tables down to 700px and become labelled cards below; content max width 1480px.

Style: "liquid glass" cards, buttons, bars and dialogs (translucent, blurred, edge-lit). It falls back to solid surfaces without `backdrop-filter` or with `prefers-reduced-transparency`.

## Not verified / limitations

- The layout audit uses desktop Chromium with device emulation, not a real iPhone/Android browser; real-device scrolling (address-bar collapse, rubber-banding, backdrop blur cost on low-end phones) is unchecked.

- No LAB, no two-host run, no real tunnel traffic: Tunnels/Monitoring were checked only against an idle local agent (state Unknown, no samples), so populated charts and the Reverse path are untested here.
- Lifecycle actions and the integrity probe were not executed (they act on a service unit).
- Manual screen reader and real-phone checks not done; no automated axe run.
- Bundle (file) pairing and the Fleet panel remain classic/not yet built: Fleet has no Agent API yet (ADR-0051 F1 transport). Update/rollback is guidance only in both panels (the Agent is unprivileged; `scripts/update.sh` and `scripts/rollback.sh` run as root). Apply and automatic pairing were exercised only against the layout/regression suite here; a real two-host run is still LAB work.
- Committed `dist/` must be rebuilt by hand; a CI freshness check is a follow-up.
- #344 still carries the old ADR-0051 file name; this PR supersedes it.

## Default switch (owner decision 2026-11-18)

- `/` now serves the React panel (also at `/next/`); the classic panel stays at `/classic` for one release as a fallback, then is removed.
- New Fleet page: `GET /v1/fleet/nodes` (hub state, per-node freshness/version/update, instances). Stale or never-reported nodes show Unknown, never a last value; an empty list reads "none enrolled (unknown, not zero)". Add server (Path A) reuses the automatic pairing flow; SSH enrollment (Path B) is not offered.
- Evidence so far: local build, `cargo test panel_`, regress and 180-page layout audit against a local agent. Not yet: CI on the exact SHA, independent review, two-host LAB smoke.
