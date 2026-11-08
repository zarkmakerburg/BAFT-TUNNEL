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
for p in /usr/local/bin/baft-tunnel /etc/systemd/system/baft-tunnel@.service /etc/baft-tunnel /var/lib/baft-tunnel /run/baft-tunnel/control; do
  [[ ! -e "$p" ]] || die "refusing to overwrite pre-existing path: $p"
done
getent group baft-tunnel >/dev/null 2>&1 || CREATED_GROUP=1
id baft-tunnel >/dev/null 2>&1 || CREATED_USER=1
cleanup() {
  local status=$?
  trap - EXIT
  if [[ -n "$AGENT_PID" ]]; then sudo kill "$AGENT_PID" >/dev/null 2>&1 || true; fi
  for i in "$SERVE" "$FWD"; do
    sudo "$ROOT/scripts/install-agent-control.sh" --remove --instance "$i" >/dev/null 2>&1 || true
    sudo systemctl stop "baft-tunnel@$i.service" >/dev/null 2>&1 || true
  done
  sudo rm -f /etc/systemd/system/baft-tunnel-control-*@.path /etc/systemd/system/baft-tunnel-control-*@.service /etc/tmpfiles.d/baft-tunnel-control.conf
  sudo rm -rf /run/baft-tunnel/control
  if [[ "$INSTALL_DONE" == 1 ]]; then sudo "$ROOT/scripts/uninstall.sh" --stop --purge-config --yes >/dev/null 2>&1 || true; fi
  sudo rm -rf /etc/baft-tunnel /var/lib/baft-tunnel
  sudo rm -f /usr/local/bin/baft-tunnel /etc/systemd/system/baft-tunnel@.service
  sudo systemctl daemon-reload >/dev/null 2>&1 || true
  [[ "$CREATED_USER" == 1 ]] && sudo userdel baft-tunnel >/dev/null 2>&1 || true
  [[ "$CREATED_GROUP" == 1 ]] && sudo groupdel baft-tunnel >/dev/null 2>&1 || true
  rm -rf "$TMP"
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
  { sudo nft list ruleset 2>/dev/null || true; sudo iptables-save 2>/dev/null | grep -v '^#' || true; } | sha256sum | cut -d' ' -f1
  ss -ltnH | awk '{print $4}' | grep -vE ':(18090|18443|18444|18081|18099)$' | sort
  systemctl list-units --all --plain --no-legend --state=active,failed 2>/dev/null | awk '{print $1}' | grep -vE '^baft-tunnel' | sort
}
snapshot >"$TMP/side-before.txt"

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
sudo "$ROOT/scripts/install-agent-control.sh" --instance "$SERVE" >"$TMP/ctl-serve.log"
sudo "$ROOT/scripts/install-agent-control.sh" --instance "$FWD" >"$TMP/ctl-fwd.log"
for k in start stop; do
  sudo systemctl is-active --quiet "baft-tunnel-control-$k@$SERVE.path" || die "path unit $k for $SERVE is not active"
done
[[ "$(sudo stat -c '%U:%G %a' /run/baft-tunnel/control)" == "root:baft-tunnel 770" ]] || die "control dir is not root:baft-tunnel 0770"

# 2. The agent user can request a start, and only for an instance that has a path unit.
sudo -u baft-tunnel touch "/run/baft-tunnel/control/$SERVE.start" || die "agent user cannot write a request"
wait_state "$SERVE" active || die "request did not start $SERVE"
[[ "$(sudo systemctl show -p User --value "baft-tunnel@$SERVE.service")" == baft-tunnel ]] || die "unit does not run as baft-tunnel"
for n in $(seq 1 20); do [[ ! -e "/run/baft-tunnel/control/$SERVE.start" ]] && break; sleep 0.25; done
[[ ! -e "/run/baft-tunnel/control/$SERVE.start" ]] || die "request file was not consumed"
sudo -u baft-tunnel touch "/run/baft-tunnel/control/notinstalled.start"
sleep 3
! sudo systemctl is-active --quiet "baft-tunnel@notinstalled.service" || die "a request without a path unit started a unit"
sudo rm -f /run/baft-tunnel/control/notinstalled.start
step "2b malformed/unregistered requests"
# 2b. Malformed, unregistered, duplicate, stale and symlinked requests (P0-1 acceptance).
CTL=/run/baft-tunnel/control
units_before="$(systemctl list-units --all --plain --no-legend 'baft-tunnel*' | awk '{print $1}' | sort)"
for bad in 'bad name' '-lead' 'a.b' 'UP..PER' "$(printf 'x%.0s' $(seq 1 70))" 'notinstalled' 'agentctl-other'; do
  sudo -u baft-tunnel touch "$CTL/$bad.start" "$CTL/$bad.stop" 2>/dev/null || true
