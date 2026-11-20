# Fleet Join Member Activation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Complete the local Member Join path from a one-time Hub offer through durable credential installation and the first mTLS report attempt.

**Architecture:** Add a root-only local CLI flow that creates the Member key and challenge-bound CSR, reuses exact pending state on retry, validates and atomically publishes a versioned credential bundle plus Member config, then ACKs and queues the existing Agent restart. Make Hub delivery idempotent for exact retries in `AwaitingReport` so a lost ACK response is recoverable.

**Tech Stack:** Rust, Tokio, Clap, OpenSSL-backed existing Fleet CA helpers, rustls/tokio-rustls, serde, existing Fleet control helper and systemd Agent lifecycle.

**Spec:** `docs/superpowers/specs/2026-11-20-fleet-join-member-activation-design.md`

## Global Constraints

- No `CAP_NET_ADMIN`, firewall, route, sysctl or real-host changes.
- No SSH enrollment, panel API, inbound Member listener, Go in core or Reverse enablement.
- Member private key is generated locally and never enters a Join frame, helper request, stdout, stderr or log.
- Positive ACK follows durable credential and config persistence; ACK never means `Enrolled`.
- Only the exact issued leaf's first accepted mandatory-mTLS report transitions the Hub offer to `Enrolled`.
- Preserve existing Hub config when adding Member config; refuse unsafe paths and conflicting state.
- No merge, release, deploy or two-host LAB.

## Review Focus

- A process crash between durable publication, ACK, restart queue and pending-state cleanup must be recoverable without creating a new CSR.
- A same-offer retry after the Hub accepted ACK must not fail merely because the offer is already `AwaitingReport`.
- Existing Hub-only Fleet config must remain byte-semantically equivalent in meaning after adding a Member section.
- A symlink, wrong owner/group/mode, partial file, malformed PEM or pre-existing generation must fail closed without replacing user data.
- A restart queue success must not be presented as proof that the first report was accepted or the node is Enrolled.

---

### Task 1: Make Hub delivery safely idempotent after ACK

**Files:**
- Modify: `src/fleet_enroll.rs`
- Modify: `src/fleet_run.rs`
- Test: `src/fleet_enroll.rs`, `src/fleet_run.rs`

**Interfaces:**
- `Enroller::submit` accepts only an exact digest/token/nonce/node retry in `Submitted`, `Issued`, or `AwaitingReport` while unexpired.
- `Enroller::certificate_for_delivery` returns the existing certificate in `Issued` and `AwaitingReport` only.
- `Enroller::mark_delivered` is idempotent in `AwaitingReport` only when the stored delivered fingerprint equals the fingerprint derived from the same issued certificate.
- `admit_join_request` may return `Deliver` for exact retry in `AwaitingReport`; it must never reissue or change the certificate.

- [x] Write tests for exact retry in `AwaitingReport`, changed CSR rejection, changed token/nonce/node rejection, repeated ACK no-op, and deny/expiry still refusing delivery.
- [x] Run focused Enroller and Fleet Join admission tests; confirm the new retry test fails for the expected state rejection (run `44705660845`).
- [x] Implement the narrow idempotent retry behavior without changing the original issued certificate or delivered fingerprint (`0656744`).
- [x] Run the focused tests and the full Rust test suite in Linux CI (exact SHA `a66e16d`, run `44708622808`, all jobs success).

### Task 2: Add secure Member bundle storage and atomic config publication

**Files:**
- Create: `src/fleet_member.rs`
- Modify: `src/main.rs` (module declaration only if needed)
- Test: `src/fleet_member.rs`

**Interfaces:**
- Define `MemberPaths` for one instance and immutable generation paths.
- Define `PendingJoin` containing the validated `JoinOffer`, exact CSR and local private-key path; secrets are never Debug-printed.
- Implement `prepare_pending(instance, offer)` to create or load one exact pending transaction. It must refuse a different offer while one is pending and refuse symlink/non-regular/unsafe existing paths.
- Implement `publish_bundle(instance, offer, response, key_path)` to validate PEM/key/cert/CA/CRL, publish an immutable generation atomically, and return the exact public/private paths.
- Implement `write_member_config(instance, member_section)` to preserve any valid existing Hub section and atomically add MemberSection.
- Implement `finish_pending(instance)` to remove only the one-time offer and CSR after successful ACK and restart queue; retain the Member private key and public trust files.

