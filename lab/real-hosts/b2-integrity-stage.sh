# shellcheck shell=bash
# B2 stage: protocol generation 2 end to end plus the ADR-0028 local integrity probe, sourced by
# phase1-two-host-e2e.sh when LAB_B2=1. Installed product path on both LAB hosts:
#   clean install with `install.sh --binary ... --register NAME` (binary, users, both units, agent token),
#   the tunnel unit carries BAFT_TUNNEL_CONTROL_SOCKET for its own instance,
#   guided setup with `--protocol-version 2` on both nodes (tcp-tls Direct, offline CA),
#   start through POST /v1/tunnel/lifecycle, then POST /v1/verify on both nodes:
#     control_unavailable while the unit is stopped, one pass of exactly 65536 bytes,
#     idempotent replay of the same key (no second probe), rate_limited on a new key inside 60 s,
#     a v1/v2 mismatch that must not relay and must not be reported as pass,
#     and a direct connect to the control socket as root and as the tunnel user, which must be refused.
#   Finally status/diagnostics/audit must agree with the probe, the data plane must still carry
#   traffic afterwards, and a full uninstall must leave the hosts as they were found.
# Only resources this stage creates are removed; listeners, units and firewall rules are compared.
# Topology: abroad = serve (owns the service target), iran = forward (owns the ingress).
# The trusted LAB workflow only accepts a SHA that is already in main history, so this stage is
# dispatched from a checkout at the tested SHA: run it after the B2 change is on main, or from a
# trusted copy of the workflow pinned to that SHA.

echo "=== B2 INTEGRITY: protocol v2 on both nodes, agent-service install, probe pass and negatives ==="
stop_role iran serve; stop_role abroad serve; stop_role iran forward; stop_role abroad forward; sleep 1
B2_MSVC=18095; B2_MTP=18331; B2_AGP=18097; B2_PEER_TIMEOUT=120
B2_WAIT="${B2_COOLDOWN_WAIT:-61}"
[[ "$B2_WAIT" =~ ^[0-9]+$ ]] && (( B2_WAIT >= 60 && B2_WAIT <= 300 )) || { echo "B2_COOLDOWN_WAIT must be 60-300 seconds" >&2; exit 2; }
EB=/etc/baft-tunnel; UB=/usr/local/bin/baft-tunnel; AGS=/var/lib/baft-tunnel-agent; PKG="$REMOTE_DIR/pkg"
B2_SRV=b2srv; B2_CLI=b2cli; B2_TID=b2acc
B2_SHA="$(sha256sum "$MVP_BINARY_PATH" | awk '{print $1}')"
b2_inst() { if [[ "$1" == iran ]]; then echo "$B2_CLI"; else echo "$B2_SRV"; fi; }
b2_peer() { if [[ "$1" == iran ]]; then echo abroad; else echo iran; fi; }
b2_side_snapshot() { # side : normalized firewall rules, listeners outside the test ports, baft-tunnel* unit files
  ssh_host "$1" "echo '# firewall'; { nft list ruleset 2>/dev/null | sed -E 's/packets [0-9]+ bytes [0-9]+//'; iptables-save 2>/dev/null | grep -v '^#' | sed -E 's/\\[[0-9]+:[0-9]+\\]//'; };
    echo '# listeners'; ss -ltnH | awk '{print \$4}' | grep -vE ':($B2_MSVC|$B2_MTP|$B2_AGP|$E2E_PORT|$LOCAL_PORT|$ECHO_PORT)\$' | sort -u;
    echo '# units'; systemctl list-unit-files --no-legend 'baft-tunnel*' 2>/dev/null | wc -l"
}

echo "--- clean-host check ---"
B2_USERS_BEFORE=()
for side in iran abroad; do
  ssh_host "$side" "! test -e $UB && ! test -e $EB && ! test -e /var/lib/baft-tunnel && ! test -e $AGS && [ \"\$(systemctl list-unit-files --no-legend 'baft-tunnel*' | wc -l)\" = 0 ]" ||
    { echo "$side is not clean: an earlier BAFT-TUNNEL install is present (binary, $EB, state, users or units)" >&2; exit 1; }
  ssh_host "$side" "systemctl --version | head -1; . /etc/os-release; echo \$PRETTY_NAME"
  for u in baft-tunnel baft-tunnel-agent; do
    if ssh_host "$side" "id $u >/dev/null 2>&1"; then B2_USERS_BEFORE+=("$side:$u"); fi
  done
  b2_side_snapshot "$side" > "$LOCAL_TMP/b2-before-$side.txt"
