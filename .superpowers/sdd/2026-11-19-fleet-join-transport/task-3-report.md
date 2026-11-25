# Task 3 report — One-shot Hub Join listener and member session

Status: **PASS — implementation and exact-SHA CI verified; test-first RED evidence item not independently recovered**

Implementation HEAD: `26f66133117a4c84fb6544e845e3e49c25b13483`
PR: [#403](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/403) — OPEN / Draft, stacked on PR #402 exact HEAD `337901e18f47f1c54a0f17a07704cefd8f273b36`.

Implemented:
- One-shot Hub listener with bounded concurrent sessions, per-IP/total failure controls, offer expiry, and explicit retry/close/deliver admission outcomes.
- Member client verifies the exact pinned Hub leaf and identity before sending the CSR.
- TLS session remains open through admission and public certificate/trust delivery; the member acknowledges only after its caller-side persistence/report step.
- Success is not returned without a valid member ACK. The transport ACK does not transition Enroller to Enrolled.
- Bounded frame size and read/write/connect/handshake/session deadlines; malformed, replayed, stalled, oversized and disconnected requests fail closed.
- CA/CRL and public Hub identity are delivered only inside the already pinned TLS session; no private key is sent.

Behavioral tests in `src/fleet_join.rs` include:
- `fleet_join_loopback_delivers_public_trust_and_waits_for_member_ack` — generated Hub certificate, valid member CSR, successful delivery and ACK lifecycle.
- `fleet_join_client_rejects_wrong_pin_and_wrong_hub_identity_before_request`
- `fleet_join_rejects_expired_offer_before_opening_a_connection`
- `fleet_join_server_refuses_wrong_offer_id`
- `fleet_join_server_retries_bad_token_and_nonce_without_delivering_certificates`
- `fleet_join_server_rejects_wrong_node_identity`
- `fleet_join_replay_and_concurrent_submission_deliver_only_once`
- `fleet_join_rejects_oversized_and_stalled_frames_without_success`
- `fleet_join_disconnect_before_response_never_reports_delivery_success`
- `fleet_join_member_rejects_hub_certificate_bundle_not_matching_tls_pin`
- `fleet_join_server_closes_offer_when_admission_controller_closes_it`
- `fleet_join_server_does_not_mark_delivery_success_without_member_ack`

Exact-SHA hosted evidence for `26f66133117a4c84fb6544e845e3e49c25b13483`:
- `phase1-rust` run [44562305399](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/44562305399) — SUCCESS; Rust focused tests, formatting, Clippy, release build and Ubuntu 22.04 systemd baseline all passed.
- The same run's Ubuntu 24.04 installer, React panel, UI/Playwright, and offline CA container jobs also passed.
- The current PR status rollup reports 13/13 completed-success checks on this exact head after PR #403 was correctly re-based onto PR #402.
- Local `cargo fmt --all -- --check` and `git diff --check` — PASS.

Test-first note:
- The PR description records that the test-only parent `5a71a2a` failed because the new lifecycle tests referenced `join`, `serve`, and `Admission` before implementation. During this checkpoint, the exact run URL/result for that parent could not be independently recovered; the plan therefore leaves the explicit RED-evidence checkbox open.

Limitations:
- No Enroller integration, durable Member certificate/CA/CRL persistence, first mandatory-mTLS Report correlation, or `AwaitingReport -> Enrolled` transition.
- No two-host LAB acceptance. Fleet Join remains unadvertised and cannot mark a node Enrolled.
- No firewall, route, sysctl, `CAP_NET_ADMIN`, real-host, release or deployment changes.
