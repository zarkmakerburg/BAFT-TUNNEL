#!/usr/bin/env bash
# Trusted main-branch harness for the ADR-0049 stream-resume (protocol v3) two-host chaos LAB: a relay on the Iran host is cut twice while a payload is echoed through the tunnel.
# Only this script receives SSH credentials. The PR binary is executed remotely as
# baft-tunnel / baft-tunnel-agent under transient, bounded systemd services.
set -Eeuo pipefail
umask 077

: "${LAB_IRAN_HOST:?Missing LAB_IRAN_HOST}"
: "${LAB_ABROAD_HOST:?Missing LAB_ABROAD_HOST}"
: "${LAB_SSH_PRIVATE_KEY:?Missing LAB_SSH_PRIVATE_KEY}"
: "${LAB_SSH_KNOWN_HOSTS:?Missing LAB_SSH_KNOWN_HOSTS}"
: "${CANDIDATE_SHA:?Missing CANDIDATE_SHA}"
: "${CANDIDATE_BINARY:?Missing CANDIDATE_BINARY}"

[[ "$CANDIDATE_SHA" =~ ^[0-9a-f]{40}$ ]] || { echo 'candidate SHA must be 40 lowercase hex characters' >&2; exit 2; }
[[ -f "$CANDIDATE_BINARY" && ! -L "$CANDIDATE_BINARY" && -s "$CANDIDATE_BINARY" ]] || {
  echo 'candidate binary is missing, empty or a symlink' >&2; exit 2;
}
(( $(wc -c < "$CANDIDATE_BINARY") <= 104857600 )) || { echo 'candidate binary exceeds 100 MiB' >&2; exit 2; }
CANDIDATE_BINARY_SHA256="$(sha256sum "$CANDIDATE_BINARY" | awk '{print $1}')"
[[ "$CANDIDATE_BINARY_SHA256" =~ ^[0-9a-f]{64}$ ]] || { echo 'could not calculate candidate binary SHA-256' >&2; exit 2; }
echo "RES_CANDIDATE_BINARY_SHA256=$CANDIDATE_BINARY_SHA256"

