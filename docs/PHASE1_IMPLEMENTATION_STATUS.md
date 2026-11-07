# Phase 1 implementation status

**Authorization:** Owner-approved 2026-11-06.
**Branch:** phase1/rust-tcp-transport.
**Latest exact-head CI-verified commit:** 5249b3e15624a148c3169aa1696b2bcd65784daf.
**Current security remediation:** Rust 1.88.0 baseline, `time 0.3.47`, removal of unmaintained `rustls-pemfile`; exact-head CI PASS (2026-11-07).
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
- Node private keys remain unencrypted PEM in the current two-host LAB workflow. Runtime JSON supports a mutually exclusive key_credential source resolved from systemd's CREDENTIALS_DIRECTORY; validation/apply fail closed if it is absent or invalid. Real Ubuntu 24.04/systemd 255 hosted-CI integration tests pass encrypted LoadCredentialEncrypted= delivery, actual BAFT TLS-key parsing, controlled-failure restart/redelivery, and a separate ephemeral-host installation lifecycle: installer, encrypted credential drop-in, service start/restart/stop, and safe uninstall. These are ephemeral CI results, not proof of persistent service lifecycle or dedicated operator-host key custody; that production gate remains open.
- A hardened systemd template and install/rollback/uninstall scripts are implemented. Staged installer lifecycle tests cover install, permissions, systemd hardening assertions, backup/rollback, safe uninstall, and explicit config purge. They do not yet prove persistent lifecycle on a dedicated operator-controlled Linux/systemd host.
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
- TLS tests reject expired, missing and malformed CRLs and a parseable CRL signed by the wrong issuer on both TLS roles.
- Existing identity tests reject missing, duplicate, conflicting, or wrong URI SANs, wrong peer identities, wrong tunnel IDs, and wrong directions. Framing regressions reject zero/oversized lengths, truncated payloads, oversized outbound serialization, and duplicate/unknown JSON fields.
- The offline PKI test creates a temporary CA and verifies P-256 CSR checks, URI/DNS/IP SAN issuance, 0600 node-key permissions, CA-key isolation, certificate revocation, and node/server-name mismatch rejection.
- Local Mac validation on the latest runtime/test code passed: formatting, 34/34 Rust tests, Clippy with warnings denied, and diff checks. Earlier local validation also covered shell syntax, workflow pin checks, LAB orchestration mocks, preflight mocks, NetEm cleanup failure paths, and the PKI lifecycle regression.

## Exact-head CI evidence

- Product-code and TLS negative-test commit 705f937f63604876d846ec6e740b90062e1fe747 passed Phase 1 CI [42248834201](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42248834201): formatting, all Rust tests, Clippy with warnings denied, and release build.
- Current branch head 1597e4bf4258a3ae006b108e8998d29c477a816b passed Phase 1 CI [42250860492](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42250860492): formatting, all Rust tests (including expired/untrusted peer and CRL issuer-mismatch negatives), Clippy with warnings denied, and release build.
- Exact head 0267d657bcdff0ac63d175b99fbaa5a1fdac26ac passed Phase 1 PR CI [42283784220](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42283784220) and push CI [42283775796](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42283775796), plus Phase 0 PR CI [42283784213](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42283784213) and push CI [42283775784](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42283775784). Phase 1 includes formatting, 30/30 Rust tests, Clippy with warnings denied, release build, and a real transient systemd 255 encrypted-credential integration run. The systemd test used systemd-creds + LoadCredentialEncrypted=, verified root-owned service-scoped read-only credential access, invoked the actual BAFT config validate/TLS key parsing path as an unprivileged service, and verified fail-closed behavior outside the credential context. Output: BAFT_SYSTEMD_CREDENTIAL_RUNTIME=PASS. This is ephemeral hosted-runner evidence, not persistent operator-host lifecycle proof.
- Exact head 6bca09b3ffbe6ed9d28ee135386c40ef647624ef passed Phase 1 PR CI [42284719217](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42284719217) and push CI [42284711012](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42284711012), plus Phase 0 PR CI [42284719294](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42284719294) and push CI [42284710751](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42284710751). The systemd integration additionally forces one controlled failure after successful key parsing and verifies restart plus credential redelivery; the second invocation succeeds. All four exact-head runs passed.
- Runtime/test head b0eef9f7dbc70e18322d632aa7cd171de2dff872 passed Phase 1 PR CI [42285653584](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42285653584) and push CI [42285645285](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42285645285), plus Phase 0 PR CI [42285653388](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42285653388) and push CI [42285645178](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42285645178). The Rust suite increased to 35 tests with adversarial framing regressions and a deterministic 1,024-case handshake-frame corpus; both Phase 1 event types passed formatting, tests, Clippy, release build and the systemd encrypted-credential + restart/redelivery integration. Latest docs-only head 44cd1fad48d874d55218c1093729d2248c7a8a5b also passed all four workflows [Phase 1 PR 42285785134](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42285785134), [Phase 1 push 42285778351](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42285778351), [Phase 0 PR 42285785095](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42285785095), and [Phase 0 push 42285778285](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42285778285).
- Runtime/installer code head 5af31f942fe5b20cb4005787c40801a8441de9ae passed Phase 1 CI [42259334888](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42259334888) and Phase 0 CI [42259334926](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42259334926). Phase 0 included installer staged lifecycle tests, shell validation, immutable action pins, mock SSH/SCP orchestration, offline PKI lifecycle, Docker Compose validation, and isolated Docker/netem integration. Phase 1 included formatting, Rust tests, Clippy with warnings denied, and release build.
- Documentation-only guardrails commit 8866b439be7bd0ba4180e9eab59a1f9235a8c262 adds this policy and does not change runtime code; no new runtime test claim is made for that documentation-only commit.

