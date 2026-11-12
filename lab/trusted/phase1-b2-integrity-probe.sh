#!/usr/bin/env bash
# Trusted main-branch harness for the B2 two-host integrity-probe LAB.
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
INSTANCE="b2lab-$RUN_ID"
REMOTE_BASE="/var/tmp/baft-b2-lab-$RUN_ID"
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
ECHO_UNIT="baft-b2-echo-$RUN_ID"
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
  echo "B2_LAB_DIAGNOSTICS host=$side"
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
    test -d /var/tmp && test ! -L /var/tmp
    python3 -c 'import os,stat; s=os.stat("/var/tmp", follow_symlinks=False); assert stat.S_ISDIR(s.st_mode) and s.st_uid == 0 and s.st_mode & stat.S_ISVTX and s.st_mode & 0o002'
    mount_options=\$(findmnt -n -o OPTIONS --target /var/tmp)
    case ",\$mount_options," in *,noexec,*) echo "/var/tmp is mounted noexec; refusing candidate execution" >&2; exit 2 ;; esac
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

# Create ephemeral /run-only trees and stage the candidate executable (root-owned, read-only to
# service users). No candidate script, installer or other PR-controlled file is executed.
for side in iran abroad; do
  [[ "$side" == iran ]] && PREPARED_IRAN=1 || PREPARED_ABROAD=1
  ssh_host "$side" "mkdir -m 0755 '$REMOTE_BASE' && test -d '$REMOTE_BASE' && test ! -L '$REMOTE_BASE' && test \$(stat -c '%U:%G:%a' '$REMOTE_BASE') = root:root:755"
  if [[ ! -e "$LOCAL_TMP/baft-tunnel" ]]; then install -m 0755 "$CANDIDATE_BINARY" "$LOCAL_TMP/baft-tunnel"; fi
  copy_to_host "$side" "$LOCAL_TMP/baft-tunnel" "$LOCAL_TMP/echo.py" "$LOCAL_TMP/payload.py"
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
ssh_host abroad "systemd-run --quiet --collect --unit='$ECHO_UNIT' --property=User=nobody --property=Group=nogroup --property=UMask=0077 --property=NoNewPrivileges=yes --property=PrivateTmp=yes --property=PrivateDevices=yes --property=ProtectSystem=strict --property=ProtectHome=yes --property=ProtectKernelTunables=yes --property=ProtectKernelModules=yes --property=ProtectKernelLogs=yes --property=ProtectControlGroups=yes --property=ProtectClock=yes --property=ProtectHostname=yes --property=RestrictSUIDSGID=yes --property=RestrictNamespaces=yes --property=RestrictRealtime=yes --property=LockPersonality=yes --property=MemoryDenyWriteExecute=yes --property='SystemCallFilter=@system-service' --property=SystemCallArchitectures=native --property=CapabilityBoundingSet= --property=AmbientCapabilities= --property=RuntimeMaxSec=600 --property=TimeoutStopSec=3 --property=MemoryMax=96M --property=TasksMax=32 --property=LimitNOFILE=128 --property=IPAddressDeny=any --property='IPAddressAllow=127.0.0.0/8 ::1/128' --property='RestrictAddressFamilies=AF_INET' --property='ReadWritePaths=$ECHO_STATE' -- /usr/bin/python3 '$REMOTE_BASE/echo.py' '$ECHO_STATE/stats.json' '$ECHO_PORT'"
ssh_host abroad "systemd-run --quiet --collect --unit='$ABROAD_TUNNEL_UNIT' --property=User=baft-tunnel --property=Group=baft-tunnel --property=UMask=0077 --property=WorkingDirectory='$(side_dir abroad)' --property='RuntimeDirectory=baft-tunnel/$INSTANCE' --property=RuntimeDirectoryMode=0750 --property='Environment=BAFT_TUNNEL_CONTROL_SOCKET=/run/baft-tunnel/$INSTANCE/control.sock' --property=NoNewPrivileges=yes --property=PrivateTmp=yes --property=PrivateDevices=yes --property=ProtectSystem=strict --property=ProtectHome=yes --property=ProtectKernelTunables=yes --property=ProtectKernelModules=yes --property=ProtectKernelLogs=yes --property=ProtectControlGroups=yes --property=ProtectClock=yes --property=ProtectHostname=yes --property=RestrictSUIDSGID=yes --property=RestrictNamespaces=yes --property=RestrictRealtime=yes --property=LockPersonality=yes --property=MemoryDenyWriteExecute=yes --property='SystemCallFilter=@system-service' --property=SystemCallArchitectures=native --property=CapabilityBoundingSet= --property=AmbientCapabilities= --property=RuntimeMaxSec=600 --property=TimeoutStopSec=10 --property=MemoryMax=512M --property=TasksMax=256 --property=LimitNOFILE=4096 --property=ProtectProc=invisible --property=ProcSubset=pid --property='InaccessiblePaths=-/etc/baft-tunnel -/var/lib/baft-tunnel -/var/lib/baft-tunnel-agent -/run/baft-tunnel/control' --property=IPAddressDeny=any --property="IPAddressAllow=127.0.0.0/8 ::1/128 $IRAN_CIDR" --property='RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX' -- '$REMOTE_BIN' apply --config '$(side_dir abroad)/$INSTANCE.json' --confirm"
ssh_host iran "systemd-run --quiet --collect --unit='$IRAN_TUNNEL_UNIT' --property=User=baft-tunnel --property=Group=baft-tunnel --property=UMask=0077 --property=WorkingDirectory='$(side_dir iran)' --property='RuntimeDirectory=baft-tunnel/$INSTANCE' --property=RuntimeDirectoryMode=0750 --property='Environment=BAFT_TUNNEL_CONTROL_SOCKET=/run/baft-tunnel/$INSTANCE/control.sock' --property=NoNewPrivileges=yes --property=PrivateTmp=yes --property=PrivateDevices=yes --property=ProtectSystem=strict --property=ProtectHome=yes --property=ProtectKernelTunables=yes --property=ProtectKernelModules=yes --property=ProtectKernelLogs=yes --property=ProtectControlGroups=yes --property=ProtectClock=yes --property=ProtectHostname=yes --property=RestrictSUIDSGID=yes --property=RestrictNamespaces=yes --property=RestrictRealtime=yes --property=LockPersonality=yes --property=MemoryDenyWriteExecute=yes --property='SystemCallFilter=@system-service' --property=SystemCallArchitectures=native --property=CapabilityBoundingSet= --property=AmbientCapabilities= --property=RuntimeMaxSec=600 --property=TimeoutStopSec=10 --property=MemoryMax=512M --property=TasksMax=256 --property=LimitNOFILE=4096 --property=ProtectProc=invisible --property=ProcSubset=pid --property='InaccessiblePaths=-/etc/baft-tunnel -/var/lib/baft-tunnel -/var/lib/baft-tunnel-agent -/run/baft-tunnel/control' --property=IPAddressDeny=any --property="IPAddressAllow=127.0.0.0/8 ::1/128 $ABROAD_CIDR" --property='RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX' -- '$REMOTE_BIN' apply --config '$(side_dir iran)/$INSTANCE.json' --confirm"

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

