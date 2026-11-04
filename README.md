# BAFT-TUNNEL

BAFT-TUNNEL is an independent TCP/UDP tunnel platform for connecting Linux servers in either direction. It is a clean project: no BAFT source code, Git history, credentials, release keys, or deployment state are imported.

**Current status:** Phase 0 — architecture and lab foundation. This is not a production-ready tunnel.

## Phase gates

| Phase | Scope | Exit evidence |
|---|---|---|
| 0 | Architecture, threat model, repository skeleton, network lab | Reviewed ADRs, threat model, reproducible netem lab, CI |
| 1 | Rust tunnel engine, authentication, CLI, installer | Interoperability, security and network tests |
| 2 | Control plane, agents, desired state, signed updates | Multi-node reconciliation and rollback tests |
| 3 | Web panel, wizard, monitoring, alerts | E2E user journeys and accessibility |
| 4 | Profiles, failover, UDP and QUIC | Measured gains under lossy/blocked paths |
| 5 | Hardening, independent security review, documentation, v1.0 | Release checklist and signed artifacts |

Each phase ends at a review gate. No production deployment, release, or phase transition occurs without owner approval.

## Design principles

- Evidence before status: stale or missing telemetry is `unknown`, never `healthy`.
- Plan before apply: all mutations are previewed, idempotent, recorded, and rollback-capable.
- Least privilege: the agent runs unprivileged and receives only narrowly scoped capabilities.
- No traffic inspection or payload logging.
- Transport plugins have explicit conformance tests.
- Network-lab simulation is labeled as simulation and never represented as a real two-host test.
- Anti-probing and fingerprint-resistance claims are bounded and documented; no claim of being undetectable.
- Mandatory [engineering guardrails](docs/ENGINEERING_GUARDRAILS.md) capture lessons from prior engineering incidents.

## Technology baseline

- Core language: Rust (engine, agent, backend/API, CLI and installer). Go is not part of the core stack.
- Async runtime: Tokio; TLS: rustls; HTTP/API: Axum.
- Web panel: React + TypeScript + Vite.
- Storage: SQLite WAL initially; PostgreSQL remains optional.

## Phase 0 network simulation lab

Requirements: Docker Engine + Docker Compose plugin.

```bash
cd lab
docker compose build
docker compose up -d
sh scripts/apply-netem.sh moderate
sh scripts/show-netem.sh
sh scripts/clear-netem.sh
docker compose down
```

The lab creates two isolated Linux endpoints and applies Linux `tc netem` impairment. It is simulation only; it does not establish an Iran-to-abroad production path.

## Six-host real-server LAB (2 Iran + 4 abroad)

A manual, read-only six-host preflight is provided. Configure the `baft-tunnel-lab` GitHub Actions Environment using [docs/REAL_HOSTS_LAB.md](docs/REAL_HOSTS_LAB.md). It inspects all six hosts and tests all eight Iran-to-abroad TCP paths. Push/PR CI runs mock-only tests and never contacts real hosts. No active install, firewall, service, or reboot operation is included in Phase 0.

Local mock tests:

```bash
bash lab/real-hosts/test-preflight-matrix.sh
sh lab/scripts/test-scripts.sh
```

## Security and repository status

The canonical repository is private. No public open-source license is granted; do not redistribute, sublicense, or reuse the code without the owner's explicit written permission. Repository visibility and access are controlled by the owner.

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md), [docs/THREAT_MODEL.md](docs/THREAT_MODEL.md), [docs/SECURITY_GUIDE.md](docs/SECURITY_GUIDE.md), [docs/REAL_HOSTS_LAB.md](docs/REAL_HOSTS_LAB.md), and [docs/ENGINEERING_GUARDRAILS.md](docs/ENGINEERING_GUARDRAILS.md).
