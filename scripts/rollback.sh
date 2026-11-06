#!/usr/bin/env bash
set -Eeuo pipefail
usage() {
  echo "Usage: rollback.sh [--destdir ROOT] BACKUP_DIR" >&2
}
die() { echo "rollback: $*" >&2; exit 2; }
ROOT_PREFIX=""
while (($#)); do
  case "$1" in
    --destdir) (($# >= 2)) || die "--destdir needs a path"; ROOT_PREFIX="$2"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) break ;;
  esac
done
(($# == 1)) || { usage; exit 2; }
BACKUP_DIR="$1"
[[ -d "$BACKUP_DIR" ]] || die "backup directory not found"
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
BIN_DEST="$ROOT_PREFIX/usr/local/bin/baft-tunnel"
UNIT_DEST="$ROOT_PREFIX/etc/systemd/system/baft-tunnel@.service"
mkdir -p "$(dirname "$BIN_DEST")" "$(dirname "$UNIT_DEST")"
if [[ -f "$BACKUP_DIR/had-binary" ]]; then
  install -m 0755 "$BACKUP_DIR/baft-tunnel" "$BIN_DEST.rollback.$$"
  mv -f "$BIN_DEST.rollback.$$" "$BIN_DEST"
else
  rm -f "$BIN_DEST"
fi
if [[ -f "$BACKUP_DIR/had-unit" ]]; then
  install -m 0644 "$BACKUP_DIR/baft-tunnel@.service" "$UNIT_DEST.rollback.$$"
  mv -f "$UNIT_DEST.rollback.$$" "$UNIT_DEST"
else
  rm -f "$UNIT_DEST"
fi
if [[ "$STAGING" == "0" ]]; then systemctl daemon-reload; fi
echo "Previous binary/unit restored. No service was started or restarted."
