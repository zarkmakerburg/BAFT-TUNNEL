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
BIN="$STAGE/usr/local/bin/baft-tunnel"
UNIT="$STAGE/etc/systemd/system/baft-tunnel@.service"
[[ "$("$BIN" --version)" == "baft-tunnel 0.1.0" ]]
[[ "$(mode_of "$BIN")" == "755" ]]
[[ "$(mode_of "$UNIT")" == "644" ]]
[[ "$(mode_of "$STAGE/etc/baft-tunnel")" == "750" ]]
grep -Fq 'NoNewPrivileges=true' "$UNIT"
grep -Fq 'ProtectSystem=strict' "$UNIT"
grep -Fq 'CapabilityBoundingSet=' "$UNIT"
grep -Fq 'ExecStartPre=' "$UNIT"

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
"$INSTALL" --binary "$FAKE" --destdir "$STAGE" >/dev/null
if "$UNINSTALL" --destdir "$STAGE" --purge-config >/dev/null 2>&1; then
  echo "uninstall accepted config purge without --yes" >&2
  exit 1
fi
"$UNINSTALL" --destdir "$STAGE" --purge-config --yes >/dev/null
[[ ! -e "$STAGE/etc/baft-tunnel" && ! -e "$STAGE/var/lib/baft-tunnel" ]]
[[ ! -e "$DROPIN" && ! -e "$(dirname "$DROPIN")" ]]
[[ ! -e "$STAGE/usr/local/bin/baft-tunnel" && ! -e "$STAGE/etc/systemd/system/baft-tunnel@.service" ]]

echo "BAFT_INSTALL_TEST=PASS (staged install, systemd hardening, rollback, baseline credential provisioning/replace/rollback, fail-closed credential checks, safe uninstall, explicit config purge)"
