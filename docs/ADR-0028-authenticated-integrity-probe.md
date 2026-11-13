# ADR-0028: Authenticated in-product integrity probe

**Status:** Design approved; B2 implementation-start gate satisfied on 2026-11-12; B2 runtime merged into `main` on 2026-11-12 (PR #177 through PR #179); B2 feature acceptance remains pending (the latest trusted B2 two-host run failed).
**Scope:** Phase 1 B2, the panel's **Test tunnel** action and the integrity evidence tier.
**Builds on:** ADR-0011 (authorization), ADR-0021 (Direct/Reverse topology), ADR-0022 (Apply/Verify/Receipt), ADR-0023 (agent control boundary), ADR-0024 (operations evidence), and `docs/READINESS_CONTRACT.md`.

## Context

ADR-0024 deliberately leaves the agent's `integrity` tier Unknown: the agent does not read tunnel payload, and counters prove only that bytes moved in both directions. The current `StreamOpen` contract authorizes only `TcpForward`. B2 adds a bounded, authenticated probe stream answered by the remote serve node so an operator can verify that a known payload crossed the actual tunnel unchanged.

The existing U7 two-host 64 KiB/2 MiB checks remain release/LAB evidence. They do not provide an on-demand, in-product result for the operator's currently configured tunnel. A successful probe is evidence for that probe and time only; it is not a production-readiness decision.

## Accepted design decisions

The project owner approved the following B2 design choices:

1. **Protocol version 2; no silent downgrade.** B2's `IntegrityProbe` capability requires protocol v2. A peer that does not support the required version/capability must be reported as unsupported; the implementation must not silently downgrade, reinterpret an old stream or treat an older peer as B2-compatible. The minimum B2-compatible peer version is v2. Before implementation, document and test the peer-upgrade/compatibility matrix, including the effect on existing v1 tunnel sessions; do not claim mixed-version interoperability without evidence.
2. **Per-instance Unix-domain control socket with peer credentials.** Each tunnel unit exposes a narrowly scoped socket in its systemd `RuntimeDirectory`. The tunnel unit authenticates the connecting process using `SO_PEERCRED` and permits only the registered `baft-tunnel-agent` UID. Filesystem permissions/group membership are an additional restriction, not a substitute for peer-credential authorization. Missing credential support or an unexpected UID fails closed.
3. **Fixed resource limits.** Each probe transfers exactly 64 KiB; at most one probe may be active per tunnel instance; a new probe may start no more than once per 60 seconds per tunnel instance; and the complete operation has a 10-second end-to-end deadline.

These are approved design decisions, not evidence that the feature is implemented or accepted. The prerequisites for starting B2 implementation and the overall Phase 1 closeout gate are separated below; completing B2 cannot be a prerequisite to starting B2 itself.

## Accepted peer-upgrade policy (owner decision, 2026-11-12)

The project owner selected **Option A — strict coordinated peer-pair upgrade** from `docs/PROTOCOL_V2_COMPATIBILITY_MATRIX.md`.

- Existing v1↔v1 sessions may continue while both running endpoints remain on v1.
- New sessions require matching protocol generations: v1↔v1 or v2↔v2.
- Any new mixed v1↔v2 session is rejected. There is no automatic fallback and no optional legacy-v1 compatibility mode.
- Both endpoints of each tunnel pair must be upgraded in one coordinated maintenance window.
- If one endpoint restarts or is rolled back to v1 while its peer is v2, a new session is unavailable until both endpoints are restored to the same protocol generation.
- This decision does not claim zero-downtime rolling upgrades and does not change the current implementation baseline: main remains protocol v1 until a separately gated v2 implementation is completed.

The compatibility matrix and pre-implementation contract tests must implement this accepted policy. The implementation-start prerequisites are now satisfied: U7 and B1 are accepted, the independent F0 reviewer approved the design-level trust boundary, and the Option A contract tests passed on exact head `babdb7f5bb82d8235f8f9e8f1145f843c2f8b735` and merged as PR #176 (`74b8ba33b2144c7537f710c720e70e6c25ebfca1`). B2 runtime implementation merged in PR #177 (through PR #179, `4c8dffd`), and the trusted two-host harness merged in PR #178. This does **not** constitute B2 feature acceptance; the trusted B2 run on `fbac1db` failed in its orchestration step, so the two-host LAB and the remaining acceptance evidence are still required.

## 1. Dedicated capability; no reuse of TCP forwarding

- Add a distinct `IntegrityProbe` stream capability. It must never open the configured service destination, enter the TCP-forward relay path, or be accepted as `TcpForward`.
- The probe runs only inside an already authenticated and authorized peer session. It binds to the session's authenticated peer identity, tunnel ID and connection direction; the remote responder accepts no caller-supplied host, port, path or command.
- Update the session capability contract and stream-open contract together. Protocol v2 is required for B2. Reject unsupported versions/capabilities explicitly; do not silently downgrade, reinterpret an old stream or assume mixed-version peers support B2. Document and test the peer-upgrade/compatibility matrix before implementation.
- Direct and Reverse use the same probe semantics. The node that owns the local Test tunnel action initiates a probe stream; the remote service-side node responds over that stream. Reverse remains unavailable until its independent real implementation and acceptance gate under ADR-0021 are satisfied.

## 2. Probe exchange

- The initiator generates a fresh cryptographically random challenge payload. The payload is exactly 64 KiB; the user cannot choose an arbitrary size.
- The responder reads exactly the fixed length in bounded chunks, computes SHA-256 while streaming, and returns a small framed response containing the protocol version, request nonce, byte count and digest.
- The initiator independently hashes the bytes it sent and accepts success only when the nonce, exact byte count and digest all match. Truncation, extra bytes, malformed frames, mismatch, timeout, disconnect or unsupported capability is a failure/Unknown result, never a pass.
- Frames are length-prefixed and strictly bounded before allocation. The implementation must not buffer an unbounded payload or allocate based on an untrusted length. No payload or digest-derived user data is logged; only result metadata is retained.

## 3. Resource and abuse limits

The approved initial limits are:

- Exactly 64 KiB per probe; no client-controlled payload length.
- One active probe per tunnel instance.
- At most one probe start per tunnel instance per 60 seconds.
- A 10-second end-to-end deadline, including stream open, transfer and response.
- Fixed small response frame; bounded concurrency and memory independent of peer input.

The tunnel process enforces these limits, not only the panel or agent. Rejected requests return a typed reason and do not consume an unbounded task, stream slot or memory. Any proposed tuning requires measured LAB evidence and a reviewed ADR update.

## 4. Agent-to-unit control boundary

The agent is a separate Unix user from the tunnel process (ADR-0023 / SA-09); it cannot directly access the in-memory Yamux session. The approved local control path is:

- The tunnel unit creates a per-instance Unix-domain control socket inside its systemd `RuntimeDirectory`.
- The unit verifies the connecting process using `SO_PEERCRED` and accepts the probe operation only from the registered `baft-tunnel-agent` UID. Filesystem mode/group membership is defense in depth, not authorization.
- The socket protocol exposes only a fixed `run_integrity_probe` operation for that unit's configured peer/tunnel. It accepts no arbitrary destination, command, config path or payload size.
- Socket creation, permissions, stale-socket cleanup, process restart, instance isolation, request framing and timeout behavior require tests. If peer-credential verification is unavailable or the caller UID is wrong, fail closed.

The local agent API remains bearer-token protected and loopback-only. The panel invokes one explicit user action; no background or automatic probing is enabled by default.

## 5. API, diagnostics and readiness semantics

- Add an explicit authenticated `POST /v1/verify` operation (final route name may be settled during implementation) with no caller-controlled network target or payload size. It returns a typed result: `pass`, `fail`, or `unknown`, exact payload size, SHA-256 match boolean, peer/tunnel identity, timestamp, duration and a stable reason code. It must not return secrets or raw peer errors.
- Audit the request and outcome using ADR-0024's actor/target/resource model; resource is a dedicated integrity-verification action. Preserve the request idempotency and audit durability conventions already used by mutating/verification endpoints.
- `integrity` may become `pass` only from a successful authenticated B2 probe. A missing, stale, failed, unsupported or unverifiable result is not a pass. The report must identify the source as the in-product probe and expose its timestamp/age.
- A pass is scoped to this peer, tunnel, direction, payload size and probe time. It does not imply every application flow is intact, prove censorship resistance, or guarantee future health.
- The live integrity tier may show Pass only while the most recent successful probe is no older than 60 seconds (the existing data-path evidence window). After expiry, the live tier returns to Unknown while the last-probe record remains available as historical evidence. This freshness rule never changes Apply receipt ready.
- `ready` and Apply receipt readiness remain `unknown` in every outcome. A successful probe must not set a production-ready state, change Apply state, or promote unrelated evidence tiers.

## 6. Panel behavior

- Add a clearly labelled **Test tunnel** action to the existing tunnel detail/diagnostics flow.
- Require an explicit click; show progress, result, timestamp and a plain-language explanation of the limited claim.
- Distinguish pass, failed integrity, unavailable peer/session, unsupported peer version, rate limit and timeout. Never render Unknown as success.
- Keep technical values LTR inside Persian RTL layout. Do not expose raw payload or internal socket details.

## Required verification

### Unit and protocol tests

- Correct payload and digest pass; altered payload/digest, wrong nonce, wrong byte count, truncation, extra bytes, malformed/unknown fields and unsupported version fail closed.
- Replay of a prior response is rejected; nonce is unique per request and bound to the active session/tunnel.
- Cross-peer, cross-tunnel and wrong-direction probe requests are rejected before payload processing.
- `IntegrityProbe` cannot reach the fixed TCP service target and cannot be treated as `TcpForward`.
- Oversized frames are rejected before allocation; timeout, disconnect, cancellation and concurrent stream exhaustion release all permits.
- One-active/one-per-60-second limits hold under concurrent requests and across repeated API calls.
- Unix socket peer-credential, wrong UID, stale socket, restart, permission and instance-isolation tests pass.
- Protocol-v1/older peers produce a clear unsupported result for B2; there is no silent fallback. Existing v1 session compatibility/upgrade behavior is tested explicitly.
- API audit records actor/target/resource/result without token, payload, private data or raw network errors. `ready` remains Unknown for pass, fail and unavailable outcomes.

### Integration and LAB

- Local two-process integration proves the full Agent → local socket → active tunnel session → remote responder → digest comparison path.
- Exercise Direct and Reverse separately using the same capability and probe protocol. Reverse testing is blocked until the real Reverse implementation passes its separate ADR-0021 gate.
- A normal probe passes on an intact tunnel. A controlled test-harness corruption/truncation injection fails; do not add a production tamper switch.
- Real two-host LAB records exact code SHA, binary SHA, direction, payload size, digest comparison, duration, resource-limit behavior and cleanup. Run one real-host workflow at a time. No persistent firewall, route, sysctl or service configuration changes.
- Test with an older peer and prove a clear unsupported-version result; no silent fallback.

## Non-goals

- No generic throughput benchmark, speed-test claim, arbitrary payload upload, arbitrary TCP destination, service-port scan or continuous monitoring.
- No transparent stream migration, traffic shaping, padding, censorship-resistance or traffic-invisibility claim.
- No Apply/rollback, firewall mutation, automatic remediation, stable release or production-readiness change.

## B2 implementation-start gate

The following prerequisites defined the B2 implementation-start gate:

1. **U7 accepted:** the signed-RC.7 integrated MVP and trusted two-host acceptance remain the accepted product baseline.
2. **B1 accepted and merged:** structured Preflight/Diagnostics evidence and bilingual localization are merged in main (PR #168, merge `311b706`) with all exact-head CI groups green.
3. **F0 independently reviewed and approved:** a separate trust-boundary/security review must cover the Agent-to-unit socket and process-identity boundary, exact `SO_PEERCRED` UID enforcement, socket ownership/permissions and per-instance isolation, socket lifecycle/restart/stale-socket handling, bounded framing/backlog/deadline, concurrency/rate/resource exhaustion, fail-closed behavior, audit boundaries, local Agent API exposure, and confirmation that the probe grants no systemd/root-helper authority. A static self-review or this ADR alone does not satisfy F0.
4. **Protocol-v2 compatibility plan accepted:** document the v1/v2 peer-upgrade matrix, mixed-version rejection behavior, no-downgrade guarantee, and impact on new versus already-established v1 sessions; define negative and two-version interoperability tests before implementation.

**Gate status (2026-11-12): SATISFIED.** The prerequisites above have been met and B2 runtime/API/panel implementation merged into `main` (PR #177 through PR #179); the trusted B2 harness merged in PR #178. This gate deliberately does not imply that B2 itself is complete or accepted.

## B2 feature-acceptance gate

After implementation, B2 is not accepted until all required unit, protocol, integration and real two-host LAB evidence in this ADR passes. Direct and Reverse evidence must be reported separately; Reverse remains unavailable until its independent implementation and acceptance gate under ADR-0021 passes. A successful probe proves only the measured probe and must never set global `ready`.

## Overall Phase 1 closeout / Phase 2 gate

The overall Phase 1 closeout and any Phase 2 implementation remain gated on **all B1–B6 items being accepted and evidenced**, including B2's own implementation and feature-acceptance tests above. This is the aggregate completion gate, not a prerequisite for starting B2.

B2 runtime implementation may proceed under the approved design. This ADR does not authorize host mutation, real-host LAB, release, deployment or Phase 2. Until B2 feature acceptance, the integrity tier remains Unknown except for separately recorded probe/Verify/LAB evidence.