done
sleep 3
units_after="$(systemctl list-units --all --plain --no-legend 'baft-tunnel*' | awk '{print $1}' | sort)"
[[ "$units_before" == "$units_after" ]] || { diff <(echo "$units_before") <(echo "$units_after") >&2; die "a malformed or unregistered request changed units"; }
active "$SERVE" || die "malformed requests stopped $SERVE"
sudo find "$CTL" -maxdepth 1 -type f -delete
step "2b duplicate request"
# Duplicate request on a running unit: consumed, unit untouched (same main PID, no restart).
pid_before="$(sudo systemctl show -p MainPID --value "baft-tunnel@$SERVE.service")"
sudo -u baft-tunnel touch "$CTL/$SERVE.start"; sleep 1; sudo -u baft-tunnel touch "$CTL/$SERVE.start"
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
sudo -u baft-tunnel ln -sfn "$TMP/sentinel" "$CTL/$SERVE.start" || die "agent user cannot create the test symlink"
for n in $(seq 1 20); do [[ ! -e "$CTL/$SERVE.start" && ! -L "$CTL/$SERVE.start" ]] && break; sleep 0.25; done
[[ ! -L "$CTL/$SERVE.start" ]] || die "symlink request was not removed"
[[ "$(sudo cat "$TMP/sentinel")" == sentinel ]] || die "symlink request touched its target"
active "$SERVE" || die "symlink request disturbed $SERVE"
step "2b stale stop request"
# Stale request: a file left from before the unit stopped is consumed once and not replayed later.
sudo -u baft-tunnel touch "$CTL/$SERVE.stop"; wait_state "$SERVE" inactive || die "stop failed"
sleep 2
inactive_after="$(sudo systemctl is-active "baft-tunnel@$SERVE.service" || true)"
[[ "$inactive_after" != active ]] || die "a consumed stop request was replayed into a start"
sudo -u baft-tunnel touch "$CTL/$SERVE.start"; wait_state "$SERVE" active || die "restart request failed"
step "2c no direct systemctl rights"
# The agent user has no direct systemctl rights (no polkit rule, no sudo).
! sudo -u baft-tunnel systemctl --no-ask-password stop "baft-tunnel@$SERVE.service" >/dev/null 2>&1 || die "agent user can stop units directly"
active "$SERVE" || die "unit stopped by the denied direct stop"
sudo -u baft-tunnel touch "/run/baft-tunnel/control/$SERVE.stop"
wait_state "$SERVE" inactive || die "stop request did not stop $SERVE"

step "3 Apply start"
# 3. Apply start through the agent API: success with verify, then rollback of a failing start.
sudo install -d -o baft-tunnel -g baft-tunnel -m 0700 /var/lib/baft-tunnel/agent
sudo -u baft-tunnel "$RUNTIME_BIN" agent-token --out /var/lib/baft-tunnel/agent/token >/dev/null
TOKEN="$(sudo cat /var/lib/baft-tunnel/agent/token)"
# The agent itself validates its config outside systemd, so give it a file-key variant.
sudo install -o baft-tunnel -g baft-tunnel -m 0600 "$NODE/node.key" /var/lib/baft-tunnel/agent/agent.key
python3 - "$TMP/$SERVE.json" "$TMP/agent-config.json" <<'PY'
import json, sys
c = json.load(open(sys.argv[1])); c.pop("key_credential"); c["key"] = "/var/lib/baft-tunnel/agent/agent.key"
c["listen"] = "127.0.0.1:18444"
json.dump(c, open(sys.argv[2], "w"))
PY
sudo install -o baft-tunnel -g baft-tunnel -m 0600 "$TMP/agent-config.json" /var/lib/baft-tunnel/agent/config.json
sudo -u baft-tunnel "$RUNTIME_BIN" agent --config /var/lib/baft-tunnel/agent/config.json --token-file /var/lib/baft-tunnel/agent/token \
  --state-dir /var/lib/baft-tunnel/agent --listen 127.0.0.1:18090 >"$TMP/agent.log" 2>&1 &
