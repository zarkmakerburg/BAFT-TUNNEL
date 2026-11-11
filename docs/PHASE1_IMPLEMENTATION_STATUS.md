# Phase 1 implementation status

> **Current verified addendum (2026-11-11):** current main is `b2c8ab5020754cad15485371a1f062de4092229f`. D2 panel-managed TCP-TLS PKI is merged in PR #150 and its trusted two-host repeat passed in run 42996224181. U1 CRL renewal and real two-host push passed in run 42656507521 on ancestor SHA `8be74376176442106669f4a81293f0865629ecaf`. Exact scope and limitations are recorded in the dated addendum at the end of this file. Historical branch/status text below describes earlier development and is not the current branch state.

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


## 2026-11-07 — Ubuntu 22.04 / systemd 249 baseline decision

This addendum supersedes prior statements that treated systemd 250+ encrypted credentials as a universal Phase 1 host requirement. Historical evidence above remains preserved.

- Accepted [ADR-0016](ADR-0016-ubuntu-2204-systemd-compatibility.md): Ubuntu 22.04 LTS with systemd 249 is the default supported Linux baseline. Do not upgrade the current LAB hosts or require Ubuntu 24.04/systemd 250+.
- Baseline node-key delivery uses systemd `LoadCredential=` with a root:root 0600 source file in a root:root 0700 directory and the existing strict `key_credential` resolver. The source remains plaintext at rest; filesystem permissions and service-scoped delivery are not encryption.
- `LoadCredentialEncrypted=` remains an optional enhanced mode on systemd 250+ and is not a baseline release requirement. Explicit selection must fail closed when unavailable; no silent downgrade.
- Operator guidance updated to distinguish baseline and optional encrypted modes.
- Both configured LAB hosts were upgraded through the approved conservative package-upgrade workflow, without distribution upgrade or reboot. They remain Ubuntu 22.04/systemd 249.11. Iran host: 45 packages upgraded; abroad host: 172 packages upgraded. Post-upgrade apt simulations showed no installable upgrades; `dpkg --audit` was empty on both; no reboot required. The upgrade did not change systemd major version.
- **Implementation/testing still required:** installer/service integration for baseline credential delivery, an actual systemd 249 lifecycle test on the LAB baseline, and updated exact-head CI. This documentation/decision commit alone does not claim that the baseline credential path has been implemented or tested.
- Existing systemd 255 encrypted-delivery CI remains valid evidence for the optional enhanced mode only. It does not establish systemd 249 compatibility.
- PR #3 remains Draft. Persistent service lifecycle, real-host soak/impairment, and remote CRL/CA/pin rollover gates remain open.

## 2026-11-07 — P1-000 baseline credential install path (implementation)

- `scripts/install.sh` now provisions the Ubuntu 22.04 / systemd 249 baseline credential: `--instance NAME --node-key PATH` detects the systemd version (247+ required), installs `/etc/baft-tunnel/credentials/NAME/node-key` (root:root 0600 in a root:root 0700 directory) and a managed drop-in `baft-tunnel@NAME.service.d/credentials.conf` containing only `LoadCredential=node-key:<path>`. No key material is placed in JSON, environment, arguments, drop-ins or logs; the key source must be a regular non-symlink PEM private key of bounded size.
- Optional `--credential-mode encrypted` requires systemd 250+ and `systemd-creds encrypt`; it fails closed and never falls back to baseline mode.
- Existing configs that set `key` are refused (never both `key` and `key_credential`). A different existing credential is replaced only with `--replace-credential`; the previous key is kept in the root-only (0700) backup directory.
- Install, upgrade and credential changes use a per-run backup with automatic rollback on failure; `rollback.sh` restores binary/unit and/or credential state by backup scope. `uninstall.sh` keeps configuration, credentials, drop-ins and state unless `--purge-config --yes` is given; purge removes only drop-ins marked as installer-managed.
- Fixed a defect in the draft installer that moved the source unit file (`mv -f "$UNIT_SOURCE" ...`); the installer now only copies it.
- Verification so far: `scripts/test-install.sh` (staged, local) passes, including baseline provisioning, idempotent re-run, replace/rollback, fail-closed checks and safe uninstall. This is simulation/staged evidence only.
- Added `scripts/pki/test-systemd-baseline-lifecycle.sh` and CI job `ubuntu-2204-baseline` (runner `ubuntu-22.04`, systemd 249) covering real `LoadCredential=` start/restart/stop, source permissions, missing-credential fail-closed behavior, and uninstall retention/purge. **Result**: PASS on hosted `ubuntu-22.04` / systemd 249.11 at commit `05f6c54c5c21d0ea08b1bbfc18ed5f2a07c3f185` ([run 42389639511](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42389639511), job `ubuntu-2204-baseline`): real install, `LoadCredential=` start/restart/stop, root:root 0700/0600 source permissions, fail-closed on missing credential, plain uninstall retains credentials/config, explicit purge. Finding: systemd 249 hides `/run/credentials` from `ExecStartPre=`, so the unit's pre-start `config validate` was removed (`apply` validates and resolves the credential itself). This is hosted-CI evidence, not the persistent LAB host. Hosted CI is not evidence for the persistent LAB host; the LAB isolated-instance test (P1-000 step 9) remains open.

### P1-000 step 9 — isolated LAB credential test (real hosts)

