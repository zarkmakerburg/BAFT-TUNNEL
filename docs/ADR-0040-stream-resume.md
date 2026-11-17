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

## Implementation status (2026-11-17)
`src/resume_core.rs` holds the pure core: per-stream send window with hard caps and a per-tunnel budget (backpressure, never silent drop), receiver offset logic (duplicate, overlap, gap), `resume_from` (unrecoverable / peer-ahead rejection) and single-use, identity-bound, expiring resume tokens. It includes a seeded random-cut property test (loss, duplication, cuts; output equals input). It is **not wired**: protocol v3 framing, handshake binding, `ResumeHello`/`Gone` and the v2<->v3 rejection tests do not exist yet, so no resume feature is claimed.

## Wire and integration design (2026-11-17, amendment, still no code)

Status of this section: proposed; it fixes the shape so the implementation can land in small reviewed PRs. Nothing here is implemented, and nothing may be advertised as resume until the acceptance list above passes on a real two-host LAB.

### What actually has to change
Today each proxied TCP connection is one yamux substream and its pump task owns both ends. When the session dies, the substream dies and the local socket is closed. Resume therefore needs a layer **between the local socket and the substream** that owns the replay state (`SendWindow`, `RecvState` from `resume_core.rs`) and survives the substream:

```
local socket <-> ResumableStream(stream_id, SendWindow, RecvState) <-> current substream (replaceable)
```

A stream is detached when its substream or session fails and re-attached when a new session proves the same identity. The local socket stays open for at most `resume_grace` (default 60 s); the local application sees a stall, not a reset.

### Protocol v3 (strict, no downgrade)
- New constant `PROTOCOL_V3`, new capability `ResumeStreams`; v3 capabilities are v2's plus it. v1/v2/v3 peers reject each other with an explicit incompatibility (same Option A as v2). The baft engine stays v1.
- `ServerHello` gains `resume_token` (16 random bytes, hex). It is issued per authenticated session and is only valid for the session that follows from it. `binding = SHA-256("baft-resume-v1" || client_id || server_id || tunnel_id || direction || client_nonce || server_nonce)` over the *original* handshake; the client recomputes it. A resume attempt carries the old token and the old nonces, so a captured token is useless without the matching identities and tunnel.
- Substream header for v3 TCP streams adds `stream_id` (u64, chosen by the opener, unique per tunnel lifetime) and a flag `resumable`. Data frames on a resumable stream are `[u8 kind][u64 offset][u32 len][payload]` with kinds `Data`, `Ack` (cumulative offset), `Fin`, and `Reset`. Max payload 64 KiB.
- On a new session the opener sends, on the first control substream, `ResumeHello { old_token, old_binding_nonces, streams: [(stream_id, recv_offset)] }`, bounded to 256 streams. The acceptor answers per stream `Resume { recv_offset }` or `Gone`. Each side then resends from the other's offset (`SendWindow::resume_from`), which returns `Unrecoverable`/`PeerAhead` for impossible offsets and turns them into `Gone` plus a local close.
- Authorization is re-run on the new session before any `ResumeHello` is processed (current certificate, CRL, pins), so revocation ends resumable streams. A redeemed token is consumed even on failure (`TokenBook`).

### Memory and abuse bounds
Per-stream window default 1 MiB (max 16 MiB), per-tunnel budget 64 MiB (`TunnelBudget`), at most 256 resumable streams, token book swept at `resume_grace`. A peer that never ACKs hits the window and the sender stops reading its local socket; it cannot grow memory past the budget.

### Interaction with the rest
- **Session rotation (ADR-0049):** with resume, a rotation can migrate in-flight streams to the new session instead of letting the old session drain, removing the drain window. Until then rotation keeps its make-before-break drain unchanged.
- **Failback (ADR-0039):** safe to enable only for resumable tunnels; until then it stays off by default.
- **Health:** `resume` events (attached, gone, window-full) become counters beside rotations and link switches; they do not change the quality state.

### Implementation plan (separate PRs, each with tests and exact-SHA CI)
1. `PROTOCOL_V3` and `ResumeStreams` capability with strict mutual rejection tests, off by default and not selectable in setup yet.
2. `ResumableStream` framing layer over in-memory duplex streams, with the random-cut property test extended to the async path and buffer-full backpressure tests.
3. Handshake: `resume_token`, binding, `ResumeHello`/`Resume`/`Gone`, replay and wrong-identity rejection tests.
4. Tunnel integration (detach/re-attach, grace timer) behind a config flag.
5. Chaos test (session cut twice during a large transfer, sha256 equal) in CI, then a real two-host LAB request.

Open decision for the owner/security review: whether resume is allowed for Reverse tunnels in the first release (recommendation: Direct only, like rotation).
