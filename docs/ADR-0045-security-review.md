# ADR-0045 security review (Fleet F0)

Independent design review, 2026-11-17; not a pre-stable external review.

Scope: `docs/ADR-0045-fleet-control-plane.md` as written. No code exists, and none was reviewed. Binding constraints were taken as given: Rust core, userspace only, no `CAP_NET_ADMIN`, offline CA, no TOFU, no arbitrary remote command.

## Findings

**F-1 High: what a compromised hub can do.** The hub-local job-signing key (0600 file, open question 2) can sign any allow-listed job for every member. An attacker holding it can stop every tunnel in the fleet or keep restarting them. Once `apply` exists, it can also rewrite tunnel configuration, which may include the peer and route. The ADR does not say how members come to trust this key, and the only revocation is re-pairing every node by hand.
*Amendment:* "The hub job-signing key is certified by the offline CA under a distinct `fleet-hub-signer` profile, is pinned by name in each member's pairing record, and is revocable by CRL. Members rate-limit jobs per instance. `apply` is not hub-signable and needs either an offline-signed payload or confirmation in the member's local panel."

**F-2 High: hub server identity is under-specified.** The bundle carries only a "hub CA pin". Any certificate from the same CA could then pass as the hub, including any node's tunnel certificate or `fleet-node` certificate.
*Amendment:* "The hub presents a `fleet-hub` certificate (serverAuth, its own private EKU OID). The member checks the EKU, the hub name taken from the pairing bundle, and the CRL. A CA pin alone is never enough."

**F-3 High: cross-node and cross-purpose job replay.** A job names a "target instance", but instance names are not unique across nodes. A job captured from node A could therefore be valid on node B. Replay state is also not said to survive a restart.
*Amendment:* "Each job binds `target_node_id` (taken from the member's certificate), the instance, the job type and `not_after`, all inside the signature. `not_after - issued ≤ 10 min` and allowed clock skew is ≤ 2 min. The member keeps seen `job_id`s on disk until `not_after + skew` and rejects any job whose `not_after` is beyond that window."

**F-4 Medium: EKU confusion.** "EKU distinct" names no OIDs and gives no rule for a certificate that carries both EKUs.
*Amendment:* "Each profile has one private EKU OID. A certificate carrying more than one BAFT EKU, or `anyExtendedKeyUsage`, is rejected on every channel. The tunnel handshake rejects the `fleet-node` and `fleet-hub` OIDs, and tests cover all of these cross-profile cases."

**F-5 Medium: node identity in reports.** A member could put another node's id in a report body and poison the inventory.
*Amendment:* "The hub takes node id and role only from the authenticated `fleet-node` certificate and rejects any report whose body id differs."

**F-6 Medium: `seq` lifecycle.** The ADR does not say what happens to `seq` after a reinstall or a state wipe. Without that, the node is either locked out for good or allowed to roll back.
*Amendment:* "`seq` is scoped to (certificate serial, node epoch). A new epoch is accepted only with a new certificate serial, and the hub logs it."

**F-7 Medium: stolen or swapped enrollment bundle.** The bundle authenticates nothing by itself. The real risk is an operator signing an attacker's CSR, or a member importing a bundle that points at an attacker's hub.
*Amendment:* "Each CSR embeds the bundle nonce and the node id. The operator compares a CSR fingerprint shown on the member's local panel before signing. The hub burns the nonce on first use and refuses any bundle past its expiry (≤ 24 h)."

**F-8 Medium: no CRL freshness rule.** The ADR does not say how the offline CRL reaches members, or what happens when it is past `nextUpdate`.
*Amendment:* "The hub and every member load the CRL from an operator-installed file. When the CRL is past `nextUpdate`, jobs fail closed and reports are accepted but flagged 'revocation unknown'. Revoking a `fleet-node` certificate never revokes the node's tunnel certificate, and the reverse also holds."

**F-9 Medium: DoS bounds.** The ADR states caps but gives no numbers, and it does not mention that the hub now exposes a new inbound port.
*Amendment:* "Concrete limits are fixed in F1: report ≤ 64 KiB, events ≤ 256 per report, ≤ 1 connection per node, a minimum report interval, a maximum node count, pre-auth handshake rate limits, and a bounded hub memory and disk budget. These are tested at their limits. The hub listener is opt-in and off by default."

**F-10 Medium: inventory reveals the fleet layout.** A compromised hub, or its at-rest store, gives away every node's role, peer and engine. In this threat model that is a list of targets for censors.
*Amendment:* "Reports carry no peer addresses, SNI or endpoints by default. The hub stores only the fields defined in this ADR and states its retention limits."

**F-11 Medium: a job can cut its own management path.** `tunnel.stop` or `tunnel.restart` can sever the path the member uses to reach the hub.
*Amendment:* "A job that could cut the member→hub path runs with an automatic revert unless a receipt from the member is acknowledged within N minutes. The local panel remains the way back."

**F-12 Medium: hub audit can be altered.** A compromised hub can rewrite its own audit, and the operator identity in a job is asserted only by the hub.
*Amendment:* "Members sign receipts with their `fleet-node` key and keep a hash-chained local audit. The hub stores those receipts unaltered, so the members' own records are what show a compromised hub."

**F-13 Low: panel injection.** Display name, instance names and event text come from members. They can carry markup or Unicode bidi overrides, which is a particular risk with the RTL UI.
*Amendment:* "All member-supplied strings are escaped, length-capped, and stripped of bidi and control characters before display."

**F-14 Info: same CA (open question 1).** Reusing the CA is acceptable once F-2 and F-4 are adopted. An offline sub-CA for Fleet, restricted by EKU, is an optional way to reduce the blast radius later.

**F-15 Info: one-way reachability (open question 4).** Keeping connections member-initiated only is correct. Document that when an abroad member cannot reach an Iran-side hub, it shows as `Never` or `Stale`, with no inbound fallback. In that case, put the hub abroad.

**F-16 Info: hub as a single point of failure.** This is acceptable as specified, because the data plane does not depend on the hub and the local panel is the break-glass path. F5 must test this.

## Verdict

**Accept with amendments.** F-1, F-2 and F-3 are High and block F0 acceptance until their amendment sentences are in the ADR. F-4 to F-12 must be in the ADR before F1 or F3 starts, whichever the finding applies to. No Critical findings.
