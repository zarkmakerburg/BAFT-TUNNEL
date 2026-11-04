#!/bin/sh
set -eu
script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
compose_file="$script_dir/../compose.yaml"

# A stopped project has no live qdisc to remove. Docker/Compose errors must
# remain visible; never turn a failed inspection or delete into a false PASS.
running_services=$(docker compose -f "$compose_file" ps --status running --services)
for node in endpoint-a endpoint-b; do
  if printf '%s\n' "$running_services" | grep -Fxq "$node"; then
    qdisc=$(docker compose -f "$compose_file" exec -T "$node" tc qdisc show dev eth0)
    if printf '%s\n' "$qdisc" | grep -q 'netem'; then
      docker compose -f "$compose_file" exec -T "$node" tc qdisc del dev eth0 root
      qdisc=$(docker compose -f "$compose_file" exec -T "$node" tc qdisc show dev eth0)
      if printf '%s\n' "$qdisc" | grep -q 'netem'; then
        echo "netem qdisc remains on $node after delete" >&2
        exit 1
      fi
    fi
  fi
done
echo "Verified no netem qdisc remains on running LAB endpoints."
