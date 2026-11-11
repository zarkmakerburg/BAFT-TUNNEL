# BAFT-TUNNEL — Quick start (English)

> **Signed pre-release v0.1.0-rc.7; not a stable release.** Start in a test environment. Trusted two-host acceptance passed on 2026-11-11 in [U7 run 43063199428](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/43063199428). This proves the bounded Direct workflow below, not production readiness.

## Supported path and requirements

- Two x86_64 Ubuntu 22.04 LTS hosts with systemd 249; one is **serve** (normally abroad), the other **forward** (normally Iran).
- A reachable TCP tunnel port on the serve host. Open it in your provider/network firewall yourself; BAFT-TUNNEL does not change firewall rules.
- A local TCP service on the serve host, bound to loopback, whose port will be carried through the tunnel.
- Supported pre-release path: **tcp-tls + Direct**. Reverse is Preview; noise-tcp is Experimental Direct only; BAFT is Preview and cannot yet be configured by the Wizard.

## 1. Download and verify the signed release

On each host, download the package and signed checksum files from the same release:

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

This installs the binary, systemd units, local Agent and bilingual menu; it does **not** start the tunnel. Choose English or Persian in the menu. The supported Ubuntu 22.04 baseline uses systemd LoadCredential= for service-scoped key delivery; this is not encryption at rest.

## 3. Prepare offline TLS material

The CA private key must stay on a separate offline administration machine. Generate each node key and CSR on that node, issue its certificate on the offline CA, and return only that node's private key plus its public certificate, CA certificate and current CRL. Never copy the CA private key to either server.

Follow **Phase 1 offline certificate bootstrap** in the [Operator Guide](OPERATOR_GUIDE.md) for the exact new-node, issue-node, verify-node and file-permission steps. Use each host's real node ID and the DNS name/IP the peer will use. The certificate must bind the exact node ID and peer-facing DNS/IP SAN. Keep the node private key mode 0600; keep CA material offline.

## 4. Pair the nodes (Direct)

On each host, in sudo baft-tunnel menu, choose **Pair the two nodes**:

1. Select tcp-tls, **Direct**, and this host's role.
2. Use serve on the host with the reachable tunnel listener (normally abroad); use forward on the other host.
3. Enter the same tunnel ID, service port and tunnel port on both nodes. On the forward node, enter the serve host's reachable IP/DNS name.
4. Run init on each node. Exchange the generated public pairing bundles; do not exchange private keys.
5. Compare the printed checksums over a separate trusted channel (for example, a phone call). On each node, complete join using the other node's bundle and the checksum read from the other side.
6. Review the configuration plan before proceeding.

A bundle is not a secret, but its checksum must be compared out of band to detect substitution. Never accept a mismatch. Do not overwrite an existing instance; choose a new name or deliberately resolve the existing installation first.

## 5. Start and verify

On both nodes, choose **Tunnel → Start** in the menu. Start enables the instance at boot; Stop disables it at boot; Restart restarts it. Confirm the action when prompted.

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
