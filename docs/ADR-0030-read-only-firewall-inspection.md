# ADR-0030: Read-only firewall visibility for a configured tunnel

**Status:** Proposed — design only; owner decision and security review required before implementation.  
**Scope:** Phase 1 B3, local Agent firewall evidence for one registered tunnel instance.  
**Builds on:** ADR-0016 (Ubuntu 22.04 / systemd 249), ADR-0022 (Apply / Verify / Receipt), ADR-0023 (fixed-command control path), ADR-0024 (operations evidence), ADR-0021 (Direct / Reverse connection roles), and `docs/READINESS_CONTRACT.md`.

## 1. Context

The current Agent performs read-only host Preflight but does not inspect firewall policy. It must not infer firewall state from a listening port, a successful TCP connect, a service status, or the absence of a known firewall manager. The firewall tier is currently unavailable / Unknown.

B3 proposes a bounded observation of the local host firewall policy relevant to the configured tunnel endpoint. It must not create, edit, enable, disable, reload, flush, or otherwise mutate firewall rules or firewall services.

The product distinguishes:
- **Firewall evidence:** a bounded observation of the local host's relevant firewall policy.
- **Peer reachability:** a separate network observation.
- **Tunnel session authorization:** a separate authenticated-session observation.
- **Data-path integrity and Ready:** separate evidence tiers; a firewall result must never promote either.

This ADR does not turn a local ruleset inspection into proof that packets traverse the network or that a peer can connect.

## 2. Goals and non-goals

### Goals
1. Expose a truthful `firewall` tier: `pass`, `fail`, or `unknown`.
2. Inspect only the firewall path relevant to the registered instance's tunnel socket and TCP port.
3. Use a single fixed, read-only privileged operation through the existing per-instance systemd path-unit boundary.
4. Return a small, privacy-safe evidence summary; never return the full host ruleset to the Agent or panel.
5. Preserve Unknown when the ruleset cannot be interpreted completely and unambiguously.

### Non-goals
- Firewall configuration, repair, remediation, rule creation/deletion, service reload, or package installation.
- A generic command runner, shell, arbitrary binary, arbitrary arguments, or caller-supplied ruleset filter.
- A complete host firewall/security audit, packet capture, port scan, external reachability check, or end-to-end tunnel test.
- Claims that an accepted firewall path means the tunnel is Ready or that the peer is reachable.
- Reading or returning unrelated host rules, interface inventory, process data, or other tenants' firewall details.
- Supporting every firewall manager or every nftables expression in the first release.

## 3. Proposed control boundary

Add a dedicated firewall inspection helper, separate from lifecycle, setup and PKI helpers:

1. The local Agent accepts an authenticated, instance-bound read request (proposed API: `GET /v1/firewall`). The request carries no command, port, family, address, file path or firewall-manager choice.
2. The Agent derives the registered instance name from its own bound configuration. It writes one bounded request file, `/run/baft-tunnel/control/NAME.firewall`.
3. A dedicated root-owned `baft-tunnel-control-firewall@NAME.path` triggers one oneshot service whose only `ExecStart` is `/usr/local/bin/baft-tunnel firewall helper --instance NAME`. That closed CLI subcommand validates the request and registered instance, then invokes the single fixed nftables subprocess in §4. Neither the service nor the helper invokes `systemctl`, a shell, sudo, a package manager, or any firewall write operation.
4. The helper reads the installed, service-owned configuration for exactly `NAME`; it does not trust caller-supplied configuration or a caller-selected port.
5. The helper writes a bounded result receipt, `NAME.firewall-result`, using the same symlink refusal, regular-file, ownership, mode and per-instance binding protections used by the existing setup / PKI helper paths.
6. The Agent reads and validates the receipt, then exposes the evidence through the existing authenticated loopback API. No raw ruleset is exposed.
7. Requests are serialized per instance and rate-limited. A missing helper, stale/malformed receipt, timeout, permission error or unsupported backend returns `unknown`.

The helper must be installed and removed per registered instance through the existing control installer. It must not widen the permissions of the existing lifecycle, setup or PKI operations.

## 4. Proposed initial backend and fixed operation

