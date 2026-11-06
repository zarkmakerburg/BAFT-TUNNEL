#!/usr/bin/env bash
# Exercise the installed BAFT systemd template with an encrypted node credential.
set -Eeuo pipefail
umask 077

ROOT="$(cd "$(dirname "$0")/../.." && pwd -P)"
BIN="\${1:-$ROOT/target/release/baft-tunnel}"
SERVICE_NAME="credential-ci"
SERVICE_UNIT="baft-tunnel@\${SERVICE_NAME}.service"
DROPIN_DIR="/etc/systemd/system/\${SERVICE_UNIT}.d"
CONFIG_FILE="/etc/baft-tunnel/\${SERVICE_NAME}.json"
PKI_DIR="/etc/baft-tunnel/pki/credential-ci"
CREDENTIAL_DIR="/etc/baft-tunnel/credentials/credential-ci"
TMP="$(mktemp -d "\${TMPDIR:-/tmp}/baft-systemd-service.XXXXXX")"
INSTALL_DONE=0
CREATED_USER=0
CREATED_GROUP=0
BACKUP_DIR=""

die() { echo "systemd service lifecycle: $*" >&2; exit 1; }
[[ -x "$BIN" ]] || die "missing executable: $BIN"
for command in systemd systemd-creds systemctl openssl sudo install; do
  command -v "$command" >/dev/null 2>&1 || die "missing command: $command"
done
systemd_version="$(systemd --version | awk 'NR == 1 { print $2 }')"
[[ "$systemd_version" =~ ^[0-9]+$ ]] && (( systemd_version >= 250 )) || die "systemd 250+ required; found $systemd_version"
pid1="$(ps -p 1 -o comm= | tr -d '[:space:]')"
[[ "$pid1" == "systemd" ]] || die "PID 1 is '$pid1', not systemd"
[[ "$(id -u)" == "0" ]] || sudo -n true || die "root/sudo access required"

# This is an ephemeral CI host. Refuse collisions rather than replacing any
# pre-existing BAFT installation, config, account, or service instance.
for path in /usr/local/bin/baft-tunnel /etc/systemd/system/baft-tunnel@.service /etc/baft-tunnel /var/lib/baft-tunnel "$DROPIN_DIR"; do
  [[ ! -e "$path" ]] || die "refusing to overwrite pre-existing path: $path"
done
active_units="$(systemctl list-units --state=active --plain --no-legend 'baft-tunnel@*.service' 2>/dev/null || true)"
[[ -z "$active_units" ]] || die "pre-existing BAFT-TUNNEL service is active"

if ! getent group baft-tunnel >/dev/null 2>&1; then CREATED_GROUP=1; fi
if ! id baft-tunnel >/dev/null 2>&1; then CREATED_USER=1; fi

cleanup() {
  local status=$?
  trap - EXIT
  sudo systemctl stop "$SERVICE_UNIT" >/dev/null 2>&1 || true
  sudo rm -rf "$DROPIN_DIR"
  sudo systemctl daemon-reload >/dev/null 2>&1 || true
  if [[ "$INSTALL_DONE" == "1" ]]; then
    sudo "$ROOT/scripts/uninstall.sh" --stop --purge-config --yes >/dev/null 2>&1 || true
  else
    sudo rm -rf /etc/baft-tunnel /var/lib/baft-tunnel
    sudo rm -f /usr/local/bin/baft-tunnel /etc/systemd/system/baft-tunnel@.service
    sudo systemctl daemon-reload >/dev/null 2>&1 || true
  fi
  if [[ -n "$BACKUP_DIR" ]]; then sudo rm -rf "$BACKUP_DIR"; fi
  if [[ "$CREATED_USER" == "1" ]]; then sudo userdel baft-tunnel >/dev/null 2>&1 || true; fi
  if [[ "$CREATED_GROUP" == "1" ]]; then sudo groupdel baft-tunnel >/dev/null 2>&1 || true; fi
  rm -rf "$TMP"
  exit "$status"
}
trap cleanup EXIT
chmod 755 "$TMP"
RUNTIME_BIN="$TMP/baft-tunnel"
install -m 0755 "$BIN" "$RUNTIME_BIN"

sudo systemd-creds setup
mkdir -p "$TMP/ca/newcerts" "$TMP/node"
chmod 755 "$TMP/ca" "$TMP/ca/newcerts" "$TMP/node"
CA="$TMP/ca"
NODE="$TMP/node"
: > "$CA/index.txt"
printf '1000\n' > "$CA/serial"
printf '1000\n' > "$CA/crlnumber"
openssl req -x509 -newkey rsa:3072 -nodes -keyout "$CA/ca.key" -out "$CA/ca.crt" \
  -days 30 -sha256 -subj "/CN=BAFT-TUNNEL service lifecycle test CA" \
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
openssl req -new -key "$NODE/node.key" -out "$NODE/node.csr" -subj "/CN=credential-ci"
cat > "$TMP/node-ext.cnf" <<'EOF_EXT'
[node_ext]
basicConstraints=critical,CA:FALSE
keyUsage=critical,digitalSignature
extendedKeyUsage=serverAuth,clientAuth
subjectAltName=URI:urn:baft-tunnel:node:credential-ci,DNS:tunnel.example.test
subjectKeyIdentifier=hash
authorityKeyIdentifier=keyid,issuer
EOF_EXT
openssl ca -batch -notext -config "$CA/openssl.cnf" -extfile "$TMP/node-ext.cnf" \
  -extensions node_ext -days 30 -in "$NODE/node.csr" -out "$NODE/node.crt" >/dev/null 2>&1
