#!/usr/bin/env bash
# Read-only Phase 0 two-host LAB preflight: one Iran node and one abroad node.
# No install, remote file writes, service changes, firewall changes, or reboot.
set -Eeuo pipefail

: "${SSH_KEY_FILE:?Set SSH_KEY_FILE to the dedicated BAFT-TUNNEL LAB private key}"
: "${SSH_KNOWN_HOSTS_FILE:?Set SSH_KNOWN_HOSTS_FILE to verified host keys for both hosts}"
command -v ssh >/dev/null || { echo 'Missing local dependency: ssh' >&2; exit 2; }
command -v python3 >/dev/null || { echo 'Missing local dependency: python3' >&2; exit 2; }

IRAN_HOST="${LAB_IRAN_HOST:-}"
ABROAD_HOST="${LAB_ABROAD_HOST:-}"
[[ -n "$IRAN_HOST" ]] || { echo 'Missing required host setting: LAB_IRAN_HOST' >&2; exit 2; }
[[ -n "$ABROAD_HOST" ]] || { echo 'Missing required host setting: LAB_ABROAD_HOST' >&2; exit 2; }
[[ "$IRAN_HOST" != "$ABROAD_HOST" ]] || { echo 'Iran and abroad hosts must be different' >&2; exit 2; }

validate_host() {
  local label="$1" value="$2"
  if [[ "$value" == *:* ]]; then
    python3 - "$value" <<'PY' >/dev/null 2>&1 || { echo "Invalid IPv6 address for $label" >&2; exit 2; }
import ipaddress, sys
ipaddress.IPv6Address(sys.argv[1])
PY
  else
    [[ "$value" =~ ^[A-Za-z0-9.-]+$ ]] || { echo "Invalid host value for $label" >&2; exit 2; }
  fi
}
validate_host LAB_IRAN_HOST "$IRAN_HOST"
validate_host LAB_ABROAD_HOST "$ABROAD_HOST"

IRAN_USER="${LAB_IRAN_SSH_USER:-root}"
ABROAD_USER="${LAB_ABROAD_SSH_USER:-root}"
IRAN_PORT="${LAB_IRAN_SSH_PORT:-22}"
ABROAD_PORT="${LAB_ABROAD_SSH_PORT:-22}"
LAB_TEST_PORTS="${LAB_TEST_PORTS:-443,8443,2053,18300}"
CONNECT_TIMEOUT="${CONNECT_TIMEOUT:-5}"

for item in "$IRAN_USER" "$ABROAD_USER"; do
  [[ "$item" =~ ^[A-Za-z_][A-Za-z0-9_.-]*[$]?$ ]] || { echo 'Invalid SSH username' >&2; exit 2; }
done
for port in "$IRAN_PORT" "$ABROAD_PORT"; do
  [[ "$port" =~ ^[0-9]+$ ]] && (( port >= 1 && port <= 65535 )) || { echo 'Invalid SSH port' >&2; exit 2; }
done
[[ "$CONNECT_TIMEOUT" =~ ^[0-9]+$ ]] && (( CONNECT_TIMEOUT >= 1 && CONNECT_TIMEOUT <= 30 )) || { echo 'Invalid CONNECT_TIMEOUT' >&2; exit 2; }
IFS=',' read -r -a PORTS <<< "$LAB_TEST_PORTS"
(( ${#PORTS[@]} > 0 )) || { echo 'LAB_TEST_PORTS must not be empty' >&2; exit 2; }
for port in "${PORTS[@]}"; do
  [[ "$port" =~ ^[0-9]+$ ]] && (( port >= 1 && port <= 65535 )) || { echo 'Invalid port in LAB_TEST_PORTS' >&2; exit 2; }
done

[[ -f "$SSH_KEY_FILE" && -r "$SSH_KEY_FILE" ]] || { echo 'SSH key must be a readable regular file' >&2; exit 2; }
[[ -f "$SSH_KNOWN_HOSTS_FILE" && -s "$SSH_KNOWN_HOSTS_FILE" && -r "$SSH_KNOWN_HOSTS_FILE" ]] || { echo 'known_hosts must be a readable, non-empty regular file' >&2; exit 2; }

umask 077
ssh_opts() {
  local key="$1" port="$2"
  SSH_OPTS=(-i "$key" -p "$port" -o BatchMode=yes -o IdentitiesOnly=yes
    -o StrictHostKeyChecking=yes -o UserKnownHostsFile="$SSH_KNOWN_HOSTS_FILE"
    -o ConnectTimeout=12 -o ServerAliveInterval=10 -o ServerAliveCountMax=2 -o LogLevel=ERROR)
}
remote_facts() {
  local label="$1" host="$2" user="$3" port="$4"
  echo "=== HOST PREFLIGHT: $label ==="
  ssh_opts "$SSH_KEY_FILE" "$port"
  ssh "${SSH_OPTS[@]}" "$user@$host" 'bash -s' <<'REMOTE'
set -eu
os_name=$(awk -F= '$1 == "PRETTY_NAME" { value=substr($0,index($0,"=")+1); sub(/^"/,"",value); sub(/"$/,"",value); print value; exit }' /etc/os-release 2>/dev/null || true)
printf 'OS=%s\n' "${os_name:-unknown}"
printf 'KERNEL=%s\n' "$(uname -sr)"
printf 'ARCH=%s\n' "$(uname -m)"
printf 'INIT=%s\n' "$(ps -p 1 -o comm= 2>/dev/null | tr -d ' ' || true)"
for tool in systemctl nft ss python3 tc; do
  if command -v "$tool" >/dev/null 2>&1; then printf 'TOOL_%s=present\n' "$tool"; else printf 'TOOL_%s=missing\n' "$tool"; fi
done
printf 'PREFLIGHT_REMOTE=ok\n'
REMOTE
}

remote_facts iran "$IRAN_HOST" "$IRAN_USER" "$IRAN_PORT"
remote_facts abroad "$ABROAD_HOST" "$ABROAD_USER" "$ABROAD_PORT"

echo '=== IRAN -> ABROAD TCP CONNECTIVITY ==='
ssh_opts "$SSH_KEY_FILE" "$IRAN_PORT"
port_csv=$(IFS=,; echo "${PORTS[*]}")
ssh "${SSH_OPTS[@]}" "$IRAN_USER@$IRAN_HOST" \
  "python3 - '$ABROAD_HOST' '$port_csv' '$CONNECT_TIMEOUT'" <<'REMOTE'
import socket, sys, time
host, raw_ports, timeout = sys.argv[1], sys.argv[2], float(sys.argv[3])
for port in (int(p) for p in raw_ports.split(',')):
    started = time.monotonic()
    try:
        with socket.create_connection((host, port), timeout=timeout):
            state = 'tcp-open'
    except ConnectionRefusedError:
        state = 'host-reachable-no-listener'
    except socket.timeout:
        state = 'timeout-or-filtered'
    except OSError as exc:
        state = 'unreachable-' + type(exc).__name__
    print(f'PORT_{port}={state} RTT_MS={(time.monotonic()-started)*1000:.1f}')
REMOTE

echo 'TWO-HOST PREFLIGHT COMPLETE: Iran and abroad hosts inspected; one Iran-to-abroad path checked; no remote state changed.'
