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
"$APPLY" check "$CA/ca.crt" "$TMP/current.crl" "$TMP/candidate.crl" | grep -q 'CRL_CHECK=PASS cas=1 old_numbers=4096 candidate_numbers=4097'

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

# Rollover overlap (ADR-0017 B1): a bundle of two CAs with one CRL per CA.
NEWCA="$TMP/newca"; DUP="$TMP/dup"
make_ca "$NEWCA" 'BAFT-TUNNEL CRL Test CA 2'
make_ca "$DUP" 'BAFT-TUNNEL CRL Test CA'
cat "$CA/ca.crt" "$NEWCA/ca.crt" > "$TMP/bundle-ca.pem"
printf '1003\n' > "$CA/crlnumber"
openssl ca -gencrl -batch -config "$CA/openssl.cnf" -out "$TMP/a-4099.crl" >/dev/null 2>&1
openssl ca -gencrl -batch -config "$NEWCA/openssl.cnf" -out "$TMP/b-4096.crl" >/dev/null 2>&1
printf '1001\n' > "$NEWCA/crlnumber"
openssl ca -gencrl -batch -config "$NEWCA/openssl.cnf" -out "$TMP/b-4097.crl" >/dev/null 2>&1
cat "$TMP/a-4099.crl" "$TMP/b-4096.crl" > "$TMP/both-1.crl"
"$APPLY" check "$TMP/bundle-ca.pem" "$TMP/current.crl" "$TMP/both-1.crl" | grep -q 'CRL_CHECK=PASS cas=2 old_numbers=4096 candidate_numbers=4099,4096'
printf '1004\n' > "$CA/crlnumber"
openssl ca -gencrl -batch -config "$CA/openssl.cnf" -out "$TMP/a-4100.crl" >/dev/null 2>&1
cat "$TMP/a-4100.crl" "$TMP/b-4097.crl" > "$TMP/both-2.crl"
"$APPLY" check "$TMP/bundle-ca.pem" "$TMP/both-1.crl" "$TMP/both-2.crl" | grep -q 'CRL_CHECK=PASS cas=2'
if "$APPLY" check "$TMP/bundle-ca.pem" "$TMP/current.crl" "$TMP/candidate.crl" >/dev/null 2>&1; then
  echo 'bundle candidate missing the new CA CRL was accepted (must fail closed)' >&2; exit 1
fi
if "$APPLY" check "$TMP/bundle-ca.pem" "$TMP/both-2.crl" "$TMP/both-1.crl" >/dev/null 2>&1; then
  echo 'per-CA CRL number rollback inside a bundle was accepted' >&2; exit 1
fi
openssl ca -gencrl -batch -config "$OTHER/openssl.cnf" -out "$TMP/other.crl" >/dev/null 2>&1
cat "$TMP/a-4099.crl" "$TMP/b-4096.crl" "$TMP/other.crl" > "$TMP/with-unknown.crl"
if "$APPLY" check "$TMP/bundle-ca.pem" "$TMP/current.crl" "$TMP/with-unknown.crl" >/dev/null 2>&1; then
  echo 'CRL from an unconfigured CA inside a bundle was accepted' >&2; exit 1
fi
cat "$CA/ca.crt" "$DUP/ca.crt" > "$TMP/dup-ca.pem"
if "$APPLY" check "$TMP/dup-ca.pem" "$TMP/current.crl" "$TMP/a-4099.crl" >/dev/null 2>&1; then
  echo 'CA bundle with duplicate subjects was accepted' >&2; exit 1
fi

bash -n "$APPLY"
echo 'BAFT_CRL_APPLY_TEST=PASS (signature/issuer, increasing CRL number, expiry, malformed and stale rejection, multi-CA bundles with per-CA numbers, missing/unknown/duplicate CA rejection)'
