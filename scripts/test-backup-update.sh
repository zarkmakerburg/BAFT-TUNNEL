#!/usr/bin/env bash
# Staged tests for backup.sh, restore.sh and update.sh (ADR-0025). No systemd, no network, no real keys.
set -Eeuo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd -P)"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/baft-backup-test.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT
S="$ROOT/scripts"
STAGE="$TMP/root"
mode_of() { stat -c '%a' "$1"; }
fails() { if "$@" >"$TMP/fail.log" 2>&1; then echo "unexpected success: $*" >&2; exit 1; fi; }
fake() { # fake VERSION [health-fail]
  cat > "$TMP/baft-tunnel" <<EOT
#!/usr/bin/env sh
[ "\${1:-}" = "--version" ] && { echo "baft-tunnel $1"; exit 0; }
exit 0
EOT
  chmod 755 "$TMP/baft-tunnel"
}
cp_bin() { cp "$TMP/baft-tunnel" "$TMP/baft-tunnel.$1"; }

KEY="$TMP/key.pem"
openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-256 -out "$KEY" 2>/dev/null
chmod 600 "$KEY"
fake 0.1.0; cp_bin v1; fake 0.2.0; cp_bin v2
"$S/install.sh" --binary "$TMP/baft-tunnel.v1" --destdir "$STAGE" >/dev/null
"$S/install.sh" --instance edge --node-key "$KEY" --destdir "$STAGE" --systemd-version 249 >/dev/null
mkdir -p "$STAGE/etc/baft-tunnel/pki/edge" "$STAGE/var/lib/baft-tunnel"
printf '{"key_credential":"node-key","mode":"forward"}\n' > "$STAGE/etc/baft-tunnel/edge.json"
printf 'ca-cert\n' > "$STAGE/etc/baft-tunnel/pki/edge/ca.crt"
printf 'state-1\n' > "$STAGE/var/lib/baft-tunnel/state"

# --- backup: permissions, checksum, manifest without contents ---
"$S/backup.sh" --destdir "$STAGE" > "$TMP/backup.log"
B1="$(sed -n 's/^Backup: //p' "$TMP/backup.log")"
[[ "$(mode_of "$B1")" == "700" && "$(mode_of "$B1/backup.tar")" == "600" && "$(mode_of "$B1/backup.tar.sha256")" == "600" && "$(mode_of "$B1/manifest.txt")" == "600" ]]
(cd "$B1" && sha256sum --check --status backup.tar.sha256)
grep -q 'etc/baft-tunnel/credentials/edge/node-key' "$B1/manifest.txt"
if grep -q 'PRIVATE KEY' "$B1/manifest.txt" "$TMP/backup.log"; then echo "key material in manifest/log" >&2; exit 1; fi
[[ ! -e "$STAGE/usr/local/bin/baft-tunnel.bak" ]]
! tar -tf "$B1/backup.tar" | grep -q 'usr/local/bin'

# --- restore: drift, then restore, then byte-identical ---
digest() { (cd "$STAGE" && find etc/baft-tunnel var/lib/baft-tunnel etc/systemd/system/baft-tunnel@edge.service.d -type f -print0 | LC_ALL=C sort -z | xargs -0 sha256sum | sha256sum | awk '{print $1}'); }
D0="$(digest)"
rm -f "$STAGE/etc/baft-tunnel/edge.json" "$STAGE/etc/baft-tunnel/credentials/edge/node-key"
printf 'bad\n' > "$STAGE/var/lib/baft-tunnel/state"
rm -rf "$STAGE/etc/systemd/system/baft-tunnel@edge.service.d"
printf 'x\n' > "$STAGE/etc/baft-tunnel/stray-file"
fails "$S/restore.sh" --destdir "$STAGE" "$B1"           # --yes required
"$S/restore.sh" --destdir "$STAGE" --yes "$B1" > "$TMP/restore.log"
[[ "$(digest)" == "$D0" ]]
[[ ! -e "$STAGE/etc/baft-tunnel/stray-file" ]]
cmp -s "$KEY" "$STAGE/etc/baft-tunnel/credentials/edge/node-key"
[[ "$(mode_of "$STAGE/etc/baft-tunnel/credentials")" == "700" && "$(mode_of "$STAGE/etc/baft-tunnel/credentials/edge")" == "700" && "$(mode_of "$STAGE/etc/baft-tunnel/credentials/edge/node-key")" == "600" ]]
grep -q 'No service was started' "$TMP/restore.log"
if grep -q 'PRIVATE KEY' "$TMP/restore.log"; then echo "key material in restore log" >&2; exit 1; fi
SNAP="$(sed -n 's/^Pre-restore snapshot: \([^ ]*\) .*/\1/p' "$TMP/restore.log")"
[[ -d "$SNAP" && "$(mode_of "$SNAP")" == "700" ]]
tar -tf "$SNAP/backup.tar" | grep -q 'stray-file'        # the pre-restore state is recoverable

