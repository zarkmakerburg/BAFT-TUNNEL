#!/usr/bin/env bash
# Shared, fail-closed inventory of Agent, control, and tunnel units for uninstall preflight.
#
# The listings are requested without unit-name patterns on purpose: systemctl exits 1
# when a pattern matches nothing ("no units found"), which is indistinguishable from
# a real failure and would make a healthy uninstall abort. Listing every unit keeps
# the exit status meaningful, and the filters below select exactly the units this
# installer owns.
baft_list_agent_control_units() {
  local loaded_units enabled_units loaded_names enabled_names combined

  loaded_units="$(systemctl list-units --all --plain --no-legend)" ||
    return 1
  enabled_units="$(systemctl list-unit-files --no-legend)" ||
    return 1
  loaded_names="$(printf '%s\n' "$loaded_units" |
    awk 'NF && ($1 ~ /^baft-tunnel-agent@/ || $1 ~ /^baft-tunnel-control-/) { print $1 }')" ||
    return 1
  enabled_names="$(printf '%s\n' "$enabled_units" |
    awk '$2 == "enabled" && $1 ~ /^baft-tunnel-agent@/ { print $1 }')" ||
    return 1
  combined="$(printf '%s\n%s\n' "$loaded_names" "$enabled_names" | awk 'NF { print }' | sort -u)" ||
    return 1

  printf '%s' "$combined"
}

baft_list_tunnel_instance_units() {
  local active_units enabled_units active_names enabled_names combined

  active_units="$(systemctl list-units --state=active --plain --no-legend)" ||
    return 1
  enabled_units="$(systemctl list-unit-files --type=service --no-legend)" ||
    return 1
  active_names="$(printf '%s\n' "$active_units" |
    awk '$1 ~ /^baft-tunnel@.+\.service$/ { print $1 }')" ||
    return 1
  enabled_names="$(printf '%s\n' "$enabled_units" |
    awk '$2 == "enabled" && $1 ~ /^baft-tunnel@.+\.service$/ { print $1 }')" ||
    return 1
  combined="$(printf '%s\n%s\n' "$active_names" "$enabled_names" | awk 'NF { print }' | sort -u)" ||
    return 1

  printf '%s' "$combined"
}
