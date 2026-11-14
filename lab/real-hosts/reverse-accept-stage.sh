# shellcheck shell=bash
# Reverse (tcp-tls, Preview) acceptance stage, sourced by phase1-two-host-e2e.sh when LAB_REVERSE_ACCEPT>0.
# It does not repeat the P0-4 Reverse checks. It exercises the INSTALLED product path: clean install, guided
# setup with --direction reverse, Apply start through the non-root local agent into hardened systemd units,
# then stability over time (LAB_REVERSE_ACCEPT seconds), reconnect, diagnostics/status/traffic on both nodes,
# backup/restore, guarded update (same code, different digest) with automatic rollback, revocation and uninstall.
# Only resources this stage creates are removed; firewall, listeners and other units are compared.
# Topology: iran = public node (setup role "serve": listens on the tunnel port, owns the user ingress),
#           abroad = service node (setup role "forward": dials out, relays to the local service).

echo "=== REVERSE ACCEPTANCE: clean install from ${MVP_SOURCE}, Reverse via setup+Apply, ${REVERSE_ACCEPT}s soak, backup/restore, update/rollback ==="
stop_role iran serve; stop_role abroad serve; stop_role iran forward; stop_role abroad forward; sleep 1
MSVC=18094; MTP=18330; MAG=18096
EB=/etc/baft-tunnel; UB=/usr/local/bin/baft-tunnel; AGD=/var/lib/baft-tunnel/agent; PKG="$REMOTE_DIR/pkg"
PUB=revpub; SVC=revsvc          # instance names on iran / abroad
MVP_SHA="$(sha256sum "$MVP_BINARY_PATH" | awk '{print $1}')"

side_snapshot() { # side : normalized firewall rules, listeners outside the test ports, baft-tunnel* unit-file count
  ssh_host "$1" "echo '# firewall'; { nft list ruleset 2>/dev/null | sed -E 's/packets [0-9]+ bytes [0-9]+//'; iptables-save 2>/dev/null | grep -v '^#' | sed -E 's/\\[[0-9]+:[0-9]+\\]//'; };
    echo '# listeners'; ss -ltnH | awk '{print \$4}' | grep -vE ':($MSVC|$MTP|$MAG|$E2E_PORT|$LOCAL_PORT|$ECHO_PORT)\$' | sort -u;
    echo '# units'; systemctl list-unit-files --no-legend 'baft-tunnel*' 2>/dev/null | wc -l"
}
echo "--- clean-host check ---"
RA_USER_BEFORE=(); RA_FW_DRIFT=()
for side in iran abroad; do
  ssh_host "$side" "! test -e $UB && ! test -e $EB && ! test -e /var/lib/baft-tunnel && [ \"\$(systemctl list-unit-files --no-legend 'baft-tunnel*' | wc -l)\" = 0 ]" ||
    { echo "$side is not clean: an earlier BAFT-TUNNEL install is present (binary, $EB, state or units)" >&2; exit 1; }
  if ssh_host "$side" "id baft-tunnel >/dev/null 2>&1"; then RA_USER_BEFORE+=("$side"); fi
  side_snapshot "$side" > "$LOCAL_TMP/ra-before-$side.txt"
done

ra_cleanup() {
  for side in iran abroad; do
    ssh_host "$side" "for i in $PUB $SVC; do touch /run/baft-tunnel/control/\$i.stop 2>/dev/null; systemctl stop baft-tunnel@\$i.service 2>/dev/null; bash '$PKG/scripts/install-agent-control.sh' --remove --instance \$i >/dev/null 2>&1; done;
      [ -s '$REMOTE_DIR/agent.pid' ] && kill \$(cat '$REMOTE_DIR/agent.pid') 2>/dev/null; rm -f '$REMOTE_DIR/agent.pid';
      [ -x '$PKG/scripts/uninstall.sh' ] && bash '$PKG/scripts/uninstall.sh' --stop --purge-config --yes >/dev/null 2>&1;
      rm -rf /var/backups/baft-tunnel-backup.* 2>/dev/null;
      systemctl reset-failed 'baft-tunnel*' 2>/dev/null; systemctl daemon-reload; true" || true
  done
  for side in iran abroad; do
    if [[ " ${RA_USER_BEFORE[*]} " != *" $side "* ]]; then ssh_host "$side" "userdel baft-tunnel-agent 2>/dev/null; groupdel baft-tunnel-agent 2>/dev/null; userdel baft-tunnel 2>/dev/null; groupdel baft-tunnel 2>/dev/null; true" || true; fi
  done
}
# set +e: under errexit the (exit N) that restores the status would end the trap before cleanup runs.
trap 'ra_status=$?; set +e; ra_cleanup; (exit "$ra_status"); cleanup' EXIT

