# shellcheck shell=bash
# Automatic pairing acceptance (ADR-0050), sourced by phase1-two-host-e2e.sh when LAB_AUTO_PAIR=1.
# INSTALLED product path on both hosts, driven only through the local agent API in setup mode, exactly as the
# panel does it: agent (service user) -> request file -> root helper -> privilege-dropped pair worker -> pinned
# TLS exchange -> user approval with nonce and matching key fingerprint -> config on both hosts -> Start through
# the agent -> real data through the tunnel. No file is copied between the hosts by this script.
# Round 1 Direct:  abroad = listener/serve (service echo here), iran = dialer/forward (answers "Iran": rotation 10 s).
# Round 2 Reverse: iran = public node/serve (user ingress), abroad = service node/forward (dials out), Preview.
# Each round starts from a clean host and ends with an uninstall; unrelated firewall/listeners are compared.
# The pairing code is a 15-minute single-use secret: it is masked in logs and handed over only through stdin.

echo "=== AUTO PAIRING: installed path via agent API, Direct then Reverse, from ${MVP_SOURCE} ==="
stop_role iran serve; stop_role abroad serve; stop_role iran forward; stop_role abroad forward; sleep 1
AP_SVC=18094; AP_AG=18096; AP_TP="$E2E_PORT"
EB=/etc/baft-tunnel; UB=/usr/local/bin/baft-tunnel; AGD=/var/lib/baft-tunnel-agent/lab-agent; PKG="$REMOTE_DIR/pkg"
AP_SHA="$(sha256sum "$MVP_BINARY_PATH" | awk '{print $1}')"
AP_HOST_iran="$LAB_IRAN_HOST"; AP_HOST_abroad="$LAB_ABROAD_HOST"

ap_snapshot() { # side : normalized firewall rules, listeners outside the test ports, baft-tunnel* unit-file count
  ssh_host "$1" "echo '# firewall'; { nft list ruleset 2>/dev/null | sed -E 's/packets [0-9]+ bytes [0-9]+//'; iptables-save 2>/dev/null | grep -v '^#' | sed -E 's/\\[[0-9]+:[0-9]+\\]//'; };
    echo '# listeners'; ss -ltnH | awk '{print \$4}' | grep -vE ':($AP_SVC|$AP_AG|$AP_TP|$LOCAL_PORT|$ECHO_PORT)\$' | sort -u;
    echo '# units'; systemctl list-unit-files --no-legend 'baft-tunnel*' 2>/dev/null | wc -l"
}
echo "--- clean-host check ---"
AP_USER_BEFORE=(); AP_FW_DRIFT=()
for side in iran abroad; do
  ssh_host "$side" "! test -e $UB && ! test -e $EB && ! test -e /var/lib/baft-tunnel && [ \"\$(systemctl list-unit-files --no-legend 'baft-tunnel*' | wc -l)\" = 0 ]" ||
    { echo "$side is not clean: an earlier BAFT-TUNNEL install is present (binary, $EB, state or units)" >&2; exit 1; }
  if ssh_host "$side" "id baft-tunnel >/dev/null 2>&1"; then AP_USER_BEFORE+=("$side"); fi
  ap_snapshot "$side" > "$LOCAL_TMP/ap-before-$side.txt"
done

