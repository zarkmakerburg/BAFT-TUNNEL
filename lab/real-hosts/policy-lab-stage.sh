#!/usr/bin/env bash
# Two-host policy-pack LAB: measure real (engine, carrier, profile) triples on the LAB path.
#
# What it does, per arm, on the two LAB hosts:
#   abroad: python3 measure.py server  (the tunnel's fixed destination: bulk + echo sink)
#           baft-tunnel apply --config serve.json   (listener)
#   iran:   baft-tunnel apply --config forward.json (joiner, loopback entry)
#           python3 measure.py send   (bulk flow, N MiB, deterministic)
#           python3 measure.py probe  (interactive flow, 64-byte round trips, run concurrently)
# then it stops everything and reports what the sink actually received and how long the
# interactive round trips took. Every number is measured from real sockets on the wire.
#
# Safety: everything lives under one run-owned /tmp directory; nothing is installed, no
# systemd unit is touched, no firewall/route/sysctl/SSH setting is changed, and the teardown
# kills only processes whose command line names this run's directory. It refuses to start if
# that directory or any of its ports is already in use, and it fails if anything survives it.
#
# The arms are profiles the *binary under test* can genuinely resolve: a `policy.profile` in
# the config, with `BAFT_TUNNEL_POLICY_PACK=sched` as the pack's switch. The control arm has
# neither the switch nor a profile, so it measures the same path with the product's default
# data path — that is the control every profile number is compared against.
#
# Env: LAB_IRAN_HOST LAB_ABROAD_HOST SSH_KEY_FILE SSH_KNOWN_HOSTS_FILE
#      LAB_IRAN_SSH_USER LAB_ABROAD_SSH_USER LAB_IRAN_SSH_PORT LAB_ABROAD_SSH_PORT
#      POLICY_LAB_BINARY POLICY_LAB_PKI POLICY_LAB_ARMS POLICY_LAB_SIZE_MIB
#      POLICY_LAB_PROBES POLICY_LAB_INTERVAL_MS POLICY_LAB_TUNNEL_PORT
#      POLICY_LAB_SINK_PORT POLICY_LAB_ENTRY_PORT GITHUB_RUN_ID
set -Eeuo pipefail
umask 077

: "${LAB_IRAN_HOST:?Missing LAB_IRAN_HOST}"
: "${LAB_ABROAD_HOST:?Missing LAB_ABROAD_HOST}"
: "${SSH_KEY_FILE:?Missing SSH_KEY_FILE}"
: "${SSH_KNOWN_HOSTS_FILE:?Missing SSH_KNOWN_HOSTS_FILE}"
: "${POLICY_LAB_BINARY:?Missing POLICY_LAB_BINARY}"
: "${POLICY_LAB_PKI:?Missing POLICY_LAB_PKI}"

IRAN_USER="${LAB_IRAN_SSH_USER:-root}"
ABROAD_USER="${LAB_ABROAD_SSH_USER:-root}"
IRAN_SSH_PORT="${LAB_IRAN_SSH_PORT:-22}"
ABROAD_SSH_PORT="${LAB_ABROAD_SSH_PORT:-22}"
ARMS="${POLICY_LAB_ARMS:-control balanced/1 gaming/1 trading/1 streaming/1 bulk/1}"
SIZE_MIB="${POLICY_LAB_SIZE_MIB:-32}"
PROBES="${POLICY_LAB_PROBES:-20}"
INTERVAL_MS="${POLICY_LAB_INTERVAL_MS:-50}"
TP="${POLICY_LAB_TUNNEL_PORT:-18300}"
SINK_PORT="${POLICY_LAB_SINK_PORT:-18094}"
ENTRY_PORT="${POLICY_LAB_ENTRY_PORT:-18080}"
RUN_ID="${GITHUB_RUN_ID:-local-$$}"
REMOTE_DIR="/tmp/baft-policy-lab-${RUN_ID}"
WORK="$(mktemp -d)"

die() { echo "policy-lab: $*" >&2; exit 2; }
note() { echo "policy-lab: $*" >&2; }

[[ "$LAB_IRAN_HOST" != "$LAB_ABROAD_HOST" ]] || die "the two LAB hosts must differ"
for value in "$SIZE_MIB" "$PROBES" "$INTERVAL_MS" "$TP" "$SINK_PORT" "$ENTRY_PORT"; do
  [[ "$value" =~ ^[0-9]+$ ]] || die "numeric input expected, got '$value'"
