#!/usr/bin/env bash
# Start a BAFT-TUNNEL tcp-tls Direct pair with a throwaway PKI (ephemeral, 2-day CA, files in $1).
# Usage: baft_up.sh DIR BIN SERVE_LISTEN SERVE_DIAL TARGET ENTRY_LISTEN [serve-prefix] [forward-prefix]
# serve-prefix / forward-prefix are optional command prefixes (e.g. "ip netns exec bsrv"). Prints PIDs to DIR/pids.
set -euo pipefail
D=$1; BIN=$2; SL=$3; SD=$4; TG=$5; EL=$6; PS=${7:-}; PF=${8:-}
mkdir -p "$D/newcerts"; cd "$D"; : > index.txt; echo 1000 > serial; echo 1000 > crlnumber
openssl req -x509 -newkey rsa:3072 -nodes -keyout ca.key -out ca.crt -days 2 -sha256 -subj '/CN=BAFT bench ephemeral CA' \
  -addext 'basicConstraints=critical,CA:TRUE' -addext 'keyUsage=critical,keyCertSign,cRLSign' >/dev/null 2>&1
cat > openssl.cnf <<EOF
[ ca ]
default_ca = CA_default
[ CA_default ]
database = $D/index.txt
new_certs_dir = $D/newcerts
certificate = $D/ca.crt
private_key = $D/ca.key
serial = $D/serial
crlnumber = $D/crlnumber
default_md = sha256
default_days = 2
default_crl_days = 2
policy = policy_any
copy_extensions = copy
[ policy_any ]
commonName = supplied
EOF
for node in entry exit; do
  openssl req -new -newkey rsa:2048 -nodes -keyout $node.key -out $node.csr -subj "/CN=$node.bench" >/dev/null 2>&1
  printf 'basicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature\nextendedKeyUsage=serverAuth,clientAuth\nsubjectAltName=DNS:%s.bench,URI:urn:baft-tunnel:node:%s\n' $node $node > $node.ext
  printf '[ %s_ext ]\n' $node >> openssl.cnf; cat $node.ext >> openssl.cnf
  openssl ca -batch -notext -config openssl.cnf -extensions ${node}_ext -in $node.csr -out $node.crt -days 2 >/dev/null 2>&1
done
openssl ca -gencrl -batch -config openssl.cnf -out ca.crl >/dev/null 2>&1
pin() { openssl x509 -in "$1" -outform DER | sha256sum | awk '{print $1}'; }
chmod 600 *.key
$PS "$BIN" serve --listen "$SL" --target "$TG" --local-id exit --peer-id entry --tunnel-id bench --direction direct \
  --peer-fingerprint "$(pin entry.crt)" --ca ca.crt --crl ca.crl --cert exit.crt --key exit.key --max-streams 8 > serve.log 2>&1 &
echo $! > pids; sleep 1
$PF "$BIN" forward --listen "$EL" --remote "$SD" --server-name exit.bench --local-id entry --peer-id exit --tunnel-id bench --direction direct \
  --peer-fingerprint "$(pin exit.crt)" --ca ca.crt --crl ca.crl --cert entry.crt --key entry.key --max-streams 8 > forward.log 2>&1 &
echo $! >> pids; sleep 2
