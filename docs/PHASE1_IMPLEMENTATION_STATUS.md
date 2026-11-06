# Phase 1 implementation status

**Authorization:** Owner-approved 2026-11-06.
**Branch:** phase1/rust-tcp-transport.
**Latest validated runtime/installer code head:** 5af31f942fe5b20cb4005787c40801a8441de9ae.\n**Current documentation/guardrails head:** 8866b439be7bd0ba4180e9eab59a1f9235a8c262.
**Runtime product-code SHA:** 775e1c5afcf1582c01d8ca01c25874bcda552bee.
**Status:** In progress; not production-ready. ADR-0010 is the current transport decision; ADR-0009 is superseded. Offline PKI tooling is defined by ADR-0012.

## Implemented

- Rust CLI with explicit serve and forward roles.
- Direct and Reverse use the same TCP + TLS 1.3 mutual-authentication + bounded Yamux adapter; no plaintext, legacy TLS, or silent downgrade.
- TLS 1.3 early data is disabled on both peers.
- Peer leaf certificates are validated against the configured CA, a SHA-256 leaf pin, and exactly one URI SAN identity (urn:baft-tunnel:node:<node-id>) using x509-parser. Missing, duplicate/conflicting, malformed, or trailing certificate data is rejected.
- Both TLS client and server require a PEM CRL. Rustls rejects revoked certificates, denies unknown revocation status by default, and enforces CRL expiration. Missing/empty CRL files fail closed during configuration loading.
- Session authorization binds the TLS-authenticated peer identity, expected peer, tunnel ID, connection direction, and allowed capability set. Phase 1 currently allows only tcp-forward; empty, duplicate, or unknown capabilities fail closed.
- Each Yamux stream starts with a bounded, versioned authorization frame binding tunnel ID, direction, and capability before the server connects to its fixed destination.
- Fixed unicast IP:port destination; a peer cannot choose an arbitrary target or trigger DNS resolution.
- Bounded handshake frames, handshake/connect deadlines, session and stream admission, Yamux receive windows, and pending local-connection queues.
- Source-side reconnect supervisor uses capped exponential backoff and jitter. In-flight TCP streams are not resumed after carrier loss.
- Strict JSON config validation, redacted plan, explicit-confirm apply, and redacted support bundle.
- Experimental offline PKI helper (scripts/pki/baft-pki.sh): encrypted P-384 offline CA, per-node EC P-256 key/CSR generation, CA-supplied URI plus DNS/IP SANs, certificate issuance, CRL generation/revocation, and certificate/CRL verification. The helper never copies the CA key into a node bundle.
- Node private keys remain unencrypted PEM in the current LAB workflow. Runtime JSON now supports a mutually exclusive key_credential source resolved from systemd's CREDENTIALS_DIRECTORY, and validation/apply fail closed if it is absent or invalid. Encrypted delivery still requires a per-instance LoadCredentialEncrypted= drop-in on systemd 250+; it has not yet been proven on a qualifying Linux host. This is not yet a closed production key-custody gate.
- A hardened systemd template and install/rollback/uninstall scripts are implemented. Staged installer lifecycle tests cover install, permissions, systemd hardening assertions, backup/rollback, safe uninstall, and explicit config purge. They do not yet prove an actual Linux/systemd host lifecycle.
- Noise IK prototype remains isolated research code and is not wired into the production data plane.

## Real-host LAB evidence — PASS

Run [42246759289](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42246759289), testing runtime product-code SHA 775e1c5afcf1582c01d8ca01c25874bcda552bee on the two configured LAB hosts:

- Direct (Iran dials abroad): 2,097,289 bytes, SHA-256 1bb59406b95f296d1ffac9536e3c14a5dfc67c2632ca9ae26d1c85ca8b414fa8, 0.889 s.
- Reverse (abroad dials Iran): same byte count and SHA-256, 0.830 s.
- 64 KiB probes passed in both directions.
- After restarting the abroad listener, Direct forwarding recovered and passed a further 64 KiB integrity probe in 0.334 s.
- Revocation test 1: the server rejected the revoked Iran client certificate.
- Revocation test 2: the Iran client rejected the revoked abroad server certificate.
- Workflow cleanup passed. Only temporary binaries, certificates, and processes under /tmp were used. No firewall, route, sysctl, systemd, or persistent host configuration was changed.

