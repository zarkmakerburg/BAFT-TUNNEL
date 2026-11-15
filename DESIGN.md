# BAFT-TUNNEL Design System

This file has two parts. **Part A** is the design system of the local Agent panel (`panel/`), implemented in `panel/panel.css`, `panel/ui.js` and `panel/app.js` (ADR-0032). **Part B** is the earlier design standard for the future Central Fleet Console; it is unchanged and still does not authorize any Central Fleet work.

---

# Part A — Local panel: "Weave"

## A1. Philosophy
The panel is an operations console for one node. Four rules decide every design question:
1. **Truth before polish.** Every value comes from an endpoint or is shown as Unknown / Unavailable / Error. Unknown is never styled as healthy. "Ready" does not exist as a state (ADR-0028, READINESS_CONTRACT).
2. **Status is never colour alone.** Every state is glyph + word + colour.
3. **Progressive disclosure.** Routine tasks first; evidence, receipts and raw configuration are one click away and never hidden from advanced users.
4. **RTL is a layout, not a mirror.** Persian RTL and English LTR share one stylesheet written with logical properties; technical identifiers keep LTR order.

Identity: the mark is two strands crossing between two endpoints (*baft* = weave); the accent is a cool cyan on deep navy, restrained so status colours stay the loudest thing on screen.

## A2. Tokens (all defined once in `panel/panel.css`)
| Group | Tokens |
|---|---|
| Surface | `--bg --nav --surface --surface-2 --surface-3 --line --line-strong` |
| Text | `--fg --muted` (muted ≥ 4.5:1 on every surface, both themes) |
| Accent | `--accent --accent-fg --accent-soft --accent-line` |
| Status | `--ok --warn --bad --unknown --info` + `-bg` tints |
| Chart | `--chart-1 --chart-2 --chart-grid` |
| Radius | `--r-sm 6 · --r 10 · --r-lg 14 · --r-pill` |
| Spacing | 4 · 8 · 12 · 16 · 24 · 32 · 48 (`--s1…--s7`) |
| Type | `--fs-0 .78 · --fs-1 .85 · --fs-2 .94 (body) · --fs-3 1.05 · --fs-4 1.25 · --fs-5 1.7` rem; line-height 1.65; tabular numerals |
| Font | `Vazirmatn` if installed, else system UI / Noto Sans Arabic; no web fonts (offline, strict CSP) |
| Elevation | `--shadow` (cards), `--shadow-lg` (overlays only); depth is mostly borders, not shadows |
| Size | `--ctl 40px` (44px on coarse pointers), `--sidebar 248`, `--rail 64`, `--tabbar 64`, `--topbar 60` |
| Motion | `--t-fast 120ms · --t-base 200ms · --ease cubic-bezier(.2,.7,.2,1)` |

Themes: `data-theme` = `dark` (default) · `light` · `auto` (follows the OS).

## A3. Status semantics (six states)
| State | Glyph | Meaning | Source in the panel |
|---|---|---|---|
| Healthy | ✓ | Evidence observed (pass / carrying traffic / listening) | `ok` |
| Warning | ! | Degraded or needs attention (no session, stale, expiring) | `warn` |
| Critical | ✕ | Failed (down, not responding, expired) | `bad` |
| Offline | ○ | Deliberately stopped | `off` |
| Connecting | ◌ | In progress (pulsing glyph) | `conn` / `info` |
| Unknown | ? | Not measured; dashed outline | `unknown` |

Data states every page handles: **loading** (skeleton), **empty**, **unavailable** (endpoint not offered in this mode, 404), **error** (request failed; never replaced by a default), **stale** (data older than 15 s: dimmed + "Updated … ago" in warning colour), **partial** (banner naming the sources that failed), **success** (toast + receipt).

