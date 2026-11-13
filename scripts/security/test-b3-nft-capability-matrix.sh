#!/usr/bin/env bash
set -euo pipefail

NFT=/usr/sbin/nft
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

echo "=== B3 capability experiment: read-only nftables listing ==="
echo "OS: $(. /etc/os-release; printf '%s %s' "$PRETTY_NAME" "$VERSION_ID")"
SYSTEMD_VERSION="$(systemctl --version | head -n 1)"
echo "systemd: $SYSTEMD_VERSION"
if ! grep -Eq '^systemd 249([. ]|$)' <<<"$SYSTEMD_VERSION"; then
  echo "ERROR: this experiment requires systemd 249; refusing to report a valid matrix." >&2
  exit 2
fi

if [[ ! -x "$NFT" ]]; then
  echo "ERROR: $NFT is unavailable." >&2
  exit 2
fi
"$NFT" --version

run_case() {
  local name="$1"
  shift
  local out="$TMP/$name.stdout" err="$TMP/$name.stderr" rc=0
  echo
  echo "--- CASE: $name ---"
  set +e
  timeout 10s "$@" >"$out" 2>"$err"
  rc=$?
  set -e
  echo "exit_code=$rc"
  echo "stdout_bytes=$(wc -c <"$out" | tr -d ' ')"
  echo "stderr_bytes=$(wc -c <"$err" | tr -d ' ')"
  echo "stdout_begin"
  head -c 4096 "$out" || true
  echo
  echo "stdout_end"
  echo "stderr_begin"
  head -c 2048 "$err" || true
  echo
  echo "stderr_end"
  printf '%s\n' "$rc" >"$TMP/$name.rc"
}

# These are read-only list operations. The before/after fingerprints detect
# any unexpected ruleset change during the experiment.
timeout 10s sudo "$NFT" --json list ruleset >"$TMP/before.json"

run_case unprivileged sudo -u nobody -- "$NFT" --json list ruleset
run_case cap_net_admin_only sudo capsh --caps=cap_net_admin+ep -- -c "grep '^CapEff:' /proc/self/status; $NFT --json list ruleset"
run_case root_without_cap_net_admin sudo capsh --drop=cap_net_admin -- -c "grep '^CapEff:' /proc/self/status; $NFT --json list ruleset"
run_case root_baseline sudo "$NFT" --json list ruleset

CAP_ONLY_EFF="$(awk '/^CapEff:/ {print $2; exit}' "$TMP/cap_net_admin_only.stdout")"
ROOT_NO_CAP_EFF="$(awk '/^CapEff:/ {print $2; exit}' "$TMP/root_without_cap_net_admin.stdout")"
if [[ -z "$CAP_ONLY_EFF" || -z "$ROOT_NO_CAP_EFF" ]]; then
  echo "ERROR: could not capture effective capability masks." >&2
  exit 6
fi
echo
echo "cap_net_admin_only_effective_mask=$CAP_ONLY_EFF"
echo "root_without_cap_net_admin_effective_mask=$ROOT_NO_CAP_EFF"
if (( 16#$CAP_ONLY_EFF != 0x1000 )); then
  echo "ERROR: CAP_NET_ADMIN-only case did not run with exactly CAP_NET_ADMIN effective." >&2
  exit 7
fi
if (( (16#$ROOT_NO_CAP_EFF & 0x1000) != 0 )); then
  echo "ERROR: root-without-CAP_NET_ADMIN case still has CAP_NET_ADMIN effective." >&2
  exit 8
fi

timeout 10s sudo "$NFT" --json list ruleset >"$TMP/after.json"
BEFORE="$(sha256sum "$TMP/before.json" | awk '{print $1}')"
AFTER="$(sha256sum "$TMP/after.json" | awk '{print $1}')"
echo
echo "ruleset_before_sha256=$BEFORE"
echo "ruleset_after_sha256=$AFTER"
if [[ "$BEFORE" != "$AFTER" ]]; then
  echo "ERROR: ruleset fingerprint changed during read-only experiment." >&2
  exit 3
fi

ROOT_RC="$(cat "$TMP/root_baseline.rc")"
CAP_RC="$(cat "$TMP/cap_net_admin_only.rc")"
if [[ "$ROOT_RC" != "0" ]]; then
  echo "ERROR: root baseline could not list the ruleset; matrix is inconclusive." >&2
  exit 4
fi
if [[ "$CAP_RC" != "0" ]]; then
  echo "ERROR: CAP_NET_ADMIN-only invocation could not list the ruleset; matrix is inconclusive." >&2
  exit 5
fi

echo
echo "B3_NFT_CAPABILITY_MATRIX=PASS"
echo "Interpretation: compare unprivileged and root-without-CAP_NET_ADMIN outcomes with CAP_NET_ADMIN-only and root baselines; ruleset fingerprint stayed unchanged."
echo "No firewall rule, service, route, sysctl, or host configuration was intentionally changed."
