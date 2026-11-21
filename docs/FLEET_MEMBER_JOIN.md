# Fleet Member Join operator guide

This guide covers the Member-side activation command. Fleet Join is Preview: a successful local command does not by itself mean the Hub has enrolled the node.

## Requirements

- Run the command as root (`sudo` or a root shell). It writes root-managed credentials and `/etc/baft-tunnel/fleet/NAME.json`.
- Use an instance name accepted by the existing `baft-tunnel@NAME` service naming rules.
- Obtain one valid, unexpired, encoded JoinOffer from the Hub operator. The offer is a one-time secret; do not put it in command arguments, shell history, tickets, logs, or chat.
- The Hub must be reachable at the address and TLS identity pinned by the offer. The command does not configure firewalls, routes, sysctls, or network interfaces.

## Start a Member Join

Pass the encoded offer as exactly one line on standard input. For example, when the offer is held in a shell variable by a trusted local workflow:

```sh
printf '%s\n' "$JOIN_OFFER" | sudo baft-tunnel fleet join --instance edge
```

Alternatively, run `sudo baft-tunnel fleet join --instance edge` and provide the single offer line through stdin, then close stdin. The CLI accepts no offer positional argument and does not accept the token-bearing offer in argv. Input is bounded to 16 KiB and must contain exactly one non-empty UTF-8 line.

The command requires effective UID 0 and validates the instance name before reading the offer. It generates the Member P-256 key and challenge-bound CSR locally, reuses the exact pending transaction on retry, verifies the pinned Hub TLS identity and returned credential material, durably publishes the Member generation and config, sends the positive Join ACK, then queues the existing per-instance Agent restart.

## Resume or retry

```sh
sudo baft-tunnel fleet join --instance edge --resume
```

- If a pending offer exists, `--resume` retries the same transaction using the stored offer, key and exact CSR; it does not generate a replacement CSR.
- If Join/ACK succeeds but queuing the restart request fails, the pending offer, CSR and private key are retained. Correct the local helper/request blockage, then run `--resume` again.
- If the one-time pending state has already been cleaned and a valid Member config is installed, `--resume` validates that config and queues only the Agent restart. It does not need a new offer.
- A different offer cannot overwrite an existing committed pending transaction. Resolve or resume that transaction first.

## Credential and config locations

Per-instance pending state is stored under:

```text
/etc/baft-tunnel/fleet-member/NAME/pending/
  offer.json       root-only, mode 0600
  request.csr      root-only, mode 0600
  member.key       root-only, mode 0600
```

After validation, an immutable generation is published under:

```text
/etc/baft-tunnel/fleet-member/NAME/generations/SHA256/
```

Published generation directories are root-owned and Agent-group readable/traversable (mode 0750). The installed Member private key is `root:<Agent group>` mode 0640; public certificates and CRL are `root:root` mode 0644. The per-instance Fleet config is `/etc/baft-tunnel/fleet/NAME.json` (root-managed, mode 0640). Existing Hub configuration is preserved when the Member section is added.

The one-time offer, CSR and pending private-key copy are removed only after the positive ACK and restart request have both succeeded. The installed generation retains the private key and public trust material required by the Agent reporter.

## Interpreting the result

A successful command prints:

```text
state=restart_queued report_pending=true
```

This means the per-instance Agent restart request was queued. It does **not** mean the first report reached the Hub or that the node is `Enrolled`. The existing Agent starts the outbound mandatory-mTLS Member reporter after restart. Only the Hub accepting the first report from the exact issued certificate transitions the offer to `Enrolled`.

The CLI never prints `Enrolled`. A timeout, validation error, or local queue error is not proof of enrollment; retain the error details without copying the offer or credential material into logs or support messages.

## Scope and limitations

This flow uses the existing outbound TLS Join client and Agent/reporting lifecycle. It adds no inbound Member listener, SSH enrollment, panel API, firewall or route changes, `CAP_NET_ADMIN`, or Reverse enablement. Hub Enroller state remains in-memory in this slice; Hub process restart durability is not claimed. No two-host LAB acceptance or production-readiness claim is implied.
