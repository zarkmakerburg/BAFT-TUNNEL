# ADR-0020: Generic managed-runtime host and engine runtime manifest

Status: accepted (owner mission "EXECUTION MISSION - BAFT ENGINE #3", 2026-11-08)
Builds on ADR-0018 (engine framework) and ADR-0019 (BAFT engine host).

## Decision

1. Rust stays the core. Engines written in other languages (Go first) run as separate,
   hash-pinned child processes. Go is a supported engine language, not a replacement.
2. `src/runtime_host.rs` is the generic host. It knows only: verify a pinned artifact,
   run an optional validate step, start the child with an empty environment and no stdin,
   probe readiness, restart within a bound, stop with SIGTERM then SIGKILL. It does not
   know any engine's protocol or configuration, and tunnel payload never crosses it.
3. Each engine owns an adapter that builds a `ChildSpec` (binary, validate and run
   arguments, readiness address, restart budget). `baft_engine::child_spec` is the first.
   A future Go engine adds an adapter and a manifest entry; the host, supervisor and core
   do not change.
4. `release/engine-manifest.json` (schema_version 1, strict, `deny_unknown_fields`) records
   per engine: id, runtime language, version, source repository, exact 40-hex commit,
   artifact path and SHA-256, platforms, config adapter, required capabilities, readiness
   contract, resource policy and licence metadata. The set is static; there is no dynamic
   loading and no marketplace.
5. A running process is not a healthy tunnel. The host reports `Ready` only after the
   readiness probe succeeds; an exit is restarted at most `max_restarts` per window, then
   the host fails so the service manager sees the tunnel as down.

## Consequences

- The BAFT dialer that exits after the recovery retention window (risk R14) is now
  restarted by the host within its budget, instead of relying on the unit alone.
- The readiness contract is TCP accept on the local listener. It proves the engine is
  serving locally, not that the peer is connected; end-to-end health still needs the
  probes of a later phase, and BAFT stays unavailable in setup until they pass.
- Resource limits beyond the unit (`max_memory_mib`) are recorded in the manifest but not
  yet enforced by the host.