# Create a separate local Agent identity and token on each host; the SSH key has already been
# removed from the process environment and is never copied to either host.
for side in iran abroad; do
  api_port="$(api_port_for "$side")"
  ssh_host "$side" "runuser -u baft-tunnel-agent -- '$REMOTE_BIN' agent-token --out '$(agent_state_dir "$side")/token'"
  agent_unit="$(agent_unit_for "$side")"
  ssh_host "$side" "systemd-run --quiet --unit='$agent_unit' --property=User=baft-tunnel-agent --property=Group=baft-tunnel-agent --property=UMask=0077 --property=SupplementaryGroups=baft-tunnel --property=NoNewPrivileges=yes --property=PrivateTmp=yes --property=PrivateDevices=yes --property=ProtectSystem=strict --property=ProtectHome=yes --property=ProtectKernelTunables=yes --property=ProtectKernelModules=yes --property=ProtectKernelLogs=yes --property=ProtectControlGroups=yes --property=ProtectClock=yes --property=ProtectHostname=yes --property=RestrictSUIDSGID=yes --property=RestrictNamespaces=yes --property=RestrictRealtime=yes --property=LockPersonality=yes --property=MemoryDenyWriteExecute=yes --property='SystemCallFilter=@system-service' --property=SystemCallArchitectures=native --property=CapabilityBoundingSet= --property=AmbientCapabilities= --property=RuntimeMaxSec=600 --property=TimeoutStopSec=10 --property=MemoryMax=256M --property=TasksMax=64 --property=LimitNOFILE=1024 --property='InaccessiblePaths=-/etc/baft-tunnel -/var/lib/baft-tunnel -/var/lib/baft-tunnel-agent -/run/baft-tunnel/control' --property='ReadWritePaths=$(agent_state_dir "$side")' --property=IPAddressDeny=any --property='IPAddressAllow=127.0.0.0/8 ::1/128' --property='RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX' -- '$REMOTE_BIN' agent --config '$(side_dir "$side")/$INSTANCE.json' --token-file '$(agent_state_dir "$side")/token' --state-dir '$(agent_state_dir "$side")' --instance '$INSTANCE' --listen '127.0.0.1:$api_port'"
