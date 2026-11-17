# ADR-0049: Direct session rotation (make before break)

Status: accepted (2026-11-17). Relates to: ADR-0039 (link policy), ADR-0041 (transport health). Gate for v0.3.0 Stable.

## Context
LAB evidence (2026-11-17): on the Iran to abroad path, a raw TLS flow (no BAFT-TUNNEL code) moves a 2 MiB echo in about 0.85 s when new, but takes 81 to 101 s (about 21 KB/s) once the flow is 60 s or older (control workflow `lab-tls-bulk-control.yml`, run 44056840174). Direct sessions are long-lived, so their 2 MB probes die near 0.65 to 0.87 MB with a yamux timeout after about 36 to 43 s. Reverse (the other side dials) is not affected. The path shapes aged flows; the product is not at fault, but it can avoid depending on one aged flow.

## Decision
1. Optional, off by default: `session_rotate_secs` on a Direct forward tunnel (0 = off, otherwise 30 to 3600). Not for Reverse and not for the baft engine.
2. After the age limit (plus up to 10 % jitter) the dialer opens a replacement session on the same link, authenticated like any session. Only after it is up does it switch: new local connections use the new session, the old session stops accepting new ones and keeps serving its in-flight streams for at most 120 s, then is closed. Make before break: no queued connection is dropped and no stream is cut.
3. If the replacement dial fails the current session keeps running and the dial is retried after 15 s. A rotation is not a link failure: link policy state is untouched.
4. The server needs no change: it already accepts concurrent sessions from one peer (`MAX_SESSIONS`).
5. Security is unchanged: same TLS/Noise engine, same pinned identity, same handshake; no plaintext path, no new privilege, no firewall, route or sysctl change.

## Limits (honest)
Rotation cannot help if the shaper throttles young flows too or if the age limit is longer than the shaper's threshold. The threshold is measured in LAB before a default is recommended. A single long stream still ages with its session; only streams opened after a rotation start fresh.

## Acceptance
Unit/integration: a stream opened before a rotation survives it and new streams work after two rotations (`session_rotation_keeps_old_streams_alive_and_serves_new_ones`). LAB: 2 MB integrity probes on sessions older than the previous failure age pass with rotation on and fail without it.

## Rollback
Set `session_rotate_secs` to 0 (the default); behaviour is then identical to before.
