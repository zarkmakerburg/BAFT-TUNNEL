# Protocol v1/v2 Peer Compatibility and Upgrade Matrix

**Status:** Proposed — owner decision required; this document does not authorize B2 runtime implementation.  
**Scope:** ADR-0028 B2 implementation-start prerequisite.  
**Baseline:** `main` at `7bf0b9ab1ae7c905d2ae8b1c6d3b369ed2a59b0a` (2026-11-12).  
**Related:** [ADR-0028](ADR-0028-authenticated-integrity-probe.md), [ADR-0011](ADR-0011-authorization.md), [ADR-0024](ADR-0024-operations-evidence.md), [Readiness contract](READINESS_CONTRACT.md).

## 1. Verified current protocol baseline

The current mainline source is protocol v1 only:

- `src/handshake.rs` defines `VERSION: u16 = 1`.
- Client and server handshake validation reject a version other than `VERSION`.
- `StreamOpen` also carries the same version and the server rejects a mismatch.
- `src/authorization.rs` defines only `StreamCapability::TcpForward`; `PHASE1_CAPABILITIES` contains only that capability.
- Existing handshake tests cover successful v1 handshakes and rejection of version/binding/capability mismatches.

Consequently, the current source does **not** implement v2, `IntegrityProbe`, mixed-version negotiation, or a typed v1/v2 compatibility response. This document must not be read as evidence that any of those behaviors already work.

## 2. Compatibility matrix — proposed strict policy

The following is the conservative proposed policy consistent with ADR-0028's prohibition on silent downgrade. It requires explicit owner acceptance before it becomes the implementation contract.

| Initiator | Responder | New session | TCP forwarding | IntegrityProbe | Required operator-visible result |
|---|---|---|---|---|---|
| v1 | v1 | Accept using v1 | Supported as today | Not available | Existing v1 behavior; integrity remains Unknown |
| v2 | v2 | Accept using v2 | Supported | Supported only when separately authorized in the v2 capability set | Probe returns pass/fail/unknown for that probe only |
| v2 | v1 | Reject under strict policy | No new session under strict policy | Unsupported | Explicit incompatibility; no silent v1 fallback |
| v1 | v2 | Reject under strict policy | No new session under strict policy | Unsupported | Explicit incompatibility; no reinterpretation as TcpForward |
| v1 | v2, legacy mode explicitly enabled | Accept only if the owner selects the optional compatibility policy in §4 | TcpForward only | Always unavailable | Explicitly selected legacy-v1 mode; never automatic |

### Session continuity during rolling upgrades

A process that is already running v1 may keep its existing in-memory v1 session until that process or connection is restarted or lost. This is not a guarantee that a v1 session survives a service restart, binary replacement, network interruption, or reconnect.

Under the strict policy above, once either endpoint restarts into a v2-only build, a new v1↔v2 session is rejected. Therefore, peer pairs must be upgraded in a coordinated maintenance window, or the owner must explicitly approve the optional legacy compatibility policy in §4. Do not claim zero-downtime rolling upgrades.

## 3. No-downgrade and capability rules

Regardless of the owner-selected mixed-version policy:

1. A v2 session must identify its selected protocol version explicitly; the selected version must be authenticated/bound to the handshake transcript.
2. `IntegrityProbe` is a distinct capability. It must never be inferred from `TcpForward`, mapped to a generic stream, or sent through the TCP-forward destination path.
3. A v1 peer must never be treated as B2-capable.
4. A v2 probe request to a peer/session without v2 `IntegrityProbe` support returns a typed unsupported result where the peer protocol can express it. If the old peer simply closes or cannot return that error, the caller must report a bounded handshake incompatibility/unknown result rather than inventing a confirmed peer version.
5. No implicit retry using v1 is allowed after a v2 negotiation or probe failure.
6. If explicit legacy-v1 mode is approved, it must be operator-configured, visible in diagnostics, restricted to `TcpForward`, and incapable of invoking `IntegrityProbe`. It is not a fallback path and must not be enabled automatically.
7. Version selection and capability authorization are separate checks. A v2 handshake alone does not authorize `IntegrityProbe`; the authenticated session must advertise and authorize that capability.

## 4. Owner decision required: mixed-version data-plane behavior

ADR-0028 already decides that B2 requires v2 and forbids silent downgrade. It does not fully decide whether a v2-capable node may explicitly preserve ordinary v1 TCP forwarding with an older peer.

Choose one policy before implementation:

### Option A — strict coordinated upgrade (recommended for the smallest security surface)

- v1↔v1 continues unchanged while both endpoints remain on v1.
- v2↔v2 supports v2 and B2.
- Any new v1↔v2 session is rejected; no automatic fallback.
- Upgrade both ends of each tunnel pair in one coordinated maintenance window.
- A restart/reconnect between the two upgrades may leave the tunnel unavailable until both endpoints are v2.

### Option B — explicit legacy-v1 data-plane compatibility

