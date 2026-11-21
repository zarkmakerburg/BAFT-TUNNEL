# Fleet Join Member Activation Design

## Intent

Complete the Member side of Fleet Join after the Hub has issued a certificate. A new server receives a one-time JoinOffer from the operator, creates its private key and CSR locally, receives the issued public trust material over the pinned Join TLS session, durably installs its Member configuration, acknowledges delivery, and starts the existing outbound mTLS reporter. The Hub's existing certificate-bound first-report callback remains the only transition to `Enrolled`.

## Approaches considered

1. **Dedicated local CLI (chosen):** `baft-tunnel fleet join --instance NAME` reads the one-time offer from stdin, creates and stores the Member key/CSR locally, performs Join, installs the Member config, acknowledges, and queues the existing per-instance Agent restart. `--resume` retries an incomplete Join from protected local state or restarts an already configured Member. This keeps the protocol independent of the panel and SSH and uses the existing Agent/reporter lifecycle.
2. **Agent local API / panel flow:** better UI, but expands the authenticated local API and adds live reload/restart coordination before the underlying Member lifecycle exists. Defer until the CLI flow is accepted.
3. **Manual bundle plus operator-edited config:** smallest code change, but leaves configuration, restart, and recovery to manual steps and does not reliably orchestrate the first report. Rejected for this slice.

## Goals

- Provide an explicit `fleet join` CLI path that reads the encoded JoinOffer from stdin, never argv or logs.
- Generate an EC P-256 Member private key and signed CSR locally. The offer nonce must be present as exactly one PKCS#9 `challengePassword` attribute covered by the CSR signature.
- Never send or return Member private-key bytes. Keep the key root-owned and readable only by the Agent service group.
- Persist the offer/CSR in a root-only pending area before the first network submission so an interrupted pre-ACK attempt can retry the exact CSR and credentials.
- Validate the issued Member certificate against the locally generated key, Fleet CA, CRL, node identity and Fleet Node certificate profile before activation.
- Publish an immutable versioned Member bundle and update `/etc/baft-tunnel/fleet/NAME.json` atomically, preserving any existing Hub section.
- Send positive Join ACK only after the public trust bundle and Member configuration are durable. Do not ACK on validation, persistence, or configuration failure.
- Queue the existing per-instance Agent restart only after ACK. On restart-queue failure, retain retry state; `fleet join --resume` must be able to finish without generating a new key or CSR.
- Make exact Join retries safe after the Hub has moved the offer to `AwaitingReport`; retry must not reset or weaken the first-report binding.
- Start the existing Member reporter on Agent restart. Its first report is attempted immediately by the current `tokio::time::interval` behavior; only the Hub's accepted mandatory-mTLS report from the exact issued leaf can mark the offer `Enrolled`.

## Non-goals

- Panel UI, SSH enrollment, remote shell execution, a new listener, inbound Member networking, or any firewall/route/sysctl change.
- Any `CAP_NET_ADMIN` use, Go in core, Reverse enablement, or changes to tunnel behavior.
- Hub-side durable Enroller state across Hub process restart; this slice relies on the existing in-memory Enroller and records that limitation.
- Automatic certificate renewal/revocation, multi-generation cleanup policy, production readiness, two-host LAB acceptance, merge, release, or deployment.

## Runtime and storage design

- Add a focused `fleet_member` module and a `FleetCommand::Join` CLI variant. Normal Join accepts `--instance NAME` and reads exactly one encoded offer line from stdin. Resume accepts `--instance NAME --resume` and uses the protected pending state when present; if no pending state remains, it validates the installed Member config and queues an Agent restart.
- The command requires effective UID 0 because it writes the root-managed Fleet config and member credential store. It does not invoke SSH or alter host networking. Instance names are validated with the existing `apply::valid_instance` guard.
- Use a per-instance Member root under `/etc/baft-tunnel/fleet-member/NAME`. A single bounded pending transaction stores the encoded offer, CSR and private key with strict root ownership and mode `0600` (directory `0700`). No second, different pending offer may overwrite it.
- After receiving a successful Join response, validate all PEM inputs and the member key/certificate match. Write the member certificate, CA, CRL and authenticated Hub certificate into a same-filesystem staging generation; use `create_new`, explicit modes, file `sync_all`, directory `sync_all`, and atomic rename. The private key is `root:<Agent group>` mode `0640`; public certificates are `root:root` mode `0644`; directories are `root:<Agent group>` mode `0750` after publication. Refuse symlinks, non-regular files, pre-existing conflicting paths, and unsafe ownership/modes.
- Update the per-instance Fleet JSON through a same-directory temporary file, preserving an existing Hub section and rejecting malformed/unknown configuration. The new `member` paths point only to the immutable generation. Sync the temporary file and parent directory around the atomic rename.
- After durable config publication, send `JoinAck { ok: true }`. Then queue `baft-tunnel-agent@NAME.service` restart through the existing Fleet control helper. Remove the pending offer and CSR only after restart queuing succeeds. If any step before that fails, keep enough root-only state to retry the exact request; never create a new key/CSR for the same pending offer.
- `--resume` uses the stored offer and CSR to retry if they remain. If the pending secrets have already been removed, it verifies the installed Member paths and only queues restart. No offer, token, nonce, CSR or private key is printed or logged.
- Hub delivery becomes idempotent for an exact same offer ID, node ID, token, nonce and CSR digest while the offer is `AwaitingReport`. The existing issued leaf and delivered fingerprint remain unchanged; repeated ACK is a no-op success. Denied, expired, closed, mismatched, or changed-CSR attempts remain refused.
- The existing `fleet_run::start` consumes the installed MemberSection and starts `fleet_report::run`. No separate report transport or Member listener is added. CLI output distinguishes `restart_queued / report_pending` from `Enrolled`; it never claims enrollment itself.

## Recovery and failure semantics

- Before a successful positive ACK, any failed or interrupted attempt remains retryable with the same stored offer, key and CSR.
- A failed certificate/config write never publishes a partially written generation and never sends a positive ACK.
- If ACK may have reached the Hub but the client did not finish, exact retry in `AwaitingReport` re-delivers the same certificate and is idempotent.
- If restart queuing fails after ACK, retain pending state; `--resume` retries the exact Join/ACK and restart. If pending state has already been cleaned, `--resume` only restarts from the installed config.
- If the Hub process restarts, its in-memory Enroller state is lost; no claim of durable enrollment recovery across Hub restart is made in this slice.

## Acceptance

- CLI accepts offer only from stdin; secrets never appear in argv, stdout, stderr, audit or logs.
- Generated CSR has exactly one matching challengePassword and a P-256 key; the private key never enters JoinRequest or JoinResponse.
- Wrong Hub pin/identity, malformed response, invalid CA/CRL, wrong member cert/key, wrong node identity/profile, expired offer, or unsafe paths fail before positive ACK.
- File modes/ownership, symlink refusal, atomic publication, crash/retry state and preservation of existing Hub config are covered by tests.
- An exact retry after `AwaitingReport` returns the same issued public material and does not alter the delivered certificate fingerprint or state; changed CSR/credentials and denied/expired offers remain refused.
- Agent restart is queued only after durable persistence and positive ACK; Member reporting starts from existing runtime wiring and the first report is attempted immediately.
- Exact-SHA Linux CI must pass. No real-host LAB or two-host enrollment claim is implied.
