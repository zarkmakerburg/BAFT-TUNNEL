# ADR-0029: Guided setup for the BAFT engine (ADR-0019 phase F, roadmap B4)

**Status:** Accepted 2026-11-14 (owner full-autonomy delegation). Implementation in this PR; acceptance needs the real two-host LAB run through the Wizard path (`phase1-trusted-b4-baft-lab.yml`). Accepted on REAL two-host LAB 2026-11-14 on merged main `86ba404` ([LAB run 43644908151](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/43644908151)).
**Scope:** `setup plan/init/join` for engine `baft`, the PKI identity profile, the engine store install path, the Wizard engine choice.
**Builds on:** ADR-0019 (BAFT engine host), ADR-0020 (managed runtime host), ADR-0021 (Direct/Reverse), ADR-0026 (panel pairing via the root setup helper), ADR-0027 (offline tcp-tls enrollment).

## Context

BAFT is shown in the Wizard as Preview, Direct only (PR #133) but setup refused to write its configuration. The runtime adapter, the hash-checked engine store and the store launcher already passed real-host LAB gates (P0-3). Missing were: a configuration writer, a way to get BAFT-compatible certificates through the panel, and a supported way to put the signed engine into the store.

## Decisions

1. **Same pairing model as tcp-tls.** baft nodes use the offline CA, the panel CSR/upload flow (ADR-0027), the bundle exchange and checksum comparison. The bundle `public` value is the node certificate's SHA-256; the serving node's bundle carries its server name.
2. **Identity profile fixed at CSR time.** BAFT hard-requires exactly one URI SAN `urn:baft:node:<id>` (design R15). The root PKI helper takes `identity_profile` (`baft-tunnel` default, `baft`) only on `create_csr`, records it in the root-owned `node.csr.meta`, and validates the uploaded leaf against that stored profile. A later request naming another profile is refused. The agent derives the profile from the Wizard engine (pre-Init) or from the existing setup state or configuration, and refuses `noise-tcp`. `baft-pki.sh` gains `issue-baft-node` and `verify-baft-node`; `revoke-node` accepts both profiles. `setup init` for baft refuses a certificate without exactly that URI SAN.
3. **Engine pin from the binary's own manifest.** `release/engine-manifest.json` is compiled into `baft-tunnel` (`baft-tunnel engine manifest` prints it). Guided setup writes `"baft": {"sha256": <manifest pin>, "store": "/usr/local/lib/baft-tunnel-engines"}`. The launcher already refuses a store whose `current` release differs from the pin, so a mismatched or tampered store fails closed at start.
4. **Root-owned store outside the package directory.** `install.sh --binary B --baft-engine ENGINE` verifies the engine against B's built-in manifest and installs it into `/usr/local/lib/baft-tunnel-engines` (root:root, world-readable, not writable by the service or agent user). It is outside `/usr/local/lib/baft-tunnel`, which every `--binary` install replaces. A refused engine leaves no `current` and the install rolls back. `uninstall.sh` removes the store.
5. **Pairing binds the exact engine artifact.** BAFT pairing uses bundle version 2 and persists `engine_sha256` in local pairing state and the public bundle. Join refuses a missing or different peer digest, a changed local build pin, or a version-1 bundle; tcp-tls and Noise retain byte-compatible version-1 bundles. The certificate fingerprint remains separate from the engine digest and is used only as the dialer's listener pin.
5. **Pins.** The baft listener admits peers by CA, identity allowlist and revocation and takes no pin (the runtime rejects one). The dialer pins the listener certificate from the serving bundle (one pin; the engine has no rollover list). Cert rollover for baft therefore means a new pairing or a manual pin update, as stated in R12.
6. **Preflight gate.** For baft the agent adds `engine_store`: blocked unless the pinned release is the verified `current` in the store. Init stays off until it passes.
7. **Still Preview, Direct only.** Reverse and protocol v2 stay refused for baft in setup and in the runtime. Traffic counters and end-to-end status stay Unknown for this engine (no fake data).

## Consequences

- An operator selects baft in the Wizard, enrolls a baft-profile certificate, and pairs with no manual JSON editing.
- The signed engine must be installed with `--baft-engine` (one extra flag); the panel says so when the gate blocks.
- Security posture of the BAFT path is unchanged from ADR-0019 (authorization enforced inside the Go engine; R11 dialer revocation limit remains and is mitigated by the listener pin).

## Evidence required to call B4 done

Real two-host LAB through the agent API only (no root `setup` commands by the operator): clean install with `--baft-engine` (signed asset, cosign and SHA256SUMS verified, equal to the candidate pin), engine-store preflight pass, baft-profile CSR create/recover, offline CA signs with `issue-baft-node`, upload, Init, bundle export/inspect/wrong-checksum refusal, Join, Start, 64 KiB and 2 MiB byte-exact through the store engine running as `baft-tunnel`, Stop through the agent closes the engine and ports, uninstall leaves no store, users or units.
