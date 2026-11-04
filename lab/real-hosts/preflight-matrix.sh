#!/usr/bin/env bash
# Read-only six-host LAB preflight: 2 Iran nodes x 4 abroad nodes.
# No install, remote file writes, service changes, firewall changes, or reboot.
set -Eeuo pipefail

: "${SSH_KEY_FILE:?Set SSH_KEY_FILE to the dedicated BAFT-TUNNEL LAB private key}"
: "${SSH_KNOWN_HOSTS_FILE:?Set SSH_KNOWN_HOSTS_FILE to pinned host keys for all six hosts}"
command -v ssh >/dev/null || { echo 'Missing local dependency: ssh' >&2; exit 2; }
command -v python3 >/dev/null || { echo 'Missing local dependency: python3' >&2; exit 2; }

HOST_LABELS=(IR_1 IR_2 EX_1 EX_2 EX_3 EX_4)
HOSTS=()
for label in "${HOST_LABELS[@]}"; do
  var_name="${label}_HOST"
  value="${!var_name:-}"
  [[ -n "$value" ]] || { echo "Missing required host setting: $var_name" >&2; exit 2; }
  if [[ "$value" == *:* ]]; then
    python3 - "$value" <<'PY' >/dev/null 2>&1 || { echo "Invalid IPv6 address in $var_name" >&2; exit 2; }
import ipaddress, sys
ipaddress.IPv6Address(sys.argv[1])
PY
  else
    [[ "$value" =~ ^[A-Za-z0-9.-]+$ ]] || { echo "Invalid host value in $var_name" >&2; exit 2; }
  fi
  HOSTS+=("$value")
done

for ((i=0; i<${#HOSTS[@]}; i++)); do
  for ((j=i+1; j<${#HOSTS[@]}; j++)); do
    [[ "${HOSTS[$i]}" != "${HOSTS[$j]}" ]] || {
      echo "Duplicate host configured for ${HOST_LABELS[$i]} and ${HOST_LABELS[$j]}" >&2
      exit 2
    }
  done
done

SSH_USER="${SSH_USER:-bafttunnel-lab}"
SSH_PORT="${SSH_PORT:-22}"
LAB_TEST_PORTS="${LAB_TEST_PORTS:-443,8443,2053,18300}"
CONNECT_TIMEOUT="${CONNECT_TIMEOUT:-5}"
[[ "$SSH_USER" =~ ^[A-Za-z_][A-Za-z0-9_.-]*[$]?$ ]] || { echo 'Invalid SSH_USER' >&2; exit 2; }
[[ "$SSH_PORT" =~ ^[0-9]+$ ]] && (( SSH_PORT >= 1 && SSH_PORT <= 65535 )) || { echo 'Invalid SSH_PORT' >&2; exit 2; }
[[ "$CONNECT_TIMEOUT" =~ ^[0-9]+$ ]] && (( CONNECT_TIMEOUT >= 1 && CONNECT_TIMEOUT <= 30 )) || { echo 'Invalid CONNECT_TIMEOUT' >&2; exit 2; }
IFS=',' read -r -a PORTS <<< "$LAB_TEST_PORTS"
(( ${#PORTS[@]} > 0 )) || { echo 'LAB_TEST_PORTS must not be empty' >&2; exit 2; }
for port in "${PORTS[@]}"; do
  [[ "$port" =~ ^[0-9]+$ ]] && (( port >= 1 && port <= 65535 )) || { echo 'Invalid port in LAB_TEST_PORTS' >&2; exit 2; }
done

[[ -f "$SSH_KEY_FILE" && -r "$SSH_KEY_FILE" ]] || { echo 'SSH key must be a readable regular file' >&2; exit 2; }
[[ -f "$SSH_KNOWN_HOSTS_FILE" && -s "$SSH_KNOWN_HOSTS_FILE" && -r "$SSH_KNOWN_HOSTS_FILE" ]] || { echo 'known_hosts must be a readable, non-empty regular file' >&2; exit 2; }

umask 077
SSH_OPTS=(-i "$SSH_KEY_FILE" -p "$SSH_PORT" -o BatchMode=yes -o IdentitiesOnly=yes
  -o StrictHostKeyChecking=yes -o UserKnownHostsFile="$SSH_KNOWN_HOSTS_FILE"
  -o ConnectTimeout=12 -o ServerAliveInterval=10 -o ServerAliveCountMax=2 -o LogLevel=ERROR)

remote_facts() {
  local label="$1" host="$2"
  echo "=== HOST PREFLIGHT: $label ==="
  ssh "${SSH_OPTS[@]}" "$SSH_USER@$host" 'bash -s' <<'REMOTE'
set -eu
. /etc/os-release 2>/dev/null || true
printf 'OS=%s\n' "${PRETTY_NAME:-unknown}"
printf 'KERNEL=%s\n' "$(uname -sr)"
printf 'ARCH=%s\n' "$(uname -m)"
printf 'INIT=%s\n' "$(ps -p 1 -o comm= 2>/dev/null | tr -d ' ' || true)"
for tool in systemctl nft ss python3 tc; do
  if command -v "$tool" >/dev/null 2>&1; then printf 'TOOL_%s=present\n' "$tool"; else printf 'TOOL_%s=missing\n' "$tool"; fi
done
printf 'PREFLIGHT_REMOTE=ok\n'
REMOTE
}

for i in "${!HOSTS[@]}"; do remote_facts "${HOST_LABELS[$i]}" "${HOSTS[$i]}"; done

echo '=== IRAN -> ABROAD CONNECTIVITY MATRIX ==='
for ir_index in 0 1; do
  for ex_index in 2 3 4 5; do
    ir_label="${HOST_LABELS[$ir_index]}"; ex_label="${HOST_LABELS[$ex_index]}"
    ir_host="${HOSTS[$ir_index]}"; ex_host="${HOSTS[$ex_index]}"
    echo "--- $ir_label -> $ex_label ---"
    port_csv=$(IFS=,; echo "${PORTS[*]}")
    ssh "${SSH_OPTS[@]}" "$SSH_USER@$ir_host" \
      "python3 - '$ex_host' '$port_csv' '$CONNECT_TIMEOUT'" <<'REMOTE'
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
  done
done

echo 'MATRIX PREFLIGHT COMPLETE: 6 hosts inspected; 8 Iran-to-abroad paths checked; no remote state changed.'
