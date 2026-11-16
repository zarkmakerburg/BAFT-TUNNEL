#!/usr/bin/env bash
# LAB stage: real Let's Encrypt (ACME http-01) for the panel HTTPS front end on a LAB host.
# DOMAIN picks the host (lab-ex.* -> abroad, lab-ir.* -> iran). ACME=staging runs the signed package
# install and then panel-https.sh --domain --staging; ACME=production runs the public one-line installer
# with --domain (browser-trusted certificate). First proves DNS points at the host and that ports 80/443
# are reachable from the Internet, then checks the certificate, the login box, renewal, and removes
# everything it added (front end, certificate, ACME account if new, BAFT-TUNNEL, added packages, users).
# Host addresses and the token never print; only match/mismatch.
set -euo pipefail

: "${TAG:?}" "${DOMAIN:?}" "${ACME:?}" "${LAB_SSH_PRIVATE_KEY:?}" "${LAB_SSH_KNOWN_HOSTS:?}"
[[ "$TAG" =~ ^v[0-9]+\.[0-9]+\.[0-9]+(-rc\.[0-9]+)?$ ]] || { echo 'TAG must be a release tag' >&2; exit 2; }
[[ "$ACME" == staging || "$ACME" == production ]] || { echo 'ACME must be staging or production' >&2; exit 2; }
case "$DOMAIN" in
  lab-ex.goldapp.tr) SIDE=abroad; HOST="${LAB_ABROAD_HOST:?}"; USER_="${LAB_ABROAD_SSH_USER:-root}"; PORT_="${LAB_ABROAD_SSH_PORT:-22}" ;;
  lab-ir.goldapp.tr) SIDE=iran; HOST="${LAB_IRAN_HOST:?}"; USER_="${LAB_IRAN_SSH_USER:-root}"; PORT_="${LAB_IRAN_SSH_PORT:-22}" ;;
  *) echo 'DOMAIN is not a LAB domain' >&2; exit 2 ;;
esac
echo "::add-mask::${LAB_ABROAD_HOST:-unset-abroad}"; echo "::add-mask::${LAB_IRAN_HOST:-unset-iran}"
[[ "$USER_" =~ ^[a-z_][a-z0-9_-]{0,31}$ && "$PORT_" =~ ^[0-9]{1,5}$ ]] || { echo 'bad SSH user/port' >&2; exit 2; }
INST=httpsqa

W="$(mktemp -d "${RUNNER_TEMP:-/tmp}/baft-https-domain.XXXXXX")"; chmod 700 "$W"
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
fail() { echo "HTTPS_DOMAIN_FAIL domain=$DOMAIN side=$SIDE acme=$ACME | $*" >&2; exit 1; }
code() { local c; c="$(curl -sS -o /dev/null -w '%{http_code}' --max-time 20 "$@" 2>/dev/null)" || true; echo "${c:-000}"; }

echo "--- DNS: $DOMAIN must resolve to the $SIDE LAB host (compared here, never printed) ---"
A4="$(getent ahostsv4 "$DOMAIN" 2>/dev/null | awk '{print $1}' | sort -u || true)"
if [[ -z "$A4" ]]; then DNS=none; elif [[ "$A4" == "$HOST" ]]; then DNS=match; elif grep -qxF "$HOST" <<<"$A4"; then DNS=match_plus_other; else DNS=mismatch; fi
A6="$(getent ahostsv6 "$DOMAIN" 2>/dev/null | awk '$1 ~ /:/ && $1 !~ /^::ffff:/ {print $1}' | sort -u || true)"
AAAA=none; [[ -z "$A6" ]] || AAAA=present
echo "dns_a=$DNS dns_aaaa=$AAAA"
[[ "$DNS" == match ]] || fail "DNS A record for $DOMAIN is '$DNS' (want exactly the $SIDE LAB host)"

echo '--- precheck: host clean of BAFT-TUNNEL and the HTTPS front end; ports 80 and 443 free ---'
ssh_h "! test -e /usr/local/bin/baft-tunnel && ! test -e /etc/baft-tunnel && ! test -e /etc/nginx/conf.d/baft-tunnel-panel.conf && ! test -e /etc/baft-tunnel-https && ! test -e /etc/letsencrypt/live/baft-tunnel-panel && [ \"\$(systemctl list-unit-files --no-legend 'baft-tunnel*' | wc -l)\" = 0 ]" ||
  fail 'host is not clean of BAFT-TUNNEL (another run or a leftover install)'
ssh_h "! ss -Hltn 'sport = :80' | grep -q . && ! ss -Hltn 'sport = :443' | grep -q ." || fail 'port 80 or 443 is already in use on the host'