done
(( SIZE_MIB >= 4 && SIZE_MIB <= 256 )) || die "POLICY_LAB_SIZE_MIB must be 4-256"
(( PROBES >= 20 && PROBES <= 600 )) || die "POLICY_LAB_PROBES must be 20-600"
# Only now: bash evaluates the expansion, so an unvalidated value must never reach it.
BULK_BYTES=$(( SIZE_MIB << 20 ))
for port in "$TP" "$SINK_PORT" "$ENTRY_PORT"; do
  (( port >= 1024 && port <= 65535 )) || die "invalid port $port"
done
[[ "$REMOTE_DIR" =~ ^/tmp/baft-policy-lab-[A-Za-z0-9._-]+$ ]] || die "unexpected run directory"
[[ -x "$POLICY_LAB_BINARY" && ! -L "$POLICY_LAB_BINARY" ]] || die "binary is missing or unsafe"
[[ -r "$SSH_KEY_FILE" && -r "$SSH_KNOWN_HOSTS_FILE" ]] || die "SSH credentials are not readable"
for f in ca.crt ca.crl entry.crt entry.key exit.crt exit.key; do
  [[ -f "$POLICY_LAB_PKI/$f" && ! -L "$POLICY_LAB_PKI/$f" ]] || die "throwaway PKI is incomplete: $f"
done
[[ -f "$(dirname "$0")/policy-lab-measure.py" ]] || die "measurement tool is missing"
read -r -a ARM_LIST <<<"$ARMS"
[[ "${ARM_LIST[0]}" == control ]] || die "the first arm must be the control"
PROFILE_ARMS=0
for arm in "${ARM_LIST[@]}"; do
  if [[ "$arm" == control ]]; then
    continue
  fi
  [[ "$arm" =~ ^(balanced|gaming|trading|streaming|bulk)/1$ ]] || die "unknown arm '$arm'"
  PROFILE_ARMS=$(( PROFILE_ARMS + 1 ))
done
(( PROFILE_ARMS >= 1 )) || die "need at least one profile arm beside the control"

BINARY_SHA="$(sha256sum "$POLICY_LAB_BINARY" | awk '{print $1}')"
pin() { openssl x509 -in "$1" -outform DER | sha256sum | awk '{print $1}'; }
ENTRY_PIN="$(pin "$POLICY_LAB_PKI/entry.crt")"
EXIT_PIN="$(pin "$POLICY_LAB_PKI/exit.crt")"

SSH_OPTS=(-i "$SSH_KEY_FILE" -o UserKnownHostsFile="$SSH_KNOWN_HOSTS_FILE" -o StrictHostKeyChecking=yes
          -o BatchMode=yes -o ConnectTimeout=15 -o ServerAliveInterval=10 -o ServerAliveCountMax=3)
ssh_host() { # side command
  local side="$1" command="$2" host user port
  if [[ "$side" == iran ]]; then host="$LAB_IRAN_HOST"; user="$IRAN_USER"; port="$IRAN_SSH_PORT"
  else host="$LAB_ABROAD_HOST"; user="$ABROAD_USER"; port="$ABROAD_SSH_PORT"; fi
  timeout 180 ssh "${SSH_OPTS[@]}" -p "$port" "$user@$host" "$command" </dev/null
}
ssh_long() { # side command : same host, a transfer-sized bound instead of the control-path one
  local side="$1" command="$2" host user port
  if [[ "$side" == iran ]]; then host="$LAB_IRAN_HOST"; user="$IRAN_USER"; port="$IRAN_SSH_PORT"
  else host="$LAB_ABROAD_HOST"; user="$ABROAD_USER"; port="$ABROAD_SSH_PORT"; fi
  timeout 900 ssh "${SSH_OPTS[@]}" -p "$port" "$user@$host" "$command" </dev/null
}
copy_to_host() { # side source remote_path
  local side="$1" source="$2" dest="$3" host user port
  if [[ "$side" == iran ]]; then host="$LAB_IRAN_HOST"; user="$IRAN_USER"; port="$IRAN_SSH_PORT"
  else host="$LAB_ABROAD_HOST"; user="$ABROAD_USER"; port="$ABROAD_SSH_PORT"; fi
  timeout 300 scp -q "${SSH_OPTS[@]}" -P "$port" "$source" "$user@$host:$dest"
}
fetch_from_host() { # side remote_path local_path
  local side="$1" remote="$2" local="$3" host user port
  if [[ "$side" == iran ]]; then host="$LAB_IRAN_HOST"; user="$IRAN_USER"; port="$IRAN_SSH_PORT"
  else host="$LAB_ABROAD_HOST"; user="$ABROAD_USER"; port="$ABROAD_SSH_PORT"; fi
  timeout 120 scp -q "${SSH_OPTS[@]}" -P "$port" "$user@$host:$remote" "$local"
}

