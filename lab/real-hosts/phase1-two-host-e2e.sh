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
IMPAIR_PORT=19100
SOAK_SECONDS="${LAB_SOAK_SECONDS:-0}"
IMPAIR="${LAB_IMPAIR:-0}"
LOSS="${LAB_LOSS_PERCENT:-0}"
[[ "$LOSS" =~ ^[0-9]+$ ]] && (( LOSS <= 20 )) || { echo "LAB_LOSS_PERCENT must be 0-20" >&2; exit 2; }
LOSS_TABLE=baft_lab_loss
NOISE="${LAB_NOISE:-0}"
[[ "$NOISE" == 0 || "$NOISE" == 1 ]] || { echo "LAB_NOISE must be 0 or 1" >&2; exit 2; }
[[ "$SOAK_SECONDS" =~ ^[0-9]+$ ]] && (( SOAK_SECONDS <= 3600 )) || { echo "LAB_SOAK_SECONDS must be 0-3600" >&2; exit 2; }
[[ "$IMPAIR" == 0 || "$IMPAIR" == 1 ]] || { echo "LAB_IMPAIR must be 0 or 1" >&2; exit 2; }
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
  ssh_host "$side" "nohup '$REMOTE_DIR/baft-tunnel' serve --listen '0.0.0.0:$E2E_PORT' --target '127.0.0.1:$ECHO_PORT' --local-id '$local_id' --peer-id '$peer_id' --tunnel-id 'lab-e2e' --direction '$direction' --peer-fingerprint '$peer_pin' --ca '$REMOTE_DIR/ca.crt' --crl '$REMOTE_DIR/ca.crl' --cert '$REMOTE_DIR/$cert' --key '$REMOTE_DIR/$key' --max-streams 8 > '$REMOTE_DIR/serve.log' 2>&1 < /dev/null & echo \$! > '$REMOTE_DIR/serve.pid'"
}
start_forward() {
  local side="$1" local_id="$2" peer_id="$3" peer_host="$4" peer_dns="$5" direction="$6" peer_pin="$7" cert="$8" key="$9" remote_port="${10:-$E2E_PORT}"
  ssh_host "$side" "nohup '$REMOTE_DIR/baft-tunnel' forward --listen '127.0.0.1:$LOCAL_PORT' --remote '$peer_host:$remote_port' --server-name '$peer_dns' --local-id '$local_id' --peer-id '$peer_id' --tunnel-id 'lab-e2e' --direction '$direction' --peer-fingerprint '$peer_pin' --ca '$REMOTE_DIR/ca.crt' --crl '$REMOTE_DIR/ca.crl' --cert '$REMOTE_DIR/$cert' --key '$REMOTE_DIR/$key' --max-streams 8 > '$REMOTE_DIR/forward.log' 2>&1 < /dev/null & echo \$! > '$REMOTE_DIR/forward.pid'"
}
probe_host() {
  local side="$1" bytes="${2:-2097289}"
  ssh_host "$side" "python3 '$REMOTE_DIR/probe.py' 127.0.0.1 '$LOCAL_PORT' '$bytes'"
}
proc_stats() {
  local side="$1" role="$2"
  ssh_host "$side" "p=\$(cat '$REMOTE_DIR/$role.pid'); echo \$(ls /proc/\$p/fd | wc -l) \$(awk '/VmRSS/{print \$2}' /proc/\$p/status)"
}
start_impair() {
  ssh_host iran "nohup python3 '$REMOTE_DIR/impair.py' '$IMPAIR_PORT' '$LAB_ABROAD_HOST' '$E2E_PORT' > '$REMOTE_DIR/impair.log' 2>&1 < /dev/null & echo \$! > '$REMOTE_DIR/impair.pid'"
}
cleanup() {
  local status=$?
  trap - EXIT
  if (( status != 0 )); then
    for side in iran abroad; do
      ssh_host "$side" "for f in serve.log forward.log echo.log impair.log; do if [ -f '$REMOTE_DIR/'\$f ]; then echo === $side:\$f ===; tail -n 80 '$REMOTE_DIR/'\$f; fi; done" || true
    done
  fi
  for side in iran abroad; do
    if (( LOSS > 0 )); then ssh_host "$side" "nft delete table inet $LOSS_TABLE 2>/dev/null || true" || true; fi
    ssh_host "$side" "for role in serve forward echo impair; do f='$REMOTE_DIR/'\$role'.pid'; if [ -s \"\$f\" ]; then kill \$(cat \"\$f\") 2>/dev/null || true; fi; done; sleep 0.2; rm -rf '$REMOTE_DIR'" || status=1
  done
  rm -rf "$LOCAL_TMP"
  if (( status != 0 )); then
    echo "LAB cleanup or E2E failed; inspect run logs and host process state." >&2
  fi
  exit "$status"
}
trap cleanup EXIT

