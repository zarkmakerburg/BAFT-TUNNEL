# BAFT-TUNNEL quick start

> **Legacy guide — pending U8 reconciliation.** Its steps have not yet been reconciled line-by-line with the running signed-RC.7 U7 acceptance. For the current supported installed-service procedure, use the [Operator guide](OPERATOR_GUIDE.md); this file will be rewritten after U7 evidence is final.

> Pre-release (Preview). Use on test hosts you can wipe. Not production-ready.

Two Ubuntu 22.04+ hosts (systemd 249+): **abroad** serves, **iran** forwards. Run every step as root on both hosts unless noted.

## 0. One-line install (Ubuntu 22.04+, x86_64)

On each host, replace t1 with that host's instance name:

    curl -fsSL https://raw.githubusercontent.com/zarkmakerburg/BAFT-TUNNEL/main/scripts/quick-install.sh | sudo bash -s -- --register t1

This selects the newest published release (including Preview while the project has no stable release), verifies its Sigstore signature and SHA-256, installs the binary, systemd units and local panel agent, and does **not** start a tunnel. Add --stable to require a stable release, or --tag vX.Y.Z to pin a release. The installer currently supports Linux x86_64 only.

## 1. Download and verify the signed package

```bash
TAG=vX.Y.Z   # a post-hardening release signed from its version tag
R=https://github.com/zarkmakerburg/BAFT-TUNNEL/releases/download/$TAG
P=baft-tunnel-$TAG-linux-x86_64
curl -fLO $R/$P.tar.gz -fLO $R/SHA256SUMS -fLO $R/SHA256SUMS.cosign.bundle
cosign verify-blob SHA256SUMS --bundle SHA256SUMS.cosign.bundle \
  --certificate-identity "https://github.com/zarkmakerburg/BAFT-TUNNEL/.github/workflows/release.yml@refs/tags/$TAG" \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com
sha256sum -c --ignore-missing SHA256SUMS      # must print: $P.tar.gz: OK
tar -xzf $P.tar.gz && cd $P
```

Use a release published after the tag-only signing policy was introduced. Legacy releases signed from `refs/heads/main` are intentionally rejected. Without cosign, `sha256sum -c` alone only proves the download matches `SHA256SUMS`, not who signed it.

## 2. Install (binary, units, menu, local agent)

```bash
sudo scripts/install.sh --binary ./baft-tunnel --register t1
```

`sudo baft-tunnel menu` (Persian/English) wraps every step below if you prefer a menu.

## 3. Pair the two hosts

This legacy example explicitly selects `noise-tcp`, which is Experimental and Direct-only; it is not the default or the supported engine. For the supported `tcp-tls` path and its offline PKI prerequisites, follow the [Operator guide](OPERATOR_GUIDE.md).

```bash
C="--engine noise-tcp --direction direct --tunnel-id t1 --service-port 8080 --instance t1"
# abroad:
sudo baft-tunnel setup init $C --role serve   --node-id abroad --peer-id iran
# iran:
sudo baft-tunnel setup init $C --role forward --node-id iran --peer-id abroad --peer-host ABROAD_IP
```

Each prints a **Checksum**. Copy `/etc/baft-tunnel/bundle.json` to the other host, read the checksums to each other over a channel you trust, then on each host:

```bash
sudo baft-tunnel setup join --bundle PEER_BUNDLE.json --checksum PEER_CHECKSUM
```

## 4. Start and check

```bash
sudo baft-tunnel config plan --config /etc/baft-tunnel/t1.json    # review
ssh -L 18080:127.0.0.1:18080 HOST                                 # from your PC
sudo cat /var/lib/baft-tunnel-agent/token                         # panel token
```

Open `http://127.0.0.1:18080/`, paste the token, Tunnels → **Start**, then **Run diagnostics**. Without the panel: `sudo systemctl enable --now baft-tunnel@t1`.

## 5. Day-2

