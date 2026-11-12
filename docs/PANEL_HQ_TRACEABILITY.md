# Panel: decision-to-implementation traceability (HQ Issue #2, binding comment 6720757885)

Historical baseline: `main` at `650e955` (after PR #61). This file records the original panel traceability work; the current status corrections below supersede stale Before/After statements. Current panel remains dependency-free static HTML/CSS/JavaScript served by the local Agent; no React/Vite build step.

State column: I = implemented, P = partial, M = missing (backend gap, hidden or explained in the UI), NA = not applicable to the MVP. "Before" is the state of the panel at the baseline.

| # | Requirement (HQ section) | Source | Before | After | Evidence | Remaining / action | Pri |
|---|---|---|---|---|---|---|---|
| 1 | Graphite/Navy base, Electric Blue accent, semantic green/amber/red, tokens only (2) | `panel/panel.css` `:root` tokens | P (light default, generic blue) | I | Browser flows: dark default; light and dark both render | Light theme kept as option; no brand typeface bundled (system + Vazirmatn if installed) | P1 |
| 2 | Dashboard hierarchy: summary cards, health/activity, warnings, empty state (5A) | `pages.js` overview | M | I | Screenshots desktop/mobile; first-run banner when nothing is stored | Servers count is "this node" only (no inventory) | P1 |
| 3 | Navigation: persistent sidebar, active state, title, language control; mobile tab bar (4, 5A/B) | `app.js`, `panel.css` | P | I | Browser: `aria-current`, sidebar side flips with language; mobile tab bar + More | Firewall and Traffic pages omitted from navigation on purpose (no capability) | P1 |
| 4 | Servers: identity, role, state, freshness, version, capabilities, trust, diagnostics (5D) | `pages.js` servers; `/v1/status,/v1/security,/v1/engines` | M | P | Resource cards for this node and its peer; unknown fields use the dashed Unknown pill | No enrolled-server inventory and no remote server state in the backend (gap G1) | P1 |
| 5 | Tunnels: name, direction, endpoints, engine, state, last verified, metrics, actions (5E) | `pages.js` tunnels | P (table) | P | Resource card; reflows on mobile | End-to-end tunnel state, peer identity and last verified time are now exposed by diagnostics/status (G2 closed by PR #109 and panel PR #120). Integrity remains Unknown in product status; lifecycle actions are now available through the local Agent API (see ADR-0023/0024) | P1 |
| 6 | Direct/Reverse first, plain-language, small topology diagram (1.7, 5C) | `pages.js` wizard step 1 | M | I | Browser: direction required before continuing | Diagram is generic; initiator semantics are not claimed (ADR-0011: direction is a mutually agreed label) | P1 |
| 7 | Wizard steps, one decision per step, Back/Next, inline validation, state kept, summary (5C, 6) | `pages.js` wizard | P (single form) | I | Browser: required-field errors, port range, same-id check, choices kept on Back, summary card | None | P1 |
| 8 | Preflight Pass/Warning/Blocked/Unknown with evidence; unknown never Pass; block on Blocked (5C, 6) | `src/agent.rs` `preflight`, `POST /v1/setup/preflight` | M | I (local checks) | Unit tests `preflight_*`; browser: tcp-tls without PKI is Blocked and disables Continue; peer reachability, peer identity and firewall are always Unknown | Remote checks need a peer-contact capability (gap G3) | P0 |
| 9 | Explicit Apply, verify, receipt only with a real authorized path (1.4, 6) | wizard step 6, `POST /v1/setup/apply`, `src/apply.rs` | M (local stages) | I (implemented lifecycle; readiness remains conservative) | Unit tests in `apply.rs` and `agent.rs`; systemd control-helper tests; trusted D2 LAB Start PASS (run 42996224181) and U7 integrated acceptance PASS (run 43063199428) | Local Agent lifecycle includes init/join/start/stop/restart and boot persistence with receipts; remote fleet actions and firewall mutation are not implemented; `ready` remains Unknown | P0 |
| 10 | Activity: timestamp, actor, operation, target, result, receipt; real filters; no secrets (5F) | `pages.js` activity, `/v1/audit` | P | P | Result filter on real records; hash as receipt; cards on mobile | Audit v2 records actor fingerprint, target, resource and receipt reference (G5 closed by PR #109) | P1 |
| 11 | Agent page: version, capabilities, freshness, diagnostics, no secrets (4.5) | `pages.js` agent, `/v1/health` | M | I | Live health check with measured latency; "updated N s ago" in top bar | No enrollment flow exists | P1 |
| 12 | Firewall and traffic visibility only if real (4.6, 4.7) | `/v1/traffic`; firewall capability unavailable | NA | P (traffic implemented; firewall unavailable) | Traffic counters are exposed through `/v1/traffic` and rendered in the panel (G6 closed by PR #109 and PR #120); no firewall inspection or mutation is claimed (G7) | P2 |
| 13 | Persian RTL / English LTR, switch updates everything, isolated technical values (7) | `i18n.js`, `BAFT.ltr` | P | P (B1 implementation; latest exact-head CI in progress) | Existing browser language flows plus B1 bilingual evidence/source/remediation/next-action assertions and 103-code × fa/en contract | Formal accessibility audit and physical-phone test remain open | P0 |
| 14 | Desktop and mobile; tables to cards; no overflow; touch targets (1.8, 5B) | `panel.css` | P | I | Browser: 7 pages at 1280 px and 390 px in both languages, no horizontal overflow; 44 px targets | Not tested on a physical phone | P0 |
| 15 | Accessibility and security: focus, labels, non-colour status, CSP, no innerHTML (7, 9) | all | P | I | Skip link and `aria-current`; every status has glyph + word; unit test: every asset has no `innerHTML`/`eval`; browser: 0 CSP violations | No formal accessibility audit | P0 |
| 16 | Empty / unknown / degraded / error states (6) | all pages | P | I | Browser: status failure shows the "agent not responding" banner and Unknown, no stale healthy value; empty history, audit off, no operations all have text | None | P0 |

## Gaps recorded for the backend (not faked in the UI)

- G1 Enrolled-server inventory and per-server remote state.
- G2 **Closed** by PR #109 and panel PR #120: end-to-end tunnel diagnostics/status, peer identity and last-verified timestamp are exposed. The product does not promote this to `ready`; integrity remains Unknown unless separately measured.
- G3 Preflight does not yet provide a complete remote-peer evidence contract (peer-side identity/port ownership and authoritative remote state). Peer-aware diagnostics exist under ADR-0024, but they are not equivalent to remote fleet inventory.
- G4 **Closed for implemented lifecycle**: init/join, start/stop/restart and boot persistence are implemented through the local Agent/control helper; receipts remain secret-free and `ready` remains Unknown by design.
- G5 **Closed** by PR #109: audit v2 includes actor fingerprint, target, resource and receipt reference.
- G6 **Closed** by PR #109 and panel PR #120: runtime traffic/throughput counters are exposed through the API and shown in the panel.
- G7 Read-only firewall inspection.
- G8 Structured Preflight/Diagnostics evidence codes + typed params and Persian/English localization: implemented in B1; latest exact head in PR #168 is `bd784300fcc2928cc6412363d423da6542451d33`; phase0, fuzz-smoke, package/signature, panel-ui, Ubuntu 22.04/systemd 249 and offline-CA jobs passed, while Rust remains in progress. Merge is pending.
- G9 **Partly closed**: BAFT is visible as Preview, Direct only; guided setup cannot yet configure it (B4 remains open).

## Changed files

- `src/agent.rs`: `POST /v1/setup/preflight` (read-only, uses the same `setup::plan`), `PlanBody::to_request` shared by plan and preflight, history sampler, `/v1/history`, `/v1/security`, `/v1/audit`, `/v1/engines`, panel asset table.
- `src/config.rs`, `src/tls.rs`: engine name, TLS file paths, certificate lifetime helper.
- `panel/*`: `index.html`, `panel.css` (tokens and components), `i18n.js`, `app.js` (shell, API client, freshness), `charts.js`, `pages.js` (page registry; add a page with `BAFT.register`), `boot.js`. The old `panel/panel.js` is removed.
- `scripts/panel-qa/*.js`: browser checks (not part of CI; run against a local agent).

## Test evidence (historical baseline; newer evidence is linked in the current roadmap and HQ board)

- Local: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` (100 tests pass), `cargo check` of the fuzz crate.
- Browser (headless Chromium, local agent with a real noise-tcp tunnel listening and a stored desired state): `scripts/panel-qa/browser-flows.js` 76 PASS 0 FAIL (desktop and mobile, fa and en, all pages, wizard flow, blocked preflight, no Apply control, no unexpected HTTP errors); `scripts/panel-qa/language-theme-unknown.js` 10 PASS 0 FAIL (default dark RTL, language switch, theme switch, skip link focus, status failure shows Unknown, no CSP violation).
- Real agent + real local noise-tcp tunnel (no mocks, not LAB), 2026-11-09: `scripts/panel-qa/p1-real-agent.js` carrying 14 PASS 0 FAIL, degraded (peer stopped) 13 PASS 0 FAIL. Found and fixed a login defect: a wrong token auto-logged-out and hid the "Wrong token" message. Contract: [READINESS_CONTRACT.md](READINESS_CONTRACT.md).
- Real-host LAB for the panel and preflight endpoint: PASS. Run 42490325790 on main `28df6f4` (setup=1): `AGENT_RESULT ... panel_served=1 wizard_plan_ok=1 preflight_ok=1`, `PHASE1_TWO_HOST_E2E=PASS` (two real hosts, temporary /tmp processes only). Covers the local checks and the read-only contract (`applied:false`, unknown stays unknown); the peer reachability probe (#71) and the capability contract (#70 covers `/v1/engines`, not yet asserted in LAB) are local/CI evidence only. Physical phone test and formal accessibility audit: NOT RUN. Earlier attempts 42483230203, 42487952538, 42488204011 failed because concurrent LAB runs from another thread collided on the shared hosts (port in use / stale cleanup), not because of the code.


## 2026-11-11 status reconciliation

- This matrix began at `650e955`; the original browser evidence and historical Before/After observations above are preserved, not represented as current-head test results.
- Current main at this documentation update is `52a2825254f8254386f71af7220c7cb244200fc2` (U8 PR #165 merged). The local Agent lifecycle now includes start/stop/restart and boot persistence; audit v2 and traffic counters are implemented. `ready` remains Unknown by design.
- The trusted D2 panel/PKI two-host repeat passed on the immutable RC.7 candidate (run [42996224181](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42996224181)).
- The trusted U7 integrated acceptance run [43063199428](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/43063199428) passed release-tag, signed-asset and real two-host acceptance. U7 is accepted within the bounded Direct scope.
- Remaining product gaps in this matrix are remote fleet inventory (G1), complete remote preflight contract (G3), read-only firewall visibility (G7), and BAFT guided configuration (G9/B4). G8 is implemented; B1's latest synchronized PR #168 has partial exact-head CI success and is awaiting the remaining Rust job and merge. Formal accessibility audit and physical-phone testing have not been run.
