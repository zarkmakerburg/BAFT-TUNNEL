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

# Backups from older installers carry no scope markers and always covered the
# binary and unit. Newer backups name what the install touched.
SCOPE_BINARY=0
if [[ -f "$BACKUP_DIR/scope-binary" || ! -f "$BACKUP_DIR/scope-credential" ]]; then SCOPE_BINARY=1; fi

if [[ "$SCOPE_BINARY" == "1" ]]; then
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
fi

if [[ -f "$BACKUP_DIR/scope-credential" ]]; then
  INSTANCE="$(head -n 1 "$BACKUP_DIR/scope-credential")"
  [[ "$INSTANCE" =~ ^[A-Za-z0-9][A-Za-z0-9_.-]{0,62}$ && "$INSTANCE" != *..* ]] || die "backup names an invalid instance"
  CRED_DIR="$ROOT_PREFIX/etc/baft-tunnel/credentials/$INSTANCE"
  DROPIN_DIR="$ROOT_PREFIX/etc/systemd/system/baft-tunnel@$INSTANCE.service.d"
  OWNER_ARGS=()
  if [[ "$STAGING" == "0" ]]; then OWNER_ARGS=(-o root -g root); fi
  install -d "${OWNER_ARGS[@]}" -m 0700 "$ROOT_PREFIX/etc/baft-tunnel/credentials" "$CRED_DIR"
  for name in node-key node-key.cred; do
    if [[ -f "$BACKUP_DIR/credential/$name" ]]; then
      (umask 077; install "${OWNER_ARGS[@]}" -m 0600 "$BACKUP_DIR/credential/$name" "$CRED_DIR/$name.rollback.$$")
      mv -f "$CRED_DIR/$name.rollback.$$" "$CRED_DIR/$name"
    else
      rm -f "$CRED_DIR/$name"
    fi
  done
  if [[ -f "$BACKUP_DIR/credential/credentials.conf" ]]; then
    install -d -m 0755 "$DROPIN_DIR"
    install -m 0644 "$BACKUP_DIR/credential/credentials.conf" "$DROPIN_DIR/credentials.conf.rollback.$$"
    mv -f "$DROPIN_DIR/credentials.conf.rollback.$$" "$DROPIN_DIR/credentials.conf"
  else
    rm -f "$DROPIN_DIR/credentials.conf"
    rmdir "$DROPIN_DIR" 2>/dev/null || true
  fi
  echo "Credential state for instance $INSTANCE restored."
fi

if [[ "$STAGING" == "0" ]]; then systemctl daemon-reload; fi
echo "Rollback complete. No service was started or restarted."
