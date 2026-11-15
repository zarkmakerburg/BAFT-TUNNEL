# Pre-Stable security audit (0.2.0 gate)

- Audited commit: `7011e3bd60ed97b4a94f04c02d8d52443511b212` (main, 2026-11-15).
- Baseline: the 2026-11-09 assessment ([SECURITY_ASSESSMENT.md](SECURITY_ASSESSMENT.md)) and the PR #199
  privilege-boundary review (main `dc0e947`, L-01..L-04, SA-11).
- Scope: everything changed in `dc0e947..7011e3b` (B4 BAFT Direct guided setup #201, B5 persisted
  history #212, B6 panel a11y #211, S4 fuzz #213, prerelease tag/LAB workflows #202..#207), re-checked
  against the agent API, root PKI/setup helpers, engine store, install/uninstall, panel and release
  signing.
- Method: manual source review per component. No LAB run and no host was touched.
- Evidence level: source review + CI on the Ubuntu runner only (not VM, LAB or Production).

## Result

No Critical, High or Medium findings. Nothing in this audit blocks a Stable 0.2.0 on security grounds.
One Low was fixed in the same PR; the rest are accepted Info items.

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
`urn:baft:node:<peer_id>` URI SAN (`allowed_peer_identities`). This matches ADR-0019 and the
tcp-tls trust model when the CA is dedicated to one tunnel pair.

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
- **Agent API:** all new routes sit behind the constant-time bearer check; Host allowlist, request
  deadline and CSP unchanged.
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
stays Preview/Direct; neither is a security blocker for a Stable that ships them marked Preview.