**Initial backend proposal: nftables only.** Legacy iptables, unsupported firewall managers, unsupported nftables schema versions, missing nftables tooling, and ambiguous mixed ownership are `unknown` in the first release. Do not infer that no firewall exists because `nft` is missing or its ruleset is empty.

The only allowed inspection command is:

```text
/usr/sbin/nft --json list ruleset
```

The helper must:
- invoke the fixed absolute executable directly, without a shell;
- pass no caller-controlled arguments;
- never invoke `-f`, `flush`, `add`, `insert`, `delete`, `replace`, `reset`, or any other mutation operation;
- bound stdout, stderr, execution time and memory before parsing;
- kill and reap the child on timeout or output-limit breach;
- parse JSON locally and discard the raw ruleset after producing the minimal evidence receipt.

Proposed initial resource limits (subject to owner review): at most 1 MiB stdout, 64 KiB stderr, 5 seconds end-to-end, one active inspection per instance, and one new inspection per instance per 30 seconds. Limits must be enforced in code, not only documented.

## 5. Selecting the relevant packet path

The helper derives the endpoint from the validated local runtime configuration, not from API input:

- The configured tunnel transport is TCP for the initially supported Rust `tcp-tls` and `noise-tcp` engines.
- The configured tunnel socket port is taken from the local endpoint configuration.
- For a `serve` role, inspect the local inbound `input` hook for the configured listening address family and tunnel port.
- For a `forward` role, inspect the local outbound `output` hook for the configured remote serve endpoint and tunnel port.
- Direct and Reverse retain the connection-role meanings in ADR-0021: `serve` is the tunnel listener and `forward` is the tunnel dialer. The application ingress/target port is not the tunnel port and must not be substituted for it.
- If the config's endpoint, port, engine, address family, or applicable hook cannot be derived unambiguously, return `unknown`.
- Do not perform DNS lookups as a side effect of this read operation. If outbound policy depends on a hostname whose current addresses are not represented as a literal configured peer IP, return `unknown` for address-dependent rules.
- Wildcard listeners, source-address-dependent rules with no verified peer source address, interface-dependent rules without an authoritative interface binding, and dual-stack ambiguity are `unknown` unless the evaluator can prove the result for every applicable packet tuple.

## 6. Conservative result semantics

The evaluator must understand that nftables chains are attached to Netfilter hooks and may be ordered by priority. An `accept` verdict in one base chain ends that chain's evaluation, but packet processing can continue into later base chains; a later matching `drop` or `reject` can still discard the packet. A drop verdict is terminal.

Therefore:

- **`fail`:** only when a supported, fully evaluated applicable ruleset path proves that a matching `drop` / `reject` verdict, or a definitive drop policy, prevents the configured tunnel packet.
- **`pass`:** only when every applicable base chain and every relevant decision on the configured packet path is fully supported and evaluated, and the complete path proves no applicable drop/reject remains.
- **`unknown`:** missing or empty evidence; missing privilege; command failure; truncated/oversized output; parse/schema error; unsupported expressions; unsupported chain types/hooks; unresolved address/interface; ambiguous priority/order; unsupported sets/maps or dynamic verdicts; uninspectable jump/goto paths; multiple packet families not fully covered; or any uncertainty about which rules would match.

In particular, a matching `accept` in one chain is **not** sufficient for `pass`. No matching explicit allow rule is **not** sufficient for `fail`. The evaluator must never guess.

### Initial supported syntax proposal

Start with a deliberately narrow, versioned parser for nftables JSON:
- families `ip`, `ip6` and `inet`;
- base chains attached to the relevant `input` or `output` hook;
- TCP protocol and constant tunnel-port comparisons;
- literal IP address selectors only when the packet tuple is known;
- chain policy and direct terminal `accept`, `drop` or `reject` verdicts;
- non-verdict counters and comments only when they do not change matching or control flow.

Treat unknown expressions or statements as unsupported, not as no-ops. The initial parser must return `unknown` for jumps/gotos, maps, sets, dynamic sets, concatenations, verdict maps, rate limits, interface/conntrack/mark selectors, NAT alterations, bridge/netdev hooks, or any other construct not explicitly implemented and tested. The supported subset may expand only with fixture coverage and a security review.

## 7. Evidence contract and UI