## Security-negative and PKI tests

- TLS integration tests reject expired and untrusted client certificates and expired and untrusted server certificates.
- TLS tests reject missing and malformed CRLs and a parseable CRL signed by the wrong issuer on both TLS roles.
- Existing identity tests reject missing, duplicate, conflicting, or wrong URI SANs, wrong peer identities, wrong tunnel IDs, and wrong directions.
- The offline PKI test creates a temporary CA and verifies P-256 CSR checks, URI/DNS/IP SAN issuance, 0600 node-key permissions, CA-key isolation, certificate revocation, and node/server-name mismatch rejection.
- Local Mac validation passed: 24 Rust tests, Clippy with warnings denied, release build, shell syntax, workflow pin checks, LAB orchestration mocks, preflight mocks, NetEm cleanup failure-path tests, and the PKI lifecycle regression.

## Exact-head CI evidence

- Product-code and TLS negative-test commit 705f937f63604876d846ec6e740b90062e1fe747 passed Phase 1 CI [42248834201](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42248834201): formatting, all Rust tests, Clippy with warnings denied, and release build.
- Current branch head 1597e4bf4258a3ae006b108e8998d29c477a816b passed Phase 1 CI [42250860492](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42250860492): formatting, all Rust tests (including expired/untrusted peer and CRL issuer-mismatch negatives), Clippy with warnings denied, and release build.
- Runtime/installer code head 5af31f942fe5b20cb4005787c40801a8441de9ae passed Phase 1 CI [42259334888](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42259334888) and Phase 0 CI [42259334926](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42259334926). Phase 0 included installer staged lifecycle tests, shell validation, immutable action pins, mock SSH/SCP orchestration, offline PKI lifecycle, Docker Compose validation, and isolated Docker/netem integration. Phase 1 included formatting, Rust tests, Clippy with warnings denied, and release build.\n- Documentation-only guardrails commit 8866b439be7bd0ba4180e9eab59a1f9235a8c262 adds this policy and does not change runtime code; no new runtime test claim is made for that documentation-only commit.

## Engineering guardrails

The mandatory cross-project engineering policy is documented in [BAFT_LESSONS_AND_ENGINEERING_GUARDRAILS.md](BAFT_LESSONS_AND_ENGINEERING_GUARDRAILS.md). It converts lessons into requirements for truthful connectivity state, safe desired-state apply/rollback, security boundaries, simple installation/UX, LAB evidence classification, CI enforcement, and release gates.

## Mandatory remaining Phase 1 acceptance work

- Online enrollment/authorization workflow and control-plane integration. Current PKI helper is manual/offline only.
- Real systemd encrypted-credential delivery and restart/permission integration on systemd 250+; the resolver is wired into config validation and apply, but actual Linux host proof remains open. TPM2-backed provisioning and PKCS#11/TPM signing remain separate follow-up work.
- Automated CRL publication/distribution, CRL refresh coordination, safe CA/pin rollover, and overlapping pins. CRL updates are currently supplied out of band; restart both peers to apply them.
- Negative tests for expired CRLs and additional trust-anchor/CA lifecycle failure modes.
- Further replay-sensitive operation, framing fuzz, session-isolation, adversarial resource-exhaustion, reconnect/drain, and concurrency testing.
- Actual Linux/systemd host installation, service lifecycle, upgrade, rollback, and uninstall evidence. Staged lifecycle tests pass, but real-host installation is NOT RUN.
- Dependency advisory/license audit, protocol-boundary fuzzing, and independent security review.
- Longer-duration LAB soak and controlled packet-loss/latency measurements.

## Later work (not Phase 1 acceptance blockers)

- QUIC adapter using native QUIC streams, with UDP-disabled fallback verification.
- ECH deployment evaluation where DNS and server infrastructure support it; document residual metadata.
- Relay topology with end-to-end encryption between tunnel endpoints.
- Noise interoperability/security comparison; do not route production traffic through the prototype without a new accepted ADR.

## Release boundary

This evidence is not a security certification. Phase 1 remains Draft and is not production-ready. No production host mutation, release artifact publication, or deployment has been performed. Do not merge into the release branch or publish a product release until the remaining mandatory acceptance gates are closed and independently reviewed.