- v2 nodes may accept a v1 session only when a dedicated, explicit operator setting enables legacy compatibility for that peer/tunnel.
- The selected protocol is recorded and shown as v1; no silent negotiation or retry is permitted.
- The session is limited to the existing v1 `TcpForward` capability.
- `IntegrityProbe` is unavailable and must return unsupported for that session.
- Default is disabled; configuration must not be silently migrated to enable it.
- Tests must prove that the legacy mode cannot reach the v2 probe handler and that disabling it rejects the mixed-version session.

**No option is accepted by this document.** The owner must select A or B and record the decision in ADR-0028 before B2 implementation starts.

## 5. Upgrade and rollback matrix

| Upgrade state | Expected behavior under Option A | Operational requirement |
|---|---|---|
| Both endpoints remain v1 | Existing v1 forwarding continues; no probe | No change |
| Initiator upgraded to v2; responder remains v1 | New handshake rejected | Complete peer-pair upgrade; do not assume reconnect |
| Responder upgraded to v2; initiator remains v1 | New handshake rejected | Complete peer-pair upgrade |
| Both endpoints upgraded to v2 | v2 session; B2 capability may be used after authorization | Run v2 handshake and B2 acceptance tests |
| One endpoint rolled back to v1 | New mixed-version session rejected | Roll back both endpoints or keep the pair offline until versions match |
| v2 peer lacks `IntegrityProbe` authorization | Session may exist, but probe is rejected as unsupported/not-authorized | Do not infer support from protocol version alone |

Under Option B, the two mixed-version rows may establish only an explicitly configured v1 `TcpForward` session; probe remains unavailable. Rollback must not auto-enable compatibility.

## 6. Required compatibility tests and evidence

### Existing v1 baseline tests (already present; rerun against the exact baseline)

- v1↔v1 client/server handshake succeeds with matching identities, tunnel and direction.
- v1 handshake rejects unsupported version values.
- v1 stream-open rejects a version mismatch.
- Empty, unknown or duplicate capabilities are rejected.
- Only `TcpForward` is authorized in the current v1 capability contract.

These tests establish the current v1 baseline only. They do not establish v2 or mixed-version interoperability.

### Pre-implementation contract tests (required before the B2 runtime feature work)

Before B2 implementation is authorized, add and run isolated protocol-contract tests for the selected Option A or B. They must not require an `IntegrityProbe` data-path implementation:

- Version-selection test vectors prove the exact accepted/rejected version pairs.
- No-downgrade test proves that a failed v2 negotiation never triggers an implicit v1 retry.
- Capability-separation test proves v1 can never advertise or authorize `IntegrityProbe`.
- Typed incompatibility test proves a peer incompatibility is not misreported as a network success or probe pass.
- Session-continuity test documents the boundary: an existing in-memory v1 session is not a promise of reconnect compatibility after restart.
- If Option B is selected, explicit configuration tests prove legacy mode defaults off, is visible, authorizes only v1 `TcpForward`, and cannot invoke B2.

Record exact source SHA, test command, test result and logs. A written test plan or green unrelated CI is not test evidence.

### B2 feature tests (after implementation; required for B2 acceptance, not a pre-implementation test)

- v2↔v2 handshake and `IntegrityProbe` authorization.
- v2 probe to v1 peer produces unsupported/incompatibility and never falls back.
- `IntegrityProbe` cannot be interpreted as `TcpForward` or reach a configured TCP service target.
- Both directions of the mixed-version matrix are tested.
- Upgrade/restart/reconnect and rollback behavior is tested on both endpoint roles.
- Real two-host LAB records exact binaries, protocol versions, selected policy, session result, probe result and any outage/recovery interval.

## 7. Gate interpretation

This document is a proposed compatibility contract only.

The ADR-0028 B2 implementation-start gate remains **BLOCKED** until:

1. The owner selects Option A or Option B and that decision is recorded in ADR-0028.
2. The pre-implementation contract tests above are implemented and pass on an exact SHA.
3. The independent F0 trust-boundary/security review is approved, including UDS lifecycle, safe stale-socket handling, exact `SO_PEERCRED` UID enforcement, bounded framing and fail-closed behavior.
4. U7 and merged B1 remain accepted.

This document does not authorize B2 runtime/API/panel implementation, protocol migration, LAB, host changes, release, deployment, or Phase 2 work.

## 8. Evidence references

- Current main baseline: https://github.com/zarkmakerburg/BAFT-TUNNEL/commit/7bf0b9ab1ae7c905d2ae8b1c6d3b369ed2a59b0a
- Current handshake source: https://github.com/zarkmakerburg/BAFT-TUNNEL/blob/main/src/handshake.rs
- Current authorization source: https://github.com/zarkmakerburg/BAFT-TUNNEL/blob/main/src/authorization.rs
- B2 design and gate: https://github.com/zarkmakerburg/BAFT-TUNNEL/blob/main/docs/ADR-0028-authenticated-integrity-probe.md
