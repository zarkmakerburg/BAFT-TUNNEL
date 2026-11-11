# BAFT-TUNNEL Architecture

**Verified implementation snapshot:** 11 November 2026, `main` `1219f547`. This document describes the implemented local product, not the aspirational multi-node architecture. See [PRODUCT_ROADMAP.md](PRODUCT_ROADMAP.md) and the ADRs for gated future work.

## Product boundary

BAFT-TUNNEL is an owner-controlled Linux server-to-server tunnel product. The GitHub repository is publicly viewable, but no licence is granted. The core is Rust. The product is not a customer, subscription, reseller, or billing panel. Third-party integrations may use a future stable API, but a central control plane/Fleet is not currently implemented.

The BAFT main engine is an optional, separately pinned Go process under ADR-0019. It is not Rust code copied into this repository and is not the default engine.

## Implemented components

- **Tunnel runtime:** Rust CLI and runtime; Tokio async execution; rustls TLS 1.3 mutual authentication for `tcp-tls`; Yamux multiplexing; bounded framing, sessions, streams, queues and deadlines.
- **Engines:** `tcp-tls` is the supported pre-release engine. Direct has two-host evidence; Reverse is a real outbound-only path but remains Preview. `noise-tcp` is Experimental Direct only. The BAFT engine is a separate pinned process and Preview Direct only; guided setup cannot yet configure it.
- **Local Agent:** a per-host loopback HTTP API, installed as a dedicated systemd service. It exposes local status, diagnostics, desired-state validation, lifecycle operations, audit, traffic and PKI pairing operations. It is not a central controller and does not manage a remote fleet.
- **Panel:** dependency-free static HTML/CSS/JavaScript served by the local Agent; Persian RTL and English LTR. There is no React/Vite build chain.
- **CLI and setup:** install, setup plan/init/join, config validation, Apply lifecycle, backup/restore, update/rollback, support bundle and PKI helpers.
- **Persistence:** local files and JSON state; no central database or shared cloud state.

## Connection topology

- **Direct:** the forward node dials the peer's reachable listener and exposes a local ingress.
- **Reverse (`tcp-tls` Preview):** the service-side node initiates the outbound carrier; the public-side node owns the user ingress and forwards logical streams back through the established authenticated connection. It is not merely a label in the hello frame.

Topology selection does not change cryptographic policy. `noise-tcp` Reverse is not accepted and must remain unavailable. Relay and automatic path selection are future work, not implemented capabilities.

## Trust and data plane

For `tcp-tls`, both peers use TLS 1.3 mutual authentication. Peer validation binds the certificate chain, issuing-CA CRL, configured SHA-256 peer pin(s), and the expected URI SAN node identity. CRL status fails closed when missing, malformed, expired, or unverifiable. During a controlled CA rollover, the trust bundle may contain multiple CA certificates with one CRL per CA, and the peer may accept up to two distinct pins. The offline CA private key remains outside the nodes; CRL push is an operator-initiated SSH workflow.

After TLS authentication, a bounded versioned stream-open authorization frame binds the tunnel ID, direction and allowed capability before the fixed local destination is connected. A remote peer cannot choose an arbitrary destination or cause DNS resolution. TLS 1.3 early data is disabled. No plaintext or legacy-TLS fallback exists.

`noise-tcp` uses a separate Noise IK engine and pinned static peer keys; it is Experimental and does not inherit the `tcp-tls` acceptance claims. The BAFT engine is supervised as a separate process and launched only from its hash-verified Engine Store.

## Agent authorization and receipts

The Agent binds to loopback and authenticates local API requests with a token. Privileged host mutations are delegated to fixed root-owned helpers with constrained inputs. Apply operations validate the desired action, use idempotency and rollback where implemented, and record secret-free receipts/audit events. The Agent's local status is not a central desired-state controller. Missing or stale evidence is `unknown`; an observed session or heartbeat alone never means `ready`. Apply receipts keep `ready` Unknown by design.

## Host and key custody baseline

The supported baseline is Ubuntu 22.04 LTS with systemd 249. Node private keys are provisioned with root-only source permissions and delivered to the tunnel service using `LoadCredential=`. This protects access through service-scoped delivery but is not encryption at rest. `LoadCredentialEncrypted=` is optional on systemd 250+ and is not required or available on the baseline LAB hosts. No TPM-backed signing claim is made.

The installed Agent and tunnel runtime run as separate unprivileged service identities. The panel/API is local to each host. The current product does not expose an online CA signer, online enrollment endpoint, central inventory, remote Jobs, Fleet or multi-tenant control plane.

## Operations and evidence

The installer, service lifecycle, PKI helpers, CRL update/rollback, backup/restore and guarded update paths have unit, CI and selected two-host LAB evidence. Evidence is always tied to its exact SHA and run. A hosted CI test, local test, simulation and real two-host LAB are different evidence classes. The current signed RC.7 U7 integrated acceptance is tracked separately; its outcome must not be assumed before the run completes.

No firewall mutation is performed by the product. Unknown firewall state remains Unknown. Active forwarded TCP streams are closed on carrier loss or service shutdown; they are not transparently migrated or resumed.

## Deferred work and phase boundary

Central enrollment/Fleet, signed remote desired state, remote Jobs, multiple tunnels per host, additional transports (QUIC/UDP, WebSocket, Relay), automatic failover, and a full pre-stable independent security review are deferred. The owner has paused the independent review; do not initiate it without renewed authorization. Do not describe these planned capabilities as implemented. No stable release or production deployment is authorized by this architecture document.