# --- preflight: this run must be the only owner of its directory and ports ----------------
# The baseline also records the firewall ruleset digest and the listener count, so the teardown
# can prove this run left both exactly as it found them.
declare -A FW_BEFORE LISTENERS_BEFORE
for side in abroad iran; do
  baseline="$(ssh_host "$side" "set -eu
    test \"\$(id -u)\" = 0
    test ! -e '$REMOTE_DIR' && test ! -L '$REMOTE_DIR'
    if ss -ltnH 2>/dev/null | awk '{print \$4}' | grep -qE ':$TP\$'; then echo 'tunnel port in use' >&2; exit 3; fi
    if ss -ltnH 2>/dev/null | awk '{print \$4}' | grep -qE ':$SINK_PORT\$'; then echo 'sink port in use' >&2; exit 3; fi
    if ss -ltnH 2>/dev/null | awk '{print \$4}' | grep -qE ':$ENTRY_PORT\$'; then echo 'entry port in use' >&2; exit 3; fi
    if ! nft --stateless list ruleset > /dev/null 2>&1; then echo 'firewall unreadable' >&2; exit 4; fi
    fw=\$(nft --stateless list ruleset | sha256sum | awk '{print \$1}')
    listeners=\$(ss -ltnH 2>/dev/null | wc -l)
    echo \"fw=\$fw listeners=\$listeners\"")" || die "preflight failed on $side"
  FW_BEFORE[$side]="${baseline#*fw=}"; FW_BEFORE[$side]="${FW_BEFORE[$side]%% *}"
  LISTENERS_BEFORE[$side]="${baseline##*listeners=}"
  echo "POLICY_LAB_PREFLIGHT side=$side result=ok fw_ruleset_sha256=${FW_BEFORE[$side]} listeners=${LISTENERS_BEFORE[$side]}"
done

