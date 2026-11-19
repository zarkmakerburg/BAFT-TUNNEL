# Fleet Join Hub Listener Integration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Connect the existing Fleet Join protocol to a single opt-in Hub listener and the Enroller state machine, without claiming Member-side or end-to-end completion.

**Architecture:** Add explicit optional Join bind configuration with the advertised host derived from the Hub certificate SAN to the root-managed Hub setup. Run one bounded listener shared by offers, validate through Enroller and preserve the existing operator/root-helper signing gate, and mark delivery only after the matching positive Join ACK. Generate the one-time encoded JoinOffer from the configured address and the Hub certificate identity.

**Tech Stack:** Rust, Tokio, tokio-rustls, rustls, serde, existing Fleet Enroller/CA/helper/Agent modules.

**Spec:** `docs/superpowers/specs/2026-11-20-fleet-join-hub-listener-design.md`

## Global Constraints

- No `CAP_NET_ADMIN`, firewall, route, sysctl or real-host changes.
- No merge, release, deploy or two-host LAB.
- Fleet CA private key remains root-helper-only.
- Join ACK advances only to `AwaitingReport`; only PR #404's exact-certificate first accepted report may advance to `Enrolled`.
- Member-side persistence/report orchestration is explicitly out of scope.
- Preserve existing Hub provisioning requests when Join fields are omitted.
- No Reverse exposure and no Go in core.

## Review Focus

- A late positive ACK racing with deny or expiry must not resurrect an offer.
- A malformed or oversized request must not consume a valid offer's token unless the existing single-use validation boundary explicitly requires closure.
- A long-lived listener must not terminate after a lifetime-cumulative failure count or retain unbounded peer-IP state.
- Unsupported or malformed dial SAN input must not generate an unusable or identity-mismatched JoinOffer.
- Operator signing or response-delivery failure must not move the offer to `AwaitingReport`.

---

### Task 1: Add opt-in Join configuration to Hub provisioning

**Files:**
- Modify: `src/agent.rs`
- Modify: `src/fleet_helper.rs`
- Modify: `src/fleet_run.rs`
- Test: existing unit tests in those modules

**Interfaces:**
- `HubSection.join: Option<JoinSection>` with exact fields `listen: SocketAddr`, `advertise_host: String`, `advertise_port: u16`, `hub_id: String`, and `hub_name: String`.
- `Ctx.fleet_join_offer: Mutex<Option<JoinOfferContext>>`, populated only after the listener binds; `JoinOfferContext` contains the exact advertised host/port, Hub ID/name and Hub leaf SHA-256 pin.
- `fleet_helper::request_hub_identity(..., join_listen: Option<&str>, ...)` carries the optional bind socket to the root helper.
- Root helper request accepts optional `join_listen`; the advertised host is derived from the validated `dial_san`.

- [x] Write `hub_provision_rejects_invalid_join_bind`, `hub_provision_derives_join_host_from_dial_san`, and `hub_provision_preserves_legacy_without_join` tests in `src/fleet_helper.rs`; write `fleet_config_validates_optional_join_section` in `src/fleet_run.rs`.
- [x] RED confirmed on `b02bbd1` (450 passed, 3 expected new failures); GREEN confirmed by `cargo +1.88.0 test --locked --all-targets` on exact SHA `343ba1e` in phase1-rust run `44631710539`.
- [x] Add strict validation for nonzero bind port and the derived TLS server name.
- [x] Persist the optional Join section in the root-generated config using the provisioned Hub ID and TLS dial name.
- [x] Preserve compatibility for existing requests that omit `join_listen`.
- [x] Full Rust test suite and `cargo fmt --all -- --check` passed on exact SHA `343ba1e` (phase1-rust run `44631710539`).

### Task 2: Build a shared, bounded Join listener dispatcher

**Files:**
- Modify: `src/fleet_join.rs`
- Test: `src/fleet_join.rs` unit tests

**Interfaces:**
- Introduce `serve_shared<F, Fut, A, AFut>(listener: TcpListener, acceptor: TlsAcceptor, admit: F, acknowledged: A, shutdown: watch::Receiver<bool>) -> Result<()>`, where `admit(JoinRequest, SocketAddr) -> Fut<Output = Admission>` and `acknowledged(String, String) -> AFut`. Define `Admission::Deliver { response: JoinResponse, expires: u64 }` so each session is bounded by its offer expiry.
- Delivery completion callback receives the exact validated `offer_id` and `node_id` only after `JoinAck { ok: true }`.
- Existing single-offer transport tests remain covered or are migrated to the shared listener API.

- [ ] Write `shared_listener_serves_two_distinct_offers`, `shared_listener_ack_completes_only_matching_offer`, `shared_listener_negative_ack_never_completes_offer`, `shared_listener_disconnect_before_ack_never_completes_offer`, `shared_listener_rejects_malformed_and_oversized_frames`, and `shared_listener_rate_accounting_is_bounded` tests in `src/fleet_join.rs`.
- [ ] Run `cargo test shared_listener --bin baft-tunnel`; verify the expected missing shared-listener behavior.
- [ ] Implement bounded connection/session handling and listener shutdown through the Agent watch channel.
- [ ] Implement bounded rolling failure accounting with a hard cap on tracked IPs.
- [ ] Ensure malformed frames, negative ACK, timeout and disconnect never invoke delivery completion.
- [ ] Run all `fleet_join` tests.

### Task 3: Bridge listener admission to Enroller and operator approval state