echo '--- reachability from the Internet: a throwaway listener on 80 and 443 answers a one-time nonce ---'
NONCE="$(openssl rand -hex 16)"
ssh_h "d=\$(mktemp -d); echo $NONCE > \$d/nonce; cd \$d; for p in 80 443; do nohup python3 -m http.server \$p --bind 0.0.0.0 >/dev/null 2>&1 < /dev/null & echo \$! >> \$d/pids; done; echo \$d > /root/.baft-https-qa-probe; sleep 2"
R80=closed; R443=closed
[[ "$(curl -fsS --max-time 15 "http://$DOMAIN/nonce" 2>/dev/null)" == "$NONCE" ]] && R80=open
[[ "$(curl -fsS --max-time 15 "http://$DOMAIN:443/nonce" 2>/dev/null)" == "$NONCE" ]] && R443=open
ssh_h "d=\$(cat /root/.baft-https-qa-probe); kill \$(cat \$d/pids) 2>/dev/null; rm -rf \$d /root/.baft-https-qa-probe; sleep 1; true"
echo "port80=$R80 port443=$R443 (from the GitHub runner, via the domain name)"
[[ "$R80" == open ]] || fail "port 80 on the $SIDE host is not reachable from the Internet (ACME http-01 needs it)"
[[ "$R443" == open ]] || fail "port 443 on the $SIDE host is not reachable from the Internet"

ssh_h "dpkg-query -W -f='\${Package}\n' | sort > /root/.baft-https-qa-pkgs.before; test -e /etc/letsencrypt && echo 1 > /root/.baft-https-qa-le.before || echo 0 > /root/.baft-https-qa-le.before"
USERS_BEFORE="$(ssh_h "for u in baft-tunnel baft-tunnel-agent; do id \$u >/dev/null 2>&1 && echo \$u; done; true")"

REMOVED=0
cleanup() {
  local st=$?; set +e
  echo '--- cleanup: front end, certificate, BAFT-TUNNEL and anything the installer added ---'
  ssh_h "set +e
    for p in /usr/local/lib/baft-tunnel-https/panel-https.sh /usr/local/lib/baft-tunnel/scripts/panel-https.sh; do
      test -f \$p && { bash \$p --remove >/dev/null 2>&1; break; }
    done
    command -v certbot >/dev/null 2>&1 && certbot delete --cert-name baft-tunnel-panel --non-interactive >/dev/null 2>&1
    rm -f /etc/letsencrypt/renewal-hooks/deploy/baft-tunnel-panel-reload.sh
    [ \"\$(cat /root/.baft-https-qa-le.before 2>/dev/null)\" = 0 ] && rm -rf /etc/letsencrypt /var/lib/letsencrypt /var/log/letsencrypt
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
    fi
    rm -f /root/.baft-https-qa-pkgs.before /root/.baft-https-qa-pkgs.after /root/.baft-https-qa-le.before
    systemctl is-active --quiet nginx 2>/dev/null && nginx -t >/dev/null 2>&1 && systemctl reload nginx
    true" 2>&1 | mask
  for u in baft-tunnel-agent baft-tunnel; do
    grep -qx "$u" <<<"$USERS_BEFORE" || ssh_h "userdel $u >/dev/null 2>&1; groupdel $u >/dev/null 2>&1; true" || true
  done
  if ssh_h "! ss -Hltn 'sport = :443' | grep -q . && ! ss -Hltn 'sport = :80' | grep -q . && ! test -e /usr/local/bin/baft-tunnel && ! test -e /etc/baft-tunnel-https && ! test -e /etc/letsencrypt/live/baft-tunnel-panel && [ \"\$(systemctl list-unit-files --no-legend 'baft-tunnel*' | wc -l)\" = 0 ]"; then
    REMOVED=1
  fi
  echo "HTTPS_DOMAIN_CLEANUP removed=$REMOVED"
  rm -rf "$W"
  (( REMOVED == 1 )) || st=1
  exit "$st"
}
trap cleanup EXIT

URL="https://raw.githubusercontent.com/zarkmakerburg/BAFT-TUNNEL/$TAG/scripts/quick-install.sh"
if [[ "$ACME" == production ]]; then
  echo "--- install: public one-line installer for $TAG with --domain $DOMAIN (Let's Encrypt production) ---"
  ssh_h "set -o pipefail; curl -fsSL '$URL' | bash -s -- --register $INST --tag $TAG --domain $DOMAIN --no-menu" 2>&1 | mask | tee "$W/install.log" ||
    fail 'the one-line installer failed'
else
  echo "--- install: signed package via the one-line installer (--no-https), then panel-https.sh --domain $DOMAIN --staging ---"
  ssh_h "set -o pipefail; curl -fsSL '$URL' | bash -s -- --register $INST --tag $TAG --no-https --no-menu" 2>&1 | mask ||
    fail 'the one-line installer failed'
  ssh_h "bash /usr/local/lib/baft-tunnel/scripts/panel-https.sh --domain $DOMAIN --staging" 2>&1 | mask | tee "$W/install.log" ||
    fail 'panel-https.sh --domain --staging failed'
fi
grep -qF "Link  : https://$DOMAIN/" "$W/install.log" && LINK=1 || LINK=0
grep -qF 'Token : output is not a terminal, so it is not printed.' "$W/install.log" && TOKBOX=1 || TOKBOX=0
grep -qF "Certificate: Let's Encrypt" "$W/install.log" || fail 'installer did not report a Let'"'"'s Encrypt certificate'
(( LINK == 1 )) || fail 'the login box does not show the https link for the domain'

echo '--- certificate as served to the Internet ---'
echo | openssl s_client -connect "$DOMAIN:443" -servername "$DOMAIN" 2>/dev/null | openssl x509 -noout -issuer -enddate -ext subjectAltName > "$W/cert.txt" || fail 'no certificate served on 443'
ISSUER="$(sed -n 's/^issuer=//p' "$W/cert.txt" | sed 's/^ *//')"
END="$(sed -n 's/^notAfter=//p' "$W/cert.txt")"
DAYS=$(( ( $(date -d "$END" +%s) - $(date +%s) ) / 86400 ))
grep -qF "DNS:$DOMAIN" "$W/cert.txt" || fail 'served certificate does not name the domain'
grep -q "Let's Encrypt\|(STAGING)" <<<"$ISSUER" || fail "unexpected issuer: $ISSUER"
if [[ "$ACME" == staging ]]; then grep -q 'STAGING' <<<"$ISSUER" || fail 'staging run got a non-staging issuer'; TRUST=(-k); else TRUST=(); fi
echo "issuer: $ISSUER | expires in ${DAYS} days"
(( DAYS >= 30 )) || fail "certificate expires in $DAYS days"

( umask 077; ssh_h "cat /var/lib/baft-tunnel-agent/token" > "$W/token" ) || fail 'could not read the agent token'
H="$(code "${TRUST[@]}" "https://$DOMAIN/v1/health")"
U="$(code "${TRUST[@]}" "https://$DOMAIN/v1/status")"
A="$(code "${TRUST[@]}" -H "Authorization: Bearer $(cat "$W/token")" "https://$DOMAIN/v1/status")"
P="$(code "${TRUST[@]}" "https://$DOMAIN/")"
rm -f "$W/token"
R="$(curl -sS -o /dev/null -w '%{http_code} %{redirect_url}' --max-time 15 "http://$DOMAIN/" 2>/dev/null || true)"
echo "from the runner (public, $([[ $ACME == production ]] && echo 'system trust store' || echo 'staging, -k')): health=$H unauth=$U auth=$A page=$P http80=${R%% *}"
[[ "$H" == 200 && "$U" == 401 && "$A" == 200 && "$P" == 200 ]] || fail "HTTPS checks failed (health=$H unauth=$U auth=$A page=$P)"
[[ "$R" == "301 https://$DOMAIN/" ]] || fail "port 80 does not redirect to https ($R)"

echo '--- renewal ---'
ssh_h "systemctl is-active --quiet certbot.timer" && TIMER=1 || TIMER=0
ssh_h "timeout 180 certbot renew --dry-run --cert-name baft-tunnel-panel >/dev/null 2>&1 < /dev/null" && DRY=1 || DRY=0
ssh_h "test -x /etc/letsencrypt/renewal-hooks/deploy/baft-tunnel-panel-reload.sh" && HOOK=1 || HOOK=0
ssh_h "! ss -Hltn | awk '{print \$4}' | grep -Eq '^(0\.0\.0\.0|\[::\]|\*):18080\$'" || fail 'the agent listens publicly on 18080'
(( TIMER == 1 && HOOK == 1 )) || fail "renewal not wired (certbot.timer=$TIMER deploy_hook=$HOOK)"
(( DRY == 1 )) || fail 'certbot renew --dry-run failed'

echo "HTTPS_DOMAIN_RESULT tag=$TAG domain=$DOMAIN side=$SIDE acme=$ACME installer=$([[ $ACME == production ]] && echo quick-install-domain || echo quick-install+panel-https-staging) dns=$DNS aaaa=$AAAA port80=$R80 port443=$R443 days_left=$DAYS health=$H unauth=$U auth=$A page=$P http_redirect=1 login_link=$LINK token_not_printed_non_tty=$TOKBOX certbot_timer=$TIMER renew_dry_run=$DRY deploy_hook=$HOOK agent_loopback=1"
