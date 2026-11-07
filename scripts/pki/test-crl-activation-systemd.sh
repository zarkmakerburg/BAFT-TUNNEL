#!/usr/bin/env bash
# Exercise CRL validation, atomic activation, and rollback against a disposable
# transient systemd unit on an isolated Ubuntu 24.04+ test host.
set -Eeuo pipefail
umask 077

ROOT="$(cd "$(dirname "$0")/../.." && pwd -P)"
APPLY="$ROOT/scripts/pki/apply-crl.sh"
for command in systemd systemd-run systemctl openssl python3 sudo flock; do
  command -v "$command" >/dev/null 2>&1 || { echo "CRL systemd test: missing command: $command" >&2; exit 2; }
done
systemd_version="$(systemd --version | awk 'NR == 1 { print $2 }')"
[[ "$systemd_version" =~ ^[0-9]+$ ]] && (( systemd_version >= 250 )) || {
  echo "CRL systemd test: systemd 250+ required; found $systemd_version" >&2; exit 2;
}
pid1="$(ps -p 1 -o comm= | tr -d '[:space:]')"
[[ "$pid1" == systemd ]] || { echo "CRL systemd test: PID 1 is '$pid1', not systemd" >&2; exit 2; }
sudo -n systemctl show-environment >/dev/null

TMP="$(mktemp -d "${TMPDIR:-/tmp}/baft-crl-systemd.XXXXXX")"
INSTANCE="crl-activation-$$"
SERVICE="baft-tunnel@${INSTANCE}.service"
cleanup() {
  local status=$?
  trap - EXIT
  sudo systemctl stop "$SERVICE" >/dev/null 2>&1 || true
  sudo systemctl reset-failed "$SERVICE" >/dev/null 2>&1 || true
  rm -rf "$TMP"
  exit "$status"
}
trap cleanup EXIT
mkdir -p "$TMP/newcerts"
: > "$TMP/index.txt"
printf '1000\n' > "$TMP/serial"
printf '1000\n' > "$TMP/crlnumber"
openssl req -x509 -newkey rsa:2048 -nodes -keyout "$TMP/ca.key" -out "$TMP/ca.crt" \
  -days 3 -sha256 -subj '/CN=BAFT-TUNNEL CRL activation test CA' \
  -addext 'basicConstraints=critical,CA:TRUE,pathlen:0' \
  -addext 'keyUsage=critical,keyCertSign,cRLSign' >/dev/null 2>&1
cat > "$TMP/openssl.cnf" <<CFG
[ ca ]
default_ca = CA_default
[ CA_default ]
database = $TMP/index.txt
new_certs_dir = $TMP/newcerts
certificate = $TMP/ca.crt
private_key = $TMP/ca.key
serial = $TMP/serial
crlnumber = $TMP/crlnumber
default_md = sha256
default_crl_days = 2
policy = policy_any
[ policy_any ]
commonName = supplied
[ crl_ext ]
authorityKeyIdentifier = keyid:always
CFG
openssl ca -gencrl -batch -config "$TMP/openssl.cnf" -out "$TMP/current.crl" >/dev/null 2>&1
openssl ca -gencrl -batch -config "$TMP/openssl.cnf" -out "$TMP/candidate-rollback.crl" >/dev/null 2>&1
openssl ca -gencrl -batch -config "$TMP/openssl.cnf" -out "$TMP/candidate-success.crl" >/dev/null 2>&1

"$APPLY" check "$TMP/ca.crt" "$TMP/current.crl" "$TMP/candidate-rollback.crl" >/dev/null

# The transient service starts normally. A marker makes exactly the next
# systemd restart fail; the helper must restore the old CRL and restart again.
cat > "$TMP/service.sh" <<CFG
#!/usr/bin/env bash
set -Eeuo pipefail
if [[ -e '$TMP/fail-once' ]]; then
  rm -f '$TMP/fail-once'
  exit 42
