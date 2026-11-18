# ADR-0050: automatic pairing and per-tunnel CA (one code, no file transfer)

Status: Proposed (2026-11-17). Needs the independent security review required for new trust flows before the
pairing listener is enabled by default. Supersedes nothing; adds an *auto* mode beside ADR-0012/0014 offline mode.

## Problem

See `AUTO_TUNNEL_FLOW_AUDIT.md`: an offline CA ceremony, two hand-synchronised wizards and a manual bundle exchange
make a first tunnel unreachable for ordinary users. Owner requirement: the user makes a few choices; the system
does the rest.

## Decision

Add **auto mode**. The offline-CA mode stays available as "Advanced / my own CA" and is unchanged.

1. **Listener generates the pairing.** The node that accepts the tunnel connection (serve in Direct; the reachable
   side in Reverse) asks for a tunnel. The user chooses only: name, direction, service port. The system generates
   node ids, tunnel id, tunnel port (default 8443, checked free), server name (detected address, overridable),
   protocol v2, `tcp-tls`.
2. **Pairing code** (one string, `BAFT1-...`, also usable as one-line command `... --join CODE`): listener
   address:port, SHA-256 pin of a short-lived pairing certificate, a 128-bit one-time token, the generated
   tunnel parameters and the peer node id. Expires in 15 min, single use, max 3 token attempts then the
   listener closes. The code is a bearer secret for that window and is never logged.
3. **Per-tunnel CA.** At pairing creation the root helper creates a P-384 CA *for this tunnel only* with X.509
   name constraints limited to the two DNS/IP server names (URI constraints are not usable for `urn:` node ids; the node-id allowlist stays enforced by the data plane and by the helper, which signs only the two fixed ids), `pathlen:0`. The CA key stays in a
   root-only directory on the listener host and never leaves it; it never goes to the peer, the agent process,
   the panel, logs or backups. A one-year CRL (empty) is issued; renewal is automatic by the same helper.
4. **Join.** The second server runs `join CODE` (installer flag, CLI, or paste in the panel). It generates its
   key and CSR locally (key never leaves), dials the pairing listener, pins the pairing certificate from the
   code, proves the token, and sends the CSR. The listener's panel shows the joiner (node id, source address) and
   the user approves with one click. The helper signs (identity fixed by the CA side, as ADR-0012), returns
   leaf + CA + CRL. Both sides install through the existing validated install path, write config via the
   existing `setup` planner and start the unit. Both panels show real preflight/verify results.
5. **No runtime change.** The data plane still requires CA, CRL, leaf, URI SAN and peer pin (ADR-0011/0012);
   auto mode only automates how those files are produced and delivered.
6. **Constraints kept:** no CAP_NET_ADMIN, no firewall/route/sysctl change (user opens the tunnel port; the
   panel says so in plain words), Reverse stays Preview, no secrets in logs/receipts, idempotent and
   rollback-safe (a failed join leaves no partial config), strict bounds on every message.

## Trust analysis (to be reviewed)

- Compromise of the listener host exposes its own pair CA, which can only mint certificates for the two
  allowed names (name constraints), and the attacker already holds the listener's key. Trade-off versus ADR-0014's
  offline CA is explicit and limited to two-node pairs. Fleet/multi-node deployments keep the offline CA.
- A leaked code lets someone race the intended peer within 15 minutes; the user approval on the listener (shows
  peer node id and source address) and single use bound this. Optional "paired from the panel" display of a short
  verification string is left for review.
- The pairing listener is the only new network surface: TLS 1.3, pinned self-signed cert, bounded frame sizes,
  deadlines, one accepted session, runs only during the 15-minute window, non-root, no CA key access (signing
  via root helper request, as ADR-0023/0026).

## Staged plan

1. This ADR + audit (docs).
2. Pair-local CA + signing in Rust (name constraints, CRL), tests.
3. Pairing code codec and pair listener/joiner protocol over pinned TLS, two-process tests.
4. Agent routes + root helper wiring; `baft-tunnel join CODE`; installer `--join`.
5. Panel: three-choice "Create tunnel" flow (Persian RTL / English), advanced wizard kept.
6. Automatic CRL renewal, docs, acceptance script; LAB two-host run requested from the coordinator.

Evidence labels: stages 1-5 are CI/local only; no LAB claim until the two-host run.

## Addendum 1 (2026-11-17): review fixes

- **Privilege separation (H1).** The process that touches the network is a worker (`baft-tunnel pair worker`, a child of
  the root helper) that drops to the service user, clears supplementary groups, sets no-new-privileges and verifies it
  cannot regain root before it reads anything. It generates the one-time pairing identity itself and receives only the
  token, joiner id, expiry and port over a pipe; the pair CA and every private key stay in the root parent. The worker
  forwards the single verified join request; the parent signs only for the joiner id fixed in the code (never the listener
  id), so a compromised worker cannot obtain another identity. Without root the same worker runs in-process.