# --- restore refusals: tampered archive, links, outside paths ---
cp -a "$B1" "$TMP/tampered"; printf 'x' >> "$TMP/tampered/backup.tar"
fails "$S/restore.sh" --destdir "$STAGE" --yes "$TMP/tampered"; grep -q checksum "$TMP/fail.log"
mk_bad() { # mk_bad NAME  (reads a prepared tree in $TMP/evil)
  mkdir -p "$TMP/$1"; chmod 700 "$TMP/$1"
  tar -C "$TMP/evil" -cf "$TMP/$1/backup.tar" .
  (cd "$TMP/$1" && sha256sum backup.tar > backup.tar.sha256)
}
rm -rf "$TMP/evil"; mkdir -p "$TMP/evil/etc/baft-tunnel"; ln -s /etc/passwd "$TMP/evil/etc/baft-tunnel/link"
(cd "$TMP/evil" && rm -rf ../evil2 && mkdir ../evil2 && tar -cf ../evil2/backup.tar etc/baft-tunnel && cd ../evil2 && sha256sum backup.tar > backup.tar.sha256)
fails "$S/restore.sh" --destdir "$STAGE" --yes "$TMP/evil2"; grep -q 'link' "$TMP/fail.log"
rm -rf "$TMP/evil" "$TMP/evil3"; mkdir -p "$TMP/evil/etc/baft-tunnel" "$TMP/evil/etc/cron.d"; echo x > "$TMP/evil/etc/cron.d/job"
mkdir "$TMP/evil3"; (cd "$TMP/evil" && tar -cf ../evil3/backup.tar etc/baft-tunnel etc/cron.d) && (cd "$TMP/evil3" && sha256sum backup.tar > backup.tar.sha256)
fails "$S/restore.sh" --destdir "$STAGE" --yes "$TMP/evil3"; grep -q allowlist "$TMP/fail.log"
[[ "$(digest)" == "$D0" && ! -e "$STAGE/etc/cron.d/job" ]]

# --- update: success path keeps config and keys ---
D_CFG="$(digest)"
cat > "$TMP/health-ok" <<'EOT'
#!/bin/sh
exit 0
EOT
# Fails only while the rejected 0.3.0 build is installed, so the rolled-back version verifies healthy.
printf '#!/bin/sh\n[ "$("%s/usr/local/bin/baft-tunnel" --version)" != "baft-tunnel 0.3.0" ]\n' "$STAGE" > "$TMP/health-bad"
chmod 755 "$TMP/health-ok" "$TMP/health-bad"
"$S/update.sh" --binary "$TMP/baft-tunnel.v2" --instance edge --health-cmd "$TMP/health-ok" --destdir "$STAGE" > "$TMP/update.log"
grep -q "UPDATE_OK from='baft-tunnel 0.1.0' to='baft-tunnel 0.2.0'" "$TMP/update.log"
[[ "$("$STAGE/usr/local/bin/baft-tunnel" --version)" == "baft-tunnel 0.2.0" && "$(digest)" == "$D_CFG" ]]

# --- update: failed health check returns to the previous version ---
fake 0.3.0; cp_bin v3
V2_SUM="$(sha256sum "$STAGE/usr/local/bin/baft-tunnel" | awk '{print $1}')"
set +e; "$S/update.sh" --binary "$TMP/baft-tunnel.v3" --instance edge --health-cmd "$TMP/health-bad" --destdir "$STAGE" > "$TMP/upd-bad.out" 2> "$TMP/upd-bad.err"; rc=$?; set -e
[[ "$rc" == "1" ]]
grep -q "UPDATE_FAILED_ROLLED_BACK restored='baft-tunnel 0.2.0' rejected='baft-tunnel 0.3.0'" "$TMP/upd-bad.err"
[[ "$(sha256sum "$STAGE/usr/local/bin/baft-tunnel" | awk '{print $1}')" == "$V2_SUM" && "$(digest)" == "$D_CFG" ]]

# --- update: bad binary and missing installation are refused before any change ---
printf '#!/bin/sh\nexit 1\n' > "$TMP/broken"; chmod 755 "$TMP/broken"
fails "$S/update.sh" --binary "$TMP/broken" --destdir "$STAGE"
[[ "$(sha256sum "$STAGE/usr/local/bin/baft-tunnel" | awk '{print $1}')" == "$V2_SUM" ]]
fails "$S/update.sh" --binary "$TMP/baft-tunnel.v3" --destdir "$TMP/empty-root"
fails "$S/update.sh" --binary "$TMP/baft-tunnel.v3" --instance '../x' --destdir "$STAGE"
if grep -rq 'PRIVATE KEY' "$TMP"/*.log "$TMP"/*.out "$TMP"/*.err 2>/dev/null; then echo "key material in a log" >&2; exit 1; fi

echo "BAFT_BACKUP_UPDATE_TEST=PASS (backup perms/checksum/no secrets in output, restore byte-identical with pre-restore snapshot, tamper/link/outside-path refusal, update success keeps config+keys, failed health check rolls back, refusals change nothing)"