- Workflow `p1-000-lab-credential-test.yml`, [run 42390940225](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42390940225), commit `263d237ad648622eab01c52e101d4854cb1dbf59`: PASS on both LAB hosts (Iran and abroad, Ubuntu 22.04 / systemd 249). Each ran the baseline `LoadCredential=` lifecycle test (install, start/restart/stop, root-only permissions, fail-closed on missing credential, uninstall retention/purge) from a disposable `/tmp` directory; the test refuses to run over an existing BAFT-TUNNEL install, so no existing service was touched, and the SSH key was removed after the run. No reboot, no OS/systemd upgrade. Evidence type: real-host LAB.

## 2026-11-07 — Review hardening (PR #13, merged 21b9f92) and P1-003 handshake tests

- PR #13: global (32) and per-source (2) pending-handshake limits independent of session slots, non-fatal accept errors with backoff, 900 s idle timeout on relayed streams, forward listeners default to loopback (`--allow-non-loopback-listen` to override), CRL expiry warning (<24 h) plus support-bundle field, unit `TasksMax`/`MemoryMax`/`ProtectProc=invisible`/`ProcSubset=pid`. CI green on `ubuntu-24.04` and `ubuntu-22.04` / systemd 249 at head `7b02cff`. Evidence type: hosted CI and unit tests; not soak or LAB.
- P1-003 (in progress): added a handshake test rejecting malformed server nonces (empty, short, non-hex, oversized) and unknown hello fields. Existing coverage already includes per-field hello mutation, wrong direction, framed-reader corpus and truncation.

## 2026-11-07 — Board refresh and P1-001/P1-002 state

