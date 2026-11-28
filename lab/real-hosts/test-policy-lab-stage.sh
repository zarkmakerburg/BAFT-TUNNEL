#!/usr/bin/env bash
# Contract test for the two-host policy LAB: the stage must refuse to run half-authored, must
# prove the pack engaged for a profile and stay silent for the control, must clean up with a
# run-bounded pattern, and the measurement tool must really move bytes (checked on loopback here,
# on the LAB hosts in the workflow).
set -Eeuo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
stage="$root/lab/real-hosts/policy-lab-stage.sh"
tool="$root/lab/real-hosts/policy-lab-measure.py"
wf="$root/.github/workflows/policy-lab.yml"
work="$(mktemp -d)"
port=$(( 20000 + RANDOM % 20000 ))
server_pid=""
cleanup() {
  [[ -n "$server_pid" ]] && kill "$server_pid" 2>/dev/null || true
  rm -rf "$work"
}
trap cleanup EXIT

for f in "$stage" "$tool" "$wf"; do
  test -f "$f" || { echo "missing $f" >&2; exit 1; }
done
bash -n "$stage"
python3 -c 'import ast,sys; ast.parse(open(sys.argv[1]).read())' "$tool"

# --- the stage refuses to start without its inputs ---------------------------------------
if env -u POLICY_LAB_BINARY -u POLICY_LAB_PKI -u LAB_IRAN_HOST bash "$stage" >"$work/refuse.log" 2>&1; then
  echo "stage ran without required inputs" >&2; exit 1
fi
grep -q 'Missing LAB_IRAN_HOST' "$work/refuse.log"

# --- evidence and control assertions the stage must carry --------------------------------
grep -F 'policy pack engaged: profile=' "$stage" >/dev/null
grep -F 'env -u BAFT_TUNNEL_POLICY_PACK' "$stage" >/dev/null
grep -F 'control arm engaged the pack; the comparison would be invalid' "$stage" >/dev/null
grep -F 'did not prove the pack engaged on both hosts' "$stage" >/dev/null
grep -F 'E2E_REMOTE_TMP_CLEANUP side=$side result=pass' "$stage" >/dev/null
grep -F 'POLICY_LAB_FW_DRIFT side=$side result=none' "$stage" >/dev/null
grep -F 'POLICY_LAB_LISTENERS_UNCHANGED side=$side result=pass' "$stage" >/dev/null
grep -F 'POLICY_LAB_HOST_CLEANUP side=$side result=PASS' "$stage" >/dev/null
grep -F 'POLICY_LAB_ARM arm=$label' "$stage" >/dev/null
grep -F 'evidence=LAB' "$stage" >/dev/null
grep -F '[[ "${ARM_LIST[0]}" == control ]]' "$stage" >/dev/null
# a measured arm that did not deliver its bytes fails the run
grep -F 'if (( sha_ok != 1 )); then FAILED=1; fi' "$stage" >/dev/null
grep -F 'exit 1' "$stage" >/dev/null

# --- cleanup kills only this run's directory --------------------------------------------
grep -F "pattern=\"/tmp/\${token}/\"" "$stage" >/dev/null
if grep -E 'pkill +-[A-Z]+ +-f +.?(baft-tunnel|python3|policy-lab-measure)' "$stage" >/dev/null; then
  echo "a teardown pattern is not bounded to the run directory" >&2; exit 1
fi

# --- the workflow builds the pack, pins the commit and never runs anywhere but main ------
grep -F 'cargo +1.88.0 build --locked --release --features policy-pack' "$wf" >/dev/null
grep -F '[[ "$CANDIDATE_SHA" == "$GITHUB_SHA" ]]' "$wf" >/dev/null
grep -F "github.ref == 'refs/heads/main'" "$wf" >/dev/null
grep -F 'group: baft-tunnel-trusted-lab-hosts' "$wf" >/dev/null
grep -F 'RUN_LAB_POLICY' "$wf" >/dev/null
grep -F 'policy-lab-stage.sh' "$wf" >/dev/null
grep -F 'POLICY_LAB_TMP_CLEANUP_AFTER side=$side result=pass' "$wf" >/dev/null
grep -F 'UDP has no arm here and stays NOT TESTED' "$wf" >/dev/null
# nothing here installs, starts a unit or touches the firewall
if grep -E 'systemctl (start|enable|restart)|iptables |nft (add|insert|delete)|ufw ' "$stage" "$wf" >/dev/null; then
  echo "the policy LAB must not change host state" >&2; exit 1
fi

# --- the tool measures real bytes on loopback -------------------------------------------
python3 "$tool" server --port "$port" --bulk-size 4194304 --idle 6 --json "$work/sink.json" >"$work/server.log" 2>&1 &
server_pid=$!
for _ in $(seq 1 40); do grep -q listening "$work/server.log" 2>/dev/null && break; sleep 0.1; done
python3 "$tool" probe --target "127.0.0.1:$port" --count 20 --interval-ms 10 --json "$work/probe.json" >/dev/null 2>&1 &
probe_pid=$!
sleep 0.2
python3 "$tool" send --target "127.0.0.1:$port" --size 4194304 --json "$work/send.json" >/dev/null
wait "$probe_pid"
for _ in $(seq 1 40); do test -s "$work/sink.json" && break; sleep 0.25; done
python3 - "$work" <<'PY'
import json, sys
w = sys.argv[1]
sink = json.load(open(f"{w}/sink.json"))
sent = json.load(open(f"{w}/send.json"))
probe = json.load(open(f"{w}/probe.json"))
assert sink["bulk_complete"] and sink["bulk_total"] == 4194304, sink
assert sink["bulk_sha"] == sent["sha_expected"], "the sink's digest must equal what the sender generated"
assert probe["count"] == 20 and probe["ok"] == 20, probe
assert sink["echo_trips"] == 20, sink
assert probe["p50_us"] and probe["p95_us"]
print("policy-lab tool: 4 MiB delivered byte-exact, 20/20 round trips, p50=%sus"
      % (probe["p50_us"],))
PY

echo "policy-lab stage contract: ok"
