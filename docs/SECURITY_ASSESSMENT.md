# Independent security assessment: P0 + P1 surface

Date: 2026-11-09. Baseline: `main` at `4852ecf` (PR #109). Reviewer: an assessor that did not
write the reviewed code. Method: manual source review of the files below, unit tests for every
fix, CI on the fix branches. No LAB run was needed: every fix is covered by unit tests or the
real-systemd `ubuntu-2204-baseline` job. Nothing here makes the product production-ready.

Scope: agent control API and token auth (`src/agent.rs`, `src/main.rs`), diagnostics / status /
audit / traffic routes, Apply/Verify/Receipt (`src/apply.rs`, `src/setup.rs`), systemd units and
the path-unit privilege grant (`systemd/`, `scripts/install-agent-control.sh`), install/uninstall
(`scripts/install.sh`, `scripts/uninstall.sh`), offline PKI (`scripts/pki/baft-pki.sh`, CRL
scripts), release signing (`.github/workflows/release.yml`, `docs/RELEASE.md`), panel assets.
Out of scope: the transport cryptography itself (covered by P0 tests and LAB), the BAFT engine.

Threat actors considered: remote network attacker; malicious web page in the operator's browser
(CSRF, DNS rebinding); unprivileged local user; a compromised tunnel process (`baft-tunnel@NAME`,
the most exposed component); an attacker who can influence a release; holder of the agent token
(treated as the host administrator).

## Findings

| ID | Severity | Title | Status |
|----|----------|-------|--------|
| SA-01 | Medium | Agent could run as root; Apply then creates files as root at a caller-chosen path | Fixed (#112) |
| SA-02 | Medium | Compromised tunnel process could read the agent token (same user) | Fixed (#112) |
| SA-03 | Medium | Release signature policy accepts any ref; `ref` input can sign an unreviewed commit | Fixed (#114) |
| SA-04 | Low | No `Host` check on the loopback API (DNS rebinding) | Fixed (#112) |
| SA-05 | Low | Slow clients could hold all 16 agent connection slots for 45 s each | Fixed (#112) |
| SA-06 | Low | Release workflow expands a dispatcher-controlled tag directly in shell | Fixed (#114) |
| SA-07 | Low | Apply/preflight accept any absolute `dir`; preflight probes any IP:port | Accepted (token = admin) |
| SA-08 | Low | `audit.log` grows without rotation; `GET /v1/audit` reads the whole file | Fixed (4 MiB rotation to `audit.log.1`; endpoint reads a 256 KiB tail) |
| SA-09 | Low | Agent and tunnel share one uid; the tunnel can still signal (kill) the agent | Fixed in code (U2): agent runs as `baft-tunnel-agent`; real-systemd CI evidence; LAB check pending |
| SA-10 | Info | Instance-name rules differ (`.` allowed by install/setup, refused by the control helper) | Fixed (one rule: letters, digits, `_`, `-`) |

### SA-01 (Medium): agent as root

Evidence: `docs/OPERATOR_GUIDE.md` started the agent with a token under `/etc/baft-tunnel`
(root-only), so operators ran it as root; `lab/real-hosts/phase1-two-host-e2e.sh` also runs it as
root. `POST /v1/setup/apply` passes the caller's `plan.dir` (any absolute path) to `setup::init` /
`join`, which create directories (`0750`) and files (pairing state, bundle, node key,
`NAME.json`). As root that is a file-creation primitive anywhere on the host for whoever holds the
token, and it voids ADR-0023's assumption that the agent is unprivileged.
Fix: `baft-tunnel agent` refuses to start when the effective uid is 0 unless `--allow-root` is
given; the guide now runs it as `baft-tunnel` via `setpriv` with the token in
`/var/lib/baft-tunnel/agent`. The LAB e2e script passes `--allow-root` explicitly (ephemeral test
directories only); the MVP stage and the real-systemd CI already use the service user.

### SA-02 (Medium): agent token readable from the tunnel unit

Evidence: the agent and `baft-tunnel@NAME` both run as `baft-tunnel`; the agent token and state
live in `/var/lib/baft-tunnel/agent` (owner `baft-tunnel`, `0600`). `ProtectSystem=strict` makes
paths read-only, not unreadable, so code execution inside the network-facing tunnel would yield
the token and with it Apply `start`/`stop` of every instance that has the control helper, plus
file creation as `baft-tunnel`.
Fix: `InaccessiblePaths=-/var/lib/baft-tunnel` in `systemd/baft-tunnel@.service` (the tunnel never
uses that path; `-` tolerates its absence). Residual risk: SA-09.

### SA-03 (Medium): release signature binding

Evidence: `docs/RELEASE.md`, `release.yml` self-verify and the LAB workflow verify with
`--certificate-identity-regexp '^…/release.yml@'`, which accepts a signature from the workflow on
any branch or tag. Separately, `workflow_dispatch` input `ref` checks out any branch or commit while
the signing certificate still says `refs/heads/main`, so the signature does not bind to reviewed
code.
Fix: verification pins the identity to `release.yml@refs/heads/main` or `@refs/tags/v*`; the
workflow refuses to build a commit that is not contained in `origin/main`. All four existing
releases were dispatched from `main` (runs 42394963062, 42505948827, 42527359063, 42533087739), so
they still verify.

### SA-04 (Low): DNS rebinding

Evidence: the agent did not look at `Host`. A rebinding page could read `/v1/health` and the static
panel; it could not read the token (kept in the agent origin's `sessionStorage`) and every data
route needs `Authorization: Bearer`, which cross-origin pages cannot send without a CORS preflight
that the agent never grants. Impact therefore low.
Fix: requests whose `Host` name is not `127.0.0.1`, `localhost` or `::1` (any port, so `ssh -L`
still works) get `421` before routing. A request without `Host` (non-browser) is still accepted.

### SA-05 (Low): connection-slot exhaustion

Evidence: 16 concurrent connections, a 5 s timeout per read and 45 s per handler; a local client
sending one byte every 4 s kept a slot for 45 s. Fix: the whole request (head and body) must
arrive within 5 s; a test checks a trickling client is dropped. Residual: 16 fresh connections can
still fill the slots for 5 s at a time (local users only; loopback listener).

### SA-06 (Low): shell expansion in the release workflow

Evidence: `${{ steps.v.outputs.tag }}` (from the dispatch input `tag` or a pushed tag name) was
expanded into `run:` scripts. Only users with write access can trigger the workflow. Fix: passed
through `env:` and validated against `^v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$`.

### SA-07 (Low, accepted): caller-chosen paths and probes

`plan.dir` may be any absolute path the agent user can write, preflight reports whether paths
exist, and preflight with `peer_host` makes one TCP connect to any IP:port. All require the token,
whose holder administers the host; with SA-01 fixed the agent cannot write outside what
`baft-tunnel` may write. Accepted; revisit if remote (non-loopback) management is added.

### SA-08 (Low, fixed): audit log growth

`audit.log` is append-only without rotation and `GET /v1/audit` reads the whole file to return the
last 100 lines. Token holders only. Fix: `audit.log` is renamed to `audit.log.1` (one generation, mode 0600 kept) before an append finds it at 4 MiB or more, and `GET /v1/audit` reads only the last 256 KiB of each file and returns the newest 100 entries.

### SA-09 (Low, fixed in code by U2): shared uid between agent and tunnel

Same uid means a compromised tunnel process can still send signals to the agent (denial of service)
and, where Yama `ptrace_scope` is 0, ptrace it (Ubuntu default is 1, which blocks it). Recommendation:
a separate `baft-tunnel-agent` user in group `baft-tunnel` and a shipped hardened agent unit. Not done in this pass: the split changes state-directory ownership, the control directory group and the token path, so it must be validated on the LAB hosts before shipping. Impact today is denial of service of the agent only (it restarts under systemd; Ubuntu `ptrace_scope` 1 blocks ptrace). Tracked for the first production-hardening phase.

**Fix (U2, 2026-11-09):** the shipped unit `baft-tunnel-agent@NAME.service` runs the agent as its own system user `baft-tunnel-agent` (supplementary group `baft-tunnel` for read access to the instance configuration and the unit's session/counter files, and for writing control requests). Token and state live in `/var/lib/baft-tunnel-agent` (0700, owner `baft-tunnel-agent`; the tunnel unit also has it in `InaccessiblePaths=`). The tunnel user can no longer read the token, signal or ptrace the agent. The agent never reads the tunnel's private key (it validates the configuration without the key, `validate_public`). The tunnel's runtime directory is 0750 so the agent can read the evidence files; the BAFT engine's bundle with its key copy moved to a private 0700 child (`engine/`). Residual: the tunnel user can still drop control requests (start/stop/enable/disable of its own registered instance only), unchanged from ADR-0023. Evidence: `scripts/pki/test-agent-service-lifecycle.sh` on real systemd 249.

### SA-10 (Info, fixed): instance-name rules

`install.sh` and `setup` accept `.` in instance names; the control helper and agent `--instance`
refuse it. Effect: Apply `start` refuses dotted names (fails closed). Fix: one rule everywhere new instances are created (`valid_instance`, `setup`, `install.sh`): 1-63 letters, digits, `_` or `-`, starting alphanumeric. `update.sh`, `rollback.sh` and `restore.sh` keep tolerating a dotted name already installed.

## Verified without findings

- Token: 256-bit random, created `0600` with `create_new`, read only from a regular non-symlink
  file without group/other bits, compared in constant time; audit records only a domain-separated
  8-byte hash prefix.
- Routing: unknown paths 404 before auth; every data route needs the token; bodies capped at
  64 KiB, heads at 8 KiB; `deny_unknown_fields` on Apply and plan bodies; Apply needs
  `confirm: true`, a restricted idempotency key and a re-run preflight gate; receipts never hold
  key material or bundle contents.
- Panel: strict CSP (`default-src 'none'`, `script-src 'self'`, `frame-ancestors 'none'`), token in
  `sessionStorage` only, DOM built without `innerHTML`.
- Path-unit grant (ADR-0023): fixed `systemctl --no-block start|stop baft-tunnel@%i` only, enabled
  per instance, request directory `root:baft-tunnel 0770`, unit needs `/etc/baft-tunnel/%i.json`;
  the tunnel unit itself cannot write the request directory (`ProtectSystem=strict`).
- Tunnel unit: no capabilities, `NoNewPrivileges`, syscall filter, `MemoryDenyWriteExecute`,
  private runtime directory `0700`, `UMask=0077`.
- Tunnel pre-auth limits: 10 s handshake timeout, 32 pending handshakes, 2 per source IP.
- install/uninstall: refuses symlinked credential paths and unmanaged drop-ins, root-only backups,
  rollback on failure, purge only with `--purge-config --yes`.
- PKI: CA key encrypted (P-384), `copy_extensions = none`, CSR CN must equal the node id, P-256 only,
  validity 1-397 days, CRL push with `StrictHostKeyChecking=yes`.
- Release: actions pinned by commit, `persist-credentials: false`, keyless cosign with self-verify,
  glibc floor check, engine artifact hash checked against the manifest, SBOM.

## Evidence

- PR #112 (SA-01, SA-02, SA-04, SA-05 and this document): head `64ebbed`, checks `rust`,
  `ubuntu-2204-baseline` (run 42548543707), `lab-config` (run 42548543817), `fuzz`
  (run 42548543657) all green; merged as `a8d160b`.
- PR #114 (SA-03, SA-06): head `a0be0ff`, `rust` and `ubuntu-2204-baseline` (run 42549668812),
  `lab-config` (run 42549668945) green; merged as `f3c8b54`. The signing certificates of
  v0.1.0-phase1-pre, rc.2, rc.3 and rc.4 all name `release.yml@refs/heads/main` and match the
  pinned identity.
- Exact main `f3c8b54`: `phase1-rust` workflow_dispatch run 42550456502.
- No LAB run, no tag, release or deploy. Open items: SA-07 (accepted), SA-09 (accepted for Preview, deferred).
