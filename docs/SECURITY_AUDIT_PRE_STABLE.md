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
Fact check: `git diff --stat dc0e947..7011e3b` is empty for `tunnel.rs`, `noise.rs`, `handshake.rs`,
`control_socket.rs`, `authorization.rs`, `key_custody.rs`, `runtime_host.rs`, `apply.rs`,
`install-agent-control.sh`, `quick-install.sh`, `backup.sh`, `restore.sh`, `rollback.sh` and
`release.yml`: they are byte-identical to what the PR #199 review and the earlier assessment saw.

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

## Stable gate statement

For the components with "Full" coverage above, nothing found blocks Stable 0.2.0. Before using this
as the security gate, the "not audited" list needs either a re-review or an explicit decision that
the earlier assessments (2026-11-09, PR #199) still cover it because the code is unchanged.
