#!/usr/bin/env bash
set -Eeuo pipefail
trap 'rc=$?; echo "BAFT_AGENT_CONTROL_MIGRATION_TEST=FAIL line=${LINENO} command=${BASH_COMMAND@Q} exit=$rc" >&2' ERR
ROOT="$(cd "$(dirname "$0")/.." && pwd -P)"
HELPER="$ROOT/scripts/migrate-agent-control-dir.sh"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/baft-control-migration.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT
BIN="$TMP/bin"
mkdir -p "$BIN"

cat > "$BIN/stat" <<'SH'
#!/usr/bin/env bash
[[ "${1:-}" == -c ]] || exit 2
if [[ "${3:-}" == "$FAKE_CONTROL_DIR" ]]; then cat "$FAKE_CONTROL_STATE"; elif [[ "${3:-}" == "$FAKE_CONTROL_PARENT" ]]; then cat "$FAKE_CONTROL_PARENT_STATE"; else /usr/bin/stat "$@"; fi
SH
cat > "$BIN/systemctl" <<'SH'
#!/usr/bin/env bash
set -Eeuo pipefail
contains() { grep -Fxq -- "$2" "$1" 2>/dev/null; }
add_state() {
  local state=$1 unit=$2 file="$FAKE_STATE_DIR/$1"
  for f in active inactive failed unknown activating deactivating; do
    grep -Fxv -- "$unit" "$FAKE_STATE_DIR/$f" > "$FAKE_STATE_DIR/$f.tmp" 2>/dev/null || true
    mv "$FAKE_STATE_DIR/$f.tmp" "$FAKE_STATE_DIR/$f"
  done
  printf '%s\n' "$unit" >> "$file"
}
cmd=$1; shift
[[ "${LIST_UNITS_FAIL:-}" == 1 && "$cmd" == list-units ]] && exit 1
case "$cmd" in
  list-units)
    kind= pattern= all=0
    for arg in "$@"; do
      case "$arg" in
        --type=*) kind="${arg#--type=}" ;;
        --all) all=1 ;;
        *) pattern="$arg" ;;
      esac
    done
    [[ "$kind" == path || "$kind" == service ]] || exit 2
    [[ "${LIST_UNITS_FAIL_PATTERN:-}" == "$pattern" ]] && exit 1
    files=(active)
    [[ "$all" == 1 ]] && files=(active inactive failed unknown activating deactivating)
    for state_file in "${files[@]}"; do
      while IFS= read -r unit; do
        [[ -n "$unit" ]] || continue
        [[ "$unit" == $pattern ]] || continue
        if [[ "$kind" == path && "$unit" == *.path ]] ||
           [[ "$kind" == service && "$unit" == *.service ]]; then
          printf '%s loaded active running fake\n' "$unit"
        fi
      done < "$FAKE_STATE_DIR/$state_file"
    done
    ;;
  is-active)
    [[ "${1:-}" == --quiet ]] && shift
    unit=$1
    if contains "$FAKE_STATE_DIR/active" "$unit"; then echo active; exit 0; fi
    if contains "$FAKE_STATE_DIR/failed" "$unit"; then echo failed; exit 1; fi
    if contains "$FAKE_STATE_DIR/unknown" "$unit"; then echo unknown; exit 4; fi
    if contains "$FAKE_STATE_DIR/activating" "$unit"; then echo activating; exit 0; fi
    if contains "$FAKE_STATE_DIR/deactivating" "$unit"; then echo deactivating; exit 0; fi
    echo inactive; exit 3
    ;;
  stop)
    unit=$1
    printf 'stop %s\n' "$unit" >> "$FAKE_LOG"
    if [[ "${REQUEST_ON_STOP_UNIT:-}" == "$unit" ]]; then mkdir -p "$(dirname "$REQUEST_FILE")"; : > "$REQUEST_FILE"; fi
    if [[ "${STOP_FAIL_UNIT:-}" == "$unit" ]]; then
      [[ "${STOP_FAIL_UNKNOWN:-}" == 1 ]] && add_state unknown "$unit"
      exit 1
    fi
    add_state inactive "$unit"
    ;;
  start)
    unit=$1
    printf 'start %s\n' "$unit" >> "$FAKE_LOG"
    [[ "${START_FAIL_UNIT:-}" == "$unit" ]] && exit 1
    add_state active "$unit"
    ;;
  daemon-reload)
    printf 'daemon-reload\n' >> "$FAKE_LOG"
    if [[ "${DAEMON_RELOAD_FAIL:-}" == 1 ]]; then exit 1; fi
    exit 0
    ;;
  *) echo "unexpected systemctl command: $cmd $*" >&2; exit 2 ;;