# Generate a short-lived private LAB CA, two node certificates, and an authenticated CRL.
# The CA key stays on the runner; the CRL is mandatory on both TLS peers.
mkdir -p "$LOCAL_TMP/newcerts"
: > "$LOCAL_TMP/index.txt"
printf '1000\n' > "$LOCAL_TMP/serial"
printf '1000\n' > "$LOCAL_TMP/crlnumber"
openssl req -x509 -newkey rsa:3072 -nodes -keyout "$LOCAL_TMP/ca.key" -out "$LOCAL_TMP/ca.crt" \
  -days 2 -sha256 -subj "/CN=BAFT-TUNNEL ephemeral LAB CA" \
  -addext "basicConstraints=critical,CA:TRUE" \
  -addext "keyUsage=critical,keyCertSign,cRLSign" >/dev/null 2>&1
cat > "$LOCAL_TMP/openssl.cnf" <<EOF
[ ca ]
default_ca = CA_default
[ CA_default ]
database = $LOCAL_TMP/index.txt
new_certs_dir = $LOCAL_TMP/newcerts
certificate = $LOCAL_TMP/ca.crt
private_key = $LOCAL_TMP/ca.key
serial = $LOCAL_TMP/serial
crlnumber = $LOCAL_TMP/crlnumber
default_md = sha256
default_days = 2
default_crl_days = 2
policy = policy_any
copy_extensions = copy
[ policy_any ]
commonName = supplied
EOF
for node in iran abroad control; do
  openssl req -new -newkey rsa:2048 -nodes -keyout "$LOCAL_TMP/$node.key" \
    -out "$LOCAL_TMP/$node.csr" -subj "/CN=$node.lab" >/dev/null 2>&1
  cat > "$LOCAL_TMP/$node.ext" <<EOF
basicConstraints=critical,CA:FALSE
keyUsage=critical,digitalSignature
extendedKeyUsage=serverAuth,clientAuth
subjectAltName=DNS:$node.lab,URI:urn:baft-tunnel:node:$node
EOF
  printf '[ %s_ext ]\n' "$node" >> "$LOCAL_TMP/openssl.cnf"
  cat "$LOCAL_TMP/$node.ext" >> "$LOCAL_TMP/openssl.cnf"
  openssl ca -batch -notext -config "$LOCAL_TMP/openssl.cnf" -extensions "${node}_ext" \
    -in "$LOCAL_TMP/$node.csr" -out "$LOCAL_TMP/$node.crt" -days 2 >/dev/null 2>&1
done
openssl ca -gencrl -batch -config "$LOCAL_TMP/openssl.cnf" -out "$LOCAL_TMP/ca.crl" >/dev/null 2>&1
IRAN_PIN="$(openssl x509 -in "$LOCAL_TMP/iran.crt" -outform DER | sha256sum | awk '{print $1}')"
ABROAD_PIN="$(openssl x509 -in "$LOCAL_TMP/abroad.crt" -outform DER | sha256sum | awk '{print $1}')"
CONTROL_PIN="$(openssl x509 -in "$LOCAL_TMP/control.crt" -outform DER | sha256sum | awk '{print $1}')"

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
cat > "$LOCAL_TMP/impair.py" <<'PY'
# Application-level impairment proxy (latency, jitter, small chunks). It does not touch
# interfaces, qdiscs, routes or firewall rules.
import random, socket, sys, threading, time
lport, rhost, rport = int(sys.argv[1]), sys.argv[2], int(sys.argv[3])
listener = socket.socket()
listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
listener.bind(("127.0.0.1", lport))
listener.listen(16)
def pipe(a, b):
    try:
        while True:
            data = a.recv(16384)
            if not data:
                break
            time.sleep(random.uniform(0.04, 0.12))
            b.sendall(data)
    except OSError:
        pass
    finally:
        for s in (a, b):
            try: s.shutdown(socket.SHUT_RDWR)
            except OSError: pass
