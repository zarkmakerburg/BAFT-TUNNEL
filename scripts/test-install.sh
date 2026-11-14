#!/usr/bin/env bash
set -Eeuo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd -P)"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/baft-install-test.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT
INSTALL="$ROOT/scripts/install.sh"
ROLLBACK="$ROOT/scripts/rollback.sh"
UNINSTALL="$ROOT/scripts/uninstall.sh"
STAGE="$TMP/root"
FAKE="$TMP/baft-tunnel"

write_fake() {
  local version="$1"
  cat > "$FAKE" <<EOF
#!/usr/bin/env sh
[ "\${1:-}" = "--version" ] && { echo "baft-tunnel $version"; exit 0; }
exit 0
EOF
  chmod 755 "$FAKE"
}
mode_of() {
  if stat -c '%a' "$1" >/dev/null 2>&1; then stat -c '%a' "$1"; else stat -f '%Lp' "$1"; fi
}

write_fake 0.1.0
"$INSTALL" --binary "$FAKE" --destdir "$STAGE" >/dev/null
[[ -x "$STAGE/usr/local/lib/baft-tunnel/scripts/quick-install.sh" ]]
[[ -f "$STAGE/usr/local/lib/baft-tunnel/scripts/migrate-agent-control-dir.sh" ]]
BIN="$STAGE/usr/local/bin/baft-tunnel"
UNIT="$STAGE/etc/systemd/system/baft-tunnel@.service"
[[ "$("$BIN" --version)" == "baft-tunnel 0.1.0" ]]
[[ "$(mode_of "$BIN")" == "755" ]]
[[ "$(mode_of "$UNIT")" == "644" ]]
[[ "$(mode_of "$STAGE/etc/baft-tunnel")" == "750" ]]
grep -Fq 'NoNewPrivileges=true' "$UNIT"
grep -Fq 'ProtectSystem=strict' "$UNIT"
grep -Fq 'CapabilityBoundingSet=' "$UNIT"
grep -Fq 'apply --config' "$UNIT"

mkdir -p "$STAGE/etc/baft-tunnel/pki" "$STAGE/var/lib/baft-tunnel"
printf 'keep-config\n' > "$STAGE/etc/baft-tunnel/iran.json"
printf 'keep-state\n' > "$STAGE/var/lib/baft-tunnel/state"
write_fake 0.2.0
"$INSTALL" --binary "$FAKE" --destdir "$STAGE" >/dev/null
BACKUP="$(ls -td "$STAGE"/var/backups/baft-tunnel-install.* | head -n 1)"
[[ -f "$BACKUP/had-binary" && -f "$BACKUP/had-unit" ]]
[[ "$("$BACKUP/baft-tunnel" --version)" == "baft-tunnel 0.1.0" ]]
[[ "$("$BIN" --version)" == "baft-tunnel 0.2.0" ]]
"$ROLLBACK" --destdir "$STAGE" "$BACKUP" >/dev/null
[[ "$("$BIN" --version)" == "baft-tunnel 0.1.0" ]]

# --- Baseline credential provisioning (LoadCredential=, systemd 249) ---
fails() { if "$@" >"$TMP/fail.log" 2>&1; then echo "unexpected success: $*" >&2; exit 1; fi; }
KEY1="$TMP/key1.pem"; KEY2="$TMP/key2.pem"
openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-256 -out "$KEY1" 2>/dev/null
openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-256 -out "$KEY2" 2>/dev/null
chmod 600 "$KEY1" "$KEY2"
CRED_ROOT="$STAGE/etc/baft-tunnel/credentials"
CRED_DIR="$CRED_ROOT/edge"
DROPIN="$STAGE/etc/systemd/system/baft-tunnel@edge.service.d/credentials.conf"
BIN_SUM="$(cksum < "$BIN")"

