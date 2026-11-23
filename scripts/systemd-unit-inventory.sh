#!/usr/bin/env bash
# Shared, fail-closed inventory of Agent, control, and tunnel units for uninstall preflight.
baft_list_agent_control_units() {
  local loaded_units enabled_units loaded_names enabled_names combined

  loaded_units="$(systemctl list-units --all --plain --no-legend 'baft-tunnel-agent@*' 'baft-tunnel-control-*')" ||
    return 1
  enabled_units="$(systemctl list-unit-files --no-legend 'baft-tunnel-agent@*')" ||
    return 1
  loaded_names="$(printf '%s\n' "$loaded_units" | awk 'NF { print $1 }')" ||
    return 1
  enabled_names="$(printf '%s\n' "$enabled_units" | awk '$2 == "enabled" { print $1 }')" ||
    return 1
  combined="$(printf '%s\n%s\n' "$loaded_names" "$enabled_names" | awk 'NF { print }' | sort -u)" ||
    return 1

  printf '%s' "$combined"
}

baft_list_tunnel_instance_units() {
  local active_units enabled_units active_names enabled_names combined

  active_units="$(systemctl list-units --state=active --plain --no-legend 'baft-tunnel@*.service')" ||
    return 1
  enabled_units="$(systemctl list-unit-files --type=service --no-legend 'baft-tunnel@*.service')" ||
    return 1
  active_names="$(printf '%s\n' "$active_units" | awk '$1 ~ /^baft-tunnel@.+\.service$/ { print $1 }')" ||
    return 1
  enabled_names="$(printf '%s\n' "$enabled_units" | awk '$2 == "enabled" && $1 ~ /^baft-tunnel@.+\.service$/ { print $1 }')" ||
    return 1
  combined="$(printf '%s\n%s\n' "$active_names" "$enabled_names" | awk 'NF { print }' | sort -u)" ||
    return 1

  printf '%s' "$combined"
}
