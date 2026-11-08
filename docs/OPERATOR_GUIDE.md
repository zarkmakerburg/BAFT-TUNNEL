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


## Baseline node-key credential delivery (Ubuntu 22.04 / systemd 249+)

Ubuntu 22.04 with systemd 249 is the default supported baseline. Use standard systemd service credentials for this baseline; do not upgrade the OS or systemd to use BAFT-TUNNEL.

1. Create a root-only source directory and install the node key there:
   ```sh
   install -d -o root -g root -m 0700 /etc/baft-tunnel/credentials/INSTANCE
   install -o root -g root -m 0600 ./node.key /etc/baft-tunnel/credentials/INSTANCE/node-key
   ```
2. Create `/etc/systemd/system/baft-tunnel@INSTANCE.service.d/credentials.conf`:
   ```ini
   [Service]
   LoadCredential=node-key:/etc/baft-tunnel/credentials/INSTANCE/node-key
   ```
3. In the instance JSON, remove `key` and set `key_credential` to `node-key`. Never set both fields.
4. Run `systemctl daemon-reload`, then start only the explicitly selected instance. `apply` validates the config and resolves the credential inside the service context (systemd 249 hides `/run/credentials` from `ExecStartPre`, so the unit has no pre-start validation); manual validation outside that context is expected to fail closed.
5. Verify service health and that the source remains root:root 0600 under a root:root 0700 directory. Do not copy the staged credential out of `CREDENTIALS_DIRECTORY` or log its contents.

This baseline uses plaintext source material protected by root-only filesystem permissions. It is **not encrypted at rest**. Full-disk encryption and host access controls are still recommended. Root compromise or offline disk access without disk encryption can expose the source key. The service receives a scoped credential, but the Rustls process still holds key material in memory.

Automated alternative to steps 1-2: `sudo scripts/install.sh --instance INSTANCE --node-key ./node.key` creates the directory, key file and drop-in with the exact ownership and modes above (it refuses symlinks, replaces a different existing key only with `--replace-credential`, and keeps the previous key in a root-only backup). `scripts/uninstall.sh` keeps credentials and configuration unless `--purge-config --yes` is given.

## Optional encrypted credential delivery (systemd 250+)

On hosts that independently support systemd 250+ and `systemd-creds`, operators may explicitly select `LoadCredentialEncrypted=` as an enhanced option. Follow the encrypted provisioning procedure below. Confirm the actual systemd version and `systemd-creds encrypt` capability; distribution labels alone are not sufficient. If encrypted mode is selected and unavailable, startup must fail closed. Never silently fall back to standard credentials or a filesystem key.

## Encrypted node-key delivery (systemd 250+; opt-in)

The application supports a key_credential JSON source. This mode must be paired with systemd encrypted credential delivery; it is not enabled by the shared unit by default.

1. Confirm the host actually supports the feature: systemd --version must report 250 or newer, and systemd-creds --help must expose encrypt. Ubuntu 22.04 / systemd 249 LAB hosts do not qualify.
2. As root, create a protected credential store and encrypt the node key using systemd's automatic TPM2/host key selection (never null-key mode):
      install -d -o root -g root -m 0700 /etc/credstore.encrypted/baft-tunnel
      systemd-creds encrypt --name=node-key --with-key=auto ./node.key /etc/credstore.encrypted/baft-tunnel/INSTANCE-node-key
      chown root:root /etc/credstore.encrypted/baft-tunnel/INSTANCE-node-key
      chmod 0600 /etc/credstore.encrypted/baft-tunnel/INSTANCE-node-key
   Replace INSTANCE with the exact systemd instance name. Auto mode prefers TPM2 when available and follows systemd's documented host-key fallback; never use --with-key=null.
3. Create /etc/systemd/system/baft-tunnel@INSTANCE.service.d/credentials.conf with:
      [Service]
      LoadCredentialEncrypted=node-key:/etc/credstore.encrypted/baft-tunnel/INSTANCE-node-key
4. In the instance JSON, remove key and set key_credential to node-key; see examples/forward-systemd-credential.json. Never set both fields.
5. Run systemctl daemon-reload, then start only the explicitly selected instance. Its `apply` validation runs in the service context where systemd supplies CREDENTIALS_DIRECTORY; manual validation outside that context is expected to fail closed.
6. Confirm the unit's credential is loaded and the tunnel starts. Do not copy decrypted credentials out of the service credential directory or log their contents.

