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
SETUP="${LAB_SETUP:-0}"
[[ "$SETUP" == 0 || "$SETUP" == 1 ]] || { echo "LAB_SETUP must be 0 or 1" >&2; exit 2; }
ROLLOVER="${LAB_ROLLOVER:-0}"
[[ "$ROLLOVER" == 0 || "$ROLLOVER" == 1 ]] || { echo "LAB_ROLLOVER must be 0 or 1" >&2; exit 2; }
REVERSE="${LAB_REVERSE:-0}"
REVERSE_R3="${LAB_REVERSE_R3:-0}"
MVP="${LAB_MVP:-0}"
REVERSE_ACCEPT="${LAB_REVERSE_ACCEPT:-0}"
[[ "$REVERSE_ACCEPT" =~ ^[0-9]+$ ]] && (( REVERSE_ACCEPT <= 3600 )) || { echo "LAB_REVERSE_ACCEPT must be 0-3600 (acceptance soak seconds; 0 = off)" >&2; exit 2; }
[[ "$MVP" == 0 || "$MVP" == 1 ]] || { echo "LAB_MVP must be 0 or 1" >&2; exit 2; }
MVP_BINARY_PATH="${MVP_BINARY:-}"; MVP_SOURCE="${MVP_SOURCE:-unknown}"
if [[ "$MVP" == 1 || "$REVERSE_ACCEPT" != 0 ]]; then [[ -x "$MVP_BINARY_PATH" ]] || { echo "LAB_MVP=1 or LAB_REVERSE_ACCEPT needs MVP_BINARY" >&2; exit 2; }; fi
[[ "$MVP" == 0 || "$REVERSE_ACCEPT" == 0 ]] || { echo "LAB_MVP and LAB_REVERSE_ACCEPT are separate runs" >&2; exit 2; }
OPS_SOAK="${LAB_OPS:-0}"
[[ "$OPS_SOAK" =~ ^[0-9]+$ ]] && (( OPS_SOAK <= 3600 )) || { echo "LAB_OPS must be 0-3600 (Direct operations stage soak seconds; 0 = off)" >&2; exit 2; }
if (( OPS_SOAK > 0 )); then
  [[ -x "$MVP_BINARY_PATH" ]] || { echo "LAB_OPS needs MVP_BINARY" >&2; exit 2; }
  [[ "$MVP" == 0 && "$REVERSE_ACCEPT" == 0 ]] || { echo "LAB_OPS is a separate run from LAB_MVP and LAB_REVERSE_ACCEPT" >&2; exit 2; }
fi
CRL_PUSH="${LAB_CRL_PUSH:-0}"
[[ "$CRL_PUSH" == 0 || "$CRL_PUSH" == 1 ]] || { echo "LAB_CRL_PUSH must be 0 or 1" >&2; exit 2; }
(( CRL_PUSH == 0 || OPS_SOAK > 0 )) || { echo "LAB_CRL_PUSH runs inside the Direct operations stage (LAB_OPS>0)" >&2; exit 2; }
AGENT_SVC="${LAB_AGENT_SVC:-0}"; AGENT_REBOOT="${LAB_REBOOT:-0}"
[[ "$AGENT_SVC" == 0 || "$AGENT_SVC" == 1 ]] && [[ "$AGENT_REBOOT" == 0 || "$AGENT_REBOOT" == 1 ]] || { echo "LAB_AGENT_SVC and LAB_REBOOT must be 0 or 1" >&2; exit 2; }
(( AGENT_REBOOT == 0 || AGENT_SVC == 1 )) || { echo "LAB_REBOOT runs inside the agent service stage (LAB_AGENT_SVC=1)" >&2; exit 2; }
if (( AGENT_SVC == 1 )); then
  [[ -x "$MVP_BINARY_PATH" ]] || { echo "LAB_AGENT_SVC needs MVP_BINARY" >&2; exit 2; }
  [[ "$MVP" == 0 && "$REVERSE_ACCEPT" == 0 && "$OPS_SOAK" == 0 ]] || { echo "LAB_AGENT_SVC is a separate run from LAB_MVP, LAB_REVERSE_ACCEPT and LAB_OPS" >&2; exit 2; }
fi
PANEL_PAIRING="${LAB_PANEL_PAIRING:-0}"
[[ "$PANEL_PAIRING" == 0 || "$PANEL_PAIRING" == 1 ]] || { echo "LAB_PANEL_PAIRING must be 0 or 1" >&2; exit 2; }
if (( PANEL_PAIRING == 1 )); then
  [[ -x "$MVP_BINARY_PATH" ]] || { echo "LAB_PANEL_PAIRING needs MVP_BINARY" >&2; exit 2; }
  [[ "$MVP" == 0 && "$REVERSE_ACCEPT" == 0 && "$OPS_SOAK" == 0 && "$AGENT_SVC" == 0 ]] || { echo "LAB_PANEL_PAIRING is a separate run from LAB_MVP, LAB_REVERSE_ACCEPT, LAB_OPS and LAB_AGENT_SVC" >&2; exit 2; }
fi
[[ "$REVERSE_R3" =~ ^[0-9]+$ ]] && (( REVERSE_R3 <= 3600 )) || { echo "LAB_REVERSE_R3 must be 0-3600 (Reverse R3 soak seconds; 0 = off)" >&2; exit 2; }
[[ "$REVERSE" == 0 || "$REVERSE" == 1 ]] || { echo "LAB_REVERSE must be 0 or 1" >&2; exit 2; }
BAFT="${LAB_BAFT:-0}"
BAFT_SYSTEMD="${LAB_BAFT_SYSTEMD:-0}"
[[ "$BAFT_SYSTEMD" == 0 || "$BAFT_SYSTEMD" == 1 ]] || { echo "LAB_BAFT_SYSTEMD must be 0 or 1" >&2; exit 2; }
[[ "$BAFT" == 0 || "$BAFT" == 1 ]] || { echo "LAB_BAFT must be 0 or 1" >&2; exit 2; }
BAFT_BINARY_PATH="${BAFT_ENGINE_BINARY:-}"
if [[ "$BAFT" == 1 ]]; then [[ -x "$BAFT_BINARY_PATH" ]] || { echo "LAB_BAFT=1 needs BAFT_ENGINE_BINARY (pinned build)" >&2; exit 2; }; fi
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
  # ssh exits 255 only when the connection itself failed (timeout, banner exchange, reset): the
  # command did not run, so a bounded retry is safe. Any other status is the command's own.
  local attempt rc=0
  for attempt in 1 2 3; do
    ssh -i "$SSH_KEY_FILE" -p "$port" -o BatchMode=yes -o IdentitiesOnly=yes \
      -o StrictHostKeyChecking=yes -o UserKnownHostsFile="$SSH_KNOWN_HOSTS_FILE" \
      -o ConnectTimeout=12 -o ServerAliveInterval=10 -o ServerAliveCountMax=2 \
      -o LogLevel=ERROR "$user@$host" "$command" && return 0 || rc=$?
    (( rc == 255 )) || return "$rc"
    sleep $((attempt * 5))
  done
  return "$rc"
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
    if (( LOSS > 0 || REVERSE_R3 > 0 )); then ssh_host "$side" "nft delete table inet $LOSS_TABLE 2>/dev/null || true" || true; fi
    ssh_host "$side" "for role in serve forward echo echo2 impair agent; do f='$REMOTE_DIR/'\$role'.pid'; if [ -s \"\$f\" ]; then kill \$(cat \"\$f\") 2>/dev/null || true; fi; done; sleep 0.2; rm -rf '$REMOTE_DIR'" || status=1
  done
  rm -rf "$LOCAL_TMP"
  if (( status != 0 )); then
    echo "LAB cleanup or E2E failed; inspect run logs and host process state." >&2
  fi
  exit "$status"
}
trap cleanup EXIT