while True:
    c, _ = listener.accept()
    try:
        u = socket.create_connection((rhost, rport), timeout=5)
    except OSError:
        c.close(); continue
    threading.Thread(target=pipe, args=(c, u), daemon=True).start()
    threading.Thread(target=pipe, args=(u, c), daemon=True).start()
PY
cat > "$LOCAL_TMP/check-ports.py" <<'PORTS_CHECK'
import socket, sys
for host, raw_port in [("0.0.0.0", sys.argv[1]), ("127.0.0.1", sys.argv[2]), ("127.0.0.1", sys.argv[3]), ("127.0.0.1", sys.argv[4])]:
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
  copy_to_host "$side" "$BINARY" "$LOCAL_TMP/ca.crt" "$LOCAL_TMP/ca.crl" \
    "$LOCAL_TMP/iran.crt" "$LOCAL_TMP/iran.key" "$LOCAL_TMP/abroad.crt" "$LOCAL_TMP/abroad.key" \
    "$LOCAL_TMP/control.crt" "$LOCAL_TMP/control.key" \
    "$LOCAL_TMP/echo.py" "$LOCAL_TMP/impair.py" "$LOCAL_TMP/probe.py" "$LOCAL_TMP/check-ports.py"
  ssh_host "$side" "chmod 700 '$REMOTE_DIR/baft-tunnel' && chmod 600 '$REMOTE_DIR/'*.key"
  ssh_host "$side" "python3 '$REMOTE_DIR/check-ports.py' '$E2E_PORT' '$LOCAL_PORT' '$ECHO_PORT' '$IMPAIR_PORT'"
done

start_echo iran
start_echo abroad
sleep 1

echo "=== DIRECT: Iran dials abroad ==="
start_serve abroad abroad iran direct "$IRAN_PIN" abroad.crt abroad.key
start_forward iran iran abroad "$LAB_ABROAD_HOST" abroad.lab direct "$ABROAD_PIN" iran.crt iran.key
probe_host iran 65536
probe_host iran 2097289

if (( SOAK_SECONDS > 0 )); then
  echo "=== SOAK: ${SOAK_SECONDS}s of repeated probes over Direct ==="
  read -r fd0 rss0 < <(proc_stats iran forward)
  read -r sfd0 srss0 < <(proc_stats abroad serve)
  end=$((SECONDS + SOAK_SECONDS)); n=0
  while (( SECONDS < end )); do
    probe_host iran 65536 >/dev/null
    n=$((n + 1))
    if (( n % 6 == 0 )); then probe_host iran 2097289 >/dev/null; fi
    sleep 10
  done
  read -r fd1 rss1 < <(proc_stats iran forward)
  read -r sfd1 srss1 < <(proc_stats abroad serve)
  echo "SOAK_RESULT rounds=$n forward_fd=$fd0->$fd1 forward_rss_kb=$rss0->$rss1 serve_fd=$sfd0->$sfd1 serve_rss_kb=$srss0->$srss1"
  (( fd1 <= fd0 + 8 && sfd1 <= sfd0 + 8 )) || { echo "fd growth during soak" >&2; exit 1; }
  (( rss1 <= rss0 * 2 + 20480 && srss1 <= srss0 * 2 + 20480 )) || { echo "memory growth during soak" >&2; exit 1; }
fi