cleanup_remote() { # side -> removes this run's directory and every process that names it
  local side="$1" pattern token
  token="${REMOTE_DIR#/tmp/}"
  pattern="/tmp/${token}/"
  ssh_host "$side" "set +e
    pkill -TERM -f '${pattern}[b]aft-tunnel' 2>/dev/null
    pkill -TERM -f '${pattern}policy-lab-measure[.]py' 2>/dev/null
    sleep 1
    pkill -KILL -f '${pattern}[b]aft-tunnel' 2>/dev/null
    pkill -KILL -f '${pattern}policy-lab-measure[.]py' 2>/dev/null
    sleep 1
    rm -rf '$REMOTE_DIR'
    remaining=\$(pgrep -f \"(${pattern}[b]aft-tunnel|${pattern}policy-lab-measure[.]py)\" | wc -l)
    if [ -e '$REMOTE_DIR' ] || [ \"\$remaining\" != 0 ]; then echo 'leftover'; exit 1; fi
    echo cleaned" || return 1
}
cleanup_all() { # always runs; a leftover or a changed host must fail the stage
  local status=0 side_status side after fw listeners
  for side in abroad iran; do
    side_status=0
    if cleanup_remote "$side" >/dev/null 2>&1; then
      echo "E2E_REMOTE_TMP_CLEANUP side=$side result=pass"
    else
      echo "E2E_REMOTE_TMP_CLEANUP side=$side result=fail" >&2
      side_status=1
    fi
    after="$(ssh_host "$side" "set +e
      if ! nft --stateless list ruleset > /dev/null 2>&1; then echo 'fw=unreadable listeners=0'; exit 0; fi
      fw=\$(nft --stateless list ruleset | sha256sum | awk '{print \$1}')
      listeners=\$(ss -ltnH 2>/dev/null | wc -l)
      echo \"fw=\$fw listeners=\$listeners\"" 2>/dev/null)" || after=""
    fw="${after#*fw=}"; fw="${fw%% *}"
    listeners="${after##*listeners=}"
    if [[ -n "$after" && "$fw" != unreadable && "$fw" == "${FW_BEFORE[$side]:-}" ]]; then
      echo "POLICY_LAB_FW_DRIFT side=$side result=none"
    else
      echo "POLICY_LAB_FW_DRIFT side=$side result=changed" >&2
      side_status=1
    fi
    if [[ -n "$after" && "$listeners" == "${LISTENERS_BEFORE[$side]:-}" ]]; then
      echo "POLICY_LAB_LISTENERS_UNCHANGED side=$side result=pass count=$listeners"
    else
      echo "POLICY_LAB_LISTENERS_UNCHANGED side=$side result=fail before=${LISTENERS_BEFORE[$side]:-?} after=${listeners:-?}" >&2
      side_status=1
    fi
    if (( side_status == 0 )); then
      echo "POLICY_LAB_HOST_CLEANUP side=$side result=PASS"
    else
      echo "POLICY_LAB_HOST_CLEANUP side=$side result=FAIL" >&2
      status=1
    fi
  done
  return "$status"
}
trap 'rc=$?; set +e; cleanup_all; crc=$?; (( rc == 0 && crc != 0 )) && rc=$crc; exit "$rc"' EXIT

# --- deploy this run's directory to both hosts --------------------------------------------
for side in abroad iran; do
  ssh_host "$side" "set -eu; mkdir -m 700 '$REMOTE_DIR'" || die "could not create the run directory on $side"
done
copy_to_host abroad "$POLICY_LAB_BINARY" "$REMOTE_DIR/baft-tunnel" || die "binary copy to abroad failed"
copy_to_host iran "$POLICY_LAB_BINARY" "$REMOTE_DIR/baft-tunnel" || die "binary copy to iran failed"
copy_to_host abroad "$(dirname "$0")/policy-lab-measure.py" "$REMOTE_DIR/policy-lab-measure.py" || die "tool copy failed"
copy_to_host iran "$(dirname "$0")/policy-lab-measure.py" "$REMOTE_DIR/policy-lab-measure.py" || die "tool copy failed"
tar -C "$POLICY_LAB_PKI" -czf "$WORK/pki.tgz" .
for side in abroad iran; do
  copy_to_host "$side" "$WORK/pki.tgz" "$REMOTE_DIR/pki.tgz" || die "PKI copy to $side failed"
  ssh_host "$side" "set -eu
    cd '$REMOTE_DIR'
    mkdir -m 700 pki
    tar -xzf pki.tgz -C pki && rm -f pki.tgz
    chmod 700 baft-tunnel
    test \"\$(sha256sum baft-tunnel | awk '{print \$1}')\" = '$BINARY_SHA'" || die "binary digest differs on $side"
done

# --- per-arm configs ---------------------------------------------------------------------
# Written here and streamed to the host so the JSON is reviewable in this script's structure.
write_config() { # arm mode outfile
  local arm="$1" mode="$2" out="$3" policy=""
  [[ "$arm" != control ]] && policy=",\"policy\":{\"profile\":\"$arm\"}"
  if [[ "$mode" == serve ]]; then
    cat >"$out" <<EOF
{
  "mode": "serve",
  "listen": "0.0.0.0:$TP",
  "target": "127.0.0.1:$SINK_PORT",
  "local_id": "exit",
  "peer_id": "entry",
  "tunnel_id": "policy-lab",
  "direction": "direct",
  "protocol_version": 2,
  "peer_fingerprint": "$ENTRY_PIN",
  "ca": "$REMOTE_DIR/pki/ca.crt",
  "crl": "$REMOTE_DIR/pki/ca.crl",
  "cert": "$REMOTE_DIR/pki/exit.crt",
  "key": "$REMOTE_DIR/pki/exit.key",
  "engine": "tcp-tls",
  "max_streams": 8$policy
}
EOF
  else
    cat >"$out" <<EOF
{
  "mode": "forward",
  "listen": "127.0.0.1:$ENTRY_PORT",
  "remote": "$LAB_ABROAD_HOST:$TP",
  "server_name": "exit.bench",
  "local_id": "entry",
  "peer_id": "exit",
  "tunnel_id": "policy-lab",
  "direction": "direct",
  "protocol_version": 2,
  "peer_fingerprint": "$EXIT_PIN",
  "ca": "$REMOTE_DIR/pki/ca.crt",
  "crl": "$REMOTE_DIR/pki/ca.crl",
  "cert": "$REMOTE_DIR/pki/entry.crt",
  "key": "$REMOTE_DIR/pki/entry.key",
  "engine": "tcp-tls",
  "max_streams": 8$policy
}
EOF
  fi
}

stop_arm() { # side : bounded stop of this arm's processes only
  local side="$1" pattern="/tmp/${REMOTE_DIR#/tmp/}/"
  ssh_host "$side" "set +e
    pkill -TERM -f '${pattern}[b]aft-tunnel' 2>/dev/null
    pkill -TERM -f '${pattern}policy-lab-measure[.]py' 2>/dev/null
    sleep 1
    pkill -KILL -f '${pattern}[b]aft-tunnel' 2>/dev/null
    pkill -KILL -f '${pattern}policy-lab-measure[.]py' 2>/dev/null
    true" >/dev/null 2>&1 || true
}

json_value() { # file key -> value or empty
  python3 - "$1" "$2" <<'PY' 2>/dev/null || true
import json, sys
try:
    value = json.load(open(sys.argv[1]))
except Exception:
    raise SystemExit
current = value
for part in sys.argv[2].split("."):
    current = current.get(part) if isinstance(current, dict) else None
print("" if current is None else current)
PY
}

note "run=$RUN_ID binary_sha256=$BINARY_SHA size_mib=$SIZE_MIB probes=$PROBES arms=${#ARM_LIST[@]}"
CONTROL_LINE=""
FAILED=0
for arm in "${ARM_LIST[@]}"; do
  label="${arm//\//-}"   # balanced/1 -> balanced-1 for filenames; $arm stays the profile id
  pack=sched
  if [[ "$arm" == control ]]; then pack=off; fi
  write_config "$arm" serve "$WORK/serve.json"
  write_config "$arm" forward "$WORK/forward.json"
  copy_to_host abroad "$WORK/serve.json" "$REMOTE_DIR/serve.json" || die "config copy failed"
  copy_to_host iran "$WORK/forward.json" "$REMOTE_DIR/forward.json" || die "config copy failed"
  ssh_host abroad "set -eu; cd '$REMOTE_DIR'; ./baft-tunnel config validate --config serve.json" >/dev/null ||
    die "serve config was refused on abroad for $arm"
  ssh_host iran "set -eu; cd '$REMOTE_DIR'; ./baft-tunnel config validate --config forward.json" >/dev/null ||
    die "forward config was refused on iran for $arm"

  # sink first (it is the tunnel's target), then the listener, then the joiner
  ssh_host abroad "set -eu; cd '$REMOTE_DIR'
    setsid nohup python3 '$REMOTE_DIR/policy-lab-measure.py' server --port $SINK_PORT --bulk-size $BULK_BYTES \
      --idle 30 --json sink-$label.json > server-$label.log 2>&1 < /dev/null &
    for n in \$(seq 1 40); do grep -q listening server-$label.log 2>/dev/null && exit 0; sleep 0.25; done
    exit 1" >/dev/null || die "sink did not start on abroad for $arm"
  if [[ "$pack" == sched ]]; then
    ssh_host abroad "set -eu; cd '$REMOTE_DIR'
      BAFT_TUNNEL_POLICY_PACK=sched setsid nohup '$REMOTE_DIR/baft-tunnel' apply --config serve.json --confirm \
        > serve-$label.log 2>&1 < /dev/null &
      sleep 2; ss -ltnH | awk '{print \$4}' | grep -qE ':$TP\$'" >/dev/null ||
      die "listener did not open $TP on abroad for $arm"
    ssh_host iran "set -eu; cd '$REMOTE_DIR'
      BAFT_TUNNEL_POLICY_PACK=sched setsid nohup '$REMOTE_DIR/baft-tunnel' apply --config forward.json --confirm \
        > forward-$label.log 2>&1 < /dev/null &
      sleep 2; ss -ltnH | awk '{print \$4}' | grep -qE ':$ENTRY_PORT\$'" >/dev/null ||
      die "joiner did not open $ENTRY_PORT on iran for $arm"
  else
    ssh_host abroad "set -eu; cd '$REMOTE_DIR'
      setsid nohup '$REMOTE_DIR/baft-tunnel' apply --config serve.json --confirm > serve-$label.log 2>&1 < /dev/null &
      sleep 2; ss -ltnH | awk '{print \$4}' | grep -qE ':$TP\$'" >/dev/null ||
      die "listener did not open $TP on abroad for $arm"
    ssh_host iran "set -eu; cd '$REMOTE_DIR'
      env -u BAFT_TUNNEL_POLICY_PACK setsid nohup '$REMOTE_DIR/baft-tunnel' apply --config forward.json --confirm > forward-$label.log 2>&1 < /dev/null &
      sleep 2; ss -ltnH | awk '{print \$4}' | grep -qE ':$ENTRY_PORT\$'" >/dev/null ||
      die "joiner did not open $ENTRY_PORT on iran for $arm"
  fi

  # The probe samples strictly inside the bulk window: it waits for the go file, which is
  # raised once the sender is connected, and it stops when the send completes. A probe that
  # ran before or after the transfer would report an idle path, not a loaded one.
  ssh_long iran "cd '$REMOTE_DIR'
    rm -f probe.go probe.done
    nohup python3 '$REMOTE_DIR/policy-lab-measure.py' probe --target 127.0.0.1:$ENTRY_PORT \
      --interval-ms $INTERVAL_MS --max-seconds 600 --go-file probe.go --stop-file probe.done \
      --json probe-$label.json > probe-$label.log 2>&1 < /dev/null &
    nohup python3 '$REMOTE_DIR/policy-lab-measure.py' send --target 127.0.0.1:$ENTRY_PORT \
      --size $BULK_BYTES --start-file probe.go --json send-$label.json > send-$label.log 2>&1 < /dev/null &
    for n in \$(seq 1 600); do test -s send-$label.json && break; sleep 0.5; done
    touch probe.done
    for n in \$(seq 1 120); do test -s probe-$label.json && break; sleep 0.5; done
    true" || note "sender reported a failure for $arm"
  # the sink finishes on its own idle timer once the bulk is complete
  for n in $(seq 1 60); do
    ssh_host abroad "test -s '$REMOTE_DIR/sink-$label.json'" >/dev/null 2>&1 && break
    sleep 2
  done
  fetch_from_host abroad "$REMOTE_DIR/sink-$label.json" "$WORK/sink-$label.json" 2>/dev/null || true
  ssh_host iran "cat '$REMOTE_DIR/send-$label.json' 2>/dev/null || true" >"$WORK/send-$label.json" || true
  ssh_host iran "cat '$REMOTE_DIR/probe-$label.json' 2>/dev/null || true" >"$WORK/probe-$label.json" || true

  ssh_host abroad "grep -c 'policy pack engaged: profile=' '$REMOTE_DIR/serve-$label.log' 2>/dev/null || true" >"$WORK/engaged-serve-$label.txt" || true
  ssh_host iran "grep -c 'policy pack engaged: profile=' '$REMOTE_DIR/forward-$label.log' 2>/dev/null || true" >"$WORK/engaged-forward-$label.txt" || true
  engaged_serve="$(tr -dc '0-9' <"$WORK/engaged-serve-$label.txt" | head -c 3)"
  engaged_forward="$(tr -dc '0-9' <"$WORK/engaged-forward-$label.txt" | head -c 3)"
  engaged_both=0
  if [[ "$arm" == control ]]; then
    # The control arm has no switch and no profile: the pack must stay silent on both hosts,
    # otherwise the comparison is between two packed runs and proves nothing. One positive
    # count is enough to mark the arm engaged, so the check below fails the run.
    if (( ${engaged_serve:-0} == 0 && ${engaged_forward:-0} == 0 )); then
      engaged_both=0
    else
      engaged_both=1
    fi
  else
    (( ${engaged_serve:-0} >= 1 && ${engaged_forward:-0} >= 1 )) && engaged_both=1
  fi
  delivered="$(json_value "$WORK/sink-$label.json" bulk_total)"
  sha_ok=0
  if [[ "$(json_value "$WORK/sink-$label.json" bulk_sha)" == "$(json_value "$WORK/send-$label.json" sha_expected)" &&
        "${delivered:-0}" == "$BULK_BYTES" ]]; then sha_ok=1; fi
  sink_secs="$(python3 - "$WORK/sink-$label.json" <<'PY' 2>/dev/null || true
import json, sys
try:
    v = json.load(open(sys.argv[1]))
    t0, t1 = v.get("t0"), v.get("t1")
    print("" if not (t0 and t1 and t1 > t0) else round(t1 - t0, 3))
except Exception:
    pass
PY
)"
  sink_mibs="$(python3 - "${sink_secs:-0}" "$BULK_BYTES" <<'PY' 2>/dev/null || true
