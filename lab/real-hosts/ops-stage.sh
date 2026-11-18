# shellcheck shell=bash
# Direct (tcp-tls) operations stage, sourced by phase1-two-host-e2e.sh when LAB_OPS>0 (soak seconds).
# Installed product path on both LAB hosts: clean install, guided setup (--direction direct), Apply start through the
# non-root local agent into hardened systemd units, diagnostics/status/traffic evidence, a bounded soak (max 3600 s), reconnect,
# backup/restore, guarded update with automatic rollback of a failing binary, revocation and uninstall.
# Only resources this stage creates are removed; firewall, listeners and other units are compared.
# Topology: abroad = serve (listens on the tunnel port, owns the service target), iran = forward (dials, owns the ingress).

echo "=== DIRECT OPS: clean install from ${MVP_SOURCE}, Direct via setup+Apply, ${OPS_SOAK}s soak, backup/restore, update/rollback ==="
stop_role iran serve; stop_role abroad serve; stop_role iran forward; stop_role abroad forward; sleep 1
MSVC=18094; MTP=18330; MAG=18096
EB=/etc/baft-tunnel; UB=/usr/local/bin/baft-tunnel; AGD=/var/lib/baft-tunnel-agent/lab-agent; PKG="$REMOTE_DIR/pkg"
CLI=opscli; SRV=opssrv          # instance names on iran (forward) / abroad (serve)
# Optional ADR-0049 session rotation on the forward (iran) node after the upgrade: LAB_ROTATE_SECS 0 = off (default), else 5..60.
ROTATE_SECS="${LAB_ROTATE_SECS:-0}"
[[ "$ROTATE_SECS" =~ ^[0-9]+$ ]] && (( ROTATE_SECS == 0 || (ROTATE_SECS >= 5 && ROTATE_SECS <= 60) )) || { echo "LAB_ROTATE_SECS must be 0 or 5-60" >&2; exit 2; }
MVP_SHA="$(sha256sum "$MVP_BINARY_PATH" | awk '{print $1}')"
MVP_INITIAL_BINARY_PATH="${MVP_INITIAL_BINARY:-$MVP_BINARY_PATH}"
[[ -f "$MVP_INITIAL_BINARY_PATH" && ! -L "$MVP_INITIAL_BINARY_PATH" && -x "$MVP_INITIAL_BINARY_PATH" ]] || { echo "MVP_INITIAL_BINARY must be an executable regular file" >&2; exit 2; }
OP_INITIAL_SHA="$(sha256sum "$MVP_INITIAL_BINARY_PATH" | awk '{print $1}')"

side_snapshot() { # side : normalized firewall rules, listeners outside the test ports, baft-tunnel* unit-file count
  ssh_host "$1" "echo '# firewall'; { nft list ruleset 2>/dev/null | sed -E 's/packets [0-9]+ bytes [0-9]+//'; iptables-save 2>/dev/null | grep -v '^#' | sed -E 's/\\[[0-9]+:[0-9]+\\]//'; };
    echo '# listeners'; ss -ltnH | awk '{print \$4}' | grep -vE ':($MSVC|$MTP|$MAG|$E2E_PORT|$LOCAL_PORT|$ECHO_PORT)\$' | sort -u;
    echo '# units'; systemctl list-unit-files --no-legend 'baft-tunnel*' 2>/dev/null | wc -l"
}
echo "--- clean-host check ---"
OP_USER_BEFORE=(); OP_FW_DRIFT=()
for side in iran abroad; do
  ssh_host "$side" "! test -e $UB && ! test -e $EB && ! test -e /var/lib/baft-tunnel && [ \"\$(systemctl list-unit-files --no-legend 'baft-tunnel*' | wc -l)\" = 0 ]" ||
    { echo "$side is not clean: an earlier BAFT-TUNNEL install is present (binary, $EB, state or units)" >&2; exit 1; }
  if ssh_host "$side" "id baft-tunnel >/dev/null 2>&1"; then OP_USER_BEFORE+=("$side"); fi
  side_snapshot "$side" > "$LOCAL_TMP/op-before-$side.txt"
done

op_cleanup() {
  for side in iran abroad; do
    ssh_host "$side" "for i in $CLI $SRV; do touch /run/baft-tunnel/control/\$i.stop 2>/dev/null; systemctl stop baft-tunnel@\$i.service 2>/dev/null; bash '$PKG/scripts/install-agent-control.sh' --remove --instance \$i >/dev/null 2>&1; done;
      [ -s '$REMOTE_DIR/agent.pid' ] && kill \$(cat '$REMOTE_DIR/agent.pid') 2>/dev/null; rm -f '$REMOTE_DIR/agent.pid';
      [ -x '$PKG/scripts/uninstall.sh' ] && bash '$PKG/scripts/uninstall.sh' --stop --purge-config --yes >/dev/null 2>&1;
      rm -rf /var/backups/baft-tunnel-backup.* 2>/dev/null;
      systemctl reset-failed 'baft-tunnel*' 2>/dev/null; systemctl daemon-reload; true" || true
  done
  for side in iran abroad; do
    if [[ " ${OP_USER_BEFORE[*]} " != *" $side "* ]]; then ssh_host "$side" "userdel baft-tunnel-agent 2>/dev/null; groupdel baft-tunnel-agent 2>/dev/null; userdel baft-tunnel 2>/dev/null; groupdel baft-tunnel 2>/dev/null; true" || true; fi
  done
}
# set +e: under errexit the (exit N) that restores the status would end the trap before cleanup runs.
trap 'op_status=$?; set +e; op_cleanup; (exit "$op_status"); cleanup' EXIT

echo "--- install (${MVP_SOURCE}, initial_sha256 ${OP_INITIAL_SHA}, target_sha256 ${MVP_SHA}) ---"
if [[ -n "${MVP_PKG:-}" ]]; then
  OP_PKG_ROOT="$(bash lab/real-hosts/prepare-ops-package.sh "$MVP_PKG" "$MVP_BINARY_PATH" "$LOCAL_TMP/op-pkg-unpacked")"
  tar -czf "$LOCAL_TMP/op-pkg.tgz" -C "$OP_PKG_ROOT" .
  echo "OPS_PACKAGE=verified_signed_bundle"
