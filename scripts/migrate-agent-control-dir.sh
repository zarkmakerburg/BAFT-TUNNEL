#!/usr/bin/env bash
# Host-wide, fail-closed migration of the root control request directory.
# Recovery: inspect and resolve the reported condition, preserve pending request files,
# then rerun the same install-agent-control.sh --instance NAME registration. Never
# delete this journal or manually restart watchers while the directory boundary is unsafe.
set -Eeuo pipefail
# BAFT_ERR_DIAGNOSTIC: preserve the failing command location on strict-mode exits.
trap 'rc=$?; printf "agent-control-migration: unhandled command failure (exit=%s, line=%s, function=%s, command=%q)\n" "$rc" "$LINENO" "${FUNCNAME[1]:-main}" "$BASH_COMMAND" >&2; exit "$rc"' ERR

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
CONTROL_PARENT="${CONTROL_DIR%/*}"
[[ "$CONTROL_PARENT" != "$CONTROL_DIR" ]] || die "control directory must have a parent directory"
[[ ! -L "$CONTROL_PARENT" ]] || die "control directory parent must not be a symlink"
if [[ ! -e "$CONTROL_PARENT" ]]; then
  install -d -o root -g root -m 0755 "$CONTROL_PARENT"
fi
[[ -d "$CONTROL_PARENT" && ! -L "$CONTROL_PARENT" ]] || die "invalid control directory parent"
[[ "$(stat -c '%U:%G %a' "$CONTROL_PARENT")" == "root:root 755" ]] ||
  die "control directory parent must be root-owned with mode 0755"
[[ ! -L "$CONTROL_DIR" ]] || die "control directory must not be a symlink"
TMPFILES_APPLIED=0

umask 077
if [[ -e "$STATE_DIR" || -L "$STATE_DIR" ]]; then
  [[ -d "$STATE_DIR" && ! -L "$STATE_DIR" ]] || die "invalid migration state directory"
  [[ "$(stat -c '%U:%G %a' "$STATE_DIR")" == "$(id -un):$(id -gn) 700" ]] ||
    die "migration state directory must already be private to the invoking privileged account"
else
  mkdir -p "$STATE_DIR"
fi
JOURNAL="$STATE_DIR/migration-v1.units"
LOCK_FILE="$STATE_DIR/migration.lock"
[[ ! -L "$JOURNAL" ]] || die "migration journal must not be a symlink"
if [[ -e "$JOURNAL" ]]; then
  [[ -f "$JOURNAL" ]] || die "migration journal is not a regular file"
  [[ "$(stat -c '%U:%G %a' "$JOURNAL")" == "$(id -un):$(id -gn) 600" ]] ||
    die "migration journal must be private to the invoking privileged account"
fi
[[ ! -L "$LOCK_FILE" ]] || die "migration lock must not be a symlink"
if [[ -e "$LOCK_FILE" ]]; then
  [[ -f "$LOCK_FILE" ]] || die "migration lock is not a regular file"
  [[ "$(stat -c '%U:%G %a' "$LOCK_FILE")" == "$(id -un):$(id -gn) 600" ]] ||
    die "migration lock must be private to the invoking privileged account"
