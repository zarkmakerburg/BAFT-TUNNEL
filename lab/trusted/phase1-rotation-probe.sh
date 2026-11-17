#!/usr/bin/env bash
# Trusted main-branch harness for the ADR-0049 session-rotation two-host probe LAB (2 MiB probes on aged Direct sessions, rotation off vs on).
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
echo "ROT_CANDIDATE_BINARY_SHA256=$CANDIDATE_BINARY_SHA256"

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
INSTANCE="rotlab-$RUN_ID"
REMOTE_BASE="/opt/baft-rot-lab-$RUN_ID"
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
ECHO_UNIT="baft-rot-echo-$RUN_ID"
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
  echo "ROT_LAB_DIAGNOSTICS host=$side"
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
      extra_active=""
      if [[ "$side" == abroad ]]; then
        extra_active=" || systemctl is-active --quiet '$ECHO_UNIT'"
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
    $([[ "$side" == abroad ]] && printf "! systemctl is-active --quiet '%s'" "$ECHO_UNIT" || printf ':')
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
  for port in "$api_port" "$TUNNEL_PORT" "$LOCAL_PORT" "$ECHO_PORT"; do
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
import hashlib, socket, sys, time
size, port = int(sys.argv[1]), int(sys.argv[2])
payload = bytes((i % 251 for i in range(size)))
end = time.monotonic() + 40
while True:
    try:
        stream = socket.create_connection(("127.0.0.1", port), timeout=2)
        break
    except OSError:
        if time.monotonic() >= end:
            raise
        time.sleep(.5)
with stream:
    stream.settimeout(30)
    stream.sendall(payload)
    stream.shutdown(socket.SHUT_WR)
    received = bytearray()
    while len(received) < len(payload):
        part = stream.recv(min(65536, len(payload) - len(received)))
        if not part:
            break
        received.extend(part)
if received != payload:
    raise SystemExit(f"payload mismatch {len(received)}/{len(payload)}")
print(f"DATA_PATH_OK bytes={len(received)} sha256={hashlib.sha256(received).hexdigest()}")
PY

# Create a unique ephemeral /opt workspace and stage the candidate executable (root-owned, read-only to
# service users). No candidate script, installer or other PR-controlled file is executed.
for side in iran abroad; do
  [[ "$side" == iran ]] && PREPARED_IRAN=1 || PREPARED_ABROAD=1
  ssh_host "$side" "mkdir -m 0755 '$REMOTE_BASE' && test -d '$REMOTE_BASE' && test ! -L '$REMOTE_BASE' && test \$(stat -c '%U:%G:%a' '$REMOTE_BASE') = root:root:755"
  if [[ ! -e "$LOCAL_TMP/baft-tunnel" ]]; then install -m 0755 "$CANDIDATE_BINARY" "$LOCAL_TMP/baft-tunnel"; fi
  copy_to_host "$side" "$LOCAL_TMP/baft-tunnel" "$LOCAL_TMP/echo.py" "$LOCAL_TMP/payload.py"
  remote_binary_sha256="$(ssh_host "$side" "sha256sum '$REMOTE_BASE/baft-tunnel' | cut -d ' ' -f1")"
  [[ "$remote_binary_sha256" == "$CANDIDATE_BINARY_SHA256" ]] || {
    echo "remote candidate binary digest mismatch: host=$side expected=$CANDIDATE_BINARY_SHA256 actual=$remote_binary_sha256" >&2
    exit 1
  }
  echo "ROT_REMOTE_BINARY_VERIFIED host=$side sha256=$remote_binary_sha256"
  ssh_host "$side" "set -eu
    chown root:root '$REMOTE_BASE/baft-tunnel'
    chmod 0755 '$REMOTE_BASE/baft-tunnel'
    chown root:root '$REMOTE_BASE/echo.py' '$REMOTE_BASE/payload.py'
    chmod 0644 '$REMOTE_BASE/echo.py' '$REMOTE_BASE/payload.py'
    install -d -o baft-tunnel -g baft-tunnel -m 0750 '$(side_dir "$side")'
    install -d -o baft-tunnel-agent -g baft-tunnel-agent -m 0700 '$(agent_state_dir "$side")'
    test \$(stat -c '%U:%G:%a' '$REMOTE_BASE/baft-tunnel') = root:root:755
    test \$(stat -c '%U:%G:%a' '$(side_dir "$side")') = baft-tunnel:baft-tunnel:750
  "
