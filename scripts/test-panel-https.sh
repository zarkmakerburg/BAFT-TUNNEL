#!/usr/bin/env bash
# Integration test for scripts/panel-https.sh on a disposable Ubuntu host/runner (needs root + nginx + openssl).
# A stub agent mimics the real agent's contract (loopback Host check, /v1/health open, /v1/status 401 without token).
# systemctl and certbot are shimmed: this proves nginx config, own-CA issuance, auth pass-through, idempotence and
# rollback. It does NOT prove a real Let's Encrypt issuance (needs a public domain) - see ADR-0033.
set -Eeuo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd -P)"
SCRIPT="$ROOT/scripts/panel-https.sh"
[[ "$(id -u)" == 0 ]] || { echo "needs root" >&2; exit 1; }
pkill -x nginx 2>/dev/null && sleep 1; rm -f /run/nginx.pid
T="$(mktemp -d)"; BIN="$T/bin"; mkdir "$BIN"
AGENT_PORT=18999; HP=8443
IP="$(ip -4 -o addr show scope global 2>/dev/null | awk '{sub(/\/.*/,"",$4); print $4; exit}')"
[[ -n "$IP" ]] || IP="$(hostname -I | awk '{print $1}')"
[[ -n "$IP" ]] || { echo "no non-loopback IPv4 address"; exit 1; }
cleanup() {
  rm -f /run/nginx.pid.stale
  [[ -z "${AGENT_PID:-}" ]] || kill "$AGENT_PID" 2>/dev/null || true
  PATH="$BIN:$PATH" bash "$SCRIPT" --remove >/dev/null 2>&1 || true
  nginx -s quit 2>/dev/null || true; sleep 0.5; rm -f /run/nginx.pid
  sed -i '/panel.baft.test/d' /etc/hosts 2>/dev/null || true
  rm -rf "$T" /etc/baft-tunnel-https /var/lib/baft-tunnel-https /etc/letsencrypt/live/baft-tunnel-panel
}
trap cleanup EXIT
cat >"$BIN/systemctl" <<'SH'
#!/bin/sh
case "$*" in
  "is-active --quiet nginx") pgrep -x nginx >/dev/null ;;
  "reload nginx") nginx -s reload ;;
  "enable --now nginx") nginx >/dev/null 2>&1 </dev/null ;;
  *) exit 0 ;;
esac
SH
cat >"$BIN/certbot" <<'SH'
#!/bin/sh
# test shim: fabricate the files certbot would write
case "$1" in certonly) ;; *) exit 0 ;; esac
d=/etc/letsencrypt/live/baft-tunnel-panel; mkdir -p "$d"
openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes -keyout "$d/privkey.pem" -out "$d/fullchain.pem" \
  -subj /CN=panel.baft.test -days 2 2>/dev/null