- [x] Write tests first for directory/file modes, create-new behavior, symlink refusal, atomic generation publication, injected write failure leaving no published partial generation, config merge preserving Hub, malformed config refusal, and pending offer mismatch refusal (13 tests in `src/fleet_member.rs`).
- [x] Run the focused storage tests and observe expected failures before implementation (2 passed / 11 failed on the stub tree, local macOS).
- [x] Implement root-owned directory/file creation with explicit `fchown`/`fchmod`, `sync_all`, same-filesystem rename and parent-directory sync (`47f6bb7`).
- [x] Run focused storage tests and Linux Rust CI (exact SHA `47f6bb7`, run `44710353743`, six of six jobs success, 489 tests passed).

### Task 3: Implement Member Join orchestration and resume semantics

**Files:**
- Modify: `src/fleet_member.rs`
- Modify: `src/fleet_join.rs` only if a narrowly scoped validation/session helper is needed
- Test: `src/fleet_member.rs`, `src/fleet_join.rs`

**Interfaces:**
- `async fn join_from_offer(instance: &str, encoded_offer: &str) -> Result<JoinOutcome>` performs the exact persisted pending transaction.
- Generate EC P-256 key material locally and create the CSR with exactly one PKCS#9 `challengePassword` equal to the offer nonce.
- Save offer, private key and CSR before network submission; retries reuse the same bytes and CSR digest.
- After `fleet_join::join`, validate the member leaf/key/CA/CRL and authenticated Hub certificate, then publish bundle and Member config before positive ACK.
- ACK only after durable publication. Keep retry state until restart has been queued successfully.
- `async fn resume(instance: &str) -> Result<JoinOutcome>` reuses pending offer/CSR if present; otherwise validates the installed Member config and queues only an Agent restart.

- [ ] Write tests for CSR challengePassword presence/uniqueness, P-256 key profile, private-key exclusion from JoinRequest/JoinResponse, exact retry reusing the same CSR, invalid response causing no positive ACK, and resume behavior for pending vs installed state.
- [ ] Run the focused tests and observe expected failures.
- [ ] Implement orchestration using the existing pinned TLS Join client and no new network listener.
- [ ] Run focused Join/Member tests and Linux Rust CI.

### Task 4: Add `fleet join` and `fleet join --resume` CLI and restart integration

**Files:**
- Modify: `src/main.rs`
- Modify: `src/fleet_helper.rs`
- Test: `src/main.rs`, `src/fleet_helper.rs`, `src/fleet_member.rs`

**Interfaces:**
- Add `FleetCommand::Join { instance: String, resume: bool }`.
- Normal Join reads one bounded offer line from stdin; it must not accept the token-bearing offer as an argv value.
- Both paths require effective UID 0 and validate `apply::valid_instance`.
- Extend the existing `RestartAgent` helper guard to accept a valid Member-only config as well as the current Hub credential config; retain exact per-instance systemd restart scope.
- Queue restart only after successful Join ACK. If queueing fails, preserve pending state and return an error. `--resume` must retry safely.
- Output only non-secret state: `restart_queued`, `report_pending`, or an actionable error. Never print `Enrolled` from the Member CLI.

- [ ] Write CLI parse/input-bound tests, root/instance validation tests, Member-only restart helper tests, and restart-failure pending-state retention tests.
- [ ] Run tests and confirm missing command/guard behavior fails as expected.
- [ ] Implement CLI and helper integration.
- [ ] Run CLI/help tests, focused Fleet tests and Linux Rust CI.

### Task 5: Documentation, exact-SHA verification and checkpoint

**Files:**
- Modify: `README.md` or the relevant Fleet operator guide only where needed
- Modify: `docs/PRODUCT_ROADMAP.md` only if the new stage changes roadmap truth
- Update: this plan's checkboxes and Issue #2 checkpoint after evidence is collected

- [ ] Document the stdin-based command, root requirement, pending retry and resume behavior, credential paths/modes, and the distinction between restart queued, report pending and Enrolled.
- [ ] Run `cargo fmt --all -- --check`, `git diff --check`, focused Fleet tests, full Rust tests, Clippy and release build on Linux CI for the exact branch SHA.
- [ ] Inspect the final diff for secret leakage, symlink races, unsafe replacement, config loss, unbounded staging state and accidental host/network operations.
- [ ] Push the feature branch and open/update a Draft PR only after the implementation and exact-SHA CI evidence are ready; do not merge.
- [ ] Append an Issue #2 checkpoint with exact branch/SHA, PR, workflow IDs, test results and the explicit absence of two-host LAB evidence.