else
  tar -czf "$LOCAL_TMP/op-pkg.tgz" scripts/*.sh scripts/pki/*.sh systemd
  echo "OPS_PACKAGE=trusted_main_scripts"
fi
cp "$MVP_INITIAL_BINARY_PATH" "$LOCAL_TMP/baft-tunnel-ra"
# The target release is also used for cross-version, same-code digest-change and failed-start rollback controls.
cp "$MVP_BINARY_PATH" "$LOCAL_TMP/baft-tunnel-op-upgrade"
cp "$MVP_BINARY_PATH" "$LOCAL_TMP/baft-tunnel-op-new"; printf '\0' >> "$LOCAL_TMP/baft-tunnel-op-new"; chmod 755 "$LOCAL_TMP/baft-tunnel-op-new"
printf '#!/bin/sh\n[ "$1" = "--version" ] && { echo "baft-tunnel 0.0.0-bad"; exit 0; }\nexit 1\n' > "$LOCAL_TMP/baft-tunnel-op-bad"; chmod 755 "$LOCAL_TMP/baft-tunnel-op-bad"
OP_NEW_SHA="$(sha256sum "$LOCAL_TMP/baft-tunnel-op-new" | awk '{print $1}')"
for side in iran abroad; do
  copy_to_host "$side" "$LOCAL_TMP/op-pkg.tgz" "$LOCAL_TMP/baft-tunnel-ra" "$LOCAL_TMP/baft-tunnel-op-upgrade" "$LOCAL_TMP/baft-tunnel-op-new" "$LOCAL_TMP/baft-tunnel-op-bad"
  ssh_host "$side" "set -e; mkdir -p '$PKG'; tar -xzf '$REMOTE_DIR/op-pkg.tgz' -C '$PKG';
    [ \"\$(sha256sum '$REMOTE_DIR/baft-tunnel-ra' | cut -d' ' -f1)\" = '$OP_INITIAL_SHA' ];
    bash '$PKG/scripts/install.sh' --binary '$REMOTE_DIR/baft-tunnel-ra' >/dev/null;
    [ \"\$(sha256sum $UB | cut -d' ' -f1)\" = '$OP_INITIAL_SHA' ]"
done

echo "--- offline PKI (runner plays the offline CA) ---"
RP="$LOCAL_TMP/rapki"; mkdir -p "$RP"; (umask 077; printf 'lab-ephemeral-op-ca-%s\n' "$RANDOM$RANDOM$RANDOM" > "$RP/pass")
export BAFT_PKI_PASSFILE="$RP/pass"
bash scripts/pki/baft-pki.sh init-ca "$RP/ca" >/dev/null 2>&1
op_issue() { # node host_ip
  bash scripts/pki/baft-pki.sh new-node "$1" "$RP/$1" >/dev/null
  bash scripts/pki/baft-pki.sh issue-node "$RP/ca" "$1" "$RP/$1/node.csr" "$RP/issued-$1" "$2" 30 >/dev/null
  bash scripts/pki/baft-pki.sh verify-node "$RP/ca" "$RP/issued-$1/ca.crl" "$RP/issued-$1/node.crt" "$1" "$2" >/dev/null
}
op_issue abroad "$LAB_ABROAD_HOST"; op_issue iran "$LAB_IRAN_HOST"
cp "$RP/issued-iran/ca.crl" "$RP/ca.crl"
for node in abroad iran; do
  cp "$RP/$node/node.key" "$RP/op-$node-node.key"; cp "$RP/issued-$node/node.crt" "$RP/op-$node-node.crt"
  cp "$RP/issued-$node/ca.crt" "$RP/op-ca.crt"; cp "$RP/ca.crl" "$RP/op-ca.crl"
  copy_to_host "$node" "$RP/op-$node-node.key" "$RP/op-$node-node.crt" "$RP/op-ca.crt" "$RP/op-ca.crl"
  ssh_host "$node" "set -e; install -o root -g root -m 0600 '$REMOTE_DIR/op-$node-node.key' $EB/node.key; install -m 0644 '$REMOTE_DIR/op-$node-node.crt' $EB/node.crt;
    install -m 0644 '$REMOTE_DIR/op-ca.crt' $EB/ca.crt; install -m 0644 '$REMOTE_DIR/op-ca.crl' $EB/ca.crl; rm -f '$REMOTE_DIR/op-$node-node.key'"
done

echo "--- guided setup with --direction direct: init, bundle exchange, join ---"
rcommon="--engine tcp-tls --direction direct --tunnel-id opsacc --service-port $MSVC --tunnel-port $MTP --dir $EB"
# The measured Integrity probe (POST /v1/verify) needs protocol v2 on both nodes; v1 is the default and reports Integrity as unknown.
if (( VERIFY == 1 )); then rcommon="$rcommon --protocol-version 2"; fi
sum_a="$(ssh_host abroad "$UB setup init $rcommon --role serve --node-id abroad --peer-id iran --instance $SRV" | awk '/^Checksum/{print $NF}')"
sum_i="$(ssh_host iran "$UB setup init $rcommon --role forward --node-id iran --peer-id abroad --peer-host '$LAB_ABROAD_HOST' --instance $CLI" | awk '/^Checksum/{print $NF}')"
[[ "$sum_a" =~ ^[0-9a-f-]{32,}$ && "$sum_i" =~ ^[0-9a-f-]{32,}$ ]] || { echo "setup init printed no checksum" >&2; exit 1; }
ssh_host abroad "cat $EB/bundle.json" > "$RP/bundle-abroad.json"
ssh_host iran "cat $EB/bundle.json" > "$RP/bundle-iran.json"
copy_to_host iran "$RP/bundle-abroad.json"; copy_to_host abroad "$RP/bundle-iran.json"
ssh_host abroad "$UB setup join --dir $EB --bundle '$REMOTE_DIR/bundle-iran.json' --checksum '$sum_i' >/dev/null"
ssh_host iran "$UB setup join --dir $EB --bundle '$REMOTE_DIR/bundle-abroad.json' --checksum '$sum_a' >/dev/null"
for pair in "iran $CLI" "abroad $SRV"; do
  read -r pside pinst <<<"$pair"
  ssh_host "$pside" "chown baft-tunnel:baft-tunnel $EB/node.key && chgrp baft-tunnel $EB/$pinst.json && bash '$PKG/scripts/install-agent-control.sh' --instance $pinst --listen 127.0.0.1:18099 >/dev/null"
done
# The generated configs must carry the Direct topology: forward on iran (ingress), serve on abroad (listener).
ssh_host iran "python3 -c 'import json;c=json.load(open(\"$EB/$CLI.json\"));assert c[\"direction\"]==\"direct\" and c[\"engine\"]==\"tcp-tls\" and c[\"mode\"]==\"forward\", c'" ||
  { echo "iran config does not describe Direct forward" >&2; exit 1; }
ssh_host abroad "python3 -c 'import json;c=json.load(open(\"$EB/$SRV.json\"));assert c[\"direction\"]==\"direct\" and c[\"mode\"]==\"serve\", c'" ||
  { echo "abroad config does not describe Direct serve" >&2; exit 1; }
OP_SNI="$(ssh_host iran "python3 -c 'import json;print(json.load(open(\"$EB/$CLI.json\"))[\"server_name\"])'")"

echo "--- local agent per host (service user, loopback) ---"
for side in iran abroad; do
  cfg=$CLI; [[ "$side" == abroad ]] && cfg=$SRV
  ssh_host "$side" "set -e; install -d -o baft-tunnel-agent -g baft-tunnel-agent -m 0700 $AGD;
    setpriv --reuid=baft-tunnel-agent --regid=baft-tunnel-agent --init-groups $UB agent-token --out $AGD/token >/dev/null;
    nohup setpriv --reuid=baft-tunnel-agent --regid=baft-tunnel-agent --init-groups $UB agent --config $EB/$cfg.json --token-file $AGD/token --state-dir $AGD --listen 127.0.0.1:$MAG --instance $cfg > '$REMOTE_DIR/agent.log' 2>&1 < /dev/null & echo \$! > '$REMOTE_DIR/agent.pid';
    for n in \$(seq 1 40); do python3 -c 'import urllib.request;urllib.request.urlopen(\"http://127.0.0.1:$MAG/v1/health\",timeout=1)' 2>/dev/null && exit 0; sleep 0.25; done; tail -20 '$REMOTE_DIR/agent.log'; exit 1"
done
op_apply() { # side instance role key -> receipt JSON on stdout (direction direct)
  local side="$1" inst="$2" role="$3" key="$4" ph=None sn=None nid=abroad pid=iran
  if [[ "$role" == forward ]]; then ph="\"$LAB_ABROAD_HOST\""; sn="\"$OP_SNI\""; nid=iran; pid=abroad; fi
  ssh_host "$side" "python3 - <<'PY'
import json, urllib.request
tok = open('$AGD/token').read().strip()
plan = {'engine': 'tcp-tls', 'role': '$role', 'direction': 'direct', 'node_id': '$nid', 'peer_id': '$pid', 'tunnel_id': 'opsacc',
        'service_port': $MSVC, 'tunnel_port': $MTP, 'peer_host': $ph, 'server_name': $sn, 'dir': '$EB', 'instance': '$inst'}
body = json.dumps({'stage': 'start', 'confirm': True, 'idempotency_key': '$key', 'plan': plan}).encode()
r = urllib.request.Request('http://127.0.0.1:$MAG/v1/setup/apply', data=body, method='POST', headers={'Authorization': 'Bearer ' + tok})
try:
    print(urllib.request.urlopen(r, timeout=90).read().decode())
except urllib.error.HTTPError as e:
    print(e.read().decode())
PY"
}
jf() { python3 -c 'import sys,json; d=json.load(sys.stdin)
for k in sys.argv[1].split("."): d = d.get(k, "") if isinstance(d, dict) else ""
print(d)' "$1"; }
unit_active() { ssh_host "$1" "systemctl is-active --quiet baft-tunnel@$2.service"; }
unit_stop() { ssh_host "$1" "setpriv --reuid=baft-tunnel-agent --regid=baft-tunnel-agent --init-groups touch /run/baft-tunnel/control/$2.stop; for n in \$(seq 1 40); do systemctl is-active --quiet baft-tunnel@$2.service || exit 0; sleep 0.5; done; exit 1"; }
op_wait() { local t0=$SECONDS; while (( SECONDS - t0 < $1 )); do probe_host iran 65536 >/dev/null 2>&1 && { echo $((SECONDS - t0)); return 0; }; sleep 2; done; return 1; }
mainpid() { ssh_host "$1" "systemctl show -p MainPID --value baft-tunnel@$2.service"; }
unit_stats() { ssh_host "$1" "p=\$(systemctl show -p MainPID --value baft-tunnel@$2.service); echo \$(ls /proc/\$p/fd | wc -l) \$(awk '/VmRSS/{print \$2}' /proc/\$p/status) \$(systemctl show -p NRestarts --value baft-tunnel@$2.service)"; }
p1_api() { # side METHOD path
  ssh_host "$1" "python3 - <<'PY'
import urllib.request
tok = open('$AGD/token').read().strip()
r = urllib.request.Request('http://127.0.0.1:$MAG$3', data=b'' if '$2' == 'POST' else None, method='$2', headers={'Authorization': 'Bearer ' + tok})
try:
    print(urllib.request.urlopen(r, timeout=30).read().decode())
except urllib.error.HTTPError as e:
    print(e.read().decode())
PY"
}
op_verify() { # side label : POST /v1/verify (measured integrity probe) -> JSON saved to op-verify-LABEL-SIDE.json
  local side="$1" label="$2" key="lab-$2-$1-$RANDOM$RANDOM"
  ssh_host "$side" "python3 - <<'PY'
import urllib.request, json
tok = open('$AGD/token').read().strip()
r = urllib.request.Request('http://127.0.0.1:$MAG/v1/verify', data=json.dumps({'idempotency_key': '$key'}).encode(), method='POST', headers={'Authorization': 'Bearer ' + tok, 'Content-Type': 'application/json'})
try:
    print(urllib.request.urlopen(r, timeout=60).read().decode())
except urllib.error.HTTPError as e:
    print(e.read().decode())
PY" >"$LOCAL_TMP/op-verify-$label-$side.json"
  python3 -c 'import json,sys; d=json.load(open(sys.argv[1])); print("integrity-probe side=%s label=%s result=%s reason=%s bytes=%s sha256_match=%s" % (sys.argv[2], sys.argv[3], d.get("result"), d.get("reason"), d.get("bytes"), d.get("sha256_match")))' "$LOCAL_TMP/op-verify-$label-$side.json" "$side" "$label"
}
op_evidence() { # label min_bytes : diagnostics, status and traffic on both nodes must agree with the measured tunnel
  local label="$1" need="$2" side peer
  for side in iran abroad; do
    peer=abroad; [[ "$side" == abroad ]] && peer=iran
    # Status reports carrying_traffic only for traffic inside its 60 s window, so measure fresh
    # traffic right before reading each side (the previous side's reads can take that long).
    probe_host iran 65536 >/dev/null || { echo "Direct tunnel did not carry data before reading $side evidence ($label)" >&2; exit 1; }
    sleep 6   # the agents sample unit counters every 5 s
    echo "op-evidence-read utc=$(date -u +%H:%M:%S) side=$side label=$label"
    p1_api "$side" GET /v1/traffic >"$LOCAL_TMP/op-traffic-$label-$side.json"
    if (( VERIFY == 1 )); then op_verify "$side" "$label"; fi
    p1_api "$side" POST /v1/diagnostics >"$LOCAL_TMP/op-diag-$label-$side.json"
    p1_api "$side" GET /v1/status >"$LOCAL_TMP/op-status-$label-$side.json"
    if (( VERIFY == 1 )); then
      python3 - "$LOCAL_TMP/op-verify-$label-$side.json" "$LOCAL_TMP/op-diag-$label-$side.json" "$side" <<'PY' || { cat "$LOCAL_TMP/op-verify-$label-$side.json" "$LOCAL_TMP/op-diag-$label-$side.json" >&2; echo "measured Integrity on $side ($label) is not a pass after /v1/verify" >&2; exit 1; }
import json, sys
v, d, side = json.load(open(sys.argv[1])), json.load(open(sys.argv[2])), sys.argv[3]
r = {t["id"]: t["result"] for t in d["tiers"]}
print("verify-tier side=%s probe=%s integrity_tier=%s" % (side, v.get("result"), r.get("integrity")))
# The forward (iran) node owns the user ingress, so its probe must be a real pass; the serve node's result is recorded only.
if side == "iran":
    assert v.get("result") == "pass" and v.get("sha256_match") is True and r.get("integrity") == "pass", (v, r)
PY
    fi
    python3 - "$LOCAL_TMP/op-traffic-$label-$side.json" "$LOCAL_TMP/op-diag-$label-$side.json" "$LOCAL_TMP/op-status-$label-$side.json" "$peer" "$need" "$side" <<'PY' || { cat "$LOCAL_TMP"/op-*-"$label-$side".json >&2; echo "Direct evidence ($label) on $side does not match the measured tunnel" >&2; exit 1; }
import json, sys
tr, dg, st = (json.load(open(f)) for f in sys.argv[1:4]); peer, need, side = sys.argv[4], int(sys.argv[5]), sys.argv[6]
assert tr["available"] and tr["fresh"] and tr["units"] == "bytes", tr
assert tr["bytes_from_peer"] >= need and tr["bytes_to_peer"] >= need, tr
r = {t["id"]: t["result"] for t in dg["tiers"]}
print("tiers[%s]=%s" % (side, json.dumps(r, sort_keys=True)))
for k in ("secure_channel", "identity", "authorization", "session", "data_path"):
    assert r.get(k) == "pass", (k, r)
assert r.get("tcp_reachability") in ("pass", "unknown"), r
assert dg["verified"] is True and dg["unit"]["result"] == "active", dg
t = st["tunnel"]
assert t["state"] == "carrying_traffic" and t["tiers"]["authorized_session"]["peer_id"] == peer, t
print("ops-evidence side=%s peer=%s from_peer=%d to_peer=%d state=%s" % (side, peer, tr["bytes_from_peer"], tr["bytes_to_peer"], t["state"]))
PY
  done
}

echo "--- Apply start (direction direct): serve node first, then forward node; data integrity measured ---"
ssh_host abroad "nohup python3 '$REMOTE_DIR/echo.py' '$MSVC' > '$REMOTE_DIR/echo2.log' 2>&1 < /dev/null & echo \$! > '$REMOTE_DIR/echo2.pid'"
OLD_LOCAL_PORT="$LOCAL_PORT"; LOCAL_PORT="$MSVC"
out="$(op_apply abroad $SRV serve op-srv-0001)"
[[ "$(echo "$out" | jf result)" == applied ]] || { echo "$out" >&2; ssh_host abroad "journalctl -u baft-tunnel@$SRV -n 40 --no-pager" >&2 || true; echo "Apply start (serve node) was not applied" >&2; exit 1; }
out="$(op_apply iran $CLI forward op-cli-0001)"
[[ "$(echo "$out" | jf result)" == applied ]] || { echo "$out" >&2; ssh_host iran "journalctl -u baft-tunnel@$CLI -n 40 --no-pager" >&2 || true; echo "Apply start (forward node) was not applied" >&2; exit 1; }
OP_SESSION="$(echo "$out" | jf stages.authorized_session)"; OP_READY="$(echo "$out" | jf ready)"
[[ "$OP_READY" != ready && "$OP_READY" != true && "$OP_READY" != True ]] || { echo "Apply receipt claimed ready without a data-path observation" >&2; exit 1; }
op_wait 60 >/dev/null || { echo "Direct tunnel did not carry data after Apply start" >&2; exit 1; }
probe_host iran 65536; probe_host iran 2097289
# Direct: the serve node listens on the tunnel port, the forward node dials out and owns the user ingress.
ssh_host abroad "ss -ltnH | awk '{print \$4}' | grep -q ':$MTP\$'" || { echo "serve node does not listen on the tunnel port" >&2; exit 1; }
ssh_host iran "! ss -ltnH | awk '{print \$4}' | grep -q ':$MTP\$' && ss -tnH state established | awk '{print \$4}' | grep -q ':$MTP\$' && ss -ltnH | awk '{print \$4}' | grep -q '127.0.0.1:$MSVC\$'" ||
  { echo "forward node listens on the tunnel port, holds no outbound connection, or lacks the user ingress" >&2; exit 1; }
# Both units must run as the service user, not root.
for pair in "iran $CLI" "abroad $SRV"; do
  read -r pside pinst <<<"$pair"
  ssh_host "$pside" "[ \"\$(ps -o user= -p \$(systemctl show -p MainPID --value baft-tunnel@$pinst.service) | tr -d ' ')\" = baft-tunnel ]" || { echo "unit $pinst on $pside does not run as baft-tunnel" >&2; exit 1; }
done
sleep 12   # the agents sample unit counters every 5 s
probe_host iran 65536
sleep 12
echo "--- diagnostics, status, traffic counters and audit on both nodes (Direct) ---"
op_evidence apply $((65536 + 2097289))
for side in iran abroad; do
  ssh_host "$side" "python3 -c 'import json,sys; [json.loads(l) for l in open(sys.argv[1])]' $AGD/audit.log && grep -q '\"actor\"' $AGD/audit.log && ! grep -qs -- \"\$(cat $AGD/token)\" $AGD/audit.log && ! grep -rqs -- \"\$(cat $AGD/token)\" $AGD/receipts && ! grep -rqs 'PRIVATE KEY' $AGD/receipts" ||
    { echo "audit log or receipts on $side lack actor fields or hold a secret" >&2; exit 1; }
done
[[ "$(op_apply iran $CLI forward op-cli-0001 | jf replayed)" == True ]] || { echo "idempotent replay failed" >&2; exit 1; }
[[ "$(op_apply iran $CLI forward op-cli-0002 | jf result)" == refused ]] || { echo "Apply start on a running forward node was not refused" >&2; exit 1; }

ops_rotation_on() { # forward node only: set session_rotate_secs in its config and restart the unit; the serve node needs no change
  (( ROTATE_SECS > 0 )) || return 0
  [[ -z "${OPS_ROTATION_DONE:-}" ]] || return 0
  ssh_host iran "set -e; python3 - '$EB/$CLI.json' '$ROTATE_SECS' <<'PYR'
import json, sys
path, secs = sys.argv[1], int(sys.argv[2])
c = json.load(open(path))
assert c.get('mode') == 'forward' and c.get('direction') == 'direct', c
c['session_rotate_secs'] = secs
with open(path, 'w') as f:
    json.dump(c, f, indent=2)
print('OPS_ROTATE_CONFIG session_rotate_secs=%d' % secs)
PYR
    systemctl restart baft-tunnel@$CLI.service"
  OPS_ROTATION_DONE=1
}
ops_030_checks() { # runs once, on the first agent generation that serves GET /v1/update
  [[ -n "${OPS_030_DONE:-}" ]] && return 0
if [[ "$(p1_api iran GET /v1/update | python3 -c 'import json,sys
try: print("state" in json.load(sys.stdin))
except Exception: print(False)')" == True ]]; then
  echo "--- 0.3.0 features: host tunnel list, update notice, certificate warning, support bundle ---"
  for pair in "iran $CLI" "abroad $SRV"; do
    read -r fside finst <<<"$pair"
    p1_api "$fside" GET /v1/host/tunnels >"$LOCAL_TMP/op-host-$fside.json"
    python3 - "$LOCAL_TMP/op-host-$fside.json" "$finst" <<'PY' || { echo "host tunnel list on $fside does not show $finst as current, configured and active" >&2; exit 1; }
import json, sys
b = json.load(open(sys.argv[1])); t = [x for x in b["tunnels"] if x["instance"] == sys.argv[2]]
assert len(t) == 1 and t[0]["current"] is True and t[0]["configured"] is True and t[0]["active"] == "active", b
assert b["total"] >= 1 and b["truncated"] is False, b
assert "PRIVATE KEY" not in json.dumps(b), "key material in the host list"
PY
    # Real check against the GitHub release API from this host (no fixture): the latest Stable is older
    # than the installed 0.3.x, so the notice must say current and offer no command.
    ssh_host "$fside" "bash '$PKG/scripts/update-check.sh' --state-dir $AGD >/dev/null && chown baft-tunnel-agent: $AGD/update-check.json" ||
      { echo "update-check.sh could not record the latest release on $fside" >&2; exit 1; }
    p1_api "$fside" GET /v1/update >"$LOCAL_TMP/op-update-$fside.json"
    python3 - "$LOCAL_TMP/op-update-$fside.json" <<'PY' || { echo "update notice on $fside is not 'current' after a real check" >&2; exit 1; }
import json, re, sys
u = json.load(open(sys.argv[1]))
assert u["state"] == "current" and re.fullmatch(r"v\d+\.\d+\.\d+", u["latest_tag"] or "") and u["command"] is None, u
PY
    # Fixture (labelled as such): a newer Stable must turn the notice on with the signed one-line command.
    ssh_host "$fside" "printf '{\"tag_name\":\"v99.0.0\",\"prerelease\":false}' > '$REMOTE_DIR/latest.json' && bash '$PKG/scripts/update-check.sh' --state-dir $AGD --api-json '$REMOTE_DIR/latest.json' >/dev/null && chown baft-tunnel-agent: $AGD/update-check.json"
    p1_api "$fside" GET /v1/update >"$LOCAL_TMP/op-update2-$fside.json"
    python3 - "$LOCAL_TMP/op-update2-$fside.json" <<'PY' || { echo "update notice on $fside did not show a newer Stable" >&2; exit 1; }
import json, sys
u = json.load(open(sys.argv[1]))
assert u["state"] == "update_available" and u["latest_tag"] == "v99.0.0" and "/v99.0.0/scripts/quick-install.sh" in (u["command"] or ""), u
PY
    ssh_host "$fside" "rm -f $AGD/update-check.json '$REMOTE_DIR/latest.json'"
    # The LAB CA issues 30-day node certificates, so the real certificate must raise the renew_soon warning.
    p1_api "$fside" GET /v1/status >"$LOCAL_TMP/op-status030-$fside.json"
    python3 - "$LOCAL_TMP/op-status030-$fside.json" <<'PY' || { echo "status on $fside lacks the certificate expiry warning" >&2; exit 1; }
import json, sys
w = [x for x in json.load(open(sys.argv[1]))["warnings"] if x.get("id") == "certificate"]
assert len(w) == 1 and w[0]["state"] == "renew_soon" and w[0]["severity"] == "warning" and 0 < w[0]["seconds_remaining"] <= 30 * 86400, w
PY
    p1_api "$fside" GET /v1/support >"$LOCAL_TMP/op-support-$fside.json"
    ssh_host "$fside" "cat $AGD/token" >"$LOCAL_TMP/op-tok-$fside"
    python3 - "$LOCAL_TMP/op-support-$fside.json" "$LOCAL_TMP/op-tok-$fside" <<'PY' || { echo "support bundle on $fside is incomplete or holds a secret" >&2; rm -f "$LOCAL_TMP/op-tok-$fside"; exit 1; }
import json, sys
raw = open(sys.argv[1]).read(); b = json.loads(raw); tok = open(sys.argv[2]).read().strip()
assert {"status", "diagnostics", "excluded"} <= set(b), sorted(b)
assert tok and tok not in raw and "PRIVATE KEY" not in raw, "secret in the support bundle"
PY
    rm -f "$LOCAL_TMP/op-tok-$fside"
  done
  OPS_030="host_tunnels=1 update_notice_real=current update_notice_fixture=1 cert_warning=renew_soon support_bundle=1"
fi
  OPS_030_DONE=1
  echo "OPS_030_RESULT $OPS_030"
}
OPS_030="skipped"
OP_CROSS_VERSION=0
if [[ "$OP_INITIAL_SHA" != "$MVP_SHA" ]]; then
  echo "--- cross-version upgrade: previous prerelease runtime to target signed release ---"
  for side in iran abroad; do
    inst=$CLI; [[ "$side" == abroad ]] && inst=$SRV
    ssh_host "$side" "bash '$PKG/scripts/update.sh' --binary '$REMOTE_DIR/baft-tunnel-op-upgrade' --instance $inst --health-secs 20" | grep -q '^UPDATE_OK' || { echo "cross-version update on $side was not accepted" >&2; exit 1; }
    [[ "$(ssh_host "$side" "sha256sum $UB | cut -d' ' -f1")" == "$MVP_SHA" ]] || { echo "cross-version update on $side did not install the target binary" >&2; exit 1; }
  done
  # Restart the old Agent processes so subsequent evidence is served by the target release too.
  for side in iran abroad; do
    cfg=$CLI; [[ "$side" == abroad ]] && cfg=$SRV
    ssh_host "$side" "set -e; [ -s '$REMOTE_DIR/agent.pid' ] && kill \$(cat '$REMOTE_DIR/agent.pid') 2>/dev/null || true; rm -f '$REMOTE_DIR/agent.pid';
      nohup setpriv --reuid=baft-tunnel-agent --regid=baft-tunnel-agent --init-groups $UB agent --config $EB/$cfg.json --token-file $AGD/token --state-dir $AGD --listen 127.0.0.1:$MAG --instance $cfg > '$REMOTE_DIR/agent.log' 2>&1 < /dev/null & echo \$! > '$REMOTE_DIR/agent.pid';
      for n in \$(seq 1 40); do python3 -c 'import urllib.request;urllib.request.urlopen(\"http://127.0.0.1:$MAG/v1/health\",timeout=1)' 2>/dev/null && exit 0; sleep 0.25; done; tail -20 '$REMOTE_DIR/agent.log'; exit 1"
  done
  # Rotation (LAB_ROTATE_SECS>0) goes on right after the upgrade, before any aged-flow probe.
  ops_rotation_on
  # Feature checks on the target agents first, so a data-path failure below cannot hide them.
  ops_030_checks
  sleep 12
  op_wait 90 >/dev/null || { echo "tunnel did not recover after the cross-version upgrade" >&2; exit 1; }
  probe_host iran 2097289
  op_evidence cross_version 65536
  OP_CROSS_VERSION=1
fi

ops_rotation_on   # no-op unless LAB_ROTATE_SECS>0 (also covers runs without a version change)

# 0.3.0 operator features, checked on the target binary before the soak so a path outage during
# the soak cannot hide them: host tunnel list (ADR-0036), update notice (ADR-0035), certificate
# expiry warning, support bundle. Releases without GET /v1/update skip this block (OPS_030=skipped).
ops_030_checks

soak_diag() { # reason : evidence at the moment a soak probe fails (journals without per-stream noise, tunnel sockets, load); max 3 per run
  (( ${SOAK_DIAG_N:-0} < 3 )) || return 0
  SOAK_DIAG_N=$(( ${SOAK_DIAG_N:-0} + 1 ))
  local dside dinst
  for pair in "iran $CLI" "abroad $SRV"; do
    read -r dside dinst <<<"$pair"
    echo "=== SOAK_DIAG $1 $dside baft-tunnel@$dinst (last 150 s, without per-stream lines) ===" >&2
    ssh_host "$dside" "journalctl -u baft-tunnel@$dinst.service --since '-150 sec' --no-pager -o short-iso | grep -vE 'yamux stream (accepted|opened)|fixed destination connected|E2E_PAYLOAD_OK' | tail -40; echo '--- sockets on the tunnel port ---'; ss -tin state all '( sport = :$MTP or dport = :$MTP )' | head -30; echo '--- load, memory, conntrack ---'; cat /proc/loadavg; awk '/MemAvailable/{print}' /proc/meminfo; cat /proc/sys/net/netfilter/nf_conntrack_count /proc/sys/net/netfilter/nf_conntrack_max 2>/dev/null || true" >&2 || true
  done
}
echo "--- stability: ${OPS_SOAK}s of traffic every ~10 s over the installed Direct tunnel ---"
read -r pf0 pr0 pn0 <<<"$(unit_stats iran $CLI)"; read -r sf0 sr0 sn0 <<<"$(unit_stats abroad $SRV)"
PID_CLI0="$(mainpid iran $CLI)"; PID_SRV0="$(mainpid abroad $SRV)"
end=$((SECONDS + OPS_SOAK)); OP_ROUNDS=0; OP_FAILS=0; OP_MAXLAT=0
while (( SECONDS < end )); do
  t0=$SECONDS
  # A failed probe stays a failure; its reason (ssh status, probe message) is kept for diagnosis.
  if out="$(probe_host iran 65536 2>&1)"; then :; else
    rc=$?; OP_FAILS=$((OP_FAILS + 1)); echo "SOAK_PROBE_FAIL utc=$(date -u +%FT%TZ) round=$OP_ROUNDS bytes=65536 rc=$rc took_s=$((SECONDS - t0)) out=$(tr '\n' ' ' <<<"$out" | cut -c1-300)" >&2
    soak_diag "round=$OP_ROUNDS"
  fi
  lat=$((SECONDS - t0)); (( lat > OP_MAXLAT )) && OP_MAXLAT=$lat
  OP_ROUNDS=$((OP_ROUNDS + 1))
  if (( OP_ROUNDS % 30 == 0 )); then
    t1=$SECONDS
    if out="$(probe_host iran 2097289 2>&1)"; then :; else
      rc=$?; OP_FAILS=$((OP_FAILS + 1)); echo "SOAK_PROBE_FAIL utc=$(date -u +%FT%TZ) round=$OP_ROUNDS bytes=2097289 rc=$rc took_s=$((SECONDS - t1)) out=$(tr '\n' ' ' <<<"$out" | cut -c1-300)" >&2
      soak_diag "round=$OP_ROUNDS-large"
    fi
  fi
  sleep 10
done
probe_host iran 2097289
read -r pf1 pr1 pn1 <<<"$(unit_stats iran $CLI)"; read -r sf1 sr1 sn1 <<<"$(unit_stats abroad $SRV)"
OP_SOAK_LINE="soak_s=$OPS_SOAK rounds=$OP_ROUNDS failed_probes=$OP_FAILS max_probe_s=$OP_MAXLAT public_fd=$pf0->$pf1 public_rss_kb=$pr0->$pr1 service_fd=$sf0->$sf1 service_rss_kb=$sr0->$sr1 restarts=$pn0->$pn1/$sn0->$sn1"
echo "OPS_SOAK $OP_SOAK_LINE"
OP_ROTATE_LINE="rotate_secs=0"
if (( ROTATE_SECS > 0 )); then
  OP_ROTATE_EVENTS="$(ssh_host iran "journalctl -u baft-tunnel@$CLI.service --no-pager -o cat | grep -c '^session rotated: draining the previous session\$' || true")"
  [[ "$OP_ROTATE_EVENTS" =~ ^[0-9]+$ ]] || OP_ROTATE_EVENTS=0
  OP_ROTATE_LINE="rotate_secs=$ROTATE_SECS rotation_events=$OP_ROTATE_EVENTS"
  echo "OPS_ROTATE $OP_ROTATE_LINE"
  (( OP_ROTATE_EVENTS >= 2 )) || { echo "rotation was enabled but fewer than 2 completed rotations were observed" >&2; exit 1; }
fi
if (( OP_FAILS != 0 )); then
  for pair in "iran $CLI" "abroad $SRV"; do
    read -r dside dinst <<<"$pair"
    echo "=== $dside: baft-tunnel@$dinst journal warnings during the soak ===" >&2
    ssh_host "$dside" "journalctl -u baft-tunnel@$dinst.service --since '-$((OPS_SOAK / 60 + 5)) min' --no-pager -o short-iso | grep -viE 'E2E_PAYLOAD_OK' | tail -40" >&2 || true
  done
  echo "probes failed during the soak" >&2; exit 1
fi
[[ "$pn0" == "$pn1" && "$sn0" == "$sn1" && "$(mainpid iran $CLI)" == "$PID_CLI0" && "$(mainpid abroad $SRV)" == "$PID_SRV0" ]] || { echo "a unit restarted during the soak" >&2; exit 1; }
(( pf1 <= pf0 + 8 && sf1 <= sf0 + 8 )) || { echo "fd growth during the soak" >&2; exit 1; }
(( pr1 <= pr0 * 2 + 20480 && sr1 <= sr0 * 2 + 20480 )) || { echo "memory growth during the soak" >&2; exit 1; }
sleep 6
op_evidence soak 2097289

echo "--- reconnect: serve-node SIGKILL (systemd restart), forward-node SIGKILL, restarts through stop + Apply start ---"
ssh_host abroad "kill -KILL \$(systemctl show -p MainPID --value baft-tunnel@$SRV.service)"
OP_SRV_KILL_S="$(op_wait 90)" || { echo "tunnel did not recover after serve-node SIGKILL" >&2; exit 1; }
ssh_host iran "kill -KILL \$(systemctl show -p MainPID --value baft-tunnel@$CLI.service)"
OP_CLI_KILL_S="$(op_wait 90)" || { echo "tunnel did not recover after forward-node SIGKILL" >&2; exit 1; }
unit_stop abroad $SRV || { echo "could not stop $SRV through the control helper" >&2; exit 1; }
[[ "$(op_apply abroad $SRV serve op-srv-0002 | jf result)" == applied ]] || { echo "service-node restart was not applied" >&2; exit 1; }
OP_SRV_APPLY_S="$(op_wait 90)" || { echo "tunnel did not recover after the serve-node Apply restart" >&2; exit 1; }
unit_stop iran $CLI || { echo "could not stop $CLI through the control helper" >&2; exit 1; }
[[ "$(op_apply iran $CLI forward op-cli-0003 | jf result)" == applied ]] || { echo "public-node restart was not applied" >&2; exit 1; }
OP_CLI_APPLY_S="$(op_wait 90)" || { echo "tunnel did not recover after the forward-node Apply restart" >&2; exit 1; }
probe_host iran 2097289

echo "--- backup, simulated loss, restore (non-root ownership must survive) ---"
op_own() { ssh_host "$1" "cd $EB && stat -c '%n %U:%G %a' node.key $2.json ca.crt ca.crl node.crt | sort | sha256sum | cut -d' ' -f1; cd $EB && sha256sum $2.json node.crt ca.crt ca.crl | sha256sum | cut -d' ' -f1"; }
OP_OWN_CLI="$(op_own iran $CLI)"; OP_OWN_SRV="$(op_own abroad $SRV)"
OP_BK_CLI="$(ssh_host iran "bash '$PKG/scripts/backup.sh'" | sed -n 's/^Backup: //p' | tail -n 1)"
OP_BK_SRV="$(ssh_host abroad "bash '$PKG/scripts/backup.sh'" | sed -n 's/^Backup: //p' | tail -n 1)"
[[ "$OP_BK_CLI" == /var/backups/baft-tunnel-backup.* && "$OP_BK_SRV" == /var/backups/baft-tunnel-backup.* ]] || { echo "backup printed no directory" >&2; exit 1; }
ssh_host iran "! grep -rqs 'PRIVATE KEY' $OP_BK_CLI/manifest.txt && [ \"\$(stat -c %a $OP_BK_CLI $OP_BK_CLI/backup.tar)\" = \"\$(printf '700\n600')\" ]" || { echo "backup modes/manifest not as documented" >&2; exit 1; }
unit_stop iran $CLI; unit_stop abroad $SRV
for side in iran abroad; do ssh_host "$side" "[ -s '$REMOTE_DIR/agent.pid' ] && kill \$(cat '$REMOTE_DIR/agent.pid'); rm -f '$REMOTE_DIR/agent.pid'; sleep 1"; done
# Loss: instance config removed and the trust anchor corrupted on both hosts.
ssh_host iran "rm -f $EB/$CLI.json; : > $EB/ca.crt"; ssh_host abroad "rm -f $EB/$SRV.json; : > $EB/ca.crt"
ssh_host iran "bash '$PKG/scripts/restore.sh' --yes '$OP_BK_CLI' >/dev/null"; ssh_host abroad "bash '$PKG/scripts/restore.sh' --yes '$OP_BK_SRV' >/dev/null"
[[ "$(op_own iran $CLI)" == "$OP_OWN_CLI" && "$(op_own abroad $SRV)" == "$OP_OWN_SRV" ]] || { echo "restore changed config content, owners or modes" >&2; exit 1; }
for side in iran abroad; do
  cfg=$CLI; [[ "$side" == abroad ]] && cfg=$SRV
  ssh_host "$side" "set -e; install -d -o baft-tunnel-agent -g baft-tunnel-agent -m 0700 $AGD; [ -s $AGD/token ] || setpriv --reuid=baft-tunnel-agent --regid=baft-tunnel-agent --init-groups $UB agent-token --out $AGD/token >/dev/null;
    nohup setpriv --reuid=baft-tunnel-agent --regid=baft-tunnel-agent --init-groups $UB agent --config $EB/$cfg.json --token-file $AGD/token --state-dir $AGD --listen 127.0.0.1:$MAG --instance $cfg > '$REMOTE_DIR/agent.log' 2>&1 < /dev/null & echo \$! > '$REMOTE_DIR/agent.pid';
    for n in \$(seq 1 40); do python3 -c 'import urllib.request;urllib.request.urlopen(\"http://127.0.0.1:$MAG/v1/health\",timeout=1)' 2>/dev/null && exit 0; sleep 0.25; done; tail -20 '$REMOTE_DIR/agent.log'; exit 1"
done
[[ "$(op_apply iran $CLI forward op-cli-0004 | jf result)" == applied ]] || { echo "forward node did not start from the restored configuration" >&2; exit 1; }
[[ "$(op_apply abroad $SRV serve op-srv-0003 | jf result)" == applied ]] || { echo "serve node did not start from the restored configuration" >&2; exit 1; }
OP_RESTORE_S="$(op_wait 90)" || { echo "Direct tunnel did not carry data after the restore" >&2; exit 1; }
probe_host iran 2097289

echo "--- guarded update: same code with a new digest, then a binary that fails at start (automatic rollback) ---"
op_digest() { ssh_host "$1" "sha256sum $UB | cut -d' ' -f1"; }
for side in iran abroad; do
  inst=$CLI; [[ "$side" == abroad ]] && inst=$SRV
  ssh_host "$side" "bash '$PKG/scripts/update.sh' --binary '$REMOTE_DIR/baft-tunnel-op-new' --instance $inst --health-secs 10" | grep -q '^UPDATE_OK' || { echo "update on $side was not accepted" >&2; exit 1; }
  [[ "$(op_digest "$side")" == "$OP_NEW_SHA" ]] || { echo "update on $side did not install the new binary" >&2; exit 1; }
done
OP_UPDATE_S="$(op_wait 90)" || { echo "Direct tunnel did not carry data after the update" >&2; exit 1; }
probe_host iran 2097289
for side in iran abroad; do ssh_host "$side" "[ -s '$REMOTE_DIR/agent.pid' ] && kill \$(cat '$REMOTE_DIR/agent.pid'); rm -f '$REMOTE_DIR/agent.pid'"; done
for side in iran abroad; do
  cfg=$CLI; [[ "$side" == abroad ]] && cfg=$SRV
  ssh_host "$side" "nohup setpriv --reuid=baft-tunnel-agent --regid=baft-tunnel-agent --init-groups $UB agent --config $EB/$cfg.json --token-file $AGD/token --state-dir $AGD --listen 127.0.0.1:$MAG --instance $cfg > '$REMOTE_DIR/agent.log' 2>&1 < /dev/null & echo \$! > '$REMOTE_DIR/agent.pid';
    for n in \$(seq 1 40); do python3 -c 'import urllib.request;urllib.request.urlopen(\"http://127.0.0.1:$MAG/v1/health\",timeout=1)' 2>/dev/null && exit 0; sleep 0.25; done; exit 1"
done
sleep 14
probe_host iran 65536
op_evidence update 65536
# Bad update on the serve node: must exit non-zero, restore the previous digest and bring the tunnel back.
if ssh_host abroad "bash '$PKG/scripts/update.sh' --binary '$REMOTE_DIR/baft-tunnel-op-bad' --instance $SRV --health-secs 6" >"$LOCAL_TMP/op-badupdate.log" 2>&1; then
  echo "update with a failing binary was accepted" >&2; exit 1
fi
[[ "$(op_digest abroad)" == "$OP_NEW_SHA" ]] || { echo "failed update did not restore the previous binary" >&2; exit 1; }
unit_active abroad $SRV || { echo "service unit is not active after the automatic rollback" >&2; exit 1; }
OP_ROLLBACK_S="$(op_wait 90)" || { echo "Direct tunnel did not carry data after the automatic rollback" >&2; exit 1; }
probe_host iran 2097289

OP_CRL_LINE="crl_push=0"
if (( CRL_PUSH == 1 )); then
  echo "--- CRL renewal: renew-crl on the offline CA (runner), push-crl.sh over SSH to both nodes, tunnel recovers ---"
  crl_num() { ssh_host "$1" "openssl crl -in $EB/ca.crl -noout -crlnumber | cut -d= -f2"; }
  crl_sha() { ssh_host "$1" "sha256sum $EB/ca.crl | cut -d' ' -f1"; }
  OP_CRL_N0_I="$(crl_num iran)"; OP_CRL_N0_A="$(crl_num abroad)"
  bash scripts/pki/baft-pki.sh renew-crl "$RP/ca" "$RP/renewed" 30 >/dev/null
  bash scripts/pki/crl-status.sh "$RP/renewed/ca.crl" 72
  OP_CRL_N1="$(openssl crl -in "$RP/renewed/ca.crl" -noout -crlnumber | cut -d= -f2)"
  OP_CRL_SHA="$(sha256sum "$RP/renewed/ca.crl" | cut -d' ' -f1)"
  (( OP_CRL_N1 > OP_CRL_N0_I && OP_CRL_N1 > OP_CRL_N0_A )) || { echo "renewed CRL number $OP_CRL_N1 is not above the installed ones" >&2; exit 1; }
  # Negative control: a 2-day CRL is refused by push-crl.sh before any connection is made.
  bash scripts/pki/baft-pki.sh renew-crl "$RP/ca" "$RP/short" 2 >/dev/null
  if bash scripts/pki/push-crl.sh --crl "$RP/short/ca.crl" --host "$IRAN_USER@$LAB_IRAN_HOST:$IRAN_SSH_PORT" --remote-ca $EB/ca.crt --remote-crl $EB/ca.crl \
      --instance $CLI --remote-apply "$PKG/scripts/pki/apply-crl.sh" --known-hosts "$SSH_KNOWN_HOSTS_FILE" --key "$SSH_KEY_FILE" >/dev/null 2>&1; then
    echo "push-crl.sh accepted a CRL with under 72 hours left" >&2; exit 1
  fi
  [[ "$(crl_num iran)" == "$OP_CRL_N0_I" ]] || { echo "the refused short CRL changed the iran CRL" >&2; exit 1; }
  op_push() { # side instance user host port [crl]
    local sudo=(); [[ "$3" == root ]] || sudo=(--sudo)
    bash scripts/pki/push-crl.sh "${sudo[@]}" --crl "${6:-$RP/renewed/ca.crl}" --host "$3@$4:$5" --remote-ca $EB/ca.crt --remote-crl $EB/ca.crl \
      --instance "$2" --remote-apply "$PKG/scripts/pki/apply-crl.sh" --known-hosts "$SSH_KNOWN_HOSTS_FILE" --key "$SSH_KEY_FILE"
  }
  t0=$SECONDS
  op_push iran $CLI "$IRAN_USER" "$LAB_IRAN_HOST" "$IRAN_SSH_PORT" | tail -n 1
  op_push abroad $SRV "$ABROAD_USER" "$LAB_ABROAD_HOST" "$ABROAD_SSH_PORT" | tail -n 1
  OP_CRL_PUSH_S=$((SECONDS - t0))
  for side in iran abroad; do
    inst=$CLI; [[ "$side" == abroad ]] && inst=$SRV
    [[ "$(crl_sha "$side")" == "$OP_CRL_SHA" && "$(crl_num "$side")" == "$OP_CRL_N1" ]] || { echo "renewed CRL is not installed on $side" >&2; exit 1; }
    unit_active "$side" "$inst" || { echo "unit $inst on $side is not active after the CRL push" >&2; exit 1; }
  done
  OP_CRL_RECOVER_S="$(op_wait 90)" || { echo "Direct tunnel did not carry data after the CRL push" >&2; exit 1; }
  probe_host iran 2097289
  # Replaying the same CRL must be refused (CRL number not greater) and leave the tunnel up.
  if op_push iran $CLI "$IRAN_USER" "$LAB_IRAN_HOST" "$IRAN_SSH_PORT" >/dev/null 2>&1; then echo "push-crl.sh re-applied an equal CRL number" >&2; exit 1; fi
  [[ "$(crl_sha iran)" == "$OP_CRL_SHA" ]] && unit_active iran $CLI || { echo "refused replay changed the iran CRL or stopped its unit" >&2; exit 1; }
  probe_host iran 65536
  for side in iran abroad; do
    p1_api "$side" GET /v1/security >"$LOCAL_TMP/op-sec-$side.json"
    p1_api "$side" GET /v1/status >"$LOCAL_TMP/op-crlstatus-$side.json"
    python3 - "$LOCAL_TMP/op-sec-$side.json" "$LOCAL_TMP/op-crlstatus-$side.json" "$side" <<'PY' || { echo "agent on $side does not report the renewed CRL as ok" >&2; exit 1; }
import json, sys
sec, st, side = json.load(open(sys.argv[1])), json.load(open(sys.argv[2])), sys.argv[3]
rem = sec.get("crl_seconds_remaining")
assert sec.get("crl_state") == "ok" and rem is not None and rem > 29 * 86400, sec
assert not [w for w in st.get("warnings", []) if w.get("id") == "crl"], st.get("warnings")
print("crl-agent side=%s crl_state=%s days_left=%d" % (side, sec["crl_state"], rem // 86400))
PY
  done
  OP_CRL_LINE="crl_push=1 crl_number=${OP_CRL_N0_I}->${OP_CRL_N1} crl_days=30 crl_push_s=${OP_CRL_PUSH_S} crl_recovery_s=${OP_CRL_RECOVER_S} crl_short_refused=1 crl_replay_refused=1 agent_crl_state=ok"
  echo "CRL_PUSH_RESULT $OP_CRL_LINE"
fi

echo "--- revocation: iran (forward) certificate revoked, new CRL on the serve node, serve restarted ---"
bash scripts/pki/baft-pki.sh revoke-node "$RP/ca" "$RP/issued-iran/node.crt" "$RP/revoked" >/dev/null
copy_to_host abroad "$RP/revoked/ca.crl"
ssh_host abroad "install -m 0644 '$REMOTE_DIR/ca.crl' $EB/ca.crl"
if (( CRL_LIVE == 1 )); then
  # S8 live reload: the serve node is NOT restarted. It must pick the new CRL up on the next accepted connection,
  # so only the forward node is killed (systemd restarts it) to force a fresh handshake with its revoked certificate.
  SRV_PID0="$(ssh_host abroad "systemctl show -p MainPID --value baft-tunnel@$SRV.service")"
  ssh_host iran "kill -KILL \$(systemctl show -p MainPID --value baft-tunnel@$CLI.service)"
  sleep 12
  SRV_PID1="$(ssh_host abroad "systemctl show -p MainPID --value baft-tunnel@$SRV.service")"
  [[ -n "$SRV_PID0" && "$SRV_PID0" != 0 && "$SRV_PID0" == "$SRV_PID1" ]] || { echo "serve node was restarted during the live CRL reload (pid $SRV_PID0 -> $SRV_PID1)" >&2; exit 1; }
  echo "CRL_LIVE_RESULT serve_pid_unchanged=1 serve_pid=$SRV_PID1 revoked_refused_without_restart=pending_probe"
else
unit_stop abroad $SRV || { echo "could not stop $SRV" >&2; exit 1; }
[[ "$(op_apply abroad $SRV serve op-srv-0005 | jf result)" == applied ]] || { echo "serve node with the new CRL was not applied" >&2; exit 1; }
sleep 12
fi
if probe_host iran 4096 >/dev/null 2>&1; then echo "revoked forward certificate still carried data" >&2; exit 1; fi
ssh_host abroad "journalctl -u baft-tunnel@$SRV --no-pager -n 200 | grep -qi revok" || { echo "serve-node log shows no revocation refusal" >&2; exit 1; }
if (( CRL_LIVE == 1 )); then echo "CRL_LIVE_RESULT revoked_refused_without_restart=1 serve_pid_unchanged=1 serve_pid=$SRV_PID1"; fi
p1_api iran POST /v1/diagnostics >"$LOCAL_TMP/op-diag-revoked.json"
python3 - "$LOCAL_TMP/op-diag-revoked.json" <<'PY' || { cat "$LOCAL_TMP/op-diag-revoked.json" >&2; echo "forward-node diagnostics did not attribute the revocation to the secure channel" >&2; exit 1; }
import json, sys
d = json.load(open(sys.argv[1])); r = {t["id"]: t["result"] for t in d["tiers"]}
print("revoked tiers=%s" % json.dumps(r, sort_keys=True))
assert r["secure_channel"] == "fail" and r["session"] in ("fail", "unknown") and r["data_path"] != "pass", r
assert d["verified"] is False, d
PY
LOCAL_PORT="$OLD_LOCAL_PORT"; stop_role abroad echo2

echo "--- uninstall and cleanup proof ---"
op_cleanup; trap cleanup EXIT
for side in iran abroad; do
  ssh_host "$side" "! test -e $UB && ! test -e $EB && ! test -e /var/lib/baft-tunnel && ! test -e /run/baft-tunnel/control && ! ls /var/backups/baft-tunnel-backup.* >/dev/null 2>&1 && ! systemctl list-units --all --plain --no-legend 'baft-tunnel*' | grep -q ." ||
    { echo "uninstall left BAFT-TUNNEL resources on $side" >&2; exit 1; }
  side_snapshot "$side" > "$LOCAL_TMP/op-after-$side.txt"
  if ! diff "$LOCAL_TMP/op-before-$side.txt" "$LOCAL_TMP/op-after-$side.txt" > "$LOCAL_TMP/op-diff-$side.txt"; then
    echo "--- host state changed on $side during the stage ($(grep -c '^[<>]' "$LOCAL_TMP/op-diff-$side.txt") lines; addresses masked) ---"
    grep '^[<>]' "$LOCAL_TMP/op-diff-$side.txt" | sed -E 's/[0-9]{1,3}(\.[0-9]{1,3}){3}(\/[0-9]+)?/<ip>/g; s/[0-9a-fA-F]{0,4}(:[0-9a-fA-F]{0,4}){2,7}(\/[0-9]+)?/<ip6>/g' | head -n 30
    if grep -E '^[<>]' "$LOCAL_TMP/op-diff-$side.txt" | grep -qiE "baft|(:| |dport |sport )($MSVC|$MTP|$MAG)\b" ||
       ! diff <(sed -n '/^# listeners/,$p' "$LOCAL_TMP/op-before-$side.txt") <(sed -n '/^# listeners/,$p' "$LOCAL_TMP/op-after-$side.txt") >/dev/null; then
      echo "listeners, unit files or BAFT-related firewall rules changed on $side" >&2; exit 1
    fi
    OP_FW_DRIFT+=("$side")
  fi
done
echo "OPS_RESULT source=${MVP_SOURCE} binary_sha256=${MVP_SHA} update_sha256=${OP_NEW_SHA} setup_direction_direct=1 apply_start_serve=applied apply_start_forward=applied non_root=1 receipt_session=${OP_SESSION} receipt_ready=${OP_READY} integrity_64k_2m=1 evidence_diag_status_traffic=1 replay=1 running_refused=1 ${OP_SOAK_LINE} serve_sigkill_s=${OP_SRV_KILL_S} forward_sigkill_s=${OP_CLI_KILL_S} serve_apply_restart_s=${OP_SRV_APPLY_S} forward_apply_restart_s=${OP_CLI_APPLY_S} backup_restore=1 restore_owners_modes_identical=1 restore_recovery_s=${OP_RESTORE_S} update_ok=1 update_recovery_s=${OP_UPDATE_S} bad_update_rolled_back=1 rollback_recovery_s=${OP_ROLLBACK_S} ${OP_CRL_LINE} revoked_refused=1 uninstalled=1 listeners_units_unchanged=1 baft_fw_rules=0 unrelated_fw_drift=${OP_FW_DRIFT[*]:-none} cross_version_update=${OP_CROSS_VERSION} ${OP_ROTATE_LINE} ${OPS_030}"
