#!/usr/bin/env bash
set -Eeuo pipefail
usage() {
  cat <<'EOF'
Usage: uninstall.sh [--destdir ROOT] [--stop] [--purge-config --yes]

Removes the binary, the menu scripts (/usr/local/lib/baft-tunnel), the engine
store (/usr/local/lib/baft-tunnel-engines), the systemd
templates, the agent units and the control helper.
Config, node-key credentials (/etc/baft-tunnel/credentials), their instance
drop-ins, state, and the agent token and state (/var/lib/baft-tunnel-agent)
are retained unless --purge-config --yes is explicitly supplied. The service
accounts baft-tunnel and baft-tunnel-agent are always retained.
Running/enabled instances must be stopped with --stop first.
EOF
}
die() { echo "uninstall: $*" >&2; exit 2; }
SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/systemd-unit-inventory.sh"
ROOT_PREFIX=""
STOP=0
PURGE=0
YES=0
while (($#)); do
  case "$1" in
    --destdir) (($# >= 2)) || die "--destdir needs a path"; ROOT_PREFIX="$2"; shift 2 ;;
    --stop) STOP=1; shift ;;
    --purge-config) PURGE=1; shift ;;
    --yes) YES=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) usage; die "unknown argument: $1" ;;
  esac
done
if [[ "$PURGE" == "1" && "$YES" != "1" ]]; then die "--purge-config requires --yes"; fi
STAGING=0
if [[ -n "$ROOT_PREFIX" ]]; then
  [[ "$ROOT_PREFIX" != "/" ]] || die "--destdir cannot be /"
  mkdir -p "$ROOT_PREFIX"
  ROOT_PREFIX="$(cd "$ROOT_PREFIX" && pwd -P)"
  STAGING=1
else
  [[ "$(id -u)" == "0" ]] || die "run as root or use --destdir"
  command -v systemctl >/dev/null 2>&1 || die "systemd/systemctl is required"
fi

if [[ "$STAGING" == "0" ]]; then
  units_output="$(baft_list_tunnel_instance_units)" ||
    die "could not enumerate tunnel instance units; refusing to continue uninstall"
  units=()
  if [[ -n "$units_output" ]]; then mapfile -t units <<< "$units_output"; fi
  if (("${#units[@]}" > 0)); then
    [[ "$STOP" == "1" ]] || die "BAFT-TUNNEL instances exist; rerun with --stop"
    systemctl disable --now "${units[@]}"
  fi
fi
# Agent service and control helper (scripts/install-agent-control.sh): per-instance agent and path
# units, templates, tmpfiles entry and the runtime request directory belong to this installation too.
assert_unit_inactive() {
  local unit=$1 state
  if ! state="$(systemctl show "$unit" --property=ActiveState --value 2>/dev/null)"; then
    die "could not verify systemd state for $unit; refusing to uninstall manage access"
  fi
  [[ "$state" == "inactive" ]] || die "$unit is not confirmed inactive (ActiveState=${state:-unknown}); refusing to uninstall manage access"
}
if [[ "$STAGING" == "0" ]]; then
  ctl_output="$(baft_list_agent_control_units)" ||
    die "could not enumerate agent/control units; refusing to remove per-instance manage access"
  ctl=()
  if [[ -n "$ctl_output" ]]; then mapfile -t ctl <<< "$ctl_output"; fi
  if (("${#ctl[@]}" > 0)); then
    systemctl disable --now "${ctl[@]}" 2>/dev/null || systemctl stop "${ctl[@]}" 2>/dev/null || true
    # A failed stop must not be followed by removal of groups still used by an agent.
    for unit in "${ctl[@]}"; do assert_unit_inactive "$unit"; done
  fi
fi
# Preflight every per-instance group before deleting any access drop-in. If a group
# is busy or cannot be queried, leave the entire access configuration intact.
shopt -s nullglob
access_files=("$ROOT_PREFIX"/etc/systemd/system/baft-tunnel-agent@*.service.d/manage-access.conf)
manage_groups_to_delete=()
if [[ "$STAGING" == "0" ]]; then
  for access in "${access_files[@]}"; do
    unit="${access##*/baft-tunnel-agent@}"
    name="${unit%.service.d/manage-access.conf}"
    group="btm-$(printf '%s' "$name" | sha256sum | awk '{print substr($1,1,16)}')"
    group_entry="$(getent group "$group" 2>/dev/null)" || group_rc=$?
    group_rc=${group_rc:-0}
    if (( group_rc == 0 )); then
      IFS=: read -r _ _ gid members <<< "$group_entry"
      [[ -n "$gid" ]] || die "could not read per-instance group $group; refusing uninstall"
      passwd_entries="$(getent passwd)" || die "could not enumerate passwd database; refusing to uninstall per-instance group $group"
      primary="$(printf '%s\n' "$passwd_entries" | awk -F: -v gid="$gid" '$4 == gid { print $1 }')"
      [[ -z "$members" && -z "$primary" ]] || die "refusing to remove non-empty per-instance group $group"
      manage_groups_to_delete+=("$group")
    elif (( group_rc != 2 )); then
      die "could not query per-instance group $group (getent exit=$group_rc); refusing uninstall"
    fi
    unset group_rc group_entry
  done
  # Delete exactly the groups observed and validated above. Do not reinterpret a
  # later NSS lookup failure as an absent group; groupdel must confirm deletion.
  for group in "${manage_groups_to_delete[@]}"; do groupdel "$group"; done
