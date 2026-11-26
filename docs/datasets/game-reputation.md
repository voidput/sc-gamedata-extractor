---
title: game-reputation (game-reputation.json)
tags: [docs, datasets]
updated: 2026-11-25
---

# game-reputation (game-reputation.json)

Every reputation rank (`SReputationStandingParams` record): its display name, the reputation threshold that reaches it, drift back toward neutral, and the `gated` flag, plus the ranks grouped by an organisation name taken from the record name. On LIVE 4.10.201.48085 it holds 380 standings in 239 groups. The file is 196,523 bytes minified.

## Source

- DataCore: `SReputationStandingParams` records under `**/reputation/**` (filter, `crates/sc-gamedata/src/domains/reputation.rs:104`). On disk they are the 380 files in `libs/foundry/records/reputation/standings`.
- String table for `displayName`.
- Code: `organisation_of` (reputation.rs:51), `parse_standing` (reputation.rs:77), `Domain::parse` (reputation.rs:112).
- Derives from no other dataset.

## Shape

Top-level keys: `_source`, `_extracted`, `_build`, `standings[]`, `byOrganisation{}`, `summary{}`. `_schema: "game-reputation@1"` follows `_build` (since tags 4.10.201.48085 / 5.0.202.35026).

`summary`: `standings` 380, `organisations` 239.

`byOrganisation`: `{ <organisation>: Standing[] }`, each list sorted by `minReputation` ascending. Keys come from `organisation_of`. 229 of the 239 groups hold a single rank (see Parse gaps). The biggest are `Guild` (51), `Negative` (20), `Affinity_Ally` (16) and `Faction_B0` to `Faction_B4` (11 each).

### `standings[]` (also the elements of each `byOrganisation` list)

| Field | Type | Meaning | Units/range | Source |
|---|---|---|---|---|
| `name` | string | Record short name (`ReputationStanding_Bounty_Agent`) | 380 | record |
| `recordName` | string | `SReputationStandingParams.<short>` | 380 | record |
| `id` | string | Record GUID | 380 | `_RecordId_` |
| `displayName` | string? | Localised rank name when real (`Liked`, `Neutral`) | 196 | `displayName` |
| `organisation` | string | Record short name minus a leading `ReputationStanding…` part and a trailing all-digit part | 239 distinct | derived from the record name |
| `minReputation` | number | Threshold to reach the rank (module doc, reputation.rs:4) | -320,000 to 1,600,000 | `minReputation` |
| `driftReputation` | number? | Drift amount; zero dropped. Unit and direction not stated beyond "decay back toward neutral" (reputation.rs:6) | on 40 | `driftReputation` |
| `driftTimeHours` | number? | Drift interval; zero dropped | 1 or 24 hours; on 114 | `driftTimeHours` |
| `gated` | true? | Present only when true. What it gates is not established | 47 | `gated` |

Example (real):

```json
{"name":"ReputationStanding_Assassination_NotEligible",
 "recordName":"SReputationStandingParams.ReputationStanding_Assassination_NotEligible",
 "id":"0dac9de8-2f7a-4d65-afd3-63d3fed2d6f2","displayName":"Not Eligible",
 "organisation":"Assassination_NotEligible","minReputation":-1000,
 "driftReputation":250,"driftTimeHours":1}
```

## Derived, estimated and curated values

- `organisation` and `byOrganisation` are a naming heuristic, not a record field: "Ranks of one organisation share a record-name prefix" (reputation.rs:5). The records never state the grouping. The ladders the game uses are the scope records (see Derivable data).
- No `Estimate` fields and no `_assumptions`.

## Redaction

Nothing (`game-reputation.json` has no `PROSE_FIELDS` entry).

## Consumers

- **starorg-codex** (`feat/dataset-reuse`) `crates/starorg-mcp/src/prebuilt.rs:177`: needs `standings`, `byOrganisation`, `summary`; strips nothing.
- **SCMCP** `src/versions.ts:40`: diffs `standings` between builds, keyed by `recordName`.
- snares, MCsim, Bindscape and sc-telemetry do not read it.
- Contracts: `_build` equals the tag; `_schema` stem `game-reputation` once added ([consumer-datasets](../plans/specs/consumer-datasets.md)).

## Derivable data

Records the game ships under `libs/foundry/records/reputation` (530 files) that the domain does not read, although its filter already matches them:

- **Scopes** (`scopes/`, 46 `SReputationScopeParams` records): `scopeName`, `displayName`, and `standingMap{reputationCeiling, initialReputation, standings[]}`, the ordered rank ladder. For example `ReputationScope_Bounty_BountyHuntersGuild` lists 8 standings from `noteligible` to `masteragent` with a ceiling of 480,001, `ReputationScope_Wikelo` lists 3 with a ceiling of 1000, and `ReputationScope_Affinity` lists 41 with a ceiling of 10,000. Together the scopes reference 355 of the 380 standings. Emitting `scopes[]{name, displayName, ceiling, initial, standings[recordName]}` would replace the record-name grouping with the game's own ladders. Consumer: starorg-codex.
- **Reward amounts** (`rewards/missionrewards_reputation`, 58 `SReputationRewardAmount` records: 35 plus 23 in `reputationawards_pyro`): `editorName` and `reputationAmount` (`ReputationRewardAmount_Positive_S` = 1000, `ReputationRewardAmount_Negative_XL` = -8000, `Wikelo_25` = 25). These are the values missions and Wikelo contracts award.
- **Giver pay bonuses** (`rewards/missionrewards_bonusuec`): `missionGiverBonuses[]{factionReputation, reputationScope, rewardBonuses[]{standing, bonusFraction}}`. For example the bounty scope pays +0.05 at `bounty_agent` up to +0.2 at `bounty_masteragent`, for Crusader Industries, Hurston Dynamics, Northrock, ArcCorp and microTech.
- **Standing fields not read**: `name` (an internal label, e.g. `BountyHunter_MidLevel`), `perkDescription` (a string key, e.g. `@RepStanding_Perk_Bonus_05`), `icon`.
- Also there: `contexts/` (27), `perks/` (2, including `perkreputationlist_wikelo`), `states/` (12), `sandboxtriggers/` (3), `globalmissionresultmodparams.json`. Their meaning is not established here.

Values computable by combining datasets:

- Reputation per mission and outcome: [game-missions](game-missions.md) records reference `reputationScope` + `reward` records (see that note). Joined with the reward amounts, this gives rep gained per contract; joined with the scope ladders, contracts per rank-up. Consumer: starorg-codex.
- Rank required per mission, from `reputationRequirements` in the mission records (standing references), joined to `standings[].recordName`.
- Wikelo rep per trade from `ContractResult_LegacyReputation` in the collector generator (61 entries, reward `wikelo_25` …), joined with [game-wikelo-trades](game-wikelo-trades.md).

Parse gaps:

- `organisation_of` strips only an all-digit tail, so the 20 `Affinity_Enemy_-005` … `-100` ranks become 20 one-rank groups (the `-` is not a digit), while `Affinity_Ally_025` … group fine.
- Ladders whose ranks are named, not numbered (`Bounty_Agent`, `Bounty_Junior`, `Assassination_Captain` …), become one group per rank: 229 groups hold one standing.
- The faction records that scopes and missions point at (`libs/foundry/records/factions/factionreputation/*.json`) are outside the filter, and absent from the extracted record tree, so faction names cannot be joined yet.
- 25 standings are in no scope ladder. Whether they are unused is not established.

Related specs: [dump-reuse](../plans/specs/dump-reuse.md), [consumer-datasets](../plans/specs/consumer-datasets.md).