fails "$INSTALL" --instance edge --node-key "$KEY1" --destdir "$STAGE" --systemd-version 246
fails "$INSTALL" --instance edge --node-key "$KEY1" --destdir "$STAGE" --systemd-version 249 --credential-mode encrypted
grep -q 'no automatic fallback' "$TMP/fail.log"
fails "$INSTALL" --instance edge --node-key "$KEY1" --destdir "$STAGE" --systemd-version 255 --credential-mode encrypted
fails "$INSTALL" --instance ../edge --node-key "$KEY1" --destdir "$STAGE" --systemd-version 249
fails "$INSTALL" --instance edge --destdir "$STAGE" --systemd-version 249
ln -s "$KEY1" "$TMP/key-link.pem"
fails "$INSTALL" --instance edge --node-key "$TMP/key-link.pem" --destdir "$STAGE" --systemd-version 249
printf 'not a key\n' > "$TMP/not-key.pem"
fails "$INSTALL" --instance edge --node-key "$TMP/not-key.pem" --destdir "$STAGE" --systemd-version 249
fails "$INSTALL" --systemd-version 249 --binary "$FAKE"
printf '{"key":"/etc/baft-tunnel/pki/edge.key"}\n' > "$STAGE/etc/baft-tunnel/edge.json"
fails "$INSTALL" --instance edge --node-key "$KEY1" --destdir "$STAGE" --systemd-version 249
[[ ! -e "$CRED_DIR" && ! -e "$DROPIN" ]]
printf '{"key_credential":"node-key"}\n' > "$STAGE/etc/baft-tunnel/edge.json"

"$INSTALL" --instance edge --node-key "$KEY1" --destdir "$STAGE" --systemd-version 249 > "$TMP/cred.log"
[[ "$(mode_of "$CRED_ROOT")" == "700" && "$(mode_of "$CRED_DIR")" == "700" ]]
[[ "$(mode_of "$CRED_DIR/node-key")" == "600" ]]
cmp -s "$KEY1" "$CRED_DIR/node-key"
[[ "$(cksum < "$BIN")" == "$BIN_SUM" ]]
grep -Fqx 'LoadCredential=node-key:/etc/baft-tunnel/credentials/edge/node-key' "$DROPIN"
grep -Fqx '# Managed by BAFT-TUNNEL scripts/install.sh' "$DROPIN"
if grep -q 'PRIVATE KEY' "$DROPIN" "$TMP/cred.log" "$STAGE/etc/baft-tunnel/edge.json"; then
  echo "key material leaked into a drop-in, log, or config" >&2; exit 1
fi
CRED_BACKUP1="$(sed -n 's/^Backup: //p' "$TMP/cred.log")"
[[ ! -e "$CRED_BACKUP1/scope-binary" && -f "$CRED_BACKUP1/scope-credential" ]]

# Idempotent re-run; a different key needs --replace-credential.
"$INSTALL" --instance edge --node-key "$KEY1" --destdir "$STAGE" --systemd-version 249 > "$TMP/cred-same.log"
grep -q unchanged "$TMP/cred-same.log"
fails "$INSTALL" --instance edge --node-key "$KEY2" --destdir "$STAGE" --systemd-version 249
cmp -s "$KEY1" "$CRED_DIR/node-key"
"$INSTALL" --instance edge --node-key "$KEY2" --replace-credential --destdir "$STAGE" --systemd-version 249 > "$TMP/cred2.log"
cmp -s "$KEY2" "$CRED_DIR/node-key"
CRED_BACKUP2="$(sed -n 's/^Backup: //p' "$TMP/cred2.log")"
[[ "$(mode_of "$CRED_BACKUP2")" == "700" && "$(mode_of "$CRED_BACKUP2/credential/node-key")" == "600" ]]
"$ROLLBACK" --destdir "$STAGE" "$CRED_BACKUP2" >/dev/null
cmp -s "$KEY1" "$CRED_DIR/node-key"
[[ "$(mode_of "$CRED_DIR/node-key")" == "600" ]]
[[ "$(cksum < "$BIN")" == "$BIN_SUM" ]]
# Rolling back the first provisioning removes the credential and drop-in only.
"$ROLLBACK" --destdir "$STAGE" "$CRED_BACKUP1" >/dev/null
[[ ! -e "$CRED_DIR/node-key" && ! -e "$DROPIN" && -x "$BIN" ]]
"$INSTALL" --instance edge --node-key "$KEY1" --destdir "$STAGE" --systemd-version 249 >/dev/null

