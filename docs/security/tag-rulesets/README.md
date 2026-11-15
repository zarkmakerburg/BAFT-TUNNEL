# Release tag rulesets (audit A-13)

These two repository rulesets protect release tags. Both were applied on 2026-11-15:

| Ruleset id | Name | Effect |
|---|---|---|
| 27302115 | `v-tags-immutable` | No `v*` tag can be deleted, updated or force-moved. It has no bypass, so this includes rc tags and admins. |
| 27302116 | `v-stable-tags-admin-create` | Only the Admin repository role can create a stable `v*` tag. `v*-rc.*` and `v0.0.0-dryrun.*` are excluded, so `tag-prerelease.yml` can still create rc tags with the workflow token. |

**Side effect:** a `v*` tag, rc included, can never be moved or deleted. A wrong tag needs a new version number.

## Apply (repository admin)

    gh api -X POST repos/zarkmakerburg/BAFT-TUNNEL/rulesets --input v-tags-immutable.json
    gh api -X POST repos/zarkmakerburg/BAFT-TUNNEL/rulesets --input v-stable-tags-admin-create.json

You can also use the web UI: Settings > Rules > Rulesets > New tag ruleset.

## Check

    gh api repos/zarkmakerburg/BAFT-TUNNEL/rulesets/27302115
    gh api repos/zarkmakerburg/BAFT-TUNNEL/rulesets/27302116

Each read-back must match the JSON in this folder: the target, the include and exclude patterns, the rule types and the bypass actors.
