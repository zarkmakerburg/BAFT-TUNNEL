# BAFT-TUNNEL Phase 1 — local TCP forwarding

> Experimental development build. Use only on loopback or disposable test hosts. Not production-ready.

## Requirements

- Linux or macOS with Rust 1.85 or newer.
- A private CA and two leaf certificates signed by it.
- Both leaf certificates must be valid for TLS client and server authentication. The server certificate must contain the DNS/IP subject alternative name used by `--server-name`.
- Exchange SHA-256 leaf-certificate fingerprints out of band.

## Build

```bash
cargo build --release
```

## Destination-side peer

```bash
./target/release/baft-tunnel serve \
  --listen 0.0.0.0:9443 --target 127.0.0.1:8080 \
  --local-id node-abroad --peer-id node-iran --tunnel-id demo-tunnel \
  --peer-fingerprint REPLACE_WITH_IRAN_CERT_SHA256 \
  --ca ./pki/ca.pem --cert ./pki/abroad.pem --key ./pki/abroad-key.pem
```

## Source-side local forwarder

```bash
./target/release/baft-tunnel forward \
  --listen 127.0.0.1:18080 --remote 203.0.113.10:9443 --server-name tunnel.example.test \
  --local-id node-iran --peer-id node-abroad --tunnel-id demo-tunnel \
  --peer-fingerprint REPLACE_WITH_ABROAD_CERT_SHA256 \
  --ca ./pki/ca.pem --cert ./pki/iran.pem --key ./pki/iran-key.pem
```

Connections to `127.0.0.1:18080` are forwarded to the server's fixed configured destination. Replace every example value before use. Never publish private keys.

## Safety and limitations

- The server destination is operator-configured; a connecting peer cannot choose it.
- The peer must pass both CA validation and the configured certificate pin.
- TLS 1.3 is the only enabled version; no plaintext fallback exists.
- Per-session Yamux streams and process-wide forwarded streams are bounded.
- The source-side forwarder retries carrier establishment with bounded exponential backoff and jitter. A carrier loss still terminates in-flight TCP streams; they are not transparently resumed. New local connections can wait in a bounded queue during reconnect.
- No installer or firewall automation exists. These commands do not change host settings.
