# ADR-0037: Reverse integrity probe and the tcp-tls Reverse Preview gate

Status: accepted (2026-11-16, owner's delegated decision authority). Roadmap: 0.3.0 O6. Builds on ADR-0021 (Amendment 1),
ADR-0028 and ADR-0030.

## Context

Reverse for tcp-tls (ADR-0021 Amendment 1) has a real data path: the service node dials out, the public node accepts users and
opens one stream per user over that connection. It passed a two-host LAB (run 42506805986) and a Preview acceptance on the
installed product path (run 42619848756, `docs/REVERSE_ACCEPTANCE.md`). Later the capability contract was closed again
(`reverse.state: unavailable`) because the integrity probe (ADR-0028) had only Direct evidence, so a Reverse tunnel could never
show an integrity result, and no independent acceptance had been run on the current protocol (v2 default since 0.3.0 O1).

In Reverse the yamux roles are swapped relative to Direct: the public node runs the user-ingress loop on the yamux **server**
side and the service node runs the stream server on the yamux **client** side. The probe code paths are shared by both loops.

## Decision

1. **Probe semantics are unchanged.** Same `IntegrityProbe` stream capability, same 64 KiB payload, same SHA-256 check, same
   `ProbeLimiter` and protocol-v2 requirement as Direct. Either node may start a probe through its own agent; the other node
   answers. The resource stays `integrity_probe/tunnel/T/peer/P/direction/reverse`.
2. **Responder rules per node.**
   - Service node (dialer, holds the target): accepts `TcpForward` and `IntegrityProbe` streams from the public node, as the
     Direct `serve` node does. A probe never connects to the target.
   - Public node (listener, user ingress): accepts only `IntegrityProbe` from the peer. A peer-opened `TcpForward` is refused,
     so a compromised service node cannot make the public node open connections.
3. **Direction binding.** The stream-open authorization binds the agreed direction label. A Reverse probe against a peer bound
   to Direct (or the reverse) is refused before any payload is read.
4. **Gate (capabilities, preflight, wizard, Apply).** tcp-tls Reverse is offered as **Preview** only after an independent
   two-host acceptance run on the exact commit that opens the gate. That run, done by the LAB/B5 owner and not by the
   implementing thread, must add to the ADR-0021 checks: an in-product integrity probe started from **each** node with a
   `pass` audit entry, and the protocol-v2 handshake on both nodes. noise-tcp and baft Reverse stay unavailable.
   Trusted LAB workflows only build commits already in `main`, and Apply refuses a direction the catalogue does not offer, so
   the gate-opening commit is merged to `main` first (no tag, no release) and the run uses
   `phase1-trusted-reverse-lab.yml` on that SHA. No release (rc or Stable) may contain the open gate until the run passes; if it
   fails, the gate commit is reverted before the next release.
5. **Never claimed.** Reverse is not Stable and not production-ready. A probe pass is scoped to that peer, tunnel, direction and
   time and never sets Apply `ready`. No firewall change and no CAP_NET_ADMIN (ADR-0030).

## Evidence levels

- Unit (this ADR's first change): `reverse_session_probes_work_from_the_public_and_the_service_node` and
  `reverse_probe_is_refused_by_a_peer_bound_to_the_direct_label` in `src/tunnel.rs`, with the real Reverse yamux roles.
- LAB: pending (point 4). CI evidence is not LAB evidence.

## Consequences

- Reverse operators get the same Test tunnel action as Direct once the gate opens.
- Open item: the public node's incoming-stream loop is bounded by yamux `max_streams` and the probe limiter, not by the
  user-stream semaphore. Acceptable because only `IntegrityProbe` is accepted there; revisit if more peer-opened capabilities
  are added.
