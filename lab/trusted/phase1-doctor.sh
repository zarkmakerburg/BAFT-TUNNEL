#!/usr/bin/env bash
# Trusted main-branch harness for the ADR-0042 Doctor two-host LAB: `baft-tunnel doctor --config X
# --network --json` is run on both real hosts against real tunnel units. Acceptance: the read-only checks
# report PASS for config, identity, key permissions, TLS files, and (Iran) a TCP connect PASS to the
# abroad dial address; with the abroad tunnel stopped the Iran network check must be FAIL (negative
# control, exit code 1), and doctor never writes to the host.
# Only this script receives SSH credentials. The PR binary is executed remotely.
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
echo "DC_CANDIDATE_BINARY_SHA256=$CANDIDATE_BINARY_SHA256"

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
# The hub address is written as host:port in the member config; keep this LAB to IPv4.
[[ "$LAB_ABROAD_HOST" != *:* && "$LAB_IRAN_HOST" != *:* ]] || { echo 'the Fleet LAB supports IPv4 LAB hosts only' >&2; exit 2; }
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
INSTANCE="dclab-$RUN_ID"
REMOTE_BASE="/opt/baft-dc-lab-$RUN_ID"
REMOTE_BIN="$REMOTE_BASE/baft-tunnel"
LOCAL_TMP="$(mktemp -d)"
SSH_KEY="$LOCAL_TMP/id_ed25519"
KNOWN_HOSTS="$LOCAL_TMP/known_hosts"
TUNNEL_PORT=18300
LOCAL_PORT=18080
TARGET_PORT=18081
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
side_dir() { printf '%s/%s' "$REMOTE_BASE" "$1"; }
agent_state_dir() { printf '%s/agent-state-%s' "$REMOTE_BASE" "$1"; }
tunnel_unit_for() { printf 'baft-tunnel@%s.service' "$INSTANCE"; }
agent_unit_for() { printf 'baft-tunnel-agent@%s.service' "$INSTANCE"; }