# Binary upgrade with an instance keeps the credential; uninstall keeps both.
"$INSTALL" --binary "$FAKE" --destdir "$STAGE" >/dev/null
cmp -s "$KEY1" "$CRED_DIR/node-key"
"$UNINSTALL" --destdir "$STAGE" >/dev/null
[[ ! -e "$BIN" && ! -e "$UNIT" ]]
[[ -f "$STAGE/etc/baft-tunnel/iran.json" && -f "$STAGE/var/lib/baft-tunnel/state" ]]
cmp -s "$KEY1" "$CRED_DIR/node-key"
[[ -f "$DROPIN" ]]

write_fake 0.3.0
# One step: binary plus agent service and control helper for an instance (--register).
"$INSTALL" --binary "$FAKE" --register iran --agent-listen 127.0.0.1:18085 --destdir "$STAGE" >/dev/null
for v in start stop enable disable setup pki; do
  [[ -f "$STAGE/etc/systemd/system/baft-tunnel-control-$v@.path" && -f "$STAGE/etc/systemd/system/baft-tunnel-control-$v@.service" ]]
done
grep -qx 'ExecStart=/usr/local/bin/baft-tunnel setup helper --instance %i' "$STAGE/etc/systemd/system/baft-tunnel-control-setup@.service"
grep -qx 'ExecStart=/usr/local/bin/baft-tunnel setup pki-helper --instance %i' "$STAGE/etc/systemd/system/baft-tunnel-control-pki@.service"
grep -qx 'PathExists=/run/baft-tunnel/control/%i.pki' "$STAGE/etc/systemd/system/baft-tunnel-control-pki@.path"
grep -qx 'TimeoutStartSec=60s' "$STAGE/etc/systemd/system/baft-tunnel-control-pki@.service"
grep -qx 'ReadWritePaths=-/etc/baft-tunnel -/var/lib/baft-tunnel -/var/lib/baft-tunnel-control /run/baft-tunnel/control' "$STAGE/etc/systemd/system/baft-tunnel-control-pki@.service"
grep -qx 'ReadWritePaths=-/etc/baft-tunnel -/var/lib/baft-tunnel -/var/lib/baft-tunnel-control /run/baft-tunnel/control' "$STAGE/etc/systemd/system/baft-tunnel-control-setup@.service"
grep -qx 'CapabilityBoundingSet=CAP_CHOWN CAP_FOWNER CAP_DAC_OVERRIDE' "$STAGE/etc/systemd/system/baft-tunnel-control-pki@.service"
grep -qx 'ExecStart=/usr/bin/systemctl --no-block enable --now baft-tunnel@%i.service' "$STAGE/etc/systemd/system/baft-tunnel-control-enable@.service"
AGENT_UNIT="$STAGE/etc/systemd/system/baft-tunnel-agent@.service"
grep -qx 'User=baft-tunnel-agent' "$AGENT_UNIT" && ! grep -q '^User=baft-tunnel$' "$AGENT_UNIT"
grep -qx 'SupplementaryGroups=baft-tunnel' "$AGENT_UNIT" && grep -qx 'NoNewPrivileges=true' "$AGENT_UNIT"
grep -q -- '--listen ${BAFT_AGENT_LISTEN}' "$AGENT_UNIT" && grep -q -- '--instance %i' "$AGENT_UNIT"
grep -qx 'Environment=BAFT_AGENT_LISTEN=127.0.0.1:18085' "$STAGE/etc/systemd/system/baft-tunnel-agent@iran.service.d/listen.conf"
[[ -f "$STAGE/etc/systemd/system/baft-tunnel-agent@.path" ]]
grep -qx 'RuntimeDirectoryMode=0750' "$UNIT" && grep -q 'InaccessiblePaths=.*/var/lib/baft-tunnel-agent' "$UNIT"
grep -qx 'Environment=BAFT_TUNNEL_CONTROL_SOCKET=/run/baft-tunnel/%i/control.sock' "$UNIT"
grep -q 'restart it once to enable integrity probes' "$ROOT/scripts/install-agent-control.sh"
# The agent never listens off loopback, and a bad name or address is refused.
refused() { if "$@" >/dev/null 2>&1; then echo "accepted: $*" >&2; exit 1; fi; }
refused "$ROOT/scripts/install-agent-control.sh" --instance iran --listen 0.0.0.0:18080 --destdir "$STAGE"
refused "$INSTALL" --binary "$FAKE" --register 'a.b' --destdir "$STAGE"
refused "$INSTALL" --binary "$FAKE" --agent-listen 127.0.0.1:1 --destdir "$STAGE"
mkdir -p "$STAGE/etc/systemd/system/multi-user.target.wants" "$STAGE/run/baft-tunnel/control"
ln -sf ../baft-tunnel-control-start@.path "$STAGE/etc/systemd/system/multi-user.target.wants/baft-tunnel-control-start@iran.path"
ln -sf ../baft-tunnel-agent@.path "$STAGE/etc/systemd/system/multi-user.target.wants/baft-tunnel-agent@iran.path"
[[ -f "$STAGE/etc/systemd/system/baft-tunnel-control-start@.path" && -f "$STAGE/etc/tmpfiles.d/baft-tunnel-control.conf" ]]
grep -qx 'd /var/lib/baft-tunnel-control 0700 root root -' "$STAGE/etc/tmpfiles.d/baft-tunnel-control.conf"
# The tunnel engine user must not share the group that can submit root control requests.
grep -qx 'd /run/baft-tunnel/control 0770 root baft-tunnel-agent -' "$STAGE/etc/tmpfiles.d/baft-tunnel-control.conf"
! grep -qx 'd /run/baft-tunnel/control 0770 root baft-tunnel -' "$STAGE/etc/tmpfiles.d/baft-tunnel-control.conf" || false
grep -Fq 'active_units path' "$ROOT/scripts/migrate-agent-control-dir.sh"
grep -Fq 'active_units service' "$ROOT/scripts/migrate-agent-control-dir.sh"
grep -Fq 'for request in "$CONTROL_DIR"/*."$verb"; do' "$ROOT/scripts/migrate-agent-control-dir.sh"
grep -Fq 'root control helpers remain active' "$ROOT/scripts/migrate-agent-control-dir.sh"
grep -Fq 'pending control request exists' "$ROOT/scripts/migrate-agent-control-dir.sh"
# The fail-closed helper runs on every registration/upgrade, never skipped for an already-safe directory.
grep -Fq '  bash "$SCRIPT_DIR/migrate-agent-control-dir.sh" --control-dir "$CONTROL_DIR" --tmpfiles "$TMPF"' "$ROOT/scripts/install-agent-control.sh"
! grep -Fq 'systemd-tmpfiles --create "$TMPF"' "$ROOT/scripts/install-agent-control.sh" || false
grep -Fq 'request appeared during directory migration' "$ROOT/scripts/migrate-agent-control-dir.sh"
grep -Fq 'group $AGENT_USER must be exclusive to the dedicated agent account' "$ROOT/scripts/install-agent-control.sh"
grep -Fq 'id -gn "$AGENT_USER"' "$ROOT/scripts/install-agent-control.sh"
grep -Fq 'getent passwd | awk -F: -v gid="$AGENT_GID"' "$ROOT/scripts/install-agent-control.sh"
grep -Fq 'OTHER_PRIMARY_USERS' "$ROOT/scripts/install-agent-control.sh"
grep -Fq 'pending control request exists' "$ROOT/scripts/migrate-agent-control-dir.sh"
grep -Fq 'request appeared during directory migration' "$ROOT/scripts/migrate-agent-control-dir.sh"
grep -Fq 'root:baft-tunnel-agent 770' "$ROOT/scripts/install-agent-control.sh"
# update.sh upgrade path (H-01): no control helper means nothing to migrate; an installed one in a
# staged tree only refreshes templates and is reported as not verified, never as migrated.
EMPTY_STAGE="$TMP/empty-stage"; mkdir -p "$EMPTY_STAGE"
[[ "$("$ROOT/scripts/install-agent-control.sh" --upgrade-boundary --destdir "$EMPTY_STAGE")" == 'CONTROL_BOUNDARY=not-installed' ]]
mkdir -p "$EMPTY_STAGE/etc/tmpfiles.d"; printf 'd /run/baft-tunnel/control 0770 root baft-tunnel -\n' > "$EMPTY_STAGE/etc/tmpfiles.d/baft-tunnel-control.conf"
[[ "$("$ROOT/scripts/install-agent-control.sh" --upgrade-boundary --destdir "$EMPTY_STAGE")" == 'CONTROL_BOUNDARY=staged-not-verified' ]]
grep -qx 'd /run/baft-tunnel/control 0770 root baft-tunnel-agent -' "$EMPTY_STAGE/etc/tmpfiles.d/baft-tunnel-control.conf"
refused "$ROOT/scripts/install-agent-control.sh" --upgrade-boundary --instance iran --destdir "$EMPTY_STAGE"
grep -Fq 'install-agent-control.sh" --upgrade-boundary' "$ROOT/scripts/update.sh"
grep -Fq 'control_boundary=$CONTROL_BOUNDARY' "$ROOT/scripts/update.sh"
if "$UNINSTALL" --destdir "$STAGE" --purge-config >/dev/null 2>&1; then
  echo "uninstall accepted config purge without --yes" >&2
  exit 1