ap_purge() { # side instances... : remove only what this stage created on one host
  local side="$1"; shift
  ssh_host "$side" "for i in $*; do touch /run/baft-tunnel/control/\$i.stop 2>/dev/null; systemctl stop baft-tunnel@\$i.service 2>/dev/null; bash '$PKG/scripts/install-agent-control.sh' --remove --instance \$i >/dev/null 2>&1; done;
      [ -s '$REMOTE_DIR/agent.pid' ] && { pkill -P \$(cat '$REMOTE_DIR/agent.pid') 2>/dev/null; kill \$(cat '$REMOTE_DIR/agent.pid') 2>/dev/null; }; rm -f '$REMOTE_DIR/agent.pid';
      [ -s '$REMOTE_DIR/echo2.pid' ] && kill \$(cat '$REMOTE_DIR/echo2.pid') 2>/dev/null; rm -f '$REMOTE_DIR/echo2.pid';
      [ -x '$PKG/scripts/uninstall.sh' ] && bash '$PKG/scripts/uninstall.sh' --stop --purge-config --yes >/dev/null 2>&1;
      rm -rf /var/backups/baft-tunnel-backup.* 2>/dev/null;
      systemctl reset-failed 'baft-tunnel*' 2>/dev/null; systemctl daemon-reload; true" || true
}
ap_users() {
  for side in iran abroad; do
    if [[ " ${AP_USER_BEFORE[*]} " != *" $side "* ]]; then ssh_host "$side" "userdel baft-tunnel-agent 2>/dev/null; groupdel baft-tunnel-agent 2>/dev/null; userdel baft-tunnel 2>/dev/null; groupdel baft-tunnel 2>/dev/null; true" || true; fi
  done
}
AP_INSTANCES=""
ap_cleanup() { for side in iran abroad; do ap_purge "$side" $AP_INSTANCES; done; ap_users; }
# set +e: under errexit the (exit N) that restores the status would end the trap before cleanup runs.
trap 'ap_status=$?; set +e; ap_cleanup; (exit "$ap_status"); cleanup' EXIT

