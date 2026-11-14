#!/usr/bin/env bash
# root `setup init/join --instance` with no manual chown, the agent started by its path unit under
# its own user, and Start / Restart / Stop through the agent API with boot persistence
# (`systemctl is-enabled`), refusals, diagnostics' next action, crash restart and a clean uninstall.
# Disposable: refuses to run if a BAFT install exists and removes everything it created.
# Needs root/sudo and systemd 247+ (Ubuntu 22.04 / 249).
set -Eeuo pipefail
umask 022
ROOT="$(cd "$(dirname "$0")/../.." && pwd -P)"
BIN="${1:-$ROOT/target/release/baft-tunnel}"
NAME=svcagent
API=127.0.0.1:18110
TMP="$(mktemp -d "${TMPDIR:-/tmp}/baft-agent-service.XXXXXX")"
CREATED_USER=0; CREATED_GROUP=0; CREATED_AGENT=0; INSTALLED=0; ECHO_PID=""
die() { echo "agent service lifecycle: $*" >&2; exit 1; }
step() { echo "[step] $*"; }
[[ -x "$BIN" ]] || die "missing executable: $BIN"
for c in systemctl python3 sudo curl setpriv; do command -v "$c" >/dev/null 2>&1 || die "missing command: $c"; done
v="$(systemctl --version | awk 'NR == 1 { print $2 }')"
[[ "$v" =~ ^[0-9]+$ ]] && (( v >= 247 )) || die "systemd 247+ required; found $v"
[[ "$(ps -p 1 -o comm= | tr -d '[:space:]')" == systemd ]] || die "PID 1 is not systemd"
for p in /usr/local/bin/baft-tunnel /etc/systemd/system/baft-tunnel@.service /etc/baft-tunnel /var/lib/baft-tunnel \
         /var/lib/baft-tunnel-agent /run/baft-tunnel/control /etc/systemd/system/baft-tunnel-agent@.service; do
  [[ ! -e "$p" ]] || die "refusing to overwrite pre-existing path: $p"
