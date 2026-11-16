#!/usr/bin/env bash
# BAFT-TUNNEL panel HTTPS front end (ADR-0033).
#
# The panel/agent stays a non-root, loopback-only service. This script puts a TLS
# terminator (nginx) in front of it, obtains a certificate and prints the HTTPS link.
#   domain  -> Let's Encrypt certificate (ACME http-01) with automatic renewal.
#   IP      -> own-CA certificate generated on this host (name-constrained to the IP).
#              Browsers warn until the CA is trusted/pinned; this is printed, never hidden.
# There is never a silent downgrade to HTTP: any failure rolls the HTTPS change back and
# exits non-zero, and the agent stays loopback-only.
set -Eeuo pipefail
umask 077

CONF=/etc/nginx/conf.d/baft-tunnel-panel.conf
ETC=/etc/baft-tunnel-https
STATE=/var/lib/baft-tunnel-https
WEBROOT=$STATE/acme-webroot
LIBDIR=/usr/local/lib/baft-tunnel-https
DEPLOY_HOOK=/etc/letsencrypt/renewal-hooks/deploy/baft-tunnel-panel-reload.sh
CERT_NAME=baft-tunnel-panel
UNIT_DIR=/etc/systemd/system
TOKEN_FILE=/var/lib/baft-tunnel-agent/token
CA_DAYS=3650
LEAF_DAYS=397
RENEW_BEFORE_DAYS=30

MODE=install
HOST=""
KIND=""          # domain | ip
SELF_SIGNED=0
EMAIL=""
STAGING=0
HTTPS_PORT=443
AGENT_LISTEN="127.0.0.1:18080"
INTERACTIVE=0
SHOW_TOKEN=0
NO_TOKEN=0
SKIP_APT=0

usage() {
  cat <<'USAGE'
BAFT-TUNNEL panel HTTPS setup (Ubuntu 22.04+, run as root)

Usage:
  panel-https.sh --domain NAME [--email ADDR] [--self-signed] [options]
  panel-https.sh --ip ADDRESS [options]
  panel-https.sh --interactive            ask for a domain or IP on the terminal
  panel-https.sh --remove                 remove the HTTPS front end (agent stays loopback-only)

Options:
  --domain NAME        DNS name pointing at this host. Certificate: Let's Encrypt (ports 80 + 443
                       must be reachable from the Internet). Renewed automatically.
  --ip ADDRESS         Public/LAN IP of this host. Certificate: own CA generated here; the browser
                       shows a warning until you trust/pin the CA printed at the end.
  --self-signed        With --domain: use the own-CA certificate instead of Let's Encrypt.
  --email ADDR         Let's Encrypt expiry-notice contact (optional; not sent anywhere else).
  --https-port N       HTTPS listen port (default 443). Port 80 is still used for ACME challenges.
  --agent-listen ADDR  Loopback agent address (default 127.0.0.1:18080).
  --token-file PATH    Agent token file (default /var/lib/baft-tunnel-agent/token).
  --show-token         Print the login token even if it was printed before.
  --no-print-token     Never print the login token.
  --staging            Let's Encrypt staging CA (testing only; certificate is not browser-trusted).
  -h, --help           Show this help.
USAGE
}

die() { echo "baft-panel-https: $*" >&2; exit 2; }
say() { echo "baft-panel-https: $*"; }

while (($#)); do
  case "$1" in
    --domain) (($# >= 2)) || die "--domain needs a name"; HOST="$2"; KIND=domain; shift 2 ;;
    --ip) (($# >= 2)) || die "--ip needs an address"; HOST="$2"; KIND=ip; shift 2 ;;
    --self-signed) SELF_SIGNED=1; shift ;;
    --email) (($# >= 2)) || die "--email needs an address"; EMAIL="$2"; shift 2 ;;
    --https-port) (($# >= 2)) || die "--https-port needs a number"; HTTPS_PORT="$2"; shift 2 ;;
    --agent-listen) (($# >= 2)) || die "--agent-listen needs an address"; AGENT_LISTEN="$2"; shift 2 ;;
    --token-file) (($# >= 2)) || die "--token-file needs a path"; TOKEN_FILE="$2"; shift 2 ;;
    --interactive) INTERACTIVE=1; shift ;;
    --show-token) SHOW_TOKEN=1; shift ;;
    --no-print-token) NO_TOKEN=1; shift ;;
    --staging) STAGING=1; shift ;;
    --skip-apt) SKIP_APT=1; shift ;;            # test hook: packages are pre-installed
    --remove) MODE=remove; shift ;;
    --renew-selfsigned) MODE=renew; shift ;;    # used by the systemd timer
    -h|--help) usage; exit 0 ;;
    *) usage >&2; die "unknown argument: $1" ;;
  esac
