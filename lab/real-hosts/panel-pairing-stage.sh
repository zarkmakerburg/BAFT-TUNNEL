# shellcheck shell=bash
# Panel pairing stage (U4 + U5b, ADR-0026), sourced by phase1-two-host-e2e.sh when LAB_PANEL_PAIRING=1.
# Installed product path on both LAB hosts, and both nodes are set up through the agent API only:
#   clean install with `install.sh --register NAME`, the agent in setup mode, preflight uses the root setup
#   helper, init through POST /v1/setup/apply on each host, own bundle exported with GET /v1/setup/bundle,
#   the peer bundle previewed with POST /v1/setup/bundle/inspect (wrong checksum flagged, wrong-checksum join
#   refused), join through the agent, both agents reload by themselves, Start through the agent,
#   64 KiB + 2 MiB integrity, then a full uninstall. No `baft-tunnel setup` command is run as root by us.
# The runner only carries public CSRs, signed public credential files, and public bundles. For tcp-tls,
# an ephemeral CA runs in a network-isolated container; its encrypted CA key stays in container tmpfs.
# Topology: abroad = serve (owns the service target), iran = forward (owns the ingress).

PANEL_ENGINE="${LAB_PANEL_ENGINE:-noise-tcp}"
[[ "$PANEL_ENGINE" == noise-tcp || "$PANEL_ENGINE" == tcp-tls ]] || { echo "invalid LAB_PANEL_ENGINE" >&2; exit 2; }
echo "=== PANEL PAIRING ($PANEL_ENGINE): install --register, setup mode, enrollment/init/export/inspect/join through the agent API only, Start ==="
stop_role iran serve; stop_role abroad serve; stop_role iran forward; stop_role abroad forward; sleep 1
# Do not let logs from earlier E2E stages (including the intentional revoked-cert test)
# be mistaken for evidence from this panel-pairing run.
for side in iran abroad; do
  ssh_host "$side" "rm -f '$REMOTE_DIR/serve.log' '$REMOTE_DIR/forward.log' '$REMOTE_DIR/echo.log' '$REMOTE_DIR/echo2.log' '$REMOTE_DIR/impair.log'" || exit 1
