#!/usr/bin/env bash
# Agent control helper on a real systemd host (ADR-0023): path units, permissions, start, stop,
# Apply start with verify and rollback through the agent API. Disposable: refuses to run if a BAFT
# install exists and removes everything it created. Needs root/sudo and systemd 247+ (Ubuntu 22.04 / 249).
set -Eeuo pipefail
umask 077
ROOT="$(cd "$(dirname "$0")/../.." && pwd -P)"
BIN="${1:-$ROOT/target/release/baft-tunnel}"
SERVE=agentctl-serve
FWD=agentctl-fwd
POS_S=agentctl-pos-srv
POS_C=agentctl-pos-cli
ECHO_PID=""
PKI_DIR=/etc/baft-tunnel/pki/agentctl
CRED_DIR=/etc/baft-tunnel/credentials
TMP="$(mktemp -d "${TMPDIR:-/tmp}/baft-agent-control.XXXXXX")"
INSTALL_DONE=0; CREATED_USER=0; CREATED_GROUP=0; AGENT_PID=""
die() { echo "agent control lifecycle: $*" >&2; exit 1; }
[[ -x "$BIN" ]] || die "missing executable: $BIN"
for c in systemctl openssl python3 sudo install curl; do command -v "$c" >/dev/null 2>&1 || die "missing command: $c"; done
v="$(systemctl --version | awk 'NR == 1 { print $2 }')"
[[ "$v" =~ ^[0-9]+$ ]] && (( v >= 247 )) || die "systemd 247+ required; found $v"
[[ "$(ps -p 1 -o comm= | tr -d '[:space:]')" == systemd ]] || die "PID 1 is not systemd"
for p in /usr/local/bin/baft-tunnel /etc/systemd/system/baft-tunnel@.service /etc/baft-tunnel /var/lib/baft-tunnel /var/lib/baft-tunnel-agent /run/baft-tunnel/control; do
  [[ ! -e "$p" ]] || die "refusing to overwrite pre-existing path: $p"