done

valid_domain() {
  local d="$1"
  (( ${#d} <= 253 )) || return 1
  [[ "$d" =~ ^([a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?\.)+[a-z]{2,63}$ ]] || [[ "$d" =~ ^([a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?\.)+xn--[a-z0-9-]{1,59}$ ]]
}

# Prints the canonical address for a usable host IP, fails for loopback/unspecified/multicast/link-local.
canon_ip() {
  python3 -I - "$1" <<'PYIP'
import ipaddress, sys
try:
    a = ipaddress.ip_address(sys.argv[1])
except ValueError:
    raise SystemExit(1)
if a.is_loopback or a.is_unspecified or a.is_multicast or a.is_link_local or a.is_reserved:
    raise SystemExit(1)
print(a.compressed)
PYIP
}

ip_mask() {  # all-ones netmask text for a /32 or /128 name constraint
  python3 -I - "$1" <<'PYMASK'
import ipaddress, sys
a = ipaddress.ip_address(sys.argv[1])
print("255.255.255.255" if a.version == 4 else ":".join(["ffff"] * 8))
PYMASK
}

validate_inputs() {
  [[ "$HTTPS_PORT" =~ ^[0-9]{1,5}$ ]] && (( HTTPS_PORT >= 1 && HTTPS_PORT <= 65535 )) || die "--https-port must be 1-65535"
  [[ "$AGENT_LISTEN" =~ ^(127\.0\.0\.1|\[::1\]):([0-9]{1,5})$ ]] &&
    (( BASH_REMATCH[2] >= 1 && BASH_REMATCH[2] <= 65535 )) || die "--agent-listen must be 127.0.0.1:PORT or [::1]:PORT"
  if [[ -n "$EMAIL" ]]; then
    [[ "$EMAIL" =~ ^[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}$ ]] || die "--email is not a valid address"
  fi
  [[ -n "$HOST" ]] || die "give --domain NAME or --ip ADDRESS"
  if [[ "$KIND" == domain ]]; then
    HOST="$(printf '%s' "$HOST" | tr 'A-Z' 'a-z')"
    valid_domain "$HOST" || die "'$HOST' is not a valid domain name (IP addresses use --ip)"
  else
    HOST="$(canon_ip "$HOST")" || die "'$HOST' is not a usable public/LAN IP address (loopback, link-local and reserved ranges are refused)"
    (( SELF_SIGNED == 0 )) || true
    SELF_SIGNED=1
  fi
  if (( STAGING == 1 )) && [[ "$KIND" != domain || $SELF_SIGNED -eq 1 ]]; then
    die "--staging only applies to a Let's Encrypt domain certificate"
  fi
}

[[ "$(id -u)" == 0 ]] || die "run as root (sudo)"

# ---------------------------------------------------------------- helpers (shared)
url_host() { if [[ "$HOST" == *:* ]]; then printf '[%s]' "$HOST"; else printf '%s' "$HOST"; fi; }
port_suffix() { if [[ "$HTTPS_PORT" == 443 ]]; then printf ''; else printf ':%s' "$HTTPS_PORT"; fi; }

nginx_reload() {
  nginx -t >/dev/null 2>&1 || { nginx -t >&2 || true; return 1; }
  if systemctl is-active --quiet nginx; then
    systemctl reload nginx
  else
    systemctl enable --now nginx
  fi
}

# ---------------------------------------------------------------- remove
if [[ "$MODE" == remove ]]; then
  rm -f "$CONF" "$DEPLOY_HOOK"
  systemctl disable --now baft-tunnel-https-renew.timer >/dev/null 2>&1 || true
  rm -f "$UNIT_DIR/baft-tunnel-https-renew.service" "$UNIT_DIR/baft-tunnel-https-renew.timer"
  systemctl daemon-reload >/dev/null 2>&1 || true
  if command -v nginx >/dev/null 2>&1; then nginx_reload || true; fi
  rm -rf "$LIBDIR"
  say "HTTPS front end removed. Certificates and CA files were kept in $ETC, $STATE and /etc/letsencrypt (delete manually if unwanted)."
  exit 0
fi

# ---------------------------------------------------------------- self-signed CA (also used by renew)
CA_KEY=$ETC/ca.key
CA_CRT=$ETC/ca.crt
LEAF_KEY=$ETC/panel.key
LEAF_CRT=$ETC/panel.crt

read_state() {
  [[ -f "$STATE/state" ]] || return 1
  # shellcheck disable=SC1090
  while IFS='=' read -r k v; do
    case "$k" in
      HOST) HOST="$v" ;;
      KIND) KIND="$v" ;;
      SELF_SIGNED) SELF_SIGNED="$v" ;;
      HTTPS_PORT) HTTPS_PORT="$v" ;;
    esac
  done <"$STATE/state"
}

leaf_ok() {
  [[ -s "$LEAF_CRT" && -s "$LEAF_KEY" && -s "$CA_CRT" ]] || return 1
  openssl x509 -in "$LEAF_CRT" -noout -checkend $((RENEW_BEFORE_DAYS * 86400)) >/dev/null 2>&1 || return 1
  if [[ "$KIND" == ip ]]; then
    openssl x509 -in "$LEAF_CRT" -noout -checkip "$HOST" 2>/dev/null | grep -q 'does match' || return 1
  else
    openssl x509 -in "$LEAF_CRT" -noout -checkhost "$HOST" 2>/dev/null | grep -q 'does match' || return 1
  fi
  openssl verify -CAfile "$CA_CRT" "$LEAF_CRT" >/dev/null 2>&1
}

ensure_selfsigned() {
  install -d -m 0700 "$ETC"
  local tmp; tmp="$(mktemp -d "$ETC/.gen.XXXXXX")"
  local san constraint
  if [[ "$KIND" == ip ]]; then
    san="IP:$HOST"; constraint="permitted;IP:$HOST/$(ip_mask "$HOST")"
  else
    san="DNS:$HOST"; constraint="permitted;DNS:$HOST"
  fi
  if [[ ! -s "$CA_KEY" || ! -s "$CA_CRT" ]] ||
     ! openssl x509 -in "$CA_CRT" -noout -checkend $((RENEW_BEFORE_DAYS * 86400)) >/dev/null 2>&1; then
    say "generating a new local CA for $HOST (name-constrained to this host only)"
    cat >"$tmp/ca.cnf" <<CNF
[req]
distinguished_name=dn
x509_extensions=v3_ca
prompt=no
[dn]
CN=BAFT-TUNNEL panel CA for $HOST
[v3_ca]
basicConstraints=critical,CA:TRUE,pathlen:0
keyUsage=critical,keyCertSign,cRLSign
subjectKeyIdentifier=hash
nameConstraints=critical,$constraint
CNF
    openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes \
      -keyout "$tmp/ca.key" -out "$tmp/ca.crt" -days "$CA_DAYS" -config "$tmp/ca.cnf" 2>/dev/null
    chmod 0600 "$tmp/ca.key"; chmod 0644 "$tmp/ca.crt"
    mv -f "$tmp/ca.key" "$CA_KEY"; mv -f "$tmp/ca.crt" "$CA_CRT"
    rm -f "$LEAF_CRT"        # a new CA invalidates the old leaf
  fi
  if ! leaf_ok; then
    say "issuing the panel certificate (valid ${LEAF_DAYS} days, renewed automatically)"
    cat >"$tmp/leaf.cnf" <<CNF
basicConstraints=critical,CA:FALSE
keyUsage=critical,digitalSignature
extendedKeyUsage=serverAuth
subjectAltName=$san
subjectKeyIdentifier=hash
authorityKeyIdentifier=keyid
CNF
    openssl req -new -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes \
      -keyout "$tmp/panel.key" -out "$tmp/panel.csr" -subj "/CN=$HOST" 2>/dev/null
    openssl x509 -req -in "$tmp/panel.csr" -CA "$CA_CRT" -CAkey "$CA_KEY" -CAcreateserial \
      -CAserial "$tmp/ca.srl" -days "$LEAF_DAYS" -extfile "$tmp/leaf.cnf" -out "$tmp/panel.crt" 2>/dev/null
    chmod 0600 "$tmp/panel.key"; chmod 0644 "$tmp/panel.crt"
    mv -f "$tmp/panel.key" "$LEAF_KEY"; mv -f "$tmp/panel.crt" "$LEAF_CRT"
  fi
  rm -rf "$tmp"
  openssl verify -CAfile "$CA_CRT" "$LEAF_CRT" >/dev/null 2>&1 || die "generated certificate failed verification"
}

if [[ "$MODE" == renew ]]; then
  read_state || die "no HTTPS state found; nothing to renew"
  if leaf_ok; then say "certificate is valid for more than $RENEW_BEFORE_DAYS days; nothing to do"; exit 0; fi
  ensure_selfsigned
  nginx_reload
  say "certificate renewed"
  exit 0
fi

# ---------------------------------------------------------------- install
if (( INTERACTIVE == 1 )) && [[ -z "$HOST" ]]; then
  if [[ -r /dev/tty ]] && { : </dev/tty; } 2>/dev/null; then
    printf 'Panel address - a domain (Let'"'"'s Encrypt) or an IP (own-CA certificate). Empty = no public HTTPS (SSH tunnel only): ' >/dev/tty
    IFS= read -r ANSWER </dev/tty || ANSWER=""
    ANSWER="$(printf '%s' "$ANSWER" | tr -d '[:space:]')"
    if [[ -z "$ANSWER" ]]; then
      say "no address given: the panel stays on loopback only (use an SSH tunnel)"
      exit 0
    fi
    if canon_ip "$ANSWER" >/dev/null 2>&1; then KIND=ip; else KIND=domain; fi
    HOST="$ANSWER"
  else
    say "no terminal for the address prompt: the panel stays on loopback only. Re-run with --domain NAME or --ip ADDRESS."
    exit 0
  fi
fi
validate_inputs

[[ "$(uname -s)" == Linux ]] || die "only Linux hosts are supported"
for tool in curl python3 openssl systemctl install ss; do
  command -v "$tool" >/dev/null 2>&1 || die "missing required command: $tool"
done
[[ -s "$TOKEN_FILE" ]] || die "agent token file $TOKEN_FILE not found; install the agent first (quick-install.sh --register NAME)"

CREATED=()
CHANGED=0
BACKUP=""
rollback() {
  local rc=$?
  trap - EXIT
  if (( rc != 0 && CHANGED == 1 )); then
    echo "baft-panel-https: failed (exit $rc); rolling back the HTTPS change..." >&2
    if [[ -n "$BACKUP" && -f "$BACKUP" ]]; then cp -p "$BACKUP" "$CONF"; else rm -f "$CONF"; fi
    local f
    for f in "${CREATED[@]}"; do rm -rf "$f"; done
    nginx_reload >/dev/null 2>&1 || true
    echo "baft-panel-https: rolled back. The agent is unchanged and loopback-only; no HTTP exposure was added." >&2
  fi
  exit "$rc"
}
trap rollback EXIT

# Ports must be free or already owned by nginx (idempotent re-run).
check_port() {
  local p="$1" line
  line="$(ss -ltnpH "sport = :$p" 2>/dev/null || true)"
  [[ -z "$line" ]] && return 0
  grep -q '"nginx"' <<<"$line" && ! grep -qv '"nginx"' <<<"$line" && return 0
  die "port $p is already in use by another program; free it or choose --https-port (port 80 is required for Let's Encrypt)"
}
check_port "$HTTPS_PORT"
if [[ "$KIND" == domain && $SELF_SIGNED -eq 0 ]]; then check_port 80; fi

if [[ "$KIND" == domain ]] && ! getent ahosts "$HOST" >/dev/null 2>&1; then
  die "$HOST does not resolve in DNS; point its A/AAAA record at this host first"
fi

install_packages() {
  local pkgs=()
  command -v nginx >/dev/null 2>&1 || pkgs+=(nginx)
  if [[ "$KIND" == domain && $SELF_SIGNED -eq 0 ]]; then command -v certbot >/dev/null 2>&1 || pkgs+=(certbot); fi
  ((${#pkgs[@]})) || return 0
  (( SKIP_APT == 0 )) || die "required packages missing (${pkgs[*]}) and --skip-apt given"
  command -v apt-get >/dev/null 2>&1 || die "install ${pkgs[*]} manually (apt-get not found)"
  say "installing: ${pkgs[*]}"
  DEBIAN_FRONTEND=noninteractive apt-get update -qq
  DEBIAN_FRONTEND=noninteractive apt-get install -y -qq --no-install-recommends "${pkgs[@]}"
}
install_packages
command -v nginx >/dev/null 2>&1 || die "nginx is not available"

install -d -m 0711 "$STATE"   # traverse-only: nginx must reach $WEBROOT; files inside stay 0600 (umask 077)
install -d -m 0755 "$WEBROOT"
if [[ -f "$CONF" ]]; then BACKUP="$STATE/backup.conf"; cp -p "$CONF" "$BACKUP"; fi
CHANGED=1

UPSTREAM="$AGENT_LISTEN"
SUFFIX="$(port_suffix)"
LOGFMT='log_format baft_panel '"'"'$remote_addr [$time_local] "$request_method $uri" $status $body_bytes_sent'"'"';'

write_conf() {  # $1 = with-https (1/0)
  local full="$1" cert key hsts="" L6_80="" L6_TLS=""
  # Only listen on IPv6 where the kernel has it (nginx refuses to start otherwise).
  if [[ -e /proc/net/if_inet6 ]]; then L6_80="listen [::]:80;"; L6_TLS="listen [::]:$HTTPS_PORT ssl;"; fi
  if (( SELF_SIGNED == 1 )); then
    cert="$LEAF_CRT"; key="$LEAF_KEY"
  else
    cert="/etc/letsencrypt/live/$CERT_NAME/fullchain.pem"; key="/etc/letsencrypt/live/$CERT_NAME/privkey.pem"
    (( STAGING == 1 )) || hsts='add_header Strict-Transport-Security "max-age=31536000" always;'
  fi
  {
    echo "# Managed by baft-tunnel panel-https.sh (ADR-0033). Do not edit; re-run the script instead."
    echo "$LOGFMT"
    echo 'limit_req_zone $binary_remote_addr zone=baft_panel:10m rate=20r/s;'
    if [[ "$KIND" == domain && $SELF_SIGNED -eq 0 ]]; then
      cat <<ACME
server {
    listen 80;
    $L6_80
    server_name $HOST;
    access_log off;
    location ^~ /.well-known/acme-challenge/ { root $WEBROOT; default_type text/plain; }
    location / { return 301 https://$HOST$SUFFIX\$request_uri; }
}
ACME
    fi
    if [[ "$full" == 1 ]]; then
      cat <<TLS
server {
    listen $HTTPS_PORT ssl;
    $L6_TLS
    server_name $(url_host);
    server_tokens off;
    access_log /var/log/nginx/baft-panel.access.log baft_panel;
    ssl_certificate $cert;
    ssl_certificate_key $key;
    ssl_protocols TLSv1.2 TLSv1.3;
    ssl_prefer_server_ciphers off;
    ssl_session_cache shared:BAFT_SSL:2m;
    ssl_session_tickets off;
    $hsts
    add_header X-Content-Type-Options nosniff always;
    add_header X-Frame-Options DENY always;
    add_header Referrer-Policy no-referrer always;
    client_max_body_size 2m;
    if (\$host != "$HOST") { return 421; }
    location / {
        limit_req zone=baft_panel burst=60 nodelay;
        proxy_pass http://$UPSTREAM;
        proxy_http_version 1.1;
        proxy_set_header Host $UPSTREAM;
        proxy_set_header Connection "";
        proxy_read_timeout 120s;
        proxy_send_timeout 120s;
    }
}
TLS
    fi
  } >"$CONF.new"
  chmod 0644 "$CONF.new"
  mv -f "$CONF.new" "$CONF"
}

if (( SELF_SIGNED == 1 )); then
  ensure_selfsigned
  write_conf 1
  nginx_reload
else
  write_conf 0
  nginx_reload
  install -d -m 0755 "$(dirname "$DEPLOY_HOOK")"
  cat >"$DEPLOY_HOOK" <<'HOOK'
#!/bin/sh
# Reload nginx after Let's Encrypt renews the BAFT-TUNNEL panel certificate.
nginx -t >/dev/null 2>&1 && systemctl reload nginx
HOOK
  chmod 0755 "$DEPLOY_HOOK"
  command -v certbot >/dev/null 2>&1 || die "certbot is not available"
  args=(certonly --webroot -w "$WEBROOT" -d "$HOST" --cert-name "$CERT_NAME" --non-interactive --agree-tos --keep-until-expiring)
  if [[ -n "$EMAIL" ]]; then args+=(--email "$EMAIL"); else args+=(--register-unsafely-without-email); fi
  (( STAGING == 0 )) || args+=(--staging)
  say "requesting a Let's Encrypt certificate for $HOST"
  certbot "${args[@]}" >/dev/null 2>"$STATE/certbot.err" || {
    sed 's/^/  certbot: /' "$STATE/certbot.err" >&2
    die "certificate request failed (is $HOST pointing at this host and port 80 reachable from the Internet?)"
  }
  write_conf 1
  nginx_reload
  # Automatic renewal: the distribution's certbot.timer must be active.
  systemctl enable --now certbot.timer >/dev/null 2>&1 || true
  systemctl is-active --quiet certbot.timer || die "certbot.timer is not active; automatic renewal would not run"
  if (( STAGING == 0 )); then
    timeout 120 certbot renew --dry-run --no-random-sleep-on-renew --cert-name "$CERT_NAME" >/dev/null 2>&1 </dev/null || say "WARNING: 'certbot renew --dry-run' failed; check $HOST DNS/port 80 before the certificate expires (90 days)"
  fi
fi

# Renewal timer for the own-CA certificate (idempotent).
if (( SELF_SIGNED == 1 )); then
  install -d -m 0755 "$LIBDIR"
  SRC="$(readlink -f "${BASH_SOURCE[0]}")"
  [[ "$SRC" == "$LIBDIR/panel-https.sh" ]] || install -m 0755 "$SRC" "$LIBDIR/panel-https.sh"
  cat >"$UNIT_DIR/baft-tunnel-https-renew.service" <<UNIT
[Unit]
Description=Renew the BAFT-TUNNEL panel own-CA certificate
[Service]
Type=oneshot
ExecStart=$LIBDIR/panel-https.sh --renew-selfsigned
NoNewPrivileges=yes
PrivateTmp=yes
ProtectHome=yes
UNIT
  cat >"$UNIT_DIR/baft-tunnel-https-renew.timer" <<'UNIT'
[Unit]
Description=Daily check of the BAFT-TUNNEL panel own-CA certificate
[Timer]
OnCalendar=daily
RandomizedDelaySec=1h
Persistent=true
[Install]
WantedBy=timers.target
UNIT
  chmod 0644 "$UNIT_DIR"/baft-tunnel-https-renew.{service,timer}
  systemctl daemon-reload
  systemctl enable --now baft-tunnel-https-renew.timer >/dev/null
  systemctl is-active --quiet baft-tunnel-https-renew.timer || die "renewal timer is not active"
fi

# ---------------------------------------------------------------- smoke test
CURL_OPTS=(--noproxy "*" --max-time 20 --connect-to "$HOST:$HTTPS_PORT:127.0.0.1:$HTTPS_PORT")
if (( SELF_SIGNED == 1 )); then CURL_OPTS+=(--cacert "$CA_CRT"); elif (( STAGING == 1 )); then CURL_OPTS+=(-k); fi
LINK="https://$(url_host)$SUFFIX/"
say "smoke test: $LINK"
ok=0
for _ in 1 2 3 4 5 6 7 8 9 10; do   # a reload returns before the new workers serve
  if curl -fsS "${CURL_OPTS[@]}" -o /dev/null "${LINK}v1/health" 2>/dev/null; then ok=1; break; fi
  sleep 1
done
(( ok == 1 )) ||
  die "HTTPS smoke test failed: /v1/health is not reachable through the proxy (is the agent running on $AGENT_LISTEN?)"
unauth="$(curl -sS "${CURL_OPTS[@]}" -o /dev/null -w '%{http_code}' "${LINK}v1/status" || true)"
[[ "$unauth" == 401 ]] || die "smoke test: unauthenticated /v1/status returned '$unauth', expected 401 (authentication must stay enforced)"
curl -fsS "${CURL_OPTS[@]}" -o /dev/null "$LINK" || die "smoke test: the panel page is not served over HTTPS"

{
  echo "HOST=$HOST"; echo "KIND=$KIND"; echo "SELF_SIGNED=$SELF_SIGNED"; echo "HTTPS_PORT=$HTTPS_PORT"
} >"$STATE/state.new"
mv -f "$STATE/state.new" "$STATE/state"
printf 'https://%s%s/\n' "$(url_host)" "$SUFFIX" >"$STATE/link.new"   # link only, never the token
mv -f "$STATE/link.new" "$STATE/link"
CHANGED=0   # success: nothing to roll back

# ---------------------------------------------------------------- report
box() {  # draw a framed block around the given lines
  local w=0 l
  for l in "$@"; do (( ${#l} > w )) && w=${#l}; done
  printf '+-%s-+\n' "$(printf '%*s' "$w" '' | tr ' ' '-')"
  for l in "$@"; do printf '| %-*s |\n' "$w" "$l"; done
  printf '+-%s-+\n' "$(printf '%*s' "$w" '' | tr ' ' '-')"
}
LOGIN_LINES=("BAFT-TUNNEL panel - HTTPS login" "" "Link  : $LINK")
if (( NO_TOKEN == 1 )); then
  LOGIN_LINES+=("Token : not printed (--no-print-token). Read: sudo cat $TOKEN_FILE")
elif [[ -f "$STATE/credentials-shown" && $SHOW_TOKEN -eq 0 ]]; then
  LOGIN_LINES+=("Token : unchanged since first run. Read: sudo cat $TOKEN_FILE")
elif [[ -t 1 ]]; then
  LOGIN_LINES+=("Token : $(cat "$TOKEN_FILE")" "" "Paste the token on the login page. Shown once here;" "stored 0600 in $TOKEN_FILE")
  : >"$STATE/credentials-shown"
else
  LOGIN_LINES+=("Token : output is not a terminal, so it is not printed." "Read: sudo cat $TOKEN_FILE")
fi
echo
box "${LOGIN_LINES[@]}"
echo
if (( SELF_SIGNED == 1 )); then
  FP="$(openssl x509 -in "$CA_CRT" -noout -fingerprint -sha256 | cut -d= -f2)"
  LFP="$(openssl x509 -in "$LEAF_CRT" -noout -fingerprint -sha256 | cut -d= -f2)"
  echo "Certificate: own CA generated on this host (NOT a public CA)."
  echo "Your browser will warn \"not secure\" until you trust this CA or pin the certificate."
  echo "  CA SHA-256 fingerprint:   $FP"
  echo "  Panel cert SHA-256:       $LFP"
  echo "  CA file (public):         $CA_CRT"
  echo "    copy it:  scp root@$(url_host):$CA_CRT ./baft-panel-ca.crt   then compare the fingerprint above"
  echo "    check:    curl --cacert ./baft-panel-ca.crt $LINK"
  echo "The CA can only certify this host name/IP (name constraint). Renewal: automatic (daily timer)."
else
  echo "Certificate: Let's Encrypt$([[ $STAGING -eq 1 ]] && echo ' STAGING (not browser-trusted)'), renewed automatically (certbot.timer)."
fi
if command -v ufw >/dev/null 2>&1 && ufw status 2>/dev/null | grep -q 'Status: active'; then
  echo "Note: ufw is active; allow $HTTPS_PORT/tcp$([[ "$KIND" == domain && $SELF_SIGNED -eq 0 ]] && echo ' and 80/tcp') yourself (this script does not change firewall rules)."
fi
