# BAFT Lessons and Engineering Guardrails

**Status:** Mandatory engineering policy for BAFT-TUNNEL.
**Scope:** This is an independent project. It adopts lessons and acceptance criteria only; it does not import BAFT or C-Tunnel source code, history, credentials, keys, or implementation assumptions.
**Applies to:** Product code, control plane, agent, installer, panel, LAB, CI, release, and operations.

This document converts prior project lessons into reviewable requirements. A feature is not complete because code exists or a command exits successfully: its observable effects, failure behavior, recovery, and cleanup must be evidenced.

## 1. Non-negotiable product principles

1. **Truthful state:** Never report a tunnel as connected, healthy, applied, secure, or ready unless the corresponding condition was actually measured. Distinguish host reachability, TCP listener reachability, authenticated handshake, authorized session, and bidirectional data-plane integrity.
2. **No placeholder success:** A stub, mock, simulated result, static UI state, or unconnected prototype must never be presented as a working production capability.
3. **Safe desired-state changes:** Configuration changes must be validated and previewed before mutation. Apply must be bounded, idempotent where possible, produce a receipt, and have a tested rollback path for every owned resource it changes.
4. **Failure is a first-class path:** Test timeout, partial failure, peer restart, network loss, invalid credentials, expired/revoked trust, resource exhaustion, and cleanup failure—not only the happy path.
5. **Simple operator experience:** Keep setup short, defaults safe, terminology understandable, and advanced controls out of the normal path. Detect common faults and explain the next corrective action without hiding technical evidence.
6. **Least privilege and ownership:** Every process, file, port, firewall rule, route, and service must have explicit ownership and scope. Never alter unrelated host state.
7. **Independent evidence:** LAB simulation, mocks, and real-host tests are different evidence classes. Label them accurately; never substitute one for another.
8. **No unnecessary complexity:** Prefer the smallest architecture that satisfies security, reliability, and user requirements. Do not add protocols or configuration switches without a demonstrated use case and acceptance plan.

## 2. Connectivity and status contract

The UI, CLI, API, logs, and support bundle must preserve these separate states:

- **Host reachable:** The remote host responds at the network layer.
- **Listener reachable:** The expected TCP/UDP port accepts a connection.
- **Transport authenticated:** The cryptographic handshake and peer identity validation succeeded.
- **Peer authorized:** The peer is permitted for this tunnel, direction, and capability.
- **Data plane verified:** A bidirectional payload probe passed integrity checks.
- **Operational/healthy:** Required probes and configured service checks are passing now.

A lower-level success must never imply a higher-level success. Status must include observation time and a useful failure reason. Stale state must be visibly marked stale rather than shown as live.

## 3. Configuration, Apply, rollback, and recovery

Before changing host state:

- Parse and validate the complete configuration, including cross-field constraints.
- Produce a redacted plan that lists intended resources and changes.
- Require explicit confirmation for destructive or externally visible host mutations.
- Record ownership and the prior state of every managed resource.
- Apply atomically where practical; otherwise use ordered steps with compensating rollback.
- Never overwrite unrelated files, firewall rules, routes, or service definitions.
- On partial failure, stop dependent steps, restore owned prior state, and report both the original failure and rollback outcome.
- Make retry behavior safe; detect already-applied state and avoid duplicate resources.
- Keep configuration, private keys, state, and logs separate. Redact secrets from output and support bundles.

Acceptance evidence must include successful apply, repeated apply, invalid config with zero mutation, mid-apply failure, rollback success, rollback failure reporting, restart/reboot behavior where relevant, and safe uninstall/upgrade.

## 4. Security and protocol boundaries

- No plaintext fallback, legacy TLS fallback, or silent downgrade.
- Authentication, identity binding, authorization, tunnel direction, and stream capability are separate checks.
- Reject malformed, oversized, duplicate, ambiguous, expired, revoked, or untrusted security inputs by default.
- Bound every externally influenced allocation, queue, session, stream, handshake, retry, and operation duration.
- Define replay behavior for control operations and data framing; do not assume TLS alone makes application operations replay-safe.
- Protect node private keys with OS-appropriate custody. File mode 0600 is a minimum safeguard, not a production key-management solution.
- Certificate/CRL publication, refresh, rotation, overlapping trust, and recovery must have a defined operational workflow.
- Do not expose arbitrary peer-selected destinations, DNS pivots, or unintended network reachability.
- Threat-model every new control-plane or host-mutating capability and test both authorization and denial paths.