if [[ "$IMPAIR" == 1 ]]; then
  echo "=== IMPAIRMENT: application-level latency/jitter proxy, then proxy restart ==="
  stop_role iran forward
  start_impair
  sleep 1
  start_forward iran iran abroad 127.0.0.1 abroad.lab direct "$ABROAD_PIN" iran.crt iran.key "$IMPAIR_PORT"
  probe_host iran 65536
  probe_host iran 1048576
  stop_role iran impair
  sleep 2
  start_impair
  probe_host iran 65536
  stop_role iran impair
  stop_role iran forward
  start_forward iran iran abroad "$LAB_ABROAD_HOST" abroad.lab direct "$ABROAD_PIN" iran.crt iran.key
  probe_host iran 65536
fi

if (( LOSS > 0 )); then
  echo "=== REAL TCP PACKET LOSS: ${LOSS}% via dedicated nftables table (test flow only) ==="
  ssh_host iran "command -v nft >/dev/null" || { echo "nft is not available on the Iran host" >&2; exit 1; }
  ssh_host iran "! nft list table inet $LOSS_TABLE >/dev/null 2>&1" || { echo "stale $LOSS_TABLE table exists" >&2; exit 1; }
  if [[ "$LAB_ABROAD_HOST" == *:* ]]; then fam=ip6; else fam=ip; fi
  ssh_host iran "nft add table inet $LOSS_TABLE && nft add chain inet $LOSS_TABLE out '{ type filter hook output priority 0; policy accept; }' && nft add chain inet $LOSS_TABLE in '{ type filter hook input priority 0; policy accept; }' && nft add rule inet $LOSS_TABLE out $fam daddr $LAB_ABROAD_HOST tcp dport $E2E_PORT numgen random mod 100 '<' $LOSS counter drop && nft add rule inet $LOSS_TABLE in $fam saddr $LAB_ABROAD_HOST tcp sport $E2E_PORT numgen random mod 100 '<' $LOSS counter drop"
  probe_host iran 65536
  probe_host iran 524288
  probe_host iran 65536
  dropped="$(ssh_host iran "nft list table inet $LOSS_TABLE" | awk '/packets/ {for(i=1;i<NF;i++) if($i=="packets") sum+=$(i+1)} END{print sum+0}')"
  echo "LOSS_RESULT percent=$LOSS dropped_packets=$dropped"
  ssh_host iran "nft delete table inet $LOSS_TABLE"
  ssh_host iran "! nft list table inet $LOSS_TABLE >/dev/null 2>&1" || { echo "loss table was not removed" >&2; exit 1; }
  (( dropped > 0 )) || { echo "no packets were dropped; loss was not exercised" >&2; exit 1; }
  probe_host iran 65536
fi

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

echo "=== CERTIFICATE REVOCATION: server rejects revoked client certificate ==="
openssl ca -batch -config "$LOCAL_TMP/openssl.cnf" -revoke "$LOCAL_TMP/iran.crt" >/dev/null 2>&1
openssl ca -gencrl -batch -config "$LOCAL_TMP/openssl.cnf" -out "$LOCAL_TMP/ca.crl" >/dev/null 2>&1
for side in iran abroad; do copy_to_host "$side" "$LOCAL_TMP/ca.crl"; done
stop_role abroad serve
stop_role abroad forward
stop_role iran forward
sleep 1
start_serve abroad abroad iran direct "$IRAN_PIN" abroad.crt abroad.key
start_forward iran iran abroad "$LAB_ABROAD_HOST" abroad.lab direct "$ABROAD_PIN" iran.crt iran.key
sleep 4
if ! ssh_host abroad "grep -qi 'revok' '$REMOTE_DIR/serve.log'"; then
  echo "Revoked client certificate was not rejected by the TLS server" >&2
  exit 1
fi
stop_role iran forward
stop_role abroad serve

echo "=== CERTIFICATE REVOCATION: client rejects revoked server certificate ==="
openssl ca -batch -config "$LOCAL_TMP/openssl.cnf" -revoke "$LOCAL_TMP/abroad.crt" >/dev/null 2>&1
openssl ca -gencrl -batch -config "$LOCAL_TMP/openssl.cnf" -out "$LOCAL_TMP/ca.crl" >/dev/null 2>&1
for side in iran abroad; do copy_to_host "$side" "$LOCAL_TMP/ca.crl"; done
start_serve abroad abroad control direct "$CONTROL_PIN" abroad.crt abroad.key
start_forward iran control abroad "$LAB_ABROAD_HOST" abroad.lab direct "$ABROAD_PIN" control.crt control.key
sleep 4
if ! ssh_host iran "grep -qi 'revok' '$REMOTE_DIR/forward.log'"; then
  echo "Revoked server certificate was not rejected by the TLS client" >&2
  exit 1
