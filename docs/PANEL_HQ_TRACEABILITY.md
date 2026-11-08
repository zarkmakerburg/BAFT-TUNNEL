# Panel: decision-to-implementation traceability (HQ Issue #2, binding comment 6720757885)

Baseline: `main` at `650e955` (after PR #61); starting point of this work was `15c9784` plus the unmerged dashboard draft. Architecture is unchanged: the dependency-free panel served by the agent (PR #52). No framework, no build step, no new dependency.

State column: I = implemented, P = partial, M = missing (backend gap, hidden or explained in the UI), NA = not applicable to the MVP. "Before" is the state of the panel at the baseline.

| # | Requirement (HQ section) | Source | Before | After | Evidence | Remaining / action | Pri |
|---|---|---|---|---|---|---|---|
| 1 | Graphite/Navy base, Electric Blue accent, semantic green/amber/red, tokens only (2) | `panel/panel.css` `:root` tokens | P (light default, generic blue) | I | Browser flows: dark default; light and dark both render | Light theme kept as option; no brand typeface bundled (system + Vazirmatn if installed) | P1 |
| 2 | Dashboard hierarchy: summary cards, health/activity, warnings, empty state (5A) | `pages.js` overview | M | I | Screenshots desktop/mobile; first-run banner when nothing is stored | Servers count is "this node" only (no inventory) | P1 |
| 3 | Navigation: persistent sidebar, active state, title, language control; mobile tab bar (4, 5A/B) | `app.js`, `panel.css` | P | I | Browser: `aria-current`, sidebar side flips with language; mobile tab bar + More | Firewall and Traffic pages omitted from navigation on purpose (no capability) | P1 |
| 4 | Servers: identity, role, state, freshness, version, capabilities, trust, diagnostics (5D) | `pages.js` servers; `/v1/status,/v1/security,/v1/engines` | M | P | Resource cards for this node and its peer; unknown fields use the dashed Unknown pill | No enrolled-server inventory and no remote server state in the backend (gap G1) | P1 |
| 5 | Tunnels: name, direction, endpoints, engine, state, last verified, metrics, actions (5E) | `pages.js` tunnels | P (table) | P | Resource card; reflows on mobile | Last verified, peer connection: Unknown (no end-to-end verification, gap G2). Lifecycle actions: none exist in the API, shown as an explanation, no buttons | P1 |
| 6 | Direct/Reverse first, plain-language, small topology diagram (1.7, 5C) | `pages.js` wizard step 1 | M | I | Browser: direction required before continuing | Diagram is generic; initiator semantics are not claimed (ADR-0011: direction is a mutually agreed label) | P1 |
| 7 | Wizard steps, one decision per step, Back/Next, inline validation, state kept, summary (5C, 6) | `pages.js` wizard | P (single form) | I | Browser: required-field errors, port range, same-id check, choices kept on Back, summary card | None | P1 |
| 8 | Preflight Pass/Warning/Blocked/Unknown with evidence; unknown never Pass; block on Blocked (5C, 6) | `src/agent.rs` `preflight`, `POST /v1/setup/preflight` | M | I (local checks) | Unit tests `preflight_*`; browser: tcp-tls without PKI is Blocked and disables Continue; peer reachability, peer identity and firewall are always Unknown | Remote checks need a peer-contact capability (gap G3) | P0 |
| 9 | Explicit Apply, verify, receipt only with a real authorized path (1.4, 6) | wizard step 6, `POST /v1/setup/apply`, `src/apply.rs` | M (local stages) | I | Unit tests in `apply.rs` and `agent.rs` (gate refusal, idempotent replay, rollback of only created files, join verification, no secret in receipt); browser: Apply is disabled until the confirm box is ticked; live agent run produced a verified receipt | Apply covers only `setup init`/`join`: no service start, no firewall, no peer contact (see receipt `not_done`) | P0 |
| 10 | Activity: timestamp, actor, operation, target, result, receipt; real filters; no secrets (5F) | `pages.js` activity, `/v1/audit` | P | P | Result filter on real records; hash as receipt; cards on mobile | Audit records have no actor or target field (gap G5) | P1 |
| 11 | Agent page: version, capabilities, freshness, diagnostics, no secrets (4.5) | `pages.js` agent, `/v1/health` | M | I | Live health check with measured latency; "updated N s ago" in top bar | No enrollment flow exists | P1 |
| 12 | Firewall and traffic visibility only if real (4.6, 4.7) | none | NA | NA | Traffic card says counters are not exposed; no chart, no number | Runtime counters and firewall inspection (gaps G6, G7) | P2 |
| 13 | Persian RTL / English LTR, switch updates everything, isolated technical values (7) | `i18n.js`, `BAFT.ltr` | P | I | Browser: dir flips, sidebar side flips, titles change, IDs/ports/versions in `bdi dir=ltr` | Preflight evidence strings from the server are English (gap G8) | P0 |
| 14 | Desktop and mobile; tables to cards; no overflow; touch targets (1.8, 5B) | `panel.css` | P | I | Browser: 7 pages at 1280 px and 390 px in both languages, no horizontal overflow; 44 px targets | Not tested on a physical phone | P0 |
| 15 | Accessibility and security: focus, labels, non-colour status, CSP, no innerHTML (7, 9) | all | P | I | Skip link and `aria-current`; every status has glyph + word; unit test: every asset has no `innerHTML`/`eval`; browser: 0 CSP violations | No formal accessibility audit | P0 |
| 16 | Empty / unknown / degraded / error states (6) | all pages | P | I | Browser: status failure shows the "agent not responding" banner and Unknown, no stale healthy value; empty history, audit off, no operations all have text | None | P0 |

## Gaps recorded for the backend (not faked in the UI)

- G1 Enrolled-server inventory and per-server remote state.
- G2 End-to-end tunnel verification (peer connection, last verified time).
- G3 Preflight checks that need the peer: reachability, identity, port ownership on the peer.
- G4 (closed for local init/join by ADR-0022) Authorized apply API with idempotency, verification, rollback and a durable receipt. Still open: service start and rollback of a started service.
- G5 Actor and target fields in the audit log.
- G6 Runtime traffic and throughput counters exposed through the API.
- G7 Read-only firewall inspection.
- G8 Machine-readable evidence codes in preflight results (so evidence can be localized).
- G9 The BAFT engine is not offered anywhere in the panel; it stays hidden until its runtime and acceptance gates pass.

## Changed files

- `src/agent.rs`: `POST /v1/setup/preflight` (read-only, uses the same `setup::plan`), `PlanBody::to_request` shared by plan and preflight, history sampler, `/v1/history`, `/v1/security`, `/v1/audit`, `/v1/engines`, panel asset table.
- `src/config.rs`, `src/tls.rs`: engine name, TLS file paths, certificate lifetime helper.
- `panel/*`: `index.html`, `panel.css` (tokens and components), `i18n.js`, `app.js` (shell, API client, freshness), `charts.js`, `pages.js` (page registry; add a page with `BAFT.register`), `boot.js`. The old `panel/panel.js` is removed.
- `scripts/panel-qa/*.js`: browser checks (not part of CI; run against a local agent).

## Test evidence (local and browser; CI and LAB are listed in the PR)

- Local: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` (100 tests pass), `cargo check` of the fuzz crate.
- Browser (headless Chromium, local agent with a real noise-tcp tunnel listening and a stored desired state): `scripts/panel-qa/browser-flows.js` 76 PASS 0 FAIL (desktop and mobile, fa and en, all pages, wizard flow, blocked preflight, no Apply control, no unexpected HTTP errors); `scripts/panel-qa/language-theme-unknown.js` 10 PASS 0 FAIL (default dark RTL, language switch, theme switch, skip link focus, status failure shows Unknown, no CSP violation).
- Real-host LAB for the panel and preflight endpoint: PASS. Run 42490325790 on main `28df6f4` (setup=1): `AGENT_RESULT ... panel_served=1 wizard_plan_ok=1 preflight_ok=1`, `PHASE1_TWO_HOST_E2E=PASS` (two real hosts, temporary /tmp processes only). Covers the local checks and the read-only contract (`applied:false`, unknown stays unknown); the peer reachability probe (#71) and the capability contract (#70 covers `/v1/engines`, not yet asserted in LAB) are local/CI evidence only. Physical phone test and formal accessibility audit: NOT RUN. Earlier attempts 42483230203, 42487952538, 42488204011 failed because concurrent LAB runs from another thread collided on the shared hosts (port in use / stale cleanup), not because of the code.