done
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
  if [[ "$PANEL_ENGINE" == tcp-tls ]]; then
    if [[ "$1" == abroad ]]; then
      printf '{"engine":"tcp-tls","role":"serve","direction":"direct","node_id":"abroad","peer_id":"iran","tunnel_id":"ppair","service_port":%s,"tunnel_port":%s,"dir":"%s","instance":"%s"}' "$MSVC" "$MTP" "$EB" "$SRV"
    else
      printf '{"engine":"tcp-tls","role":"forward","direction":"direct","node_id":"iran","peer_id":"abroad","tunnel_id":"ppair","service_port":%s,"tunnel_port":%s,"peer_host":"%s","server_name":"%s","dir":"%s","instance":"%s"}' "$MSVC" "$MTP" "$LAB_ABROAD_HOST" "$LAB_ABROAD_HOST" "$EB" "$CLI"
    fi
  elif [[ "$1" == abroad ]]; then
    printf '{"engine":"noise-tcp","role":"serve","direction":"direct","node_id":"abroad","peer_id":"iran","tunnel_id":"ppair","service_port":%s,"tunnel_port":%s,"dir":"%s","instance":"%s"}' "$MSVC" "$MTP" "$EB" "$SRV"
  else
    printf '{"engine":"noise-tcp","role":"forward","direction":"direct","node_id":"iran","peer_id":"abroad","tunnel_id":"ppair","service_port":%s,"tunnel_port":%s,"peer_host":"%s","dir":"%s","instance":"%s"}' "$MSVC" "$MTP" "$LAB_ABROAD_HOST" "$EB" "$CLI"
  fi
}
pp_tls_enroll() {
  echo "--- D2: create local CSRs on both agents; only public CSRs leave the hosts ---"
  local side node san req_id pid_before pid_after expected_pid pid_before_abroad pid_before_iran
  mkdir -p "$LOCAL_TMP/d2-certs/csr" "$LOCAL_TMP/d2-certs/signed"
  for side in abroad iran; do
    if [[ "$side" == abroad ]]; then node=abroad; san="$LAB_ABROAD_HOST"; else node=iran; san="$LAB_IRAN_HOST"; fi
    req_id="pki-csr-$side-0001"
    python3 - "$LOCAL_TMP/d2-certs/csr-request-$side.json" "$req_id" "$node" "$san" <<'PY'
import json, sys
json.dump({"request_id":sys.argv[2],"node_id":sys.argv[3],"server_name":sys.argv[4],"confirm":True},open(sys.argv[1],"w"))
PY
    pp_api "$side" POST /v1/pki/csr/create "$LOCAL_TMP/d2-certs/csr-request-$side.json" > "$LOCAL_TMP/d2-certs/csr-response-$side.json"
    python3 - "$LOCAL_TMP/d2-certs/csr-response-$side.json" "$LOCAL_TMP/d2-certs/csr/$side.csr" "$node" "$san" <<'PY'
import json, sys
d=json.load(open(sys.argv[1]))
assert d.get("result") == "created", d
assert d.get("node_id") == sys.argv[3], d
assert d.get("expected_server_san") == "IP:" + sys.argv[4], d
assert d.get("private_key_exported") is False, d
assert len(d.get("public_key_sha256","")) == 64, d
open(sys.argv[2],"w").write(d["csr_pem"])
PY
    python3 - "$LOCAL_TMP/d2-certs/csr-read-$side.json" "$node" "$san" <<'PY'
import json, sys
json.dump({"request_id":"pki-read-"+sys.argv[2]+"-0001","node_id":sys.argv[2],"server_name":sys.argv[3]},open(sys.argv[1],"w"))
PY
    pp_api "$side" POST /v1/pki/csr/read "$LOCAL_TMP/d2-certs/csr-read-$side.json" > "$LOCAL_TMP/d2-certs/csr-recovered-$side.json"
    python3 - "$LOCAL_TMP/d2-certs/csr-response-$side.json" "$LOCAL_TMP/d2-certs/csr-recovered-$side.json" <<'PY'
import json, sys
a,b=(json.load(open(x)) for x in sys.argv[1:])
assert b.get("result") == "retrieved" and b.get("csr_pem") == a.get("csr_pem"), (a,b)
assert b.get("public_key_sha256") == a.get("public_key_sha256"), (a,b)
assert b.get("private_key_exported") is False, b
PY
    pid_before="$(ssh_host "$side" "systemctl show -p MainPID --value baft-tunnel-agent@$(pp_inst "$side").service")"
    [[ "$pid_before" =~ ^[1-9][0-9]*$ ]] || pp_fail "agent PID missing before PKI upload on $side" "$side"
    if [[ "$side" == abroad ]]; then pid_before_abroad="$pid_before"; else pid_before_iran="$pid_before"; fi
  done

  echo "--- D2: isolated offline CA signs both CSRs; CA key stays in container tmpfs ---"
  local image="baft-pki-offline:${GITHUB_RUN_ID:-local}"
  docker build --quiet -f "$PWD/lab/real-hosts/Dockerfile.offline-ca" -t "$image" "$PWD/lab/real-hosts" >/dev/null
  local ca_pass
  ca_pass="$(openssl rand -hex 32)"
  printf '%s\n' "$ca_pass" | docker run --rm -i --network none --read-only \
    --tmpfs /tmp:rw,noexec,nosuid,size=64m \
    -v "$LOCAL_TMP/d2-certs:/workspace:rw" \
    -v "$PWD/scripts/pki:/pki:ro" \
    -e D2_IRAN_SAN="$LAB_IRAN_HOST" -e D2_ABROAD_SAN="$LAB_ABROAD_HOST" \
    "$image" /bin/bash -euo pipefail -c '
      umask 077
      cat > /tmp/passphrase
      chmod 600 /tmp/passphrase
      export BAFT_PKI_PASSFILE=/tmp/passphrase
      mkdir -p /tmp/ca /workspace/signed
      bash /pki/baft-pki.sh init-ca /tmp/ca "BAFT-TUNNEL D2 LAB CA" >/dev/null
      bash /pki/baft-pki.sh issue-node /tmp/ca abroad /workspace/csr/abroad.csr /workspace/signed/abroad "$D2_ABROAD_SAN" 90 >/dev/null
      bash /pki/baft-pki.sh issue-node /tmp/ca iran /workspace/csr/iran.csr /workspace/signed/iran "$D2_IRAN_SAN" 90 >/dev/null
      cp /tmp/ca/ca.crl /workspace/signed/abroad/ca.crl
      cp /tmp/ca/ca.crl /workspace/signed/iran/ca.crl
      bash /pki/baft-pki.sh verify-node /tmp/ca /tmp/ca/ca.crl /workspace/signed/abroad/node.crt abroad "$D2_ABROAD_SAN" >/dev/null
      bash /pki/baft-pki.sh verify-node /tmp/ca /tmp/ca/ca.crl /workspace/signed/iran/node.crt iran "$D2_IRAN_SAN" >/dev/null
      # These are public enrollment artifacts only. The container may create them
      # with a different uid; grant the ephemeral runner cleanup access as well.
      find /workspace/signed -type d -exec chmod 777 {} +
      find /workspace/signed -type f -exec chmod 666 {} +
      test -f /tmp/ca/ca.key
      test ! -e /workspace/ca.key
      rm -f /tmp/passphrase
    '
  unset ca_pass
  docker image rm "$image" >/dev/null 2>&1 || true
  test ! -e "$LOCAL_TMP/d2-certs/ca.key" || { echo "offline CA private key escaped its tmpfs" >&2; exit 1; }

  echo "--- D2: import only leaf certificate, CA certificate and CRL through each Agent API ---"
  for side in abroad iran; do
    if [[ "$side" == abroad ]]; then node=abroad; san="$LAB_ABROAD_HOST"; else node=iran; san="$LAB_IRAN_HOST"; fi
    python3 - "$LOCAL_TMP/d2-certs/install-$side.json" "$node" "$san" "$LOCAL_TMP/d2-certs/signed/$side" <<'PY'
import json, pathlib, sys
d=pathlib.Path(sys.argv[4])
json.dump({
 "request_id":"pki-install-"+sys.argv[2]+"-0001",
 "node_id":sys.argv[2],
 "server_name":sys.argv[3],
 "confirm":True,
 "certificate_pem":(d/"node.crt").read_text(),
 "ca_pem":(d/"ca.crt").read_text(),
 "crl_pem":(d/"ca.crl").read_text()
},open(sys.argv[1],"w"))
PY
    pp_api "$side" POST /v1/pki/certificate/install "$LOCAL_TMP/d2-certs/install-$side.json" > "$LOCAL_TMP/d2-certs/install-response-$side.json"
    python3 - "$LOCAL_TMP/d2-certs/install-response-$side.json" <<'PY'
import json, re, sys
d=json.load(open(sys.argv[1]))
assert d.get("result") == "installed", d
assert d.get("private_key_exported") is False, d
assert d.get("service_restarted") is False, d
assert re.fullmatch(r"[0-9a-f]{64}",d.get("active_set","")), d
assert re.fullmatch(r"[0-9a-f]{64}",d.get("certificate_sha256","")), d
assert d.get("crl_seconds_remaining",0) >= 86400, d
PY
    pid_after="$(ssh_host "$side" "systemctl show -p MainPID --value baft-tunnel-agent@$(pp_inst "$side").service")"
    if [[ "$side" == abroad ]]; then expected_pid="$pid_before_abroad"; else expected_pid="$pid_before_iran"; fi
    [[ "$pid_after" == "$expected_pid" ]] || pp_fail "certificate upload restarted the Agent on $side" "$side"
    ssh_host "$side" "! systemctl is-active --quiet baft-tunnel@$(pp_inst "$side").service && [ \"\$(stat -c '%U:%G %a' $EB/node.key)\" = 'baft-tunnel:baft-tunnel 600' ] && ! runuser -u baft-tunnel-agent -- cat $EB/node.key >/dev/null 2>&1" ||
      pp_fail "certificate import started the tunnel or exposed the local private key on $side" "$side"
    pp_api "$side" POST /v1/setup/preflight "$LOCAL_TMP/pp-plan-$side.json" > "$LOCAL_TMP/d2-certs/preflight-$side.json"
    python3 - "$LOCAL_TMP/d2-certs/preflight-$side.json" <<'PY'
import json, sys
d=json.load(open(sys.argv[1]))
c={x["id"]:x["result"] for x in d["checks"]}
assert c.get("pki_files") == "pass", c
assert d.get("overall") != "blocked", d
PY
  done
  echo "D2_TLS_ENROLLMENT_RESULT csr_create=2 csr_recovery=2 offline_ca_network_none=1 private_ca_key_exported=0 certificate_import=2 same_ca_crl=1 no_restart=2 key_custody=2 preflight_pki_pass=2"
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
  if ! python3 - "$LOCAL_TMP/pp-pf-$side.json" "$PANEL_ENGINE" <<'PY'
import json,sys
c={x["id"]:x["result"] for x in json.load(open(sys.argv[1]))["checks"]}
assert c.get("setup_helper") == "pass", c
if sys.argv[2] == "tcp-tls":
    assert c.get("pki_files") == "warning", c
PY
  then
    cat "$LOCAL_TMP/pp-pf-$side.json" >&2
    pp_fail "preflight on $side does not use the setup helper or has unexpected PKI state" "$side"
  fi
done

if [[ "$PANEL_ENGINE" == tcp-tls ]]; then
  pp_tls_enroll
fi

echo "--- init on both hosts through the agent API (root setup helper), export own bundles ---"
declare -A PP_CK
for side in abroad iran; do
  pp_body "$LOCAL_TMP/pp-init-$side.json" init "pp-init-$side-0001" "$(pp_plan "$side")"
  pp_api "$side" POST /v1/setup/apply "$LOCAL_TMP/pp-init-$side.json" > "$LOCAL_TMP/pp-init-out-$side.json"
  [[ "$(jf result < "$LOCAL_TMP/pp-init-out-$side.json")" == applied && "$(jf source < "$LOCAL_TMP/pp-init-out-$side.json")" == "root setup helper" ]] ||
    { cat "$LOCAL_TMP/pp-init-out-$side.json" >&2; pp_fail "init through the agent on $side was not applied by the root helper" "$side"; }
  if [[ "$PANEL_ENGINE" == tcp-tls ]]; then
    ssh_host "$side" "[ \"\$(stat -c '%U:%G %a' $EB/node.key)\" = 'baft-tunnel:baft-tunnel 600' ] && ! runuser -u baft-tunnel-agent -- cat $EB/node.key >/dev/null 2>&1" ||
      pp_fail "node key on $side has the wrong owner/mode or is readable by the agent user" "$side"
  else
  ssh_host "$side" "[ \"\$(stat -c '%U:%G %a' $EB/node.noise)\" = 'baft-tunnel:baft-tunnel 600' ] && ! runuser -u baft-tunnel-agent -- cat $EB/node.noise >/dev/null 2>&1" ||
    pp_fail "node key on $side has the wrong owner/mode or is readable by the agent user" "$side"
  fi
  pp_api "$side" GET /v1/setup/bundle > "$LOCAL_TMP/pp-own-$side.json"
  PP_CK[$side]="$(jf checksum < "$LOCAL_TMP/pp-own-$side.json")"
  [[ "${PP_CK[$side]}" =~ ^[0-9a-f]{8}(-[0-9a-f]{8}){3}$ && "${PP_CK[$side]}" == "$(jf pairing.checksum < "$LOCAL_TMP/pp-init-out-$side.json")" ]] ||
    pp_fail "exported checksum on $side is missing or differs from the init receipt" "$side"
  python3 -c 'import json,sys; open(sys.argv[2],"w").write(json.load(open(sys.argv[1]))["bundle"])' "$LOCAL_TMP/pp-own-$side.json" "$LOCAL_TMP/pp-bundle-$side.json"
  copy_to_host "$side" "$LOCAL_TMP/pp-bundle-$side.json" ||
    pp_fail "could not stage public bundle for key-exposure check on $side" "$side"
  if [[ "$PANEL_ENGINE" == tcp-tls ]]; then
    ssh_host "$side" "! grep -q 'PRIVATE KEY' '$REMOTE_DIR/pp-bundle-$side.json'; rc=\$?; rm -f '$REMOTE_DIR/pp-bundle-$side.json'; exit \$rc" ||
      pp_fail "the exported bundle on $side contains private-key PEM" "$side"
  else
    ssh_host "$side" "k=\$(tr -d '[:space:]' < $EB/node.noise); n=\$(printf '%s' \"\$k\" | wc -c); r=1; [ \"\$n\" -ge 16 ] && ! grep -qF -- \"\$k\" '$REMOTE_DIR/pp-bundle-$side.json' && r=0; rm -f '$REMOTE_DIR/pp-bundle-$side.json'; exit \$r" ||
      pp_fail "the exported bundle on $side contains key material" "$side"
  fi
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
  if [[ "$PANEL_ENGINE" == tcp-tls ]]; then
    ssh_host "$side" "grep -q 'pki/current/node.crt' $EB/$(pp_inst "$side").json && grep -q 'pki/current/ca.crt' $EB/$(pp_inst "$side").json" ||
      pp_fail "tcp-tls config on $side does not use the active versioned credential set after join" "$side"
    ssh_host "$side" "openssl verify -purpose any -CAfile $EB/pki/current/ca.crt -CRLfile $EB/pki/current/ca.crl -crl_check $EB/pki/current/node.crt >/dev/null" ||
      pp_fail "the active local certificate on $side does not verify against its active CA/CRL" "$side"
  fi
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
echo "PANEL_PAIRING_RESULT engine=$PANEL_ENGINE source=$MVP_SOURCE binary_sha256=$PP_SHA clean_hosts=1 setup_mode_after_install=1 agent_user=baft-tunnel-agent preflight_setup_helper=1 pki_enrollment=$([[ "$PANEL_ENGINE" == tcp-tls ]] && echo 1 || echo 0) init_via_agent=2 key_not_agent_readable=1 export_checksum=1 bundle_key_free=1 wrong_checksum_flagged=1 wrong_checksum_join_refused=1 join_via_agent=2 agents_reloaded=2 root_setup_commands_by_us=0 start_via_agent=2 data_s=$PP_DATA_S integrity_64k_2m=1 audit_setup=1 helper_receipts=1 state_secret_free=1 uninstalled=1 users_removed=1 baft_fw_rules=0 host_drift=$PP_DRIFT"
