# ADR-0051: add and configure another server from the panel

Status: **Proposed** (2026-11-17). Security-sensitive: SSH enrollment must not be enabled before the independent review this ADR requests. Builds on ADR-0045 (Fleet, accepted with conditions), ADR-0050 (automatic pairing, Proposed in PR #338), ADR-0023 (control helper), ADR-0033 (panel HTTPS), ADR-0034 (offline signature verification), ADR-0030 (no `CAP_NET_ADMIN`).

## Goal

The owner wants, as in the BAFT project: in the panel, *add a server* (address and credentials), the panel installs and pairs the agent, and the user then configures tunnels on it from the same panel. Tunnel creation between two panel-managed servers is a few choices (Iran or abroad, name, ports), per ADR-0050.

## Decision

Two enrollment paths, shipped in this order. Both end the same way: the remote server is a **paired, enrolled node** and no credential is kept.

### Path A: pasted one-line command (first, no credentials in the panel)

1. The panel asks for a display name and creates an enrollment code (ADR-0050 style: single use, 15 minutes, bearer secret, never logged).
2. It shows one command: the signed quick installer with `--join CODE`. The user pastes it on the new server (their own SSH session). The installer verifies the release signature offline with the pinned root (ADR-0034) and registers the agent in setup mode.
3. The new agent dials back outbound to the panel host (pairing listener, ADR-0050; Fleet reporter after ADR-0045 F1), proves the code and sends its CSR. The operator approves on the panel: the approval step shows node id, source address, version and the short `verify` key fingerprint, which the user compares with the new server's screen; approval requires the one-time `nonce` from `GET /v1/pair/status` (ADR-0050 revision). Pairing refuses a config directory that already holds a configuration. Done.

This needs no new attack surface beyond ADR-0050's listener and works from Iran hosts that cannot reach the internet only if the panel host is reachable by them.

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
