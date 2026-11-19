# ADR-0051: add and configure another server from the panel

Status: **Proposed** (2026-11-17). Security-sensitive: SSH enrollment (Path B) remains disabled until its implementation, security self-review, negative tests and staged acceptance gates pass. Per the owner's 2026-11-19 decision, independent external review is not a project prerequisite. Builds on ADR-0045 (Fleet, accepted with conditions), ADR-0050 (automatic pairing, Proposed in PR #338), ADR-0023 (control helper), ADR-0033 (panel HTTPS), ADR-0034 (offline signature verification), ADR-0030 (no `CAP_NET_ADMIN`).

## Goal

The owner wants, as in the BAFT project: in the panel, *add a server* (address and credentials), the panel installs and pairs the agent, and the user then configures tunnels on it from the same panel. Tunnel creation between two panel-managed servers is a few choices (Iran or abroad, name, ports), per ADR-0050.

## Decision

Two enrollment paths, shipped in this order. Both end the same way: the remote server is a **paired, enrolled node** and no credential is kept.

### Path A: pasted one-line command (first, no credentials in the panel)

1. The panel asks for a display name and creates an enrollment code (ADR-0050 style: single use, 15 minutes, bearer secret, never logged).
2. It shows one command: the signed quick installer with `--pair CODE` (ADR-0050 as implemented in PR #338: `quick-install.sh --pair CODE`; the first server uses `--offer NAME --service-port PORT`). The user pastes it on the new server (their own SSH session). The command pins a release tag; the panel may show it only for a release that actually contains pairing, so until such a release exists the panel shows the already-installed form `sudo baft-tunnel pair accept CODE` and says the one-line install needs a newer release. The installer verifies the release signature offline with the pinned root (ADR-0034) and registers the agent in setup mode.
3. The new agent dials back outbound to the panel host (pairing listener, ADR-0050; Fleet reporter after ADR-0045 F1), proves the code and sends its CSR. The operator approves on the panel: the approval step shows node id, source address, version and the short `verify` key fingerprint, which the user compares with the new server's screen; approval requires the one-time `nonce` from `GET /v1/pair/status` (ADR-0050 revision). Pairing refuses a config directory that already holds a configuration. Done.

This needs no new attack surface beyond ADR-0050's listener. The one-line installer downloads from GitHub, so on a host without internet it works only if the panel serves the bundle (see L2 below).

### Path B: SSH enrollment (second, off by default, behind its own acceptance gates)

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
- **Owner decision (2026-11-19).** Independent external review is not required before Path B or any other project stage. Path B remains disabled until its own implementation-specific security self-review, negative tests and staged acceptance gates pass. The prior PR #341 review findings below remain useful hardening constraints, not an approval prerequisite.

## Staged plan (each a small PR, CI on the exact SHA, local/CI evidence until a LAB slot is granted)

1. This ADR and the review request.
2. Panel "Add server" page with Path A (code and one-line command), list of enrolled servers (pending, Live, Stale, Never), honest Unknown. Depends on ADR-0050 stage 4/5.
3. Fleet F1 transport (reporter and hub) with every ADR-0045 amendment tested.
4. Enrollment helper unit and SSH runner (Path B), disabled by default, tests against a throwaway OpenSSH server in a container: host key mismatch rejected, wrong password, secret never in logs or audit, rollback on injected failure, cancel, timeout.
5. Tunnel creation between two managed servers.
6. Two-host LAB acceptance in a coordinator-approved window only.

## Out of scope and fixed constraints

No firewall, route, sysctl or `CAP_NET_ADMIN` change on any host; the panel tells the user which port to open. No arbitrary remote commands. No stored operator credentials. No real hosts beyond the two LAB hosts, and only in an approved window. Implementation and CI evidence are recorded in the subsequent addenda and PRs; no two-host LAB acceptance is claimed by this ADR.

## Security hardening amendments (from prior review; retained as design constraints)

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

- `src/fleet_ca.rs` (openssl through the existing `pair_ca` helpers, no new dependency). The CA key is root-owned 0600 in a non-listable 0711 directory; only the public CA certificate and CRL are 0644 so the unprivileged Agent can verify peers. The signing database/config and `newcerts` remain root-only. The key lives only in the root helper, separate from any per-pair CA.
- Name constrained to URI names under `.fleet.baft.invalid`; a fleet-node certificate carries `baft-fleet://<node-id>.fleet.baft.invalid` and the hub reads the node id from it (F-5). Node ids are lowercase letters, digits and hyphens, at most 63 characters.
- One BAFT EKU only (`fleet::EKU_FLEET_NODE`) plus standard `clientAuth` (F-4); no `serverAuth`.
- Leaf validity 30 days, CRL validity 30 days, revocation supported. Same CSR gate as the tunnel CA (shared `pair_ca::check_csr`: one PEM request, valid signature, CN equals node id, EC P-256).
- `fleet_ca::sign_hub` issues the `fleet-hub` server certificate (F-2): `serverAuth` plus the fleet-hub EKU only, URI `baft-fleet-hub://<name>.fleet.baft.invalid` and the DNS or IP the members dial. The Slice 1 implementation wires this through the root Fleet helper; the CA private key stays in the helper and the Hub key is delivered only through the instance-scoped systemd credential. The Join listener and enrollment flow (F-7) remain later slices. The unprivileged Agent never calls the CA signing module.

Evidence: container-free unit tests with the real `openssl` (profile, EKU exclusivity, name, 30-day bound, chain verify, bad CN, wrong curve, invalid ids, revocation). Not LAB-verified.

## Addendum: enrollment state machine (F-7)

`src/fleet_enroll.rs` holds the hub-side enrollment rules, pure and unwired: one live offer per node id (max 16), 15 minute TTL, one-time token stored only as a SHA-256 hash, three wrong-token attempts close the offer, a valid token is consumed by the first submit, the CSR must carry a proof bound to the offer nonce and its fingerprint, and a certificate is signed only after the operator types the CSR fingerprint the new server printed. Signing is injected (`fleet_ca::sign` through the root helper), so the module holds no keys. The join listener, the Add-server API and the helper operation are the next slices.

## Addendum: root Fleet helper (M5)

`src/fleet_helper.rs` plus `baft-tunnel-control-fleet@.path/.service` follow the pairing-helper pattern (ADR-0050): the agent drops `NAME.fleet` (node id and CSR only), the root unit runs `baft-tunnel fleet helper --instance NAME` with no network and the usual hardening, signs with `fleet_ca::sign` from `/etc/baft-tunnel/fleet-ca` and writes `NAME.fleet-result` (public certificate only). The helper never creates the CA; a missing CA fails closed. `POST /v1/fleet/enroll/confirm` now signs through this helper (8 s wait) and answers 503 when there is no instance or helper. The remaining Fleet slice is the node join over the hub listener and its end-to-end acceptance.

The operator initialization slice exposes `sudo baft-tunnel fleet ca init --confirm` (optional `--name`) for the registered LAB only. It is root-only, requires explicit confirmation, refuses any pre-existing CA path, requires `/etc/baft-tunnel` to be a real root-owned non-group/world-writable directory, and verifies the resulting CA directory, private key and public trust-file modes before reporting success. It prints only public file paths; it never prints or exports `ca.key`. The command does not enroll nodes or open a listener. Any non-LAB deployment still requires the offline CA ceremony in O9.

The Hub identity slice provisions a per-instance `fleet-hub` key inside the root helper, publishes only its server certificate, writes a root-owned Fleet config and a narrowly scoped Agent systemd drop-in, and delivers the key with `LoadCredential=fleet-hub-key`. The drop-in adds only the credential; the Agent auto-loads the root-managed per-instance config by its validated instance name, preserving the existing systemd `ExecStart` and any operator overrides. The operator explicitly activates it by restarting that Agent instance. The Fleet panel reports configured, restart-required and active states separately. No firewall, route, sysctl or `CAP_NET_ADMIN` changes are made. Real systemd/credential acceptance is required before this slice is considered complete.
