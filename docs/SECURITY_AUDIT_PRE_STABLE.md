# Pre-Stable security audit (0.2.0 gate)

- Audited commit: `7011e3bd60ed97b4a94f04c02d8d52443511b212` (main, 2026-11-15).
- Baseline: the 2026-11-09 assessment ([SECURITY_ASSESSMENT.md](SECURITY_ASSESSMENT.md)) and the PR #199
  privilege-boundary review (main `dc0e947`, L-01..L-04, SA-11).
- Scope: everything changed in `dc0e947..7011e3b` (B4 BAFT Direct guided setup #201, B5 persisted
  history #212, B6 panel a11y #211, S4 fuzz #213, prerelease tag/LAB workflows #202..#207), re-checked
  against the agent API, root PKI/setup helpers, engine store, install/uninstall, panel and release
  signing.
- Method: manual source review, targeted by component (see Coverage). No LAB run, no host touched.
- Evidence level: source review + CI on the Ubuntu runner only (not VM, LAB or Production). The only
  dynamic test added or run by this audit is the A-01 unit test; everything else is read-only.

## Result

Within the coverage below: no Critical, High or Medium findings, one Low fixed (A-01), three Info.
This is not a whole-system clean result. Components marked "not audited" were not re-examined
since the 2026-11-09 assessment and the PR #199 review; they must not be counted as clean for the
Stable gate on the strength of this document.

## Coverage

| Component | What was read | Depth | Dynamic |
|-----------|---------------|-------|---------|
| B4 BAFT Direct setup | `setup.rs` diff `dc0e947..7011e3b` (plan/init/join, `tls_material`, bundle v2, `uses_certificates`), `baft_engine.rs` (`pinned_sha256`, `certificate_summary`, render peer identities/pin, `resolve_binary`, `run`), `engine_store::resolve_for_launch`/`verify_release`/`install`, `config.rs` baft validation, `capabilities.rs` diff | Full for changed code | No (CI only) |
| Root setup helper | `setup_helper.rs` lines 1-250 (request parsing, instance/dir binding, receipts, result write), `setup::adopt_for_service` | Full | No |
| Root PKI helper | `pki_helper.rs` diff (identity profile), `parse_request`, `run_at`, `run_openssl`; read paths use `O_NOFOLLOW` (checked by grep) | Changed code full; `install_certificate_at`/`create_csr_at` bodies only where the diff touches them | No |
| update.sh + control-dir migration | `scripts/update.sh` (full), `scripts/migrate-agent-control-dir.sh` (full) | Full | No |
| Agent API auth / pairing | `agent.rs`: `parse_listen`, `read_token`, `serve`, `allowed_host`, `handle`, `read_request`, `route` auth gate, `constant_time_eq`, `own_bundle`, `inspect_bundle`, `setup_apply` (gate + helper hand-off), `pki_csr_action` diff, `engines_with_store`, `history` | Full for listed functions | No |
| B5 history | `history_store.rs` (load, flush, diag runs, atomic write); agent hooks | Full | A-01 unit test |
| install/uninstall | `install.sh` and `uninstall.sh` diff (engine store), `baft-pki.sh` diff | Diff only | No |
| Panel | `pages.js` diff; grep for `innerHTML` in `panel/*.js` (none) | Diff only | No |
| Workflows | `tag-prerelease.yml` (full), `phase1-trusted-prerelease-upgrade-lab.yml` (validation job and input use), `phase1-rust.yml` audit/deny steps | Partial | No |
| Systemd units | grep of `User`, `NoNewPrivileges`, `ProtectSystem`, `CapabilityBoundingSet`, paths | Skim | No |

**Not audited in this pass** (unchanged since the earlier reviews, not re-read): tunnel data path
(`tunnel.rs`, `tls.rs` outside the diff, `noise.rs`, `handshake.rs`), `control_socket.rs`,
`authorization.rs`, `key_custody.rs`, `runtime_host.rs` (supervise), `apply.rs`, the rest of
`agent.rs` (diagnostics, lifecycle, desired state, verify), `install-agent-control.sh`,
`quick-install.sh`, `backup.sh`/`restore.sh`/`rollback.sh`, `release.yml` and cosign verification,
`phase1-trusted-b4-baft-lab.yml`, `panel` code outside the diff, LAB scripts, fuzz harness.
`cargo audit` was not run locally (CI runs it; see below).
Correction (second pass): these files are unchanged since `dc0e947`, but that is not coverage. The
PR #199 review covered only the control-request boundary (`install-agent-control.sh`,
`migrate-agent-control-dir.sh`, `update.sh`, units and tests), and the 2026-11-09 assessment read
`apply.rs` and `release.yml` at `4852ecf`, out-scoped the transport, and did not name
`control_socket.rs`, `authorization.rs`, `key_custody.rs` or the backup scripts. Since `4852ecf`
`apply.rs` (+489/-83), `release.yml` (+62/-36), `tunnel.rs` (+539), `tls.rs` (+254),
`handshake.rs` (+185), `authorization.rs` (+147) changed and `control_socket.rs`, `backup.sh`,
`restore.sh` are new. They were therefore audited in the second pass below.

