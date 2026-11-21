#!/usr/bin/env bash
# Install the exact candidate and start the isolated pairing-test agent on one LAB host.
set -Eeuo pipefail
umask 077

die() { echo "auto-pair install: $*" >&2; exit 2; }
[[ "$#" == 8 ]] || die "expected BINARY SHA256 PACKAGE_DIR CONFIG_DIR AGENT_STATE_DIR REMOTE_DIR API_PORT INSTANCE"

BINARY="$1"
EXPECTED_SHA="$2"
PKG="$3"
CONFIG_DIR="$4"
AGENT_STATE_DIR="$5"
REMOTE_DIR="$6"
API_PORT="$7"
INSTANCE="$8"

[[ "$(id -u)" == 0 ]] || die "must run as root"
[[ "$EXPECTED_SHA" =~ ^[0-9a-f]{64}$ ]] || die "invalid candidate SHA-256"
[[ "$REMOTE_DIR" =~ ^/tmp/baft-tunnel-e2e-[A-Za-z0-9._-]+$ ]] || die "invalid run directory"
[[ -d "$REMOTE_DIR" && ! -L "$REMOTE_DIR" ]] || die "run directory is missing or unsafe"
[[ "$(stat -c '%U:%G %a' "$REMOTE_DIR")" == root:root\ 700 ]] || die "run directory ownership or mode changed"
run_marker="$REMOTE_DIR/.baft-tunnel-e2e-run-owner"
[[ -f "$run_marker" && ! -L "$run_marker" ]] || die "LAB run ownership marker is missing"
[[ "$(stat -c '%U:%G %a' "$run_marker")" == root:root\ 600 ]] || die "LAB run ownership marker changed"
[[ "$(cat "$run_marker")" == "$REMOTE_DIR" ]] || die "LAB run ownership marker belongs to another run"
[[ "$PKG" == "$REMOTE_DIR/pkg" ]] || die "package path is outside this run directory"
[[ -d "$PKG" && ! -L "$PKG" ]] || die "package directory is missing or unsafe"
[[ "$(stat -c '%U:%G' "$PKG")" == root:root ]] || die "package directory is not root-owned"
[[ "$BINARY" == "$REMOTE_DIR/baft-tunnel-ap" ]] || die "binary path is outside this run directory"
[[ "$CONFIG_DIR" == /etc/baft-tunnel ]] || die "unexpected config directory"
[[ ! -e "$CONFIG_DIR" && ! -L "$CONFIG_DIR" ]] || die "pre-existing config directory; refusing install"
[[ "$AGENT_STATE_DIR" == /var/lib/baft-tunnel-agent ]] || die "unexpected agent state directory"
[[ ! -e "$AGENT_STATE_DIR" && ! -L "$AGENT_STATE_DIR" ]] || die "pre-existing agent state; refusing install"
for path in /usr/local/bin/baft-tunnel /usr/local/lib/baft-tunnel /usr/local/lib/baft-tunnel-engines \
  /var/lib/baft-tunnel /var/lib/baft-tunnel-control /run/baft-tunnel \
  /etc/tmpfiles.d/baft-tunnel-control.conf; do
  [[ ! -e "$path" && ! -L "$path" ]] || die "pre-existing BAFT resource $path; refusing install"
done
! getent passwd baft-tunnel >/dev/null 2>&1 || die "pre-existing service account; refusing install"
! getent passwd baft-tunnel-agent >/dev/null 2>&1 || die "pre-existing agent account; refusing install"
! getent group baft-tunnel >/dev/null 2>&1 || die "pre-existing service group; refusing install"
! getent group baft-tunnel-agent >/dev/null 2>&1 || die "pre-existing agent group; refusing install"
! find /etc/systemd/system -name 'baft-tunnel*' -print -quit | grep -q . || die "pre-existing systemd resources; refusing install"
[[ -z "$(find /var/backups -maxdepth 1 \( -name 'baft-tunnel-install.*' -o -name 'baft-tunnel-backup.*' \) -print -quit 2>/dev/null)" ]] ||
  die "pre-existing BAFT backup; refusing install"
