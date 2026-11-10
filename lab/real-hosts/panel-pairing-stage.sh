# shellcheck shell=bash
# Panel pairing stage (U4 + U5b, ADR-0026), sourced by phase1-two-host-e2e.sh when LAB_PANEL_PAIRING=1.
# Installed product path on both LAB hosts, and both nodes are set up through the agent API only:
#   clean install with `install.sh --register NAME`, the agent in setup mode, preflight uses the root setup
#   helper, init through POST /v1/setup/apply on each host, own bundle exported with GET /v1/setup/bundle,
#   the peer bundle previewed with POST /v1/setup/bundle/inspect (wrong checksum flagged, wrong-checksum join
#   refused), join through the agent, both agents reload by themselves, Start through the agent,
#   64 KiB + 2 MiB integrity, then a full uninstall. No `baft-tunnel setup` command is run as root by us.
# The runner only carries each public bundle and its checksum from one agent to the other (the operator's
# separate channel). Engine noise-tcp: no PKI files are needed, so the panel path is complete.
# Topology: abroad = serve (owns the service target), iran = forward (owns the ingress).

echo "=== PANEL PAIRING: install --register, setup mode, init/export/inspect/join through the agent API only, Start ==="
stop_role iran serve; stop_role abroad serve; stop_role iran forward; stop_role abroad forward; sleep 1
MSVC=18094; MTP=18330; MAG=18095
EB=/etc/baft-tunnel; UB=/usr/local/bin/baft-tunnel; AGS=/var/lib/baft-tunnel-agent; PKG="$REMOTE_DIR/pkg"
SRV=ppairsrv; CLI=ppaircli
PP_SHA="$(sha256sum "$MVP_BINARY_PATH" | awk '{print $1}')"
pp_inst() { if [[ "$1" == iran ]]; then echo "$CLI"; else echo "$SRV"; fi; }

pp_snapshot() { # side : normalized firewall rules, listeners outside the test ports, baft-tunnel* unit-file count
  ssh_host "$1" "echo '# firewall'; { nft list ruleset 2>/dev/null | sed -E 's/packets [0-9]+ bytes [0-9]+//'; iptables-save 2>/dev/null | grep -v '^#' | sed -E 's/\\[[0-9]+:[0-9]+\\]//'; };
    echo '# listeners'; ss -ltnH | awk '{print \$4}' | grep -vE ':($MSVC|$MTP|$MAG|$E2E_PORT|$LOCAL_PORT|$ECHO_PORT)\$' | sort -u;
    echo '# units'; systemctl list-unit-files --no-legend 'baft-tunnel*' 2>/dev/null | wc -l"
}
echo "--- clean-host check ---"
PP_USERS_BEFORE=(); PP_DRIFT=()
for side in iran abroad; do
  ssh_host "$side" "! test -e $UB && ! test -e $EB && ! test -e /var/lib/baft-tunnel && ! test -e $AGS && [ \"\$(systemctl list-unit-files --no-legend 'baft-tunnel*' | wc -l)\" = 0 ]" ||
    { echo "$side is not clean: an earlier BAFT-TUNNEL install is present (binary, $EB, state or units)" >&2; exit 1; }
  ssh_host "$side" "systemctl --version | head -1; . /etc/os-release; echo \$PRETTY_NAME"
  for u in baft-tunnel baft-tunnel-agent; do
    if ssh_host "$side" "id $u >/dev/null 2>&1"; then PP_USERS_BEFORE+=("$side:$u"); fi
  done
  pp_snapshot "$side" > "$LOCAL_TMP/pp-before-$side.txt"
done

