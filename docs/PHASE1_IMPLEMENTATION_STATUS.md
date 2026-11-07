# Phase 1 implementation status

**Authorization:** Owner-approved 2026-11-06.
**Branch:** phase1/rust-tcp-transport.
**Latest branch-head CI-verified commit (superseded by dated addendum below):** 4baba491499faacfc051b66ef78f3db1263695ee. See the 2026-11-07 current-head addendum for the authoritative state.
**Current security remediation:** Rust 1.88.0 baseline, `time 0.3.47`, removal of unmaintained `rustls-pemfile`; exact-head CI PASS (2026-11-07).
**Latest runtime product-code SHA:** 6afaf9a8758c435986c9f60ad50863491e67266c (includes tracked per-stream task cleanup and nested bounded shutdown budgets; no runtime code changed in later docs-only commits).
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
- Runtime shutdown handles SIGTERM and Ctrl-C: listeners stop accepting new connections, active Yamux sessions receive orderly GOAWAY/close, and session cleanup is bounded by a 20-second deadline. Existing forwarded TCP streams are closed, not migrated or resumed. Operator guidance documents the behavior. This is orderly shutdown, not transparent stream draining.
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
- Historical local validation on an earlier runtime/test head passed formatting, 34/34 Rust tests, Clippy with warnings denied, and diff checks; see the latest exact-head verification below for current evidence. Earlier local validation also covered shell syntax, workflow pin checks, LAB orchestration mocks, preflight mocks, NetEm cleanup failure paths, and the PKI lifecycle regression.

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

- **Enrollment decision resolved (2026-11-07):** Phase 1 uses operator-controlled offline CA provisioning per accepted ADR-0014. Online enrollment/control-plane integration is deferred and is not a Phase 1 acceptance blocker. No public enrollment endpoint or online CA signer is implemented or authorized.
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


## Enrollment decision — ADR-0014

[ADR-0014: enrollment and control-plane trust boundary](ADR-0014-online-enrollment-and-control-plane.md) is **Accepted for offline operator-controlled enrollment in Phase 1**. The existing offline CA helper is the onboarding baseline; node private keys remain on their nodes and the CA private key remains offline. Online enrollment, a public endpoint, and a network-accessible signer are deferred to a later phase and are not Phase 1 release blockers. Reconsideration requires a new owner-reviewed decision and independent security review.

## Handshake negative-test history

- Test-only commit 5249b3e15624a148c3169aa1696b2bcd65784daf adds eight independent client-side checks that reject malformed or mismatched ServerHello bindings; its exact-head Phase 0/Phase 1 push/PR workflows passed.
- Follow-up commit df39f155eeb6565f732fb8a80590591fe2173b17 adds eight server-side rejection cases for protocol version, client identity, server identity, tunnel ID, direction, empty/duplicate capabilities, and malformed client nonce. These are historical changes, superseded as the latest branch head by d901d11.

## Historical exact-head verification — d901d11 PASS