[[ "$API_PORT" =~ ^[0-9]{1,5}$ ]] && (( 10#$API_PORT >= 1 && 10#$API_PORT <= 65535 )) || die "invalid agent API port"
[[ "$INSTANCE" =~ ^[A-Za-z0-9][A-Za-z0-9_-]{0,62}$ ]] || die "invalid instance name"
[[ -x "$BINARY" && ! -L "$BINARY" ]] || die "candidate binary is missing or unsafe"
[[ -f "$PKG/scripts/install.sh" && -f "$PKG/scripts/install-agent-control.sh" ]] || die "install package is incomplete"

binary_sha="$(sha256sum "$BINARY" | awk '{print $1}')"
[[ "$binary_sha" == "$EXPECTED_SHA" ]] || die "uploaded candidate binary SHA-256 mismatch"

install_output="$(bash "$PKG/scripts/install.sh" --binary "$BINARY")"
backup_dir="$(printf '%s\n' "$install_output" | sed -n 's/^Backup: //p')"
[[ "$backup_dir" =~ ^/var/backups/baft-tunnel-install\.[[:alnum:]]+$ ]] || die "installer did not return one scoped backup directory"
printf '%s\n' "$backup_dir" > "$REMOTE_DIR/install-backup-$INSTANCE.path"
printf '%s\n' "$backup_dir"
installed_sha="$(sha256sum /usr/local/bin/baft-tunnel | awk '{print $1}')"
[[ "$installed_sha" == "$EXPECTED_SHA" ]] || die "installed binary SHA-256 mismatch"

# Record the complete installed library tree. Cleanup refuses to uninstall if another
# process adds, removes, or changes any file while the LAB is running.
[[ -d /usr/local/lib/baft-tunnel && ! -L /usr/local/lib/baft-tunnel ]] || die "installed library directory is missing or unsafe"
(
  cd /usr/local/lib/baft-tunnel
  find . -type f -print0 | sort -z | xargs -0 sha256sum
) > "$REMOTE_DIR/installed-lib.sha256"
(
  cd /usr/local/lib/baft-tunnel
  find . -type d -print | sort
) > "$REMOTE_DIR/installed-lib-dirs.txt"
getent group baft-tunnel > "$REMOTE_DIR/account-baft-tunnel.group.before"

bash "$PKG/scripts/install-agent-control.sh" --instance "$INSTANCE" --listen "127.0.0.1:$API_PORT" >/dev/null
listen_dropin="/etc/systemd/system/baft-tunnel-agent@$INSTANCE.service.d/listen.conf"
[[ -f "$listen_dropin" && ! -L "$listen_dropin" ]] || die "agent listen drop-in is missing or unsafe"
[[ "$(stat -c '%U:%G %a' "$listen_dropin")" == root:root\ 644 ]] || die "agent listen drop-in ownership or mode is unexpected"
sha256sum "$listen_dropin" > "$REMOTE_DIR/agent-listen-$INSTANCE.sha256"
getent passwd baft-tunnel > "$REMOTE_DIR/account-baft-tunnel.passwd"
getent group baft-tunnel > "$REMOTE_DIR/account-baft-tunnel.group"
getent passwd baft-tunnel-agent > "$REMOTE_DIR/account-baft-tunnel-agent.passwd"
getent group baft-tunnel-agent > "$REMOTE_DIR/account-baft-tunnel-agent.group"
instance_state="$AGENT_STATE_DIR/$INSTANCE"
for attempt in $(seq 1 40); do
  if [[ -d "$instance_state" && ! -L "$instance_state" ]] &&
     [[ "$(stat -c '%U:%G %a' "$instance_state")" == baft-tunnel-agent:baft-tunnel-agent\ 700 ]]; then
    break
  fi
  sleep 0.25
done
[[ -d "$instance_state" && ! -L "$instance_state" ]] || die "systemd did not create the isolated agent state directory"
[[ "$(stat -c '%U:%G %a' "$instance_state")" == baft-tunnel-agent:baft-tunnel-agent\ 700 ]] || die "agent state directory ownership or mode is unexpected"
printf '%s\n' "$REMOTE_DIR" > "$instance_state/.baft-tunnel-lab-run-owner"
chmod 600 "$instance_state/.baft-tunnel-lab-run-owner"
chown root:root "$instance_state/.baft-tunnel-lab-run-owner"
[[ "$(stat -c '%U:%G %a' "$AGENT_STATE_DIR/token")" == baft-tunnel-agent:baft-tunnel-agent\ 600 ]] || die "agent token ownership or mode is unexpected"
sha256sum "$AGENT_STATE_DIR/token" > "$REMOTE_DIR/agent-token.sha256"

for attempt in $(seq 1 40); do
  if python3 - "$API_PORT" <<'PY'
import sys
import urllib.request
urllib.request.urlopen("http://127.0.0.1:%s/v1/health" % sys.argv[1], timeout=1)
PY
  then
    break
  fi
  sleep 0.25
done

if ! python3 - "$API_PORT" <<'PY'
import sys
import urllib.request
urllib.request.urlopen("http://127.0.0.1:%s/v1/health" % sys.argv[1], timeout=1)
PY
then
  journalctl -u "baft-tunnel-agent@$INSTANCE.service" -n 40 --no-pager >&2 || true
  die "agent API did not become healthy on port $API_PORT"
fi
systemctl is-active --quiet "baft-tunnel-agent@$INSTANCE.service" || die "systemd agent service is not active"
agent_pid="$(systemctl show -p MainPID --value "baft-tunnel-agent@$INSTANCE.service")"
[[ "$agent_pid" =~ ^[1-9][0-9]*$ ]] || die "systemd agent service has no main PID"
[[ "$(ps -o user= -p "$agent_pid" | tr -d ' ')" == baft-tunnel-agent ]] || die "agent service is not running as baft-tunnel-agent"
