# Troubleshooting

## Docker/netem simulation
- If Docker Compose is missing, install the Compose plugin for Docker Engine.
- If `tc` reports `Operation not permitted`, confirm the simulation containers have `NET_ADMIN`.
- Run `sh lab/scripts/clear-netem.sh` before stopping the simulation to remove qdisc impairment.
- The netem scripts target only the two Compose containers. Do not adapt them to production hosts.

## Six-host preflight
- Confirm all six host variables are present and unique.
- Confirm the dedicated SSH key is readable and the pinned `known_hosts` file is non-empty.
- For a non-default SSH port, use `[host]:port` entries in `known_hosts`.
- A refused TCP connection means the host answered but no listener accepted the port; a timeout may mean filtering or loss.
- The preflight requires Bash and Python 3 on each remote host.
