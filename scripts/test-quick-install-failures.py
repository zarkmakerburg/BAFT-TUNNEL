#!/usr/bin/env python3
"""Exercise quick-install download, archive checksum, and Cosign failure paths."""
import hashlib, io, json, os, pathlib, subprocess, tarfile, tempfile

ROOT = pathlib.Path(__file__).resolve().parents[1]
INSTALLER = ROOT / "scripts/quick-install.sh"
TAG = "v1.2.3"
ARCHIVE = f"baft-tunnel-{TAG}-linux-x86_64.tar.gz"

with tempfile.TemporaryDirectory(prefix="baft-installer-negative-") as td:
    base = pathlib.Path(td)
    fixtures = base / "fixtures"; fixtures.mkdir()
    pkg = base / "pkg" / ARCHIVE.removesuffix(".tar.gz")
    (pkg / "scripts").mkdir(parents=True)
    binary = pkg / "baft-tunnel"
    binary.write_text("#!/bin/sh\necho baft-tunnel test\n")
    binary.chmod(0o755)
    installer = pkg / "scripts/install.sh"
    installer.write_text("#!/bin/sh\nexit 0\n"); installer.chmod(0o755)
    archive_path = fixtures / ARCHIVE
    with tarfile.open(archive_path, "w:gz") as tf:
        tf.add(pkg, arcname=pkg.name)
    digest = hashlib.sha256(archive_path.read_bytes()).hexdigest()
    (fixtures / "SHA256SUMS").write_text(f"{digest}  {ARCHIVE}\n")
    (fixtures / "SHA256SUMS.cosign.bundle").write_text("fixture bundle\n")
    (fixtures / "cosign.deb").write_bytes(b"fixture deb\n")

    bindir = base / "bin"; bindir.mkdir()
    def shim(name, body):
        p = bindir / name; p.write_text("#!/bin/bash\nset -e\n" + body); p.chmod(0o755)
    shim("uname", 'if [[ "$1" == -s ]]; then echo Linux; else echo x86_64; fi\n')
    shim("curl", r'''url="${!#}"
out=""
while (($#)); do if [[ "$1" == -o ]]; then out="$2"; shift 2; else shift; fi; done
if [[ "$url" == */releases/tags/* ]]; then
  printf '%s\n' '{"tag_name":"v1.2.3","draft":false,"prerelease":false}'
  exit 0
fi
if [[ "$url" == *cosign_2.5.3_amd64.deb ]]; then cp "$BAFT_FIXTURES/cosign.deb" "$out"; exit 0; fi
if [[ "$url" == */''' + ARCHIVE + r''' ]]; then
  [[ "$BAFT_TEST_MODE" != downloadfail ]] || exit 22
  cp "$BAFT_FIXTURES/''' + ARCHIVE + r'''" "$out"; exit 0
fi
for f in SHA256SUMS SHA256SUMS.cosign.bundle; do
  if [[ "$url" == */$f ]]; then cp "$BAFT_FIXTURES/$f" "$out"; exit 0; fi
done
echo "unexpected mocked URL: $url" >&2; exit 22
''')
    shim("sha256sum", r'''if [[ "$1" == --check && "$2" == --status ]]; then exit 0; fi
if [[ "$1" == --ignore-missing && "$BAFT_TEST_MODE" == checksumfail ]]; then exit 1; fi
exec /usr/bin/sha256sum "$@"
''')
    shim("dpkg-deb", r'''[[ "$1" == -x ]] || exit 1
root="$3/usr/bin"; mkdir -p "$root"
cat > "$root/cosign-linux-amd64" <<'COSIGN'
#!/bin/bash
[[ "$BAFT_TEST_MODE" != signaturefail ]]
COSIGN
chmod +x "$root/cosign-linux-amd64"
''')

    env = os.environ.copy()
    env.update(PATH=f"{bindir}:{env['PATH']}", BAFT_FIXTURES=str(fixtures))
    cases = {
        "downloadfail": "archive download failure",
        "checksumfail": "archive checksum failure",
        "signaturefail": "Cosign signature verification failure",
    }
    for mode, label in cases.items():
        env["BAFT_TEST_MODE"] = mode
        proc = subprocess.run([str(INSTALLER), "--tag", TAG, "--verify-only"],
                              env=env, text=True, stdout=subprocess.PIPE,
                              stderr=subprocess.STDOUT, timeout=30)
        if proc.returncode == 0:
            raise SystemExit(f"FAIL: installer accepted {label}\n{proc.stdout}")
        print(f"PASS: rejected {label} (exit {proc.returncode})")
print("QUICK_INSTALL_NEGATIVE_TESTS=PASS")
