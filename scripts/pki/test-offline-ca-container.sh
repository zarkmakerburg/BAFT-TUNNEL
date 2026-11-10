#!/usr/bin/env bash
set -Eeuo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"
command -v docker >/dev/null 2>&1 || { echo "Docker is required for the network-isolated offline CA test" >&2; exit 2; }
TMP="$(mktemp -d "${TMPDIR:-/tmp}/baft-offline-ca-test.XXXXXX")"
IMAGE="baft-pki-offline:test-$$"
cleanup() {
  rm -rf "$TMP"
  docker image rm "$IMAGE" >/dev/null 2>&1 || true
}
trap cleanup EXIT
mkdir -p "$TMP/csr" "$TMP/signed"
scripts/pki/baft-pki.sh new-node abroad "$TMP/node-abroad"
scripts/pki/baft-pki.sh new-node iran "$TMP/node-iran"
cp "$TMP/node-abroad/node.csr" "$TMP/csr/abroad.csr"
cp "$TMP/node-iran/node.csr" "$TMP/csr/iran.csr"
docker build --quiet -f lab/real-hosts/Dockerfile.offline-ca -t "$IMAGE" lab/real-hosts >/dev/null
PASS="$(openssl rand -hex 32)"
printf '%s\n' "$PASS" | docker run --rm -i --network none --read-only \
  --tmpfs /tmp:rw,noexec,nosuid,nodev,size=64m,mode=1777 \
  --user "$(id -u):$(id -g)" \
  -v "$TMP:/workspace:rw" \
  -v "$ROOT/scripts/pki:/pki:ro" \
  -e D2_IRAN_SAN=192.0.2.10 -e D2_ABROAD_SAN=198.51.100.20 \
  "$IMAGE" /bin/bash -euo pipefail -c '
    umask 077
    cat > /tmp/passphrase
    chmod 600 /tmp/passphrase
    export BAFT_PKI_PASSFILE=/tmp/passphrase
    mkdir -p /tmp/ca /workspace/signed
    bash /pki/baft-pki.sh init-ca /tmp/ca "BAFT-TUNNEL offline CA test" >/dev/null
    bash /pki/baft-pki.sh issue-node /tmp/ca abroad /workspace/csr/abroad.csr /workspace/signed/abroad 198.51.100.20 90 >/dev/null
    bash /pki/baft-pki.sh issue-node /tmp/ca iran /workspace/csr/iran.csr /workspace/signed/iran 192.0.2.10 90 >/dev/null
    cp /tmp/ca/ca.crl /workspace/signed/abroad/ca.crl
    cp /tmp/ca/ca.crl /workspace/signed/iran/ca.crl
    bash /pki/baft-pki.sh verify-node /tmp/ca /tmp/ca/ca.crl /workspace/signed/abroad/node.crt abroad 198.51.100.20 >/dev/null
    bash /pki/baft-pki.sh verify-node /tmp/ca /tmp/ca/ca.crl /workspace/signed/iran/node.crt iran 192.0.2.10 >/dev/null
    test -f /tmp/ca/ca.key
    test ! -e /workspace/ca.key
    rm -f /tmp/passphrase
  '
unset PASS
test ! -e "$TMP/ca.key"
test -s "$TMP/signed/abroad/node.crt"
test -s "$TMP/signed/abroad/ca.crt"
test -s "$TMP/signed/abroad/ca.crl"
test -s "$TMP/signed/iran/node.crt"
test -s "$TMP/signed/iran/ca.crt"
test -s "$TMP/signed/iran/ca.crl"
python3 - "$TMP/signed" <<'PYOWNER'
import os
import pathlib
import sys
root = pathlib.Path(sys.argv[1])
for name in ("abroad", "iran"):
    st = (root / name).stat()
    assert (st.st_uid, st.st_gid) == (os.getuid(), os.getgid()), (
        f"{name} output must remain owned by the invoking runner so cleanup is safe"
    )
PYOWNER
cmp -s "$TMP/signed/abroad/ca.crl" "$TMP/signed/iran/ca.crl"
echo "OFFLINE_CA_CONTAINER_TEST=PASS network=none private_ca_key_exported=0 two_node_certs=2 shared_crl=1"
