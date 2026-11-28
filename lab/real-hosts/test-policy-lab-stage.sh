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
grep -F '(( PROFILE_ARMS >= 1 ))' "$stage" >/dev/null
# the profile id keeps its slash in the config, but files use a safe label
grep -F 'label="${arm//\//-}"' "$stage" >/dev/null
# an unvalidated value must never reach bash arithmetic expansion
python3 - "$stage" <<'PY2'
import re, sys
text = open(sys.argv[1]).read()
assert text.index('must be 4-256') < text.index('BULK_BYTES=$(( SIZE_MIB << 20 ))'), \
    'SIZE_MIB reaches arithmetic expansion before its bounds are checked'
PY2
# teardown patterns cannot match the shell that runs them
grep -F "policy-lab-measure[.]py" "$stage" >/dev/null
if grep -F "pkill -TERM -f '\${pattern}policy-lab-measure.py'" "$stage" >/dev/null; then
  echo "an unbracketed teardown pattern can match its own shell" >&2; exit 1
fi
# the firewall snapshot is stateless and its failure is not masked
grep -F 'nft --stateless list ruleset' "$stage" >/dev/null
grep -F 'firewall unreadable' "$stage" >/dev/null
grep -F '"$fw" != unreadable' "$stage" >/dev/null
# the probe samples only inside the bulk window, at a floor of loaded samples
grep -F -- '--go-file probe.go --stop-file probe.done' "$stage" >/dev/null
grep -F 'probe.go' "$stage" >/dev/null
grep -F 'touch probe.done' "$stage" >/dev/null
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
python3 "$tool" server --port "$port" --bulk-size 134217728 --idle 3 --json "$work/sink.json" >"$work/server.log" 2>&1 &
server_pid=$!
for _ in $(seq 1 40); do grep -q listening "$work/server.log" 2>/dev/null && break; sleep 0.1; done
# window: the probe waits for the go file (raised after the sender connects) and stops when the
# bulk finishes, exactly as the stage does on the hosts.
python3 "$tool" probe --target "127.0.0.1:$port" --interval-ms 5 --max-seconds 60 \
  --go-file "$work/go" --stop-file "$work/stop" --json "$work/probe.json" >/dev/null 2>&1 &
probe_pid=$!
python3 "$tool" send --target "127.0.0.1:$port" --size 134217728 --start-file "$work/go" \
  --json "$work/send.json" >/dev/null &
send_pid=$!
wait "$send_pid"
touch "$work/stop"
wait "$probe_pid"
# the sink writes its mid-run record when the bulk completes and its final one when it exits;
# wait for the process so the numbers read below are the final measurement
for _ in $(seq 1 60); do kill -0 "$server_pid" 2>/dev/null || break; sleep 0.25; done
wait "$server_pid" 2>/dev/null || true
server_pid=""
for _ in $(seq 1 40); do test -s "$work/sink.json" && break; sleep 0.25; done
python3 - "$work" <<'PY'
import json, sys
w = sys.argv[1]
sink = json.load(open(f"{w}/sink.json"))
sent = json.load(open(f"{w}/send.json"))
probe = json.load(open(f"{w}/probe.json"))
assert sink["bulk_complete"] and sink["bulk_total"] == 134217728, sink
assert sink["bulk_sha"] == sent["sha_expected"], "the sink's digest must equal what the sender generated"
# loopback finishes 128 MiB in ~150 ms, so this window is short by nature; the floor that
# matters on the hosts (POLICY_LAB_PROBES inside a multi-second bulk) is asserted by the stage.
assert probe["ok"] >= 10, probe
assert probe["ok"] == probe["count"], probe
# The sink's record is stamped when the bulk completes, so it cannot hold echoes the probe
# measured afterwards; what it must show is that both flows used the same destination.
assert sink["echo_streams"] >= 1, sink
assert sink["echo_trips"] == probe["ok"], (sink["echo_trips"], probe["ok"])
assert probe["p50_us"] and probe["p95_us"] and probe["window_s"] > 0
print("policy-lab tool: 128 MiB delivered byte-exact, %d round trips inside the bulk window, p50=%sus"
      % (probe["ok"], probe["p50_us"]))
PY

echo "policy-lab stage contract: ok"
