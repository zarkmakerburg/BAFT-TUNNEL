#!/usr/bin/env bash
# Guarded binary update with automatic rollback (ADR-0025).
set -Eeuo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd -P)"
usage() {
  cat <<'EOF'
Usage: update.sh --binary PATH [--instance NAME]... [--health-secs N]
                 [--health-cmd EXECUTABLE] [--destdir ROOT]

Updates the installed baft-tunnel binary and unit template:
  1. backs up configuration, PKI, keys and state (backup.sh),
  2. installs the new binary (install.sh keeps the previous one),
  3. restarts the instances that were running (default: all active
     baft-tunnel@*.service; or the named --instance values),
  4. requires each to stay active with no restarts for --health-secs
     (default 10) and, if given, --health-cmd NAME to exit 0.
Before step 1, an installed agent control helper is moved to the dedicated
request-directory boundary (install-agent-control.sh --upgrade-boundary);
if that refuses, nothing is updated.
If any step fails, rollback.sh restores the previous binary and unit, the same
instances are restarted and re-verified, and the exit status is 1. If the
rollback itself is unhealthy the exit status is 3. Configuration and keys are
never modified; a digest check proves it. Instances that were stopped stay stopped.
With --destdir (staging) systemd is not used; the health check is
"<binary> --version" plus --health-cmd.
EOF
}
die() { echo "update: $*" >&2; exit 2; }
ROOT_PREFIX=""; BINARY=""; HEALTH_SECS=10; HEALTH_CMD=""; declare -a INSTANCES=()
while (($#)); do
  case "$1" in
    --binary) (($# >= 2)) || die "--binary needs a path"; BINARY="$2"; shift 2 ;;
    --instance) (($# >= 2)) || die "--instance needs a name"; INSTANCES+=("$2"); shift 2 ;;
    --health-secs) (($# >= 2)) || die "--health-secs needs a number"; HEALTH_SECS="$2"; shift 2 ;;
    --health-cmd) (($# >= 2)) || die "--health-cmd needs a path"; HEALTH_CMD="$2"; shift 2 ;;
    --destdir) (($# >= 2)) || die "--destdir needs a path"; ROOT_PREFIX="$2"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) usage >&2; die "unknown argument: $1" ;;
  esac
done
[[ -n "$BINARY" ]] || { usage >&2; die "--binary is required"; }
[[ "$HEALTH_SECS" =~ ^[0-9]{1,3}$ ]] || die "--health-secs must be a number of seconds"
[[ -z "$HEALTH_CMD" || -x "$HEALTH_CMD" ]] || die "--health-cmd must be an executable"
for n in "${INSTANCES[@]}"; do
  [[ "$n" =~ ^[A-Za-z0-9][A-Za-z0-9_.-]{0,62}$ && "$n" != *..* ]] || die "invalid instance name"
done
STAGING=0; DEST_ARGS=()
if [[ -n "$ROOT_PREFIX" ]]; then
  [[ "$ROOT_PREFIX" != "/" ]] || die "--destdir cannot be /"
  mkdir -p "$ROOT_PREFIX"; ROOT_PREFIX="$(cd "$ROOT_PREFIX" && pwd -P)"; STAGING=1; DEST_ARGS=(--destdir "$ROOT_PREFIX")
else
  [[ "$(id -u)" == "0" ]] || die "run as root or use --destdir"
  command -v systemctl >/dev/null 2>&1 || die "systemd/systemctl is required"
fi
BIN_DEST="$ROOT_PREFIX/usr/local/bin/baft-tunnel"
[[ -x "$BIN_DEST" && -f "$ROOT_PREFIX/etc/systemd/system/baft-tunnel@.service" ]] || die "no existing installation to update; use install.sh"
[[ -x "$BINARY" ]] || die "provide an executable release binary with --binary"
"$BINARY" --version >/dev/null 2>&1 || die "new binary failed its --version check"
OLD_VERSION="$("$BIN_DEST" --version 2>/dev/null | head -n 1 || echo unknown)"
NEW_VERSION="$("$BINARY" --version 2>/dev/null | head -n 1)"

if [[ "$STAGING" == "0" && ${#INSTANCES[@]} -eq 0 ]]; then
  mapfile -t units < <(systemctl list-units --state=active,activating --plain --no-legend 'baft-tunnel@*.service' | awk '{print $1}')
  for u in "${units[@]}"; do u="${u#baft-tunnel@}"; INSTANCES+=("${u%.service}"); done
fi

state_digest() { # config, PKI and credentials: a digest only, never contents
  local dir="$ROOT_PREFIX/etc/baft-tunnel"
  (cd "$dir" && find . -type f -print0 | LC_ALL=C sort -z | xargs -0 -r sha256sum | sha256sum | awk '{print $1}')
}
unit_of() { printf 'baft-tunnel@%s.service' "$1"; }

healthy() { # healthy: every instance stays active with no automatic restarts for HEALTH_SECS
  local n t ok=1
  declare -A nr0=()
  if [[ "$STAGING" == "0" ]]; then
    for n in "${INSTANCES[@]}"; do nr0[$n]="$(systemctl show -p NRestarts --value "$(unit_of "$n")")"; done
    for ((t = 0; t < HEALTH_SECS; t++)); do
      for n in "${INSTANCES[@]}"; do
        systemctl is-active --quiet "$(unit_of "$n")" || { echo "update: $n is not active" >&2; return 1; }
      done
      sleep 1
    done
    for n in "${INSTANCES[@]}"; do
      [[ "$(systemctl show -p ActiveState --value "$(unit_of "$n")")" == "active" ]] || ok=0
      [[ "$(systemctl show -p NRestarts --value "$(unit_of "$n")")" == "${nr0[$n]}" ]] || { echo "update: $n restarted during the health window" >&2; ok=0; }
    done
    ((ok == 1)) || return 1
  else
    "$BIN_DEST" --version >/dev/null 2>&1 || { echo "update: installed binary fails --version" >&2; return 1; }
  fi
  if [[ -n "$HEALTH_CMD" ]]; then
    for n in "${INSTANCES[@]}"; do "$HEALTH_CMD" "$n" >/dev/null 2>&1 || { echo "update: health command failed for $n" >&2; return 1; }; done
  fi
}
restart_all() {
  local n
  [[ "$STAGING" == "0" ]] || return 0
  systemctl daemon-reload
  for n in "${INSTANCES[@]}"; do
    systemctl reset-failed "$(unit_of "$n")" >/dev/null 2>&1 || true
    systemctl restart "$(unit_of "$n")" || { echo "update: restart of $n failed" >&2; return 1; }
  done
  # Running agents pick up the new binary too (not part of the tunnel health gate).
  systemctl try-restart 'baft-tunnel-agent@*.service' >/dev/null 2>&1 || true
}

# Existing installs get the control-request boundary (root:baft-tunnel-agent 0770) here, before the
# binary changes. Fail-closed: on any refusal nothing else is updated and the reason is printed.
CONTROL_BOUNDARY="$("$SCRIPT_DIR/install-agent-control.sh" --upgrade-boundary "${DEST_ARGS[@]}" | sed -n 's/^CONTROL_BOUNDARY=//p' | tail -n 1)" ||
  die "control-directory boundary migration failed; binary not updated. Resolve the reported condition and rerun update.sh"
[[ -n "$CONTROL_BOUNDARY" ]] || die "control-directory boundary state unknown; binary not updated"
DIGEST_BEFORE="$(state_digest)"
DATA_BACKUP="$("$SCRIPT_DIR/backup.sh" "${DEST_ARGS[@]}" | sed -n 's/^Backup: //p' | tail -n 1)"
[[ -d "$DATA_BACKUP" ]] || die "data backup failed; nothing was changed"
INSTALL_OUT="$("$SCRIPT_DIR/install.sh" --binary "$BINARY" "${DEST_ARGS[@]}")" || die "install failed (install.sh restored the previous files)"
BIN_BACKUP="$(printf '%s\n' "$INSTALL_OUT" | sed -n 's/^Backup: //p' | tail -n 1)"
[[ -d "$BIN_BACKUP" ]] || die "installer reported no backup directory"

if restart_all && healthy; then
  [[ "$(state_digest)" == "$DIGEST_BEFORE" ]] || { echo "update: WARNING config/keys digest changed during update" >&2; exit 3; }
  echo "UPDATE_OK from='$OLD_VERSION' to='$NEW_VERSION' instances=${#INSTANCES[@]} config_keys_digest=unchanged control_boundary=$CONTROL_BOUNDARY"
  echo "Data backup: $DATA_BACKUP"
  echo "Binary backup: $BIN_BACKUP"
  echo "Both are root-only; delete them when rollback is no longer needed."
  exit 0
fi

echo "update: new version failed; rolling back to '$OLD_VERSION'" >&2
"$SCRIPT_DIR/rollback.sh" "${DEST_ARGS[@]}" "$BIN_BACKUP" >/dev/null || { echo "update: ROLLBACK FAILED; restore manually from $BIN_BACKUP" >&2; exit 3; }
if restart_all && healthy && [[ "$(state_digest)" == "$DIGEST_BEFORE" ]]; then
  echo "UPDATE_FAILED_ROLLED_BACK restored='$OLD_VERSION' rejected='$NEW_VERSION' config_keys_digest=unchanged" >&2
  echo "Data backup: $DATA_BACKUP" >&2
  exit 1
fi
echo "update: ROLLBACK COMPLETED BUT THE PREVIOUS VERSION IS NOT HEALTHY; data backup: $DATA_BACKUP, binary backup: $BIN_BACKUP" >&2
exit 3
