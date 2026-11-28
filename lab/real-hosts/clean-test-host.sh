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

# An interrupted or failed two-host E2E round leaves its own helpers in
# /tmp/baft-tunnel-e2e-<run id>/; those helpers hold the fixed LAB ports (echo 19090,
# local forward 18080, impairment 19100), so every later round refuses at its port
# preflight and the whole LAB blocks on a leftover process. Remove only this
# repository's own test artifacts: directories matching that exact pattern and processes
# whose command line starts with the same path. Nothing else on the host is signalled.
e2e_pattern_dir='^/tmp/baft-tunnel-e2e-[0-9]+$'
e2e_pattern_python='^python3 /tmp/baft-tunnel-e2e-[0-9]+/(echo|echo2|impair|probe|agent)\.py( |$)'
e2e_pattern_binary='^/tmp/baft-tunnel-e2e-[0-9]+/(baft-tunnel|baft-engine)( |$)'
mapfile -t e2e_dirs < <(find /tmp -mindepth 1 -maxdepth 1 -name 'baft-tunnel-e2e-[0-9]*' -print 2>/dev/null || true)
for dir in "${e2e_dirs[@]}"; do
  [[ "$dir" =~ $e2e_pattern_dir ]] || die "unexpected E2E leftover path: $dir"
  [[ ! -L "$dir" ]] || die "E2E leftover is a symlink: $dir"
  [[ -d "$dir" ]] || die "E2E leftover is not a directory: $dir"
  [[ "$(stat -c '%U' "$dir")" == root ]] || die "E2E leftover directory is not root-owned: $dir"
done
for pattern in "$e2e_pattern_python" "$e2e_pattern_binary"; do
  pkill -TERM -f "$pattern" 2>/dev/null || true
done
sleep 1
for pattern in "$e2e_pattern_python" "$e2e_pattern_binary"; do
  pkill -KILL -f "$pattern" 2>/dev/null || true
done
if ((${#e2e_dirs[@]})); then rm -rf -- "${e2e_dirs[@]}"; fi
for pattern in "$e2e_pattern_python" "$e2e_pattern_binary"; do
  ! pgrep -f "$pattern" >/dev/null 2>&1 || die 'an E2E helper from an earlier run is still running'
done
for dir in "${e2e_dirs[@]}"; do
  [[ ! -e "$dir" && ! -L "$dir" ]] || die "E2E leftover directory remains: $dir"
done

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
! pgrep -f "$e2e_pattern_python" >/dev/null 2>&1 || die 'E2E python helper from an earlier run remains'
! pgrep -f "$e2e_pattern_binary" >/dev/null 2>&1 || die 'E2E binary from an earlier run remains'
! find /tmp -mindepth 1 -maxdepth 1 -name 'baft-tunnel-e2e-[0-9]*' -print -quit 2>/dev/null | grep -q . || die 'E2E run directory remains'

echo "BAFT_TEST_HOST_CLEANUP=PASS host=$(hostname)"