fi
stop_role iran forward
stop_role abroad serve

if (( NOISE == 1 )); then
  echo "=== NOISE-TCP: pinned static keys, no CA (Direct, then rogue key must be refused) ==="
  noise_pub() { ssh_host "$1" "'$REMOTE_DIR/baft-tunnel' noise-keygen --out '$REMOTE_DIR/$2.noise'" | tr -d '[:space:]'; }
  IRAN_NPUB="$(noise_pub iran iran)"; ABROAD_NPUB="$(noise_pub abroad abroad)"; ROGUE_NPUB="$(noise_pub iran rogue)"
  for v in "$IRAN_NPUB" "$ABROAD_NPUB" "$ROGUE_NPUB"; do [[ "$v" =~ ^[0-9a-f]{64}$ ]] || { echo "noise-keygen output invalid" >&2; exit 1; }; done
  noise_serve() { # side local_id peer_id peer_pub keyfile
    ssh_host "$1" "printf '{\"mode\":\"serve\",\"engine\":\"noise-tcp\",\"direction\":\"direct\",\"listen\":\"0.0.0.0:$E2E_PORT\",\"target\":\"127.0.0.1:$ECHO_PORT\",\"local_id\":\"$2\",\"peer_id\":\"$3\",\"tunnel_id\":\"lab-e2e\",\"key\":\"$REMOTE_DIR/$5.noise\",\"noise_peer_public\":\"$4\",\"max_streams\":8}' > '$REMOTE_DIR/noise-serve.json'; nohup '$REMOTE_DIR/baft-tunnel' apply --config '$REMOTE_DIR/noise-serve.json' --confirm > '$REMOTE_DIR/serve.log' 2>&1 < /dev/null & echo \$! > '$REMOTE_DIR/serve.pid'"
  }
  noise_forward() { # side local_id peer_id peer_host peer_pub keyfile
    ssh_host "$1" "printf '{\"mode\":\"forward\",\"engine\":\"noise-tcp\",\"direction\":\"direct\",\"listen\":\"127.0.0.1:$LOCAL_PORT\",\"remote\":\"$4:$E2E_PORT\",\"local_id\":\"$2\",\"peer_id\":\"$3\",\"tunnel_id\":\"lab-e2e\",\"key\":\"$REMOTE_DIR/$6.noise\",\"noise_peer_public\":\"$5\",\"max_streams\":8}' > '$REMOTE_DIR/noise-forward.json'; nohup '$REMOTE_DIR/baft-tunnel' apply --config '$REMOTE_DIR/noise-forward.json' --confirm > '$REMOTE_DIR/forward.log' 2>&1 < /dev/null & echo \$! > '$REMOTE_DIR/forward.pid'"
  }
  noise_serve abroad abroad iran "$IRAN_NPUB" abroad
  noise_forward iran iran abroad "$LAB_ABROAD_HOST" "$ABROAD_NPUB" iran
  sleep 2
  probe_host iran 65536
  probe_host iran 2097289
  stop_role iran forward
  echo "--- NOISE rogue: unenrolled client key must not relay ---"
  noise_forward iran iran abroad "$LAB_ABROAD_HOST" "$ABROAD_NPUB" rogue
  sleep 2
  if probe_host iran 65536 >/dev/null 2>&1; then
    echo "Noise server accepted an unenrolled client key" >&2
    exit 1
  fi
  echo "NOISE_RESULT enrolled_ok=1 rogue_refused=1"
  stop_role iran forward
  stop_role abroad serve
fi

echo "PHASE1_TWO_HOST_E2E=PASS soak_s=$SOAK_SECONDS impair=$IMPAIR loss=$LOSS noise=$NOISE (Direct, Reverse, 2 MiB integrity, recovery, server/client certificate revocation; temporary /tmp processes only)"