esac
SH
cat > "$BIN/systemd-tmpfiles" <<'SH'
#!/usr/bin/env bash
set -Eeuo pipefail
printf 'tmpfiles\n' >> "$FAKE_LOG"
[[ "${TMPFILES_FAIL:-}" == 1 ]] && exit 1
mkdir -p "$FAKE_CONTROL_DIR"
if [[ "${TMPFILES_BAD_STATE:-}" == 1 ]]; then
  printf 'root:baft-tunnel-agent 775\n' > "$FAKE_CONTROL_STATE"
else
  printf 'root:baft-tunnel-agent 770\n' > "$FAKE_CONTROL_STATE"
fi
if [[ "${REQUEST_ON_TMPFILES:-}" == 1 ]]; then mkdir -p "$(dirname "$REQUEST_FILE")"; : > "$REQUEST_FILE"; fi
SH
chmod 755 "$BIN/"*

CONTROL="baft-tunnel-control-start@iran.path"
CONTROL2="baft-tunnel-control-stop@outside.path"
AGENT_PATH="baft-tunnel-agent@iran.path"
AGENT_PATH2="baft-tunnel-agent@outside.path"
AGENT_SERVICE="baft-tunnel-agent@iran.service"
INACTIVE_AGENT="baft-tunnel-agent@offline.service"
setup_case() {
  CASE="$TMP/case"
  rm -rf "$CASE"
  mkdir -p "$CASE/state" "$CASE/parent/control"
  : > "$CASE/state/active"; : > "$CASE/state/inactive"
  : > "$CASE/state/failed"; : > "$CASE/state/unknown"
  : > "$CASE/state/activating"; : > "$CASE/state/deactivating"
  : > "$CASE/log"
  printf 'root:baft-tunnel 770\n' > "$CASE/control-state"
  printf '%s\n' "$CONTROL" "$CONTROL2" "$AGENT_PATH" "$AGENT_PATH2" "$AGENT_SERVICE" > "$CASE/state/active"
  printf '%s\n' "$INACTIVE_AGENT" > "$CASE/state/inactive"
  export FAKE_STATE_DIR="$CASE/state" FAKE_CONTROL_DIR="$CASE/parent/control" FAKE_CONTROL_PARENT="$CASE/parent"
  printf 'root:root 755\n' > "$CASE/parent-state"
  export FAKE_CONTROL_STATE="$CASE/control-state" FAKE_CONTROL_PARENT_STATE="$CASE/parent-state" FAKE_LOG="$CASE/log"
  export REQUEST_FILE="$CASE/parent/control/iran.start"
  unset STOP_FAIL_UNIT STOP_FAIL_UNKNOWN START_FAIL_UNIT DAEMON_RELOAD_FAIL TMPFILES_FAIL TMPFILES_BAD_STATE REQUEST_ON_STOP_UNIT REQUEST_ON_TMPFILES LIST_UNITS_FAIL_PATTERN
}
run_helper() {
  PATH="$BIN:$PATH" bash "$HELPER" --control-dir "$FAKE_CONTROL_DIR" --tmpfiles "$CASE/tmpfiles.conf" --state-dir "$CASE/state-dir"
}
expect_fail() {
  if run_helper >"$CASE/out" 2>&1; then
    echo "expected migration failure, got success" >&2
    cat "$CASE/out" >&2
    exit 1
  fi
}

# Successful multi-instance-style migration: all active snapshots stop in dependency
# order, safe ownership is verified, then only the prior active units return.
setup_case
mkdir -p "$CASE/state-dir"
chmod 0700 "$CASE/state-dir"
run_helper
[[ ! -e "$CASE/state-dir/migration-v1.units" ]]
grep -Fxq "$CONTROL" "$CASE/state/active"
grep -Fxq "$AGENT_PATH" "$CASE/state/active"
grep -Fxq "$AGENT_SERVICE" "$CASE/state/active"
! grep -Fxq "start $INACTIVE_AGENT" "$CASE/log" || false
[[ "$(cat "$FAKE_CONTROL_STATE")" == 'root:baft-tunnel-agent 770' ]]
[[ "$(sed -n '1p' "$CASE/log")" == "stop $CONTROL" ]]
[[ "$(sed -n '2p' "$CASE/log")" == "stop $CONTROL2" ]]
[[ "$(sed -n '3p' "$CASE/log")" == "stop $AGENT_PATH" ]]
[[ "$(sed -n '4p' "$CASE/log")" == "stop $AGENT_PATH2" ]]
[[ "$(sed -n '5p' "$CASE/log")" == "stop $AGENT_SERVICE" ]]
[[ "$(sed -n '8p' "$CASE/log")" == "start $CONTROL" ]]
[[ "$(sed -n '9p' "$CASE/log")" == "start $CONTROL2" ]]
[[ "$(sed -n '10p' "$CASE/log")" == "start $AGENT_PATH" ]]
[[ "$(sed -n '11p' "$CASE/log")" == "start $AGENT_PATH2" ]]
[[ "$(sed -n '12p' "$CASE/log")" == "start $AGENT_SERVICE" ]]

