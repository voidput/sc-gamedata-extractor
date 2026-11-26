---
title: game-missions (game-missions.json)
tags: [docs, datasets]
updated: 2026-11-25
---

# game-missions (game-missions.json)

Every `MissionBrokerEntry` record, the contracts the mobiGlas contract manager offers: title, giver, type, difficulty, payout, buy-in and the lawful flag, with string-table keys. Developer and test entries are kept and flagged `notForRelease`. On LIVE 4.10.201.48085 it holds 2584 missions (1978 live, 606 `notForRelease`). The extract is 2,194,410 bytes minified; with `description` stripped as publish does, about 1,198,030 bytes.

## Source

- DataCore: `MissionBrokerEntry` records under `**/missionbroker/**` (filter, `crates/sc-gamedata/src/domains/missions.rs:143`).
- String table for `title`, `missionGiver` and `description`.
- Code: `parse_mission` (missions.rs:101), `Domain::parse` (missions.rs:151).
- Derives from no other dataset.

## Shape

Top-level keys: `_source`, `_extracted`, `_build`, `missions[]`, `byGiver{}`, `summary{}`. `_schema: "game-missions@1"` follows `_build` (since tags 4.10.201.48085 / 5.0.202.35026).

`summary` (LIVE): `missions` 2584, `live` 1978, `notForRelease` 606, `withPayout` 1858 (live with `reward` > 0), `medianReward` 17500, `maxReward` 500000 (both over live paying missions, all currencies mixed).

`byGiver`: live missions per resolved `giver` text, largest first; 70 keys (Covalex Independent Contractors 843, Ruto 92, `~mission(Contractor)` 77, Rough & Ready 74 …). 27 of the 70 keys are unresolved `~mission(...)` tokens.

### `missions[]`

| Field | Type | Meaning | Units/range | Source |
|---|---|---|---|---|
| `name` | string | Real title, else the record short name | 2584 | `title`, record name |
| `recordName` | string | `MissionBrokerEntry.<short>` | 2584 | record |
| `id` | string | Record GUID | 2584 | `_RecordId_` |
| `title` | string? | Localised title when real; can hold `~mission(...)` tokens | 2387; longest 75 chars | `title` |
| `description` | string? | Localised briefing (stripped on publish) | 2347 | `description` |
| `giver` | string? | Localised giver | 2381 | `missionGiver` |
| `titleKey`, `descriptionKey`, `giverKey` | string? | String-table keys | 2502 / 2500 / 2446 | same fields |
| `nameStatus` | string? | Title status when not real: `unresolved` 103, `empty` 82, `placeholder` 12 | 197 | `title` |
| `type` | string | File stem of the `type` reference (`mercenary` 759, `hauling_planetary` 477, `missiontype.delivery` 342, `bountyhunter` 286 …; 20 values) | 2584 | `type` |
| `difficulty` | number? | `missionDifficulty`, kept when ≥ 0 (-1 means unset, missions.rs:122-123) | 1, 2, 3, 5; on 855 | `missionDifficulty` |
| `reward` | number? | `missionReward.reward`, zero dropped | 1 to 711,750; on 2345 | same |
| `rewardMax` | number? | `missionReward.max`, zero dropped. Relation to `reward` not established | on 68 | same |
| `currency` | string | `missionReward.currencyType` | `UEC` 2581, `MER` 2, `INVALID` 1 | same |
| `buyIn` | number? | `missionBuyInAmount`, zero dropped | on 100 | same |
| `notForRelease` | true? | Developer/test content | 606 | `notForRelease` |
| `lawful` | bool | `lawfulMission` | false on 449 | same |
| `tags` | string[]? | Names of `missionTags` | never present (see Parse gaps) | `missionTags.tags` |

Example (trimmed, real):

```json
{"name":"~mission(ReputationRank) Rank - ~mission(CargoGradeToken) Cargo Haul",
 "recordName":"MissionBrokerEntry.HaulCargo_Multi2ToSingle_Processed_AgriculturalSupplies_Stanton_SmallGrade",
 "id":"150bcfbe-8409-4f10-95ee-bac3a43ad128",
 "title":"~mission(ReputationRank) Rank - ~mission(CargoGradeToken) Cargo Haul",
 "giver":"Covalex Independent Contractors","titleKey":"Covalex_HaulCargo_MultiToSingle_title",
 "descriptionKey":"HaulCargo_MultiToSingle_desc","giverKey":"mission_contractor_0003",
 "type":"hauling_solar","difficulty":2,"reward":57000,"currency":"UEC","lawful":true}
```

## Derived, estimated and curated values

- `name` falls back to the record short name when the title is a placeholder.
- `summary.medianReward` takes the element at `len/2` of the sorted live rewards and mixes `UEC` with `MER`.
- No estimates and no curated sources. The payouts are the record values. Contract-system missions (`ContractGenerator` records under `contracts/contractgenerator`) are not in this dataset, and this domain reads no payout for them; how their pay is set is not established here.

## Redaction

`description` is removed everywhere and listed in `_redacted`. A `title` longer than 120 UTF-16 units is also removed, and `name` (when it copies the title) becomes the record short name, with `"title over 120 characters"` added to `_redacted` (`without_long_title`, localization.rs:483). On LIVE no title is over 120 characters, so `_redacted` is `["description"]`. In the published string table only 190 of 2502 `titleKey`s and 97 of 2446 `giverKey`s survive (lower-case `_title`/`_from` keys do not match the name-key rule), so the dataset's own `title`/`giver` are the published text.