done

b2_cleanup() {
  for side in iran abroad; do
    inst="$(b2_inst "$side")"
    ssh_host "$side" "set +e; systemctl disable --now baft-tunnel@$inst.service 2>/dev/null;
      systemctl disable --now baft-tunnel-agent@$inst.service 2>/dev/null;
      [ -d '$PKG/scripts' ] && bash '$PKG/scripts/install-agent-control.sh' --remove --instance $inst >/dev/null 2>&1;
      [ -d '$PKG/scripts' ] && bash '$PKG/scripts/uninstall.sh' --stop --purge-config --yes >/dev/null 2>&1;
      rm -rf /var/backups/baft-tunnel-backup.* 2>/dev/null;
      rm -rf /run/baft-tunnel/$inst 2>/dev/null;
      systemctl reset-failed 'baft-tunnel*' 2>/dev/null; systemctl daemon-reload; true" || true
  done
  for side in iran abroad; do
    for u in baft-tunnel-agent baft-tunnel; do
      if [[ " ${B2_USERS_BEFORE[*]} " != *" $side:$u "* ]]; then ssh_host "$side" "userdel $u 2>/dev/null; groupdel $u 2>/dev/null; true" || true; fi
    done
  done
}
# set +e: under errexit the (exit N) that restores the status would end the trap before cleanup runs.
trap 'b2_status=$?; set +e; b2_cleanup; (exit "$b2_status"); cleanup' EXIT

