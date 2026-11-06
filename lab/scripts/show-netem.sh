#!/bin/sh
set -eu
script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
compose_file="$script_dir/../compose.yaml"
for node in endpoint-a endpoint-b; do
  echo "=== $node ==="
  docker compose -f "$compose_file" exec -T "$node" tc -s qdisc show dev eth0
done