| ID | Severity | Status | Area |
|----|----------|--------|------|
| A-01 | Low | Fixed in this PR | B5 history writer followed a link at its temporary name |
| A-02 | Info | Accepted | Engine store launch checks digest and mode, not owner |
| A-03 | Info | Accepted | History disk I/O under a lock on the async sampler |
| A-04 | Info | By design | BAFT serving node has no client certificate pin |

### A-01 (Low, fixed): history temp file followed links

`history_store::write_atomic_json` opened `history.json.tmp` / `diagnostics-history.json.tmp` with
`create+truncate` and no `O_NOFOLLOW`. A link planted at that name would have been written through
by the agent user. Only the agent user or root can write the state directory
(`StateDirectory=baft-tunnel-agent/%i`, mode 0700), so exploitation needs prior compromise of that
account; impact is limited to files that account can write. Fix: open with `O_NOFOLLOW`; regression
test `a_planted_link_at_the_temporary_name_is_not_followed`.

### A-02 (Info): engine store owner not checked at launch

`engine_store::resolve_for_launch` re-hashes the stored engine against the pin built into the
binary and refuses group/other-writable files, but does not require root ownership. The store is
created by root (`install.sh --baft-engine`), its `current` pointer is validated as
`releases/<64 hex>`, and the config's `baft.store`/`baft.sha256` are written only by the root setup
helper from built-in constants (not from the pairing bundle or the agent). The tunnel runs as
`baft-tunnel` with no capabilities. Residual: none beyond root. Optional hardening: owner == 0 check.

### A-03 (Info): history I/O in the sampler

`record_long`/`run_diagnostics` write (with `fsync`) at most once a minute while holding a
`std::sync::Mutex`, and `GET /v1/history` re-reads the bounded (512 KiB) diagnostics file. All
paths are authenticated; worst case is a short stall of the agent, not of the tunnel.

### A-04 (By design): BAFT serve side unpinned

For engine `baft` the forwarding node pins the serving node's certificate SHA-256; the serving node
accepts clients chained to the offline CA, not revoked by the CRL, with exactly the peer's
`urn:baft:node:<peer_id>` URI SAN (`allowed_peer_identities`). This matches ADR-0019 when the CA is
dedicated to one tunnel pair. Note the interaction with SA-07: the agent's certificate install
carries the CA PEM, so a token holder could install a different CA and then be trusted by a BAFT
serving node. The token is already admin (SA-07 accepted), so this adds no new privilege, but it is
why the serving side is weaker than tcp-tls, which pins on both sides.

## Verified without findings