echo "--- install (${MVP_SOURCE}, sha256 ${MVP_SHA}) ---"
tar -czf "$LOCAL_TMP/ra-pkg.tgz" scripts/*.sh scripts/pki/*.sh systemd
cp "$MVP_BINARY_PATH" "$LOCAL_TMP/baft-tunnel-ra"
# Update candidates: same code with a different digest (trailing NUL), and a binary that passes --version but exits at start.
cp "$MVP_BINARY_PATH" "$LOCAL_TMP/baft-tunnel-ra-new"; printf '\0' >> "$LOCAL_TMP/baft-tunnel-ra-new"; chmod 755 "$LOCAL_TMP/baft-tunnel-ra-new"
printf '#!/bin/sh\n[ "$1" = "--version" ] && { echo "baft-tunnel 0.0.0-bad"; exit 0; }\nexit 1\n' > "$LOCAL_TMP/baft-tunnel-ra-bad"; chmod 755 "$LOCAL_TMP/baft-tunnel-ra-bad"
RA_NEW_SHA="$(sha256sum "$LOCAL_TMP/baft-tunnel-ra-new" | awk '{print $1}')"
for side in iran abroad; do
  copy_to_host "$side" "$LOCAL_TMP/ra-pkg.tgz" "$LOCAL_TMP/baft-tunnel-ra" "$LOCAL_TMP/baft-tunnel-ra-new" "$LOCAL_TMP/baft-tunnel-ra-bad"
  ssh_host "$side" "set -e; mkdir -p '$PKG'; tar -xzf '$REMOTE_DIR/ra-pkg.tgz' -C '$PKG';
    [ \"\$(sha256sum '$REMOTE_DIR/baft-tunnel-ra' | cut -d' ' -f1)\" = '$MVP_SHA' ];
    bash '$PKG/scripts/install.sh' --binary '$REMOTE_DIR/baft-tunnel-ra' >/dev/null;
    [ \"\$(sha256sum $UB | cut -d' ' -f1)\" = '$MVP_SHA' ]"
done

echo "--- offline PKI (runner plays the offline CA) ---"
RP="$LOCAL_TMP/rapki"; mkdir -p "$RP"; (umask 077; printf 'lab-ephemeral-ra-ca-%s\n' "$RANDOM$RANDOM$RANDOM" > "$RP/pass")
export BAFT_PKI_PASSFILE="$RP/pass"
bash scripts/pki/baft-pki.sh init-ca "$RP/ca" >/dev/null 2>&1
ra_issue() { # node host_ip
  bash scripts/pki/baft-pki.sh new-node "$1" "$RP/$1" >/dev/null
  bash scripts/pki/baft-pki.sh issue-node "$RP/ca" "$1" "$RP/$1/node.csr" "$RP/issued-$1" "$2" 30 >/dev/null
  bash scripts/pki/baft-pki.sh verify-node "$RP/ca" "$RP/issued-$1/ca.crl" "$RP/issued-$1/node.crt" "$1" "$2" >/dev/null
}
ra_issue abroad "$LAB_ABROAD_HOST"; ra_issue iran "$LAB_IRAN_HOST"
cp "$RP/issued-iran/ca.crl" "$RP/ca.crl"
for node in abroad iran; do
  cp "$RP/$node/node.key" "$RP/ra-$node-node.key"; cp "$RP/issued-$node/node.crt" "$RP/ra-$node-node.crt"
  cp "$RP/issued-$node/ca.crt" "$RP/ra-ca.crt"; cp "$RP/ca.crl" "$RP/ra-ca.crl"
  copy_to_host "$node" "$RP/ra-$node-node.key" "$RP/ra-$node-node.crt" "$RP/ra-ca.crt" "$RP/ra-ca.crl"
  ssh_host "$node" "set -e; install -o root -g root -m 0600 '$REMOTE_DIR/ra-$node-node.key' $EB/node.key; install -m 0644 '$REMOTE_DIR/ra-$node-node.crt' $EB/node.crt;
    install -m 0644 '$REMOTE_DIR/ra-ca.crt' $EB/ca.crt; install -m 0644 '$REMOTE_DIR/ra-ca.crl' $EB/ca.crl; rm -f '$REMOTE_DIR/ra-$node-node.key'"
done

echo "--- guided setup with --direction reverse: init, bundle exchange, join ---"
rcommon="--engine tcp-tls --direction reverse --tunnel-id revacc --service-port $MSVC --tunnel-port $MTP --dir $EB"
sum_i="$(ssh_host iran "$UB setup init $rcommon --role serve --node-id iran --peer-id abroad --instance $PUB" | awk '/^Checksum/{print $NF}')"
sum_a="$(ssh_host abroad "$UB setup init $rcommon --role forward --node-id abroad --peer-id iran --peer-host '$LAB_IRAN_HOST' --instance $SVC" | awk '/^Checksum/{print $NF}')"
[[ "$sum_a" =~ ^[0-9a-f-]{32,}$ && "$sum_i" =~ ^[0-9a-f-]{32,}$ ]] || { echo "setup init printed no checksum" >&2; exit 1; }
ssh_host abroad "cat $EB/bundle.json" > "$RP/bundle-abroad.json"
ssh_host iran "cat $EB/bundle.json" > "$RP/bundle-iran.json"
copy_to_host iran "$RP/bundle-abroad.json"; copy_to_host abroad "$RP/bundle-iran.json"
ssh_host abroad "$UB setup join --dir $EB --bundle '$REMOTE_DIR/bundle-iran.json' --checksum '$sum_i' >/dev/null"
ssh_host iran "$UB setup join --dir $EB --bundle '$REMOTE_DIR/bundle-abroad.json' --checksum '$sum_a' >/dev/null"
for pair in "iran $PUB" "abroad $SVC"; do
  read -r pside pinst <<<"$pair"
  ssh_host "$pside" "chown baft-tunnel:baft-tunnel $EB/node.key && chgrp baft-tunnel $EB/$pinst.json && bash '$PKG/scripts/install-agent-control.sh' --instance $pinst --listen 127.0.0.1:18099 >/dev/null"
done
# The generated configs must carry the Reverse topology: ingress+listen on the public node, target+remote on the service node.
ssh_host iran "python3 -c 'import json;c=json.load(open(\"$EB/$PUB.json\"));assert c[\"direction\"]==\"reverse\" and c[\"engine\"]==\"tcp-tls\" and c[\"mode\"]==\"serve\" and c[\"ingress\"]==\"127.0.0.1:$MSVC\" and c[\"listen\"].endswith(\":$MTP\") and \"target\" not in c, c'" ||
  { echo "public-node config does not describe Reverse" >&2; exit 1; }
ssh_host abroad "python3 -c 'import json;c=json.load(open(\"$EB/$SVC.json\"));assert c[\"direction\"]==\"reverse\" and c[\"mode\"]==\"forward\" and c[\"target\"]==\"127.0.0.1:$MSVC\" and c[\"remote\"]==\"$LAB_IRAN_HOST:$MTP\" and \"listen\" not in c, c'" ||
  { echo "service-node config does not describe Reverse" >&2; exit 1; }
RA_SNI="$(ssh_host abroad "python3 -c 'import json;print(json.load(open(\"$EB/$SVC.json\"))[\"server_name\"])'")"

echo "--- local agent per host (service user, loopback) ---"
for side in iran abroad; do
  cfg=$PUB; [[ "$side" == abroad ]] && cfg=$SVC
  ssh_host "$side" "set -e; install -d -o baft-tunnel -g baft-tunnel -m 0700 $AGD;
    setpriv --reuid=baft-tunnel --regid=baft-tunnel --init-groups $UB agent-token --out $AGD/token >/dev/null;
    nohup setpriv --reuid=baft-tunnel --regid=baft-tunnel --init-groups $UB agent --config $EB/$cfg.json --token-file $AGD/token --state-dir $AGD --listen 127.0.0.1:$MAG --instance $cfg > '$REMOTE_DIR/agent.log' 2>&1 < /dev/null & echo \$! > '$REMOTE_DIR/agent.pid';
    for n in \$(seq 1 40); do python3 -c 'import urllib.request;urllib.request.urlopen(\"http://127.0.0.1:$MAG/v1/health\",timeout=1)' 2>/dev/null && exit 0; sleep 0.25; done; tail -20 '$REMOTE_DIR/agent.log'; exit 1"
done
ra_apply() { # side instance role key -> receipt JSON on stdout (direction reverse)
  local side="$1" inst="$2" role="$3" key="$4" ph=None sn=None nid=iran pid=abroad
  if [[ "$role" == forward ]]; then ph="\"$LAB_IRAN_HOST\""; sn="\"$RA_SNI\""; nid=abroad; pid=iran; fi
  ssh_host "$side" "python3 - <<'PY'
import json, urllib.request
tok = open('$AGD/token').read().strip()
plan = {'engine': 'tcp-tls', 'role': '$role', 'direction': 'reverse', 'node_id': '$nid', 'peer_id': '$pid', 'tunnel_id': 'revacc',
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
RA_SINCE_iran="$(ssh_host iran 'date -u "+%F %T"')"; RA_SINCE_abroad="$(ssh_host abroad 'date -u "+%F %T"')"
ra_authfail() { local since="RA_SINCE_$1"; ssh_host "$1" "journalctl --utc --since '${!since}' -u baft-tunnel@$2.service --no-pager 2>/dev/null | grep -c 'secure_channel stage failed' || true"; }
ra_wait() { local t0=$SECONDS; while (( SECONDS - t0 < $1 )); do probe_host iran 65536 >/dev/null 2>&1 && { echo $((SECONDS - t0)); return 0; }; sleep 2; done; return 1; }
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
ra_evidence() { # label min_bytes : diagnostics, status and traffic on both nodes must agree with the measured tunnel
  local label="$1" need="$2" side peer
  for side in iran abroad; do
    peer=abroad; [[ "$side" == abroad ]] && peer=iran
    p1_api "$side" GET /v1/traffic >"$LOCAL_TMP/ra-traffic-$label-$side.json"
    p1_api "$side" POST /v1/diagnostics >"$LOCAL_TMP/ra-diag-$label-$side.json"
    p1_api "$side" GET /v1/status >"$LOCAL_TMP/ra-status-$label-$side.json"
    python3 - "$LOCAL_TMP/ra-traffic-$label-$side.json" "$LOCAL_TMP/ra-diag-$label-$side.json" "$LOCAL_TMP/ra-status-$label-$side.json" "$peer" "$need" "$side" <<'PY' || { cat "$LOCAL_TMP"/ra-*-"$label-$side".json >&2; echo "Reverse evidence ($label) on $side does not match the measured tunnel" >&2; exit 1; }
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
print("reverse-evidence side=%s peer=%s from_peer=%d to_peer=%d state=%s" % (side, peer, tr["bytes_from_peer"], tr["bytes_to_peer"], t["state"]))
PY
  done
}

echo "--- Apply start (direction reverse): public node first, then service node; data integrity measured ---"
ssh_host abroad "nohup python3 '$REMOTE_DIR/echo.py' '$MSVC' > '$REMOTE_DIR/echo2.log' 2>&1 < /dev/null & echo \$! > '$REMOTE_DIR/echo2.pid'"
OLD_LOCAL_PORT="$LOCAL_PORT"; LOCAL_PORT="$MSVC"
out="$(ra_apply iran $PUB serve ra-pub-0001)"
[[ "$(echo "$out" | jf result)" == applied ]] || { echo "$out" >&2; ssh_host iran "journalctl -u baft-tunnel@$PUB -n 40 --no-pager" >&2 || true; echo "Apply start (public node) was not applied" >&2; exit 1; }
out="$(ra_apply abroad $SVC forward ra-svc-0001)"
[[ "$(echo "$out" | jf result)" == applied ]] || { echo "$out" >&2; ssh_host abroad "journalctl -u baft-tunnel@$SVC -n 40 --no-pager" >&2 || true; echo "Apply start (service node) was not applied" >&2; exit 1; }
RA_SESSION="$(echo "$out" | jf stages.authorized_session)"; RA_READY="$(echo "$out" | jf ready)"
[[ "$RA_READY" != ready && "$RA_READY" != true && "$RA_READY" != True ]] || { echo "Apply receipt claimed ready without a data-path observation" >&2; exit 1; }
ra_wait 60 >/dev/null || { echo "Reverse tunnel did not carry data after Apply start" >&2; exit 1; }
probe_host iran 65536; probe_host iran 2097289
# Outbound-only: the service node must have no listener on the tunnel port and hold an outbound connection to the public node.
ssh_host abroad "! ss -ltnH | awk '{print \$4}' | grep -q ':$MTP\$' && ss -tnH state established | awk '{print \$4}' | grep -q ':$MTP\$'" ||
  { ssh_host abroad "ss -tanH | awk '{print \$1, \$4, \$5}' | grep ':$MTP' | sed -E 's/[0-9]{1,3}(\\.[0-9]{1,3}){3}/<ip>/g'" >&2 || true; echo "service node listens on the tunnel port or has no outbound connection to the public node" >&2; exit 1; }
ssh_host iran "ss -ltnH | awk '{print \$4}' | grep -q ':$MTP\$' && ss -ltnH | awk '{print \$4}' | grep -q '127.0.0.1:$MSVC\$'" || { echo "public node lacks the tunnel listener or the user ingress" >&2; exit 1; }
# Both units must run as the service user, not root.
for pair in "iran $PUB" "abroad $SVC"; do
  read -r pside pinst <<<"$pair"
  ssh_host "$pside" "[ \"\$(ps -o user= -p \$(systemctl show -p MainPID --value baft-tunnel@$pinst.service) | tr -d ' ')\" = baft-tunnel ]" || { echo "unit $pinst on $pside does not run as baft-tunnel" >&2; exit 1; }
done
sleep 12   # the agents sample unit counters every 5 s
probe_host iran 65536
sleep 12
echo "--- diagnostics, status, traffic counters and audit on both nodes (Reverse) ---"
ra_evidence apply $((65536 + 2097289))
for side in iran abroad; do
  ssh_host "$side" "python3 -c 'import json,sys; [json.loads(l) for l in open(sys.argv[1])]' $AGD/audit.log && grep -q '\"actor\"' $AGD/audit.log && ! grep -qs -- \"\$(cat $AGD/token)\" $AGD/audit.log && ! grep -rqs -- \"\$(cat $AGD/token)\" $AGD/receipts && ! grep -rqs 'PRIVATE KEY' $AGD/receipts" ||
    { echo "audit log or receipts on $side lack actor fields or hold a secret" >&2; exit 1; }
done
[[ "$(ra_apply iran $PUB serve ra-pub-0001 | jf replayed)" == True ]] || { echo "idempotent replay failed" >&2; exit 1; }
[[ "$(ra_apply iran $PUB serve ra-pub-0002 | jf result)" == refused ]] || { echo "Apply start on a running public node was not refused" >&2; exit 1; }

echo "--- stability: ${REVERSE_ACCEPT}s of traffic every ~10 s over the installed Reverse tunnel ---"
read -r pf0 pr0 pn0 <<<"$(unit_stats iran $PUB)"; read -r sf0 sr0 sn0 <<<"$(unit_stats abroad $SVC)"
PID_PUB0="$(mainpid iran $PUB)"; PID_SVC0="$(mainpid abroad $SVC)"
end=$((SECONDS + REVERSE_ACCEPT)); RA_ROUNDS=0; RA_FAILS=0; RA_MAXLAT=0
while (( SECONDS < end )); do
  t0=$SECONDS
  if probe_host iran 65536 >/dev/null 2>&1; then :; else RA_FAILS=$((RA_FAILS + 1)); fi
  lat=$((SECONDS - t0)); (( lat > RA_MAXLAT )) && RA_MAXLAT=$lat
  RA_ROUNDS=$((RA_ROUNDS + 1))
  if (( RA_ROUNDS % 30 == 0 )); then probe_host iran 2097289 >/dev/null 2>&1 || RA_FAILS=$((RA_FAILS + 1)); fi
  sleep 10
done
probe_host iran 2097289
read -r pf1 pr1 pn1 <<<"$(unit_stats iran $PUB)"; read -r sf1 sr1 sn1 <<<"$(unit_stats abroad $SVC)"
RA_SOAK_LINE="soak_s=$REVERSE_ACCEPT rounds=$RA_ROUNDS failed_probes=$RA_FAILS max_probe_s=$RA_MAXLAT public_fd=$pf0->$pf1 public_rss_kb=$pr0->$pr1 service_fd=$sf0->$sf1 service_rss_kb=$sr0->$sr1 restarts=$pn0->$pn1/$sn0->$sn1"
echo "REVERSE_SOAK $RA_SOAK_LINE"
(( RA_FAILS == 0 )) || { echo "probes failed during the Reverse soak" >&2; exit 1; }
[[ "$pn0" == "$pn1" && "$sn0" == "$sn1" && "$(mainpid iran $PUB)" == "$PID_PUB0" && "$(mainpid abroad $SVC)" == "$PID_SVC0" ]] || { echo "a unit restarted during the Reverse soak" >&2; exit 1; }
(( pf1 <= pf0 + 8 && sf1 <= sf0 + 8 )) || { echo "fd growth during the Reverse soak" >&2; exit 1; }
(( pr1 <= pr0 * 2 + 20480 && sr1 <= sr0 * 2 + 20480 )) || { echo "memory growth during the Reverse soak" >&2; exit 1; }
sleep 6
ra_evidence soak 2097289
RA_SOAK_FAILS="$(ra_authfail iran $PUB) $(ra_authfail abroad $SVC)"
echo "REVERSE_SECURE_CHANNEL_FAILURES_AFTER_SOAK public/service=${RA_SOAK_FAILS// //} (this run only)"
[[ "$RA_SOAK_FAILS" == "0 0" ]] || { echo "secure_channel stage failures were logged during a healthy soak: $RA_SOAK_FAILS" >&2; exit 1; }

echo "--- reconnect: service-node SIGKILL (systemd restart), public-node SIGKILL, restarts through stop + Apply start ---"
ssh_host abroad "kill -KILL \$(systemctl show -p MainPID --value baft-tunnel@$SVC.service)"
RA_SVC_KILL_S="$(ra_wait 90)" || { echo "tunnel did not recover after service-node SIGKILL" >&2; exit 1; }
ssh_host iran "kill -KILL \$(systemctl show -p MainPID --value baft-tunnel@$PUB.service)"
RA_PUB_KILL_S="$(ra_wait 90)" || { echo "tunnel did not recover after public-node SIGKILL" >&2; exit 1; }
unit_stop abroad $SVC || { echo "could not stop $SVC through the control helper" >&2; exit 1; }
[[ "$(ra_apply abroad $SVC forward ra-svc-0002 | jf result)" == applied ]] || { echo "service-node restart was not applied" >&2; exit 1; }
RA_SVC_APPLY_S="$(ra_wait 90)" || { echo "tunnel did not recover after the service-node Apply restart" >&2; exit 1; }
unit_stop iran $PUB || { echo "could not stop $PUB through the control helper" >&2; exit 1; }
[[ "$(ra_apply iran $PUB serve ra-pub-0003 | jf result)" == applied ]] || { echo "public-node restart was not applied" >&2; exit 1; }
RA_PUB_APPLY_S="$(ra_wait 90)" || { echo "tunnel did not recover after the public-node Apply restart" >&2; exit 1; }
probe_host iran 2097289

echo "--- backup, simulated loss, restore (non-root ownership must survive) ---"
ra_own() { ssh_host "$1" "cd $EB && stat -c '%n %U:%G %a' node.key $2.json ca.crt ca.crl node.crt | sort | sha256sum | cut -d' ' -f1; cd $EB && sha256sum $2.json node.crt ca.crt ca.crl | sha256sum | cut -d' ' -f1"; }
RA_OWN_PUB="$(ra_own iran $PUB)"; RA_OWN_SVC="$(ra_own abroad $SVC)"
RA_BK_PUB="$(ssh_host iran "bash '$PKG/scripts/backup.sh'" | sed -n 's/^Backup: //p' | tail -n 1)"
RA_BK_SVC="$(ssh_host abroad "bash '$PKG/scripts/backup.sh'" | sed -n 's/^Backup: //p' | tail -n 1)"
[[ "$RA_BK_PUB" == /var/backups/baft-tunnel-backup.* && "$RA_BK_SVC" == /var/backups/baft-tunnel-backup.* ]] || { echo "backup printed no directory" >&2; exit 1; }
ssh_host iran "! grep -rqs 'PRIVATE KEY' $RA_BK_PUB/manifest.txt && [ \"\$(stat -c %a $RA_BK_PUB $RA_BK_PUB/backup.tar)\" = \"\$(printf '700\n600')\" ]" || { echo "backup modes/manifest not as documented" >&2; exit 1; }
unit_stop iran $PUB; unit_stop abroad $SVC
for side in iran abroad; do ssh_host "$side" "[ -s '$REMOTE_DIR/agent.pid' ] && kill \$(cat '$REMOTE_DIR/agent.pid'); rm -f '$REMOTE_DIR/agent.pid'; sleep 1"; done
# Loss: instance config removed and the trust anchor corrupted on both hosts.
ssh_host iran "rm -f $EB/$PUB.json; : > $EB/ca.crt"; ssh_host abroad "rm -f $EB/$SVC.json; : > $EB/ca.crt"
ssh_host iran "bash '$PKG/scripts/restore.sh' --yes '$RA_BK_PUB' >/dev/null"; ssh_host abroad "bash '$PKG/scripts/restore.sh' --yes '$RA_BK_SVC' >/dev/null"
[[ "$(ra_own iran $PUB)" == "$RA_OWN_PUB" && "$(ra_own abroad $SVC)" == "$RA_OWN_SVC" ]] || { echo "restore changed config content, owners or modes" >&2; exit 1; }
for side in iran abroad; do
  cfg=$PUB; [[ "$side" == abroad ]] && cfg=$SVC
  ssh_host "$side" "set -e; install -d -o baft-tunnel -g baft-tunnel -m 0700 $AGD; [ -s $AGD/token ] || setpriv --reuid=baft-tunnel --regid=baft-tunnel --init-groups $UB agent-token --out $AGD/token >/dev/null;
    nohup setpriv --reuid=baft-tunnel --regid=baft-tunnel --init-groups $UB agent --config $EB/$cfg.json --token-file $AGD/token --state-dir $AGD --listen 127.0.0.1:$MAG --instance $cfg > '$REMOTE_DIR/agent.log' 2>&1 < /dev/null & echo \$! > '$REMOTE_DIR/agent.pid';
    for n in \$(seq 1 40); do python3 -c 'import urllib.request;urllib.request.urlopen(\"http://127.0.0.1:$MAG/v1/health\",timeout=1)' 2>/dev/null && exit 0; sleep 0.25; done; tail -20 '$REMOTE_DIR/agent.log'; exit 1"
done
[[ "$(ra_apply iran $PUB serve ra-pub-0004 | jf result)" == applied ]] || { echo "public node did not start from the restored configuration" >&2; exit 1; }
[[ "$(ra_apply abroad $SVC forward ra-svc-0003 | jf result)" == applied ]] || { echo "service node did not start from the restored configuration" >&2; exit 1; }
RA_RESTORE_S="$(ra_wait 90)" || { echo "Reverse tunnel did not carry data after the restore" >&2; exit 1; }
probe_host iran 2097289

echo "--- guarded update: same code with a new digest, then a binary that fails at start (automatic rollback) ---"
ra_digest() { ssh_host "$1" "sha256sum $UB | cut -d' ' -f1"; }
for side in iran abroad; do
  inst=$PUB; [[ "$side" == abroad ]] && inst=$SVC
  ssh_host "$side" "bash '$PKG/scripts/update.sh' --binary '$REMOTE_DIR/baft-tunnel-ra-new' --instance $inst --health-secs 10" | grep -q '^UPDATE_OK' || { echo "update on $side was not accepted" >&2; exit 1; }
  [[ "$(ra_digest "$side")" == "$RA_NEW_SHA" ]] || { echo "update on $side did not install the new binary" >&2; exit 1; }
done
RA_UPDATE_S="$(ra_wait 90)" || { echo "Reverse tunnel did not carry data after the update" >&2; exit 1; }
probe_host iran 2097289
for side in iran abroad; do ssh_host "$side" "[ -s '$REMOTE_DIR/agent.pid' ] && kill \$(cat '$REMOTE_DIR/agent.pid'); rm -f '$REMOTE_DIR/agent.pid'"; done
for side in iran abroad; do
  cfg=$PUB; [[ "$side" == abroad ]] && cfg=$SVC
  ssh_host "$side" "nohup setpriv --reuid=baft-tunnel --regid=baft-tunnel --init-groups $UB agent --config $EB/$cfg.json --token-file $AGD/token --state-dir $AGD --listen 127.0.0.1:$MAG --instance $cfg > '$REMOTE_DIR/agent.log' 2>&1 < /dev/null & echo \$! > '$REMOTE_DIR/agent.pid';
    for n in \$(seq 1 40); do python3 -c 'import urllib.request;urllib.request.urlopen(\"http://127.0.0.1:$MAG/v1/health\",timeout=1)' 2>/dev/null && exit 0; sleep 0.25; done; exit 1"
done
sleep 14
probe_host iran 65536
sleep 12   # the agents sample unit counters every 5 s; the data_path tier needs payload inside its window
ra_evidence update 65536
RA_UPD_FAILS="$(ra_authfail iran $PUB) $(ra_authfail abroad $SVC)"
echo "REVERSE_SECURE_CHANNEL_FAILURES_AFTER_UPDATE public/service=${RA_UPD_FAILS// //} (this run only, no revocation yet)"
[[ "$RA_UPD_FAILS" == "0 0" ]] || { echo "secure_channel stage failures were logged before revocation: $RA_UPD_FAILS" >&2; exit 1; }
# Bad update on the service node: must exit non-zero, restore the previous digest and bring the tunnel back.
if ssh_host abroad "bash '$PKG/scripts/update.sh' --binary '$REMOTE_DIR/baft-tunnel-ra-bad' --instance $SVC --health-secs 6" >"$LOCAL_TMP/ra-badupdate.log" 2>&1; then
  echo "update with a failing binary was accepted" >&2; exit 1
fi
[[ "$(ra_digest abroad)" == "$RA_NEW_SHA" ]] || { echo "failed update did not restore the previous binary" >&2; exit 1; }
unit_active abroad $SVC || { echo "service unit is not active after the automatic rollback" >&2; exit 1; }
RA_ROLLBACK_S="$(ra_wait 90)" || { echo "Reverse tunnel did not carry data after the automatic rollback" >&2; exit 1; }
probe_host iran 2097289

echo "--- revocation: service-node certificate revoked, new CRL on the public node, public node restarted ---"
bash scripts/pki/baft-pki.sh revoke-node "$RP/ca" "$RP/issued-abroad/node.crt" "$RP/revoked" >/dev/null
copy_to_host iran "$RP/revoked/ca.crl"
ssh_host iran "install -m 0644 '$REMOTE_DIR/ca.crl' $EB/ca.crl"
unit_stop iran $PUB || { echo "could not stop $PUB" >&2; exit 1; }
[[ "$(ra_apply iran $PUB serve ra-pub-0005 | jf result)" == applied ]] || { echo "public node with the new CRL was not applied" >&2; exit 1; }
sleep 12
if probe_host iran 4096 >/dev/null 2>&1; then echo "revoked service-node certificate still carried data" >&2; exit 1; fi
ssh_host iran "journalctl -u baft-tunnel@$PUB --no-pager -n 200 | grep -qi revok" || { echo "public-node log shows no revocation refusal" >&2; exit 1; }
p1_api abroad POST /v1/diagnostics >"$LOCAL_TMP/ra-diag-revoked.json"
python3 - "$LOCAL_TMP/ra-diag-revoked.json" <<'PY' || { cat "$LOCAL_TMP/ra-diag-revoked.json" >&2; echo "service-node diagnostics did not attribute the revocation to the secure channel" >&2; exit 1; }
import json, sys
d = json.load(open(sys.argv[1])); r = {t["id"]: t["result"] for t in d["tiers"]}
print("revoked tiers=%s" % json.dumps(r, sort_keys=True))
assert r["secure_channel"] == "fail" and r["session"] in ("fail", "unknown") and r["data_path"] != "pass", r
assert d["verified"] is False, d
PY
LOCAL_PORT="$OLD_LOCAL_PORT"; stop_role abroad echo2

echo "--- uninstall and cleanup proof ---"
ra_cleanup; trap cleanup EXIT
for side in iran abroad; do
  ssh_host "$side" "! test -e $UB && ! test -e $EB && ! test -e /var/lib/baft-tunnel && ! test -e /run/baft-tunnel/control && ! ls /var/backups/baft-tunnel-backup.* >/dev/null 2>&1 && ! systemctl list-units --all --plain --no-legend 'baft-tunnel*' | grep -q ." ||
    { echo "uninstall left BAFT-TUNNEL resources on $side" >&2; exit 1; }
  side_snapshot "$side" > "$LOCAL_TMP/ra-after-$side.txt"
  if ! diff "$LOCAL_TMP/ra-before-$side.txt" "$LOCAL_TMP/ra-after-$side.txt" > "$LOCAL_TMP/ra-diff-$side.txt"; then
    echo "--- host state changed on $side during the stage ($(grep -c '^[<>]' "$LOCAL_TMP/ra-diff-$side.txt") lines; addresses masked) ---"
    grep '^[<>]' "$LOCAL_TMP/ra-diff-$side.txt" | sed -E 's/[0-9]{1,3}(\.[0-9]{1,3}){3}(\/[0-9]+)?/<ip>/g; s/[0-9a-fA-F]{0,4}(:[0-9a-fA-F]{0,4}){2,7}(\/[0-9]+)?/<ip6>/g' | head -n 30
    if grep -E '^[<>]' "$LOCAL_TMP/ra-diff-$side.txt" | grep -qiE "baft|(:| |dport |sport )($MSVC|$MTP|$MAG)\b" ||
       ! diff <(sed -n '/^# listeners/,$p' "$LOCAL_TMP/ra-before-$side.txt") <(sed -n '/^# listeners/,$p' "$LOCAL_TMP/ra-after-$side.txt") >/dev/null; then
      echo "listeners, unit files or BAFT-related firewall rules changed on $side" >&2; exit 1
    fi
    RA_FW_DRIFT+=("$side")
  fi
done
echo "REVERSE_ACCEPT_RESULT source=${MVP_SOURCE} binary_sha256=${MVP_SHA} update_sha256=${RA_NEW_SHA} setup_direction_reverse=1 apply_start_public=applied apply_start_service=applied non_root=1 outbound_only=1 receipt_session=${RA_SESSION} receipt_ready=${RA_READY} integrity_64k_2m=1 evidence_diag_status_traffic=1 replay=1 running_refused=1 ${RA_SOAK_LINE} service_sigkill_s=${RA_SVC_KILL_S} public_sigkill_s=${RA_PUB_KILL_S} service_apply_restart_s=${RA_SVC_APPLY_S} public_apply_restart_s=${RA_PUB_APPLY_S} backup_restore=1 restore_owners_modes_identical=1 restore_recovery_s=${RA_RESTORE_S} update_ok=1 update_recovery_s=${RA_UPDATE_S} bad_update_rolled_back=1 rollback_recovery_s=${RA_ROLLBACK_S} revoked_refused=1 uninstalled=1 listeners_units_unchanged=1 baft_fw_rules=0 unrelated_fw_drift=${RA_FW_DRIFT[*]:-none} cross_version_update=0"
