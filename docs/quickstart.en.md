# BAFT-TUNNEL — Quick start (English)

> **Signed pre-release v0.1.0-rc.7; not a stable release.** Start in a test environment. Trusted two-host acceptance passed on 2026-11-11 in [U7 run 43063199428](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/43063199428). This proves the bounded Direct workflow below, not production readiness.

## Supported path and requirements

- Two x86_64 Ubuntu 22.04 LTS hosts with systemd 249; one is **serve** (normally abroad), the other **forward** (normally Iran).
- A reachable TCP tunnel port on the serve host. Open it in your provider/network firewall yourself; BAFT-TUNNEL does not change firewall rules.
- A local TCP service on the serve host, bound to loopback, whose port will be carried through the tunnel.
- Supported pre-release path: **tcp-tls + Direct**. Reverse is Preview; noise-tcp is Experimental Direct only; BAFT is Preview and cannot yet be configured by the Wizard.

## 1. Download and verify the signed release

On each host, download the package and signed checksum files from the same release. You need curl, cosign, sha256sum and tar installed:

```bash
TAG=v0.1.0-rc.7
BASE="https://github.com/zarkmakerburg/BAFT-TUNNEL/releases/download/$TAG"
PKG="baft-tunnel-$TAG-linux-x86_64"
curl -fL "$BASE/$PKG.tar.gz" -o "$PKG.tar.gz"
curl -fL "$BASE/SHA256SUMS" -o SHA256SUMS
curl -fL "$BASE/SHA256SUMS.cosign.bundle" -o SHA256SUMS.cosign.bundle

cosign verify-blob SHA256SUMS --bundle SHA256SUMS.cosign.bundle \
  --certificate-identity "https://github.com/zarkmakerburg/BAFT-TUNNEL/.github/workflows/release.yml@refs/tags/$TAG" \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com
sha256sum --ignore-missing --check SHA256SUMS
tar -xzf "$PKG.tar.gz"
cd "$PKG"
```

Do not continue unless Cosign verification succeeds and the package checksum reports OK. A checksum without signature verification does not establish who published the package.

## 2. Install and register the local Agent

On each host, choose a unique instance name (example: t1):

```bash
sudo scripts/install.sh --binary ./baft-tunnel --register t1
sudo baft-tunnel menu
```

This installs the binary, systemd units, local Agent and bilingual menu; it does **not** start the tunnel. The supported Ubuntu 22.04 baseline uses systemd LoadCredential= for service-scoped key delivery; this is not encryption at rest. Open the panel through an SSH tunnel as shown in Step 5. **Important:** the menu's standalone “Pair the two nodes” helper currently creates noise-tcp (Experimental); do not use that menu pairing shortcut for the supported tcp-tls path. Use the panel Wizard below.

## 3. Enroll tcp-tls credentials through the panel

On each host, open the local panel over SSH (Step 5) and use the Wizard's certificate/PKI section:

1. Enter this node's own DNS/IP certificate identity separately from the peer's certificate name.
2. Choose **Generate CSR**. The private key is created on the node and never leaves it. Copy/download only the public CSR and its displayed metadata.
3. On a separate offline CA machine, issue the certificate with the documented `scripts/pki/baft-pki.sh issue-node` procedure. The CA private key never goes to either server. Use the exact node ID and DNS/IP SAN requested by the panel; do not add wildcard or extra identity SANs.
4. Return only the leaf certificate, CA certificate and current CRL to the matching node, then upload those three public files in the panel. The root helper validates that the leaf matches the locally held key, the CA/CRL are valid, the certificate identity and SAN are exact, and the certificate is not revoked. Upload does not restart the service.
5. Wait for the panel's successful install receipt before continuing. If validation fails, the previous active credential set remains unchanged.

The CA key stays offline. Do not upload a private key, CA database or passphrase. See [Operator Guide](OPERATOR_GUIDE.md) for offline CA issuance, certificate renewal, CRL renewal and push details. This is offline/manual signing, not online enrollment.

