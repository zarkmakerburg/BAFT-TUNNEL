# Reverse Architecture Comparison (research, docs only)

- Status: research report v1.0, 2026-11-08. Evidence labels as in `ENGINE_ECOSYSTEM_COMPARISON.md` section 0 (V/I/X/D/N). Nothing was run.
- Purpose: guide the ADR-0021 implementation plan. This document does **not** change ADR-0021: Reverse stays reported unavailable until the two-host LAB gate passes.
- Pinned sources: rathole `a292f7e` (v0.5.0), frp `d42812a` (v0.72.0 line), chisel `5ee6c74` (v1.13.0), BackPack `9d58f84` (VERSION v1.9.0).

## 1. Vocabulary

Terms follow the ADR-0021 topology contract: `connection_initiator`, `connection_listener`, `traffic_ingress`, `traffic_egress`/`service_target`. In every project below the **service node** (private side, holds the real service) is the initiator, and the **public node** (reachable side) is the listener and owns ingress. The projects differ in how the data path is built on top of that one outbound connection.

## 2. Trace per project

### 2.1 rathole (Rust) - control channel plus one TCP per user flow

| Step | What happens | Source |
|---|---|---|
| Initiator / listener | Client (service node) dials; server (public node) listens on `bind_addr`. | `server.rs:135-222` |
| Control connection | One per **service**. Client sends `ControlChannelHello(ver, sha256(service_name))`; server answers with a random 32-byte nonce hello. | `protocol.rs`, `server.rs:276-300` |
| Peer authentication | Client sends `Auth(sha256(token ‖ nonce))`; server recomputes and compares with `!=` (not constant-time; the digest is bound to a fresh nonce so practical timing risk is low). Failure returns `Ack::AuthFailed`; unknown service returns `Ack::ServiceNotExist`. Token is per service. | `server.rs:312-335` |
| Heartbeat | Server writes `HeartBeat` every `heartbeat_interval`; client drops the channel after `heartbeat_timeout`. | `server.rs:508-535`, `client.rs:483` |
| Service mapping | Server config `services.<name>.bind_addr` (public listener) + token; client config `services.<name>.local_addr` (egress). Mapping is by `sha256(name)`. | `config.rs`, `server.rs:109` |
| Data stream creation | When a visitor connects to a service `bind_addr`, server sends `CreateDataChannel` on the control channel. Client opens a **new TCP connection** to `remote_addr`, sends `DataChannelHello(ver, session_key)`, server hands it to the waiting visitor, writes `StartForwardTcp`, then `copy_bidirectional`. | `server.rs:626-655`, `client.rs:177-235` |
| Multiplexing | **None.** One TCP connection per user flow. UDP: dedicated `run_udp_connection_pool` with a length-prefixed `UdpHeader{from,len}` on one data channel. | `protocol.rs`, `server.rs:660` |
| Return traffic | Same data TCP connection, bidirectional copy. | `server.rs:645` |
| Reconnect | Client control channel retried with `run_control_chan_backoff` (x3 multiplier, capped by `retry_interval`, no max elapsed); data-channel connect retried every 100 ms for up to 10 s; server drops a previous control channel for the same service when a new authenticated one arrives (stale-handle fix). Config hot-reload adds/removes services. | `constants.rs`, `server.rs:341-350`, `client.rs:499-560` |
| Weakness | The data channel hello carries `session_key` = `sha256(token ‖ nonce)`, which is the lookup key for the control channel (`get2(&nonce)`). Over the plain `tcp` transport it is a bearer credential visible on the wire; with `noise` or `tls` transports it is protected. | `client.rs:203-208`, `server.rs:361-380` |
| Noise | `Noise_NK_25519_ChaChaPoly_BLAKE2s` default: client pins the server public key, server does not authenticate the client by key (token does). | `config.rs:126-136` |
| Evidence | V: `tests/integration_test.rs` runs tcp, tls, noise, websocket transports for TCP and UDP. |  |

### 2.2 frp (Go) - session identity plus work-connection pool, optional yamux