fi
exec 9>"$LOCK_FILE"
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
  local kind=$1 pattern=$2 unit state listed
  listed="$(systemctl list-units --type="$kind" --all --no-legend --plain "$pattern")" ||
    die "could not enumerate $kind units matching $pattern; refusing to change directory ownership"
  while IFS= read -r unit; do
    [[ -n "$unit" ]] || continue
    state="$(systemctl is-active "$unit" 2>/dev/null || true)"
    [[ -n "$state" ]] || state=unknown
    case "$state" in
      active|activating|deactivating|unknown) printf '%s\n' "$unit" ;;
    esac
  done < <(printf '%s\n' "$listed" | awk 'NF { print $1 }')
}
root_helpers_active() {
  local unit state listed
  listed="$(systemctl list-units --type=service --all --no-legend --plain 'baft-tunnel-control-*@*.service')" ||
    die "could not enumerate root control helper services; refusing to proceed"
  while IFS= read -r unit; do
    [[ -n "$unit" ]] || continue
    state="$(systemctl is-active "$unit" 2>/dev/null || true)"
    [[ -n "$state" ]] || state=unknown
    case "$state" in
      active|activating|deactivating|unknown)
        echo "agent-control-migration: root helper is $state: $unit" >&2
        return 0
        ;;
    esac
  done < <(printf '%s\n' "$listed" | awk 'NF { print $1 }')
  return 1
}
valid_unit() {
  local kind=$1 unit=$2
  case "$kind" in
    control) [[ "$unit" =~ ^baft-tunnel-control-(start|stop|enable|disable|setup|pki)@[A-Za-z0-9][A-Za-z0-9_-]{0,62}\.path$ ]] ;;
    agent_path) [[ "$unit" =~ ^baft-tunnel-agent@[A-Za-z0-9][A-Za-z0-9_-]{0,62}\.path$ ]] ;;
    agent_service) [[ "$unit" =~ ^baft-tunnel-agent@[A-Za-z0-9][A-Za-z0-9_-]{0,62}\.service$ ]] ;;
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
  local unit listed
  # Do not use process substitution here: its exit status is not propagated to
  # the parent shell. Capture each enumeration so a systemctl failure aborts
  # before the journal is committed or any unit is stopped.
  listed="$(active_units path 'baft-tunnel-control-*@*.path')" ||
    die "could not snapshot root control path watchers; refusing to change directory ownership"
  while IFS= read -r unit; do
    if [[ -n "$unit" ]]; then unique_append CONTROL_PATHS "$unit"; fi
  done <<< "$listed"
  listed="$(active_units path 'baft-tunnel-agent@*.path')" ||
    die "could not snapshot agent path units; refusing to change directory ownership"
  while IFS= read -r unit; do
    if [[ -n "$unit" ]]; then unique_append AGENT_PATHS "$unit"; fi
  done <<< "$listed"
  listed="$(active_units service 'baft-tunnel-agent@*.service')" ||
    die "could not snapshot agent services; refusing to change directory ownership"
  while IFS= read -r unit; do
    if [[ -n "$unit" ]]; then unique_append AGENT_SERVICES "$unit"; fi
  done <<< "$listed"
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

# Revoke tunnel-account access before any quiescence or pending-request check, so the
# tunnel account cannot keep planting requests that block migration indefinitely.
# The agent (primary group baft-tunnel-agent) and root helpers keep access.
INITIAL_STATE="$(directory_state)"
if [[ -d "$CONTROL_DIR" && "$INITIAL_STATE" != "$EXPECTED" ]]; then
  chown root:baft-tunnel-agent "$CONTROL_DIR" || die "could not revoke tunnel access to the control directory"
  chmod 00770 "$CONTROL_DIR" || die "could not revoke tunnel access to the control directory"
  [[ "$(directory_state)" == "$EXPECTED" ]] || die "control directory did not reach $EXPECTED after revoking tunnel access"
fi

if [[ -f "$JOURNAL" ]]; then
  load_journal
else
  root_helpers_active && die "a root control helper is active; retry after it finishes"
  has_pending_requests && die "pending control request exists; inspect it as root before migration"
  snapshot_current_units
  write_journal
fi

if [[ "$INITIAL_STATE" != "$EXPECTED" ]]; then
  root_helpers_active && die "a root control helper is active; journal retained and migration stopped"
  has_pending_requests && die "pending control request exists; journal retained and migration stopped"
  # Include anything that became active after the durable snapshot.
  snapshot_current_units
  write_journal
  STOP_FAILURE=0
  # Stop request producers (agent) before consumers (root watchers), so no agent
  # request can land after its watcher is gone.
  stop_units "agent path" "${AGENT_PATHS[@]}" || STOP_FAILURE=1
  stop_units "agent service" "${AGENT_SERVICES[@]}" || STOP_FAILURE=1
  stop_units "root control watcher" "${CONTROL_PATHS[@]}" || STOP_FAILURE=1
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
  systemd-tmpfiles --create "$TMPFILES" || die "systemd-tmpfiles failed; units remain stopped and migration journal retained"
  TMPFILES_APPLIED=1
  [[ "$(directory_state)" == "$EXPECTED" ]] || die "control directory did not reach $EXPECTED; units remain stopped and journal retained"
  has_pending_requests && die "request appeared during directory migration; units remain stopped and journal retained"
fi

# A safe directory plus a durable journal means this is either the first restore
# or recovery after an interrupted restore. Re-running start is idempotent.
root_helpers_active && die "a root control helper is active; refusing to restore migration snapshot"
has_pending_requests && die "pending request exists; refusing to restore migration snapshot"
if (( ! TMPFILES_APPLIED )); then
  systemd-tmpfiles --create "$TMPFILES" || die "systemd-tmpfiles failed during migration recovery; journal retained and units remain stopped"
fi
[[ "$(directory_state)" == "$EXPECTED" ]] || die "control directory is not at the required safe owner/group/mode; journal retained and units remain stopped"
has_pending_requests && die "request appeared during migration recovery; units remain stopped and journal retained"
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