**Files:**
- Modify: `src/fleet_run.rs`
- Modify: `src/agent.rs`
- Modify: `src/fleet_enroll.rs`
- Modify: `src/fleet_ca.rs`
- Modify: `src/fleet_api.rs`
- Test: corresponding module unit tests

**Interfaces:**
- Admission validates `JoinRequest` against the exact live Enroller offer and CSR profile. `fleet_ca::validate_node_csr` must validate the exact offer nonce as one PKCS#9 `challengePassword` attribute inside the CSR signature.
- The existing authenticated `POST /v1/fleet/enroll/confirm` remains the only signing path through the root helper; the Join listener never signs. Add `State::Signing` and `SignRequest { id: String, node_id: String, csr_pem: String }`. Confirmation uses `Enroller::begin_confirm(id, fingerprint, now) -> Result<SignRequest, Error>` and `Enroller::finish_confirm(id, now, result: Result<String, Error>) -> Result<(), Error>` so no Enroller lock is held while the root helper runs; finish commits only if the offer remains `Signing` and unexpired.
- An exact repeated JoinRequest is idempotent while the offer is `Submitted` or `Issued`; changed CSR/token/nonce is rejected.
- Use `tls::fleet_join_acceptor(cert, key)` for Join TLS; do not reuse the report listener's mTLS acceptor.
- Successful response contains only the already-issued member certificate, public CA certificate, CRL and Hub certificate.
- Positive ACK callback calls `mark_delivered(offer_id, now)` after rechecking current state.

- [ ] Write `fleet_csr_requires_exact_offer_nonce_challenge_password` and `fleet_csr_rejects_missing_duplicate_or_wrong_nonce_challenge` in `src/fleet_ca.rs`; write `enroller_submit_allows_only_exact_idempotent_retry_after_first_submit`, `enroller_submit_rejects_changed_csr_after_first_submit`, `enroller_begin_confirm_claims_signing_state`, `enroller_finish_confirm_rejects_late_result_after_deny`, `enroller_finish_confirm_rejects_expired_result`, and `enroller_signing_failure_never_marks_delivered` in `src/fleet_enroll.rs`; write `fleet_confirm_releases_enroller_lock_during_helper_call`, `join_admission_waits_for_operator_confirmation`, `join_admission_delivers_only_after_confirmation`, and `join_ack_racing_deny_cannot_resurrect_offer` in `src/fleet_run.rs` / `src/agent.rs`.
- [ ] Run `cargo test fleet_csr_ --bin baft-tunnel`, `cargo test enroller_submit_ --bin baft-tunnel`, `cargo test enroller_begin_confirm --bin baft-tunnel`, `cargo test enroller_finish_confirm --bin baft-tunnel`, `cargo test enroller_signing_failure --bin baft-tunnel`, `cargo test fleet_confirm_releases_enroller_lock --bin baft-tunnel`, `cargo test join_admission_ --bin baft-tunnel`, and `cargo test join_ack_racing_deny --bin baft-tunnel`; confirm expected failures.
- [ ] Implement exact-request retry validation and return `Retry` while awaiting operator confirmation; never sign from the listener. Refactor operator confirmation into begin/sign/finish phases so the root helper runs without the Enroller mutex held.
- [ ] Deliver the certificate only after the operator confirm route has moved the offer to `Issued`; assemble only public trust material.
- [ ] Implement exact offer completion only after a positive ACK.
- [ ] Run focused Enroller, Join and Agent tests.

### Task 4: Generate a one-time JoinOffer from the operator API

**Files:**
- Modify: `src/agent.rs`
- Modify: `src/fleet_api.rs`
- Test: `src/agent.rs` and `src/fleet_api.rs` unit tests

**Interfaces:**
- `POST /v1/fleet/enroll/offer` remains bearer-authenticated.
- When Join is configured and bound, response includes one encoded validated `JoinOffer`.
- When Join is not configured, return unavailable without creating an Enroller offer.
- On encoding failure after offer creation, close the offer before returning.

- [ ] Write `fleet_offer_unavailable_without_join_listener`, `fleet_offer_contains_exact_hub_pin_and_advertised_endpoint`, and `fleet_offer_encoding_failure_closes_created_offer` tests in `src/agent.rs`.
- [ ] Run `cargo test fleet_offer_ --bin baft-tunnel`; verify expected failures.
- [ ] Implement offer construction using configured bind port, derived dial SAN and parsed Hub certificate DER.
- [ ] Ensure token/nonce are not logged or exposed by list/status responses.
- [ ] Run focused API and certificate tests.

### Task 5: Runtime wiring and exact-SHA CI

**Files:**
- Modify: `src/fleet_run.rs`
- Update: this plan's checkboxes and Issue #2 checkpoint after evidence is collected.

- [ ] Add `JoinOfferContext { host: String, port: u16, hub_id: String, hub_name: String, hub_pin: String }` and `Ctx.fleet_join_offer: Mutex<Option<JoinOfferContext>>`; set it only after a successful Join listener bind and successful Hub certificate pin derivation.
- [ ] Wire the listener only when valid Join configuration is present and the bind succeeds; use a separate no-client-auth TLS 1.3 Join acceptor.
- [ ] Verify shutdown and bind failures are visible; do not claim Join is active when bind failed.
- [ ] Run focused Fleet tests and the full Rust test suite.
- [ ] Run `cargo fmt --all -- --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo build --release` locally where supported.
- [ ] Open a Draft PR and require Linux CI on the exact PR head SHA.
- [ ] Inspect diff and security hygiene; record exact evidence and remaining non-goals in Issue #2.
