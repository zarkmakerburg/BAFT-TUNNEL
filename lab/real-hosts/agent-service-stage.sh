# shellcheck shell=bash
# Agent service and tunnel lifecycle stage (U2/U3/U5a, ADR-0023 Amendment 1, SA-09), sourced by
# phase1-two-host-e2e.sh when LAB_AGENT_SVC=1. Installed product path on both LAB hosts:
#   clean install with `install.sh --binary ... --register NAME` (one step), guided setup as root with no
#   manual chown, the agent service baft-tunnel-agent@NAME starting by itself under its own user,
#   Start / Restart / Stop / Start through POST /v1/tunnel/lifecycle (enable --now / disable --now),
#   measured 64 KiB + 2 MiB integrity, diagnostics next_action, and with LAB_REBOOT=1 one real reboot of
#   both hosts: the agent and the tunnel must come back with no command from us. Then a full uninstall.
# Only resources this stage creates are removed; listeners, unit files and BAFT-related firewall rules are compared.
# Topology: abroad = serve (owns the service target), iran = forward (owns the ingress).

echo "=== AGENT SERVICE: install --register, setup, lifecycle Start/Restart/Stop via the agent service, reboot=${AGENT_REBOOT} ==="
stop_role iran serve; stop_role abroad serve; stop_role iran forward; stop_role abroad forward; sleep 1
MSVC=18094; MTP=18330; MAG=18095
EB=/etc/baft-tunnel; UB=/usr/local/bin/baft-tunnel; AGS=/var/lib/baft-tunnel-agent; PKG="$REMOTE_DIR/pkg"
SRV=asvcsrv; CLI=asvccli
AS_SHA="$(sha256sum "$MVP_BINARY_PATH" | awk '{print $1}')"
AS_MARK="/var/tmp/baft-tunnel-lab-reboot-${GITHUB_RUN_ID:-$$}"
as_inst() { if [[ "$1" == iran ]]; then echo "$CLI"; else echo "$SRV"; fi; }

side_snapshot() { # side : normalized firewall rules, listeners outside the test ports, baft-tunnel* unit-file count
  ssh_host "$1" "echo '# firewall'; { nft list ruleset 2>/dev/null | sed -E 's/packets [0-9]+ bytes [0-9]+//'; iptables-save 2>/dev/null | grep -v '^#' | sed -E 's/\\[[0-9]+:[0-9]+\\]//'; };
    echo '# listeners'; ss -ltnH | awk '{print \$4}' | grep -vE ':($MSVC|$MTP|$MAG|$E2E_PORT|$LOCAL_PORT|$ECHO_PORT)\$' | sort -u;
    echo '# units'; systemctl list-unit-files --no-legend 'baft-tunnel*' 2>/dev/null | wc -l"
}
echo "--- clean-host check ---"
AS_USERS_BEFORE=(); AS_DRIFT=()
for side in iran abroad; do
  ssh_host "$side" "! test -e $UB && ! test -e $EB && ! test -e /var/lib/baft-tunnel && ! test -e $AGS && [ \"\$(systemctl list-unit-files --no-legend 'baft-tunnel*' | wc -l)\" = 0 ]" ||
    { echo "$side is not clean: an earlier BAFT-TUNNEL install is present (binary, $EB, state or units)" >&2; exit 1; }
  ssh_host "$side" "systemctl --version | head -1; . /etc/os-release; echo \$PRETTY_NAME"
  for u in baft-tunnel baft-tunnel-agent; do
    if ssh_host "$side" "id $u >/dev/null 2>&1"; then AS_USERS_BEFORE+=("$side:$u"); fi
  done
  side_snapshot "$side" > "$LOCAL_TMP/as-before-$side.txt"
done