pp_cleanup() {
  for side in iran abroad; do
    inst="$(pp_inst "$side")"
    ssh_host "$side" "[ -d '$PKG/scripts' ] || exit 0;
      systemctl disable --now baft-tunnel@$inst.service 2>/dev/null;
      bash '$PKG/scripts/install-agent-control.sh' --remove --instance $inst >/dev/null 2>&1;
      bash '$PKG/scripts/uninstall.sh' --stop --purge-config --yes >/dev/null 2>&1;
      systemctl reset-failed 'baft-tunnel*' 2>/dev/null; systemctl daemon-reload; true" || true
  done
  for side in iran abroad; do
    for u in baft-tunnel-agent baft-tunnel; do
      if [[ " ${PP_USERS_BEFORE[*]} " != *" $side:$u "* ]]; then ssh_host "$side" "userdel $u 2>/dev/null; groupdel $u 2>/dev/null; true" || true; fi
    done
  done
}
# set +e: under errexit the (exit N) that restores the status would end the trap before cleanup runs.
trap 'pp_status=$?; set +e; pp_cleanup; (exit "$pp_status"); cleanup' EXIT

echo "--- one-step install with --register (${MVP_SOURCE}, sha256 ${PP_SHA}); no configuration yet ---"
tar -czf "$LOCAL_TMP/pp-pkg.tgz" scripts/*.sh scripts/pki/*.sh systemd
cp "$MVP_BINARY_PATH" "$LOCAL_TMP/baft-tunnel-pp"
for side in iran abroad; do
  inst="$(pp_inst "$side")"
  copy_to_host "$side" "$LOCAL_TMP/pp-pkg.tgz" "$LOCAL_TMP/baft-tunnel-pp"
  ssh_host "$side" "set -e; mkdir -p '$PKG'; tar -xzf '$REMOTE_DIR/pp-pkg.tgz' -C '$PKG';
    [ \"\$(sha256sum '$REMOTE_DIR/baft-tunnel-pp' | cut -d' ' -f1)\" = '$PP_SHA' ];
    bash '$PKG/scripts/install.sh' --binary '$REMOTE_DIR/baft-tunnel-pp' --register $inst --agent-listen 127.0.0.1:$MAG >/dev/null;
    [ \"\$(sha256sum $UB | cut -d' ' -f1)\" = '$PP_SHA' ];
    systemctl is-enabled --quiet baft-tunnel-control-setup@$inst.path; systemctl is-active --quiet baft-tunnel-control-setup@$inst.path;
    ! test -e $EB/$inst.json; ! systemctl is-active --quiet baft-tunnel@$inst.service" ||
    { echo "install --register on $side did not leave the expected state (setup helper path unit, no config, tunnel not started)" >&2; exit 1; }
done

pp_api() { # side METHOD path [local-json-file] -> response JSON on stdout (token read as root on the host, never printed)
  local body=""
  if [[ -n "${4:-}" ]]; then copy_to_host "$1" "$4"; body="$REMOTE_DIR/$(basename "$4")"; fi
  ssh_host "$1" "python3 - <<'PY'
import urllib.request, os
tok = open('$AGS/token').read().strip()
path = '$body'
data = open(path, 'rb').read() if path else (b'' if '$2' == 'POST' else None)
if path: os.remove(path)
r = urllib.request.Request('http://127.0.0.1:$MAG$3', data=data, method='$2', headers={'Authorization': 'Bearer ' + tok, 'Content-Type': 'application/json'})
try:
    print(urllib.request.urlopen(r, timeout=120).read().decode())
except urllib.error.HTTPError as e:
    print(e.read().decode())
PY"
}
jf() { python3 -c 'import sys,json; d=json.load(sys.stdin)
for k in sys.argv[1].split("."): d = d.get(k, "") if isinstance(d, dict) else ""
print(d)' "$1"; }
pp_fail() { echo "$1" >&2; ssh_host "$2" "journalctl -u baft-tunnel-agent@$(pp_inst "$2") -u baft-tunnel-control-setup@$(pp_inst "$2") -n 40 --no-pager" >&2 || true; exit 1; }

echo "--- both agents run in setup mode before any configuration ---"
for side in iran abroad; do
  ok=0
  for _ in $(seq 1 60); do
    pp_api "$side" GET /v1/status > "$LOCAL_TMP/pp-st0-$side.json" 2>/dev/null &&
      [[ "$(jf setup_pending < "$LOCAL_TMP/pp-st0-$side.json" 2>/dev/null)" == True ]] && { ok=1; break; }
    sleep 1
  done
  (( ok == 1 )) || pp_fail "agent on $side is not in setup mode after install" "$side"
  [[ "$(jf setup_helper.available < "$LOCAL_TMP/pp-st0-$side.json")" == True ]] || { cat "$LOCAL_TMP/pp-st0-$side.json" >&2; pp_fail "setup helper not available to the agent on $side" "$side"; }
  ssh_host "$side" "pid=\$(systemctl show -p MainPID --value baft-tunnel-agent@$(pp_inst "$side").service); [ \"\$(ps -o user= -p \$pid)\" = baft-tunnel-agent ]" ||
    pp_fail "agent on $side does not run as baft-tunnel-agent" "$side"
done

pp_plan() { # side -> plan JSON
  if [[ "$1" == abroad ]]; then
    printf '{"engine":"noise-tcp","role":"serve","direction":"direct","node_id":"abroad","peer_id":"iran","tunnel_id":"ppair","service_port":%s,"tunnel_port":%s,"dir":"%s","instance":"%s"}' "$MSVC" "$MTP" "$EB" "$SRV"
  else
    printf '{"engine":"noise-tcp","role":"forward","direction":"direct","node_id":"iran","peer_id":"abroad","tunnel_id":"ppair","service_port":%s,"tunnel_port":%s,"peer_host":"%s","dir":"%s","instance":"%s"}' "$MSVC" "$MTP" "$LAB_ABROAD_HOST" "$EB" "$CLI"
  fi
}
pp_body() { # file stage key side [bundle-file checksum]
  python3 - "$@" <<'PY'
import json, sys
out, stage, key, plan = sys.argv[1], sys.argv[2], sys.argv[3], json.loads(sys.argv[4])
d = {"stage": stage, "confirm": True, "idempotency_key": key, "plan": plan}
if len(sys.argv) > 5:
    d["bundle"] = open(sys.argv[5]).read(); d["checksum"] = sys.argv[6]
json.dump(d, open(out, "w"))
PY
}

echo "--- preflight through the agent uses the root setup helper ---"
for side in iran abroad; do
  pp_plan "$side" > "$LOCAL_TMP/pp-plan-$side.json"
  pp_api "$side" POST /v1/setup/preflight "$LOCAL_TMP/pp-plan-$side.json" > "$LOCAL_TMP/pp-pf-$side.json"
  python3 -c 'import json,sys; c={x["id"]:x["result"] for x in json.load(open(sys.argv[1]))["checks"]}
assert c.get("setup_helper") == "pass" and "directory_writable" not in c, c' "$LOCAL_TMP/pp-pf-$side.json" ||
    { cat "$LOCAL_TMP/pp-pf-$side.json" >&2; pp_fail "preflight on $side does not use the setup helper" "$side"; }
done

echo "--- init on both hosts through the agent API (root setup helper), export own bundles ---"
declare -A PP_CK
for side in abroad iran; do
  pp_body "$LOCAL_TMP/pp-init-$side.json" init "pp-init-$side-0001" "$(pp_plan "$side")"
  pp_api "$side" POST /v1/setup/apply "$LOCAL_TMP/pp-init-$side.json" > "$LOCAL_TMP/pp-init-out-$side.json"
  [[ "$(jf result < "$LOCAL_TMP/pp-init-out-$side.json")" == applied && "$(jf source < "$LOCAL_TMP/pp-init-out-$side.json")" == "root setup helper" ]] ||
    { cat "$LOCAL_TMP/pp-init-out-$side.json" >&2; pp_fail "init through the agent on $side was not applied by the root helper" "$side"; }
  ssh_host "$side" "[ \"\$(stat -c '%U:%G %a' $EB/node.noise)\" = 'baft-tunnel:baft-tunnel 600' ] && ! runuser -u baft-tunnel-agent -- cat $EB/node.noise >/dev/null 2>&1" ||
    pp_fail "node key on $side has the wrong owner/mode or is readable by the agent user" "$side"
  pp_api "$side" GET /v1/setup/bundle > "$LOCAL_TMP/pp-own-$side.json"
  PP_CK[$side]="$(jf checksum < "$LOCAL_TMP/pp-own-$side.json")"
  [[ "${PP_CK[$side]}" =~ ^[0-9a-f]{8}(-[0-9a-f]{8}){3}$ && "${PP_CK[$side]}" == "$(jf pairing.checksum < "$LOCAL_TMP/pp-init-out-$side.json")" ]] ||
    pp_fail "exported checksum on $side is missing or differs from the init receipt" "$side"
  python3 -c 'import json,sys; open(sys.argv[2],"w").write(json.load(open(sys.argv[1]))["bundle"])' "$LOCAL_TMP/pp-own-$side.json" "$LOCAL_TMP/pp-bundle-$side.json"
  copy_to_host "$side" "$LOCAL_TMP/pp-bundle-$side.json" &&
    ssh_host "$side" "k=\$(tr -d '[:space:]' < $EB/node.noise); r=1; [ \${#k} -ge 16 ] && ! grep -qF -- \"\$k\" '$REMOTE_DIR/pp-bundle-$side.json' && r=0; rm -f '$REMOTE_DIR/pp-bundle-$side.json'; exit \$r" ||
    pp_fail "the exported bundle on $side contains key material" "$side"
done

echo "--- import preview, wrong checksum refused, join through the agent on both hosts ---"
PP_BAD=00000000-00000000-00000000-00000000
for side in abroad iran; do
  peer=iran; [[ "$side" == iran ]] && peer=abroad
  python3 -c 'import json,sys; json.dump({"bundle": open(sys.argv[1]).read(), "checksum": sys.argv[2]}, open(sys.argv[3],"w"))' "$LOCAL_TMP/pp-bundle-$peer.json" "$PP_BAD" "$LOCAL_TMP/pp-insp-bad-$side.json"
  [[ "$(pp_api "$side" POST /v1/setup/bundle/inspect "$LOCAL_TMP/pp-insp-bad-$side.json" | jf checksum_match)" == False ]] || pp_fail "a wrong checksum was not flagged on $side" "$side"
  python3 -c 'import json,sys; json.dump({"bundle": open(sys.argv[1]).read(), "checksum": sys.argv[2]}, open(sys.argv[3],"w"))' "$LOCAL_TMP/pp-bundle-$peer.json" "${PP_CK[$peer]}" "$LOCAL_TMP/pp-insp-$side.json"
  pp_api "$side" POST /v1/setup/bundle/inspect "$LOCAL_TMP/pp-insp-$side.json" > "$LOCAL_TMP/pp-insp-out-$side.json"
  [[ "$(jf checksum_match < "$LOCAL_TMP/pp-insp-out-$side.json")" == True && "$(jf matches_this_node < "$LOCAL_TMP/pp-insp-out-$side.json")" == True ]] ||
    { cat "$LOCAL_TMP/pp-insp-out-$side.json" >&2; pp_fail "import preview on $side rejected the right peer bundle" "$side"; }
  pp_body "$LOCAL_TMP/pp-join-bad-$side.json" join "pp-join-$side-0000" "$(pp_plan "$side")" "$LOCAL_TMP/pp-bundle-$peer.json" "$PP_BAD"
  [[ "$(pp_api "$side" POST /v1/setup/apply "$LOCAL_TMP/pp-join-bad-$side.json" | jf result)" != applied ]] || pp_fail "join with a wrong checksum was applied on $side" "$side"
  ssh_host "$side" "! test -e $EB/$(pp_inst "$side").json" || pp_fail "a refused join wrote the configuration on $side" "$side"
  pp_body "$LOCAL_TMP/pp-join-$side.json" join "pp-join-$side-0001" "$(pp_plan "$side")" "$LOCAL_TMP/pp-bundle-$peer.json" "${PP_CK[$peer]}"
  pp_api "$side" POST /v1/setup/apply "$LOCAL_TMP/pp-join-$side.json" > "$LOCAL_TMP/pp-join-out-$side.json"
  [[ "$(jf result < "$LOCAL_TMP/pp-join-out-$side.json")" == applied ]] || { cat "$LOCAL_TMP/pp-join-out-$side.json" >&2; pp_fail "join through the agent on $side was not applied" "$side"; }
  ssh_host "$side" "[ \"\$(stat -c '%U:%G %a' $EB/$(pp_inst "$side").json)\" = 'root:baft-tunnel 640' ]" || pp_fail "instance config on $side has the wrong owner/mode" "$side"
done

echo "--- both agents leave setup mode by themselves ---"
for side in iran abroad; do
  ok=0
  for _ in $(seq 1 60); do
    s="$(pp_api "$side" GET /v1/status 2>/dev/null || true)"
    [[ -n "$s" ]] && ! grep -q '"setup_pending"' <<<"$s" && grep -q '"tunnel"' <<<"$s" && { ok=1; break; }
    sleep 1
  done
  (( ok == 1 )) || pp_fail "agent on $side did not reload with the configuration" "$side"
done

echo "--- Start through the agent: serve, then forward; data both sizes ---"
ssh_host abroad "nohup python3 '$REMOTE_DIR/echo.py' '$MSVC' > '$REMOTE_DIR/echo2.log' 2>&1 < /dev/null & echo \$! > '$REMOTE_DIR/echo2.pid'"
OLD_LOCAL_PORT="$LOCAL_PORT"; LOCAL_PORT="$MSVC"
for side in abroad iran; do
  printf '{"action":"start","confirm":true,"idempotency_key":"pp-start-%s-0001"}' "$side" > "$LOCAL_TMP/pp-start-$side.json"
  pp_api "$side" POST /v1/tunnel/lifecycle "$LOCAL_TMP/pp-start-$side.json" > "$LOCAL_TMP/pp-start-out-$side.json"
  [[ "$(jf result < "$LOCAL_TMP/pp-start-out-$side.json")" == applied ]] || { cat "$LOCAL_TMP/pp-start-out-$side.json" >&2; pp_fail "Start on $side after panel pairing was not applied" "$side"; }
done
t0=$SECONDS; PP_DATA_S=""
while (( SECONDS - t0 < 60 )); do probe_host iran 65536 >/dev/null 2>&1 && { PP_DATA_S=$((SECONDS - t0)); break; }; sleep 2; done
[[ -n "$PP_DATA_S" ]] || { ssh_host iran "journalctl -u baft-tunnel@$CLI -n 40 --no-pager" >&2 || true; echo "no data through the panel-paired tunnel" >&2; exit 1; }
probe_host iran 65536; probe_host iran 2097289

echo "--- audit, helper receipts, no secrets in agent state ---"
for side in iran abroad; do
  inst="$(pp_inst "$side")"
  ssh_host "$side" "python3 - $AGS/$inst/audit.log <<'PY'
import json, sys
res = [json.loads(l)['resource'] for l in open(sys.argv[1])]
assert 'setup/init' in res and 'setup/join' in res, res
PY
    test -f /var/lib/baft-tunnel/setup-receipts/$inst/pp-join-$side-0001.json &&
    ! grep -rqs -- \"\$(cat $AGS/token)\" $AGS/$inst /var/lib/baft-tunnel/setup-receipts && ! grep -rqs 'PRIVATE KEY' $AGS/$inst" ||
    pp_fail "audit, helper receipt or secret check failed on $side" "$side"
done
LOCAL_PORT="$OLD_LOCAL_PORT"; stop_role abroad echo2

echo "--- uninstall and cleanup proof ---"
pp_cleanup; trap cleanup EXIT
for side in iran abroad; do
  ssh_host "$side" "! test -e $UB && ! test -e $EB && ! test -e /var/lib/baft-tunnel && ! test -e $AGS && ! test -e /run/baft-tunnel/control &&
    ! ls /etc/systemd/system/multi-user.target.wants/ | grep -q '^baft-tunnel' && ! systemctl list-units --all --plain --no-legend 'baft-tunnel*' | grep -q . &&
    ! ss -ltnH | awk '{print \$4}' | grep -qE ':($MSVC|$MTP|$MAG)\$'" ||
    { echo "uninstall left BAFT-TUNNEL resources on $side" >&2; exit 1; }
  for u in baft-tunnel-agent baft-tunnel; do
    if [[ " ${PP_USERS_BEFORE[*]} " != *" $side:$u "* ]]; then ssh_host "$side" "! id $u >/dev/null 2>&1" || { echo "user $u was left on $side" >&2; exit 1; }; fi
  done
  pp_snapshot "$side" > "$LOCAL_TMP/pp-after-$side.txt"
  if ! diff "$LOCAL_TMP/pp-before-$side.txt" "$LOCAL_TMP/pp-after-$side.txt" > "$LOCAL_TMP/pp-diff-$side.txt"; then
    # Printed with addresses masked and capped: the run log is public and the host's firewall is not ours to publish.
    echo "--- host state changed on $side during the stage ($(grep -c '^[<>]' "$LOCAL_TMP/pp-diff-$side.txt") lines; addresses masked) ---"
    grep '^[<>]' "$LOCAL_TMP/pp-diff-$side.txt" | sed -E 's/[0-9]{1,3}(\.[0-9]{1,3}){3}(\/[0-9]+)?/<ip>/g; s/[0-9a-fA-F]{0,4}(:[0-9a-fA-F]{0,4}){2,7}(\/[0-9]+)?/<ip6>/g' | head -n 30
    grep -E '^[<>]' "$LOCAL_TMP/pp-diff-$side.txt" | grep -qiE "baft|(:| |dport |sport )($MSVC|$MTP|$MAG)\b" &&
      { echo "BAFT-related firewall rules or units changed on $side" >&2; exit 1; }
    ! diff <(sed -n '/^# units/,$p' "$LOCAL_TMP/pp-before-$side.txt") <(sed -n '/^# units/,$p' "$LOCAL_TMP/pp-after-$side.txt") >/dev/null &&
      { echo "baft-tunnel unit files changed on $side" >&2; exit 1; }
    ! diff <(sed -n '/^# listeners/,/^# units/p' "$LOCAL_TMP/pp-before-$side.txt") <(sed -n '/^# listeners/,/^# units/p' "$LOCAL_TMP/pp-after-$side.txt") >/dev/null &&
      { echo "listeners changed on $side" >&2; exit 1; }
    PP_DRIFT+=("$side")
  fi
done
echo "PANEL_PAIRING_RESULT source=${MVP_SOURCE} binary_sha256=${PP_SHA} clean_hosts=1 setup_mode_after_install=1 agent_user=baft-tunnel-agent preflight_setup_helper=1 init_via_agent=2 key_not_agent_readable=1 export_checksum=1 bundle_key_free=1 wrong_checksum_flagged=1 wrong_checksum_join_refused=1 join_via_agent=2 agents_reloaded=2 root_setup_commands_by_us=0 start_via_agent=2 data_s=${PP_DATA_S} integrity_64k_2m=1 audit_setup=1 helper_receipts=1 state_secret_free=1 uninstalled=1 users_removed=1 baft_fw_rules=0 host_drift=${PP_DRIFT[*]:-none}"
