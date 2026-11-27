#!/usr/bin/env bash
# Remove only the exact resources created by one automatic-pairing LAB run on this host.
set -Eeuo pipefail
umask 077

die() { echo "auto-pair cleanup: $*" >&2; exit 2; }
[[ "$#" -ge 4 ]] || die "expected SHA256 PACKAGE_DIR AGENT_STATE_DIR REMOTE_DIR and scoped cleanup arguments"

EXPECTED_SHA="$1"
PKG="$2"
AGENT_STATE_DIR="$3"
REMOTE_DIR="$4"
shift 4

UB=/usr/local/bin/baft-tunnel
EB=/etc/baft-tunnel
STATE_DIR=/var/lib/baft-tunnel-agent
CONTROL_DIR=/run/baft-tunnel/control
MANAGE_DIR=/run/baft-tunnel/manage
UNIT_DIR=/etc/systemd/system
ENGINE_STORE=/usr/local/lib/baft-tunnel-engines
BACKUP_DIRS=()
INSTANCES=()
AUTO_PAIR_ECHO_PORT=""

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
[[ "$AGENT_STATE_DIR" == /var/lib/baft-tunnel-agent ]] || die "unexpected agent state directory"

# A round can invoke this helper twice: once when the round finishes and once from the
# stage's EXIT trap. The first pass removes the installation (including the config root),
# so the second pass must be a successful no-op instead of failing on the absent config
# root. Exit 0 only when every artifact owned by this helper is already gone; any partial
# state still falls through to the strict, fail-closed checks below.
installation_absent() {
  local path
  for path in "$UB" "$EB" /var/lib/baft-tunnel "$AGENT_STATE_DIR" /var/lib/baft-tunnel-control \
    /run/baft-tunnel /usr/local/lib/baft-tunnel "$ENGINE_STORE" \
    /etc/tmpfiles.d/baft-tunnel-control.conf; do
    [[ ! -e "$path" && ! -L "$path" ]] || return 1
  done
  [[ -z "$(find "$UNIT_DIR" -mindepth 1 -name 'baft-tunnel*' -print -quit 2>/dev/null)" ]] || return 1
  ! systemctl list-unit-files --no-legend 'baft-tunnel*' 2>/dev/null | grep -q '^baft-tunnel' || return 1
  [[ -z "$(find /var/backups -maxdepth 1 \( -name 'baft-tunnel-install.*' -o -name 'baft-tunnel-backup.*' \) -print -quit 2>/dev/null)" ]] || return 1
  for account in baft-tunnel baft-tunnel-agent; do
    ! getent passwd "$account" >/dev/null 2>&1 || return 1
    ! getent group "$account" >/dev/null 2>&1 || return 1
  done
  return 0
}
if installation_absent; then
  echo 'auto-pair cleanup: installation already absent; nothing to remove'
  exit 0
fi

[[ -d "$EB" && ! -L "$EB" ]] || die "config root is missing or unsafe"
[[ "$(stat -c '%U:%G %a' "$EB")" == root:baft-tunnel\ 750 ]] || die "config root ownership or mode changed"
# This LAB never creates credential directories; preserve any that appear.
[[ ! -e "$EB/credentials" && ! -L "$EB/credentials" ]] || die "unexpected credentials directory appeared; preserving it"
[[ -d /run/baft-tunnel && ! -L /run/baft-tunnel ]] || die "runtime parent is missing or unsafe"
[[ -d "$CONTROL_DIR" && ! -L "$CONTROL_DIR" ]] || die "control directory is missing or unsafe"
[[ "$(stat -c '%U:%G %a' "$CONTROL_DIR")" == root:baft-tunnel-agent\ 770 ]] ||
  die "control directory ownership or mode changed"

