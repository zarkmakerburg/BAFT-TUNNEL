#!/usr/bin/env bash
set -Eeuo pipefail

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
stage="$root/lab/real-hosts/auto-pair-stage.sh"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT HUP INT TERM

# Exercise the actual production shell function with a mocked API and clock.
sed -n '/^ap_verify_diag() {/,/^}/p' "$stage" > "$tmp/ap-verify-function.sh"
[[ -s "$tmp/ap-verify-function.sh" ]] || { echo "could not extract ap_verify_diag" >&2; exit 1; }
. "$tmp/ap-verify-function.sh"

LOCAL_TMP="$tmp"
AP_PROBE_COOLDOWN=60
ap_api_calls=0
sleep_calls=0
diag_calls=0
response_mode=rate_then_pass

fail() { echo "automatic-pair integrity retry test: $*" >&2; exit 1; }
ap_api() {
  ap_api_calls=$((ap_api_calls + 1))
  case "$response_mode:$ap_api_calls" in
    rate_then_pass:1)
      printf '%s\n' '{"result":"unknown","reason":"rate_limited"}'
      ;;
    rate_then_pass:2)
      printf '%s\n' '{"result":"pass","sha256_match":true,"bytes":65536,"direction":"direct"}'
      ;;
    rate_still_limited:*)
      printf '%s\n' '{"result":"unknown","reason":"rate_limited"}'
      ;;
    integrity_fail:*)
      printf '%s\n' '{"result":"fail","sha256_match":false,"bytes":65536,"direction":"direct","reason":"integrity_mismatch"}'
      ;;
    timeout:*)
      printf '%s\n' '{"result":"unknown","sha256_match":false,"bytes":0,"direction":"direct","reason":"timeout"}'
      ;;
    *) fail "unexpected API call $response_mode:$ap_api_calls" ;;
  esac
}
sleep() {
  [[ "$1" == "$((AP_PROBE_COOLDOWN + 1))" ]] || fail "cooldown wait was $1 seconds, expected 61"
  sleep_calls=$((sleep_calls + 1))
}
ap_diag() { diag_calls=$((diag_calls + 1)); }

# The exact rate_limited outcome waits out the product cooldown once, then still requires PASS.
ap_verify_diag iran apd direct
[[ "$ap_api_calls" == 2 && "$sleep_calls" == 1 && "$diag_calls" == 1 ]] ||
  fail "rate_limited retry counts were API=$ap_api_calls sleep=$sleep_calls diagnostics=$diag_calls"

# A second rate limit after the single retry remains a failure; there is no retry loop.
ap_api_calls=0; sleep_calls=0; diag_calls=0; response_mode=rate_still_limited
if ap_verify_diag iran apd direct >/dev/null 2>&1; then
  fail "a second rate_limited response was accepted"
fi
[[ "$ap_api_calls" == 2 && "$sleep_calls" == 1 && "$diag_calls" == 0 ]] ||
  fail "second rate_limited response was retried or masked"

# A genuine integrity failure is not retried or converted into success.
ap_api_calls=0; sleep_calls=0; diag_calls=0; response_mode=integrity_fail
if ap_verify_diag iran apd direct >/dev/null 2>&1; then
  fail "integrity mismatch was accepted"
fi
[[ "$ap_api_calls" == 1 && "$sleep_calls" == 0 && "$diag_calls" == 0 ]] ||
  fail "integrity mismatch was retried or masked"

# Other UNKNOWN outcomes (for example timeout) are not retried either.
ap_api_calls=0; sleep_calls=0; diag_calls=0; response_mode=timeout
if ap_verify_diag iran apd direct >/dev/null 2>&1; then
  fail "timeout UNKNOWN was accepted"
fi
[[ "$ap_api_calls" == 1 && "$sleep_calls" == 0 && "$diag_calls" == 0 ]] ||
  fail "timeout was retried or masked"

echo "Automatic pairing integrity cooldown retry: PASS (mock API/clock only)"
