# Fleet Join Hub Listener Integration Design

## Intent

Connect the existing bounded Fleet Join transport to the Hub Agent's enrollment lifecycle. The Hub exposes one opt-in Join listener shared by all active offers, admits a request only through the existing Enroller and operator approval flow, and advances an offer to `AwaitingReport` only after a positive transport ACK. The existing PR #404 certificate-bound first-report callback remains the only path from `AwaitingReport` to `Enrolled`.

This is Hub-side integration only. It does not implement Member-side persistence or report orchestration and does not claim end-to-end or two-host acceptance.

## Goals

- Run one long-lived Join listener for the configured Hub, rather than binding one TCP port per offer.
- Support multiple concurrent, bounded Join sessions and multiple live offers on that listener.
- Validate offer ID, node ID, token, nonce, CSR profile, offer expiry and current Enroller state before delivery. The nonce must also appear exactly once as the PKCS#9 `challengePassword` attribute inside the CSR's signed CertificationRequestInfo; checking only the separate JoinRequest field is insufficient.
- Preserve the operator approval gate: the first Join request submits the CSR and waits; only the existing authenticated `POST /v1/fleet/enroll/confirm` may sign through the root Fleet helper. The Join listener never signs. Refactor confirmation into a two-phase Enroller transition so the Enroller mutex is released while the bounded root-helper request runs.
- Permit bounded idempotent retries of the exact same submitted CSR/token/nonce while the offer is `Submitted` or `Issued`; a changed CSR or credential is rejected. The first submit consumes the token for any new request; this exact retry is a replay of the same operation, not a second submission. Store a SHA-256 digest of the original CSR so retry matching does not depend on retaining or re-signing a modified request.
- Deliver only the member leaf certificate, public Fleet CA certificate, CRL and Hub certificate.
- Move the matching offer to `AwaitingReport` only after `JoinAck { ok: true }`; refusal, disconnect, timeout, malformed frames, expired or denied offers must not do so.
- Generate a validated, one-time `JoinOffer` from the authenticated Hub certificate identity and explicit operator-configured dial address.
- Preserve the PR #404 rule: Join ACK is not enrollment; only a later accepted mandatory-mTLS report from the exact delivered leaf can transition to `Enrolled`.

## Non-goals

- Member-side private-key generation, durable trust-bundle persistence, automatic report startup or restart recovery.
- A public unauthenticated enrollment API. The operator offer route remains behind the existing Agent bearer-token authentication.
- React UI changes, firewall opening, port forwarding, route/sysctl changes, `CAP_NET_ADMIN`, real-host operations or LAB execution.
- Replacing the root helper or changing Fleet CA signing policy.
- Reverse support, Stable/Public readiness, merge, release or deployment.

## Configuration and provisioning contract

The existing Hub report listener and Join listener are distinct protocols and must not share one TCP socket.

Extend Hub provisioning with one optional value:
- `join_listen`: local `SocketAddr` to bind, with a nonzero port.

The advertised host is derived from the already-validated `dial_san` used to issue the Hub certificate. Do not accept a second independently supplied advertised hostname: it could differ from the only DNS/IP SAN on the certificate and create a JoinOffer that fails identity verification.

When enabled, root-managed Fleet config stores a `join` section containing the bind socket, advertised host and port, Hub ID (the provisioned `hub_name`) and TLS server name (both host values derived from the provisioned `dial_san`). When omitted, no Join listener starts and enrollment-offer creation returns a clear unavailable response without creating an Enroller offer. Existing Hub provisioning requests without Join fields remain valid.

The advertised port is the configured `join_listen` port. The bind IP may be a wildcard address; the advertised host is always derived from the certificate SAN and must never be inferred from the bind address or the local machine.

## Runtime design

