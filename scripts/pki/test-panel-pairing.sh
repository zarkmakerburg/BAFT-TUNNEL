#!/usr/bin/env bash
# Panel pairing on real systemd (U4 + U5b, ADR-0026): after `install.sh --register` the agent runs
# in setup mode; init and join go through the agent API only (no root command for setup), the root
# setup helper writes /etc/baft-tunnel with the service owners, the own bundle is exported and the
# peer bundle imported with its checksum, the agent reloads with the configuration, and Start
# brings the tunnel up. Tamper cases: a symlinked request, a request for another directory,
# instance or stage, an oversized request and a planted result link are refused and change nothing.
# Disposable: refuses to run if a BAFT install exists and removes everything it created.
# Needs root/sudo and systemd 247+ (Ubuntu 22.04 / 249).
set -Eeuo pipefail
umask 022
ROOT="$(cd "$(dirname "$0")/../.." && pwd -P)"
BIN="${1:-$ROOT/target/release/baft-tunnel}"
NAME=pairpanel
API=127.0.0.1:18130
CTL=/run/baft-tunnel/control
TMP="$(mktemp -d "${TMPDIR:-/tmp}/baft-panel-pairing.XXXXXX")"
CREATED_USER=0; CREATED_GROUP=0; CREATED_AGENT=0; INSTALLED=0; ECHO_PID=""
die() { echo "panel pairing: $*" >&2; exit 1; }
step() { echo "[step] $*"; }
[[ -x "$BIN" ]] || die "missing executable: $BIN"
for c in systemctl python3 sudo curl; do command -v "$c" >/dev/null 2>&1 || die "missing command: $c"; done
v="$(systemctl --version | awk 'NR == 1 { print $2 }')"
[[ "$v" =~ ^[0-9]+$ ]] && (( v >= 247 )) || die "systemd 247+ required; found $v"
[[ "$(ps -p 1 -o comm= | tr -d '[:space:]')" == systemd ]] || die "PID 1 is not systemd"
for p in /usr/local/bin/baft-tunnel /etc/systemd/system/baft-tunnel@.service /etc/baft-tunnel /var/lib/baft-tunnel \
         /var/lib/baft-tunnel-agent "$CTL" /etc/systemd/system/baft-tunnel-agent@.service; do
  [[ ! -e "$p" ]] || die "refusing to overwrite pre-existing path: $p"
done
getent group baft-tunnel >/dev/null 2>&1 || CREATED_GROUP=1
id baft-tunnel >/dev/null 2>&1 || CREATED_USER=1
id baft-tunnel-agent >/dev/null 2>&1 || CREATED_AGENT=1
cleanup() {
  local status=$?
  trap - EXIT
  if [[ "$status" != 0 ]]; then
    sudo journalctl -u "baft-tunnel-agent@$NAME.service" -u "baft-tunnel-control-setup@$NAME.service" \
      -u "baft-tunnel@$NAME.service" -n 120 --no-pager >&2 2>/dev/null || true
  fi
  [[ -n "$ECHO_PID" ]] && kill "$ECHO_PID" >/dev/null 2>&1 || true
  sudo "$ROOT/scripts/install-agent-control.sh" --remove --instance "$NAME" >/dev/null 2>&1 || true
  sudo systemctl disable --now "baft-tunnel@$NAME.service" >/dev/null 2>&1 || true
  if [[ "$INSTALLED" == 1 ]]; then sudo "$ROOT/scripts/uninstall.sh" --stop --purge-config --yes >/dev/null 2>&1 || true; fi
  sudo rm -rf /etc/baft-tunnel /var/lib/baft-tunnel /var/lib/baft-tunnel-agent /var/lib/baft-tunnel-control "$CTL"
  sudo rm -f /usr/local/bin/baft-tunnel /etc/systemd/system/baft-tunnel@.service \
    /etc/systemd/system/baft-tunnel-control-*@.* /etc/systemd/system/baft-tunnel-agent@.* /etc/tmpfiles.d/baft-tunnel-control.conf
  sudo rm -rf /etc/systemd/system/baft-tunnel-agent@*.service.d
  sudo systemctl daemon-reload >/dev/null 2>&1 || true
  [[ "$CREATED_AGENT" == 1 ]] && { sudo userdel baft-tunnel-agent >/dev/null 2>&1; sudo groupdel baft-tunnel-agent >/dev/null 2>&1; } || true
  [[ "$CREATED_USER" == 1 ]] && sudo userdel baft-tunnel >/dev/null 2>&1 || true
  [[ "$CREATED_GROUP" == 1 ]] && sudo groupdel baft-tunnel >/dev/null 2>&1 || true
  rm -rf "$TMP"
  exit "$status"
}
trap cleanup EXIT
trap 'exit 143' TERM
chmod 755 "$TMP"
install -m 0755 "$BIN" "$TMP/baft-tunnel"

