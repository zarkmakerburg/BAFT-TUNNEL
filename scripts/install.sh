#!/usr/bin/env bash
set -Eeuo pipefail
umask 022

ROOT_PREFIX=""
BINARY=""
INSTANCE=""
REGISTER=""
AGENT_LISTEN=""
NODE_KEY=""
CREDENTIAL_MODE="baseline"
REPLACE_CREDENTIAL=0
ASSUME_SYSTEMD_VERSION=""
BAFT_ENGINE=""
SERVICE_USER="baft-tunnel"
CREDENTIAL_ID="node-key"
MAX_CREDENTIAL_BYTES=16384
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd -P)"
UNIT_SOURCE="$SCRIPT_DIR/../systemd/baft-tunnel@.service"
DROPIN_MARKER="# Managed by BAFT-TUNNEL scripts/install.sh"

usage() {
  cat <<'EOF'
Usage: install.sh                (no arguments on a terminal: interactive menu)
       install.sh [--binary PATH [--baft-engine PATH]] [--instance NAME --node-key PATH
                  [--credential-mode baseline|encrypted] [--replace-credential]]
                  [--register NAME [--agent-listen 127.0.0.1:PORT]]
                  [--destdir ROOT [--systemd-version N]]

Install the BAFT-TUNNEL binary and systemd template unit, and/or provision the
node-key service credential for one instance. At least one of --binary or
--instance is required. No tunnel is enabled, started, or restarted.
With --binary the scripts and units are also copied to /usr/local/lib/baft-tunnel
so that `baft-tunnel menu` (Persian/English interactive menu) works afterwards.

--baft-engine PATH (with --binary): verify the signed BAFT engine asset
(baft-engine-TAG-linux-amd64) against the engine manifest built into the
binary and install it as `current` in the root-owned engine store
/usr/local/lib/baft-tunnel-engines (the previous release is kept for
`baft-tunnel engine rollback`). Needed before the panel can set up the baft
engine (Preview, Direct only). A binary with another digest is refused.

--instance/--node-key (PEM key) are for the tcp-tls engine only; noise-tcp
creates its own key with `baft-tunnel setup init` and needs neither.

--register NAME (one step for an installed instance): also installs the local
agent service baft-tunnel-agent@NAME under its own user baft-tunnel-agent, its
API token, and the root control helper for baft-tunnel@NAME
(scripts/install-agent-control.sh), including the root setup helper. The agent
runs at once and at boot: in setup mode until /etc/baft-tunnel/NAME.json exists
(pair from the panel, or `setup init/join --instance NAME` as root), then it
reloads by itself; the panel then starts, stops and restarts the tunnel.

Credential modes (the private key never goes into JSON, environment, or args):
  baseline   Default. systemd 247+ (Ubuntu 22.04 / systemd 249 supported).
             Key -> /etc/baft-tunnel/credentials/NAME/node-key (root:root 0600,
             directory root:root 0700) plus a drop-in with LoadCredential=.
             Plaintext at rest, protected by root-only file permissions.
  encrypted  Optional, systemd 250+ with systemd-creds encrypt. Key is
             encrypted to /etc/baft-tunnel/credentials/NAME/node-key.cred and
             delivered with LoadCredentialEncrypted=. Fails closed when the
             host lacks support; it never falls back to baseline mode.

An existing credential with different content is only replaced when
--replace-credential is given. The previous key is kept in the root-only
backup directory so rollback.sh can restore it.

--destdir is staging/test mode: no user, ownership, or systemd changes occur.
--systemd-version is accepted only with --destdir.
EOF
}
die() { echo "install: $*" >&2; exit 2; }