AGENT_PID=$!
for n in $(seq 1 40); do curl -fs http://127.0.0.1:18090/v1/health >/dev/null 2>&1 && break; sleep 0.25; done
curl -fs http://127.0.0.1:18090/v1/health >/dev/null || { cat "$TMP/agent.log" >&2; die "agent did not start"; }
apply() { # NAME ROLE KEY
  curl -s -X POST http://127.0.0.1:18090/v1/setup/apply -H "Authorization: Bearer $TOKEN" -d "{\"stage\":\"start\",\"confirm\":true,\"idempotency_key\":\"$3\",\"plan\":{\"engine\":\"tcp-tls\",\"role\":\"$2\",\"direction\":\"direct\",\"node_id\":\"agentctl\",\"peer_id\":\"peer-test\",\"tunnel_id\":\"agentctl\",\"service_port\":18099,\"tunnel_port\":18443,\"peer_host\":$4,\"server_name\":$5,\"instance\":\"$1\"}}"
}
field() { python3 -c 'import sys,json; d=json.load(sys.stdin); print(d.get(sys.argv[1], ""))' "$1"; }
# serve listens on 127.0.0.1:18443 and has no peer to reach: verify passes without a path probe.
out="$(apply "$SERVE" serve ctl-serve-0001 null null)"
[[ "$(echo "$out" | field result)" == applied ]] || { echo "$out" >&2; die "Apply start (serve) was not applied"; }
active "$SERVE" || die "Apply start left $SERVE inactive"
# A malformed or unregistered instance name is refused by the API and creates nothing.
for bad in '../etc' 'bad name' '-x'; do
  [[ "$(apply "$bad" serve ctl-bad-$RANDOM null null | field result)" == refused ]] || die "malformed instance '$bad' was not refused"
done
! sudo ls "$CTL" | grep -qE '^(\.\.|bad|-x)' || die "a malformed API request left a file"
# A repeat with the same key replays the receipt without acting.
[[ "$(apply "$SERVE" serve ctl-serve-0001 null null | field replayed)" == True ]] || die "idempotent replay failed"
# The same request on a running unit is refused and changes nothing.
[[ "$(apply "$SERVE" serve ctl-serve-0002 null null | field result)" == refused ]] || die "running unit was not refused"
sudo -u baft-tunnel touch "/run/baft-tunnel/control/$SERVE.stop"; wait_state "$SERVE" inactive || die "could not stop $SERVE"
# A unit that cannot start (missing certificate): verification fails, Apply stops it and reports rolled_back.
out="$(apply "$FWD" forward ctl-fwd-0001 '"127.0.0.1"' '"tunnel.example.test"')"
[[ "$(echo "$out" | field result)" == rolled_back ]] || { echo "$out" >&2; die "failing start was not rolled back"; }
wait_state "$FWD" inactive || die "rollback left $FWD running"
echo "$out" | grep -q "baft-tunnel" && ! echo "$out" | grep -q "BEGIN" || die "receipt content check failed"
nrec="$(sudo find /var/lib/baft-tunnel/agent/receipts -name '*.json' | wc -l)"
[[ "$nrec" -ge 2 ]] || die "expected receipts, found $nrec"
! sudo grep -rq -- "$TOKEN" /var/lib/baft-tunnel/agent/receipts || die "a receipt contains the token"
# Nothing else changed: firewall hash, listeners and unrelated units match the baseline.
sudo kill "$AGENT_PID" 2>/dev/null || true; wait "$AGENT_PID" 2>/dev/null || true; AGENT_PID=""; sleep 1
snapshot >"$TMP/side-after.txt"
diff "$TMP/side-before.txt" "$TMP/side-after.txt" >&2 || die "firewall, listeners or unrelated units changed"

echo "AGENT_CONTROL_LIFECYCLE=PASS (systemd $v: path units active, control dir root:baft-tunnel 0770, request starts and stops only baft-tunnel@NAME, no direct rights for the agent user, Apply start verified and replayed, running unit refused, failing start rolled back, malformed/unregistered/duplicate/stale/symlink requests harmless, no firewall/listener/unit side effects)"
