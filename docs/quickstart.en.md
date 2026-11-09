# BAFT-TUNNEL Phase 1 — local TCP forwarding

> Experimental development build. Use only on loopback or disposable test hosts. Not production-ready.
>
> **Legacy developer path.** This page builds from source and runs the binary by hand. For a server, use the installed-service path in the [Operator guide](OPERATOR_GUIDE.md) (`scripts/install.sh`, `baft-tunnel@` unit). A rewritten quickstart follows the MVP acceptance run.

## Requirements

- Linux or macOS with Rust 1.85 or newer.
- A private CA and two leaf certificates signed by it. See the [offline PKI bootstrap](OPERATOR_GUIDE.md) for the experimental issuance workflow.
- Both leaf certificates must be valid for TLS client and server authentication. The server certificate must contain the DNS/IP subject alternative name used by `--server-name`.
- Provide a current CA-signed certificate revocation list (CRL) on both peers; a missing, expired, or unusable CRL fails closed. CRL updates are not hot-reloaded: distribute the updated CRL and restart both endpoint processes.
- Exchange SHA-256 leaf-certificate fingerprints out of band.

## Build

```bash
cargo build --release
```

## Destination-side peer

```bash
./target/release/baft-tunnel serve \
  --listen 0.0.0.0:9443 --target 127.0.0.1:8080 \
  --local-id node-abroad --peer-id node-iran --tunnel-id demo-tunnel --direction direct \
  --peer-fingerprint REPLACE_WITH_IRAN_CERT_SHA256 \
  --ca ./pki/ca.pem --crl ./pki/ca.crl --cert ./pki/abroad.pem --key ./pki/abroad-key.pem
```

## Source-side local forwarder

```bash
./target/release/baft-tunnel forward \
  --listen 127.0.0.1:18080 --remote 203.0.113.10:9443 --server-name tunnel.example.test \
  --local-id node-iran --peer-id node-abroad --tunnel-id demo-tunnel --direction direct \
  --peer-fingerprint REPLACE_WITH_ABROAD_CERT_SHA256 \
  --ca ./pki/ca.pem --crl ./pki/ca.crl --cert ./pki/iran.pem --key ./pki/iran-key.pem
```

Connections to `127.0.0.1:18080` are forwarded to the server's fixed configured destination. Both peers must use the same explicit `--direction` value. For Reverse mode, place `serve` on the Iran-side node and `forward` on the abroad-side node, and set `--direction reverse` on both commands. Replace every example value before use. Never publish private keys.

## Safety and limitations

- The server destination is operator-configured; a connecting peer cannot choose it.
- The peer must pass CA validation, a current CRL check, the configured certificate pin, node-identity binding, and the configured Direct/Reverse direction authorization.
- Each Yamux stream carries a bounded stream-open authorization frame binding the tunnel, direction, and TCP-forward capability.
- TLS 1.3 is the only enabled version; no plaintext fallback exists.
- Per-session Yamux streams and process-wide forwarded streams are bounded.
- The source-side forwarder retries carrier establishment with bounded exponential backoff and jitter. A carrier loss still terminates in-flight TCP streams; they are not transparently resumed. New local connections can wait in a bounded queue during reconnect.
- These commands do not install anything or change host settings. An installer exists (`scripts/install.sh`, see the Operator guide); there is no firewall automation.


## JSON configuration workflow

A sample forwarder configuration is available at `examples/forward.json`. Paths are resolved relative to the process working directory. The required `crl` field points to a current PEM CRL signed by the configured CA.

Validate the JSON, identities, socket addresses, TLS certificate chain and private key without opening listeners:

```bash
baft-tunnel config validate --config examples/forward.json
```

Preview the effective plan (private key contents and certificate contents are never printed):

```bash
baft-tunnel config plan --config examples/forward.json
```

Start only after reviewing the plan. Applying a config requires the explicit confirmation flag:

```bash
baft-tunnel apply --config examples/forward.json --confirm
```

The apply command starts the configured tunnel process in the foreground. It does not install a service, change firewall rules, alter routes, or modify system settings. Keep private key files readable only by the service account. The example fingerprint and endpoint are placeholders and must be replaced.


## Redacted support bundle

Create a diagnostic report that excludes private-key contents, certificate contents, certificate fingerprints, file paths and network endpoints:

```bash
baft-tunnel support-bundle --config ./config.json
```

The report includes only runtime version/platform, tunnel role and logical identities, stream limit, whether certificate files are present, and explicit NOT_RUN markers for network probes and real-host LAB. Review the logical identity and tunnel labels before sharing the report.
