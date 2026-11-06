#!/usr/bin/env bash
# Temporary two-host BAFT-TUNNEL E2E. No firewall, route, sysctl, or service-manager changes.
set -Eeuo pipefail
umask 077

: "${LAB_IRAN_HOST:?Missing LAB_IRAN_HOST}"
: "${LAB_ABROAD_HOST:?Missing LAB_ABROAD_HOST}"
: "${SSH_KEY_FILE:?Missing SSH_KEY_FILE}"
: "${SSH_KNOWN_HOSTS_FILE:?Missing SSH_KNOWN_HOSTS_FILE}"

IRAN_USER="${LAB_IRAN_SSH_USER:-root}"
ABROAD_USER="${LAB_ABROAD_SSH_USER:-root}"
IRAN_SSH_PORT="${LAB_IRAN_SSH_PORT:-22}"
ABROAD_SSH_PORT="${LAB_ABROAD_SSH_PORT:-22}"
E2E_PORT="${LAB_E2E_PORT:-18300}"
LOCAL_PORT="${LAB_E2E_LOCAL_PORT:-18080}"
ECHO_PORT=19090
REMOTE_DIR="/tmp/baft-tunnel-e2e-${GITHUB_RUN_ID:-$$}"
BINARY="${BAFT_TUNNEL_BINARY:-target/release/baft-tunnel}"
LOCAL_TMP="$(mktemp -d)"

