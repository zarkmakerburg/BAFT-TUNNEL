#!/usr/bin/env bash
# Read-only inventory of existing BAFT-TUNNEL LAB hosts.
# No remote files are created, read for contents, or modified; no service/network changes.
set -Eeuo pipefail
umask 077

: "${SSH_KEY_FILE:?Missing SSH_KEY_FILE}"
: "${SSH_KNOWN_HOSTS_FILE:?Missing SSH_KNOWN_HOSTS_FILE}"
: "${LAB_IRAN_HOST:?Missing LAB_IRAN_HOST}"
: "${LAB_ABROAD_HOST:?Missing LAB_ABROAD_HOST}"

for name in SSH_KEY_FILE SSH_KNOWN_HOSTS_FILE; do
  value="${!name}"
  [[ -f "$value" && -r "$value" ]] || { echo "$name must reference a readable regular file" >&2; exit 2; }
done
[[ -s "$SSH_KNOWN_HOSTS_FILE" ]] || { echo 'known_hosts is empty' >&2; exit 2; }
[[ "$LAB_IRAN_HOST" != "$LAB_ABROAD_HOST" ]] || { echo 'LAB hosts must differ' >&2; exit 2; }

validate_host() {
  local value="$1"
  if [[ "$value" == *:* ]]; then
    python3 - "$value" <<'PY'
import ipaddress, sys
ipaddress.IPv6Address(sys.argv[1])
PY
  else
    [[ "$value" =~ ^[A-Za-z0-9.-]+$ ]] || return 1
  fi
}
validate_host "$LAB_IRAN_HOST" || { echo 'Invalid Iran host value' >&2; exit 2; }
validate_host "$LAB_ABROAD_HOST" || { echo 'Invalid abroad host value' >&2; exit 2; }

IRAN_USER="${LAB_IRAN_SSH_USER:-root}"
ABROAD_USER="${LAB_ABROAD_SSH_USER:-root}"
IRAN_PORT="${LAB_IRAN_SSH_PORT:-22}"
ABROAD_PORT="${LAB_ABROAD_SSH_PORT:-22}"
for user in "$IRAN_USER" "$ABROAD_USER"; do
  [[ "$user" =~ ^[A-Za-z_][A-Za-z0-9_.-]*[$]?$ ]] || { echo 'Invalid SSH username' >&2; exit 2; }
