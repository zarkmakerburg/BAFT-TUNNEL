#!/usr/bin/env bash
# Back up BAFT-TUNNEL configuration, PKI, node-key credentials, instance drop-ins and state (ADR-0025).
# The binary is not part of a data backup; install.sh/update.sh keep their own binary backups.
set -Eeuo pipefail
usage() {
  cat <<'EOF'
Usage: backup.sh [--destdir ROOT]

Creates a root-only directory /var/backups/baft-tunnel-backup.XXXXXX (0700) holding
backup.tar (0600), backup.tar.sha256 and manifest.txt (file names and modes only).
The archive contains node private keys; keep it root-only and delete it when no
longer needed. Prints "Backup: DIR". Nothing is stopped, started or changed.
EOF
}
die() { echo "backup: $*" >&2; exit 2; }
ROOT_PREFIX=""
while (($#)); do
  case "$1" in
    --destdir) (($# >= 2)) || die "--destdir needs a path"; ROOT_PREFIX="$2"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) usage >&2; die "unknown argument: $1" ;;
  esac
done
if [[ -n "$ROOT_PREFIX" ]]; then
  [[ "$ROOT_PREFIX" != "/" ]] || die "--destdir cannot be /"
  mkdir -p "$ROOT_PREFIX"
  ROOT_PREFIX="$(cd "$ROOT_PREFIX" && pwd -P)"
else
  [[ "$(id -u)" == "0" ]] || die "run as root or use --destdir"
fi
umask 077
[[ -d "$ROOT_PREFIX/etc/baft-tunnel" ]] || die "nothing to back up: $ROOT_PREFIX/etc/baft-tunnel does not exist"
[[ ! -L "$ROOT_PREFIX/etc/baft-tunnel" ]] || die "refusing symlinked /etc/baft-tunnel"

paths=(etc/baft-tunnel)
[[ ! -d "$ROOT_PREFIX/var/lib/baft-tunnel" ]] || paths+=(var/lib/baft-tunnel)
# Agent token, receipts, desired state and audit log (baft-tunnel-agent@NAME, SA-09).
[[ ! -d "$ROOT_PREFIX/var/lib/baft-tunnel-agent" ]] || paths+=(var/lib/baft-tunnel-agent)
shopt -s nullglob
for dropin in "$ROOT_PREFIX"/etc/systemd/system/baft-tunnel@*.service.d; do
  paths+=("etc/systemd/system/$(basename "$dropin")")
done
shopt -u nullglob

BACKUP_ROOT="$ROOT_PREFIX/var/backups"
mkdir -p "$BACKUP_ROOT"
DIR="$(mktemp -d "$BACKUP_ROOT/baft-tunnel-backup.XXXXXX")"
chmod 0700 "$DIR"
trap 'rm -rf "$DIR"' ERR
tar --numeric-owner -C "${ROOT_PREFIX:-/}" -cpf "$DIR/backup.tar" "${paths[@]}"
chmod 0600 "$DIR/backup.tar"
(cd "$DIR" && sha256sum backup.tar > backup.tar.sha256 && chmod 0600 backup.tar.sha256)
{
  echo "created_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  bin="$ROOT_PREFIX/usr/local/bin/baft-tunnel"
  if [[ -x "$bin" ]]; then echo "binary_version=$("$bin" --version 2>/dev/null | head -n 1 || echo unknown)"; fi
  echo "--- entries (names and modes only; no contents) ---"
  tar -tvf "$DIR/backup.tar"
} > "$DIR/manifest.txt"
chmod 0600 "$DIR/manifest.txt"
trap - ERR
echo "Backup: $DIR"
echo "Contains private keys; directory is root-only (0700). Binary is not included."
