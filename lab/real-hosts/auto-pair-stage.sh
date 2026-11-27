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
# The baseline E2E logs are no longer authoritative after those processes stop. Remove them so
# a later pairing failure cannot accidentally report stale baseline tunnel output as AP evidence.
AP_SVC=18094; AP_AG=18096; AP_TP="$E2E_PORT"
# Keep aligned with src/integrity_probe.rs::PROBE_COOLDOWN.
AP_PROBE_COOLDOWN=60
EB=/etc/baft-tunnel; UB=/usr/local/bin/baft-tunnel; AGD=/var/lib/baft-tunnel-agent; PKG="$REMOTE_DIR/pkg"
AP_SHA="$(sha256sum "$MVP_BINARY_PATH" | awk '{print $1}')"
AP_HOST_iran="$LAB_IRAN_HOST"; AP_HOST_abroad="$LAB_ABROAD_HOST"

ap_snapshot() { # side : normalized firewall rules, listeners outside the test ports, baft-tunnel* unit-file count
  ssh_host "$1" "echo '# firewall'; { nft list ruleset 2>/dev/null | sed -E 's/packets [0-9]+ bytes [0-9]+//'; iptables-save 2>/dev/null | grep -v '^#' | sed -E 's/\\[[0-9]+:[0-9]+\\]//'; };
    echo '# listeners'; ss -ltnH | awk '{print \$4}' | grep -vE ':($AP_SVC|$AP_AG|$AP_TP|$LOCAL_PORT|$ECHO_PORT)\$' | sort -u;
    echo '# units'; systemctl list-unit-files --no-legend 'baft-tunnel*' 2>/dev/null | wc -l"
}
echo "--- clean-host check ---"
AP_FW_DRIFT=()
AP_INSTANCES=""
AP_INSTALLED_HOSTS=()
AP_BACKUPS_iran=()
AP_BACKUPS_abroad=()
for side in iran abroad; do
  ssh_host "$side" "test -d '$REMOTE_DIR' && test ! -L '$REMOTE_DIR' &&
    [ \"\$(stat -c '%U:%G %a' '$REMOTE_DIR')\" = 'root:root 700' ] &&
    test -f '$REMOTE_DIR/.baft-tunnel-e2e-run-owner' && test ! -L '$REMOTE_DIR/.baft-tunnel-e2e-run-owner' &&
    [ \"\$(stat -c '%U:%G %a' '$REMOTE_DIR/.baft-tunnel-e2e-run-owner')\" = 'root:root 600' ] &&
    [ \"\$(cat '$REMOTE_DIR/.baft-tunnel-e2e-run-owner')\" = '$REMOTE_DIR' ] &&
    test ! -e '$UB' && test ! -L '$UB' &&
    test ! -e '$EB' && test ! -L '$EB' &&
    test ! -e /var/lib/baft-tunnel && test ! -L /var/lib/baft-tunnel &&
    test ! -e /var/lib/baft-tunnel-agent && test ! -L /var/lib/baft-tunnel-agent &&
    test ! -e /var/lib/baft-tunnel-control && test ! -L /var/lib/baft-tunnel-control &&
    test ! -e /run/baft-tunnel && test ! -L /run/baft-tunnel &&
    test ! -e /usr/local/lib/baft-tunnel && test ! -L /usr/local/lib/baft-tunnel &&
    test ! -e /usr/local/lib/baft-tunnel-engines && test ! -L /usr/local/lib/baft-tunnel-engines &&
    test ! -e /etc/tmpfiles.d/baft-tunnel-control.conf && test ! -L /etc/tmpfiles.d/baft-tunnel-control.conf &&
    ! find /etc/systemd/system -name 'baft-tunnel*' -print -quit | grep -q . &&
    ! getent passwd baft-tunnel >/dev/null 2>&1 &&
    ! getent passwd baft-tunnel-agent >/dev/null 2>&1 &&
    ! getent group baft-tunnel >/dev/null 2>&1 &&
    ! getent group baft-tunnel-agent >/dev/null 2>&1 &&
    ! systemctl list-unit-files --no-legend 'baft-tunnel*' 2>/dev/null | grep -q '^baft-tunnel' &&
    ! systemctl list-units --all --no-legend 'baft-tunnel*' 2>/dev/null | grep -q '^baft-tunnel' &&
    ! systemctl list-units --all --state=failed --no-legend 'baft-tunnel*' 2>/dev/null | grep -q '^baft-tunnel' &&
    [ -z \"\$(find /var/backups -maxdepth 1 \\( -name 'baft-tunnel-install.*' -o -name 'baft-tunnel-backup.*' \\) -print -quit 2>/dev/null)\" ]" ||
    { echo "$side is not clean: pre-existing BAFT-TUNNEL files, units, accounts, state or backups are present" >&2; exit 1; }
  ap_snapshot "$side" > "$LOCAL_TMP/ap-before-$side.txt"
done

# Only after both remote run directories are verified as owned by this E2E run,
# remove stale baseline logs so failure evidence cannot be misattributed.
for side in iran abroad; do
  ssh_host "$side" "rm -f -- '$REMOTE_DIR/serve.log' '$REMOTE_DIR/forward.log' '$REMOTE_DIR/echo.log' '$REMOTE_DIR/impair.log' '$REMOTE_DIR/ap-echo.log' '$REMOTE_DIR/agent.log'" || exit 1
done

ap_purge() { # side instances... : invoke the host-side, path-scoped cleanup helper
  local side="$1"; shift
  local backup_var="AP_BACKUPS_$side" backup_dir instance remote_command
  local -n backup_dirs="$backup_var"
  local -a command_parts
  [[ " ${AP_INSTALLED_HOSTS[*]} " == *" $side "* ]] || return 0
  command_parts=(bash "$PKG/lab/real-hosts/auto-pair-cleanup-host.sh" "$AP_SHA" "$PKG" "$AGD" "$REMOTE_DIR")
  if [[ "$side" == abroad ]]; then command_parts+=(--echo-port "$AP_SVC"); fi
  for backup_dir in "${backup_dirs[@]}"; do
    [[ "$backup_dir" =~ ^/var/backups/baft-tunnel-install\.[[:alnum:]]+$ ]] || {
      echo "refusing unrecognized install backup path on $side" >&2; return 1;
    }
    command_parts+=(--backup-dir "$backup_dir")
  done
  for instance in $*; do
    [[ "$instance" =~ ^[A-Za-z0-9][A-Za-z0-9_-]{0,62}$ ]] || {
      echo "refusing unrecognized instance name during cleanup" >&2; return 1;
    }
    command_parts+=(--instance "$instance")
  done
  printf -v remote_command '%q ' "${command_parts[@]}"
  ssh_host "$side" "$remote_command"
}
ap_failure_evidence() {
  local status="$1" side inst log
  (( status != 0 )) || return 0
  echo "--- automatic-pairing failure evidence (candidate=${MVP_SOURCE}) ---" >&2
  for side in iran abroad; do
    echo "--- auto-pair host=$side ---" >&2
    for log in ap-echo.log; do
      ssh_host "$side" "if [ -f '$REMOTE_DIR/$log' ]; then echo '=== $side:$log ==='; tail -n 80 '$REMOTE_DIR/$log'; fi" >&2 || true
    done
    for inst in $AP_INSTANCES; do
      ssh_host "$side" "journalctl -u baft-tunnel-agent@$inst.service -n 80 --no-pager 2>&1; journalctl -u baft-tunnel@$inst.service -n 80 --no-pager 2>&1" >&2 || true
    done
    ssh_host "$side" "echo '# test-port sockets'; ss -ltnp 2>/dev/null | grep -E ':(18094|18096|$AP_TP)\\b' || true;
      echo '# relevant processes'; ps -eo pid,ppid,user,args | grep -E '[b]aft-tunnel|[e]cho.py' | head -n 40" >&2 || true
  done
}
ap_cleanup() {
  local status="${1:-0}" cleanup_failed=0 side
  local -a remaining_hosts=()
  LAB_PRESERVE_REMOTE=1
  ap_failure_evidence "$status"
  for side in iran abroad; do
    if ap_purge "$side" $AP_INSTANCES; then
      echo "AUTO_PAIR_CLEANUP_HOST side=$side result=pass"
      if [[ "$side" == iran ]]; then AP_BACKUPS_iran=(); else AP_BACKUPS_abroad=(); fi
    else
      echo "AUTO_PAIR_CLEANUP_HOST side=$side result=fail" >&2
      cleanup_failed=1
      [[ " ${AP_INSTALLED_HOSTS[*]} " == *" $side "* ]] && remaining_hosts+=("$side")
    fi
  done
  AP_INSTALLED_HOSTS=("${remaining_hosts[@]}")
  if (( cleanup_failed == 0 )); then
    LAB_PRESERVE_REMOTE=0
    AP_INSTANCES=""
    AP_BACKUPS_iran=()
    AP_BACKUPS_abroad=()
  else
    LAB_PRESERVE_REMOTE=1
  fi
  (( cleanup_failed == 0 )) || return 1
}
# set +e: under errexit the (exit N) that restores the status would end the trap before cleanup runs.
trap 'ap_status=$?; set +e; ap_cleanup_status=0; ap_cleanup "$ap_status" || ap_cleanup_status=$?; if (( ap_status == 0 && ap_cleanup_status != 0 )); then ap_status=$ap_cleanup_status; fi; (exit "$ap_status"); cleanup' EXIT

tar -czf "$LOCAL_TMP/ap-pkg.tgz" scripts/*.sh scripts/pki/*.sh systemd lab/real-hosts/auto-pair-install-host.sh lab/real-hosts/auto-pair-cleanup-host.sh lab/real-hosts/lib-runtime-children.sh
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

ap_pair_file_status() { # side instance -> prints only state and a bounded failure reason (never code/nonce/fingerprint)
  local side="$1" inst="$2" raw
  raw="$(ssh_host "$side" "if test -f /run/baft-tunnel/control/$inst.pair-status; then cat /run/baft-tunnel/control/$inst.pair-status; else printf '__MISSING__'; fi" </dev/null)" || return 1
  python3 -c 'import json,sys
raw=sys.stdin.read().strip()
if raw == "__MISSING__":
    print("missing")
else:
    try:
        value=json.loads(raw)
        state=value.get("state", "malformed") if isinstance(value,dict) else "malformed"
        print(state)
        if state == "failed":
            reason=str(value.get("reason", "unspecified"))[:240]
            print("reason="+reason, file=sys.stderr)
    except (ValueError, TypeError):
        print("malformed")' <<<"$raw"
}

ap_pair_failure_diagnostics() { # side instance -> bounded, non-secret failure evidence
  local side="$1" inst="$2" http
  echo "--- pairing failure evidence: side=$side instance=$inst ---" >&2
  echo "helper_status=$(ap_pair_file_status "$side" "$inst" 2>&1 | tr '\n' ' ')" >&2 || true
  http="$(ssh_host "$side" "curl -sS -o /dev/null -w '%{http_code} bytes=%{size_download}' -H \"Authorization: Bearer \$(cat $AGD/token)\" http://127.0.0.1:$AP_AG/v1/pair/status 2>/dev/null || printf 'transport-error'" </dev/null 2>/dev/null || true)"
  echo "agent_pair_status_http=$http" >&2
  ssh_host "$side" "systemctl show baft-tunnel-control-pair@$inst.service -p LoadState -p ActiveState -p SubState -p Result -p ExecMainStatus --no-pager 2>/dev/null || true;
    systemctl show baft-tunnel-control-pair@$inst.path -p LoadState -p ActiveState -p SubState --no-pager 2>/dev/null || true;
    journalctl -u baft-tunnel-control-pair@$inst.service -n 20 --no-pager -o cat 2>/dev/null || true" >&2 || true
}

ap_wait_pair_file_done() { # side instance timeout -> waits for durable helper status, independent of Agent setup/runtime mode
  local side="$1" inst="$2" limit="$3" t0=$SECONDS state=missing
  while (( SECONDS - t0 < limit )); do
    state="$(ap_pair_file_status "$side" "$inst" 2>/dev/null || printf 'unreadable')"
    case "$state" in
      done) return 0 ;;
      failed)
        echo "pair helper failed on $side ($inst)" >&2
        ap_pair_failure_diagnostics "$side" "$inst"
        return 1
        ;;
      missing|idle|working|waiting|approval|unreadable|malformed) ;;
      *) echo "unexpected pair helper state on $side ($inst): $state" >&2 ;;
    esac
    sleep 1
  done
  echo "timed out waiting for helper status done on $side ($inst); last helper state: $state" >&2
  ap_pair_failure_diagnostics "$side" "$inst"
  return 1
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
d = json.load(open(sys.argv[1])); tiers = {t["id"]: t for t in d["tiers"]}; r = {k: v["result"] for k, v in tiers.items()}
print("tiers[%s]=%s" % (sys.argv[2], json.dumps(r, sort_keys=True)))
for k in ("secure_channel", "identity", "authorization", "session", "integrity"):
    assert r.get(k) == "pass", (k, r)
assert tiers["integrity"].get("source") == "in_product_probe", tiers["integrity"]
assert tiers["integrity"].get("params", {}).get("age_s", 999) <= 60, tiers["integrity"]
assert d["unit"]["result"] == "active", d
PY
}

ap_verify_diag() { # side label direction : run the authenticated B2 probe, then require fresh diagnostics PASS
  local side="$1" label="$2" direction="$3" key body response_file retry_key
  key="apv-${label}-${side}-${RANDOM}${RANDOM}"
  body="{\"idempotency_key\":\"$key\"}"
  response_file="$LOCAL_TMP/ap-verify-$label-$side.json"
  ap_api "$side" POST /v1/verify <<<"$body" >"$response_file"
  if python3 - "$response_file" <<'PY'
import json, sys
v = json.load(open(sys.argv[1]))
raise SystemExit(0 if v.get("result") == "unknown" and v.get("reason") == "rate_limited" else 1)
PY
  then
    # Preserve the first completed receipt as evidence. A retry must use a new key:
    # the API replays completed receipts and will not rerun a probe for the same key.
    cp -- "$response_file" "$response_file.rate-limited"
    echo "integrity probe on $side ($label) is rate-limited; waiting $((AP_PROBE_COOLDOWN + 1))s (cooldown plus margin) before one retry" >&2
    sleep "$((AP_PROBE_COOLDOWN + 1))"
    retry_key="apv-${label}-${side}-${RANDOM}${RANDOM}"
    while [[ "$retry_key" == "$key" ]]; do
      retry_key="apv-${label}-${side}-${RANDOM}${RANDOM}"
    done
    key="$retry_key"
    body="{\"idempotency_key\":\"$key\"}"
    ap_api "$side" POST /v1/verify <<<"$body" >"$response_file"
  fi
  python3 - "$response_file" "$side" "$direction" <<'PY' || { cat "$response_file" >&2; echo "authenticated integrity probe failed on $side ($label)" >&2; return 1; }
import json, sys
v = json.load(open(sys.argv[1]))
assert v.get("result") == "pass", v
assert v.get("sha256_match") is True and v.get("bytes") == 65536, v
assert v.get("direction") == sys.argv[3], v
print("integrity_probe[%s] direction=%s result=%s bytes=%s sha256_match=%s" % (sys.argv[2], v.get("direction"), v.get("result"), v.get("bytes"), v.get("sha256_match")))
PY
  ap_diag "$side" "$label"
}

ap_round() { # name direction listener_side joiner_side joiner_location
  local rn="$1" dir="$2" ls="$3" js="$4" jloc="$5" lloc li ji code out nonce vl vj side inst pair rot n backup_dir install_status
  li="${rn}l"; ji="${rn}j"; AP_INSTANCES="$li $ji"
  if [[ "$ls" == abroad ]]; then lloc=abroad; else lloc=iran; fi
  echo "--- round $rn ($dir): listener=$ls ($lloc) joiner=$js ($jloc) ---"
  for side in iran abroad; do
    inst="$li"; [[ "$side" == "$js" ]] && inst="$ji"
    copy_to_host "$side" "$LOCAL_TMP/ap-pkg.tgz" "$LOCAL_TMP/baft-tunnel-ap"
    ssh_host "$side" "set -e; mkdir -p '$PKG'; tar -xzf '$REMOTE_DIR/ap-pkg.tgz' -C '$PKG'"
    AP_INSTALLED_HOSTS+=("$side")
    install_status=0
    backup_dir="$(ssh_host "$side" "bash '$PKG/lab/real-hosts/auto-pair-install-host.sh' '$REMOTE_DIR/baft-tunnel-ap' '$AP_SHA' '$PKG' '$EB' '$AGD' '$REMOTE_DIR' '$AP_AG' '$inst'")" || install_status=$?
    [[ "$backup_dir" =~ ^/var/backups/baft-tunnel-install\.[[:alnum:]]+$ ]] || {
      echo "install helper did not return a scoped backup path on $side" >&2
      return 1
    }
    if [[ "$side" == iran ]]; then AP_BACKUPS_iran+=("$backup_dir"); else AP_BACKUPS_abroad+=("$backup_dir"); fi
    (( install_status == 0 )) || {
      echo "install or agent startup failed on $side (exit=$install_status)" >&2
      return "$install_status"
    }
  done
  # setup mode: the agent says setup_pending until pairing wrote the configuration
  for side in iran abroad; do
    inst="$li"; [[ "$side" == "$js" ]] && inst="$ji"
    [[ "$(ap_api "$side" GET /v1/status </dev/null | apjf setup_pending)" == True ]] || { echo "agent on $side is not in setup mode" >&2; return 1; }
  done
  ssh_host abroad "nohup python3 '$REMOTE_DIR/echo.py' '$AP_SVC' > '$REMOTE_DIR/ap-echo.log' 2>&1 < /dev/null & echo \$! > '$REMOTE_DIR/ap-echo.pid'"

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
  # Both agents may exit setup mode as soon as the helper writes config. The API route is setup-only,
  # so completion is read from each durable root-helper status file, not from /v1/pair/status.
  ap_wait_pair_file_done "$ls" "$li" 150 || return 1
  ap_wait_pair_file_done "$js" "$ji" 150 || return 1
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
    ap_cfg_check "$ls" "$li" "assert c['mode']=='serve' and c['direction']=='direct' and c['engine']=='tcp-tls' and c['target']=='127.0.0.1:$AP_SVC' and 'session_rotate_secs' not in c;assert c['protocol_version']==2" || return 1
    if [[ "$jloc" == iran ]]; then rot="assert c.get('session_rotate_secs')==10"; else rot="assert 'session_rotate_secs' not in c"; fi
    ap_cfg_check "$js" "$ji" "assert c['mode']=='forward' and c['direction']=='direct' and c['listen']=='127.0.0.1:$AP_SVC' and c['remote']=='$lhost:$AP_TP';$rot;assert c['protocol_version']==2" || return 1
  else
    ap_cfg_check "$ls" "$li" "assert c['mode']=='serve' and c['direction']=='reverse' and c['ingress']=='127.0.0.1:$AP_SVC' and 'target' not in c;assert c['protocol_version']==2" || return 1
    ap_cfg_check "$js" "$ji" "assert c['mode']=='forward' and c['direction']=='reverse' and c['target']=='127.0.0.1:$AP_SVC' and 'listen' not in c and 'session_rotate_secs' not in c and c['remote']=='$lhost:$AP_TP';assert c['protocol_version']==2" || return 1
  fi
  # the pair CA key stays on the listener only
  ssh_host "$ls" "test -s $EB/pair-ca/ca.key" || { echo "listener has no pair CA key" >&2; return 1; }
  ssh_host "$js" "! test -e $EB/pair-ca && ! find $EB -name 'ca.key' | grep -q ." || { echo "the joiner holds CA key material" >&2; return 1; }
  ssh_host "$ls" "! ls /run/baft-tunnel/control/ 2>/dev/null | grep -q '\\.pair\$'" || { echo "request file was not consumed" >&2; return 1; }
  # no worker process is left behind (the [b] keeps pgrep from matching its own ssh command line)
  for side in "$ls" "$js"; do ssh_host "$side" "! pgrep -f '[b]aft-tunnel pair worker' >/dev/null" || { echo "pair worker still running on $side" >&2; return 1; }; done

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
  ap_verify_diag "$ls" "$rn" "$dir" || return 1
  ap_verify_diag "$js" "$rn" "$dir" || return 1
  echo "AUTO_PAIR_ROUND name=$rn direction=$dir listener=$ls joiner=$js joiner_location=$jloc fingerprints_match=1 wrong_nonce_refused=1 second_request_refused=1 configs_ok=1 ca_key_listener_only=1 workers_gone=1 start_via_agent=1 non_root=1 integrity_64k_2m=1 integrity_probe_both_nodes=pass up_s=$AP_UP_S diagnostics_tiers=pass"
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
