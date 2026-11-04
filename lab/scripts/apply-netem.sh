#!/bin/sh
set -eu
profile="${1:-moderate}"
case "$profile" in
  clean) delay="0ms"; loss="0%"; rate="1000mbit" ;;
  mild) delay="40ms"; loss="2%"; rate="100mbit" ;;
  moderate) delay="90ms"; loss="5%"; rate="50mbit" ;;
  harsh) delay="180ms"; loss="10%"; rate="10mbit" ;;
  *) echo "usage: $0 {clean|mild|moderate|harsh} [endpoint-a|endpoint-b]" >&2; exit 2 ;;
esac
target="${2:-endpoint-a}"
case "$target" in endpoint-a|endpoint-b) ;; *) echo "target must be endpoint-a or endpoint-b" >&2; exit 2 ;; esac
script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
compose_file="$script_dir/../compose.yaml"
docker compose -f "$compose_file" exec -T "$target" tc qdisc replace dev eth0 root netem delay "$delay" loss "$loss" rate "$rate"
printf 'Applied profile=%s to %s: delay=%s loss=%s rate=%s\n' "$profile" "$target" "$delay" "$loss" "$rate"