done
ssh_host abroad "install -d -o nobody -g nogroup -m 0700 '$ECHO_STATE' && install -m 0600 -o nobody -g nogroup /dev/null '$ECHO_STATE/stats.json'"

# Create both protocol-v2 Noise pairing bundles through the normal, explicit setup CLI path.
out="$(ssh_host iran "runuser -u baft-tunnel -- '$REMOTE_BIN' setup init --engine noise-tcp --role forward --direction direct --protocol-version 2 --node-id iran --peer-id abroad --tunnel-id '$INSTANCE' --service-port '$LOCAL_PORT' --tunnel-port '$TUNNEL_PORT' --peer-host '$LAB_ABROAD_HOST' --dir '$(side_dir iran)' --instance '$INSTANCE'")"
IRAN_CHECKSUM="$(printf '%s\n' "$out" | awk '/^Checksum / {print $NF}')"
[[ "$IRAN_CHECKSUM" =~ ^[0-9a-f]{8}(-[0-9a-f]{8}){3}$ ]] || { echo 'could not read Iran pairing checksum' >&2; exit 1; }
out="$(ssh_host abroad "runuser -u baft-tunnel -- '$REMOTE_BIN' setup init --engine noise-tcp --role serve --direction direct --protocol-version 2 --node-id abroad --peer-id iran --tunnel-id '$INSTANCE' --service-port '$ECHO_PORT' --tunnel-port '$TUNNEL_PORT' --dir '$(side_dir abroad)' --instance '$INSTANCE'")"
ABROAD_CHECKSUM="$(printf '%s\n' "$out" | awk '/^Checksum / {print $NF}')"
[[ "$ABROAD_CHECKSUM" =~ ^[0-9a-f]{8}(-[0-9a-f]{8}){3}$ ]] || { echo 'could not read Abroad pairing checksum' >&2; exit 1; }

copy_from_host iran "$(side_dir iran)/bundle.json" "$LOCAL_TMP/iran.bundle.json"
copy_from_host abroad "$(side_dir abroad)/bundle.json" "$LOCAL_TMP/abroad.bundle.json"
copy_to_host iran "$LOCAL_TMP/abroad.bundle.json"
copy_to_host abroad "$LOCAL_TMP/iran.bundle.json"
ssh_host iran "install -m 0644 '$REMOTE_BASE/abroad.bundle.json' '$(side_dir iran)/peer.bundle.json' && chown baft-tunnel:baft-tunnel '$(side_dir iran)/peer.bundle.json' && runuser -u baft-tunnel -- '$REMOTE_BIN' setup join --dir '$(side_dir iran)' --bundle '$(side_dir iran)/peer.bundle.json' --checksum '$ABROAD_CHECKSUM'"
ssh_host abroad "install -m 0644 '$REMOTE_BASE/iran.bundle.json' '$(side_dir abroad)/peer.bundle.json' && chown baft-tunnel:baft-tunnel '$(side_dir abroad)/peer.bundle.json' && runuser -u baft-tunnel -- '$REMOTE_BIN' setup join --dir '$(side_dir abroad)' --bundle '$(side_dir abroad)/peer.bundle.json' --checksum '$IRAN_CHECKSUM'"

ssh_host iran "chown root:baft-tunnel '$(side_dir iran)' && chmod 0750 '$(side_dir iran)'"
ssh_host abroad "chown root:baft-tunnel '$(side_dir abroad)' && chmod 0750 '$(side_dir abroad)'"