validate_ip() { python3 - "$1" <<'PYIP'
import ipaddress, sys
ipaddress.ip_address(sys.argv[1])
PYIP
}
validate_user() { [[ "$1" =~ ^[A-Za-z_][A-Za-z0-9_.-]*[$]?$ ]]; }
validate_port() { [[ "$1" =~ ^[0-9]{1,5}$ ]] && (( 10#$1 >= 1 && 10#$1 <= 65535 )); }

validate_ip "$LAB_IRAN_HOST"
validate_ip "$LAB_ABROAD_HOST"
[[ "$LAB_IRAN_HOST" != "$LAB_ABROAD_HOST" ]] || { echo 'LAB hosts must differ' >&2; exit 2; }
ip_cidr() {
  python3 - "$1" <<'PYCIDR'
import ipaddress, sys
address = ipaddress.ip_address(sys.argv[1])
print(f"{address}/{address.max_prefixlen}")
PYCIDR
}
IRAN_CIDR="$(ip_cidr "$LAB_IRAN_HOST")"
ABROAD_CIDR="$(ip_cidr "$LAB_ABROAD_HOST")"

IRAN_USER="${LAB_IRAN_SSH_USER:-root}"
ABROAD_USER="${LAB_ABROAD_SSH_USER:-root}"
IRAN_PORT="${LAB_IRAN_SSH_PORT:-22}"
ABROAD_PORT="${LAB_ABROAD_SSH_PORT:-22}"
validate_user "$IRAN_USER" && validate_user "$ABROAD_USER" || { echo 'invalid LAB SSH user' >&2; exit 2; }
validate_port "$IRAN_PORT" && validate_port "$ABROAD_PORT" || { echo 'invalid LAB SSH port' >&2; exit 2; }

RUN_ID="${GITHUB_RUN_ID:-$$}"
[[ "$RUN_ID" =~ ^[0-9]+$ ]] || { echo 'invalid workflow run ID' >&2; exit 2; }
INSTANCE="reslab-$RUN_ID"
REMOTE_BASE="/opt/baft-res-lab-$RUN_ID"
REMOTE_BIN="$REMOTE_BASE/baft-tunnel"
ECHO_STATE="$REMOTE_BASE/echo-state"
LOCAL_TMP="$(mktemp -d)"
SSH_KEY="$LOCAL_TMP/id_ed25519"
KNOWN_HOSTS="$LOCAL_TMP/known_hosts"
TUNNEL_PORT=18300
LOCAL_PORT=18080
ECHO_PORT=18080
IRAN_API_PORT=18180
ABROAD_API_PORT=18180
ECHO_UNIT="baft-res-echo-$RUN_ID"
RELAY_UNIT="baft-res-relay-$RUN_ID"
RELAY_PORT=18400
PREPARED_IRAN=0
PREPARED_ABROAD=0

install -m 600 /dev/null "$SSH_KEY"
printf '%s\n' "$LAB_SSH_PRIVATE_KEY" > "$SSH_KEY"
printf '%s\n' "$LAB_SSH_KNOWN_HOSTS" > "$KNOWN_HOSTS"
chmod 600 "$KNOWN_HOSTS"
# Never export raw credentials to ssh/scp or to candidate processes.
unset LAB_SSH_PRIVATE_KEY LAB_SSH_KNOWN_HOSTS

host_for() { [[ "$1" == iran ]] && printf '%s' "$LAB_IRAN_HOST" || printf '%s' "$LAB_ABROAD_HOST"; }
user_for() { [[ "$1" == iran ]] && printf '%s' "$IRAN_USER" || printf '%s' "$ABROAD_USER"; }
port_for() { [[ "$1" == iran ]] && printf '%s' "$IRAN_PORT" || printf '%s' "$ABROAD_PORT"; }
api_port_for() { [[ "$1" == iran ]] && printf '%s' "$IRAN_API_PORT" || printf '%s' "$ABROAD_API_PORT"; }
side_dir() { printf '%s/%s' "$REMOTE_BASE" "$1"; }
agent_state_dir() { printf '%s/agent-state-%s' "$REMOTE_BASE" "$1"; }
tunnel_unit_for() { printf 'baft-tunnel@%s.service' "$INSTANCE"; }
agent_unit_for() { printf 'baft-tunnel-agent@%s.service' "$INSTANCE"; }
ABROAD_TUNNEL_UNIT="$(tunnel_unit_for abroad)"
IRAN_TUNNEL_UNIT="$(tunnel_unit_for iran)"

ssh_host() {
  local side="$1" command="$2" host
  host="$(host_for "$side")"
  [[ "$host" == *:* ]] && host="[$host]"
  ssh -i "$SSH_KEY" -p "$(port_for "$side")" -o BatchMode=yes -o IdentitiesOnly=yes \
    -o StrictHostKeyChecking=yes -o UserKnownHostsFile="$KNOWN_HOSTS" \
    -o GlobalKnownHostsFile=/dev/null -o ConnectTimeout=12 -o ServerAliveInterval=10 \
    -o ServerAliveCountMax=2 -o LogLevel=ERROR "$(user_for "$side")@$host" "$command"
}
copy_to_host() {
  local side="$1"; shift
  local host; host="$(host_for "$side")"; [[ "$host" == *:* ]] && host="[$host]"
  scp -i "$SSH_KEY" -P "$(port_for "$side")" -o BatchMode=yes -o IdentitiesOnly=yes \
    -o StrictHostKeyChecking=yes -o UserKnownHostsFile="$KNOWN_HOSTS" \
    -o GlobalKnownHostsFile=/dev/null -o ConnectTimeout=12 -o LogLevel=ERROR \
    "$@" "$(user_for "$side")@$host:$REMOTE_BASE/"
}
copy_from_host() {
  local side="$1" remote_file="$2" local_file="$3" host
  host="$(host_for "$side")"; [[ "$host" == *:* ]] && host="[$host]"
  scp -i "$SSH_KEY" -P "$(port_for "$side")" -o BatchMode=yes -o IdentitiesOnly=yes \
    -o StrictHostKeyChecking=yes -o UserKnownHostsFile="$KNOWN_HOSTS" \
    -o GlobalKnownHostsFile=/dev/null -o ConnectTimeout=12 -o LogLevel=ERROR \
    "$(user_for "$side")@$host:$remote_file" "$local_file"
}
diagnose() {
  local side="$1"
  echo "RES_LAB_DIAGNOSTICS host=$side"
  ssh_host "$side" "journalctl --no-pager -n 100 -u '$(tunnel_unit_for "$side")' -u '$(agent_unit_for "$side")' -u '$ECHO_UNIT' 2>/dev/null || true; ss -lntp || true" || true
}
cleanup() {
  local rc=$? prepared units extra_active
  trap - EXIT
  for side in iran abroad; do
    prepared=0
    [[ "$side" == iran && "$PREPARED_IRAN" == 1 ]] && prepared=1
    [[ "$side" == abroad && "$PREPARED_ABROAD" == 1 ]] && prepared=1
    if (( prepared )); then
      if (( rc != 0 )); then diagnose "$side"; fi
      units="'$(agent_unit_for "$side")' '$(tunnel_unit_for "$side")'"
      [[ "$side" == abroad ]] && units="$units '$ECHO_UNIT'"
      [[ "$side" == iran ]] && units="$units '$RELAY_UNIT'"
      extra_active=""
      if [[ "$side" == abroad ]]; then
        extra_active=" || systemctl is-active --quiet '$ECHO_UNIT'"
      else
        extra_active=" || systemctl is-active --quiet '$RELAY_UNIT'"
      fi
      if ! ssh_host "$side" "
        systemctl stop $units 2>/dev/null || true
        sleep 1
        if systemctl is-active --quiet '$(agent_unit_for "$side")' || systemctl is-active --quiet '$(tunnel_unit_for "$side")' $extra_active; then
          echo 'LAB units remain active; preserving their runtime files' >&2
          exit 1
        fi
        test ! -e '/run/baft-tunnel/$INSTANCE' || { echo 'systemd runtime directory remains; preserving files' >&2; exit 1; }
        systemctl reset-failed $units 2>/dev/null || true
        rm -rf '$REMOTE_BASE'
      "; then
        echo "LAB cleanup incomplete on $side; temporary files were preserved" >&2
        rc=1
      fi
    fi
  done
  rm -rf "$LOCAL_TMP"
  exit "$rc"
}
trap cleanup EXIT

# Fail closed if hosts are not already provisioned with the two distinct service identities.
# This workflow never creates, edits or removes host accounts, firewall rules, packages or
# persistent systemd files.
for side in iran abroad; do
  api_port="$(api_port_for "$side")"
  ssh_host "$side" "set -eu
    test \$(id -u) -eq 0
    command -v systemd-run >/dev/null
    command -v systemctl >/dev/null
    command -v runuser >/dev/null
    command -v curl >/dev/null
    command -v ss >/dev/null
    test -x /usr/bin/python3
    test "$(uname -m)" = x86_64
    command -v timeout >/dev/null
    command -v findmnt >/dev/null
    test -d /opt && test ! -L /opt
    test \$(stat -c '%U:%G' /opt) = root:root
    opt_mode=\$(stat -c '%a' /opt)
    python3 -c 'import sys; assert int(sys.argv[1], 8) & 0o022 == 0' \$opt_mode
    opt_options=\$(findmnt -n -o OPTIONS --target /opt)
    case \",\$opt_options,\" in *,noexec,*) echo \"/opt is mounted noexec; refusing candidate execution\" >&2; exit 2 ;; esac
    id baft-tunnel >/dev/null
    id baft-tunnel-agent >/dev/null
    id nobody >/dev/null
    test \$(id -u baft-tunnel) != \$(id -u baft-tunnel-agent)
    test \$(id -u nobody) != 0
    test \$(id -gn baft-tunnel) = baft-tunnel
    test \$(id -gn baft-tunnel-agent) = baft-tunnel-agent
    getent group baft-tunnel >/dev/null
    getent group nogroup >/dev/null
    id -nG baft-tunnel-agent | grep -Eq '(^| )baft-tunnel( |$)'
    test ! -e '$REMOTE_BASE' && test ! -L '$REMOTE_BASE'
    test ! -e '/run/baft-tunnel/$INSTANCE' && test ! -L '/run/baft-tunnel/$INSTANCE'
    ! systemctl is-active --quiet '$(agent_unit_for "$side")'
    ! systemctl is-active --quiet '$(tunnel_unit_for "$side")'
    $([[ "$side" == abroad ]] && printf "! systemctl is-active --quiet '%s'" "$ECHO_UNIT" || printf "! systemctl is-active --quiet '%s'" "$RELAY_UNIT")
    if systemctl list-units --type=service --state=running --no-legend 'baft-tunnel@*.service' | grep -q .; then
      echo 'existing BAFT-TUNNEL data-plane unit found; refusing shared-host candidate execution' >&2
      exit 2
    fi
    if systemctl list-units --type=service --state=running --no-legend 'baft-tunnel-agent@*.service' | grep -q .; then
      echo 'existing BAFT-TUNNEL agent unit found; refusing shared-host candidate execution' >&2
      exit 2
    fi
    if systemctl list-units --type=service --state=running --no-legend 'baft-tunnel-control-*.service' | grep -q .; then
      echo 'existing BAFT-TUNNEL privileged control helper found; refusing shared-host candidate execution' >&2
      exit 2
    fi
    if [ -e '/run/baft-tunnel' ] || [ -L '/run/baft-tunnel' ]; then
      test -d '/run/baft-tunnel' && test ! -L '/run/baft-tunnel'
      test \$(stat -c '%U:%G' '/run/baft-tunnel') = root:root
      parent_mode=\$(stat -c '%a' '/run/baft-tunnel')
      python3 -c 'import sys; assert int(sys.argv[1], 8) & 0o022 == 0' "\$parent_mode"
      for entry in /run/baft-tunnel/*; do
        [ -e "\$entry" ] || [ -L "\$entry" ] || continue
        if [ "\$entry" = '/run/baft-tunnel/control' ]; then
          test -d "\$entry" && test ! -L "\$entry"
          test \$(stat -c '%U:%G:%a' "\$entry") = root:baft-tunnel:770
          continue
        fi
        echo 'existing BAFT-TUNNEL runtime path found; refusing shared-host candidate execution' >&2
        exit 2
      done
    fi
  "
  for port in "$api_port" "$TUNNEL_PORT" "$LOCAL_PORT" "$ECHO_PORT" "$RELAY_PORT"; do
    busy="$(ssh_host "$side" "ss -lntH 'sport = :$port'")"
    [[ -z "$busy" ]] || { echo "LAB port already in use: host=$side port=$port" >&2; exit 2; }
  done
done

# Build tiny trusted echo and payload fixtures; neither reads nor receives SSH credentials.
cat > "$LOCAL_TMP/echo.py" <<'PY'
import json, os, socket, sys, threading
stats, port = sys.argv[1], int(sys.argv[2])
lock = threading.Lock()
connections = total = 0
def handle(client):
    global connections, total
    count = 0
    with client:
        while True:
            data = client.recv(65536)
            if not data:
                break
            client.sendall(data)
            count += len(data)
    with lock:
        connections += 1
        total += count
        temporary = stats + ".tmp"
        with open(temporary, "w", encoding="utf-8") as out:
            json.dump({"connections": connections, "bytes": total}, out)
        os.replace(temporary, stats)
server = socket.socket()
server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
server.bind(("127.0.0.1", port))
server.listen(32)
while True:
    client, _ = server.accept()
    threading.Thread(target=handle, args=(client,), daemon=True).start()
PY
cat > "$LOCAL_TMP/payload.py" <<'PY'
import hashlib, socket, sys, threading, time
size, port, rate = int(sys.argv[1]), int(sys.argv[2]), int(sys.argv[3])  # rate in bytes/second, 0 = unpaced
payload = bytes((i * 7 + i // 251) % 256 for i in range(size))
end = time.monotonic() + 40
while True:
    try:
        stream = socket.create_connection(("127.0.0.1", port), timeout=2)
        break
    except OSError:
        if time.monotonic() >= end:
            raise
        time.sleep(.5)
stream.settimeout(120)
received = bytearray()
def reader():
    try:
        while len(received) < size:
            part = stream.recv(65536)
            if not part:
                break
            received.extend(part)
    except OSError:
        pass
thread = threading.Thread(target=reader, daemon=True)
thread.start()
start = time.monotonic()
sent = 0
chunk = 16384
try:
    while sent < size:
        stream.sendall(payload[sent:sent + chunk])
        sent += min(chunk, size - sent)
        if rate:
            ahead = sent / rate - (time.monotonic() - start)
            if ahead > 0:
                time.sleep(ahead)
except OSError as error:
    print(f"SEND_FAILED sent={sent} error={error}")
thread.join(timeout=120)
elapsed = time.monotonic() - start
if bytes(received) != payload:
    print(f"PAYLOAD_MISMATCH received={len(received)} expected={size} secs={elapsed:.1f}")
    raise SystemExit(1)
print(f"DATA_PATH_OK bytes={len(received)} sha256={hashlib.sha256(received).hexdigest()} secs={elapsed:.1f}")
PY
# Loopback relay between the Iran forwarder and the abroad server. SIGUSR1 closes every relayed
# connection (a clean TCP cut of the path); the relay keeps listening so the forwarder can redial.
cat > "$LOCAL_TMP/relay.py" <<'PY'
import signal, socket, sys, threading
listen_port, dest_host, dest_port = int(sys.argv[1]), sys.argv[2], int(sys.argv[3])
lock = threading.Lock()
active = set()
cuts = 0
def pump(a, b):
    try:
        while True:
            data = a.recv(65536)
            if not data:
                break
            b.sendall(data)
    except OSError:
        pass
    finally:
        for s in (a, b):
            try:
                s.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass
def handle(inbound):
    try:
        outbound = socket.create_connection((dest_host, dest_port), timeout=10)
    except OSError:
        inbound.close()
        return
    with lock:
        active.add(inbound)
        active.add(outbound)
    t = threading.Thread(target=pump, args=(outbound, inbound), daemon=True)
    t.start()
    pump(inbound, outbound)
    t.join()
    with lock:
        active.discard(inbound)
        active.discard(outbound)
    inbound.close()
    outbound.close()
def cut(_signum, _frame):
    global cuts
    with lock:
        n = len(active) // 2
        for s in list(active):
            try:
                s.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass
    cuts += 1
    print(f"RELAY_CUT number={cuts} connections={n}", flush=True)
signal.signal(signal.SIGUSR1, cut)
server = socket.socket()
server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
server.bind(("127.0.0.1", listen_port))
server.listen(16)
print("RELAY_READY", flush=True)
while True:
    client, _ = server.accept()
    threading.Thread(target=handle, args=(client,), daemon=True).start()
PY

# Create a unique ephemeral /opt workspace and stage the candidate executable (root-owned, read-only to
# service users). No candidate script, installer or other PR-controlled file is executed.
for side in iran abroad; do
  [[ "$side" == iran ]] && PREPARED_IRAN=1 || PREPARED_ABROAD=1
  ssh_host "$side" "mkdir -m 0755 '$REMOTE_BASE' && test -d '$REMOTE_BASE' && test ! -L '$REMOTE_BASE' && test \$(stat -c '%U:%G:%a' '$REMOTE_BASE') = root:root:755"
  if [[ ! -e "$LOCAL_TMP/baft-tunnel" ]]; then install -m 0755 "$CANDIDATE_BINARY" "$LOCAL_TMP/baft-tunnel"; fi
  copy_to_host "$side" "$LOCAL_TMP/baft-tunnel" "$LOCAL_TMP/echo.py" "$LOCAL_TMP/payload.py" "$LOCAL_TMP/relay.py"
  remote_binary_sha256="$(ssh_host "$side" "sha256sum '$REMOTE_BASE/baft-tunnel' | cut -d ' ' -f1")"
  [[ "$remote_binary_sha256" == "$CANDIDATE_BINARY_SHA256" ]] || {
    echo "remote candidate binary digest mismatch: host=$side expected=$CANDIDATE_BINARY_SHA256 actual=$remote_binary_sha256" >&2
    exit 1
  }
  echo "RES_REMOTE_BINARY_VERIFIED host=$side sha256=$remote_binary_sha256"
  ssh_host "$side" "set -eu
    chown root:root '$REMOTE_BASE/baft-tunnel'
    chmod 0755 '$REMOTE_BASE/baft-tunnel'
    chown root:root '$REMOTE_BASE/echo.py' '$REMOTE_BASE/payload.py' '$REMOTE_BASE/relay.py'
    chmod 0644 '$REMOTE_BASE/echo.py' '$REMOTE_BASE/payload.py' '$REMOTE_BASE/relay.py'
    install -d -o baft-tunnel -g baft-tunnel -m 0750 '$(side_dir "$side")'
    install -d -o baft-tunnel-agent -g baft-tunnel-agent -m 0700 '$(agent_state_dir "$side")'
    test \$(stat -c '%U:%G:%a' '$REMOTE_BASE/baft-tunnel') = root:root:755
    test \$(stat -c '%U:%G:%a' '$(side_dir "$side")') = baft-tunnel:baft-tunnel:750
  "
done
ssh_host abroad "install -d -o nobody -g nogroup -m 0700 '$ECHO_STATE' && install -m 0600 -o nobody -g nogroup /dev/null '$ECHO_STATE/stats.json'"

# tcp-tls Direct with a throwaway PKI generated on the runner (ephemeral 2-day CA; the CA key never
# leaves the runner and is never copied to a LAB host). Each host receives only its own key,
# certificate, the CA certificate and the CRL.
PKI="$LOCAL_TMP/pki"; mkdir -p "$PKI/newcerts"
( cd "$PKI"; : > index.txt; echo 1000 > serial; echo 1000 > crlnumber
  openssl req -x509 -newkey rsa:3072 -nodes -keyout ca.key -out ca.crt -days 2 -sha256 -subj '/CN=BAFT resume lab ephemeral CA' \
    -addext 'basicConstraints=critical,CA:TRUE' -addext 'keyUsage=critical,keyCertSign,cRLSign' >/dev/null 2>&1
  cat > openssl.cnf <<CNF
[ ca ]
default_ca = CA_default
[ CA_default ]
database = $PKI/index.txt
new_certs_dir = $PKI/newcerts
certificate = $PKI/ca.crt
private_key = $PKI/ca.key
serial = $PKI/serial
crlnumber = $PKI/crlnumber
default_md = sha256
default_days = 2
default_crl_days = 2
policy = policy_any
copy_extensions = copy
[ policy_any ]
commonName = supplied
CNF
  for node in entry exit; do
    openssl req -new -newkey rsa:2048 -nodes -keyout $node.key -out $node.csr -subj "/CN=$node.reslab" >/dev/null 2>&1
    printf 'basicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature\nextendedKeyUsage=serverAuth,clientAuth\nsubjectAltName=DNS:%s.reslab,URI:urn:baft-tunnel:node:%s\n' "$node" "$node" > $node.ext
    printf '[ %s_ext ]\n' "$node" >> openssl.cnf; cat $node.ext >> openssl.cnf
    openssl ca -batch -notext -config openssl.cnf -extensions ${node}_ext -in $node.csr -out $node.crt -days 2 >/dev/null 2>&1
  done
  openssl ca -gencrl -batch -config openssl.cnf -out ca.crl >/dev/null 2>&1 )
pin() { openssl x509 -in "$1" -outform DER | sha256sum | awk '{print $1}'; }
ENTRY_PIN="$(pin "$PKI/entry.crt")"; EXIT_PIN="$(pin "$PKI/exit.crt")"
[[ "$ENTRY_PIN" =~ ^[0-9a-f]{64}$ && "$EXIT_PIN" =~ ^[0-9a-f]{64}$ ]] || { echo 'could not compute certificate pins' >&2; exit 1; }

# Iran runs the forward node ("entry"), abroad runs the serve node ("exit").
python3 - "$LOCAL_TMP" "$INSTANCE" "$LOCAL_PORT" "$TUNNEL_PORT" "$ECHO_PORT" "$RELAY_PORT" "$LAB_ABROAD_HOST" "$ENTRY_PIN" "$EXIT_PIN" "$(side_dir iran)" "$(side_dir abroad)" <<'PYCFG'
import json, sys
tmp, inst, local_port, tunnel_port, echo_port, relay_port, abroad, entry_pin, exit_pin, idir, adir = sys.argv[1:]
common = {"tunnel_id": inst, "engine": "tcp-tls", "direction": "direct", "protocol_version": 3, "ca": "ca.crt", "crl": "ca.crl", "max_streams": 8}
fwd = dict(common, mode="forward", listen=f"127.0.0.1:{local_port}", remote=f"127.0.0.1:{relay_port}",
           server_name="exit.reslab", local_id="entry", peer_id="exit", peer_fingerprint=exit_pin,
           cert=f"{idir}/entry.crt", key=f"{idir}/entry.key")
srv = dict(common, mode="serve", listen=f"0.0.0.0:{tunnel_port}", target=f"127.0.0.1:{echo_port}",
           local_id="exit", peer_id="entry", peer_fingerprint=entry_pin,
           cert=f"{adir}/exit.crt", key=f"{adir}/exit.key")
fwd["ca"] = f"{idir}/ca.crt"; fwd["crl"] = f"{idir}/ca.crl"
srv["ca"] = f"{adir}/ca.crt"; srv["crl"] = f"{adir}/ca.crl"
json.dump(fwd, open(f"{tmp}/forward.json", "w")); json.dump(srv, open(f"{tmp}/serve.json", "w"))
PYCFG
for side in iran abroad; do
  if [[ "$side" == iran ]]; then node=entry; cfg=forward.json; else node=exit; cfg=serve.json; fi
  copy_to_host "$side" "$PKI/$node.crt" "$PKI/$node.key" "$PKI/ca.crt" "$PKI/ca.crl" "$LOCAL_TMP/$cfg"
  ssh_host "$side" "set -eu
    d='$(side_dir "$side")'
    install -m 0644 -o root -g baft-tunnel '$REMOTE_BASE/$node.crt' \"\$d/$node.crt\"
    install -m 0640 -o root -g baft-tunnel '$REMOTE_BASE/$node.key' \"\$d/$node.key\"
    install -m 0644 -o root -g baft-tunnel '$REMOTE_BASE/ca.crt' \"\$d/ca.crt\"
    install -m 0644 -o root -g baft-tunnel '$REMOTE_BASE/ca.crl' \"\$d/ca.crl\"
    install -m 0640 -o root -g baft-tunnel '$REMOTE_BASE/$cfg' \"\$d/$INSTANCE.json\"
    rm -f '$REMOTE_BASE/$node.key' '$REMOTE_BASE/$cfg'
    chown root:baft-tunnel \"\$d\" && chmod 0750 \"\$d\"
  "
done

# The echo destination is a nobody-owned fixture. The data plane and agent run as distinct,
# pre-existing service accounts; systemd creates the exact 0750 per-instance runtime directory.
echo "RES_LAB_START_ECHO_FIXTURE host=abroad unit=$ECHO_UNIT state=$ECHO_STATE"
ssh_host abroad "systemd-run --quiet --collect --unit='$ECHO_UNIT' --property=User=nobody --property=Group=nogroup --property=UMask=0077 --property=NoNewPrivileges=yes --property=PrivateTmp=yes --property=PrivateDevices=yes --property=ProtectSystem=strict --property=ProtectHome=yes --property=ProtectKernelTunables=yes --property=ProtectKernelModules=yes --property=ProtectKernelLogs=yes --property=ProtectControlGroups=yes --property=ProtectClock=yes --property=ProtectHostname=yes --property=RestrictSUIDSGID=yes --property=RestrictNamespaces=yes --property=RestrictRealtime=yes --property=LockPersonality=yes --property=MemoryDenyWriteExecute=yes --property='SystemCallFilter=@system-service' --property=SystemCallArchitectures=native --property=CapabilityBoundingSet= --property=AmbientCapabilities= --property=RuntimeMaxSec=2400 --property=TimeoutStopSec=3 --property=MemoryMax=96M --property=TasksMax=32 --property=LimitNOFILE=128 --property=IPAddressDeny=any --property='IPAddressAllow=127.0.0.0/8 ::1/128' --property='RestrictAddressFamilies=AF_INET' --property='ReadWritePaths=$ECHO_STATE' -- /usr/bin/python3 '$REMOTE_BASE/echo.py' '$ECHO_STATE/stats.json' '$ECHO_PORT'"
echo "RES_LAB_START_TUNNEL host=abroad unit=$ABROAD_TUNNEL_UNIT"
ssh_host abroad "systemd-run --quiet --collect --unit='$ABROAD_TUNNEL_UNIT' --property=User=baft-tunnel --property=Group=baft-tunnel --property=UMask=0077 --property=WorkingDirectory='$(side_dir abroad)' --property='RuntimeDirectory=baft-tunnel/$INSTANCE' --property=RuntimeDirectoryMode=0750 --property='Environment=BAFT_TUNNEL_CONTROL_SOCKET=/run/baft-tunnel/$INSTANCE/control.sock' --property=NoNewPrivileges=yes --property=PrivateTmp=yes --property=PrivateDevices=yes --property=ProtectSystem=strict --property=ProtectHome=yes --property=ProtectKernelTunables=yes --property=ProtectKernelModules=yes --property=ProtectKernelLogs=yes --property=ProtectControlGroups=yes --property=ProtectClock=yes --property=ProtectHostname=yes --property=RestrictSUIDSGID=yes --property=RestrictNamespaces=yes --property=RestrictRealtime=yes --property=LockPersonality=yes --property=MemoryDenyWriteExecute=yes --property='SystemCallFilter=@system-service' --property=SystemCallArchitectures=native --property=CapabilityBoundingSet= --property=AmbientCapabilities= --property=RuntimeMaxSec=2400 --property=TimeoutStopSec=10 --property=MemoryMax=512M --property=TasksMax=256 --property=LimitNOFILE=4096 --property=ProtectProc=invisible --property=ProcSubset=pid --property='InaccessiblePaths=-/etc/baft-tunnel -/var/lib/baft-tunnel -/var/lib/baft-tunnel-agent -/run/baft-tunnel/control' --property=IPAddressDeny=any --property='IPAddressAllow=127.0.0.0/8 ::1/128 $IRAN_CIDR' --property='RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX' -- '$REMOTE_BIN' apply --config '$(side_dir abroad)/$INSTANCE.json' --confirm"
echo "RES_LAB_START_RELAY host=iran unit=$RELAY_UNIT"
ssh_host iran "systemd-run --quiet --collect --unit='$RELAY_UNIT' --property=User=nobody --property=Group=nogroup --property=UMask=0077 --property=NoNewPrivileges=yes --property=PrivateTmp=yes --property=PrivateDevices=yes --property=ProtectSystem=strict --property=ProtectHome=yes --property=ProtectKernelTunables=yes --property=ProtectKernelModules=yes --property=ProtectControlGroups=yes --property=RestrictSUIDSGID=yes --property=LockPersonality=yes --property=MemoryDenyWriteExecute=yes --property='SystemCallFilter=@system-service' --property=CapabilityBoundingSet= --property=AmbientCapabilities= --property=RuntimeMaxSec=2400 --property=TimeoutStopSec=3 --property=MemoryMax=96M --property=TasksMax=128 --property=LimitNOFILE=512 --property=IPAddressDeny=any --property='IPAddressAllow=127.0.0.0/8 $ABROAD_CIDR' --property='RestrictAddressFamilies=AF_INET' -- /usr/bin/python3 '$REMOTE_BASE/relay.py' '$RELAY_PORT' '$LAB_ABROAD_HOST' '$TUNNEL_PORT'"
echo "RES_LAB_START_TUNNEL host=iran unit=$IRAN_TUNNEL_UNIT"
start_iran_tunnel() {
  ssh_host iran "systemd-run --quiet --collect --unit='$IRAN_TUNNEL_UNIT' --property=User=baft-tunnel --property=Group=baft-tunnel --property=UMask=0077 --property=WorkingDirectory='$(side_dir iran)' --property='RuntimeDirectory=baft-tunnel/$INSTANCE' --property=RuntimeDirectoryMode=0750 --property='Environment=BAFT_TUNNEL_CONTROL_SOCKET=/run/baft-tunnel/$INSTANCE/control.sock' --property=NoNewPrivileges=yes --property=PrivateTmp=yes --property=PrivateDevices=yes --property=ProtectSystem=strict --property=ProtectHome=yes --property=ProtectKernelTunables=yes --property=ProtectKernelModules=yes --property=ProtectKernelLogs=yes --property=ProtectControlGroups=yes --property=ProtectClock=yes --property=ProtectHostname=yes --property=RestrictSUIDSGID=yes --property=RestrictNamespaces=yes --property=RestrictRealtime=yes --property=LockPersonality=yes --property=MemoryDenyWriteExecute=yes --property='SystemCallFilter=@system-service' --property=SystemCallArchitectures=native --property=CapabilityBoundingSet= --property=AmbientCapabilities= --property=RuntimeMaxSec=2400 --property=TimeoutStopSec=10 --property=MemoryMax=512M --property=TasksMax=256 --property=LimitNOFILE=4096 --property=ProtectProc=invisible --property=ProcSubset=pid --property='InaccessiblePaths=-/etc/baft-tunnel -/var/lib/baft-tunnel -/var/lib/baft-tunnel-agent -/run/baft-tunnel/control' --property=IPAddressDeny=any --property='IPAddressAllow=127.0.0.0/8 ::1/128 $ABROAD_CIDR' --property='RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX' -- '$REMOTE_BIN' apply --config '$(side_dir iran)/$INSTANCE.json' --confirm"
}
start_iran_tunnel

wait_listener() {
  local side="$1" port="$2" attempt
  for attempt in {1..30}; do
    if ssh_host "$side" "ss -lntH 'sport = :$port' | grep -q LISTEN"; then return 0; fi
    sleep 1
  done
  echo "listener did not become ready: side=$side port=$port" >&2
  return 1
}
wait_listener abroad "$ECHO_PORT"
wait_listener abroad "$TUNNEL_PORT"
wait_listener iran "$LOCAL_PORT"
wait_control_socket() {
  local side="$1" attempt
  for attempt in {1..30}; do
    if ssh_host "$side" "test -S '/run/baft-tunnel/$INSTANCE/control.sock' && test \$(stat -c '%U:%G:%a' '/run/baft-tunnel/$INSTANCE/control.sock') = baft-tunnel:baft-tunnel:660"; then
      echo "CONTROL_SOCKET_READY host=$side owner=baft-tunnel:baft-tunnel mode=0660"
      return 0
    fi
    sleep 1
  done
  echo "B2 control socket missing or unsafe: host=$side" >&2
  return 1
}
wait_control_socket iran
wait_control_socket abroad
verify_unit_network_policy() {
  local side="$1" unit="$2" expected_allow="$3" properties
  properties="$(ssh_host "$side" "systemctl show '$unit' -p ActiveState -p IPAddressDeny -p IPAddressAllow --no-pager")"
  printf '%s\n' "$properties" | grep -Eq '^ActiveState=active$' || { echo "LAB unit is not active: $side/$unit" >&2; return 1; }
  printf '%s\n' "$properties" | python3 -c 'import sys; lines=dict(line.split("=",1) for line in sys.stdin.read().splitlines() if "=" in line); denied=set(lines.get("IPAddressDeny","").split()); assert denied in ({"any"},{"0.0.0.0/0","::/0"}), lines' || { echo "LAB unit has no full IPv4+IPv6 IPAddressDeny policy: $side/$unit" >&2; return 1; }
  printf '%s\n' "$properties" | python3 -c 'import sys; lines=dict(line.split("=",1) for line in sys.stdin.read().splitlines() if "=" in line); allow=set(lines.get("IPAddressAllow","").split()); expected={"127.0.0.0/8","::1/128",sys.argv[1]}; assert expected <= allow, {"expected": sorted(expected), "actual": sorted(allow)}' "$expected_allow"
  echo "NETWORK_EGRESS_POLICY_READY host=$side unit=$unit allow=localhost,$expected_allow"
}
verify_unit_network_policy iran "$(tunnel_unit_for iran)" "$ABROAD_CIDR"
verify_unit_network_policy abroad "$(tunnel_unit_for abroad)" "$IRAN_CIDR"


SIZE_MIB="${SIZE_MIB:-16}"
[[ "$SIZE_MIB" =~ ^[0-9]+$ ]] && (( SIZE_MIB >= 4 && SIZE_MIB <= 64 )) || { echo 'SIZE_MIB must be 4..64' >&2; exit 2; }
SIZE=$(( SIZE_MIB * 1024 * 1024 ))
RATE=$(( 1024 * 1024 ))   # 1 MiB/s: the transfer lasts SIZE_MIB seconds, so both cuts land mid-transfer

# The data path must come up before the chaos run (a fresh v3 session, small payload).
ok=0
for attempt in {1..15}; do
  if ssh_host iran "timeout 20 python3 '$REMOTE_BASE/payload.py' 65536 '$LOCAL_PORT' 0" >/dev/null 2>&1; then ok=1; break; fi
  sleep 2
done
(( ok )) || { echo 'data path never came up' >&2; exit 1; }
echo "RES_DATA_PATH_UP=1"

# Resource baselines of the forwarder and server before the chaos run (flat after, no restarts).
unit_pid() { ssh_host "$1" "systemctl show -p MainPID --value '$(tunnel_unit_for "$1")'"; }
unit_restarts() { ssh_host "$1" "systemctl show -p NRestarts --value '$(tunnel_unit_for "$1")'; systemctl show -p ActiveEnterTimestampMonotonic --value '$(tunnel_unit_for "$1")'" | paste -sd' '; }
fd_count() { ssh_host "$1" "ls /proc/$2/fd | wc -l"; }
IRAN_PID_BEFORE="$(unit_pid iran)"; ABROAD_PID_BEFORE="$(unit_pid abroad)"
[[ "$IRAN_PID_BEFORE" =~ ^[0-9]+$ && "$ABROAD_PID_BEFORE" =~ ^[0-9]+$ && "$IRAN_PID_BEFORE" != 0 && "$ABROAD_PID_BEFORE" != 0 ]] || { echo 'could not read tunnel PIDs' >&2; exit 1; }
IRAN_RESTART_BEFORE="$(unit_restarts iran)"; ABROAD_RESTART_BEFORE="$(unit_restarts abroad)"
IRAN_FD_BEFORE="$(fd_count iran "$IRAN_PID_BEFORE")"; ABROAD_FD_BEFORE="$(fd_count abroad "$ABROAD_PID_BEFORE")"
echo "RES_BASELINE iran_pid=$IRAN_PID_BEFORE abroad_pid=$ABROAD_PID_BEFORE iran_fd=$IRAN_FD_BEFORE abroad_fd=$ABROAD_FD_BEFORE"

cut_relay() { ssh_host iran "systemctl kill -s SIGUSR1 '$RELAY_UNIT'"; }
echo "RES_CHAOS_START size_mib=$SIZE_MIB rate_bytes_per_sec=$RATE cuts=2"
ssh_host iran "timeout 170 python3 '$REMOTE_BASE/payload.py' $SIZE '$LOCAL_PORT' $RATE" >"$LOCAL_TMP/chaos.out" 2>&1 &
PAYLOAD_JOB=$!
CUTS_DONE=0
for delay in 4 5; do
  sleep "$delay"
  kill -0 "$PAYLOAD_JOB" 2>/dev/null || { echo 'transfer ended before the planned cut' >&2; break; }
  cut_relay
  CUTS_DONE=$((CUTS_DONE+1))
  echo "RES_CUT number=$CUTS_DONE"
done
CHAOS_RC=0
wait "$PAYLOAD_JOB" || CHAOS_RC=$?
cat "$LOCAL_TMP/chaos.out" || true
(( CUTS_DONE == 2 )) || { echo "only $CUTS_DONE of 2 cuts landed mid-transfer" >&2; exit 1; }
(( CHAOS_RC == 0 )) || { echo 'chaos payload failed: bytes were lost or the stream broke' >&2; exit 1; }
grep -Eq "^DATA_PATH_OK bytes=$SIZE sha256=[0-9a-f]{64} " "$LOCAL_TMP/chaos.out" || { echo 'no verified DATA_PATH_OK line' >&2; exit 1; }
CHAOS_SHA="$(grep -Eo 'sha256=[0-9a-f]{64}' "$LOCAL_TMP/chaos.out" | head -n1 | cut -d= -f2)"

# Evidence that the cuts happened and the streams were resumed, not merely redialed.
RELAY_LOG="$(ssh_host iran "journalctl --no-pager -u '$RELAY_UNIT' -o cat 2>/dev/null || true")"
printf '%s\n' "$RELAY_LOG" | grep -E 'RELAY_CUT' || true
RELAY_CUTS="$(printf '%s\n' "$RELAY_LOG" | grep -Ec 'RELAY_CUT number=[0-9]+ connections=[1-9]' || true)"
(( RELAY_CUTS >= 2 )) || { echo "relay cut evidence insufficient: $RELAY_CUTS cuts with live connections" >&2; exit 1; }
FWD_LOG="$(ssh_host iran "journalctl --no-pager -u '$IRAN_TUNNEL_UNIT' -o cat 2>/dev/null || true")"
printf '%s\n' "$FWD_LOG" | grep -E 'session resumed|resume failed|tunnel session lost' || true
RESUME_EVENTS="$(printf '%s\n' "$FWD_LOG" | grep -Ec '^session resumed: [1-9][0-9]* streams continued' || true)"
(( RESUME_EVENTS >= 2 )) || { echo "resume evidence insufficient: $RESUME_EVENTS resumed sessions" >&2; exit 1; }

# No restarts, same PIDs, flat descriptor counts once the transfer is done.
sleep 5
IRAN_PID_AFTER="$(unit_pid iran)"; ABROAD_PID_AFTER="$(unit_pid abroad)"
[[ "$IRAN_PID_AFTER" == "$IRAN_PID_BEFORE" && "$ABROAD_PID_AFTER" == "$ABROAD_PID_BEFORE" ]] || { echo 'a tunnel service restarted during the chaos run' >&2; exit 1; }
[[ "$(unit_restarts iran)" == "$IRAN_RESTART_BEFORE" && "$(unit_restarts abroad)" == "$ABROAD_RESTART_BEFORE" ]] || { echo 'restart counters changed' >&2; exit 1; }
IRAN_FD_AFTER="$(fd_count iran "$IRAN_PID_AFTER")"; ABROAD_FD_AFTER="$(fd_count abroad "$ABROAD_PID_AFTER")"
(( IRAN_FD_AFTER <= IRAN_FD_BEFORE + 4 && ABROAD_FD_AFTER <= ABROAD_FD_BEFORE + 4 )) || {
  echo "descriptor counts grew: iran $IRAN_FD_BEFORE->$IRAN_FD_AFTER abroad $ABROAD_FD_BEFORE->$ABROAD_FD_AFTER" >&2; exit 1;
}
for side in iran abroad; do
  ssh_host "$side" "systemctl is-active --quiet '$(tunnel_unit_for "$side")'"
done
echo "RES_ACCEPTANCE=PASS sha256_equal=1 cuts=$RELAY_CUTS resumed_sessions=$RESUME_EVENTS restarts=0 fd_iran=$IRAN_FD_BEFORE/$IRAN_FD_AFTER fd_abroad=$ABROAD_FD_BEFORE/$ABROAD_FD_AFTER"
echo "RES_LAB_SUMMARY candidate_sha=$CANDIDATE_SHA binary_sha256=$CANDIDATE_BINARY_SHA256 size_mib=$SIZE_MIB payload_sha256=$CHAOS_SHA cuts=$RELAY_CUTS resumed_sessions=$RESUME_EVENTS"
