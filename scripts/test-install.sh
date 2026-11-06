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

"$UNINSTALL" --destdir "$STAGE"
[[ ! -e "$BIN" && ! -e "$UNIT" ]]
[[ -f "$STAGE/etc/baft-tunnel/iran.json" && -f "$STAGE/var/lib/baft-tunnel/state" ]]

write_fake 0.3.0
"$INSTALL" --binary "$FAKE" --destdir "$STAGE" >/dev/null
if "$UNINSTALL" --destdir "$STAGE" --purge-config >/dev/null 2>&1; then
  echo "uninstall accepted config purge without --yes" >&2
  exit 1
fi
"$UNINSTALL" --destdir "$STAGE" --purge-config --yes >/dev/null
[[ ! -e "$STAGE/etc/baft-tunnel" && ! -e "$STAGE/var/lib/baft-tunnel" ]]
[[ ! -e "$STAGE/usr/local/bin/baft-tunnel" && ! -e "$STAGE/etc/systemd/system/baft-tunnel@.service" ]]

echo "BAFT_INSTALL_TEST=PASS (staged install, systemd hardening, rollback, safe uninstall, explicit config purge)"