SH
chmod +x "$BIN"/*
head -c 24 /dev/urandom | od -An -tx1 | tr -d ' \n' >"$T/token"; chmod 600 "$T/token"
cat >"$T/agent.py" <<'PY'
import http.server, sys
class H(http.server.BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def do_GET(self):
        host = (self.headers.get("Host") or "").rsplit(":", 1)[0]
        if host not in ("127.0.0.1", "localhost", "[::1]"):
            self.send_response(421); self.end_headers(); return
        if self.path == "/v1/health": code, body = 200, b'{"ok":true}'
        elif self.path == "/": code, body = 200, b"<html>panel</html>"
        else: code, body = 401, b'{"error":"unauthorized"}'
        self.send_response(code); self.end_headers(); self.wfile.write(body)
http.server.HTTPServer(("127.0.0.1", int(sys.argv[1])), H).serve_forever()
PY
start_agent() { python3 "$T/agent.py" "$AGENT_PORT" & AGENT_PID=$!; sleep 0.5; }
stop_agent() { kill "$AGENT_PID"; wait "$AGENT_PID" 2>/dev/null || true; AGENT_PID=""; }
run() { PATH="$BIN:$PATH" bash ${PANEL_TRACE:+-x} "$SCRIPT" --skip-apt --token-file "$T/token" --agent-listen "127.0.0.1:$AGENT_PORT" --https-port "$HP" "$@"; }
fail() { echo "FAIL: $*" >&2; exit 1; }

# validation (no changes made)
run --domain 'Bad_Domain' >/dev/null 2>&1 && fail "invalid domain accepted"
run --ip 127.0.0.1 >/dev/null 2>&1 && fail "loopback IP accepted"
run --ip 169.254.1.1 >/dev/null 2>&1 && fail "link-local IP accepted"
run >/dev/null 2>&1 && fail "missing host accepted"
[[ ! -e /etc/nginx/conf.d/baft-tunnel-panel.conf ]] || fail "validation failures left a config behind"

rm -f /etc/nginx/sites-enabled/default   # sandbox has no IPv6; the distro default site listens on [::]:80
start_agent
# 1. IP mode, own CA
OUT="$(run --ip "$IP")" || { echo "$OUT"; fail "IP install"; }
grep -q "https://$IP:$HP/" <<<"$OUT" || fail "link not printed"
grep -q 'NOT a public CA' <<<"$OUT" || fail "own-CA warning missing"
grep -q "$(cat "$T/token")" <<<"$OUT" && fail "token printed to a non-terminal"
[[ "$(cat /var/lib/baft-tunnel-https/link)" == "https://$IP:$HP/" ]] || fail "link state file wrong"
grep -q "$(cat "$T/token")" /var/lib/baft-tunnel-https/link && fail "token leaked into link file"
CA=/etc/baft-tunnel-https/ca.crt
GATE="$(cat /var/lib/baft-tunnel-https/gate)"
[[ "$GATE" =~ ^[A-Za-z0-9_-]{16}$ ]] || fail "no web path"
grep -q "$GATE" <<<"$OUT" && fail "web path printed to a non-terminal"
ncurl() { command curl -sS --noproxy "*" --max-time 10 --cacert "$CA" --connect-to "$IP:$HP:127.0.0.1:$HP" "$@"; }
curl() { ncurl -H "Cookie: baft_gate=$GATE" "$@"; }
[[ "$(ncurl -o /dev/null -w '%{http_code}' https://$IP:$HP/v1/health)" == 404 ]] || fail "panel reachable without the web path"
[[ "$(ncurl -o /dev/null -w '%{http_code}' https://$IP:$HP/)" == 404 ]] || fail "panel page reachable without the web path"
ncurl -o /dev/null -D - "https://$IP:$HP/$GATE/" | tr -d '\r' | grep -qi "^set-cookie: baft_gate=$GATE;.*HttpOnly" || fail "web path does not set the cookie"
grep -q "$GATE" /var/log/nginx/baft-panel.access.log 2>/dev/null && fail "web path written to the nginx access log"
[[ "$(curl -o /dev/null -w '%{http_code}' https://$IP:$HP/v1/health)" == 200 ]] || fail "health via https"
[[ "$(curl -o /dev/null -w '%{http_code}' https://$IP:$HP/v1/status)" == 401 ]] || fail "auth must stay enforced"
[[ "$(curl -sS -k --max-time 10 -H 'Host: evil.example' -o /dev/null -w '%{http_code}' https://127.0.0.1:$HP/)" == 421 ]] || fail "foreign Host accepted"
[[ "$(stat -c %a /etc/baft-tunnel-https/ca.key)" == 600 && "$(stat -c %a /etc/baft-tunnel-https/panel.key)" == 600 ]] || fail "key perms"
openssl x509 -in "$CA" -noout -text | grep -q 'Name Constraints' || fail "CA is not name-constrained"
# a different host name must not verify against this CA's leaf
curl -sS --noproxy "*" --max-time 10 --cacert "$CA" --connect-to "other.example:$HP:127.0.0.1:$HP" "https://other.example:$HP/" >/dev/null 2>&1 && fail "wrong hostname verified"
# 2. idempotent re-run keeps the certificate
F1="$(openssl x509 -in /etc/baft-tunnel-https/panel.crt -noout -fingerprint -sha256)"
run --ip "$IP" >/dev/null || fail "re-run"
[[ "$F1" == "$(openssl x509 -in /etc/baft-tunnel-https/panel.crt -noout -fingerprint -sha256)" ]] || fail "re-run reissued a valid certificate"
# 3. rollback: agent down -> smoke test fails -> previous config restored
cp /etc/nginx/conf.d/baft-tunnel-panel.conf "$T/conf.before"
stop_agent
run --ip "$IP" --show-token >/dev/null 2>&1 && fail "smoke test did not fail with agent down"
cmp -s "$T/conf.before" /etc/nginx/conf.d/baft-tunnel-panel.conf || fail "rollback did not restore previous config"
start_agent
# 4. domain mode (certbot shimmed, staging flag): config generation + redirect + HSTS-off
echo "$IP panel.baft.test" >>/etc/hosts
OUT="$(run --domain panel.baft.test --staging --email ops@example.com)" || { echo "$OUT"; fail "domain install"; }
grep -q 'Let.s Encrypt STAGING' <<<"$OUT" || fail "staging label missing"
grep -q 'acme-challenge' /etc/nginx/conf.d/baft-tunnel-panel.conf || fail "acme location missing"
[[ "$(curl -sS --noproxy "*" --max-time 10 -o /dev/null -w '%{http_code}' --connect-to panel.baft.test:80:127.0.0.1:80 http://panel.baft.test/x)" == 301 ]] || fail "http does not redirect"
# 4b. real certbot + real ACME protocol against a local Pebble test CA (CI only; set PEBBLE_CA=<minica pem>).
# Proves the webroot challenge through nginx, issuance, install and the renewal dry run; it is not Let's Encrypt itself.
if [[ -n "${PEBBLE_CA:-}" ]]; then
  run --remove >/dev/null || fail "remove before pebble"
  rm -rf /etc/letsencrypt/live/baft-tunnel-panel /etc/letsencrypt/archive/baft-tunnel-panel /etc/letsencrypt/renewal/baft-tunnel-panel.conf
  RC="$(command -v certbot)" || fail "real certbot missing"
  cat >"$BIN/certbot" <<SH
#!/bin/sh
export REQUESTS_CA_BUNDLE="$PEBBLE_CA"
exec "$RC" "\$@" --server https://localhost:14000/dir
SH
  chmod +x "$BIN/certbot"
  curl -sS --max-time 15 --cacert "$PEBBLE_CA" https://localhost:15000/roots/0 -o "$T/pebble-root.pem" || fail "cannot fetch the Pebble root"
  export CURL_CA_BUNDLE="$T/pebble-root.pem"   # the installer's smoke test verifies the chain like a browser would
  OUT="$(run --domain panel.baft.test --email ops@example.com)" || {
    echo "$OUT"; tail -n 40 /var/log/letsencrypt/letsencrypt.log 2>/dev/null | cut -c1-300
    echo "--- hosts"; grep baft.test /etc/hosts; echo "--- listeners"; ss -ltn | grep -E ':(80|8443) ' || true
    echo "--- nginx conf"; cat /etc/nginx/conf.d/baft-tunnel-panel.conf 2>/dev/null | head -30
    fail "pebble domain install"; }
  [[ -s /etc/letsencrypt/live/baft-tunnel-panel/fullchain.pem ]] || fail "no certificate issued by pebble"
  ISS="$(openssl x509 -in /etc/letsencrypt/live/baft-tunnel-panel/fullchain.pem -noout -issuer)"
  grep -qi pebble <<<"$ISS" || fail "unexpected issuer: $ISS"
  SERVED="$(echo | openssl s_client -connect "127.0.0.1:$HP" -servername panel.baft.test 2>/dev/null | openssl x509 -noout -issuer)"
  grep -qi pebble <<<"$SERVED" || fail "nginx does not serve the issued certificate: $SERVED"
  CODE="$(command curl -sS --noproxy "*" --cacert "$T/pebble-root.pem" --max-time 10 -H "Cookie: baft_gate=$(cat /var/lib/baft-tunnel-https/gate)" -o /dev/null -w '%{http_code}' --connect-to "panel.baft.test:$HP:127.0.0.1:$HP" "https://panel.baft.test:$HP/" 2>&1)" || true
  [[ "$CODE" == 200 ]] || fail "panel not served over the issued certificate (got: $CODE)"
  unset CURL_CA_BUNDLE
  echo "PEBBLE_ACME=PASS ($ISS)"
fi
# 5. removal
run --remove >/dev/null || fail "remove"
[[ ! -e /etc/nginx/conf.d/baft-tunnel-panel.conf ]] || fail "remove left config"
echo PANEL_HTTPS_TEST=PASS