fi
for access in "${access_files[@]}"; do
  unit="${access##*/baft-tunnel-agent@}"
  name="${unit%.service.d/manage-access.conf}"
  rm -f "$access" "$ROOT_PREFIX/etc/systemd/system/baft-tunnel-control-manage@$name.socket.d/access.conf"
  rmdir "$ROOT_PREFIX/etc/systemd/system/baft-tunnel-agent@$name.service.d" "$ROOT_PREFIX/etc/systemd/system/baft-tunnel-control-manage@$name.socket.d" 2>/dev/null || true
done
shopt -u nullglob
rm -f "$ROOT_PREFIX"/etc/systemd/system/multi-user.target.wants/baft-tunnel-control-*.path \
  "$ROOT_PREFIX"/etc/systemd/system/sockets.target.wants/baft-tunnel-control-manage@*.socket \
  "$ROOT_PREFIX"/etc/systemd/system/multi-user.target.wants/baft-tunnel-agent@*.path \
  "$ROOT_PREFIX"/etc/systemd/system/multi-user.target.wants/baft-tunnel-agent@*.service \
  "$ROOT_PREFIX"/etc/systemd/system/multi-user.target.wants/baft-tunnel@*.service \
  "$ROOT_PREFIX"/etc/systemd/system/baft-tunnel-control-*@.path "$ROOT_PREFIX"/etc/systemd/system/baft-tunnel-control-*@.service \
    "$ROOT_PREFIX"/etc/systemd/system/baft-tunnel-control-*@.socket \
  "$ROOT_PREFIX"/etc/systemd/system/baft-tunnel-agent@.path "$ROOT_PREFIX"/etc/systemd/system/baft-tunnel-agent@.service \
  "$ROOT_PREFIX/etc/tmpfiles.d/baft-tunnel-control.conf"
rm -rf "$ROOT_PREFIX/run/baft-tunnel/control" "$ROOT_PREFIX/run/baft-tunnel/manage"
# Agent listen drop-ins written by install-agent-control.sh (no secrets, no operator content).
shopt -s nullglob
for dropin in "$ROOT_PREFIX"/etc/systemd/system/baft-tunnel-agent@*.service.d/listen.conf; do
  if grep -Fqx '# Managed by BAFT-TUNNEL scripts/install-agent-control.sh' "$dropin"; then
    rm -f "$dropin"
    rmdir "$(dirname "$dropin")" 2>/dev/null || true
  fi
done
shopt -u nullglob
rm -f "$ROOT_PREFIX/usr/local/bin/baft-tunnel" "$ROOT_PREFIX/etc/systemd/system/baft-tunnel@.service"
rm -rf "$ROOT_PREFIX/usr/local/lib/baft-tunnel"
rm -rf "$ROOT_PREFIX/usr/local/lib/baft-tunnel-engines"
if [[ "$PURGE" == "1" ]]; then
  # Remove only drop-ins written by install.sh; leave any operator-owned files.
  shopt -s nullglob
  for dropin in "$ROOT_PREFIX"/etc/systemd/system/baft-tunnel@*.service.d/credentials.conf; do
    if grep -Fqx '# Managed by BAFT-TUNNEL scripts/install.sh' "$dropin"; then
      rm -f "$dropin"
      rmdir "$(dirname "$dropin")" 2>/dev/null || true
    fi
  done
  shopt -u nullglob
  rm -rf "$ROOT_PREFIX/etc/baft-tunnel" "$ROOT_PREFIX/var/lib/baft-tunnel" "$ROOT_PREFIX/var/lib/baft-tunnel-control" "$ROOT_PREFIX/var/lib/baft-tunnel-agent"
fi
if [[ "$STAGING" == "0" ]]; then systemctl daemon-reload; fi
echo "Removed binary and systemd template. Service account was retained."
if [[ "$PURGE" == "0" ]]; then
  echo "Configuration, node-key credentials, instance drop-ins, and state were retained."
  echo "Remove them only with: uninstall.sh --purge-config --yes"
else
  echo "Configuration, node-key credentials, managed drop-ins, and state were purged."
fi
