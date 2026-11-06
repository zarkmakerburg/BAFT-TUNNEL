# BAFT-TUNNEL UI Design System

**Status:** Phase 0 design specification; requirements only.  
**Target implementation phase:** Phase 3 — Web panel and wizard.  
**Branch scope:** Documentation only. This document does not authorize frontend implementation, Phase 1/2/3 execution, merge, release or deployment.

## 1. Product and visual direction

Adopt **Infrastructure Pro × Minimal Control**:

- Operationally credible, calm infrastructure software.
- Dense information only where it helps an operator make a decision.
- Clear hierarchy, generous spacing and restrained color.
- Simple default workflows; advanced controls are progressively disclosed.
- No decorative charts, excessive gradients, glass effects or ambiguous icon-only actions.
- The UI must never conceal uncertainty, security impact or irreversible consequences.

The visual reference is the information clarity of mature cloud consoles combined with the restraint and consistency of modern productivity software. References are inspiration, not templates to copy.

## 2. Brand principles

1. **Clarity before density:** show the current service verdict and next safe action first.
2. **Evidence before status:** status labels must be derived from fresh backend evidence.
3. **Safe by default:** preview consequential changes and require explicit authorization.
4. **Progressive disclosure:** keep common tasks simple and reveal expert controls when requested.
5. **Consistent lifecycle:** terminology and state transitions must match CLI, API, agent and backend.
6. **International by design:** Persian-first RTL and English LTR must both be first-class.

## 3. Design tokens

### Color palette

| Token | Hex | Intended use |
|---|---|---|
| canvas | `#0B1120` | Main application background |
| navigation | `#101827` | Sidebar and navigation surfaces |
| surface | `#151F30` | Cards, tables and panels |
| surface-raised | `#1C2A3E` | Hovered or raised surfaces |
| border | `#263449` | Dividers and component outlines |
| primary | `#3B82F6` | Primary actions and selected navigation |
| primary-hover | `#2563EB` | Primary action hover |
| success | `#22C55E` | Confirmed healthy/success state |
| warning | `#F59E0B` | Degraded or attention state |
| danger | `#EF4444` | Failure and destructive action |
| text-primary | `#F1F5F9` | Main text |
| text-secondary | `#94A3B8` | Supporting text |
| text-muted | `#64748B` | Metadata and disabled text |

Color must never be the only status signal. Pair status color with a text label and, where useful, an icon. Validate contrast against WCAG 2.2 AA in implementation. A light theme is a later explicit theme variant; do not introduce an inconsistent second palette page by page.

### Typography

- Primary family: Inter with system-ui fallback; verify Persian glyph coverage and choose a consistent Persian-capable fallback.
- Body default: 14px / 1.5 line height.
- Page heading: 24px / 1.25, semibold.
- Section heading: 16px / 1.4, semibold.
- Labels: 12–13px, medium.
- Numeric metrics: tabular numerals.
- Use sentence case; avoid all-caps labels except compact technical identifiers.

### Spacing and geometry

Use a 4px base spacing scale.

| Token | Value |
|---|---:|
| space-1 | 4px |
| space-2 | 8px |
| space-3 | 12px |
| space-4 | 16px |
| space-5 | 20px |
| space-6 | 24px |
| space-7 | 28px |
| space-8 | 32px |

- Desktop sidebar: 248px; collapsed: 72px.
- Main content max width: 1440px.
- Desktop page padding: 28px; tablet: 20px; mobile: 16px.
- Standard control height: 40–42px.
- Card radius: 12px; input/button radius: 8px; modal radius: 14px.
- Use subtle 1px borders; avoid heavy shadows.

### Motion

- Short transitions: 120–180ms.
- Use motion only to communicate state or spatial relationships.
- Respect reduced-motion preferences.
- Never use animation as the only indication that an operation is progressing.

## 4. Application shell and navigation

### Primary navigation

1. Overview
2. Servers
3. Tunnels
4. Agents
5. Firewall
6. Traffic
7. Activity
8. Settings

The exact information architecture remains subject to Phase 3 role-journey validation against the binding `PRODUCT_UX_ACCEPTANCE.md`. Security, updates, users and advanced diagnostics must remain discoverable without overwhelming everyday operations.

### Shell

- Persistent sidebar on desktop; collapsible on tablet; drawer/bottom navigation on mobile as usability testing dictates.
- Top bar contains breadcrumb/page context, search where meaningful, notifications and account menu.
- Page title and primary action are visually distinct.
- Avoid duplicate navigation patterns and unexplained abbreviations.

## 5. Page-level layout requirements

### Overview

Priority order:
1. Service verdict and next safe action.
2. Active/attention-required servers and tunnels.
3. Traffic/performance summary with units and freshness.
4. Recent meaningful activity.

Metric cards must not imply readiness from heartbeat alone.

### Servers

A responsive list/table with server name, region, address (appropriately masked where needed), agent state, resource summary, last-seen freshness and safe actions. Unknown or stale telemetry must be explicit.

### Tunnels

Show tunnel identity, source/destination, protocol, service/port, lifecycle state, traffic and last verification. Creation/editing uses a guided flow with preview before apply.

### Agents