Proposed receipt fields:
- `schema_version`;
- `instance`;
- `state` (`pass` / `fail` / `unknown`);
- `backend` (`nftables` or a bounded reason such as `unsupported`);
- `hook` (`input` / `output` / `unknown`);
- `protocol` (`tcp` / `unknown`);
- `port` (only the configured tunnel port);
- `address_family` (`ipv4` / `ipv6` / `dual` / `unknown`);
- `observed_at`, `duration_ms`, `freshness_seconds`;
- `ruleset_fingerprint` (SHA-256 of the bounded raw bytes, not the raw rules);
- `reason_code` and a short bounded explanation;
- explicit `limitations` describing the parser/backend coverage.

Do not include raw rules, unrelated ports, unrelated IP addresses, full chain contents, secrets, or unbounded command output in the API, panel, receipt or logs.

The panel must label this evidence as **local firewall inspection** and show source/time/freshness/limitations. It must not label `pass` as peer reachability, an open public port, authenticated session, integrity, or Ready. `unknown` is a normal result when the host firewall cannot be understood safely.

The `firewall` tier is independent of `listener`, `session`, `data_path`, `integrity` and global `ready`. B3 must not alter any readiness precedence defined by ADR-0024 or the readiness contract.

## 8. Security requirements

- No generic executable, subcommand, shell fragment, path, address, port, or family may be supplied by the Agent or user.
- Per-instance request/result files must be bounded, regular, no-follow, single-consumption, ownership-checked and protected against cross-instance replay.
- The helper runs with the minimum privilege demonstrated by Ubuntu 22.04 / systemd 249 tests. Any required capability must be explicitly justified; do not copy a broad capability set from another helper without evidence.
- The helper's supported code path must never issue firewall mutation requests or modify services, routes, sysctls, packages, tunnel config, certificates or node keys. **This is an intended-code-path guarantee, not an absence-of-authority guarantee:** the verified nftables listing operation requires `CAP_NET_ADMIN`, and that capability also permits firewall administration if the helper is compromised.
- Protect the helper with strict systemd sandboxing while preserving the narrowly required Netlink access; do not use `PrivateNetwork=true` if that hides the host ruleset from the helper.
- Bound concurrency, request frequency, process lifetime, stdout/stderr, JSON depth, object count and parser work.
- Do not log or export the full ruleset. Record only bounded reason codes and safe evidence metadata.
- Read-only inspection must not require the Agent to run as root, join a privileged group, or expose its API beyond loopback.
- Stale results are `unknown`; never present an old observation as current.


### Empirical privilege boundary — 2026-11-13

