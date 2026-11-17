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