tar -czf "$LOCAL_TMP/ap-pkg.tgz" scripts/*.sh scripts/pki/*.sh systemd
cp "$MVP_BINARY_PATH" "$LOCAL_TMP/baft-tunnel-ap"

ap_api() { # side METHOD path ; JSON body on stdin for POST (kept off the command line)
  local side="$1" method="$2" path="$3"
  ssh_host "$side" "python3 -c 'import sys,urllib.request,urllib.error
tok=open(\"$AGD/token\").read().strip()
body=sys.stdin.buffer.read() if \"$method\"==\"POST\" else None
r=urllib.request.Request(\"http://127.0.0.1:$AP_AG$path\",data=body,method=\"$method\",headers={\"Authorization\":\"Bearer \"+tok,\"Content-Type\":\"application/json\"})
try: print(urllib.request.urlopen(r,timeout=30).read().decode())
except urllib.error.HTTPError as e: print(e.read().decode())'"
}
apjf() { python3 -c 'import sys,json
d=json.load(sys.stdin)
for k in sys.argv[1].split("."): d = d.get(k, "") if isinstance(d, dict) else ""
print(d)' "$1"; }
ap_wait_state() { # side want_state timeout -> prints the status JSON; fails on state "failed" or timeout
  local side="$1" want="$2" limit="$3" t0=$SECONDS out st
  while (( SECONDS - t0 < limit )); do
    out="$(ap_api "$side" GET /v1/pair/status </dev/null)"; st="$(apjf state <<<"$out")"
    if [[ "$st" == "$want" ]]; then printf '%s' "$out"; return 0; fi
    if [[ "$st" == failed ]]; then echo "pairing failed on $side: $(apjf reason <<<"$out")" >&2; return 1; fi
    sleep 1
  done
  echo "timed out waiting for state $want on $side (last state: ${st:-none})" >&2; return 1
}
ap_cfg_check() { # side instance expected_python_assertion
  # the assertion travels on stdin: it contains single quotes that would break a quoted ssh command
  { printf 'import json\nc=json.load(open("%s"))\n' "$EB/$2.json"; printf '%s\n' "${3//;/$'\n'}"; } | ssh_host "$1" "python3 -" &&
    ssh_host "$1" "[ \"\$(stat -c %U $EB/node.key)\" = baft-tunnel ]" </dev/null ||
    { echo "generated configuration on $1 did not match expectations ($3)" >&2; ssh_host "$1" "cat $EB/$2.json | sed -E 's/[0-9]{1,3}(\\.[0-9]{1,3}){3}/<ip>/g'" >&2 || true; return 1; }
}
ap_diag() { # side label : tiers must show a verified, authorized, data-carrying session
  local side="$1" label="$2"
  ap_api "$side" POST /v1/diagnostics </dev/null >"$LOCAL_TMP/ap-diag-$label-$side.json"
  python3 - "$LOCAL_TMP/ap-diag-$label-$side.json" "$side" <<'PY' || { cat "$LOCAL_TMP/ap-diag-$label-$side.json" >&2; echo "diagnostics did not verify the paired tunnel on $side" >&2; return 1; }
import json, sys
d = json.load(open(sys.argv[1])); r = {t["id"]: t["result"] for t in d["tiers"]}
print("tiers[%s]=%s" % (sys.argv[2], json.dumps(r, sort_keys=True)))
for k in ("secure_channel", "identity", "authorization", "session"):
    assert r.get(k) == "pass", (k, r)
assert d["unit"]["result"] == "active", d
PY
}

ap_round() { # name direction listener_side joiner_side joiner_location
  local rn="$1" dir="$2" ls="$3" js="$4" jloc="$5" lloc li ji code out nonce vl vj side inst pair rot n
  li="${rn}l"; ji="${rn}j"; AP_INSTANCES="$li $ji"
  if [[ "$ls" == abroad ]]; then lloc=abroad; else lloc=iran; fi
  echo "--- round $rn ($dir): listener=$ls ($lloc) joiner=$js ($jloc) ---"
  for side in iran abroad; do
    inst="$li"; [[ "$side" == "$js" ]] && inst="$ji"
    copy_to_host "$side" "$LOCAL_TMP/ap-pkg.tgz" "$LOCAL_TMP/baft-tunnel-ap"
    ssh_host "$side" "set -e; mkdir -p '$PKG'; tar -xzf '$REMOTE_DIR/ap-pkg.tgz' -C '$PKG';
      [ \"\$(sha256sum '$REMOTE_DIR/baft-tunnel-ap' | cut -d' ' -f1)\" = '$AP_SHA' ];
      bash '$PKG/scripts/install.sh' --binary '$REMOTE_DIR/baft-tunnel-ap' >/dev/null;
      [ \"\$(sha256sum $UB | cut -d' ' -f1)\" = '$AP_SHA' ];
      bash '$PKG/scripts/install-agent-control.sh' --instance $inst --listen 127.0.0.1:18099 >/dev/null;
      install -d -o baft-tunnel-agent -g baft-tunnel-agent -m 0700 $AGD;
      setpriv --reuid=baft-tunnel-agent --regid=baft-tunnel-agent --init-groups $UB agent-token --out $AGD/token >/dev/null;
      nohup bash -c 'while :; do setpriv --reuid=baft-tunnel-agent --regid=baft-tunnel-agent --init-groups $UB agent --config $EB/$inst.json --token-file $AGD/token --state-dir $AGD --listen 127.0.0.1:$AP_AG --instance $inst; sleep 1; done' > '$REMOTE_DIR/agent.log' 2>&1 < /dev/null & echo \$! > '$REMOTE_DIR/agent.pid';
      for n in \$(seq 1 40); do python3 -c 'import urllib.request;urllib.request.urlopen(\"http://127.0.0.1:$AP_AG/v1/health\",timeout=1)' 2>/dev/null && exit 0; sleep 0.25; done; tail -20 '$REMOTE_DIR/agent.log'; exit 1"
  done
  # setup mode: the agent says setup_pending until pairing wrote the configuration
  for side in iran abroad; do
    inst="$li"; [[ "$side" == "$js" ]] && inst="$ji"
    [[ "$(ap_api "$side" GET /v1/status </dev/null | apjf setup_pending)" == True ]] || { echo "agent on $side is not in setup mode" >&2; return 1; }
  done
  ssh_host abroad "nohup python3 '$REMOTE_DIR/echo.py' '$AP_SVC' > '$REMOTE_DIR/echo2.log' 2>&1 < /dev/null & echo \$! > '$REMOTE_DIR/echo2.pid'"

  local lhost="AP_HOST_$ls"; lhost="${!lhost}"
  local offer; offer="{\"name\":\"labpair-$rn\",\"service_port\":$AP_SVC,\"tunnel_port\":$AP_TP,\"host\":\"$lhost\",\"direction\":\"$dir\",\"confirm\":true}"
  out="$(ap_api "$ls" POST /v1/pair/offer <<<"$offer")"
  [[ "$(apjf state <<<"$out")" == starting ]] || { echo "offer was not accepted: $out" >&2; return 1; }
  out="$(ap_wait_state "$ls" waiting 90)" || return 1
  code="$(apjf code <<<"$out")"
  [[ "$code" == BAFT1-* ]] || { echo "listener did not publish a pairing code" >&2; return 1; }
  echo "::add-mask::$code"
  [[ "$(apjf port <<<"$out")" == "$AP_TP" ]] || { echo "waiting status does not carry the tunnel port" >&2; return 1; }
  # the panel's firewall hint is guidance only; the LAB port is already reachable, nothing is changed here
  out="$(ap_api "$js" POST /v1/pair/accept <<<"{\"code\":\"$code\",\"confirm\":true,\"location\":\"$jloc\"}")"
  [[ "$(apjf state <<<"$out")" == starting ]] || { echo "accept was not taken: $(apjf error <<<"$out")" >&2; return 1; }
  # a second request while one is pending is refused
  [[ "$(ap_api "$js" POST /v1/pair/accept <<<"{\"code\":\"$code\",\"confirm\":true}" | apjf error)" == *pending* ]] || { echo "a second pairing request was not refused" >&2; return 1; }
  out="$(ap_wait_state "$ls" approval 120)" || return 1
  nonce="$(apjf nonce <<<"$out")"; vl="$(apjf verify <<<"$out")"
  [[ -n "$nonce" && -n "$vl" ]] || { echo "approval status has no nonce or fingerprint" >&2; return 1; }
  # the joiner shows the same fingerprint while it waits (user compares the two)
  vj=""; for n in $(seq 1 30); do vj="$(ap_api "$js" GET /v1/pair/status </dev/null | apjf verify)"; [[ -n "$vj" ]] && break; sleep 1; done
  [[ "$vj" == "$vl" ]] || { echo "key fingerprints differ between the two servers: listener=$vl joiner=$vj" >&2; return 1; }
  # approval is bound to this request: a wrong nonce is refused and does not approve
  [[ "$(ap_api "$ls" POST /v1/pair/approve <<<'{"approve":true,"nonce":"wrong"}' | apjf error)" == *"no matching approval"* ]] || { echo "approval with a wrong nonce was not refused" >&2; return 1; }
  [[ "$(ap_api "$ls" GET /v1/pair/status </dev/null | apjf state)" == approval ]] || { echo "wrong-nonce approval changed the state" >&2; return 1; }
  [[ "$(ap_api "$ls" POST /v1/pair/approve <<<"{\"approve\":true,\"nonce\":\"$nonce\"}" | apjf ok)" == True ]] || { echo "approval was not taken" >&2; return 1; }
  ap_wait_state "$ls" done 150 >/dev/null || return 1
  # the joiner agent exits and restarts once the configuration appears (setup mode), so its final state is read
  # from the helper status file rather than through the agent
  local t0=$SECONDS jst=""
  while (( SECONDS - t0 < 150 )); do
    jst="$(ssh_host "$js" "cat /run/baft-tunnel/control/$ji.pair-status 2>/dev/null" </dev/null | apjf state)"
    [[ "$jst" == done || "$jst" == failed ]] && break
    sleep 1
  done
  [[ "$jst" == done ]] || { echo "joiner pairing did not finish (state: ${jst:-none})" >&2; return 1; }
  # both agents reload the new configuration (exit and restart); wait until they answer again as configured
  for side in "$ls" "$js"; do
    local sout=""
    for n in $(seq 1 60); do
      sout="$(ap_api "$side" GET /v1/status </dev/null 2>/dev/null)"
      [[ -n "$sout" && "$(apjf setup_pending <<<"$sout" 2>/dev/null)" != True ]] && break
      sleep 1
    done
  done
  # status files never carry key material
  for side in "$ls" "$js"; do
    ssh_host "$side" "! grep -Eq 'PRIVATE|BEGIN' /run/baft-tunnel/control/*.pair-status 2>/dev/null" || { echo "a pair-status file carries key material" >&2; return 1; }
  done

  if [[ "$dir" == direct ]]; then
    ap_cfg_check "$ls" "$li" "assert c['mode']=='serve' and c['direction']=='direct' and c['engine']=='tcp-tls' and c['target']=='127.0.0.1:$AP_SVC' and 'session_rotate_secs' not in c" || return 1
    if [[ "$jloc" == iran ]]; then rot="assert c.get('session_rotate_secs')==10"; else rot="assert 'session_rotate_secs' not in c"; fi
    ap_cfg_check "$js" "$ji" "assert c['mode']=='forward' and c['direction']=='direct' and c['listen']=='127.0.0.1:$AP_SVC' and c['remote']=='$lhost:$AP_TP';$rot" || return 1
  else
    ap_cfg_check "$ls" "$li" "assert c['mode']=='serve' and c['direction']=='reverse' and c['ingress']=='127.0.0.1:$AP_SVC' and 'target' not in c" || return 1
    ap_cfg_check "$js" "$ji" "assert c['mode']=='forward' and c['direction']=='reverse' and c['target']=='127.0.0.1:$AP_SVC' and 'listen' not in c and 'session_rotate_secs' not in c and c['remote']=='$lhost:$AP_TP'" || return 1
  fi
  # the pair CA key stays on the listener only
  ssh_host "$ls" "test -s $EB/pair-ca/ca.key" || { echo "listener has no pair CA key" >&2; return 1; }
  ssh_host "$js" "! test -e $EB/pair-ca && ! find $EB -name 'ca.key' | grep -q ." || { echo "the joiner holds CA key material" >&2; return 1; }
  ssh_host "$ls" "! ls /run/baft-tunnel/control/ 2>/dev/null | grep -q '\\.pair\$'" || { echo "request file was not consumed" >&2; return 1; }
  # no worker process is left behind
  for side in "$ls" "$js"; do ssh_host "$side" "! pgrep -f 'baft-tunnel pair worker' >/dev/null" || { echo "pair worker still running on $side" >&2; return 1; }; done

  for side in "$ls" "$js"; do
    for n in $(seq 1 30); do [[ "$(ap_api "$side" GET /v1/status </dev/null | apjf setup_pending)" != True ]] && break; sleep 1; done
    [[ "$(ap_api "$side" GET /v1/status </dev/null | apjf setup_pending)" != True ]] || { echo "agent on $side still reports setup pending after pairing" >&2; return 1; }
  done
  echo "--- Start through the agent: listener first, then joiner ---"
  for pair in "$ls $li" "$js $ji"; do
    read -r side inst <<<"$pair"
    out="$(ap_api "$side" POST /v1/tunnel/lifecycle <<<"{\"action\":\"start\",\"confirm\":true,\"idempotency_key\":\"ap-$rn-$inst\"}")"
    ssh_host "$side" "for n in \$(seq 1 40); do systemctl is-active --quiet baft-tunnel@$inst.service && exit 0; sleep 0.5; done; exit 1" ||
      { echo "unit $inst on $side did not start: $out" >&2; ssh_host "$side" "journalctl -u baft-tunnel@$inst -n 30 --no-pager" >&2 || true; return 1; }
    ssh_host "$side" "[ \"\$(ps -o user= -p \$(systemctl show -p MainPID --value baft-tunnel@$inst.service) | tr -d ' ')\" = baft-tunnel ]" || { echo "unit $inst on $side does not run as baft-tunnel" >&2; return 1; }
  done
  local old_local="$LOCAL_PORT"; LOCAL_PORT="$AP_SVC"
  local t0=$SECONDS ok=0
  while (( SECONDS - t0 < 90 )); do probe_host iran 65536 >/dev/null 2>&1 && { ok=1; break; }; sleep 2; done
  (( ok )) || { echo "paired $dir tunnel did not carry data" >&2; LOCAL_PORT="$old_local"; return 1; }
  AP_UP_S="$((SECONDS - t0))"
  probe_host iran 65536; probe_host iran 2097289
  LOCAL_PORT="$old_local"
  if [[ "$dir" == reverse ]]; then
    ssh_host "$js" "! ss -ltnH | awk '{print \$4}' | grep -q ':$AP_TP\$' && ss -tnH state established | awk '{print \$4}' | grep -q ':$AP_TP\$'" ||
      { echo "reverse service node listens on the tunnel port or has no outbound connection" >&2; return 1; }
  fi
  sleep 12   # the agents sample unit counters every 5 s
  ap_diag "$ls" "$rn" || return 1
  ap_diag "$js" "$rn" || return 1
  echo "AUTO_PAIR_ROUND name=$rn direction=$dir listener=$ls joiner=$js joiner_location=$jloc fingerprints_match=1 wrong_nonce_refused=1 second_request_refused=1 configs_ok=1 ca_key_listener_only=1 workers_gone=1 start_via_agent=1 non_root=1 integrity_64k_2m=1 up_s=$AP_UP_S diagnostics_tiers=pass"
  ap_cleanup
  for side in iran abroad; do
    ssh_host "$side" "! test -e $UB && ! test -e $EB && ! test -e /var/lib/baft-tunnel && ! test -e /run/baft-tunnel/control" || { echo "uninstall left BAFT-TUNNEL resources on $side" >&2; return 1; }
  done
  AP_INSTANCES=""
}

ap_round apd direct abroad iran iran || exit 1
ap_round apr reverse iran abroad abroad || exit 1

echo "--- host state compared with the start ---"
for side in iran abroad; do
  ap_snapshot "$side" > "$LOCAL_TMP/ap-after-$side.txt"
  if ! diff "$LOCAL_TMP/ap-before-$side.txt" "$LOCAL_TMP/ap-after-$side.txt" > "$LOCAL_TMP/ap-diff-$side.txt"; then
    echo "host state changed on $side ($(grep -c '^[<>]' "$LOCAL_TMP/ap-diff-$side.txt") lines; addresses masked)"
    grep '^[<>]' "$LOCAL_TMP/ap-diff-$side.txt" | sed -E 's/[0-9]{1,3}(\.[0-9]{1,3}){3}(\/[0-9]+)?/<ip>/g; s/[0-9a-fA-F]{0,4}(:[0-9a-fA-F]{0,4}){2,7}(\/[0-9]+)?/<ip6>/g' | head -n 30
    if grep -E '^[<>]' "$LOCAL_TMP/ap-diff-$side.txt" | grep -qiE "baft|(:| |dport |sport )($AP_SVC|$AP_TP|$AP_AG)\b" ||
       ! diff <(sed -n '/^# listeners/,$p' "$LOCAL_TMP/ap-before-$side.txt") <(sed -n '/^# listeners/,$p' "$LOCAL_TMP/ap-after-$side.txt") >/dev/null; then
      echo "listeners, unit files or BAFT-related firewall rules changed on $side" >&2; exit 1
    fi
    AP_FW_DRIFT+=("$side")
  fi
done
trap cleanup EXIT
echo "AUTO_PAIR_RESULT source=${MVP_SOURCE} binary_sha256=${AP_SHA} rounds=direct,reverse reverse_preview=1 listeners_units_unchanged=1 baft_fw_rules=0 unrelated_fw_drift=${AP_FW_DRIFT[*]:-none} (installed path, agent API only, no file moved between hosts; LAB evidence)"
