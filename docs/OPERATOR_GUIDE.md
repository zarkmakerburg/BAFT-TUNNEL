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
6. Configure both endpoints with the same trusted CA, current CRL, expected peer node identity, tunnel ID, direction, and out-of-band leaf SHA-256 pin. The `--server-name` value must match the peer certificate's DNS/IP SAN.
7. To revoke a node, on the offline CA run:
   `bash scripts/pki/baft-pki.sh revoke-node ./offline-ca ./issued-iran/node.crt ./revoked`
   Deliver the new `ca.crl` to both peers and restart both endpoint processes. Verify that revoked peers are rejected before restoring traffic.

The CA key is encrypted and must never be copied to endpoints. Node private keys are currently unencrypted PEM files protected by mode `0600` and private directories because the runtime does not yet support encrypted key files. Use a dedicated service account and restrictive filesystem permissions. Encrypted node-key storage, OS keystore/HSM integration, online enrollment, automated CRL distribution, rotation, and safe overlapping pin rollover remain open acceptance work.

Run the isolated PKI regression suite with `bash scripts/pki/test.sh`. It creates and removes a temporary test CA; it does not touch the offline CA or LAB hosts.

## Remaining operator workflow

Before production use, this guide still needs tunnel plans, apply receipts, key rotation, backups, restore drills, monitoring, incident response, upgrade/rollback, and safe uninstall. The current real-host LAB uses temporary processes and files under `/tmp`; it does not install BAFT-TUNNEL as a persistent service.