A focused, read-only capability matrix ran on an ephemeral GitHub-hosted **Ubuntu 22.04.5 / systemd 249.11-0ubuntu3.23** runner with nftables 1.0.2. Evidence: [workflow run 43418581447](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/43418581447), test revision `ca3f128c6576a1d5b08a5f105a148c3301fc7cd8` (harness in draft PR #196).

| Invocation | Effective capability state | Result |
|---|---|---|
| Unprivileged `nobody` | No elevated capability | Exit 1 — `Operation not permitted` |
| `nft --json list ruleset` with only `CAP_NET_ADMIN` | Effective mask `0000000000001000` | Exit 0 |
| Root with `CAP_NET_ADMIN` removed | Effective mask `000001ffffffefff` (bit 12 absent; other capabilities retained) | Exit 1 — `Operation not permitted` |
| Root baseline | Full runner capability set | Exit 0 |

The before/after ruleset SHA-256 was identical (`bd0847b270f0edf0ed0da7569b234ba4dc94e14c7b4b78254942389a0fdbf694`). The experiment invoked only `nft --json list ruleset`; it did not invoke a firewall mutation command or intentionally change rules, services, routes, sysctls or host configuration.

**Conclusion for this tested baseline:** `CAP_NET_ADMIN` is required and sufficient for this exact listing operation. Removing it makes the command fail even while other root capabilities remain. The helper therefore necessarily holds a capability that can also mutate firewall state. A fixed argv, code-level command allowlist and sandbox reduce exposure but do not, by themselves, remove that kernel authority. No seccomp/LSM policy that permits listing while rejecting mutation has been demonstrated.

**Evidence boundary:** this was an isolated hosted runner, not either two-host LAB machine. It establishes the command's privilege requirement on the stated Ubuntu 22.04/systemd 249 baseline; it does not prove that every kernel/vendor configuration is identical, nor that a particular service sandbox prevents mutation.

Before implementation, the owner must explicitly accept or reject this residual compromise risk. If accepted, the security review must assess the exact helper boundary and compensating controls. The product must not describe the helper as technically incapable of firewall mutation.

## 9. Required verification before B3 acceptance

### Unit / parser fixtures
- IPv4, IPv6 and inet input/output base chains.
- Multiple base chains in different priorities; later drop after earlier accept; terminal drop/reject; accept policy; drop policy.
- A missing hook, empty ruleset and missing `nft` binary all yield `unknown`.
- Direct exact-port allow/drop rules; other ports do not affect this instance's result.
- Unrelated rules and tables do not leak into the receipt.
- Unknown expressions, sets/maps, dynamic verdicts, jumps/gotos, interface/source/destination conditions, NAT, bridge/netdev and ambiguous priorities yield `unknown`.
- Hostname peer, wildcard listener and incomplete dual-stack evidence yield `unknown` unless the evaluator proves every applicable tuple.
- Malformed JSON, unsupported schema version, duplicate objects, truncated output, oversized output, excessive depth/object counts and invalid UTF-8 fail closed to `unknown`.
- Child missing, permission denied, non-zero exit, timeout, stdout/stderr overflow and concurrent invocation.
- Cross-instance request, wrong config path/port, symlink, non-regular file, replay, malformed request and rate-limit cases.

### Systemd / installer
- Ubuntu 22.04 / systemd 249: install, start, inspect, repeated request, timeout, restart and uninstall of the firewall helper path/service units.
- Prove no firewall mutation: snapshot the relevant kernel ruleset before and after inspection and compare fingerprints; prove the helper's command allowlist contains only the fixed JSON list operation.
- Prove no unrelated service, rule, route, sysctl, package, or file is changed.
- Verify the actual privilege/capability requirements rather than assuming root alone is sufficient.
- Run the existing installer/uninstaller tests and preserve other instances' helper registrations.

### LAB
- On dedicated test nodes with known pre-existing test rules, demonstrate a definite blocked result, a completely evaluated permitted result, and an intentionally unsupported/ambiguous Unknown result.
- Do not add, remove or reload firewall rules during the inspection run. Any fixture provisioning must happen before the acceptance run in a disposable, isolated environment and be recorded separately.
- Confirm the observation is local and does not claim external reachability or tunnel/data-path integrity.
- Direct and Reverse tunnel-role interpretation must be tested separately. The firewall feature does not expand Reverse availability or engine support.

## 10. Implementation gate

This ADR is proposed and does not authorize code, API, systemd, panel, installer, LAB, host, release or deployment changes.

Before implementation:
1. The owner accepts the backend scope, conservative `pass/fail/unknown` semantics and resource limits in this ADR.
2. A focused security review approves the fixed root helper boundary, exact command/capabilities, path-unit file handling and parser fail-closed behavior.
3. The supported nftables JSON schema and syntax subset are versioned with fixtures.
4. The implementation plan demonstrates that unsupported firewall state remains Unknown and does not block unrelated tunnel operations.
5. The owner explicitly accepts the documented residual risk that compromise of a helper holding `CAP_NET_ADMIN` can permit firewall mutation; a claim of kernel-enforced read-only Netlink access is prohibited unless separately proven.

B3 acceptance requires all unit, systemd, installer and read-only LAB evidence above. No firewall mutation is authorized as part of B3. B3 completion does not authorize B2 implementation, Phase 2, or a production release.

## References

- [nftables manual — ruleset listing and JSON output](https://netfilter.org/projects/nftables/manpage.html#RULESET): the proposed fixed `list ruleset` operation is observational; it is distinct from ruleset mutation commands.
- [nftables manual — verdict statements](https://netfilter.org/projects/nftables/manpage.html#VERDICT_STATEMENT): accept, drop, return, jump and goto change packet-evaluation control flow.
- [Netfilter nftables documentation — base-chain configuration](https://wiki.nftables.org/wiki-nftables/index.php/Configuring_chains): base chains attach to hooks and are ordered by priority; multiple chains can participate in a hook.