| Step | What happens | Source |
|---|---|---|
| Initiator / listener | `frpc` (service node) dials `frps`. Protocols: tcp (default), kcp, quic, websocket, wss. | `pkg/config/v1/client.go:105-151` |
| Control connection | One per client session. `Login` message carries `RunID`, `PoolCount`, `PrivilegeKey`, `Timestamp`, client spec. Server `ControlManager` keys controls by `RunID`. | `pkg/msg/msg.go:76`, `server/control.go:65-170` |
| Peer authentication | Token mode: `PrivilegeKey = md5(token ‖ unix_timestamp)`, verified with `ConstantTimeEqString`; OIDC alternative. I did not find a timestamp-skew check in server code in this pass, so replay protection rests on TLS (default on for `frpc`). MD5 is weak by current standards. Optional mutual TLS via `trustedCaFile` on the server TLS config. | `pkg/auth/token.go`, `pkg/util/util/util.go:50`, `pkg/config/v1/common.go:100` |
| Re-login | A second `Login` with the same `RunID` replaces the old control (`cm.ctlsByRunID[...]`, old one closed). | `server/control.go:110-165` |
| Service mapping | Client sends `NewProxy` per proxy (type tcp/udp/http/https/stcp/xtcp/sudp, remote port or domain, local ip:port). Server `ports` and `registry` allocate and check `allowPorts`. | `pkg/msg/msg.go:102`, `server/ports`, `server/registry` |
| Data stream creation | Server sends `ReqWorkConn` on control; client opens a new connection (or yamux stream if `tcpMux`) and sends `NewWorkConn`; server answers `StartWorkConn` with the proxy name and visitor address; pool pre-warms `PoolCount` (capped by `maxPoolCount`, default 5). | `server/control.go:441-470`, `client/control.go` |
| Multiplexing | `tcpMux` default **true** (`fatedier/yamux`) for the tcp protocol; KCP/QUIC bring their own. Heartbeat is unnecessary under mux and relies on mux keepalive (`TCPMuxKeepaliveInterval`). | `pkg/config/v1/server.go:160-190` |
| Return traffic | Same work connection / stream. | |
| Reconnect | `loopLoginUntilSuccess` with fast-backoff 1 s doubling to `maxInterval`; rate-limited re-dial if the control exits immediately; proxies are re-registered by the new control. | `client/service.go:310-430` |
| Evidence | V: `server/*_test.go`, `client/service_*_test.go`, `test/e2e`. |  |

### 2.3 chisel (Go) - SSH over WebSocket, channels as streams

| Step | What happens | Source |
|---|---|---|
| Initiator / listener | Client dials the server over HTTP and upgrades to WebSocket with subprotocol `chisel-v3`; `ssh.NewClientConn` runs over it. | `client/client_connect.go:91-110`, `share/version.go:12` |
| Peer authentication | Server host key is an ECDSA key; the client pins its SHA-256 fingerprint (`--fingerprint`, strongly recommended; mismatch closes). The server authenticates the client with user:password from an authfile; each user has regex permissions on remotes, including reverse ones (`^R:127\.0\.0\.1:1080$`). | `client/client.go:34,217`, `server/server_handler.go:63`, `share/settings/user.go` |
| Control | The client sends an SSH request `config` listing remotes; server validates (`r.Reverse && !s.config.Reverse` rejects when `--reverse` is off; non-listenable remotes rejected). | `server/server_handler.go:105-150` |
| Service mapping | Remote syntax `R:<server-bind>:<port>:<local-host>:<port>/<proto>`, plus `socks`, `udpsocks`. | `share/settings/remote.go` |
| Data stream creation | Server calls `tunnel.BindRemotes` (listens on the reverse port); for each visitor it opens an SSH channel to the client, which dials the local target. | `server/server_handler.go:185`, `share/tunnel/tunnel.go:153` |
| Multiplexing | All channels share the one SSH connection. | |
| Return traffic | Same SSH channel. UDP over a dedicated channel type (`tunnel_out_ssh_udp.go`). | |
| Liveness | Application `ping` SSH request every keepalive interval (default 25 s) with a timeout; a bug where the ping blocked for 15+ minutes on dead paths is documented as fixed in `TASKS.md` item 1, which is the most reported upstream issue. | `share/tunnel/tunnel.go:191-222` |
| Reconnect | `jpillora/backoff` from `--min-retry-interval` (default 1 s); tests in `client_retry_test.go`. | `client/client.go:108` |
| Evidence | V: `client_*_test.go`, `server_*_test.go`, `test/e2e`. |  |

### 2.4 BackPack (Go) - signal bytes on a control channel plus a pre-opened pool

| Step | What happens | Source |
|---|---|---|
| Initiator / listener | "Iran" server listens and exposes forwarded ports; "kharej" client dials out (`CONTEXT.md`). | `docs/architecture.md` |
| Control channel | One long-lived connection carrying single signal bytes (`SG_HB`, `SG_Chan`, `SG_Ping`, `SG_Closed`, `SG_RTT`, `SG_ChanV2`, `SG_Pool`). WebSocket carries the same as binary messages (`internal/controlwire`). | `internal/utils/signals.go` |
| Peer authentication | Shared token. Server accepts a connection only after it proves the token; for `stealth` the Noise NNpsk0 handshake is keyed on the token. `SG_ChanV2` handshake returns a fresh nonce that authorises later pool connections (legacy path falls back to source-address matching). Compare is `subtle.ConstantTimeCompare`. No per-peer identity. | `internal/server/transport/announce.go:102-120`, `tcp.go:195-270` |
| Data stream creation | Client **pre-opens** a pool of tunnel connections sized by `poolMaintainer` (`poolmaintain.go`); server pairs each queued user connection with a pool connection (`pairing.go`, `pairloop.go`); `SG_Chan` asks the client for one more. | `docs/architecture.md` |
| Multiplexing | Optional: `tcpmux`, `wsmux`, `wssmux` use smux (v1/v2 negotiated in the ack). Non-mux transports use one connection per flow like rathole. | `internal/client/transport/tcpmux.go` |
| Lifecycle | A **generation** (listener, forwarded ports, pairing workers on one context) outlives its client; a re-dialing client is seated in place and no port is re-bound; only a listener failure ends a generation. | ADR 0001, 0005 in `docs/adr` |
| Reconnect | Client backoff (`backoff.go`), listener bind-failure back-off (`bindfail.go`) so an occupied port does not hot-loop the unit; transport fallback chain (`fallback_transports`). | `internal/server/transport/bindfail.go`, `config/fallback.go` |
| Evidence | V: 481 test files incl. `lifecycle_test.go`, `restart_test.go`, `handshake_test.go`, `refusal_test.go`. |  |