step "1 one-step install; the agent runs in setup mode before any configuration"
sudo "$ROOT/scripts/install.sh" --binary "$TMP/baft-tunnel" --register "$NAME" --agent-listen "$API" >"$TMP/install.log"
INSTALLED=1
sudo systemctl is-enabled --quiet "baft-tunnel-control-setup@$NAME.path" || die "setup helper path unit is not enabled"
sudo systemctl is-active --quiet "baft-tunnel-control-setup@$NAME.path" || die "setup helper path unit is not active"
for n in $(seq 1 60); do curl -fs "http://$API/v1/health" >/dev/null 2>&1 && break; sleep 0.5; done
curl -fs "http://$API/v1/health" >/dev/null || die "agent is not running in setup mode"
TOKEN="$(sudo cat /var/lib/baft-tunnel-agent/token)"
api() { curl -s --max-time 90 -X "$1" "http://$API$2" -H "Authorization: Bearer $TOKEN" ${3:+--data-binary "$3"}; }
code() { curl -s -o /dev/null -w '%{http_code}' --max-time 90 -X "$1" "http://$API$2" -H "Authorization: Bearer $TOKEN" ${3:+--data-binary "$3"}; }
field() { python3 -c 'import sys,json; d=json.load(sys.stdin)
for k in sys.argv[1].split("."): d = d.get(k, "") if isinstance(d, dict) else ""
print(d)' "$1"; }
api GET /v1/status >"$TMP/st0.json"
[[ "$(field setup_pending <"$TMP/st0.json")" == True ]] || { cat "$TMP/st0.json" >&2; die "status does not report setup mode"; }
[[ "$(field setup_helper.available <"$TMP/st0.json")" == True ]] || { cat "$TMP/st0.json" >&2; die "setup helper not available to the agent"; }
[[ "$(code GET /v1/security)" == 409 ]] || die "setup mode answered a non-setup route"
[[ ! -e "/etc/baft-tunnel/$NAME.json" ]] || die "configuration exists before pairing"

PLAN="{\"engine\":\"noise-tcp\",\"role\":\"serve\",\"direction\":\"direct\",\"node_id\":\"abroad\",\"peer_id\":\"iran\",\"tunnel_id\":\"pairdemo\",\"service_port\":18140,\"tunnel_port\":18141,\"dir\":\"/etc/baft-tunnel\",\"instance\":\"$NAME\"}"

step "2 tamper and scope refusals by the root helper (nothing written)"
before="$(sudo find /etc/baft-tunnel -mindepth 1 | sort)"
# Wait until the helper consumed the request and its one-shot unit finished.
helper_idle() {
  for n in $(seq 1 80); do
    ! sudo test -e "$CTL/$NAME.setup" && ! sudo test -L "$CTL/$NAME.setup" &&
      ! systemctl is-active --quiet "baft-tunnel-control-setup@$NAME.service" && return 0
    sleep 0.25
  done
  return 1
}
helper_result() { # wait for the helper's answer to a request the test dropped as the agent user
  for n in $(seq 1 40); do sudo test -f "$CTL/$NAME.setup-result" && ! sudo test -L "$CTL/$NAME.setup-result" && break; sleep 0.25; done
  helper_idle || die "the setup helper did not consume the request"
  sudo cat "$CTL/$NAME.setup-result" 2>/dev/null; sudo rm -f "$CTL/$NAME.setup-result"
}
echo '{"stage":"init","key":"tamper-0001","plan":{}}' >"$TMP/elsewhere.json"; chmod 644 "$TMP/elsewhere.json"
sudo -u baft-tunnel-agent ln -s "$TMP/elsewhere.json" "$CTL/$NAME.setup"
[[ "$(helper_result | field result)" == refused ]] || die "a symlinked request was not refused"
[[ ! -e "$CTL/$NAME.setup" ]] || die "the symlinked request was not removed"
for bad in "{\"stage\":\"init\",\"key\":\"tamper-0002\",\"plan\":${PLAN/\/etc\/baft-tunnel//tmp}}" \
           "{\"stage\":\"init\",\"key\":\"tamper-0003\",\"plan\":${PLAN/\"$NAME\"/\"other\"}}" \
           "{\"stage\":\"start\",\"key\":\"tamper-0004\",\"plan\":$PLAN}" \
           "{\"stage\":\"init\",\"key\":\"tamper-0005\",\"plan\":$PLAN,\"config\":{\"mode\":\"serve\"}}"; do
  printf '%s' "$bad" | sudo -u baft-tunnel-agent tee "$CTL/$NAME.setup" >/dev/null
  out="$(helper_result)"
  [[ "$(echo "$out" | field result)" == refused ]] || { echo "$out" >&2; die "request was not refused: $bad"; }
done
head -c 70000 /dev/zero | tr '\0' ' ' | sudo -u baft-tunnel-agent tee "$CTL/$NAME.setup" >/dev/null
[[ "$(helper_result | field result)" == refused ]] || die "an oversized request was not refused"
# A result link planted by a group member is replaced, never followed.
echo keep >"$TMP/victim"; chmod 644 "$TMP/victim"
sudo -u baft-tunnel-agent ln -s "$TMP/victim" "$CTL/$NAME.setup-result"
printf '%s' "{\"stage\":\"start\",\"key\":\"tamper-0006\",\"plan\":$PLAN}" | sudo -u baft-tunnel-agent tee "$CTL/$NAME.setup" >/dev/null
helper_result >/dev/null
[[ "$(cat "$TMP/victim")" == keep ]] || die "the helper wrote through a planted result link"
[[ "$(sudo find /etc/baft-tunnel -mindepth 1 | sort)" == "$before" ]] || die "a refused request changed /etc/baft-tunnel"
# The API refuses another directory before anything reaches root.
[[ "$(code POST /v1/setup/apply "{\"stage\":\"init\",\"confirm\":true,\"idempotency_key\":\"api-other-dir\",\"plan\":${PLAN/\/etc\/baft-tunnel//tmp}}")" == 422 ]] || die "API accepted another directory"

step "3 init through the agent API only (root helper), then export the own bundle"
out="$(api POST /v1/setup/preflight "$PLAN")"
echo "$out" | python3 -c 'import json,sys; c={x["id"]:x["result"] for x in json.load(sys.stdin)["checks"]}
assert c.get("setup_helper") == "pass" and "directory_writable" not in c, c' || { echo "$out" >&2; die "preflight does not use the setup helper"; }
out="$(api POST /v1/setup/apply "{\"stage\":\"init\",\"confirm\":true,\"idempotency_key\":\"pair-init-0001\",\"plan\":$PLAN}")"
[[ "$(echo "$out" | field result)" == applied ]] || { echo "$out" >&2; die "init through the panel was not applied"; }
[[ "$(echo "$out" | field source)" == "root setup helper" ]] || die "init did not go through the root helper"
[[ "$(sudo stat -c '%U:%G %a' /etc/baft-tunnel/node.noise)" == "baft-tunnel:baft-tunnel 600" ]] || die "node key owner/mode wrong"
if sudo -u baft-tunnel-agent cat /etc/baft-tunnel/node.noise >/dev/null 2>&1; then die "the agent user can read the node key"; fi
[[ "$(echo "$(api POST /v1/setup/apply "{\"stage\":\"init\",\"confirm\":true,\"idempotency_key\":\"pair-init-0001\",\"plan\":$PLAN}")" | field replayed)" == True ]] || die "same key was not replayed"
api GET /v1/setup/bundle >"$TMP/own.json"
own_ck="$(field checksum <"$TMP/own.json")"
[[ "$own_ck" == "$(echo "$out" | field pairing.checksum)" ]] || die "exported checksum differs from the receipt"
python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["bundle"])' "$TMP/own.json" >"$TMP/own-bundle.json"
sudo python3 -c 'import sys; k = open("/etc/baft-tunnel/node.noise").read().strip()
sys.exit(1 if len(k) >= 16 and k in open(sys.argv[1]).read() else 0)' "$TMP/own-bundle.json" || die "the exported bundle contains key material"

step "4 the peer prepares itself (other host, CLI); import preview with the checksum; join via the helper"
mkdir -p "$TMP/peer"; chmod 700 "$TMP/peer"
"$TMP/baft-tunnel" setup init --engine noise-tcp --role forward --direction direct --node-id iran --peer-id abroad \
  --tunnel-id pairdemo --service-port 18140 --tunnel-port 18141 --peer-host 127.0.0.1 --dir "$TMP/peer" >"$TMP/init-b.log"
cb="$(sed -n 's/^Checksum.*: //p' "$TMP/init-b.log")"
peer_json="$(python3 -c 'import json,sys; print(json.dumps(open(sys.argv[1]).read()))' "$TMP/peer/bundle.json")"
v="$(api POST /v1/setup/bundle/inspect "{\"bundle\":$peer_json,\"checksum\":\"00000000-00000000-00000000-00000000\"}")"
[[ "$(echo "$v" | field checksum_match)" == False ]] || die "a wrong checksum was not flagged"
v="$(api POST /v1/setup/bundle/inspect "{\"bundle\":$peer_json,\"checksum\":\"$cb\"}")"
[[ "$(echo "$v" | field checksum_match)" == True && "$(echo "$v" | field matches_this_node)" == True ]] || { echo "$v" >&2; die "import preview rejected the right bundle"; }
out="$(api POST /v1/setup/apply "{\"stage\":\"join\",\"confirm\":true,\"idempotency_key\":\"pair-join-0000\",\"plan\":$PLAN,\"bundle\":$peer_json,\"checksum\":\"00000000-00000000-00000000-00000000\"}")"
[[ "$(echo "$out" | field result)" != applied ]] || die "join with a wrong checksum was applied"
[[ ! -e "/etc/baft-tunnel/$NAME.json" ]] || die "a refused join wrote the configuration"
out="$(api POST /v1/setup/apply "{\"stage\":\"join\",\"confirm\":true,\"idempotency_key\":\"pair-join-0001\",\"plan\":$PLAN,\"bundle\":$peer_json,\"checksum\":\"$cb\"}")"
[[ "$(echo "$out" | field result)" == applied ]] || { echo "$out" >&2; die "join through the panel was not applied"; }
[[ "$(sudo stat -c '%U:%G %a' "/etc/baft-tunnel/$NAME.json")" == "root:baft-tunnel 640" ]] || die "instance config owner/mode wrong"
"$TMP/baft-tunnel" setup join --dir "$TMP/peer" --bundle "$TMP/own-bundle.json" --checksum "$own_ck" >"$TMP/join-b.log"

step "5 the agent leaves setup mode by itself and Start brings the tunnel up"
for n in $(seq 1 60); do
  s="$(api GET /v1/status 2>/dev/null || true)"
  [[ -n "$s" ]] && ! echo "$s" | grep -q '"setup_pending"' && echo "$s" | grep -q '"tunnel"' && break
  sleep 0.5
done
api GET /v1/status | grep -q '"setup_pending"' && die "agent did not reload after the configuration appeared"
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
S(("127.0.0.1", 18140), H).serve_forever()
PY
ECHO_PID=$!
out="$(api POST /v1/tunnel/lifecycle '{"action":"start","confirm":true,"idempotency_key":"pair-start-0001"}')"
[[ "$(echo "$out" | field result)" == applied ]] || { echo "$out" >&2; die "Start after panel pairing was not applied"; }
sudo systemctl is-active --quiet "baft-tunnel@$NAME.service" || die "tunnel not active after Start"
sudo python3 - "/var/lib/baft-tunnel-agent/$NAME/audit.log" <<'PY' || die "audit lines miss the panel setup stages"
import json, sys
res = [json.loads(l)["resource"] for l in open(sys.argv[1])]
assert "setup/init" in res and "setup/join" in res, res
PY
! sudo grep -rq -- "$TOKEN" /var/lib/baft-tunnel-agent/"$NAME" /var/lib/baft-tunnel-control/setup-receipts || die "receipts or audit contain the token"
sudo test -f "/var/lib/baft-tunnel-control/setup-receipts/$NAME/pair-join-0001.json" || die "the helper kept no receipt"

step "6 uninstall leaves nothing"
kill "$ECHO_PID" 2>/dev/null || true; ECHO_PID=""
sudo "$ROOT/scripts/uninstall.sh" --stop --purge-config --yes >"$TMP/uninstall.log"
INSTALLED=0
left="$(sudo find /etc/systemd/system -name 'baft-tunnel*' 2>/dev/null; sudo ls -d /var/lib/baft-tunnel-agent /etc/baft-tunnel "$CTL" 2>/dev/null || true)"
[[ -z "$left" ]] || { echo "$left" >&2; die "uninstall left files behind"; }

echo "PANEL_PAIRING=PASS (systemd $v: agent in setup mode after install, init and join through the agent API and the root setup helper only, service owners set, own bundle exported with checksum, peer bundle previewed and checksum-checked, wrong checksum refused, symlinked/oversized/other-dir/other-instance/other-stage/extra-field requests refused, planted result link not followed, agent reloaded by itself, Start applied, audit and receipts without the token, uninstall left nothing)"
