# P3-001: guided `setup` CLI — execution plan

Status: plan accepted by delegation (owner, 2026-11-07: weigh options, choose, proceed). Part of [PRODUCT_ROADMAP.md](PRODUCT_ROADMAP.md).

## Why this is the top blocker
Today an operator must hand-assemble keys, a JSON config, CLI flags and (for TLS) an offline PKI. Nothing user-facing (local API, wizard, integrated acceptance) can exist before one validated code path produces a correct configuration and exchanges trust material between two hosts. Agent, API and panel all reuse this code, so it comes first.

## Decision: Noise first, TLS second
- `noise-tcp` needs no CA: each node has a static key; pairing is an exchange of two public keys. This is the simplest honest path for a non-expert and needs no new trust model.
- `tcp-tls` keeps the offline CA (ADR-0012/0017). A guided path that makes a node sign certificates would change the trust model and needs its own ADR; until then the guided TLS path consumes PKI material produced by `baft-pki.sh` (slice S3).

## Pairing bundle (public, no secrets)
A small JSON file, exchanged by any channel the operator trusts, plus a short checksum shown on both hosts to compare out of band:

```
{ "version": 1, "tunnel_id", "engine", "direction", "port",
  "node_id", "peer_hint": "<optional address>", "public": "<64 hex Noise public key>" }
```
Rules: no private key, no credential, no host address unless the operator typed it; unknown fields rejected; engine, direction and port must match on both sides; the checksum (SHA-256 over the canonical bytes, shown in groups) is verified by the operator before `join` activates anything.

## Slices
| Slice | Content | Acceptance |
|---|---|---|
| S1 | `setup plan`: from engine, direction, port, node id, tunnel id, produce the full planned config and file list without writing anything. Shared config-generation module used later by API and wizard. | Output passes `config validate`; deterministic; invalid combinations (port range, engine/direction, ids) rejected with clear errors; unit tests; no file written. |
| S2 | `setup init` and `setup join` for `noise-tcp`: generate the node key (0600, never overwrite), write config, export or import the pairing bundle, verify checksum. | Bundle contains no secret (test greps private key bytes); tampered bundle, mismatched engine/direction/port and reused tunnel id are refused; existing files never overwritten; a pair of generated configs is cross-consistent (each side's `noise_peer_public` equals the other's bundle key) and passes full runtime validation (CI); the actual tunnel between the generated configs is exercised on real hosts in S4. |
| S3 | `tcp-tls` guided path consuming `baft-pki.sh` output (CA bundle, CRL, node cert and key, pins). | Generated config validates; missing or expired CRL refused; pins and node identity checked; CI test with a temporary CA. |
| S4 | LAB stage: two clean hosts configured only through `setup`; probe passes; engine switched. | Clean host to working tunnel in at most 5 minutes with at most 3 operator commands per host; engine switch (rewrite config, restart, probe) in 60 s or less; no leftovers after cleanup; run ID and SHA recorded. |

## Out of scope
Web UI, agent, rotation supervisor, automatic CA, any online enrollment.

## Evidence rules
Each slice merges only with green CI. LAB is run once at S4 (no re-running validated stages). Results go to `PHASE1_IMPLEMENTATION_STATUS.md` with run ID and SHA; this plan and the roadmap are updated in the same PR as the change.