## Engineering guardrails

The mandatory cross-project engineering policy is documented in [BAFT_LESSONS_AND_ENGINEERING_GUARDRAILS.md](BAFT_LESSONS_AND_ENGINEERING_GUARDRAILS.md). It converts lessons into requirements for truthful connectivity state, safe desired-state apply/rollback, security boundaries, simple installation/UX, LAB evidence classification, CI enforcement, and release gates.

## Dependency security audit

- The first RustSec audit found `time 0.3.45` (RUSTSEC-2026-0009, stack-exhaustion DoS) and `rustls-pemfile 2.2.0` (unmaintained warning). The vulnerable `time` version was transitive through `x509-parser`/`asn1-rs` and the test-only certificate generator.
- Remediation: upgraded the pinned Rust baseline to 1.88.0 (required by fixed `time 0.3.47`), updated `time`/`time-core`/`time-macros` in `Cargo.lock`, and removed `rustls-pemfile` in favor of the PEM parsing API already re-exported by `rustls-pki-types`. Existing TLS integration tests exercise certificate, CRL, and private-key parsing through the replacement.
- Local RustSec audit on the corrected lockfile: `cargo +1.88.0 audit --no-fetch -D warnings` passed with no vulnerability, unmaintained, unsound, or yanked-dependency findings (157 dependencies scanned). Local `cargo +1.88.0 test --locked --all-targets`: 35/35 passed; Clippy with `-D warnings`, optimized release build, rustfmt check, and `git diff --check` also passed on this updated dependency graph.
- CI now installs pinned `cargo-audit 0.22.1` and runs `cargo audit -D warnings` so advisories and unmaintained/unsound/yanked warnings block Phase 1 CI.

## Latest exact-head CI — PASS

Branch head `e91a83ed5ff98fbf175f1cd1381143e40e5e4e47` passed all four exact-head workflows:

- Phase 1 push [42309114881](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42309114881) and PR [42309118735](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42309118735): pinned Rust 1.88.0, pinned cargo-audit 0.22.1, RustSec audit with warnings denied, formatting, 35/35 Rust tests, Clippy with warnings denied, optimized release build, encrypted systemd credential runtime validation, and installed systemd service lifecycle all passed.
- Phase 0 push [42309114884](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42309114884) and PR [42309118743](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42309118743): passed.
- RustSec scanned the locked dependency graph with no vulnerable, unmaintained, unsound, or yanked dependency findings.
- This exact-head result closes the dependency-remediation CI gate only; it does not close the remaining product/security acceptance gates below.

Previous branch head `f303bfcbd115b9da95fa9b8f3130850066bfdece` passed all four exact-head workflows:

- Phase 1 push [42288011714](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42288011714) and PR [42288020266](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42288020266): formatting, 35/35 Rust tests, Clippy with warnings denied, release build, encrypted systemd credential delivery/runtime key parsing/restart redelivery, and installed systemd service lifecycle all passed.
- Phase 0 push [42288011578](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42288011578) and PR [42288022829](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42288022829): passed.
- The installed-service test ran on a disposable Ubuntu 24.04/systemd 255 hosted runner. It installed the binary and unit, configured `LoadCredentialEncrypted=`, verified service identity, started/restarted/stopped the service, and safely uninstalled/purged the temporary installation. Output: `BAFT_SYSTEMD_SERVICE_LIFECYCLE=PASS`. Runner output warned that its systemd host credential secret file was not on encrypted media; therefore this validates systemd credential mechanism delivery in CI, not TPM-backed or encrypted-host-storage guarantees.
- The first attempt at this lifecycle test failed before host mutation because the test script escaped shell expansions; corrected in `f303bfc`, and both exact-head Phase 1 runs passed.

## Mandatory remaining Phase 1 acceptance work

- Online enrollment/authorization workflow and control-plane integration. Current PKI helper is manual/offline only.
- Dedicated operator-controlled Linux host proof for encrypted credential mode across persistent service start/restart, permissions, upgrade/rollback and uninstall. The ephemeral Ubuntu 24.04/systemd 255 transient-unit integration test passes; the two configured LAB hosts are Ubuntu 22.04/systemd 249 and cannot exercise LoadCredentialEncrypted=. TPM2-backed provisioning and PKCS#11/TPM signing remain separate follow-up work.
- Automated CRL publication/distribution, CRL refresh coordination, safe CA/pin rollover, and overlapping pins. CRL updates are currently supplied out of band; restart both peers to apply them.
- Additional trust-anchor/CA lifecycle failure modes beyond current negative coverage. Expired CRLs are already tested to fail closed on both TLS client and server roles; CRLs signed by the wrong issuer are also rejected.
- Further replay-sensitive operation, framing fuzz, session-isolation, adversarial resource-exhaustion, reconnect/drain, and concurrency testing.
- Dedicated operator-controlled Linux/systemd host installation, persistent lifecycle, upgrade, rollback, and uninstall evidence. Ephemeral Ubuntu 24.04/systemd 255 CI installation and lifecycle now PASS; the two configured LAB hosts are Ubuntu 22.04/systemd 249 and the ephemeral runner is not a persistent operator-controlled host.
- Dependency advisory/license audit, protocol-boundary fuzzing, and independent security review.
- Longer-duration LAB soak and controlled packet-loss/latency measurements.

## Later work (not Phase 1 acceptance blockers)

- QUIC adapter using native QUIC streams, with UDP-disabled fallback verification.
- ECH deployment evaluation where DNS and server infrastructure support it; document residual metadata.
- Relay topology with end-to-end encryption between tunnel endpoints.
- Noise interoperability/security comparison; do not route production traffic through the prototype without a new accepted ADR.

## Release boundary

This evidence is not a security certification. Phase 1 remains Draft and is not production-ready. No production host mutation, release artifact publication, or deployment has been performed. Do not merge into the release branch or publish a product release until the remaining mandatory acceptance gates are closed and independently reviewed.


## Enrollment architecture proposal — ADR-0014

[ADR-0014: online enrollment and control-plane trust boundary](ADR-0014-online-enrollment-and-control-plane.md) is a proposed design only. It defines scoped single-use enrollment grants, digest-only token storage, node-generated keys and CSR proof-of-possession, server-controlled node identity, idempotent signer issuance, crash-safe grant reservation, signed CRL refresh, explicit CA/pin overlap, and control-plane outage isolation. No enrollment endpoint is implemented or exposed, and no CA key is moved online. Issuer model, operator authentication, persistence/backup decisions, and independent security review remain decision gates.

## Latest handshake negative-test head — PASS

- Test-only commit 5249b3e15624a148c3169aa1696b2bcd65784daf adds eight independent client-side checks that reject malformed or mismatched ServerHello bindings, and all four Phase 1/Phase 0 push/PR CI workflows passed on that exact head: Phase 1 PR [42312289460](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42312289460), Phase 1 push [42312283842](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42312283842), Phase 0 PR [42312289434](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42312289434), Phase 0 push [42312283741](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42312283741).
- Follow-up test-only work adds eight independent server-side rejection cases for protocol version, client identity, server identity, tunnel ID, direction, empty/duplicate capabilities, and malformed client nonce.
- Local validation after this follow-up: 37/37 Rust tests, rustfmt check, Clippy with warnings denied, optimized release build, RustSec audit with warnings denied (157 dependencies; no findings), and git diff --check all passed. Exact-head CI for the server-side follow-up is pending; no new real-host LAB result is claimed.