fi
"$UNINSTALL" --destdir "$STAGE" --purge-config --yes >/dev/null
[[ ! -e "$STAGE/etc/baft-tunnel" && ! -e "$STAGE/var/lib/baft-tunnel" && ! -e "$STAGE/var/lib/baft-tunnel-control" ]]
[[ ! -e "$DROPIN" && ! -e "$(dirname "$DROPIN")" ]]
[[ ! -e "$STAGE/usr/local/bin/baft-tunnel" && ! -e "$STAGE/etc/systemd/system/baft-tunnel@.service" ]]
[[ -z "$(compgen -G "$STAGE/etc/systemd/system/baft-tunnel-control-*" || true)" ]]
[[ ! -e "$STAGE/etc/tmpfiles.d/baft-tunnel-control.conf" && ! -e "$STAGE/run/baft-tunnel/control" ]]
[[ ! -L "$STAGE/etc/systemd/system/multi-user.target.wants/baft-tunnel-control-start@iran.path" ]]
[[ -z "$(compgen -G "$STAGE/etc/systemd/system/baft-tunnel-agent@*" || true)" ]]
[[ ! -L "$STAGE/etc/systemd/system/multi-user.target.wants/baft-tunnel-agent@iran.path" && ! -e "$STAGE/var/lib/baft-tunnel-agent" ]]

