# ADR-0040: Stream resume across transport loss (design, protocol v3)

Status: proposed, design only (2026-11-17). Relates to: ADR-0039, ADR-0028, `PROTOCOL_V2_COMPATIBILITY_MATRIX.md`, ADR-0038. Roadmap: Rakhsh integration P0-2.

## Context
When the session carrying yamux dies, every TCP stream dies. Resume would let streams continue on a new session without loss, duplication or reordering.

## Decision
1. **Protocol v3**, not an extension of v2. v2 has a fixed capability set and strict Option A; resume changes data-plane framing and the handshake transcript. v3 follows the same strict policy: v2 and v3 peers reject each other with an explicit incompatibility, no silent fallback, no downgrade.
2. Scope: TCP forward streams only. UDP/L3 not covered. Resume does **not** survive a restart of either process (streams live in memory); no restart-resume claim is allowed without persistence, which is out of scope.
3. Mechanism (generic, TCP-like): each stream direction has a byte offset; frames carry offset; receiver accepts exactly the next byte, drops duplicates, requests resend on a gap; cumulative ACK frees the sender buffer; on reconnect a `ResumeHello` exchanges (stream id, received offset, written offset) and each side resends from the peer's offset.
4. Binding: resume is valid only inside a **resume token** issued by the authenticated session (random 128-bit, bound to node identities, tunnel id and the original handshake transcript hash) and only for a new session with the same identities. The token is single-use per reconnect and expires after `resume_grace` (default 60 s). This prevents replay and session confusion.
5. Memory: hard cap per stream window (default 1 MiB, configurable up to 16 MiB) and per-tunnel total (default 64 MiB). When full, the stream applies backpressure to its local socket (stops reading); when the tunnel cap is hit, new streams are refused, existing streams keep their buffer. Nothing is dropped silently.
6. Failure: after `resume_grace` without a session, streams are closed on both sides; unknown streams in `ResumeHello` get an explicit `Gone`.
7. Authorization is re-checked on resume (a CRL entry or revoked authorization ends the streams).

## Acceptance (before any feature claim)
Property/chaos test of the offset logic with random cuts (no loss, no duplicate, in order); large transfer with session cut twice, sha256 equal; buffer-full behaviour; replayed `ResumeHello` rejected; v2<->v3 rejection tests; memory under N streams.

## Not decided here
Wire frame layout and exact limits beyond the defaults above, to be fixed in the implementation PR. Implementation starts only after ADR-0039 lands, because resume makes failback cheap.