- **CSR identity profile (#201):** the profile (`baft-tunnel` or `baft`) is fixed at CSR time in
  root-owned metadata; install and read refuse a different profile; unknown names fail strict
  deserialisation; `validate_leaf_policy` requires exactly one matching URI SAN.
- **BAFT pairing:** v2 bundles require a 64-hex engine digest equal to the local pin; mismatched
  versions/engines are refused; agent `inspect` also compares against the built-in pin.
- **BAFT certificate at init:** chain, EKU, validity, CRL and exact SAN profile are checked before
  a bundle is produced.
- **Install/uninstall:** `--baft-engine` refuses symlinks and non-directories, validates pointer
  targets, rolls back pointers on failure, re-verifies the stored copy after install.
- **Agent API:** loopback-only listen, 0600 token of at least the minimum length, connection
  slots, whole-request read deadline, 512 KiB body cap, Host allowlist (421) and a constant-time
  bearer check in front of every non-health route, including all routes added since `dc0e947`.
- **Root setup helper:** consumes the request once, `O_NOFOLLOW` bounded read, strict
  deserialisation, binds instance and config directory, writes no agent-authored file (config is
  derived from validated plan fields and built-in BAFT pin/store constants), result written
  `create_new` after unlinking any planted link; `adopt_for_service` uses `lchown` and refuses
  non-regular files.
- **update.sh / migration:** fail-closed boundary migration before any binary change, journaled
  unit stop/restore, refusal on pending requests or active root helpers, root-only state with
  ownership checks, config/key digest compared before and after. Info: `update.sh` runs
  `BINARY --version` as root before installing it, so the operator must verify the signature first
  (documented install path does this; not re-checked here).
- **Panel:** new history/alert views render through text nodes only (no `innerHTML`).
- **History files:** size-bounded, `symlink_metadata` regular-file check, strict version and
  monotonic-bucket validation, 0600 files.
- **Workflows:** `tag-prerelease.yml` only tags `vX.Y.Z-rc.N` on commits of main with green
  required checks and never creates stable tags; LAB workflows validate tags before any job that
  interpolates them and keep secrets in the `baft-tunnel-lab` environment.
- **Dependencies:** `cargo audit -D warnings` and `cargo deny` run in `phase1-rust` (last
  completed main run 43682746676 on `1f452de`: success).

## Still open, unchanged

SA-07 (token = admin) and SA-11 accepted; L-01..L-04 documented. Reverse stays Preview and BAFT
stays Preview/Direct.

## Second pass (same audited SHA `7011e3b`)

Owner decision (via the coordinator, under the owner's delegation): the Stable gate needs the parts
not covered above. Read-only review unless stated.

| Component | What was read | Depth |
|-----------|---------------|-------|
| Transport engines | `engine.rs` (TLS listener/dialer: TLS 1.3 only, mandatory CRL with expiry, pin + URI SAN identity on both sides; Noise engine), `tls.rs` (`client_config`, `server_config`, `verify_local_certificate`, legacy P-256 key normalisation, `roots`), `noise.rs` (IK, pinned statics both directions, prologue binds ids/tunnel/direction, record bounds) | Full |
| Session authorization | `handshake.rs` (hello binding, nonces, `verify_peer_identity`, `verify_peer_fingerprint`, `validate_fingerprint`), `authorization.rs` (full) | Full |
| Tunnel server/client | `tunnel.rs`: `PendingGuard`, `serve_versioned` accept path, `closed_without_data`, `server_session_versioned`, `serve_streams_versioned` (fixed destination, per-stream capability authorization, slot limits), `connect_peer_staged`, `yamux_config` | Full for listed; reverse ingress relay and `integrity_probe.rs` skimmed |
| Control socket | `control_socket.rs` non-test part (full) | Full |
| Key custody | `key_custody.rs` non-test part (full) | Full |
| Apply | `apply.rs`: `write_new`, `tracked`, `SystemUnits` request/state/session, `run_with` (create-only, rollback of created files only), `persist`, `replay` | Full for listed; `run_lifecycle`/`run_stop` skimmed |
| Release | `release.yml` (full) | Full |
| Backup/restore/rollback | `backup.sh`, `restore.sh`, `rollback.sh` (full) | Full |

Dynamic: the A-05 unit test (new) and the existing `pending_guard` tests were run locally (3/3 pass).

| ID | Severity | Status | Title |
|----|----------|--------|-------|
| A-05 | Low | Fixed (second-pass PR) | Pre-auth per-source limit counted each IPv6 address separately |
| A-06 | Low | Fixed (release split PR) | Release build runs in the job that holds the OIDC signing permission |
| A-07 | Low | Open, recommended before Stable | A pushed stable tag releases without a required-checks gate |
| A-08 | Info | Accepted | Agent reads the tunnel's `session.json` by path after a metadata check |

**A-05 (Low, fixed).** The unauthenticated-connection limit (32 global, 2 per source) keyed IPv6 by
full address, so one host with a /64 could hold all 32 pending slots for up to the pre-auth window
(peek 10 s + engine handshake 10 s + protocol hello 10 s) and block new peer sessions; established
sessions are unaffected. Fix: IPv6 counted per /64, IPv4-mapped IPv6 as IPv4
(`pending_source`), test `pending_guard_counts_ipv6_per_64_and_mapped_ipv4_as_ipv4`. Residual: 16
distinct IPv4 sources can still fill the global pool (design limit, unchanged).

**A-06 (Low).** `release.yml` builds the binary, the Go engine and `cargo-cyclonedx` in the same job
that has `id-token: write`, so code run at build time (build scripts, Go toolchain modules) can
request an OIDC token and sign arbitrary blobs as `release.yml@refs/tags/...`. Dependencies are
locked and audited, and a compromised dependency would already poison the signed binary, so the
extra impact is limited to signing other blobs. Recommendation: build in a job without
`id-token`, sign in a separate job that only downloads `dist/`.

**A-07 (Low).** `tag-prerelease.yml` checks `rust`, `ubuntu-2204-baseline`, `lab-config` before
tagging, but only for `-rc.N`. A stable `vX.Y.Z` tag can only be pushed by hand, and `release.yml`
then requires only "ancestor of main". Recommendation before 0.2.0: a tag ruleset restricting
`v*` creation, and the same required-checks loop in `release.yml` (or a stable path in
`tag-prerelease.yml`). Repository rulesets were not inspected (no API access used for that).

**A-08 (Info).** `SystemUnits::session` checks `symlink_metadata` then reads
`/run/baft-tunnel/NAME/session.json`, a tunnel-writable directory; a compromised tunnel could swap in
a link. The value is used only when it parses as JSON with `state=authorized` and the unit's main
PID, so at most a status display is affected.

Second-pass items verified without findings: TLS 1.3 only with early data off; CRL mandatory with
expiry enforced on both sides; both TLS sides check the certificate pin and the exact
`urn:baft-tunnel:node:<peer>` URI SAN; Noise IK pins the peer static key on both sides and binds
ids, tunnel and direction in the prologue; the hello is checked field by field and authorization
rejects unknown, duplicate or missing capabilities; integrity probes are v2-only and rate limited;
each stream connects only the fixed destination; the control socket accepts only the
`baft-tunnel-agent` uid via `SO_PEERCRED`, one fixed 13-byte request, opens its directory with
`O_NOFOLLOW` and checks owner/mode; systemd credentials resolve only inside
`CREDENTIALS_DIRECTORY`; apply creates files with `create_new` and rolls back only what it created;
restore verifies the checksum, an allowlist of paths, and refuses links and special files.

## Third pass: PR #215 and release gating (main `b8538f2`)

Read: the full #215 diff (`tls.rs` `ReloadingAcceptor`, `engine.rs` listener, `tunnel.rs` call
site, `apply-crl.sh`, `push-crl.sh`). Dynamic: none beyond CI (the PR's own reload test).

| ID | Severity | Status | Title |
|----|----------|--------|-------|
| A-07 | Low | Fixed (this PR) | `release.yml` now requires `rust`, `ubuntu-2204-baseline`, `lab-config` green on the tagged commit for a stable tag; `-rc` tags unchanged |
| A-09 | Info | Accepted | `apply-crl --no-restart` keeps live sessions of a newly revoked peer until they reconnect (documented in the script; default still restarts) |
| A-10 | Info | Accepted | The listener `stat`s four TLS files per accepted connection before the handshake; bounded by the pre-auth limits |

Verified in #215: a changed file set is rebuilt with the same TLS 1.3 / mandatory-CRL verifier; a
file that does not load makes the listener refuse new connections (fail closed) until a loadable
version appears; peer pins and URI identity checks are unchanged; the files stay root-owned and
`apply-crl.sh` still enforces the CRL number order; `--no-restart` is a constant string in the
remote command.

Also read in this pass: `quick-install.sh` (full). It pins the cosign package by SHA-256, verifies
`SHA256SUMS` against the identity `release.yml@refs/tags/<TAG>` and the GitHub OIDC issuer, requires
exactly one checksum line for the archive, validates every tar member (regular files and
directories under the release root only) before extraction, and refuses drafts.

| ID | Severity | Status | Title |
|----|----------|--------|-------|
| A-11 | Info | Fixed (owner decision, delegated) | The quick installer now selects stable releases only by default; with no stable release it exits non-zero naming `--channel prerelease`. Prereleases need `--channel prerelease` or an exact `--tag` (LAB workflows pin `--tag`). Tested in `scripts/test-quick-install-failures.py` |

A-06 fixed after rc.9 was published: `release.yml` now has a `build` job (`contents: read`,
`checks: read`, no `id-token`) that builds everything and uploads `dist/`, and a
`sign-and-publish` job (`contents: write`, `id-token: write`) that checks out the same ref, receives
exactly the six expected artifacts, writes `SHA256SUMS`, signs, self-verifies, publishes and runs
the quick-installer check. The signing identity stays `release.yml@refs/tags/<TAG>`, so existing
verifiers are unchanged. A `dry_run` dispatch input builds and hands over on any ref but skips
signing and publishing; `lab/trusted/test-workflow-boundary.sh` now fails if the build job gains
`id-token`/`contents: write` or the signing job builds code. A tag ruleset that restricts who may create `v*` tags is a repository
setting the owner applies; it was not inspected.

## Stable gate statement

After all four passes, nothing was found at Critical, High or Medium in the components marked "Full" or
in the fourth-pass components. A-06, A-07 and A-11 are fixed. The first stable release made through
the split workflow is its first real signing run; a dry run on the branch exercised build and
hand-over only. The `v*` tag rulesets (A-13) are applied and were checked by reading the
configuration back, not by creating tags. The fifth pass (main `05aa2dc`) closed the remaining "not audited" list: every
component named in earlier passes as not audited has now been read in full. Nothing outside the
repository was audited: third-party crates beyond `cargo audit`/`cargo deny`, the BAFT engine's
own source (separate AGPL binary, pinned by SHA-256), GitHub Actions runners and the hosts. Evidence
level stays source review + CI.

## Fourth pass (main `71c6c19`, owner decision via the coordinator)

| Component | Coverage | Result |
|-----------|----------|--------|
| `scripts/install-agent-control.sh` + `systemd/control/*`, `systemd/agent/*` | Full | Instance name and `--listen` validated before use; token created once as the agent user (0600, never printed); request directory enforced `root:baft-tunnel-agent 0770` through the fail-closed migration helper; group exclusivity checked; each root helper runs one fixed `systemctl` command for `baft-tunnel@%i` only; agent unit has an empty capability set, `ProtectSystem=strict`, syscall filter. A-12 |
| Reverse ingress relay (`tunnel.rs` `IngressSide`, reverse branch of `serve_versioned`, `run_forward_session_versioned`) | Full | Streams open only after the TLS/Noise handshake and the protocol handshake authorize the peer; a new authenticated session replaces the old one (old one is signalled before the queue lock is taken); waiting user connections are bounded by a semaphore and `INGRESS_QUEUE_MAX_WAIT`; the queue is bounded; stream slots are bounded and excess connections are dropped; relays use the idle timeout. No finding |
| `run_lifecycle` (`apply.rs`) and `tunnel_lifecycle` (`agent.rs`) | Full | Instance comes from the agent's own `--instance`, never from the request; `confirm: true` and a bounded idempotency key required; one lock; config parsed and `validate_public()`d (agent never reads the private key); direction must be offered by the engine; only `/etc/baft-tunnel`; receipt and audit line per action. No finding |
| Panel outside B4..B6 (`app.js`, `pages.js`, `charts.js`, `i18n.js`, `index.html`) | Full | No `innerHTML`/`eval`/HTML sinks; the `h()` helper builds text-only DOM; all links are constant in-app hashes; API calls send the token as a Bearer header (no cookies, so no CSRF); token kept in `sessionStorage`; the agent serves a strict CSP (`default-src 'none'`, `script-src 'self'`, `frame-ancestors 'none'`), `nosniff`, `no-referrer`, `no-store`. No finding |
| Repository settings | Read only | `main` protection: required checks `rust`, `ubuntu-2204-baseline`, `lab-config`; reviews required; no force push or deletion; admins not enforced; not strict (branch need not be up to date). Rulesets: none before this pass. A-13 |

| ID | Severity | Status | Title |
|----|----------|--------|-------|
| A-12 | Info | Accepted (by design) | All registered instances share one agent account and one admin token (SA-07), so the agent of one instance can drop control requests for another registered instance. One trust domain per host; documented, not a boundary |
| A-13 | Low | Resolved | No `v*` tag ruleset existed. Applied 2026-11-15: ruleset 27302115 `v-tags-immutable` (deletion, update, non_fast_forward on `refs/tags/v*`, no bypass) and 27302116 `v-stable-tags-admin-create` (creation limited to the Admin role; excludes `v*-rc.*` and `v0.0.0-dryrun.*`). Verified by reading the configuration back through the API only, not by creating or moving tags. Side effect: no `v*` tag, rc included, can ever be moved or deleted. Definitions and apply steps: `docs/security/tag-rulesets/` |

## Fifth pass (main `05aa2dc`, owner decision via the coordinator)

| Component | Coverage | Result |
|-----------|----------|--------|
| `src/integrity_probe.rs` | Full | Fixed-size frames (magic, exact length, 32-byte nonce, 64 KiB payload streamed through a 4 KiB stack buffer); no peer-controlled length reaches an allocation; trailing bytes rejected both ways; nonce echo plus SHA-256 and byte count checked; 10 s end-to-end deadline on both sides; one active probe per instance and one start per 60 s; failures typed, only mismatch/malformed is `fail`. No finding |
| `src/observe.rs` | Full | Evidence files read with `symlink_metadata` (regular file only), size-capped at 4 KiB before and during the read; session and counters accepted only when the recorded pid is alive and matches the unit's main pid; rate maths skips counter resets and pid changes. No finding |
| `src/session_status.rs` | Full | Writes only under systemd `RUNTIME_DIRECTORY` (absolute, existing; unit sets `baft-tunnel:baft-tunnel 0750`), temp file then rename, mode 0644, no secrets (ids, pid, counters). A-15 |
| `src/runtime_host.rs` | Full | Manifest `deny_unknown_fields`, schema pinned, id/commit/sha256/https source/relative artifact/limits validated; binary must be a regular file not group/other writable and match the pinned SHA-256 before start; child gets a cleared environment and null stdin, `kill_on_drop`; restart budget and backoff bounded. A-14 |
| `src/main.rs` | Full | CLI dispatch only. Root helpers refuse unless euid 0 and take paths that are fixed constants; the agent refuses root unless `--allow-root`; the agent validates only public config (never reads the private key); `menu` execs `menu.sh` next to the installed binary. No finding |
| `scripts/install.sh` | Full | Instance/register names validated; node key must be a regular non-symlink PEM ≤ 16 KiB; symlinked credential directories, files and drop-ins refused; unmanaged drop-ins never overwritten; credential written 0600 root:root via temp + rename, encrypted mode fails closed; engine store and pointers checked (directory, not symlink, `releases/<sha256>` targets); full rollback on failure; `/etc/baft-tunnel` root:baft-tunnel 0750. A-16 |
| `scripts/menu.sh` | Full | No `eval`/`sh -c`; every value typed by the operator is checked against a regex before use and passed as an array element; commands are echoed with `printf %q` before running; checksum normalised and confirmed. A-16 |

| ID | Severity | Status | Title |
|----|----------|--------|-------|
| A-14 | Info | Accepted | `runtime_host` checks the engine binary's SHA-256 and then starts it by path, and restarts re-exec without re-checking. Safe because the engine store is root-owned (`install.sh` sets `root:root`, releases are content-addressed) and the mode check refuses group/other-writable files; it would not hold for a store owned by the tunnel user. Same root cause as A-02 |
| A-15 | Info | Accepted | `session_status` writes its temp file with `std::fs::write`, which follows a planted link. Only the tunnel user can write its 0750 runtime directory, so there is no cross-user boundary; noted for consistency with A-01 |
| A-16 | Info | Open (docs/UX) | The root menu still offers noise-tcp pairing including Reverse, and `install.sh` ends by suggesting that menu pairing, while the quickstart says to use the panel Wizard for the supported tcp-tls path. The menu starts units with `systemctl` directly, so the panel's direction gate does not apply there. Root-only and documented as Experimental; align the text before 0.2.0 |

