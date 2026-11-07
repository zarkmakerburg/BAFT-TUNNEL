#!/usr/bin/env bash
set -Eeuo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd -P)"
PKI="$ROOT/scripts/pki/baft-pki.sh"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/baft-pki-test.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT
umask 077

CA="$TMP/ca"
NODE="$TMP/iran"
REVOKED="$TMP/revoked"
mkdir -p "$CA/newcerts"
: > "$CA/index.txt"
printf '1000\n' > "$CA/serial"
printf '1000\n' > "$CA/crlnumber"
openssl req -x509 -newkey rsa:3072 -nodes -keyout "$CA/ca.key" -out "$CA/ca.crt" \
  -days 2 -sha256 -subj "/CN=BAFT-TUNNEL Test CA" \
  -addext "basicConstraints=critical,CA:TRUE,pathlen:0" \
  -addext "keyUsage=critical,keyCertSign,cRLSign" >/dev/null 2>&1
cat > "$CA/openssl.cnf" <<EOF
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
EOF
openssl ca -gencrl -batch -config "$CA/openssl.cnf" -out "$CA/ca.crl" >/dev/null 2>&1

"$PKI" new-node iran "$NODE"
if stat -c '%a' "$NODE/node.key" >/dev/null 2>&1; then
  key_mode="$(stat -c '%a' "$NODE/node.key")"
else
  key_mode="$(stat -f '%Lp' "$NODE/node.key")"
fi
[[ "$key_mode" == "600" ]] || { echo "node private key mode is $key_mode, expected 600" >&2; exit 1; }
[[ ! -e "$NODE/ca.key" ]]
"$PKI" issue-node "$CA" iran "$NODE/node.csr" "$NODE" tunnel.iran.example.test 30
"$PKI" verify-node "$CA" "$NODE/ca.crl" "$NODE/node.crt" iran tunnel.iran.example.test >/dev/null
openssl x509 -in "$NODE/node.crt" -noout -ext subjectAltName | grep -Fq 'URI:urn:baft-tunnel:node:iran'
openssl x509 -in "$NODE/node.crt" -noout -ext subjectAltName | grep -Fq 'DNS:tunnel.iran.example.test'
if "$PKI" verify-node "$CA" "$NODE/ca.crl" "$NODE/node.crt" iran other.iran.example.test >/dev/null 2>&1; then
  echo "certificate was accepted for the wrong server DNS name" >&2
  exit 1
fi
ABROAD="$TMP/abroad"
"$PKI" new-node abroad "$ABROAD"
"$PKI" issue-node "$CA" abroad "$ABROAD/node.csr" "$ABROAD" 192.0.2.10 30
"$PKI" verify-node "$CA" "$ABROAD/ca.crl" "$ABROAD/node.crt" abroad 192.0.2.10 >/dev/null
openssl x509 -in "$ABROAD/node.crt" -noout -ext subjectAltName | grep -Fq 'IP Address:192.0.2.10'
[[ ! -e "$NODE/ca.key" ]] || { echo "CA private key leaked into node bundle" >&2; exit 1; }

if "$PKI" issue-node "$CA" abroad "$NODE/node.csr" "$TMP/wrong-cn" tunnel.abroad.example.test 30 >/dev/null 2>&1; then
  echo "CSR subject mismatch was accepted" >&2
  exit 1
fi
if "$PKI" new-node ../escape "$TMP/invalid" >/dev/null 2>&1; then
  echo "path-like node ID was accepted" >&2
  exit 1
fi
if "$PKI" verify-node "$CA" "$NODE/ca.crl" "$NODE/node.crt" abroad tunnel.iran.example.test >/dev/null 2>&1; then
  echo "certificate was accepted for the wrong node ID" >&2
  exit 1
fi

"$PKI" revoke-node "$CA" "$NODE/node.crt" "$REVOKED" >/dev/null
if "$PKI" verify-node "$CA" "$REVOKED/ca.crl" "$NODE/node.crt" iran tunnel.iran.example.test >/dev/null 2>&1; then
  echo "revoked certificate was accepted" >&2
  exit 1
fi
openssl crl -in "$REVOKED/ca.crl" -noout -text | grep -q 'Revoked Certificates'
mkdir -p "$TMP/not-empty"
: > "$TMP/not-empty/keep"
if "$PKI" init-ca "$TMP/not-empty" </dev/null >/dev/null 2>&1; then
  echo "init-ca overwrote a non-empty directory" >&2
  exit 1
fi

if "$PKI" init-ca "$TMP/bad-name-ca" 'bad/name' </dev/null >/dev/null 2>&1; then
  echo "init-ca accepted an invalid CA name" >&2
  exit 1
fi
[[ ! -e "$TMP/bad-name-ca/ca.key" ]] || { echo "init-ca created a CA for an invalid name" >&2; exit 1; }
echo "BAFT_PKI_TEST=PASS (P-256 node CSR, CA signing, URI/DNS/IP SANs, 0600 node key, CA isolation, revocation, mismatch rejection)"