- Refreshed roadmap board: [docs/PROJECT_BOARD.md](PROJECT_BOARD.md) (Persian; owner can re-upload).
- P1-001: PR #15 (`8bcafc9`) added `soak_seconds` and `impair` inputs to the manual two-host E2E. **Result: PASS** on the two real LAB hosts (Ubuntu 22.04 / systemd 249), [run 42398701622](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42398701622), commit `8bcafc9`: 1800 s Direct soak, 128 rounds of integrity-checked probes (64 KiB, plus 2 MiB every sixth round), forward fd 11→11, RSS 5024→5024 kB; serve fd 11→11, RSS 6000→6364 kB; then the latency/jitter proxy phase (1 MiB at ~5.8 s, proxy restart recovery), then Direct, Reverse and revocation checks as before. Temporary files cleaned. The impairment is application-level (latency/jitter proxy, proxy restart) and is **not** real TCP packet loss; real packet loss remains untested.
- P1-002: ADR-0017 (PR #16) is proposed and awaiting owner decision; nothing implemented.

## 2026-11-07 — P1-001 real-host soak and impairment (LAB, PASS)

Run [42401889166](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42401889166), product-code SHA 8bcafc9ff2f1c5f67d0f394f82bb5ffb8067cfa1 (main), two LAB hosts (Ubuntu 22.04 / systemd 249), inputs `soak_seconds=900`, `impair=1`. Real-host evidence, not simulation or hosted CI.

- Result line: `PHASE1_TWO_HOST_E2E=PASS soak_s=900 impair=1` (Direct soak 900 s, application-level latency/jitter impairment, Direct and Reverse 2,097,289-byte integrity with SHA-256 1bb59406…ca8b414fa8 in both directions, listener-restart recovery, server-side and client-side certificate revocation rejection).
- Under impairment: 64 KiB probes kept SHA-256 4b640d85…a68a4df2 (0.38–2.09 s); a 1 MiB probe passed in 5.9 s. After the abroad-listener restart, Direct recovered within one probe (0.39 s).
- Total job 19 min 59 s; temporary /tmp processes only, cleanup step passed, no persistent host change.
- Scope limit: impairment is application-level (not kernel NetEm/packet loss on the real hosts), and the soak is 15 minutes. Longer soak and kernel-level loss remain for Phase 6 (P6-001). P1-001 is accepted at this profile.

## 2026-11-07 — P1-002a dual peer pin (ADR-0017 B1, step 2)

`peer_fingerprint` (config and CLI) now accepts one SHA-256 pin, or two distinct comma-separated pins for a certificate rollover window; three pins, empty entries and duplicates are rejected, and a peer matching either pin is accepted (identity URI SAN and CA/CRL checks still apply). Unit and TLS-level tests added; local 44/44 tests, rustfmt, Clippy `-D warnings`. Evidence is local only (CI runner unavailable at commit time). Remaining for P1-002: multiple CA certificates with per-CA CRLs, operator push/expiry-report helpers, rollover sequence test, LAB run.

## 2026-11-07 — P1-002b multi-CA bundle with per-CA CRLs (ADR-0017 B1, step 1)

The runtime already loads a PEM bundle of CA certificates and a PEM file of several CRLs; this change proves and documents the rollover overlap. New TLS test `ca_bundle_with_per_ca_crls_supports_rollover_overlap`: with both CAs and both CRLs configured, certificates from the old and the new CA authenticate; a CA outside the bundle is rejected; with only the old CA's CRL loaded, new-CA certificates are rejected (unknown revocation status, fail closed). Finding: CRLs are matched to their CA by subject name, so rollover CAs need different subjects; `baft-pki.sh init-ca CA_DIR [CA_NAME]` now takes a name (default unchanged), invalid names are rejected, and the PKI test no longer depends on a missing directory for the non-empty check. Local: 45/45 Rust tests, Clippy `-D warnings`, rustfmt, PKI script test PASS. Local evidence only (CI runners unavailable at commit time). Remaining: push/expiry-report helper, full rollover sequence script test, LAB run.

## 2026-11-07 — P1-002c CRL freshness report (ADR-0017 A1)

Added `scripts/pki/crl-status.sh` (report-only, per-CRL issuer/number/nextUpdate, exit 1 on WARN or expired, supports multi-CRL bundles) and `scripts/pki/test-crl-status.sh` (wired into the Phase 0 workflow). Local test PASS. Known gap: `apply-crl.sh` still activates exactly one CRL for one CA, so the multi-CA overlap file is assembled by the operator; extending activation to bundles is P1-002e. P1-002 stays open.

## 2026-11-07 — P1-002e CRL bundle activation (ADR-0017 B1)

`scripts/pki/apply-crl.sh` now validates and activates CRL bundles: 1–4 CA certificates, one CRL per CA, per-issuer signature, per-issuer increasing CRL number, expiry, fail closed on a missing CRL for any configured CA, a CRL from an unconfigured CA, duplicate issuers, or duplicate CA subjects. Single-CA behavior is unchanged except that the `CRL_CHECK=PASS` line now reads `cas=N old_numbers=… candidate_numbers=…`. `test-apply-crl.sh` extended (local PASS); systemd activation/rollback logic is unchanged and still covered by the existing CI job, not re-run here. Remaining for P1-002: full rollover sequence test and LAB run.

## 2026-11-07 — P2-003 stream engine seam (ADR-0018, behaviour-neutral)

New `src/engine.rs`: `StreamEngine` trait (`id`, `accept`, `dial`) producing a boxed, mutually authenticated byte stream, with `TlsEngine` (id `tcp-tls`) wrapping the existing TLS 1.3 accept/dial plus SHA-256 pin(s) and URI SAN identity verification. `tunnel.rs` now obtains its secure stream from the engine; hello/authorization, Yamux, limits, reconnect, CRL reload on reconnect and shutdown are unchanged. Local: 45/45 tests (including the in-process tunnel E2E tests), rustfmt, Clippy `-D warnings`. Local evidence only; no new LAB run yet, so the real-host E2E will be repeated before the next pre-release. Next: config `engine:` field and Noise+TCP as a second engine (P2-004).

## 2026-11-07 — P2-004b Noise+TCP engine (ADR-0018, not yet configurable)

`NoiseEngine` (`noise-tcp`) implements `StreamEngine` on the existing Noise IK adapter: X25519/ChaCha20-Poly1305/BLAKE2s, pinned peer static key (no CA), prologue binding protocol version, both node identities, tunnel and direction. Tests: enrolled keys authenticate and relay bytes; an unenrolled key, a mismatched tunnel id and a mismatched direction are all rejected (47/47 local tests, Clippy `-D warnings`, rustfmt). The engine is not selectable from configuration yet and is not part of any release claim. Next (P2-004c): config `engine:` field with `noise` settings, `noise-keygen` command, and an in-process tunnel E2E over Noise; then a LAB E2E.

## P2-003 real-host LAB verification: BLOCKED (infrastructure), 2026-11-07

Three dispatches of `phase1-two-host-e2e.yml` on main (runs 42404833577 @4a38561, 42405069748 and 42405579721 @f8e8b82) all failed in the first job (`validate-confirmation`) within about 2 seconds with correct inputs, and the LAB job was skipped. No LAB host was contacted. The same instant failure affects every PR-triggered job since #20, so the likely cause is the hosted Actions runner/quota, not the code. Job logs are not retrievable (HTTP 404).

Status: the behaviour-neutral refactor (P2-003) is verified locally only (cargo test, clippy, fmt). Real-host E2E for the refactor remains OPEN and will be re-run once runners are available.

## P2-004c: `engine` config field and `noise-tcp` wiring (local evidence only), 2026-11-07

- Config accepts `engine: "tcp-tls"` (default, unchanged behaviour) or `"noise-tcp"`. Noise uses the existing `key`/`key_credential` custody for a hex static private key, plus `noise_peer_public` (64 hex). `ca`, `crl`, `cert`, `peer_fingerprint` and `server_name` must be omitted for Noise and are required for TLS; mixed or missing material fails validation (fail closed).
- `baft-tunnel noise-keygen --out FILE` writes the private key (0600, never overwrites) and prints the public key.
- In-process tunnel test over Noise passes (2x framed payload through Yamux), and a dialer pinning the wrong server key and a client with an unenrolled key are both refused.
- Evidence: cargo test (49 pass), clippy `-D warnings`, fmt. Not yet run on real LAB hosts (hosted runners unavailable, see P2-003 note). Detectability: Noise has no TLS fingerprint but is a high-entropy stream and is NOT claimed undetectable.

## P1-001 real-host LAB: TCP packet loss and 3600 s soak PASS, 2026-11-07

- Real TCP packet loss (nftables, dedicated table, removed after the test): run 42412456810 on main, `LOSS_RESULT percent=2 dropped_packets=15`, `PHASE1_TWO_HOST_E2E=PASS` (Direct, Reverse, integrity, recovery, revocation).
- 3600 s soak (no impairment): run 42412781202 on main, 266 rounds, `forward_fd=11->11`, `serve_fd=11->11`, RSS growth under 200 KB, `PHASE1_TWO_HOST_E2E=PASS`.
- Logs checked: no host addresses printed (hosts are masked Environment secrets).
- This engine path is the refactored one (P2-003), so it also serves as the real-host confirmation for P2-003. `noise-tcp` (P2-004) is NOT yet covered by a LAB run.

## P2-004 real-host LAB: `noise-tcp` PASS (Direct), 2026-11-08

- Run 42445109749 on main: `NOISE_RESULT enrolled_ok=1 rogue_refused=1`, `PHASE1_TWO_HOST_E2E=PASS ... noise=1`. Enrolled static keys relayed 64 KiB and 2 MiB payloads over `noise-tcp` (no CA); a client with an unenrolled key did not relay.
- Not covered: Reverse direction over Noise, packet loss/soak over Noise, and any detectability claim (Noise is a high-entropy stream, NOT claimed undetectable).
- Incident: the first Noise run (42441461484) hung because of a backgrounding bug in the new stage (ssh kept the inherited stdout) and was cancelled; it left a temporary process on a LAB host. Fixed in #32 and removed with the new `cleanup_stale` mode (#33, #34), which only touches `/tmp/baft-tunnel-e2e-*` leftovers. Hosts verified free by the next run's port check.

## 2026-11-08 — P1-002 operator push helper (ADR-0017 A1)

Added `scripts/pki/push-crl.sh` (stages the CRL on each node over SSH and runs the node's `apply-crl.sh apply`; one node at a time, stops at the first failure, strict SSH options, refuses CRLs under 72 h, `--dry-run`) and `scripts/pki/test-push-crl.sh` (mock SSH/SCP, wired into the Phase 0 workflow). Operator guide gained the push and rollover-sequence sections. Implemented so far for ADR-0017: dual pins (#22), multi-CA bundle with per-CA CRLs (#23, #25), freshness report (#24), push helper (this change). Still open: a full rollover-sequence run on the real LAB hosts, and the independent security review (waived by the owner, not performed).

## 2026-11-08 — P1-002 rollover LAB PASS and P1-004 acceptance closure

- Rollover sequence on the real LAB hosts (run 42447019410 on main, `ROLLOVER_RESULT overlap_ok=1 new_certs_ok=1 old_ca_removed_refused=1`, `PHASE1_TWO_HOST_E2E=PASS`): (1) CA bundle with per-CA CRLs and two pins per side, old certificates still accepted; (2) certificates from the new CA accepted; (3) old CA and old pins removed, an old-CA client refused. Ephemeral CAs, temporary `/tmp` processes only.
- **P1-004 closure (evidence at main `e4453bc`):** hosted CI green (Ubuntu 24.04 and Ubuntu 22.04 / systemd 249); two-host Direct/Reverse/revocation PASS; 30 min soak with impairment PASS (run 42398701622); real 2% TCP packet loss PASS (42412456810); 3600 s soak PASS (42412781202); `noise-tcp` Direct PASS (42445109749); rollover PASS (42447019410); adversarial handshake tests (PR #13/#14); Ubuntu 22.04 credential path (P1-000).
- **Documented exceptions, not claimed:** soak longer than one hour (workflow cap is 3600 s); `push-crl.sh` is mock-SSH tested only, not run against real hosts with installed `apply-crl.sh`; independent security review waived by the owner and not performed; Noise Reverse/packet-loss untested; no detectability claim. Production release is NOT claimed.

## 2026-11-08 — M0: `noise-tcp` Reverse LAB PASS

- Run 42457633244 on main (`2093ebc` plus the script fix in #41): `NOISE_RESULT direct_ok=1 direct_rogue_refused=1 reverse_ok=1 reverse_rogue_refused=1`, `PHASE1_TWO_HOST_E2E=PASS ... noise=1`. In both directions enrolled keys relayed 64 KiB and 2 MiB payloads and an unenrolled client key did not relay.
- Incident: run 42456443965 failed in the Reverse stage because the earlier TLS Reverse stage leaves its serve process on the Iran host and the new stage then overwrote its pid file; run 42457380581 then failed the port check on the orphan. Fixed in the script (#41) and the orphan removed with `cleanup_stale`. No Noise defect was involved.
- Not covered: Noise under packet loss or soak; no detectability claim.


## P3-001 `setup` CLI (S1 to S4)
- S1 `setup plan` (PR #42), S2 `noise-tcp` init/join (PR #43), S3 `tcp-tls` from `baft-pki.sh` output (PR #44): CI green.
- S4 LAB: run 42461145830 on `54bebff`, `SETUP_RESULT noise_install_s=38 engine_switch_to_tls_s=42 commands_per_host=3 probe_ok=1`, `PHASE1_TWO_HOST_E2E=PASS ... setup=1`. Both hosts configured only through `setup` (init, join, apply); 64 KiB and 2 MiB probes passed for both engines. Targets met (at most 5 min, at most 3 commands, switch at most 60 s). No host addresses in the log.
- Caveats: the hosts were the LAB hosts cleaned by the stage, not freshly provisioned VMs; tcp-tls PKI files were the ephemeral LAB certs standing in for `baft-pki.sh` output. Bundle copy and checksum comparison are manual steps outside the 3 commands.

## P3-002 agent and local API (A1 to A3)
- A1 read-only status API (PR #47), A2 validated desired state with audit log (PR #48): CI green, 71 unit tests.
- A3 LAB: run 42466445718 on `d05b727`: `AGENT_RESULT status_listener_accepting=1 desired_state_accepted=1 applied=0`, `SETUP_RESULT noise_install_s=34 engine_switch_to_tls_s=34 commands_per_host=3 probe_ok=1`, `PHASE1_TWO_HOST_E2E=PASS ... setup=1`. The tcp-tls pair was built from real `scripts/pki/baft-pki.sh` output (ephemeral CA on the runner). No host addresses in the log.
- Incident: run 42465515538 passed the agent check but failed the 60 s switch limit (81 s) because PKI issuance and delivery were inside the timed window; PKI installation is a one-time step, so the stage now times only the switch (PR #50).
- Caveats: hosts are the LAB hosts cleaned by the stage, not freshly provisioned VMs; the agent is read/validate/store only and never applies a config; the CA passphrase came from `BAFT_PKI_PASSFILE` (automation only).

## P4-001 local panel and wizard (W1 to W3)
- W1/W2 (PR #52): static panel (Persian RTL and English, strict CSP, no `innerHTML`) and wizard backend `POST /v1/setup/plan`; CI green, 74 unit tests; headless-browser run showed wrong-token refusal, status, wizard commands and the language switch.
- W3 LAB: run 42469113232 on `1080dbd`: `AGENT_RESULT status_listener_accepting=1 desired_state_accepted=1 applied=0 panel_served=1 wizard_plan_ok=1`, `SETUP_RESULT noise_install_s=41 engine_switch_to_tls_s=39 commands_per_host=3 probe_ok=1`, `PHASE1_TWO_HOST_E2E=PASS ... setup=1`. No host addresses in the log.
- Decision: no React/Vite for the MVP (see PLAN-P4-001-panel.md). Not covered: panel in a real browser against a real host (checked headless against a local agent only), accessibility audit, executing setup from the panel.

## R-001 upgrade and rollback drill, P1-005 fuzz
- R-001 (PR #59): the `ubuntu-2204-baseline` job (systemd 249) builds `v0.1.0-phase1-pre`, installs it with the current config and `LoadCredential=` node key, upgrades to the new build with `install.sh`, restarts, rolls back with `rollback.sh`, restarts again. Each step checks the service is active and the forward listener accepts connections, binary checksums match, and config and credential checksums are unchanged. `UPGRADE_ROLLBACK_RESULT upgrade_s=1 rollback_s=0`, `BAFT_SYSTEMD_BASELINE_LIFECYCLE=PASS`. The previous release accepts the current config format. Not covered: the signed release asset itself (built from the tag instead), a tunnel carrying traffic during the upgrade, a real LAB host.
- P1-005 (PR #58): four fuzz targets (config JSON, pairing bundle, agent HTTP, handshake); `fuzz-smoke` CI green; locally 40 s per target, about 5.8M executions in total, no crash.

## P4-002 panel alignment with HQ UI spec
- Local: fmt, clippy, 100 unit tests (incl. `preflight_*`, per-asset no-`innerHTML` check). Browser (headless, local agent): 76 + 10 checks PASS across desktop/mobile and fa/en (`scripts/panel-qa/`). CI on PR #62 head: all four jobs PASS. Real-host LAB for the new pages NOT RUN. Traceability and backend gaps: [PANEL_HQ_TRACEABILITY.md](PANEL_HQ_TRACEABILITY.md).

## P5-005 Reverse correction (ADR-0021), 2026-11-08

- Code trace shows Direct and Reverse behave identically: the `serve` node always listens and the `forward` node always dials; the direction is only compared in the hello. The earlier "reverse" LAB results (including `NOISE_RESULT reverse_ok=1`) prove label acceptance, not a reverse data path.
- Capabilities, wizard and preflight now report Reverse as unavailable. Real Reverse needs a protocol-affecting mode and an ADR amendment; the acceptance gate is in ADR-0021.
- Evidence: local unit tests (112 pass), clippy, fmt, headless browser 76 PASS 0 FAIL. No LAB claim for this change.

## P4-002 real-host LAB: panel and read-only preflight PASS, 2026-11-08

- Run 42490325790 on main `28df6f4`, setup=1: `AGENT_RESULT status_listener_accepting=1 desired_state_accepted=1 applied=0 panel_served=1 wizard_plan_ok=1 preflight_ok=1`; `SETUP_RESULT noise_install_s=33 engine_switch_to_tls_s=33 commands_per_host=3 probe_ok=1`; `PHASE1_TWO_HOST_E2E=PASS` (Direct, Reverse, 2 MiB integrity, recovery, revocation).
- The preflight check asserts `applied:false`, `apply_available:false`, planner pass, peer reachability and firewall unknown (never pass).
- Lesson: LAB hosts are shared. Concurrent runs from different threads collide (port 18300 in use; `cleanup_stale` kills other runs' processes and only cleans, it does not run the test). Dispatch one run at a time and wait for the other thread.
- Not covered: the outbound peer reachability probe (#71) and the `/v1/engines` capability contract on real hosts.

## Reverse (ADR-0021 Amendment 1) real two-host LAB, 2026-11-08

- Run [42506805986](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42506805986) on main `2d8cd5e`, `reverse=1`, tcp-tls: `REVERSE_RESULT engine=tcp-tls outbound_only=1 integrity_64k_2m=1 reconnect_service=1 reconnect_public=1 busy_port_refused=1 wrong_identity_refused=1 wrong_pin_refused=1 revoked_refused=1`; `PHASE1_TWO_HOST_E2E=PASS ... reverse=1`.
- The service node dials out only (no listener on the tunnel port, one established outbound connection); 64 KiB and 2 MiB payloads kept their SHA-256 through the user ingress on the public node; both nodes recovered after a restart; a second public node on a busy port failed without disturbing the first; a valid certificate of another node, a wrong pin and a revoked certificate were refused.
- Capabilities, wizard and preflight now offer Reverse for **tcp-tls as a preview only**. It is NOT stable or production-ready: one LAB run, no soak, no packet loss, no agent-restart scenario. `noise-tcp` Reverse has no real data-path evidence and stays unavailable. The older "reverse" results (including `NOISE_RESULT reverse_ok=1`) were label-acceptance only.

## P0-3 BAFT Engine Store → Launcher real-host LAB, 2026-11-08

- Run [42510577167](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42510577167) on main `2d2718b` (engine built locally from the pin): `BAFT_SYSTEMD_RESULT ... engine_kill_recovered=1 store_launch_ok=1 store_pin_mismatch_refused=1`.
- Run [42522105698](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42522105698) on main `06c273e`, `baft=1 baft_systemd=1 baft_release=v0.1.0-rc.2`: engine taken from the signed release (cosign `Verified OK`, `SHA256SUMS` OK, digest `e4f04837253f83b7e52bca29bd93f0e4b2bd627325a75dee37d93b1057513647` = manifest pin). `BAFT_STORE_ROLLBACK_RESULT engine_source=signed-release:v0.1.0-rc.2 b_installed=1 a_pin_refused_while_b_current=1 b_carried_2mib=1 rollback_to_a=1 b_pin_refused_after_rollback=1 tampered_rollback_refused=1 a_carried_2mib_after_rollback=1`; `BAFT_SYSTEMD_RESULT hardened_direct_ok=1 mdwe=yes no_seccomp_kill=1 recovery_s=41 engine_kill_recovery_s=35 store_launch_ok=1 store_pin_mismatch_refused=1 store_rollback_ok=1`; `PHASE1_TWO_HOST_E2E=PASS`. Units, throwaway user, `/opt/baft-lab-e2e` and `/tmp` dirs removed by the stage cleanup.
- Release B in the rollback case is the same engine with a trailing marker (different digest) and a manifest that differs only in `artifact_sha256`; it proves store/pin mechanics, not a second upstream BAFT version.
- Not changed: BAFT stays hidden from `/v1/engines` and the Wizard.

## P0-4 TCP-TLS Reverse R3 real-host LAB, 2026-11-08

- Run [42525790470](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42525790470), main `5a72a25` (PR #94 fixed the 100% blackhole nft rule that failed run 42524033576), inputs `reverse=1 reverse_r3=1200 loss_percent=2`.
- `REVERSE_R3_RESULT service_sigkill_cycles=5 service_recovery_max_s=3 public_sigkill_recovery_s=3 blackhole_30s_recovery_s=8 midtransfer_recovery_s=2 midtransfer_user_conn=completed loss_percent=2 loss_dropped_packets=18 soak_s=1200 soak_rounds=90 public_fd=12->12 public_rss_kb=7512->7676 service_fd=10->10 service_rss_kb=5524->5524 stray_processes=0 nft_left=0`
- `REVERSE_RESULT engine=tcp-tls outbound_only=1 integrity_64k_2m=1 reconnect_service=1 reconnect_public=1 busy_port_refused=1 wrong_identity_refused=1 wrong_pin_refused=1 revoked_refused=1`
- Evidence label: REAL-LAB (two LAB hosts, temporary /tmp processes, dedicated nft table removed). Reverse stays Preview until owner review; Noise Reverse is not covered and stays unavailable. Not production-ready.

## P0-5 integrated two-host MVP acceptance (signed prerelease), 2026-11-09

- Run [42538458094](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42538458094), main `92c4e70`, inputs `mvp=1 mvp_release=v0.1.0-rc.4`. Result: `PHASE1_TWO_HOST_E2E=PASS ... mvp=1`.
- Artifact: signed prerelease [v0.1.0-rc.4](https://github.com/zarkmakerburg/BAFT-TUNNEL/releases/tag/v0.1.0-rc.4) built from `e5d5b51` on Ubuntu 22.04 (cosign keyless bundle verified in the workflow), `baft-tunnel` sha256 `c1652caa3bebde797dcc4955e3d776d339d7165de5df95647e3eafa40bcaf00f`, needs at most GLIBC_2.34. Hosts: Ubuntu 22.04.5 LTS, systemd 249.
- `MVP_RESULT source=signed-release:v0.1.0-rc.4 clean_hosts=1 installed=1 wrong_checksum_refused=1 setup_join=1 control_helper=1 apply_start_serve=applied apply_start_forward=applied receipt_session=observed receipt_ready=unknown integrity_64k_2m=1 replay=1 running_refused=1 receipts_secret_free=1 forward_sigkill_recovery_s=4 serve_restart_recovery_s=3 occupied_port_rolled_back=1 wrong_pin_refused=1 wrong_identity_refused=1 revoked_refused=1 uninstalled=1 listeners_units_unchanged=1 baft_fw_rules=0 unrelated_fw_drift=abroad`
- Path exercised: install.sh with the signed binary; offline PKI on the runner; `setup init` / bundle exchange / `setup join` with checksums (wrong checksum refused, no config written); per-instance control helper (`install-agent-control.sh`) on both hosts; local agent as the service user; `POST /v1/setup/apply` stage `start` for serve then forward; 64 KiB + 2,097,289-byte probes through the tunnel (SHA-256 `1bb59406…ca8b414fa8`); Apply receipts keep `ready=unknown` (the data path was measured by the stage, not claimed by Apply). This is also the two-host proof for the P0-1 control helper and P0-2 receipt truth.
- Side effects: listeners and `baft-tunnel*` unit files unchanged after uninstall; no firewall change naming BAFT or the stage's ports. **Unrelated host firewall drift was observed on abroad** (an nftables address set's elements changed while the stage ran; addresses masked in the log). "No firewall side effects" is therefore not claimed as proven; the claim is "no BAFT-related firewall change, unrelated host drift observed".
- Evidence label: REAL-LAB. Not production-ready.

### P0-5 incidents found and fixed on the way (all REAL-LAB)

- v0.1.0-rc.3 is **superseded; do not install it on Ubuntu 22.04**. Its `baft-tunnel` binary was linked on ubuntu-24.04 and needs GLIBC_2.39, so it does not start on 22.04 (run 42528251746; the installer refused it at `--version`). Earlier rc `baft-tunnel` assets share this build host. Fixed in #99: release builds on ubuntu-22.04 and refuses a binary above the glibc 2.35 floor.
- Leftover-process incident: in run 42528251746 a `set -e` + `(exit N)` stage trap skipped the main cleanup, leaving that run's echo helper (127.0.0.1:19090) and `/tmp/baft-tunnel-e2e-*` directory on the LAB hosts; the earlier statement that the failed run left the hosts unchanged was wrong. The next run (42533396921) refused to start at its port preflight. Fixed in #100; leftovers removed by the scoped `cleanup_stale=1` run 42534285191.
- #101: the stage now accepts the dash-grouped `setup init` checksum (run 42534426118). #102: the refusal checks no longer use `pkill -f <path>`, which killed its own remote shell (run 42535679925). #103: firewall drift is shown (masked) and classified instead of a bare hash mismatch (run 42536874662).

## P1 operations evidence (ADR-0024): diagnostics, end-to-end status, traffic counters, audit, 2026-11-09

PR #109 merged as main `4852ecf`. What the agent (`--instance NAME`) now serves: `POST/GET /v1/diagnostics` (tiers tcp_reachability, secure_channel, identity, authorization, session, data_path, integrity), `GET /v1/status` → `tunnel` (state, per-tier evidence, last failure stage, `last_verified_unix`), `GET /v1/traffic` (bytes per direction, rates, interval, freshness) and audit v2 (actor fingerprint, target, resource, receipt id). Integrity always stays Unknown.

| Evidence | Result |
|---|---|
| Baseline CI, main `92b0645` | phase1-rust run 42542916316: success |
| PR CI, head `f4c78cc` | phase1-rust 42545238536 (rust + ubuntu-2204-baseline with real-systemd step 4b), phase0 42545238535, fuzz-smoke 42545238632: success |
| Exact-SHA CI, main `4852ecf` | phase1-rust run 42545957126: success |
| Real-host LAB, main `4852ecf` (local build, binary sha256 `6160c9be…d5e4`) | phase1-two-host-e2e run 42545941726, mvp=1: PASS. Both hosts `p1_evidence=1`: traffic from_peer = to_peer = 2,162,825 bytes (64 KiB + 2 MiB sent), all tiers pass up to data_path, integrity unknown, state `carrying_traffic` with the correct peer id, audit with actor and no token. After revocation the dialing node reports `secure_channel: fail` (`p1_revoked_tier=secure_channel`). Cleanup: uninstall proven, listeners/units unchanged, no BAFT firewall rules; unrelated nft address-set drift on abroad (as in P0-5) |

Open at this historical snapshot: remote Job receipts (need the P1-1 enrollment ADR); counter history beyond the agent's one-hour memory; read-only firewall inspection and "Add from Link" (P1 items 7 to 8). B1 structured localized evidence was implemented and its exact-head CI passed later (recorded below).

## 2026-11-11 — Current main acceptance addendum (U1 + D2)

Current repository baseline: `main` `b2c8ab5020754cad15485371a1f062de4092229f`.

- **U1 CRL renewal and real push — REAL-LAB PASS:** [run 42656507521](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42656507521), product code SHA `8be74376176442106669f4a81293f0865629ecaf` (ancestor of current main), `LAB_CRL_PUSH=1`. Offline `renew-crl` issued a 30-day CRL; `push-crl.sh` delivered and activated it on both nodes sequentially; CRL number advanced `0x1002→0x1003`; both agents reported `crl_state=ok`; 2 MiB integrity passed after activation and recovery was 2 s; a 2-day CRL and replayed CRL were refused. Overall `PHASE1_TWO_HOST_E2E=PASS`. The run also observed unrelated firewall drift on abroad; no BAFT firewall rules remained. This is historical evidence on the exact listed SHA, not a test of current HEAD.
- **D2 panel-managed TLS PKI — merged and trusted repeat PASS:** PR [#150](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/150), merge commit `e5a7451f259be7df14187a58db5fe1c29c51a81b`. Trusted repeat [run 42996224181](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42996224181) used immutable RC.7 product candidate `51c0f53cb3cc71b2f...`, built without LAB credentials. On both Ubuntu 22.04 hosts, panel→Agent→root helper→isolated offline CA→certificate/CA/CRL import→pairing→Start passed; private node keys remained local and unreadable by Agent; CA private key remained in network-isolated runner tmpfs; checksum tampering and revoked peers were refused; 64 KiB and 2,097,289-byte payload integrity passed; listener-restart recovery and cleanup passed. Run result `PHASE1_TWO_HOST_E2E=PASS`, `PANEL_PAIRING_RESULT engine=tcp-tls`, `baft_fw_rules=0`, `host_drift=none`.
- **Drift qualification:** the earlier nftables set delta from run 42989166468 was not reproduced in run 42996224181. Its external owner/source remains unidentified; preserve S10 as a separate environmental follow-up. Do not claim attribution or use this as proof of universal firewall neutrality.
- Scope: D2 evidence is Direct only; no soak, impairment, rollover, reboot or Reverse coverage in this repeat. No Stable release or production deployment is implied.


## 2026-11-11 — U7 harness correction on current main

- Historical product parent `1219f547e6d9c2fe9605385f37a7d5dd8dab2995` includes PR #162's U7 host-side checksum correction. Main subsequently advanced through U8 documentation PR #165 to `52a2825254f8254386f71af7220c7cb244200fc2`.
- At the time of this note, trusted U7 run [43063199428](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/43063199428) had passed release-tag and asset checks while the two-host stage was still running. The final accepted outcome is recorded in the following U7 addendum.
- This addendum preserves the prior evidence and does not close or reopen product acceptance gates by itself.


## 2026-11-11 — TRUSTED U7 SIGNED RC.7 TWO-HOST ACCEPTANCE PASS

- Workflow: [43063199428](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/43063199428), pinned to product parent SHA `1219f547e6d9c2fe9605385f37a7d5dd8dab2995`. All three jobs passed: release-tag ancestry, signed RC.7/RC.5 asset verification, and real two-host U7 acceptance.
- Package: signed `v0.1.0-rc.7`; previous signed prerelease RC.5 used for cross-version upgrade. The release package and binary were verified by the workflow before transfer to the LAB runner.
- Two-host result: `PHASE1_TWO_HOST_E2E=PASS`. `OPS_RESULT` recorded `integrity_64k_2m=1`, `cross_version_update=1`, `backup_restore=1`, `bad_update_rolled_back=1`, `revoked_refused=1`, `uninstalled=1`, `soak_s=3600`, `rounds=264`, `failed_probes=0`, `max_probe_s=5`, `restarts=0->0/0->0`, and `baft_fw_rules=0`.
- The run exercised clean install and guided Direct setup, Apply start on serve and forward, diagnostics/status/traffic/audit, 64 KiB and 2 MiB matching SHA-256 payloads, RC.5→RC.7 upgrade, a one-hour traffic soak, SIGKILL/restart and Apply recovery, backup/simulated loss/restore with owner/mode preservation, guarded update and automatic rollback of a failing binary, certificate revocation refusal, uninstall and cleanup.
- Limits: `ready` and diagnostics `integrity` remained Unknown by design. U7 ran Direct only; `crl_push=0` (U1 has separate two-host evidence in run 42656507521). No Reverse, BAFT, Noise, Stable or production claim. `unrelated_fw_drift=abroad` recorded a six-line address-set delta; no BAFT-TUNNEL firewall rules existed, but the external source remains unknown (S10).


## 2026-11-11 — B1 STRUCTURED LOCALIZED EVIDENCE EXACT-HEAD CI PASS

- Draft implementation was first tested at `cc37f48bbfe96542c5c657715d0b9c01370de365` in [PR #164](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/164). All four workflow groups passed: phase1-rust, phase0, fuzz-smoke and same-SHA package/signature verification.
- Linux Rust exact-head checks passed dependency audit, license policy, formatting, focused tests, Clippy, release build, staged installer/credential/rollback, signed one-line installer, Persian/English interactive menu/PTY, encrypted credential delivery, installed systemd lifecycle, and CRL activation/rollback. Ubuntu 22.04 baseline, panel UI, offline CA and fuzz-smoke all passed.
- Two earlier assertions were corrected without weakening Unknown handling: unreadable systemd unit state keeps the unit-level check action as next step; the identity-tier `next_code` is asserted separately.
- The original PR #164 branch is behind main after U8 PR #165 merged and was closed as superseded. B1 was synchronized onto post-U8 main in [PR #168](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/168), head `1272dcac72d553b653ef7eb4f8f72047528c3291`; all four exact-head CI workflow groups passed. Merge remains pending. No LAB or runtime operation was performed for B1.
