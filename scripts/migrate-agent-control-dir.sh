#!/usr/bin/env bash
# Host-wide, fail-closed migration of the root control request directory.
set -Eeuo pipefail

CONTROL_DIR=/run/baft-tunnel/control
TMPFILES=/etc/tmpfiles.d/baft-tunnel-control.conf
STATE_DIR=/var/lib/baft-tunnel-control
while (($#)); do
  case "$1" in
    --control-dir) (($# >= 2)) || exit 2; CONTROL_DIR=$2; shift 2 ;;
    --tmpfiles) (($# >= 2)) || exit 2; TMPFILES=$2; shift 2 ;;
    --state-dir) (($# >= 2)) || exit 2; STATE_DIR=$2; shift 2 ;;
    *) echo "agent-control-migration: unknown argument: $1" >&2; exit 2 ;;
  esac
done
die() { echo "agent-control-migration: $*" >&2; exit 2; }
EXPECTED='root:baft-tunnel-agent 770'
VERBS=(start stop enable disable setup pki)
JOURNAL="$STATE_DIR/migration-v1.units"

[[ ! -L "$STATE_DIR" ]] || die "state directory must not be a symlink"
mkdir -p "$STATE_DIR"
chmod 0700 "$STATE_DIR"
[[ -d "$STATE_DIR" && ! -L "$STATE_DIR" ]] || die "invalid migration state directory"
[[ ! -L "$JOURNAL" ]] || die "migration journal must not be a symlink"
exec 9>"$STATE_DIR/migration.lock"
flock -n 9 || die "another control-directory migration is already running"

directory_state() { stat -c '%U:%G %a' "$CONTROL_DIR" 2>/dev/null || true; }
has_pending_requests() {
  local verb request
  [[ -d "$CONTROL_DIR" ]] || return 1
  for verb in "${VERBS[@]}"; do
    for request in "$CONTROL_DIR"/*."$verb"; do
      [[ -e "$request" || -L "$request" ]] && return 0
    done
  done
  return 1
}
active_units() {
  local kind=$1 pattern=$2
  systemctl list-units --type="$kind" --state=active --no-legend --plain "$pattern" |
    awk 'NF { print $1 }'
}
root_helpers_active() {
  [[ -n "$(active_units service 'baft-tunnel-control-*@*.service')" ]]
}
valid_unit() {
  case "$1:$2" in
    control:baft-tunnel-control-(start|stop|enable|disable|setup|pki)@[A-Za-z0-9][A-Za-z0-9_-]*.path) [[ "$2" =~ ^baft-tunnel-control-(start|stop|enable|disable|setup|pki)@[A-Za-z0-9][A-Za-z0-9_-]{0,62}\.path$ ]] ;;
    agent_path:baft-tunnel-agent@[A-Za-z0-9][A-Za-z0-9_-]*.path) [[ "$2" =~ ^baft-tunnel-agent@[A-Za-z0-9][A-Za-z0-9_-]{0,62}\.path$ ]] ;;
    agent_service:baft-tunnel-agent@[A-Za-z0-9][A-Za-z0-9_-]*.service) [[ "$2" =~ ^baft-tunnel-agent@[A-Za-z0-9][A-Za-z0-9_-]{0,62}\.service$ ]] ;;
    *) return 1 ;;
  esac
}
declare -a CONTROL_PATHS=() AGENT_PATHS=() AGENT_SERVICES=()
load_journal() {
  local kind unit
  while IFS=$'\t' read -r kind unit; do
    [[ -n "$kind" && -n "$unit" ]] || die "malformed migration journal"
    valid_unit "$kind" "$unit" || die "invalid unit in migration journal: $kind $unit"
    case "$kind" in
      control) CONTROL_PATHS+=("$unit") ;;
      agent_path) AGENT_PATHS+=("$unit") ;;
      agent_service) AGENT_SERVICES+=("$unit") ;;
    esac
  done < "$JOURNAL"
}
unique_append() {
  local target=$1 unit=$2 item
  local -n arr="$target"
  for item in "${arr[@]}"; do [[ "$item" == "$unit" ]] && return 0; done
  arr+=("$unit")
}
snapshot_current_units() {
  local unit
  while IFS= read -r unit; do [[ -n "$unit" ]] && unique_append CONTROL_PATHS "$unit"; done < <(active_units path 'baft-tunnel-control-*@*.path')
  while IFS= read -r unit; do [[ -n "$unit" ]] && unique_append AGENT_PATHS "$unit"; done < <(active_units path 'baft-tunnel-agent@*.path')
  while IFS= read -r unit; do [[ -n "$unit" ]] && unique_append AGENT_SERVICES "$unit"; done < <(active_units service 'baft-tunnel-agent@*.service')
}
write_journal() {
  local tmp="$JOURNAL.tmp.$$" unit
  ( umask 077
    : > "$tmp"
    for unit in "${CONTROL_PATHS[@]}"; do printf 'control\t%s\n' "$unit" >> "$tmp"; done
    for unit in "${AGENT_PATHS[@]}"; do printf 'agent_path\t%s\n' "$unit" >> "$tmp"; done
    for unit in "${AGENT_SERVICES[@]}"; do printf 'agent_service\t%s\n' "$unit" >> "$tmp"; done
  )
  chmod 0600 "$tmp"
  mv -f "$tmp" "$JOURNAL"
}
stop_units() {
  local label=$1; shift
  local unit stop_rc state failed=0
  for unit in "$@"; do
    stop_rc=0
    systemctl stop "$unit" || stop_rc=$?
    state="$(systemctl is-active "$unit" 2>/dev/null || true)"
    case "$state" in
      inactive|failed) ;;
      *)
        echo "agent-control-migration: could not quiesce $label $unit (stop_rc=$stop_rc, state=${state:-unknown})" >&2
        failed=1
        ;;
    esac
  done
  return "$failed"
}
restore_units() {
  local label=$1; shift
  local unit start_rc state failed=0
  for unit in "$@"; do
    start_rc=0
    systemctl start "$unit" || start_rc=$?
    state="$(systemctl is-active "$unit" 2>/dev/null || true)"
    if [[ "$state" != active ]]; then
      echo "agent-control-migration: could not restore $label $unit (start_rc=$start_rc, state=${state:-unknown})" >&2
      failed=1
    fi
  done
  return "$failed"
}

if [[ -f "$JOURNAL" ]]; then
  load_journal
else
  root_helpers_active && die "a root control helper is active; retry after it finishes"
  has_pending_requests && die "pending control request exists; inspect it as root before migration"
  snapshot_current_units
  write_journal
fi

if [[ "$(directory_state)" != "$EXPECTED" ]]; then
  root_helpers_active && die "a root control helper is active; journal retained and migration stopped"
  has_pending_requests && die "pending control request exists; journal retained and migration stopped"
  # Include anything that became active after the durable snapshot.
  snapshot_current_units
  write_journal
  STOP_FAILURE=0
  stop_units "root control watcher" "${CONTROL_PATHS[@]}" || STOP_FAILURE=1
  stop_units "agent path" "${AGENT_PATHS[@]}" || STOP_FAILURE=1
  stop_units "agent service" "${AGENT_SERVICES[@]}" || STOP_FAILURE=1
  if (( STOP_FAILURE )); then
    for unit in "${CONTROL_PATHS[@]}" "${AGENT_PATHS[@]}" "${AGENT_SERVICES[@]}"; do
      [[ -n "$unit" ]] || continue
      state="$(systemctl is-active "$unit" 2>/dev/null || true)"
      echo "agent-control-migration: snapshot unit state: $unit=${state:-unknown}" >&2
    done
    die "one or more units remain active or unknown; no units were restarted and journal retained"
  fi
  root_helpers_active && die "root control helpers remain active; all snapshotted units remain stopped"
  has_pending_requests && die "pending request appeared during quiescence; units remain stopped and journal retained"
  systemd-tmpfiles --create "$TMPFILES"
  [[ "$(directory_state)" == "$EXPECTED" ]] || die "control directory did not reach $EXPECTED; units remain stopped and journal retained"
  has_pending_requests && die "request appeared during directory migration; units remain stopped and journal retained"
fi

# A safe directory plus a durable journal means this is either the first restore
# or recovery after an interrupted restore. Re-running start is idempotent.
root_helpers_active && die "a root control helper is active; refusing to restore migration snapshot"
has_pending_requests && die "pending request exists; refusing to restore migration snapshot"
systemctl daemon-reload || die "systemd daemon-reload failed; journal retained and units remain stopped"
RESTORE_FAILURE=0
restore_units "root control watcher" "${CONTROL_PATHS[@]}" || RESTORE_FAILURE=1
restore_units "agent path" "${AGENT_PATHS[@]}" || RESTORE_FAILURE=1
restore_units "agent service" "${AGENT_SERVICES[@]}" || RESTORE_FAILURE=1
if (( RESTORE_FAILURE )); then
  die "one or more units were not restored; actual states were reported and journal retained for retry"
fi
rm -f -- "$JOURNAL"
echo "agent-control-migration: directory boundary verified and prior units restored"
