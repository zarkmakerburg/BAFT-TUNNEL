# S5 whole-`main` security audit — 2026-11-24

**Status: S5 remains OPEN.** This is an internal technical source review, not an independent
human security review. It does not close S5 and must not be presented as one. Closure still
requires a reviewer outside the authors and implementers of the reviewed changes.

## 1. Scope and baseline

| Item | Value |
|---|---|
| Baseline | `main` = `a3637aa1f2fb11b8cea8bba044b765362210cb48` (merge of PR #430) |
| Code delta since baseline | None: `a0cb55f` (merge of #431) is docs-only |
| Exact-SHA CI at baseline | 10/10 check runs SUCCESS ([run 45278026658](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/45278026658), [run 45278026495](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/45278026495)) |
| Date | 2026-11-24 |
| Author | Codebuff agent (contributed to code under review) — **not independent** |
| Two-host LAB | Not used; suspended by owner order. No host was touched |
| `CAP_NET_ADMIN` | Not used, not requested, not granted anywhere |
| Deploy / release / tag / Production | None |

The owner paused the external review on 2026-11-11 and re-authorized it on 2026-11-24; the
suspension history is preserved in `docs/REMAINING_WORK_PLAN.md` (O6) and
`docs/PRODUCT_ROADMAP.md`.

## 2. Method and evidence labels

Read-only source review of the current tree plus the repository's own negative tests. Nothing
was built or executed: this workspace has no Rust toolchain and no systemd, so every statement
below is source-level unless labelled otherwise.

- **[CODE]** read in the source at the baseline.
- **[TEST]** a repository test that asserts the behaviour (read, not run).
- **[CI]** exact-SHA CI evidence recorded on GitHub.
- **[INF]** inferred from the code, not executed.

## 3. Coverage

| Area | Files reviewed | Depth |
|---|---|---|
| systemd units and hardening | `systemd/baft-tunnel@.service`, `systemd/agent/*`, `systemd/control/*` (all path, socket and oneshot units) | Full read |
| Privileged manage helper | `src/tunnel_helper.rs` (auth, socket activation, request handling, revert) | Partial: ~470–860; the file is 1377 lines |
| Unix control socket | `src/control_socket.rs` (bind, path validation, SO_PEERCRED, request framing) | Partial: 150–410 |
| Agent HTTP API | `src/agent.rs` (read/limit/host/auth/router, setup mode, token load, constant-time compare) | Partial: ~400–1015, 4425–4460; the file is 7027 lines |
| TLS / mTLS / CA / CRL | `src/tls.rs` (client verifier, CRL expiry enforcement, TLS 1.3, reload semantics) | Partial: 300–430 |
| SSRF guard for probes/location | `src/target_guard.rs` | Full read |
| Installer / uninstaller | `scripts/uninstall.sh` (full), `scripts/install-agent-control.sh` (1–120), nginx gate in `scripts/panel-https.sh` (330–440) | Partial |
| Apply receipts | `src/apply.rs` (persistence path, verification checks) | Partial |

Not reviewed to audit depth (see §7): `src/tunnel.rs` (3382 lines), `src/handshake.rs`,
`src/pki_helper.rs`, `src/setup.rs`, `src/pair_*.rs`, `src/fleet_*.rs`, `src/baft_engine.rs`,
`scripts/install.sh`, `scripts/update.sh`, `scripts/backup.sh`, `scripts/restore.sh`,
`panel/`, `panel-react/`, `lab/`, `fuzz/`, `.github/workflows/release.yml`.

## 4. Controls verified in this pass

1. **Per-instance socket activation is authenticated before any client byte is read.**
   `serve()` checks `LISTEN_PID`, `LISTEN_FDS == 1`, the descriptor name `manage`, the socket's
   actual path and `SO_ACCEPTCONN`, then reads `SO_PEERCRED` and the peer cgroup before reading
   the request [CODE `tunnel_helper.rs:606–660, 750–800`].
2. **UID plus exact cgroup, not UID alone.** `authorize_peer` requires the dedicated
   `baft-tunnel-agent` account and the exact
   `/system.slice/baft-tunnel-agent@<instance>.service` cgroup path, and the cgroup is re-read
   after the request to catch PID recycling [CODE `tunnel_helper.rs:505–545, 770–790`].
   Adversarial tests exist for the cgroup match [TEST `peer_authorization_requires_the_exact_systemd_instance_cgroup`].
3. **Per-instance manage access needs root to grant.** The socket is root:root `0600` by default;
   `install-agent-control.sh` adds a per-instance `btm-<hash>` group, verifies that the group is
   empty and is nobody's primary group, and grants it to exactly one agent unit
   [CODE `install-agent-control.sh:60–100`]; `uninstall.sh` refuses to delete a non-empty group
   and removes the drop-ins only after confirming units are inactive.
4. **No ambient privilege in the tunnel service.** `CapabilityBoundingSet=` empty,
   `NoNewPrivileges=yes`, `ProtectSystem=strict`, `ProtectProc=invisible`, `PrivateDevices`,
   `SystemCallFilter=@system-service`, `InaccessiblePaths` for the agent state and token
   [CODE `systemd/baft-tunnel@.service`].
5. **Agent API requires a Bearer token from a root-only file.** `read_token` refuses a token file
   with any group/other permission and a token shorter than 32 characters
   [CODE `agent.rs:71–85`]; comparison is constant-time apart from a length check
   [CODE `agent.rs:4425–4431`].
6. **DNS-rebinding guard.** Requests whose `Host` is not `127.0.0.1`, `localhost` or `::1` are
   answered 421 before routing; tests cover `127.0.0.1.evil.example`, `localhost.evil.example`
   and a rebinding name [CODE `agent.rs:428–450`; TEST `agent.rs:4580–4625`]. The nginx profile
   also rejects a mismatched `$host` and rewrites `Host` to the loopback upstream
   [CODE `panel-https.sh:395–414`].
7. **Request and body size limits.** Head capped by `MAX_REQUEST_BYTES`, body by
   `MAX_BODY_BYTES` with 431/413 answers, whole request under one I/O timeout, bounded connection
   slots [CODE `agent.rs:412–505`].
8. **mTLS is mandatory for tunnel traffic, with revocation enforced and early data disabled.**
   `WebPkiClientVerifier` with `with_crls(...).enforce_revocation_expiration()`, TLS 1.3 only,
   `max_early_data_size = 0` [CODE `tls.rs:369–390`].
9. **TLS material replacement fails closed.** A changed CA/CRL/cert/key rebuilds the acceptor;
   if the new material does not load, new connections are refused rather than served with the
   old file set [CODE `tls.rs:306–365`].
10. **Probe/location targets are constrained to globally routable unicast.** Loopback, private,
    link-local (including the cloud metadata address), CGNAT, benchmark, documentation,
    multicast, ULA and NAT64 ranges are refused, with a per-target and global hourly budget
    [CODE `target_guard.rs:1–110`].
11. **Uninstaller is fail-closed on its privileged inventory.** It enumerates instance and
    control units, refuses to continue when enumeration fails, requires `--stop` when instances
    exist, verifies `ActiveState=inactive` before removing manage access, and refuses to delete
    groups that are non-empty or whose lookup errored [CODE `scripts/uninstall.sh:40–110`].
12. **Control helpers are fixed-command oneshots** with narrow capabilities
    (`CAP_CHOWN CAP_FOWNER CAP_DAC_OVERRIDE`, plus `CAP_SETUID/SETGID` only for the pairing
    helper's network child), `PrivateNetwork` where no network is needed, and request removal in
    `ExecStopPost` [CODE `systemd/control/*`].

## 5. Findings

Severity uses Critical / High / Medium / Low / Info. No Critical or High finding was identified
in the reviewed surface. These are source-review findings, not exploit results.

### S5-01 — Agent HTTP parser accepts ambiguous message framing — **Low**

**Evidence [CODE].** `read_request` (`src/agent.rs:475–505`) reads only `Content-Length`, takes
the first matching header via `header()` (`agent.rs:700–710`), and never inspects
`Transfer-Encoding` or duplicate `Content-Length` headers. A `Transfer-Encoding: chunked`
request without `Content-Length` is therefore treated as a zero-length body, and duplicate
`Content-Length` headers resolve to the first value.

**Impact.** `read_request` runs before authentication, so any loopback client — including an
unauthenticated one hitting `/v1/health`, a static asset or a 401 route — reaches the parser.
What needs the Bearer token is any body-dependent state-changing route, so the practical risk is
a framing/behaviour mismatch rather than an authentication bypass: an ambiguous request is
answered 401/404/422 or handled with an empty body, and no path was found where framing lets a
request skip `route`'s token check. In front of nginx the ambiguity is currently contained:
`proxy_http_version 1.1` with default request buffering makes nginx de-chunk and send one
`Content-Length`, and nginx rejects conflicting framing before it reaches the agent.

**Reproduction (source-level).** Unauthenticated, send
`GET /v1/health HTTP/1.1\r\nHost: localhost\r\nTransfer-Encoding: chunked\r\n\r\n`
and observe the parser ignores the encoding; with a valid token, send the same on
`POST /v1/setup/plan` and observe the route receives an empty body, or send two `Content-Length`
headers with different values and observe the first one is used.

**Suggested fix.** In `read_request`, answer 400 when any `Transfer-Encoding` is present, when
`Content-Length` appears more than once, or when the declared length and the consumed bytes
differ; reject `Content-Length` values with signs or surrounding whitespace. Add tests for each
case next to the existing host and size tests.

### S5-02 — Manage access needs the group, but the group alone does not authorize — **Info**

**Evidence [CODE].** `install-agent-control.sh` grants the per-instance `btm-<hash>` group to the
agent unit and sets `SocketGroup=<group>, SocketMode=0660`; `authorize_peer`
(`tunnel_helper.rs:511–541`) independently requires the peer's `SO_PEERCRED` UID **and primary
GID** to equal the `baft-tunnel-agent` passwd entry **and** the peer's cgroup to be the exact
`/system.slice/baft-tunnel-agent@<instance>.service`. Group membership is therefore only the
socket access gate; it is not part of the authenticated identity.

**Impact.** Adding a different-UID human or service to `btm-<hash>` requires root and lets that
process connect and be refused by `authorize_peer`; it does **not** grant permission to invoke
tunnel removal or editing, because the UID/GID and exact-cgroup checks still fail. The residual
risk is small: the extra process can occupy the single-connection helper long enough to consume
a socket activation (bounded by `StartLimitBurst=60`/10 s and `TimeoutStartSec=50`), and the
invariant that keeps the group effectively exclusive is enforced by convention plus
`uninstall.sh`'s refusal to delete a non-empty group.

**Suggested fix.** Document the invariant next to `ensure_manage_access` and in
`docs/SECURITY_ASSESSMENT.md` ("`btm-*` groups must stay empty; membership is manage access"),
and optionally verify supplementary groups with `SO_PEERGROUPS` so the helper can require the
instance group explicitly instead of relying on the socket's file mode.

### S5-03 — `/v1/health` and panel assets are served without authentication — **Info**

**Evidence [CODE].** `route` answers `/v1/health` before the token check (`agent.rs:712–720`) and
`static_asset` is served before `route` (`agent.rs:452–466`). The HTTPS profile's secret-path
cookie (`panel-https.sh:333–414`) is a discoverability control, not authentication; the API
itself stays Bearer-gated.

**Impact.** Informational: a local unprivileged user can confirm the agent is running and read
the static panel files. No state, secret or token is exposed.

**Suggested fix.** None required; keep it explicit in the security documentation so it is not
mistaken for an authenticated surface.

### S5-04 — TLS reload detection uses metadata, not content — **Info**

**Evidence [CODE].** `ReloadingAcceptor::current` compares `(ino, len, mtime)` per file
(`tls.rs:322–360`). A same-size replacement that preserves the modification time would not be
reloaded.

**Impact.** Informational: the CA, CRL, certificate and key are root-managed (`/etc/baft-tunnel`,
root-owned or service-user key), so an actor able to preserve those attributes is already root.

**Suggested fix.** Optional: hash the four files (SHA-256) on change detection, or compare
`ctime` as well.

### S5-05 — Live TLS sessions are not re-evaluated after a CRL change — **Info (accepted by design)**

**Evidence [CODE/DOC].** The comment on `ReloadingAcceptor` states that established sessions are
not re-checked and that a revocation that must cut off live streams still needs a restart; S8 in
`docs/REMAINING_WORK_PLAN.md` records the same.

**Impact.** Informational, documented: a revoked peer keeps an already-established session until
the unit restarts.

**Suggested fix.** Keep as accepted residual; if live cut-off is required, restart on revocation
(default `apply-crl.sh` behaviour) and say so in the operator guide.

## 6. Tests reviewed (read, not run)

| Test | What it asserts |
|---|---|
| `tunnel_helper.rs::peer_authorization_requires_the_exact_systemd_instance_cgroup` | A peer from another instance's cgroup is refused |
| `agent.rs::allowed_host` cases | Rebinding-style `Host` values are refused (421) |
| `agent.rs::constant_time_eq` usage / token load tests | Token file permission and length rules |
| `control_socket.rs::peer_credentials_require_exact_uid_not_group_membership` | UID alone, primary GID compared, group membership is not identity |
| `pki_helper.rs::refuses_symlinked_receipt_root_without_changing_target_permissions`, `agent_receipt_reader_rejects_symlinks_and_non_root_permissions` | Symlink and permission handling on receipt paths |
| `scripts/test-panel-pairing.sh` | Symlinked and oversized setup requests refused; refused requests leave `/etc/baft-tunnel` unchanged; planted result symlink not written through |
| `scripts/test-uninstall-systemd-inventory.sh` | Uninstaller fails closed when `systemctl` inventory errors |

None of these were executed in this workspace (no Rust toolchain, no systemd). CI on `a3637aa`
did run the suite and was green, but that is a separate evidence class and is not a substitute
for executing the negative tests here.

## 7. Limitations

- **No independent review.** The author contributed to code under review. S5 stays OPEN.
- **Partial file coverage.** The large modules listed in §3 were not read end to end; findings in
  unread regions are not excluded.
- **No execution.** No build, no unit/integration test run, no dynamic exploitation, no
  `cargo audit` (the toolchain is absent and dependencies were not installed).
- **No LAB.** The two-host Iran/Turkey environment was not used, by owner order. Instance
  isolation was therefore not re-validated on real hosts in this pass.
- **No dependency or supply-chain review** beyond noting that `Cargo.lock` is committed; advisories,
  release signing (`release.yml`) and the separate BAFT engine binary (ADR-0019) are out of scope
  here.
- **No panel JavaScript audit** beyond CSP and static serving.

## 8. Result

- Scope covered: systemd units, privileged helpers, Unix sockets, `SO_PEERCRED`, cgroup binding,
  agent HTTP API, TLS/mTLS and CA/CRL handling, installer/uninstaller paths, file and permission
  handling, error paths and trust boundaries.
- Findings: 0 Critical, 0 High, 0 Medium, 1 Low (S5-01), 4 Info (S5-02, S5-03, S5-04, S5-05).
- S5 status: **OPEN**. An independent reviewer outside the authors and implementers of the
  reviewed changes is still required; this report is input to that review, not its replacement.
- No deploy, release, tag, Production change or LAB run was performed.