- Logs: `journalctl -u baft-tunnel@t1`; support report: `baft-tunnel support-bundle --config /etc/baft-tunnel/t1.json`.
- Update: `sudo scripts/update.sh --binary ./baft-tunnel --instance t1` from a newer package; undo with `scripts/rollback.sh BACKUP_DIR`.
- tcp-tls only: renew the CRL before it expires (`baft-pki.sh renew-crl`, `push-crl.sh`).
- Remove: `sudo scripts/uninstall.sh --stop` (add `--purge-config --yes` to delete config and keys).

Limits: no firewall automation; open the tunnel port yourself. Reverse is Preview. See [Troubleshooting](https://github.com/zarkmakerburg/BAFT-TUNNEL/blob/main/docs/TROUBLESHOOTING.md).

---

## Appendix: developer build from source (legacy)

> Builds from source and runs the binary by hand, outside systemd. For servers use the steps above.

### Requirements

- Linux or macOS with Rust 1.85 or newer.
- A private CA and two leaf certificates signed by it. See the [offline PKI bootstrap](OPERATOR_GUIDE.md) for the experimental issuance workflow.
- Both leaf certificates must be valid for TLS client and server authentication. The server certificate must contain the DNS/IP subject alternative name used by `--server-name`.
- Provide a current CA-signed certificate revocation list (CRL) on both peers; a missing, expired, or unusable CRL fails closed. CRL updates are not hot-reloaded: distribute the updated CRL and restart both endpoint processes.
- Exchange SHA-256 leaf-certificate fingerprints out of band.

### Build

```bash
cargo build --release
```

### Destination-side peer

```bash
./target/release/baft-tunnel serve \
  --listen 0.0.0.0:9443 --target 127.0.0.1:8080 \
  --local-id node-abroad --peer-id node-iran --tunnel-id demo-tunnel --direction direct \
  --peer-fingerprint REPLACE_WITH_IRAN_CERT_SHA256 \
  --ca ./pki/ca.pem --crl ./pki/ca.crl --cert ./pki/abroad.pem --key ./pki/abroad-key.pem
```

### Source-side local forwarder

```bash
./target/release/baft-tunnel forward \
  --listen 127.0.0.1:18080 --remote 203.0.113.10:9443 --server-name tunnel.example.test \
  --local-id node-iran --peer-id node-abroad --tunnel-id demo-tunnel --direction direct \
  --peer-fingerprint REPLACE_WITH_ABROAD_CERT_SHA256 \
  --ca ./pki/ca.pem --crl ./pki/ca.crl --cert ./pki/iran.pem --key ./pki/iran-key.pem
```

Connections to `127.0.0.1:18080` are forwarded to the server's fixed configured destination. Both peers must use the same explicit `--direction` value. For Reverse mode, place `serve` on the Iran-side node and `forward` on the abroad-side node, and set `--direction reverse` on both commands. Replace every example value before use. Never publish private keys.

### Safety and limitations

- The server destination is operator-configured; a connecting peer cannot choose it.
- The peer must pass CA validation, a current CRL check, the configured certificate pin, node-identity binding, and the configured Direct/Reverse direction authorization.
- Each Yamux stream carries a bounded stream-open authorization frame binding the tunnel, direction, and TCP-forward capability.
- TLS 1.3 is the only enabled version; no plaintext fallback exists.
- Per-session Yamux streams and process-wide forwarded streams are bounded.
- The source-side forwarder retries carrier establishment with bounded exponential backoff and jitter. A carrier loss still terminates in-flight TCP streams; they are not transparently resumed. New local connections can wait in a bounded queue during reconnect.
- These commands do not install anything or change host settings. An installer exists (`scripts/install.sh`, see the Operator guide); there is no firewall automation.


### JSON configuration workflow

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


### Redacted support bundle

Create a diagnostic report that excludes private-key contents, certificate contents, certificate fingerprints, file paths and network endpoints:

```bash
baft-tunnel support-bundle --config ./config.json
```

The report includes only runtime version/platform, tunnel role and logical identities, stream limit, whether certificate files are present, and explicit NOT_RUN markers for network probes and real-host LAB. Review the logical identity and tunnel labels before sharing the report.
