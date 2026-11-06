# ADR-0011: explicit tunnel, direction, and stream authorization

**Status:** Accepted for Phase 1 implementation
**Date:** 2026-11-06

## Context

TLS mutual authentication and certificate-to-node identity binding prove which enrolled node is connected. They do not, by themselves, grant that node permission to use a particular tunnel, connection topology, or stream capability. A claimed node ID or tunnel ID in an application handshake must never be treated as authorization merely because the TLS connection succeeded.

## Decision

- Every serve and forward configuration must explicitly declare direction: direct or reverse. Both peers must agree on the same value.
- The bounded TLS-protected session handshake binds the authenticated peer identity, expected local/peer identities, tunnel ID, direction, and capability set.
- Phase 1 currently allows exactly one stream capability: tcp-forward. Unknown, empty, or duplicate capabilities are rejected.
- Every Yamux stream begins with a bounded, versioned stream-open authorization frame. The server verifies tunnel ID, direction, and capability before connecting to its fixed destination.
- Stream authorization frames are carried inside the authenticated TLS 1.3 channel; they are not accepted on unauthenticated or plaintext transports.
- Authentication, session authorization, and per-stream authorization are separate checks. A successful TLS handshake alone never authorizes a stream.
- Unknown fields/capabilities and unsupported protocol versions fail closed. No fallback or downgrade is permitted.

## Consequences

- Direct and Reverse are explicit, mutually agreed topology directions rather than inferred from which process happens to dial.
- A peer cannot open a stream for another tunnel, use the opposite direction, or request an unimplemented capability.
- Future capabilities must be added deliberately with policy and negative tests before being enabled.
- This does not yet implement the full credential lifecycle, dynamic multi-tenant authorization policy, revocation, or administrative enrollment workflow; those remain Phase 1 acceptance work.
