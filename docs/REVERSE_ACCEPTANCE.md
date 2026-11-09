# Reverse (tcp-tls) acceptance before public display in the Wizard

Scope: Reverse with the `tcp-tls` engine, currently Preview (ADR-0021 Amendment 1). `noise-tcp` Reverse stays disabled; the `baft` engine does not support Reverse. This is a separate gate from P0-4 (core data path, run 42525790470), which is not repeated.

## Acceptance criteria

All must pass on the two shared LAB hosts, on an exact `main` SHA, with run evidence. Stage: `lab/real-hosts/reverse-accept-stage.sh` (`reverse_accept=<soak seconds>`).

| # | Criterion | Check |
|---|-----------|-------|
| R1 | Installed product path | Clean install, `setup init/join --direction reverse`, generated configs describe Reverse (public node: listen + ingress; service node: target + remote), Apply start through the non-root local agent into hardened `baft-tunnel@` units running as `baft-tunnel` |
| R2 | Outbound-only | Service node has no listener on the tunnel port and holds an outbound connection to the public node |
| R3 | Integrity | 64 KiB and 2 MiB echo through the user ingress match |
| R4 | Observability | `/v1/diagnostics`, `/v1/status`, `/v1/traffic` on both nodes agree with the measured traffic; tiers secure_channel, identity, authorization, session, data_path pass; receipt does not claim `ready` |
| R5 | Stability | Soak (`reverse_accept` seconds, target 1800): 0 failed probes, 0 unit restarts, bounded fd and RSS growth |
| R6 | Reconnect | SIGKILL of each node and stop + Apply restart of each node recover within 90 s |
| R7 | Backup/restore | `backup.sh`, simulated loss, `restore.sh`: content, owners and modes identical (non-root key ownership survives), tunnel works after restart |
| R8 | Update/rollback | `update.sh` to a new digest keeps the tunnel; a binary that fails at start is rolled back automatically and the tunnel recovers |
| R9 | Revocation | Revoked service-node certificate is refused; diagnostics attribute it to the secure channel |
| R10 | Cleanup | Uninstall leaves no BAFT-TUNNEL resources; listeners and unit files unchanged; firewall drift is reported, never hidden |

Not covered (stated limits): cross-version update (no earlier release has Reverse Preview plus the hardened agent), multi-hour soak, hostile network paths beyond P0-4 R3, other operating systems.

## Verdict

Pending LAB run. See HQ Issue #2 and the Roadmap once recorded.
