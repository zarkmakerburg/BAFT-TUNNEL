#!/usr/bin/env bash
# Restore a backup made by backup.sh (ADR-0025). Verifies first, snapshots current state, swaps atomically per tree.
set -Eeuo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd -P)"
usage() {
  cat <<'EOF'
Usage: restore.sh [--destdir ROOT] [--stop] --yes BACKUP_DIR

Verifies BACKUP_DIR/backup.tar against its SHA-256 and a path/type allowlist
(only /etc/baft-tunnel, /var/lib/baft-tunnel and baft-tunnel@*.service.d; no
links or devices), takes a pre-restore snapshot of the current state, then
replaces those trees. Credentials are forced to root-only modes. Running
instances must be stopped first (or pass --stop). Nothing is started.
EOF
}
die() { echo "restore: $*" >&2; exit 2; }
ROOT_PREFIX=""; STOP=0; YES=0; BACKUP_DIR=""
while (($#)); do
  case "$1" in
    --destdir) (($# >= 2)) || die "--destdir needs a path"; ROOT_PREFIX="$2"; shift 2 ;;
    --stop) STOP=1; shift ;;
    --yes) YES=1; shift ;;
    -h|--help) usage; exit 0 ;;
    -*) usage >&2; die "unknown argument: $1" ;;
    *) [[ -z "$BACKUP_DIR" ]] || die "only one backup directory"; BACKUP_DIR="$1"; shift ;;
  esac
done
[[ -n "$BACKUP_DIR" ]] || { usage >&2; exit 2; }
[[ "$YES" == "1" ]] || die "restore replaces current configuration and keys; pass --yes"
[[ -d "$BACKUP_DIR" && ! -L "$BACKUP_DIR" ]] || die "backup directory not found"
STAGING=0
if [[ -n "$ROOT_PREFIX" ]]; then
  [[ "$ROOT_PREFIX" != "/" ]] || die "--destdir cannot be /"
  mkdir -p "$ROOT_PREFIX"; ROOT_PREFIX="$(cd "$ROOT_PREFIX" && pwd -P)"; STAGING=1
else
  [[ "$(id -u)" == "0" ]] || die "run as root or use --destdir"
  command -v systemctl >/dev/null 2>&1 || die "systemd/systemctl is required"
fi
umask 077
TAR="$BACKUP_DIR/backup.tar"
[[ -f "$TAR" && ! -L "$TAR" && -f "$BACKUP_DIR/backup.tar.sha256" ]] || die "backup.tar or backup.tar.sha256 missing"
(cd "$BACKUP_DIR" && sha256sum --check --status backup.tar.sha256) || die "checksum mismatch; refusing to restore"

