# BAFT-TUNNEL Design System

**Status:** Initial design standard for the future Central Fleet Console.  
**Scope:** Product UX, visual language, interaction patterns, accessibility, responsive behavior, motion, and design QA.  
**Authority:** This document supplements (and does not override) `docs/PRODUCT_UX_ACCEPTANCE.md`, the accepted architecture decisions, security model, or phase gates. It does not authorize Central Fleet implementation, deployment, enrollment, or changes to the local Agent panel.

## 1. Product design principles

The Central Fleet Console is an operational control surface for an owner/operator managing BAFT-TUNNEL nodes and connections across regions. It is not a generic hosting dashboard, customer billing panel, reseller portal, or marketing site.

1. **Clarity before decoration.** The interface must make the current operational situation and safest next action obvious.
2. **Fleet-first, node-aware.** Present a coherent fleet view while preserving precise per-node context and evidence.
3. **Progressive disclosure.** Keep routine workflows simple; expose advanced diagnostics and controls when requested.
4. **Truthful state.** Never imply health, readiness, successful execution, or freshness without corresponding evidence.
5. **Safe by design.** Preview scope and consequences before consequential actions; clearly separate read-only inspection from mutations.
6. **Resilient operations.** A disconnected console must not imply that the data plane has stopped. The local Agent panel remains available as a separate per-host operational path.
7. **Consistent, multilingual UX.** Persian-first RTL and English LTR must be first-class layouts, not superficial translations.
8. **Calm, professional visual language.** Use disciplined hierarchy, restrained surfaces, legible typography, and meaningful status colors. Avoid ornamental gradients, excessive glass effects, oversized empty hero sections, and animation that competes with operational information.

## 2. Reference research and reuse policy

The following sources are research inputs. They are not approved templates and must not be copied wholesale.

