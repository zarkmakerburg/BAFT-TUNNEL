#!/usr/bin/env bash
# Exercise the migration helper against Ubuntu 22.04's real systemd manager.
# Run as root on an isolated CI runner; no tunnel network capabilities are needed.
set -Eeuo pipefail

if [[ "$EUID" -ne 0 ]]; then
  echo "agent-control-migration-systemd: must run as root" >&2
  exit 2
fi

SYSTEMD_VERSION="$(systemd --version | awk 'NR == 1 { print $2 }')"
[[ "$SYSTEMD_VERSION" == "249" ]] || {
  echo "agent-control-migration-systemd: expected systemd 249, got $SYSTEMD_VERSION" >&2
  exit 2
}

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd -P)"
HELPER="$SCRIPT_DIR/../migrate-agent-control-dir.sh"
UNIT_DIR=/run/systemd/system
TMP_ROOT="$(mktemp -d /run/baft-agent-control-migration-c2.XXXXXX)"
STATE_DIR="$(mktemp -d /var/lib/baft-agent-control-migration-c2.XXXXXX)"
CONTROL_PARENT="$TMP_ROOT/baft-tunnel"
CONTROL_DIR="$CONTROL_PARENT/control"
TMPFILES="$TMP_ROOT/tmpfiles.conf"
START_LOG="$TMP_ROOT/started-instances"
CREATED_UNIT_FILES=()
GROUP_CREATED_AGENT=0
GROUP_CREATED_TUNNEL=0
UNITS_STARTED=0

ALL_UNITS=(
  baft-tunnel-agent@iran.service
  baft-tunnel-agent@outside.service
  baft-tunnel-agent@offline.service
  baft-tunnel-agent@iran.path
  baft-tunnel-agent@outside.path
  baft-tunnel-agent@offline.path
  baft-tunnel-control-start@iran.path
  baft-tunnel-control-start@outside.path
)

cleanup() {
  local rc=$?
  trap - EXIT
  if (( UNITS_STARTED )); then
    for unit in "\${ALL_UNITS[@]}"; do
      systemctl stop "$unit" >/dev/null 2>&1 || true
    done
  fi
  for file in "\${CREATED_UNIT_FILES[@]}"; do
    rm -f -- "$file"
  done
  systemctl daemon-reload >/dev/null 2>&1 || true
  rm -f -- "$TMPFILES"
  rm -rf -- "$TMP_ROOT" "$STATE_DIR"
  if (( GROUP_CREATED_AGENT )); then
    groupdel baft-tunnel-agent >/dev/null 2>&1 || true
  fi
  if (( GROUP_CREATED_TUNNEL )); then
    groupdel baft-tunnel >/dev/null 2>&1 || true
  fi
  exit "$rc"
}
trap cleanup EXIT

# Refuse to collide with any pre-existing service or unit definition.
for unit in "\${ALL_UNITS[@]}" baft-tunnel-control-start@iran.service baft-tunnel-control-start@outside.service; do
  load_state="$(systemctl show "$unit" --property=LoadState --value 2>/dev/null || true)"
  if [[ -n "$load_state" && "$load_state" != "not-found" ]]; then
    echo "agent-control-migration-systemd: refusing pre-existing unit $unit ($load_state)" >&2
    exit 3
  fi
done
for file in \
  "$UNIT_DIR/baft-tunnel-agent@.service" \
  "$UNIT_DIR/baft-tunnel-agent@.path" \
  "$UNIT_DIR/baft-tunnel-control-start@.path" \
  "$UNIT_DIR/baft-tunnel-control-start@.service"; do
  if [[ -e "$file" || -L "$file" ]]; then
    echo "agent-control-migration-systemd: refusing pre-existing unit file $file" >&2
    exit 3
  fi
done

if ! getent group baft-tunnel-agent >/dev/null; then
  groupadd --system baft-tunnel-agent
  GROUP_CREATED_AGENT=1
fi
if ! getent group baft-tunnel >/dev/null; then
  groupadd --system baft-tunnel
  GROUP_CREATED_TUNNEL=1
fi

install -d -o root -g root -m 0755 "$CONTROL_PARENT"
install -d -o root -g baft-tunnel -m 0770 "$CONTROL_DIR"
: > "$START_LOG"
chmod 0600 "$START_LOG"

write_unit() {
  local path=$1
  shift
  cat > "$path"
  CREATED_UNIT_FILES+=("$path")
}

write_unit "$UNIT_DIR/baft-tunnel-agent@.service" <<UNIT
[Unit]
Description=BAFT C2 migration test agent %i
[Service]
Type=simple
ExecStart=/bin/sh -c 'echo "%i" >> "$START_LOG"; exec /bin/sleep infinity'
Restart=no
UNIT