done
for side in iran abroad; do
  properties="$(ssh_host "$side" "systemctl show '$(agent_unit_for "$side")' -p ActiveState -p IPAddressDeny -p IPAddressAllow --no-pager")"
  printf '%s\n' "$properties" | grep -Eq '^ActiveState=active$' || { echo "Agent unit is not active: $side" >&2; exit 1; }
  printf '%s\n' "$properties" | python3 -c 'import sys; lines=dict(line.split("=",1) for line in sys.stdin.read().splitlines() if "=" in line); denied=set(lines.get("IPAddressDeny","").split()); assert denied in ({"any"},{"0.0.0.0/0","::/0"}), lines' || { echo "Agent has no full IPv4+IPv6 IPAddressDeny policy: $side" >&2; exit 1; }
  printf '%s\n' "$properties" | python3 -c 'import sys; lines=dict(line.split("=",1) for line in sys.stdin.read().splitlines() if "=" in line); assert set(lines.get("IPAddressAllow","").split()) == {"127.0.0.0/8","::1/128"}, lines'
done

api_request() {
  local side="$1" method="$2" path="$3" body="${4:-}" api_port
  api_port="$(api_port_for "$side")"
  if [[ "$method" == POST ]]; then
    ssh_host "$side" "set -eu; token=\$(cat '$(agent_state_dir "$side")/token'); curl --fail --silent --show-error --max-time 25 -X POST -H \"Authorization: Bearer \$token\" -H 'Content-Type: application/json' --data '$body' 'http://127.0.0.1:$api_port$path'"
  else
    ssh_host "$side" "set -eu; token=\$(cat '$(agent_state_dir "$side")/token'); curl --fail --silent --show-error --max-time 10 -H \"Authorization: Bearer \$token\" 'http://127.0.0.1:$api_port$path'"
  fi
}
wait_session() {
  local side="$1" expected_peer="$2" status attempt
  for attempt in {1..40}; do
    if status="$(api_request "$side" GET /v1/status 2>/dev/null)"; then
      if printf '%s' "$status" | python3 -c 'import json,sys; d=json.load(sys.stdin); s=d.get("tunnel",{}).get("tiers",{}).get("authorized_session",{}); sys.exit(0 if s.get("result") == "observed" and s.get("peer_id") == sys.argv[1] and s.get("direction") == "direct" else 1)' "$expected_peer" ; then
        printf '%s' "$status" | python3 -c 'import json,sys; d=json.load(sys.stdin); print("AUTHORIZED_SESSION peer=%s" % d["tunnel"]["tiers"]["authorized_session"].get("peer_id","?"))'
        return 0
      fi
    fi
    sleep 1
  done
  echo "authenticated session not observed: side=$side expected_peer=$expected_peer" >&2
  return 1
}
wait_session iran abroad
wait_session abroad iran

# Confirm ordinary data transfer first, then snapshot the destination TCP service counters.
ssh_host iran "python3 '$REMOTE_BASE/payload.py' 65536 '$LOCAL_PORT' && python3 '$REMOTE_BASE/payload.py' 2097289 '$LOCAL_PORT'"
sleep 1
BEFORE_STATS="$(ssh_host abroad "cat '$ECHO_STATE/stats.json'")"

