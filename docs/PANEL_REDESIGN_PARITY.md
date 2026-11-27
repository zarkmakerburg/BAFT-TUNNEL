# Panel redesign: capability parity matrix

Baseline: `d47171a` (main). Classification: **Preserve** (unchanged behaviour), **Improve** (same function, better UX), **Migrate** (moved), **Replaced** (same need, different flow), **Partial** (only part of the capability), **Not migrated** (classic panel only), **Remove** (none). Evidence column names the check that exercised it; "real agent" = `baft-tunnel agent` running locally with a loopback noise-tcp tunnel (not a two-host LAB).

Rows marked *(reconciled)* were checked against `panel-react/src` on 2026-11-27 by **reading the source, not by running the panel**. Every other row keeps the evidence label it had. Where the code and an older claim disagreed, the code is recorded and the older claim is dropped.

**Merged:** the closures below landed on `main` in [PR #480](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/480) (merge `678c099a`); its exact-head CI on `1ccbca42` was green on all fifteen check runs, including `panel-react`, `panel-ui` and the axe job. Merging changes no evidence label in the older rows: they were not re-run in a browser, and the rows still marked NOT VERIFIED stay that way.

| # | Capability (old location → new) | Class | Backend | Evidence |
|---|---|---|---|---|
| 1 | Token sign-in, sessionStorage only, 401 → sign-out | Improve (empty/offline messages) | `GET /v1/status` | p1-real-agent flow logic unchanged; screenshots QA |
| 2 | Overview KPIs, health strip, latency, traffic, warnings, recent ops | Improve *(reconciled: plus topology, link table, geo map)* | status, history, traffic, security, audit | screenshots QA, p1-ops (mock) |
| 3 | 24 h history + alerts + diagnostics runs (Overview → Monitoring) | Migrate/Improve (range switch: last hour / 24 h, only real ranges) | `/v1/history` | screenshots QA |
| 4 | Servers: this node, capabilities, trust (pin/CA/cert/CRL), peer Unknown | Preserve | status, security, engines | screenshots QA |
| 5 | Tunnel state tiers, peer, data path, integrity, last failure (Tunnels) | Preserve | status | p1-ops, screenshots QA |
| 6 | Diagnostics run + tier table + next action (Tunnels → Diagnostics) | Migrate | `POST/GET /v1/diagnostics` | p1-ops (mock) |
| 7 | Integrity probe (Tunnels → Diagnostics); never shown healthy unless sha256 match | Migrate | `POST /v1/verify` | NOT VERIFIED in browser on a v2 pair (needs protocol v2 two-node); *(reconciled)* a pass is shown only for `sha256_match === true` |
| 8 | Lifecycle Start/Stop/Restart with receipt (confirm dialog replaces `window.confirm`) | Improve | `POST /v1/tunnel/lifecycle` | NOT VERIFIED (agent had no `--instance`; control shows "unavailable"). Contract test in `cargo test agent::` |
| 9 | Create-tunnel wizard: 3 steps (install node, connect, create tunnel) with Direct/Reverse-preview, engines, preflight, plan | Improve *(reconciled: condensed from the classic 6-step flow; same planner endpoints)* | engines, setup/preflight, setup/plan, setup/apply | browser-flows, wizard QA (earlier build) |
| 10 | BAFT Direct guided setup (B4) | Partial *(reconciled: only `guided_setup.available` filtering and the engine row; the guided steps themselves are not rendered)* | engines (`guided_setup`) | wizard-baft: NOT RUN here (local host has no engine store; BAFT correctly shows not selectable) |
| 11 | Panel pairing | Replaced *(reconciled: automatic pairing, ADR-0050; the classic bundle init/export/inspect/join flow is absent from the new panel and its own copy still sends applying/pairing to the classic panel)* | pair/offer, pair/accept, pair/status, pair/approve, setup/apply | pairing QA needs setup-mode root helper: NOT RUN |
| 12 | tcp-tls CSR create/recover/download, PEM upload, install | Not migrated *(reconciled: no `pki/*` call exists in `panel-react/src`; classic panel only)* | pki/csr/*, pki/certificate/install | read from code 2026-11-27 |
| 13 | Activity (audit) table + result filter | Improve *(reconciled: also free-text search over op/target/receipt fields)* | `/v1/audit` | screenshots QA |
| 14 | Agent health check + capabilities + ops guidance (read-only) | Preserve *(reconciled: the capability list shows only what the Agent reports — audit and desired state; plan/apply are wizard steps, not reported capabilities)* | `/v1/health` | p1-ops (mock) |
| 15 | Settings: language, theme, version, sign-out | Preserve (+ sidebar mode, support bundle) | none, `GET /v1/support` | language-theme-unknown |
| 16 | Setup-mode gating (only wizard + settings while pairing pending) | Preserve | status `setup_pending` | pki-enrollment (setup mode) |
| 17 | axe accessibility audit incl. dark-theme contrast (B6) | Preserve | CI | a11y-axe |
| New | Command palette, notifications, breadcrumb, drawer, icon rail, skeletons, partial/stale/error states; Fleet page; geo map; topology/crest dashboard; policy card; automatic pairing | New | `fleet`, `geo`; the policy card reads the ADR-0047 object inside `status` (no endpoint of its own) | screenshots QA |
| Removed | none | | | |

Deliberately not built: users/access management, tunnel editing, multi-tunnel list, per-peer observed health, historical audit search. GeoIP auto-detection (ADR-0052) is not built, but the map and its manual country pickers are, and the map loads no images from the network. Not migrated from the classic panel: CSR/certificate enrollment and the manual pairing bundle.

## Independent review follow-up (commit after 1d6140e)

An independent read-only review of 1d6140e found 15 items. The fixes it received at the time are recorded here as they were (integrity probe badge kind, invented "not listening" warning, render-sequence stranding on skeleton, refresh not replacing the page during an in-flight operation, render errors shown as an alert, physical key codes for shortcuts, LTR chart value, logout clearing wizard data). Two of those wordings were imprecise and are corrected here: the refresh guard is `hold()` / `__baftHold` plus tab visibility (`app.tsx`), not a focus check; and the stale-state handling is described in the table below, because the dim rule previously survived the review.

### Closed on 2026-11-27 (`panel-react`)

| Item | Fix (root cause) | Check that fails without it |
|---|---|---|
| Hardcoded `plan`/`apply` capability pills | `CapPill state={null}` invented a capability the Agent never reports. The two rows are gone from `pages/Agent.tsx` and `pages/Servers.tsx`; both cards now say plan and apply run from the create-tunnel wizard and are not reported capabilities (`cap.setup.note`, fa/en). | `panel-defects.test.mjs` rejects `CapPill state={null}` and requires the note. |
| `?? 0` in three places | `pages/parts.tsx` and `pages/Monitoring.tsx` sum through `totalOrNull` (`series.mjs`): a missing side makes the point absent so the chart breaks the line instead of plotting 0. `pages/Servers.tsx` shows "unknown" instead of 0% availability when the median has no availability beside it. | Unit test on `totalOrNull` (a measured 0 stays 0, a missing side stays null) plus source assertions on all three files. |
| 24 h "no samples" for unavailable history | `Monitoring` asks `longHistoryNotice` (`history-view.mjs`) before choosing the message: unavailable or missing history says `ui.unavail.history`; only an `ok` source with zero samples says "no samples". | Unit test over all four states plus a call-site assertion. |
| Health strip colour-only | `StatusStrip` sizes a failed cell shorter through `cellGeometry` (`health-strip.mjs`) and captions the strip with ✓ up / ✕ down (`strip.up`, `strip.down`), so up/down has a shape and a text key, not only a colour. | Unit test that the failed cell's `y > 0` and height is smaller, plus assertions that the component uses the helper and names both states. |
| Map labels ≈7 px on a narrow card | The names were text inside a viewBox-scaled SVG, so their size followed the zoom. They now live in a normal-flow `figcaption.geo-pins` sized with `--fs-1`; `.pin-label` and `fontSize={viewW / 46}` are gone, and the dots keep their `<title>` for hover. | Assertions that the component no longer sizes text by `viewW`, that the caption exists, and that the CSS rule is gone. |
| `.flip` unused / rail icon not mirrored | `Icon` now accepts `className`, and the sidebar collapse button passes `flip`, so the `[dir=rtl] .flip` rule that already existed finally applies to the `panel` glyph. | Assertion that the button opts in and the CSS rule exists. |
| Stale dim contradicts the doc | Chose **no dim**: `.shell.is-stale .view{opacity:.82}` and the now-dead `is-stale` class are removed, and staleness is shown as text — a `Pill` with `t('stale')` beside the "updated …" line — so it does not rely on colour or on an opacity that can push text below AA. The doc now says what the code does. | Assertions that neither the stylesheet nor the shell contains `is-stale` and that the stale pill is rendered. |

Each of the seven was reverse-mutated: the old code or rule was put back, its check failed (three unit assertions and four source/CSS assertions), and the fix was restored.

### Still open

Nothing from that list is actionable any more. The panel still has no automated browser/E2E pass of its own here: rows 7, 8, 10, 11 above stay **NOT VERIFIED** or **PARTIAL** for that reason, and screenshot/axe evidence in this document still refers to the earlier build (axe remains wired in CI via `scripts/panel-qa/a11y-axe.js` in `phase1-rust.yml`). Everything closed on 2026-11-27 is verified by the panel's typecheck and unit/source tests, not by looking at a rendered page.
