#!/usr/bin/env bash
set -Eeuo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd -P)"
SCRIPT="$ROOT/scripts/quick-install.sh"
TMP_TEST="$(mktemp -d)"
trap 'rm -rf "$TMP_TEST"' EXIT
[[ -x "$SCRIPT" ]]
HELP="$("$SCRIPT" --help)"
grep -Fq -- '--register NAME' <<<"$HELP"
grep -Fq -- '--tag TAG' <<<"$HELP"
grep -Fq -- '--stable' <<<"$HELP"
grep -Fq -- '--channel CHANNEL' <<<"$HELP"
grep -Fq -- '--verify-only' <<<"$HELP"
grep -Fq '7a6b86904016cc271e854aee54ca5b8ec5bd7fe28369e8775025c7ece029e2a4' "$SCRIPT"
grep -Fq 'verify-blob "$TMP/SHA256SUMS"' "$SCRIPT"
# The exact identity includes the requested tag; branch and foreign identities cannot match.
grep -Fq 'IDENTITY="https://github.com/$REPO/.github/workflows/release.yml@refs/tags/$TAG"' "$SCRIPT"
test "$(grep -c -- '--certificate-identity "$IDENTITY"' "$SCRIPT")" = 2
test "$(grep -c -- '--certificate-oidc-issuer https://token.actions.githubusercontent.com' "$SCRIPT")" = 2
# Offline path (ADR-0034): pinned trust only, no network, same pins, fails closed; block matches trusted_root.json.
grep -Fq -- '--bundle "$TMP/SHA256SUMS.cosign.bundle" --offline' "$SCRIPT"
grep -Fq 'SIGSTORE_ROOT_FILE="$TMP/sigstore/fulcio.pem"' "$SCRIPT"
grep -Fq 'die "release signature verification failed (online and with the pinned Sigstore trust root); nothing was installed"' "$SCRIPT"
if grep -Eq 'sha256sum --check SHA256SUMS.*\|\||insecure|--insecure-ignore' "$SCRIPT"; then echo 'checksum-only or insecure fallback found' >&2; exit 1; fi
bash "$ROOT/scripts/sigstore/refresh-trust.sh" --check >/dev/null
# curl | bash safety: the script is one brace group (parsed completely before it runs) and no child
# reads the piped script; the HTTPS outcome is always stated before the menu, which is opt-in.
test "$(grep -v '^#' "$SCRIPT" | sed -n '1p')" = '{'
test "$(tail -n1 "$SCRIPT")" = '}'
grep -Fq -- '--register "$INSTANCE" </dev/null' "$SCRIPT"
grep -Fq 'bash "$PKG/scripts/panel-https.sh" "${HARGS[@]}" </dev/null' "$SCRIPT"
grep -Fq 'HTTPS setup FAILED' "$SCRIPT"
grep -Fq 'Panel (HTTPS): $PANEL_LINK' "$SCRIPT"
grep -Fq 'Press Enter to open the BAFT-TUNNEL menu' "$SCRIPT"
grep -Fq 'NEEDRESTART_SUSPEND=1 apt-get install' "$ROOT/scripts/panel-https.sh"
# Behavioural: piping the script to bash while a stdin-reading child runs must not lose lines.
pipe_demo="$TMP_TEST/pipe-demo.sh"
printf '%s\n' '{' 'cat >/dev/null' 'echo LINE_AFTER_CHILD' '}' >"$pipe_demo"
[[ "$(bash <"$pipe_demo")" == LINE_AFTER_CHILD ]]
python3 - <<'PYIDENTITY'
base = "https://github.com/zarkmakerburg/BAFT-TUNNEL/.github/workflows/release.yml@"
expected = base + "refs/tags/v1.2.3-rc.4"
assert expected == base + "refs/tags/" + "v1.2.3-rc.4"
for candidate in (base + "refs/heads/main", base + "refs/heads/feature", base + "refs/tags/v1.2.3/evil", "https://github.com/other/repo/.github/workflows/release.yml@refs/tags/v1.2.3-rc.4"):
    assert candidate != expected, f"unexpected identity equality: {candidate}"
PYIDENTITY
# Run the exact archive-validation heredoc from quick-install against positive and hostile tarballs.
python3 - "$SCRIPT" "$TMP_TEST/validate_archive.py" <<'PYEXTRACT'
import pathlib, sys
source = pathlib.Path(sys.argv[1]).read_text()
start = source.index("python3 - \"$TMP/$ARCHIVE\" \"$NAME\" <<'PYTAR'\n")
start = source.index("\n", start) + 1
end = source.index("\nPYTAR\n", start)
pathlib.Path(sys.argv[2]).write_text(source[start:end])
PYEXTRACT
python3 - "$TMP_TEST" <<'PYARCHIVE'
import io, pathlib, sys, tarfile
root = pathlib.Path(sys.argv[1])
def archive(name, entries):
    path = root / name
    with tarfile.open(path, "w:gz") as tf:
        for info, data in entries:
            tf.addfile(info, io.BytesIO(data) if data is not None else None)
    return path
