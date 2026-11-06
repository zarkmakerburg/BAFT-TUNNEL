# Operator Guide

## Phase 1 offline certificate bootstrap (experimental)

This is a manual offline-CA workflow for the current TCP/TLS transport. It is not online enrollment and is not yet a production control plane.

1. On a dedicated offline administration machine, initialize the encrypted CA:
   `bash scripts/pki/baft-pki.sh init-ca ./offline-ca`
   OpenSSL prompts for the CA key passphrase. Keep the whole CA directory offline and back it up securely.
2. On each endpoint, generate its private key and CSR locally:
   `bash scripts/pki/baft-pki.sh new-node iran ./node-iran`
   Use the actual enrolled node ID. Keep `node.key` on that endpoint; transfer only `node.csr` to the offline CA.
3. On the offline CA, issue a node certificate with the endpoint's DNS name or IP address:
   `bash scripts/pki/baft-pki.sh issue-node ./offline-ca iran ./node-iran/node.csr ./issued-iran tunnel.iran.example.test 90`
   Repeat with the other node ID, CSR, and its own server name. The certificate contains both the exact `urn:baft-tunnel:node:<node-id>` URI SAN and the DNS/IP SAN required by `--server-name`. The CA does not copy CSR-requested extensions.
4. Return only `node.crt`, `ca.crt`, and `ca.crl` to the corresponding endpoint. Its private key remains where it was generated.
5. Verify the bundle before configuring the tunnel:
   `bash scripts/pki/baft-pki.sh verify-node ./offline-ca ./issued-iran/ca.crl ./issued-iran/node.crt iran tunnel.iran.example.test`
6. Install the service runtime's private-key copy with least privilege. The generated key remains mode `0600`; do not relax its permissions in the enrollment workspace. For a systemd installation, place the runtime copy under the root-owned, service-group-readable PKI directory:
   ```sh
   install -d -o root -g baft-tunnel -m 0750 /etc/baft-tunnel/pki
   install -o root -g baft-tunnel -m 0640 ./node-iran/node.key /etc/baft-tunnel/pki/node.key
   install -o root -g root -m 0644 ./issued-iran/node.crt /etc/baft-tunnel/pki/node.crt
   install -o root -g root -m 0644 ./issued-iran/ca.crt /etc/baft-tunnel/pki/ca.crt
   install -o root -g root -m 0644 ./issued-iran/ca.crl /etc/baft-tunnel/pki/ca.crl
   ```
   Configure the endpoint to use these installed paths. The private key is readable only by root and the dedicated `baft-tunnel` service group; never copy the offline CA key to the host.
7. Configure both endpoints with the same trusted CA, current CRL, expected peer node identity, tunnel ID, direction, and out-of-band leaf SHA-256 pin. The `--server-name` value must match the peer certificate's DNS/IP SAN.
8. To revoke a node, on the offline CA run:
   `bash scripts/pki/baft-pki.sh revoke-node ./offline-ca ./issued-iran/node.crt ./revoked`
   Deliver the new `ca.crl` to both peers and restart both endpoint processes. Verify that revoked peers are rejected before restoring traffic.

The CA key is encrypted and must never be copied to endpoints. Node private keys are currently unencrypted PEM files. The enrollment-generated key is mode `0600`; the separately installed systemd runtime copy is root-owned, group `baft-tunnel`, mode `0640`, with its parent directory root:`baft-tunnel` mode `0750`. This is a least-privilege filesystem arrangement, not encrypted-at-rest key storage. Encrypted node-key storage, OS keystore/HSM integration, online enrollment, automated CRL distribution, rotation, and safe overlapping pin rollover remain open acceptance work.

Run the isolated PKI regression suite with `bash scripts/pki/test.sh`. It creates and removes a temporary test CA; it does not touch the offline CA or LAB hosts.

## Remaining operator workflow

Before production use, this guide still needs tunnel plans, apply receipts, key rotation, backups, restore drills, monitoring, incident response, upgrade/rollback, and safe uninstall. The current real-host LAB uses temporary processes and files under `/tmp`; it does not install BAFT-TUNNEL as a persistent service.