# Operator-requested recovery: remove ONLY leftovers of earlier runs of this script
# (/tmp/baft-tunnel-e2e-* directories, the processes recorded in their pid files, and orphans
# whose command line is exactly this script's temp binary/helper paths).
if [[ "${LAB_CLEANUP_STALE:-0}" == 1 || "${LAB_CLEANUP_STALE:-0}" == 2 ]]; then
  if [[ "${LAB_CLEANUP_STALE}" == 2 ]]; then
    # Mode 2 additionally removes an installed BAFT-TUNNEL left by an interrupted Reverse
    # acceptance run (instances revpub/revsvc, /etc/baft-tunnel, state, backups), using this
    # checkout's own scripts. The baft-tunnel service user is kept.
    tar -czf "$LOCAL_TMP/stale-pkg.tgz" scripts/uninstall.sh scripts/install-agent-control.sh systemd
    for side in iran abroad; do
      ssh_host "$side" "mkdir -p '$REMOTE_DIR/pkg'" || true
      copy_to_host "$side" "$LOCAL_TMP/stale-pkg.tgz" || true
      ssh_host "$side" "tar -xzf '$REMOTE_DIR/stale-pkg.tgz' -C '$REMOTE_DIR/pkg';
        for i in revpub revsvc; do touch /run/baft-tunnel/control/\$i.stop 2>/dev/null; systemctl stop baft-tunnel@\$i.service 2>/dev/null; bash '$REMOTE_DIR/pkg/scripts/install-agent-control.sh' --remove --instance \$i >/dev/null 2>&1; done;
        for f in /var/lib/baft-tunnel/agent/*.pid /tmp/baft-tunnel-e2e-*/agent.pid; do [ -s \"\$f\" ] && kill \$(cat \"\$f\") 2>/dev/null; done;
        bash '$REMOTE_DIR/pkg/scripts/uninstall.sh' --stop --purge-config --yes >/dev/null 2>&1;
        rm -rf /var/backups/baft-tunnel-backup.* 2>/dev/null; systemctl reset-failed 'baft-tunnel*' 2>/dev/null; systemctl daemon-reload;
        echo STALE_INSTALL_REMOVED" || true
    done
  fi
  for side in iran abroad; do
    ssh_host "$side" "for f in /tmp/baft-tunnel-e2e-*/*.pid; do [ -s \"\$f\" ] && kill \$(cat \"\$f\") 2>/dev/null; done; pkill -f '^/tmp/baft-tunnel-e2e-[0-9]+/baft-tunnel ' 2>/dev/null; pkill -f '^/tmp/baft-tunnel-e2e-[0-9]+/baft-engine ' 2>/dev/null; pkill -f 'python3 /tmp/baft-tunnel-e2e-[0-9]+/(echo|impair)\.py' 2>/dev/null; sleep 1; rm -rf /tmp/baft-tunnel-e2e-*; echo CLEANUP_STALE_DONE" || true
  done
  exit 0
fi

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
# A second certificate carrying the abroad identity; it is revoked only by the Reverse stage.
openssl req -new -newkey rsa:2048 -nodes -keyout "$LOCAL_TMP/revabroad.key" \
  -out "$LOCAL_TMP/revabroad.csr" -subj "/CN=revabroad.lab" >/dev/null 2>&1
cat > "$LOCAL_TMP/revabroad.ext" <<EOF
basicConstraints=critical,CA:FALSE
keyUsage=critical,digitalSignature
extendedKeyUsage=serverAuth,clientAuth
subjectAltName=DNS:abroad.lab,URI:urn:baft-tunnel:node:abroad
EOF
printf '[ revabroad_ext ]\n' >> "$LOCAL_TMP/openssl.cnf"
cat "$LOCAL_TMP/revabroad.ext" >> "$LOCAL_TMP/openssl.cnf"
openssl ca -batch -notext -config "$LOCAL_TMP/openssl.cnf" -extensions revabroad_ext \
  -in "$LOCAL_TMP/revabroad.csr" -out "$LOCAL_TMP/revabroad.crt" -days 2 >/dev/null 2>&1
openssl ca -gencrl -batch -config "$LOCAL_TMP/openssl.cnf" -out "$LOCAL_TMP/ca.crl" >/dev/null 2>&1
IRAN_PIN="$(openssl x509 -in "$LOCAL_TMP/iran.crt" -outform DER | sha256sum | awk '{print $1}')"
ABROAD_PIN="$(openssl x509 -in "$LOCAL_TMP/abroad.crt" -outform DER | sha256sum | awk '{print $1}')"
CONTROL_PIN="$(openssl x509 -in "$LOCAL_TMP/control.crt" -outform DER | sha256sum | awk '{print $1}')"
REVABROAD_PIN="$(openssl x509 -in "$LOCAL_TMP/revabroad.crt" -outform DER | sha256sum | awk '{print $1}')"

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
    "$LOCAL_TMP/control.crt" "$LOCAL_TMP/control.key" "$LOCAL_TMP/revabroad.crt" "$LOCAL_TMP/revabroad.key" \
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

if [[ "$REVERSE" == 1 ]]; then
  echo "=== REVERSE (ADR-0021, tcp-tls): abroad (service node) dials iran (public node) ==="
  BIN="$REMOTE_DIR/baft-tunnel"
  # rev_public SIDE LOCAL PEER PEER_PIN CERT KEY: tunnel listener + user ingress, no target.
  rev_public() {
    local json
    json="{\"mode\":\"serve\",\"engine\":\"tcp-tls\",\"direction\":\"reverse\",\"listen\":\"0.0.0.0:$E2E_PORT\",\"ingress\":\"127.0.0.1:$LOCAL_PORT\",\"local_id\":\"$2\",\"peer_id\":\"$3\",\"tunnel_id\":\"lab-e2e\",\"peer_fingerprint\":\"$4\",\"ca\":\"$REMOTE_DIR/ca.crt\",\"crl\":\"$REMOTE_DIR/ca.crl\",\"cert\":\"$REMOTE_DIR/$5\",\"key\":\"$REMOTE_DIR/$6\",\"max_streams\":8}"
    ssh_host "$1" "printf '%s' '$json' > '$REMOTE_DIR/rev-public.json'; nohup '$BIN' apply --config '$REMOTE_DIR/rev-public.json' --confirm > '$REMOTE_DIR/serve.log' 2>&1 < /dev/null & echo \$! > '$REMOTE_DIR/serve.pid'"
  }
  # rev_service SIDE LOCAL PEER PEER_HOST PEER_DNS PEER_PIN CERT KEY: dials out, relays to the local echo.
  rev_service() {
    local json
    json="{\"mode\":\"forward\",\"engine\":\"tcp-tls\",\"direction\":\"reverse\",\"target\":\"127.0.0.1:$ECHO_PORT\",\"remote\":\"$4:$E2E_PORT\",\"server_name\":\"$5\",\"local_id\":\"$2\",\"peer_id\":\"$3\",\"tunnel_id\":\"lab-e2e\",\"peer_fingerprint\":\"$6\",\"ca\":\"$REMOTE_DIR/ca.crt\",\"crl\":\"$REMOTE_DIR/ca.crl\",\"cert\":\"$REMOTE_DIR/$7\",\"key\":\"$REMOTE_DIR/$8\",\"max_streams\":8}"
    ssh_host "$1" "printf '%s' '$json' > '$REMOTE_DIR/rev-service.json'; nohup '$BIN' apply --config '$REMOTE_DIR/rev-service.json' --confirm > '$REMOTE_DIR/forward.log' 2>&1 < /dev/null & echo \$! > '$REMOTE_DIR/forward.pid'"
  }
  rev_expect_refused() { # description
    if probe_host iran 4096 >/dev/null 2>&1; then
      echo "Reverse tunnel carried data for $1" >&2
      exit 1
    fi
  }
  # Make sure the only echo that can answer is the one on the service node.
  stop_role iran echo
  sleep 1
  rev_public iran iran abroad "$ABROAD_PIN" iran.crt iran.key
  sleep 1
  rev_service abroad abroad iran "$LAB_IRAN_HOST" iran.lab "$IRAN_PIN" abroad.crt abroad.key
  probe_host iran 65536
  probe_host iran 2097289
  # The private (service) side initiates outbound only: it must not listen on the tunnel port.
  ssh_host abroad "! ss -H -ltn | grep -q ':$E2E_PORT '" || { echo "service node listens on the tunnel port" >&2; exit 1; }
  ssh_host abroad "ss -H -tn state established | grep -q '$LAB_IRAN_HOST:$E2E_PORT'" || { echo "service node has no outbound tunnel connection" >&2; exit 1; }
  echo "--- REVERSE reconnect: restart the service node ---"
  stop_role abroad forward; sleep 2
  rev_service abroad abroad iran "$LAB_IRAN_HOST" iran.lab "$IRAN_PIN" abroad.crt abroad.key
  probe_host iran 65536
  echo "--- REVERSE reconnect: restart the public node ---"
  stop_role iran serve; sleep 2
  rev_public iran iran abroad "$ABROAD_PIN" iran.crt iran.key
  probe_host iran 65536
  probe_host iran 2097289
  echo "--- REVERSE busy port: a second public node on the same port must fail and not disturb the first ---"
  ssh_host iran "cp '$REMOTE_DIR/rev-public.json' '$REMOTE_DIR/rev-public2.json'"
  if ssh_host iran "timeout 15 '$BIN' apply --config '$REMOTE_DIR/rev-public2.json' --confirm" >/dev/null 2>&1; then
    echo "second Reverse public node started on a busy port" >&2
    exit 1
  fi
  probe_host iran 65536
  echo "--- REVERSE wrong identity: a valid certificate of another node must not relay ---"
  stop_role abroad forward; sleep 1
  rev_service abroad control iran "$LAB_IRAN_HOST" iran.lab "$IRAN_PIN" control.crt control.key
  sleep 3
  rev_expect_refused "a certificate of another node"
  echo "--- REVERSE wrong pin: the service node must refuse a public node with a different certificate ---"
  stop_role abroad forward; sleep 1
  rev_service abroad abroad iran "$LAB_IRAN_HOST" iran.lab "$CONTROL_PIN" abroad.crt abroad.key
  sleep 3
  rev_expect_refused "a wrongly pinned peer"
  echo "--- REVERSE recovery after refusals ---"
  stop_role abroad forward; sleep 1
  rev_service abroad abroad iran "$LAB_IRAN_HOST" iran.lab "$IRAN_PIN" abroad.crt abroad.key
  probe_host iran 65536
  if (( REVERSE_R3 > 0 )); then
    echo "=== REVERSE R3: crash/replace cycles, blackhole, mid-transfer kill, loss, soak, cleanup ==="
    rev_wait() { # seconds : wait until the Reverse tunnel carries 64 KiB again; prints the wait
      local t0=$SECONDS
      while (( SECONDS - t0 < $1 )); do
        if probe_host iran 65536 >/dev/null 2>&1; then echo $((SECONDS - t0)); return 0; fi
        sleep 2
      done
      return 1
    }
    kill9() { ssh_host "$1" "kill -KILL \$(cat '$REMOTE_DIR/$2.pid') 2>/dev/null; rm -f '$REMOTE_DIR/$2.pid'" || true; }
    read -r r3_pfd0 r3_prss0 < <(proc_stats iran serve)
    echo "--- R3 service node SIGKILL and restart, 5 cycles (public node must replace the stale session) ---"
    R3_SVC_MAX=0
    for i in 1 2 3 4 5; do
      kill9 abroad forward; sleep 1
      rev_service abroad abroad iran "$LAB_IRAN_HOST" iran.lab "$IRAN_PIN" abroad.crt abroad.key
      t="$(rev_wait 60)" || { echo "Reverse did not recover after service-node SIGKILL cycle $i" >&2; exit 1; }
      probe_host iran 2097289 >/dev/null
      if (( t > R3_SVC_MAX )); then R3_SVC_MAX=$t; fi
    done
    echo "--- R3 public node SIGKILL and restart (service node must reconnect by itself) ---"
    kill9 iran serve; sleep 1
    rev_public iran iran abroad "$ABROAD_PIN" iran.crt iran.key
    R3_PUB_S="$(rev_wait 90)" || { echo "Reverse did not recover after public-node SIGKILL" >&2; exit 1; }
    probe_host iran 2097289 >/dev/null
    read -r r3_pfd0 r3_prss0 < <(proc_stats iran serve)
    echo "--- R3 network blackhole 30 s on the tunnel flow (no RST), then recovery ---"
    ssh_host iran "! nft list table inet $LOSS_TABLE >/dev/null 2>&1" || { echo "stale $LOSS_TABLE table exists" >&2; exit 1; }
    r3_fam=ip; [[ "$LAB_ABROAD_HOST" == *:* ]] && r3_fam=ip6
    r3_rules() { # percent : drop that share (1-100) of the Reverse tunnel flow between abroad and iran:E2E_PORT
      local share="numgen random mod 100 '<' $1"
      (( $1 >= 100 )) && share=""
      ssh_host iran "nft add table inet $LOSS_TABLE && nft add chain inet $LOSS_TABLE in '{ type filter hook input priority 0; policy accept; }' && nft add chain inet $LOSS_TABLE out '{ type filter hook output priority 0; policy accept; }' && nft add rule inet $LOSS_TABLE in $r3_fam saddr $LAB_ABROAD_HOST tcp dport $E2E_PORT $share counter drop && nft add rule inet $LOSS_TABLE out $r3_fam daddr $LAB_ABROAD_HOST tcp sport $E2E_PORT $share counter drop"
    }
    r3_unrules() { ssh_host iran "nft delete table inet $LOSS_TABLE; ! nft list table inet $LOSS_TABLE >/dev/null 2>&1" || { echo "R3 nft table was not removed" >&2; exit 1; }; }
    r3_rules 100
    sleep 30
    if probe_host iran 4096 >/dev/null 2>&1; then echo "data passed through a full blackhole" >&2; exit 1; fi
    r3_unrules
    R3_BH_S="$(rev_wait 120)" || { echo "Reverse did not recover after a 30 s blackhole" >&2; exit 1; }
    probe_host iran 2097289 >/dev/null
    echo "--- R3 interruption mid-transfer: service node killed during a 2 MiB transfer ---"
    ssh_host iran "nohup python3 '$REMOTE_DIR/probe.py' 127.0.0.1 '$LOCAL_PORT' 2097289 > '$REMOTE_DIR/midprobe.log' 2>&1 < /dev/null & echo \$! > '$REMOTE_DIR/midprobe.pid'"
    sleep 0.3
    kill9 abroad forward
    sleep 3
    rev_service abroad abroad iran "$LAB_IRAN_HOST" iran.lab "$IRAN_PIN" abroad.crt abroad.key
    R3_MID_S="$(rev_wait 60)" || { echo "Reverse did not recover after a mid-transfer kill" >&2; exit 1; }
    probe_host iran 2097289 >/dev/null
    ssh_host iran "p=\$(cat '$REMOTE_DIR/midprobe.pid'); for _ in \$(seq 1 30); do kill -0 \$p 2>/dev/null || break; sleep 1; done; ! kill -0 \$p 2>/dev/null" || { echo "interrupted user connection was left hanging" >&2; exit 1; }
    R3_MID_PROBE="$(ssh_host iran "grep -q E2E_PAYLOAD_OK '$REMOTE_DIR/midprobe.log' && echo completed || echo failed_closed")"
    R3_LOSS_DROPPED=0
    if (( LOSS > 0 )); then
      echo "--- R3 real TCP packet loss ${LOSS}% on the Reverse tunnel flow ---"
      r3_rules "$LOSS"
      probe_host iran 65536; probe_host iran 2097289
      R3_LOSS_DROPPED="$(ssh_host iran "nft list table inet $LOSS_TABLE" | awk '/packets/ {for(i=1;i<NF;i++) if($i=="packets") sum+=$(i+1)} END{print sum+0}')"
      r3_unrules
    fi
    echo "--- R3 soak ${REVERSE_R3}s over Reverse ---"
    read -r r3_sfd0 r3_srss0 < <(proc_stats abroad forward)
    end=$((SECONDS + REVERSE_R3)); n=0
    while (( SECONDS < end )); do
      probe_host iran 65536 >/dev/null
      n=$((n + 1))
      if (( n % 6 == 0 )); then probe_host iran 2097289 >/dev/null; fi
      sleep 10
    done
    read -r r3_pfd1 r3_prss1 < <(proc_stats iran serve)
    read -r r3_sfd1 r3_srss1 < <(proc_stats abroad forward)
    (( r3_pfd1 <= r3_pfd0 + 8 && r3_sfd1 <= r3_sfd0 + 8 )) || { echo "fd growth in Reverse R3 (public $r3_pfd0->$r3_pfd1, service $r3_sfd0->$r3_sfd1)" >&2; exit 1; }
    (( r3_prss1 <= r3_prss0 * 2 + 20480 && r3_srss1 <= r3_srss0 * 2 + 20480 )) || { echo "memory growth in Reverse R3" >&2; exit 1; }
    ssh_host iran "! nft list table inet $LOSS_TABLE >/dev/null 2>&1" || { echo "R3 left an nft table behind" >&2; exit 1; }
    R3_STRAY="$(ssh_host abroad "pgrep -fc '^$REMOTE_DIR/baft-tunnel apply --config $REMOTE_DIR/rev-service.json' || true")"
    [[ "$R3_STRAY" == 1 ]] || { echo "expected exactly one service-node process, found $R3_STRAY" >&2; exit 1; }
    echo "REVERSE_R3_RESULT service_sigkill_cycles=5 service_recovery_max_s=$R3_SVC_MAX public_sigkill_recovery_s=$R3_PUB_S blackhole_30s_recovery_s=$R3_BH_S midtransfer_recovery_s=$R3_MID_S midtransfer_user_conn=$R3_MID_PROBE loss_percent=$LOSS loss_dropped_packets=$R3_LOSS_DROPPED soak_s=$REVERSE_R3 soak_rounds=$n public_fd=$r3_pfd0->$r3_pfd1 public_rss_kb=$r3_prss0->$r3_prss1 service_fd=$r3_sfd0->$r3_sfd1 service_rss_kb=$r3_srss0->$r3_srss1 stray_processes=0 nft_left=0"
  fi
  echo "--- REVERSE revoked certificate: a revoked service-node certificate must be refused ---"
  openssl ca -batch -config "$LOCAL_TMP/openssl.cnf" -revoke "$LOCAL_TMP/revabroad.crt" >/dev/null 2>&1
  openssl ca -gencrl -batch -config "$LOCAL_TMP/openssl.cnf" -out "$LOCAL_TMP/ca.crl" >/dev/null 2>&1
  for side in iran abroad; do copy_to_host "$side" "$LOCAL_TMP/ca.crl"; done
  stop_role abroad forward; stop_role iran serve; sleep 1
  rev_public iran iran abroad "$REVABROAD_PIN" iran.crt iran.key
  rev_service abroad abroad iran "$LAB_IRAN_HOST" iran.lab "$IRAN_PIN" revabroad.crt revabroad.key
  sleep 4
  ssh_host iran "grep -qi 'revok' '$REMOTE_DIR/serve.log'" || { echo "revoked service-node certificate was not rejected" >&2; exit 1; }
  rev_expect_refused "a revoked certificate"
  stop_role abroad forward; stop_role iran serve; sleep 1
  start_echo iran
  echo "REVERSE_RESULT engine=tcp-tls outbound_only=1 integrity_64k_2m=1 reconnect_service=1 reconnect_public=1 busy_port_refused=1 wrong_identity_refused=1 wrong_pin_refused=1 revoked_refused=1"
fi

if (( SETUP == 1 )); then
  echo "=== SETUP (P3-001 S4): clean directories configured only through 'setup', then engine switch ==="
  SVC_PORT=18091; TPORT=18310; SEP="$REMOTE_DIR/setup"
  stop_role iran serve; stop_role abroad serve; stop_role iran forward; stop_role abroad forward; sleep 1
  ssh_host abroad "nohup python3 '$REMOTE_DIR/echo.py' '$SVC_PORT' > '$REMOTE_DIR/echo2.log' 2>&1 < /dev/null & echo \$! > '$REMOTE_DIR/echo2.pid'"
  OLD_LOCAL_PORT="$LOCAL_PORT"; LOCAL_PORT="$SVC_PORT"
  BIN="$REMOTE_DIR/baft-tunnel"
  # setup_pair ENGINE DIRNAME: runs init+join on both hosts and starts both processes.
  # Per host the operator runs three commands: init, join, apply.
  setup_pair() {
    local engine="$1" name="$2" d="$SEP-$2" sum_a sum_i
    ssh_host abroad "mkdir -p '$d'"; ssh_host iran "mkdir -p '$d'"
    local common="--engine $engine --direction direct --tunnel-id lab-setup --service-port $SVC_PORT --tunnel-port $TPORT --dir '$d'"
    sum_a="$(ssh_host abroad "'$BIN' setup init $common --role serve --node-id abroad --peer-id iran" | awk '/^Checksum/{print $NF}')"
    sum_i="$(ssh_host iran "'$BIN' setup init $common --role forward --node-id iran --peer-id abroad --peer-host '$LAB_ABROAD_HOST'" | awk '/^Checksum/{print $NF}')"
    [[ -n "$sum_a" && -n "$sum_i" ]] || { echo "setup init printed no checksum" >&2; exit 1; }
    # Bundle transfer and the out-of-band checksum comparison are the operator's manual steps.
    ssh_host abroad "cat '$d/bundle.json'" > "$LOCAL_TMP/bundle-abroad.json"
    ssh_host iran "cat '$d/bundle.json'" > "$LOCAL_TMP/bundle-iran.json"
    cp "$LOCAL_TMP/bundle-abroad.json" "$LOCAL_TMP/peer-bundle-abroad-$name.json"
    cp "$LOCAL_TMP/bundle-iran.json" "$LOCAL_TMP/peer-bundle-iran-$name.json"
    copy_to_host iran "$LOCAL_TMP/peer-bundle-abroad-$name.json"
    copy_to_host abroad "$LOCAL_TMP/peer-bundle-iran-$name.json"
    ssh_host abroad "'$BIN' setup join --dir '$d' --bundle '$REMOTE_DIR/peer-bundle-iran-$name.json' --checksum '$sum_i'"
    ssh_host iran "'$BIN' setup join --dir '$d' --bundle '$REMOTE_DIR/peer-bundle-abroad-$name.json' --checksum '$sum_a'"
    ssh_host abroad "nohup '$BIN' apply --config '$d/config.json' --confirm > '$REMOTE_DIR/serve.log' 2>&1 < /dev/null & echo \$! > '$REMOTE_DIR/serve.pid'"
    ssh_host iran "nohup '$BIN' apply --config '$d/config.json' --confirm > '$REMOTE_DIR/forward.log' 2>&1 < /dev/null & echo \$! > '$REMOTE_DIR/forward.pid'"
    sleep 2
    probe_host iran 65536
    probe_host iran 2097289
  }
  # Installing PKI material is a one-time operator step done before any engine switch, so it is
  # prepared (and timed) separately from the switch itself.
  prepare_tls_pki() {
    local d="$SEP-tls"
    ssh_host abroad "mkdir -p '$d'"; ssh_host iran "mkdir -p '$d'"
    # Real offline-PKI output (scripts/pki/baft-pki.sh) generated on the runner, which plays the
    # offline admin machine. Only node.key/node.crt/ca.crt/ca.crl reach each host.
    local node dir_n
    for node in abroad iran; do
      dir_n="$LOCAL_TMP/realpki/$node"
      host_ip="$LAB_ABROAD_HOST"; [[ "$node" == iran ]] && host_ip="$LAB_IRAN_HOST"
      bash scripts/pki/baft-pki.sh new-node "$node" "$dir_n" >/dev/null
      bash scripts/pki/baft-pki.sh issue-node "$LOCAL_TMP/realpki/ca" "$node" "$dir_n/node.csr" "$LOCAL_TMP/realpki/issued-$node" "$host_ip" 30 >/dev/null
      bash scripts/pki/baft-pki.sh verify-node "$LOCAL_TMP/realpki/ca" "$LOCAL_TMP/realpki/issued-$node/ca.crl" "$LOCAL_TMP/realpki/issued-$node/node.crt" "$node" "$host_ip" >/dev/null
      mkdir -p "$LOCAL_TMP/realpki/send-$node"
      cp "$dir_n/node.key" "$LOCAL_TMP/realpki/issued-$node/"{node.crt,ca.crt,ca.crl} "$LOCAL_TMP/realpki/send-$node/"
      chmod 600 "$LOCAL_TMP/realpki/send-$node/node.key"
      for f in node.key node.crt ca.crt ca.crl; do
        cp "$LOCAL_TMP/realpki/send-$node/$f" "$LOCAL_TMP/realpki/send-$node/$node-$f"
        copy_to_host "$node" "$LOCAL_TMP/realpki/send-$node/$node-$f"
        ssh_host "$node" "mv '$REMOTE_DIR/$node-$f' '$d/$f'"
      done
      ssh_host "$node" "chmod 600 '$d/node.key'"
    done
  }
  t0=$SECONDS
  setup_pair noise-tcp noise
  noise_s=$((SECONDS - t0))
  stop_role iran forward; stop_role abroad serve; sleep 1
  mkdir -p "$LOCAL_TMP/realpki"; umask 077
  printf 'lab-ephemeral-ca-passphrase-%s\n' "$RANDOM$RANDOM" > "$LOCAL_TMP/realpki/pass"
  export BAFT_PKI_PASSFILE="$LOCAL_TMP/realpki/pass"
  bash scripts/pki/baft-pki.sh init-ca "$LOCAL_TMP/realpki/ca" >/dev/null 2>&1
  prepare_tls_pki
  t1=$SECONDS
  setup_pair tcp-tls tls
  switch_s=$((SECONDS - t1))
  # P3-002 A3: the agent reports the live tunnel and accepts a validated desired state.
  ad="$SEP-tls"
  ssh_host abroad "'$BIN' agent-token --out '$ad/agent.token' >/dev/null; mkdir -p '$ad/state'; nohup '$BIN' agent --config '$ad/config.json' --token-file '$ad/agent.token' --state-dir '$ad/state' --listen 127.0.0.1:18081 --allow-root > '$REMOTE_DIR/agent.log' 2>&1 < /dev/null & echo \$! > '$REMOTE_DIR/agent.pid'"
  sleep 1
  agent_call() {
    ssh_host abroad "python3 - '$ad' $1 $2 <<'PY'
import sys, urllib.request
d, method, path = sys.argv[1:4]
tok = open(d + '/agent.token').read().strip()
body = open(d + '/config.json','rb').read() if method == 'PUT' else None
r = urllib.request.Request('http://127.0.0.1:18081' + path, data=body, method=method, headers={'Authorization': 'Bearer ' + tok})
print(urllib.request.urlopen(r, timeout=5).read().decode())
PY"
  }
  agent_status="$(agent_call GET /v1/status)"
  [[ "$agent_status" == *'"listener_accepting":true'* ]] || { echo "agent does not see the live listener" >&2; exit 1; }
  agent_put="$(agent_call PUT /v1/desired)"
  [[ "$agent_put" == *'"accepted":true'* && "$agent_put" == *'"applied":false'* ]] || { echo "agent refused a valid desired state" >&2; exit 1; }
  # P4-001 W3: the panel is served by the agent on the real host and the wizard backend returns
  # the same setup commands that this stage just ran.
  panel_out="$(ssh_host abroad "python3 - '$ad' <<'PY'
import sys, json, urllib.request
d = sys.argv[1]
page = urllib.request.urlopen('http://127.0.0.1:18081/', timeout=5)
csp = page.headers.get('Content-Security-Policy', '')
html = page.read().decode()
tok = open(d + '/agent.token').read().strip()
body = json.dumps({'engine': 'tcp-tls', 'role': 'serve', 'direction': 'direct', 'node_id': 'abroad', 'peer_id': 'iran', 'tunnel_id': 'lab-setup', 'service_port': 18091, 'tunnel_port': 18310, 'dir': d}).encode()
r = urllib.request.Request('http://127.0.0.1:18081/v1/setup/plan', data=body, method='POST', headers={'Authorization': 'Bearer ' + tok})
plan = json.loads(urllib.request.urlopen(r, timeout=5).read().decode())
rp = urllib.request.Request('http://127.0.0.1:18081/v1/setup/preflight', data=body, method='POST', headers={'Authorization': 'Bearer ' + tok})
pf = json.loads(urllib.request.urlopen(rp, timeout=5).read().decode())
res = {c['id']: c['result'] for c in pf['checks']}
pf_ok = pf['applied'] is False and pf['apply_available'] is False and pf['overall'] in ('unknown', 'blocked') and res.get('planner') == 'pass' and res.get('peer_reachability') == 'unknown' and res.get('firewall') == 'unknown'
ok = pf_ok and '<title>BAFT-TUNNEL</title>' in html and \"script-src 'self'\" in csp and plan['applied'] is False and plan['commands'][0].startswith('baft-tunnel setup init --engine tcp-tls --role serve') and '--service-port 18091' in plan['commands'][0]
print('PANEL_OK' if ok else 'PANEL_BAD')
PY")"
  [[ "$panel_out" == *PANEL_OK* ]] || { echo "panel or wizard check failed" >&2; exit 1; }
  echo "AGENT_RESULT status_listener_accepting=1 desired_state_accepted=1 applied=0 panel_served=1 wizard_plan_ok=1 preflight_ok=1"
  stop_role abroad agent
  stop_role iran forward; stop_role abroad serve
  ssh_host abroad "p=\$(cat '$REMOTE_DIR/echo2.pid' 2>/dev/null); [ -n \"\$p\" ] && kill \$p 2>/dev/null; true"
  LOCAL_PORT="$OLD_LOCAL_PORT"
  echo "SETUP_RESULT noise_install_s=$noise_s engine_switch_to_tls_s=$switch_s commands_per_host=3 (init, join, apply; bundle copy and checksum comparison are manual) probe_ok=1"
  if (( switch_s > 60 )); then
    echo "Engine switch took ${switch_s}s (limit 60s)" >&2
    exit 1
  fi
fi

if (( ROLLOVER == 1 )); then
  echo "=== ROLLOVER (ADR-0017 B1): two CAs with per-CA CRLs, two pins, then old CA removed ==="
  R="$LOCAL_TMP/ca2"; mkdir -p "$R/newcerts" "$R/bundle"
  : > "$R/index.txt"; printf '2000\n' > "$R/serial"; printf '2000\n' > "$R/crlnumber"
  openssl req -x509 -newkey rsa:3072 -nodes -keyout "$R/ca.key" -out "$R/ca.crt" \
    -days 2 -sha256 -subj "/CN=BAFT-TUNNEL ephemeral LAB CA 2" \
    -addext "basicConstraints=critical,CA:TRUE" -addext "keyUsage=critical,keyCertSign,cRLSign" >/dev/null 2>&1
  cat > "$R/openssl.cnf" <<CNF
[ ca ]
default_ca = CA_default
[ CA_default ]
database = $R/index.txt
new_certs_dir = $R/newcerts
certificate = $R/ca.crt
private_key = $R/ca.key
serial = $R/serial
crlnumber = $R/crlnumber
default_md = sha256
default_days = 2
default_crl_days = 2
policy = policy_any
copy_extensions = copy
[ policy_any ]
commonName = supplied
CNF
  for node in iran abroad; do
    openssl req -new -newkey rsa:2048 -nodes -keyout "$LOCAL_TMP/${node}2.key" \
      -out "$R/$node.csr" -subj "/CN=$node.lab" >/dev/null 2>&1
    printf '[ %s_ext ]\nbasicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature\nextendedKeyUsage=serverAuth,clientAuth\nsubjectAltName=DNS:%s.lab,URI:urn:baft-tunnel:node:%s\n' "$node" "$node" "$node" >> "$R/openssl.cnf"
    openssl ca -batch -notext -config "$R/openssl.cnf" -extensions "${node}_ext" \
      -in "$R/$node.csr" -out "$LOCAL_TMP/${node}2.crt" -days 2 >/dev/null 2>&1
  done
  openssl ca -gencrl -batch -config "$R/openssl.cnf" -out "$R/ca.crl" >/dev/null 2>&1
  cat "$LOCAL_TMP/ca.crt" "$R/ca.crt" > "$R/bundle/ca.crt"
  cat "$LOCAL_TMP/ca.crl" "$R/ca.crl" > "$R/bundle/ca.crl"
  IRAN_PIN2="$(openssl x509 -in "$LOCAL_TMP/iran2.crt" -outform DER | sha256sum | awk '{print $1}')"
  ABROAD_PIN2="$(openssl x509 -in "$LOCAL_TMP/abroad2.crt" -outform DER | sha256sum | awk '{print $1}')"
  for side in iran abroad; do
    copy_to_host "$side" "$LOCAL_TMP/iran2.crt" "$LOCAL_TMP/iran2.key" "$LOCAL_TMP/abroad2.crt" "$LOCAL_TMP/abroad2.key" "$R/bundle/ca.crt" "$R/bundle/ca.crl"
    ssh_host "$side" "chmod 600 '$REMOTE_DIR/'*.key"
  done
  echo "--- ROLLOVER 1/3: overlap, old certificates, bundle trust, two pins each side ---"
  start_serve abroad abroad iran direct "$IRAN_PIN,$IRAN_PIN2" abroad.crt abroad.key
  start_forward iran iran abroad "$LAB_ABROAD_HOST" abroad.lab direct "$ABROAD_PIN,$ABROAD_PIN2" iran.crt iran.key
  probe_host iran 65536
  stop_role iran forward; stop_role abroad serve; sleep 1
  echo "--- ROLLOVER 2/3: new CA certificates, bundle trust, two pins each side ---"
  start_serve abroad abroad iran direct "$IRAN_PIN,$IRAN_PIN2" abroad2.crt abroad2.key
  start_forward iran iran abroad "$LAB_ABROAD_HOST" abroad.lab direct "$ABROAD_PIN,$ABROAD_PIN2" iran2.crt iran2.key
  probe_host iran 65536
  probe_host iran 2097289
  stop_role iran forward; stop_role abroad serve; sleep 1
  echo "--- ROLLOVER 3/3: old CA and old pins removed; old-CA client must be refused ---"
  for side in iran abroad; do copy_to_host "$side" "$R/ca.crt" "$R/ca.crl"; done
  start_serve abroad abroad iran direct "$IRAN_PIN2" abroad2.crt abroad2.key
  start_forward iran iran abroad "$LAB_ABROAD_HOST" abroad.lab direct "$ABROAD_PIN2" iran2.crt iran2.key
  probe_host iran 65536
  stop_role iran forward; sleep 1
  start_forward iran iran abroad "$LAB_ABROAD_HOST" abroad.lab direct "$ABROAD_PIN2" iran.crt iran.key
  sleep 2
  if probe_host iran 65536 >/dev/null 2>&1; then
    echo "Server accepted a certificate from the removed CA" >&2
    exit 1
  fi
  echo "ROLLOVER_RESULT overlap_ok=1 new_certs_ok=1 old_ca_removed_refused=1"
  stop_role iran forward; stop_role abroad serve
  for side in iran abroad; do copy_to_host "$side" "$LOCAL_TMP/ca.crt" "$LOCAL_TMP/ca.crl"; done
fi

if (( BAFT == 1 )); then
  echo "=== BAFT ENGINE: pinned Go engine host (Direct, unauthorized identity, restart, Reverse, bad hash, revocation) ==="
  BAFT_SHA="$(sha256sum "$BAFT_BINARY_PATH" | awk '{print $1}')"
  # BAFT requires exactly one URI SAN, urn:baft:node:<id>. Same CA, engine-specific identity profile.
  for node in iran abroad control baftrev revsrv; do
    openssl req -new -newkey rsa:2048 -nodes -keyout "$LOCAL_TMP/b-$node.key" \
      -out "$LOCAL_TMP/b-$node.csr" -subj "/CN=b-$node.lab" >/dev/null 2>&1
    printf '[ b_%s_ext ]\nbasicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature\nextendedKeyUsage=serverAuth,clientAuth\nsubjectAltName=DNS:%s.lab,URI:urn:baft:node:%s\n' "$node" "$node" "$node" >> "$LOCAL_TMP/openssl.cnf"
    openssl ca -batch -notext -config "$LOCAL_TMP/openssl.cnf" -extensions "b_${node}_ext" \
      -in "$LOCAL_TMP/b-$node.csr" -out "$LOCAL_TMP/b-$node.crt" -days 2 >/dev/null 2>&1
  done
  for side in iran abroad; do
    copy_to_host "$side" "$BAFT_BINARY_PATH" "$LOCAL_TMP"/b-*.crt "$LOCAL_TMP"/b-*.key
    ssh_host "$side" "mv '$REMOTE_DIR/$(basename "$BAFT_BINARY_PATH")' '$REMOTE_DIR/baft-engine' 2>/dev/null || true; chmod 700 '$REMOTE_DIR/baft-engine'; chmod 600 '$REMOTE_DIR/'*.key"
  done
  cert_pin() { openssl x509 -in "$LOCAL_TMP/$1" -outform DER | sha256sum | awk '{print $1}'; }
  baft_cfg() { # role local_id peer_id cert key [peer_host peer_dns] [sha] [listener_cert_pin]
    local role="$1" local_id="$2" peer_id="$3" cert="$4" key="$5" peer_host="${6:-}" peer_dns="${7:-}" sha="${8:-$BAFT_SHA}" pin="${9:-}" common pinfield=""
    [[ -z "$pin" ]] || pinfield="\"peer_fingerprint\":\"$pin\","
    local bextra=""
    [[ -z "${BAFT_STORE:-}" ]] || bextra=",\"store\":\"$BAFT_STORE\""
    common="\"local_id\":\"$local_id\",\"peer_id\":\"$peer_id\",\"tunnel_id\":\"lab-e2e\",\"direction\":\"direct\",\"engine\":\"baft\",\"ca\":\"$REMOTE_DIR/ca.crt\",\"crl\":\"$REMOTE_DIR/ca.crl\",\"cert\":\"$REMOTE_DIR/$cert\",\"key\":\"$REMOTE_DIR/$key\",\"max_streams\":8,\"baft\":{\"binary\":\"$REMOTE_DIR/baft-engine\",\"sha256\":\"$sha\"$bextra}"
    if [[ "$role" == serve ]]; then
      printf '{"mode":"serve","listen":"0.0.0.0:%s","target":"127.0.0.1:%s",%s}' "$E2E_PORT" "$ECHO_PORT" "$common"
    else
      printf '{"mode":"forward","listen":"127.0.0.1:%s","remote":"%s:%s","server_name":"%s",%s%s}' "$LOCAL_PORT" "$peer_host" "$E2E_PORT" "$peer_dns" "$pinfield" "$common"
    fi
  }
  baft_start() { # side role cfgjson
    local side="$1" role="$2" cfg="$3"
    # Only the engine command is backgrounded: an `a && b && c &` chain would keep ssh's stdout
    # open through its subshell and make this call block until the tunnel stops (found in the
    # first real run, which hung here).
    ssh_host "$side" "rm -rf '$REMOTE_DIR/rt-$role' && mkdir -m 700 '$REMOTE_DIR/rt-$role' && printf '%s' '$cfg' > '$REMOTE_DIR/baft-$role.json'; RUNTIME_DIRECTORY='$REMOTE_DIR/rt-$role' nohup '$REMOTE_DIR/baft-tunnel' apply --config '$REMOTE_DIR/baft-$role.json' --confirm > '$REMOTE_DIR/$role.log' 2>&1 < /dev/null & echo \$! > '$REMOTE_DIR/$role.pid'"
  }
  baft_rss() { # side role : launcher + child RSS in kB
    ssh_host "$1" "p=\$(cat '$REMOTE_DIR/$2.pid'); t=0; for q in \$p \$(pgrep -P \$p); do t=\$((t + \$(awk '/VmRSS/{print \$2}' /proc/\$q/status))); done; echo \$t"
  }
  expect_refused() { # side description
    sleep 3
    if probe_host "$1" 65536 >/dev/null 2>&1; then echo "BAFT accepted: $2" >&2; exit 1; fi
  }
  echo "--- BAFT Direct: Iran dials abroad ---"
  baft_start abroad serve "$(baft_cfg serve abroad iran b-abroad.crt b-abroad.key)"
  baft_start iran forward "$(baft_cfg forward iran abroad b-iran.crt b-iran.key "$LAB_ABROAD_HOST" abroad.lab "" "$(cert_pin b-abroad.crt)")"
  probe_host iran 65536
  probe_host iran 2097289
  BAFT_RSS_SERVE="$(baft_rss abroad serve)"; BAFT_RSS_FWD="$(baft_rss iran forward)"
  baft_fds() { # side role : open fds of launcher + child
    ssh_host "$1" "p=\$(cat '$REMOTE_DIR/$2.pid'); t=0; for q in \$p \$(pgrep -P \$p); do t=\$((t + \$(ls /proc/\$q/fd | wc -l))); done; echo \$t"
  }
  BAFT_SOAK_RESULT=""; BAFT_LOSS_RESULT=""
  if (( SOAK_SECONDS > 0 )); then
    echo "--- BAFT stability: ${SOAK_SECONDS}s soak over the BAFT Direct tunnel ---"
    bf0="$(baft_fds iran forward)"; bsf0="$(baft_fds abroad serve)"; br0="$BAFT_RSS_FWD"; bsr0="$BAFT_RSS_SERVE"
    end=$((SECONDS + SOAK_SECONDS)); bn=0
    while (( SECONDS < end )); do
      probe_host iran 65536 >/dev/null; bn=$((bn + 1))
      if (( bn % 6 == 0 )); then probe_host iran 2097289 >/dev/null; fi
      sleep 10
    done
    bf1="$(baft_fds iran forward)"; bsf1="$(baft_fds abroad serve)"; br1="$(baft_rss iran forward)"; bsr1="$(baft_rss abroad serve)"
    BAFT_SOAK_RESULT="soak_s=$SOAK_SECONDS soak_rounds=$bn fwd_fd=$bf0->$bf1 serve_fd=$bsf0->$bsf1 fwd_rss_kb=$br0->$br1 serve_rss_kb=$bsr0->$bsr1"
    echo "BAFT_SOAK_RESULT $BAFT_SOAK_RESULT"
    (( bf1 <= bf0 + 8 && bsf1 <= bsf0 + 8 )) || { echo "BAFT fd growth during soak" >&2; exit 1; }
    (( br1 <= br0 * 2 + 20480 && bsr1 <= bsr0 * 2 + 20480 )) || { echo "BAFT memory growth during soak" >&2; exit 1; }
  fi
  if (( LOSS > 0 )); then
    echo "--- BAFT stability: real TCP packet loss ${LOSS}% on the BAFT flow ---"
    ssh_host iran "command -v nft >/dev/null" || { echo "nft is not available on the Iran host" >&2; exit 1; }
    ssh_host iran "! nft list table inet $LOSS_TABLE >/dev/null 2>&1" || { echo "stale $LOSS_TABLE table exists" >&2; exit 1; }
    if [[ "$LAB_ABROAD_HOST" == *:* ]]; then bfam=ip6; else bfam=ip; fi
    ssh_host iran "nft add table inet $LOSS_TABLE && nft add chain inet $LOSS_TABLE out '{ type filter hook output priority 0; policy accept; }' && nft add chain inet $LOSS_TABLE in '{ type filter hook input priority 0; policy accept; }' && nft add rule inet $LOSS_TABLE out $bfam daddr $LAB_ABROAD_HOST tcp dport $E2E_PORT numgen random mod 100 '<' $LOSS counter drop && nft add rule inet $LOSS_TABLE in $bfam saddr $LAB_ABROAD_HOST tcp sport $E2E_PORT numgen random mod 100 '<' $LOSS counter drop"
    probe_host iran 65536; probe_host iran 524288; probe_host iran 65536
    bdropped="$(ssh_host iran "nft list table inet $LOSS_TABLE" | awk '/packets/ {for(i=1;i<NF;i++) if($i=="packets") sum+=$(i+1)} END{print sum+0}')"
    ssh_host iran "nft delete table inet $LOSS_TABLE"
    ssh_host iran "! nft list table inet $LOSS_TABLE >/dev/null 2>&1" || { echo "loss table was not removed" >&2; exit 1; }
    (( bdropped > 0 )) || { echo "no packets were dropped; loss was not exercised" >&2; exit 1; }
    probe_host iran 65536
    BAFT_LOSS_RESULT="loss_percent=$LOSS loss_dropped_packets=$bdropped"
    echo "BAFT_LOSS_RESULT $BAFT_LOSS_RESULT"
  fi
  echo "--- BAFT unauthorized identity: valid certificate that is not the allowed peer ---"
  stop_role iran forward; sleep 1
  baft_start iran forward "$(baft_cfg forward control abroad b-control.crt b-control.key "$LAB_ABROAD_HOST" abroad.lab)"
  expect_refused iran "a certificate whose identity is not the allowed peer"
  stop_role iran forward; sleep 1
  echo "--- BAFT listener pin: a pin that does not match the listener certificate must be refused ---"
  baft_start iran forward "$(baft_cfg forward iran abroad b-iran.crt b-iran.key "$LAB_ABROAD_HOST" abroad.lab "" "$(cert_pin b-control.crt)")"
  expect_refused iran "a listener certificate that differs from the pin"
  stop_role iran forward; sleep 1
  echo "--- BAFT restart: listener killed and started again, dialer must recover on its own ---"
  baft_start iran forward "$(baft_cfg forward iran abroad b-iran.crt b-iran.key "$LAB_ABROAD_HOST" abroad.lab)"
  probe_host iran 65536
  stop_role abroad serve; sleep 2
  baft_start abroad serve "$(baft_cfg serve abroad iran b-abroad.crt b-abroad.key)"
  # BAFT keeps trying to recover the old session for recovery.retention_seconds (30 s); the listener
  # restarted without it, so the dialer process then exits. Under systemd the unit restarts it
  # (Restart=on-failure); this script plays that part and requires a fresh session to carry data.
  BAFT_T0=$SECONDS; BAFT_RECOVERED=0; BAFT_DIALER_RESTARTED=0
  for _ in $(seq 1 24); do
    if probe_host iran 65536 >/dev/null 2>&1; then BAFT_RECOVERED=1; break; fi
    if ! ssh_host iran "kill -0 \$(cat '$REMOTE_DIR/forward.pid') 2>/dev/null"; then
      BAFT_DIALER_RESTARTED=1
      baft_start iran forward "$(baft_cfg forward iran abroad b-iran.crt b-iran.key "$LAB_ABROAD_HOST" abroad.lab)"
    fi
    sleep 5
  done
  (( BAFT_RECOVERED == 1 )) || { echo "BAFT tunnel did not carry data again after the listener restarted" >&2; exit 1; }
  BAFT_RECOVERY_S=$((SECONDS - BAFT_T0))
  stop_role iran forward; stop_role abroad serve; sleep 1
  echo "--- BAFT bad binary hash must refuse to start ---"
  baft_start abroad serve "$(baft_cfg serve abroad iran b-abroad.crt b-abroad.key "" "" "$(printf '0%.0s' $(seq 64))")"
  sleep 2
  ssh_host abroad "grep -q 'does not match the pinned value' '$REMOTE_DIR/serve.log'" || { echo "pinned hash was not enforced" >&2; exit 1; }
  stop_role abroad serve
  echo "--- BAFT Reverse: abroad dials Iran ---"
  baft_start iran serve "$(baft_cfg serve iran abroad b-iran.crt b-iran.key)"
  baft_start abroad forward "$(baft_cfg forward abroad iran b-abroad.crt b-abroad.key "$LAB_IRAN_HOST" iran.lab)"
  ssh_host abroad "python3 '$REMOTE_DIR/probe.py' 127.0.0.1 '$LOCAL_PORT' 65536"
  ssh_host abroad "python3 '$REMOTE_DIR/probe.py' 127.0.0.1 '$LOCAL_PORT' 2097289"
  stop_role abroad forward; stop_role iran serve; sleep 1
  echo "--- BAFT revocation: revoked client certificate must be refused (CRL converted to BAFT revocation list) ---"
  openssl ca -batch -config "$LOCAL_TMP/openssl.cnf" -revoke "$LOCAL_TMP/b-baftrev.crt" >/dev/null 2>&1
  openssl ca -gencrl -batch -config "$LOCAL_TMP/openssl.cnf" -out "$LOCAL_TMP/ca.crl" >/dev/null 2>&1
  for side in iran abroad; do copy_to_host "$side" "$LOCAL_TMP/ca.crl"; done
  baft_start abroad serve "$(baft_cfg serve abroad baftrev b-abroad.crt b-abroad.key)"
  baft_start iran forward "$(baft_cfg forward baftrev abroad b-baftrev.crt b-baftrev.key "$LAB_ABROAD_HOST" abroad.lab)"
  expect_refused iran "a revoked client certificate"
  stop_role iran forward; stop_role abroad serve
  echo "--- BAFT revoked listener: the dialer must refuse a revoked listener certificate ---"
  # Control first: the same listener certificate works while it is not revoked (old CRL has only baftrev).
  baft_start abroad serve "$(baft_cfg serve revsrv iran b-revsrv.crt b-revsrv.key)"
  baft_start iran forward "$(baft_cfg forward iran revsrv b-iran.crt b-iran.key "$LAB_ABROAD_HOST" revsrv.lab)"
  probe_host iran 65536
  stop_role iran forward; stop_role abroad serve; sleep 1
  openssl ca -batch -config "$LOCAL_TMP/openssl.cnf" -revoke "$LOCAL_TMP/b-revsrv.crt" >/dev/null 2>&1
  openssl ca -gencrl -batch -config "$LOCAL_TMP/openssl.cnf" -out "$LOCAL_TMP/ca.crl" >/dev/null 2>&1
  for side in iran abroad; do copy_to_host "$side" "$LOCAL_TMP/ca.crl"; done
  baft_start abroad serve "$(baft_cfg serve revsrv iran b-revsrv.crt b-revsrv.key)"
  baft_start iran forward "$(baft_cfg forward iran revsrv b-iran.crt b-iran.key "$LAB_ABROAD_HOST" revsrv.lab)"
  expect_refused iran "a revoked listener certificate"
  stop_role iran forward; stop_role abroad serve
  if (( BAFT_SYSTEMD == 1 )); then
    echo "--- BAFT under hardened systemd units (shipped unit hardening, Ubuntu 22.04 / systemd 249 baseline) ---"
    # Transient units carry exactly the hardening keys of systemd/baft-tunnel@.service, so a change to
    # the shipped unit changes this test. A throwaway user and /opt directory are removed afterwards.
    SD_UNIT="$(dirname "${BASH_SOURCE[0]}")/../../systemd/baft-tunnel@.service"
    [[ -r "$SD_UNIT" ]] || { echo "shipped unit not found: $SD_UNIT" >&2; exit 2; }
    SD_PROPS="$(awk '/^\[Service\]/{s=1;next} /^\[/{s=0} s && /^(NoNewPrivileges|PrivateTmp|PrivateDevices|ProtectSystem|ProtectHome|ProtectKernelTunables|ProtectKernelModules|ProtectControlGroups|ProtectClock|ProtectHostname|ProtectProc|ProcSubset|RestrictAddressFamilies|RestrictSUIDSGID|RestrictNamespaces|RestrictRealtime|LockPersonality|MemoryDenyWriteExecute|SystemCallFilter|SystemCallArchitectures|CapabilityBoundingSet|AmbientCapabilities|UMask|LimitNOFILE|TasksMax|MemoryMax)=/{printf "-p %s ", "\"" $0 "\""}' "$SD_UNIT")"
    [[ "$SD_PROPS" == *MemoryDenyWriteExecute=true* && "$SD_PROPS" == *SystemCallFilter=* ]] || { echo "hardening keys not found in the shipped unit" >&2; exit 2; }
    SD_DIR=/opt/baft-lab-e2e
    sd_prepare() { # side
      ssh_host "$1" "set -e; systemctl --version | head -1; . /etc/os-release; echo \$PRETTY_NAME;
        id baftlab >/dev/null 2>&1 || useradd --system --no-create-home --shell /usr/sbin/nologin baftlab;
        rm -rf '$SD_DIR'; mkdir -m 755 '$SD_DIR';
        cp '$REMOTE_DIR/baft-tunnel' '$REMOTE_DIR/baft-engine' '$REMOTE_DIR/ca.crt' '$REMOTE_DIR/ca.crl' '$REMOTE_DIR'/b-*.crt '$SD_DIR'/;
        cp '$REMOTE_DIR'/b-*.key '$SD_DIR'/; chmod 755 '$SD_DIR/baft-tunnel' '$SD_DIR/baft-engine'; chmod 644 '$SD_DIR/ca.crt' '$SD_DIR/ca.crl' '$SD_DIR'/b-*.crt;
        chown baftlab:baftlab '$SD_DIR'/b-*.key; chmod 600 '$SD_DIR'/b-*.key"
    }
    sd_cfg() { ( REMOTE_DIR="$SD_DIR"; baft_cfg "$@" ); }
    sd_run() { # side role cfgjson
      local side="$1" role="$2" cfg="$3"
      ssh_host "$side" "printf '%s' '$cfg' > '$SD_DIR/sd-$role.json'; systemctl reset-failed baft-lab-$role 2>/dev/null; systemd-run --quiet --unit=baft-lab-$role --collect -p User=baftlab -p Group=baftlab -p WorkingDirectory=$SD_DIR -p RuntimeDirectory=baft-lab/$role -p RuntimeDirectoryMode=0700 -p Restart=on-failure -p RestartSec=3s -p TimeoutStopSec=20s -p KillSignal=SIGTERM $SD_PROPS '$SD_DIR/baft-tunnel' apply --config '$SD_DIR/sd-$role.json' --confirm"
    }
    sd_stop() { ssh_host "$1" "systemctl stop baft-lab-$2 2>/dev/null; systemctl reset-failed baft-lab-$2 2>/dev/null; true" || true; }
    sd_cleanup() {
      for side in iran abroad; do
        ssh_host "$side" "systemctl stop baft-lab-serve baft-lab-forward 2>/dev/null; systemctl reset-failed baft-lab-serve baft-lab-forward 2>/dev/null; rm -rf '$SD_DIR'; userdel baftlab 2>/dev/null; true" || true
      done
    }
    trap 'sd_status=$?; set +e; sd_cleanup; (exit "$sd_status"); cleanup' EXIT
    sd_prepare abroad; sd_prepare iran
    # Engine store -> launcher: the abroad listener runs the store's verified `current` release.
    SD_MANIFEST="$(dirname "${BASH_SOURCE[0]}")/../../release/engine-manifest.json"
    grep -q "$BAFT_SHA" "$SD_MANIFEST" || { echo "engine under test ($BAFT_SHA) is not the release/engine-manifest.json artifact" >&2; exit 2; }
    copy_to_host abroad "$SD_MANIFEST"
    ssh_host abroad "cp '$REMOTE_DIR/engine-manifest.json' '$SD_DIR/engine-manifest.json'; '$SD_DIR/baft-tunnel' engine install --store '$SD_DIR/store' --manifest '$SD_DIR/engine-manifest.json' --id baft --binary '$SD_DIR/baft-engine' && '$SD_DIR/baft-tunnel' engine verify --store '$SD_DIR/store' --id baft"
    sd_store_cfg() { ( REMOTE_DIR="$SD_DIR"; BAFT_STORE="$SD_DIR/store"; baft_cfg "$@" ); }
    sd_run abroad serve "$(sd_store_cfg serve abroad iran b-abroad.crt b-abroad.key)"
    sd_run iran forward "$(sd_cfg forward iran abroad b-iran.crt b-iran.key "$LAB_ABROAD_HOST" abroad.lab)"
    sleep 8
    probe_host iran 65536; probe_host iran 2097289
    SD_PROP_CHECK="$(ssh_host abroad "systemctl show baft-lab-serve -p ActiveState -p MemoryDenyWriteExecute -p NoNewPrivileges -p NRestarts | tr '\n' ' '")"
    echo "systemd properties (abroad serve): $SD_PROP_CHECK"
    [[ "$SD_PROP_CHECK" == *"ActiveState=active"* && "$SD_PROP_CHECK" == *"MemoryDenyWriteExecute=yes"* ]] || { echo "hardened unit not active with MemoryDenyWriteExecute" >&2; exit 1; }
    if ssh_host abroad "journalctl -u baft-lab-serve --no-pager 2>/dev/null | grep -Eqi 'bad system call|SIGSYS|status=31'"; then echo "seccomp kill seen in serve unit" >&2; exit 1; fi
    echo "--- BAFT systemd recovery: listener unit restarted, dialer unit must come back by itself ---"
    ssh_host iran "systemctl show baft-lab-forward -p NRestarts" >/dev/null
    SD_T0=$SECONDS; SD_REC=0
    sd_stop abroad serve; sleep 2
    sd_run abroad serve "$(sd_store_cfg serve abroad iran b-abroad.crt b-abroad.key)"
    for _ in $(seq 1 30); do
      if probe_host iran 65536 >/dev/null 2>&1; then SD_REC=1; break; fi
      sleep 5
    done
    (( SD_REC == 1 )) || { echo "BAFT did not recover under systemd" >&2; exit 1; }
    SD_REC_S=$((SECONDS - SD_T0))
    SD_NRESTARTS="$(ssh_host iran "systemctl show baft-lab-forward -p NRestarts --value")"
    echo "--- BAFT systemd engine kill: SIGKILL of the Go engine, launcher must restart it ---"
    ssh_host abroad "pkill -KILL -f '^$SD_DIR/store/baft/releases/.*/engine ' || true"
    SD_KILL_T0=$SECONDS; SD_KILL_REC=0
    for _ in $(seq 1 30); do
      sleep 5
      if probe_host iran 65536 >/dev/null 2>&1; then SD_KILL_REC=1; break; fi
    done
    (( SD_KILL_REC == 1 )) || { echo "BAFT did not recover after the engine was killed under systemd" >&2; exit 1; }
    SD_KILL_S=$((SECONDS - SD_KILL_T0))
    sd_stop iran forward; sd_stop abroad serve
    echo "--- BAFT store: a config pinning another release must be refused by the launcher ---"
    ssh_host abroad "printf '%s' '$(sd_store_cfg serve abroad iran b-abroad.crt b-abroad.key "" "" "$(printf '0%.0s' $(seq 64))")' > '$SD_DIR/sd-wrongpin.json'; if RUNTIME_DIRECTORY='$SD_DIR/rt-wrongpin' '$SD_DIR/baft-tunnel' apply --config '$SD_DIR/sd-wrongpin.json' --confirm >'$SD_DIR/wrongpin.log' 2>&1; then exit 1; fi; grep -q 'not the release pinned' '$SD_DIR/wrongpin.log'"
    echo "--- BAFT store rollback: release B over A, pins must follow current, tampered previous refused ---"
    # Release B is the same pinned engine with a trailing marker (ELF ignores trailing bytes), so it has
    # a different digest; its manifest differs from the release manifest only in artifact_sha256.
    cp "$BAFT_BINARY_PATH" "$LOCAL_TMP/baft-engine-b"; printf 'baft-lab-rollback-marker-%s' "${GITHUB_RUN_ID:-$$}" >> "$LOCAL_TMP/baft-engine-b"
    BAFT_SHA_B="$(sha256sum "$LOCAL_TMP/baft-engine-b" | awk '{print $1}')"
    python3 - "$SD_MANIFEST" "$BAFT_SHA_B" > "$LOCAL_TMP/engine-manifest-b.json" <<'MANIFEST'