# Engine-store pointer rollback after engine install succeeds but verification fails.
# This fake CLI exercises install.sh's transaction/rollback shell path without a production failpoint.
ENGINE_FAKE="$TMP/engine-fake"
cat > "$ENGINE_FAKE" <<'EOF_ENGINE_FAKE'
#!/usr/bin/env bash
set -Eeuo pipefail
if [[ "\${1:-}" == "--version" ]]; then echo "baft-tunnel engine-test"; exit 0; fi
if [[ "\${1:-}" == "engine" && "\${2:-}" == "manifest" ]]; then
  printf '{"engines":[{"id":"baft"]}\n'
  exit 0
fi
if [[ "\${1:-}" == "engine" && "\${2:-}" == "install" ]]; then
  shift 2
  store=""; asset=""
  while (($#)); do
    case "$1" in
      --store) store="$2"; shift 2 ;;
      --binary) asset="$2"; shift 2 ;;
      *) shift ;;
    esac
  done
  [[ -n "$store" && -n "$asset" ]]
  digest="$(sha256sum "$asset" | awk '{print $1}')"
  engine_dir="$store/baft"
  mkdir -p "$engine_dir/releases/$digest"
  cp "$asset" "$engine_dir/releases/$digest/engine"
  if [[ -L "$engine_dir/current" ]]; then
    old="$(readlink "$engine_dir/current")"
    ln -sfn "$old" "$engine_dir/previous"
  fi
  ln -sfn "releases/$digest" "$engine_dir/current"
  exit 0
fi
if [[ "\${1:-}" == "engine" && "\${2:-}" == "verify" ]]; then
  [[ "\${BAFT_TEST_FAIL_VERIFY:-0}" != "1" ]] || exit 42
  exit 0