## A4. Layout and responsive behaviour
- Shell: sidebar (grouped: Operate · Set up · Troubleshoot · System) + top bar (breadcrumb/title, freshness, state badge, command palette, notifications, theme, language).
- ≥ 900 px: sidebar, full (248) or icon rail (64, user choice, `Ctrl/⌘ B`). 1100 px and below: palette collapses to an icon.
- < 900 px (tablet and phone): sidebar becomes a **drawer** (native `<dialog>`, scroll locked, focus returned) opened from the top-bar menu button, plus a bottom tab bar (Overview · Tunnels · Create · Diagnostics · More). Tables become labelled cards.
- < 700 px: theme/language move to Settings and the palette; the state badge shows its glyph (the word stays for screen readers).
- Breakpoints: 420 · 700 · 900 · 1100. Content max width 1320.

## A5. Components (`panel/ui.js`)
Badge/pill · KPI card · Card · Key-value list · Empty/Unavailable/Error block · Skeleton · Warning list · Segmented control · Command palette (`Ctrl/⌘ K`, combobox/listbox, arrow keys) · Notification popover · Navigation drawer · Confirm dialog (names the action and its consequence; used for Start/Stop/Restart) · Toast (polite live region). All overlays are native `<dialog>`: focus trap, Esc, inert background, focus restored to the opener.

Charts (`panel/charts.js`) are dependency-free SVG with `role="img"` and a text alternative; time axes stay left-to-right in RTL (conventional for series), numbers use locale digits.

## A6. Motion
Purposeful only: view enter (6 px rise, 200 ms), drawer/dialog entrance, skeleton shimmer, pulse on *Connecting*. No parallax, no decorative loops, nothing that moves while an operator reads values. `prefers-reduced-motion` removes all animation and transitions.

## A7. Accessibility rules
Semantic landmarks, one `h1` per page, skip link, visible 2 px focus ring, 44 px targets on touch, labelled controls, `aria-live` for results and toasts, `aria-busy` while loading, `aria-current` in navigation, status never by colour alone, forced-colors borders. CI runs axe-core (critical/serious fail) over every page in fa/en, dark/light, desktop/mobile (`scripts/panel-qa/a11y-axe.js`).

## A8. RTL and mixed content
`dir` and `lang` are set on `<html>`; layout uses `inline-*`/`block-*` logical properties only; directional icons use `.flip`. IDs, IPs, ports, URLs, hashes, versions and commands are wrapped in `<bdi dir="ltr">` (`BAFT.ltr`) or `<code>`/`<pre>` with `dir="ltr"`. Numbers and dates use `fa-IR` / `en-GB` through `Intl`. Radio groups, tables, dialogs and the drawer are verified per component (`scripts/panel-qa/screenshots.js`), not by a global `direction: rtl`.

## A9. What we took from the research (and what we did not)
Patterns adopted, not code or dependencies (the panel is dependency-free, served by the agent under CSP `script-src 'self'`): shadcn/ui **sidebar** (collapsible icon rail, `Ctrl/⌘ B`, sheet-style mobile navigation, 16 rem widths), shadcn **command** (combobox palette, arrow/Enter), shadcn **data-table** conventions (labelled empty state, sr-only action labels), component.gallery conventions for badge, stepper, empty state, drawer; Motion's 0.2–0.3 s ease-out defaults; the awesome-design-md section list for this document. Not adopted: React/Tailwind/Radix (build chain and supply-chain surface, ADR-0032), Aceternity/Magic UI/React Bits effects (decorative), Spline and 3dicons (weight), mapcn (MapLibre + Tailwind; there is no multi-node geography in the backend), illustration packs (no screen needs one). See ADR-0032 for the sites that could not be consulted.

---

# Part B — Central Fleet Console standard (future, unchanged)


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

### Selected direction

**Approved: Modern Minimal.** This is the selected visual direction for the Central Fleet Console. Hybrid Operations and Premium Infrastructure were comparison concepts only and are not the implementation target.

Use a light-first, calm, precise, uncluttered enterprise interface. Prioritize whitespace with disciplined density, clear typography, neutral surfaces, subtle borders, restrained semantic color, and strong information hierarchy. Avoid decorative gradients, excessive glass effects, oversized hero areas, ornamental motion, and visual noise. A coherent dark theme may be supported later, but it must preserve the same semantic hierarchy and accessibility.

