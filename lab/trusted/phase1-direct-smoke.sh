#!/usr/bin/env bash
# Trusted, bounded LAB harness. This file must be sourced from the protected main branch.
# Candidate code is only executed as the unprivileged 'nobody' account on dedicated LAB hosts.
set -Eeuo pipefail
umask 077

: "${LAB_IRAN_HOST:?Missing LAB_IRAN_HOST}"
: "${LAB_ABROAD_HOST:?Missing LAB_ABROAD_HOST}"
: "${LAB_SSH_PRIVATE_KEY:?Missing LAB_SSH_PRIVATE_KEY}"
: "${LAB_SSH_KNOWN_HOSTS:?Missing LAB_SSH_KNOWN_HOSTS}"
: "${CANDIDATE_SHA:?Missing CANDIDATE_SHA}"
: "${CANDIDATE_BINARY:?Missing CANDIDATE_BINARY}"
[[ "$CANDIDATE_SHA" =~ ^[0-9a-f]{40}$ ]]
[[ -f "$CANDIDATE_BINARY" && ! -L "$CANDIDATE_BINARY" && -s "$CANDIDATE_BINARY" ]]
(( $(wc -c < "$CANDIDATE_BINARY") <= 104857600 )) || { echo "candidate binary exceeds 100 MiB" >&2; exit 2; }

