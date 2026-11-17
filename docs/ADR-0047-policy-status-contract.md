# ADR-0047: Policy Pack status contract (API, CLI, panel)

**Status:** Proposed. Contract only; no endpoint, field or panel card exists yet.
**Date:** 2026-11-17
**Related:** [ADR-0031](ADR-0031-adaptive-traffic-policy-pack.md) decisions 8, 10, 14, [ADR-0044](ADR-0044-engine-agnostic-policy-interface.md), [ADR-0024](ADR-0024-p1-operations-evidence.md) (Unknown, freshness), [ADR-0041](ADR-0041-transport-health-model.md), issues [#240](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/240), [#241](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/241)

## Context

TP-5.1 (API/CLI) and TP-5.2 (panel) must never show a profile as working without evidence, and never fill a gap with a made-up number (project rule: no fake data in the panel). This ADR fixes the shape of the status so engine work and UI work can proceed independently.

## Decision (proposed)

1. **One read-only status object per tunnel**, exposed as `policy` inside the existing agent status (a new `GET /v1/policy` may be added later; no write path in this ADR). When the pack is not compiled in or is off, `policy` is `{"state":"off"}` and the panel shows a single "off" line.
2. **State values** (closed set): `off`, `active`, `fallback` (running Balanced after a guard violation, stale metric or version mismatch, with `cause`), `unknown` (no data yet or agent cannot tell). `unknown` is the default; nothing is inferred.
3. **Per-tuple evidence, not per-profile.** For each (engine, carrier, profile) the object carries `claim`: `unsupported`, `unknown`, or `measured`. `measured` requires a result record with `evidence_level` (`simulated`, `vm`, `lab`), the run id and the exact SHA. `lab` is shown only for a real two-host LAB run. A tuple without a record is `unknown`; there is no "ready" state in this contract (production is never claimed).
4. **Capabilities** (ADR-0044 decision 4) are shown as `supported`/`unsupported`/`unknown` per engine/carrier, never hidden.
5. **Metrics fields** follow ADR-0024: each value has `unit`, `interval_seconds`, `provenance` (`measured` or `estimated`, separate fields), `fresh` (boolean, stale after 3 intervals). Missing values are `null` plus `available:false`, never 0. Packet loss on a reliable carrier is only a proxy and carries `kind:"loss_proxy"`.
6. **Counters of the guard** (`violations`, last `cause`, last fallback time) are included so a fallback is visible and explainable.
7. **Panel (fa/en, RTL).** One card "Traffic policy": state badge, active profile (configured, not guessed), per-tuple claim table with an evidence-level chip, last fallback cause. Copy states plainly that policy affects only BAFT-TUNNEL's own queueing, not internet path latency (ADR-0031 decision 4). The card is hidden when the state is `off` and the feature is not compiled in. No chart without data; an empty state says "no measurements yet".
8. **CLI** prints the same object (`--json`) and a short text form; same Unknown rules.
9. **Compatibility.** The object carries `interface_version` (ADR-0044); a client seeing a newer version shows the raw state only.

## Consequences

- Positive: UI and engine can be built in parallel against one contract; no path to display unmeasured claims.
- Cost: an evidence record store (run id, SHA, level) must exist before any `measured` claim can appear; until then every tuple is `unknown`.

## Rejected

- A single "profile ready" flag (hides per-engine differences, ADR-0031 decision 14).
- Showing a default or estimated latency when nothing was measured.