openssl ca -gencrl -batch -config "$CA/openssl.cnf" -out "$CA/ca.crl" >/dev/null 2>&1
chmod 644 "$NODE/node.crt"
sudo systemd-creds encrypt --name=node-key --with-key=auto "$NODE/node.key" "$TMP/node-key.cred"

# Install only after all disposable PKI material is ready.
sudo "$ROOT/scripts/install.sh" --binary "$RUNTIME_BIN" >"$TMP/install.log"
BACKUP_DIR="$(sed -n 's/^Backup: //p' "$TMP/install.log" | tail -n 1)"
[[ -n "$BACKUP_DIR" && -d "$BACKUP_DIR" ]] || die "installer did not report its rollback backup"
INSTALL_DONE=1

sudo install -d -o root -g baft-tunnel -m 0750 "$PKI_DIR" "$CREDENTIAL_DIR"
sudo install -o root -g root -m 0644 "$CA/ca.crt" "$PKI_DIR/ca.crt"
sudo install -o root -g root -m 0644 "$CA/ca.crl" "$PKI_DIR/ca.crl"
sudo install -o root -g root -m 0644 "$NODE/node.crt" "$PKI_DIR/node.crt"
sudo install -o root -g root -m 0600 "$TMP/node-key.cred" "$CREDENTIAL_DIR/node-key.cred"
python3 - "$TMP/config.json" "$PKI_DIR" <<'PY'
import json
import pathlib
import sys
config_path = pathlib.Path(sys.argv[1])
pki = pathlib.Path(sys.argv[2])
config = {
    "mode": "forward",
    "listen": "127.0.0.1:18080",
    "remote": "203.0.113.10:9443",
    "server_name": "tunnel.example.test",
    "local_id": "credential-ci",
    "peer_id": "peer-test",
    "tunnel_id": "credential-ci-lifecycle",
    "direction": "direct",
    "peer_fingerprint": "a" * 64,
    "ca": str(pki / "ca.crt"),
    "cert": str(pki / "node.crt"),
    "key_credential": "node-key",
    "max_streams": 8,
    "crl": str(pki / "ca.crl"),
}
config_path.write_text(json.dumps(config, indent=2) + "\n")
PY
sudo install -o root -g baft-tunnel -m 0640 "$TMP/config.json" "$CONFIG_FILE"
sudo install -d -o root -g root -m 0755 "$DROPIN_DIR"
printf '[Service]\nLoadCredentialEncrypted=node-key:%s\n' "$CREDENTIAL_DIR/node-key.cred" |
  sudo tee "$DROPIN_DIR/10-encrypted-credential.conf" >/dev/null
sudo systemctl daemon-reload

wait_active() {
  local attempt
  for attempt in $(seq 1 40); do
    if sudo systemctl is-active --quiet "$SERVICE_UNIT"; then return 0; fi
    sleep 0.25
  done
  sudo systemctl status --no-pager "$SERVICE_UNIT" >&2 || true
  sudo journalctl -u "$SERVICE_UNIT" -n 80 --no-pager >&2 || true
  return 1
}

sudo systemctl start "$SERVICE_UNIT"
wait_active || die "installed service did not become active"
[[ "$(sudo systemctl show -p User --value "$SERVICE_UNIT")" == "baft-tunnel" ]] || die "service did not run as baft-tunnel"
start_before="$(sudo systemctl show -p ExecMainStartTimestampMonotonic --value "$SERVICE_UNIT")"
[[ -n "$start_before" && "$start_before" != "0" ]] || die "systemd did not record the service start"

sudo systemctl restart "$SERVICE_UNIT"
wait_active || die "service did not recover after explicit systemctl restart"
start_after="$(sudo systemctl show -p ExecMainStartTimestampMonotonic --value "$SERVICE_UNIT")"
[[ -n "$start_after" && "$start_after" != "$start_before" ]] || die "service start timestamp did not advance after restart"

sudo systemctl stop "$SERVICE_UNIT"
! sudo systemctl is-active --quiet "$SERVICE_UNIT" || die "service remained active after stop"
sudo rm -rf "$DROPIN_DIR"
sudo systemctl daemon-reload

sudo "$ROOT/scripts/uninstall.sh" --stop --purge-config --yes
INSTALL_DONE=0
[[ ! -e /usr/local/bin/baft-tunnel ]] || die "uninstall left the binary installed"
[[ ! -e /etc/systemd/system/baft-tunnel@.service ]] || die "uninstall left the systemd unit installed"
[[ ! -e /etc/baft-tunnel && ! -e /var/lib/baft-tunnel ]] || die "uninstall left config/state behind"
echo "BAFT_SYSTEMD_SERVICE_LIFECYCLE=PASS (systemd $systemd_version, real install, encrypted credential, service start/restart/stop, safe uninstall)"
