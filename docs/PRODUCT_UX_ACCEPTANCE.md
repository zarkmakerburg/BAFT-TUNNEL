# Product UX and Installer Acceptance Criteria

**Status:** Binding Phase 0 product constraint (Decision D-008); requirements only, not evidence of implementation.  
**Implementation gate:** This document does not authorize Phase 1 or Phase 3 work, merge, release, deployment or phase transition.

## Product promise

An operator should be able to install the product, pair two nodes, define a forwarded service and verify real traffic without learning the internal transport architecture. Technical complexity belongs inside the system. The dashboard and web panel must be comprehensive in capability but simple in ordinary use.

Simplicity must never hide a material risk, destructive consequence, authorization boundary or uncertainty.

## First-run and guided lifecycle

1. **Preflight:** check OS/architecture, privileges, outbound connectivity, ports, disk, clock and conflicting listeners without changing the host.
2. **Install:** use a signed, checksum-verified package and dedicated service identity. Re-running must be safe.
3. **Pair:** use a short-lived, single-use enrollment code; clearly show which side initiates outbound connectivity.
4. **Configure:** create a tunnel through a small wizard using source, destination, protocol/service and ports.
5. **Preview:** show changes, port conflicts, firewall impact, affected nodes, consequences and rollback before apply.
6. **Apply:** require explicit authorization for consequential changes; use safe defaults and idempotent operations.
7. **Verify:** inspect fresh evidence at the real effect boundary, including authenticated transport and actual forwarded traffic.
8. **Receipt:** report what changed, what did not, verification evidence, warnings and recovery/rollback instructions.

Normal setup must not require manual configuration-file editing. Failure messages must identify the failing layer and provide copyable diagnostics without secrets. Uninstall must remove only resources provably created and owned by that installation.

## Status vocabulary

- **Ready:** fresh, authenticated and end-to-end evidence satisfies the defined readiness contract.
- **Degraded:** service is functioning but a material quality, redundancy or policy condition is impaired.
- **Needs attention:** a known actionable issue requires operator intervention.
- **Offline:** fresh evidence confirms the service or node is unavailable.
- **Unknown:** evidence is missing, stale, contradictory or could not be collected.

Every status must expose its evidence source and freshness, plus the next useful action. A heartbeat alone never means Ready. Missing or stale evidence is unknown, never healthy.

## Proposed information architecture

Validate this structure with role journeys and usability evidence during Phase 3; it is a Phase 0 product baseline, not a final visual design.

1. **Overview** — overall service verdict, active tunnels, important alerts, recent changes and one clear next action.
2. **Connections** — paired nodes, tunnel state, endpoints, path status and safe connection actions.
3. **Traffic & Performance** — throughput, latency, loss, uptime and useful trends with units, time range and freshness.
4. **Setup & Wizard** — preflight, install, enrollment, pairing, configuration preview, apply and verification.
5. **Profiles & Routing** — understandable presets and user-facing route choices; expose advanced parameters progressively.
6. **Security** — identity, access, certificate/key status, audit history and security alerts; never reveal secret values.
7. **Monitoring & Alerts** — actionable incidents, notification settings, evidence freshness and history.
8. **System & Updates** — version, health checks, update/rollback status, backup/restore and maintenance.
9. **Advanced Diagnostics** — privacy-safe diagnostics, bounded logs, exportable support bundle and expert details.
10. **Administration** — users, roles, permissions, organization and preferences.

Navigation must be consistent and searchable, with clear context/breadcrumbs where useful. Everyday operations must remain distinct from advanced administration.

## Role, responsive and accessibility requirements

- Provide role-appropriate views; backend authorization remains authoritative and must match visible UI affordances.
- Design for desktop and mobile, including narrow layouts and usable touch targets.
- Support keyboard navigation, visible focus, accessible names, semantic forms/tables, readable contrast and assistive technology.
- Pair icons with text where meaning is not universally obvious.
- Support Persian-first RTL and English LTR without mirrored-layout defects.

## Required UI states

Design and test first-run, empty, loading, stale, partial-data, offline, permission-denied, validation-error, apply-in-progress, partial-apply, rollback, recovery and success states. Each state must explain what happened and what the user can safely do next.

## Dashboard quality rules

- Prioritize operational questions over decorative charts.
- Show units, time windows, freshness and provenance for metrics.
- Avoid presenting every metric or setting on the landing page.
- Use progressive disclosure for advanced controls and technical terminology.
- Never make destructive actions one-click or ambiguous; show scope and consequences before confirmation.
- Keep terminology, state transitions and actions consistent across CLI, API, agent and panel.
- Never report Ready until authenticated transport, listener ownership, destination reachability and actual forwarded traffic are verified with fresh evidence.

## Source and design-system evaluation

Use current reputable sources as research inputs, not templates to copy blindly. Before adopting a dashboard kit, block library or component, review accessibility, keyboard behavior, responsive/mobile usability, RTL/LTR support, information architecture, component semantics, maintainability, dependency health, license/usage terms, integration fit, security and upstream maintenance.

Candidate references for evaluation during Phase 3:

- Dashboard Design Patterns research: https://dashboarddesignpatterns.github.io/
- shadcn/ui dashboard blocks: https://www.shadcn.io/dashboard-block
- shadcn/ui component foundation: https://ui.shadcn.com/
- WAI-ARIA Authoring Practices: https://www.w3.org/WAI/ARIA/apg/
- WCAG 2.2: https://www.w3.org/TR/WCAG22/

No source is approved for wholesale copying by appearance alone. Record selected components, versions, license checks and rationale before reuse.

## Acceptance criteria

- A first-time operator can identify the service verdict and next safe action without networking expertise.
- Common install, pairing, setup and recovery journeys are guided end to end.
- Normal setup requires no manual configuration-file editing.
- Every step is retryable; partial installation can be diagnosed and rolled back without deleting unrelated resources.
- Advanced controls are discoverable but do not overwhelm default workflows.
- Statuses are based on explicit evidence, with source and freshness visible.
- Critical changes show scope, consequence, authorization and a preview.
- Mobile, desktop, keyboard, accessibility and RTL/LTR journeys are tested.
- Empty, stale, partial failure and recovery states are understandable.
- UI lifecycle and status behavior match the backend contract; there are no UI-only bypasses.
- E2E tests cover fresh install, retry, port conflict, interruption, rollback, re-enrollment and uninstall.

## Phase application

- **Phase 0:** architecture and product acceptance constraint only; preserve this document as binding.
- **Phase 1–2:** define stable user-oriented lifecycle/status contracts; keep engine internals out of ordinary user workflows.
- **Phase 3:** review information architecture, role journeys, wireframes and design system before panel implementation.
- **Phase 4–5:** preserve the same interaction model as advanced capabilities are added; validate usability before v1.0.

**Gate:** This document does not authorize Phase 1 or Phase 3 implementation or any merge, release, deployment or phase transition.