as_cleanup() {
  for side in iran abroad; do
    inst="$(as_inst "$side")"
    ssh_host "$side" "rm -f '$AS_MARK';
      [ -d '$PKG/scripts' ] || exit 0;
      systemctl disable --now baft-tunnel@$inst.service 2>/dev/null;
      bash '$PKG/scripts/install-agent-control.sh' --remove --instance $inst >/dev/null 2>&1;
      bash '$PKG/scripts/uninstall.sh' --stop --purge-config --yes >/dev/null 2>&1;
      systemctl reset-failed 'baft-tunnel*' 2>/dev/null; systemctl daemon-reload; true" || true
    # The package may be gone after a reboot (/tmp is cleared): never leave an install behind for that reason.
    ssh_host "$side" "test -e $UB || test -e $EB || test -e $AGS" 2>/dev/null && {
      echo "cleanup on $side needs the package again (after a reboot); copying it" >&2
      ssh_host "$side" "mkdir -p '$PKG'" && copy_to_host "$side" "$LOCAL_TMP/as-pkg.tgz" &&
        ssh_host "$side" "tar -xzf '$REMOTE_DIR/as-pkg.tgz' -C '$PKG';
          systemctl disable --now baft-tunnel@$inst.service 2>/dev/null;
          bash '$PKG/scripts/install-agent-control.sh' --remove --instance $inst >/dev/null 2>&1;
          bash '$PKG/scripts/uninstall.sh' --stop --purge-config --yes >/dev/null 2>&1;
          systemctl reset-failed 'baft-tunnel*' 2>/dev/null; systemctl daemon-reload; true" || true
    }
  done
  for side in iran abroad; do
    for u in baft-tunnel-agent baft-tunnel; do
      if [[ " ${AS_USERS_BEFORE[*]} " != *" $side:$u "* ]]; then ssh_host "$side" "userdel $u 2>/dev/null; groupdel $u 2>/dev/null; true" || true; fi
    done
  done
}
# set +e: under errexit the (exit N) that restores the status would end the trap before cleanup runs.
trap 'as_status=$?; set +e; as_cleanup; (exit "$as_status"); cleanup' EXIT