import json, sys
m = json.load(open(sys.argv[1]))
[e for e in m["engines"] if e["id"] == "baft"][0]["artifact_sha256"] = sys.argv[2]
json.dump(m, sys.stdout)
MANIFEST
    copy_to_host abroad "$LOCAL_TMP/baft-engine-b" "$LOCAL_TMP/engine-manifest-b.json"
    ssh_host abroad "install -m 644 '$REMOTE_DIR/engine-manifest-b.json' '$SD_DIR/engine-manifest-b.json'; install -m 755 '$REMOTE_DIR/baft-engine-b' '$SD_DIR/baft-engine-b'; '$SD_DIR/baft-tunnel' engine install --store '$SD_DIR/store' --manifest '$SD_DIR/engine-manifest-b.json' --id baft --binary '$SD_DIR/baft-engine-b'"
    store_pin_refused() { # sha : the launcher must refuse a config whose pin is not the store's current
      ssh_host abroad "printf '%s' '$(sd_store_cfg serve abroad iran b-abroad.crt b-abroad.key "" "" "$1")' > '$SD_DIR/sd-pin.json'; if RUNTIME_DIRECTORY='$SD_DIR/rt-pin' '$SD_DIR/baft-tunnel' apply --config '$SD_DIR/sd-pin.json' --confirm >'$SD_DIR/pin.log' 2>&1; then exit 1; fi; grep -q 'not the release pinned' '$SD_DIR/pin.log'"
    }
    store_carries() { # sha : run the abroad listener from the store with this pin and carry data
      sd_run abroad serve "$(sd_store_cfg serve abroad iran b-abroad.crt b-abroad.key "" "" "$1")"
      sd_run iran forward "$(sd_cfg forward iran abroad b-iran.crt b-iran.key "$LAB_ABROAD_HOST" abroad.lab)"
      local ok=0
      for _ in $(seq 1 12); do sleep 5; if probe_host iran 65536 >/dev/null 2>&1; then ok=1; break; fi; done
      (( ok == 1 )) && probe_host iran 2097289 >/dev/null
      local rc=$?
      ssh_host abroad "pgrep -f '^$SD_DIR/store/baft/releases/$1/engine ' >/dev/null" || rc=1
      sd_stop iran forward; sd_stop abroad serve
      return $rc
    }
    store_pin_refused "$BAFT_SHA" || { echo "store current=B but a config pinned to A was not refused" >&2; exit 1; }
    store_carries "$BAFT_SHA_B" || { echo "store release B did not carry data when pinned" >&2; exit 1; }
    ssh_host abroad "'$SD_DIR/baft-tunnel' engine rollback --store '$SD_DIR/store' --id baft | grep -q 'current = $BAFT_SHA'" || { echo "rollback did not make A current" >&2; exit 1; }
    store_pin_refused "$BAFT_SHA_B" || { echo "after rollback a config still pinned to B was not refused" >&2; exit 1; }
    # previous is now B: tamper it on disk; a rollback to it must be refused and current must stay A.
    ssh_host abroad "printf x >> '$SD_DIR/store/baft/releases/$BAFT_SHA_B/engine'; if '$SD_DIR/baft-tunnel' engine rollback --store '$SD_DIR/store' --id baft >'$SD_DIR/rb.log' 2>&1; then exit 1; fi; '$SD_DIR/baft-tunnel' engine verify --store '$SD_DIR/store' --id baft | grep -q '/$BAFT_SHA/engine'" || { echo "rollback to a tampered release was not refused" >&2; exit 1; }
    store_carries "$BAFT_SHA" || { echo "store release A did not carry data after rollback" >&2; exit 1; }
    echo "BAFT_STORE_ROLLBACK_RESULT engine_source=${BAFT_ENGINE_SOURCE:-unknown} release_a=$BAFT_SHA b_installed=1 a_pin_refused_while_b_current=1 b_carried_2mib=1 rollback_to_a=1 b_pin_refused_after_rollback=1 tampered_rollback_refused=1 a_carried_2mib_after_rollback=1"
    sd_cleanup; trap cleanup EXIT
    echo "BAFT_SYSTEMD_RESULT hardened_direct_ok=1 mdwe=yes no_seccomp_kill=1 recovery_s=$SD_REC_S dialer_unit_restarts=$SD_NRESTARTS engine_kill_recovered=1 engine_kill_recovery_s=$SD_KILL_S store_launch_ok=1 store_pin_mismatch_refused=1 store_rollback_ok=1 engine_source=${BAFT_ENGINE_SOURCE:-unknown}"
  fi
  echo "BAFT_RESULT direct_ok=1 unauthorized_refused=1 restart_recovered=1 restart_recovery_s=$BAFT_RECOVERY_S dialer_restarted=$BAFT_DIALER_RESTARTED bad_hash_refused=1 reverse_ok=1 revoked_refused=1 pin_ok=1 wrong_pin_refused=1 revoked_listener_refused=1 rss_kb_serve=$BAFT_RSS_SERVE rss_kb_forward=$BAFT_RSS_FWD $BAFT_SOAK_RESULT $BAFT_LOSS_RESULT"
