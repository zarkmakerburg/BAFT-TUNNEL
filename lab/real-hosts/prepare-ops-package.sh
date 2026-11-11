#!/usr/bin/env bash
# Safely unpack the signed operator package for the trusted Direct operations LAB stage.
# Usage: prepare-ops-package.sh PACKAGE.tar.gz EXPECTED_BINARY OUTPUT_DIR
set -Eeuo pipefail
[[ $# == 3 ]] || { echo 'usage: prepare-ops-package.sh PACKAGE.tar.gz EXPECTED_BINARY OUTPUT_DIR' >&2; exit 2; }
archive=$1 binary=$2 out=$3
[[ -f "$archive" && ! -L "$archive" && -r "$archive" ]] || { echo 'package must be a readable regular file' >&2; exit 2; }
[[ -f "$binary" && ! -L "$binary" && -x "$binary" ]] || { echo 'expected binary must be an executable regular file' >&2; exit 2; }
[[ ! -e "$out" && ! -L "$out" ]] || { echo 'output path must not already exist' >&2; exit 2; }
mkdir -m 0700 "$out"
python3 - "$archive" "$out" "$binary" <<'PY'
import pathlib, re, sys, tarfile
archive, output, expected_binary = map(pathlib.Path, sys.argv[1:])
with tarfile.open(archive, "r:gz") as tf:
    members = tf.getmembers()
    if not members or len(members) > 2000:
        raise SystemExit("empty or oversized operator package entry list")
    if sum(max(0, member.size) for member in members) > 50 * 1024 * 1024:
        raise SystemExit("operator package expands beyond 50 MiB")
    roots = set()
    seen = set()
    kinds = {}
    for member in members:
        path = pathlib.PurePosixPath(member.name)
        if path.is_absolute() or not path.parts or ".." in path.parts:
            raise SystemExit(f"unsafe package path: {member.name!r}")
        if not (member.isdir() or member.isfile()):
            raise SystemExit(f"unsupported package entry: {member.name!r}")
        if member.name in seen:
            raise SystemExit(f"duplicate package path: {member.name!r}")
        seen.add(member.name)
        kinds[path] = member.isdir()
        roots.add(path.parts[0])
    if len(roots) != 1:
        raise SystemExit("package must contain exactly one top-level directory")
    root_name = roots.pop()
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]{0,127}", root_name):
        raise SystemExit("package root name contains unsupported characters")
    for path, is_dir in kinds.items():
        for parent in path.parents:
            if parent == pathlib.PurePosixPath("."):
                continue
            if parent in kinds and not kinds[parent]:
                raise SystemExit(f"package path descends through a regular file: {parent}")
        if not is_dir and any(path in other.parents for other in kinds if other != path):
            raise SystemExit(f"regular package file is also used as a directory: {path}")
    root = output / root_name
    tf.extractall(output, members=members)
    required = [
        root / "baft-tunnel",
        root / "scripts/install.sh",
        root / "scripts/update.sh",
        root / "scripts/backup.sh",
        root / "scripts/restore.sh",
        root / "scripts/uninstall.sh",
        root / "scripts/install-agent-control.sh",
        root / "scripts/pki/apply-crl.sh",
        root / "systemd/baft-tunnel@.service",
    ]
    for path in required:
        if not path.is_file() or path.is_symlink():
            raise SystemExit(f"signed package is missing a required regular file: {path.relative_to(root)}")
    if not (root / "baft-tunnel").stat().st_mode & 0o111:
        raise SystemExit("packaged binary is not executable")
    if not __import__("filecmp").cmp(root / "baft-tunnel", expected_binary, shallow=False):
        raise SystemExit("packaged binary does not match the separately selected candidate binary")
    print(root)
PY
