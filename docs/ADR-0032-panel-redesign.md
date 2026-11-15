# ADR-0032: Panel redesign on the existing dependency-free stack

Status: accepted (2026-11-15) by delegated owner decision. Lands after Stable 0.2.0 (target: next minor); nothing here changes the agent API, the CSP or any privilege.

## Context

The local Agent panel (`panel/`) was three static files plus i18n and charts, compiled into the binary and served under `Content-Security-Policy: default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'` (PLAN-P4-001: no build step, no CDN, works offline). It worked, but: navigation was a flat list of seven items with no grouping; the top bar had no search, notifications or breadcrumb; confirmations used `window.confirm`; monitoring, diagnostics and troubleshooting were buried inside the Tunnels card; data states (error vs unavailable vs stale vs partial) were collapsed into "null"; the 5-second refresh could replace content under the operator; and there was no written design system.

## Decision

1. **Keep the stack.** Vanilla JS + CSS served by the agent. No React, Tailwind, Radix, Vite or npm runtime dependency. Reason: the shipped panel must stay offline, CSP-strict and auditable (supply-chain surface of a node-local security tool); the owner mandate also says not to change architecture for aesthetics. shadcn/ui, 21st.dev, Motion Primitives and the other libraries were studied as **pattern sources** only (see DESIGN.md A9).
2. **Information architecture** from real capabilities only: Operate (Overview, Tunnels, Servers, Monitoring) · Set up (Create tunnel wizard) · Troubleshoot (Diagnostics, Activity) · System (Agent, Settings). Not added because the backend has nothing behind them: Users & Access (single bearer token), multi-tunnel inventory, remote node management, geography/maps.
3. **Shell**: grouped sidebar with icon-rail mode, top bar with breadcrumb, freshness, state badge, command palette (`Ctrl/⌘ K`), notifications popover (evidence-derived warnings only), theme and language; below 900 px a drawer + bottom tab bar. All overlays use native `<dialog>`.
4. **Design system** documented in `DESIGN.md` Part A and implemented in `panel/panel.css` (tokens), `panel/ui.js` (components).
5. **Honest data states**: `BAFT.load` returns per-endpoint `ok | unavailable | error`; pages render distinct empty/unavailable/error blocks; a partial-data banner names failed sources; data older than 15 s is dimmed and labelled stale; the periodic refresh never runs while an overlay is open, an action is in flight or a result is being read, and Diagnostics/Wizard/Settings do not auto-refresh.
6. **Lifecycle confirmation** moves from `window.confirm` to an in-page dialog that names the action and consequence (idempotency key, receipt and verification unchanged).
7. **No new backend surface.** Existing endpoints only. Editing an existing tunnel is stated as unsupported in the UI (the agent never applies a config from a PUT).

## Preserved (verified by QA scripts, see `docs/PANEL_REDESIGN_PARITY.md`)
Wizard incl. BAFT Direct guided setup, panel pairing, tcp-tls CSR/certificate upload, Reverse shown as Preview/disabled per catalog, preflight/plan/apply receipts; 24 h history + alerts; diagnostics tiers with next action; integrity probe (`/v1/verify`, integrity stays Unknown unless proven); lifecycle Start/Stop/Restart; audit with filter; ops guidance (backup/restore/update/rollback, read-only); firewall remains Unknown (ADR-0030); no CAP_NET_ADMIN anywhere; fa/en, dark/light/auto.

## Consequences
- `panel/ui.js` is a new embedded asset (`PANEL_ASSETS`); QA scripts that clicked Diagnostics on the Tunnels page now use the Diagnostics page.
- The panel is larger (~+30 KB source). Still no build step.
- Pure-DOM overlays rely on `<dialog>` (all supported browsers since 2022).
- A later move to a framework stays possible behind the same API (as PLAN-P4-001 says).

## Research record (what was actually consulted, 2026-11-15)
Reached and read: ui.shadcn.com (sidebar, command, data-table), component.gallery (index, badge, empty state), motion.dev (transitions), animejs.com (docs index), motion-primitives.com, reactbits.dev, magicui.design, ui.aceternity.com, 21st.dev, getdesign.md, github.com/VoltAgent/awesome-design-md (section list), mapcn.dev, uiverse.io, spline.design, undraw.co, 3dicons.co, openpeeps.com, httpster.net, mobbin.com (marketing page only). **Not reached:** styles.refero.design (server error), land-book.com (client error). **Mobbin search (MCP) refused: paid plan required**, so no Mobbin screens were examined. Findings that were pages of marketing or listings are not claimed as specifics.
