#!/usr/bin/env bash
# Real systemd acceptance for ADR-0051 Hub identity provisioning and per-instance LoadCredential.
# Disposable: refuses an existing BAFT install and removes only what it creates.
# Run as root on Ubuntu 22.04/systemd 249+ with the built release binary.
set -Eeuo pipefail
umask 077
ROOT="$(cd "$(dirname "$0")/../.." && pwd -P)"
BIN="${1:-$ROOT/target/release/baft-tunnel}"
NAME="fh$(printf '%x' "$RANDOM$BASHPID")"
HUB_NAME="hub-$NAME"
API=127.0.0.1:18117
PORT="$(python3 - <<'PY'
import socket
s=socket.socket(); s.bind(('127.0.0.1',0)); print(s.getsockname()[1]); s.close()
PY
)"
LISTEN="127.0.0.1:$PORT"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/baft-fleet-hub.XXXXXX")"
INSTALLED=0
CREATED_USER=0; CREATED_GROUP=0; CREATED_AGENT=0; CREATED_AGENT_GROUP=0
getent group baft-tunnel >/dev/null 2>&1 || CREATED_GROUP=1
id baft-tunnel >/dev/null 2>&1 || CREATED_USER=1
id baft-tunnel-agent >/dev/null 2>&1 || CREATED_AGENT=1
getent group baft-tunnel-agent >/dev/null 2>&1 || CREATED_AGENT_GROUP=1
DROPIN="/etc/systemd/system/baft-tunnel-agent@$NAME.service.d/fleet-hub.conf"
fail() { echo "fleet Hub systemd test: $*" >&2; exit 1; }
step() { echo "[step] $*"; }
[[ "$(id -u)" == 0 ]] || fail "must run as root"
[[ -x "$BIN" ]] || fail "missing executable: $BIN"
for c in systemctl journalctl python3 curl runuser setpriv openssl timeout ss stat; do command -v "$c" >/dev/null 2>&1 || fail "missing command: $c"; done
[[ "$(ps -p 1 -o comm= | tr -d '[:space:]')" == systemd ]] || fail "PID 1 is not systemd"
for p in /usr/local/bin/baft-tunnel /usr/local/lib/baft-tunnel /usr/local/lib/baft-tunnel-engines \
         /etc/baft-tunnel /var/lib/baft-tunnel /var/lib/baft-tunnel-agent \
         /var/lib/baft-tunnel-control /run/baft-tunnel/control /etc/tmpfiles.d/baft-tunnel-control.conf \
         /etc/systemd/system/baft-tunnel@.service /etc/systemd/system/baft-tunnel-agent@.service \
         /etc/systemd/system/baft-tunnel-agent@.path "$DROPIN" "$(dirname "$DROPIN")"; do
  [[ ! -e "$p" && ! -L "$p" ]] || fail "refusing to overwrite pre-existing path: $p"
done
for p in /etc/systemd/system/baft-tunnel-control-*@.service /etc/systemd/system/baft-tunnel-control-*@.path; do
  [[ ! -e "$p" && ! -L "$p" ]] || fail "refusing to overwrite pre-existing control unit: $p"
done
for p in /etc/systemd/system/baft-tunnel-agent@*.service.d/fleet-hub.conf \
         /etc/systemd/system/baft-tunnel@*.service.d/credentials.conf; do
  [[ ! -e "$p" && ! -L "$p" ]] || fail "refusing to remove pre-existing managed-looking drop-in: $p"
done
for p in /etc/systemd/system/multi-user.target.wants/baft-tunnel*; do
  [[ ! -e "$p" && ! -L "$p" ]] || fail "refusing to overwrite pre-existing enabled unit link: $p"
