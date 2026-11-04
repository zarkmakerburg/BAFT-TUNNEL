#!/usr/bin/env bash
# Executes only inside the isolated Docker Compose LAB network.
set -Eeuo pipefail
root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
compose_file="$root/lab/compose.yaml"
cleanup() {
  sh "$root/lab/scripts/clear-netem.sh" >/dev/null 2>&1 || true
  docker compose -f "$compose_file" down --remove-orphans --volumes >/dev/null 2>&1 || true
}
trap cleanup EXIT HUP INT TERM

docker compose -f "$compose_file" build
docker compose -f "$compose_file" up -d
for node in endpoint-a endpoint-b; do
  docker compose -f "$compose_file" exec -T "$node" ip link show dev eth0 >/dev/null
done

docker compose -f "$compose_file" exec -T endpoint-a ping -c 3 -W 3 endpoint-b

sh "$root/lab/scripts/apply-netem.sh" mild endpoint-a
qdisc=$(docker compose -f "$compose_file" exec -T endpoint-a tc qdisc show dev eth0)
printf '%s\n' "$qdisc"
grep -q 'netem' <<<"$qdisc"
grep -q 'delay 40ms' <<<"$qdisc"
grep -q 'loss 2%' <<<"$qdisc"

docker compose -f "$compose_file" exec -T endpoint-b iperf3 -s -D
iperf_output=$(docker compose -f "$compose_file" exec -T endpoint-a iperf3 -c endpoint-b -t 2)
printf '%s\n' "$iperf_output"
grep -q 'sender' <<<"$iperf_output"
grep -q 'receiver' <<<"$iperf_output"

sh "$root/lab/scripts/clear-netem.sh"
qdisc=$(docker compose -f "$compose_file" exec -T endpoint-a tc qdisc show dev eth0)
if grep -q 'netem' <<<"$qdisc"; then
  echo 'netem qdisc still present after cleanup' >&2
  exit 1
fi

echo 'Docker netem integration: PASS (isolated containers only; ping + TCP iperf + qdisc cleanup)'
