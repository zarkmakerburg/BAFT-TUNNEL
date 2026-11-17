# ADR-0045: Fleet — multi-node inventory and control (F0 design)

Status: **Proposed, amended after independent design review** (2026-11-17; findings in [ADR-0045-security-review.md](ADR-0045-security-review.md)). Roadmap: Fleet (after 0.4.0 Policy Pack; this ADR is design only). Supersedes the direction text of draft PR #171 (its ADR-0029 number collides with the merged BAFT guided-setup ADR; #171 is left untouched). Builds on ADR-0014 (offline CA), ADR-0023 (agent control helper), ADR-0026/0027 (pairing, enrollment), ADR-0030 (no `CAP_NET_ADMIN`), ADR-0033 (panel HTTPS), ADR-0036 (tunnels per host).

## Context

Today every host has a loopback agent and a static local panel. ADR-0036 lets one panel list the tunnels of its own host, but nothing shows several hosts together, and the Servers page shows the configured peer as `Unknown` because nothing observes it. Operators with an Iran and an abroad node, or several of each, must open each panel separately.

Constraints that already bind (not re-argued here): Rust core, userspace only, no `CAP_NET_ADMIN`, node private keys never leave the node, the CA key stays offline (ADR-0014), no trust-on-first-use, no arbitrary remote command, Unknown is shown whenever a value is not measured, the data plane never depends on the management plane.

## Decision

Fleet is built in slices, each separately reviewable and each shipping only what it can prove. **Read-only first; no remote mutation until F3.**

### Shape

1. **Fleet hub = an existing agent running in `hub` role**, not a new service. It serves the same panel with an added Fleet page. One hub per operator group; any node may be the hub; the hub is not required for any tunnel to forward. Rejected for now: a separate multi-tenant SaaS service (adds a database, accounts and a new trust anchor before the single-owner need is proven).
2. **Outbound reporting, not inbound control.** A member node's agent opens an outbound HTTPS connection to the hub and pushes typed, versioned, size-bounded **reports**. The hub never needs a public management port on members, and members need no inbound port beyond what the tunnel already uses.
3. **Separate Fleet identity.** The report channel uses its own client certificate (profile `fleet-node`, EKU distinct from the tunnel profile), issued by the same offline CA through the existing CSR flow (ADR-0027). A tunnel certificate is never accepted on the Fleet channel and the reverse. The hub pins the CA and checks the CRL exactly as tunnel peers do. No online signer, no TOFU.
4. **Enrollment = operator-mediated pairing** (as ADR-0026): the hub shows a one-time bundle (hub name, hub CA pin, expiry, single use); the member imports it in its panel and generates a CSR; the operator signs offline and installs the certificate. Revocation = CRL, same as today.

### Report types (F1, all read-only)

`node.hello.v1` (node id, agent version, instance names, engines, protocol versions), `node.status.v1` (per-instance state as the local `/v1/status` already derives it, tunnel state, `last_verified_unix`), `node.health.v1` (Integrity result and Health kept separate, as locally), `node.events.v1` (bounded list of local audit/failover events, secret-free). Each carries `seq` (monotonic per node), `sent_unix`, a body size cap, and a schema version; unknown versions are rejected, not guessed.

### Inventory and freshness

Hub keeps per node: id, display name, role (iran/abroad/other), version, last report time, last `seq`, connection state. Freshness is **derived on read** from `last report time`: `Live` (< 3× interval), `Stale` (shown with age), `Never`. A stale or never-reporting node shows its fields as Unknown, never its last value as current. Connection freshness is not tunnel readiness: a node can be Live with a Down tunnel, and a heartbeat never implies Ready.

### Authenticated control (F3 only, not before F1/F2 are accepted)

Typed jobs (`tunnel.start`, `tunnel.stop`, `tunnel.restart`, later `apply`), allow-list only, each with `job_id`, `idempotency_key`, `not_after`, target instance, and the operator identity. Delivered over the same outbound channel (the member polls or holds a stream); the member verifies hub signature, expiry and replay before running the existing local helper path (ADR-0023), so a job can do nothing a local operator could not do. Every job writes a local audit record and a receipt back. No shell, file path or payload execution. Break-glass is the local panel, which always works.

### Audit

Hub audit: operator, action, target node, job id, decision, result, time. Member audit: job id, verified-by, result. Secrets and forwarded data never logged; retention bounded and documented.

### Panel (user-visible rules)