| Source | Intended research use |
|---|---|
| [Mobbin](https://mobbin.com/) | Real product patterns for dashboards, onboarding, tables, filters, settings, and multi-step workflows |
| [Httpster](https://httpster.net/) and [Land-book](https://land-book.com/) | Brand direction, typography, composition, and visual restraint; primarily marketing/brand inspiration, not dashboard IA |
| [Component Gallery](https://component.gallery/) | Compare component behavior and design-system conventions |
| [getdesign.md](https://getdesign.md/) and [Awesome Design MD](https://github.com/VoltAgent/awesome-design-md) | Maintain concise, tool-readable design rules and reusable project context |
| [shadcn/ui](https://ui.shadcn.com/) | Candidate foundation for accessible, customizable application primitives |
| [21st.dev](https://21st.dev/) | Discover candidate components and implementation approaches; review code and dependencies before adoption |
| [Magic UI](https://magicui.design/) and [Aceternity UI](https://ui.aceternity.com/) | Selective inspiration for distinctive but restrained visual elements |
| [Motion Primitives](https://motion-primitives.com/) and [Anime.js](https://animejs.com/) | Motion patterns and targeted animation where motion improves comprehension |

### Adoption checklist

Before a reference, component, package, or snippet is adopted, record:
- the user problem it solves and why a native implementation is insufficient;
- accessibility semantics, keyboard behavior, focus management, and screen-reader behavior;
- responsive behavior and touch usability;
- RTL/LTR support and localization constraints;
- dependency footprint, maintenance health, security posture, and integration cost;
- license and usage terms;
- version/pinning strategy and any required modifications.

Do not add a library solely because it appears in the reference list. Prefer the smallest coherent component set. Avoid duplicate animation systems. No third-party component may weaken security boundaries or introduce external runtime/CDN dependencies into the local Agent panel.

## 3. Product boundaries and architecture constraints

- The Central Fleet Console is a future, separately gated control-plane product surface.
- The per-host Agent and its dependency-free local panel remain intact and independently useful.
- Do not turn the local Agent into a publicly exposed management endpoint.
- Agent-to-control-plane connectivity is expected to be outbound and mutually authenticated, subject to the separate trust-boundary ADR and security review.
- Central control-plane availability must not be a prerequisite for established data-plane forwarding.
- A control-channel heartbeat is not proof that an Agent is healthy or that a tunnel is ready.
- UI visibility is never authorization. Backend policy remains authoritative for every action.
- Never expose node private keys, CA private keys, enrollment secrets, API tokens, or other secret material in the UI, logs, telemetry, exports, or screenshots.
- Do not add customer accounts, billing, subscriptions, reseller tiers, or marketing CRM concepts to this product surface without a separate product-boundary decision.

## 4. Information architecture

The following is the proposed Central Fleet navigation model. Validate it with operator journeys and task-based usability review before implementation.

1. **Overview** — fleet verdict, regional distribution, active tunnels, actionable alerts, recent changes, and the next safe action.
2. **Nodes** — searchable/filterable inventory, region/group, Agent connectivity, version, evidence freshness, and node details.
3. **Connections** — tunnels, endpoints, topology, direction, engine capability, lifecycle state, and verified data-plane evidence.
4. **Traffic & Performance** — throughput, latency, loss, uptime, and trends with units, time range, source, and freshness.
5. **Alerts & Diagnostics** — prioritized incidents, failing layer, actionable explanation, safe remediation guidance, and privacy-safe diagnostics.
6. **Activity & Audit** — actor, target, operation, authorization, timestamps, outcome, and evidence/receipt references.
7. **Security** — identities, trust state, certificate/CRL status, roles, policy, and security events; secret values are never displayed.
8. **Updates & Maintenance** — installed version, available signed version, update preview, maintenance status, backup/restore and rollback history.
9. **Administration** — users, roles, permissions, organization-level settings, and operator preferences.

Keep routine operations visually distinct from advanced diagnostics and administration. Navigation labels and page names must use one consistent terminology across the UI, CLI, API, and documentation.

## 5. Core journeys

### Add a node

1. Explain prerequisites and the outbound connectivity model in plain language.
2. Run a non-mutating preflight and show any blockers.
3. Generate a short-lived, single-use enrollment invitation only after the trust and enrollment design is accepted.
4. Present a copyable, signed-release-aware installation command with an explicit target node name and region.
5. Show pending connection, identity verification, and enrollment evidence as separate stages.
6. Confirm inventory registration only after the control channel is authenticated and the server-side enrollment receipt is verified.
7. Offer a clear next action without implying that a tunnel has been created or is ready.

### Create or change a tunnel

1. Select source and destination nodes using clear region and role labels.
2. Offer only engine/direction combinations that the backend advertises as available for those exact nodes.
3. Collect the minimum user-facing service details; keep advanced transport parameters collapsed by default.
4. Preview affected nodes, listener/destination, ports, policy, firewall impact (if applicable), interruption risk, and rollback behavior.
5. Require explicit authorization for consequential changes.
6. Show operation progress, partial results, and per-node receipts.
7. Verify authenticated transport and actual forwarded application traffic before showing Ready.

### Diagnose a problem

1. Identify the failing layer: control channel, Agent, tunnel control state, transport, listener, destination, or application traffic.
2. Show evidence timestamp, freshness, source, and confidence/limitations.
3. Explain the likely cause in operator-friendly language.
4. Recommend the safest next action and show its scope.
5. Keep raw logs and technical detail available as an expandable expert view, with secrets redacted.

## 6. Status model and evidence presentation

Use the canonical vocabulary from `docs/PRODUCT_UX_ACCEPTANCE.md`:
- **Ready** — fresh, authenticated, end-to-end evidence satisfies the readiness contract.
- **Degraded** — functioning, but a material quality, redundancy, or policy condition is impaired.
- **Needs attention** — a known actionable issue requires operator intervention.
- **Offline** — fresh evidence confirms the service or node is unavailable.
- **Unknown** — evidence is missing, stale, contradictory, or could not be collected.

Show these as independent dimensions; do not collapse them into one green dot:
- Control Plane connectivity;
- Agent health;
- Data Plane / tunnel readiness.

Every operational status and metric must expose, directly or on inspection:
- what is being measured;
- evidence source;
- observation time and freshness;
- scope (fleet, region, node, or tunnel);
- known limitations or missing evidence.

A heartbeat, TCP connection, successful API response, process-running state, or successful Apply receipt alone never proves end-to-end readiness. Missing or stale evidence is Unknown, never healthy. Never use color as the only status signal; pair status color with text and/or an icon.

## 7. Visual language and design tokens

### Direction

Use a **quiet technical / premium operations** visual direction: precise, restrained, information-dense without feeling crowded. Start with a light theme as the primary design target; support a coherent dark theme without changing semantic meaning.

Do not finalize brand colors or typography by guesswork. Define tokens first, then validate three distinct visual directions with representative screens before selecting the final visual identity.

### Token categories

All UI colors, spacing, typography, borders, radii, shadows, and motion must be tokenized. Avoid one-off magic values in production components.

Required semantic color tokens:
- canvas, surface, elevated surface, muted surface;
- primary text, secondary text, muted text, inverse text;
- border, subtle border, focus ring;
- primary action, secondary action, destructive action;
- success, warning, danger, informational, unknown;
- chart series and chart grid/text.

Status colors must be consistent across cards, tables, maps, charts, notifications, and detail pages. Use neutral surfaces for most content; reserve strong color for actions, focus, and meaningful state.

Required layout tokens:
- spacing scale;
- content max-widths and page gutters;
- navigation/sidebar widths;
- grid breakpoints;
- table row heights and density variants;
- standard radii, borders, elevation, and overlay layers.

Required typography tokens:
- display, page title, section title, body, label, caption, code/monospace;
- separate font stacks and line-height rules for Persian and Latin text;
- numeric alignment for metrics, timestamps, versions, and IDs.

Use tabular numerals for operational metrics. Keep identifiers and commands in readable LTR/code treatment even inside RTL pages.

## 8. Component standards

Build a consistent component inventory before page-by-page implementation.

Core components:
- application shell, responsive navigation, breadcrumbs, page header;
- status badge with text, icon, freshness, and tooltip/detail;
- metric card with unit, time window, trend, and provenance;
- searchable/filterable/sortable table with density controls;
- node and tunnel summary rows/cards;
- region/group selector;
- empty, loading, stale, offline, permission-denied, and error states;
- wizard stepper, validated form fields, inline help, and copyable command;
- preview/diff panel for consequential changes;
- confirmation dialog that names scope and consequence;
- progress and partial-success summary;
- audit timeline and evidence/receipt viewer;
- notification/toast for transient feedback, never as the sole record of an important result;
- chart with accessible textual summary, units, range, and source;
- expandable technical detail and redacted diagnostic viewer.

Component rules:
- Use semantic HTML and established accessible primitives.
- Tables must remain usable on narrow screens; choose intentional responsive transformations rather than accidental horizontal overflow.
- Forms must have visible labels, inline validation, clear required/optional markers, and actionable error messages.
- Destructive actions must never be triggered by a vague icon-only control.
- Keep keyboard focus visible and restore focus after dialogs.
- Use skeletons only when layout stability helps; never fabricate placeholder values that look real.
- Distinguish no data, not yet collected, stale data, unavailable capability, and permission denied.

### Candidate frontend foundation

For the future Central Fleet frontend, React + TypeScript with Tailwind and shadcn/ui is the preferred candidate foundation because it allows project-owned components and a consistent accessible primitive layer. This is a design recommendation, not an implementation authorization or a finalized dependency decision. Evaluate current versions, licenses, RTL behavior, bundle size, security, and integration fit before adoption.

The local Agent panel remains dependency-free unless a separate, evidence-backed architecture decision changes that constraint.

## 9. Persian RTL and English LTR

- Persian is the primary product language; English is fully supported.
- Set document direction and language at the application boundary, not with ad hoc per-component overrides.
- Mirror directional layout and navigation where appropriate, but do not mirror logos, icons with intrinsic meaning, charts, code, IP addresses, ports, or technical identifiers.
- Keep command lines, IP addresses, hostnames, fingerprints, hashes, UUIDs, and protocol tokens in LTR isolated runs.
- Use locale-aware dates and numbers while retaining unambiguous machine-readable values in detail views.
- Test mixed Persian/English strings, punctuation, parentheses, numeric tables, copy/paste, validation messages, and narrow layouts.
- Never use Persian glyph shaping or direction hacks as a substitute for correct bidi layout.

## 10. Responsive behavior

Design and test at narrow mobile, tablet, laptop, and wide desktop sizes.

- Desktop: persistent navigation where space allows; data tables and multi-column operational views.
- Tablet: collapsible navigation, adaptive grids, prioritized columns, and touch-friendly controls.
- Mobile: compact navigation, single-column detail flows, stacked metrics, and explicit table-to-card/row transformations.
- Preserve the most important operational verdict and next action at every width.
- Do not hide warnings, authorization scope, freshness, or destructive consequences on mobile.
- Avoid hover-only functionality; all interactions must work by touch and keyboard.

## 11. Motion and interaction feedback

Motion is functional feedback, not decoration.

- Prefer short, subtle transitions for navigation, disclosure, selection, and state changes.
- Animate only when it helps communicate cause/effect, hierarchy, or progress.
- Do not animate continuously changing charts or status indicators in a distracting way.
- Respect `prefers-reduced-motion`; provide a reduced-motion experience with no essential information conveyed only through animation.
- Avoid parallax, cursor-following effects, looping backgrounds, excessive spring/bounce, and long entrance sequences in operational screens.
- Use one primary motion approach; do not combine Anime.js and multiple React motion libraries without a documented need.
- Async operations must communicate queued/running/succeeded/failed/partial/unknown states independently of animation.

## 12. Accessibility and quality

Target WCAG 2.2 AA for the Central Fleet Console. Follow WAI-ARIA Authoring Practices for interactive patterns.

Required:
- complete keyboard operation and logical focus order;
- visible focus indicators;
- accessible names, labels, descriptions, and error associations;
- adequate contrast in both themes;
- semantic headings, landmarks, tables, and form controls;
- screen-reader announcements for meaningful asynchronous state changes;
- no status communicated by color alone;
- touch targets suitable for mobile;
- reduced-motion support;
- zoom/reflow without loss of core functionality.

## 13. Required page states

Every page and component must define:
- first-run / empty inventory;
- loading;
- fresh data;
- stale data;
- partial data;
- offline / disconnected;
- permission denied;
- unavailable capability;
- validation error;
- operation queued / running;
- partial apply;
- failed operation;
- rollback / recovery;
- success with evidence;
- unknown outcome / timed-out response.

Each state must say what happened, what is known, what remains unknown, and what the operator can safely do next. Never present an uncertain outcome as failure or success without checking the operation receipt/state.

## 14. Design QA and acceptance

Before a screen is accepted:
1. Validate the target operator task and information hierarchy.
2. Review three visual directions on representative screens before committing to a final look.
3. Test Persian RTL and English LTR, including mixed technical strings.
4. Test desktop, tablet, and mobile layouts.
5. Test keyboard-only and screen-reader flows for core tasks.
6. Verify every displayed status against its backend evidence contract.
7. Verify empty, stale, partial, offline, denied, failure, and recovery states.
8. Check that consequential operations show scope, authorization, preview, and receipts.
9. Review performance, bundle size, dependency/license inventory, and browser console/network behavior.
10. Record findings and close blocking defects before implementation handoff.

Design acceptance is not runtime acceptance. A visually complete screen does not prove backend capability, security, readiness, or phase exit.

## 15. Reference links

- [Mobbin](https://mobbin.com/)
- [Httpster](https://httpster.net/)
- [Land-book](https://land-book.com/)
- [Component Gallery](https://component.gallery/)
- [getdesign.md](https://getdesign.md/)
- [VoltAgent/awesome-design-md](https://github.com/VoltAgent/awesome-design-md)
- [Magic UI](https://magicui.design/)
- [21st.dev](https://21st.dev/)
- [shadcn/ui](https://ui.shadcn.com/)
- [Aceternity UI](https://ui.aceternity.com/)
- [Refero Styles](https://styles.refero.design/)
- [Anime.js](https://animejs.com/)
- [Motion Primitives](https://motion-primitives.com/)
- [WCAG 2.2](https://www.w3.org/TR/WCAG22/)
- [WAI-ARIA Authoring Practices](https://www.w3.org/WAI/ARIA/apg/)

---
This document defines design constraints and evaluation criteria. It does not claim that the Central Fleet Console is implemented or that any referenced third-party component has been adopted.