fi

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
  NDIR=direct
  noise_pub() { ssh_host "$1" "'$REMOTE_DIR/baft-tunnel' noise-keygen --out '$REMOTE_DIR/$2.noise'" | tr -d '[:space:]'; }
  IRAN_NPUB="$(noise_pub iran iran)"; ABROAD_NPUB="$(noise_pub abroad abroad)"; ROGUE_NPUB="$(noise_pub iran rogue)"
  for v in "$IRAN_NPUB" "$ABROAD_NPUB" "$ROGUE_NPUB"; do [[ "$v" =~ ^[0-9a-f]{64}$ ]] || { echo "noise-keygen output invalid" >&2; exit 1; }; done
  noise_serve() { # side local_id peer_id peer_pub keyfile
    ssh_host "$1" "printf '{\"mode\":\"serve\",\"engine\":\"noise-tcp\",\"direction\":\"$NDIR\",\"listen\":\"0.0.0.0:$E2E_PORT\",\"target\":\"127.0.0.1:$ECHO_PORT\",\"local_id\":\"$2\",\"peer_id\":\"$3\",\"tunnel_id\":\"lab-e2e\",\"key\":\"$REMOTE_DIR/$5.noise\",\"noise_peer_public\":\"$4\",\"max_streams\":8}' > '$REMOTE_DIR/noise-serve.json'; nohup '$REMOTE_DIR/baft-tunnel' apply --config '$REMOTE_DIR/noise-serve.json' --confirm > '$REMOTE_DIR/serve.log' 2>&1 < /dev/null & echo \$! > '$REMOTE_DIR/serve.pid'"
  }
  noise_forward() { # side local_id peer_id peer_host peer_pub keyfile
    ssh_host "$1" "printf '{\"mode\":\"forward\",\"engine\":\"noise-tcp\",\"direction\":\"$NDIR\",\"listen\":\"127.0.0.1:$LOCAL_PORT\",\"remote\":\"$4:$E2E_PORT\",\"local_id\":\"$2\",\"peer_id\":\"$3\",\"tunnel_id\":\"lab-e2e\",\"key\":\"$REMOTE_DIR/$6.noise\",\"noise_peer_public\":\"$5\",\"max_streams\":8}' > '$REMOTE_DIR/noise-forward.json'; nohup '$REMOTE_DIR/baft-tunnel' apply --config '$REMOTE_DIR/noise-forward.json' --confirm > '$REMOTE_DIR/forward.log' 2>&1 < /dev/null & echo \$! > '$REMOTE_DIR/forward.pid'"
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
  stop_role iran forward
  stop_role abroad serve
  echo "NOISE_RESULT direct_ok=1 direct_rogue_refused=1 (Reverse is judged separately, ADR-0021)"
fi

if (( MVP == 1 )); then
  # shellcheck source=lab/real-hosts/mvp-stage.sh
  . "$(dirname "${BASH_SOURCE[0]}")/mvp-stage.sh"
fi

if (( REVERSE_ACCEPT > 0 )); then
  # shellcheck source=lab/real-hosts/reverse-accept-stage.sh
  . "$(dirname "${BASH_SOURCE[0]}")/reverse-accept-stage.sh"
fi

if (( OPS_SOAK > 0 )); then
  # shellcheck source=lab/real-hosts/ops-stage.sh
  . "$(dirname "${BASH_SOURCE[0]}")/ops-stage.sh"
fi

if (( AGENT_SVC == 1 )); then
  # shellcheck source=lab/real-hosts/agent-service-stage.sh
  . "$(dirname "${BASH_SOURCE[0]}")/agent-service-stage.sh"
fi

if (( PANEL_PAIRING == 1 )); then
  # shellcheck source=lab/real-hosts/panel-pairing-stage.sh
  . "$(dirname "${BASH_SOURCE[0]}")/panel-pairing-stage.sh"
fi

echo "PHASE1_TWO_HOST_E2E=PASS soak_s=$SOAK_SECONDS impair=$IMPAIR loss=$LOSS noise=$NOISE rollover=$ROLLOVER setup=$SETUP baft=$BAFT reverse=$REVERSE reverse_r3=$REVERSE_R3 mvp=$MVP agent_svc=$AGENT_SVC reboot=$AGENT_REBOOT panel_pairing=$PANEL_PAIRING (Direct, 2 MiB integrity, recovery, server/client certificate revocation; temporary /tmp processes only)"