echo "--- one-step install with --register (${MVP_SOURCE}, sha256 ${AS_SHA}) ---"
tar -czf "$LOCAL_TMP/as-pkg.tgz" scripts/*.sh scripts/pki/*.sh systemd
cp "$MVP_BINARY_PATH" "$LOCAL_TMP/baft-tunnel-as"
for side in iran abroad; do
  inst="$(as_inst "$side")"
  copy_to_host "$side" "$LOCAL_TMP/as-pkg.tgz" "$LOCAL_TMP/baft-tunnel-as"
  ssh_host "$side" "set -e; mkdir -p '$PKG'; tar -xzf '$REMOTE_DIR/as-pkg.tgz' -C '$PKG';
    [ \"\$(sha256sum '$REMOTE_DIR/baft-tunnel-as' | cut -d' ' -f1)\" = '$AS_SHA' ];
    bash '$PKG/scripts/install.sh' --binary '$REMOTE_DIR/baft-tunnel-as' --register $inst --agent-listen 127.0.0.1:$MAG >/dev/null;
    [ \"\$(sha256sum $UB | cut -d' ' -f1)\" = '$AS_SHA' ];
    [ \"\$(stat -c '%U %a' $AGS/token)\" = 'baft-tunnel-agent 600' ];
    systemctl is-enabled --quiet baft-tunnel-agent@$inst.path; systemctl is-active --quiet baft-tunnel-agent@$inst.path;
    ! systemctl is-active --quiet baft-tunnel-agent@$inst.service;
    ! systemctl is-active --quiet baft-tunnel@$inst.service; ! systemctl is-enabled --quiet baft-tunnel@$inst.service" ||
    { echo "install --register on $side did not leave the expected state (token, agent path unit, tunnel not started)" >&2; exit 1; }
done

echo "--- offline PKI (runner plays the offline CA) ---"
AP="$LOCAL_TMP/aspki"; mkdir -p "$AP"; (umask 077; printf 'lab-ephemeral-as-ca-%s\n' "$RANDOM$RANDOM$RANDOM" > "$AP/pass")
export BAFT_PKI_PASSFILE="$AP/pass"
bash scripts/pki/baft-pki.sh init-ca "$AP/ca" >/dev/null 2>&1
as_issue() { # node host_ip
  bash scripts/pki/baft-pki.sh new-node "$1" "$AP/$1" >/dev/null
  bash scripts/pki/baft-pki.sh issue-node "$AP/ca" "$1" "$AP/$1/node.csr" "$AP/issued-$1" "$2" 30 >/dev/null
  bash scripts/pki/baft-pki.sh verify-node "$AP/ca" "$AP/issued-$1/ca.crl" "$AP/issued-$1/node.crt" "$1" "$2" >/dev/null
}
as_issue abroad "$LAB_ABROAD_HOST"; as_issue iran "$LAB_IRAN_HOST"
cp "$AP/issued-iran/ca.crl" "$AP/ca.crl"
for node in abroad iran; do
  cp "$AP/$node/node.key" "$AP/as-$node-node.key"; cp "$AP/issued-$node/node.crt" "$AP/as-$node-node.crt"
  cp "$AP/issued-$node/ca.crt" "$AP/as-ca.crt"; cp "$AP/ca.crl" "$AP/as-ca.crl"
  copy_to_host "$node" "$AP/as-$node-node.key" "$AP/as-$node-node.crt" "$AP/as-ca.crt" "$AP/as-ca.crl"
  ssh_host "$node" "set -e; install -o root -g root -m 0600 '$REMOTE_DIR/as-$node-node.key' $EB/node.key; install -m 0644 '$REMOTE_DIR/as-$node-node.crt' $EB/node.crt;
    install -m 0644 '$REMOTE_DIR/as-ca.crt' $EB/ca.crt; install -m 0644 '$REMOTE_DIR/as-ca.crl' $EB/ca.crl; rm -f '$REMOTE_DIR/as-$node-node.key'"
done

echo "--- guided setup as root, no manual chown: init, bundle exchange, join ---"
acommon="--engine tcp-tls --direction direct --tunnel-id asvc --service-port $MSVC --tunnel-port $MTP --dir $EB"
sum_a="$(ssh_host abroad "$UB setup init $acommon --role serve --node-id abroad --peer-id iran --instance $SRV" | awk '/^Checksum/{print $NF}')"
sum_i="$(ssh_host iran "$UB setup init $acommon --role forward --node-id iran --peer-id abroad --peer-host '$LAB_ABROAD_HOST' --instance $CLI" | awk '/^Checksum/{print $NF}')"
[[ "$sum_a" =~ ^[0-9a-f-]{32,}$ && "$sum_i" =~ ^[0-9a-f-]{32,}$ ]] || { echo "setup init printed no checksum" >&2; exit 1; }
ssh_host abroad "cat $EB/bundle.json" > "$AP/bundle-abroad.json"
ssh_host iran "cat $EB/bundle.json" > "$AP/bundle-iran.json"
copy_to_host iran "$AP/bundle-abroad.json"; copy_to_host abroad "$AP/bundle-iran.json"
ssh_host abroad "$UB setup join --dir $EB --bundle '$REMOTE_DIR/bundle-iran.json' --checksum '$sum_i' >/dev/null"
ssh_host iran "$UB setup join --dir $EB --bundle '$REMOTE_DIR/bundle-abroad.json' --checksum '$sum_a' >/dev/null"
for side in iran abroad; do
  inst="$(as_inst "$side")"
  # setup adopts the files for the service (src/setup.rs adopt_for_service): key owned by the tunnel user, config group-readable.
  ssh_host "$side" "[ \"\$(stat -c '%U %a' $EB/node.key)\" = 'baft-tunnel 600' ] && [ \"\$(stat -c '%G' $EB/$inst.json)\" = baft-tunnel ]" ||
    { ssh_host "$side" "stat -c '%n %U:%G %a' $EB/node.key $EB/$inst.json" >&2 || true; echo "setup on $side did not adopt the key and config for the service user" >&2; exit 1; }
done

echo "--- agent service starts by itself once the config exists (own user, loopback) ---"
as_agent_up() { # side inst : agent service active, healthy on MAG, running as baft-tunnel-agent
  ssh_host "$1" "for n in \$(seq 1 60); do systemctl is-active --quiet baft-tunnel-agent@$2.service && python3 -c 'import urllib.request;urllib.request.urlopen(\"http://127.0.0.1:$MAG/v1/health\",timeout=1)' 2>/dev/null && break; sleep 1; done;
    systemctl is-active --quiet baft-tunnel-agent@$2.service || { journalctl -u baft-tunnel-agent@$2 -n 30 --no-pager; exit 1; };
    pid=\$(systemctl show -p MainPID --value baft-tunnel-agent@$2.service); [ \"\$(ps -o user= -p \$pid)\" = baft-tunnel-agent ];
    ss -ltnH \"sport = :$MAG\" | awk '{print \$4}' | grep -qx '127.0.0.1:$MAG'"
}
for side in iran abroad; do
  as_agent_up "$side" "$(as_inst "$side")" || { echo "agent service on $side did not start by itself as baft-tunnel-agent on loopback" >&2; exit 1; }
done
AS_AGENT_SELF_START=1

as_api() { # side METHOD path [json-body] -> response JSON on stdout (token read as root, never printed)
  ssh_host "$1" "python3 - <<'PY'
import json, urllib.request
tok = open('$AGS/token').read().strip()
body = '''${4:-}'''.encode() if '''${4:-}''' else (b'' if '$2' == 'POST' else None)
r = urllib.request.Request('http://127.0.0.1:$MAG$3', data=body, method='$2', headers={'Authorization': 'Bearer ' + tok, 'Content-Type': 'application/json'})
try:
    print(urllib.request.urlopen(r, timeout=120).read().decode())
except urllib.error.HTTPError as e:
    print(e.read().decode())
PY"
}
as_life() { # side action key -> receipt JSON
  as_api "$1" POST /v1/tunnel/lifecycle "{\"action\": \"$2\", \"confirm\": true, \"idempotency_key\": \"$3\"}"
}
jf() { python3 -c 'import sys,json; d=json.load(sys.stdin)
for k in sys.argv[1].split("."): d = d.get(k, "") if isinstance(d, dict) else ""
print(d)' "$1"; }
as_expect() { # side action key want-result -> receipt JSON in $AS_OUT
  AS_OUT="$(as_life "$1" "$2" "$3")"
  [[ "$(echo "$AS_OUT" | jf result)" == "$4" ]] || {
    echo "$AS_OUT" >&2; ssh_host "$1" "journalctl -u baft-tunnel@$(as_inst "$1") -n 40 --no-pager" >&2 || true
    echo "$2 on $1 was not $4" >&2; exit 1; }
}
as_boot() { # side -> "active enabled" for the tunnel unit
  ssh_host "$1" "echo \$(systemctl is-active baft-tunnel@$(as_inst "$1").service) \$(systemctl is-enabled baft-tunnel@$(as_inst "$1").service 2>/dev/null || true)"
}
as_wait() { # seconds : until 64 KiB passes
  local t0=$SECONDS
  while (( SECONDS - t0 < $1 )); do probe_host iran 65536 >/dev/null 2>&1 && { echo $((SECONDS - t0)); return 0; }; sleep 2; done
  return 1
}
as_echo() { ssh_host abroad "nohup python3 '$REMOTE_DIR/echo.py' '$MSVC' > '$REMOTE_DIR/echo2.log' 2>&1 < /dev/null & echo \$! > '$REMOTE_DIR/echo2.pid'"; }

echo "--- Start through the agent service: serve, then forward (enable --now) ---"
as_echo
OLD_LOCAL_PORT="$LOCAL_PORT"; LOCAL_PORT="$MSVC"
as_expect abroad start as-srv-start-1 applied
as_expect iran start as-cli-start-1 applied
[[ "$(echo "$AS_OUT" | jf boot.enabled_at_boot)" == True ]] || { echo "$AS_OUT" >&2; echo "start receipt does not report enabled at boot" >&2; exit 1; }
[[ "$(echo "$AS_OUT" | jf ready)" == unknown ]] || { echo "start receipt claimed ready" >&2; exit 1; }
for side in iran abroad; do [[ "$(as_boot "$side")" == "active enabled" ]] || { echo "tunnel unit on $side is not active and enabled after Start" >&2; exit 1; }; done
AS_START_S="$(as_wait 60)" || { echo "no data through the tunnel after Start" >&2; exit 1; }
probe_host iran 65536; probe_host iran 2097289
[[ "$(as_life iran start as-cli-start-1 | jf replayed)" == True ]] || { echo "idempotent replay of Start failed" >&2; exit 1; }
[[ "$(as_life iran start as-cli-start-2 | jf result)" == refused ]] || { echo "Start on a running unit was not refused" >&2; exit 1; }

echo "--- diagnostics on the running tunnel: next_action ---"
sleep 12
as_api iran POST /v1/diagnostics > "$LOCAL_TMP/as-diag.json"
AS_NEXT="$(jf next_action.action < "$LOCAL_TMP/as-diag.json")"
[[ "$AS_NEXT" =~ ^(none|rerun|fix|check|start)$ ]] || { cat "$LOCAL_TMP/as-diag.json" >&2; echo "diagnostics has no next_action" >&2; exit 1; }
[[ "$AS_NEXT" != start ]] || { echo "diagnostics asks to start a running tunnel" >&2; exit 1; }

echo "--- Restart through the agent service (new process, stays enabled) ---"
as_expect iran restart as-cli-restart-1 applied
python3 - "$AS_OUT" <<'PY' || { echo "$AS_OUT" >&2; echo "restart receipt lacks a new process or the boot link" >&2; exit 1; }
import json, sys
r = json.loads(sys.argv[1]); v = {c["id"]: c["result"] for c in r["verification"]}
assert v.get("new_process") == "pass" and v.get("stopped_before_start") == "pass" and v.get("enabled_at_boot") == "pass", v
PY
AS_RESTART_S="$(as_wait 60)" || { echo "no data through the tunnel after Restart" >&2; exit 1; }

echo "--- Stop through the agent service (disable --now), then Start again ---"
as_expect iran stop as-cli-stop-1 applied
[[ "$(as_boot iran)" == "inactive disabled" ]] || { as_boot iran >&2; echo "Stop did not leave the unit inactive and disabled" >&2; exit 1; }
sleep 2
if probe_host iran 4096 >/dev/null 2>&1; then echo "data still crossed after Stop" >&2; exit 1; fi
as_api iran POST /v1/diagnostics > "$LOCAL_TMP/as-diag-stopped.json"
[[ "$(jf next_action.action < "$LOCAL_TMP/as-diag-stopped.json")" == start ]] || { cat "$LOCAL_TMP/as-diag-stopped.json" >&2; echo "diagnostics on a stopped tunnel does not say start" >&2; exit 1; }
[[ "$(as_life iran stop as-cli-stop-2 | jf result)" == refused ]] || { echo "Stop on a stopped, disabled unit was not refused" >&2; exit 1; }
as_expect iran start as-cli-start-3 applied
[[ "$(as_boot iran)" == "active enabled" ]] || { echo "Start after Stop did not enable the unit again" >&2; exit 1; }
as_wait 60 >/dev/null || { echo "no data after Start following Stop" >&2; exit 1; }
for side in iran abroad; do
  ssh_host "$side" "! grep -rqs -- \"\$(cat $AGS/token)\" $AGS/$(as_inst "$side") && ! grep -rqs 'PRIVATE KEY' $AGS/$(as_inst "$side")" ||
    { echo "agent state on $side holds a secret" >&2; exit 1; }
done

AS_REBOOT_S=none; AS_REBOOT_DATA=0
if [[ "$AGENT_REBOOT" == 1 ]]; then
  echo "--- real reboot of both hosts (owner-approved, once): agent and tunnel must come back with no command from us ---"
  declare -A AS_BOOT_ID
  for side in iran abroad; do AS_BOOT_ID[$side]="$(ssh_host "$side" "cat /proc/sys/kernel/random/boot_id")"; done
  # A marker under /var/tmp survives the reboot: an ssh retry can never trigger a second reboot.
  for side in iran abroad; do
    ssh_host "$side" "if [ -e '$AS_MARK' ]; then echo already-rebooted; else touch '$AS_MARK'; nohup sh -c 'sleep 3; systemctl reboot' >/dev/null 2>&1 < /dev/null & echo reboot-scheduled; fi"
  done
  t0=$SECONDS
  for side in iran abroad; do
    while :; do
      (( SECONDS - t0 < 600 )) || { echo "$side did not come back over SSH within 600 s after the reboot" >&2; exit 1; }
      id="$(ssh_host "$side" "cat /proc/sys/kernel/random/boot_id" 2>/dev/null || true)"
      [[ -n "$id" && "$id" != "${AS_BOOT_ID[$side]}" ]] && break
      sleep 10
    done
    echo "$side is back (new boot) after $((SECONDS - t0)) s"
  done
  # Nothing is started by us here: only observed.
  for side in iran abroad; do
    inst="$(as_inst "$side")"
    ssh_host "$side" "for n in \$(seq 1 90); do systemctl is-active --quiet baft-tunnel@$inst.service && break; sleep 1; done;
      echo \"$side: tunnel=\$(systemctl is-active baft-tunnel@$inst.service) boot=\$(systemctl is-enabled baft-tunnel@$inst.service) agent=\$(systemctl is-active baft-tunnel-agent@$inst.service) control=\$(stat -c '%U:%G %a' /run/baft-tunnel/control)\";
      systemctl is-active --quiet baft-tunnel@$inst.service && systemctl is-enabled --quiet baft-tunnel@$inst.service && test -d /run/baft-tunnel/control" ||
      { ssh_host "$side" "journalctl -b -u baft-tunnel@$inst -u baft-tunnel-agent@$inst -n 40 --no-pager" >&2 || true; echo "tunnel on $side did not come back by itself after the reboot" >&2; exit 1; }
    as_agent_up "$side" "$inst" || { echo "agent service on $side did not come back by itself after the reboot" >&2; exit 1; }
  done
  AS_REBOOT_S=$((SECONDS - t0))
  # /tmp was cleared by the reboot: put the test helpers (not the product) back, then measure data again.
  for side in iran abroad; do
    ssh_host "$side" "mkdir -p '$REMOTE_DIR/pkg'"
    copy_to_host "$side" "$LOCAL_TMP/echo.py" "$LOCAL_TMP/probe.py" "$LOCAL_TMP/as-pkg.tgz"
    ssh_host "$side" "tar -xzf '$REMOTE_DIR/as-pkg.tgz' -C '$PKG'"
  done
  as_echo
  as_wait 90 >/dev/null || { echo "no data through the tunnel after the reboot" >&2; exit 1; }
  probe_host iran 65536; probe_host iran 2097289
  AS_REBOOT_DATA=1
  as_api iran POST /v1/diagnostics > "$LOCAL_TMP/as-diag-reboot.json"
  [[ "$(jf next_action.action < "$LOCAL_TMP/as-diag-reboot.json")" != start ]] || { echo "diagnostics after the reboot says start" >&2; exit 1; }
  for side in iran abroad; do ssh_host "$side" "rm -f '$AS_MARK'"; done
fi
LOCAL_PORT="$OLD_LOCAL_PORT"; stop_role abroad echo2

echo "--- uninstall and cleanup proof ---"
as_cleanup; trap cleanup EXIT
for side in iran abroad; do
  ssh_host "$side" "! test -e $UB && ! test -e $EB && ! test -e /var/lib/baft-tunnel && ! test -e $AGS && ! test -e /run/baft-tunnel/control &&
    ! ls /etc/systemd/system/multi-user.target.wants/ | grep -q '^baft-tunnel' && ! systemctl list-units --all --plain --no-legend 'baft-tunnel*' | grep -q . &&
    ! ss -ltnH | awk '{print \$4}' | grep -qE ':($MSVC|$MTP|$MAG)\$'" ||
    { echo "uninstall left BAFT-TUNNEL resources on $side" >&2; exit 1; }
  for u in baft-tunnel-agent baft-tunnel; do
    if [[ " ${AS_USERS_BEFORE[*]} " != *" $side:$u "* ]]; then ssh_host "$side" "! id $u >/dev/null 2>&1" || { echo "user $u was left on $side" >&2; exit 1; }; fi
  done
  side_snapshot "$side" > "$LOCAL_TMP/as-after-$side.txt"
  if ! diff "$LOCAL_TMP/as-before-$side.txt" "$LOCAL_TMP/as-after-$side.txt" > "$LOCAL_TMP/as-diff-$side.txt"; then
    # Printed with addresses masked and capped: the run log is public and the host's firewall is not ours to publish.
    echo "--- host state changed on $side during the stage ($(grep -c '^[<>]' "$LOCAL_TMP/as-diff-$side.txt") lines; addresses masked) ---"
    grep '^[<>]' "$LOCAL_TMP/as-diff-$side.txt" | sed -E 's/[0-9]{1,3}(\.[0-9]{1,3}){3}(\/[0-9]+)?/<ip>/g; s/[0-9a-fA-F]{0,4}(:[0-9a-fA-F]{0,4}){2,7}(\/[0-9]+)?/<ip6>/g' | head -n 30
    grep -E '^[<>]' "$LOCAL_TMP/as-diff-$side.txt" | grep -qiE "baft|(:| |dport |sport )($MSVC|$MTP|$MAG)\b" &&
      { echo "BAFT-related firewall rules or units changed on $side" >&2; exit 1; }
    ! diff <(sed -n '/^# units/,$p' "$LOCAL_TMP/as-before-$side.txt") <(sed -n '/^# units/,$p' "$LOCAL_TMP/as-after-$side.txt") >/dev/null &&
      { echo "baft-tunnel unit files changed on $side" >&2; exit 1; }
    # A listener change is a failure without a reboot; after a reboot, host services may legitimately differ (reported).
    if [[ "$AGENT_REBOOT" != 1 ]] && ! diff <(sed -n '/^# listeners/,/^# units/p' "$LOCAL_TMP/as-before-$side.txt") <(sed -n '/^# listeners/,/^# units/p' "$LOCAL_TMP/as-after-$side.txt") >/dev/null; then
      echo "listeners changed on $side" >&2; exit 1
    fi
    AS_DRIFT+=("$side")
  fi
done
echo "AGENT_SERVICE_RESULT source=${MVP_SOURCE} binary_sha256=${AS_SHA} clean_hosts=1 install_register_one_step=1 setup_no_manual_chown=1 agent_self_start=${AS_AGENT_SELF_START} agent_user=baft-tunnel-agent agent_loopback=1 start_enabled_at_boot=1 start_data_s=${AS_START_S} integrity_64k_2m=1 replay=1 running_refused=1 diag_next_running=${AS_NEXT} restart_new_process=1 restart_data_s=${AS_RESTART_S} stop_disabled=1 stop_no_data=1 diag_next_stopped=start stopped_refused=1 start_again=1 state_secret_free=1 reboot=${AGENT_REBOOT} reboot_back_s=${AS_REBOOT_S} reboot_data_64k_2m=${AS_REBOOT_DATA} uninstalled=1 users_removed=1 baft_fw_rules=0 host_drift=${AS_DRIFT[*]:-none}"