# No arguments on a terminal: the interactive menu (it calls this script with flags).
if (($# == 0)) && [[ -t 0 && -t 1 && -f "$SCRIPT_DIR/menu.sh" ]]; then
  exec bash "$SCRIPT_DIR/menu.sh"
fi

while (($#)); do
  case "$1" in
    --binary) (($# >= 2)) || die "--binary needs a path"; BINARY="$2"; shift 2 ;;
    --baft-engine) (($# >= 2)) || die "--baft-engine needs a path"; BAFT_ENGINE="$2"; shift 2 ;;
    --destdir) (($# >= 2)) || die "--destdir needs a path"; ROOT_PREFIX="$2"; shift 2 ;;
    --instance) (($# >= 2)) || die "--instance needs a name"; INSTANCE="$2"; shift 2 ;;
    --register) (($# >= 2)) || die "--register needs a name"; REGISTER="$2"; shift 2 ;;
    --agent-listen) (($# >= 2)) || die "--agent-listen needs 127.0.0.1:PORT"; AGENT_LISTEN="$2"; shift 2 ;;
    --node-key) (($# >= 2)) || die "--node-key needs a path"; NODE_KEY="$2"; shift 2 ;;
    --credential-mode) (($# >= 2)) || die "--credential-mode needs a value"; CREDENTIAL_MODE="$2"; shift 2 ;;
    --replace-credential) REPLACE_CREDENTIAL=1; shift ;;
    --systemd-version) (($# >= 2)) || die "--systemd-version needs a number"; ASSUME_SYSTEMD_VERSION="$2"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) usage >&2; die "unknown argument: $1" ;;
  esac
done

[[ -n "$BINARY" || -n "$INSTANCE" ]] || { usage >&2; die "provide --binary and/or --instance"; }
if [[ -n "$REGISTER" ]]; then
  [[ "$REGISTER" =~ ^[A-Za-z0-9][A-Za-z0-9_-]{0,62}$ ]] ||
    die "invalid --register name (allowed: letters, digits, _ -; max 63; start alphanumeric)"
  [[ -f "$SCRIPT_DIR/install-agent-control.sh" ]] || die "install-agent-control.sh not found next to install.sh"
else
  [[ -z "$AGENT_LISTEN" ]] || die "--agent-listen requires --register"
fi
[[ -f "$UNIT_SOURCE" ]] || die "systemd template not found: $UNIT_SOURCE"
if [[ -n "$BINARY" ]]; then
  [[ -x "$BINARY" ]] || die "provide an executable release binary with --binary"
  BINARY="$(cd "$(dirname "$BINARY")" && pwd -P)/$(basename "$BINARY")"
  "$BINARY" --version >/dev/null 2>&1 || die "binary failed its --version check"
fi
if [[ -n "$BAFT_ENGINE" ]]; then
  [[ -n "$BINARY" ]] || die "--baft-engine requires --binary (its built-in manifest pins the engine)"
  [[ -f "$BAFT_ENGINE" && ! -L "$BAFT_ENGINE" ]] || die "--baft-engine must be a regular, non-symlink file"
  BAFT_ENGINE="$(cd "$(dirname "$BAFT_ENGINE")" && pwd -P)/$(basename "$BAFT_ENGINE")"
fi
if [[ -n "$INSTANCE" ]]; then
  # Restrict to names that need no systemd escaping and cannot traverse paths.
  # Same rule as the agent control helper and `valid_instance` (SA-10): no '.'.
  [[ "$INSTANCE" =~ ^[A-Za-z0-9][A-Za-z0-9_-]{0,62}$ ]] ||
    die "invalid instance name (allowed: letters, digits, _ -; max 63; start alphanumeric)"
  [[ -n "$NODE_KEY" ]] || die "--instance requires --node-key PATH"
  case "$CREDENTIAL_MODE" in baseline|encrypted) ;; *) die "--credential-mode must be baseline or encrypted" ;; esac
  # Validate the key source without ever printing its contents.
  [[ -f "$NODE_KEY" && ! -L "$NODE_KEY" ]] || die "--node-key must be a regular, non-symlink file"
  [[ -r "$NODE_KEY" ]] || die "--node-key is not readable"
  key_size="$(wc -c < "$NODE_KEY" | tr -d '[:space:]')"
  (( key_size > 0 && key_size <= MAX_CREDENTIAL_BYTES )) || die "--node-key size is outside the permitted range (1..$MAX_CREDENTIAL_BYTES bytes)"
  grep -Eq '^-----BEGIN ([A-Z]+ )?PRIVATE KEY-----$' "$NODE_KEY" || die "--node-key is not a PEM private key"
  if command -v openssl >/dev/null 2>&1; then
    openssl pkey -in "$NODE_KEY" -noout >/dev/null 2>&1 || die "--node-key could not be parsed as a private key"
  fi
else
  [[ -z "$NODE_KEY" && "$REPLACE_CREDENTIAL" == "0" && "$CREDENTIAL_MODE" == "baseline" ]] ||
    die "--node-key, --credential-mode and --replace-credential require --instance"
fi

STAGING=0
if [[ -n "$ROOT_PREFIX" ]]; then
  [[ "$ROOT_PREFIX" != "/" ]] || die "--destdir cannot be /"
  mkdir -p "$ROOT_PREFIX"
  ROOT_PREFIX="$(cd "$ROOT_PREFIX" && pwd -P)"
  STAGING=1
else
  [[ -z "$ASSUME_SYSTEMD_VERSION" ]] || die "--systemd-version is only allowed with --destdir"
  [[ "$(id -u)" == "0" ]] || die "run as root or use --destdir for staging"
  command -v systemctl >/dev/null 2>&1 || die "systemd/systemctl is required"
  command -v useradd >/dev/null 2>&1 || die "useradd is required"
fi

# systemd version and credential capability detection.
SYSTEMD_VERSION=""
if [[ -n "$ASSUME_SYSTEMD_VERSION" ]]; then
  SYSTEMD_VERSION="$ASSUME_SYSTEMD_VERSION"
elif command -v systemctl >/dev/null 2>&1; then
  SYSTEMD_VERSION="$(systemctl --version 2>/dev/null | awk 'NR == 1 { print $2 }')"
fi
if [[ -n "$INSTANCE" ]]; then
  [[ "$SYSTEMD_VERSION" =~ ^[0-9]+$ ]] || die "cannot determine the systemd version (got '${SYSTEMD_VERSION:-none}')"
  (( SYSTEMD_VERSION >= 247 )) || die "systemd $SYSTEMD_VERSION lacks LoadCredential=; systemd 247+ is required"
  if [[ "$CREDENTIAL_MODE" == "encrypted" ]]; then
    (( SYSTEMD_VERSION >= 250 )) || die "encrypted mode needs systemd 250+; found $SYSTEMD_VERSION. Use --credential-mode baseline (no automatic fallback)"
    [[ "$STAGING" == "0" ]] || die "encrypted mode needs a live host; it cannot be staged with --destdir"
    command -v systemd-creds >/dev/null 2>&1 || die "encrypted mode needs systemd-creds; none found"
    systemd-creds --help 2>/dev/null | grep -q 'encrypt' || die "systemd-creds on this host does not support encrypt"
  fi
fi

BIN_DIR="$ROOT_PREFIX/usr/local/bin"
UNIT_DIR="$ROOT_PREFIX/etc/systemd/system"
CONFIG_DIR="$ROOT_PREFIX/etc/baft-tunnel"
STATE_DIR="$ROOT_PREFIX/var/lib/baft-tunnel"
BACKUP_ROOT="$ROOT_PREFIX/var/backups"
BIN_DEST="$BIN_DIR/baft-tunnel"
UNIT_DEST="$UNIT_DIR/baft-tunnel@.service"
# Scripts and units for `baft-tunnel menu` (same relative layout as the release tree).
LIB_DEST="$ROOT_PREFIX/usr/local/lib/baft-tunnel"
ENGINE_STORE="$ROOT_PREFIX/usr/local/lib/baft-tunnel-engines"

# The engine store is installation-owned. Never follow or replace a symlink here.
if [[ -e "$ENGINE_STORE" || -L "$ENGINE_STORE" ]]; then
  [[ -d "$ENGINE_STORE" && ! -L "$ENGINE_STORE" ]] || die "refusing non-directory or symlinked engine store: $ENGINE_STORE"
fi

if [[ -z "$BINARY" ]]; then
  [[ -x "$BIN_DEST" && -f "$UNIT_DEST" ]] || die "binary/template not installed yet; pass --binary as well"
fi

if [[ -n "$INSTANCE" ]]; then
  CRED_ROOT="$CONFIG_DIR/credentials"
  CRED_DIR="$CRED_ROOT/$INSTANCE"
  HOST_CRED_DIR="/etc/baft-tunnel/credentials/$INSTANCE"
  if [[ "$CREDENTIAL_MODE" == "baseline" ]]; then
    CRED_NAME="node-key"; OTHER_NAME="node-key.cred"
    DROPIN_LINE="LoadCredential=$CREDENTIAL_ID:$HOST_CRED_DIR/$CRED_NAME"
  else
    CRED_NAME="node-key.cred"; OTHER_NAME="node-key"
    DROPIN_LINE="LoadCredentialEncrypted=$CREDENTIAL_ID:$HOST_CRED_DIR/$CRED_NAME"
  fi
  CRED_FILE="$CRED_DIR/$CRED_NAME"
  OTHER_FILE="$CRED_DIR/$OTHER_NAME"
  DROPIN_DIR="$UNIT_DIR/baft-tunnel@$INSTANCE.service.d"
  DROPIN="$DROPIN_DIR/credentials.conf"
  CONFIG_FILE="$CONFIG_DIR/$INSTANCE.json"

  # Refuse unsafe states before any change is made.
  for path in "$CRED_ROOT" "$CRED_DIR"; do
    [[ ! -L "$path" ]] || die "refusing symlinked credential directory: $path"
  done
  for path in "$CRED_FILE" "$OTHER_FILE" "$DROPIN"; do
    [[ ! -L "$path" ]] || die "refusing symlinked path: $path"
  done
  if [[ -e "$CONFIG_FILE" ]]; then
    if grep -Eq '"key"[[:space:]]*:' "$CONFIG_FILE"; then
      die "$CONFIG_FILE sets \"key\"; remove it and use \"key_credential\": \"$CREDENTIAL_ID\" (never both)"
    fi
    grep -Eq "\"key_credential\"[[:space:]]*:[[:space:]]*\"$CREDENTIAL_ID\"" "$CONFIG_FILE" ||
      echo "install: warning: $CONFIG_FILE does not set \"key_credential\": \"$CREDENTIAL_ID\"" >&2
  fi
  if [[ -e "$DROPIN" ]] && ! grep -Fqx "$DROPIN_MARKER" "$DROPIN"; then
    die "refusing to overwrite unmanaged drop-in: $DROPIN"
  fi
  CRED_UNCHANGED=0
  if [[ -e "$CRED_FILE" ]]; then
    if [[ "$CREDENTIAL_MODE" == "baseline" ]] && cmp -s "$NODE_KEY" "$CRED_FILE"; then
      CRED_UNCHANGED=1
    elif [[ "$REPLACE_CREDENTIAL" != "1" ]]; then
      die "a different credential already exists for $INSTANCE; pass --replace-credential to replace it"
    fi
  fi
  if [[ -e "$OTHER_FILE" && "$REPLACE_CREDENTIAL" != "1" ]]; then
    die "instance $INSTANCE has a credential in the other mode; pass --replace-credential to switch modes"
  fi
fi

mkdir -p "$BIN_DIR" "$UNIT_DIR" "$CONFIG_DIR/pki" "$STATE_DIR" "$BACKUP_ROOT"
chmod 0750 "$CONFIG_DIR" "$CONFIG_DIR/pki" "$STATE_DIR"

# A failed first install removes the account and group it created (they are not left behind).
CREATED_USER=0
CREATED_GROUP=0
remove_created_account() {
  if [[ "$CREATED_USER" == "1" ]]; then userdel "$SERVICE_USER" >/dev/null 2>&1 || true; fi
  if [[ "$CREATED_GROUP" == "1" ]]; then groupdel "$SERVICE_USER" >/dev/null 2>&1 || true; fi
}
if [[ "$STAGING" == "0" ]]; then
  trap 'remove_created_account' EXIT
  if ! getent group "$SERVICE_USER" >/dev/null 2>&1; then
    groupadd --system "$SERVICE_USER"
    CREATED_GROUP=1
  fi
  if ! id "$SERVICE_USER" >/dev/null 2>&1; then
    useradd --system --gid "$SERVICE_USER" --home-dir "$STATE_DIR" --shell /usr/sbin/nologin "$SERVICE_USER"
    CREATED_USER=1
  fi
  chown root:"$SERVICE_USER" "$CONFIG_DIR" "$CONFIG_DIR/pki"
  chown "$SERVICE_USER":"$SERVICE_USER" "$STATE_DIR"
fi

# mktemp -d creates the backup directory with mode 0700; it may hold a
# previous private key and must stay root-only.
BACKUP_DIR="$(mktemp -d "$BACKUP_ROOT/baft-tunnel-install.XXXXXX")"
chmod 0700 "$BACKUP_DIR"
if [[ -n "$BINARY" ]]; then
  : > "$BACKUP_DIR/scope-binary"
  [[ ! -e "$BIN_DEST" ]] || { cp -p "$BIN_DEST" "$BACKUP_DIR/baft-tunnel"; : > "$BACKUP_DIR/had-binary"; }
  [[ ! -e "$UNIT_DEST" ]] || { cp -p "$UNIT_DEST" "$BACKUP_DIR/baft-tunnel@.service"; : > "$BACKUP_DIR/had-unit"; }
  [[ ! -d "$LIB_DEST" ]] || { cp -a "$LIB_DEST" "$BACKUP_DIR/lib"; : > "$BACKUP_DIR/had-lib"; }
fi
if [[ -n "$BAFT_ENGINE" ]]; then
  : > "$BACKUP_DIR/scope-engine-store"
  ENGINE_DIR="$ENGINE_STORE/baft"
  if [[ -e "$ENGINE_STORE" || -L "$ENGINE_STORE" ]]; then
    [[ -d "$ENGINE_STORE" && ! -L "$ENGINE_STORE" ]] || die "refusing unsafe engine store root: $ENGINE_STORE"
  fi
  if [[ -e "$ENGINE_DIR" || -L "$ENGINE_DIR" ]]; then
    [[ -d "$ENGINE_DIR" && ! -L "$ENGINE_DIR" ]] || die "refusing unsafe BAFT engine directory: $ENGINE_DIR"
    : > "$BACKUP_DIR/had-engine-dir"
    for pointer in current previous; do
      link="$ENGINE_DIR/$pointer"
      if [[ -e "$link" || -L "$link" ]]; then
        [[ -L "$link" ]] || die "refusing non-symlink engine pointer: $link"
        target="$(readlink -- "$link")"
        [[ "$target" =~ ^releases/[0-9a-f]{64}$ ]] || die "refusing malformed engine pointer: $link"
        printf '%s\\n' "$target" > "$BACKUP_DIR/engine-$pointer-target"
      fi
    done
  fi
fi
if [[ -n "$INSTANCE" ]]; then
  printf '%s\n' "$INSTANCE" > "$BACKUP_DIR/scope-credential"
  mkdir -m 0700 "$BACKUP_DIR/credential"
  for name in node-key node-key.cred; do
    [[ ! -e "$CRED_DIR/$name" ]] || (umask 077; cp -p "$CRED_DIR/$name" "$BACKUP_DIR/credential/$name")
  done
  [[ ! -e "$DROPIN" ]] || cp -p "$DROPIN" "$BACKUP_DIR/credential/credentials.conf"
fi

SUCCESS=0
CRED_TOUCHED=0
restore_credential() {
  local name
  for name in node-key node-key.cred; do
    rm -f "$CRED_DIR/$name.new.$$"
    if [[ -e "$BACKUP_DIR/credential/$name" ]]; then
      cp -p "$BACKUP_DIR/credential/$name" "$CRED_DIR/$name"
    else
      rm -f "$CRED_DIR/$name"
    fi
  done
  rm -f "$DROPIN.new.$$"
  if [[ -e "$BACKUP_DIR/credential/credentials.conf" ]]; then
    cp -p "$BACKUP_DIR/credential/credentials.conf" "$DROPIN"
  else
    rm -f "$DROPIN"
    rmdir "$DROPIN_DIR" 2>/dev/null || true
  fi
}
rollback_install() {
  local status=$?
  trap - EXIT
  if [[ -n "$BINARY" ]]; then rm -f "$BIN_DEST.new.$$" "$UNIT_DEST.new.$$"; fi
  if [[ "$SUCCESS" != "1" ]]; then
    if [[ -n "$BINARY" ]]; then
      if [[ -f "$BACKUP_DIR/had-binary" ]]; then cp -p "$BACKUP_DIR/baft-tunnel" "$BIN_DEST"; else rm -f "$BIN_DEST"; fi
      if [[ -f "$BACKUP_DIR/had-unit" ]]; then cp -p "$BACKUP_DIR/baft-tunnel@.service" "$UNIT_DEST"; else rm -f "$UNIT_DEST"; fi
      rm -rf "$LIB_DEST.new.$$"
      if [[ -f "$BACKUP_DIR/had-lib" ]]; then rm -rf "$LIB_DEST"; cp -a "$BACKUP_DIR/lib" "$LIB_DEST"; else rm -rf "$LIB_DEST"; fi
    fi
    if [[ -n "$BAFT_ENGINE" ]]; then
      ENGINE_DIR="$ENGINE_STORE/baft"
      if [[ -f "$BACKUP_DIR/had-engine-dir" ]]; then
        install -d -m 0755 "$ENGINE_DIR"
        for pointer in current previous; do
          link="$ENGINE_DIR/$pointer"
          target_file="$BACKUP_DIR/engine-$pointer-target"
          tmp="$ENGINE_DIR/.$pointer.rollback.${BASHPID}"
          rm -f -- "$tmp"
          if [[ -f "$target_file" ]]; then
            target="$(cat "$target_file")"
            [[ "$target" =~ ^releases/[0-9a-f]{64}$ ]] || die "invalid saved engine pointer: $pointer"
            ln -s -- "$target" "$tmp"
            mv -Tf -- "$tmp" "$link"
          else
            rm -f -- "$link"
          fi
        done
      else
        rm -rf -- "$ENGINE_DIR"
      fi
    fi
    if [[ "$STAGING" == "0" ]]; then remove_created_account; fi
    if [[ "$CRED_TOUCHED" == "1" ]]; then restore_credential; fi
    if [[ "$STAGING" == "0" ]]; then systemctl daemon-reload >/dev/null 2>&1 || true; fi
    echo "install: failed; previous files restored" >&2
  fi
  exit "$status"
}
trap rollback_install EXIT

if [[ -n "$BINARY" ]]; then
  install -m 0755 "$BINARY" "$BIN_DEST.new.$$"
  mv -f "$BIN_DEST.new.$$" "$BIN_DEST"
  install -m 0644 "$UNIT_SOURCE" "$UNIT_DEST.new.$$"
  mv -f "$UNIT_DEST.new.$$" "$UNIT_DEST"
  # Copy only the operator scripts and unit files (no tests, no panel QA tooling).
  rm -rf "$LIB_DEST.new.$$"
  install -d -m 0755 "$LIB_DEST.new.$$/scripts/pki" "$LIB_DEST.new.$$/systemd"
  for f in "$SCRIPT_DIR"/*.sh "$SCRIPT_DIR"/pki/*.sh; do
    case "$(basename "$f")" in test-*|test.sh|apply-branch-protection.sh|build-baft-engine.sh|baft-validate-golden.sh) continue ;; esac
    rel="${f#"$SCRIPT_DIR"/}"
    install -m 0755 "$f" "$LIB_DEST.new.$$/scripts/$rel"
  done
  cp -R "$SCRIPT_DIR/../systemd/." "$LIB_DEST.new.$$/systemd/"
  chmod -R u=rwX,go=rX "$LIB_DEST.new.$$"
  rm -rf "$LIB_DEST"
  mv -f "$LIB_DEST.new.$$" "$LIB_DEST"
fi

if [[ -n "$BAFT_ENGINE" ]]; then
  # Root-owned store outside LIB_DEST (which every --binary install replaces). The engine
  # store refuses a binary whose SHA-256 differs from the manifest built into BIN_DEST.
  install -d -m 0755 "$ENGINE_STORE"
  if [[ "$STAGING" == "0" ]]; then chown root:root "$ENGINE_STORE"; fi
  "$BIN_DEST" engine manifest > "$BACKUP_DIR/engine-manifest.json"
  "$BIN_DEST" engine install --store "$ENGINE_STORE" --manifest "$BACKUP_DIR/engine-manifest.json" \
    --id baft --binary "$BAFT_ENGINE"
  "$BIN_DEST" engine verify --store "$ENGINE_STORE" --id baft
fi

if [[ -n "$INSTANCE" ]]; then
  CRED_TOUCHED=1
  OWNER_ARGS=()
  if [[ "$STAGING" == "0" ]]; then OWNER_ARGS=(-o root -g root); fi
  install -d "${OWNER_ARGS[@]}" -m 0700 "$CRED_ROOT" "$CRED_DIR"
  chmod 0700 "$CRED_ROOT" "$CRED_DIR"
  if [[ "$CRED_UNCHANGED" == "0" ]]; then
    if [[ "$CREDENTIAL_MODE" == "baseline" ]]; then
      (umask 077; install "${OWNER_ARGS[@]}" -m 0600 "$NODE_KEY" "$CRED_FILE.new.$$")
    else
      (umask 077; systemd-creds encrypt --name="$CREDENTIAL_ID" --with-key=auto "$NODE_KEY" "$CRED_FILE.new.$$")
      chown root:root "$CRED_FILE.new.$$"
      chmod 0600 "$CRED_FILE.new.$$"
    fi
    mv -f "$CRED_FILE.new.$$" "$CRED_FILE"
  fi
  if [[ "$STAGING" == "0" ]]; then chown root:root "$CRED_FILE"; fi
  chmod 0600 "$CRED_FILE"
  # Never leave a plaintext key behind after switching to encrypted mode (or
  # a stale encrypted blob after switching back); it is kept in the backup.
  rm -f "$OTHER_FILE"
  install -d -m 0755 "$DROPIN_DIR"
  printf '%s\n# Contains no secrets. Mode: %s\n[Service]\n%s\n' \
    "$DROPIN_MARKER" "$CREDENTIAL_MODE" "$DROPIN_LINE" > "$DROPIN.new.$$"
  chmod 0644 "$DROPIN.new.$$"
  mv -f "$DROPIN.new.$$" "$DROPIN"
fi

if [[ "$STAGING" == "0" ]]; then
  if command -v systemd-analyze >/dev/null 2>&1 && [[ -n "$BINARY" ]]; then
    systemd-analyze verify "$UNIT_DEST"
  fi
  systemctl daemon-reload
fi

SUCCESS=1
if [[ -t 1 && -f "$SCRIPT_DIR/menu.sh" ]]; then bash "$SCRIPT_DIR/menu.sh" --banner; fi
if [[ -n "$BINARY" ]]; then echo "Installed $BIN_DEST and $UNIT_DEST"; echo "Menu: sudo baft-tunnel menu (Persian/English)"; fi
if [[ -n "$INSTANCE" ]]; then
  echo "Credential: $CREDENTIAL_MODE mode for instance $INSTANCE (systemd ${SYSTEMD_VERSION})"
  echo "  source:  $CRED_FILE (root:root 0600, directory 0700)$([[ "$CRED_UNCHANGED" == "1" ]] && echo ', unchanged')"
  echo "  drop-in: $DROPIN"
  echo "  config must use \"key_credential\": \"$CREDENTIAL_ID\" and must not set \"key\"."
  if [[ "$CREDENTIAL_MODE" == "baseline" ]]; then
    echo "  note: baseline credentials are plaintext at rest, protected by root-only permissions."
  fi
  if [[ -e "$BACKUP_DIR/credential/node-key" || -e "$BACKUP_DIR/credential/node-key.cred" ]]; then
    echo "  note: the previous credential is kept in the root-only backup below; delete it once rollback is no longer needed."
  fi
fi
echo "Backup: $BACKUP_DIR"
if [[ -n "$REGISTER" ]]; then
  REG_ARGS=(--instance "$REGISTER")
  [[ -z "$AGENT_LISTEN" ]] || REG_ARGS+=(--listen "$AGENT_LISTEN")
  [[ "$STAGING" == "0" ]] || REG_ARGS+=(--destdir "$ROOT_PREFIX")
  "$SCRIPT_DIR/install-agent-control.sh" "${REG_ARGS[@]}"
fi
echo "No tunnel was enabled, started, or restarted."
if [[ -n "$BINARY" ]]; then
  NEXT_NAME="${REGISTER:-main}"
  echo
  echo "Next step: pair this node with the other node (run on both, one with --role serve):"
  echo "  sudo baft-tunnel menu   ->  Pair the two nodes"
  echo "  or: sudo baft-tunnel setup init --engine noise-tcp --role forward|serve --direction direct \\"
  echo "        --node-id THIS_NODE --peer-id OTHER_NODE --tunnel-id $NEXT_NAME --service-port PORT \\"
  echo "        [--peer-host OTHER_NODE_IP  (forward only)] --instance $NEXT_NAME"
fi
