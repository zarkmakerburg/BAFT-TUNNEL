#!/usr/bin/env bash
# Contract test for lab/real-hosts/re-pin-host-keys.sh.
# Real ssh-keygen key material; only ssh-keyscan is mocked, so no network is used.
set -Eeuo pipefail

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
script="$root/lab/real-hosts/re-pin-host-keys.sh"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT HUP INT TERM

fail() { echo "re-pin contract: $*" >&2; exit 1; }
contains() { grep -Fq -- "$2" "$1" || fail "missing expected output: $2"; }
not_contains() { ! grep -Fq -- "$2" "$1" || fail "unexpected output: $2"; }

# --- real ED25519 host keys -------------------------------------------------
for k in a b c; do
  ssh-keygen -q -t ed25519 -N '' -C "lab-$k" -f "$tmp/key-$k" >/dev/null
  awk '{print $1 " " $2}' "$tmp/key-$k.pub" > "$tmp/blob-$k"
done
read -r type_a blob_a < "$tmp/blob-a"
read -r type_b blob_b < "$tmp/blob-b"
read -r type_c blob_c < "$tmp/blob-c"
fp_a=$(ssh-keygen -lf "$tmp/key-a.pub" | awk '{print $2}')
fp_b=$(ssh-keygen -lf "$tmp/key-b.pub" | awk '{print $2}')
fp_bad=$(ssh-keygen -lf "$tmp/key-c.pub" | awk '{print $2}')

mkdir "$tmp/bin"
cat > "$tmp/bin/ssh-keyscan" <<'MOCK'
#!/usr/bin/env bash
set -Eeuo pipefail
host='' port=22
while (($#)); do
  case "$1" in
    -T) shift 2 ;;
    -p) port="$2"; shift 2 ;;
    -t) shift 2 ;;
    *) host="$1"; shift ;;
  esac
done
side='other'
[[ "$host" == "$LAB_IRAN_HOST" ]] && side=iran
[[ "$host" == "$LAB_ABROAD_HOST" ]] && side=abroad
key_var="LAB_KEY_$side"
blob="${!key_var:-}"
[[ -n "$blob" ]] || exit 1
if [[ "${LAB_MODE:-ok}" == "empty-$side" ]]; then exit 0; fi
printf '%s ssh-ed25519 %s\n' "$host" "$blob"
if [[ "${LAB_MODE:-ok}" == "double-$side" ]]; then
  printf '%s ssh-ed25519 %s\n' "$host" "$LAB_KEY_extra"
fi
MOCK
chmod +x "$tmp/bin/ssh-keyscan"
export PATH="$tmp/bin:$PATH"

iran=203.0.113.10
abroad=198.51.100.20
export LAB_IRAN_HOST="$iran" LAB_ABROAD_HOST="$abroad"
export LAB_KEY_iran="$blob_a" LAB_KEY_abroad="$blob_b" LAB_KEY_extra="$blob_c"

# --- happy path: only fingerprints leave the machine ------------------------
out=$(cd "$tmp" && "$script" --iran "$iran" --abroad "$abroad" --out "$tmp/kh") ||
  fail 'happy path failed'
grep -Fq "iran_host_key_fingerprint=$fp_a" <<<"$out" || fail 'missing Iran fingerprint'
grep -Fq "abroad_host_key_fingerprint=$fp_b" <<<"$out" || fail 'missing abroad fingerprint'
grep -Fq 'entries=2 mode=600' <<<"$out" || fail 'known_hosts file was not written 0600'
not_contains <(printf '%s\n' "$out") 'AAAAC3NzaC1lZDI1NTE5' # key material stays off stdout
[[ "$(stat -c '%a' "$tmp/kh")" == 600 ]] || fail 'known_hosts mode is not 600'
[[ "$(wc -l < "$tmp/kh")" == 2 ]] || fail 'known_hosts must hold exactly two entries'
grep -Fxq "$iran $type_a $blob_a" "$tmp/kh" || fail 'Iran entry is not a bare-host known_hosts line'
grep -Fxq "$abroad $type_b $blob_b" "$tmp/kh" || fail 'abroad entry is not a bare-host known_hosts line'

# --- explicit print and non-default ports ----------------------------------
out=$(cd "$tmp" && "$script" --iran "$iran" --iran-port 2222 --abroad "$abroad" --out "$tmp/kh2" \
  --print-known-hosts) || fail 'print path failed'
grep -Fq "[$iran]:2222 $type_a $blob_a" "$tmp/kh2" || fail 'non-22 port entry is not bracketed'
grep -Fq -- "--- paste into secret LAB_SSH_KNOWN_HOSTS" <<<"$out" || fail 'paste guidance missing'

# --- fail-closed cases ------------------------------------------------------
expect_fail() {
  local label="$1"; shift
  if (cd "$tmp" && "$script" "$@" >"$tmp/out" 2>"$tmp/err"); then
    fail "$label unexpectedly succeeded"
  fi
  echo "$label: $(tail -1 "$tmp/err")"
}
LAB_MODE=empty-iran expect_fail 'no host key' --iran "$iran" --abroad "$abroad" --out "$tmp/x1"
grep -Fq 'presented no ED25519 host key' "$tmp/err" || fail 'missing zero-key message'
LAB_MODE=double-abroad expect_fail 'two distinct host keys' --iran "$iran" --abroad "$abroad" --out "$tmp/x2"
grep -Fq 'expected exactly one' "$tmp/err" || fail 'missing duplicate-key message'
LAB_KEY_abroad="$blob_a" expect_fail 'same key on both hosts' --iran "$iran" --abroad "$abroad" --out "$tmp/x3"
LAB_KEY_abroad="$blob_b" expect_fail 'identical hosts' --iran "$iran" --abroad "$iran" --out "$tmp/x4"
expect_fail 'fingerprint mismatch' --iran "$iran" --abroad "$abroad" --out "$tmp/x5" --expect-iran "$fp_bad"
grep -Fq 'do not pin this key' "$tmp/err" || fail 'missing mismatch message'
GITHUB_ACTIONS=true expect_fail 'CI refusal' --iran "$iran" --abroad "$abroad" --out "$tmp/x6"
grep -Fq 'must never reach CI logs' "$tmp/err" || fail 'missing CI refusal message'

echo 'LAB host-key re-pin tests: PASS (real ssh-keygen keys, ssh-keyscan mocked; no network)'
