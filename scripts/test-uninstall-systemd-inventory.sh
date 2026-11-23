#!/usr/bin/env bash
set -Eeuo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
source "$ROOT/scripts/systemd-unit-inventory.sh"

systemctl() {
  case "$1:$2" in
    list-units:--all)
      [[ "${FAKE_SYSTEMCTL_FAIL:-}" == "list-units-all" ]] && return 71
      printf '%s\n' \
        'baft-tunnel-agent@alpha.service loaded active running BAFT agent' \
        'baft-tunnel-control-manage@alpha.socket loaded active listening BAFT manage socket'
      ;;
    list-units:--state=active)
      [[ "${FAKE_SYSTEMCTL_FAIL:-}" == "list-units-active" ]] && return 73
      printf '%s\n' \
        'baft-tunnel@active.service loaded active running BAFT tunnel'
      ;;
    list-unit-files:--type=service)
      [[ "${FAKE_SYSTEMCTL_FAIL:-}" == "list-unit-files-tunnel" ]] && return 74
      printf '%s\n' \
        'baft-tunnel@enabled.service enabled enabled' \
        'baft-tunnel@disabled.service disabled enabled' \
        'baft-tunnel@static.service static -'
      ;;
    list-unit-files:*)
      [[ "${FAKE_SYSTEMCTL_FAIL:-}" == "list-unit-files-agent" ]] && return 72
      printf '%s\n' \
        'baft-tunnel-agent@beta.service enabled enabled' \
        'baft-tunnel-agent@gamma.service disabled enabled' \
        'baft-tunnel-agent@delta.service static -'
      ;;
    *) return 99 ;;
  esac
}

expected_agent=$'baft-tunnel-agent@alpha.service\nbaft-tunnel-agent@beta.service\nbaft-tunnel-control-manage@alpha.socket'
actual_agent="$(baft_list_agent_control_units)"
[[ "$actual_agent" == "$expected_agent" ]] || {
  printf 'unexpected agent/control inventory:\n%s\n' "$actual_agent" >&2
  exit 1
}

expected_tunnel=$'baft-tunnel@active.service\nbaft-tunnel@enabled.service'
actual_tunnel="$(baft_list_tunnel_instance_units)"
[[ "$actual_tunnel" == "$expected_tunnel" ]] || {
  printf 'unexpected tunnel inventory:\n%s\n' "$actual_tunnel" >&2
  exit 1
}

for failing_command in list-units-all list-unit-files-agent list-units-active list-unit-files-tunnel; do
  case "$failing_command" in
    list-units-all|list-unit-files-agent)
      if FAKE_SYSTEMCTL_FAIL="$failing_command" baft_list_agent_control_units >/dev/null 2>&1; then
        echo "agent/control inventory unexpectedly succeeded when $failing_command failed" >&2
        exit 1
      fi
      ;;
    list-units-active|list-unit-files-tunnel)
      if FAKE_SYSTEMCTL_FAIL="$failing_command" baft_list_tunnel_instance_units >/dev/null 2>&1; then
        echo "tunnel inventory unexpectedly succeeded when $failing_command failed" >&2
        exit 1
      fi
      ;;
  esac
done

echo "UNINSTALL_SYSTEMD_INVENTORY=PASS (both inventories fail closed; disabled/static excluded)"
