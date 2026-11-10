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
grep -Fq -- '--verify-only' <<<"$HELP"
grep -Fq '7a6b86904016cc271e854aee54ca5b8ec5bd7fe28369e8775025c7ece029e2a4' "$SCRIPT"
grep -Fq 'verify-blob "$TMP/SHA256SUMS"' "$SCRIPT"
# The exact identity includes the requested tag; branch and foreign identities cannot match.
grep -Fq -- '--certificate-identity "https://github.com/$REPO/.github/workflows/release.yml@refs/tags/$TAG"' "$SCRIPT"
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
if "$SCRIPT" --register t1 --agent-listen 0.0.0.0:18080 >/dev/null 2>&1; then
  echo 'non-loopback agent listen address was accepted' >&2; exit 1
fi
python3 "$ROOT/scripts/test-quick-install-failures.py"
echo QUICK_INSTALL_TEST=PASS
