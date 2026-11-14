#!/usr/bin/env bash
set -Eeuo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd -P)"
HELPER="$ROOT/scripts/migrate-agent-control-dir.sh"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/baft-control-migration.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT
BIN="$TMP/bin"
mkdir -p "$BIN"

cat > "$BIN/stat" <<'SH'
#!/usr/bin/env bash
[[ "${1:-}" == -c ]] || exit 2
if [[ "${3:-}" == "$FAKE_CONTROL_DIR" ]]; then cat "$FAKE_CONTROL_STATE"; else /usr/bin/stat "$@"; fi
SH
cat > "$BIN/systemctl" <<'SH'
#!/usr/bin/env bash
set -Eeuo pipefail
contains() { grep -Fxq -- "$2" "$1" 2>/dev/null; }
add_state() {
  local state=$1 unit=$2 file="$FAKE_STATE_DIR/$1"
  for f in active inactive failed unknown; do
    grep -Fxv -- "$unit" "$FAKE_STATE_DIR/$f" > "$FAKE_STATE_DIR/$f.tmp" 2>/dev/null || true
    mv "$FAKE_STATE_DIR/$f.tmp" "$FAKE_STATE_DIR/$f"
  done
  printf '%s\n' "$unit" >> "$file"
}
cmd=$1; shift
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
    files=(active)
    [[ "$all" == 1 ]] && files=(active inactive failed unknown)
    for state_file in "${files[@]}"; do
      while IFS= read -r unit; do
        [[ -n "$unit" ]] || continue
        [[ "$unit" == $pattern ]] || continue
        if [[ "$kind" == path && "$unit" == *.path ]] ||
           [[ "$kind" == service && "$unit" == *.service ]]; then
          printf '%s loaded active running fake\\n' "$unit"
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
    [[ "${DAEMON_RELOAD_FAIL:-}" == 1 ]] && exit 1
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
  mkdir -p "$CASE/state" "$CASE/control"
  mkdir -p "$CASE/control"
  : > "$CASE/state/active"; : > "$CASE/state/inactive"
  : > "$CASE/state/failed"; : > "$CASE/state/unknown"
  : > "$CASE/log"
  printf 'root:baft-tunnel 770\n' > "$CASE/control-state"
  printf '%s\n' "$CONTROL" "$AGENT_PATH" "$AGENT_SERVICE" > "$CASE/state/active"
  export FAKE_STATE_DIR="$CASE/state" FAKE_CONTROL_DIR="$CASE/control"
  export FAKE_CONTROL_STATE="$CASE/control-state" FAKE_LOG="$CASE/log"
  export REQUEST_FILE="$CASE/control/iran.start"
  unset STOP_FAIL_UNIT STOP_FAIL_UNKNOWN START_FAIL_UNIT DAEMON_RELOAD_FAIL TMPFILES_FAIL TMPFILES_BAD_STATE REQUEST_ON_STOP_UNIT REQUEST_ON_TMPFILES
}
run_helper() {
  PATH="$BIN:$PATH" "$HELPER" --control-dir "$FAKE_CONTROL_DIR" --tmpfiles "$CASE/tmpfiles.conf" --state-dir "$CASE/state-dir"
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
run_helper
[[ ! -e "$CASE/state-dir/migration-v1.units" ]]
grep -Fxq "$CONTROL" "$CASE/state/active"
grep -Fxq "$AGENT_PATH" "$CASE/state/active"
grep -Fxq "$AGENT_SERVICE" "$CASE/state/active"
! grep -Fxq "$INACTIVE_AGENT" "$CASE/log"
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

# Active root helper and pre-existing pending request must refuse before stopping.
setup_case
printf '%s\n' 'baft-tunnel-control-setup@iran.service' >> "$CASE/state/active"
expect_fail
! grep -q '^stop ' "$CASE/log"
setup_case
: > "$REQUEST_FILE"
expect_fail
! grep -q '^stop ' "$CASE/log"

# Stop failure (including unknown post-stop state) is fail-closed and recoverable.
setup_case
STOP_FAIL_UNIT="$AGENT_PATH" STOP_FAIL_UNKNOWN=1 run_helper >"$CASE/out" 2>&1 && exit 1
[[ -f "$CASE/state-dir/migration-v1.units" ]]
[[ "$(cat "$FAKE_CONTROL_STATE")" == 'root:baft-tunnel 770' ]]
! grep -q '^start ' "$CASE/log"
run_helper
[[ ! -e "$CASE/state-dir/migration-v1.units" ]]

# tmpfiles failure and ownership/mode mismatch never restore writers or watchers.
setup_case
TMPFILES_FAIL=1 run_helper >"$CASE/out" 2>&1 && exit 1
! grep -q '^start ' "$CASE/log"
run_helper
setup_case
TMPFILES_BAD_STATE=1 run_helper >"$CASE/out" 2>&1 && exit 1
! grep -q '^start ' "$CASE/log"

# A request appearing while writers are being stopped or during tmpfiles blocks restore.
setup_case
REQUEST_ON_STOP_UNIT="$CONTROL" run_helper >"$CASE/out" 2>&1 && exit 1
[[ "$(cat "$FAKE_CONTROL_STATE")" == 'root:baft-tunnel 770' ]]
! grep -q '^start ' "$CASE/log"
rm -f "$REQUEST_FILE"
run_helper
setup_case
REQUEST_ON_TMPFILES=1 run_helper >"$CASE/out" 2>&1 && exit 1
[[ "$(cat "$FAKE_CONTROL_STATE")" == 'root:baft-tunnel-agent 770' ]]
! grep -q '^start ' "$CASE/log"
rm -f "$REQUEST_FILE"
run_helper

# daemon-reload failure leaves the durable journal for a safe retry.
setup_case
DAEMON_RELOAD_FAIL=1 run_helper >"$CASE/out" 2>&1 && exit 1
! grep -q '^start ' "$CASE/log"
run_helper
[[ ! -e "$CASE/state-dir/migration-v1.units" ]]

# Partial restore failure is reported; journal survives and a rerun completes recovery.
setup_case
START_FAIL_UNIT="$AGENT_SERVICE" run_helper >"$CASE/out" 2>&1 && exit 1
[[ -f "$CASE/state-dir/migration-v1.units" ]]
run_helper
[[ ! -e "$CASE/state-dir/migration-v1.units" ]]
grep -Fxq "$AGENT_SERVICE" "$CASE/state/active"

echo "BAFT_AGENT_CONTROL_MIGRATION_TEST=PASS (preflight refusal, ordered quiescence, verified ownership, request-race refusal, tmpfiles and daemon-reload failures, partial restore recovery)"
