# ADR-0051: add and configure another server from the panel

Status: **Proposed** (2026-11-17). Security-sensitive: SSH enrollment must not be enabled before the independent review this ADR requests. Builds on ADR-0045 (Fleet, accepted with conditions), ADR-0050 (automatic pairing, Proposed in PR #338), ADR-0023 (control helper), ADR-0033 (panel HTTPS), ADR-0034 (offline signature verification), ADR-0030 (no `CAP_NET_ADMIN`).

## Goal

The owner wants, as in the BAFT project: in the panel, *add a server* (address and credentials), the panel installs and pairs the agent, and the user then configures tunnels on it from the same panel. Tunnel creation between two panel-managed servers is a few choices (Iran or abroad, name, ports), per ADR-0050.

## Decision

Two enrollment paths, shipped in this order. Both end the same way: the remote server is a **paired, enrolled node** and no credential is kept.

### Path A: pasted one-line command (first, no credentials in the panel)

1. The panel asks for a display name and creates an enrollment code (ADR-0050 style: single use, 15 minutes, bearer secret, never logged).
2. It shows one command: the signed quick installer with `--pair CODE` (ADR-0050 as implemented in PR #338: `quick-install.sh --pair CODE`; the first server uses `--offer NAME --service-port PORT`). The user pastes it on the new server (their own SSH session). The command pins a release tag; the panel may show it only for a release that actually contains pairing, so until such a release exists the panel shows the already-installed form `sudo baft-tunnel pair accept CODE` and says the one-line install needs a newer release. The installer verifies the release signature offline with the pinned root (ADR-0034) and registers the agent in setup mode.
3. The new agent dials back outbound to the panel host (pairing listener, ADR-0050; Fleet reporter after ADR-0045 F1), proves the code and sends its CSR. The operator approves on the panel: the approval step shows node id, source address, version and the short `verify` key fingerprint, which the user compares with the new server's screen; approval requires the one-time `nonce` from `GET /v1/pair/status` (ADR-0050 revision). Pairing refuses a config directory that already holds a configuration. Done.

This needs no new attack surface beyond ADR-0050's listener. The one-line installer downloads from GitHub, so on a host without internet it works only if the panel serves the bundle (see L2 below).

### Path B: SSH enrollment (second, off by default, behind review)

The panel runs the same installation over SSH so the user only types address and credentials.

- **Where it runs.** Not in the agent process. A separate short-lived unit `baft-tunnel-enroll@.service`, started through the root control helper (ADR-0023 pattern), under its own sandbox: dedicated user, no capabilities, `NoNewPrivileges`, `ProtectSystem=strict`, `PrivateTmp`, `RestrictAddressFamilies=AF_INET AF_INET6`, no access to CA keys, agent token or state. One enrollment at a time; hard time limit (default 10 minutes).
- **How it connects.** The system OpenSSH client (already on Ubuntu 22.04; no new crate). A Rust SSH library was considered and rejected for now: a new dependency with a large audit surface. Password or key reaches the process only through a pipe or `memfd` that an `SSH_ASKPASS` helper (our own binary) reads; never argv, environment, a regular file or the audit log. Buffers are zeroized. A sudo password, when needed, is sent over stdin to `sudo -S` only.
- **Host key.** No trust on first use and never `StrictHostKeyChecking=no`. The panel shows the host key fingerprint and the operator must confirm it (compare with the provider console). Only the confirmed fingerprint is stored, as part of the node record.
- **What it executes.** A fixed, allow-listed sequence: upload the signed bundle over the SSH channel (so the target needs no internet; useful from Iran), verify the signature on the target with the pinned root, run the installer, then `pair accept -` with the code on stdin (ADR-0050 as revised after its review; no temporary file, nothing in argv), and read back the result. No operator-supplied command, path or script is ever run.
- **Credentials lifetime.** In the browser only in the form field, cleared after submit and never written to storage. On the panel host only inside the helper pipe for the duration of the run. Nothing is persisted, so there is nothing to revoke afterwards.
- **Transport guard.** The form refuses to submit unless the panel is on loopback or HTTPS (ADR-0033).
- **Target guard.** Outbound only to the address the operator typed; loopback, link-local and cloud-metadata addresses are refused; port defaults to 22.
- **Authorization.** Authenticated operator, an explicit "I own this server" confirmation, the host key confirmation, a rate limit (one active run, small attempt budget). Every run is audited: operator, target address, login user, host key fingerprint, step reached, result. Never the password, key or code.
- **Failure and rollback.** Each step is idempotent and recorded. On failure the helper runs the installer's uninstall for only what this run created and reports exactly which step failed; the target is left as it was found. A cancel button stops the run and rolls back the same way.

### After enrollment

- **Trust.** Per-tunnel trust follows ADR-0050 (per-pair CA, name constraints). Fleet reporting identity (`fleet-node` profile, ADR-0045) needs a decision D1 below.
- **Remote configuration.** Once enrolled, the panel configures the node with ADR-0045 typed jobs (plan, apply, start/stop), not with SSH. Until F3 exists the panel offers only inventory and the guided pairing; it does not claim remote control.
- **Revocation.** Remove server revokes the node's certificate in the CRL, deletes its record, and asks the node (if reachable) to unenroll; a node that stays offline is rejected by CRL when it returns. Local panel and tunnels on that host keep working.
- **Create tunnel between two managed servers.** The panel asks Iran or abroad, a name and ports, then drives ADR-0050 on both nodes. The user never copies files or codes.

## Decisions (owner-delegated, 2026-11-17, coordinator)

- **D1. CA.** Reuse the ADR-0050 per-tunnel CA mechanism. A Fleet-local CA is added only if this ADR shows a concrete need that the per-tunnel CA cannot meet (open: Fleet reporting identity for panel-enrolled nodes, ADR-0045 `fleet-node` profile; to be argued in the Fleet transport PR). CA keys never leave the host that owns them.
- **D2. SSH client.** A pure Rust SSH library, no shelling out to `ssh` and no Go, with strict host-key verification. This replaces the OpenSSH-client plan above: the askpass/`memfd` mechanics are dropped, the secret stays in zeroized memory inside the enrollment unit, and the new dependency goes through `cargo deny`/audit review in the Path B PR. The unit sandbox, host-key confirmation and fixed step list stay as written.
- **D3. sudo.** No blanket sudo. Only the signed installer is run, through a narrow fixed command; the sudo password lives in memory only and is sent only to that command.
- **Review.** An independent security review of this ADR (Opus reviewer thread, PR #341) is required before Path B is enabled. Path A proceeds meanwhile.

## Staged plan (each a small PR, CI on the exact SHA, local/CI evidence until a LAB slot is granted)

1. This ADR and the review request.
2. Panel "Add server" page with Path A (code and one-line command), list of enrolled servers (pending, Live, Stale, Never), honest Unknown. Depends on ADR-0050 stage 4/5.
3. Fleet F1 transport (reporter and hub) with every ADR-0045 amendment tested.
4. Enrollment helper unit and SSH runner (Path B), disabled by default, tests against a throwaway OpenSSH server in a container: host key mismatch rejected, wrong password, secret never in logs or audit, rollback on injected failure, cancel, timeout.
5. Tunnel creation between two managed servers.
6. Two-host LAB acceptance in a coordinator-approved window only.

## Out of scope and fixed constraints

No firewall, route, sysctl or `CAP_NET_ADMIN` change on any host; the panel tells the user which port to open. No arbitrary remote commands. No stored credentials. No real hosts beyond the two LAB hosts, and only in an approved window. Nothing here is implemented, tested or LAB-verified.

## Amendments from the independent review (binding before Path B stage 4 starts)

Review: PR #341 comment 6960083527 (no Critical, 4 High, 6 Medium, 4 Low). D2 (pure Rust SSH library, no shelling out) is kept; findings written for the OpenSSH client are applied in their library form.

**High**
- **H1 secret path.** The agent heap holds the secret for the length of the request and zeroizes it. It reaches the enroll unit only over an fd: a socket-activated `baft-tunnel-enroll.socket` (`Accept=yes`) or `SCM_RIGHTS`; never a request file. A request file, if used, carries only target, user, port and pinned fingerprint.
- **H2 SSRF and spray.** Resolve the name once, show the IP, connect only to that IP. Deny private, CGNAT, ULA, multicast and reserved ranges (and the hub's own addresses) unless an explicit "LAN target" switch is on. Each run starts with `IPAddressDeny=any` and `IPAddressAllow=<resolved IP>/32` (systemd/BPF, no `CAP_NET_ADMIN`, ADR-0030). Port 22 or one explicit port. Uniform errors. Attempt budget: 3 per target per hour, 10 per day globally, with lockout.
- **H3 injection.** Host is an IP literal or LDH DNS name, user matches `^[a-z_][a-z0-9_-]{0,31}$`, port is a number. With a library there is no argv, config file, `ProxyCommand` or `%` expansion; no agent or port forwarding is ever requested, one password attempt per run, 10 s connect timeout. Remote commands are fixed strings with no operator input.
- **H4 host key.** Fetch the keys once, pin the confirmed key type and key in memory for the run, so a downgrade or change between confirmation and auth fails. The UI offers a copy-paste command for the provider's web console (`ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub`). An unconfirmed-by-console confirmation is labelled `hostkey=operator-asserted` in the UI and audit. Prefer Path A whenever the user already has a shell.

**Medium**
- **M1.** Library keys stay in zeroized memory; `MemorySwapMax=0`, `LimitCORE=0`, `PR_SET_DUMPABLE=0`, `mlock` on our buffers, `DynamicUser=yes`; the unit allows `AF_UNIX` for journald. Library-internal copies are documented as not guaranteed zeroized.
- **M2.** One exec channel per step; `sudo -S -p ''` receives exactly one line; payload and pairing code never share that stdin. First Preview: root or passwordless sudo only.
- **M3.** Snapshot target pre-state; refuse existing installs in Preview. Timeout and cancel still roll back (`ExecStopPost`, `TimeoutStopSec`). A lost connection reports Unknown with manual cleanup steps, never "left as found".
- **M4.** Upload into a root `mktemp -d` (0700), check sha256 after upload and before running; the real signature check is on the hub side plus the authenticated channel (a verifier shipped in the same upload adds nothing).
- **M5.** D1 conditions: the Fleet CA key lives only in the root helper, separate from any per-pair CA, name-constrained, `fleet-node` EKU only, validity at most 30 days, CRL; nodes pin the hub identity separately. This ADR-0014 trade-off is recorded as accepted for panel-enrolled nodes only.
- **M6.** Bearer token in a header (no cookie auth on this route), Origin and Host checks, panel token re-entered per run. The root helper and enroll unit re-check target guard, rate limit and single active run themselves.

**Low:** render remote banner/stderr as capped plain text, control and ANSI stripped, not verbatim in the audit (L1); Path A prefers `--pair -` with a hidden prompt over a code in shell history, and the Iran claim is corrected: the one-line installer downloads from GitHub, so it works on a host without internet only when the panel serves the bundle (L2); Remove server also deletes the pinned host key and aborts an open code, and only the hub enforces the CRL (L3); one global active run, showing who holds the slot, plus a per-operator budget (L4).

**Gates.** Path A ships as Preview once the ADR-0050 review fixes (listener holding the CA key, deadline, approval binding) are merged; Path B never opens the pairing listener implicitly. Path B ships only default-off, as Preview, after stage 4 implements H1 to H4 and M1 to M3 with container tests for: secret absent from argv, env, files, journal and audit (canary-password grep); host-key mismatch and downgrade rejected; injection inputs rejected; private-range target refused by `IPAddressAllow`; rollback after injected failure, timeout and cancel; existing install untouched. No LAB run and no UI switch before that.

## Addendum 2026-11-18: D1 resolved, Fleet CA for `fleet-node` certificates

The concrete need D1 asked for: the per-tunnel CA (ADR-0050) signs exactly two node ids fixed at creation and is name constrained to their two addresses, so it cannot enroll a third, panel-added server. Reusing it would mean widening its constraints, which weakens the tunnel trust. A separate Fleet CA is therefore added, under the M5 conditions:

- `src/fleet_ca.rs` (openssl through the existing `pair_ca` helpers, no new dependency). The key is created 0600 in a 0700 directory and is meant to live only in the root helper, separate from any per-pair CA.
- Name constrained to URI names under `.fleet.baft.invalid`; a fleet-node certificate carries `baft-fleet://<node-id>.fleet.baft.invalid` and the hub reads the node id from it (F-5). Node ids are lowercase letters, digits and hyphens, at most 63 characters.
- One BAFT EKU only (`fleet::EKU_FLEET_NODE`) plus standard `clientAuth` (F-4); no `serverAuth`.
- Leaf validity 30 days, CRL validity 30 days, revocation supported. Same CSR gate as the tunnel CA (shared `pair_ca::check_csr`: one PEM request, valid signature, CN equals node id, EC P-256).
- `fleet_ca::sign_hub` issues the `fleet-hub` server certificate (F-2): `serverAuth` plus the fleet-hub EKU only, URI `baft-fleet-hub://<name>.fleet.baft.invalid` and the DNS or IP the members dial. Not wired yet: the root helper operation and the enrollment flow (F-7) come in later slices. Nothing in the agent calls this module today.

Evidence: container-free unit tests with the real `openssl` (profile, EKU exclusivity, name, 30-day bound, chain verify, bad CN, wrong curve, invalid ids, revocation). Not LAB-verified.

## Addendum: enrollment state machine (F-7)

`src/fleet_enroll.rs` holds the hub-side enrollment rules, pure and unwired: one live offer per node id (max 16), 15 minute TTL, one-time token stored only as a SHA-256 hash, three wrong-token attempts close the offer, a valid token is consumed by the first submit, the CSR must carry a proof bound to the offer nonce and its fingerprint, and a certificate is signed only after the operator types the CSR fingerprint the new server printed. Signing is injected (`fleet_ca::sign` through the root helper), so the module holds no keys. The join listener, the Add-server API and the helper operation are the next slices.

## Addendum: root Fleet helper (M5)

`src/fleet_helper.rs` plus `baft-tunnel-control-fleet@.path/.service` follow the pairing-helper pattern (ADR-0050): the agent drops `NAME.fleet` (node id and CSR only), the root unit runs `baft-tunnel fleet helper --instance NAME` with no network and the usual hardening, signs with `fleet_ca::sign` from `/etc/baft-tunnel/fleet-ca` and writes `NAME.fleet-result` (public certificate only). The helper never creates the CA; a missing CA fails closed. `POST /v1/fleet/enroll/confirm` now signs through this helper (8 s wait) and answers 503 when there is no instance or helper. Creating the Fleet CA (an operator command) and the node join over the hub listener are the next slices.


## Addendum 2026-11-19: Fleet Join TLS bootstrap and certificate delivery

**Owner decision:** The owner approved the architecture in this addendum. This resolves the transport and bootstrap direction for Path A / panel-driven Fleet enrollment. It does not authorize implementation on real hosts, opening ports, firewall or route changes, deployment, or release. Existing independent-review and staged acceptance gates remain in force.

### Decision: separate Join and Report TLS channels

Fleet enrollment uses two distinct TLS endpoints with different authentication requirements. They MUST NOT be combined by making the existing report listener accept optional client certificates or by selecting authentication policy after the TLS handshake.

1. **Fleet Join listener (temporary, one-shot):** TLS 1.3 with server authentication only. A not-yet-enrolled member has no Fleet client certificate, so this endpoint intentionally does not request one. It is a Hub-side listener/session used only for a live enrollment offer; the member always initiates the connection outbound. The listener/session is bounded by the offer TTL, connection deadline, message limits, concurrency limits and one-time token semantics.
2. **Fleet Report listener (persistent):** TLS 1.3 mandatory mTLS, unchanged in security posture. It continues to require a valid client certificate chaining to the Fleet CA, CRL acceptance and the expected `fleet-node` identity. Missing, expired or revoked client certificates fail during TLS authentication. Join traffic MUST NOT be routed through this listener.

The Join transport may reuse low-level, reviewed transport techniques from the one-shot pairing implementation (TLS 1.3, bounded framing, deadlines, one-shot session and explicit acknowledgement). It MUST define Fleet-specific message types and protocol semantics; tunnel pairing codes, identities and state MUST NOT be reused as Fleet enrollment objects.

### Hub identity bootstrap and trust

Before the Join listener is enabled, the Hub must have a provisioned `fleet-hub` certificate and private key. The existing `fleet_ca::sign_hub` capability must be wired through a narrowly scoped root-helper operation; the Fleet CA private key remains only in the root helper and is never sent to the agent, browser or joining member.

The operator-facing Join offer/code carries, at minimum:

- Hub address and port, `hub_id` and `hub_name`;
- the exact SHA-256 fingerprint of the Hub leaf certificate;
- offer ID, target node ID, random one-time bearer token, nonce, expiry and protocol version.

The member MUST authenticate the Hub against the exact pinned leaf fingerprint before sending its CSR or accepting trust material. It also validates the Hub certificate's validity period, `fleet-hub` EKU, URI SAN bound to `hub_id`, and DNS/IP SAN bound to the configured `hub_name`/dial target. A name match without the exact pin is insufficient. After the pinned TLS session is established, the Hub may send the public Fleet CA certificate and current CRL/trust bundle. No CA private key is ever transmitted.

### Member key generation, submission and approval

- The joining member generates its private key locally and creates its CSR locally. The private key MUST never leave the member.
- The Join endpoint accepts no client certificate by design, because enrollment has not yet completed. Authorization is instead provided by a random, single-use bearer token of at least 128 bits, stored server-side only as a cryptographic hash. The token is not placed in a URL, browser history, logs or audit records.
- Preserve the offer's fail-closed properties: one live offer per node ID, 15-minute TTL, one-time consumption, bounded wrong-token attempts, rate/concurrency limits, strict frame and field limits, and explicit connection/read/write deadlines. Restart or lost-session behavior must not silently restore a consumed token or mark an uncertain enrollment complete.
- The Hub validates the CSR signature (the cryptographic proof of possession), allowed key algorithm/curve, subject and node identity. It parses the CSR itself and computes the canonical public-key fingerprint; it MUST NOT trust a client-supplied fingerprint as authoritative.
- The operator approves the exact fingerprint computed from the validated CSR and shown by the joining member. The current `fleet_enroll::proof()` construction (plain SHA-256 over public nonce and fingerprint) is NOT proof of private-key possession and MUST NOT be treated as such. The CSR signature is the key-possession proof. If a request-binding proof is retained, it must be domain-separated and keyed by the offer secret; it is additional binding, not a replacement for token authorization or CSR validation.
- The root helper signs only the exact validated and operator-approved CSR. The agent and panel never receive the Fleet CA private key.

### Certificate delivery, first report and enrollment state

Keep the one-shot Join TLS session available while the operator reviews the request. After approval and root-helper signing, deliver to the joining member over that same pinned TLS session only the issued member certificate and required public trust material (CA certificate, CRL and Hub identity). The browser receives status and public metadata only; it never receives the member private key or CA secret.

The member installs the issued certificate and then establishes a fresh connection to the existing mandatory-mTLS Report listener. It sends its initial authenticated Fleet report and returns the Join acknowledgement. The Hub marks the node **Enrolled** only after the first report is accepted under the `fleet-node` identity and the acknowledgement is received.

If the Join connection is lost before delivery, after signing, or before acknowledgement, the state is **Issued/Unconfirmed** or **Unknown**, as appropriate; it is never reported as Enrolled. Recovery and retry behavior must be explicit and fail closed. A consumed one-shot offer may require a new operator-issued offer rather than replaying its token or silently reusing an uncertain session.

### Required negative and failure tests

Before the Join path is enabled, automated tests must cover at least:

- wrong Hub pin, Hub ID or Hub name; expired/not-yet-valid Hub certificate; wrong EKU, URI SAN or DNS/IP SAN;
- wrong, expired, replayed or exhausted token; wrong node ID; duplicate and concurrent submissions; rate and concurrency limits;
- malformed CSR, invalid CSR signature, disallowed key algorithm/curve, subject mismatch and tampered client-supplied fingerprint;
- oversized frames/fields, slow client, read/write deadline expiry and abrupt disconnects;
- report listener rejects missing client certificate, invalid chain, expired certificate and revoked certificate/CRL cases;
- disconnect before signing, after signing/delivery and before acknowledgement never produces an Enrolled state; recovery follows the documented fail-closed path;
- member private key and Fleet CA private key never appear in browser/API responses, process arguments, environment, files outside their designated protected locations, logs or audit records.

The two-host end-to-end acceptance must demonstrate the complete sequence: operator creates offer, member verifies the pinned Hub and submits a locally generated CSR, operator approves the exact fingerprint, member receives its certificate and public trust bundle, member connects to the mandatory-mTLS Report listener, the Hub accepts the first report, and the Join acknowledgement completes enrollment. Unit or container tests alone do not count as two-host LAB evidence.

### Operational constraints and implementation order

- No inbound listener is added to the member. Only the Hub-side Join listener is temporary; the existing Report listener remains persistent and mandatory-mTLS.
- No automatic firewall, route, sysctl or host-network changes are permitted. No `CAP_NET_ADMIN` is permitted. The operator is told which Hub port must be made reachable and configures it separately.
- Implement in gated slices: (1) provision and verify the Hub certificate through the root helper; (2) implement the isolated Join TLS transport and Fleet-specific bounded protocol; (3) bind it to the enrollment state machine and operator approval; (4) deliver the certificate and public trust bundle; (5) require first accepted mTLS report plus acknowledgement before Enrolled; (6) complete negative tests and two-host LAB acceptance.
- Until all required implementation, independent review and LAB gates pass, the panel and API MUST NOT advertise this Join flow as available. Existing report mTLS behavior remains unchanged.

This addendum supersedes any earlier interpretation of the F-7 `proof` field as standalone proof of private-key possession. It does not change the separate Path B SSH review gate or authorize enabling SSH enrollment.