Show identity, associated server, version, authenticated connection state, last heartbeat and update state. A heartbeat is not proof of a ready tunnel.

### Firewall and security

Present rule direction, source/destination, protocol, port, owner and status. Explain impact and affected resources before apply. Require confirmation for consequential changes and expose a verifiable receipt.

### Traffic

Charts require units, time range, axis labels, provenance and freshness. Provide accessible tabular summaries; do not make color the only series identifier.

### Activity

Show time, event, actor, affected resource, outcome and detail. Do not log traffic payloads or secret values. Filters must be keyboard accessible.

### Settings

Group preferences, network, security, notifications, API/access and system configuration. Destructive actions must be visually separated and explain scope.

## 6. Guided setup wizard

The wizard follows the binding product lifecycle and must not invent UI-only success:

1. Welcome and explain the task.
2. Select/identify source and destination servers.
3. Run read-only preflight.
4. Guide agent installation/enrollment when the relevant backend contract exists.
5. Configure service/protocol/ports.
6. Show a complete change preview, including firewall impact, conflicts, affected nodes and rollback.
7. Request explicit authorization.
8. Apply and show actual operation progress.
9. Verify fresh evidence at the real effect boundary.
10. Produce a receipt: changed/not changed, verification evidence, warnings and recovery steps.

Keep one primary decision per step. Technical commands, when unavoidable, must be copyable and accompanied by a plain-language explanation. Never display secrets in the UI or logs.

## 7. Shared component inventory

- AppShell, Sidebar, Topbar, Breadcrumbs
- Button (primary, secondary, danger, ghost)
- StatusIndicator / StatusBadge
- MetricCard
- DataTable / ResponsiveRecordList
- SearchField / FilterMenu
- FormField / Select / Switch
- ConfirmDialog / ChangePreview
- Drawer / Modal
- Toast / InlineAlert
- EmptyState / LoadingSkeleton / ErrorPanel
- Stepper / WizardStep
- CopyableCommand
- ServerSummary / TunnelSummary
- TrafficChart / AccessibleDataSummary
- ActivityFeed
- EvidenceFreshness

All components must have documented keyboard, focus, disabled, loading, error and responsive behavior. Do not create visually distinct one-off variants without a reusable reason.

## 8. Responsive, RTL and accessibility

- Desktop: full navigation and multi-column operational layouts.
- Tablet: collapsible navigation and reduced table columns.
- Mobile: single-column content, touch-friendly actions and responsive record cards instead of unusably wide tables.
- Support Persian-first RTL and English LTR. Direction must be set semantically, not simulated with ad-hoc mirroring.
- Verify mixed-direction IP addresses, ports, code, timestamps and command snippets.
- Use semantic headings, labels, tables and buttons.
- Provide visible keyboard focus, accessible names, logical tab order and sufficient contrast.
- Respect reduced motion and browser zoom.
- Target WCAG 2.2 AA.

## 9. Required UI states

Every data-driven surface must account for:
- Initial loading
- Empty / first-run
- Fresh success
- Stale evidence
- Partial data
- Unknown state
- Offline
- Permission denied
- Validation failure
- Operation in progress
- Partial apply
- Rollback in progress / completed / failed
- Recovery required

Each state explains what is known, what is not known and the next safe action. Never convert missing or stale evidence into a green/healthy state.

## 10. Frontend implementation constraints for Phase 3

- React + TypeScript + Vite + Tailwind + shadcn/ui are the accepted baseline from the HQ board; confirm versions and current repository state before implementation.
- Centralize tokens and component variants; avoid page-local styling drift.
- Keep API contracts typed and generated/validated from the actual backend contract when available.
- The frontend must not implement authorization or privileged network effects on its own. Backend authorization is authoritative.
- Do not fabricate server, tunnel, agent, firewall or traffic data in production paths. Clearly isolated fixtures may be used only for component development and tests.
- Do not modify Rust engine, transport, agent or backend to satisfy visual-only preferences.
- Do not add dependencies without checking maintenance, license, security and accessibility fit.

## 11. Phase 3 acceptance checklist

- [ ] Role journeys and information architecture validated before implementation.
- [ ] Tokens and shared components implemented once and reused.
- [ ] Overview communicates service verdict, evidence freshness and next safe action.
- [ ] Servers, tunnels, agents, firewall/security, traffic, activity and settings have consistent patterns.
- [ ] Wizard follows preflight → preview → explicit apply → verify → receipt.
- [ ] All statuses map to real backend evidence and never overstate readiness.
- [ ] Loading, empty, stale, partial, error, rollback and recovery states are covered.
- [ ] Desktop, tablet and mobile layouts are usable.
- [ ] Persian RTL and English LTR are tested, including mixed-direction technical values.
- [ ] Keyboard and WCAG 2.2 AA checks are completed.
- [ ] Component and end-to-end tests cover critical user journeys and failure paths.
- [ ] No Rust/backend behavior or authorization boundary was weakened for UI convenience.

## 12. Phase gate

This file is a design specification only. It records the visual and interaction direction for later Phase 3 work. It does not authorize frontend code, Phase 1/2/3 implementation, real-host operations, merge, release, deployment or a phase transition. Those remain separate HQ/owner gates.
