# ADR-0045: UDP/datagram carrier (design)

**Status:** Proposed. Design only; nothing is implemented, no dependency is added.
**Date:** 2026-11-17
**Related:** [ADR-0031](ADR-0031-adaptive-traffic-policy-pack.md) decision 11, [ADR-0044](ADR-0044-engine-agnostic-policy-interface.md), [ADR-0018](ADR-0018-engine-framework.md), [ADR-0030](ADR-0030-read-only-firewall-visibility.md), [ADR-0039](ADR-0039-carrier-link-policy.md), issue [#244](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/244)

## Context

Every carrier today is one TCP connection (`tcp-tls`, `noise-tcp`, the BAFT engine over h2). Loss on it delays all Yamux streams (head-of-line blocking, research doc section 1). ADR-0031 decision 11 makes a datagram carrier a separate prerequisite for any Gaming or Trading benefit claim. This ADR fixes the constraints and the options so that an implementation ADR/PR can start; it does not choose a dependency.

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

Working recommendation: evaluate **A** first behind a feature flag; reject B unless A fails the audit or the dependency policy (`deny.toml`).

## Risks to measure, not assume

- UDP may be filtered, rate-limited or shaped on some paths. Whether it works on the Iran-to-abroad path is unknown and must be measured in LAB before any user-facing claim.
- Amplification and reflection: the listener must not answer unauthenticated datagrams with larger responses (address validation, retry token or equivalent).
- Path MTU and fragmentation; NAT rebinding; reordering.
- Interaction with the policy interface: engines declare `datagram: Supported` only after evidence (ADR-0044 decision 4).

## Acceptance (all required before Preview is announced)

Independent security review; fuzz target for the datagram parser/handshake; CI two-process test (loopback); real two-host LAB run with one-at-a-time rule, isolated and cleaned up; per-tuple benchmark against the TCP carrier with an evidence label; documented fail-closed and DoS behaviour. LAB is shared and serialized with other work.

## Consequences

- Positive: a path to remove TCP head-of-line blocking for latency-sensitive classes without privileges.
- Cost: a second transport to audit and operate; larger attack surface (unauthenticated UDP exposure).
- Risk: benefit may be nil on paths that filter UDP; the evidence gate applies.

## Rejected

- Raw sockets or kernel tunnelling (needs privileges).
- Custom congestion control without an audit.
- Silent TCP fallback.
