#!/usr/bin/env bash
set -Eeuo pipefail
usage() {
  cat <<'EOF'
Usage: uninstall.sh [--destdir ROOT] [--stop] [--purge-config --yes]

Removes the binary and systemd template. Config, keys, state, and service
account are retained unless --purge-config --yes is explicitly supplied.
Running/enabled instances must be stopped with --stop first.
EOF
}
die() { echo "uninstall: $*" >&2; exit 2; }
ROOT_PREFIX=""
STOP=0
PURGE=0
YES=0
while (($#)); do
  case "$1" in
    --destdir) (($# >= 2)) || die "--destdir needs a path"; ROOT_PREFIX="$2"; shift 2 ;;
    --stop) STOP=1; shift ;;
    --purge-config) PURGE=1; shift ;;
    --yes) YES=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) usage; die "unknown argument: $1" ;;
  esac
done
if [[ "$PURGE" == "1" && "$YES" != "1" ]]; then die "--purge-config requires --yes"; fi
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

if [[ "$STAGING" == "0" ]]; then
  mapfile -t units < <(
    {
      systemctl list-units --state=active --plain --no-legend 'baft-tunnel@*.service' | awk '{print $1}'
      systemctl list-unit-files --type=service --no-legend 'baft-tunnel@*.service' | awk '$2 == "enabled" && $1 ~ /^baft-tunnel@.+\.service$/ {print $1}'
    } | sort -u
  )
  if (("${#units[@]}" > 0)); then
    [[ "$STOP" == "1" ]] || die "BAFT-TUNNEL instances exist; rerun with --stop"
    systemctl disable --now "${units[@]}"
  fi
fi
rm -f "$ROOT_PREFIX/usr/local/bin/baft-tunnel" "$ROOT_PREFIX/etc/systemd/system/baft-tunnel@.service"
if [[ "$PURGE" == "1" ]]; then
  rm -rf "$ROOT_PREFIX/etc/baft-tunnel" "$ROOT_PREFIX/var/lib/baft-tunnel"
fi
if [[ "$STAGING" == "0" ]]; then systemctl daemon-reload; fi
echo "Removed binary and systemd template. Service account was retained."
if [[ "$PURGE" == "0" ]]; then echo "Configuration, private keys, and state were retained."; fi