If encrypted delivery is selected but the host, encrypted credential file, credential ID, or service-manager delivery is unavailable, startup must fail. Do not fall back to a plaintext key path or LoadCredential=. Systemd's selected host and/or TPM2 key binds the credential to the host OS and/or TPM device; this is not a claim of TPM non-exportable signing. The Rustls TLS stack still consumes the decrypted key in process memory. The Ubuntu 22.04 baseline uses standard LoadCredential=; the optional encrypted mode is not required for baseline support.

## Remaining operator workflow

Before production use, this guide still needs tunnel plans, apply receipts, key rotation, backups, restore drills, monitoring, incident response, upgrade/rollback, and safe uninstall. The current real-host LAB uses temporary processes and files under `/tmp`; it does not install BAFT-TUNNEL as a persistent service.


## Controlled service shutdown

The `serve` and `forward` runtime handles SIGTERM (systemd stop) and Ctrl-C. It stops accepting new peer/local connections, signals the active Yamux session to close with GOAWAY, and waits for session cleanup up to the configured 20-second shutdown deadline. If the deadline expires, remaining session tasks are aborted so systemd is not left waiting indefinitely. Active forwarded TCP streams are closed with their Yamux session; they are not migrated or transparently resumed. Clients may reconnect and establish new streams after the service starts again.

For systemd, use `systemctl stop baft-tunnel@INSTANCE`; the unit's `TimeoutStopSec=20s` bounds the same shutdown window. Do not use SIGKILL for routine stops.

## Guarded offline CRL update (ADR-0015)

CRLs are still issued only on the offline CA. Transfer only the signed `ca.crl` to both nodes through the approved operator channel. Do not copy the CA key, CA database, or passphrase.

1. On each node, validate the candidate without changing files or services:
   `bash scripts/pki/apply-crl.sh check /etc/baft-tunnel/pki/ca.crt /etc/baft-tunnel/pki/ca.crl ./candidate/ca.crl`
2. Confirm both nodes report the same candidate CRL number and SHA-256 fingerprint. The check rejects malformed PEM/trailing data, invalid signature or issuer, missing CRL Number, equal/lower CRL Number, and expired/not-yet-valid candidates.
3. As root, activate one already-running instance at a time:
   `bash scripts/pki/apply-crl.sh apply /etc/baft-tunnel/pki/ca.crt /etc/baft-tunnel/pki/ca.crl ./candidate/ca.crl iran`
   The helper serializes local updates with a per-CRL lock, revalidates the staged bytes, keeps a root-only rollback copy, atomically replaces the live CRL, restarts the selected instance, and checks that it is active. It refuses to start or enable a stopped instance. If restart/active verification fails, it restores the previous CRL and attempts recovery.
4. If any activation fails, stop the rollout. Restore the prior CRL on every node already updated and verify recovery. A rollback restores the old trust state and may re-admit a revoked certificate; it is an explicit availability recovery, not a security-neutral operation.
5. After both nodes activate, verify rejection of the revoked identity and reconnection of the intended peer. There is no distributed transaction; interruption during the rollout is expected. The helper never contacts the other node or distributes files.

The helper does not automate CA rollover, pin overlap, or remote CRL distribution. Run `bash scripts/pki/test-apply-crl.sh` for its isolated temporary-CA validation suite. On a disposable systemd 250+ host, `bash scripts/pki/test-crl-activation-systemd.sh` additionally exercises real transient-service activation and rollback; neither test touches a live CA or production service.

## CA rollover naming (ADR-0017 B1)

During a CA rollover the trust bundle holds the old and the new CA, each with its own CRL. CRLs are matched to their CA by subject name, so create the replacement CA with a different name: `bash scripts/pki/baft-pki.sh init-ca ./offline-ca-2 "BAFT-TUNNEL Private CA 2026-11"`. A node also accepts up to two comma-separated `peer_fingerprint` pins while certificates are replaced. If a CRL for one CA in the bundle is missing or expired, certificates from that CA are rejected (fail closed).

## CRL freshness report (ADR-0017 A1)

`bash scripts/pki/crl-status.sh /etc/baft-tunnel/ca.crl [WARN_HOURS]` prints one line per CRL in the file (issuer, CRL number, nextUpdate, hours remaining) and exits 1 if any CRL is under WARN_HOURS (default 72) or expired. It only reports; it never fetches or changes anything. Re-issue CRLs weekly with a validity of at least 14 days, push them to each node, and run this check from cron or a systemd timer you own.

## Activating a CRL bundle during CA rollover (ADR-0017 B1)

