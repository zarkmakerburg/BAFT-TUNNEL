#!/usr/bin/env bash
# Install the agent control helper (ADR-0023): lets the unprivileged agent ask systemd, through
# request files, to start or stop exactly baft-tunnel@NAME. No polkit, no sudo, no wildcard grant.
#   install-agent-control.sh --instance NAME [--destdir ROOT]
#   install-agent-control.sh --remove --instance NAME [--destdir ROOT]
set -Eeuo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SRC="$SCRIPT_DIR/../systemd/control"
ROOT_PREFIX=""; INSTANCE=""; REMOVE=0
die() { echo "install-agent-control: $*" >&2; exit 2; }
while (($#)); do
  case "$1" in
    --instance) (($# >= 2)) || die "--instance needs a name"; INSTANCE="$2"; shift 2 ;;
    --destdir) (($# >= 2)) || die "--destdir needs a path"; ROOT_PREFIX="$2"; shift 2 ;;
    --remove) REMOVE=1; shift ;;
    -h|--help) sed -n 2,5p "$0"; exit 0 ;;
    *) die "unknown argument: $1" ;;
  esac
done
# The name becomes part of a unit name and a file name: letters, digits, '_' and '-' only.
[[ "$INSTANCE" =~ ^[A-Za-z0-9][A-Za-z0-9_-]{0,62}$ ]] || die "--instance must be 1-63 characters of A-Z a-z 0-9 _ - and start alphanumeric"
STAGING=0; [[ -n "$ROOT_PREFIX" ]] && STAGING=1
if [[ "$STAGING" == "0" ]]; then
  [[ "$(id -u)" == "0" ]] || die "must run as root"
  getent group baft-tunnel >/dev/null || die "group baft-tunnel does not exist; run install.sh first"
fi
UNIT_DIR="$ROOT_PREFIX/etc/systemd/system"
TMPF="$ROOT_PREFIX/etc/tmpfiles.d/baft-tunnel-control.conf"
FILES=(baft-tunnel-control-start@.path baft-tunnel-control-start@.service baft-tunnel-control-stop@.path baft-tunnel-control-stop@.service)
if [[ "$REMOVE" == "1" ]]; then
  if [[ "$STAGING" == "0" ]]; then
    systemctl disable --now "baft-tunnel-control-start@$INSTANCE.path" "baft-tunnel-control-stop@$INSTANCE.path" 2>/dev/null || true
  fi
  rm -f "$ROOT_PREFIX/etc/systemd/system/multi-user.target.wants/baft-tunnel-control-start@$INSTANCE.path" \
        "$ROOT_PREFIX/etc/systemd/system/multi-user.target.wants/baft-tunnel-control-stop@$INSTANCE.path"
  echo "Removed the control path units for $INSTANCE. Template units and tmpfiles are kept for other instances."
  exit 0
fi
install -d -m 0755 "$UNIT_DIR" "$(dirname "$TMPF")"
for f in "${FILES[@]}"; do install -m 0644 "$SRC/$f" "$UNIT_DIR/$f"; done
install -m 0644 "$SRC/baft-tunnel-control.conf" "$TMPF"
if [[ "$STAGING" == "0" ]]; then
  systemd-tmpfiles --create "$TMPF"
  systemctl daemon-reload
  systemctl enable --now "baft-tunnel-control-start@$INSTANCE.path" "baft-tunnel-control-stop@$INSTANCE.path"
fi
echo "Installed the agent control helper for instance $INSTANCE."
echo "The agent can now request start/stop of baft-tunnel@$INSTANCE only. Nothing was started."