- Commit: [d901d11bc37541c069b5a402d2ee78576f32be8c](https://github.com/zarkmakerburg/BAFT-TUNNEL/commit/d901d11bc37541c069b5a402d2ee78576f32be8c).
- Phase 1 push [42336031466](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42336031466) and PR [42336035061](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42336035061): SUCCESS.
- Phase 0 push [42336031483](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42336031483) and PR [42336035064](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42336035064): SUCCESS.
- Phase 1 passed pinned Rust 1.88.0/cargo-audit 0.22.1, RustSec audit with warnings denied, formatting, 37 Rust tests, Clippy, optimized release build, encrypted systemd credential runtime validation, and installed systemd service lifecycle. Audit scanned 157 dependencies with no vulnerability, unmaintained, unsound, or yanked findings.
- Local exact-head validation also passed 37/37 Rust tests, rustfmt, Clippy with warnings denied, optimized release build, RustSec audit with warnings denied, and diff checks.
- This closes exact-head CI only. It does not close online enrollment/control-plane authorization, automated CRL distribution/refresh/rotation, independent security review, persistent operator-host custody/lifecycle/signal-stop verification, or longer real-host soak. No new real-host LAB run is claimed for d901d11.


## Historical exact-head verification — 6afaf9a PASS

- Commit: [6afaf9a8758c435986c9f60ad50863491e67266c](https://github.com/zarkmakerburg/BAFT-TUNNEL/commit/6afaf9a8758c435986c9f60ad50863491e67266c).
- Phase 1 push [42338027443](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42338027443) and PR [42338030998](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42338030998): PASS.
- Phase 0 push [42338027246](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42338027246) and PR [42338030975](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42338030975): PASS.
- Phase 1 exact-head CI completed pinned Rust 1.88.0 and cargo-audit 0.22.1 installation, RustSec audit with warnings denied, formatting, all 37 Rust tests, Clippy with warnings denied, optimized release build, encrypted systemd credential runtime validation, and installed systemd service lifecycle.
- Local exact-head validation: 37/37 Rust tests, rustfmt, Clippy, optimized release build, RustSec audit (157 dependencies; no vulnerability, unmaintained, unsound, or yanked findings), and diff check — PASS.
- This closes exact-head CI only. No new two-host LAB run is claimed for this SHA. Persistent operator-controlled Linux lifecycle, online enrollment/control-plane authorization, automated CRL distribution/refresh/rollover, independent security review, and longer real-host soak remain open.
- PR #3 remains Draft/open/unmerged. No release or deployment.


## Latest branch-head verification — e987e7a PASS

- Branch head: [e987e7a1fe336feea3a1d748d953379fcd59fbb6](https://github.com/zarkmakerburg/BAFT-TUNNEL/commit/e987e7a1fe336feea3a1d748d953379fcd59fbb6) (documentation-only follow-up to runtime head 6afaf9a).
- Phase 1 push [42343084813](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42343084813) and PR [42343080891](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42343080891): PASS.
- Phase 0 push [42343081031](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42343081031) and PR [42343084816](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42343084816): PASS.
- Phase 1 exact-head jobs completed pinned Rust 1.88.0 and cargo-audit 0.22.1 installation, RustSec audit with warnings denied, formatting, 37 Rust tests, Clippy with warnings denied, optimized release build, encrypted systemd credential runtime validation, and installed systemd service lifecycle.
- Phase 0 jobs completed the isolated Docker netem integration workflow successfully.
- Local exact-head validation of runtime code remains the 6afaf9a result documented above; e987e7a changes documentation only.
- CI passing on this branch head does not close remaining Phase 1 acceptance gates or constitute a new two-host LAB run. PR #3 remains Draft/open/unmerged; no release or deployment.


## Latest branch-head verification — 4baba49 PASS

- Branch head: [4baba491499faacfc051b66ef78f3db1263695ee](https://github.com/zarkmakerburg/BAFT-TUNNEL/commit/4baba491499faacfc051b66ef78f3db1263695ee), documentation-only; runtime code remains at 6afaf9a.
- Phase 0 push [42344000561](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42344000561) and PR [42343996322](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42343996322): PASS.
- Phase 1 push [42344000576](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42344000576) and PR [42343996324](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42343996324): PASS.
- Phase 1 exact-head jobs passed pinned Rust 1.88.0/cargo-audit 0.22.1 installation, RustSec audit with warnings denied, formatting, 37 Rust tests, Clippy with warnings denied, optimized release build, encrypted systemd credential runtime validation, and installed systemd service lifecycle.
- Phase 0 exact-head jobs passed installer/control-script checks and isolated Docker netem integration.
- These workflows verify branch-head CI only. They do not constitute a new two-host LAB run or close the remaining Phase 1 acceptance gates. PR #3 remains Draft/open/unmerged; no release or deployment.

## 2026-11-07 — Current branch evidence addendum

This dated addendum supersedes older "latest" snapshots above without deleting historical evidence.

- Current reviewed branch head at time of this entry: `275b565eebdb57447b735f6666bd62ea7e86b966` (`phase1/rust-tcp-transport`). PR #3 remains Draft/open/unmerged; no merge, release, or deployment.
- Exact-head Phase 0 push [42358485306](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42358485306), Phase 1 push [42358485167](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42358485167), Phase 0 PR [42358489007](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42358489007), Phase 1 PR [42358489003](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42358489003), and LAB SSH credential verification [42358485289](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42358485289) all completed SUCCESS.
- **New real-host evidence on this exact SHA:** manually gated two-host workflow [42358624201](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42358624201) completed SUCCESS. Direct (Iran → abroad) and Reverse (abroad → Iran) each passed 64 KiB and 2,097,289-byte integrity probes. The 2,097,289-byte payload SHA-256 matched in both directions: `1bb59406b95f296d1ffac9536e3c14a5dfc67c2632ca9ae26d1c85ba8b414fa8`. Direct/Reverse transfer times were 0.694 s / 0.673 s. Restarting the abroad listener was followed by successful Direct recovery and another 64 KiB integrity probe. The server rejected a revoked client certificate and the client rejected a revoked server certificate. Workflow result: `PHASE1_TWO_HOST_E2E=PASS`.
- Test resources were temporary files/processes under `/tmp`; runner SSH credentials were removed. No persistent service, firewall, route, sysctl, or production configuration was changed. Full evidence is also recorded in [HQ comment 6704955602](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/2#issuecomment-6704955602).

### Remaining mandatory Phase 1 acceptance gates

1. **Enrollment decision resolved:** ADR-0014 is accepted for offline operator-controlled enrollment in Phase 1. Online enrollment/control-plane integration is deferred, not a Phase 1 release blocker. No public enrollment endpoint or online CA signer is authorized. Reconsider online enrollment only through a new owner-reviewed decision and independent security review.
2. Dedicated operator-controlled Linux host proof remains open for encrypted credential custody and persistent service start/restart, permissions, upgrade/rollback, uninstall, and signal-stop. Hosted Ubuntu 24.04/systemd 255 transient-unit and disposable install lifecycle CI passed, but is not persistent operator-host evidence. The two current Ubuntu 22.04/systemd 249 LAB hosts cannot exercise `LoadCredentialEncrypted=`.
3. Automated CRL publication/distribution/refresh and safe CA/pin rollover with an explicit overlap/rollback procedure remain open. Current CRL updates are out-of-band and require peer restart.
4. Further justified adversarial/replay/resource-exhaustion testing, protocol-boundary fuzzing, dependency/license review, and an independent security review remain open. Avoid redundant test proliferation; prioritize uncovered effect boundaries and evidence gaps.
5. Longer-duration real-host soak and controlled latency/loss acceptance remain open. Existing Docker netem evidence is simulation and must not be represented as real-host impairment proof.

The new E2E result closes the missing post-shutdown-change real-host Direct/Reverse/recovery/revocation evidence only. It does not close the gates above or constitute production readiness/security certification. Phase 1 remains in progress and PR #3 remains Draft.

## 2026-11-07 — Guarded offline CRL activation implementation

- Accepted [ADR-0015](ADR-0015-crl-update-and-rollover.md): offline CA issuance remains authoritative; node-side activation is explicit and local; no online signer, remote distribution, or two-node transaction is introduced.
- Added `scripts/pki/apply-crl.sh check` to validate exactly one PEM CRL, CA signature and issuer, mandatory CRL Number, strictly increasing number versus the installed CRL, and candidate `lastUpdate`/`nextUpdate` window without mutation.
- Added `apply-crl.sh apply` for root-only activation of an already-active named `baft-tunnel@INSTANCE.service`: stage in the same directory, atomic replacement, restart and active-state check, restore prior CRL and attempt service recovery on failure. It does not enable/start stopped services, alter firewall/network settings, or contact the other node.
- Added isolated `scripts/pki/test-apply-crl.sh` covering valid advancement, equal/lower CRL number rejection, different-CA signature rejection, malformed/trailing-data rejection, and expired CRL rejection. Local run: `BAFT_CRL_APPLY_TEST=PASS`; shell syntax and `git diff --check` passed.
- Phase 0 CI now runs this test with temporary CAs only. No live CA, service, or LAB host was changed. CI for this new head is pending.
- This improves local CRL update safety only. Remote distribution, two-node orchestration, safe CA/pin rollover, persistent operator-host lifecycle, independent security review, and real-host soak remain open.

- Follow-up hardening: serialized concurrent local applies with a per-CRL `flock`, revalidated the exact staged candidate bytes while holding the lock, and changed the rollback copy to explicit root:root mode 0600 (rather than preserving potentially broad source permissions). Re-ran the CRL regression suite after these changes: PASS.
- Full local Rust verification on the current source: RustSec audit PASS (157 dependencies), cargo-deny licenses/sources PASS, rustfmt PASS, 37/37 Rust tests PASS, Clippy with warnings denied PASS, release build PASS, CRL regression PASS, offline PKI regression PASS, and `git diff --check` PASS. These local results do not replace exact-head CI or the operator-host proof.

- Real systemd activation/rollback proof completed in an isolated local Ubuntu 24.04.4 ARM64 VM (systemd 255): `BAFT_CRL_SYSTEMD_ACTIVATION=PASS`. This exercised an actual transient systemd unit, successful CRL activation, a deliberately failed first restart, restoration of the prior CRL with root:root 0644 permissions, successful service recovery, and root-only 0600 lock/backup handling. The test exposed and fixed a rollback-mode bug: the private 0600 backup is now restored through a fresh 0644 staging file.
- Same isolated VM: `BAFT_SYSTEMD_CREDENTIAL_RUNTIME=PASS` and `BAFT_SYSTEMD_SERVICE_LIFECYCLE=PASS` (real installer, encrypted systemd credential delivery, start/restart/stop, safe uninstall). systemd-creds warned that its temporary host secret was not on encrypted media; this is a disposable software-key test only, not TPM/hardware-backed custody evidence.
- Added `scripts/pki/test-crl-activation-systemd.sh` to Phase 1 CI after the existing service lifecycle test so successful activation and recovered rollback remain regression-gated.


## 2026-11-07 — Current Phase 1 status and owner waiver (93355df)

This addendum supersedes earlier statements in this document about the latest branch head and mandatory independent review. Historical test results and decisions above remain preserved as history.

- Current branch: `phase1/rust-tcp-transport`; current exact HEAD: `93355df08d25b30f5a31fb78433f6ae0af76dc31`. Working tree was verified clean and the local branch matched `origin/phase1/rust-tcp-transport`.
- Exact-head Phase 1 push CI [42377033575](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42377033575) and PR CI [42377037504](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42377037504): **PASS** on the same SHA. Both completed dependency audit, license/source checks, formatting, focused Rust tests, Clippy, release build, systemd encrypted-credential delivery, installed service lifecycle, and deterministic CRL activation/rollback.
- Local exact-source checks recorded at HQ: cargo-audit (157 locked dependencies), cargo-deny, rustfmt, 37/37 Rust tests, Clippy with warnings denied, release build, offline PKI and CRL regression suites: PASS. These do not replace operator-host evidence.
- Existing real two-host Direct/Reverse/recovery/revocation E2E evidence remains tied to runtime SHA `275b565eebdb57447b735f6666bd62ea7e86b966` and workflow [42358624201](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42358624201); it is not a soak or impairment acceptance test for `93355df`.
- **Owner-approved waiver (2026-11-07):** a separate independent protocol/security review is not required for Phase 1 acceptance. No independent review is claimed. The waiver is recorded at [HQ comment 6708145366](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/2#issuecomment-6708145366).

### Remaining Phase 1 acceptance work

1. Persistent operator-controlled Linux host proof for credential custody and service start/restart, permissions, upgrade/rollback, uninstall, and signal-stop. Hosted Ubuntu 24.04/systemd 255 CI uses disposable hosts; the two configured LAB nodes are Ubuntu 22.04/systemd 249 and cannot exercise `LoadCredentialEncrypted=`.
2. Longer real-host soak and controlled latency/loss acceptance. Existing E2E evidence is bounded; Docker netem remains simulation and cannot close this gate. No dedicated soak workflow currently exists.
3. CRL lifecycle expansion: authenticated remote publication/distribution/refresh coordination and safe CA/pin rollover with explicit overlap and rollback. Current issuance remains offline and activation remains explicit/local per ADR-0015.

Dependency/license/security CI is green for the exact head and is retained as verified evidence, not an open gate. PR #3 remains OPEN / Draft / unmerged. No production/LAB persistent host changes, merge, release, or deployment have been performed.
