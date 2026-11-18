# ADR-0053: manage every enrolled node from one panel, with version and remote update

Status: **Proposed** (2026-11-18). Security-sensitive: remote update changes code on another host. It must not be enabled before the independent review this ADR requests. Owner requirement (zark): when connected to one node's panel the user can manage all enrolled nodes from there, see each node's version, see whether an update exists and trigger it (BAFT-style). Builds on ADR-0045 (Fleet, F3 typed jobs), ADR-0051 (add server), ADR-0025 (update with automatic rollback), ADR-0035 (update notice), ADR-0034 (offline signature verification), ADR-0023 (root control helper), ADR-0030 (no `CAP_NET_ADMIN`).

## Decision

### 1. One panel, many nodes
The node whose panel the user opened is the **hub** (ADR-0045: a role of an existing agent). Enrolled nodes (ADR-0051) report to it over the outbound `fleet-node` channel. The Fleet page lists every node with:

| Column | Source | Unknown when |
|---|---|---|
| Name, role (Iran / abroad), address | enrollment record | never (operator-entered) |
| Version | `node.hello.v1` / `node.status.v1` (`version` field, sanitised) | node never reported or report is Stale |
| Update state | hub compares the reported version with the hub's own verified release notice (ADR-0035, stable channel) | version unknown, or no notice file on the hub |
| Tunnels, Integrity, Health | `node.status.v1` tiers, kept separate | Stale or Never |
| Last seen, freshness | derived on read (Live / Stale / Never) | Never |

A node that is Stale or Never shows every measured field as Unknown, never its last value as current (ADR-0045). Clicking a node opens that node's per-node view inside the hub panel; actions go through typed jobs, never through a proxied session to the node's own panel.

### 2. Update availability
The agent stays offline (ADR-0035). The hub's `update-check.json` is the only source of "latest"; a node that reports a lower version than that tag is `update_available`, equal is `current`, a version that does not parse is `unknown_installed`. Nodes are never told "latest" by anything but the hub's verified file, and only `vMAJOR.MINOR.PATCH` stable tags are offered by default; a pre-release needs an explicit Advanced switch and an extra confirmation.

### 3. Remote update as a typed job (F3, default-off Preview)
`node.update` is one job in the ADR-0045 F3 vocabulary. It is not a shell, takes no path, URL or command, and carries only: `job_id`, `idempotency_key`, `not_after`, target node id, `from_version`, `to_tag`, and the SHA-256 of the release bundle manifest. Rules:

1. **Signed and bound.** The hub signs the job with its `fleet-hub-signer` certificate (ADR-0045 F-1, F-3). The node rejects an uncertified or revoked signer, a wrong target id, an expired or replayed job, and any `to_tag` that is not a strictly higher stable `vMAJOR.MINOR.PATCH` than the running version (no downgrade through this job; rollback is a separate job, rule 6).
2. **No internet needed on the node.** The release bundle and its signatures are obtained by an operator-run script on the hub host (or placed there by the operator), never by the agent process (amendment M4), and served to the node over the authenticated mutual channel. The node re-verifies the signature offline and the manifest hash from the job before anything runs. A bundle that fails either check is refused and nothing is changed.
3. **Executor is the existing guarded update.** The node's root helper (ADR-0023 pattern, a separate short-lived unit) runs `update.sh` (ADR-0025): data backup, install keeping the previous binary, restart of the instances that were active, health check, and automatic rollback with re-verification. The helper accepts only this fixed sequence; the agent process never gets root.
4. **Self-lockout guard (ADR-0045 F-11).** After the restart the node must reach the hub again within a timeout (default 5 minutes); if it cannot, the helper rolls back and the receipt says so.
5. **Receipt.** The node returns a signed receipt (`updated`, `rejected_and_rolled_back`, `rollback_unhealthy`, with versions before and after and the config digest result from ADR-0025). The hub shows exactly that state. If the node goes silent mid-update the hub shows Unknown with manual-cleanup text, never "updated".
6. **Rollback job.** `node.rollback` restores the previous binary through `rollback.sh` (ADR-0025), only if the helper still holds the previous binary; otherwise it is not offered.

### 4. Confirmation, order and blast radius
- Every update is confirmed in the panel by the operator for that node (name, from version, to tag, "this restarts the tunnel and briefly interrupts traffic"), with the panel token re-entered; no bulk "update all" in Preview. One update at a time, one active job per node (ADR-0045 rate limits).
- Recommended order shown in the panel: members first, the hub last, so the hub can still see the result. The hub never updates itself through this job (it uses the local operator path, ADR-0035).
- The data plane never depends on the hub. A hub outage leaves tunnels running and only blocks fleet actions (ADR-0045 F5); the local panel stays the break-glass path.

### 5. Audit and exposure
Hub audit: operator, action, node, job id, from and to versions, decision, result, time. Node audit: job id, verified-by, result, hash-chained and signed (ADR-0045 F-12). No secrets in either. Fleet inventory is visible only to the authenticated operator (F-10) and node text is escaped and stripped of bidi controls (F-13).