## 4. Pair the nodes with the Wizard (Direct)

On each host, open **Wizard** in the local panel:

1. Select **tcp-tls**, **Direct**, and this host's role. Use serve on the host with the reachable tunnel listener (normally abroad), and forward on the other host.
2. Enter matching tunnel ID, service port and tunnel port on both nodes. On the forward node, enter the serve host's reachable IP/DNS name.
3. Follow the displayed Init step on each node. Exchange only the generated public pairing bundles; never exchange private keys.
4. Compare the displayed checksums over a separate trusted channel (for example, a phone call). Complete Join on each node using the peer's bundle and the checksum read from the other side.
5. Review the plan and the peer identity before Apply. Apply is a separate explicit action.

A bundle is not a secret, but its checksum must be compared out of band to detect substitution. Never accept a mismatch. Setup refuses to overwrite an existing instance; resolve the existing installation deliberately before retrying.

## 5. Start and verify

On both nodes, choose **Start** from the panel after both pairing steps have succeeded. Start enables the instance at boot; Stop disables it at boot; Restart restarts it. Confirm the action when prompted. The bilingual menu also provides these service lifecycle and maintenance actions.

For the local panel, create an SSH tunnel from your workstation:

```bash
ssh -L 18080:127.0.0.1:18080 USER@HOST
```

Open http://127.0.0.1:18080/ and use the local Agent token shown by the menu or stored at /var/lib/baft-tunnel-agent/token (read it as root; never share it). Use **Tunnels → Start** and **Run diagnostics**. The panel and Agent listen on loopback; do not expose port 18080 publicly.

Generate application traffic through the configured service port. Diagnostics separates TCP reachability, secure channel, identity, authorization, session and data-path evidence. **Integrity remains Unknown in ordinary product diagnostics**; a reachable port or session is not proof of payload integrity. U7's 64 KiB and 2 MiB hash checks are separate LAB evidence.

## 6. Day-two operations

- **Logs/status:** use Status and diagnostics in the menu; system logs: journalctl -u baft-tunnel@t1.
- **Backup/restore:** use the menu. Restore replaces configuration and keys and stops affected tunnels first; verify the restored instance afterwards.
- **Update:** download and verify the next signed package first, then use the menu's guarded Update action with its binary. The installer records a backup. If the new binary fails its start check, the guarded update rolls back automatically. Manual rollback: sudo scripts/rollback.sh BACKUP_DIR.
- **CRL renewal:** for tcp-tls, renew only on the offline CA and use the operator push/activation workflow in the [Operator Guide](OPERATOR_GUIDE.md). Separate U1 LAB run [42656507521](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42656507521) verified a 30-day CRL push, increasing CRL number, recovery and rejection of short/replayed CRLs.
- **Uninstall:** sudo scripts/uninstall.sh --stop removes installed service/Agent components but keeps configuration and keys. Add --purge-config --yes only when you intentionally want to delete them; this is destructive.

## Verified limits

- U7 [run 43063199428](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/43063199428) passed on signed RC.7: clean install, guided Direct setup, Apply start on both roles, 64 KiB/2 MiB hash integrity, diagnostics/status/traffic/audit, RC.5→RC.7 upgrade, 3600-second soak (264 rounds, zero failed probes, no restarts), process recovery, backup/restore, guarded update and failed-update rollback, certificate revocation refusal, uninstall and cleanup.
- U7 did not prove Reverse, BAFT, Noise, production readiness or a stable release. ready remains Unknown by design. Firewall changes are not managed by the product.
- An unrelated six-line firewall/address-set drift was observed on the abroad LAB host during U7; no BAFT-TUNNEL firewall rules were present. Its source remains open as S10. Do not claim firewall neutrality.
- Active TCP streams are not transparently resumed after carrier loss; applications must reconnect.
- This quick start does not authorize a stable release or production deployment.

See [Troubleshooting](TROUBLESHOOTING.md) and the [Operator Guide](OPERATOR_GUIDE.md) for maintenance details.