## 3. Comparison

| Aspect | rathole | frp | chisel | BackPack | BAFT-TUNNEL (PR #76, code present, not advertised) |
|---|---|---|---|---|---|
| Initiator | service client | service client | service client | kharej client | service node (`reverse_service`) |
| Listener | public server | public server | public server | Iran server | public node (ingress) |
| Control channel | per service | per client (RunID) | SSH connection | per tunnel, signal bytes | the single authenticated session |
| Data stream | new TCP per flow | work conn or yamux stream | SSH channel | pooled conn or smux stream | yamux stream, roles inverted (dialer accepts) |
| Mux | none | yamux (default) | SSH | optional smux | yamux |
| Service mapping | per service name | per proxy name | per remote string | per forwarded port | one target per tunnel |
| Auth | per-service token + nonce | token (md5+ts) / OIDC / opt. mTLS | host-key pin + user:pass + regex ACL | shared token | per-node identity (mTLS or Noise static key) + CRL |
| Stale session | new control replaces old | same RunID replaces | SSH conn drops | generation survives re-dial | new authenticated session replaces previous (PR #76) |
| Liveness | heartbeat both ways | heartbeat or mux keepalive | SSH ping with timeout | heartbeat signal + RTT | yamux keepalive |
| Return path | same data TCP | same conn/stream | same channel | same conn/stream | same yamux stream |

## 4. Findings that matter for ADR-0021

1. **Inverting stream roles on one authenticated session (what PR #76 does) is the lowest-risk design.** It is structurally the chisel/SSH model: no second credential for data streams, so rathole's bearer-digest weakness cannot occur, and it needs no new wire protocol beyond the existing hello/direction binding. frp and BackPack add a pool of pre-opened connections for latency; BAFT-TUNNEL's yamux streams give that without extra dials.
2. **Stale-session replacement is universal** (rathole "drop previous control channel", frp RunID replace, BackPack generation seating). PR #76 has the equivalent. The LAB gate must prove it against a half-open connection, not only a clean restart.
3. **Liveness needs an application timeout, not just TCP.** chisel's most-reported bug was a ping that never timed out on dead paths. The LAB gate should cut the path silently (drop, not reset) and measure time-to-detect and time-to-recover; yamux keepalive settings (`KEEPALIVE_INTERVAL`, write timeout 10 s) must be shown to bound it.
4. **Ingress when no session exists** differs: rathole/frp keep the visitor waiting for a work connection with a deadline; chisel refuses the remote when no channel opens. Define it explicitly (bounded queue, bounded wait, clear close) and test it.
5. **Service mapping**: all four references bind the public port from the server config or from a client request that the server validates against an allowlist (frp `allowPorts`, chisel user regex). BAFT-TUNNEL's "public node owns ingress" already keeps the bind decision on the public node; keep it there and never accept a bind request from the service node.
6. **Per-service tokens (rathole/frp) conflict with per-node identity.** Do not adopt them. Adopt names only for display and routing.
7. **Reverse authentication note for the owner's question on "peer authentication"**: none of the four references offers per-peer certificate identity with revocation as the default. BackPack, rathole and frp authenticate with shared secrets; chisel pins the server key and uses passwords for clients. BAFT-TUNNEL's model is stronger and should not be relaxed to fit an engine.

## 5. Guidance for the ADR-0021 acceptance run (design only)

The existing gate (two hosts, private side outbound-only, 64 KiB and 2 MiB integrity, reconnect, wrong identity/pin/revoked refused, occupied port, cleanup) is kept unchanged. Additions suggested by the references, to be run by the thread that owns the LAB:

| Added check | Reference origin | Pass criterion |
|---|---|---|
| Silent path cut (drop packets, no RST) for 30 s, then restore | chisel TASKS item 1 | Session declared dead within the documented keepalive bound; new session carries data without operator action |
| Half-open stale session: kill service node with no FIN, restart it before the public node notices | rathole/frp replace logic | New session accepted immediately; old one closed; no port re-bind |
| Visitor connects while no session exists | rathole/frp/chisel differences | Behaviour matches the documented bounded wait or refusal; no unbounded queue growth |
| Many concurrent visitors (e.g. 500) | frp/rathole pool design | No stream starvation; stream limit reached returns a clean close, not a hang |
| Public node restarts | all | Service node reconnects with backoff within the documented bound |
| Inbound path to the service node blocked by firewall for the whole run | ADR-0021 | Proves the private side only dials out |

Result classes remain honest: PASS only for measured behaviour, WARNING for partial, BLOCKED when the gate prerequisite is missing, UNKNOWN when not measured.