- **Approval (H2/H3).** The joiner waits as long as the approval may take (180 s against a 120 s approval window). The
  first token-valid request ends the window whatever happens, so a denial ends the pairing and the token cannot be
  retried. The `approval` status carries a random nonce; the approve request must repeat it, only in the `approval` state.
  Both servers show a short fingerprint (`ab12-cd34-ef56`) of the joiner's key, so the user can see that the approved key
  is the one the joiner generated.
- **Commit and rollback.** The listener reports success only after the joiner acknowledged a complete install; a nack,
  timeout or any failure removes everything the pairing created on both sides. A pairing refuses to start in a directory
  that already holds a configuration (one pair per configuration directory).
- **Limits.** 3 failures per source address, 20 in total (then the window closes), 16 concurrent connections, 5 s
  handshake deadline; parser details are not echoed to the panel.
- **Lifetimes.** CA 1825 days, leaves and CRL 1800 days. Revocation is re-pairing; automatic renewal stays a later step.
  Name constraints cover DNS/IP only; the node-id allowlist is enforced by the fixed ids above and the data plane.
- **Installer.** `--offer` without a terminal is refused unless `--auto-approve` is given (with a warning); the code is
  passed to `pair accept -` on standard input. Pairing exists only on demand and is never on by default.

## Addendum 2 (2026-11-17): location question and Reverse (design; stage (a) implemented, Reverse not yet)

Owner input: ask "is this server in Iran or abroad?" and derive the rest.

1. **Question.** Each server answers once: Iran or abroad. Together with "does the service run on this server?" (the first
   server already supplies the service port) this fixes everything else; the user still only picks name and ports.
2. **Who listens.** The listener must be reachable. Inbound from abroad to an Iran server is usually blocked or throttled,
   so the abroad server listens whenever one server is abroad and one in Iran. Both abroad or both in Iran: either can
   listen; default is the server that created the code.
3. **Direct or Reverse.** If the service runs on the abroad server, the tunnel is Direct with the Iran server dialing out
   (forward). If the service runs on the Iran server, the tunnel is Reverse: the Iran server is the service node that dials
   out and the abroad server is the public node that accepts users (ADR-0021 Amendment 1, ADR-0037). The choice is a
   preselected default shown in plain words, never a silent switch, and never a downgrade of an explicit Reverse request to
   Direct.
4. **Defaults by location.** `session_rotate_secs` (ADR-0049, 5-3600, Direct dialer only) defaults to 10 on the Iran-side
   Direct dialer given the aged-flow throttle; Reverse cannot rotate, which the UI states. Backup links and ports are not
   preselected.
5. **Preflight, not assumption.** Before creating the code the panel runs a reachability check of the chosen listener port
   from the other server's side (the join itself is the first proof: a refused connect becomes "the other server cannot
   reach port N; open it in this server's firewall or hosting panel", plain language, no firewall changes by us). Location is
   a hint; reachability is evidence.
6. **Reverse pairing.** Reuses the pair CA, code, worker, approval and rollback unchanged. The code gains a direction
   (`direct`|`reverse`, already a field) and the roles map: listener = public node (user ingress), joiner = service node
   (dials out, holds the target). Bundles/config come from the same `setup::init`/`join` Reverse path the guided setup
   uses. Reverse stays Preview and unadvertised until the two-host acceptance gate of ADR-0037; the capability contract is
   read from `GET /v1/engines`, so the flow offers Reverse only while that contract does.
7. **Stages.** (a) location question and Direct defaults in the panel/CLI; (b) Reverse pairing core + tests; (c) panel and
   installer wording; (d) two-host LAB run requested from the coordinator, queued after the pairing LAB run.

### Stage (a) as built (2026-11-18)

The panel asks "Where is this server? Inside Iran / Abroad" on both steps (CLI `--location`, installer `--location`). Only
the joiner's answer changes configuration: an Iran-side Direct dialer gets `session_rotate_secs = 10` (validated through the
normal config loader; failure rolls the pairing back). The first server's answer is a hint: choosing Iran shows the advice to
do step one on the abroad server and states that reachability is checked when the other server connects. Nothing else is
inferred from the answer, no listener or direction is switched silently, and Reverse is still not offered by pairing.

### Stage (b)/(c) as built (2026-11-18): Reverse pairing

`pair offer --direction reverse` (panel: "which server runs your service?" = the second server; installer `--direction`)
creates a Reverse tcp-tls Preview tunnel with the same code, CA, worker, approval and rollback. The listener is the public
node (`ingress` on `127.0.0.1:PORT`), the joiner is the service node that dials out (`target` `127.0.0.1:PORT`); `setup`
requires the same service port number on both nodes, so one number is used. An Iran answer does not add session rotation to
a Reverse joiner (ADR-0049 refuses it) and the CLI says so. Evidence: unit/e2e tests assert the generated configs and that
they load; panel QA covers the question. Not yet shown: a Reverse data path through a pairing-generated config, which needs
two hosts (LAB), and Reverse stays Preview and unadvertised outside this flow.
