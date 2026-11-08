#!/usr/bin/env bash
# Mock-SSH test for push-crl.sh: no network connections are made.
set -Eeuo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd -P)"
PUSH="$ROOT/scripts/pki/push-crl.sh"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/baft-push-crl-test.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT
make_crl() { # dir cn days
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
make_crl "$TMP/good" "Push CA" 20
make_crl "$TMP/short" "Short CA" 1

mkdir -p "$TMP/bin"
cat > "$TMP/bin/ssh" <<'STUB'
#!/usr/bin/env bash
cmd="${*: -1}"
echo "ssh $*" >> "$STUB_LOG"
case "$cmd" in
  *mktemp*) echo /tmp/baft-crl-push.AbC123 ;;
  *apply-crl.sh*) [[ -z "${FAIL_HOST:-}" ]] || [[ "$*" != *"$FAIL_HOST"* ]] || exit 7 ;;
esac
STUB
cat > "$TMP/bin/scp" <<'STUB'
#!/usr/bin/env bash
echo "scp $*" >> "$STUB_LOG"
STUB
chmod +x "$TMP/bin/ssh" "$TMP/bin/scp"
export STUB_LOG="$TMP/calls.log" PATH="$TMP/bin:$PATH"
args=(--crl "$TMP/good/ca.crl" --remote-ca /etc/baft-tunnel/ca.crt --remote-crl /etc/baft-tunnel/ca.crl --instance demo)

# 1. Dry run contacts nothing.
: > "$STUB_LOG"
out="$("$PUSH" --dry-run "${args[@]}" --host root@one.example --host root@two.example:2222)"
grep -q 'PUSH_CRL=OK nodes=2 dry_run=1' <<<"$out"
[[ ! -s "$STUB_LOG" ]] || { echo "dry-run made calls" >&2; exit 1; }

# 2. Normal push: strict SSH options, one node fully done before the next, staging dir removed.
: > "$STUB_LOG"
out="$("$PUSH" "${args[@]}" --host root@one.example --host root@two.example:2222)"
grep -q 'PUSH_CRL=OK nodes=2 dry_run=0' <<<"$out"
grep -q 'StrictHostKeyChecking=yes' "$STUB_LOG"; grep -q 'BatchMode=yes' "$STUB_LOG"
grep -qF "apply '/etc/baft-tunnel/ca.crt' '/etc/baft-tunnel/ca.crl' '/tmp/baft-crl-push.AbC123/candidate.crl' 'demo'" "$STUB_LOG"
first_two="$(grep -n 'two.example' "$STUB_LOG" | head -1 | cut -d: -f1)"
last_one="$(grep -n 'one.example' "$STUB_LOG" | tail -1 | cut -d: -f1)"
(( last_one < first_two )) || { echo "nodes were interleaved" >&2; exit 1; }
[[ "$(grep -c "rm -rf '/tmp/baft-crl-push.AbC123'" "$STUB_LOG")" == 2 ]]
grep -q -- '-p 2222' "$STUB_LOG"; grep -q -- '-P 2222' "$STUB_LOG"

# 3. Failure on the first node stops the run; the second node is never contacted; staging is cleaned.
: > "$STUB_LOG"
if FAIL_HOST=one.example "$PUSH" "${args[@]}" --host root@one.example --host root@two.example >/dev/null 2>"$TMP/err"; then echo "expected failure" >&2; exit 1; fi
grep -q 'FAILED on node 1' "$TMP/err"
! grep -q 'two.example' "$STUB_LOG"
grep -q "rm -rf '/tmp/baft-crl-push.AbC123'" "$STUB_LOG"

# 4. A CRL close to expiry is refused before any connection.
: > "$STUB_LOG"
if "$PUSH" --crl "$TMP/short/ca.crl" --remote-ca /a/ca --remote-crl /a/crl --instance demo --host root@one.example >/dev/null 2>&1; then echo "expiring CRL accepted" >&2; exit 1; fi
[[ ! -s "$STUB_LOG" ]]

# 5. Hostile inputs are rejected before any connection.
for bad in "--host root@one.example;id" "--host one.example" "--instance a;b" "--remote-ca /etc/../x" "--remote-apply relative.sh"; do
  : > "$STUB_LOG"
  # shellcheck disable=SC2086
  if "$PUSH" --dry-run "${args[@]}" --host root@ok.example $bad >/dev/null 2>&1; then echo "accepted: $bad" >&2; exit 1; fi
  [[ ! -s "$STUB_LOG" ]]
done
echo "push-crl tests: PASS (mock SSH/SCP only; no network connections made)"
