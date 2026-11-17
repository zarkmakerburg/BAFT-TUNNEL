# ADR-0042: `doctor`, config schema validation, and P2 design notes (quota, DNS)

Status: proposed (2026-11-17). Relates to: ADR-0039, ADR-0041, ADR-0028, ADR-0036, `READINESS_CONTRACT.md`. Roadmap: Rakhsh integration P1 and P2. Design only; no code in this ADR.

## 1. `baft-tunnel doctor` (P1)
A thin read-only front end over what already exists. Preflight (`POST /v1/setup/preflight`, `src/agent.rs`) already returns pass / warning / blocked / unknown per check and never turns unknown into pass. Doctor adds no new probing engine:
- Calls the local agent over the existing UDS/`SO_PEERCRED` path (no new port, no token on the command line).
- Aggregates: preflight checks, link states (ADR-0041 health, shown separately from Integrity evidence, ADR-0028), certificate expiry, local permissions, UDS mode and owner, resource limits, protocol compatibility.
- Output: human table by default, `--json` for machines. Result vocabulary: `PASS`, `WARNING`, `FAIL` (= existing `blocked`), `UNKNOWN`. Exit code: 0 all pass/warning, 1 any FAIL, 2 any UNKNOWN with no FAIL (so scripts cannot read unknown as healthy).
- Never changes state, never prints secrets, never runs network scans beyond the configured peer.
Acceptance: golden-output tests for each state, exit-code tests, unknown-stays-unknown test, redaction test.

## 2. Config validation (P1)
`src/config.rs` already validates. Gaps to close, each as a separate small PR: a single schema table (key, type, allowed values, direction applicability, deprecated flag) used by the parser, `doctor`, `setup plan` and docs generation; unknown keys rejected with a "did you mean" hint unless explicitly marked forward-compatible; deprecated keys warn; direction-incompatible keys rejected; docs/test drift check from the schema. New link-policy keys (ADR-0039) enter through this schema from day one.

## 3. Quota and bandwidth limits (P2, design only)
Per-tunnel byte counters up/down (the session counters already exist), period quota (day/week/month, UTC), persisted atomically to the tunnel's state dir with a schema version, restored on start. Speed limits by back-pressure only (stop reading from the local socket), never packet drop. Enforcement at the forwarding ingress of each tunnel; quota exhausted refuses new streams and ends running ones at the next chunk; alert events at 80% and 100%. Depends on ADR-0036 (one tunnel per instance) so accounting is per instance. Not in the core until a real multi-tunnel host exists in LAB.

## 4. DNS and split routing (P2, design only)
Out of scope for the tunnel engine: it would make BAFT-TUNNEL a general proxy/router. If ever wanted, it is an optional separate module with its own ADR, listener bound to loopback by default, authentication on non-loopback, resolver over the tunnel only (no leaking to the host resolver), and leak tests. No work until the owner asks for it.

## Implementation status (2026-11-17)
Implemented: `baft-tunnel doctor --config FILE [--network] [--json]` (`src/doctor.rs`), an offline, read-only check of one config file: parse (unknown keys already rejected by serde `deny_unknown_fields`), public validation, identity and protocol line, key file permissions, CA/CRL/cert presence, node certificate and CRL expiry, link list, optional single TCP connect per remote. Exit code 0 (pass/warning), 1 (any FAIL), 2 (UNKNOWN and no FAIL). Integrity evidence, link health and agent socket security are listed under `not_checked` (they need the live agent), never as results. Not done: agent-API doctor, schema table as the single reference, deprecation warnings, "did you mean" hints, docs generated from the schema.
