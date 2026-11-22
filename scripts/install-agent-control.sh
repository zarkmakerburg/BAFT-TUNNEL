#!/usr/bin/env bash
# Register one instance with the local agent (ADR-0023 + amendment, SA-09):
#   - the agent service baft-tunnel-agent@NAME (its own user baft-tunnel-agent, loopback only),
#     running from registration on (setup mode until /etc/baft-tunnel/NAME.json exists), and at boot;
#   - the API token /var/lib/baft-tunnel-agent/token (created once, mode 0600, never printed);
#   - the control helpers: root path units for fixed lifecycle/setup/PKI/pair/fleet commands, plus
#     a per-instance systemd Unix socket for destructive manage operations (ADR-0055). The manage
#     helper authenticates SO_PEERCRED and the exact agent cgroup before touching root-owned files.
#     No polkit, no sudo.
#   install-agent-control.sh --instance NAME [--listen 127.0.0.1:PORT] [--destdir ROOT]
#   install-agent-control.sh --remove --instance NAME [--destdir ROOT]
#   install-agent-control.sh --upgrade-boundary [--destdir ROOT]
#     (update.sh: refresh templates and migrate an existing install's request directory
#      to root:baft-tunnel-agent 0770, fail-closed; prints CONTROL_BOUNDARY=...)
set -Eeuo pipefail
# BAFT_ERR_DIAGNOSTIC: preserve the failing command location on strict-mode exits.
trap 'rc=$?; echo "install-agent-control: unhandled command failure (exit=$rc, line=${BASH_LINENO[0]:-?}, function=${FUNCNAME[1]:-main})" >&2; exit "$rc"' ERR
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SRC="$SCRIPT_DIR/../systemd/control"
AGENT_SRC="$SCRIPT_DIR/../systemd/agent"
ROOT_PREFIX=""; INSTANCE=""; REMOVE=0; LISTEN=""; UPGRADE=0
AGENT_USER=baft-tunnel-agent
SERVICE_GROUP=baft-tunnel
die() { echo "install-agent-control: $*" >&2; exit 2; }
while (($#)); do
  case "$1" in
    --instance) (($# >= 2)) || die "--instance needs a name"; INSTANCE="$2"; shift 2 ;;
    --listen) (($# >= 2)) || die "--listen needs 127.0.0.1:PORT"; LISTEN="$2"; shift 2 ;;
    --destdir) (($# >= 2)) || die "--destdir needs a path"; ROOT_PREFIX="$2"; shift 2 ;;
    --remove) REMOVE=1; shift ;;
    --upgrade-boundary) UPGRADE=1; shift ;;
    -h|--help) sed -n 2,12p "$0"; exit 0 ;;
    *) die "unknown argument: $1" ;;
  esac
done
(( ! UPGRADE )) || [[ -z "$INSTANCE" && -z "$LISTEN" && "$REMOVE" == 0 ]] || die "--upgrade-boundary takes no --instance, --listen or --remove"
# The name becomes part of a unit name and a file name: letters, digits, '_' and '-' only.
(( UPGRADE )) || [[ "$INSTANCE" =~ ^[A-Za-z0-9][A-Za-z0-9_-]{0,62}$ ]] || die "--instance must be 1-63 characters of A-Z a-z 0-9 _ - and start alphanumeric"
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
VERBS=(start stop enable disable setup pki pair fleet)
FILES=(); for v in "${VERBS[@]}"; do FILES+=("baft-tunnel-control-$v@.path" "baft-tunnel-control-$v@.service"); done
FILES+=("baft-tunnel-control-manage@.socket" "baft-tunnel-control-manage@.service")
CTL_PATHS=(); for v in "${VERBS[@]}"; do CTL_PATHS+=("baft-tunnel-control-$v@$INSTANCE.path"); done
CTL_SOCKET="baft-tunnel-control-manage@$INSTANCE.socket"
CTL_SERVICE="baft-tunnel-control-manage@$INSTANCE.service"
LEGACY_MANAGE_PATH="baft-tunnel-control-manage@$INSTANCE.path"
AGENT_UNITS=("baft-tunnel-agent@$INSTANCE.path" "baft-tunnel-agent@$INSTANCE.service")
DROPIN_DIR="$UNIT_DIR/baft-tunnel-agent@$INSTANCE.service.d"
CONTROL_DIR=/run/baft-tunnel/control
CONTROL_PARENT=/run/baft-tunnel
install_templates() {
  install -d -m 0755 "$UNIT_DIR" "$(dirname "$TMPF")"
  for f in "${FILES[@]}"; do install -m 0644 "$SRC/$f" "$UNIT_DIR/$f"; done
  for f in baft-tunnel-agent@.service baft-tunnel-agent@.path; do install -m 0644 "$AGENT_SRC/$f" "$UNIT_DIR/$f"; done
  # Retire the old shared-directory manage path unit; the new socket is root-owned and
  # its peer must prove membership in the exact agent@INSTANCE cgroup.
  rm -f "$UNIT_DIR/baft-tunnel-control-manage@.path"
  install -m 0644 "$SRC/baft-tunnel-control.conf" "$TMPF"
}
retire_legacy_manage_for_instance() {
  local name=$1 unit="baft-tunnel-control-manage@$1.path" service="baft-tunnel-control-manage@$1.service"
  if [[ "$STAGING" == "0" ]]; then
    [[ ! -e "$CONTROL_DIR/$name.manage" && ! -L "$CONTROL_DIR/$name.manage" ]] ||
      die "legacy manage request for $name is pending; inspect it as root before upgrading"
    if systemctl is-active --quiet "$service"; then
      die "legacy manage helper for $name is active; retry after it finishes"
    fi
    systemctl disable --now "$unit" >/dev/null 2>&1 || true
    systemctl is-active --quiet "$unit" && die "legacy manage path unit for $name could not be stopped"
  fi
  rm -f "$UNIT_DIR/$unit" "$UNIT_DIR/multi-user.target.wants/$unit"
}
retire_all_legacy_manage_paths() {
  local link unit name listed existing found
  local -a names=()
  if [[ "$STAGING" == "0" ]]; then
    for link in "$UNIT_DIR/multi-user.target.wants"/baft-tunnel-control-manage@*.path; do
      [[ -e "$link" || -L "$link" ]] || continue
      unit="${link##*/}"; name="${unit#baft-tunnel-control-manage@}"; name="${name%.path}"
      [[ "$name" =~ ^[A-Za-z0-9][A-Za-z0-9_-]{0,62}$ ]] || die "invalid legacy manage unit name: $unit"
      found=0; for existing in "${names[@]}"; do [[ "$existing" == "$name" ]] && found=1; done
      (( found )) || names+=("$name")
    done
    listed="$(systemctl list-units --type=path --all --no-legend --plain 'baft-tunnel-control-manage@*.path')" ||
      die "could not enumerate legacy manage path units; refusing upgrade"
    while IFS= read -r unit; do
      [[ -n "$unit" ]] || continue
      name="${unit#baft-tunnel-control-manage@}"; name="${name%.path}"
      [[ "$name" =~ ^[A-Za-z0-9][A-Za-z0-9_-]{0,62}$ ]] || die "invalid active legacy manage unit name: $unit"
      found=0; for existing in "${names[@]}"; do [[ "$existing" == "$name" ]] && found=1; done
      (( found )) || names+=("$name")
    done < <(printf '%s\n' "$listed" | awk 'NF { print $1 }')
    for name in "${names[@]}"; do retire_legacy_manage_for_instance "$name"; done
  else
    rm -f "$UNIT_DIR/baft-tunnel-control-manage@.path"
    rm -f "$UNIT_DIR/multi-user.target.wants"/baft-tunnel-control-manage@*.path
  fi
  rm -f "$UNIT_DIR/baft-tunnel-control-manage@.path"
}
enable_manage_sockets_for_registered_agents() {
  local link unit name
  for link in "$UNIT_DIR/multi-user.target.wants"/baft-tunnel-agent@*.service; do
    [[ -e "$link" || -L "$link" ]] || continue
    unit="${link##*/}"; name="${unit#baft-tunnel-agent@}"; name="${name%.service}"
    [[ "$name" =~ ^[A-Za-z0-9][A-Za-z0-9_-]{0,62}$ ]] || die "invalid registered agent unit name: $unit"
    systemctl enable --now "baft-tunnel-control-manage@$name.socket"
  done
}
# Enforce the request-group boundary and migrate the request directory if needed (fail-closed).
enforce_control_boundary() {
  local AGENT_GID OTHER_PRIMARY_USERS
  AGENT_GID="$(getent group "$AGENT_USER" | awk -F: '{print $3}')"
  OTHER_PRIMARY_USERS="$(getent passwd | awk -F: -v gid="$AGENT_GID" -v agent="$AGENT_USER" '$4 == gid && $1 != agent { print $1 }')"
  if [[ -z "$AGENT_GID" ]] ||
     [[ "$(id -gn "$AGENT_USER")" != "$AGENT_USER" ]] ||
     [[ -n "$(getent group "$AGENT_USER" | awk -F: '{print $4}')" ]] ||
     [[ -n "$OTHER_PRIMARY_USERS" ]] ||
     id -nG "$SERVICE_GROUP" | tr ' ' '\n' | grep -qx "$AGENT_USER"; then
    die "group $AGENT_USER must be exclusive to the dedicated agent account"
  fi
  # Always run the fail-closed helper, also on an already-migrated directory: it re-checks active
  # root helpers and pending requests, so a refused request is still refused on every rerun.
  bash "$SCRIPT_DIR/migrate-agent-control-dir.sh" --control-dir "$CONTROL_DIR" --tmpfiles "$TMPF"
  [[ "$(stat -c '%U:%G %a' "$CONTROL_DIR")" == "root:baft-tunnel-agent 770" ]] ||
    die "control directory did not reach root:baft-tunnel-agent 0770"
}
if (( UPGRADE )); then
  if [[ ! -f "$TMPF" && ! -e "$UNIT_DIR/baft-tunnel-control-start@.path" ]]; then
    echo "CONTROL_BOUNDARY=not-installed"
    exit 0
  fi
  if [[ "$STAGING" == "0" ]]; then
    id "$AGENT_USER" >/dev/null 2>&1 ||
      die "agent control is installed but account $AGENT_USER is missing; re-run install-agent-control.sh --instance NAME"
  fi
  retire_all_legacy_manage_paths
  install_templates
  if [[ "$STAGING" == "1" ]]; then
    echo "CONTROL_BOUNDARY=staged-not-verified"
    exit 0
  fi
  enforce_control_boundary
  enable_manage_sockets_for_registered_agents
  echo "CONTROL_BOUNDARY=root:baft-tunnel-agent-0770"
  exit 0
fi
if [[ "$REMOVE" == "1" ]]; then
  retire_legacy_manage_for_instance "$INSTANCE"
  if [[ "$STAGING" == "0" ]]; then
    if systemctl is-active --quiet "$CTL_SERVICE"; then
      die "manage helper for $INSTANCE is active; retry removal after it finishes"
    fi
    systemctl disable --now "${AGENT_UNITS[@]}" "${CTL_PATHS[@]}" "$CTL_SOCKET" "$CTL_SERVICE" 2>/dev/null || true
  fi
  for u in "${AGENT_UNITS[@]}" "${CTL_PATHS[@]}"; do rm -f "$UNIT_DIR/multi-user.target.wants/$u"; done
  rm -f "$UNIT_DIR/sockets.target.wants/$CTL_SOCKET" "$UNIT_DIR/multi-user.target.wants/$LEGACY_MANAGE_PATH"
  rm -f "$DROPIN_DIR/listen.conf"; rmdir "$DROPIN_DIR" 2>/dev/null || true
  [[ "$STAGING" == "1" ]] || systemctl daemon-reload
  echo "Removed the agent, lifecycle path units and manage socket for $INSTANCE. Templates, the token and agent state are kept for other instances."
  exit 0
fi

retire_all_legacy_manage_paths
install_templates
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
  enforce_control_boundary
  systemctl enable --now "${CTL_PATHS[@]}"
  enable_manage_sockets_for_registered_agents
  systemctl enable --now "$CTL_SOCKET"
  # The agent starts now: in setup mode (panel pairing) until NAME.json exists, then normally.
  systemctl enable --now "baft-tunnel-agent@$INSTANCE.path"
  if ! systemctl enable --now "baft-tunnel-agent@$INSTANCE.service" >/dev/null 2>&1; then
    echo "install-agent-control: agent service failed to enable/start: baft-tunnel-agent@$INSTANCE.service" >&2
    systemctl status "baft-tunnel-agent@$INSTANCE.service" --no-pager >&2 || true
    journalctl -u "baft-tunnel-agent@$INSTANCE.service" -n 80 --no-pager >&2 || true
    die "agent service failed to start for instance $INSTANCE"
  fi
else
  install -d -m 0700 "$STATE"
fi
echo "Registered instance $INSTANCE with the local agent."
echo "  agent:  baft-tunnel-agent@$INSTANCE (user $AGENT_USER), running now and at boot (setup mode until /etc/baft-tunnel/$INSTANCE.json exists)"
echo "  panel:  http://${LISTEN:-127.0.0.1:18080}/ (loopback; from another machine: ssh -L 18080:127.0.0.1:18080 HOST)"
echo "  token:  /var/lib/baft-tunnel-agent/token (read it as root: sudo cat ...; never share it)"
echo "  helper: lifecycle/setup/PKI requests use the existing fixed path units; remove/edit use a root-owned Unix socket"
echo "          and verify the connecting PID belongs to baft-tunnel-agent@$INSTANCE.service before changing files."
echo "The tunnel itself was not started."
if [[ "$STAGING" == "0" ]] && systemctl is-active --quiet "baft-tunnel@$INSTANCE.service"; then
  echo "  note: the tunnel is already active; restart it once to enable integrity probes."
  echo "        It was not restarted automatically to avoid interrupting traffic."
fi