# The echo destination is a nobody-owned fixture. The data plane and agent run as distinct,
# pre-existing service accounts; systemd creates the exact 0750 per-instance runtime directory.
echo "ROT_LAB_START_ECHO_FIXTURE host=abroad unit=$ECHO_UNIT state=$ECHO_STATE"
ssh_host abroad "systemd-run --quiet --collect --unit='$ECHO_UNIT' --property=User=nobody --property=Group=nogroup --property=UMask=0077 --property=NoNewPrivileges=yes --property=PrivateTmp=yes --property=PrivateDevices=yes --property=ProtectSystem=strict --property=ProtectHome=yes --property=ProtectKernelTunables=yes --property=ProtectKernelModules=yes --property=ProtectKernelLogs=yes --property=ProtectControlGroups=yes --property=ProtectClock=yes --property=ProtectHostname=yes --property=RestrictSUIDSGID=yes --property=RestrictNamespaces=yes --property=RestrictRealtime=yes --property=LockPersonality=yes --property=MemoryDenyWriteExecute=yes --property='SystemCallFilter=@system-service' --property=SystemCallArchitectures=native --property=CapabilityBoundingSet= --property=AmbientCapabilities= --property=RuntimeMaxSec=1500 --property=TimeoutStopSec=3 --property=MemoryMax=96M --property=TasksMax=32 --property=LimitNOFILE=128 --property=IPAddressDeny=any --property='IPAddressAllow=127.0.0.0/8 ::1/128' --property='RestrictAddressFamilies=AF_INET' --property='ReadWritePaths=$ECHO_STATE' -- /usr/bin/python3 '$REMOTE_BASE/echo.py' '$ECHO_STATE/stats.json' '$ECHO_PORT'"
echo "ROT_LAB_START_TUNNEL host=abroad unit=$ABROAD_TUNNEL_UNIT"
ssh_host abroad "systemd-run --quiet --collect --unit='$ABROAD_TUNNEL_UNIT' --property=User=baft-tunnel --property=Group=baft-tunnel --property=UMask=0077 --property=WorkingDirectory='$(side_dir abroad)' --property='RuntimeDirectory=baft-tunnel/$INSTANCE' --property=RuntimeDirectoryMode=0750 --property='Environment=BAFT_TUNNEL_CONTROL_SOCKET=/run/baft-tunnel/$INSTANCE/control.sock' --property=NoNewPrivileges=yes --property=PrivateTmp=yes --property=PrivateDevices=yes --property=ProtectSystem=strict --property=ProtectHome=yes --property=ProtectKernelTunables=yes --property=ProtectKernelModules=yes --property=ProtectKernelLogs=yes --property=ProtectControlGroups=yes --property=ProtectClock=yes --property=ProtectHostname=yes --property=RestrictSUIDSGID=yes --property=RestrictNamespaces=yes --property=RestrictRealtime=yes --property=LockPersonality=yes --property=MemoryDenyWriteExecute=yes --property='SystemCallFilter=@system-service' --property=SystemCallArchitectures=native --property=CapabilityBoundingSet= --property=AmbientCapabilities= --property=RuntimeMaxSec=1500 --property=TimeoutStopSec=10 --property=MemoryMax=512M --property=TasksMax=256 --property=LimitNOFILE=4096 --property=ProtectProc=invisible --property=ProcSubset=pid --property='InaccessiblePaths=-/etc/baft-tunnel -/var/lib/baft-tunnel -/var/lib/baft-tunnel-agent -/run/baft-tunnel/control' --property=IPAddressDeny=any --property='IPAddressAllow=127.0.0.0/8 ::1/128 $IRAN_CIDR' --property='RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX' -- '$REMOTE_BIN' apply --config '$(side_dir abroad)/$INSTANCE.json' --confirm"
echo "ROT_LAB_START_TUNNEL host=iran unit=$IRAN_TUNNEL_UNIT"
start_iran_tunnel() {
  ssh_host iran "systemd-run --quiet --collect --unit='$IRAN_TUNNEL_UNIT' --property=User=baft-tunnel --property=Group=baft-tunnel --property=UMask=0077 --property=WorkingDirectory='$(side_dir iran)' --property='RuntimeDirectory=baft-tunnel/$INSTANCE' --property=RuntimeDirectoryMode=0750 --property='Environment=BAFT_TUNNEL_CONTROL_SOCKET=/run/baft-tunnel/$INSTANCE/control.sock' --property=NoNewPrivileges=yes --property=PrivateTmp=yes --property=PrivateDevices=yes --property=ProtectSystem=strict --property=ProtectHome=yes --property=ProtectKernelTunables=yes --property=ProtectKernelModules=yes --property=ProtectKernelLogs=yes --property=ProtectControlGroups=yes --property=ProtectClock=yes --property=ProtectHostname=yes --property=RestrictSUIDSGID=yes --property=RestrictNamespaces=yes --property=RestrictRealtime=yes --property=LockPersonality=yes --property=MemoryDenyWriteExecute=yes --property='SystemCallFilter=@system-service' --property=SystemCallArchitectures=native --property=CapabilityBoundingSet= --property=AmbientCapabilities= --property=RuntimeMaxSec=1500 --property=TimeoutStopSec=10 --property=MemoryMax=512M --property=TasksMax=256 --property=LimitNOFILE=4096 --property=ProtectProc=invisible --property=ProcSubset=pid --property='InaccessiblePaths=-/etc/baft-tunnel -/var/lib/baft-tunnel -/var/lib/baft-tunnel-agent -/run/baft-tunnel/control' --property=IPAddressDeny=any --property='IPAddressAllow=127.0.0.0/8 ::1/128 $ABROAD_CIDR' --property='RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX' -- '$REMOTE_BIN' apply --config '$(side_dir iran)/$INSTANCE.json' --confirm"
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


ROTATE_SECS="${ROTATE_SECS:-10}"
[[ "$ROTATE_SECS" =~ ^[0-9]+$ ]] && (( ROTATE_SECS >= 5 && ROTATE_SECS <= 60 )) || { echo 'ROTATE_SECS must be 5..60' >&2; exit 2; }
PROBES="${PROBES:-3}"
[[ "$PROBES" =~ ^[0-9]+$ ]] && (( PROBES >= 1 && PROBES <= 6 )) || { echo 'PROBES must be 1..6' >&2; exit 2; }

probe() {
  local label="$1" size="$2" t0 t1 result
  t0="$(date +%s.%N)"
  if ssh_host iran "timeout 75 python3 '$REMOTE_BASE/payload.py' $size '$LOCAL_PORT'" >"$LOCAL_TMP/probe.out" 2>&1 \
    && grep -Eq "^DATA_PATH_OK bytes=$size sha256=[0-9a-f]{64}$" "$LOCAL_TMP/probe.out"; then
    result=PASS
  else
    result=FAIL
  fi
  t1="$(date +%s.%N)"
  python3 -c 'import sys; print("ROT_PROBE label=%s size=%s result=%s secs=%.1f" % (sys.argv[1], sys.argv[2], sys.argv[3], float(sys.argv[5])-float(sys.argv[4])))' "$label" "$size" "$result" "$t0" "$t1"
  if [[ "$result" == PASS ]]; then
    cat "$LOCAL_TMP/probe.out"
  else
    tail -n 20 "$LOCAL_TMP/probe.out" || true
  fi
  [[ "$result" == PASS ]]
}
# First proof the data path works at all (fresh session, small payload), retrying while the session forms.
ok=0
for attempt in {1..30}; do
  if ssh_host iran "timeout 20 python3 '$REMOTE_BASE/payload.py' 65536 '$LOCAL_PORT'" >/dev/null 2>&1; then ok=1; break; fi
  sleep 2
done
(( ok )) || { echo 'data path never came up' >&2; exit 1; }
echo "ROT_DATA_PATH_UP=1"

run_phase() {
  local label="$1" i pass=0
  # The session is now older than the measured throttle threshold (flow age 30 s) once we wait.
  echo "ROT_PHASE_WAIT label=$label seconds=60"
  sleep 60
  for (( i=1; i<=PROBES; i++ )); do
    if probe "$label-$i" 2097152; then pass=$((pass+1)); fi
    sleep 20
  done
  echo "ROT_PHASE_RESULT label=$label pass=$pass total=$PROBES"
  PHASE_PASS="$pass"
}
run_phase rotation_off
OFF_PASS="$PHASE_PASS"

# Turn rotation on for the Iran forward side only (the server needs no change) and restart that unit.
ssh_host iran "systemctl stop '$IRAN_TUNNEL_UNIT'; sleep 2; python3 - '$(side_dir iran)/$INSTANCE.json' '$ROTATE_SECS' <<'PYCFG'
import json, sys
path, secs = sys.argv[1], int(sys.argv[2])
cfg = json.load(open(path))
done = []
def walk(o):
    if isinstance(o, dict):
        if 'max_streams' in o and 'peer_id' in o:
            o['session_rotate_secs'] = secs
            done.append(1)
        for v in o.values():
            walk(v)
    elif isinstance(o, list):
        for v in o:
            walk(v)
walk(cfg)
assert len(done) == 1, 'forward config object not found'
with open(path, 'w') as f:
    json.dump(cfg, f, indent=2)
print('ROT_CONFIG_PATCHED session_rotate_secs=%d' % secs)
PYCFG"
start_iran_tunnel
wait_listener iran "$LOCAL_PORT"
ok=0
for attempt in {1..30}; do
  if ssh_host iran "timeout 20 python3 '$REMOTE_BASE/payload.py' 65536 '$LOCAL_PORT'" >/dev/null 2>&1; then ok=1; break; fi
  sleep 2
done
(( ok )) || { echo 'data path did not come back with rotation on' >&2; exit 1; }
run_phase "rotation_${ROTATE_SECS}s"
ON_PASS="$PHASE_PASS"

# A successful transfer alone is not proof that rotation occurred. Require observable
# replacement-session events after the rotation-enabled forwarder was started.
ROTATION_LOG="$(ssh_host iran "journalctl --no-pager -u '$IRAN_TUNNEL_UNIT' -o cat 2>/dev/null || true")"
printf '%s\n' "$ROTATION_LOG" | grep -E 'session age limit reached|session rotated: draining the previous session|replacement session dial failed' || true
ROTATION_EVENTS="$(printf '%s\n' "$ROTATION_LOG" | grep -c '^session rotated: draining the previous session$' || true)"
[[ "$ROTATION_EVENTS" =~ ^[0-9]+$ ]] || ROTATION_EVENTS=0
(( ROTATION_EVENTS >= 2 )) || {
  echo "rotation evidence insufficient: expected at least 2 completed rotations, observed $ROTATION_EVENTS" >&2
  exit 1
}

# Acceptance is intentionally differential and ordered: the aged, non-rotating
# baseline must fail all probes; the rotation-enabled phase must pass all probes.
(( OFF_PASS == 0 )) || {
  echo "rotation-off baseline did not reproduce the failure: pass=$OFF_PASS total=$PROBES" >&2
  exit 1
}
(( ON_PASS == PROBES )) || {
  echo "rotation-on phase did not pass every probe: pass=$ON_PASS total=$PROBES" >&2
  exit 1
}
echo "ROT_ACCEPTANCE=PASS order=rotation_off_then_rotation_on off=$OFF_PASS/$PROBES on=$ON_PASS/$PROBES rotation_events=$ROTATION_EVENTS"
for side in iran abroad; do
  ssh_host "$side" "systemctl is-active --quiet '$(tunnel_unit_for "$side")'"
done
echo "ROT_LAB_SUMMARY candidate_sha=$CANDIDATE_SHA binary_sha256=$CANDIDATE_BINARY_SHA256 probes=$PROBES rotation_off_pass=$OFF_PASS rotation_${ROTATE_SECS}s_pass=$ON_PASS rotation_events=$ROTATION_EVENTS"
