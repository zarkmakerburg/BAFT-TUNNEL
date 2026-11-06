#!/usr/bin/env bash
set -Eeuo pipefail
umask 022

ROOT_PREFIX=""
BINARY=""
SERVICE_USER="baft-tunnel"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd -P)"
UNIT_SOURCE="$SCRIPT_DIR/../systemd/baft-tunnel@.service"

usage() {
  cat <<'EOF'
Usage: install.sh --binary PATH [--destdir ROOT]

Install the BAFT-TUNNEL binary and systemd template unit.
Without --destdir this requires root on a systemd Linux host, creates
the baft-tunnel service account and directories, and runs daemon-reload.
It does not enable or start any service instance.

--destdir is staging/test mode: no user, systemd, or host changes occur.
EOF
}
die() { echo "install: $*" >&2; exit 2; }

while (($#)); do
  case "$1" in
    --binary) (($# >= 2)) || die "--binary needs a path"; BINARY="$2"; shift 2 ;;
    --destdir) (($# >= 2)) || die "--destdir needs a path"; ROOT_PREFIX="$2"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) usage; die "unknown argument: $1" ;;
  esac
done

[[ -n "$BINARY" && -x "$BINARY" ]] || die "provide an executable release binary with --binary"
[[ -f "$UNIT_SOURCE" ]] || die "systemd template not found: $UNIT_SOURCE"
BINARY="$(cd "$(dirname "$BINARY")" && pwd -P)/$(basename "$BINARY")"
"$BINARY" --version >/dev/null 2>&1 || die "binary failed its --version check"

STAGING=0
if [[ -n "$ROOT_PREFIX" ]]; then
  [[ "$ROOT_PREFIX" != "/" ]] || die "--destdir cannot be /"
  mkdir -p "$ROOT_PREFIX"
  ROOT_PREFIX="$(cd "$ROOT_PREFIX" && pwd -P)"
  STAGING=1
else
  [[ "$(id -u)" == "0" ]] || die "run as root or use --destdir for staging"
  command -v systemctl >/dev/null 2>&1 || die "systemd/systemctl is required"
  command -v useradd >/dev/null 2>&1 || die "useradd is required"
fi

BIN_DIR="$ROOT_PREFIX/usr/local/bin"
UNIT_DIR="$ROOT_PREFIX/etc/systemd/system"
CONFIG_DIR="$ROOT_PREFIX/etc/baft-tunnel"
STATE_DIR="$ROOT_PREFIX/var/lib/baft-tunnel"
BACKUP_ROOT="$ROOT_PREFIX/var/backups"
mkdir -p "$BIN_DIR" "$UNIT_DIR" "$CONFIG_DIR/pki" "$STATE_DIR" "$BACKUP_ROOT"
chmod 0750 "$CONFIG_DIR" "$CONFIG_DIR/pki" "$STATE_DIR"

if [[ "$STAGING" == "0" ]]; then
  if ! getent group "$SERVICE_USER" >/dev/null 2>&1; then
    groupadd --system "$SERVICE_USER"
  fi
  if ! id "$SERVICE_USER" >/dev/null 2>&1; then
    useradd --system --gid "$SERVICE_USER" --home-dir "$STATE_DIR" --shell /usr/sbin/nologin "$SERVICE_USER"
  fi
  chown root:"$SERVICE_USER" "$CONFIG_DIR" "$CONFIG_DIR/pki"
  chown "$SERVICE_USER":"$SERVICE_USER" "$STATE_DIR"
fi

BACKUP_DIR="$(mktemp -d "$BACKUP_ROOT/baft-tunnel-install.XXXXXX")"
BIN_DEST="$BIN_DIR/baft-tunnel"
UNIT_DEST="$UNIT_DIR/baft-tunnel@.service"
[[ ! -e "$BIN_DEST" ]] || { cp -p "$BIN_DEST" "$BACKUP_DIR/baft-tunnel"; : > "$BACKUP_DIR/had-binary"; }
[[ ! -e "$UNIT_DEST" ]] || { cp -p "$UNIT_DEST" "$BACKUP_DIR/baft-tunnel@.service"; : > "$BACKUP_DIR/had-unit"; }
SUCCESS=0
rollback_install() {
  local status=$?
  trap - EXIT
  rm -f "$BIN_DEST.new.$$" "$UNIT_DEST.new.$$"
  if [[ "$SUCCESS" != "1" ]]; then
    if [[ -f "$BACKUP_DIR/had-binary" ]]; then cp -p "$BACKUP_DIR/baft-tunnel" "$BIN_DEST"; else rm -f "$BIN_DEST"; fi
    if [[ -f "$BACKUP_DIR/had-unit" ]]; then cp -p "$BACKUP_DIR/baft-tunnel@.service" "$UNIT_DEST"; else rm -f "$UNIT_DEST"; fi
    if [[ "$STAGING" == "0" ]]; then systemctl daemon-reload >/dev/null 2>&1 || true; fi
  fi
  exit "$status"
}
trap rollback_install EXIT

install -m 0755 "$BINARY" "$BIN_DEST.new.$$"
mv -f "$BIN_DEST.new.$$" "$BIN_DEST"
install -m 0644 "$UNIT_SOURCE" "$UNIT_DEST.new.$$"
mv -f "$UNIT_DEST.new.$$" "$UNIT_DEST"

if [[ "$STAGING" == "0" ]]; then
  if command -v systemd-analyze >/dev/null 2>&1; then
    systemd-analyze verify "$UNIT_DEST"
  fi
  systemctl daemon-reload
fi

SUCCESS=1
echo "Installed $BIN_DEST and $UNIT_DEST"
echo "Backup: $BACKUP_DIR"
echo "No service was enabled or started. Create /etc/baft-tunnel/NAME.json, then enable baft-tunnel@NAME.service explicitly."