## 5. Installation and operator UX

The standard path must be beginner-friendly:

- One clearly documented install path with preflight checks and actionable errors.
- Safe defaults; no automatic firewall opening, service activation, or persistent host mutation without an explicit documented decision.
- Installation and upgrade must be repeatable and preserve user configuration and keys.
- Rollback and uninstall must clearly state what is removed and what is retained.
- Service runs under a dedicated least-privilege identity with documented file ownership and permissions.
- The normal panel/wizard path asks only for information genuinely required; advanced options remain optional.
- Never display a green/connected state based only on saved configuration or a successful API response.

Actual Linux/systemd installation must be tested on a disposable Linux host before claiming installation readiness. Staged tests and CI are useful but are not equivalent to a real host lifecycle test.

## 6. LAB and test evidence policy

Every evidence item must state: code SHA, environment, test class (unit/mock/simulation/real-host), exact topology, operation performed, measured result, cleanup result, and what it does not prove.

Required test layers, selected proportionately to risk:

- Unit and parser boundary tests.
- Integration tests for both Direct and Reverse orientations.
- Negative security tests (identity, authorization, revocation, expiry, malformed input, downgrade).
- Resource-limit and concurrency tests.
- Recovery tests (peer restart, link loss, reconnect, interrupted apply).
- Installer/upgrade/rollback/uninstall lifecycle tests.
- Isolated network impairment tests for latency/loss and cleanup.
- Real-host E2E with payload integrity in both directions before claiming real connectivity.
- Longer soak and controlled impairment before production readiness.

Mocks prove orchestration behavior only. Container/netem proves the isolated simulation only. A host preflight proves only the probes it actually ran. Report each limitation.

## 7. CI and pull-request enforcement

A change is not accepted until the relevant automated gates pass on the exact proposed head SHA:

- Formatting, lint, unit/integration tests, and release build.
- Shell syntax and installer lifecycle tests for installer changes.
- Immutable third-party action pinning and workflow permission minimization.
- Security-negative tests for protocol, identity, PKI, and authorization changes.
- Secret scanning and dependency advisory/license review.
- No unverified claims in README, CLI, API, UI, or release notes.
- PR description lists implemented behavior, evidence links, limitations, remaining risks, and explicit NOT RUN items.

Where a requirement cannot yet be automated, it must be a named manual acceptance gate with an owner and evidence artifact. A green CI badge alone is not a production-readiness decision.

## 8. Release gates

Do not mark a phase production-ready or publish a production release while any mandatory security, lifecycle, or real-world acceptance gate remains open. Require:

1. Exact release candidate SHA and reproducible build evidence.
2. All required CI checks green on that exact SHA.
3. Real-host tests for the claimed supported topologies.
4. Key custody, enrollment, revocation, rotation, and recovery workflows accepted.
5. Installer, upgrade, rollback, and uninstall tested on supported Linux targets.
6. Threat model and operational documentation updated.
7. Independent security review of protocol and control-plane boundaries.
8. Known limitations and unsupported scenarios disclosed.

## 9. Current Phase 1 known gaps (do not treat as completed)

At the time this policy was added, Phase 1 still requires work on online enrollment/control-plane authorization, encrypted node-key custody or OS keystore integration, automated CRL lifecycle and safe trust rollover, broader replay/framing/resource/concurrency testing, dependency and license audit, protocol fuzzing, independent security review, and longer real-host soak/impairment testing. The installer has staged lifecycle coverage, but actual Linux/systemd host lifecycle evidence is still required.

## 10. Definition of done

A capability is done only when all of the following are true:

- The implementation is connected to the real runtime path.
- Its success and failure states are observable and truthful.
- Security and authorization boundaries are enforced server-side.
- Side effects are scoped, owned, reversible where applicable, and tested.
- Relevant tests pass on the exact commit.
- LAB evidence accurately identifies simulation versus real-host execution.
- User-facing setup and recovery instructions are understandable.
- Documentation states limitations and does not overclaim.

If any item is missing, mark the capability **in progress**, **experimental**, or **NOT RUN**—never complete.