done
cleanup() {
  local status=$?
  trap - EXIT
  if [[ "$status" != 0 ]]; then
    journalctl -u "baft-tunnel-control-fleet@$NAME.service" -u "baft-tunnel-agent@$NAME.service" -n 100 --no-pager >&2 2>/dev/null || true
  fi
  systemctl disable --now "baft-tunnel-agent@$NAME.path" "baft-tunnel-agent@$NAME.service" \
    "baft-tunnel-control-fleet@$NAME.path" >/dev/null 2>&1 || true
  systemctl stop "baft-tunnel-control-fleet@$NAME.service" >/dev/null 2>&1 || true
  if [[ "$INSTALLED" == 1 ]]; then
    "$ROOT/scripts/install-agent-control.sh" --remove --instance "$NAME" >/dev/null 2>&1 || true
    "$ROOT/scripts/uninstall.sh" --stop --purge-config --yes >/dev/null 2>&1 || true
  fi
  rm -rf /etc/baft-tunnel /var/lib/baft-tunnel /var/lib/baft-tunnel-agent /run/baft-tunnel/control /var/lib/baft-tunnel-control
  rm -f /usr/local/bin/baft-tunnel /etc/systemd/system/baft-tunnel@.service \
    /etc/systemd/system/baft-tunnel-agent@.service /etc/systemd/system/baft-tunnel-agent@.path \
    /etc/systemd/system/baft-tunnel-control-*@.service /etc/systemd/system/baft-tunnel-control-*@.path \
    /etc/systemd/system/multi-user.target.wants/baft-tunnel* /etc/tmpfiles.d/baft-tunnel-control.conf
  rm -f "$DROPIN"
  rmdir "$(dirname "$DROPIN")" 2>/dev/null || true
  systemctl daemon-reload >/dev/null 2>&1 || true
  if [[ "$CREATED_AGENT" == 1 ]]; then userdel baft-tunnel-agent >/dev/null 2>&1 || true; fi
  if [[ "$CREATED_AGENT_GROUP" == 1 ]]; then groupdel baft-tunnel-agent >/dev/null 2>&1 || true; fi
  if [[ "$CREATED_USER" == 1 ]]; then userdel baft-tunnel >/dev/null 2>&1 || true; fi
  if [[ "$CREATED_GROUP" == 1 ]]; then groupdel baft-tunnel >/dev/null 2>&1 || true; fi
  rm -rf "$TMP"
  exit "$status"
}
trap cleanup EXIT
trap 'exit 143' TERM
chmod 755 "$TMP"

step "1 install the Agent and bounded Fleet helper on a disposable instance"
install -m 0755 "$BIN" "$TMP/baft-tunnel"
"$ROOT/scripts/install.sh" --binary "$TMP/baft-tunnel" --register "$NAME" --agent-listen "$API" >"$TMP/install.log"
INSTALLED=1
for unit in "baft-tunnel-agent@$NAME.service" "baft-tunnel-control-fleet@$NAME.path"; do
  systemctl is-active --quiet "$unit" || fail "$unit is not active after install"
done
for n in $(seq 1 40); do curl -fs "http://$API/v1/health" >/dev/null 2>&1 && break; sleep 0.25; done
curl -fs "http://$API/v1/health" >/dev/null || fail "setup-mode Agent API did not start"

step "2 initialize the explicit Fleet CA and provision a Hub through the authenticated Agent API"
"$BIN" fleet ca init --confirm --name "BAFT Fleet Hub Test CA" >"$TMP/ca-init.log"
[[ "$(stat -c '%U:%G %a' /etc/baft-tunnel/fleet-ca)" == 'root:root 711' ]] || fail "Fleet CA directory must be root:root 0711"
[[ "$(stat -c '%U:%G %a' /etc/baft-tunnel/fleet-ca/ca.key)" == 'root:root 600' ]] || fail "Fleet CA private key must remain root:root 0600"
for public in ca.crt ca.crl; do
  [[ "$(stat -c '%U:%G %a' "/etc/baft-tunnel/fleet-ca/$public")" == 'root:root 644' ]] || fail "Fleet CA $public must be public root:root 0644"
done
if runuser -u baft-tunnel-agent -- cat /etc/baft-tunnel/fleet-ca/ca.key >/dev/null 2>&1; then fail "Agent can read the Fleet CA private key"; fi
KEY="/etc/baft-tunnel/fleet-hub/$NAME/hub-key"
CERT="/etc/baft-tunnel/fleet-hub/$NAME/hub.crt"
CONFIG="/etc/baft-tunnel/fleet/$NAME.json"
python3 - "$HUB_NAME" "$LISTEN" >"$TMP/request.json" <<'PY'
import json, sys
print(json.dumps({"op":"provision_hub","hub_name":sys.argv[1],"dial_san":"DNS:localhost","listen":sys.argv[2]}))
PY
TOKEN="$(cat /var/lib/baft-tunnel-agent/token)"
HTTP="$(curl -sS -o "$TMP/provision-result.json" -w '%{http_code}' -X POST -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' --data-binary @"$TMP/request.json" "http://$API/v1/fleet/hub/provision")"
[[ "$HTTP" == 202 ]] || fail "Hub provision API returned HTTP $HTTP: $(cat "$TMP/provision-result.json")"
python3 - "$TMP/provision-result.json" <<'PYJSON' || fail "Agent API did not return a complete Hub identity"
import json, sys
v=json.load(open(sys.argv[1]))
assert v.get('state') == 'done', v
assert v.get('credential_id') == 'fleet-hub-key', v
assert v.get('restart_required') is True, v
assert 'BEGIN CERTIFICATE' in v.get('certificate',''), v
assert 'PRIVATE KEY' not in json.dumps(v), 'private key leaked into result'
PYJSON
HTTP="$(curl -sS -o "$TMP/fleet-before-restart.json" -w '%{http_code}' -H "Authorization: Bearer $TOKEN" "http://$API/v1/fleet/nodes")"
[[ "$HTTP" == 200 ]] || fail "Fleet status API returned HTTP $HTTP"
python3 - "$TMP/fleet-before-restart.json" <<'PYJSON' || fail "Fleet status did not report pending Hub activation"
import json, sys
hub=json.load(open(sys.argv[1])).get('hub', {})
assert hub.get('configured') is True and hub.get('restart_required') is True and hub.get('listener') is False, hub
PYJSON

