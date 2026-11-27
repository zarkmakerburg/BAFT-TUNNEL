#!/usr/bin/env bash
set -Eeuo pipefail

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
stage="$root/lab/real-hosts/auto-pair-stage.sh"
phase1="$root/lab/real-hosts/phase1-two-host-e2e.sh"
install_helper="$root/lab/real-hosts/auto-pair-install-host.sh"
cleanup_helper="$root/lab/real-hosts/auto-pair-cleanup-host.sh"
workflow="$root/.github/workflows/phase1-trusted-pairing-lab.yml"
lab_config_workflow="$root/.github/workflows/phase0.yml"
phase0_workflow="$root/.github/workflows/phase0-two-host-lab.yml"
inventory_workflow="$root/.github/workflows/phase0-read-only-inventory.yml"
fleet_preflight_workflow="$root/.github/workflows/fleet-join-lab-preflight.yml"

fail() { echo "auto-pair safety contract: $*" >&2; exit 1; }
contains() { grep -Fq -- "$2" "$1" || fail "missing required guard: $2"; }
not_contains() { ! grep -Fq -- "$2" "$1" || fail "unsafe broad cleanup remains: $2"; }

# A pairing run must refuse hosts with any pre-existing BAFT installation or state.
for guard in \
  '/var/lib/baft-tunnel-agent' \
  '/var/lib/baft-tunnel-control' \
  '/run/baft-tunnel' \
  'getent passwd baft-tunnel' \
  'getent passwd baft-tunnel-agent' \
  'getent group baft-tunnel' \
  'getent group baft-tunnel-agent' \
  'baft-tunnel-install.*' \
  'baft-tunnel-backup.*'; do
  contains "$stage" "$guard"
done

# Host-side scripts own their argument parsing; cleanup is scoped and fail-closed.
contains "$stage" 'auto-pair-install-host.sh'
contains "$stage" 'auto-pair-cleanup-host.sh'
contains "$stage" 'AP_BACKUPS_iran'
contains "$stage" 'AP_BACKUPS_abroad'
contains "$stage" 'AGD=/var/lib/baft-tunnel-agent;'
contains "$stage" 'LAB_PRESERVE_REMOTE=1'
contains "$install_helper" 'installed-lib.sha256'
contains "$install_helper" 'account-baft-tunnel.passwd'
contains "$install_helper" 'account-baft-tunnel-agent.passwd'
contains "$install_helper" 'account-baft-tunnel.group.before'
contains "$install_helper" 'pre-existing BAFT resource'
contains "$install_helper" 'package directory is missing or unsafe'
contains "$install_helper" '--listen "127.0.0.1:$API_PORT"'
contains "$install_helper" '.baft-tunnel-lab-run-owner'
contains "$install_helper" 'agent-token.sha256'
contains "$install_helper" 'agent-listen-$INSTANCE.sha256'
contains "$install_helper" 'systemctl is-active --quiet "baft-tunnel-agent@$INSTANCE.service"'
not_contains "$install_helper" 'nohup'
not_contains "$install_helper" 'agent --config'
contains "$cleanup_helper" 'systemctl reset-failed "baft-tunnel@$instance.service"'
contains "$cleanup_helper" 'rmdir -- "$backup_dir"'
contains "$cleanup_helper" 'scope-binary'
contains "$cleanup_helper" 'backup contains unexpected pre-existing resources'
contains "$cleanup_helper" 'pairing-state.json'
contains "$cleanup_helper" 'installed-lib-dirs.txt'
contains "$cleanup_helper" 'install-backup-$instance.path'
contains "$cleanup_helper" '1000.pem 1001.pem'
contains "$cleanup_helper" 'unexpected agent state files remain'
contains "$cleanup_helper" 'agent-token.sha256'
contains "$cleanup_helper" 'package directory is not root-owned'
contains "$cleanup_helper" 'config root ownership or mode changed'
contains "$cleanup_helper" 'at least one explicit --instance is required'
contains "$cleanup_helper" 'no run-owned installer backup path was supplied or recovered'
contains "$cleanup_helper" 'unit remains active; preserving cleanup target'
contains "$cleanup_helper" '--echo-port'
contains "$cleanup_helper" 'ap-echo.pid'
contains "$cleanup_helper" '[p]ython3 $REMOTE_DIR/echo.py $AUTO_PAIR_ECHO_PORT'
not_contains "$cleanup_helper" 'pgrep -f "[p]ython3 $REMOTE_DIR/echo.py"'
contains "$stage" 'ap-echo.pid'
contains "$stage" '--echo-port "$AP_SVC"'
not_contains "$stage" 'echo2.pid'
contains "$stage" 'AP_PROBE_COOLDOWN=60'
contains "$stage" 'v.get("result") == "unknown" and v.get("reason") == "rate_limited"'
contains "$stage" 'sleep "$((AP_PROBE_COOLDOWN + 1))"'
contains "$stage" 'assert v.get("result") == "pass", v'
contains "$stage" 'assert v.get("sha256_match") is True and v.get("bytes") == 65536, v'
contains "$root/src/integrity_probe.rs" 'pub const PROBE_COOLDOWN: Duration = Duration::from_secs(60);'
contains "$cleanup_helper" 'control directory ownership or mode changed'
contains "$cleanup_helper" 'unexpected control request type; preserving'
contains "$cleanup_helper" 'for suffix in start stop enable disable setup setup-result pki pki-result pair pair-approve pair-status fleet'
not_contains "$cleanup_helper" 'touch "$CONTROL_DIR/$instance.stop"'
not_contains "$cleanup_helper" 'find "$CONTROL_DIR" -maxdepth 1 -type f -name "$instance.*" -delete'
not_contains "$cleanup_helper" 'rm -f -- "$EB/credentials/$instance/node-key"'
contains "$cleanup_helper" 'agent listen drop-in changed; preserving it'
contains "$cleanup_helper" 'agent-listen-$instance.sha256'
contains "$cleanup_helper" 'agent instance state has no run-ownership marker'
contains "$cleanup_helper" 'rm -rf -- "$instance_dir"'
not_contains "$cleanup_helper" 'rm -rf "$AGENT_STATE_DIR"'
contains "$cleanup_helper" 'run-owned manifest ownership or mode changed'
contains "$cleanup_helper" 'passwd manifest changed'
contains "$cleanup_helper" 'group manifest changed'
contains "$cleanup_helper" 'service group did not return to its original membership'
contains "$cleanup_helper" 'account changed before cleanup; preserving installation'
not_contains "$cleanup_helper" 'rm -rf /var/backups'
not_contains "$cleanup_helper" 'rm -rf /var/lib/baft-tunnel-agent'
contains "$cleanup_helper" 'installed-lib.sha256'
contains "$cleanup_helper" 'account-$account.passwd'
contains "$cleanup_helper" 'account-baft-tunnel.group.before'
contains "$cleanup_helper" 'uninstall.sh" --stop'
contains "$phase1" 'LAB_PRESERVE_REMOTE'
contains "$phase1" 'result=preserved'
contains "$phase1" 'E2E_REMOTE_SECRET_SCRUB'
contains "$phase1" 'iran.key abroad.key control.key revabroad.key'
contains "$phase1" 'LAB_AUTO_PAIR requires root SSH on both hosts'
contains "$phase1" 'LAB run directory already exists; refusing to reuse it'
contains "$phase1" 'root:root 700'
not_contains "$cleanup_helper" '--purge-config'
not_contains "$cleanup_helper" 'rm -rf /var/backups/baft-tunnel-backup.*'
not_contains "$cleanup_helper" "systemctl reset-failed 'baft-tunnel*'"

