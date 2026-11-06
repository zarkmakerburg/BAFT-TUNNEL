# Phase 1 Execution Plan — Offline Preparation

**Status:** Preparation proposal only. This document does not authorize Phase 1 implementation, merge, phase transition, real-host access, release, or deployment.

**Prepared because:** the six-host LAB is temporarily unavailable. Work in this document is limited to decisions, interfaces, and verification design that can be reviewed offline. No host credentials or live network assumptions are required.

## 1. Phase 1 outcome

Deliver the smallest independently testable Rust data-plane foundation that can establish an authenticated node-to-node session and forward a bounded TCP stream over it, with a safe CLI and a disposable-host installer design.

Do not claim production readiness, censorship resistance, UDP support, transparent migration of established streams, or real Iran-to-abroad performance from local tests.

## 2. Proposed scope and order

### P1.1 — Protocol and dependency decision
- Review candidate Rust multiplexing libraries for maintenance, license, async compatibility, bounded memory, flow control, cancellation, graceful drain, and security history.
- Record the selected library and version in an ADR before implementation.
- Define a versioned handshake: protocol version, node identities, tunnel ID, capabilities, fresh nonce, and explicit rejection behavior.
- Define frame and stream limits, backpressure behavior, timeouts, and downgrade rejection.
- No custom cryptography or custom TLS implementation.

**Exit evidence:** ADR approved; dependency/license review; protocol state diagram and compatibility rules.

### P1.2 — Authenticated transport core
- Rust workspace boundaries: protocol/types, identity, transport adapter, engine, CLI.
- TLS 1.3 mutual authentication using rustls; verify pinned node identity in addition to certificate-chain validation.
- Use separate long-term node identity and transport credentials.
- Enforce fresh nonces and replay-resistant handshake semantics.
- Bound concurrent sessions, streams, buffers, and per-peer resource use.
- Support reconnect with bounded exponential backoff and jitter; graceful drain and cancellation.
- Keep payload bytes out of logs, traces, metrics, and error messages.

**Exit evidence:** unit and loopback integration tests; negative authentication, wrong identity, replay, downgrade, malformed frame, oversized frame, resource exhaustion, cancellation, reconnect, and drain tests.

### P1.3 — TCP forwarding vertical slice
- Establish one authenticated peer session and forward one explicitly configured TCP listener to one destination.
- Make listener binding explicit; refuse unexpected address/port conflicts.
- Validate destination and configuration before opening listeners.
- Return truthful lifecycle states: stopped, starting, connected, degraded, failed, unknown.
- Do not report healthy merely because a control connection or heartbeat exists.
- Keep UDP out of this first vertical slice.

**Exit evidence:** loopback client-to-destination byte-integrity test; concurrent stream test; disconnect/reconnect test; bounded-resource test; proof that unrelated listeners and services remain untouched.

### P1.4 — CLI and configuration
- Provide inspect/validate/plan commands before any mutating command.
- Display a human-readable preview of listener, destination, peer identity, and resource changes.
- Require explicit confirmation for apply; support non-interactive mode only with an explicit machine-readable authorization flag.
- Redact private keys and tokens from output and support bundles.
- Use strict configuration parsing; reject unknown security-sensitive fields and unsafe defaults.

**Exit evidence:** CLI contract tests; invalid-config tests; redaction tests; plan/apply idempotency tests.

### P1.5 — Installer design and disposable-host implementation
- Target the supported Linux distributions explicitly; detect unsupported systems before mutation.
- Install a versioned binary and dedicated unprivileged service account.
- Keep privileged operations narrow and separately auditable.
- Do not change firewall rules, sysctls, routing, or unrelated services in the initial installer.
- Use systemd service hardening; verify file ownership/modes and service state.
- Support install rerun, uninstall, and rollback without deleting resources not provably owned by this installation.

**Exit evidence:** disposable-VM tests for fresh install, rerun, interruption, upgrade/rollback, uninstall, reboot persistence, and preservation of unrelated services. Real six-host LAB remains a separate later gate.

## 3. Required security invariants

1. No unauthenticated data-plane session.
2. No fallback to plaintext or weaker protocol on authentication/negotiation failure.
3. No unbounded queues, buffers, streams, or peer-created resource allocation.
4. No payload logging or secret exposure.
5. No mutation before validated plan and explicit authorization.
6. No cleanup of resources without verifiable product ownership.
7. Errors and crashes must not be converted into successful/healthy status.
8. Persist security-relevant state with the durability and atomicity requirements in ENGINEERING_GUARDRAILS.md.
9. Any replay protection must be durable before a security-sensitive side effect.
10. A local/loopback test is not real-host or cross-border evidence.

## 4. Test strategy (risk-proportionate)

| Layer | Evidence to produce | What it does not prove |
|---|---|---|
| Static / unit | parsing, limits, state machine, identity checks, frame validation | socket interoperability |
| Loopback integration | mutual-auth session, TCP byte integrity, reconnect, concurrency | public reachability or geographic path quality |
| Fault injection | timeout, reset, malformed input, partial writes, cancellation, resource exhaustion | kernel/power-loss durability unless specifically tested |
| Disposable Linux VM | installer, systemd, permissions, rollback, uninstall, reboot | safety on arbitrary production hosts |
| Six-host LAB | actual SSH preflight and approved path tests when owner configures access | broad internet performance or censorship resistance |
| Cross-border application E2E | real forwarded traffic and measured path behavior under approved conditions | universal availability or undetectability |

Add tests only for concrete risk boundaries and acceptance criteria. Preserve failing runs and tie every result to the exact commit SHA.

## 5. Decisions that must be resolved before code

- Multiplexing library and its license/security review.
- Node certificate/identity issuance, rotation, revocation, and trust bootstrap.
- Exact wire protocol versioning and compatibility policy.
- Durable replay-state storage and crash/power-loss guarantees.
- Supported Linux distributions and minimum kernel/systemd versions.
- Installer privilege boundary and service account capabilities.
- Initial TCP listener exposure defaults and destination policy.
- Whether the first vertical slice uses static peer configuration or a minimal local enrollment flow.

These decisions should be resolved in a short ADR set; avoid implementing a control plane or web panel in Phase 1.

## 6. Explicit exclusions

- UDP/QUIC and WebSocket/HTTP2 adapters.
- Control plane, multi-tenant panel, agent fleet management, desired-state reconciliation, and signed remote updates (Phase 2).
- Web dashboard and setup wizard implementation (Phase 3).
- Firewall automation, sysctl tuning, traffic obfuscation, or claims of invisibility.
- Production deployment, release publication, and changes to existing servers.

## 7. Phase 1 acceptance gate

Phase 1 can be proposed for owner acceptance only when:
- the authenticated TCP forwarding vertical slice works in controlled local/disposable environments;
- identity, replay, downgrade, parser, resource-limit, and secret-redaction tests pass;
- CLI plan/apply behavior is explicit, idempotent, and auditable;
- installer lifecycle tests prove rollback and preservation of unrelated resources;
- all evidence is tied to exact SHAs and accurately labeled by test environment;
- unresolved security risks and limitations are documented.

Real six-host LAB and cross-border E2E remain NOT RUN until access is available and a separately reviewed test plan is authorized.

## 8. Governance

This is an offline planning artifact only. Phase 1 remains **NOT AUTHORIZED** until the owner explicitly approves the phase start. PR #1 remains Draft; no merge, release, deployment, production mutation, or phase transition is implied.
