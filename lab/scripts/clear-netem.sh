#!/bin/sh
set -eu
script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
compose_file="$script_dir/../compose.yaml"
for node in endpoint-a endpoint-b; do
  docker compose -f "$compose_file" exec -T "$node" tc qdisc del dev eth0 root 2>/dev/null || true
done
echo "Cleared lab netem qdiscs."
