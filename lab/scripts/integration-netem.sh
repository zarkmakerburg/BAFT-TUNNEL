#!/usr/bin/env bash
# Executes only inside the isolated Docker Compose LAB network.
set -Eeuo pipefail
root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
compose_file="$root/lab/compose.yaml"

cleanup() {
  local result=0
  if ! sh "$root/lab/scripts/clear-netem.sh"; then
    echo "ERROR: failed to verify/remove LAB netem qdisc" >&2
    result=1
  fi
  if ! docker compose -f "$compose_file" down --remove-orphans --volumes; then
    echo "ERROR: failed to remove isolated Docker Compose LAB resources" >&2
    result=1
  fi
  return "$result"
}

finish() {
  local original_status=$?
  trap - EXIT HUP INT TERM
  if ! cleanup; then
    original_status=1
  fi
  exit "$original_status"
}
trap finish EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

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