regular = tarfile.TarInfo("package/baft-tunnel"); regular.size = 4
valid = archive("valid.tar.gz", [(regular, b"test")])
traversal = tarfile.TarInfo("package/../../outside"); traversal.size = 1
bad_path = archive("traversal.tar.gz", [(traversal, b"x")])
link = tarfile.TarInfo("package/link"); link.type = tarfile.SYMTYPE; link.linkname = "../../outside"
bad_link = archive("link.tar.gz", [(link, None)])
(root / "valid.path").write_text(str(valid))
(root / "traversal.path").write_text(str(bad_path))
(root / "link.path").write_text(str(bad_link))
PYARCHIVE
python3 "$TMP_TEST/validate_archive.py" "$(cat "$TMP_TEST/valid.path")" package
if python3 "$TMP_TEST/validate_archive.py" "$(cat "$TMP_TEST/traversal.path")" package >/dev/null 2>&1; then
  echo 'path-traversal archive was accepted' >&2; exit 1
fi
if python3 "$TMP_TEST/validate_archive.py" "$(cat "$TMP_TEST/link.path")" package >/dev/null 2>&1; then
  echo 'symlink archive entry was accepted' >&2; exit 1
fi
grep -Fq 'sha256sum --ignore-missing --check SHA256SUMS' "$SCRIPT"
grep -Fq 'awk -v archive="$ARCHIVE"' "$SCRIPT"
grep -Fq 'COSIGN_BIN="$TMP/cosign-root/usr/bin/cosign-linux-amd64"' "$SCRIPT"
grep -Fq 'Authorization: Bearer $GITHUB_TOKEN' "$SCRIPT"
grep -Fq 'github_api_get "$API/releases/tags/$TAG"' "$SCRIPT"
grep -Fq 'tarfile' "$SCRIPT"
grep -Fq 'bash "$PKG/scripts/install.sh" --binary "$PKG/baft-tunnel" --register "$INSTANCE"' "$SCRIPT"
if "$SCRIPT" --tag 'v1.2' --register test >/dev/null 2>&1; then
  echo 'malformed semantic version tag was accepted' >&2; exit 1
fi
if "$SCRIPT" --tag '../main' --register test >/dev/null 2>&1; then
  echo 'unsafe release tag was accepted' >&2; exit 1
fi
if "$SCRIPT" --register 'bad.name' >/dev/null 2>&1; then
  echo 'unsafe instance name was accepted' >&2; exit 1
fi
if "$SCRIPT" --tag v0.1.0-rc.6 >/dev/null 2>&1; then
  echo 'missing required instance registration was accepted' >&2; exit 1
fi
if "$SCRIPT" --tag v0.1.0-rc.6 --stable --register t1 >/dev/null 2>&1; then
  echo 'conflicting stable and pinned release selection was accepted' >&2; exit 1
fi
if "$SCRIPT" --tag v0.1.0-rc.6 --channel prerelease --register t1 >/dev/null 2>&1; then
  echo 'conflicting channel and pinned release selection was accepted' >&2; exit 1
fi
if "$SCRIPT" --channel nightly --register t1 >/dev/null 2>&1; then
  echo 'unknown release channel was accepted' >&2; exit 1
fi
if "$SCRIPT" --register t1 --agent-listen 0.0.0.0:18080 >/dev/null 2>&1; then
  echo 'non-loopback agent listen address was accepted' >&2; exit 1
fi
# Regression (LAB run 43879430384): the HTTPS outcome block must survive set -e on the default
# port 443. Run the real block from the script against a temporary state directory.
HB="$(mktemp -d)"
awk '/# Success is what panel-https.sh recorded/{p=1} p{print} p&&/^  fi$/{exit}' "$SCRIPT" \
  | sed "s#/var/lib/baft-tunnel-https#$HB#g" >"$HB/block.sh"
grep -q 'HTTPS_OK=1' "$HB/block.sh"
printf 'abcdEFGH12345678\n' >"$HB/gate"
for case in 'h.example|443|https://h.example/' 'h.example||https://h.example/' \
            'h.example|8443|https://h.example:8443/' '2001:db8::1|443|https://[2001:db8::1]/'; do
  IFS='|' read -r h port want <<<"$case"
  printf 'HOST=%s\nHTTPS_PORT=%s\n' "$h" "$port" >"$HB/state"
  got="$(bash -c 'set -Eeuo pipefail; HTTPS_OK=0; PANEL_LINK=""; HTTPS_RC=0; source "$1"; echo "$HTTPS_OK|$PANEL_LINK"' _ "$HB/block.sh")" \
    || { echo "HTTPS outcome block exited non-zero for host=$h port=${port:-none}" >&2; exit 1; }
  [[ "$got" == "1|${want}<web path>/ (web path: sudo cat $HB/gate)" ]] \
    || { echo "HTTPS outcome block gave '$got' for host=$h port=${port:-none}" >&2; exit 1; }
done
rm -rf "$HB"
python3 "$ROOT/scripts/test-quick-install-failures.py"
echo QUICK_INSTALL_TEST=PASS