verify_probe() {
  local side="$1" peer="$2" response audit status attempt agent_unit limited audit_after
  response="$(api_request "$side" POST /v1/verify "")"
  printf '%s' "$response" | python3 -c '
import json, sys
d = json.load(sys.stdin)
peer, tunnel = sys.argv[1], sys.argv[2]
assert d.get("result") == "pass", d
assert d.get("reason") is None, d
assert d.get("bytes") == 65536, d
assert d.get("sha256_match") is True, d
assert d.get("peer_id") == peer, d
assert d.get("tunnel_id") == tunnel, d
assert d.get("direction") == "direct", d
assert d.get("timestamp", 0) > 0, d
assert d.get("duration_ms") is not None and 0 <= d["duration_ms"] <= 10000, d
assert "sha256" not in d and "digest" not in d, d
print("INTEGRITY_PROBE_PASS peer=%s bytes=%s duration_ms=%s" % (peer, d["bytes"], d["duration_ms"]))
' "$peer" "$INSTANCE"

  audit="$(api_request "$side" GET /v1/audit)"
  printf '%s' "$audit" | python3 -c '
import json, sys
d = json.load(sys.stdin)
resource = "tunnel/" + sys.argv[1]
events = [e for e in d.get("entries", []) if e.get("action") == "integrity_probe" and e.get("resource") == resource]
assert len(events) == 1 and events[0].get("result") == "pass", events
assert events[0].get("detail") == "sha256_verified", events
assert not events[0].get("sha256"), events
print("INTEGRITY_AUDIT_PASS")
' "$INSTANCE"

  agent_unit="$(agent_unit_for "$side")"
  ssh_host "$side" "systemctl restart '$agent_unit'"
  status=""
  for attempt in {1..20}; do
    if status="$(api_request "$side" GET /v1/status 2>/dev/null)" && [[ -n "$status" ]]; then
      if printf '%s' "$status" | python3 -c '
import json, sys
d = json.load(sys.stdin)
tunnel = d.get("tunnel") or {}
tiers = tunnel.get("tiers", {})
integrity = tiers.get("integrity", {})
last = integrity.get("last_probe") or {}
assert integrity.get("result") == "unknown", integrity
assert last.get("result") == "pass" and last.get("source") == "in_product_probe" and last.get("timestamp", 0) > 0, last
' ; then break; fi
    fi
    sleep 1
  done
  [[ -n "$status" ]] || { echo "Agent did not recover after restart: $side" >&2; return 1; }
  printf '%s' "$status" | python3 -c '
import json, sys
d = json.load(sys.stdin)
last = d["tunnel"]["tiers"]["integrity"]["last_probe"]
assert last["result"] == "pass" and last["source"] == "in_product_probe"
print("LAST_PROBE_PERSISTED_AFTER_AGENT_RESTART_PASS")
'

  limited="$(api_request "$side" POST /v1/verify "")"
  printf '%s' "$limited" | python3 -c '
import json, sys
d = json.load(sys.stdin)
assert d.get("result") == "unknown" and d.get("reason") == "rate_limited", d
assert d.get("bytes") == 0 and d.get("sha256_match") is False, d
assert d.get("duration_ms") is None and d.get("timestamp", 0) > 0, d
print("PROBE_COOLDOWN_PASS")
'

  audit_after="$(api_request "$side" GET /v1/audit)"
  printf '%s' "$audit_after" | python3 -c '
import json, sys
d = json.load(sys.stdin)
resource = "tunnel/" + sys.argv[1]
events = [e for e in d.get("entries", []) if e.get("action") == "integrity_probe" and e.get("resource") == resource]
assert len(events) == 2, events
assert sorted(e.get("result") for e in events) == ["pass", "unknown"], events
assert sorted(e.get("detail") for e in events) == ["rate_limited", "sha256_verified"], events
assert all(not e.get("sha256") for e in events), events
print("INTEGRITY_AUDIT_EVENTS_PASS")
' "$INSTANCE"
  status="$(api_request "$side" GET /v1/status)"
  printf '%s' "$status" | python3 -c '
import json, sys
d = json.load(sys.stdin)
integrity = d["tunnel"]["tiers"]["integrity"]
last = integrity["last_probe"]
assert integrity["result"] == "unknown", integrity
assert last["result"] == "unknown" and last["reason"] == "rate_limited", last
print("LAST_PROBE_COOLDOWN_PERSISTED_PASS")
'
}
verify_probe iran abroad
verify_probe abroad iran

AFTER_STATS="$(ssh_host abroad "cat '$ECHO_STATE/stats.json'")"
python3 - "$BEFORE_STATS" "$AFTER_STATS" <<'PYSTATS'
import json, sys
before, after = map(json.loads, sys.argv[1:])
assert before == after, (before, after)
print("INTEGRITY_PROBE_DID_NOT_TOUCH_TCP_TARGET=PASS")
PYSTATS

# Both B2 probes must leave ordinary forwarding operational.
ssh_host iran "python3 '$REMOTE_BASE/payload.py' 65536 '$LOCAL_PORT' && python3 '$REMOTE_BASE/payload.py' 2097289 '$LOCAL_PORT'"
for side in iran abroad; do
  ssh_host "$side" "systemctl is-active --quiet '$(tunnel_unit_for "$side")' && systemctl is-active --quiet '$(agent_unit_for "$side")'"
done

echo "TRUSTED_B2_INTEGRITY_LAB=PASS candidate_sha=$CANDIDATE_SHA protocol=v2 direction=direct probes_both_directions=1 setup_v2_selection=1 bytes=65536 audit=pass+rate_limited agent_restart_persistence=1 cooldown=1 tcp_target_untouched=1 dataplane_before_after=1 candidate_unprivileged=1 ssh_key_exposed_to_candidate=0"
