# shellcheck shell=bash
# P0-5 integrated MVP acceptance stage, sourced by phase1-two-host-e2e.sh when LAB_MVP=1.
# Clean install of one binary (signed release asset or local build, recorded as such) on both hosts,
# guided setup with bundle checksums, per-instance control helper, Apply start through the local agent,
# measured 64 KiB + 2 MiB integrity, recovery, refusals, failed-start rollback and a full uninstall.
# Only resources this stage creates are removed; firewall, listeners and other units are compared.

echo "=== MVP (P0-5): clean install from ${MVP_SOURCE}, setup, Apply start, verify, negatives, uninstall ==="
stop_role iran serve; stop_role abroad serve; stop_role iran forward; stop_role abroad forward; sleep 1
MSVC=18094; MTP=18330; MAG=18096; MALT=18097
EB=/etc/baft-tunnel; UB=/usr/local/bin/baft-tunnel; AGD=/var/lib/baft-tunnel/agent
SRV=mvpsrv; CLI=mvpcli
MVP_SHA="$(sha256sum "$MVP_BINARY_PATH" | awk '{print $1}')"

side_snapshot() { # side : normalized firewall rules, listeners outside the test ports, baft-tunnel* unit-file count
  ssh_host "$1" "echo '# firewall'; { nft list ruleset 2>/dev/null | sed -E 's/packets [0-9]+ bytes [0-9]+//'; iptables-save 2>/dev/null | grep -v '^#' | sed -E 's/\\[[0-9]+:[0-9]+\\]//'; };
    echo '# listeners'; ss -ltnH | awk '{print \$4}' | grep -vE ':($MSVC|$MTP|$MAG|$MALT|$E2E_PORT|$LOCAL_PORT|$ECHO_PORT)\$' | sort -u;
    echo '# units'; systemctl list-unit-files --no-legend 'baft-tunnel*' 2>/dev/null | wc -l"
}
echo "--- MVP clean-host check ---"
MVP_USER_BEFORE=(); MVP_FW_DRIFT=()
for side in iran abroad; do
  ssh_host "$side" "! test -e $UB && ! test -e $EB && ! test -e /var/lib/baft-tunnel && [ \"\$(systemctl list-unit-files --no-legend 'baft-tunnel*' | wc -l)\" = 0 ]" ||
    { echo "$side is not clean: an earlier BAFT-TUNNEL install is present (binary, $EB, state or units)" >&2; exit 1; }
  ssh_host "$side" "systemctl --version | head -1; . /etc/os-release; echo \$PRETTY_NAME"
  if ssh_host "$side" "id baft-tunnel >/dev/null 2>&1"; then MVP_USER_BEFORE+=("$side"); fi
  side_snapshot "$side" > "$LOCAL_TMP/mvp-before-$side.txt"
done

mvp_cleanup() {
  for side in iran abroad; do
    ssh_host "$side" "for i in $SRV $CLI; do touch /run/baft-tunnel/control/\$i.stop 2>/dev/null; systemctl stop baft-tunnel@\$i.service 2>/dev/null; bash '$REMOTE_DIR/pkg/scripts/install-agent-control.sh' --remove --instance \$i >/dev/null 2>&1; done;
      [ -s '$REMOTE_DIR/agent.pid' ] && kill \$(cat '$REMOTE_DIR/agent.pid') 2>/dev/null; rm -f '$REMOTE_DIR/agent.pid';
      [ -x '$REMOTE_DIR/pkg/scripts/uninstall.sh' ] && bash '$REMOTE_DIR/pkg/scripts/uninstall.sh' --stop --purge-config --yes >/dev/null 2>&1;
      systemctl reset-failed 'baft-tunnel*' 2>/dev/null; systemctl daemon-reload; true" || true
  done
  for side in iran abroad; do
    if [[ " ${MVP_USER_BEFORE[*]} " != *" $side "* ]]; then ssh_host "$side" "userdel baft-tunnel-agent 2>/dev/null; groupdel baft-tunnel-agent 2>/dev/null; userdel baft-tunnel 2>/dev/null; groupdel baft-tunnel 2>/dev/null; true" || true; fi
  done
}
# set +e: under errexit the (exit N) that restores the status would end the trap before cleanup runs.
trap 'mvp_status=$?; set +e; mvp_cleanup; (exit "$mvp_status"); cleanup' EXIT

