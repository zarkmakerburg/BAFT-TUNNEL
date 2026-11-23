#!/usr/bin/env bash
set -Eeuo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
source "$ROOT/scripts/systemd-unit-inventory.sh"

# Fake systemctl mirroring the two behaviours that matter here:
#  * the inventory calls it without unit-name patterns, so a listing exits 0 even
#    when nothing matches (real systemd exits 1 for an unmatched pattern, which is
#    not a failure and must not abort uninstall);
#  * every listing can be made to fail, and that must fail the inventory closed.
systemctl() {
  local sub=$1 arg
  shift
  for arg in "$@"; do
    case "$arg" in
      *'*'*)
        echo "fake systemctl: inventory must not ask for unit patterns ($arg)" >&2
        return 90
        ;;
    esac
  done
  local empty="${FAKE_SYSTEMCTL_EMPTY:-0}"
  local fail="${FAKE_SYSTEMCTL_FAIL:-}"
  local all_args=" $* "
  case "$sub" in
    list-units)
      if [[ "$all_args" == *" --state=active "* ]]; then
        [[ "$fail" == "list-units-active" ]] && return 73
        [[ "$empty" == "1" ]] ||
          printf '%s\n' 'baft-tunnel@active.service loaded active running BAFT tunnel'
      elif [[ "$all_args" == *" --all "* ]]; then
        [[ "$fail" == "list-units-all" ]] && return 71
        [[ "$empty" == "1" ]] ||
          printf '%s\n' \
            'baft-tunnel-agent@alpha.service loaded active running BAFT agent' \
            'baft-tunnel-control-manage@alpha.socket loaded active listening BAFT manage socket'
      else
        echo "fake systemctl: unexpected list-units arguments: $*" >&2
        return 98
      fi
      ;;
    list-unit-files)
      if [[ "$all_args" == *" --type=service "* ]]; then
        [[ "$fail" == "list-unit-files-tunnel" ]] && return 74
        [[ "$empty" == "1" ]] ||
          printf '%s\n' \
            'baft-tunnel@enabled.service enabled enabled' \
            'baft-tunnel@disabled.service disabled enabled' \
            'baft-tunnel@static.service static -'
      else
        [[ "$fail" == "list-unit-files-agent" ]] && return 72
        [[ "$empty" == "1" ]] ||
          printf '%s\n' \
            'baft-tunnel-agent@beta.service enabled enabled' \
            'baft-tunnel-agent@gamma.service disabled enabled' \
            'baft-tunnel-agent@delta.service static -'
      fi
      ;;
    *)
      echo "fake systemctl: unexpected invocation: $sub $*" >&2
      return 99
      ;;
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

# No matching units at all (the common uninstall path) is an empty, successful inventory.
[[ -z "$(FAKE_SYSTEMCTL_EMPTY=1 baft_list_agent_control_units)" ]] || {
  echo "agent/control inventory must be empty and succeed when nothing matches" >&2
  exit 1
}
[[ -z "$(FAKE_SYSTEMCTL_EMPTY=1 baft_list_tunnel_instance_units)" ]] || {
  echo "tunnel inventory must be empty and succeed when nothing matches" >&2
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

echo "UNINSTALL_SYSTEMD_INVENTORY=PASS (empty listings succeed; both inventories fail closed; disabled/static excluded)"
