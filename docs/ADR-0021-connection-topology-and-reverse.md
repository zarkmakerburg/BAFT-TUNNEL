# ADR-0021: Connection topology contract; Reverse is not operational yet

Status: accepted (2026-11-08). Decision source: owner architecture correction relayed in HQ Issue #2 follow-up.

## Context

The panel and ADR-0011 describe two directions, Direct and Reverse. ADR-0011 defines them as a label both peers must agree on. The owner asked whether Reverse changes the real connection behaviour. This ADR records what the code does (traced from the socket calls, not from names or comments) and what the project may therefore claim.

## Verified behaviour (code trace, `src/tunnel.rs`, `src/handshake.rs`, `src/setup.rs`)

| Question | `serve` node | `forward` node |
|---|---|---|
| Opens the TCP socket to the peer | never | yes (`connect_peer`, retried with backoff 1 s to 60 s) |
| Accepts the TCP connection | yes (`TcpListener::bind` on the tunnel port, then the mutual TLS or Noise handshake and hello) | never |
| Where application traffic enters | not here | the local loopback listener (`TcpListener::bind` on `listen`), one yamux stream per connection |
| Where traffic leaves | the fixed `target`, dialed per stream | not here |
| Return traffic | the same yamux stream on the same connection | |
| Must be publicly reachable | yes (tunnel port) | no (outbound only) |
| On connection drop | keeps listening; waits for the next session | reconnects with backoff, CRL/PKI files reloaded |

`ConnectionDirection` is only carried in the hello and compared for equality on both sides (`handshake.rs`) and bound into the Noise prologue. It does not change who dials, where traffic enters or where it leaves. Direct and Reverse therefore behave identically; the earlier "reverse" LAB runs prove that the label is accepted and data flows exactly as in Direct. They are not evidence of a reverse data path.

## Decision

1. Reverse is reported as **unavailable** for every engine. `GET /v1/engines` lists only `direct` in `directions`, with `reverse: {state: unavailable}`; the wizard disables the Reverse choice with an explanation; preflight returns `direction_support: blocked` for a Reverse plan. The configuration label stays accepted by the CLI and runtime for compatibility with existing configurations.
2. The topology contract below is the shared vocabulary. Capability discovery, preflight and preview use it; `GET /v1/engines` exposes the verified part as `direction_model`.
3. A real Reverse is future work with its own acceptance gate (below). No capability is advertised before it passes on two real hosts, per engine (`tcp-tls` first; `noise-tcp` judged separately).

## Topology contract (version 1)

Separate network direction from application data direction:

- `connection_initiator`: which node opens the TCP socket.
- `connection_listener`: which node accepts it.
- `traffic_ingress`: where application connections enter the tunnel.
- `traffic_egress` and `service_target`: where they leave and which address is dialed.
- `relay_node`: a node that only forwards (none today).
- `tunnel_direction`: the agreed authorization label (`direct` or `reverse`); not evidence of any of the above.
- `peer_identity`, `transport_capabilities`: from the engine contract.

Implemented today (Direct): initiator = forward, listener = serve, ingress = forward loopback listener, egress and target = serve side, no relay, public reachability needed on serve.

## What a real Reverse needs

The node that holds the service (private, possibly behind NAT) must be the connection initiator, and the reachable node must receive user connections and send them back through that connection. In the current runtime the stream opener is the dialer and the stream acceptor is the listener, so this needs: a mode in which the dialing node accepts streams and dials the target, and the listening node opens streams from a local listener; the hello and authorization must bind the new roles; target exposure stays a single configured address; mutual authentication, CRL and pinning stay enforced; reconnect restores the forwarding path.

Acceptance gate before any Reverse claim: two real hosts; the private side initiates outbound only (no inbound path to it); application data and return traffic integrity (64 KiB and 2 MiB); reconnect after interruption; wrong identity, wrong pin and revoked certificate refused; occupied port; cleanup leaves no process or rule. Mock, loopback and simulation results do not count.

## Consequences

- No capability is shown that the transport does not have. The wizard offers Direct only until the gate passes.
- Existing configurations that carry `direction: reverse` keep loading; preflight blocks planning a new one from the panel.
- This ADR does not change the wire protocol. A real Reverse mode is a protocol-affecting change and needs its own ADR amendment and review before implementation.