## Consumers

- **starorg-codex** (`feat/dataset-reuse`): `crates/starorg-mcp/src/prebuilt.rs:172` needs `missions`, `byGiver`, `summary`, accepts `description` stripped, and refuses a file where a long title was stripped (it changes `name`). `crates/starorg-mcp/src/tools.rs:1294` (`mission`) searches `name`, `title`, `giver`, `recordName`, `type` and filters on `type`.
- **SCMCP** `src/versions.ts:39`: diffs `missions` between builds, keyed by `recordName`.
- snares, MCsim, Bindscape and sc-telemetry do not read it.
- Contracts: `_build` equals the tag; `_schema` stem `game-missions` once added ([consumer-datasets](../plans/specs/consumer-datasets.md)).

## Derivable data

Fields the 2584 records carry that the domain does not read (counted in `~/git/sc-gamedata/extracted-data/libs/foundry/records/missionbroker`; those files are dated 2026-11-12, an earlier extraction than the release data, though the count matches LIVE):

- `missionResultReputationRewards[]`: five lists per mission (one per outcome; which index is which outcome is not stated in the record). 1801 missions have at least one `SReputationAmountParams`, each naming a `factionReputation`, a `reputationScope` and a `reward` record such as `reputationrewardamount_positive_xxxs`. The `reward` records sit under `reputation/rewards/missionrewards_reputation` (58 `SReputationRewardAmount` records: 35 plus 23 in `reputationawards_pyro`) and carry `reputationAmount` (e.g. `ReputationRewardAmount_Positive_S` = 1000). This gives reputation gained per mission and scope. Consumer: starorg-codex, joined with [game-reputation](game-reputation.md).
- `reputationRequirements` (an object on 1571): expressions like `{factionReputation: arccorp, reputationScope: security, comparison: "GreaterThanOrEqualTo", standing: reputationstanding_security_rank2}`, so the rank needed per mission, joinable to `game-reputation.standings[].recordName`.
- `reputationPrerequisites.wantedLevel` (max 2 on 2178, 5 on 393): the crime-stat range allowed.
- `missionDeadline.missionCompletionTime` (0 on 1454, 300 on 706, 90 on 188 …), `maxInstances`, `maxPlayersPerInstance`, `canBeShared` (false on 218), `onceOnly` (true on 125), `hasPersonalCooldown` + `personalCooldownTime`, `respawnTime`, `instanceLifeTime`. Units are not stated in the records.
- `partialRewardPayout` (on 849): `percentageThresholds[]` such as 76-99% → `currencyRewardMultiplier` 0.76, 51-75% → 0.45.
- `missionReward.plusBonuses` (true on 228), and the bonus records under `reputation/rewards/missionrewards_bonusuec` (`bonusFraction` 0.05 to 0.2 per standing, per giver faction).
- `locationMissionAvailable` (a starmap reference on 1836) and `localityAvailable` (407): where a mission is offered, joinable to [game-starmap](game-starmap.md) by record path.
- `missionGiverRecord` (on 1459, e.g. `missiongiver_covalexindependentcontractors` 900): the giver as a record. It could resolve the 27 `~mission(Contractor…)` giver keys (planned in [dump-enhancements](../plans/specs/dump-enhancements.md) item 9).
- `requiredMissions[]` (non-empty on 425) and `associatedMissions[]`: mission chains.
- `missionModule` path (e.g. `PU_Missions/DeliveryMission` 340, `PU_Missions/SpaceBounty` 277): a finer mission kind than `type`.
- `properties[]` (28,141 `MissionProperty` entries): hauling and item properties. `autoSpawnSettings.inventoryItems` (3070 occurrences) and `haulingOrders[]` (858, e.g. `HaulingOrder_Resource` with `resource` `ResourceType.ProcyonCanister_Time`) are in these records; they are the source of the item links in dump-enhancements item 9. The `haulingOrderContent` field that item names is not in `missionbroker` records; it occurs in `contracts/contracttemplates` records.

Values computable by combining datasets:

- Reputation needed and gained per mission, by joining the fields above with `game-reputation`.
- Jurisdiction and system per mission, through `locationMissionAvailable` → `game-starmap` → `jurisdiction`/`system`.
- Pay per giver and type (median and max per `type`, `UEC` only), which `summary` mixes today.

Parse gaps:

- `tags` is never emitted: `tag_names` (missions.rs:86) reads `missionTags.tags`, but in the records `missionTags` is a plain array of `Tag` references (368 missions carry one, e.g. `Tag.6804b0a9-…` on 308). Reading the array and resolving the tags through the tag database would fill it.
- `byGiver` counts unresolved `~mission(...)` tokens as givers (27 of 70 keys).
- `summary.medianReward`/`maxReward` mix currencies (`MER` rewards are 2000) and leave out `rewardMax`.
- `difficulty` is set on only 855 of 2584 missions.

Related specs: [dump-enhancements](../plans/specs/dump-enhancements.md) (items 1, 2, 9), [dump-reuse](../plans/specs/dump-reuse.md).
