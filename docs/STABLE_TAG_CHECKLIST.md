# Stable tag checklist (v0.2.2 and v0.3.0)

Purpose: when the owner is on an admin machine, the tag step is one approval and a short command list. Nothing here is done yet. No Stable tag exists for 0.2.2 or 0.3.0.

Rules that apply to both: the Stable tag goes on exactly the commit of the last signed rc; the tag push needs the owner's own-word approval in the tag device thread; `release.yml` signs only from a `v*` tag and checks tag == Cargo.toml version; the tag commit must be contained in `main`.

## v0.2.2

Status: ready except for the owner's approval and an admin machine.

| Check | Value |
|---|---|
| Commit to tag | `5dd0890cac9deddcac66c3105d3fbaf9164e391e` (same as `v0.2.2-rc.3`) |
| Cargo version at that commit | 0.2.2 |
| Binary SHA-256 (baft-tunnel) | `5f4ef12063c9d8a7a55ce993380c84d42ec648ad8121d3c4dd02cea43ac54247` |
| Evidence | `docs/STABLE_0.2.2_EVIDENCE.md` (real two-host LAB; not production) |

Steps:

1. Owner writes the approval in the tag device thread, naming the action and target: push tag `v0.2.2` on `5dd0890`.
2. On the admin machine: `git fetch origin main && git merge-base --is-ancestor 5dd0890cac9deddcac66c3105d3fbaf9164e391e origin/main`, then `git tag v0.2.2 5dd0890cac9deddcac66c3105d3fbaf9164e391e && git push origin v0.2.2`.
3. Wait for `release.yml` to finish. Verify: eight assets, checksums OK, binary SHA-256 equals the rc.3 value above, release is `latest` and not a prerelease.
4. Bump the README one-line install pin from `v0.2.1` to `v0.2.2` in a separate PR. Merge it only after step 3.
5. Update the roadmap snapshot line and HQ Issue #2.

## v0.3.0

Status: blocked. Do not tag.

Blocker: the Direct long-lived-session stall (aged flows on the Iran path, see `docs/STABLE_0.3.0_EVIDENCE.md` Known items). Stable waits for it to be fixed or its cause understood and accepted by the coordinator.

Before tagging:

1. The stall is fixed (for example the session rotation in PR #315 with LAB before/after evidence) or accepted as an external path limit.
2. If any code changes, cut a new signed rc from main with `tag-prerelease.yml` (`confirm=TAG_SIGNED_PRERELEASE`); rust, ubuntu-2204-baseline and lab-config must be green on that sha.
3. An upgrade soak (previous Stable to the final rc) passes on that rc, including the first 2 MB probe after the upgrade.
4. `docs/STABLE_0.3.0_EVIDENCE.md` is updated for the final rc: commit, binary SHA-256, run links, known items.
5. Cargo version is 0.3.0 at the rc commit.

Then, with the owner's written approval in the tag device thread for `v0.3.0` on the final rc commit:

1. Push the tag from the admin machine as for 0.2.2.
2. Verify assets, checksums, binary SHA-256 equals the final rc, `latest`, not a prerelease.
3. Put the evidence file into the release body (owner approval in the tag thread if the body edit needs it).
4. Bump the README pin to `v0.3.0` in a separate PR, merged only after the tag exists.
5. Update the roadmap and HQ Issue #2.

Order: tag 0.2.2 first, then 0.3.0 once unblocked. They are independent commits; the 0.3.0 upgrade soak should start from the released 0.2.2.