Fleet page lists nodes with Live/Stale/Never, version, tunnel state, Integrity and Health as separate columns, last-seen with age, and an explicit empty state when no node has enrolled ("Add node" starts the pairing steps). Persian RTL first-class, an Advanced section for identity/CRL/sequence detail, no fake rows. Until F1 exists the page shows this host's own instances only and says so.

## Slices

| Slice | Scope | Accept when |
|---|---|---|
| F0 | This ADR + independent security review + owner acceptance | Review has no open Critical/High |
| F1 | `fleet-node` profile, pairing bundle, outbound reports, hub inventory, Fleet page (read-only) | Unauthenticated/wrong-profile/revoked/replayed nodes rejected in tests; no new inbound port on members; staleness tests; two-host LAB read-only |
| F2 | Desired-state preview/diff per node | Invalid/stale/conflicting state rejected; no side effects |
| F3 | Typed jobs above | Replay/idempotency/expiry tests, rollback, audit receipts, two-host LAB |
| F4 | History and alerts (builds on B5) | Freshness visible, retention verified |
| F5 | Hub outage and reconnect | Tunnels keep forwarding through hub outage; reconnect never replays a mutation |

## Local-first slice shipped ahead of F1

Independent of the hub, the panel's Fleet page can show **this host's instances** from the existing `GET /v1/host/tunnels` (ADR-0036) and the configured peer as `Unknown / not observed`. This adds no protocol, no secret and no claim about remote nodes.

## Open questions for the security review

1. Reuse of the tunnel CA for the `fleet-node` profile vs. a separate Fleet CA (default here: same CA, distinct profile and pin).
2. Hub-signed jobs: key custody for the hub signing key (default: hub-local key file, mode 0600, rotated by re-pairing; offline signing deferred).
3. Single-owner authority only for F1–F3; roles are out of scope.
4. Hub on the Iran side may reach members abroad only through paths that exist; behaviour when only one direction is open (default: member-initiated only, so the abroad node must reach the hub; document the case where it cannot).

## Consequences

Fleet adds a role and a channel, not a service and not a CA. The local panel stays the emergency path. Nothing here is implemented or LAB-verified; no slice is claimed shipped by this ADR.

## Amendments from the design review (binding for F1)

1. **Hub signer (F-1).** The job-signing key is certified under its own `fleet-hub-signer` profile by the offline CA and is revocable by CRL; members reject jobs from an uncertified or revoked signer. Per-member job rate limits apply. The hub never signs `apply` jobs until a separate review.
2. **Hub identity (F-2).** The hub serves under a `fleet-hub` server profile; the pairing bundle binds the hub name and members check it. A tunnel or `fleet-node` certificate is never accepted as hub.
3. **Job binding (F-3).** Each job signs the target node id and instance, a short validity window with bounded clock skew, and the replay cache is persisted on disk.
4. **One EKU per profile (F-4).** Every profile has one dedicated EKU OID; a certificate carrying more than one BAFT EKU is rejected.
5. **Node identity (F-5, F-6).** The hub takes node id and role from the client certificate, never from the report body; `seq` restarts are accepted only with a new certificate or an explicit operator reset, shown in the audit.
6. **Enrollment (F-7).** The CSR is bound to the bundle nonce, and its fingerprint is confirmed out of band before the operator signs.
7. **CRL (F-8).** A past-due CRL fails closed for jobs and is shown as a warning for reports.
8. **Bounds (F-9).** F1 states numeric limits (report size, rate, connections) and treats the hub listener as a new inbound port that needs HTTPS front-end hardening per ADR-0033.
9. **Exposure (F-10).** Fleet inventory is shown only to the authenticated operator and is never published; it lists no data it does not need.
10. **Self-lockout (F-11).** A `stop` or `restart` job that could cut the member's path to the hub carries an automatic revert if the member cannot reach the hub within a timeout.
11. **Audit (F-12).** Members sign job receipts and keep a hash-chained local audit; the hub audit is not the only record.
12. **Display (F-13).** Names and event text from members are escaped and stripped of bidi control characters before display.
13. **Accepted as written:** reusing the tunnel CA with the profiles above (F-14), member-initiated-only connections with the one-direction case documented (F-15), the hub as a single point of failure because tunnels never depend on it, tested in F5 (F-16).

The review is an independent design review, not a pre-stable external review. F1 stays blocked until the owner accepts this amended ADR.