step "3 verify root-only key, public certificate, per-instance config and sandboxed drop-in"
[[ "$(stat -c '%U:%G %a' "$KEY")" == 'root:root 600' ]] || fail "Hub key is not root:root 0600"
[[ "$(stat -c '%U:%G %a' "$CERT")" == 'root:root 644' ]] || fail "Hub certificate is not root:root 0644"
[[ "$(stat -c '%a' "$(dirname "$KEY")")" == 711 ]] || fail "Hub identity directory is not 0711"
[[ "$(stat -c '%U:%G %a' "$CONFIG")" == 'root:root 644' ]] || fail "Fleet config is not root:root 0644"
[[ "$(stat -c '%U:%G %a' "$DROPIN")" == 'root:root 644' ]] || fail "Agent drop-in is not root:root 0644"
grep -Fqx 'LoadCredential=fleet-hub-key:/etc/baft-tunnel/fleet-hub/%i/hub-key' "$DROPIN" || fail "LoadCredential is not instance-scoped"
if grep -q '^ExecStart=' "$DROPIN"; then fail "Hub drop-in must not replace the Agent ExecStart"; fi
if runuser -u baft-tunnel-agent -- cat "$KEY" >/dev/null 2>&1; then fail "Agent can read the root-held credential source"; fi
if grep -q 'PRIVATE KEY' "$TMP/provision-result.json" "$CONFIG" "$DROPIN"; then fail "private key leaked into result/config/drop-in"; fi
systemctl is-active --quiet "baft-tunnel-agent@$NAME.service" || fail "helper unexpectedly stopped the running Agent"

step "4 activate through the authenticated Agent API and verify systemd delivered the per-instance credential"
HTTP="$(curl -sS -o "$TMP/restart-result.json" -w '%{http_code}' -X POST -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' --data '{}' "http://$API/v1/fleet/hub/restart" || true)"
if [[ "$HTTP" != 202 ]]; then
  sleep 1
  systemctl is-active --quiet "baft-tunnel-agent@$NAME.service" || fail "Hub activation request was not accepted (HTTP ${HTTP:-none})"
fi
for n in $(seq 1 60); do
  systemctl is-active --quiet "baft-tunnel-agent@$NAME.service" &&
    journalctl -u "baft-tunnel-agent@$NAME.service" --no-pager -n 80 | grep -Fq "Fleet hub listening on $LISTEN" && break
  sleep 0.25
done
systemctl is-active --quiet "baft-tunnel-agent@$NAME.service" || fail "Agent failed after Fleet config activation"
PID="$(systemctl show -p MainPID --value "baft-tunnel-agent@$NAME.service")"
[[ "$PID" =~ ^[1-9][0-9]*$ ]] || fail "Agent has no main PID"
CREDENTIALS_DIR="$(tr '\0' '\n' <"/proc/$PID/environ" | sed -n 's/^CREDENTIALS_DIRECTORY=//p')"
[[ -n "$CREDENTIALS_DIR" && -f "$CREDENTIALS_DIR/fleet-hub-key" ]] || fail "systemd did not deliver fleet-hub-key"
cmp -s "$KEY" "$CREDENTIALS_DIR/fleet-hub-key" || fail "delivered credential differs from the root-held key"
curl -fs "http://$API/v1/health" >/dev/null || fail "Agent API did not recover after restart"
ss -ltnH | awk '{print $4}' | grep -qx "$LISTEN" || fail "Fleet Hub listener is not bound to the configured address"

step "5 verify the Hub report listener rejects a client without a certificate"
if timeout 5 openssl s_client -connect "$LISTEN" -servername localhost -CAfile /etc/baft-tunnel/fleet-ca/ca.crt -verify_return_error </dev/null >"$TMP/no-client-cert.log" 2>&1; then
  grep -Eqi 'certificate required|peer did not return a certificate' "$TMP/no-client-cert.log" || fail "Hub accepted a client without a certificate"
else
  grep -Eqi 'certificate required|peer did not return a certificate' "$TMP/no-client-cert.log" || fail "Hub TLS failed for an unexpected reason: $(tail -n 8 "$TMP/no-client-cert.log" | tr '\n' ' ')"
fi

echo "FLEET_HUB_CREDENTIAL_SYSTEMD=PASS (root-generated Hub key, server-only certificate, no key in agent-readable files/results, per-instance LoadCredential, real systemd daemon-reload and restart, Fleet listener active, mandatory client certificate)"