## What this ADR does not allow
No remote shell or arbitrary command, no file upload other than the verified release bundle over the Fleet channel, no firewall, route or sysctl change, no `CAP_NET_ADMIN`, no unattended or scheduled remote updates, no update of a node that is Stale, Never or running a version that cannot report job support.

## Staged plan
1. This ADR and an independent security review (Opus). Findings that are High block F3 `node.update`.
2. Fleet F1 (read-only): transport, `fleet-node` identity, inventory with version and Live/Stale/Never; the update column is informational. Needs ADR-0045 amendments F-2, F-4 to F-9, F-13 tested.
3. Fleet F3 jobs (start/stop first), then `node.update` and `node.rollback` behind a default-off setting, after the review is green.
4. Container tests: replayed, wrong-target, expired, downgrade and bad-signature jobs rejected; failed health check rolls back; node silent after update shows Unknown.
5. Two-host LAB in a coordinator-approved window: update a LAB node from the other, including an injected failure that must roll back.

Nothing here is implemented, tested or LAB-verified.

## Amendments from the independent review (binding before `node.update` ships)

Review: PR #371 comment 6977488794 (no Critical, 3 High, 5 Medium, 3 Low). These supersede any looser wording above.

**High**
- **H1 bundle bound to tag and signer.** The node verifies the Sigstore certificate identity exactly: the expected OIDC issuer, a SAN equal to this repository's `release.yml` at `refs/tags/<to_tag>`, and the Rekor inclusion proof offline with the pinned root (ADR-0034); any other signer is refused. The signed manifest carries the tag and version. The node requires manifest tag == `to_tag`, manifest hash == the job's hash, and installed binary sha == the manifest entry. After install, `--version` must equal `to_tag`, otherwise the update rolls back. This also stops a compromised hub from serving an older genuinely signed release under a higher tag.
- **H2 root boundary.** The root helper copies the bundle into a root-owned 0700 staging directory and re-verifies on that copy: job signature, target, `not_after`, replay cache and H1. It executes only the binary from that copy and runs the already installed `update.sh`, never a script from the bundle. Nothing is extracted before verification; extraction, if needed, allows regular files only (no links, devices or `..`) within a size cap. The agent process cannot choose a path to run.
- **H3 commit protocol.** An update commits only when both hold: (a) the ADR-0025 health check passes, and (b) a hub-signed `update.ack` bound to the `job_id` reaches the **new** binary over a fresh `fleet-node` session. A persistent pending-update marker is written before the restart; on boot an uncommitted marker means roll back. Commit is an atomic marker swap. A lockfile keeps local `update.sh` or `restore.sh` and a remote job from running at the same time. A hub that is down for the whole window therefore ends in rollback, shown as such.

**Medium**
- **M1 rollback is not a free downgrade.** `node.rollback` is allowed only to the exact previous binary sha recorded by that update, within a bounded window (24 h) or while the update is uncommitted, with per-node rate limits.
- **M2 transfer.** `not_after` gates acceptance. An accepted job is persisted; the bundle transfer has its own deadline, is chunked and resumable against the manifest hash, uses a separate stream outside the 64 KiB report limit, and a hard size cap plus a disk budget are checked before anything is written (the throttled Iran path is about 21 KB/s).
- **M3 confirmation.** The server issues a single-use confirmation nonce (TTL at most 2 minutes) bound to node id, `from_version`, `to_tag` and manifest hash. The confirm request needs the nonce plus Origin and CSRF checks, and the job is built from the server-side record, not from browser fields. Re-entering the bearer token alone is not confirmation.
- **M4 no outbound call from the agent.** Bundle fetching is an operator-run script (or an operator-placed file) with the URL derived only from the fixed repository and the validated tag; the agent never takes a URL from `update-check.json` `source`.
- **M5 node-side policy.** The node rejects a job whose `from_version` differs from its running version, enforces at most one update per N hours (default 24), and accepts a pre-release only if its own local configuration opts in.

**Low.** `update-check.json` is unsigned and agent-writable, so it only drives display and H1 is the real gate (L1). The receipt includes the manifest hash, installed binary sha, verified signer identity and rollback reason, and states which key signs it (the node's `fleet-node` key; the helper's rollback receipt is relayed under that signature) (L2). The hub signer key is a 0600 file, so a hub compromise can update nodes within signed releases; the node-side limits in M5 bound that, and offline signing remains a later option (L3).

**Gates.** `node.update` ships only default-off Preview after stage 4 implements H1 to H3 with container tests for: a mislabelled older signed bundle, a foreign Sigstore identity, an agent-side path swap, a reboot in the middle of the window and the hub down during the window. M1 to M5 are required before the two-host LAB run. F-1 and F-3 (job signing and binding) stay prerequisites.