fi
exec /usr/bin/sleep infinity
CFG
chmod 0755 "$TMP/service.sh"
if sudo systemctl cat "$SERVICE" >/dev/null 2>&1; then
  echo "CRL systemd test: refusing pre-existing unit $SERVICE" >&2; exit 1
fi
sudo systemd-run --unit="$SERVICE" --property=Restart=on-failure --property=RestartSec=30s "$TMP/service.sh" >/dev/null
wait_active() {
  local attempt
  for attempt in $(seq 1 40); do
    sudo systemctl is-active --quiet "$SERVICE" && return 0
    sleep 0.25
  done
  sudo systemctl status --no-pager "$SERVICE" >&2 || true
  sudo journalctl -u "$SERVICE" -n 40 --no-pager >&2 || true
  return 1
}
wait_active || { echo 'CRL systemd test: transient service failed to start' >&2; exit 1; }

old_fingerprint="$(openssl crl -in "$TMP/current.crl" -noout -fingerprint -sha256)"
touch "$TMP/fail-once"
set +e
sudo "$APPLY" apply "$TMP/ca.crt" "$TMP/current.crl" "$TMP/candidate-rollback.crl" "$INSTANCE" >"$TMP/rollback.log" 2>&1
rollback_status=$?
set -e
if [[ "$rollback_status" != 1 ]] || ! grep -q 'CRL_APPLY=ROLLED_BACK' "$TMP/rollback.log"; then
  cat "$TMP/rollback.log" >&2
  echo "CRL systemd test: expected recovered rollback exit 1, got $rollback_status" >&2
  exit 1
fi
restored_fingerprint="$(openssl crl -in "$TMP/current.crl" -noout -fingerprint -sha256)"
[[ "$restored_fingerprint" == "$old_fingerprint" ]] || { echo 'CRL systemd test: rollback did not restore original CRL' >&2; exit 1; }
[[ "$(stat -c '%a:%u:%g' "$TMP/current.crl")" == '644:0:0' ]] || { echo 'CRL systemd test: rollback did not restore root:root 0644 readability' >&2; exit 1; }
wait_active || { echo 'CRL systemd test: service did not recover after CRL rollback' >&2; exit 1; }
if find "$TMP" -maxdepth 1 -name '.current.crl.rollback.*' | grep -q .; then
  echo 'CRL systemd test: rollback backup remained after successful recovery' >&2; exit 1
fi

sudo "$APPLY" apply "$TMP/ca.crt" "$TMP/current.crl" "$TMP/candidate-success.crl" "$INSTANCE" >"$TMP/success.log" 2>&1
grep -q 'CRL_APPLY=PASS' "$TMP/success.log" || { cat "$TMP/success.log" >&2; exit 1; }
wait_active || { echo 'CRL systemd test: service inactive after successful CRL activation' >&2; exit 1; }
installed_fingerprint="$(openssl crl -in "$TMP/current.crl" -noout -fingerprint -sha256)"
candidate_fingerprint="$(openssl crl -in "$TMP/candidate-success.crl" -noout -fingerprint -sha256)"
[[ "$installed_fingerprint" == "$candidate_fingerprint" ]] || { echo 'CRL systemd test: candidate CRL was not activated' >&2; exit 1; }
[[ "$(stat -c '%a:%u:%g' "$TMP/current.crl")" == '644:0:0' ]] || { echo 'CRL systemd test: live CRL mode/owner is not root:root 0644' >&2; exit 1; }
[[ "$(stat -c '%a:%u:%g' "$TMP/.current.crl.apply.lock")" == '600:0:0' ]] || { echo 'CRL systemd test: lock file is not root-only 0600' >&2; exit 1; }
if find "$TMP" -maxdepth 1 \( -name '.current.crl.rollback.*' -o -name '.current.crl.staged.*' \) | grep -q .; then
  echo 'CRL systemd test: temporary rollback/staging files leaked' >&2; exit 1
fi

echo "BAFT_CRL_SYSTEMD_ACTIVATION=PASS (systemd $systemd_version, successful restart, one-shot restart failure, atomic rollback and service recovery, root-only lock/backup)"
