#!/usr/bin/env bash
set -Eeuo pipefail
umask 077
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd -P)"
PKI="$SCRIPT_DIR/baft-pki.sh"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/baft-pki-profile.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT
CA="$TMP/ca"
mkdir -p "$CA/newcerts"
: > "$CA/index.txt"
printf '1000\n' > "$CA/serial"
printf '1000\n' > "$CA/crlnumber"
openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-384 -out "$CA/ca.key" 2>/dev/null
openssl req -new -x509 -key "$CA/ca.key" -out "$CA/ca.crt" -days 365 \
  -sha384 -subj "/CN=BAFT Profile Test CA" \
  -addext "basicConstraints=critical,CA:TRUE,pathlen:0" \
  -addext "keyUsage=critical,keyCertSign,cRLSign" 2>/dev/null
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
default_md = sha384
default_days = 90
default_crl_days = 30
policy = policy_node
unique_subject = no
copy_extensions = none
crl_extensions = crl_ext
[ policy_node ]
commonName = supplied
[ crl_ext ]
authorityKeyIdentifier = keyid:always
EOF
chmod 600 "$CA/ca.key" "$CA/index.txt" "$CA/serial" "$CA/crlnumber"
chmod 644 "$CA/ca.crt" "$CA/openssl.cnf"
openssl ca -gencrl -batch -config "$CA/openssl.cnf" -out "$CA/ca.crl" >/dev/null 2>&1
chmod 644 "$CA/ca.crl"

fail() { echo "PKI_PROFILE_TEST=FAIL: $*" >&2; exit 1; }
expect_fail() {
  if "$@" >"$TMP/negative.log" 2>&1; then
    fail "unexpected success: $*"
  fi
}

"$PKI" new-node baft-node "$TMP/baft-node" >/dev/null
"$PKI" new-node tcp-node "$TMP/tcp-node" >/dev/null

"$PKI" issue-node --profile baft "$CA" baft-node \
  "$TMP/baft-node/node.csr" "$TMP/baft-issued" baft-node.baft-tunnel.invalid >/dev/null
"$PKI" verify-node --profile baft "$CA" "$TMP/baft-issued/ca.crl" \
  "$TMP/baft-issued/node.crt" baft-node baft-node.baft-tunnel.invalid >/dev/null
expect_fail "$PKI" verify-node "$CA" "$TMP/baft-issued/ca.crl" \
  "$TMP/baft-issued/node.crt" baft-node baft-node.baft-tunnel.invalid

# Legacy commands remain TCP-TLS by default and reject BAFT profile certificates.
"$PKI" issue-node "$CA" tcp-node "$TMP/tcp-node/node.csr" \
  "$TMP/tcp-issued" tcp-node.lab >/dev/null
"$PKI" verify-node "$CA" "$TMP/tcp-issued/ca.crl" \
  "$TMP/tcp-issued/node.crt" tcp-node tcp-node.lab >/dev/null
expect_fail "$PKI" verify-node --profile baft "$CA" "$TMP/tcp-issued/ca.crl" \
  "$TMP/tcp-issued/node.crt" tcp-node tcp-node.lab

# Revocation recognizes both URI namespaces and the resulting CRL blocks the certificate.
"$PKI" revoke-node "$CA" "$TMP/baft-issued/node.crt" "$TMP/baft-revoked" >/dev/null
expect_fail "$PKI" verify-node --profile baft "$CA" "$TMP/baft-revoked/ca.crl" \
  "$TMP/baft-issued/node.crt" baft-node baft-node.baft-tunnel.invalid

"$PKI" revoke-node "$CA" "$TMP/tcp-issued/node.crt" "$TMP/tcp-revoked" >/dev/null
expect_fail "$PKI" verify-node "$CA" "$TMP/tcp-revoked/ca.crl" \
  "$TMP/tcp-issued/node.crt" tcp-node tcp-node.lab

echo "PKI_PROFILE_TEST=PASS (BAFT/TCP-TLS URI profiles, cross-profile rejection, dual-profile revocation)"