# The LAB candidate must be the exact signed release binary, not a fresh source build.
contains "$workflow" 'EXPECTED_RELEASE_TAG: v0.3.1-rc.1'
contains "$workflow" 'EXPECTED_CANDIDATE_SHA: 4c2a5bc306030e647e4f62b009082ff7fa8fe096'
contains "$workflow" 'EXPECTED_BINARY_SHA256: 38404ee3b5317a8a6c37b84e75d0d5f9dd6fc45b50266467fb261d6d6410aa9d'
for wf in "$workflow" "$phase0_workflow" "$inventory_workflow" "$fleet_preflight_workflow"; do
  contains "$wf" 'group: baft-tunnel-trusted-lab-hosts'
  contains "$wf" 'cancel-in-progress: false'
done
contains "$workflow" 'gh release download'
contains "$workflow" 'cosign verify-blob SHA256SUMS --bundle SHA256SUMS.cosign.bundle'
contains "$workflow" '--certificate-identity "$identity"'
contains "$workflow" '--certificate-oidc-issuer https://token.actions.githubusercontent.com'
contains "$workflow" 'sha256sum --check --ignore-missing SHA256SUMS'
contains "$lab_config_workflow" 'test-auto-pair-integrity-retry.sh'
contains "$workflow" 'baft-tunnel-v0.3.1-rc.1-linux-x86_64'
not_contains "$workflow" 'cargo +1.88.0 build --locked --release'

contains "$cleanup_helper" 'MANAGE_DIR=/run/baft-tunnel/manage'
contains "$cleanup_helper" 'lib-runtime-children.sh'
contains "$cleanup_helper" 'baft_runtime_children_known /run/baft-tunnel "$CONTROL_DIR" "$MANAGE_DIR"'
contains "$cleanup_helper" 'baft_manage_dir_known "$MANAGE_DIR"'
contains "$cleanup_helper" 'manage runtime directory survived uninstall'
contains "$stage" 'lab/real-hosts/lib-runtime-children.sh'
contains "$stage" 'lab/real-hosts/auto-pair-cleanup-host.sh'
not_contains "$cleanup_helper" 'rm -rf /run/baft-tunnel'
bash "$root/lab/real-hosts/test-runtime-children.sh"

agent_userdel_line="$(grep -nF 'userdel baft-tunnel-agent'  "$cleanup_helper" | cut -d: -f1)"
service_userdel_line="$(grep -nF 'userdel baft-tunnel' "$cleanup_helper" | tail -1 | cut -d: -f1)"
[[ -n "$agent_userdel_line" && -n "$service_userdel_line" && "$agent_userdel_line" -lt "$service_userdel_line" ]] ||
  fail "agent account must be deleted before the service account"

install_agent_line="$(grep -nF 'bash "$PKG/scripts/install-agent-control.sh"' "$install_helper" | cut -d: -f1)"
service_group_after_line="$(grep -nF 'getent group baft-tunnel > "$REMOTE_DIR/account-baft-tunnel.group"' "$install_helper" | cut -d: -f1)"
[[ -n "$install_agent_line" && -n "$service_group_after_line" && "$service_group_after_line" -gt "$install_agent_line" ]] ||
  fail "service-group manifest must capture the agent supplementary membership"

echo 'Automatic pairing cleanup and signed-candidate safety contract: PASS'