# Allowlist: names and entry types. Only regular files and directories.
mapfile -t names < <(tar -tf "$TAR")
mapfile -t verbose < <(tar -tvf "$TAR")
((${#names[@]} > 0 && ${#names[@]} == ${#verbose[@]})) || die "unreadable or ambiguous archive listing"
name_re='^(etc/baft-tunnel|var/lib/baft-tunnel|var/lib/baft-tunnel-agent|etc/systemd/system/baft-tunnel@[A-Za-z0-9][A-Za-z0-9_.-]*\.service\.d)(/[^[:cntrl:]]*)?$'
for i in "${!names[@]}"; do
  n="${names[$i]}"; n="${n%/}"
  [[ "$n" =~ $name_re && "$n" != *"/../"* && "$n" != */.. && "$n" != *"/./"* ]] || die "archive entry outside the allowlist"
  [[ "${verbose[$i]:0:1}" == "-" || "${verbose[$i]:0:1}" == "d" ]] || die "archive contains a link or special file"
done
grep -qx 'etc/baft-tunnel/\?' <(printf '%s\n' "${names[@]}") || die "archive has no etc/baft-tunnel"

if [[ "$STAGING" == "0" ]]; then
  mapfile -t active < <(systemctl list-units --state=active,activating --plain --no-legend 'baft-tunnel@*.service' | awk '{print $1}')
  if ((${#active[@]} > 0)); then
    [[ "$STOP" == "1" ]] || die "BAFT-TUNNEL instances are running; stop them or pass --stop"
    systemctl stop "${active[@]}"
  fi
fi

SNAP_ARGS=()
[[ -z "$ROOT_PREFIX" ]] || SNAP_ARGS=(--destdir "$ROOT_PREFIX")
SNAP_OUT="$("$SCRIPT_DIR/backup.sh" "${SNAP_ARGS[@]}")" || die "could not take the pre-restore snapshot"
SNAPSHOT="$(printf '%s\n' "$SNAP_OUT" | sed -n 's/^Backup: //p' | tail -n 1)"

BACKUP_ROOT="$ROOT_PREFIX/var/backups"
STAGE="$(mktemp -d "$BACKUP_ROOT/.baft-restore.XXXXXX")"
mkdir "$STAGE/new" "$STAGE/old"
OWNER_OPT=--no-same-owner
[[ "$(id -u)" != "0" ]] || OWNER_OPT=--same-owner
tar -xpf "$TAR" -C "$STAGE/new" --numeric-owner "$OWNER_OPT"

declare -a MOVED=()
undo() {
  local i rel
  for ((i = ${#MOVED[@]} - 1; i >= 0; i--)); do
    rel="${MOVED[$i]}"
    rm -rf "$ROOT_PREFIX/${rel:?}"
    if [[ -e "$STAGE/old/$rel" ]]; then mv "$STAGE/old/$rel" "$ROOT_PREFIX/$rel"; fi
  done
}
finish() {
  local status=$?
  trap - EXIT
  if ((status != 0)); then
    undo || true
    echo "restore: failed; previous state put back (snapshot: ${SNAPSHOT:-none})" >&2
  fi
  rm -rf "$STAGE"
  exit "$status"
}
trap finish EXIT

swap() { # swap REL: move current REL aside (if any), move the restored one in
  local rel="$1"
  mkdir -p "$(dirname "$ROOT_PREFIX/$rel")" "$(dirname "$STAGE/old/$rel")"
  MOVED+=("$rel")
  if [[ -e "$ROOT_PREFIX/$rel" ]]; then mv "$ROOT_PREFIX/$rel" "$STAGE/old/$rel"; fi
  if [[ -e "$STAGE/new/$rel" ]]; then mv "$STAGE/new/$rel" "$ROOT_PREFIX/$rel"; fi
}
swap etc/baft-tunnel
[[ ! -d "$STAGE/new/var/lib/baft-tunnel" ]] || swap var/lib/baft-tunnel
[[ ! -d "$STAGE/new/var/lib/baft-tunnel-agent" ]] || swap var/lib/baft-tunnel-agent
# Instance drop-ins mirror the backup exactly: current ones go aside, backed-up ones come in.
shopt -s nullglob
for d in "$ROOT_PREFIX"/etc/systemd/system/baft-tunnel@*.service.d "$STAGE"/new/etc/systemd/system/baft-tunnel@*.service.d; do
  rel="etc/systemd/system/$(basename "$d")"
  [[ " ${MOVED[*]} " == *" $rel "* ]] || swap "$rel"
done
shopt -u nullglob

# Never trust archived modes for secrets.
CRED="$ROOT_PREFIX/etc/baft-tunnel/credentials"
if [[ -d "$CRED" ]]; then
  find "$CRED" -type d -exec chmod 0700 {} +
  find "$CRED" -type f -exec chmod 0600 {} +
  if [[ "$STAGING" == "0" ]]; then chown -R root:root "$CRED"; fi
fi
if [[ "$STAGING" == "0" ]]; then systemctl daemon-reload; fi
trap - EXIT
rm -rf "$STAGE"
echo "Restored from $BACKUP_DIR"
echo "Pre-restore snapshot: $SNAPSHOT (root-only; delete when no longer needed)"
echo "No service was started or restarted."