echo "--- install (sha256 ${B2_SHA}): scripts, units and one-step --register per host ---"
tar -czf "$LOCAL_TMP/b2-pkg.tgz" scripts/*.sh scripts/pki/*.sh systemd
for side in iran abroad; do
  inst="$(b2_inst "$side")"
  copy_to_host "$side" "$LOCAL_TMP/b2-pkg.tgz" "$MVP_BINARY_PATH"
  ssh_host "$side" "set -e; mkdir -p '$PKG'; tar -xzf '$REMOTE_DIR/b2-pkg.tgz' -C '$PKG';
    [ \"\$(sha256sum '$REMOTE_DIR/baft-tunnel' | cut -d' ' -f1)\" = '$B2_SHA' ];
    bash '$PKG/scripts/install.sh' --binary '$REMOTE_DIR/baft-tunnel' --register $inst --agent-listen 127.0.0.1:$B2_AGP >/dev/null;
    [ \"\$(sha256sum $UB | cut -d' ' -f1)\" = '$B2_SHA' ]
    systemctl cat baft-tunnel@$inst.service | grep -Fq 'Environment=BAFT_TUNNEL_CONTROL_SOCKET=/run/baft-tunnel/$inst/control.sock'
    [ \"\$(stat -c '%U %a' $AGS/token)\" = 'baft-tunnel-agent 600' ]
    id -nG baft-tunnel-agent | grep -qw baft-tunnel
    systemctl is-active --quiet baft-tunnel-agent@$inst.service
    ! systemctl is-active --quiet baft-tunnel@$inst.service" ||
    { echo "$side install --register did not leave the expected state (control socket wiring, token, group, units)" >&2; exit 1; }
done
echo "B2_INSTALL control_socket_env=1 token=baft-tunnel-agent:600 agent_group=baft-tunnel tunnel_not_started=1"

echo "--- offline PKI (runner plays the offline CA) ---"
RP="$LOCAL_TMP/b2pki"; mkdir -p "$RP"; (umask 077; printf 'lab-ephemeral-b2-ca-%s\n' "$RANDOM$RANDOM$RANDOM" > "$RP/pass")
export BAFT_PKI_PASSFILE="$RP/pass"
bash scripts/pki/baft-pki.sh init-ca "$RP/ca" >/dev/null 2>&1
for node in abroad iran; do
  [[ "$node" == abroad ]] && host="$LAB_ABROAD_HOST" || host="$LAB_IRAN_HOST"
  bash scripts/pki/baft-pki.sh new-node "$node" "$RP/$node" >/dev/null
  bash scripts/pki/baft-pki.sh issue-node "$RP/ca" "$node" "$RP/$node/node.csr" "$RP/issued-$node" "$host" 30 >/dev/null
  bash scripts/pki/baft-pki.sh verify-node "$RP/ca" "$RP/issued-$node/ca.crl" "$RP/issued-$node/node.crt" "$node" "$host" >/dev/null
  cp "$RP/issued-$node/node.key" "$RP/b2-$node-node.key"; cp "$RP/issued-$node/node.crt" "$RP/b2-$node-node.crt"
  cp "$RP/issued-$node/ca.crt" "$RP/b2-ca.crt"; cp "$RP/ca/ca.crl" "$RP/b2-ca.crl" 2>/dev/null || cp "$RP/issued-$node/ca.crl" "$RP/b2-ca.crl"
  copy_to_host "$node" "$RP/b2-$node-node.key" "$RP/b2-$node-node.crt" "$RP/b2-ca.crt" "$RP/b2-ca.crl"
  ssh_host "$node" "set -e; install -o root -g root -m 0600 '$REMOTE_DIR/b2-$node-node.key' $EB/node.key;
    install -m 0644 '$REMOTE_DIR/b2-$node-node.crt' $EB/node.crt; install -m 0644 '$REMOTE_DIR/b2-ca.crt' $EB/ca.crt;
    install -m 0644 '$REMOTE_DIR/b2-ca.crl' $EB/ca.crl; rm -f '$REMOTE_DIR/b2-$node-node.key'"
done

echo "--- guided setup with --protocol-version 2 on both nodes ---"
b2_rcommon="--engine tcp-tls --direction direct --protocol-version 2 --tunnel-id $B2_TID --service-port $B2_MSVC --tunnel-port $B2_MTP --dir $EB"
sum_a="$(ssh_host abroad "$UB setup init $b2_rcommon --role serve --node-id abroad --peer-id iran --instance $B2_SRV" | awk '/^Checksum/{print $NF}')"
sum_i="$(ssh_host iran "$UB setup init $b2_rcommon --role forward --node-id iran --peer-id abroad --peer-host '$LAB_ABROAD_HOST' --instance $B2_CLI" | awk '/^Checksum/{print $NF}')"
[[ "$sum_a" =~ ^[0-9a-f-]{32,}$ && "$sum_i" =~ ^[0-9a-f-]{32,}$ ]] || { echo "setup init printed no checksum" >&2; exit 1; }
ssh_host abroad "cat $EB/bundle.json" > "$RP/bundle-abroad.json"
ssh_host iran "cat $EB/bundle.json" > "$RP/bundle-iran.json"
copy_to_host iran "$RP/bundle-abroad.json"; copy_to_host abroad "$RP/bundle-iran.json"
ssh_host abroad "$UB setup join --dir $EB --bundle '$REMOTE_DIR/bundle-iran.json' --checksum '$sum_i' >/dev/null"
ssh_host iran "$UB setup join --dir $EB --bundle '$REMOTE_DIR/bundle-abroad.json' --checksum '$sum_a' >/dev/null"
for pair in "iran $B2_CLI" "abroad $B2_SRV"; do
  read -r pside pinst <<<"$pair"
  ssh_host "$pside" "set -e; chown baft-tunnel:baft-tunnel $EB/node.key; chgrp baft-tunnel $EB/$pinst.json;
    python3 -c 'import json;c=json.load(open(\"$EB/$pinst.json\"));assert c[\"direction\"]==\"direct\" and c[\"engine\"]==\"tcp-tls\" and c[\"protocol_version\"]==2, c'"
done
ssh_host iran "python3 -c 'import json;c=json.load(open(\"$EB/$B2_CLI.json\"));assert c[\"mode\"]==\"forward\", c'" || { echo "iran config does not describe Direct forward" >&2; exit 1; }
ssh_host abroad "python3 -c 'import json;c=json.load(open(\"$EB/$B2_SRV.json\"));assert c[\"mode\"]==\"serve\", c'" || { echo "abroad config does not describe Direct serve" >&2; exit 1; }
echo "B2_SETUP protocol_version=2 direction=direct engine=tcp-tls forward=iran serve=abroad"

b2_api() { # side METHOD path [json-body] -> response JSON on stdout (token read as root, never printed)
  ssh_host "$1" "python3 - <<'PY'
import urllib.request
tok = open('$AGS/token').read().strip()
body = '''${4:-}'''.strip()
r = urllib.request.Request('http://127.0.0.1:$B2_AGP$3', data=(body.encode() if body else None) if '$2' != 'GET' else None, method='$2',
                           headers={'Authorization': 'Bearer ' + tok})
try:
    print(urllib.request.urlopen(r, timeout=90).read().decode())
except urllib.error.HTTPError as e:
    print(e.read().decode())
    raise SystemExit(3)
PY"
}
b2_lifecycle() { # side action key
  b2_api "$1" POST /v1/tunnel/lifecycle "{\"action\": \"$2\", \"confirm\": true, \"idempotency_key\": \"$3\"}"
}
b2_verify() { # side idempotency-key -> response JSON on stdout
  b2_api "$1" POST /v1/verify "{\"idempotency_key\": \"$2\"}"
}
b2_wait_carry() { # seconds: the forward node relays a 64 KiB round trip again
  local t0=$SECONDS
  while (( SECONDS - t0 < $1 )); do probe_host iran 65536 >/dev/null 2>&1 && { echo $((SECONDS - t0)); return 0; }; sleep 2; done
  return 1
}
b2_socket_stat() { ssh_host "$1" "stat -c '%U %G %a %F' /run/baft-tunnel/$(b2_inst "$1")/control.sock"; }
b2_audit_count() { # side action receipt-id -> number of matching audit records
  b2_api "$1" GET /v1/audit | python3 -c 'import json,sys; a=json.load(sys.stdin)["entries"]; print(sum(1 for e in a if e.get("action")==sys.argv[1] and e.get("receipt_id")==sys.argv[2]))' "$2" "$3"
}
b2_stop_tunnel() { b2_lifecycle "$1" stop "b2-stop-$2-$RANDOM$RANDOM" >/dev/null; }
b2_start_tunnel() { b2_lifecycle "$1" start "b2-start-$2-$RANDOM$RANDOM" >/dev/null; }

echo "--- start through the agent service, socket created by the tunnel unit ---"
for side in iran abroad; do b2_start_tunnel "$side" "$SECONDS"; done
for side in iran abroad; do
  inst="$(b2_inst "$side")"
  ssh_host "$side" "for n in \$(seq 1 40); do systemctl is-active --quiet baft-tunnel@$inst.service && exit 0; sleep 0.5; done; systemctl status --no-pager baft-tunnel@$inst.service; exit 1"
  for n in $(seq 1 40); do [[ "$(b2_socket_stat "$side" 2>/dev/null)" == "baft-tunnel baft-tunnel 660 socket" ]] && break; sleep 0.25; done
  [[ "$(b2_socket_stat "$side")" == "baft-tunnel baft-tunnel 660 socket" ]] ||
    { echo "$side control socket is not baft-tunnel:baft-tunnel 0660 socket: $(b2_socket_stat "$side" || true)" >&2; exit 1; }
done
b2_wait_carry "$B2_PEER_TIMEOUT" >/dev/null || { echo "protocol v2 Direct session did not carry 64 KiB" >&2; exit 1; }
echo "B2_SOCKET owner=baft-tunnel group=baft-tunnel mode=660 type=socket session_carried=1"

echo "--- negative: probe while the tunnel unit is stopped must be unknown/control_unavailable ---"
for side in iran abroad; do b2_stop_tunnel "$side" "$SECONDS"; done
for side in iran abroad; do
  b2_verify "$side" "b2-unavailable-$side" > "$LOCAL_TMP/b2-unavailable-$side.json" || true
  python3 - "$LOCAL_TMP/b2-unavailable-$side.json" "$side" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
assert d["result"] == "unknown" and d.get("reason") == "control_unavailable", (sys.argv[2], d)
assert d["bytes"] == 0 and d["sha256_match"] is False, d
PY
done
b2_api iran GET /v1/status > "$LOCAL_TMP/b2-status-stopped.json"
python3 - "$LOCAL_TMP/b2-status-stopped.json" <<'PY'
import json, sys
t = json.load(open(sys.argv[1]))["tunnel"]
assert t["tiers"]["integrity"]["result"] != "pass", t["tiers"]["integrity"]
assert t["state"] != "carrying_traffic", t["state"]
PY
echo "B2_UNAVAILABLE result=unknown reason=control_unavailable status_integrity=not_pass"

echo "--- positive: one probe per node, 65536 bytes, status/diagnostics/audit agreement ---"
for side in iran abroad; do b2_start_tunnel "$side" "$SECONDS"; done
for side in iran abroad; do
  for n in $(seq 1 40); do [[ "$(b2_socket_stat "$side" 2>/dev/null)" == "baft-tunnel baft-tunnel 660 socket" ]] && break; sleep 0.25; done
done
b2_wait_carry "$B2_PEER_TIMEOUT" >/dev/null || { echo "session did not come back after the stop/start cycle" >&2; exit 1; }
for side in iran abroad; do
  b2_verify "$side" "b2-pass-$side" > "$LOCAL_TMP/b2-pass-$side.json"
  python3 - "$LOCAL_TMP/b2-pass-$side.json" "$side" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
assert d["result"] == "pass", (sys.argv[2], d)
assert d["bytes"] == 65536 and d["sha256_match"] is True, d
assert d.get("reason") in (None, ""), d
assert d["timestamp"] > 0 and (d["duration_ms"] is None or d["duration_ms"] <= 10_000), d
PY
  b2_api "$side" GET /v1/status > "$LOCAL_TMP/b2-status-$side.json"
  b2_api "$side" POST /v1/diagnostics > "$LOCAL_TMP/b2-diag-$side.json"
  python3 - "$LOCAL_TMP/b2-status-$side.json" "$LOCAL_TMP/b2-diag-$side.json" "$(b2_peer "$side")" "$side" <<'PY'
import json, sys
st = json.load(open(sys.argv[1]))["tunnel"]
assert st["tiers"]["integrity"]["result"] == "pass", st["tiers"]["integrity"]
assert st["tiers"]["integrity"]["freshness_s"] == 60, st["tiers"]["integrity"]
assert st["state"] == "carrying_traffic", st["state"]
assert st["tiers"]["authorized_session"]["peer_id"] == sys.argv[3], st["tiers"]["authorized_session"]
dg = json.load(open(sys.argv[2]))
r = {t["id"]: t["result"] for t in dg["tiers"]}
assert r.get("integrity") == "pass" and dg["verified"] is True, r
print("probe-evidence side=%s peer=%s tiers=%s" % (sys.argv[4], sys.argv[3], json.dumps(r, sort_keys=True)))
PY
  [[ "$(b2_audit_count "$side" integrity_probe "b2-pass-$side")" == "1" ]] ||
    { echo "$side audit has no unique integrity_probe record for b2-pass-$side" >&2; exit 1; }
  ssh_host "$side" "set -e; [ \"\$(stat -c '%U %a' $AGS/integrity-probe-receipts.json)\" = 'baft-tunnel-agent 600' ];
    ! grep -qF \"\$(cat $AGS/token)\" $AGS/integrity-probe-receipts.json"
done
echo "B2_PASS bytes=65536 sha256_match=1 status_tier=pass diagnostics_tier=pass audit=1 receipts=600"

echo "--- idempotent replay of the same key: same answer, no second probe ---"
for side in iran abroad; do
  b2_verify "$side" "b2-pass-$side" > "$LOCAL_TMP/b2-replay-$side.json"
  cmp -s "$LOCAL_TMP/b2-pass-$side.json" "$LOCAL_TMP/b2-replay-$side.json" ||
    { echo "$side replay of the same idempotency key returned a different answer" >&2; exit 1; }
  [[ "$(b2_audit_count "$side" integrity_probe "b2-pass-$side")" == "1" ]] ||
    { echo "$side replay produced a second integrity_probe record" >&2; exit 1; }
done
echo "B2_IDEMPOTENT replay_identical=1 audit_records=1"

echo "--- negative: a new key inside the 60 s window must be unknown/rate_limited ---"
for side in iran abroad; do
  b2_verify "$side" "b2-burst-$side" > "$LOCAL_TMP/b2-burst-$side.json" || true
  python3 - "$LOCAL_TMP/b2-burst-$side.json" "$side" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
assert d["result"] == "unknown" and d.get("reason") == "rate_limited", (sys.argv[2], d)
assert d["bytes"] == 0 and d["sha256_match"] is False, d
PY
done
echo "B2_RATE_LIMIT result=unknown reason=rate_limited cooldown_s=60"

echo "--- negative: v1 on the forward node must not relay and must not be reported as pass ---"
ssh_host iran "set -e; python3 - <<'PY'
import json
p = '$EB/$B2_CLI.json'
c = json.load(open(p))
c['protocol_version'] = 1
json.dump(c, open(p, 'w'), indent=2)
PY
chgrp baft-tunnel $EB/$B2_CLI.json"
b2_lifecycle iran restart "b2-v1-restart" >/dev/null
sleep 5
if probe_host iran 65536 >/dev/null 2>&1; then echo "a v1 forward node still relayed traffic to a v2 serve node" >&2; exit 1; fi
b2_api iran GET /v1/status > "$LOCAL_TMP/b2-status-v1.json"
python3 - "$LOCAL_TMP/b2-status-v1.json" <<'PY'
import json, sys
t = json.load(open(sys.argv[1]))["tunnel"]
assert t["state"] != "carrying_traffic", t["state"]
assert t["tiers"]["integrity"]["result"] != "pass", t["tiers"]["integrity"]
PY
echo "B2_PROTOCOL_MISMATCH v1_forward_refused=1 status_not_carrying=1 integrity_not_pass=1"

echo "--- restore protocol 2, wait out the cooldown, and probe again ---"
ssh_host iran "set -e; python3 - <<'PY'
import json
p = '$EB/$B2_CLI.json'
c = json.load(open(p))
c['protocol_version'] = 2
json.dump(c, open(p, 'w'), indent=2)
PY
chgrp baft-tunnel $EB/$B2_CLI.json"
b2_lifecycle iran restart "b2-v2-restart" >/dev/null
b2_wait_carry "$B2_PEER_TIMEOUT" >/dev/null || { echo "protocol v2 session did not come back after the mismatch run" >&2; exit 1; }
sleep "$B2_WAIT"
b2_verify iran "b2-pass-again" > "$LOCAL_TMP/b2-pass-again.json"
python3 - "$LOCAL_TMP/b2-pass-again.json" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
assert d["result"] == "pass" and d["bytes"] == 65536 and d["sha256_match"] is True, d
PY
probe_host iran 2097289 >/dev/null
echo "B2_RESTORED protocol_version=2 probe_pass=1 data_plane_2mib=1"

echo "--- negative: the control socket refuses root and the tunnel user itself ---"
for who in "root:0" "baft-tunnel:baft-tunnel"; do
  user="${who%%:*}"; group="${who##*:}"
  if ssh_host iran "setpriv --reuid=$user --regid=$group --init-groups python3 - <<'PY'
import socket, sys
s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
s.settimeout(5)
s.connect('/run/baft-tunnel/$B2_CLI/control.sock')
s.sendall(b'BAFT-PROBE-V2')
data = b''
try:
    while len(data) < 4096:
        chunk = s.recv(4096)
        if not chunk:
            break
        data += chunk
except OSError:
    pass
sys.exit(0 if data.startswith(b'BTR2') else 1)
PY"; then echo "the control socket answered a $user peer (the agent is supposed to accept only baft-tunnel-agent)" >&2; exit 1; fi
done
echo "B2_PEER_UID root_refused=1 tunnel_user_refused=1"

echo "--- uninstall and host-drift comparison ---"
for side in iran abroad; do
  inst="$(b2_inst "$side")"
  b2_stop_tunnel "$side" "$SECONDS" >/dev/null 2>&1 || true
  ssh_host "$side" "set -e; bash '$PKG/scripts/install-agent-control.sh' --remove --instance $inst >/dev/null;
    bash '$PKG/scripts/uninstall.sh' --stop --purge-config --yes >/dev/null;
    ! test -e $EB && ! test -e $UB && [ \"\$(systemctl list-unit-files --no-legend 'baft-tunnel*' | wc -l)\" = 0 ]"
done
for side in iran abroad; do
  b2_side_snapshot "$side" > "$LOCAL_TMP/b2-after-$side.txt"
  diff -u "$LOCAL_TMP/b2-before-$side.txt" "$LOCAL_TMP/b2-after-$side.txt" >&2 ||
    { echo "$side changed outside this stage (firewall, listeners or unit files)" >&2; exit 1; }
done

echo "B2_RESULT protocol_v2_both=1 control_socket_env=1 socket_mode=660 probe_pass=1 probe_bytes=65536 status_tier=pass diagnostics_tier=pass audit=1 idempotent_replay=1 rate_limited=1 control_unavailable=1 v1_peer_refused=1 peer_uid_refused=1 data_plane_after_probe=1 cleanup_clean=1"
