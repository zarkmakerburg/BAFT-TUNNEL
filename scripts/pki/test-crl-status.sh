#!/usr/bin/env bash
set -Eeuo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd -P)"
STATUS="$ROOT/scripts/pki/crl-status.sh"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/baft-crl-status-test.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT
make_crl() { # dir cn crl_days
  local dir="$1" cn="$2" days="$3"
  mkdir -p "$dir/newcerts"; : > "$dir/index.txt"; printf '1000\n' > "$dir/serial"; printf '1000\n' > "$dir/crlnumber"
  openssl req -x509 -newkey rsa:2048 -nodes -keyout "$dir/ca.key" -out "$dir/ca.crt" -days 3 -sha256 -subj "/CN=$cn" \
    -addext 'basicConstraints=critical,CA:TRUE,pathlen:0' -addext 'keyUsage=critical,keyCertSign,cRLSign' >/dev/null 2>&1
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
default_crl_days = $days
policy = p
[ p ]
commonName = supplied
CFG
  openssl ca -gencrl -batch -config "$dir/openssl.cnf" -out "$dir/ca.crl" >/dev/null 2>&1
}
make_crl "$TMP/old" "Test CA old" 10
make_crl "$TMP/new" "Test CA new" 1
cat "$TMP/old/ca.crl" "$TMP/new/ca.crl" > "$TMP/bundle.crl"
# Both fresh at a 1-hour threshold: OK for both CRLs, exit 0.
out="$("$STATUS" "$TMP/bundle.crl" 1)"
[[ "$(grep -c '^OK ' <<<"$out")" == 2 ]] || { echo "$out" >&2; echo "expected two OK lines" >&2; exit 1; }
# The 1-day CRL is under a 48-hour threshold: WARN and exit 1; the 10-day one stays OK.
if out="$("$STATUS" "$TMP/bundle.crl" 48)"; then echo "expected WARN exit" >&2; exit 1; fi
grep -q '^WARN .*Test CA new' <<<"$out"
grep -q '^OK .*Test CA old' <<<"$out"
# Invalid inputs fail with exit 2 and never succeed silently.
printf 'garbage\n' > "$TMP/bad.crl"
rc=0; "$STATUS" "$TMP/bad.crl" >/dev/null 2>&1 || rc=$?
[[ "$rc" == 2 ]] || { echo "garbage file must exit 2, got $rc" >&2; exit 1; }
rc=0; "$STATUS" "$TMP/missing.crl" >/dev/null 2>&1 || rc=$?
[[ "$rc" == 2 ]] || { echo "missing file must exit 2, got $rc" >&2; exit 1; }
echo "BAFT_CRL_STATUS_TEST=PASS (per-CRL freshness, WARN threshold, bad input fails)"
