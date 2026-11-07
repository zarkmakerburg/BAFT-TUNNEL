#!/usr/bin/env bash
set -Eeuo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd -P)"
APPLY="$ROOT/scripts/pki/apply-crl.sh"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/baft-crl-apply-test.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT
umask 077

make_ca() {
  local dir="$1" cn="$2"
  mkdir -p "$dir/newcerts"
  : > "$dir/index.txt"
  printf '1000\n' > "$dir/serial"
  printf '1000\n' > "$dir/crlnumber"
  openssl req -x509 -newkey rsa:2048 -nodes -keyout "$dir/ca.key" -out "$dir/ca.crt" \
    -days 3 -sha256 -subj "/CN=$cn" \
    -addext 'basicConstraints=critical,CA:TRUE,pathlen:0' \
    -addext 'keyUsage=critical,keyCertSign,cRLSign' >/dev/null 2>&1
  cat > "$dir/openssl.cnf" <<CFG
[ ca ]
default_ca = CA_default
[ CA_default ]
database = $dir/index.txt
new_certs_dir = $dir/newcerts
certificate = $dir/ca.crt
private_key = $dir/ca.key
serial = $dir/serial
crlnumber = $dir/crlnumber
default_md = sha256
default_crl_days = 2
policy = policy_any
[ policy_any ]
commonName = supplied
[ crl_ext ]
authorityKeyIdentifier = keyid:always
CFG
}

CA="$TMP/ca"; OTHER="$TMP/other"
make_ca "$CA" 'BAFT-TUNNEL CRL Test CA'
make_ca "$OTHER" 'Different CRL Test CA'
openssl ca -gencrl -batch -config "$CA/openssl.cnf" -out "$TMP/current.crl" >/dev/null 2>&1
printf '1001\n' > "$CA/crlnumber"
openssl ca -gencrl -batch -config "$CA/openssl.cnf" -out "$TMP/candidate.crl" >/dev/null 2>&1
"$APPLY" check "$CA/ca.crt" "$TMP/current.crl" "$TMP/candidate.crl" | grep -q 'CRL_CHECK=PASS old_number=4096 candidate_number=4097'

if "$APPLY" check "$CA/ca.crt" "$TMP/current.crl" "$TMP/current.crl" >/dev/null 2>&1; then
  echo 'equal CRL number was accepted' >&2; exit 1
fi
if "$APPLY" check "$CA/ca.crt" "$TMP/candidate.crl" "$TMP/current.crl" >/dev/null 2>&1; then
  echo 'lower CRL number was accepted' >&2; exit 1
fi
if "$APPLY" check "$OTHER/ca.crt" "$TMP/current.crl" "$TMP/candidate.crl" >/dev/null 2>&1; then
  echo 'CRL signed by another CA was accepted' >&2; exit 1
fi
cp "$TMP/candidate.crl" "$TMP/malformed.crl"
printf '\ninvalid' >> "$TMP/malformed.crl"
if "$APPLY" check "$CA/ca.crt" "$TMP/current.crl" "$TMP/malformed.crl" >/dev/null 2>&1; then
  echo 'malformed CRL was accepted' >&2; exit 1
fi

# Generate a valid but expired candidate to verify the validity-window check.
printf '1002\n' > "$CA/crlnumber"
openssl ca -gencrl -batch -config "$CA/openssl.cnf" \
  -crl_lastupdate 20200101000000Z -crl_nextupdate 20200102000000Z \
  -out "$TMP/expired.crl" >/dev/null 2>&1
if "$APPLY" check "$CA/ca.crt" "$TMP/current.crl" "$TMP/expired.crl" >/dev/null 2>&1; then
  echo 'expired CRL was accepted' >&2; exit 1
fi

bash -n "$APPLY"
echo 'BAFT_CRL_APPLY_TEST=PASS (signature/issuer, increasing CRL number, expiry, malformed and stale rejection)'