- `fleet_run::start` keeps the existing mandatory-mTLS acceptor exclusively for report ingestion. If Join config is present, it binds a separate TCP listener and builds the existing `tls::fleet_join_acceptor(cert, key)` (TLS 1.3, no client certificate) for the pre-enrollment exchange. The Join protocol's one-time token/nonce and CSR proof are its application-level admission controls; the Hub is pinned by the JoinOffer.
- Refactor the current single-offer transport into a shared-listener dispatcher. Each connection is framed and bounded before the admission callback is invoked. The callback validates/submits the request through Enroller without holding its mutex across awaits. If the offer is `Submitted`, return a bounded `approval_pending` refusal; do not sign. If the operator has confirmed it and the offer is `Issued`, return the already-signed certificate from Enroller plus public CA/CRL/Hub material. The existing confirm route remains the only signer path. Never reuse the report listener's mandatory-mTLS acceptor for Join: a new Member has no certificate yet.
- The successful response is sent over the authenticated TLS connection. Only after a valid positive `JoinAck` does the completion callback call `Enroller::mark_delivered(offer_id, now)`. The callback must re-check current state/expiry so a concurrent deny or expiry cannot resurrect an offer.
- Denial, timeout, disconnect, failed persistence ACK, invalid CSR, invalid credentials leave the offer un-delivered or closed according to the existing Enroller rules. No state transition occurs on a transport write alone.
- Listener shutdown follows the Agent's existing watch-channel lifecycle. Per-connection tasks remain bounded. Use bounded rolling failure accounting (per-IP and global) so a permanent shared listener is not terminated by a lifetime-cumulative failure counter; the accounting map must itself have a hard size bound.
- Never hold the Hub/Enroller mutex while performing network I/O or calling the root helper. Add `State::Signing` to the Enroller state machine. Confirmation must claim this `Signing` state under lock, release the lock for the helper operation, then reacquire and commit only if the offer is still in `Signing` and unexpired. A concurrent deny/expiry must prevent a late signer result from issuing or delivering a certificate.

## Offer API

Keep `POST /v1/fleet/enroll/offer {"node_id":"..."}` behind the existing bearer-token check.

When Join is configured:
1. Create the Enroller offer.
2. Derive Hub leaf pin from the configured Hub certificate DER; derive Hub ID and TLS name from the provisioned config, not request data.
3. Build and validate `JoinOffer` using configured advertised host/port, Hub ID/name, offer ID, node ID, token, nonce and expiry.
4. Return the encoded one-time Join offer to the authenticated operator exactly once. Do not log it or include token/nonce in list/status responses.

If construction fails after offer creation, close that offer before returning an error. If Join is not configured, return unavailable before creating any offer.

## Security and concurrency constraints

- The client-supplied `offer_id`, `node_id`, token and nonce are untrusted; validate all against the live Enroller entry.
- The CSR must pass existing Fleet CA CSR validation for the exact offered node ID and contain the exact offer nonce as one PKCS#9 `challengePassword` attribute covered by the CSR signature. Missing, duplicate or mismatched challenge attributes are rejected. Reconnect retries must match the stored CSR digest and original offer credentials exactly.
- No private key is accepted in any Join frame or response.
- No Hub or Enroller lock may be held across an await, network write/read, or helper call.
- A positive ACK for one connection must not complete another offer; bind the completion to the exact offer ID and node ID carried by the validated request.
- Concurrent denial/expiry wins over a late ACK or late root-helper result; a failed/replayed request cannot restore a closed offer.
- Enforce the existing frame size, handshake and I/O deadlines, connection cap, and bounded failure rate. A shared listener must not accumulate unbounded per-IP state or stop permanently because of old failures.
- Do not advertise Join when its listener is not configured and bound successfully.

## Acceptance

Focused tests must demonstrate:
- Existing configurations without Join remain valid and do not start a listener.
- Partial/invalid Join configuration is rejected; valid config validates bind and advertised host independently.
- Multiple different offers can be served over one listener without cross-completing each other.
- Invalid token/nonce/node/CSR, missing/duplicate/wrong CSR challengePassword, expired offer, denied offer, malformed frame, TLS failure, timeout, disconnect and negative ACK do not enter `AwaitingReport`.
- A valid positive ACK changes only its exact offer to `AwaitingReport`.
- A concurrent deny/expiry cannot be undone by a late ACK.
- The existing operator confirm route is the only signing path; the Join listener must not sign or bypass operator confirmation.
- The generated offer contains the exact Hub certificate pin and configured advertised address; secrets are absent from logs and list/status.
- Listener cancellation and bounded rate accounting work under concurrent sessions.

Hosted exact-SHA Linux CI is required. No Member persistence, first-report end-to-end, two-host LAB, production or Stable claim is implied.
