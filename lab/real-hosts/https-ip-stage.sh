#!/usr/bin/env bash
# LAB stage: the public one-line installer with --ip on the abroad LAB host, for a signed release tag.
# Installs the signed package (install --register) and the nginx HTTPS front end with an own CA bound
# to the host IP, checks it from the runner (or on the host when the runner cannot reach 443), then
# removes everything it added: front end, certificates, BAFT-TUNNEL install, packages that apt pulled
# in for it, and service users that did not exist before. Host addresses and the token never print.
set -euo pipefail

: "${TAG:?TAG is required}" "${LAB_ABROAD_HOST:?}" "${LAB_SSH_PRIVATE_KEY:?}" "${LAB_SSH_KNOWN_HOSTS:?}"
[[ "$TAG" =~ ^v[0-9]+\.[0-9]+\.[0-9]+(-rc\.[0-9]+)?$ ]] || { echo 'TAG must be a release tag' >&2; exit 2; }
USER_="${LAB_ABROAD_SSH_USER:-root}"
PORT_="${LAB_ABROAD_SSH_PORT:-22}"
[[ "$USER_" =~ ^[a-z_][a-z0-9_-]{0,31}$ ]] || { echo 'bad SSH user' >&2; exit 2; }
[[ "$PORT_" =~ ^[0-9]{1,5}$ ]] || { echo 'bad SSH port' >&2; exit 2; }
INST=httpsqa
HOST="$LAB_ABROAD_HOST"
echo "::add-mask::$HOST"

W="$(mktemp -d "${RUNNER_TEMP:-/tmp}/baft-https-ip.XXXXXX")"
chmod 700 "$W"
KEY="$W/key"; KH="$W/known_hosts"
( umask 077; printf '%s\n' "$LAB_SSH_PRIVATE_KEY" > "$KEY"; printf '%s\n' "$LAB_SSH_KNOWN_HOSTS" > "$KH" )
unset LAB_SSH_PRIVATE_KEY LAB_SSH_KNOWN_HOSTS

mask() { sed -E 's/[0-9]{1,3}(\.[0-9]{1,3}){3}/<ip>/g; s/[A-Za-z0-9_-]{40,}/<redacted>/g'; }
ssh_h() {
  local a rc=0
  for a in 1 2 3; do
    ssh -i "$KEY" -p "$PORT_" -o BatchMode=yes -o IdentitiesOnly=yes -o StrictHostKeyChecking=yes \
      -o UserKnownHostsFile="$KH" -o ConnectTimeout=12 -o ServerAliveInterval=10 -o ServerAliveCountMax=3 \
      -o LogLevel=ERROR "$USER_@$HOST" "$1" && return 0 || rc=$?
    (( rc == 255 )) || return "$rc"
    sleep $((a * 5))
  done
  return "$rc"
}
fail() { echo "HTTPS_IP_FAIL | $*" >&2; exit 1; }

echo '--- precheck: the abroad LAB host is free of BAFT-TUNNEL and of the HTTPS front end; port 443 is free ---'
ssh_h "! test -e /usr/local/bin/baft-tunnel && ! test -e /etc/baft-tunnel && ! test -e /etc/nginx/conf.d/baft-tunnel-panel.conf && ! test -e /etc/baft-tunnel-https && [ \"\$(systemctl list-unit-files --no-legend 'baft-tunnel*' | wc -l)\" = 0 ]" ||
  fail 'host is not clean of BAFT-TUNNEL (another run or a leftover install)'
ssh_h "! ss -Hltn 'sport = :443' | grep -q ." || fail 'port 443 is already in use on the host'
ssh_h "dpkg-query -W -f='\${Package}\n' | sort > /root/.baft-https-qa-pkgs.before"
USERS_BEFORE="$(ssh_h "for u in baft-tunnel baft-tunnel-agent; do id \$u >/dev/null 2>&1 && echo \$u; done; true")"

