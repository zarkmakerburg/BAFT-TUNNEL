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