import sys
secs = float(sys.argv[1] or 0)
print("" if secs <= 0 else round(int(sys.argv[2]) / secs / 1048576, 2))
PY
)"
  probe_ok="$(json_value "$WORK/probe-$label.json" ok)"
  sink_echo_streams="$(json_value "$WORK/sink-$label.json" echo_streams)"
  sink_echo_trips="$(json_value "$WORK/sink-$label.json" echo_trips)"
  probe_p50="$(json_value "$WORK/probe-$label.json" p50_us)"
  probe_p95="$(json_value "$WORK/probe-$label.json" p95_us)"
  probe_window="$(json_value "$WORK/probe-$label.json" window_s)"
  sender_mibs="$(json_value "$WORK/send-$label.json" sender_MiB_per_s)"

  line="POLICY_LAB_ARM arm=$label profile=$([[ $arm == control ]] && echo none || echo "$arm") pack=$pack pack_engaged=$engaged_both engine=tcp-tls carrier=tcp bytes=$BULK_BYTES delivered_bytes=${delivered:-0} sink_secs=${sink_secs:-none} sink_MiB_per_s=${sink_mibs:-none} sender_MiB_per_s=${sender_mibs:-none} interactive_p50_us=${probe_p50:-none} interactive_p95_us=${probe_p95:-none} probe_samples=${probe_ok:-0} probe_window_s=${probe_window:-none} probe_min=$PROBES sink_echo_streams=${sink_echo_streams:-0} sha_match=$sha_ok evidence=LAB"
  echo "$line"
  [[ "$arm" == control ]] && CONTROL_LINE="$line"
  if (( sha_ok != 1 )); then FAILED=1; fi
  if [[ "$arm" == control ]]; then
    (( engaged_both == 0 )) || { note "control arm engaged the pack; the comparison would be invalid"; FAILED=1; }
  else
    (( engaged_both == 1 )) || { note "$arm did not prove the pack engaged on both hosts"; FAILED=1; }
  fi
  # The competition claim needs the sink to have seen the interactive stream at the same time as
  # the bulk: one destination, two flows, one tunnel session.
  if [[ -z "${sink_echo_streams:-}" ]] || (( ${sink_echo_streams:-0} < 1 )); then
    note "$arm's sink never saw the interactive stream; the two flows did not share the session"
    FAILED=1
  fi
  if [[ -z "${probe_ok:-}" ]] || (( ${probe_ok:-0} < PROBES )); then
    note "$arm produced ${probe_ok:-0} interactive samples inside the bulk window; $PROBES are required"
    FAILED=1
  fi
  ssh_host abroad "cd '$REMOTE_DIR'; tail -n 5 serve-$label.log 2>/dev/null | sed 's/^/  serve: /'" >&2 || true
  ssh_host iran "cd '$REMOTE_DIR'; tail -n 5 forward-$label.log 2>/dev/null | sed 's/^/  forward: /'" >&2 || true
  stop_arm abroad
  stop_arm iran
  sleep 1
done

[[ -n "$CONTROL_LINE" ]] || { echo "policy-lab: the control arm produced no line" >&2; FAILED=1; }

if (( FAILED != 0 )); then
  echo "POLICY_LAB_RESULT result=fail arms=${#ARM_LIST[@]} binary_sha256=$BINARY_SHA (a measured arm did not deliver its bytes or its interactive sample; see the lines above)" >&2
  exit 1
fi
echo "POLICY_LAB_RESULT result=pass arms=${#ARM_LIST[@]} control=off measured_triples=$((${#ARM_LIST[@]} - 1)) binary_sha256=$BINARY_SHA runner_dir=$REMOTE_DIR (engine=tcp-tls carrier=TCP/Yamux profile rows above; LAB evidence for this path and window only)"