while (($#)); do
  case "$1" in
    --backup-dir)
      (($# >= 2)) || die "--backup-dir needs a path"
      [[ "$2" =~ ^/var/backups/baft-tunnel-install\.[[:alnum:]]+$ ]] || die "unrecognized install backup path"
      BACKUP_DIRS+=("$2")
      shift 2
      ;;
    --instance)
      (($# >= 2)) || die "--instance needs a name"
      [[ "$2" =~ ^[A-Za-z0-9][A-Za-z0-9_-]{0,62}$ ]] || die "invalid instance name"
      INSTANCES+=("$2")
      shift 2
      ;;
    --echo-port)
      (($# >= 2)) || die "--echo-port needs a port"
      [[ -z "$AUTO_PAIR_ECHO_PORT" ]] || die "--echo-port may be supplied only once"
      [[ "$2" =~ ^[0-9]+$ ]] && (( 10#$2 >= 1 && 10#$2 <= 65535 )) || die "invalid automatic-pair echo port"
      AUTO_PAIR_ECHO_PORT="$2"
      shift 2
      ;;
    *) die "unexpected cleanup argument: $1" ;;
  esac
done

# Recover exact installer backup paths from per-instance run manifests if SSH lost the
# install helper's stdout after it had already recorded the path.
for instance in "${INSTANCES[@]}"; do
  manifest="$REMOTE_DIR/install-backup-$instance.path"
  if [[ -e "$manifest" || -L "$manifest" ]]; then
    [[ -f "$manifest" && ! -L "$manifest" ]] || die "unsafe backup manifest for $instance"
    [[ "$(stat -c '%U:%G %a' "$manifest")" == root:root\ 600 ]] || die "backup manifest ownership or mode changed"
    mapfile -t recorded_backups < "$manifest"
    [[ "${#recorded_backups[@]}" == 1 ]] || die "backup manifest for $instance must contain exactly one path"
    backup_dir="${recorded_backups[0]}"
    [[ "$backup_dir" =~ ^/var/backups/baft-tunnel-install\.[[:alnum:]]+$ ]] || die "backup manifest contains an unrecognized path"
    already_recorded=0
    for existing_backup in "${BACKUP_DIRS[@]}"; do
      [[ "$existing_backup" == "$backup_dir" ]] && already_recorded=1
    done
    (( already_recorded == 1 )) || BACKUP_DIRS+=("$backup_dir")
  fi
done

(( ${#INSTANCES[@]} > 0 )) || die "at least one explicit --instance is required"
(( ${#BACKUP_DIRS[@]} > 0 )) || die "no run-owned installer backup path was supplied or recovered"
for ((i=0; i<${#INSTANCES[@]}; i++)); do
  for ((j=i+1; j<${#INSTANCES[@]}; j++)); do
    [[ "${INSTANCES[i]}" != "${INSTANCES[j]}" ]] || die "duplicate instance argument: ${INSTANCES[i]}"
  done
done

[[ -f "$PKG/scripts/install-agent-control.sh" && -f "$PKG/scripts/uninstall.sh" ]] || die "run package is incomplete"
for manifest in installed-lib.sha256 installed-lib-dirs.txt \
  account-baft-tunnel.passwd account-baft-tunnel.group account-baft-tunnel.group.before \
  account-baft-tunnel-agent.passwd account-baft-tunnel-agent.group; do
  path="$REMOTE_DIR/$manifest"
  [[ -f "$path" && ! -L "$path" ]] || die "run-owned manifest is missing: $manifest"
  [[ "$(stat -c '%U:%G %a' "$path")" == root:root\ 600 ]] || die "run-owned manifest ownership or mode changed: $manifest"
done

for account in baft-tunnel-agent baft-tunnel; do
  [[ "$(getent passwd "$account" || true)" == "$(cat "$REMOTE_DIR/account-$account.passwd")" ]] ||
    die "$account account changed before cleanup; preserving installation"
  [[ "$(getent group "$account" || true)" == "$(cat "$REMOTE_DIR/account-$account.group")" ]] ||
    die "$account group changed before cleanup; preserving installation"
done

for instance in "${INSTANCES[@]}"; do
  listen_dropin="$UNIT_DIR/baft-tunnel-agent@$instance.service.d/listen.conf"
  listen_manifest="$REMOTE_DIR/agent-listen-$instance.sha256"
  if [[ -e "$listen_dropin" || -L "$listen_dropin" ]]; then
    [[ -f "$listen_dropin" && ! -L "$listen_dropin" ]] || die "agent listen drop-in has an unexpected type: $instance"
    [[ "$(stat -c '%U:%G %a' "$listen_dropin")" == root:root\ 644 ]] || die "agent listen drop-in ownership or mode changed: $instance"
    [[ -f "$listen_manifest" && ! -L "$listen_manifest" ]] || die "agent listen drop-in manifest is missing: $instance"
    [[ "$(stat -c '%U:%G %a' "$listen_manifest")" == root:root\ 600 ]] || die "agent listen manifest ownership or mode changed: $instance"
    expected_listen_sha="$(awk 'NR == 1 {print $1}' "$listen_manifest")"
    actual_listen_sha="$(sha256sum "$listen_dropin" | awk '{print $1}')"
    [[ "$expected_listen_sha" =~ ^[0-9a-f]{64}$ && "$actual_listen_sha" == "$expected_listen_sha" ]] ||
      die "agent listen drop-in changed; preserving it: $instance"
  elif [[ -e "$listen_manifest" || -L "$listen_manifest" ]]; then
    die "agent listen drop-in disappeared after its manifest was recorded: $instance"
  fi
done

# The systemd agent owns one isolated StateDirectory per LAB instance. Only remove
# directories carrying this run's root-owned marker, and only the exact token we recorded.
if [[ -e "$AGENT_STATE_DIR" || -L "$AGENT_STATE_DIR" ]]; then
  [[ -d "$AGENT_STATE_DIR" && ! -L "$AGENT_STATE_DIR" ]] || die "agent state root has an unexpected type"
  if [[ -e "$AGENT_STATE_DIR/token" || -L "$AGENT_STATE_DIR/token" ]]; then
    [[ -f "$AGENT_STATE_DIR/token" && ! -L "$AGENT_STATE_DIR/token" ]] || die "agent token has an unexpected type"
    [[ "$(stat -c '%U:%G %a' "$AGENT_STATE_DIR/token")" == baft-tunnel-agent:baft-tunnel-agent\ 600 ]] || die "agent token ownership or mode changed"
    token_manifest="$REMOTE_DIR/agent-token.sha256"
    [[ -f "$token_manifest" && ! -L "$token_manifest" ]] || die "agent token ownership manifest is missing"
    [[ "$(stat -c '%U:%G %a' "$token_manifest")" == root:root\ 600 ]] || die "agent token manifest ownership or mode changed"
    expected_token_sha="$(awk 'NR == 1 {print $1}' "$token_manifest")"
    actual_token_sha="$(sha256sum "$AGENT_STATE_DIR/token" | awk '{print $1}')"
    [[ "$expected_token_sha" =~ ^[0-9a-f]{64}$ && "$actual_token_sha" == "$expected_token_sha" ]] ||
      die "agent token changed; preserving it"
  elif [[ -e "$REMOTE_DIR/agent-token.sha256" || -L "$REMOTE_DIR/agent-token.sha256" ]]; then
    die "agent token disappeared after its manifest was recorded"
  fi
  for instance in "${INSTANCES[@]}"; do
    instance_dir="$AGENT_STATE_DIR/$instance"
    if [[ -e "$instance_dir" || -L "$instance_dir" ]]; then
      [[ -d "$instance_dir" && ! -L "$instance_dir" ]] || die "agent instance state has an unexpected type: $instance"
      [[ "$(stat -c '%U:%G %a' "$instance_dir")" == baft-tunnel-agent:baft-tunnel-agent\ 700 ]] ||
        die "agent instance state ownership or mode changed: $instance"
      marker="$instance_dir/.baft-tunnel-lab-run-owner"
      [[ -f "$marker" && ! -L "$marker" ]] || die "agent instance state has no run-ownership marker: $instance"
      [[ "$(stat -c '%U:%G %a' "$marker")" == root:root\ 600 ]] || die "agent state marker ownership or mode changed: $instance"
      [[ "$(cat "$marker")" == "$REMOTE_DIR" ]] || die "agent state marker belongs to another run: $instance"
      [[ -z "$(find "$instance_dir" -type l -print -quit)" ]] || die "agent state contains a symlink: $instance"
    fi
  done
fi

stop_pid_file() {
  local pid_file="$1" expected_marker="$2" pid args attempt
  [[ -e "$pid_file" || -L "$pid_file" ]] || return 0
  [[ -f "$pid_file" && ! -L "$pid_file" ]] || die "refusing unsafe PID file: $pid_file"
  read -r pid < "$pid_file" || die "cannot read PID file: $pid_file"
  [[ "$pid" =~ ^[0-9]+$ ]] || die "invalid PID in $pid_file"
  if kill -0 "$pid" 2>/dev/null; then
    args="$(ps -p "$pid" -o args= 2>/dev/null || true)"
    [[ "$args" == *"$expected_marker"* ]] || die "PID $pid no longer belongs to this LAB run; preserving it"
    pkill -P "$pid" 2>/dev/null || true
    kill "$pid" 2>/dev/null || true
    for attempt in $(seq 1 20); do
      kill -0 "$pid" 2>/dev/null || break
      sleep 0.1
    done
    kill -0 "$pid" 2>/dev/null && die "process $pid did not stop; preserving its state"
  fi
  rm -f -- "$pid_file"
}

# Stop only this run's instance units, then remove an explicit allowlist of its request files.
stop_if_active() {
  local unit="$1"
  systemctl stop "$unit" 2>/dev/null || true
  if systemctl is-active --quiet "$unit"; then
    die "unit remains active; preserving cleanup target: $unit"
  fi
  return 0
}
for instance in "${INSTANCES[@]}"; do
  stop_if_active "baft-tunnel@$instance.service"
  for verb in start stop enable disable setup pki pair fleet; do
    stop_if_active "baft-tunnel-control-$verb@$instance.path"
  done
  for verb in start stop enable disable setup pki pair fleet; do
    stop_if_active "baft-tunnel-control-$verb@$instance.service"
  done
  stop_if_active "baft-tunnel-agent@$instance.path"
  stop_if_active "baft-tunnel-agent@$instance.service"
  bash "$PKG/scripts/install-agent-control.sh" --remove --instance "$instance" >/dev/null 2>&1 || true
  for suffix in start stop enable disable setup setup-result pki pki-result pair pair-approve pair-status fleet; do
    request="$CONTROL_DIR/$instance.$suffix"
    if [[ -e "$request" || -L "$request" ]]; then
      [[ -f "$request" && ! -L "$request" ]] || die "unexpected control request type; preserving $request"
      rm -f -- "$request"
    fi
  done
  config_file="$EB/$instance.json"
  if [[ -e "$config_file" || -L "$config_file" ]]; then
    [[ -f "$config_file" && ! -L "$config_file" ]] || die "unexpected instance config type; preserving $config_file"
    rm -f -- "$config_file"
  fi

  systemctl reset-failed "baft-tunnel@$instance.service" "baft-tunnel-agent@$instance.service" 2>/dev/null || true
  for verb in start stop enable disable setup pki pair fleet; do
    systemctl reset-failed \
      "baft-tunnel-control-$verb@$instance.service" \
      "baft-tunnel-control-$verb@$instance.path" 2>/dev/null || true
  done
done

if [[ -n "$AUTO_PAIR_ECHO_PORT" ]]; then
  # Only the uniquely named echo process started by this automatic-pairing phase is ours.
  stop_pid_file "$REMOTE_DIR/ap-echo.pid" "$REMOTE_DIR/echo.py $AUTO_PAIR_ECHO_PORT"
  ! pgrep -f "[p]ython3 $REMOTE_DIR/echo.py $AUTO_PAIR_ECHO_PORT" >/dev/null 2>&1 ||
    die "automatic-pair echo process remains; preserving its state"
else
  [[ ! -e "$REMOTE_DIR/ap-echo.pid" && ! -L "$REMOTE_DIR/ap-echo.pid" ]] ||
    die "unexpected automatic-pair echo PID file on a host without an echo assignment; preserving it"
fi

# Remove only the known OpenSSL CA files produced by pair_ca::create/sign/crl.
if [[ -d "$EB/pair-ca" ]]; then
  [[ ! -L "$EB/pair-ca" ]] || die "pair CA path became a symlink; preserving it"
  for name in ca.key ca.crt ca.crl index.txt index.txt.attr index.txt.old index.txt.attr.old \
    serial serial.old crlnumber crlnumber.old members.json openssl.cnf; do
    path="$EB/pair-ca/$name"
    if [[ -e "$path" || -L "$path" ]]; then
      [[ -f "$path" && ! -L "$path" ]] || die "unexpected CA resource type at $path; preserving it"
      rm -f -- "$path"
    fi
  done
  if [[ -d "$EB/pair-ca/newcerts" ]]; then
    [[ ! -L "$EB/pair-ca/newcerts" ]] || die "CA newcerts path became a symlink; preserving it"
    for name in 1000.pem 1001.pem; do
      path="$EB/pair-ca/newcerts/$name"
      if [[ -e "$path" || -L "$path" ]]; then
        [[ -f "$path" && ! -L "$path" ]] || die "unexpected CA certificate type at $path; preserving it"
        rm -f -- "$path"
      fi
    done
    rmdir -- "$EB/pair-ca/newcerts" 2>/dev/null || die "unexpected files remain in CA newcerts; preserving them"
  fi
  rmdir -- "$EB/pair-ca" 2>/dev/null || die "unexpected files remain in pair CA; preserving them"
elif [[ -e "$EB/pair-ca" || -L "$EB/pair-ca" ]]; then
  die "pair CA path has an unexpected type; preserving it"
fi

for name in node.crt ca.crt ca.crl bundle.json peer-bundle.json pairing.json pairing-state.json config.json; do
  path="$EB/$name"
  if [[ -e "$path" || -L "$path" ]]; then
    [[ -f "$path" && ! -L "$path" ]] || die "unexpected pairing file type at $path; preserving it"
    rm -f -- "$path"
  fi
done
if [[ -d "$EB/pair-join" ]]; then
  [[ ! -L "$EB/pair-join" ]] || die "pair-join path became a symlink; preserving it"
  if [[ -e "$EB/pair-join/joiner.crt" || -L "$EB/pair-join/joiner.crt" ]]; then
    [[ -f "$EB/pair-join/joiner.crt" && ! -L "$EB/pair-join/joiner.crt" ]] ||
      die "unexpected pair-join certificate type; preserving it"
    rm -f -- "$EB/pair-join/joiner.crt"
  fi
  rmdir -- "$EB/pair-join" 2>/dev/null || die "unexpected pair-join files remain; preserving them"
elif [[ -e "$EB/pair-join" || -L "$EB/pair-join" ]]; then
  die "pair-join path has an unexpected type; preserving it"
fi

[[ ! -L "$EB/node.key" ]] || die "refusing symlinked run-owned node key"
rm -f -- "$EB/node.key"

if [[ -d "$AGENT_STATE_DIR" ]]; then
  ! pgrep -u baft-tunnel-agent >/dev/null 2>&1 || die "agent process remains; preserving its state"
  for instance in "${INSTANCES[@]}"; do
    instance_dir="$AGENT_STATE_DIR/$instance"
    if [[ -e "$instance_dir" || -L "$instance_dir" ]]; then
      [[ -d "$instance_dir" && ! -L "$instance_dir" ]] || die "agent instance state has an unexpected type: $instance"
      [[ "$(stat -c '%U:%G %a' "$instance_dir")" == baft-tunnel-agent:baft-tunnel-agent\ 700 ]] ||
        die "agent instance state ownership or mode changed during cleanup: $instance"
      marker="$instance_dir/.baft-tunnel-lab-run-owner"
      [[ -f "$marker" && ! -L "$marker" ]] || die "agent state marker disappeared; preserving $instance"
      [[ "$(stat -c '%U:%G %a' "$marker")" == root:root\ 600 ]] || die "agent state marker changed; preserving $instance"
      [[ "$(cat "$marker")" == "$REMOTE_DIR" ]] || die "agent state marker changed; preserving $instance"
      [[ -z "$(find "$instance_dir" -type l -print -quit)" ]] || die "agent state symlink appeared; preserving $instance"
      rm -rf -- "$instance_dir"
    fi
  done
  if [[ -e "$AGENT_STATE_DIR/token" || -L "$AGENT_STATE_DIR/token" ]]; then
    [[ -f "$AGENT_STATE_DIR/token" && ! -L "$AGENT_STATE_DIR/token" ]] || die "agent token changed; preserving it"
    [[ "$(stat -c '%U:%G %a' "$AGENT_STATE_DIR/token")" == baft-tunnel-agent:baft-tunnel-agent\ 600 ]] || die "agent token ownership or mode changed during cleanup"
    token_manifest="$REMOTE_DIR/agent-token.sha256"
    expected_token_sha="$(awk 'NR == 1 {print $1}' "$token_manifest")"
    actual_token_sha="$(sha256sum "$AGENT_STATE_DIR/token" | awk '{print $1}')"
    [[ "$actual_token_sha" == "$expected_token_sha" ]] || die "agent token changed during cleanup; preserving it"
    rm -f -- "$AGENT_STATE_DIR/token"
  fi
  rmdir -- "$AGENT_STATE_DIR" 2>/dev/null || die "unexpected agent state files remain; preserving them"
elif [[ -e "$AGENT_STATE_DIR" || -L "$AGENT_STATE_DIR" ]]; then
  die "agent state root has an unexpected type; preserving it"
fi

if [[ -d /var/lib/baft-tunnel-control ]]; then
  [[ ! -L /var/lib/baft-tunnel-control ]] || die "control state directory became a symlink; preserving it"
  [[ "$(stat -c '%U:%G %a' /var/lib/baft-tunnel-control)" == root:root\ 700 ]] ||
    die "control state directory ownership or mode changed; preserving it"
  if [[ -e /var/lib/baft-tunnel-control/migration-v1.units || -L /var/lib/baft-tunnel-control/migration-v1.units ]]; then
    die "control migration journal remains; preserving it for recovery"
  fi
  if [[ -e /var/lib/baft-tunnel-control/migration.lock || -L /var/lib/baft-tunnel-control/migration.lock ]]; then
    [[ -f /var/lib/baft-tunnel-control/migration.lock && ! -L /var/lib/baft-tunnel-control/migration.lock ]] ||
      die "control migration lock has an unexpected type; preserving it"
    [[ "$(stat -c '%U:%G %a' /var/lib/baft-tunnel-control/migration.lock)" == root:root\ 600 ]] ||
      die "control migration lock ownership or mode changed; preserving it"
    rm -f -- /var/lib/baft-tunnel-control/migration.lock
  fi
  rmdir -- /var/lib/baft-tunnel-control 2>/dev/null ||
    die "unexpected control state files remain; preserving them"
elif [[ -e /var/lib/baft-tunnel-control || -L /var/lib/baft-tunnel-control ]]; then
  die "control state path has an unexpected type; preserving it"
fi

for dir in "$EB/credentials" "$EB/pki" "$EB" "$STATE_DIR" /var/lib/baft-tunnel; do
  if [[ -d "$dir" ]]; then
    rmdir -- "$dir" 2>/dev/null || die "unexpected data remains in $dir; preserving it"
  elif [[ -e "$dir" || -L "$dir" ]]; then
    die "unexpected non-directory resource at $dir; preserving it"
  fi
done

if [[ -d "$CONTROL_DIR" ]]; then
  [[ ! -L "$CONTROL_DIR" ]] || die "control directory became a symlink; preserving it"
  find "$CONTROL_DIR" -mindepth 1 -print -quit | grep -q . &&
    die "unexpected control files remain; preserving runtime directory"
elif [[ -e "$CONTROL_DIR" || -L "$CONTROL_DIR" ]]; then
  die "control path has an unexpected type; preserving it"
fi
# /run/baft-tunnel holds the control request directory and the control-manage socket
# directory the product creates (tmpfiles entry plus baft-tunnel-control-manage@.socket).
# scripts/uninstall.sh removes both; any other runtime resource is still refused.
if [[ -d /run/baft-tunnel ]]; then
  if [[ -e "$MANAGE_DIR" || -L "$MANAGE_DIR" ]]; then
    [[ ! -L "$MANAGE_DIR" ]] || die "manage runtime directory became a symlink; preserving it"
    [[ -d "$MANAGE_DIR" ]] || die "manage runtime path has an unexpected type; preserving it"
    [[ "$(stat -c '%U' "$MANAGE_DIR")" == root ]] || die "manage runtime directory ownership changed; preserving it"
  fi
  for child in /run/baft-tunnel/*; do
    [[ -e "$child" || -L "$child" ]] || continue
    case "$child" in
      "$CONTROL_DIR" | "$MANAGE_DIR") ;;
      *) die "unexpected runtime resource $child; preserving it" ;;
    esac
  done
fi

# Refuse to uninstall if the binary, installed scripts, templates, or library tree changed.
[[ -f "$UB" && -x "$UB" && ! -L "$UB" ]] || die "installed binary is missing or unsafe"
installed_sha="$(sha256sum "$UB" | awk '{print $1}')"
[[ "$installed_sha" == "$EXPECTED_SHA" ]] || die "installed binary changed; preserving installation"
[[ ! -e "$ENGINE_STORE" && ! -L "$ENGINE_STORE" ]] || die "unexpected engine store appeared; preserving installation"
[[ -d /usr/local/lib/baft-tunnel && ! -L /usr/local/lib/baft-tunnel ]] || die "installed library directory is missing or unsafe"

[[ -z "$(find /usr/local/lib/baft-tunnel -type l -print -quit)" ]] ||
  die "installed library contains a symlink; preserving installation"
(
  cd /usr/local/lib/baft-tunnel
  find . -type f -print0 | sort -z | xargs -0 sha256sum
) > "$REMOTE_DIR/current-lib.sha256"
(
  cd /usr/local/lib/baft-tunnel
  find . -type d -print | sort
) > "$REMOTE_DIR/current-lib-dirs.txt"
cmp -s "$REMOTE_DIR/installed-lib.sha256" "$REMOTE_DIR/current-lib.sha256" ||
  die "installed library contents changed; preserving installation"
cmp -s "$REMOTE_DIR/installed-lib-dirs.txt" "$REMOTE_DIR/current-lib-dirs.txt" ||
  die "installed library directories changed; preserving installation"
rm -f -- "$REMOTE_DIR/current-lib.sha256" "$REMOTE_DIR/current-lib-dirs.txt"

cmp -s "$PKG/scripts/uninstall.sh" /usr/local/lib/baft-tunnel/scripts/uninstall.sh ||
  die "installed uninstall script changed; preserving installation"
cmp -s "$PKG/scripts/install-agent-control.sh" /usr/local/lib/baft-tunnel/scripts/install-agent-control.sh ||
  die "installed agent-control script changed; preserving installation"
cmp -s "$PKG/systemd/baft-tunnel@.service" /usr/local/lib/baft-tunnel/systemd/baft-tunnel@.service ||
  die "installed unit template changed; preserving installation"
cmp -s "$PKG/systemd/baft-tunnel@.service" "$UNIT_DIR/baft-tunnel@.service" ||
  die "systemd unit template changed; preserving installation"

for source in "$PKG"/systemd/control/baft-tunnel-control-*; do
  cmp -s "$source" "$UNIT_DIR/${source##*/}" ||
    die "control unit template changed; preserving installation"
done
for source in "$PKG"/systemd/agent/baft-tunnel-agent@.*; do
  cmp -s "$source" "$UNIT_DIR/${source##*/}" ||
    die "agent unit template changed; preserving installation"
done
{
  printf '%s\n' 'baft-tunnel@.service'
  for source in "$PKG"/systemd/control/baft-tunnel-control-* "$PKG"/systemd/agent/baft-tunnel-agent@.*; do
    printf '%s\n' "${source##*/}"
  done
} | sort > "$REMOTE_DIR/expected-unit-files.txt"
find "$UNIT_DIR" -mindepth 1 -maxdepth 1 -name 'baft-tunnel*' -printf '%f\n' | sort > "$REMOTE_DIR/actual-unit-files.txt"
cmp -s "$REMOTE_DIR/expected-unit-files.txt" "$REMOTE_DIR/actual-unit-files.txt" ||
  die "unexpected top-level BAFT-TUNNEL unit files remain; preserving installation"
cmp -s "$PKG/systemd/control/baft-tunnel-control.conf" /etc/tmpfiles.d/baft-tunnel-control.conf ||
  die "tmpfiles template changed; preserving installation"
if find "$UNIT_DIR" -mindepth 2 -name 'baft-tunnel*' -print -quit | grep -q .; then
  die "unexpected per-instance systemd resources remain; preserving installation"
fi

# The control directory was proved empty above. No config purge is used.
bash "$PKG/scripts/uninstall.sh" --stop >/dev/null
rmdir /run/baft-tunnel 2>/dev/null || true
systemctl daemon-reload

# Validate both accounts/groups before deleting either. Removing the agent account
# first restores the service group's original membership before its own deletion.
for account in baft-tunnel-agent baft-tunnel; do
  passwd_file="$REMOTE_DIR/account-$account.passwd"
  group_file="$REMOTE_DIR/account-$account.group"
  [[ -f "$passwd_file" && ! -L "$passwd_file" ]] || die "$account account has no run-owned manifest; preserving it"
  [[ -f "$group_file" && ! -L "$group_file" ]] || die "$account group has no run-owned manifest; preserving it"
  [[ "$(stat -c '%U:%G %a' "$passwd_file")" == root:root\ 600 ]] || die "$account passwd manifest changed; preserving it"
  [[ "$(stat -c '%U:%G %a' "$group_file")" == root:root\ 600 ]] || die "$account group manifest changed; preserving it"
  expected_passwd="$(cat "$passwd_file")"
  expected_group="$(cat "$group_file")"
  [[ -n "$expected_passwd" && "$(getent passwd "$account" || true)" == "$expected_passwd" ]] ||
    die "$account account changed; preserving it"
  [[ -n "$expected_group" && "$(getent group "$account" || true)" == "$expected_group" ]] ||
    die "$account group changed; preserving it"
  ! pgrep -u "$account" >/dev/null 2>&1 || die "$account process remains; preserving account"
done

userdel baft-tunnel-agent
groupdel baft-tunnel-agent
service_group_before="$REMOTE_DIR/account-baft-tunnel.group.before"
[[ -f "$service_group_before" && ! -L "$service_group_before" ]] || die "original service-group manifest is missing"
[[ "$(stat -c '%U:%G %a' "$service_group_before")" == root:root\ 600 ]] || die "original service-group manifest ownership or mode changed"
[[ "$(getent group baft-tunnel || true)" == "$(cat "$service_group_before")" ]] ||
  die "service group did not return to its original membership; preserving it"
userdel baft-tunnel
groupdel baft-tunnel

# Installer backups are removed only by exact path returned by this run, and only when empty.
for backup_dir in "${BACKUP_DIRS[@]}"; do
  if [[ -d "$backup_dir" ]]; then
    [[ ! -L "$backup_dir" ]] || die "backup path became a symlink; preserving it"
    [[ "$(stat -c '%U:%G %a' "$backup_dir")" == root:root\ 700 ]] ||
      die "backup directory ownership or mode changed; preserving it"
    scope_marker="$backup_dir/scope-binary"
    if [[ -e "$scope_marker" || -L "$scope_marker" ]]; then
      [[ -f "$scope_marker" && ! -L "$scope_marker" ]] || die "backup scope marker has an unexpected type"
      [[ "$(stat -c '%U:%G %a' "$scope_marker")" == root:root\ 600 && ! -s "$scope_marker" ]] ||
        die "backup scope marker changed; preserving backup"
      rm -f -- "$scope_marker"
    fi
    for unexpected in had-binary had-unit had-lib scope-engine-store; do
      [[ ! -e "$backup_dir/$unexpected" && ! -L "$backup_dir/$unexpected" ]] ||
        die "backup contains unexpected pre-existing resources; preserving it"
    done
    rmdir -- "$backup_dir" || die "backup directory is not empty; preserving it"
  elif [[ -e "$backup_dir" || -L "$backup_dir" ]]; then
    die "backup path has an unexpected type; preserving it"
  fi
done

if find /var/backups -maxdepth 1 \( -name 'baft-tunnel-install.*' -o -name 'baft-tunnel-backup.*' \) -print -quit | grep -q .; then
  die "untracked BAFT-TUNNEL backup remains; preserving it"
fi

# Final state must match the clean-host boundary before this helper reports success.
for path in "$UB" "$EB" /var/lib/baft-tunnel /var/lib/baft-tunnel-agent \
  /var/lib/baft-tunnel-control /run/baft-tunnel /usr/local/lib/baft-tunnel \
  "$ENGINE_STORE" /etc/tmpfiles.d/baft-tunnel-control.conf; do
  [[ ! -e "$path" && ! -L "$path" ]] || die "cleanup left $path"
done
! find "$UNIT_DIR" -name 'baft-tunnel*' -print -quit | grep -q . || die "BAFT-TUNNEL systemd units remain"
! systemctl list-unit-files --no-legend 'baft-tunnel*' 2>/dev/null | grep -q '^baft-tunnel' || die "BAFT-TUNNEL unit files remain"
! systemctl list-units --all --no-legend 'baft-tunnel*' 2>/dev/null | grep -q '^baft-tunnel' || die "BAFT-TUNNEL loaded units remain"
! systemctl list-units --all --state=failed --no-legend 'baft-tunnel*' 2>/dev/null | grep -q '^baft-tunnel' || die "BAFT-TUNNEL failed units remain"
! getent passwd baft-tunnel >/dev/null 2>&1 || die "baft-tunnel account remains"
! getent passwd baft-tunnel-agent >/dev/null 2>&1 || die "baft-tunnel-agent account remains"
! getent group baft-tunnel >/dev/null 2>&1 || die "baft-tunnel group remains"
! getent group baft-tunnel-agent >/dev/null 2>&1 || die "baft-tunnel-agent group remains"

echo "AUTO_PAIR_HOST_CLEANUP=PASS"