done
getent group baft-tunnel >/dev/null 2>&1 || CREATED_GROUP=1
id baft-tunnel >/dev/null 2>&1 || CREATED_USER=1
id baft-tunnel-agent >/dev/null 2>&1 || CREATED_AGENT=1
cleanup() {
  local status=$?
  trap - EXIT
  if [[ "$status" != 0 ]]; then
    sudo journalctl -u "baft-tunnel-agent@$NAME.service" -u "baft-tunnel@$NAME.service" -n 80 --no-pager >&2 2>/dev/null || true
  fi
  [[ -n "$ECHO_PID" ]] && kill "$ECHO_PID" >/dev/null 2>&1 || true
  sudo "$ROOT/scripts/install-agent-control.sh" --remove --instance "$NAME" >/dev/null 2>&1 || true
  sudo systemctl disable --now "baft-tunnel@$NAME.service" >/dev/null 2>&1 || true
  if [[ "$INSTALLED" == 1 ]]; then sudo "$ROOT/scripts/uninstall.sh" --stop --purge-config --yes >/dev/null 2>&1 || true; fi
  sudo rm -rf /etc/baft-tunnel /var/lib/baft-tunnel /var/lib/baft-tunnel-agent /run/baft-tunnel/control /var/lib/baft-tunnel-control
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

step "1 one-step install: binary, tunnel template, agent service, token, control helper"
sudo "$ROOT/scripts/install.sh" --binary "$TMP/baft-tunnel" --register "$NAME" --agent-listen "$API" >"$TMP/install.log"
INSTALLED=1
for u in "baft-tunnel-agent@$NAME.path" "baft-tunnel-control-enable@$NAME.path" "baft-tunnel-control-disable@$NAME.path" \
         "baft-tunnel-control-start@$NAME.path" "baft-tunnel-control-stop@$NAME.path"; do
  sudo systemctl is-enabled --quiet "$u" || die "$u is not enabled"
  sudo systemctl is-active --quiet "$u" || die "$u is not active"
done
sudo systemctl is-enabled --quiet "baft-tunnel-agent@$NAME.service" || die "agent service is not enabled at boot"
# Before any configuration the agent runs in setup mode (ADR-0026): pairing routes only.
for n in $(seq 1 60); do curl -fs "http://$API/v1/health" >/dev/null 2>&1 && break; sleep 0.5; done
sudo systemctl is-active --quiet "baft-tunnel-agent@$NAME.service" || die "agent is not running in setup mode after install"
TOK=/var/lib/baft-tunnel-agent/token
[[ "$(sudo stat -c '%U:%G %a' $TOK)" == "baft-tunnel-agent:baft-tunnel-agent 600" ]] || die "token owner/mode wrong"
[[ "$(sudo stat -c '%U %a' /var/lib/baft-tunnel-agent)" == "baft-tunnel-agent 700" ]] || die "agent state dir owner/mode wrong"
if sudo -u baft-tunnel cat "$TOK" >/dev/null 2>&1; then die "the tunnel user can read the agent token (SA-09)"; fi
id -nG baft-tunnel-agent | tr ' ' '\n' | grep -qx baft-tunnel || die "agent user is not in group baft-tunnel"
if id -nG baft-tunnel | tr ' ' '\n' | grep -qx baft-tunnel-agent; then die "tunnel user is in the agent group"; fi
[[ "$(sudo stat -c '%U:%G %a' /run/baft-tunnel/control)" == "root:baft-tunnel-agent 770" ]] || die "control dir is not root:baft-tunnel-agent 0770"

step "2 setup init/join as root sets owners and modes itself (no chown/chgrp)"
mkdir -p "$TMP/peer"; chmod 700 "$TMP/peer"
sudo /usr/local/bin/baft-tunnel setup init --engine noise-tcp --role serve --direction direct --node-id abroad --peer-id iran \
  --tunnel-id svcdemo --service-port 18120 --tunnel-port 18121 --instance "$NAME" >"$TMP/init-a.log"
"$TMP/baft-tunnel" setup init --engine noise-tcp --role forward --direction direct --node-id iran --peer-id abroad \
  --tunnel-id svcdemo --service-port 18120 --tunnel-port 18121 --peer-host 127.0.0.1 --dir "$TMP/peer" >"$TMP/init-b.log"
ca="$(sed -n 's/^Checksum.*: //p' "$TMP/init-a.log")"; cb="$(sed -n 's/^Checksum.*: //p' "$TMP/init-b.log")"
[[ -n "$ca" && -n "$cb" ]] || die "no checksum printed"
sudo install -m 0644 "$TMP/peer/bundle.json" "$TMP/peer-bundle-b.json"
sudo /usr/local/bin/baft-tunnel setup join --bundle "$TMP/peer-bundle-b.json" --checksum "$cb" >"$TMP/join-a.log"
sudo cat /etc/baft-tunnel/bundle.json >"$TMP/bundle-a.json"
"$TMP/baft-tunnel" setup join --dir "$TMP/peer" --bundle "$TMP/bundle-a.json" --checksum "$ca" >"$TMP/join-b.log"
[[ "$(sudo stat -c '%U:%G %a' /etc/baft-tunnel/node.noise)" == "baft-tunnel:baft-tunnel 600" ]] || die "node key not handed to the service user"
[[ "$(sudo stat -c '%U:%G %a' "/etc/baft-tunnel/$NAME.json")" == "root:baft-tunnel 640" ]] || die "instance config not group-readable by the service"
if sudo -u baft-tunnel-agent cat /etc/baft-tunnel/node.noise >/dev/null 2>&1; then die "the agent user can read the tunnel key"; fi

step "3 the agent reloads by itself once the configuration exists (setup mode ends), as baft-tunnel-agent, loopback only, panel served"
TOKEN="$(sudo cat "$TOK")"
for n in $(seq 1 60); do
  curl -fs "http://$API/v1/status" -H "Authorization: Bearer $TOKEN" 2>/dev/null | grep -q '"tunnel"' &&
    ! curl -fs "http://$API/v1/status" -H "Authorization: Bearer $TOKEN" 2>/dev/null | grep -q '"setup_pending"' && break
  sleep 0.5
done
curl -fs "http://$API/v1/status" -H "Authorization: Bearer $TOKEN" | grep -q '"setup_pending"' && die "agent did not leave setup mode after the configuration appeared"
[[ "$(sudo systemctl show -p User --value "baft-tunnel-agent@$NAME.service")" == baft-tunnel-agent ]] || die "agent unit user"
apid="$(sudo systemctl show -p MainPID --value "baft-tunnel-agent@$NAME.service")"
[[ "$(ps -o user= -p "$apid" | tr -d ' ')" == baft-tunnel-agent ]] || die "agent process does not run as baft-tunnel-agent"
ss -ltnH | awk '{print $4}' | grep -qx "$API" || die "agent is not listening on $API"
if ss -ltnH | awk '{print $4}' | grep -qE '^(0\.0\.0\.0|\*|\[::\]):18110$'; then die "agent listens off loopback"; fi
curl -fs "http://$API/" | grep -qi '<html' || die "panel is not served"
api() { curl -s --max-time 90 -X "$1" "http://$API$2" -H "Authorization: Bearer $TOKEN" ${3:+-d "$3"}; }
life() { api POST /v1/tunnel/lifecycle "{\"action\":\"$1\",\"confirm\":true,\"idempotency_key\":\"$2\"}"; }
field() { python3 -c 'import sys,json; d=json.load(sys.stdin)
for k in sys.argv[1].split("."): d = d.get(k, "") if isinstance(d, dict) else ""
print(d)' "$1"; }
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
S(("127.0.0.1", 18120), H).serve_forever()
PY
ECHO_PID=$!

step "4 Start: running, verified, enabled at boot"
! sudo systemctl is-enabled --quiet "baft-tunnel@$NAME.service" || die "tunnel enabled before Start"
out="$(life start svc-start-0001)"
[[ "$(echo "$out" | field result)" == applied ]] || { echo "$out" >&2; die "Start was not applied"; }
[[ "$(echo "$out" | field boot.enabled_at_boot)" == True ]] || { echo "$out" >&2; die "receipt does not state boot persistence"; }
sudo systemctl is-active --quiet "baft-tunnel@$NAME.service" || die "unit not active after Start"
[[ "$(sudo systemctl is-enabled "baft-tunnel@$NAME.service")" == enabled ]] || die "unit not enabled at boot after Start"
[[ "$(echo "$out" | field ready)" == unknown ]] || die "Start claimed ready"
[[ "$(sudo stat -c '%a' "/run/baft-tunnel/$NAME")" == 750 ]] || die "runtime dir is not 0750"
[[ "$(echo "$(life start svc-start-0001)" | field replayed)" == True ]] || die "same key was not replayed"
out="$(life start svc-start-0002)"
[[ "$(echo "$out" | field result)" == refused ]] || { echo "$out" >&2; die "Start of a running unit was not refused"; }
api GET /v1/status >"$TMP/status.json"
python3 - "$TMP/status.json" <<'PY' || { cat "$TMP/status.json" >&2; die "status does not report the unit and boot state"; }
import json, sys
t = json.load(open(sys.argv[1]))["tunnel"]
assert '"enabled_at_boot": true' in json.dumps(t), t
PY

step "5 Restart: new process, still enabled"
pid1="$(sudo systemctl show -p MainPID --value "baft-tunnel@$NAME.service")"
out="$(life restart svc-restart-01)"
[[ "$(echo "$out" | field result)" == applied ]] || { echo "$out" >&2; die "Restart was not applied"; }
pid2="$(sudo systemctl show -p MainPID --value "baft-tunnel@$NAME.service")"
[[ "$pid2" != "$pid1" && "$pid2" != 0 ]] || die "Restart did not start a new process ($pid1 -> $pid2)"
[[ "$(sudo systemctl is-enabled "baft-tunnel@$NAME.service")" == enabled ]] || die "Restart lost boot persistence"

step "6 agent crash: systemd brings it back"
sudo kill -KILL "$apid"
for n in $(seq 1 40); do
  np="$(sudo systemctl show -p MainPID --value "baft-tunnel-agent@$NAME.service")"
  [[ "$np" != 0 && "$np" != "$apid" ]] && curl -fs "http://$API/v1/health" >/dev/null 2>&1 && break
  sleep 0.5
done
curl -fs "http://$API/v1/health" >/dev/null || die "agent did not come back after a crash"

step "7 Stop: down and disabled at boot; second Stop refused; diagnostics say Start"
out="$(life stop svc-stop-00001)"
[[ "$(echo "$out" | field result)" == applied ]] || { echo "$out" >&2; die "Stop was not applied"; }
! sudo systemctl is-active --quiet "baft-tunnel@$NAME.service" || die "unit still active after Stop"
[[ "$(sudo systemctl is-enabled "baft-tunnel@$NAME.service" || true)" == disabled ]] || die "unit still enabled after Stop"
[[ "$(echo "$(life stop svc-stop-00002)" | field result)" == refused ]] || die "Stop of a stopped unit was not refused"
[[ "$(echo "$(life restart svc-restart-02)" | field result)" == refused ]] || die "Restart of a stopped unit was not refused"
api POST /v1/diagnostics >"$TMP/diag.json"
[[ "$(field next_action.action <"$TMP/diag.json")" == start ]] || { cat "$TMP/diag.json" >&2; die "diagnostics did not name Start as the next action"; }
# Malformed requests change nothing.
for bad in '{"action":"init","confirm":true,"idempotency_key":"svc-bad-0001"}' '{"action":"stop","confirm":false,"idempotency_key":"svc-bad-0002"}'; do
  [[ "$(curl -s -o /dev/null -w '%{http_code}' -X POST "http://$API/v1/tunnel/lifecycle" -H "Authorization: Bearer $TOKEN" -d "$bad")" == 422 ]] || die "bad request was not 422: $bad"
done
[[ "$(curl -s -o /dev/null -w '%{http_code}' -X POST "http://$API/v1/tunnel/lifecycle" -d '{}')" == 401 ]] || die "lifecycle without token was not 401"
sudo python3 - "/var/lib/baft-tunnel-agent/$NAME/audit.log" <<'PY' || die "audit lines miss the lifecycle actions"
import json, sys
lines = [json.loads(l) for l in open(sys.argv[1])]
res = [l["resource"] for l in lines if l["action"] == "lifecycle"]
assert {"tunnel/start", "tunnel/restart", "tunnel/stop"} <= set(res), res
PY
! sudo grep -rq -- "$TOKEN" /var/lib/baft-tunnel-agent/"$NAME" || die "agent state contains the token"

step "7b upgrade an existing pre-boundary install through update.sh (H-01)"
# Recreate the pre-PR #199 boundary: request directory group baft-tunnel, old tmpfiles entry.
CTL=/run/baft-tunnel/control
printf 'd /var/lib/baft-tunnel-control 0700 root root -\nd %s 0770 root baft-tunnel -\n' "$CTL" | sudo tee /etc/tmpfiles.d/baft-tunnel-control.conf >/dev/null
sudo chown root:baft-tunnel "$CTL"; sudo chmod 0770 "$CTL"
[[ "$(sudo stat -c '%U:%G %a' "$CTL")" == "root:baft-tunnel 770" ]] || die "could not recreate the old boundary"
sudo setpriv --reuid=baft-tunnel --regid=baft-tunnel --init-groups touch "$CTL/h01-old-probe" || die "old boundary should still let the tunnel user write (baseline)"
sudo rm -f "$CTL/h01-old-probe"
sudo "$ROOT/scripts/update.sh" --binary "$TMP/baft-tunnel" --health-secs 2 >"$TMP/update1.log" 2>&1 || { cat "$TMP/update1.log" >&2; die "update.sh failed on the upgrade path"; }
grep -q "^UPDATE_OK .* control_boundary=root:baft-tunnel-agent-0770$" "$TMP/update1.log" || { cat "$TMP/update1.log" >&2; die "update.sh did not report the migrated boundary"; }
[[ "$(sudo stat -c '%U:%G %a' "$CTL")" == "root:baft-tunnel-agent 770" ]] || die "update.sh did not migrate the control directory"
[[ "$(sudo stat -c '%U:%G %a' /run/baft-tunnel)" == "root:root 755" ]] || die "control parent is not root:root 0755 after update"
sudo grep -qx "d $CTL 0770 root baft-tunnel-agent -" /etc/tmpfiles.d/baft-tunnel-control.conf || die "update.sh did not refresh the tmpfiles entry"
[[ ! -e /var/lib/baft-tunnel-control/migration-v1.units ]] || die "migration journal left after a successful update"
if sudo setpriv --reuid=baft-tunnel --regid=baft-tunnel --init-groups touch "$CTL/h01-denied" 2>/dev/null; then die "tunnel user can still write requests after update"; fi
sudo setpriv --reuid=baft-tunnel-agent --regid=baft-tunnel-agent --init-groups touch "$CTL/h01-agent-probe" || die "agent user cannot write requests after update"
sudo rm -f "$CTL/h01-agent-probe"
# States preserved: agent and control paths active and enabled, the stopped tunnel stays stopped.
sudo systemctl is-active --quiet "baft-tunnel-agent@$NAME.service" || die "agent not active after update"
for u in "baft-tunnel-agent@$NAME.path" "baft-tunnel-control-start@$NAME.path" "baft-tunnel-control-stop@$NAME.path"; do
  sudo systemctl is-active --quiet "$u" || die "$u not active after update"
done
! sudo systemctl is-active --quiet "baft-tunnel@$NAME.service" || die "stopped tunnel was started by update"
for n in $(seq 1 60); do curl -fs "http://$API/v1/health" >/dev/null 2>&1 && break; sleep 0.5; done
curl -fs "http://$API/v1/health" >/dev/null || die "agent API not healthy after update"
# Idempotent: a second update keeps the boundary and changes no state.
sudo "$ROOT/scripts/update.sh" --binary "$TMP/baft-tunnel" --health-secs 2 >"$TMP/update2.log" 2>&1 || { cat "$TMP/update2.log" >&2; die "second update.sh failed"; }
grep -q "^UPDATE_OK .* control_boundary=root:baft-tunnel-agent-0770$" "$TMP/update2.log" || die "second update did not report the boundary"
# The update's root-only backups belong to this disposable test; remove exactly those it named.
for b in $(sed -n 's/^\(Data\|Binary\) backup: //p' "$TMP/update1.log" "$TMP/update2.log"); do
  if [[ "$b" == /var/backups/baft-tunnel-* ]]; then sudo rm -rf -- "$b"; fi
done
! sudo systemctl is-active --quiet "baft-tunnel@$NAME.service" || die "second update started the stopped tunnel"
for n in $(seq 1 60); do curl -fs "http://$API/v1/health" >/dev/null 2>&1 && break; sleep 0.5; done
echo "BAFT_AGENT_CONTROL_UPGRADE_TEST=PASS (update.sh migrated root:baft-tunnel 770 -> root:baft-tunnel-agent 770, tunnel user denied, agent allowed, stopped tunnel stayed stopped, idempotent rerun)"

step "8 Start again so uninstall must stop it; uninstall leaves nothing"
[[ "$(echo "$(life start svc-start-0003)" | field result)" == applied ]] || die "second Start failed"
kill "$ECHO_PID" 2>/dev/null || true; ECHO_PID=""
sudo "$ROOT/scripts/uninstall.sh" --stop --purge-config --yes >"$TMP/uninstall.log"
INSTALLED=0
! sudo systemctl is-active --quiet "baft-tunnel@$NAME.service" || die "uninstall left the tunnel running"
! sudo systemctl is-active --quiet "baft-tunnel-agent@$NAME.service" || die "uninstall left the agent running"
left="$(sudo find /etc/systemd/system -name 'baft-tunnel*' 2>/dev/null; sudo ls -d /var/lib/baft-tunnel-agent /etc/baft-tunnel /run/baft-tunnel/control 2>/dev/null || true)"
[[ -z "$left" ]] || { echo "$left" >&2; die "uninstall left files behind"; }
if ss -ltnH | awk '{print $4}' | grep -qE ':(18110|18121)$'; then die "a listener survived uninstall"; fi

echo "AGENT_SERVICE_LIFECYCLE=PASS (systemd $v: install.sh --register in one step, agent as baft-tunnel-agent in setup mode until setup join wrote the config, then reloaded by itself, no manual chown, token unreadable by the tunnel user, panel on loopback, Start enabled at boot and verified, Restart new PID, Stop disabled at boot, refusals, diagnostics next action Start, agent crash restarted, audit lines, uninstall left nothing)"
