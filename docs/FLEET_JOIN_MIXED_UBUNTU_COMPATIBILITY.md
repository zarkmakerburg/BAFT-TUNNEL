# Fleet Join LAB — mixed Ubuntu compatibility plan

**Status:** Owner-approved test design; no host-side operation is authorized by this document.
**LAB:** `baft-tunnel-fleet-lab-20261121` only.
**Canonical coordination/evidence:** [Issue #2](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/2).
**Related access evidence:** [read-only preflight run 44948802945](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/44948802945), `main` at `826b333747a626015843f8a9768594306d2cc2b8`.

## 1. Decision and scope

The owner approved retaining the two already-registered Fleet Join LAB VMs in their current OS states and evaluating a mixed-version pair:

- Hub (Iran): Ubuntu 24.04.4 LTS, kernel 6.8.0-147-generic, x86_64.
- Member (abroad): Ubuntu 22.04.4 LTS, kernel 5.15.0-112-generic, x86_64.

Do not provision replacement VMs or reimage either existing VM as part of this plan. Do not silently change the product's established Ubuntu 22.04 / systemd 249 baseline. This LAB experiment evaluates whether the Fleet Join control-plane workflow works across the observed mixed-OS pair; it does not, by itself, add Ubuntu 24.04 to the product's supported-platform contract.

The two VMs are registered in the dedicated Fleet Join GitHub Environment. Registration and successful SSH access do **not** prove that they are disposable, isolated from unrelated data, or safe to reset. Until a documented reset/recovery boundary is verified and separately authorized, treat them as dedicated LAB hosts with preserved state—not as disposable hosts.

## 2. Observed baseline and unknowns

| Property | Hub (Iran) | Member (abroad) | Evidence / limitation |
|---|---|---|---|
| OS | Ubuntu 24.04.4 LTS | Ubuntu 22.04.4 LTS | Read-only preflight |
| Kernel | 6.8.0-147-generic | 5.15.0-112-generic | Read-only preflight |
| Architecture | x86_64 | x86_64 | Read-only preflight |
| Init | systemd | systemd | Version not collected |
| `nft` | Present | Missing | Tool-presence check only |
| `ss`, `python3`, `tc`, `systemctl` | Present | Present | Tool-presence check only |
| TCP probes Iran → abroad | No listener on 443, 8443, 2053, 18300 | Same path result | Reachability only; no Fleet Join listener tested |
| SSH host-key pinning | Passed | Passed | Workflow used strict known_hosts; provider-console fingerprint cross-check remains a separate identity-verification item |
| Fleet CA / enrollment state | Unknown | Unknown | Not inspected |
| Existing BAFT-TUNNEL units, users, paths, firewall/routing state | Unknown | Unknown | Not inspected |
| Disposable/reset boundary | Unknown | Unknown | Not proven by SSH preflight |

No secret values, host addresses, private keys, or known_hosts contents belong in this document, repository, issue comments, or workflow artifacts.

## 3. Compatibility boundary

The first compatibility experiment is deliberately limited to **Fleet Join control-plane enrollment and status exchange over authenticated TCP/TLS**. It must not require:

- TUN device creation, interface configuration, route changes, packet forwarding, or firewall mutation;
- `CAP_NET_ADMIN` in the agent, tunnel process, installer, or helper;
- installing `nftables` merely to make the control-plane test pass;
- weakening TLS peer verification, certificate validation, authorization, or protocol-version checks.

The absence of `nft` on the Member is recorded as an environment difference, not automatically as a blocker for the control-plane-only experiment. Any feature whose explicit contract requires nftables is out of scope until separately designed and authorized. Firewall capability/status must remain Unknown unless independently measured; do not infer it from command presence.

This plan does not authorize installation, service creation, package changes, CA initialization, enrollment, tunnel startup, firewall/route/sysctl changes, reboot, cleanup, or any other remote mutation.

## 4. Staged acceptance plan

All stages below are **planned gates**, not completed tests. Before any remote stage, record its exact scope and obtain task-specific owner authorization. Re-read Issue #2, check the shared real-host concurrency group, and announce the operation before dispatch. Use only `baft-tunnel-fleet-lab-20261121`; never borrow `baft-tunnel-lab`.

### Gate 0 — identity, inventory and isolation (read-only)

1. Independently compare each pinned ED25519 host key with the provider's trusted console/API identity record. A successful SSH handshake against the configured known_hosts file is not a substitute for this independent comparison.
2. Collect exact OS release, kernel, architecture, systemd version, libc/runtime dependencies, package manager state, clock synchronization, and required executable availability.
3. Inventory BAFT-TUNNEL-owned units, processes, users/groups, directories, listeners, certificates/CA state, firewall/routing state, and any pre-existing LAB artifacts. Read only; do not stop or clean anything.
4. Establish the VM ownership and reset boundary: snapshot/rebuild mechanism, recovery access, data-retention implications, and explicit list of state that must be preserved. Do not call either host disposable until this is evidenced.

**Exit:** host identity independently verified; inventory captured without secrets; no unknown ownership collision; recovery/reset procedure documented. Any failure leaves later gates blocked.

### Gate 1 — artifact/runtime compatibility (offline or hosted first)

1. Build and verify the same immutable, signed candidate artifact for both hosts; record source SHA, artifact SHA-256, signature identity, target triple, and dynamic-library requirements.
2. Check that the candidate's runtime dependencies are compatible with both observed userspaces. Do not infer compatibility from `x86_64` alone.
3. Validate configuration and protocol schema compatibility without connecting to the hosts.
4. Confirm the planned control-plane path does not request `CAP_NET_ADMIN`, create TUN devices, or mutate firewall/routes/sysctl.

**Exit:** one exact artifact is verifiably usable on both OS versions, or the incompatibility is documented and the experiment stops. No host installation is implied.

### Gate 2 — control-plane protocol and security (tests before remote execution)

1. Test authenticated TCP/TLS enrollment, peer identity validation, authorization, replay rejection, idempotency, and bounded request handling.
2. Test heartbeat/status exchange, reconnect/backoff, duplicate enrollment, revoked/unknown identity, and unsupported protocol-version behavior.
3. Confirm no silent downgrade and no trust bypass for the older or newer peer.
4. Verify all privileged operations are absent from this control-plane path; no component receives `CAP_NET_ADMIN`.

**Exit:** all local/integration security tests pass; unsupported or invalid peers fail closed; no privileged network configuration is exercised.

### Gate 3 — authorized two-host mixed-version control-plane LAB

Only after Gates 0–2 are accepted and a separate exact-scope owner authorization is recorded:

1. Install/run only the explicitly reviewed Fleet Join control-plane components, with recorded pre-state and a bounded rollback plan.
2. Exercise Hub 24.04 ↔ Member 22.04 enrollment and status exchange using the same artifact SHA.
3. Repeat with roles reversed only if the protocol design permits symmetric roles; record the direction rather than assuming symmetry.
4. Capture exact binary SHA, protocol versions, identities (redacted), timestamps, latency, reconnect/backoff behavior, result codes, and post-state inventory.
5. Verify no TUN, route, firewall, sysctl, or `CAP_NET_ADMIN` effects. Preserve logs and state on any ambiguous failure; do not perform broad cleanup.

**Exit:** both directions required by the defined role model pass; negative identity/version cases fail closed; exact artifact and protocol evidence are attached to Issue #2; post-state diff shows only explicitly authorized resources.

### Gate 4 — separate data-plane / platform support decision

A successful control-plane experiment does not prove tunnel data-plane compatibility, firewall behavior, packet forwarding, or general Ubuntu 24.04 support. Those require a separate design and acceptance plan that still respects the owner's prohibition on `CAP_NET_ADMIN`. Do not extend this experiment into TUN/route/firewall setup.

## 5. Pass/fail semantics

- **PASS:** the named gate's exact criteria pass on the recorded artifact SHA and host pair.
- **FAIL:** a required assertion fails; retain evidence and stop dependent gates.
- **UNKNOWN / NOT VERIFIED:** missing inventory, unverified provider identity, incomplete cleanup/recovery boundary, ambiguous state, or missing evidence. Unknown is never promoted to Pass.
- A successful preflight is only access/reachability evidence. It is not a Fleet Join test, a disposability claim, or platform-support approval.

## 6. Explicit non-goals and prohibitions

- No replacement VM provisioning or reimage under this plan.
- No use of the existing Pairing LAB hosts or `baft-tunnel-lab` environment.
- No CAP_NET_ADMIN, TUN, route, packet-forwarding, firewall, nftables installation, or sysctl mutation.
- No production hosts, release, merge, deployment, or product support-matrix change.
- No cleanup of stale Pairing LAB state.
- No host-side action is authorized merely because this plan was owner-approved.

## 7. Current blockers

1. Provider-console identity cross-check for both SSH host keys: **UNKNOWN**.
2. Exact systemd versions and fuller read-only inventory: **NOT COLLECTED**.
3. Disposable/reset boundary and data-preservation proof: **UNKNOWN**.
4. Candidate artifact/runtime compatibility on both userspaces: **NOT TESTED**.
5. Fleet Join protocol/security integration tests: **NOT ACCEPTED**.
6. Any remote mutation or two-host enrollment: **NOT AUTHORIZED by this document**.

Reconcile each item with evidence on BAFT-TUNNEL Issue #2. Keep this plan as design and acceptance criteria, not as a claim that the gates have passed.