`apply-crl.sh` accepts a bundle of 1–4 CA certificates as `CA_CERT` and a bundle with one CRL per CA as the CRL files. The candidate must contain a CRL for every configured CA, each signed by that CA, each with a CRL number greater than the installed CRL of the same issuer, and none expired; a CRL from an unconfigured CA, a duplicate issuer, or a CA bundle with two identical subjects is rejected. A CA that has no installed CRL yet (the new CA) is allowed. Removing the old CA is a separate, explicit step: pass the CA bundle without it and a candidate without its CRL.

## Pushing a CRL to nodes (ADR-0017 A1)

`bash scripts/pki/push-crl.sh --crl ./ca.crl --host root@NODE1 --host root@NODE2:2222 --remote-ca /etc/baft-tunnel/ca.crt --remote-crl /etc/baft-tunnel/ca.crl --instance NAME [--sudo] [--known-hosts FILE] [--key FILE]` copies the offline-signed CRL (or CRL bundle) to a private staging directory on each node and runs that node's `apply-crl.sh apply`, so signature, CRL number, expiry and rollback are all decided on the node. Nodes are handled one at a time and the run stops at the first failure; the failed node is rolled back by `apply-crl.sh`, nodes already activated stay activated. SSH uses `BatchMode` and `StrictHostKeyChecking=yes`. The helper refuses a CRL with under 72 hours left (`crl-status.sh`). `--dry-run` prints the plan and contacts nothing. Nodes never fetch CRLs and the CA key never leaves the offline CA. Test: `bash scripts/pki/test-push-crl.sh` (mock SSH/SCP).

## CA / pin rollover sequence (ADR-0017 B1)

1. Offline: create the new CA with a different name (`init-ca`), issue its CRL.
2. Per node, one at a time: push the CA bundle (old + new) and the bundle CRL (old CRL + new CRL) with `push-crl.sh`; verify the tunnel still works.
3. Offline: issue new node certificates from the new CA; install them; set `peer_fingerprint` to `OLD_PIN,NEW_PIN` on the peer (maximum two pins).
4. Verify handshakes with the new certificates in both directions.
5. After your stated deadline: set a single new pin, then remove the old CA and its CRL (separate explicit step). Each step can be reverted by restoring the previous files and pins.
6. If the old CA key may be compromised, do NOT use the overlap: replace everything at once (flag day) and check pins out of band.

Status: the CA bundle, per-CA CRLs, dual pins, bundle activation and the push helper are implemented and unit/mock tested; the full sequence has not yet been run on the real LAB hosts.

## Guided setup (P3-001)

Two hosts, no hand-written JSON. On each host, the commands are `setup init` and `setup join`; a third, `setup plan`, previews without writing anything.

1. `baft-tunnel setup plan --engine noise-tcp --role forward --direction direct --node-id iran --peer-id abroad --tunnel-id demo --service-port 8080 --peer-host PEER_IP` shows the configuration and files. Nothing is written.
2. `setup init` (same arguments) on both hosts. The serving host uses `--role serve` and no `--peer-host`. With `noise-tcp` it creates the node key (mode 0600, never overwritten, never exported). With `tcp-tls` it expects the `baft-pki.sh` output already in `--dir` (`node.key`, `node.crt`, `ca.crt`, `ca.crl`), checks that key, certificate, CA and CRL fit together, that the certificate belongs to this node, that `node.key` is private and that the CRL has at least 24 hours left. It then writes a public bundle (`bundle.json`) and prints a checksum.
3. Copy each bundle to the other host and compare the printed checksums over a channel you trust (for example read them aloud or send them in another messenger).
4. `setup join --bundle PEER_BUNDLE --checksum CHECKSUM_FROM_PEER` on each host verifies the bundle (checksum, engine, direction, tunnel id, ports, identities, opposite roles) and writes `config.json`. Existing files are never overwritten.
5. Review with `baft-tunnel config plan --config /etc/baft-tunnel/config.json`, then start the service as usual.

The bundle holds only public material (Noise public key or certificate pin, ids, ports, and for `tcp-tls` serving nodes the certificate name). It never contains a private key.


## Local status API (read-only, P3-002 A1)
```
baft-tunnel agent-token --out /etc/baft-tunnel/agent.token
baft-tunnel agent --config /etc/baft-tunnel/config.json --token-file /etc/baft-tunnel/agent.token
curl -H "Authorization: Bearer $(cat /etc/baft-tunnel/agent.token)" http://127.0.0.1:18080/v1/status
```
Loopback only; the output is the redacted support report plus `listener_accepting`. The agent does not start or change a tunnel.