# Reject a symlink at the control directory leaf before touching any units.
setup_case
rm -rf "$FAKE_CONTROL_DIR"
mkdir -p "$CASE/target"
ln -s "$CASE/target" "$FAKE_CONTROL_DIR"
expect_fail
! grep -q '^stop ' "$CASE/log" || false

# Reject a symlinked parent and a parent whose ownership/mode is not trusted.
setup_case
mv "$FAKE_CONTROL_PARENT" "$CASE/parent-real"
ln -s "$CASE/parent-real" "$FAKE_CONTROL_PARENT"
expect_fail
! grep -q '^stop ' "$CASE/log" || false
setup_case
printf 'root:root 775\n' > "$FAKE_CONTROL_PARENT_STATE"
expect_fail
! grep -q '^stop ' "$CASE/log" || false

# Unsafe pre-existing state-directory permissions are rejected, not silently repaired.
setup_case
mkdir -p "$CASE/state-dir"
chmod 0777 "$CASE/state-dir"
expect_fail
[[ "$(stat -c '%a' "$CASE/state-dir")" == 777 ]]
! grep -q '^stop ' "$CASE/log" || false

# Malformed journals and symlinked lock files fail before any unit transition.
setup_case
mkdir -p "$CASE/state-dir"
chmod 0700 "$CASE/state-dir"
printf 'unknown\tunit\n' > "$CASE/state-dir/migration-v1.units"
chmod 0600 "$CASE/state-dir/migration-v1.units"
expect_fail
! grep -q '^stop ' "$CASE/log" || false
setup_case
mkdir -p "$CASE/state-dir"
chmod 0700 "$CASE/state-dir"
ln -s "$CASE/journal-target" "$CASE/state-dir/migration-v1.units"
expect_fail
! grep -q '^stop ' "$CASE/log" || false
grep -q 'migration journal must not be a symlink' "$CASE/out"

# A symlinked migration.lock must be rejected before opening its target or touching units.
setup_case
mkdir -p "$CASE/state-dir"
chmod 0700 "$CASE/state-dir"
printf 'sentinel-must-remain-unchanged\n' > "$CASE/lock-target"
ln -s "$CASE/lock-target" "$CASE/state-dir/migration.lock"
expect_fail
[[ "$(cat "$CASE/lock-target")" == 'sentinel-must-remain-unchanged' ]]
grep -q 'migration lock must not be a symlink' "$CASE/out"
[[ ! -s "$CASE/log" ]]

# A concurrent migration holding the lock must prevent a second invocation.
setup_case
mkdir -p "$CASE/state-dir"
chmod 0700 "$CASE/state-dir"
if (
  exec 8>"$CASE/state-dir/migration.lock"
  flock -n 8
  chmod 0600 "$CASE/state-dir/migration.lock"
  run_helper >"$CASE/out" 2>&1
); then
  echo "expected concurrent migration to fail while the lock is held" >&2
  exit 1
fi
grep -q 'another control-directory migration is already running' "$CASE/out"
! grep -q '^stop ' "$CASE/log" || false

# A fresh install may not have created the shared request directory yet.
setup_case
rm -rf "$FAKE_CONTROL_DIR"
printf 'MISSING\\n' > "$FAKE_CONTROL_STATE"
run_helper
[[ -d "$FAKE_CONTROL_DIR" ]]
[[ "$(cat "$FAKE_CONTROL_STATE")" == 'root:baft-tunnel-agent 770' ]]

# If systemd cannot enumerate loaded units, do not assume the snapshot is empty.
setup_case
LIST_UNITS_FAIL=1 expect_fail
! grep -q '^stop ' "$CASE/log" || false
grep -q 'could not enumerate root control helper services' "$CASE/out"