fi
echo "unexpected fake CLI invocation: $*" >&2
exit 64
EOF_ENGINE_FAKE
chmod 755 "$ENGINE_FAKE"
ENGINE_FAIL_STAGE="$TMP/engine-fail-stage"
"$INSTALL" --binary "$ENGINE_FAKE" --destdir "$ENGINE_FAIL_STAGE" >/dev/null
ENGINE_DIR="$ENGINE_FAIL_STAGE/usr/local/lib/baft-tunnel-engines/baft"
OLD_CURRENT="aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
OLD_PREVIOUS="bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
mkdir -p "$ENGINE_DIR/releases/$OLD_CURRENT" "$ENGINE_DIR/releases/$OLD_PREVIOUS"
printf 'old-current\n' > "$ENGINE_DIR/releases/$OLD_CURRENT/engine"
printf 'old-previous\n' > "$ENGINE_DIR/releases/$OLD_PREVIOUS/engine"
ln -s "releases/$OLD_CURRENT" "$ENGINE_DIR/current"
ln -s "releases/$OLD_PREVIOUS" "$ENGINE_DIR/previous"
printf 'candidate-engine\n' > "$TMP/engine-candidate"
if BAFT_TEST_FAIL_VERIFY=1 "$INSTALL" --binary "$ENGINE_FAKE" --baft-engine "$TMP/engine-candidate" --destdir "$ENGINE_FAIL_STAGE" >"$TMP/engine-fail.log" 2>&1; then
  echo "engine verification failure unexpectedly succeeded" >&2
  exit 1
fi
[[ "$(readlink "$ENGINE_DIR/current")" == "releases/$OLD_CURRENT" ]]
[[ "$(readlink "$ENGINE_DIR/previous")" == "releases/$OLD_PREVIOUS" ]]
[[ -f "$ENGINE_DIR/releases/$OLD_CURRENT/engine" && -f "$ENGINE_DIR/releases/$OLD_PREVIOUS/engine" ]]
[[ "$(grep -c 'previous files restored' "$TMP/engine-fail.log")" == "1" ]]

# --baft-engine: needs --binary, and a binary whose digest is not the built-in pin is refused
# with the previous install restored and no engine made current.
if bash "$INSTALL" --baft-engine "$FAKE" --destdir "$TMP/engine-only" >/dev/null 2>&1; then
  echo "--baft-engine without --binary was accepted" >&2; exit 1
fi
REAL_BIN="${BAFT_TUNNEL_TEST_BINARY:-}"
if [[ -z "$REAL_BIN" ]]; then
  for c in "$ROOT/target/release/baft-tunnel" "$ROOT/target/debug/baft-tunnel"; do [[ -x "$c" ]] && { REAL_BIN="$c"; break; }; done
fi
ENGINE_TEST=skipped
if [[ -n "$REAL_BIN" ]]; then
  ES="$TMP/engine-stage"
  "$INSTALL" --binary "$REAL_BIN" --destdir "$ES" >/dev/null
  cmp -s "$REAL_BIN" "$ES/usr/local/bin/baft-tunnel"
  "$ES/usr/local/bin/baft-tunnel" engine manifest | python3 -c 'import json,sys; m=json.load(sys.stdin); assert [e for e in m["engines"] if e["id"]=="baft"]'
  printf 'not-the-pinned-engine\n' > "$TMP/fake-engine"
  if "$INSTALL" --binary "$REAL_BIN" --baft-engine "$TMP/fake-engine" --destdir "$ES" >/dev/null 2>&1; then
    echo "an engine with the wrong digest was installed" >&2; exit 1
  fi
  cmp -s "$REAL_BIN" "$ES/usr/local/bin/baft-tunnel" || { echo "failed engine install did not restore the binary" >&2; exit 1; }
  [[ ! -e "$ES/usr/local/lib/baft-tunnel-engines/baft/current" ]] || { echo "refused engine became current" >&2; exit 1; }
  "$UNINSTALL" --destdir "$ES" >/dev/null
  [[ ! -e "$ES/usr/local/lib/baft-tunnel-engines" ]] || { echo "uninstall left the engine store" >&2; exit 1; }
  ENGINE_TEST=pass
fi
echo "BAFT_INSTALL_ENGINE_STORE=$ENGINE_TEST"

echo "BAFT_INSTALL_TEST=PASS (staged install, systemd hardening, rollback, baseline credential provisioning/replace/rollback, fail-closed credential checks, safe uninstall, explicit config purge, one-step agent registration, CSR helper unit install, agent and control helper removed)"
