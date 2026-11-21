#!/usr/bin/env bash
# Remove BAFT-TUNNEL test installation artifacts from one explicitly authorized LAB host.
# The OS, SSH access, network configuration, firewall, routes, and unrelated services are untouched.
set -Eeuo pipefail
umask 077

die() { echo "clean-test-host: $*" >&2; exit 2; }
[[ "$#" == 1 && "$1" == CLEAN_BAFT_TUNNEL_TEST_HOST ]] || die "explicit CLEAN_BAFT_TUNNEL_TEST_HOST confirmation is required"
[[ "$(id -u)" == 0 ]] || die "must run as root"
[[ -r /etc/os-release ]] || die "cannot verify host OS"
. /etc/os-release
[[ "${ID:-}" == ubuntu && "${VERSION_ID:-}" == 22.04 ]] || die "expected Ubuntu 22.04 LAB host; found ${PRETTY_NAME:-unknown}"

printf 'BAFT_TEST_HOST_CLEANUP_BEGIN host=%s os=%s\n' "$(hostname)" "${VERSION_ID}"

# Stop every BAFT-TUNNEL unit before removing its installation and service identities.
mapfile -t units < <({
  systemctl list-units --all --plain --no-legend 'baft-tunnel*' 2>/dev/null | awk '{print $1}'
  systemctl list-unit-files --no-legend 'baft-tunnel*' 2>/dev/null | awk '{print $1}'
} | grep -E '^baft-tunnel[^[:space:]]*\.(service|path|socket|timer)$' | sort -u || true)
for unit in "${units[@]}"; do
  systemctl disable --now "$unit" >/dev/null 2>&1 || systemctl stop "$unit" >/dev/null 2>&1 || true
done
pkill -TERM -x baft-tunnel 2>/dev/null || true
sleep 1
pkill -KILL -x baft-tunnel 2>/dev/null || true

# Use the product's own full uninstaller when it is present; the explicit purge is authorized here.
installed_uninstaller=/usr/local/lib/baft-tunnel/scripts/uninstall.sh
if [[ -f "$installed_uninstaller" && ! -L "$installed_uninstaller" ]]; then
  bash "$installed_uninstaller" --stop --purge-config --yes
fi

# Remove only canonical BAFT-TUNNEL product paths (including the preflight-blocking runtime parent).
for path in \
  /etc/baft-tunnel \
  /var/lib/baft-tunnel \
  /var/lib/baft-tunnel-agent \
  /var/lib/baft-tunnel-control \
  /run/baft-tunnel \
  /usr/local/bin/baft-tunnel \
  /usr/bin/baft-tunnel \
  /usr/local/lib/baft-tunnel \
  /usr/local/lib/baft-tunnel-engines \
  /opt/baft-tunnel \
  /var/log/baft-tunnel; do
  rm -rf -- "$path"
done

# Remove BAFT-TUNNEL unit files, instance drop-ins, and wants symlinks only.
for unit_dir in /etc/systemd/system /usr/lib/systemd/system /lib/systemd/system; do
  if [[ -d "$unit_dir" && ! -L "$unit_dir" ]]; then
    find "$unit_dir" -depth -mindepth 1 -name 'baft-tunnel*' -exec rm -rf -- {} +
  fi
done
for tmpfiles_dir in /etc/tmpfiles.d /usr/lib/tmpfiles.d /lib/tmpfiles.d; do
  if [[ -d "$tmpfiles_dir" && ! -L "$tmpfiles_dir" ]]; then
    find "$tmpfiles_dir" -mindepth 1 -maxdepth 1 -name 'baft-tunnel*' -delete
  fi
done

# Backups are explicitly in the BAFT-TUNNEL namespace and are test-host artifacts.
if [[ -d /var/backups && ! -L /var/backups ]]; then
  find /var/backups -mindepth 1 -maxdepth 1 \
    \( -name 'baft-tunnel-install.*' -o -name 'baft-tunnel-backup.*' \) \
    -exec rm -rf -- {} +
fi

# These two identities are dedicated BAFT-TUNNEL service accounts on the LAB hosts.
for account in baft-tunnel-agent baft-tunnel; do
  if getent passwd "$account" >/dev/null 2>&1; then
    pkill -TERM -u "$account" 2>/dev/null || true
    sleep 1
    pkill -KILL -u "$account" 2>/dev/null || true
    userdel --remove "$account"
  fi
done
for group in baft-tunnel-agent baft-tunnel; do
  if getent group "$group" >/dev/null 2>&1; then groupdel "$group"; fi
done

systemctl daemon-reload
systemctl reset-failed 'baft-tunnel*' >/dev/null 2>&1 || true

# Fail closed if any canonical BAFT-TUNNEL installation artifact remains.
for path in \
  /etc/baft-tunnel /var/lib/baft-tunnel /var/lib/baft-tunnel-agent \
  /var/lib/baft-tunnel-control /run/baft-tunnel /usr/local/bin/baft-tunnel \
  /usr/bin/baft-tunnel /usr/local/lib/baft-tunnel /usr/local/lib/baft-tunnel-engines \
  /opt/baft-tunnel /var/log/baft-tunnel /etc/tmpfiles.d/baft-tunnel-control.conf; do
  [[ ! -e "$path" && ! -L "$path" ]] || die "cleanup left $path"
done
for unit_dir in /etc/systemd/system /usr/lib/systemd/system /lib/systemd/system; do
  ! find "$unit_dir" -name 'baft-tunnel*' -print -quit 2>/dev/null | grep -q . || die "BAFT-TUNNEL systemd files remain in $unit_dir"
done
! systemctl list-unit-files --no-legend 'baft-tunnel*' 2>/dev/null | grep -q '^baft-tunnel' || die 'BAFT-TUNNEL unit files remain'
! systemctl list-units --all --no-legend 'baft-tunnel*' 2>/dev/null | grep -q '^baft-tunnel' || die 'BAFT-TUNNEL loaded units remain'
! getent passwd baft-tunnel >/dev/null 2>&1 || die 'baft-tunnel account remains'
! getent passwd baft-tunnel-agent >/dev/null 2>&1 || die 'baft-tunnel-agent account remains'
! getent group baft-tunnel >/dev/null 2>&1 || die 'baft-tunnel group remains'
! getent group baft-tunnel-agent >/dev/null 2>&1 || die 'baft-tunnel-agent group remains'
! find /var/backups -maxdepth 1 \( -name 'baft-tunnel-install.*' -o -name 'baft-tunnel-backup.*' \) -print -quit 2>/dev/null | grep -q . || die 'BAFT-TUNNEL backups remain'
! pgrep -x baft-tunnel >/dev/null 2>&1 || die 'BAFT-TUNNEL process remains'

echo "BAFT_TEST_HOST_CLEANUP=PASS host=$(hostname)"
