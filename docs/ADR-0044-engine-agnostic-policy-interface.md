# ADR-0044: Engine-agnostic traffic policy interface

**Status:** Proposed. Nothing is implemented.
**Date:** 2026-11-17
**Related:** [ADR-0031](ADR-0031-adaptive-traffic-policy-pack.md) decisions 11-14, [ADR-0018](ADR-0018-engine-framework.md), [ADR-0019](ADR-0019-baft-engine-host.md), research [POLICY_PACK_TP-0.1_RESEARCH.md](POLICY_PACK_TP-0.1_RESEARCH.md), issue [#245](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/245) (TP-2.0)

## Context

ADR-0031 requires a stable, versioned, carrier-agnostic policy interface and per-engine capability declaration before any queue, scheduler or profile is built. TP-0.1 found no BAFT-TUNNEL-owned queue between the relay and the engine today; the relay (`pump_with_idle_timeout`) is the first hook (H1) and is engine-neutral.

## Decision (proposed)

1. **Policy sits in the relay layer, above engines.** The interface sees flows and chunks of data, never TLS records, Noise records or Yamux frames. Engines are not modified (`StreamEngine` stays as in ADR-0018).
2. **Flow descriptor.** `FlowId` (opaque, per Yamux stream), `Class` (from explicit configuration, ADR-0031 decision 6: one of Balanced, Gaming, Trading, Streaming, Bulk), `Direction`. No payload inspection.
3. **Decision points.** `admit(flow) -> Admit | Reject(reason)` when a stream opens; `grant(flow, wanted_bytes) -> Grant { bytes, delay }` before each relay read/write; `observe(flow, event)` for byte counts and queue depth. All calls non-blocking and bounded; `grant` never returns a wait longer than the profile's bounded-wait limit (ADR-0031 decision 5).
4. **Capabilities.** A `Capabilities` value per (engine, carrier): `priority_enforcement`, `write_pacing`, `rtt_observation`, `queue_observation`, `datagram`. Each is `Supported | Unsupported | Unknown`; the default is `Unknown`. Anything not `Supported` is shown as such in API, CLI and panel and never as "ready" (ADR-0031 decision 14).
5. **Datagram-ready.** `grant` and `observe` work on units (`Unit::Bytes(n)` now, `Unit::Datagram` reserved) so the UDP carrier (#244) can be added without redesign. No datagram code in this ADR.
6. **Off path equals today.** With the pack disabled the relay uses the existing fixed-buffer pump byte-for-byte; the interface is only constructed when the pack is enabled. A policy error, panic or stale metric falls back to Balanced and logs the cause (ADR-0031 decision 9).
7. **Versioning.** `POLICY_INTERFACE_VERSION = 1`; a profile or engine declares the version it supports; mismatch means the pack stays off for that tuple.
8. **Out of scope.** Queue and scheduler algorithms (TP-2.1), profile parameters (TP-3.x), the BAFT Go engine (stays out per ADR-0031 decision 13 and ADR-0019), any kernel or privileged action (ADR-0030).

## Consequences

- Positive: one interface for all current and future engines; capability gaps are explicit; the off path is unchanged.
- Cost: an indirection in the relay when the pack is on; its overhead must be measured in TP-1.2 before TP-2.1 (harness status: see research doc section 8).
- Risk: the relay may be too far from the wire to improve latency (data already inside Yamux or kernel buffers). The evidence gate (ADR-0031 decision 10) applies; the result can be "no gain".

## Rejected alternatives

- Per-engine policy code (duplication, inconsistent claims).
- Policy inside the engine wrapper (H3): cannot see streams after multiplexing.
- Defining the scheduler API together with the interface: couples this ADR to unmeasured design.
