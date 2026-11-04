# BAFT-TUNNEL Quick Start

> Status: Phase 0 draft. There is no installable tunnel release yet.

## Run the network simulation
Requires Docker Engine and the Docker Compose plugin:

```bash
cd lab
docker compose build
docker compose up -d
sh scripts/apply-netem.sh moderate
sh scripts/show-netem.sh
sh scripts/clear-netem.sh
docker compose down
```

This is a two-container network simulation, not a real Iran-to-abroad connectivity test.

## Run the six-host read-only preflight
See [REAL_HOSTS_LAB.md](REAL_HOSTS_LAB.md). It requires owner-provided test hosts, a dedicated SSH key, and independently verified host-key pins.
