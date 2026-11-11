# Troubleshooting

> Current operator guidance for the signed RC.7 Direct path accepted in trusted two-host U7 [run 43063199428](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/43063199428). The product is still pre-release. Use the bilingual [quick starts](quickstart.en.md) and [Operator Guide](OPERATOR_GUIDE.md) for the complete procedure.

## First checks (both hosts)

1. Open the installed menu: `sudo baft-tunnel menu` → **Status and diagnostics**.
2. Check the selected instance's service and recent logs:
   ```bash
   sudo systemctl status baft-tunnel@NAME
   sudo journalctl -u baft-tunnel@NAME -n 100 --no-pager
   ```
3. Run **Run diagnostics** on both nodes. Read the first failing/unknown tier; later tiers depend on earlier ones.
4. Do not treat a listening TCP port, process, session or successful Apply receipt as proof of a healthy data path. The receipt's `ready` and ordinary diagnostics' `integrity` remain Unknown by design.

## Diagnostics tiers

### Unit state is Unknown / cannot be read

- Check the exact instance name and `systemctl status baft-tunnel@NAME`.
- Read the unit journal for permission, config-path, credential or startup errors.
- Confirm the Agent is installed and registered for that instance; use the menu rather than manually starting a second Agent process.
- Rerun diagnostics after correcting the unit issue. Unknown is not Pass.

### TCP reachability fails or is Unknown

- Confirm the serve node is running and listening on the configured tunnel port: `sudo ss -lntp`.
- Confirm the forward node uses the correct reachable IP/DNS and tunnel port.
- Check provider/network firewall and routing rules yourself. BAFT-TUNNEL does not inspect or mutate firewall rules; do not run unreviewed firewall commands.
- A TCP probe proves only that something answered at that address/port; it does not identify the peer or prove a tunnel.

### Secure channel fails

- Confirm both nodes use the same supported engine and Direct direction.
- For tcp-tls, confirm the CA certificate and current CRL match, the leaf certificates are valid for client/server use, and the certificate names/SANs match the configured node identities and server name.
- Check CRL freshness with the documented `crl-status.sh` procedure. Renew on the offline CA and follow the guarded push/activation procedure; never disable certificate or CRL validation to make a connection work.
- For Experimental noise-tcp, confirm the pinned peer key matches the trusted pairing bundle. Do not treat Noise evidence as acceptance of the supported tcp-tls path.

### Identity fails

- Compare the configured peer ID with the certificate URI SAN (tcp-tls) or enrolled Noise public key.
- Compare fingerprints/checksums out of band using a separate trusted channel.
- If identity material changed, stop and perform the documented re-issuance/re-pairing procedure. Never copy a private key from one node to another.

### Authorization fails

- Confirm both nodes use the same tunnel ID and Direct direction, with opposite serve/forward roles.
- Confirm the configured peer identity is the intended node and its certificate is not revoked.
- Do not remove authorization checks or change direction just to bypass a refusal.

### Session is missing

- Check that both instances are started from the installed menu and that the serve listener is reachable.
- Read both node journals; the first failing earlier tier is usually the blocker.
- Start/restart only the intended instance through the menu and rerun diagnostics on both nodes.

### Data path is Unknown or not carrying traffic

- Confirm the local destination service is running on the serve host and bound to the configured loopback/service port.
- Generate application traffic through the forward-side local port, then rerun diagnostics.
- Check the configured destination and port on both nodes. The remote peer cannot choose an arbitrary destination.
- A session without bidirectional payload evidence is not a passing data path.

### Integrity is Unknown

This is expected in ordinary product diagnostics. The current diagnostics API does not perform an authenticated payload-integrity challenge; do not relabel a successful TCP probe or session as integrity PASS. U7 separately verified 64 KiB and approximately 2 MiB payload hashes in the trusted LAB. B2 (an in-product integrity probe) remains future work.

## Backup, update and restore

- Use the menu's Backup / restore and Update / rollback actions.
- Verify the downloaded release signature and checksum before installing an update.
- A guarded update records a backup and automatically restores the prior binary/unit if its start check fails. Inspect service status and logs before retrying.
- Manual rollback is available with `sudo scripts/rollback.sh BACKUP_DIR`; use the exact backup directory printed by the installer.
- Restore replaces configuration and keys and stops affected tunnels first. Verify the restored instance and peer connection afterwards.

## Revocation and CRL

- Expired or invalid CRLs fail closed. Do not bypass this validation.
- Keep the CA key offline. Renew the CRL on the offline CA, verify it, and use the documented operator push/activation sequence.
- U1 real two-host evidence is [run 42656507521](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42656507521). U7 did not repeat CRL push (`crl_push=0)); these are separate acceptance records.

## Recovery and scope limits

- Active TCP streams are not transparently resumed after carrier loss. The application must reconnect after the tunnel is restored.
- Reverse remains Preview; noise-tcp is Experimental Direct only; BAFT is Preview. This quickstart covers tcp-tls Direct only.
- No firewall automation, stable release or production deployment is claimed. U7 observed unrelated six-line address-set drift on the abroad LAB host; its source remains open as S10.
- Never share Agent tokens, private keys, CA passphrases or unredacted support material. The Agent token is a credential.

## Docker/netem simulation (historical test tooling)

- If Docker Compose is missing, install the Compose plugin for Docker Engine.
- If `tc` reports `Operation not permitted`, confirm the simulation containers have `NET_ADMIN`.
- Run `sh lab/scripts/clear-netem.sh` before stopping the simulation to remove qdisc impairment.
- The netem scripts target only the two Compose containers. Do not adapt them to production hosts.

## Six-host preflight (historical test tooling)

- Confirm all six host variables are present and unique.
- Confirm the dedicated SSH key is readable and the pinned `known_hosts` file is non-empty.
- For a non-default SSH port, use `[host]:port` entries in `known_hosts`.
- A refused TCP connection means the host answered but no listener accepted the port; a timeout may mean filtering or loss.
- The preflight requires Bash and Python 3 on each remote host.