write_unit "$UNIT_DIR/baft-tunnel-agent@.path" <<UNIT
[Unit]
Description=BAFT C2 migration test agent path %i
[Path]
PathExistsGlob=$CONTROL_DIR/__baft_c2_never_match__
Unit=baft-tunnel-agent@%i.service
[Install]
WantedBy=multi-user.target
UNIT

write_unit "$UNIT_DIR/baft-tunnel-control-start@.path" <<UNIT
[Unit]
Description=BAFT C2 migration test control path %i
[Path]
PathExistsGlob=$CONTROL_DIR/__baft_c2_never_match__.start
Unit=baft-tunnel-control-start@%i.service
[Install]
WantedBy=multi-user.target
UNIT

write_unit "$UNIT_DIR/baft-tunnel-control-start@.service" <<UNIT
[Unit]
Description=BAFT C2 migration test inactive helper %i
[Service]
Type=oneshot
ExecStart=/bin/true
UNIT

cat > "$TMPFILES" <<EOF
d $CONTROL_DIR 0770 root baft-tunnel-agent - -
EOF

systemctl daemon-reload
UNITS_STARTED=1
systemctl start \
  baft-tunnel-agent@iran.service \
  baft-tunnel-agent@outside.service \
  baft-tunnel-agent@offline.service
systemctl stop baft-tunnel-agent@offline.service
systemctl start \
  baft-tunnel-agent@iran.path \
  baft-tunnel-agent@outside.path \
  baft-tunnel-agent@offline.path \
  baft-tunnel-control-start@iran.path \
  baft-tunnel-control-start@outside.path

for unit in \
  baft-tunnel-agent@iran.service \
  baft-tunnel-agent@outside.service \
  baft-tunnel-agent@iran.path \
  baft-tunnel-agent@outside.path \
  baft-tunnel-agent@offline.path \
  baft-tunnel-control-start@iran.path \
  baft-tunnel-control-start@outside.path; do
  [[ "$(systemctl is-active "$unit")" == "active" ]]
done
[[ "$(systemctl is-active baft-tunnel-agent@offline.service || true)" == "inactive" ]]

SNAPSHOT="$(systemctl list-units --type=service --all --no-legend --plain 'baft-tunnel-agent@*.service')"
printf '%s\n' "$SNAPSHOT"
ACTIVE_COUNT="$(awk '$1 ~ /^baft-tunnel-agent@/ && $2 == "loaded" && $3 == "active" { n++ } END { print n + 0 }' <<< "$SNAPSHOT")"
INACTIVE_COUNT="$(awk '$1 ~ /^baft-tunnel-agent@/ && $2 == "loaded" && $3 == "inactive" { n++ } END { print n + 0 }' <<< "$SNAPSHOT")"
[[ "$ACTIVE_COUNT" == "2" && "$INACTIVE_COUNT" == "1" ]] || {
  echo "agent-control-migration-systemd: expected two active and one loaded inactive agent services; got active=$ACTIVE_COUNT inactive=$INACTIVE_COUNT" >&2
  exit 1
}

: > "$START_LOG"
bash "$HELPER" \
  --control-dir "$CONTROL_DIR" \
  --tmpfiles "$TMPFILES" \
  --state-dir "$STATE_DIR"

[[ "$(stat -c '%U:%G %a' "$CONTROL_PARENT")" == "root:root 755" ]]
[[ "$(stat -c '%U:%G %a' "$CONTROL_DIR")" == "root:baft-tunnel-agent 770" ]]
for unit in \
  baft-tunnel-agent@iran.service \
  baft-tunnel-agent@outside.service \
  baft-tunnel-agent@iran.path \
  baft-tunnel-agent@outside.path \
  baft-tunnel-agent@offline.path \
  baft-tunnel-control-start@iran.path \
  baft-tunnel-control-start@outside.path; do
  [[ "$(systemctl is-active "$unit")" == "active" ]]
done
[[ "$(systemctl is-active baft-tunnel-agent@offline.service || true)" == "inactive" ]]
[[ ! -e "$STATE_DIR/migration-v1.units" ]]
grep -Fxq iran "$START_LOG"
grep -Fxq outside "$START_LOG"
! grep -Fxq offline "$START_LOG" || false
[[ "$(wc -l < "$START_LOG" | tr -d ' ')" == "2" ]]

echo "BAFT_AGENT_CONTROL_MIGRATION_SYSTEMD_TEST=PASS"
echo "systemd=$SYSTEMD_VERSION active_agents=$ACTIVE_COUNT inactive_agents=$INACTIVE_COUNT"
echo "control_dir=$(stat -c '%U:%G %a' "$CONTROL_DIR")"
