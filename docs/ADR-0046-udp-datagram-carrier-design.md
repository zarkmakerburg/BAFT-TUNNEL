# ADR-0046: UDP/datagram carrier (design, accepted 2026-11-27)

**Status:** **Accepted 2026-11-27 for the design decisions below** under the owner's delegated decision
authority. Proposed 2026-11-17. **Update 2026-11-27:** gates (a), (c) and (d) are delivered — the
dependency/licence review, the feature-gated prototype harness (`src/udp_carrier.rs`, the
`udp_datagram` fuzz target and the two-process loopback test), and the loopback benchmark harness
(`src/udp_carrier_bench.rs`) with its CI artifact. The prototype adds the reviewed dependencies and
its harnesses only: no config key, CLI verb, panel control or API route exists, and no capability,
tuple, profile or release claim changes. The independent audit (b), the two-host LAB (e) and the
fail-closed/DoS write-up (f) in "Acceptance" are **not** waived by this acceptance.
**Date:** 2026-11-17 (accepted 2026-11-27)
**Related:** [ADR-0031](ADR-0031-adaptive-traffic-policy-pack.md) decision 11, [ADR-0044](ADR-0044-engine-agnostic-policy-interface.md), [ADR-0018](ADR-0018-engine-framework.md), [ADR-0030](ADR-0030-read-only-firewall-visibility.md), [ADR-0039](ADR-0039-carrier-link-policy.md), issue [#244](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/244)

## Context

Every carrier today is one TCP connection (`tcp-tls`, `noise-tcp`, the BAFT engine over h2). Loss on it delays all Yamux streams (head-of-line blocking, research doc section 1). ADR-0031 decision 11 makes a datagram carrier a separate prerequisite for any Gaming or Trading benefit claim. This ADR fixed the constraints and the options on 2026-11-17; its acceptance on 2026-11-27 chooses the dependency direction and the gate order (below), while leaving every evidence gate and every performance claim to the audit, benchmark and LAB.

## Constraints (decided)

1. **Userspace and unprivileged.** Ordinary UDP sockets only. No raw sockets, no `CAP_NET_ADMIN`, no `CAP_NET_RAW`, no tc/iptables/sysctl (ADR-0030, roadmap constraint 8). Socket options that need no privilege are allowed (buffer sizes within `rmem_max`/`wmem_max`).
2. **Same identity model as the TCP engines.** Mutual authentication bound to one enrolled peer identity (pinned fingerprint plus URI SAN or Noise static key); fail closed; tunnel and direction authorization stay in `tunnel.rs` (ADR-0018). No plaintext mode.
3. **It is an engine/carrier, not a new product mode.** Direct/Reverse detection stays before engine selection (ADR-0021). Reverse over UDP is not public until two-host acceptance, as for TCP.
4. **Preview first.** It ships as Preview with its own capability entry (`transport.udp: true`), `Unknown` until evidence exists. No Stable claim without the acceptance below.
5. **No hidden fallback.** A UDP failure never silently switches to TCP; link policy (ADR-0039) decides switching and records the cause.

## Options (to evaluate in the implementation ADR)

| Option | For | Against |
|---|---|---|
| A. QUIC (e.g. `quinn`, rustls, mTLS with the existing pinned certificates) with the datagram extension (RFC 9221) plus streams | Standard, audited protocol design; reliable streams and unreliable datagrams on one connection; congestion control and loss recovery included; reuses the PKI | Large dependency; QUIC's own congestion control sits below the policy layer (ADR-0031 decision 13: effect must be measured, may be nil); UDP/443 fingerprint is distinctive |
| B. Noise IK over UDP with a minimal reliable/unreliable framing of our own | Reuses `noise.rs` identity; small | We own replay protection, rekeying, MTU/fragmentation, loss recovery and congestion control: high audit cost and risk |
| C. DTLS 1.3 | Standard | Rust ecosystem support is weak and unmaintained options dominate; not recommended |

Working recommendation at proposal time: evaluate **A** first behind a feature flag; reject B unless A
fails the audit or the dependency policy (`deny.toml`). The 2026-11-27 acceptance below adopts this
recommendation as the decision.

## Decision (accepted 2026-11-27)

The options above were left open on 2026-11-17. Accepting the design closes the choices that can be
made without an audit, a benchmark or a LAB host, and leaves the rest explicitly to those gates.

1. **Option A — QUIC — is the implementation.** `quinn` + `rustls`, reusing the existing pinned
   certificates and identity, with RFC 9221 datagrams beside QUIC streams. It sits behind a new,
   **non-default** cargo feature `udp-carrier`; a default build gains no dependency, no UDP socket
   and no listener. Option B (Noise-over-UDP with our own framing) is not developed in parallel: it
   is reconsidered only if A fails the dependency/licence review, the independent audit or the LAB,
   and that returns to this ADR instead of proceeding silently. Option C (DTLS 1.3) is rejected.
2. **One connection, two planes.** Handshake, peer authentication, keepalive, capability and policy
   negotiation use QUIC streams; latency-sensitive payload uses QUIC datagrams with a small fixed
   header (traffic class, flow id, sequence, length). The application never fragments; the stream
   plane carries anything that needs reliability, and the caller chooses the plane explicitly.
3. **Identity and authorization do not fork.** The same enrolled peer identity, pin and URI SAN /
   Noise static-key model applies, and `tunnel.rs` keeps Direct/Reverse detection and direction
   authorization (ADR-0018, ADR-0021). There is no plaintext datagram mode and no second identity
   store. A datagram can only arrive on a connection that completed the same handshake.
4. **Anti-amplification and DoS are design constraints, not a later hardening item.** No response
   larger than the triggering datagram before address validation; a retry token (or equivalent) on
   the handshake; bounded per-peer state and handshake concurrency; a listener answers only
   well-formed QUIC and drops everything else without per-source allocation an attacker can exhaust.
   The datagram parser gets a fuzz target as an acceptance item.
5. **MTU, loss and ordering semantics are fixed here.** The design assumes a 1200-byte minimum path
   MTU; QUIC path-MTU discovery is used where the dependency provides it, otherwise datagrams stay
   within 1200 bytes. Datagrams are unreliable and unordered — no retransmission, no application
   reordering, losses counted rather than hidden — and "no head-of-line blocking" remains a property
   to **measure**, not a claim this ADR makes.
6. **No hidden fallback.** A failed UDP path never becomes TCP silently; ADR-0039's link policy
   decides switching and records the cause (constraint 5 above).
7. **Capability and claims stay Unknown.** `transport.udp` and the engine's `datagram` capability
   stay `Unknown`/`Unsupported` until the acceptance evidence exists (ADR-0044 decision 4), and the
   status object keeps its `unsupported` datagram rows until a real carrier is measured. This ADR
   adds no `policy` field, changes no tuple claim and accepts no profile.
8. **No selection surface before Preview.** No panel control, CLI verb, config key or API route
   selects the carrier until the acceptance gates below pass. When it ships it is Preview with its
   own capability entry (constraint 4), not a Policy Pack profile and not a Stable transport.
9. **Evidence gates, in this order.** (a) dependency/licence review against `deny.toml` and the
   project's dependency policy; (b) an independent security review of this design and its threat
   model before anything beyond a prototype merges; (c) the fuzz target plus a CI two-process
   loopback test; (d) a per-tuple benchmark against the TCP carrier, labelled by evidence level;
   (e) a real two-host LAB run, one at a time and cleaned up, with Direct first and Reverse
   separately, including loss/jitter and NAT rebinding; (f) documented fail-closed and DoS
   behaviour. Preview is announced only when all six pass. (a), (c) and (d) are repo-only; (b) needs
   an independent reviewer and (e) the registered LAB hosts, neither of which this repository can
   supply by itself.
10. **What this acceptance still does not decide** — each needs its own evidence or ADR: QUIC
    congestion-control tuning and whether it is exposed at all (ADR-0031 decision 13: its effect may
    be nil); the mapping of the five profile rows onto datagram classes (ADR-0044); any UDP
    performance, filtering or censorship-resistance claim (the roadmap's Iran-path research applies);
    default-on; Stable; and release placement.

## Risks to measure, not assume

- UDP may be filtered, rate-limited or shaped on some paths. Whether it works on the Iran-to-abroad path is unknown and must be measured in LAB before any user-facing claim.
- Amplification and reflection: the listener must not answer unauthenticated datagrams with larger responses (address validation, retry token or equivalent).
- Path MTU and fragmentation; NAT rebinding; reordering.
- Interaction with the policy interface: engines declare `datagram: Supported` only after evidence (ADR-0044 decision 4).

## Acceptance (all required before Preview is announced)

These are gates (a)-(f) of the decision above; accepting this ADR does not shorten or waive any of
them: independent security review; fuzz target for the datagram parser/handshake; CI two-process test
(loopback); real two-host LAB run with one-at-a-time rule, isolated and cleaned up; per-tuple
benchmark against the TCP carrier with an evidence label; documented fail-closed and DoS behaviour.
LAB is shared and serialized with other work.

## Consequences

- Positive: a path to remove TCP head-of-line blocking for latency-sensitive classes without privileges. Until the acceptance gates pass, the honest product answer to "does UDP help?" stays **Unknown**, and Gaming/Trading keep only their tunnel-internal queueing claims on TCP.
- Accepted scope: an implementation may now start behind `udp-carrier`, in the order (a), (c), (d) — dependency/licence review, fuzz and CI loopback, benchmark. Nothing user-facing and no Preview label is authorised before (b) and (e) as well.
- Implementation note (2026-11-27): (a) is the [dependency/licence review](TP-U_DEPENDENCY_AND_LICENCE_REVIEW.md); (c) is the prototype harness — a bounded datagram frame and hello with QUIC helpers, a fuzz target, and a CI-enforced two-process loopback test that completes a real handshake and one datagram round-trip; (d) is `src/udp_carrier_bench.rs`, which compares the real datagram session with the product's TLS 1.3 + `tunnel::yamux_config` stream carrier for 64 B and 1024 B payloads, writes a `baft-udp-carrier-bench/1` artifact in CI, and enforces only echo integrity and conservative ratio bounds. Gate (c) proves the parsers refuse malformed input without crashing and that a loopback session works; gate (d) reports loopback latency percentiles and echo throughput (release run 2026-11-27: UDP median 1.5-1.9x slower than TCP; no benefit shown) and proves the harness and its CI artifact work. Neither gate measures head-of-line blocking under competing load, real paths, filtering or NAT rebinding, so nothing here supports a performance claim; (b), (e) and (f) remain open.
- Cost: a second transport to audit and operate; larger attack surface (unauthenticated UDP exposure).
- Risk: benefit may be nil on paths that filter UDP; the evidence gate applies.

## Rejected

- Raw sockets or kernel tunnelling (needs privileges).
- Custom congestion control without an audit.
- Silent TCP fallback.
