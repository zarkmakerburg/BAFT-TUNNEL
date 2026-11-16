#!/usr/bin/env bash
# Ephemeral offline CA for LAB enrollment: signs WORK/csr/{abroad,iran}.csr into WORK/signed/<side>/
# (node.crt, ca.crt, ca.crl) inside a network-isolated container. The encrypted CA key and its
# passphrase stay in container tmpfs; only public certificates and the CRL leave the container.
# Usage: offline-ca-sign.sh WORK_DIR PROFILE(node|baft-node) IRAN_SAN ABROAD_SAN
set -euo pipefail
work="$1" profile="$2" iran_san="$3" abroad_san="$4"
[[ "$profile" == node || "$profile" == baft-node ]] || { echo "profile must be node or baft-node" >&2; exit 2; }
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
image="baft-pki-offline:${GITHUB_RUN_ID:-local}-$$"
docker build --quiet -f "$here/Dockerfile.offline-ca" -t "$image" "$here" >/dev/null
trap 'docker image rm "$image" >/dev/null 2>&1 || true' EXIT
openssl rand -hex 32 | docker run --rm -i --network none --read-only \
  --tmpfs /tmp:rw,noexec,nosuid,size=64m \
  -v "$work:/workspace:rw" \
  -v "$repo/scripts/pki:/pki:ro" \
  -e D2_IRAN_SAN="$iran_san" -e D2_ABROAD_SAN="$abroad_san" -e D2_PROFILE="$profile" \
  "$image" /bin/bash -euo pipefail -c '
    umask 077
    cat > /tmp/passphrase
    chmod 600 /tmp/passphrase
    export BAFT_PKI_PASSFILE=/tmp/passphrase
    mkdir -p /tmp/ca /workspace/signed
    bash /pki/baft-pki.sh init-ca /tmp/ca "BAFT-TUNNEL D2 LAB CA" >/dev/null
    bash /pki/baft-pki.sh "issue-$D2_PROFILE" /tmp/ca abroad /workspace/csr/abroad.csr /workspace/signed/abroad "$D2_ABROAD_SAN" 90 >/dev/null
    bash /pki/baft-pki.sh "issue-$D2_PROFILE" /tmp/ca iran /workspace/csr/iran.csr /workspace/signed/iran "$D2_IRAN_SAN" 90 >/dev/null
    cp /tmp/ca/ca.crl /workspace/signed/abroad/ca.crl
    cp /tmp/ca/ca.crl /workspace/signed/iran/ca.crl
    bash /pki/baft-pki.sh "verify-$D2_PROFILE" /tmp/ca /tmp/ca/ca.crl /workspace/signed/abroad/node.crt abroad "$D2_ABROAD_SAN" >/dev/null
    bash /pki/baft-pki.sh "verify-$D2_PROFILE" /tmp/ca /tmp/ca/ca.crl /workspace/signed/iran/node.crt iran "$D2_IRAN_SAN" >/dev/null
    # Public enrollment artifacts only; the container may use another uid, so let the runner clean up.
    find /workspace/signed -type d -exec chmod 777 {} +
    find /workspace/signed -type f -exec chmod 666 {} +
    test -f /tmp/ca/ca.key
    test ! -e /workspace/ca.key
    rm -f /tmp/passphrase
  '
test ! -e "$work/ca.key" || { echo "offline CA private key escaped its tmpfs" >&2; exit 1; }