The selected direction must be demonstrated consistently across the Overview, Server Details, and Add Server journeys before implementation acceptance. Do not treat approval of the visual direction as authorization to begin gated implementation.

The information architecture must remain categorized and tabbed. Use dedicated entity cards and detail pages for Servers, Tunnels, and Nodes. The Overview must support operational charts for inbound/outbound throughput, bandwidth usage, latency, packet loss, CPU/RAM/disk, tunnel availability, connection history, and fleet health. Charts must include units, time range, source, freshness, and accessible text summaries.

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

Persian RTL and English LTR are complete application-shell modes, not text-alignment variants.

### Application direction and navigation

- Set the root document attributes together: Persian uses `<html lang="fa" dir="rtl">`; English uses `<html lang="en" dir="ltr">`. Apply direction before rendering the application shell to prevent layout flashes.
- In Persian RTL, the primary desktop sidebar is anchored to the **right**. The mobile menu/hamburger trigger is placed at the **top-right** of the page header; opening it reveals the navigation drawer from the right. In English LTR, these positions are mirrored to the left.
- Mirror the complete navigation composition: sidebar edge, drawer entry/exit direction, header control order, breadcrumbs, back/forward affordances, pagination direction, and directional disclosure indicators. Do not mirror icons whose meaning is not directional.
- The drawer close control must remain at the drawer's leading/top edge and reachable by keyboard, touch, and screen reader. Lock background scrolling while a modal drawer is open, manage focus, close on Escape, and restore focus to its trigger.
- Keep the same information architecture and feature hierarchy in both languages. Do not create separate, divergent navigation trees.

### Layout implementation

- Prefer CSS logical properties: `margin-inline-start/end`, `padding-inline-start/end`, `inset-inline-start/end`, `border-inline-start/end`, `text-align: start/end`, and `inline-size` / `block-size`.
- Avoid hard-coded `left` / `right`, `margin-left/right`, `padding-left/right`, and layout-wide `row-reverse` as RTL fixes. Use direction-aware grid/flex composition and logical properties so one implementation supports both directions.
- Define component placement relative to the inline axis. Verify sidebar width, page gutters, popovers, dropdown alignment, tooltips, menus, toast placement, dialogs, and floating controls in both directions.
- Use `dir="auto"` only for genuinely user-generated unknown-direction text; explicitly isolate known LTR technical values.

### Bidirectional content and data visualization

- Mirror directional layout and navigation where appropriate, but do not mirror logos, icons with intrinsic meaning, charts/data semantics, code, IP addresses, ports, or technical identifiers.
- Keep command lines, IP addresses, hostnames, fingerprints, hashes, UUIDs, URLs, and protocol tokens in LTR isolated runs (for example, `<bdi dir="ltr">` or an equivalent isolated code component).
- Use locale-aware dates and numbers while retaining unambiguous machine-readable values in detail views. Prevent punctuation, parentheses, and adjacent Latin tokens from reordering Persian sentences.
- Charts must preserve the semantic meaning of axes, chronological order, legends, and series. Direction may change label alignment and surrounding layout, but must not silently reverse the data or imply a different time sequence.
- Test mixed Persian/English strings, punctuation, parentheses, numeric tables, copy/paste, validation messages, and narrow layouts.

### RTL acceptance checks

- At mobile and desktop breakpoints, the Persian hamburger/menu trigger is visibly on the right and the navigation drawer opens from the right; English mirrors this on the left.
- Sidebar, header, breadcrumbs, tabs, pagination, menus, dialogs, form adornments, and notification placement follow the active direction without overlap or clipped content.
- Keyboard focus order follows the visual/logical reading order; screen-reader names and announcements remain correct.
- Switching language updates `lang`, `dir`, layout, and typography as one atomic change without stale directional styles.
- No production layout depends on one-off left/right offsets or duplicated RTL-only component markup.

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
2. Validate the approved Modern Minimal direction consistently on Overview, Server Details, and Add Server; do not reopen visual-direction selection without a new owner decision.
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
