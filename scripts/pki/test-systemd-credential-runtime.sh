#!/usr/bin/env bash
# Exercise the real systemd encrypted-credential delivery path with the BAFT runtime.
set -Eeuo pipefail
umask 077

BIN="${1:-$(cd "$(dirname "$0")/../.." && pwd -P)/target/release/baft-tunnel}"
[[ -x "$BIN" ]] || { echo "systemd credential test: missing executable: $BIN" >&2; exit 2; }
for command in systemd systemd-creds systemd-run systemctl openssl python3 sudo; do
  command -v "$command" >/dev/null 2>&1 || { echo "systemd credential test: missing command: $command" >&2; exit 2; }
done
systemd_version="$(systemd --version | awk 'NR == 1 { print $2 }')"
[[ "$systemd_version" =~ ^[0-9]+$ ]] && (( systemd_version >= 250 )) || {
  echo "systemd credential test: systemd 250+ required; found $systemd_version" >&2
  exit 2
}
pid1="$(ps -p 1 -o comm= | tr -d '[:space:]')"
[[ "$pid1" == "systemd" ]] || {
  echo "systemd credential test: PID 1 is '$pid1', not systemd; no service-manager test possible" >&2
  exit 2
}
sudo systemctl show-environment >/dev/null
sudo systemd-creds setup

TMP="$(mktemp -d "${TMPDIR:-/tmp}/baft-systemd-credential.XXXXXX")"
cleanup() {
  if [[ -n "${TMP:-}" && -d "$TMP" ]]; then
    sudo rm -f "$TMP/node-key.cred"
    rm -rf "$TMP"
  fi
}
trap cleanup EXIT
chmod 755 "$TMP"
mkdir -p "$TMP/ca/newcerts" "$TMP/node"
chmod 755 "$TMP/ca" "$TMP/ca/newcerts" "$TMP/node"

CA="$TMP/ca"
NODE="$TMP/node"
: > "$CA/index.txt"
printf '1000\n' > "$CA/serial"
printf '1000\n' > "$CA/crlnumber"
openssl req -x509 -newkey rsa:3072 -nodes -keyout "$CA/ca.key" -out "$CA/ca.crt" \
  -days 30 -sha256 -subj "/CN=BAFT-TUNNEL systemd credential test CA" \
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
openssl req -new -key "$NODE/node.key" -out "$NODE/node.csr" -subj "/CN=systemd-test"
cat > "$TMP/node-ext.cnf" <<'EOF_EXT'
[node_ext]
basicConstraints=critical,CA:FALSE
keyUsage=critical,digitalSignature
extendedKeyUsage=serverAuth,clientAuth
subjectAltName=URI:urn:baft-tunnel:node:systemd-test,DNS:tunnel.example.test
subjectKeyIdentifier=hash
authorityKeyIdentifier=keyid,issuer
EOF_EXT
openssl ca -batch -notext -config "$CA/openssl.cnf" -extfile "$TMP/node-ext.cnf" \
  -extensions node_ext -days 30 -in "$NODE/node.csr" -out "$NODE/node.crt" >/dev/null 2>&1
openssl ca -gencrl -batch -config "$CA/openssl.cnf" -out "$CA/ca.crl" >/dev/null 2>&1
chmod 644 "$NODE/node.crt"

sudo systemd-creds encrypt --name=node-key --with-key=auto "$NODE/node.key" "$TMP/node-key.cred"
sudo chmod 600 "$TMP/node-key.cred"

python3 - "$TMP" <<'PY'
import json
import pathlib
import sys
root = pathlib.Path(sys.argv[1])
config = {
    "mode": "forward",
    "listen": "127.0.0.1:18080",
    "remote": "203.0.113.10:9443",
    "server_name": "tunnel.example.test",
    "local_id": "systemd-test",
    "peer_id": "peer-test",
    "tunnel_id": "systemd-credential-test",
    "direction": "direct",
    "peer_fingerprint": "a" * 64,
    "ca": str(root / "ca" / "ca.crt"),
    "cert": str(root / "node" / "node.crt"),
    "key_credential": "node-key",
    "max_streams": 8,
    "crl": str(root / "ca" / "ca.crl"),
}
(root / "config.json").write_text(json.dumps(config, indent=2) + "\n")
(root / "run-validation.sh").write_text("""#!/usr/bin/env bash
set -Eeuo pipefail
credential_dir="${CREDENTIALS_DIRECTORY:?systemd did not set CREDENTIALS_DIRECTORY}"
credential="$credential_dir/node-key"
[[ -f "$credential" && -r "$credential" ]]
read -r mode owner group < <(stat -c '%a %u %g' "$credential")
service_uid="$(id -u)"
service_gid="$(id -g)"
[[ "$owner" == "$service_uid" ]] || {
  echo "credential owner is uid $owner, expected service uid $service_uid" >&2
  exit 1
}
case "$mode" in
  400) ;;
  440)
    [[ "$group" == "$service_gid" ]] || {
      echo "credential group is gid $group, expected service gid $service_gid" >&2
      exit 1
    }
    ;;
  *)
    echo "credential mode is $mode, expected 400 or service-group-scoped 440" >&2
    exit 1
    ;;
esac
exec "$1" config validate --config "$2"
""")
(root / "run-validation.sh").chmod(0o755)
(root / "config.json").chmod(0o644)
PY

# The same config must fail outside service context; no plaintext fallback is permitted.
if env -u CREDENTIALS_DIRECTORY "$BIN" config validate --config "$TMP/config.json" >"$TMP/outside.log" 2>&1; then
  echo "systemd credential test: config unexpectedly validated without CREDENTIALS_DIRECTORY" >&2
  exit 1
fi
grep -q 'CREDENTIALS_DIRECTORY' "$TMP/outside.log" || {
  echo "systemd credential test: expected fail-closed credential-directory error" >&2
  exit 1
}

UNIT="baft-tunnel-cred-test-$RANDOM-$$"
sudo systemd-run --wait --pipe --collect --unit="$UNIT" \
  --property=User=nobody \
  --property="LoadCredentialEncrypted=node-key:$TMP/node-key.cred" \
  /bin/bash "$TMP/run-validation.sh" "$BIN" "$TMP/config.json"

echo "BAFT_SYSTEMD_CREDENTIAL_RUNTIME=PASS (systemd $systemd_version, encrypted delivery, service-scoped read-only mode 0400/0440, runtime TLS key parse, fail-closed outside service)"
