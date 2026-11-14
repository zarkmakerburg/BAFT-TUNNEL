#!/usr/bin/env bash
# Register one instance with the local agent (ADR-0023 + amendment, SA-09):
#   - the agent service baft-tunnel-agent@NAME (its own user baft-tunnel-agent, loopback only),
#     running from registration on (setup mode until /etc/baft-tunnel/NAME.json exists), and at boot;
#   - the API token /var/lib/baft-tunnel-agent/token (created once, mode 0600, never printed);
#   - the control helper: root path units that turn a request file into one fixed command for
#     exactly baft-tunnel@NAME (start, stop, enable --now, disable --now), and the root setup
#     helper (ADR-0026) that runs setup init/join and bounded PKI CSR/import operations (ADR-0027) for NAME
#     in /etc/baft-tunnel only. No polkit, no sudo.
#   install-agent-control.sh --instance NAME [--listen 127.0.0.1:PORT] [--destdir ROOT]
#   install-agent-control.sh --remove --instance NAME [--destdir ROOT]
set -Eeuo pipefail
# BAFT_ERR_DIAGNOSTIC: preserve the failing command location on strict-mode exits.
trap 'rc=$?; printf "%s\\n" "install-agent-control: unhandled command failure (exit=$rc, line=${BASH_LINENO[0]:-?}, function=${FUNCNAME[1]:-main})" >&2; exit "$rc"' ERR
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SRC="$SCRIPT_DIR/../systemd/control"
AGENT_SRC="$SCRIPT_DIR/../systemd/agent"
ROOT_PREFIX=""; INSTANCE=""; REMOVE=0; LISTEN=""
AGENT_USER=baft-tunnel-agent
SERVICE_GROUP=baft-tunnel
die() { echo "install-agent-control: $*" >&2; exit 2; }
while (($#)); do
  case "$1" in
    --instance) (($# >= 2)) || die "--instance needs a name"; INSTANCE="$2"; shift 2 ;;
    --listen) (($# >= 2)) || die "--listen needs 127.0.0.1:PORT"; LISTEN="$2"; shift 2 ;;
    --destdir) (($# >= 2)) || die "--destdir needs a path"; ROOT_PREFIX="$2"; shift 2 ;;
    --remove) REMOVE=1; shift ;;
    -h|--help) sed -n 2,9p "$0"; exit 0 ;;
    *) die "unknown argument: $1" ;;
  esac
done
# The name becomes part of a unit name and a file name: letters, digits, '_' and '-' only.
[[ "$INSTANCE" =~ ^[A-Za-z0-9][A-Za-z0-9_-]{0,62}$ ]] || die "--instance must be 1-63 characters of A-Z a-z 0-9 _ - and start alphanumeric"
if [[ -n "$LISTEN" ]]; then
  [[ "$LISTEN" =~ ^(127\.0\.0\.1|\[::1\]):([0-9]{1,5})$ ]] && (( BASH_REMATCH[2] >= 1 && BASH_REMATCH[2] <= 65535 )) ||
    die "--listen must be 127.0.0.1:PORT or [::1]:PORT (the agent is loopback only)"
fi
STAGING=0; [[ -n "$ROOT_PREFIX" ]] && STAGING=1
if [[ "$STAGING" == "0" ]]; then
  [[ "$(id -u)" == "0" ]] || die "must run as root"
  getent group "$SERVICE_GROUP" >/dev/null || die "group $SERVICE_GROUP does not exist; run install.sh --binary first"
fi
UNIT_DIR="$ROOT_PREFIX/etc/systemd/system"
TMPF="$ROOT_PREFIX/etc/tmpfiles.d/baft-tunnel-control.conf"
STATE="$ROOT_PREFIX/var/lib/baft-tunnel-agent"
VERBS=(start stop enable disable setup pki)
FILES=(); for v in "${VERBS[@]}"; do FILES+=("baft-tunnel-control-$v@.path" "baft-tunnel-control-$v@.service"); done
CTL_PATHS=(); for v in "${VERBS[@]}"; do CTL_PATHS+=("baft-tunnel-control-$v@$INSTANCE.path"); done
AGENT_UNITS=("baft-tunnel-agent@$INSTANCE.path" "baft-tunnel-agent@$INSTANCE.service")
DROPIN_DIR="$UNIT_DIR/baft-tunnel-agent@$INSTANCE.service.d"
if [[ "$REMOVE" == "1" ]]; then
  if [[ "$STAGING" == "0" ]]; then
    systemctl disable --now "${AGENT_UNITS[@]}" "${CTL_PATHS[@]}" 2>/dev/null || true
  fi
  for u in "${AGENT_UNITS[@]}" "${CTL_PATHS[@]}"; do rm -f "$UNIT_DIR/multi-user.target.wants/$u"; done
  rm -f "$DROPIN_DIR/listen.conf"; rmdir "$DROPIN_DIR" 2>/dev/null || true
  [[ "$STAGING" == "1" ]] || systemctl daemon-reload
  echo "Removed the agent and control path units for $INSTANCE. Templates, the token and agent state are kept for other instances."
  exit 0
fi

install -d -m 0755 "$UNIT_DIR" "$(dirname "$TMPF")"
for f in "${FILES[@]}"; do install -m 0644 "$SRC/$f" "$UNIT_DIR/$f"; done
for f in baft-tunnel-agent@.service baft-tunnel-agent@.path; do install -m 0644 "$AGENT_SRC/$f" "$UNIT_DIR/$f"; done
install -m 0644 "$SRC/baft-tunnel-control.conf" "$TMPF"
if [[ -n "$LISTEN" ]]; then
  install -d -m 0755 "$DROPIN_DIR"
  printf '# Managed by BAFT-TUNNEL scripts/install-agent-control.sh\n[Service]\nEnvironment=BAFT_AGENT_LISTEN=%s\n' "$LISTEN" > "$DROPIN_DIR/listen.conf"
  chmod 0644 "$DROPIN_DIR/listen.conf"
fi

if [[ "$STAGING" == "0" ]]; then
  # The agent's own user (SA-09): not the tunnel user, member of its group for read access only.
  if ! getent group "$AGENT_USER" >/dev/null 2>&1; then groupadd --system "$AGENT_USER"; fi
  if ! id "$AGENT_USER" >/dev/null 2>&1; then
    useradd --system --gid "$AGENT_USER" --home-dir /var/lib/baft-tunnel-agent --shell /usr/sbin/nologin "$AGENT_USER"
  fi
  id -nG "$AGENT_USER" | tr ' ' '\n' | grep -qx "$SERVICE_GROUP" || usermod -a -G "$SERVICE_GROUP" "$AGENT_USER"
  install -d -o "$AGENT_USER" -g "$AGENT_USER" -m 0700 "$STATE"
  if [[ ! -e "$STATE/token" ]]; then
    [[ -x /usr/local/bin/baft-tunnel ]] || die "/usr/local/bin/baft-tunnel is missing; run install.sh --binary first"
    setpriv --reuid="$AGENT_USER" --regid="$AGENT_USER" --clear-groups \
      /usr/local/bin/baft-tunnel agent-token --out "$STATE/token" >/dev/null
  fi
  [[ "$(stat -c '%U %a' "$STATE/token")" == "$AGENT_USER 600" ]] || die "$STATE/token must be $AGENT_USER, mode 0600"
  CONTROL_DIR=/run/baft-tunnel/control
  CONTROL_PARENT=/run/baft-tunnel
  CONTROL_DIR_STATE="$(stat -c '%U:%G %a' "$CONTROL_DIR" 2>/dev/null || true)"
  CONTROL_PARENT_STATE="$(stat -c '%U:%G %a' "$CONTROL_PARENT" 2>/dev/null || true)"
  # Enforce the request-group boundary on every registration, not only on migration.
  AGENT_GID="$(getent group "$AGENT_USER" | awk -F: '{print $3}')"
  OTHER_PRIMARY_USERS="$(getent passwd | awk -F: -v gid="$AGENT_GID" -v agent="$AGENT_USER" '$4 == gid && $1 != agent { print $1 }')"
  if [[ "$(id -gn "$AGENT_USER")" != "$AGENT_USER" ]] ||
     [[ -n "$(getent group "$AGENT_USER" | awk -F: '{print $4}')" ]] ||
     [[ -n "$OTHER_PRIMARY_USERS" ]] ||
     id -nG "$SERVICE_GROUP" | tr ' ' '\n' | grep -qx "$AGENT_USER"; then
    die "group $AGENT_USER must be exclusive to the dedicated agent account"
  fi
  MIGRATION_JOURNAL=/var/lib/baft-tunnel-control/migration-v1.units
  # Resume a partially completed migration even if the directory mode was already changed.
  if [[ "$CONTROL_DIR_STATE" != "root:baft-tunnel-agent 770" || "$CONTROL_PARENT_STATE" != "root:root 755" || -L "$CONTROL_PARENT" || -L "$CONTROL_DIR" || -e "$MIGRATION_JOURNAL" || -L "$MIGRATION_JOURNAL" ]]; then
    bash "$SCRIPT_DIR/migrate-agent-control-dir.sh" --control-dir "$CONTROL_DIR" --tmpfiles "$TMPF"
  else
    systemd-tmpfiles --create "$TMPF"
    systemctl daemon-reload
  fi
  systemctl enable --now "${CTL_PATHS[@]}"
  # The agent starts now: in setup mode (panel pairing) until NAME.json exists, then normally.
  systemctl enable --now "baft-tunnel-agent@$INSTANCE.path"
  systemctl enable --now "baft-tunnel-agent@$INSTANCE.service" >/dev/null 2>&1
else
  install -d -m 0700 "$STATE"
fi
echo "Registered instance $INSTANCE with the local agent."
echo "  agent:  baft-tunnel-agent@$INSTANCE (user $AGENT_USER), running now and at boot (setup mode until /etc/baft-tunnel/$INSTANCE.json exists)"
echo "  panel:  http://${LISTEN:-127.0.0.1:18080}/ (loopback; from another machine: ssh -L 18080:127.0.0.1:18080 HOST)"
echo "  token:  /var/lib/baft-tunnel-agent/token (read it as root: sudo cat ...; never share it)"
echo "  helper: the agent can request start, stop, enable --now and disable --now of baft-tunnel@$INSTANCE only,"
echo "          setup init/join and bounded PKI CSR/certificate import for $INSTANCE in /etc/baft-tunnel (panel enrollment)."
echo "The tunnel itself was not started."
if [[ "$STAGING" == "0" ]] && systemctl is-active --quiet "baft-tunnel@$INSTANCE.service"; then
  echo "  note: the tunnel is already active; restart it once to enable integrity probes."
  echo "        It was not restarted automatically to avoid interrupting traffic."
fi
