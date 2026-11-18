# ADR-0053 — React architecture for the local Agent panel

- **Status:** Accepted by product owner
- **Date:** 2026-11-18
- **Scope:** Existing local Agent panel only
- **Decision owner:** Product owner
- **Supersedes:** The vanilla-only frontend technology constraint in ADR-0032, only for the local Agent panel. All non-conflicting security, deployment, UX and operational constraints remain binding.

## Context

The local Agent panel now spans nine registered routes: Overview, Tunnels, Servers, Monitoring, Setup, Diagnostics, Activity, Agent and Settings. It has shared navigation, live API state, charts, forms, bilingual fa/en layouts, RTL/LTR behavior and responsive desktop/mobile navigation.

Maintaining these screens as hand-written vanilla JavaScript increasingly duplicates view state, component behavior and interaction logic. The owner has explicitly approved React for these existing pages. This is a frontend implementation decision; it does not change the Rust core or authorize a Central Fleet/control-plane expansion.

## Decision

Adopt **React + TypeScript** for the local Agent panel. Use **Vite** as the initial build tool unless a repository-level compatibility check demonstrates a concrete reason not to.

The production artifact must remain static and be served by the existing Rust Agent. A Node.js server/runtime is not required on customer hosts. The React application must communicate with the Agent through the existing authenticated local API; it must not own tunnel, security, privileged OS or lifecycle logic.

## Boundaries and invariants

1. **Product scope:** migrate the existing nine local Agent routes only. Central Fleet Console, online enrollment and cloud control-plane screens remain separately gated and are not pulled into this work.
2. **Rust remains the product core.** React is presentation-only. No Go enters the core.
3. **API and trust boundary:** retain the existing Agent API, token/session authentication, authorization checks, CSRF/origin protections where applicable, and server-side validation. No auth bypass, fake dashboard mode or browser-side privileged operation.
4. **Truthful UI:** no demo inventory or fabricated metrics. Preserve Unknown, Unavailable, Error and freshness semantics; never infer Ready/healthy from mere process presence or a successful HTTP response.
5. **Localization:** Persian-first RTL and English LTR must work across the entire shell and every route. Use logical CSS properties and isolate IPs, ports, URLs, hashes and commands as LTR.
6. **Accessibility and responsive behavior:** semantic landmarks, keyboard operation, visible focus, screen-reader labels/live regions, adequate contrast and touch targets; verify desktop, tablet and mobile layouts.
7. **Supply-chain and runtime:** pin dependencies and commit the lockfile; review licenses and transitive dependencies; build reproducibly; ship self-hosted assets only. No CDN scripts, remote fonts or runtime network dependencies. Preserve the strict Content Security Policy.
8. **Packaging:** build output must be integrated into the existing Agent packaging/install/update/uninstall lifecycle without requiring Node/npm on target hosts. The exact output directory and asset-manifest integration are implementation details to verify before migration.
9. **Migration safety:** do not replace the working vanilla panel in one step. Keep it available until React achieves route and behavior parity and passes the acceptance gates below.

## Delivery sequence

1. Build three independent, no-cost React visual prototypes for the current nine routes. Show them side by side; do not merge one into the product or select a design on the owner's behalf.
2. Use the already-downloaded Datta Able Free React template as a visual reference and evaluate additional free/open-source components only after checking license, maintenance, accessibility, RTL support and dependency footprint. Paid AI generation is not a dependency; do not spend credits or subscribe.
3. The owner selects one visual direction.
4. Implement the selected design as shared React/TypeScript components, page by page, while retaining the current Agent API and Rust runtime.
5. Run route-level functional, auth, bilingual RTL/LTR, accessibility, responsive, packaging and install/upgrade regression checks.
6. Remove the legacy UI only after parity is demonstrated and the replacement is accepted through the normal PR and exact-head CI gates.

## Acceptance gates

- All nine existing routes render from the React build and preserve their existing API-backed behavior.
- No endpoint, authorization or security contract is weakened.
- Persian RTL and English LTR pass route-by-route; no horizontal overflow at supported viewport sizes.
- Keyboard/focus and automated accessibility checks pass; manual screen-reader and mobile checks are recorded separately.
- No demo data or false Ready/healthy state is introduced.
- Static assets are served by the Rust Agent; clean install, upgrade, rollback and uninstall package checks pass.
- Reproducible build, pinned dependency lockfile, license review and CSP checks pass.
- The legacy panel remains recoverable until the owner accepts the replacement.

## Consequences

- The panel gains a typed component/state model and a larger frontend build/dependency surface.
- Development and CI gain a Node-based build step, but deployed nodes continue to run only the Rust Agent and static assets.
- The existing vanilla implementation remains the fallback during staged migration.
- This ADR does not claim that prototypes, migration, browser QA or packaging changes are already implemented or tested.

## Addendum (2026-11-18): relationship to the BAFT Control Center (BCC) panel

The owner (zark) is the copyright holder of `zarkmakerburg/BAFT` (AGPL-3.0, Go) and stated on 2026-11-18 that reusing its code is allowed. This panel is TypeScript/React only; the Rust core stays free of Go (ADR-0019 unchanged: BAFT remains a separate binary and no BAFT control plane is imported).

What was taken, and how:
- **Ideas and visual language** from the BCC network map (equirectangular world, nodes at country points, an arc per tunnel, only reported data drawn, flags and country labels, dotted-land style, graticule, ocean gradient): reimplemented independently as an SVG component (`panel-react/src/components/viz.tsx`). No BCC source file or texture was copied.
- **World geometry** is Natural Earth land (public domain) via `world-atlas` 2.0.2 (ISC), converted by `panel-react/scripts/gen-worldmap.mjs`. BCC's `world-land.js` and Earth textures were not used (textures would also need an `img-src` CSP change, which this panel does not make).
- **Skipped on purpose:** multi-node features (node list, failover, auto-select, update center, backups, operations cards) because the local Agent has no API for them and they belong to Fleet/control-plane work (ADR-0051); the 3D welcome globe and photo textures (CSP, weight, no data value).
- **Data rule unchanged:** real Agent data only; unknown values are shown as Unknown.