done
getent group baft-tunnel >/dev/null 2>&1 || CREATED_GROUP=1
id baft-tunnel >/dev/null 2>&1 || CREATED_USER=1
CREATED_AGENT=0; id baft-tunnel-agent >/dev/null 2>&1 || CREATED_AGENT=1
cleanup() {
  local status=$?
  trap - EXIT
  if [[ -n "$AGENT_PID" ]]; then sudo kill "$AGENT_PID" >/dev/null 2>&1 || true; fi
  if [[ -n "${OBS_PID:-}" ]]; then sudo pkill -TERM -u baft-tunnel-agent -f -- '--listen 127.0.0.1:18091' >/dev/null 2>&1 || true; fi
  [[ -n "$ECHO_PID" ]] && kill "$ECHO_PID" >/dev/null 2>&1 || true
  for i in "$SERVE" "$FWD" "$POS_S" "$POS_C"; do
    sudo "$ROOT/scripts/install-agent-control.sh" --remove --instance "$i" >/dev/null 2>&1 || true
    sudo systemctl disable --now "baft-tunnel@$i.service" >/dev/null 2>&1 || true
  done
  sudo rm -f /etc/systemd/system/baft-tunnel-control-*@.path /etc/systemd/system/baft-tunnel-control-*@.service /etc/tmpfiles.d/baft-tunnel-control.conf \
    /etc/systemd/system/baft-tunnel-agent@.path /etc/systemd/system/baft-tunnel-agent@.service
  sudo rm -rf /etc/systemd/system/baft-tunnel-agent@*.service.d /var/lib/baft-tunnel-agent
  sudo rm -rf /run/baft-tunnel/control
  if [[ "$INSTALL_DONE" == 1 ]]; then sudo "$ROOT/scripts/uninstall.sh" --stop --purge-config --yes >/dev/null 2>&1 || true; fi
  sudo rm -rf /etc/baft-tunnel /var/lib/baft-tunnel
  sudo rm -f /usr/local/bin/baft-tunnel /etc/systemd/system/baft-tunnel@.service
  sudo systemctl daemon-reload >/dev/null 2>&1 || true
  [[ "$CREATED_AGENT" == 1 ]] && { sudo userdel baft-tunnel-agent >/dev/null 2>&1; sudo groupdel baft-tunnel-agent >/dev/null 2>&1; } || true
  [[ "$CREATED_USER" == 1 ]] && sudo userdel baft-tunnel >/dev/null 2>&1 || true
  [[ "$CREATED_GROUP" == 1 ]] && sudo groupdel baft-tunnel >/dev/null 2>&1 || true
  if [[ "$status" == 0 ]]; then
    rm -rf "$TMP"
  else
    # Keep failure diagnostics, but never preserve the temporary CA/node private keys or configs.
    DIAG_ROOT="${RUNNER_TEMP:-/tmp}"
    DIAG_DIR="$(mktemp -d "$DIAG_ROOT/baft-agent-control-failure.XXXXXX")"
    chmod 700 "$DIAG_DIR"
    for f in "$TMP"/*.log "$TMP"/fw-*.txt "$TMP"/side-*.txt; do
      [[ -f "$f" ]] && cp -p "$f" "$DIAG_DIR/"
    done
    {
      printf "%s\\n" "exit_status=$status" "github_run_id=${GITHUB_RUN_ID:-unknown}" "github_job=${GITHUB_JOB:-unknown}" "systemd_version=${v:-unknown}" "preserved_files=logs and side-effect snapshots only; private keys/configs excluded"
    } > "$DIAG_DIR/failure.txt"
    chmod 600 "$DIAG_DIR/"*
    echo "agent control lifecycle: failure diagnostics preserved at $DIAG_DIR" >&2
    rm -rf "$TMP"
  fi
  exit "$status"
}
trap cleanup EXIT
trap 'exit 143' TERM
step() { echo "[step] $*"; }
chmod 755 "$TMP"
RUNTIME_BIN="$TMP/baft-tunnel"
install -m 0755 "$BIN" "$RUNTIME_BIN"

mkdir -p "$TMP/ca/newcerts" "$TMP/node"
chmod 755 "$TMP/ca" "$TMP/ca/newcerts" "$TMP/node"
CA="$TMP/ca"
NODE="$TMP/node"
: > "$CA/index.txt"
printf '1000\n' > "$CA/serial"
printf '1000\n' > "$CA/crlnumber"
openssl req -x509 -newkey rsa:3072 -nodes -keyout "$CA/ca.key" -out "$CA/ca.crt" \
  -days 30 -sha256 -subj "/CN=BAFT-TUNNEL agent control test CA" \
  -addext "basicConstraints=critical,CA:TRUE,pathlen:0" \
  -addext "keyUsage=critical,keyCertSign,cRLSign" >/dev/null 2>&1
cat > "$CA/openssl.cnf" <<EOF_CA
[ ca ]
default_ca = CA_default
[ CA_default ]
database = "$CA/index.txt"
new_certs_dir = "$CA/newcerts"
certificate = "$CA/ca.crt"
private_key = "$CA/ca.key"
serial = "$CA/serial"
crlnumber = "$CA/crlnumber"
default_md = sha256
default_days = 30
default_crl_days = 2
policy = policy_node
unique_subject = no
copy_extensions = none
crl_extensions = crl_ext
[ policy_node ]
commonName = supplied
[ crl_ext ]
authorityKeyIdentifier = keyid:always
EOF_CA
openssl ca -gencrl -batch -config "$CA/openssl.cnf" -out "$CA/ca.crl" >/dev/null 2>&1
chmod 644 "$CA/ca.crt" "$CA/ca.crl" "$CA/openssl.cnf"
openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-256 -out "$NODE/node.key"
chmod 600 "$NODE/node.key"
openssl req -new -key "$NODE/node.key" -out "$NODE/node.csr" -subj "/CN=agentctl"
cat > "$TMP/node-ext.cnf" <<'EOF_EXT'
[node_ext]
basicConstraints=critical,CA:FALSE
keyUsage=critical,digitalSignature
extendedKeyUsage=serverAuth,clientAuth
subjectAltName=URI:urn:baft-tunnel:node:agentctl,DNS:tunnel.example.test
subjectKeyIdentifier=hash
authorityKeyIdentifier=keyid,issuer
EOF_EXT
openssl ca -batch -notext -config "$CA/openssl.cnf" -extfile "$TMP/node-ext.cnf" \
  -extensions node_ext -days 30 -in "$NODE/node.csr" -out "$NODE/node.crt" >/dev/null 2>&1
openssl ca -gencrl -batch -config "$CA/openssl.cnf" -out "$CA/ca.crl" >/dev/null 2>&1
chmod 644 "$NODE/node.crt"

sudo "$ROOT/scripts/install.sh" --binary "$RUNTIME_BIN" >"$TMP/install.log"
for i in "$SERVE" "$FWD"; do sudo "$ROOT/scripts/install.sh" --instance "$i" --node-key "$NODE/node.key" >"$TMP/cred-$i.log"; done
INSTALL_DONE=1
sudo install -d -o root -g baft-tunnel -m 0750 "$PKI_DIR"
for f in ca.crt ca.crl; do sudo install -o root -g root -m 0644 "$CA/$f" "$PKI_DIR/$f"; done
sudo install -o root -g root -m 0644 "$NODE/node.crt" "$PKI_DIR/node.crt"
mkcfg() {
python3 - "$1" "$2" "$PKI_DIR" "$TMP/$1.json" <<'PY'
import json, sys
name, mode, pki, out = sys.argv[1:5]
c = {"mode": mode, "local_id": "agentctl", "peer_id": "peer-test", "tunnel_id": "agentctl-" + mode,
     "direction": "direct", "peer_fingerprint": "a" * 64, "ca": pki + "/ca.crt", "cert": pki + "/node.crt",
     "key_credential": "node-key", "max_streams": 8, "crl": pki + "/ca.crl"}
if mode == "serve":
    c.update({"listen": "127.0.0.1:18443", "target": "127.0.0.1:18099"})
else:
    # A unit that cannot start: its certificate file does not exist, so the process exits at once.
    c.update({"listen": "127.0.0.1:18081", "remote": "127.0.0.1:1", "server_name": "tunnel.example.test", "cert": pki + "/missing.crt"})
open(out, "w").write(json.dumps(c, indent=2) + "\n")
PY
sudo install -o root -g baft-tunnel -m 0640 "$TMP/$1.json" "/etc/baft-tunnel/$1.json"
}
mkcfg "$SERVE" serve
mkcfg "$FWD" forward
sudo systemctl daemon-reload

# Baseline of everything this test must leave alone: firewall rules, listeners, other units.
snapshot() {
  # Rule content only: packet/byte counters change on a busy runner and are not configuration.
  { sudo nft list ruleset 2>/dev/null | sed -E 's/packets [0-9]+ bytes [0-9]+//' || true
    sudo iptables-save 2>/dev/null | grep -v '^#' | sed -E 's/\[[0-9]+:[0-9]+\]//' || true; } >"$TMP/fw-$1.txt"
  sha256sum <"$TMP/fw-$1.txt" | cut -d' ' -f1
  ss -ltnH | awk '{print $4}' | grep -vE ':(18090|18443|18444|18445|18081|18082|18099|1810[0-3])$' | sort
  # Units outside BAFT-TUNNEL, by persistent state only. Which .service units happen to be running is not
  # configuration: dbus-activated and on-demand ones (polkit, timedated, hostnamed, the cloud agent) start
  # and exit on their own. What this test must not leave behind is enabled unit files and active
  # socket/path/timer triggers; those are compared, minus BAFT-TUNNEL's own and the host agent's.
  { systemctl list-unit-files --no-legend --plain --state=enabled,enabled-runtime,linked 2>/dev/null | awk '{print "file " $1}'
    systemctl list-units --all --plain --no-legend --state=active,failed --type=socket,path,timer 2>/dev/null | awk '{print "trigger " $1}'
    systemctl list-units --all --plain --no-legend --state=failed --type=service 2>/dev/null | awk '{print "failed " $1}'; } \
    | grep -vE ' (baft-tunnel|azure|walinuxagent|waagent|hv-|cloud-|session-|user@|systemd-(tmpfiles|journal-flush))' | sort
}
snapshot before >"$TMP/side-before.txt"

active() { sudo systemctl is-active --quiet "baft-tunnel@$1.service"; }
wait_state() { # NAME active|inactive
  local n
  for n in $(seq 1 60); do
    if [[ "$2" == active ]]; then active "$1" && return 0; else ! active "$1" && return 0; fi
    sleep 0.5
  done
  sudo journalctl -u "baft-tunnel@$1.service" -n 60 --no-pager >&2 || true
  return 1
}

# 1. Install the helper explicitly, per instance.
# Each registration also starts that instance's agent service (own user); distinct loopback ports.
sudo "$ROOT/scripts/install-agent-control.sh" --instance "$SERVE" --listen 127.0.0.1:18100 >"$TMP/ctl-serve.log"
sudo "$ROOT/scripts/install-agent-control.sh" --instance "$FWD" --listen 127.0.0.1:18101 >"$TMP/ctl-fwd.log"
for k in start stop enable disable; do
  sudo systemctl is-active --quiet "baft-tunnel-control-$k@$SERVE.path" || die "path unit $k for $SERVE is not active"
done
[[ "$(sudo stat -c '%U:%G %a' /run/baft-tunnel/control)" == "root:baft-tunnel-agent 770" ]] || die "control dir is not root:baft-tunnel-agent 0770"

# 2. The agent user can request a start, and only for an instance that has a path unit.
sudo -u baft-tunnel-agent touch "/run/baft-tunnel/control/$SERVE.start" || die "agent user cannot write a request"
wait_state "$SERVE" active || die "request did not start $SERVE"
[[ "$(sudo systemctl show -p User --value "baft-tunnel@$SERVE.service")" == baft-tunnel ]] || die "unit does not run as baft-tunnel"
for n in $(seq 1 20); do [[ ! -e "/run/baft-tunnel/control/$SERVE.start" ]] && break; sleep 0.25; done
[[ ! -e "/run/baft-tunnel/control/$SERVE.start" ]] || die "request file was not consumed"
sudo -u baft-tunnel-agent touch "/run/baft-tunnel/control/notinstalled.start"
sleep 3
! sudo systemctl is-active --quiet "baft-tunnel@notinstalled.service" || die "a request without a path unit started a unit"
sudo rm -f /run/baft-tunnel/control/notinstalled.start
step "2b malformed/unregistered requests"
# 2b. Malformed, unregistered, duplicate, stale and symlinked requests (P0-1 acceptance).
CTL=/run/baft-tunnel/control
! sudo -u baft-tunnel touch "$CTL/$SERVE.start" 2>/dev/null || die "tunnel service user can write control requests"
sudo -u baft-tunnel-agent touch "$CTL/$SERVE.start" || die "dedicated agent user cannot write control requests"
for n in $(seq 1 20); do [[ ! -e "$CTL/$SERVE.start" ]] && break; sleep 0.25; done
[[ ! -e "$CTL/$SERVE.start" ]] || die "authorization probe request was not consumed"
units_before="$(systemctl list-units --all --plain --no-legend 'baft-tunnel*' | awk '{print $1}' | sort)"
for bad in 'bad name' '-lead' 'a.b' 'UP..PER' "$(printf 'x%.0s' $(seq 1 70))" 'notinstalled' 'agentctl-other'; do
  sudo -u baft-tunnel-agent touch "$CTL/$bad.start" "$CTL/$bad.stop" 2>/dev/null || true
done
sleep 3
units_after="$(systemctl list-units --all --plain --no-legend 'baft-tunnel*' | awk '{print $1}' | sort)"
[[ "$units_before" == "$units_after" ]] || { diff <(echo "$units_before") <(echo "$units_after") >&2; die "a malformed or unregistered request changed units"; }
active "$SERVE" || die "malformed requests stopped $SERVE"
sudo find "$CTL" -maxdepth 1 -type f -delete
step "2b duplicate request"
# Duplicate request on a running unit: consumed, unit untouched (same main PID, no restart).
pid_before="$(sudo systemctl show -p MainPID --value "baft-tunnel@$SERVE.service")"
sudo -u baft-tunnel-agent touch "$CTL/$SERVE.start"; sleep 1; sudo -u baft-tunnel-agent touch "$CTL/$SERVE.start"
for n in $(seq 1 20); do [[ ! -e "$CTL/$SERVE.start" ]] && break; sleep 0.25; done
[[ ! -e "$CTL/$SERVE.start" ]] || die "duplicate request was not consumed"
[[ "$(sudo systemctl show -p MainPID --value "baft-tunnel@$SERVE.service")" == "$pid_before" ]] || die "duplicate start restarted the unit"
step "2b symlink request"
# Symlink: a link to a root-owned file must not let the helper touch the target.
echo sentinel | sudo tee "$TMP/sentinel" >/dev/null
sudo chmod 600 "$TMP/sentinel"
# Let the second duplicate request finish being consumed before placing the link.
for n in $(seq 1 40); do [[ ! -e "$CTL/$SERVE.start" ]] && break; sleep 0.25; done
sleep 2
sudo -u baft-tunnel-agent ln -sfn "$TMP/sentinel" "$CTL/$SERVE.start" || die "agent user cannot create the test symlink"
for n in $(seq 1 20); do [[ ! -e "$CTL/$SERVE.start" && ! -L "$CTL/$SERVE.start" ]] && break; sleep 0.25; done
[[ ! -L "$CTL/$SERVE.start" ]] || die "symlink request was not removed"
[[ "$(sudo cat "$TMP/sentinel")" == sentinel ]] || die "symlink request touched its target"
active "$SERVE" || die "symlink request disturbed $SERVE"
step "2b stale stop request"
# Stale request: a file left from before the unit stopped is consumed once and not replayed later.
sudo -u baft-tunnel-agent touch "$CTL/$SERVE.stop"; wait_state "$SERVE" inactive || die "stop failed"
sleep 2
inactive_after="$(sudo systemctl is-active "baft-tunnel@$SERVE.service" || true)"
[[ "$inactive_after" != active ]] || die "a consumed stop request was replayed into a start"
sudo -u baft-tunnel-agent touch "$CTL/$SERVE.start"; wait_state "$SERVE" active || die "restart request failed"
step "2c no direct systemctl rights"
# The agent user has no direct systemctl rights (no polkit rule, no sudo).
! sudo -u baft-tunnel systemctl --no-ask-password stop "baft-tunnel@$SERVE.service" >/dev/null 2>&1 || die "agent user can stop units directly"
active "$SERVE" || die "unit stopped by the denied direct stop"
sudo -u baft-tunnel-agent touch "/run/baft-tunnel/control/$SERVE.stop"
wait_state "$SERVE" inactive || die "stop request did not stop $SERVE"

step "3 Apply start"
# 3. Apply start through the agent API: success with verify, then rollback of a failing start.
AGENT_STATE=/var/lib/baft-tunnel-agent/lifecycle-test
AGENT_TOKEN=/var/lib/baft-tunnel-agent/token
sudo install -d -o baft-tunnel-agent -g baft-tunnel-agent -m 0700 "$AGENT_STATE"
TOKEN="$(sudo cat "$AGENT_TOKEN")"
# The agent itself validates its config outside systemd, so give it a file-key variant.
sudo install -o baft-tunnel-agent -g baft-tunnel-agent -m 0600 "$NODE/node.key" "$AGENT_STATE/agent.key"
python3 - "$TMP/$SERVE.json" "$TMP/agent-config.json" <<'PY'
import json, sys
c = json.load(open(sys.argv[1])); c.pop("key_credential"); c["key"] = "/var/lib/baft-tunnel-agent/lifecycle-test/agent.key"
c["listen"] = "127.0.0.1:18444"
json.dump(c, open(sys.argv[2], "w"))
PY
sudo install -o baft-tunnel-agent -g baft-tunnel-agent -m 0600 "$TMP/agent-config.json" "$AGENT_STATE/config.json"
sudo -u baft-tunnel-agent "$RUNTIME_BIN" agent --config "$AGENT_STATE/config.json" --token-file "$AGENT_TOKEN" \
  --state-dir "$AGENT_STATE" --listen 127.0.0.1:18090 >"$TMP/agent.log" 2>&1 &
AGENT_PID=$!
for n in $(seq 1 40); do curl -fs http://127.0.0.1:18090/v1/health >/dev/null 2>&1 && break; sleep 0.25; done
curl -fs http://127.0.0.1:18090/v1/health >/dev/null || { cat "$TMP/agent.log" >&2; die "agent did not start"; }
apply() { # NAME ROLE KEY
  curl -s --max-time 60 -X POST http://127.0.0.1:18090/v1/setup/apply -H "Authorization: Bearer $TOKEN" -d "{\"stage\":\"start\",\"confirm\":true,\"idempotency_key\":\"$3\",\"plan\":{\"engine\":\"tcp-tls\",\"role\":\"$2\",\"direction\":\"direct\",\"node_id\":\"agentctl\",\"peer_id\":\"peer-test\",\"tunnel_id\":\"agentctl\",\"service_port\":18099,\"tunnel_port\":18443,\"peer_host\":$4,\"server_name\":$5,\"instance\":\"$1\"}}"
}
field() { python3 -c 'import sys,json; d=json.load(sys.stdin); print(d.get(sys.argv[1], ""))' "$1"; }
# serve listens on 127.0.0.1:18443 and has no peer to reach: verify passes without a path probe.
out="$(apply "$SERVE" serve ctl-serve-0001 null null)"
step "3 serve applied"
[[ "$(echo "$out" | field result)" == applied ]] || { echo "$out" >&2; die "Apply start (serve) was not applied"; }
active "$SERVE" || die "Apply start left $SERVE inactive"
# No peer exists: the session and data path stay Unknown, and Apply never claims ready.
[[ "$(echo "$out" | python3 -c 'import sys,json; d=json.load(sys.stdin); print(d["stages"]["authorized_session"], d["stages"]["data_path_integrity"], d["ready"])')" == "unknown unknown unknown" ]] || { echo "$out" >&2; die "receipt claims an observation that does not exist"; }
step "3 malformed names via API"
# A malformed or unregistered instance name is refused by the API and creates nothing.
for bad in '../etc' 'bad name' '-x'; do
  [[ "$(apply "$bad" serve ctl-bad-$RANDOM null null | field result)" == refused ]] || die "malformed instance '$bad' was not refused"
done
! sudo ls "$CTL" | grep -qE '^(\.\.|bad|-x)' || die "a malformed API request left a file"
step "3 replay"
# A repeat with the same key replays the receipt without acting.
[[ "$(apply "$SERVE" serve ctl-serve-0001 null null | field replayed)" == True ]] || die "idempotent replay failed"
# The same request on a running unit is refused and changes nothing.
[[ "$(apply "$SERVE" serve ctl-serve-0002 null null | field result)" == refused ]] || die "running unit was not refused"
sudo -u baft-tunnel-agent touch "/run/baft-tunnel/control/$SERVE.stop"; wait_state "$SERVE" inactive || die "could not stop $SERVE"
step "3 failing start rollback"
# A unit that cannot start (missing certificate): verification fails, Apply stops it and reports rolled_back.
out="$(apply "$FWD" forward ctl-fwd-0001 '"127.0.0.1"' '"tunnel.example.test"')"
[[ "$(echo "$out" | field result)" == rolled_back ]] || { echo "$out" >&2; die "failing start was not rolled back"; }
wait_state "$FWD" inactive || die "rollback left $FWD running"
echo "$out" | grep -q "baft-tunnel" && ! echo "$out" | grep -q "BEGIN" || die "receipt content check failed"
step "4 real authorized session and data path on one host"
# Two nodes with their own certificates and keys: a serve node and a forward node that pin each other.
issue_node() { # ID DNSNAME -> $NODE/ID.key|.crt
  openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-256 -out "$NODE/$1.key"; chmod 600 "$NODE/$1.key"
  openssl req -new -key "$NODE/$1.key" -out "$NODE/$1.csr" -subj "/CN=$1"
  cat >"$TMP/$1-ext.cnf" <<EOF_N
[node_ext]
basicConstraints=critical,CA:FALSE
keyUsage=critical,digitalSignature
extendedKeyUsage=serverAuth,clientAuth
subjectAltName=URI:urn:baft-tunnel:node:$1,DNS:$2
subjectKeyIdentifier=hash
authorityKeyIdentifier=keyid,issuer
EOF_N
  openssl ca -batch -notext -config "$CA/openssl.cnf" -extfile "$TMP/$1-ext.cnf" -extensions node_ext -days 30 \
    -in "$NODE/$1.csr" -out "$NODE/$1.crt" >/dev/null 2>&1
  chmod 644 "$NODE/$1.crt"
}
issue_node possrv tunnel.example.test
issue_node poscli client.example.test
fp() { openssl x509 -in "$1" -noout -fingerprint -sha256 | cut -d= -f2 | tr -d ':' | tr 'A-F' 'a-f'; }
sudo install -o root -g root -m 0644 "$NODE/possrv.crt" "$PKI_DIR/possrv.crt"
sudo install -o root -g root -m 0644 "$NODE/poscli.crt" "$PKI_DIR/poscli.crt"
sudo "$ROOT/scripts/install.sh" --instance "$POS_S" --node-key "$NODE/possrv.key" >"$TMP/cred-pos-s.log"
sudo "$ROOT/scripts/install.sh" --instance "$POS_C" --node-key "$NODE/poscli.key" >"$TMP/cred-pos-c.log"
python3 - "$PKI_DIR" "$TMP" "$(fp "$NODE/poscli.crt")" "$(fp "$NODE/possrv.crt")" <<'PY'
import json, sys
pki, tmp, cli_fp, srv_fp = sys.argv[1:5]
common = {"tunnel_id": "agentctl-pos", "direction": "direct", "ca": pki + "/ca.crt", "key_credential": "node-key",
          "max_streams": 8, "crl": pki + "/ca.crl"}
srv = dict(common, mode="serve", local_id="possrv", peer_id="poscli", peer_fingerprint=cli_fp,
           cert=pki + "/possrv.crt", listen="127.0.0.1:18445", target="127.0.0.1:18099")
cli = dict(common, mode="forward", local_id="poscli", peer_id="possrv", peer_fingerprint=srv_fp,
           cert=pki + "/poscli.crt", listen="127.0.0.1:18082", remote="127.0.0.1:18445",
           server_name="tunnel.example.test")
json.dump(srv, open(tmp + "/pos-srv.json", "w"), indent=2)
json.dump(cli, open(tmp + "/pos-cli.json", "w"), indent=2)
PY
sudo install -o root -g baft-tunnel -m 0640 "$TMP/pos-srv.json" "/etc/baft-tunnel/$POS_S.json"
sudo install -o root -g baft-tunnel -m 0640 "$TMP/pos-cli.json" "/etc/baft-tunnel/$POS_C.json"
sudo systemctl daemon-reload
sudo "$ROOT/scripts/install-agent-control.sh" --instance "$POS_S" --listen 127.0.0.1:18102 >/dev/null
sudo "$ROOT/scripts/install-agent-control.sh" --instance "$POS_C" --listen 127.0.0.1:18103 >/dev/null
# A fixed echo target stands in for the service behind the serve node.
python3 - <<'PY' &
import socketserver
class H(socketserver.BaseRequestHandler):
    def handle(self):
        while True:
            d = self.request.recv(65536)
            if not d: return
            self.request.sendall(d)
class S(socketserver.ThreadingTCPServer):
    allow_reuse_address = True
S(("127.0.0.1", 18099), H).serve_forever()
PY
ECHO_PID=$!
sleep 1
# Serve first: no peer yet, so the session must stay unknown.
out="$(apply "$POS_S" serve ctl-pos-s-0001 null null)"
[[ "$(echo "$out" | field result)" == applied ]] || { echo "$out" >&2; die "serve node was not applied"; }
[[ "$(echo "$out" | python3 -c 'import sys,json; d=json.load(sys.stdin); print(d["stages"]["authorized_session"], d["ready"])')" == "unknown unknown" ]] || die "session claimed before any peer exists"
# Forward second: the session is authorized, and Apply reports it as observed, still not ready.
out="$(apply "$POS_C" forward ctl-pos-c-0001 '"127.0.0.1"' '"tunnel.example.test"')"
[[ "$(echo "$out" | field result)" == applied ]] || { echo "$out" >&2; sudo journalctl -u "baft-tunnel@$POS_C.service" -n 40 --no-pager >&2; die "forward node was not applied"; }
[[ "$(echo "$out" | python3 -c 'import sys,json; d=json.load(sys.stdin); print(d["stages"]["authorized_session"], d["stages"]["data_path_integrity"], d["ready"])')" == "observed unknown unknown" ]] || { echo "$out" >&2; die "authorized session was not observed"; }
# The serve side reports the same session, bound to its own main PID.
spid="$(sudo systemctl show -p MainPID --value "baft-tunnel@$POS_S.service")"
[[ "$(sudo python3 -c 'import json,sys; d=json.load(open(sys.argv[1])); print(d["state"], d["pid"])' "/run/baft-tunnel/$POS_S/session.json")" == "authorized $spid" ]] || die "serve session file does not match its main PID"
step "4b P1 operations evidence: agent bound to the serve instance (ADR-0024)"
# A second agent reports on the serve unit: session, counters and failure stage from its runtime dir.
OBS_STATE=/var/lib/baft-tunnel-agent/observer-test
sudo install -o baft-tunnel-agent -g baft-tunnel-agent -m 0600 "$NODE/possrv.key" "$AGENT_STATE/pos.key"
sudo install -d -o baft-tunnel-agent -g baft-tunnel-agent -m 0700 "$OBS_STATE"
python3 - "$TMP/pos-srv.json" "$TMP/obs-config.json" <<'PY'
import json, sys
c = json.load(open(sys.argv[1])); c.pop("key_credential"); c["key"] = "/var/lib/baft-tunnel-agent/lifecycle-test/pos.key"
json.dump(c, open(sys.argv[2], "w"))
PY
sudo install -o baft-tunnel-agent -g baft-tunnel-agent -m 0600 "$TMP/obs-config.json" "$OBS_STATE/config.json"
sudo -u baft-tunnel-agent "$RUNTIME_BIN" agent --config "$OBS_STATE/config.json" --token-file "$AGENT_TOKEN" \
  --state-dir "$OBS_STATE" --listen 127.0.0.1:18091 --instance "$POS_S" >"$TMP/obs.log" 2>&1 &
OBS_PID=$!
for n in $(seq 1 40); do curl -fs http://127.0.0.1:18091/v1/health >/dev/null 2>&1 && break; sleep 0.25; done
obs() { curl -s --max-time 30 -X "$1" "http://127.0.0.1:18091$2" -H "Authorization: Bearer $TOKEN"; }
# Let the agent take counter samples before traffic (unit publishes every 5 s, agent samples every 5 s).
sleep 12
# Data path: 64 KiB and 2 MiB of random bytes round-trip through the tunnel unchanged.
for size in 65536 2097152; do
  head -c "$size" /dev/urandom >"$TMP/pos-in.bin"
  python3 - "$TMP/pos-in.bin" "$TMP/pos-out.bin" <<'PY'
import socket, sys, threading
data = open(sys.argv[1], "rb").read()
s = socket.create_connection(("127.0.0.1", 18082), timeout=20)
t = threading.Thread(target=lambda: (s.sendall(data), s.shutdown(socket.SHUT_WR)))
t.start()
out = bytearray()
while len(out) < len(data):
    chunk = s.recv(65536)
    if not chunk: break
    out += chunk
t.join()
open(sys.argv[2], "wb").write(out)
PY
  cmp "$TMP/pos-in.bin" "$TMP/pos-out.bin" || die "$size bytes changed on the way through the tunnel"
done
sleep 12
obs GET /v1/traffic >"$TMP/obs-traffic.json"
python3 - "$TMP/obs-traffic.json" <<'PY' || { cat "$TMP/obs-traffic.json" >&2; die "traffic counters do not show the relayed bytes"; }
import json, sys
d = json.load(open(sys.argv[1]))
need = 65536 + 2097152
assert d["available"] and d["units"] == "bytes" and d["fresh"], d
assert d["bytes_from_peer"] >= need and d["bytes_to_peer"] >= need, d
assert d["streams_total"] >= 2 and d["sessions_total"] >= 1, d
assert any(r["from_peer_bps"] > 0 and r["to_peer_bps"] > 0 for r in d["history"]), d
print("traffic: from_peer=%d to_peer=%d streams=%d" % (d["bytes_from_peer"], d["bytes_to_peer"], d["streams_total"]))
PY
obs POST /v1/diagnostics >"$TMP/obs-diag.json"
python3 - "$TMP/obs-diag.json" <<'PY' || { cat "$TMP/obs-diag.json" >&2; die "diagnostics did not verify the data path tier by tier"; }
import json, sys
d = json.load(open(sys.argv[1]))
r = {t["id"]: t["result"] for t in d["tiers"]}
assert r == {"tcp_reachability": "pass", "secure_channel": "pass", "identity": "pass", "authorization": "pass",
             "session": "pass", "data_path": "pass", "integrity": "unknown"}, r
assert d["verified"] is True and d["highest_passed_tier"] == "data_path" and d["unit"]["result"] == "active", d
PY
obs GET /v1/status >"$TMP/obs-status.json"
python3 - "$TMP/obs-status.json" <<'PY' || { cat "$TMP/obs-status.json" >&2; die "end-to-end status does not match the evidence"; }
import json, sys
t = json.load(open(sys.argv[1]))["tunnel"]
assert t["state"] == "carrying_traffic", t
assert t["tiers"]["authorized_session"]["peer_id"] == "poscli", t
assert t["tiers"]["integrity"]["result"] == "unknown" and isinstance(t["last_verified_unix"], int), t
PY
# Stop the forward node: the session indicator on the serve node must clear.
sudo -u baft-tunnel-agent touch "/run/baft-tunnel/control/$POS_C.stop"; wait_state "$POS_C" inactive || die "could not stop $POS_C"
for n in $(seq 1 40); do sudo test -e "/run/baft-tunnel/$POS_S/session.json" || break; sleep 0.5; done
! sudo test -e "/run/baft-tunnel/$POS_S/session.json" || die "serve session indicator stayed after the peer left"
# Without the peer the status drops a tier, diagnostics fail at the session tier, and the last
# verified time is kept, not invented or cleared.
obs GET /v1/status >"$TMP/obs-status2.json"
obs POST /v1/diagnostics >"$TMP/obs-diag2.json"
python3 - "$TMP/obs-status2.json" "$TMP/obs-diag2.json" <<'PY' || { cat "$TMP/obs-status2.json" "$TMP/obs-diag2.json" >&2; die "status or diagnostics kept a session that ended"; }
import json, sys
t = json.load(open(sys.argv[1]))["tunnel"]; d = json.load(open(sys.argv[2]))
assert t["state"] == "running_no_session", t
r = {x["id"]: x["result"] for x in d["tiers"]}
assert r["session"] == "fail" and r["data_path"] == "unknown" and d["verified"] is False, d
assert isinstance(d["verified_unix"], int), d
PY
# Audit: actor fingerprint, target node and resource; never the token.
sudo python3 - "$OBS_STATE/audit.log" <<'PY' || die "audit lines miss actor/target/resource"
import json, sys
lines = [json.loads(l) for l in open(sys.argv[1])]
assert len(lines) >= 2 and all(l["audit_version"] == 2 and l["actor"]["kind"] == "api_token" and len(l["actor"]["fingerprint"]) == 16
                               and l["target"]["node"] == "possrv" and l["resource"] == "diagnostics" for l in lines), lines
PY
! sudo grep -q -- "$TOKEN" "$OBS_STATE/audit.log" "$AGENT_STATE/audit.log" || die "an audit log contains the token"
# sudo does not relay a signal sent from the script's own process group, so stop the agent itself
# (only processes of the baft-tunnel user match; the pattern names this agent's unique port).
sudo pkill -TERM -u baft-tunnel-agent -f -- '--listen 127.0.0.1:18091' 2>/dev/null || true
wait "$OBS_PID" 2>/dev/null || true; OBS_PID=""
# Wait until the observing agent has released its port, so the side-effect snapshot is clean.
for n in $(seq 1 40); do curl -fs --max-time 1 http://127.0.0.1:18091/v1/health >/dev/null 2>&1 || break; sleep 0.25; done
! curl -fs --max-time 1 http://127.0.0.1:18091/v1/health >/dev/null 2>&1 || die "observing agent did not stop"
sudo -u baft-tunnel-agent touch "/run/baft-tunnel/control/$POS_S.stop"; wait_state "$POS_S" inactive || die "could not stop $POS_S"
kill "$ECHO_PID" 2>/dev/null || true; ECHO_PID=""
step "3 receipts and side effects"
nrec="$(sudo find "$AGENT_STATE/receipts" -name '*.json' | wc -l)"
[[ "$nrec" -ge 2 ]] || die "expected receipts, found $nrec"
! sudo grep -rq -- "$TOKEN" "$AGENT_STATE/receipts" || die "a receipt contains the token"
# Nothing else changed: firewall hash, listeners and unrelated units match the baseline.
sudo kill "$AGENT_PID" 2>/dev/null || true; sleep 2; AGENT_PID=""; step "3 agent stopped"
snapshot after >"$TMP/side-after.txt"
diff "$TMP/side-before.txt" "$TMP/side-after.txt" >&2 || { diff "$TMP/fw-before.txt" "$TMP/fw-after.txt" >&2 || true; die "firewall, listeners or unrelated units changed"; }

echo "AGENT_CONTROL_LIFECYCLE=PASS (systemd $v: path units active, control dir root:baft-tunnel-agent 0770, request starts and stops only baft-tunnel@NAME, no direct rights for the agent user, Apply start verified and replayed, running unit refused, failing start rolled back, real authorized session observed between two nodes with 64 KiB and 2 MiB intact, agent diagnostics/status/traffic/audit tiers matched the unit evidence, malformed/unregistered/duplicate/stale/symlink requests harmless, no firewall/listener/unit side effects)"