echo "--- MVP install (${MVP_SOURCE}, sha256 ${MVP_SHA}) ---"
tar -czf "$LOCAL_TMP/mvp-pkg.tgz" scripts/*.sh scripts/pki/*.sh systemd
cp "$MVP_BINARY_PATH" "$LOCAL_TMP/baft-tunnel-mvp"
for side in iran abroad; do
  copy_to_host "$side" "$LOCAL_TMP/mvp-pkg.tgz" "$LOCAL_TMP/baft-tunnel-mvp"
  ssh_host "$side" "set -e; mkdir -p '$REMOTE_DIR/pkg'; tar -xzf '$REMOTE_DIR/mvp-pkg.tgz' -C '$REMOTE_DIR/pkg';
    [ \"\$(sha256sum '$REMOTE_DIR/baft-tunnel-mvp' | cut -d' ' -f1)\" = '$MVP_SHA' ];
    bash '$REMOTE_DIR/pkg/scripts/install.sh' --binary '$REMOTE_DIR/baft-tunnel-mvp' >/dev/null;
    [ \"\$(sha256sum $UB | cut -d' ' -f1)\" = '$MVP_SHA' ]; $UB --version 2>/dev/null || true"
done

echo "--- MVP offline PKI (runner plays the offline CA) ---"
MP="$LOCAL_TMP/mvppki"; mkdir -p "$MP"; (umask 077; printf 'lab-ephemeral-mvp-ca-%s\n' "$RANDOM$RANDOM$RANDOM" > "$MP/pass")
export BAFT_PKI_PASSFILE="$MP/pass"
bash scripts/pki/baft-pki.sh init-ca "$MP/ca" >/dev/null 2>&1
mvp_issue() { # node host_ip
  bash scripts/pki/baft-pki.sh new-node "$1" "$MP/$1" >/dev/null
  bash scripts/pki/baft-pki.sh issue-node "$MP/ca" "$1" "$MP/$1/node.csr" "$MP/issued-$1" "$2" 30 >/dev/null
  bash scripts/pki/baft-pki.sh verify-node "$MP/ca" "$MP/issued-$1/ca.crl" "$MP/issued-$1/node.crt" "$1" "$2" >/dev/null
}
mvp_issue abroad "$LAB_ABROAD_HOST"; mvp_issue iran "$LAB_IRAN_HOST"; mvp_issue mvpctl "$LAB_IRAN_HOST"
# The latest CRL (issued after all three nodes) is the one every host gets.
cp "$MP/issued-mvpctl/ca.crl" "$MP/ca.crl"
for node in abroad iran; do
  cp "$MP/$node/node.key" "$MP/mvp-$node-node.key"; cp "$MP/issued-$node/node.crt" "$MP/mvp-$node-node.crt"
  cp "$MP/issued-$node/ca.crt" "$MP/mvp-ca.crt"; cp "$MP/ca.crl" "$MP/mvp-ca.crl"
  copy_to_host "$node" "$MP/mvp-$node-node.key" "$MP/mvp-$node-node.crt" "$MP/mvp-ca.crt" "$MP/mvp-ca.crl"
  ssh_host "$node" "set -e; install -o root -g root -m 0600 '$REMOTE_DIR/mvp-$node-node.key' $EB/node.key; install -m 0644 '$REMOTE_DIR/mvp-$node-node.crt' $EB/node.crt;
    install -m 0644 '$REMOTE_DIR/mvp-ca.crt' $EB/ca.crt; install -m 0644 '$REMOTE_DIR/mvp-ca.crl' $EB/ca.crl; rm -f '$REMOTE_DIR/mvp-$node-node.key'"
done

echo "--- MVP guided setup: init, bundle exchange, wrong checksum refused, join ---"
mcommon="--engine tcp-tls --direction direct --tunnel-id mvp --service-port $MSVC --tunnel-port $MTP --dir $EB"
sum_a="$(ssh_host abroad "$UB setup init $mcommon --role serve --node-id abroad --peer-id iran --instance $SRV" | awk '/^Checksum/{print $NF}')"
sum_i="$(ssh_host iran "$UB setup init $mcommon --role forward --node-id iran --peer-id abroad --peer-host '$LAB_ABROAD_HOST' --instance $CLI" | awk '/^Checksum/{print $NF}')"
[[ "$sum_a" =~ ^[0-9a-f-]{32,}$ && "$sum_i" =~ ^[0-9a-f-]{32,}$ ]] || { echo "setup init printed no checksum" >&2; exit 1; }
ssh_host abroad "cat $EB/bundle.json" > "$MP/bundle-abroad.json"
ssh_host iran "cat $EB/bundle.json" > "$MP/bundle-iran.json"
copy_to_host iran "$MP/bundle-abroad.json"; copy_to_host abroad "$MP/bundle-iran.json"
ssh_host iran "! $UB setup join --dir $EB --bundle '$REMOTE_DIR/bundle-abroad.json' --checksum '${sum_a//[0-9a-f]/0}' >/dev/null 2>&1 && ! test -e $EB/$CLI.json" ||
  { echo "join accepted a wrong bundle checksum" >&2; exit 1; }
ssh_host abroad "$UB setup join --dir $EB --bundle '$REMOTE_DIR/bundle-iran.json' --checksum '$sum_i' >/dev/null"
ssh_host iran "$UB setup join --dir $EB --bundle '$REMOTE_DIR/bundle-abroad.json' --checksum '$sum_a' >/dev/null"
# Documented ownership for an installed instance (OPERATOR_GUIDE "Guided setup as an installed service").
for pair in "abroad $SRV" "iran $CLI"; do
  read -r pside pinst <<<"$pair"
  ssh_host "$pside" "chown baft-tunnel:baft-tunnel $EB/node.key && chgrp baft-tunnel $EB/$pinst.json && bash '$REMOTE_DIR/pkg/scripts/install-agent-control.sh' --instance $pinst --listen 127.0.0.1:18099 >/dev/null"
done
MVP_SNI="$(ssh_host iran "python3 -c 'import json;print(json.load(open(\"$EB/$CLI.json\"))[\"server_name\"])'")"

echo "--- MVP local agent per host (service user, loopback) ---"
for side in iran abroad; do
  cfg=$SRV; [[ "$side" == iran ]] && cfg=$CLI
  ssh_host "$side" "set -e; install -d -o baft-tunnel -g baft-tunnel -m 0700 $AGD;
    setpriv --reuid=baft-tunnel --regid=baft-tunnel --init-groups $UB agent-token --out $AGD/token >/dev/null;
    nohup setpriv --reuid=baft-tunnel --regid=baft-tunnel --init-groups $UB agent --config $EB/$cfg.json --token-file $AGD/token --state-dir $AGD --listen 127.0.0.1:$MAG --instance $cfg > '$REMOTE_DIR/agent.log' 2>&1 < /dev/null & echo \$! > '$REMOTE_DIR/agent.pid';
    for n in \$(seq 1 40); do python3 -c 'import urllib.request;urllib.request.urlopen(\"http://127.0.0.1:$MAG/v1/health\",timeout=1)' 2>/dev/null && exit 0; sleep 0.25; done; tail -20 '$REMOTE_DIR/agent.log'; exit 1"
done
mvp_apply() { # side instance role key -> receipt JSON on stdout
  local side="$1" inst="$2" role="$3" key="$4" ph=None sn=None nid=abroad pid=iran
  if [[ "$role" == forward ]]; then ph="\"$LAB_ABROAD_HOST\""; sn="\"$MVP_SNI\""; nid=iran; pid=abroad; fi
  ssh_host "$side" "python3 - <<'PY'
import json, urllib.request
tok = open('$AGD/token').read().strip()
plan = {'engine': 'tcp-tls', 'role': '$role', 'direction': 'direct', 'node_id': '$nid', 'peer_id': '$pid', 'tunnel_id': 'mvp',
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
unit_stop() { # side instance : stop through the control helper, wait for inactive
  ssh_host "$1" "setpriv --reuid=baft-tunnel --regid=baft-tunnel --init-groups touch /run/baft-tunnel/control/$2.stop; for n in \$(seq 1 40); do systemctl is-active --quiet baft-tunnel@$2.service || exit 0; sleep 0.5; done; exit 1"
}
mvp_wait() { # seconds : until 64 KiB passes again
  local t0=$SECONDS
  while (( SECONDS - t0 < $1 )); do probe_host iran 65536 >/dev/null 2>&1 && { echo $((SECONDS - t0)); return 0; }; sleep 2; done
  return 1
}

echo "--- MVP Apply start: serve, then forward; authorized session observed; data integrity measured ---"
ssh_host abroad "nohup python3 '$REMOTE_DIR/echo.py' '$MSVC' > '$REMOTE_DIR/echo2.log' 2>&1 < /dev/null & echo \$! > '$REMOTE_DIR/echo2.pid'"
OLD_LOCAL_PORT="$LOCAL_PORT"; LOCAL_PORT="$MSVC"
out="$(mvp_apply abroad $SRV serve mvp-srv-0001)"
[[ "$(echo "$out" | jf result)" == applied ]] || { echo "$out" >&2; ssh_host abroad "journalctl -u baft-tunnel@$SRV -n 40 --no-pager" >&2 || true; echo "Apply start (serve) was not applied" >&2; exit 1; }
out="$(mvp_apply iran $CLI forward mvp-cli-0001)"
[[ "$(echo "$out" | jf result)" == applied ]] || { echo "$out" >&2; ssh_host iran "journalctl -u baft-tunnel@$CLI -n 40 --no-pager" >&2 || true; echo "Apply start (forward) was not applied" >&2; exit 1; }
MVP_SESSION="$(echo "$out" | jf stages.authorized_session)"; MVP_READY="$(echo "$out" | jf ready)"
[[ "$MVP_READY" != ready && "$MVP_READY" != true && "$MVP_READY" != True ]] || { echo "Apply receipt claimed ready without a data-path observation" >&2; exit 1; }
# The agents sample unit counters every 5 s: let them see the idle counters before traffic.
sleep 12
probe_host iran 65536; probe_host iran 2097289
sleep 12
echo "--- P1 operations evidence (ADR-0024): diagnostics, status, traffic counters, audit on both hosts ---"
p1_api() { # side METHOD path -> JSON on stdout
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
for side in iran abroad; do
  peer=abroad; [[ "$side" == abroad ]] && peer=iran
  p1_api "$side" GET /v1/traffic >"$LOCAL_TMP/p1-traffic-$side.json"
  p1_api "$side" POST /v1/diagnostics >"$LOCAL_TMP/p1-diag-$side.json"
  p1_api "$side" GET /v1/status >"$LOCAL_TMP/p1-status-$side.json"
  python3 - "$LOCAL_TMP/p1-traffic-$side.json" "$LOCAL_TMP/p1-diag-$side.json" "$LOCAL_TMP/p1-status-$side.json" "$peer" <<'PY' || { cat "$LOCAL_TMP"/p1-*-"$side".json >&2; echo "P1 evidence on $side does not match the measured tunnel" >&2; exit 1; }
import json, sys
tr, dg, st = (json.load(open(f)) for f in sys.argv[1:4]); peer = sys.argv[4]
need = 65536 + 2097289
assert tr["available"] and tr["fresh"] and tr["units"] == "bytes", tr
assert tr["bytes_from_peer"] >= need and tr["bytes_to_peer"] >= need, tr
r = {t["id"]: t["result"] for t in dg["tiers"]}
assert r == {"tcp_reachability": "pass", "secure_channel": "pass", "identity": "pass", "authorization": "pass",
             "session": "pass", "data_path": "pass", "integrity": "unknown"}, r
assert dg["verified"] is True and dg["unit"]["result"] == "active", dg
t = st["tunnel"]
assert t["state"] == "carrying_traffic" and t["tiers"]["authorized_session"]["peer_id"] == peer, t
print("p1 peer=%s from_peer=%d to_peer=%d state=%s" % (peer, tr["bytes_from_peer"], tr["bytes_to_peer"], t["state"]))
PY
  ssh_host "$side" "python3 -c 'import json,sys; [json.loads(l) for l in open(sys.argv[1])]' $AGD/audit.log && grep -q '\"actor\"' $AGD/audit.log && ! grep -qs -- \"\$(cat $AGD/token)\" $AGD/audit.log" \
    || { echo "audit log on $side lacks actor fields or holds the token" >&2; exit 1; }
done
P1_EVIDENCE=1
[[ "$(mvp_apply iran $CLI forward mvp-cli-0001 | jf replayed)" == True ]] || { echo "idempotent replay failed" >&2; exit 1; }
[[ "$(mvp_apply iran $CLI forward mvp-cli-0002 | jf result)" == refused ]] || { echo "Apply start on a running unit was not refused" >&2; exit 1; }
for side in iran abroad; do
  ssh_host "$side" "! grep -rqs -- \"\$(cat $AGD/token)\" $AGD/receipts && ! grep -rqs 'PRIVATE KEY' $AGD/receipts" || { echo "a receipt on $side holds a secret" >&2; exit 1; }
done

echo "--- MVP recovery: forward process SIGKILL (systemd restart), serve restart through stop + Apply start ---"
ssh_host iran "kill -KILL \$(systemctl show -p MainPID --value baft-tunnel@$CLI.service)"
MVP_KILL_S="$(mvp_wait 90)" || { echo "forward unit did not recover after SIGKILL" >&2; exit 1; }
unit_stop abroad $SRV || { echo "could not stop $SRV through the control helper" >&2; exit 1; }
[[ "$(mvp_apply abroad $SRV serve mvp-srv-0002 | jf result)" == applied ]] || { echo "serve restart was not applied" >&2; exit 1; }
MVP_SRV_S="$(mvp_wait 90)" || { echo "tunnel did not recover after the serve restart" >&2; exit 1; }
probe_host iran 2097289

echo "--- MVP occupied port: Apply start must roll back and leave the unit stopped ---"
unit_stop iran $CLI || { echo "could not stop $CLI" >&2; exit 1; }
ssh_host iran "nohup python3 -c 'import socket,time;s=socket.socket();s.setsockopt(socket.SOL_SOCKET,socket.SO_REUSEADDR,1);s.bind((\"127.0.0.1\",$MSVC));s.listen();time.sleep(300)' >/dev/null 2>&1 < /dev/null & echo \$! > '$REMOTE_DIR/impair.pid'"
sleep 1
out="$(mvp_apply iran $CLI forward mvp-cli-0003)"
[[ "$(echo "$out" | jf result)" == rolled_back ]] || { echo "$out" >&2; echo "start on an occupied port was not rolled back" >&2; exit 1; }
! unit_active iran $CLI || { echo "rollback left $CLI running" >&2; exit 1; }
stop_role iran impair; sleep 1
[[ "$(mvp_apply iran $CLI forward mvp-cli-0004 | jf result)" == applied ]] || { echo "start after the port was freed was not applied" >&2; exit 1; }
mvp_wait 60 >/dev/null || { echo "tunnel did not come back after the rollback" >&2; exit 1; }

echo "--- MVP refusals with the installed binary: wrong pin, wrong identity ---"
# Stop the previous foreground forward by its pidfile. Never 'pkill -f <path>': the remote shell's own
# command line contains the path, so pkill would kill that shell (run 42535679925 failed this way).
NEG_STOP="if [ -s '$REMOTE_DIR/neg.pid' ]; then kill \$(cat '$REMOTE_DIR/neg.pid') 2>/dev/null || true; rm -f '$REMOTE_DIR/neg.pid'; sleep 1; fi"
neg_log() { ssh_host iran "tail -n 20 '$REMOTE_DIR/neg.log'" >&2 || true; }
mvp_neg() { # json-edit-python expect(ok|refused) : foreground forward on $MALT with an edited copy of the iran config
  local want=\! ; [[ "$2" == ok ]] && want=""
  ssh_host iran "set -e; $NEG_STOP; python3 - <<'PY'
import json
c = json.load(open('$EB/$CLI.json')); c['listen'] = '127.0.0.1:$MALT'
$1
json.dump(c, open('$REMOTE_DIR/neg.json', 'w'))
PY
chown baft-tunnel '$REMOTE_DIR/neg.json'; chmod 711 '$REMOTE_DIR'
nohup setpriv --reuid=baft-tunnel --regid=baft-tunnel --init-groups timeout 25 $UB apply --config '$REMOTE_DIR/neg.json' --confirm > '$REMOTE_DIR/neg.log' 2>&1 < /dev/null & echo \$! > '$REMOTE_DIR/neg.pid'; sleep 4
$want python3 '$REMOTE_DIR/probe.py' 127.0.0.1 $MALT 4096 >/dev/null 2>&1"
}
# Control: an unchanged copy carries data, so a refusal below is caused by the edit alone.
mvp_neg "pass" ok || { neg_log; echo "control forward with an unchanged config did not carry data" >&2; exit 1; }
mvp_neg "c['peer_fingerprint'] = '0' * 64" refused || { neg_log; echo "a wrong peer pin carried data" >&2; exit 1; }
copy_to_host iran "$MP/mvpctl/node.key" "$MP/issued-mvpctl/node.crt"
ssh_host iran "install -o baft-tunnel -m 0600 '$REMOTE_DIR/node.key' '$REMOTE_DIR/ctl.key'; install -m 0644 '$REMOTE_DIR/node.crt' '$REMOTE_DIR/ctl.crt'; rm -f '$REMOTE_DIR/node.key'"
mvp_neg "c['local_id'] = 'mvpctl'; c['cert'] = '$REMOTE_DIR/ctl.crt'; c['key'] = '$REMOTE_DIR/ctl.key'" refused || { neg_log; echo "a valid certificate of another node carried data" >&2; exit 1; }
ssh_host iran "$NEG_STOP"
mvp_wait 60 >/dev/null || { echo "main tunnel did not carry data after the refusal checks" >&2; exit 1; }

echo "--- MVP revocation: iran certificate revoked, new CRL on the serve host, serve restarted ---"
bash scripts/pki/baft-pki.sh revoke-node "$MP/ca" "$MP/issued-iran/node.crt" "$MP/revoked" >/dev/null
copy_to_host abroad "$MP/revoked/ca.crl"
ssh_host abroad "install -m 0644 '$REMOTE_DIR/ca.crl' $EB/ca.crl"
unit_stop abroad $SRV || { echo "could not stop $SRV" >&2; exit 1; }
[[ "$(mvp_apply abroad $SRV serve mvp-srv-0003 | jf result)" == applied ]] || { echo "serve with the new CRL was not applied" >&2; exit 1; }
sleep 8
if probe_host iran 4096 >/dev/null 2>&1; then echo "revoked forward certificate still carried data" >&2; exit 1; fi
ssh_host abroad "journalctl -u baft-tunnel@$SRV --no-pager -n 200 | grep -qi revok" || { echo "serve log shows no revocation refusal" >&2; exit 1; }
# P1: the revoked forward node attributes the refusal to the secure-channel tier, not to TCP.
p1_api iran POST /v1/diagnostics >"$LOCAL_TMP/p1-diag-revoked.json"
python3 - "$LOCAL_TMP/p1-diag-revoked.json" <<'PY' || { cat "$LOCAL_TMP/p1-diag-revoked.json" >&2; echo "diagnostics did not attribute the revocation to the secure channel" >&2; exit 1; }
import json, sys
d = json.load(open(sys.argv[1])); r = {t["id"]: t["result"] for t in d["tiers"]}
assert r["tcp_reachability"] == "pass" and r["secure_channel"] == "fail" and r["session"] in ("fail", "unknown"), r
assert d["verified"] is False and isinstance(d["verified_unix"], int), d
PY
P1_REVOKED_TIER=secure_channel
LOCAL_PORT="$OLD_LOCAL_PORT"; stop_role abroad echo2

echo "--- MVP uninstall and cleanup proof ---"
mvp_cleanup; trap cleanup EXIT
for side in iran abroad; do
  ssh_host "$side" "! test -e $UB && ! test -e $EB && ! test -e /var/lib/baft-tunnel && ! test -e /run/baft-tunnel/control && ! systemctl list-units --all --plain --no-legend 'baft-tunnel*' | grep -q ." ||
    { echo "uninstall left BAFT-TUNNEL resources on $side" >&2; exit 1; }
  side_snapshot "$side" > "$LOCAL_TMP/mvp-after-$side.txt"
  if ! diff "$LOCAL_TMP/mvp-before-$side.txt" "$LOCAL_TMP/mvp-after-$side.txt" > "$LOCAL_TMP/mvp-diff-$side.txt"; then
    # Printed with addresses masked and capped: the run log is public and the host's firewall is not ours to publish.
    echo "--- host state changed on $side during the MVP stage ($(grep -c '^[<>]' "$LOCAL_TMP/mvp-diff-$side.txt") lines; addresses masked) ---"
    grep '^[<>]' "$LOCAL_TMP/mvp-diff-$side.txt" | sed -E 's/[0-9]{1,3}(\.[0-9]{1,3}){3}(\/[0-9]+)?/<ip>/g; s/[0-9a-fA-F]{0,4}(:[0-9a-fA-F]{0,4}){2,7}(\/[0-9]+)?/<ip6>/g' | head -n 30
    # Listener or unit-file changes, or a firewall change naming this stage's ports or BAFT-TUNNEL, fail the stage.
    # Other firewall drift (host-managed rules the stage never touches) is reported, not hidden.
    if grep -E '^[<>]' "$LOCAL_TMP/mvp-diff-$side.txt" | grep -qiE "baft|(:| |dport |sport )($MSVC|$MTP|$MAG|$MALT)\b" ||
       ! diff <(sed -n '/^# listeners/,$p' "$LOCAL_TMP/mvp-before-$side.txt") <(sed -n '/^# listeners/,$p' "$LOCAL_TMP/mvp-after-$side.txt") >/dev/null; then
      echo "listeners, unit files or BAFT-related firewall rules changed on $side" >&2; exit 1
    fi
    MVP_FW_DRIFT+=("$side")
  fi
done
echo "MVP_RESULT source=${MVP_SOURCE} binary_sha256=${MVP_SHA} clean_hosts=1 installed=1 wrong_checksum_refused=1 setup_join=1 control_helper=1 apply_start_serve=applied apply_start_forward=applied receipt_session=${MVP_SESSION} receipt_ready=${MVP_READY} integrity_64k_2m=1 replay=1 running_refused=1 receipts_secret_free=1 forward_sigkill_recovery_s=${MVP_KILL_S} serve_restart_recovery_s=${MVP_SRV_S} occupied_port_rolled_back=1 wrong_pin_refused=1 wrong_identity_refused=1 revoked_refused=1 p1_evidence=${P1_EVIDENCE:-0} p1_revoked_tier=${P1_REVOKED_TIER:-none} uninstalled=1 listeners_units_unchanged=1 baft_fw_rules=0 unrelated_fw_drift=${MVP_FW_DRIFT[*]:-none}"