REMOVED=0
cleanup() {
  local st=$?; set +e
  echo '--- cleanup: remove the HTTPS front end, BAFT-TUNNEL and anything the installer added ---'
  ssh_h "set +e
    test -x /usr/local/lib/baft-tunnel-https/panel-https.sh && bash /usr/local/lib/baft-tunnel-https/panel-https.sh --remove >/dev/null 2>&1
    test -f /usr/local/lib/baft-tunnel-https/panel-https.sh && bash /usr/local/lib/baft-tunnel-https/panel-https.sh --remove >/dev/null 2>&1
    systemctl disable --now baft-tunnel-https-renew.timer >/dev/null 2>&1
    rm -f /etc/nginx/conf.d/baft-tunnel-panel.conf
    rm -rf /etc/baft-tunnel-https /var/lib/baft-tunnel-https /usr/local/lib/baft-tunnel-https
    systemctl disable --now baft-tunnel@$INST.service >/dev/null 2>&1
    s=/usr/local/lib/baft-tunnel/scripts
    test -f \$s/install-agent-control.sh && bash \$s/install-agent-control.sh --remove --instance $INST >/dev/null 2>&1
    test -f \$s/uninstall.sh && bash \$s/uninstall.sh --stop --purge-config --yes >/dev/null 2>&1
    if test -f /root/.baft-https-qa-pkgs.before; then
      dpkg-query -W -f='\${Package}\n' | sort > /root/.baft-https-qa-pkgs.after
      added=\$(comm -13 /root/.baft-https-qa-pkgs.before /root/.baft-https-qa-pkgs.after | tr '\n' ' ')
      [ -z \"\$added\" ] || DEBIAN_FRONTEND=noninteractive apt-get purge -y -qq \$added >/dev/null 2>&1
      rm -f /root/.baft-https-qa-pkgs.before /root/.baft-https-qa-pkgs.after
    fi
    systemctl is-active --quiet nginx 2>/dev/null && nginx -t >/dev/null 2>&1 && systemctl reload nginx
    true" 2>&1 | mask
  for u in baft-tunnel-agent baft-tunnel; do
    grep -qx "$u" <<<"$USERS_BEFORE" || ssh_h "userdel $u >/dev/null 2>&1; groupdel $u >/dev/null 2>&1; true" || true
  done
  if ssh_h "! ss -Hltn 'sport = :443' | grep -q . && ! test -e /usr/local/bin/baft-tunnel && ! test -e /etc/baft-tunnel-https && [ \"\$(systemctl list-unit-files --no-legend 'baft-tunnel*' | wc -l)\" = 0 ]"; then
    REMOVED=1
  fi
  echo "HTTPS_IP_CLEANUP removed=$REMOVED"
  rm -rf "$W"
  (( REMOVED == 1 )) || st=1
  exit "$st"
}
trap cleanup EXIT

echo "--- install: public one-line installer for $TAG with --ip (signed package, install --register, HTTPS front end) ---"
URL="https://raw.githubusercontent.com/zarkmakerburg/BAFT-TUNNEL/$TAG/scripts/quick-install.sh"
ssh_h "set -o pipefail; curl -fsSL '$URL' | bash -s -- --register $INST --tag $TAG --ip $HOST --no-menu" 2>&1 | mask ||
  fail 'the one-line installer failed'
ssh_h "test -s /etc/baft-tunnel-https/ca.crt && test -s /etc/nginx/conf.d/baft-tunnel-panel.conf && systemctl is-active --quiet nginx" ||
  fail 'HTTPS front end is not running after install'

echo '--- the certificate: own CA, IP SAN equal to the host address, name-constrained, key not world-readable ---'
ssh_h "cat /etc/baft-tunnel-https/ca.crt" > "$W/ca.crt"
grep -q 'BEGIN CERTIFICATE' "$W/ca.crt" || fail 'no CA certificate'
ssh_h "openssl x509 -in /etc/baft-tunnel-https/panel.crt -noout -ext subjectAltName" > "$W/san.txt" || fail 'no panel certificate'
grep -qF "IP Address:$HOST" "$W/san.txt" || fail 'panel certificate does not carry the host IP as SAN'
openssl x509 -in "$W/ca.crt" -noout -ext nameConstraints 2>/dev/null | grep -qi 'permitted' && NC=1 || NC=0
ssh_h "find /etc/baft-tunnel-https /var/lib/baft-tunnel-https -name '*.key' -perm /o+r | grep -q . && exit 1; exit 0" || fail 'a private key is world-readable'
ssh_h "! grep -q 'PRIVATE KEY' /etc/baft-tunnel-https/ca.crt" || fail 'CA certificate file holds a private key'
( umask 077; ssh_h "cat /var/lib/baft-tunnel-agent/token" > "$W/token" ) || fail 'could not read the agent token'
[[ -s "$W/token" ]] || fail 'empty agent token'
( umask 077; ssh_h "cat /var/lib/baft-tunnel-https/gate 2>/dev/null || true" > "$W/gate" )
CK=(); GATEP=0
if [[ -s "$W/gate" ]]; then GATEP=1; CK=(-H "Cookie: baft_gate=$(cat "$W/gate")"); fi

code() { local c; c="$(curl -sS -o /dev/null -w '%{http_code}' --max-time 15 "$@" 2>/dev/null)" || true; echo "${c:-000}"; }
PATH_USED=runner
if [[ "$(code --cacert "$W/ca.crt" "https://$HOST/v1/health")" == 000 ]]; then
  PATH_USED=host
fi
if [[ "$PATH_USED" == runner ]]; then
  G="$(code --cacert "$W/ca.crt" "https://$HOST/v1/health")"
  H="$(code --cacert "$W/ca.crt" "${CK[@]}" "https://$HOST/v1/health")"
  U="$(code --cacert "$W/ca.crt" "${CK[@]}" "https://$HOST/v1/status")"
  A="$(code --cacert "$W/ca.crt" "${CK[@]}" -H "Authorization: Bearer $(cat "$W/token")" "https://$HOST/v1/status")"
  P="$(code --cacert "$W/ca.crt" "${CK[@]}" "https://$HOST/")"
  (( GATEP == 0 )) || [[ "$G" == 404 ]] || fail "panel answered $G without the web path cookie"
  # Without the own CA the certificate must not verify (no silent fallback to an untrusted chain).
  X="$(code "https://$HOST/v1/health")"
else
  echo 'runner cannot reach 443 on the LAB host; checking through the host itself (pinned CA, --resolve)'
  r="--resolve $HOST:443:127.0.0.1 --cacert /etc/baft-tunnel-https/ca.crt"
  out="$(ssh_h "c() { x=\$(curl -sS -o /dev/null -w '%{http_code}' --max-time 15 \"\$@\" 2>/dev/null); echo \${x:-000}; }
    t=\$(cat /var/lib/baft-tunnel-agent/token); g=\$(cat /var/lib/baft-tunnel-https/gate 2>/dev/null); k=\"Cookie: baft_gate=\$g\"
    echo \$(c $r -H \"\$k\" https://$HOST/v1/health) \$(c $r -H \"\$k\" https://$HOST/v1/status) \$(c $r -H \"\$k\" -H \"Authorization: Bearer \$t\" https://$HOST/v1/status) \$(c $r -H \"\$k\" https://$HOST/) \$(c --resolve $HOST:443:127.0.0.1 https://$HOST/v1/health)")"
  read -r H U A P X <<<"$out"
fi
echo "checks via $PATH_USED: health=$H unauth=$U auth=$A page=$P no_ca=$X"
[[ "$H" == 200 ]] || fail "health over HTTPS returned $H"
[[ "$U" == 401 ]] || fail "unauthenticated status returned $U, want 401"
[[ "$A" == 200 ]] || fail "authenticated status returned $A"
[[ "$P" == 200 ]] || fail "panel page returned $P"
[[ "$X" == 000 ]] || fail "certificate verified without the own CA ($X)"
ssh_h "! ss -Hltn | awk '{print \$4}' | grep -Eq '^(0\.0\.0\.0|\[::\]|\*):18080\$'" || fail 'the agent listens publicly on 18080'
ssh_h "cat /etc/nginx/conf.d/baft-tunnel-panel.conf" | grep -Eq 'ssl_protocols TLSv1\.(2 TLSv1\.)?3;' && TLS=1 || TLS=0
rm -f "$W/token" "$W/gate"

echo "HTTPS_IP_RESULT tag=$TAG web_path=$GATEP installer=quick-install path=$PATH_USED ip_san=1 name_constraint=$NC health=$H unauth=$U auth=$A page=$P no_ca_rejected=1 tls12plus=$TLS agent_loopback=1"
