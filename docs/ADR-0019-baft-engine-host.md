# ADR-0019: BAFT main engine as a separately packaged Go engine host

- Status: Proposed (owner delegated engineering decisions on 2026-11-07; merge with the design doc)
- Date: 2026-11-08
- Relates to: ADR-0003, ADR-0013, ADR-0015, ADR-0017, ADR-0018
- Evidence and details: `docs/BAFT_ENGINE_INTEGRATION_DESIGN.md`

## Context
BAFT (Go, AGPL-3.0) owns its own multiplexing, routes, recovery and listeners, and has no external control API. The Rust `StreamEngine` trait only secures one socket. ADR-0018 lists `baft` as engine #3 and requires Go to be confined to a process.

## Decision
1. Run BAFT as a separate, pinned, hash-verified Go process (one systemd instance per tunnel). Rust renders its configuration from BAFT-TUNNEL desired state, supervises it and probes it. Payload never crosses a Rust/Go IPC.
2. Add a `ManagedEngine` concept beside `StreamEngine`. `tcp-tls` and `noise-tcp` are unchanged.
3. Only BAFT's `h2` mTLS carrier is exposed in the MVP. BAFT's `ws` and `utls` carriers stay hidden until roadmap engines 4 and 5, so the approved order holds.
4. This narrows ADR-0018 clause 4 for this engine: authorization is enforced by BAFT's own fail-closed code from a Rust-generated config. Rust verifies with negative probes (wrong identity, unauthorized route, revoked cert) and never reports `authorized` or `healthy` without a fresh probe. If this is later rejected, fall back to a carrier-only sidecar (design doc, option A2).
5. Licence: BAFT stays AGPL-3.0 as a separate binary with a visible source link and exact source tag; BAFT-TUNNEL Rust stays proprietary and never links BAFT code. No relicensing without contributor consent and its own ADR.
6. No BAFT secrets, and no BAFT control plane (BCC, agent, installer, ingress), are imported.

## Consequences
- A Go crash resets active flows (upstream has no process-restart resume).
- Key files must be materialised as 0600 for the engine user (BAFT rejects group-readable keys).
- A CRL to BAFT revocation-list converter is required and must fail closed.
- Release gets an engine manifest (tag, source SHA, binary sha256, licences, source URL).