validate_ip() {
  python3 - "$1" <<'PY'
import ipaddress, sys
ipaddress.ip_address(sys.argv[1])
PY
}
validate_port() {
  [[ "$1" =~ ^[0-9]+$ ]] && (( 1 <= 10#$1 && 10#$1 <= 65535 ))
}
validate_user() {
  [[ "$1" =~ ^[A-Za-z_][A-Za-z0-9_.-]*[$]?$ ]]
}
validate_ip "$LAB_IRAN_HOST"
validate_ip "$LAB_ABROAD_HOST"
[[ "$LAB_IRAN_HOST" != "$LAB_ABROAD_HOST" ]] || { echo "LAB hosts must differ" >&2; exit 2; }
validate_user "$IRAN_USER"
validate_user "$ABROAD_USER"
validate_port "$IRAN_SSH_PORT"
validate_port "$ABROAD_SSH_PORT"
validate_port "$E2E_PORT"
validate_port "$LOCAL_PORT"
[[ -x "$BINARY" ]] || { echo "Linux release binary is missing: $BINARY" >&2; exit 2; }
[[ -r "$SSH_KEY_FILE" && -r "$SSH_KNOWN_HOSTS_FILE" ]] || { echo "SSH credentials are not readable" >&2; exit 2; }

ssh_host() {
  local side="$1" command="$2" host user port
  if [[ "$side" == iran ]]; then
    host="$LAB_IRAN_HOST"; user="$IRAN_USER"; port="$IRAN_SSH_PORT"
  else
    host="$LAB_ABROAD_HOST"; user="$ABROAD_USER"; port="$ABROAD_SSH_PORT"
  fi
  ssh -i "$SSH_KEY_FILE" -p "$port" -o BatchMode=yes -o IdentitiesOnly=yes \
    -o StrictHostKeyChecking=yes -o UserKnownHostsFile="$SSH_KNOWN_HOSTS_FILE" \
    -o ConnectTimeout=12 -o ServerAliveInterval=10 -o ServerAliveCountMax=2 \
    -o LogLevel=ERROR "$user@$host" "$command"
}
copy_to_host() {
  local side="$1"; shift
  local host user port
  if [[ "$side" == iran ]]; then
    host="$LAB_IRAN_HOST"; user="$IRAN_USER"; port="$IRAN_SSH_PORT"
  else
    host="$LAB_ABROAD_HOST"; user="$ABROAD_USER"; port="$ABROAD_SSH_PORT"
  fi
  scp -i "$SSH_KEY_FILE" -P "$port" -o BatchMode=yes -o IdentitiesOnly=yes     -o StrictHostKeyChecking=yes -o UserKnownHostsFile="$SSH_KNOWN_HOSTS_FILE"     -o ConnectTimeout=12 -o LogLevel=ERROR "$@" "$user@$host:$REMOTE_DIR/"
}
stop_role() {
  local side="$1" role="$2"
  ssh_host "$side" "if [ -s '$REMOTE_DIR/$role.pid' ]; then kill \$(cat '$REMOTE_DIR/$role.pid') 2>/dev/null || true; rm -f '$REMOTE_DIR/$role.pid'; fi" || true
}
start_echo() {
  local side="$1"
  ssh_host "$side" "nohup python3 '$REMOTE_DIR/echo.py' '$ECHO_PORT' > '$REMOTE_DIR/echo.log' 2>&1 < /dev/null & echo \$! > '$REMOTE_DIR/echo.pid'"
}
start_serve() {
  local side="$1" local_id="$2" peer_id="$3" direction="$4" peer_pin="$5" cert="$6" key="$7"
  ssh_host "$side" "nohup '$REMOTE_DIR/baft-tunnel' serve --listen '0.0.0.0:$E2E_PORT' --target '127.0.0.1:$ECHO_PORT' --local-id '$local_id' --peer-id '$peer_id' --tunnel-id 'lab-e2e' --direction '$direction' --peer-fingerprint '$peer_pin' --ca '$REMOTE_DIR/ca.crt' --cert '$REMOTE_DIR/$cert' --key '$REMOTE_DIR/$key' --max-streams 8 > '$REMOTE_DIR/serve.log' 2>&1 < /dev/null & echo \$! > '$REMOTE_DIR/serve.pid'"
}
start_forward() {
  local side="$1" local_id="$2" peer_id="$3" peer_host="$4" peer_dns="$5" direction="$6" peer_pin="$7" cert="$8" key="$9"
  ssh_host "$side" "nohup '$REMOTE_DIR/baft-tunnel' forward --listen '127.0.0.1:$LOCAL_PORT' --remote '$peer_host:$E2E_PORT' --server-name '$peer_dns' --local-id '$local_id' --peer-id '$peer_id' --tunnel-id 'lab-e2e' --direction '$direction' --peer-fingerprint '$peer_pin' --ca '$REMOTE_DIR/ca.crt' --cert '$REMOTE_DIR/$cert' --key '$REMOTE_DIR/$key' --max-streams 8 > '$REMOTE_DIR/forward.log' 2>&1 < /dev/null & echo \$! > '$REMOTE_DIR/forward.pid'"
}
probe_host() {
  local side="$1" bytes="${2:-2097289}"
  ssh_host "$side" "python3 '$REMOTE_DIR/probe.py' 127.0.0.1 '$LOCAL_PORT' '$bytes'"
}
cleanup() {
  local status=$?
  trap - EXIT
  if (( status != 0 )); then
    for side in iran abroad; do
      ssh_host "$side" "for f in serve.log forward.log echo.log; do if [ -f '$REMOTE_DIR/'\$f ]; then echo === $side:\$f ===; tail -n 80 '$REMOTE_DIR/'\$f; fi; done" || true
    done
  fi
  for side in iran abroad; do
    ssh_host "$side" "for role in serve forward echo; do f='$REMOTE_DIR/'\$role'.pid'; if [ -s \"\$f\" ]; then kill \$(cat \"\$f\") 2>/dev/null || true; fi; done; sleep 0.2; rm -rf '$REMOTE_DIR'" || status=1
  done
  rm -rf "$LOCAL_TMP"
  if (( status != 0 )); then
    echo "LAB cleanup or E2E failed; inspect run logs and host process state." >&2
  fi
  exit "$status"
}
trap cleanup EXIT

# Generate a short-lived private LAB CA and two node certificates. The CA key stays on the runner.
openssl req -x509 -newkey rsa:3072 -nodes -keyout "$LOCAL_TMP/ca.key" -out "$LOCAL_TMP/ca.crt" \
  -days 2 -sha256 -subj "/CN=BAFT-TUNNEL ephemeral LAB CA" \
  -addext "basicConstraints=critical,CA:TRUE" \
  -addext "keyUsage=critical,keyCertSign,cRLSign" >/dev/null 2>&1
for node in iran abroad; do
  openssl req -new -newkey rsa:2048 -nodes -keyout "$LOCAL_TMP/$node.key" \
    -out "$LOCAL_TMP/$node.csr" -subj "/CN=$node.lab" >/dev/null 2>&1
  cat > "$LOCAL_TMP/$node.ext" <<EOF
basicConstraints=critical,CA:FALSE
keyUsage=critical,digitalSignature
extendedKeyUsage=serverAuth,clientAuth
subjectAltName=DNS:$node.lab,URI:urn:baft-tunnel:node:$node
EOF
  openssl x509 -req -in "$LOCAL_TMP/$node.csr" -CA "$LOCAL_TMP/ca.crt" \
    -CAkey "$LOCAL_TMP/ca.key" -CAcreateserial -out "$LOCAL_TMP/$node.crt" \
    -days 2 -sha256 -extfile "$LOCAL_TMP/$node.ext" >/dev/null 2>&1
done
IRAN_PIN="$(openssl x509 -in "$LOCAL_TMP/iran.crt" -outform DER | sha256sum | awk '{print $1}')"
ABROAD_PIN="$(openssl x509 -in "$LOCAL_TMP/abroad.crt" -outform DER | sha256sum | awk '{print $1}')"

cat > "$LOCAL_TMP/echo.py" <<'PY'
import socket, sys, threading
port = int(sys.argv[1])
listener = socket.socket()
listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
listener.bind(("127.0.0.1", port))
listener.listen(32)
def handle(conn):
    with conn:
        while True:
            data = conn.recv(65536)
            if not data:
                return
            conn.sendall(data)
while True:
    connection, _ = listener.accept()
    threading.Thread(target=handle, args=(connection,), daemon=True).start()
PY
cat > "$LOCAL_TMP/check-ports.py" <<'PORTS_CHECK'
import socket, sys
for host, raw_port in [("0.0.0.0", sys.argv[1]), ("127.0.0.1", sys.argv[2]), ("127.0.0.1", sys.argv[3])]:
    port = int(raw_port)
    probe = socket.socket()
    try:
        probe.bind((host, port))
    except OSError as exc:
        raise SystemExit(f"LAB port already in use: {host}:{port}: {exc}")
    finally:
        probe.close()
print("LAB test ports are free")
PORTS_CHECK
cat > "$LOCAL_TMP/probe.py" <<'PY'
import hashlib, socket, sys, time
host, port = sys.argv[1], int(sys.argv[2])
payload_size = int(sys.argv[3]) if len(sys.argv) > 3 else (2 * 1024 * 1024 + 137)
payload = bytes((i % 251 for i in range(payload_size)))
started = time.monotonic()
deadline = started + 45
last_error = None
while True:
    try:
        sock = socket.create_connection((host, port), timeout=2)
        break
    except OSError as exc:
        last_error = exc
        if time.monotonic() >= deadline:
            raise SystemExit(f"local forward listener/session did not become ready: {last_error}")
        time.sleep(0.5)
with sock:
    sock.settimeout(30)
    sock.sendall(payload)
    sock.shutdown(socket.SHUT_WR)
    received = bytearray()
    while len(received) < len(payload):
        try:
            block = sock.recv(min(65536, len(payload) - len(received)))
        except TimeoutError as exc:
            raise SystemExit(f"payload receive timed out: received={len(received)} expected={len(payload)}") from exc
        if not block:
            break
        received.extend(block)
if bytes(received) != payload:
    raise SystemExit(f"payload mismatch: expected={len(payload)} actual={len(received)}")
elapsed = time.monotonic() - started
print(f"E2E_PAYLOAD_OK bytes={len(payload)} sha256={hashlib.sha256(received).hexdigest()} elapsed_s={elapsed:.3f}")
PY

for side in iran abroad; do
  ssh_host "$side" "mkdir -p '$REMOTE_DIR' && chmod 700 '$REMOTE_DIR'"
  copy_to_host "$side" "$BINARY" "$LOCAL_TMP/ca.crt" "$LOCAL_TMP/iran.crt" \
    "$LOCAL_TMP/iran.key" "$LOCAL_TMP/abroad.crt" "$LOCAL_TMP/abroad.key" \
    "$LOCAL_TMP/echo.py" "$LOCAL_TMP/probe.py" "$LOCAL_TMP/check-ports.py"
  ssh_host "$side" "chmod 700 '$REMOTE_DIR/baft-tunnel' && chmod 600 '$REMOTE_DIR/'*.key"
  ssh_host "$side" "python3 '$REMOTE_DIR/check-ports.py' '$E2E_PORT' '$LOCAL_PORT' '$ECHO_PORT'"
done

start_echo iran
start_echo abroad
sleep 1

echo "=== DIRECT: Iran dials abroad ==="
start_serve abroad abroad iran direct "$IRAN_PIN" abroad.crt abroad.key
start_forward iran iran abroad "$LAB_ABROAD_HOST" abroad.lab direct "$ABROAD_PIN" iran.crt iran.key
probe_host iran 65536
probe_host iran 2097289

echo "=== INTERRUPTION / RECOVERY: restart abroad listener ==="
stop_role abroad serve
sleep 2
start_serve abroad abroad iran direct "$IRAN_PIN" abroad.crt abroad.key
probe_host iran 65536

stop_role iran forward
stop_role abroad serve
sleep 1

echo "=== REVERSE: Abroad dials Iran ==="
start_serve iran iran abroad reverse "$ABROAD_PIN" iran.crt iran.key
start_forward abroad abroad iran "$LAB_IRAN_HOST" iran.lab reverse "$IRAN_PIN" abroad.crt abroad.key
probe_host abroad 65536
probe_host abroad 2097289

echo "PHASE1_TWO_HOST_E2E=PASS (Direct, Reverse, 2 MiB integrity, listener interruption/recovery; temporary /tmp processes only)"
