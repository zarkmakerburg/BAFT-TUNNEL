# P3-002: node agent and local API — execution plan

Status: in progress. Part of [PRODUCT_ROADMAP.md](PRODUCT_ROADMAP.md). Reuses the config model and `support_report` from P3-001.

## Decision: no new web framework for A1
A1 is three read-only routes, so a small hand-written HTTP/1.1 handler on tokio (already a dependency) avoids a new dependency and its audit. If A2 needs request bodies, routing or TLS, adopt a framework through its own dependency review.

## Slices
| Slice | Content | Acceptance |
|---|---|---|
| A1 | `agent` (read-only): `GET /v1/health` (open), `GET /v1/status` (bearer token). Loopback only, GET only, request size/time/connection limits. `agent-token` creates a 0600 token, never overwrites. Status = redacted support report plus `listener_accepting` probe. | Unit tests: 401 without or with wrong token, redaction (no key path or peer address in output), non-loopback listen refused, token mode and overwrite checks; CI green. Does not start or stop a tunnel. |
| A2 | Validated desired state (`--state-dir`): `PUT /v1/desired` validates with `RuntimeConfig::validate`, writes `desired.json` atomically (0600), appends `audit.log` (timestamp, result, body SHA-256, reason; no config content). `GET /v1/desired` returns the redacted report. The agent never applies a config; applying stays the explicit `apply --confirm`. | Invalid config refused and unchanged on disk; audit entry per change; tests. |
| A3 | LAB (setup stage): tcp-tls pair configured from real `baft-pki.sh` output (runner plays the offline CA); agent on the serving host reports the live listener and stores a validated desired state without applying it. Engine switch stays a CLI action (the agent never applies). | `SETUP_RESULT` and `AGENT_RESULT` lines, run ID and SHA recorded. |

## Out of scope
Panel (P4-001), remote (non-loopback) access, multi-user auth, central control plane.