validate_ip() { python3 - "$1" <<'PY'
import ipaddress, sys
ipaddress.ip_address(sys.argv[1])
PY
}
validate_port() { [[ "$1" =~ ^[0-9]{1,5}$ ]] && (( 1 <= 10#$1 && 10#$1 <= 65535 )); }
validate_user() { [[ "$1" =~ ^[A-Za-z_][A-Za-z0-9_.-]*[$]?$ ]]; }
validate_ip "$LAB_IRAN_HOST"
validate_ip "$LAB_ABROAD_HOST"
[[ "$LAB_IRAN_HOST" != "$LAB_ABROAD_HOST" ]] || { echo 'LAB hosts must differ' >&2; exit 2; }
IRAN_USER="${LAB_IRAN_SSH_USER:-root}"
ABROAD_USER="${LAB_ABROAD_SSH_USER:-root}"
IRAN_PORT="${LAB_IRAN_SSH_PORT:-22}"
ABROAD_PORT="${LAB_ABROAD_SSH_PORT:-22}"
validate_user "$IRAN_USER" && validate_user "$ABROAD_USER"
validate_port "$IRAN_PORT" && validate_port "$ABROAD_PORT"

LOCAL_TMP="$(mktemp -d)"
SSH_KEY="$LOCAL_TMP/id_ed25519"
KNOWN_HOSTS="$LOCAL_TMP/known_hosts"
RUN_ID="${GITHUB_RUN_ID:-$$}"
[[ "$RUN_ID" =~ ^[0-9]+$ ]] || { echo "invalid workflow run ID" >&2; exit 2; }
REMOTE_DIR="/tmp/baft-trusted-lab-$RUN_ID"
E2E_PORT=18300
ECHO_PORT=19090
LOCAL_PORT=18080
install -m 600 /dev/null "$SSH_KEY"
printf '%s\n' "$LAB_SSH_PRIVATE_KEY" > "$SSH_KEY"
printf '%s\n' "$LAB_SSH_KNOWN_HOSTS" > "$KNOWN_HOSTS"
chmod 600 "$KNOWN_HOSTS"
# Do not pass raw credentials to ssh/scp child processes; they use only the protected key file.
unset LAB_SSH_PRIVATE_KEY LAB_SSH_KNOWN_HOSTS

host_for() { [[ "$1" == iran ]] && printf '%s' "$LAB_IRAN_HOST" || printf '%s' "$LAB_ABROAD_HOST"; }
user_for() { [[ "$1" == iran ]] && printf '%s' "$IRAN_USER" || printf '%s' "$ABROAD_USER"; }
port_for() { [[ "$1" == iran ]] && printf '%s' "$IRAN_PORT" || printf '%s' "$ABROAD_PORT"; }
ssh_host() {
  local side="$1" command="$2"
  local host; host="$(host_for "$side")"; [[ "$host" == *:* ]] && host="[$host]"
  ssh -i "$SSH_KEY" -p "$(port_for "$side")" -o BatchMode=yes -o IdentitiesOnly=yes \
    -o StrictHostKeyChecking=yes -o UserKnownHostsFile="$KNOWN_HOSTS" \
    -o ConnectTimeout=12 -o ServerAliveInterval=10 -o ServerAliveCountMax=2 \
    -o LogLevel=ERROR "$(user_for "$side")@$host" "$command"
}
copy_to_host() {
  local side="$1"; shift
  local host; host="$(host_for "$side")"; [[ "$host" == *:* ]] && host="[$host]"
  scp -i "$SSH_KEY" -P "$(port_for "$side")" -o BatchMode=yes -o IdentitiesOnly=yes \
    -o StrictHostKeyChecking=yes -o UserKnownHostsFile="$KNOWN_HOSTS" \
    -o ConnectTimeout=12 -o LogLevel=ERROR "$@" "$(user_for "$side")@$host:$REMOTE_DIR/"
}
cleanup() {
  local rc=$?
  trap - EXIT
  for side in iran abroad; do
    ssh_host "$side" "if [ -d '$REMOTE_DIR' ]; then for f in serve.pid forward.pid echo.pid; do if [ -s '$REMOTE_DIR/'\$f ]; then kill \$(cat '$REMOTE_DIR/'\$f) 2>/dev/null || true; fi; done; sleep 0.2; rm -rf '$REMOTE_DIR'; fi" || rc=1
  done
  rm -rf "$LOCAL_TMP"
  exit "$rc"
}
trap cleanup EXIT

for side in iran abroad; do
  # The dedicated test hosts must be reachable as root for staging, but candidate processes
  # are always demoted to nobody and receive no SSH credentials.
  ssh_host "$side" 'test "$(id -u)" = 0 && id nobody >/dev/null && command -v runuser >/dev/null && command -v timeout >/dev/null && command -v ss >/dev/null && getent group nogroup >/dev/null && command -v python3 >/dev/null && command -v openssl >/dev/null'
done

mkdir -p "$LOCAL_TMP/newcerts"
: > "$LOCAL_TMP/index.txt"
printf '1000\n' > "$LOCAL_TMP/serial"
printf '1000\n' > "$LOCAL_TMP/crlnumber"
openssl req -x509 -newkey rsa:3072 -nodes -keyout "$LOCAL_TMP/ca.key" -out "$LOCAL_TMP/ca.crt" \
  -days 2 -sha256 -subj '/CN=BAFT-TUNNEL trusted ephemeral LAB CA' \
  -addext 'basicConstraints=critical,CA:TRUE' -addext 'keyUsage=critical,keyCertSign,cRLSign' >/dev/null 2>&1
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
for node in iran abroad; do
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
ABROAD_DIAL_HOST="$LAB_ABROAD_HOST"
[[ "$ABROAD_DIAL_HOST" == *:* ]] && ABROAD_DIAL_HOST="[$ABROAD_DIAL_HOST]"

cat > "$LOCAL_TMP/echo.py" <<'PY'
import socket, threading
s=socket.socket(); s.setsockopt(socket.SOL_SOCKET,socket.SO_REUSEADDR,1); s.bind(('127.0.0.1',19090)); s.listen(32)
def handle(c):
    with c:
        while True:
            d=c.recv(65536)
            if not d: return
            c.sendall(d)
while True:
    c,_=s.accept(); threading.Thread(target=handle,args=(c,),daemon=True).start()
PY
cat > "$LOCAL_TMP/probe.py" <<'PY'
import hashlib,socket,sys,time
size=int(sys.argv[1]); payload=bytes(i%251 for i in range(size)); end=time.monotonic()+45; last=None
while True:
    try: s=socket.create_connection(('127.0.0.1',18080),timeout=2); break
    except OSError as e:
        last=e
        if time.monotonic()>end: raise SystemExit(f'forward not ready: {last}')
        time.sleep(.5)
with s:
    s.settimeout(30); s.sendall(payload); s.shutdown(socket.SHUT_WR); got=bytearray()
    while len(got)<len(payload):
        b=s.recv(min(65536,len(payload)-len(got)))
        if not b: break
        got.extend(b)
if got!=payload: raise SystemExit(f'payload mismatch {len(got)}/{len(payload)}')
print(f'E2E_PAYLOAD_OK bytes={len(got)} sha256={hashlib.sha256(got).hexdigest()}')
PY

for side in iran abroad; do
  ssh_host "$side" "mkdir -p '$REMOTE_DIR' && chmod 755 '$REMOTE_DIR' && test -z \"\$(ss -lnt 2>/dev/null | grep -E ':(18300|19090|18080)\\b' || true)\""
  copy_to_host "$side" "$CANDIDATE_BINARY" "$LOCAL_TMP/ca.crt" "$LOCAL_TMP/ca.crl" \
    "$LOCAL_TMP/iran.crt" "$LOCAL_TMP/iran.key" "$LOCAL_TMP/abroad.crt" "$LOCAL_TMP/abroad.key" \
    "$LOCAL_TMP/echo.py" "$LOCAL_TMP/probe.py"
  ssh_host "$side" "chown -R nobody:nogroup '$REMOTE_DIR' && chmod 755 '$REMOTE_DIR/baft-tunnel' && chmod 600 '$REMOTE_DIR/'*.key"
done

ssh_host abroad "nohup runuser -u nobody -- sh -c 'echo \$\$ > \"$REMOTE_DIR/echo.pid\"; exec timeout --signal=TERM --kill-after=5s 180s python3 \"$REMOTE_DIR/echo.py\"' > '$REMOTE_DIR/echo.log' 2>&1 < /dev/null &"
sleep 1
ssh_host abroad "nohup runuser -u nobody -- sh -c 'echo \$\$ > \"$REMOTE_DIR/serve.pid\"; exec timeout --signal=TERM --kill-after=5s 180s \"$REMOTE_DIR/baft-tunnel\" serve --listen \"0.0.0.0:$E2E_PORT\" --target \"127.0.0.1:$ECHO_PORT\" --local-id abroad --peer-id iran --tunnel-id trusted-lab --direction direct --peer-fingerprint \"$IRAN_PIN\" --ca \"$REMOTE_DIR/ca.crt\" --crl \"$REMOTE_DIR/ca.crl\" --cert \"$REMOTE_DIR/abroad.crt\" --key \"$REMOTE_DIR/abroad.key\" --max-streams 8' > '$REMOTE_DIR/serve.log' 2>&1 < /dev/null &"
ssh_host iran "nohup runuser -u nobody -- sh -c 'echo \$\$ > \"$REMOTE_DIR/forward.pid\"; exec timeout --signal=TERM --kill-after=5s 180s \"$REMOTE_DIR/baft-tunnel\" forward --listen \"127.0.0.1:$LOCAL_PORT\" --remote \"$ABROAD_DIAL_HOST:$E2E_PORT\" --server-name abroad.lab --local-id iran --peer-id abroad --tunnel-id trusted-lab --direction direct --peer-fingerprint \"$ABROAD_PIN\" --ca \"$REMOTE_DIR/ca.crt\" --crl \"$REMOTE_DIR/ca.crl\" --cert \"$REMOTE_DIR/iran.crt\" --key \"$REMOTE_DIR/iran.key\" --max-streams 8' > '$REMOTE_DIR/forward.log' 2>&1 < /dev/null &"
sleep 2
ssh_host iran "python3 '$REMOTE_DIR/probe.py' 65536"
ssh_host iran "python3 '$REMOTE_DIR/probe.py' 2097289"
echo "TRUSTED_PHASE1_LAB=PASS candidate_sha=$CANDIDATE_SHA direct=1 payload_64k=1 payload_2mib=1 candidate_process_user=nobody ssh_key_exposed_to_candidate=0"