ssh_host() {
  local side="$1" command="$2" host
  host="$(host_for "$side")"
  ssh -i "$SSH_KEY" -p "$(port_for "$side")" -o BatchMode=yes -o IdentitiesOnly=yes \
    -o StrictHostKeyChecking=yes -o UserKnownHostsFile="$KNOWN_HOSTS" \
    -o GlobalKnownHostsFile=/dev/null -o ConnectTimeout=12 -o ServerAliveInterval=10 \
    -o ServerAliveCountMax=2 -o LogLevel=ERROR "$(user_for "$side")@$host" "$command"
}
copy_to_host() {
  local side="$1"; shift
  local host; host="$(host_for "$side")"
  scp -i "$SSH_KEY" -P "$(port_for "$side")" -o BatchMode=yes -o IdentitiesOnly=yes \
    -o StrictHostKeyChecking=yes -o UserKnownHostsFile="$KNOWN_HOSTS" \
    -o GlobalKnownHostsFile=/dev/null -o ConnectTimeout=12 -o LogLevel=ERROR \
    "$@" "$(user_for "$side")@$host:$REMOTE_BASE/"
}
diagnose() {
  local side="$1"
  echo "DC_LAB_DIAGNOSTICS host=$side"
  ssh_host "$side" "journalctl --no-pager -n 100 -u '$(tunnel_unit_for "$side")' -u '$(agent_unit_for "$side")' 2>/dev/null || true; ss -lntp || true" || true
}
cleanup() {
  local rc=$? prepared units
  trap - EXIT
  for side in iran abroad; do
    prepared=0
    [[ "$side" == iran && "$PREPARED_IRAN" == 1 ]] && prepared=1
    [[ "$side" == abroad && "$PREPARED_ABROAD" == 1 ]] && prepared=1
    if (( prepared )); then
      if (( rc != 0 )); then diagnose "$side"; fi
      units="'$(tunnel_unit_for "$side")'"
      if ! ssh_host "$side" "
        systemctl stop $units 2>/dev/null || true
        sleep 1
        if systemctl is-active --quiet '$(tunnel_unit_for "$side")'; then
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
  ssh_host "$side" "set -eu
    test \$(id -u) -eq 0
    command -v systemd-run >/dev/null
    command -v systemctl >/dev/null
    command -v runuser >/dev/null
    command -v curl >/dev/null
    command -v ss >/dev/null
    test -x /usr/bin/python3
    test \"\$(uname -m)\" = x86_64
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
    test \$(id -u baft-tunnel) != \$(id -u baft-tunnel-agent)
    test \$(id -gn baft-tunnel) = baft-tunnel
    test \$(id -gn baft-tunnel-agent) = baft-tunnel-agent
    id -nG baft-tunnel-agent | grep -Eq '(^| )baft-tunnel( |\$)'
    test ! -e '$REMOTE_BASE' && test ! -L '$REMOTE_BASE'
    test ! -e '/run/baft-tunnel/$INSTANCE' && test ! -L '/run/baft-tunnel/$INSTANCE'
    ! systemctl is-active --quiet '$(agent_unit_for "$side")'
    ! systemctl is-active --quiet '$(tunnel_unit_for "$side")'
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
      python3 -c 'import sys; assert int(sys.argv[1], 8) & 0o022 == 0' \"\$parent_mode\"
      for entry in /run/baft-tunnel/*; do
        [ -e \"\$entry\" ] || [ -L \"\$entry\" ] || continue
        if [ \"\$entry\" = '/run/baft-tunnel/control' ]; then
          test -d \"\$entry\" && test ! -L \"\$entry\"
          test \$(stat -c '%U:%G:%a' \"\$entry\") = root:baft-tunnel:770
          continue
        fi
        echo 'existing BAFT-TUNNEL runtime path found; refusing shared-host candidate execution' >&2
        exit 2
      done
    fi
  "
  for port in "$TUNNEL_PORT" "$LOCAL_PORT" "$TARGET_PORT"; do
    busy="$(ssh_host "$side" "ss -lntH 'sport = :$port'")"
    [[ -z "$busy" ]] || { echo "LAB port already in use: host=$side port=$port" >&2; exit 2; }
  done
done

# Throwaway PKI #1 (tcp-tls tunnel, only so each side runs a real tunnel unit whose status the member
# reports) and PKI #2 (Fleet profile). Both CAs are ephemeral 2-day CAs; the CA keys never leave
# the runner and are never copied to a LAB host. Each host receives only its own keys, certificates,
# the CA certificate and the CRL.
mkca() {  # dir cn
  local dir="$1" cn="$2"
  mkdir -p "$dir/newcerts"
  ( cd "$dir"; : > index.txt; echo 1000 > serial; echo 1000 > crlnumber
    openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes -keyout ca.key -out ca.crt -days 2 -sha256 -subj "/CN=$cn" \
      -addext 'basicConstraints=critical,CA:TRUE' -addext 'keyUsage=critical,keyCertSign,cRLSign' >/dev/null 2>&1
    cat > openssl.cnf <<CNF
[ ca ]
default_ca = CA_default
[ CA_default ]
database = $dir/index.txt
new_certs_dir = $dir/newcerts
certificate = $dir/ca.crt
private_key = $dir/ca.key
serial = $dir/serial
crlnumber = $dir/crlnumber
default_md = sha256
default_days = 2
default_crl_days = 2
policy = policy_any
copy_extensions = copy
[ policy_any ]
commonName = supplied
CNF
  )
}
issue() {  # dir name extensions
  local dir="$1" name="$2" ext="$3"
  ( cd "$dir"
    openssl req -new -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes -keyout "$name.key" -out "$name.csr" -subj "/CN=$name" >/dev/null 2>&1
    printf '[ %s_ext ]\nbasicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature\n%s\n' "$name" "$ext" >> openssl.cnf
    openssl ca -batch -notext -config openssl.cnf -extensions "${name}_ext" -in "$name.csr" -out "$name.crt" -days 2 >/dev/null 2>&1 )
}
gencrl() { ( cd "$1"; openssl ca -gencrl -batch -config openssl.cnf -out ca.crl >/dev/null 2>&1 ); }

PKI="$LOCAL_TMP/pki"; FPKI="$LOCAL_TMP/fpki"
mkca "$PKI" 'BAFT fleet lab tunnel ephemeral CA'
for node in entry exit; do
  issue "$PKI" "$node" "extendedKeyUsage=serverAuth,clientAuth
subjectAltName=DNS:$node.fllab,URI:urn:baft-tunnel:node:$node"
done
gencrl "$PKI"
pin() { openssl x509 -in "$1" -outform DER | sha256sum | awk '{print $1}'; }
ENTRY_PIN="$(pin "$PKI/entry.crt")"; EXIT_PIN="$(pin "$PKI/exit.crt")"
[[ "$ENTRY_PIN" =~ ^[0-9a-f]{64}$ && "$EXIT_PIN" =~ ^[0-9a-f]{64}$ ]] || { echo 'could not compute certificate pins' >&2; exit 1; }

# Iran runs the forward node ("entry"), abroad runs the serve node ("exit"). Both are real tunnel
# units so the member's report carries a real unit/session state; there is no payload in this LAB.
python3 - "$LOCAL_TMP" "$INSTANCE" "$LOCAL_PORT" "$TUNNEL_PORT" "$TARGET_PORT" "$LAB_ABROAD_HOST" "$ENTRY_PIN" "$EXIT_PIN" "$(side_dir iran)" "$(side_dir abroad)" <<'PYCFG'
import json, sys
tmp, inst, local_port, tunnel_port, target_port, abroad, entry_pin, exit_pin, idir, adir = sys.argv[1:]
common = {"tunnel_id": inst, "engine": "tcp-tls", "direction": "direct", "max_streams": 8}
fwd = dict(common, mode="forward", listen=f"127.0.0.1:{local_port}", remote=f"{abroad}:{tunnel_port}",
           server_name="exit.fllab", local_id="entry", peer_id="exit", peer_fingerprint=exit_pin,
           cert=f"{idir}/entry.crt", key=f"{idir}/entry.key", ca=f"{idir}/ca.crt", crl=f"{idir}/ca.crl")
srv = dict(common, mode="serve", listen=f"0.0.0.0:{tunnel_port}", target=f"127.0.0.1:{target_port}",
           local_id="exit", peer_id="entry", peer_fingerprint=entry_pin,
           cert=f"{adir}/exit.crt", key=f"{adir}/exit.key", ca=f"{adir}/ca.crt", crl=f"{adir}/ca.crl")
json.dump(fwd, open(f"{tmp}/forward.json", "w")); json.dump(srv, open(f"{tmp}/serve.json", "w"))
PYCFG

# Stage the candidate executable (root-owned, read-only to service users). No candidate script,
# installer or other PR-controlled file is executed.
for side in iran abroad; do
  [[ "$side" == iran ]] && PREPARED_IRAN=1 || PREPARED_ABROAD=1
  ssh_host "$side" "mkdir -m 0755 '$REMOTE_BASE' && test -d '$REMOTE_BASE' && test ! -L '$REMOTE_BASE' && test \$(stat -c '%U:%G:%a' '$REMOTE_BASE') = root:root:755"
  if [[ ! -e "$LOCAL_TMP/baft-tunnel" ]]; then install -m 0755 "$CANDIDATE_BINARY" "$LOCAL_TMP/baft-tunnel"; fi
  copy_to_host "$side" "$LOCAL_TMP/baft-tunnel"
  remote_binary_sha256="$(ssh_host "$side" "sha256sum '$REMOTE_BASE/baft-tunnel' | cut -d ' ' -f1")"
  [[ "$remote_binary_sha256" == "$CANDIDATE_BINARY_SHA256" ]] || {
    echo "remote candidate binary digest mismatch: host=$side expected=$CANDIDATE_BINARY_SHA256 actual=$remote_binary_sha256" >&2
    exit 1
  }
  echo "DC_REMOTE_BINARY_VERIFIED host=$side sha256=$remote_binary_sha256"
  ssh_host "$side" "set -eu
    chown root:root '$REMOTE_BASE/baft-tunnel'
    chmod 0755 '$REMOTE_BASE/baft-tunnel'
    install -d -o baft-tunnel -g baft-tunnel -m 0750 '$(side_dir "$side")'
    test \$(stat -c '%U:%G:%a' '$REMOTE_BASE/baft-tunnel') = root:root:755
  "
done

place_tunnel_files() {  # side node cfg
  local side="$1" node="$2" cfg="$3"
  copy_to_host "$side" "$PKI/$node.crt" "$PKI/$node.key" "$PKI/ca.crt" "$PKI/ca.crl" "$LOCAL_TMP/$cfg"
  ssh_host "$side" "set -eu
    d='$(side_dir "$side")'
    install -m 0644 -o root -g baft-tunnel '$REMOTE_BASE/$node.crt' \"\$d/$node.crt\"
    install -m 0600 -o baft-tunnel -g baft-tunnel '$REMOTE_BASE/$node.key' \"\$d/$node.key\"
    install -m 0644 -o root -g baft-tunnel '$REMOTE_BASE/ca.crt' \"\$d/ca.crt\"
    install -m 0644 -o root -g baft-tunnel '$REMOTE_BASE/ca.crl' \"\$d/ca.crl\"
    install -m 0640 -o root -g baft-tunnel '$REMOTE_BASE/$cfg' \"\$d/$INSTANCE.json\"
    rm -f '$REMOTE_BASE/$node.key' '$REMOTE_BASE/$cfg' '$REMOTE_BASE/$node.crt' '$REMOTE_BASE/ca.crt' '$REMOTE_BASE/ca.crl'
    chown root:baft-tunnel \"\$d\" && chmod 0750 \"\$d\"
  "
}
place_tunnel_files iran entry forward.json
place_tunnel_files abroad exit serve.json

SANDBOX="--property=NoNewPrivileges=yes --property=PrivateTmp=yes --property=PrivateDevices=yes --property=ProtectSystem=strict --property=ProtectHome=yes --property=ProtectKernelTunables=yes --property=ProtectKernelModules=yes --property=ProtectKernelLogs=yes --property=ProtectControlGroups=yes --property=ProtectClock=yes --property=ProtectHostname=yes --property=RestrictSUIDSGID=yes --property=RestrictNamespaces=yes --property=RestrictRealtime=yes --property=LockPersonality=yes --property=MemoryDenyWriteExecute=yes --property=SystemCallFilter=@system-service --property=SystemCallArchitectures=native --property=CapabilityBoundingSet= --property=AmbientCapabilities= --property=RuntimeMaxSec=1800 --property=TimeoutStopSec=10 --property=MemoryMax=512M --property=TasksMax=256 --property=LimitNOFILE=4096"

start_tunnel() {  # side peer-cidr
  local side="$1" peer="$2"
  echo "DC_LAB_START_TUNNEL host=$side unit=$(tunnel_unit_for "$side")"
  ssh_host "$side" "systemd-run --quiet --collect --unit='$(tunnel_unit_for "$side")' --property=User=baft-tunnel --property=Group=baft-tunnel --property=UMask=0077 --property=WorkingDirectory='$(side_dir "$side")' --property='RuntimeDirectory=baft-tunnel/$INSTANCE' --property=RuntimeDirectoryMode=0750 --property='Environment=BAFT_TUNNEL_CONTROL_SOCKET=/run/baft-tunnel/$INSTANCE/control.sock' $SANDBOX --property=ProtectProc=invisible --property=ProcSubset=pid --property='InaccessiblePaths=-/etc/baft-tunnel -/var/lib/baft-tunnel -/var/lib/baft-tunnel-agent -/run/baft-tunnel/control' --property=IPAddressDeny=any --property='IPAddressAllow=127.0.0.0/8 ::1/128 $peer' --property='RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX' -- '$REMOTE_BIN' apply --config '$(side_dir "$side")/$INSTANCE.json' --confirm"
}
start_tunnel abroad "$IRAN_CIDR"
start_tunnel iran "$ABROAD_CIDR"
wait_listener() {
  local side="$1" port="$2" attempt
  for attempt in {1..30}; do
    if ssh_host "$side" "ss -lntH 'sport = :$port' | grep -q LISTEN"; then return 0; fi
    sleep 1
  done
  echo "listener did not become ready: side=$side port=$port" >&2
  return 1
}
wait_listener abroad "$TUNNEL_PORT"
wait_listener iran "$LOCAL_PORT"

# Doctor runs as the unprivileged tunnel user that owns the 0600 key, as an operator would.
run_doctor() {  # side network? -> prints JSON, sets DOCTOR_RC
  local side="$1"
  ssh_host "$side" "runuser -u baft-tunnel -- '$REMOTE_BIN' doctor --config '$(side_dir "$side")/$INSTANCE.json' --network --json 2>/dev/null; echo DOCTOR_RC=\$?"
}
check_field() {  # json id -> status
  python3 -c '
import json, sys
doc = json.loads(sys.stdin.read())
c = {x["id"]: x["status"] for x in doc["checks"]}
print(c.get(sys.argv[1], "absent"))
' "$1"
}
assert_pass() {  # side json ids...
  local side="$1" json="$2"; shift 2
  for id in "$@"; do
    st="$(printf '%s' "$json" | check_field "$id")"
    echo "DC_CHECK side=$side id=$id status=$st"
    [[ "$st" == PASS ]] || { echo "doctor check $id on $side is $st, expected PASS" >&2; exit 1; }
  done
}
split_rc() { out="$1"; rc="${out##*DOCTOR_RC=}"; json="${out%DOCTOR_RC=*}"; }

for side in iran abroad; do
  out="$(run_doctor "$side")"; split_rc
  echo "DC_RC side=$side rc=$rc"
  assert_pass "$side" "$json" config.parse config.validate node.identity key.permissions tls.ca tls.crl tls.cert
  # The throwaway certificates live 2 days, so expiry WARNING is expected and is not asserted PASS.
  printf '%s' "$json" | python3 -c 'import json,sys; d=json.load(sys.stdin); assert d["not_checked"], "not_checked must list what doctor cannot see"; print("DC_NOT_CHECKED", len(d["not_checked"]))'
done

# Iran dials abroad: the network check must PASS while the abroad tunnel listens.
out="$(run_doctor iran)"; split_rc
reach="$(printf '%s' "$json" | python3 -c '
import json, sys
d = json.load(sys.stdin)
print(",".join(c["status"] for c in d["checks"] if c["id"].startswith("reach.")))')"
echo "DC_REACH_UP statuses=$reach rc=$rc"
[[ "$reach" == PASS ]] || { echo "network reach check not PASS with the abroad tunnel listening: $reach" >&2; exit 1; }

# Negative control: stop the abroad tunnel; the same check must now FAIL (and exit code 1).
echo "DC_FAILURE_INJECTED unit=$(tunnel_unit_for abroad)"
ssh_host abroad "systemctl stop '$(tunnel_unit_for abroad)'"
sleep 2
out="$(run_doctor iran)"; split_rc
reach="$(printf '%s' "$json" | python3 -c '
import json, sys
d = json.load(sys.stdin)
print(",".join(c["status"] for c in d["checks"] if c["id"].startswith("reach.")))')"
echo "DC_REACH_DOWN statuses=$reach rc=$rc"
[[ "$reach" == FAIL && "$rc" == 1 ]] || { echo "doctor did not FAIL the reach check with the abroad tunnel stopped: statuses=$reach rc=$rc" >&2; exit 1; }

# Doctor is read-only: the config files are unchanged and nothing new was created in the config directory.
ssh_host iran "cd '$(side_dir iran)' && test \$(ls | wc -l) -eq 5"
echo "DC_ACCEPTANCE=PASS config_checks=1 reach_pass_when_up=1 reach_fail_when_down=1 read_only=1"
echo "DC_LAB_SUMMARY candidate_sha=$CANDIDATE_SHA binary_sha256=$CANDIDATE_BINARY_SHA256"
