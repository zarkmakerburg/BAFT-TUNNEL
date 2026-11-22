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
declare -A receipt_response=()
api_keys=()

fail() { echo "automatic-pair integrity retry test: $*" >&2; exit 1; }
reset_mock() {
  ap_api_calls=0; sleep_calls=0; diag_calls=0; api_keys=()
  unset receipt_response
  declare -gA receipt_response=()
}
ap_api() {
  local request_body request_key response
  request_body="$(cat)"
  request_key="$(python3 -c 'import json,sys; print(json.load(sys.stdin)["idempotency_key"])' <<<"$request_body")"
  api_keys+=("$request_key")
  ap_api_calls=$((ap_api_calls + 1))

  # Model the production receipt journal: duplicate keys replay the stored response
  # and do not execute a new probe.
  if [[ -n "${receipt_response[$request_key]+present}" ]]; then
    python3 -c 'import json,sys; v=json.load(sys.stdin); v["replayed"]=True; print(json.dumps(v))' <<<"${receipt_response[$request_key]}"
    return 0
  fi

  case "$response_mode:$ap_api_calls" in
    rate_then_pass:1|rate_still_limited:*)
      response='{"result":"unknown","reason":"rate_limited","replayed":false}'
      ;;
    rate_then_pass:2)
      response='{"result":"pass","sha256_match":true,"bytes":65536,"direction":"direct","replayed":false}'
      ;;
    integrity_fail:*)
      response='{"result":"fail","sha256_match":false,"bytes":65536,"direction":"direct","reason":"integrity_mismatch"}'
      ;;
    timeout:*)
      response='{"result":"unknown","sha256_match":false,"bytes":0,"direction":"direct","reason":"timeout"}'
      ;;
    *) fail "unexpected API call $response_mode:$ap_api_calls" ;;
  esac
  receipt_response[$request_key]="$response"
  printf '%s\n' "$response"
}
sleep() {
  [[ "$1" == "$((AP_PROBE_COOLDOWN + 1))" ]] || fail "cooldown wait was $1 seconds, expected 61"
  sleep_calls=$((sleep_calls + 1))
}
ap_diag() { diag_calls=$((diag_calls + 1)); }

# A rate-limited receipt is cached. Recovery therefore requires a new idempotency key.
ap_verify_diag iran apd direct
[[ "$ap_api_calls" == 2 && "$sleep_calls" == 1 && "$diag_calls" == 1 ]] ||
  fail "rate_limited retry counts were API=$ap_api_calls sleep=$sleep_calls diagnostics=$diag_calls"
[[ "${api_keys[0]}" != "${api_keys[1]}" ]] ||
  fail "retry reused the original idempotency key"
[[ -f "$tmp/ap-verify-apd-iran.json.rate-limited" ]] ||
  fail "first rate-limited response was not preserved"
python3 - "$tmp/ap-verify-apd-iran.json.rate-limited" "$tmp/ap-verify-apd-iran.json" <<'PY'
import json, sys
first = json.load(open(sys.argv[1]))
retry = json.load(open(sys.argv[2]))
assert first.get("result") == "unknown" and first.get("reason") == "rate_limited", first
assert retry.get("result") == "pass" and retry.get("replayed") is False, retry
PY

# A second rate limit after the single retry remains a failure; there is no retry loop.
reset_mock; response_mode=rate_still_limited
if ap_verify_diag iran apd direct >/dev/null 2>&1; then
  fail "a second rate_limited response was accepted"
fi
[[ "$ap_api_calls" == 2 && "$sleep_calls" == 1 && "$diag_calls" == 0 ]] ||
  fail "second rate_limited response was retried or masked"
[[ "${api_keys[0]}" != "${api_keys[1]}" ]] ||
  fail "second attempt reused the first idempotency key"

# A genuine integrity failure is not retried or converted into success.
reset_mock; response_mode=integrity_fail
if ap_verify_diag iran apd direct >/dev/null 2>&1; then
  fail "integrity mismatch was accepted"
fi
[[ "$ap_api_calls" == 1 && "$sleep_calls" == 0 && "$diag_calls" == 0 ]] ||
  fail "integrity mismatch was retried or masked"

# Other UNKNOWN outcomes (for example timeout) are not retried either.
reset_mock; response_mode=timeout
if ap_verify_diag iran apd direct >/dev/null 2>&1; then
  fail "timeout UNKNOWN was accepted"
fi
[[ "$ap_api_calls" == 1 && "$sleep_calls" == 0 && "$diag_calls" == 0 ]] ||
  fail "timeout was retried or masked"

echo "Automatic pairing integrity cooldown retry: PASS (mock API/clock with receipt replay)"