done
for port in "$IRAN_PORT" "$ABROAD_PORT"; do
  [[ "$port" =~ ^[0-9]+$ ]] && (( 10#$port >= 1 && 10#$port <= 65535 )) || { echo 'Invalid SSH port' >&2; exit 2; }
done

ssh_host() {
  local host="$1" user="$2" port="$3"
  ssh -i "$SSH_KEY_FILE" -p "$port" -o BatchMode=yes -o IdentitiesOnly=yes \
    -o StrictHostKeyChecking=yes -o UserKnownHostsFile="$SSH_KNOWN_HOSTS_FILE" \
    -o GlobalKnownHostsFile=/dev/null -o ConnectTimeout=12 \
    -o ServerAliveInterval=10 -o ServerAliveCountMax=2 -o LogLevel=ERROR \
    "$user@$host" 'bash -s'
}

inventory() {
  local label="$1" host="$2" user="$3" port="$4"
  printf '\n=== READ-ONLY INVENTORY: %s ===\n' "$label"
  ssh_host "$host" "$user" "$port" <<'REMOTE'
set -u
printf 'HOST_FACTS_BEGIN\n'
printf 'OS='
awk -F= '$1 == "PRETTY_NAME" {v=substr($0,index($0,"=")+1); gsub(/^"|"$/, "", v); print v; exit}' /etc/os-release 2>/dev/null || echo unknown
printf 'KERNEL=%s\n' "$(uname -sr 2>/dev/null || echo unknown)"
printf 'ARCH=%s\n' "$(uname -m 2>/dev/null || echo unknown)"
printf 'INIT=%s\n' "$(ps -p 1 -o comm= 2>/dev/null | tr -d ' ' || true)"
printf 'UPTIME='
uptime -p 2>/dev/null || true
printf 'CURRENT_UID=%s\n' "$(id -u 2>/dev/null || echo unknown)"

printf '\n-- BAFT-TUNNEL UNIT FILES (names and states only) --\n'
if command -v systemctl >/dev/null 2>&1; then
  systemctl list-unit-files --no-legend --no-pager 'baft-tunnel*' 2>/dev/null || true
  printf '%s\n' '-- loaded or known BAFT-TUNNEL services --'
  systemctl list-units --all --no-legend --no-pager 'baft-tunnel*' 2>/dev/null || true
else
  echo 'systemctl=missing'
fi

printf '\n-- BAFT-TUNNEL PROCESSES (process names only) --\n'
ps -eo comm= 2>/dev/null | awk 'tolower($0) ~ /baft[-_]tunnel/ {print}' | sort -u || true

printf '\n-- BAFT-TUNNEL PATH METADATA (no file contents) --\n'
paths=(
  /etc/baft-tunnel
  /etc/baft-tunnel/fleet-ca
  /etc/baft-tunnel/fleet-member
  /etc/baft-tunnel/fleet
  /var/lib/baft-tunnel
  /var/lib/baft-tunnel-agent
  /var/lib/baft-tunnel-control
  /run/baft-tunnel
  /run/baft-tunnel/control
  /opt/baft-tunnel
  /usr/local/bin/baft-tunnel
  /usr/bin/baft-tunnel
  /etc/systemd/system/baft-tunnel.service
  /etc/systemd/system/baft-tunnel-agent.service
)
for path in "${paths[@]}"; do
  if [ -L "$path" ]; then
    stat -c 'PATH=%n TYPE=symlink OWNER=%U GROUP=%G MODE=%a' "$path" 2>/dev/null || true
  elif [ -e "$path" ]; then
    stat -c 'PATH=%n TYPE=%F OWNER=%U GROUP=%G MODE=%a' "$path" 2>/dev/null || true
  else
    printf 'PATH=%s STATE=absent\n' "$path"
  fi
done

printf '\n-- CA / MEMBER DIRECTORY ENTRY NAMES (max depth 2; no contents) --\n'
for dir in /etc/baft-tunnel/fleet-ca /etc/baft-tunnel/fleet-member /etc/baft-tunnel/fleet; do
  if [ -d "$dir" ] && [ ! -L "$dir" ]; then
    printf 'DIRECTORY=%s\n' "$dir"
    find "$dir" -mindepth 1 -maxdepth 2 -printf '%y %f\n' 2>/dev/null | sort | head -80 || true
  fi
done

printf '\n-- LISTENING SOCKET SUMMARY --\n'
if command -v ss >/dev/null 2>&1; then
  ss -lntH 2>/dev/null | awk '{print $4}' | sort -u | head -100 || true
else
  echo 'ss=missing'
fi

printf '\n-- INSTALLED PACKAGE MATCHES --\n'
if command -v dpkg-query >/dev/null 2>&1; then
  dpkg-query -W -f='${binary:Package} ${Version}\n' 2>/dev/null | grep -Ei '^baft[-_]tunnel' || true
elif command -v rpm >/dev/null 2>&1; then
  rpm -qa 2>/dev/null | grep -Ei '^baft[-_]tunnel' || true
else
  echo 'package-query-tool=missing'
fi
printf 'HOST_FACTS_END\n'
REMOTE
}

inventory iran "$LAB_IRAN_HOST" "$IRAN_USER" "$IRAN_PORT"
inventory abroad "$LAB_ABROAD_HOST" "$ABROAD_USER" "$ABROAD_PORT"
printf '\nINVENTORY_COMPLETE: commands were read-only; no remote configuration, files, services, firewall, routes, or packages were changed.\n'