# Failures in each migration snapshot enumeration must propagate out of the
# process substitution boundary and abort before any unit is stopped.
for failed_pattern in 'baft-tunnel-control-*@*.path' 'baft-tunnel-agent@*.path' 'baft-tunnel-agent@*.service'; do
  setup_case
  LIST_UNITS_FAIL_PATTERN="$failed_pattern" expect_fail
  ! grep -q '^stop ' "$CASE/log" || false
  grep -q 'could not enumerate' "$CASE/out"
done

# Active root helper and pre-existing pending request must refuse before stopping.
setup_case
printf '%s\n' 'baft-tunnel-control-setup@iran.service' >> "$CASE/state/active"
expect_fail
! grep -q '^stop ' "$CASE/log" || false
setup_case
: > "$REQUEST_FILE"
expect_fail
[[ -e "$REQUEST_FILE" ]]
! grep -q '^stop ' "$CASE/log" || false

# Partial stop failures at each dependency stage, including an unknown state,
# never change ownership or restart any unit. A later retry consumes the journal.
for failed_unit in "$CONTROL" "$AGENT_PATH" "$AGENT_SERVICE"; do
  setup_case
  STOP_FAIL_UNIT="$failed_unit" expect_fail
  [[ -f "$CASE/state-dir/migration-v1.units" ]]
  [[ "$(cat "$FAKE_CONTROL_STATE")" == 'root:baft-tunnel 770' ]]
  ! grep -q '^start ' "$CASE/log" || false
done
setup_case
STOP_FAIL_UNIT="$AGENT_PATH" STOP_FAIL_UNKNOWN=1 expect_fail
grep -q 'state=unknown' "$CASE/out"
[[ "$(cat "$FAKE_CONTROL_STATE")" == 'root:baft-tunnel 770' ]]
! grep -q '^start ' "$CASE/log" || false
run_helper
[[ ! -e "$CASE/state-dir/migration-v1.units" ]]

# tmpfiles failure and ownership/mode mismatch never restore writers or watchers.
setup_case
TMPFILES_FAIL=1 expect_fail
! grep -q '^start ' "$CASE/log" || false
run_helper
setup_case
TMPFILES_BAD_STATE=1 expect_fail
! grep -q '^start ' "$CASE/log" || false

# A request appearing while writers are being stopped or during tmpfiles blocks restore.
setup_case
REQUEST_ON_STOP_UNIT="$CONTROL" expect_fail
[[ "$(cat "$FAKE_CONTROL_STATE")" == 'root:baft-tunnel 770' ]]
[[ -e "$REQUEST_FILE" ]]
! grep -q '^start ' "$CASE/log" || false
rm -f "$REQUEST_FILE"
run_helper
setup_case
REQUEST_ON_TMPFILES=1 expect_fail
[[ "$(cat "$FAKE_CONTROL_STATE")" == 'root:baft-tunnel-agent 770' ]]
[[ -e "$REQUEST_FILE" ]]
! grep -q '^start ' "$CASE/log" || false
rm -f "$REQUEST_FILE"
run_helper

# daemon-reload failure leaves the durable journal for a safe retry.
setup_case
DAEMON_RELOAD_FAIL=1 expect_fail
! grep -q '^start ' "$CASE/log" || false
unset DAEMON_RELOAD_FAIL
run_helper
[[ ! -e "$CASE/state-dir/migration-v1.units" ]]

# Every restoration-start failure is reported; journal survives and a rerun
# completes recovery without activating units that were inactive in the snapshot.
for failed_unit in "$CONTROL" "$CONTROL2" "$AGENT_PATH" "$AGENT_PATH2" "$AGENT_SERVICE"; do
  setup_case
  START_FAIL_UNIT="$failed_unit" expect_fail
  [[ -f "$CASE/state-dir/migration-v1.units" ]]
  grep -q "could not restore" "$CASE/out"
  run_helper
  [[ ! -e "$CASE/state-dir/migration-v1.units" ]]
  grep -Fxq "$failed_unit" "$CASE/state/active"
  ! grep -Fxq "$INACTIVE_AGENT" "$CASE/state/active" || false
done

echo "BAFT_AGENT_CONTROL_MIGRATION_TEST=PASS (preflight refusal, ordered quiescence, verified ownership, request-race refusal, tmpfiles and daemon-reload failures, partial restore recovery)"
