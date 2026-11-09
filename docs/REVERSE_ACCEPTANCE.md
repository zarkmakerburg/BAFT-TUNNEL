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

Accepted for display as **Preview** on 2026-11-09. **Not Stable.**

LAB run: [42619848756](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42619848756), main `8fde7db`, clean install of a local build (binary sha256 `8fbbf657...2dc5b`) on both shared LAB hosts through the hardened non-root units and the local agent.

| Criterion | Result |
|---|---|
| Setup `--direction reverse`, Apply start (public node first, then service node) | applied on both; `non_root=1`; `outbound_only=1` (service node has no listener) |
| Data integrity | 64 KiB and 2 MiB echo sha256 match, before and after update |
| Stability | 1800 s, 138 rounds, 0 failed probes, max probe 4 s, fd 12→12 / 10→10, RSS +108 kB each, 0 restarts |
| Reconnect | service SIGKILL 4 s, public SIGKILL 4 s, stop + Apply restart 3 s each |
| Diagnostics, status, traffic | both nodes: secure_channel, identity, authorization, session, data_path `pass`; state `carrying_traffic`; counters equal both ways; integrity `unknown` by design |
| Backup / restore | owners and modes identical, recovery 3 s |
| Update | guarded update ok (3 s); failing binary rolled back (3 s) |
| Revocation | revoked service certificate refused; tiers `secure_channel` and `session` `fail` |
| Uninstall | clean; listeners and units unchanged |
| `secure_channel stage failed` counter | public/service 0/0 after soak and 0/0 after update (this run only) |

### Owner finding: counters 486 / 524

Cause: the local agent samples the tunnel port every 5 s with a bare TCP connect that closes without data, and the server logged each one as a failed secure channel (about 17k per day of uptime, so hundreds per soak). Not a tunnel fault. Fixed in PR #125 (a connection that closes before sending a byte is not a handshake attempt). The harness now counts per run and asserts 0/0, which the evidence run above shows. The service node always had 0.

### Defects found and fixed on the way

- #125: agent probe noise (above).
- #129: `data_path` stayed `unknown` when a short echo straddled two 5 s sampling windows (run 42604383181 failed on this). Growth in both directions within 10 s now counts.
- #126, #127: LAB harness SSH retry and stale-host cleanup (harness only).

### What may be shown

- **Preview**: Reverse with `tcp-tls`, in the Wizard and capability contract, labelled Preview.
- **Not allowed**: Stable; "Ready" (the receipt reports `ready=unknown` by design, so no Ready state may be claimed); production-ready; any Reverse for `noise-tcp` (stays disabled) or `baft` (no Reverse).

### Still open (why not Stable)

- No multi-hour soak (1800 s here, 1200 s in R3).
- Cross-version update not tested (`cross_version_update=0`).
- Single LAB pair; no real packet-loss run in this gate (covered in R3).
- Unrelated firewall drift on the abroad host during the run (3 lines of an nft set, not BAFT-TUNNEL rules; `baft_fw_rules=0`).
- Wizard role labels ("Hosts the service" / "User entry point") should be checked for Reverse wording before public display.
