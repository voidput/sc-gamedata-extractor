---
title: game-item-obtain (game-item-obtain.json)
tags: [docs, datasets]
updated: 2026-11-25
---

# game-item-obtain (game-item-obtain.json)

Where each item comes from, worked out once per build: whether it may drop as random loot and why not, the best spawn point of its top 12 loot areas with estimated odds, loot tables no area rolls (corpses, events), contract awards, Wikelo trades and crafting blueprints. It is **derived from other datasets, not from game files**, and most of its numbers are estimates (`_assumptions`). On LIVE 4.10.201.48085: 5,930 items, 2,513 shared spawn points, 61 shared table names, 10,508 loot rows; 2,242,590 bytes minified. PTU 5.0.202.35026: 6,334 items, 4,063 spawn points, 2,841,522 bytes.

## Source

- `crates/sc-gamedata/src/item_obtain.rs` (spec [dump-enhancements](../plans/specs/dump-enhancements.md) item 3). Inputs read from the output directory (`read_inputs`, item_obtain.rs:497): `game-item-tags.json`, `game-loot.json`, `game-item-sources.json` (required), `game-blueprints.json` and `game-wikelo-trades.json` (optional).
- Rebuilt by `write_item_obtain` (item_obtain.rs:544, called from `crates/sc-gamedata/src/bin/sc-gamedata.rs:1854`) after every extract that leaves the inputs on disk; removed when a required input is missing, so it never restates older inputs.
- The odds come from the shared obtain query, `BulkObtain` (`crates/sc-gamedata/src/query/obtain.rs`: `areas_all` :1201, `other_tables` :1218), the same numbers the starorg-codex app's WHERE TO GET hints show. Row building in `build_body` (item_obtain.rs:280).
- Only non-template items of `game-item-tags` are considered, and only items with something to say (a source or a loot flag) are listed.

## Shape

Top-level keys: `_source`, `_extracted`, `_build`, `_schema`, `_assumptions[]` (5 strings), `lootColumns`, `otherTableColumns`, `spawns[]`, `tables[]`, `items{}`, `summary{}`. `_schema: "game-item-obtain@1"` sits after `_build` (since tags 4.10.201.48085 / 5.0.202.35026, commit `b9e3700d0`) and is counted in the byte sizes above. `build.json` has an `extractor` and a `files[]` entry with the file's hash (since tags 4.10.201.48085 / 5.0.202.35026).

`_extracted` and `_build` are copied from `game-item-sources` (`_build` falls back to `game-item-tags`).

### Column names

- `lootColumns`: `["spawns", "kind", "scoreEstimate", "perRollEstimate"]`.
- `otherTableColumns`: `["tables", "perRollEstimate"]`.

### `spawns[]` (2,513 on LIVE, in area order)

The best spawn point of an area, shared by every item row that points at it by index.

| Field | Type | Meaning | Units/range (LIVE) | Source |
|---|---|---|---|---|
| `area` | string | `game-item-sources` `areas[].name` | 2,513 | |
| `config` | string? | Spawn config name | 2,457 | `configs[].name` |
| `entity` | string | Spawned entity class | 2,513 | `slots[].entity` |
| `entityName` | string? | Its display name | 2,207 | `slots[].entityName` |
| `table` | string? | Loot table it rolls (random-loot spawns only) | 731 | `slots[].loot.table` |
| `slotShare` | number | The slot's weight over the config's total weight | 0.002983 to 1, 4 significant digits | computed |
| `fillChance` | number? | The config's `fillChance` | 230 | `configs[].fillChance` |
| `chance` | number? | The slot's `loot.chance` | 731 | `slots[].loot.chance` |
| `maxResults` | number? | The slot's `loot.maxResults` | 731 | `slots[].loot.maxResults` |
| `contents` | string? | Value after `LootGeneration/LootContents/` in the spawn tags | 2,084 | `configs[].spawnTags` |
| `size` | string? | Value after `LootGeneration/LootSize/` | 1,188 | `configs[].spawnTags` |
| `rarity` | string? | Value after `LootGeneration/LootRarity/`, else the last word of the config name when it is a rarity word | Common 745, Uncommon 414, Rare 472, Legendary 60, Reward 6, absent 816 | `spawn_of` (item_obtain.rs:105) |
| `raritySource` | `"name"`? | Set when `rarity` came from the config name | 286 | |

### `tables[]`

61 sorted table names (`DerelictTest`, `LootTable_Actor_AcePilot`, `V3LootTable_Actor_CQC_CouncilScrip`, `Weapons_AndRandomLoot`), each a loot table that no area slot rolls and that is among the kept `otherTables` rows of at least one item.

### `items{<className>}` (5,930 on LIVE; keys are the `game-item-tags` `className` as written, mixed case on 986)

| Field | Type | Meaning | Units/range (LIVE) | Source |
|---|---|---|---|---|
| `eligibility` | object? | `{canLoot?, reason?, rarity?}` from the item's tags: `canLoot` false with `reason` (the text after `LootGeneration/CannotGenerateAsLoot/`), true when tagged `CanGenerateAsLoot`, absent when neither | 5,573 items: true 3,856, false 1,086, absent 631; reasons `PromotionalItem` 386, `InGameReward` 152, `InGameReward/Wikelo` 55, ... | `loot_eligibility` (query/obtain.rs:119) |
| `loot` | array? | Rows `[spawns[], kind, scoreEstimate, perRollEstimate?]`: indices into `spawns[]`, `loot` / `fixed` / `loose`, then the odds; rows that differ only in the index are merged | 4,033 items; rows: loot 10,078, loose 266, fixed 164 | `merge_rows` (item_obtain.rs:185) |
| `areasTotal` | number? | Areas the item can come from (all, not just the 12 kept) | 4,033 items; 1 to 133 | |
| `otherTables` | array? | Rows `[tables[], perRollEstimate]`, top 12 | 3,856 items; per-roll 0.00002138 to 0.9091 | `BulkObtain::other_tables` |
| `otherTablesTotal` | number? | Tables in all, when more than 12 | 1,125 items | |
| `contractAwards` | array? | `{generator, contract, amount?, weighting?}` from `game-item-sources` | 105 items | `award_row` (item_obtain.rs:218) |
| `wikelo` | array? | `{trade, amount?, source}`: trades whose resolved rewards name the item (`amount` and `source` copied from the reward row, `award` on LIVE), else, only when the item has no contract award either, trades whose `rewardInferred` squashes to the item name (`source: "inferred"`, no `amount`) | 65 items, 70 rows, all `award` on LIVE | `wikelo_rows` (item_obtain.rs:242) |
| `blueprints` | array? | `{recordName, rewardContracts?}`: blueprints that produce the item, and the number of that blueprint's `rewardedBy` rows | 1,416 items | `game-blueprints` |

### `summary`

`items` 5,930, `withLoot` 4,033, `bestRandomLoot` 3,855, `bestFixed` 105, `bestLoose` 178, `withOtherTables` 3,856, `withContractAwards` 105, `withWikelo` 65, `withBlueprints` 1,416, `lootRows` 10,508 (after merging), `spawns` 2,513, `tables` 61.

### Example (LIVE, trimmed: 2 of 3 `loot` rows and 2 of 6 `otherTables` rows)

```json
"behr_rifle_ballistic_01": {
  "eligibility": {"canLoot": true, "rarity": "Common"},
  "loot": [
    [ [395, 497, 579, 671, 787], "fixed", 0.1429 ],
    [ [282], "loot", 0.02609, 0.008772 ]
  ],
  "areasTotal": 98,
  "otherTables": [ [ [60], 0.01831 ], [ [0], 0.005495 ] ],
  "otherTablesTotal": 14,
  "blueprints": [{"recordName": "CraftingBlueprintRecord.BP_CRAFT_behr_rifle_ballistic_01"}]
}
```

## Derived, estimated and curated values

Everything in this file is derived. The five `_assumptions` (item_obtain.rs:63) state what the odds rest on:

- `perRollEstimate` assumes one roll picks evenly among the lootable items of the pool it reaches; the game files give weights only down to the tag selector.
- `scoreEstimate` = slotShare × fillChance × chance × (1 − (1 − perRollEstimate)^maxResults); fixed and loose spawns are slotShare × fillChance.
- Random loot is opt-in: only items tagged `LootGeneration/CanGenerateAsLoot` roll from tables; fixed and loose spawns ignore the flag.
- `rarity` is the spawn's `LootRarity` tag, else (`raritySource: "name"`) the last word of the config name.
- Merged rows rank by `scoreEstimate` (or `perRollEstimate`) descending, then by first index; `areasTotal` / `otherTablesTotal` count areas and tables, not rows.

Also derived: the `MAX_LOOT` / `MAX_OTHER_TABLES` cap of 12 (item_obtain.rs:57), the 4-significant-digit rounding (`sig`, item_obtain.rs:82), and the inferred Wikelo match (`source: "inferred"`). No curated values.

## Redaction

Nothing. Not in `PROSE_FIELDS`; names, class and record names only, nothing UEX-derived (item_obtain.rs:38).

## Consumers

- No reader of `game-item-obtain.json` was found in snares, MCsim, Bindscape, sc-telemetry, SCMCP or starorg-codex (`feat/dataset-reuse`). SCMCP (`src/vendor/sc-gamedata/obtain.ts`) and the app (`crates/starorg-gamedata/src/query/obtain.rs`) compute the same odds from the input datasets themselves; this file lets them skip that.
- Planned: patch notes summarise it (counts plus top movers by odds change, [dump-enhancements](../plans/specs/dump-enhancements.md) decision 3, not built).
- Contracts: the sc-telemetry rules for every dataset in [consumer-datasets](../plans/specs/consumer-datasets.md) (`_build` equals the tag; `_schema` stem equals the file stem). Because rows point into `spawns[]` by position, one new spawn point shifts every later index; a diff has to resolve indices before comparing.

## Derivable data

This file derives everything, so the additions are inputs it does not use yet and joins it does not make.

Parse gaps in the odds:

- **V3 `entityClasses` selectors are not read.** `entry_selector` (`crates/sc-gamedata/src/query/obtain.rs:176`) builds a selector from tags only, and an empty selector matches everything (`selects`, query/obtain.rs:196). `game-loot` has 64 objects with `entityClasses` naming 26 classes. For example `V3LootTable_Actor_CQC_CouncilScrip` has a "Unique item" archetype whose only entry names `carryable_1h_cy_physical_currency_scrip_council_1`; on LIVE 3,817 items list that table in their kept `otherTables` rows (the P4-AR rifle and canned drinks at 0.00002138), and the scrip itself gets the same 0.00002138 for it instead of the entry's own share.
- **V3 `optionalData` is ignored:** `choiceLimit` (202 objects in `game-loot`), `chanceToExist` (12, for example 0.75 on that scrip archetype), `stackSize` (52, `{min: 1, max: 2}`), `amountToSpawn` (22), `dupeLimit` (19), `priorityCount` (34). By their names these bound how many items a roll gives and whether an archetype rolls at all; their exact rules are not established.
- **Spawn-with rules are ignored:** 111 `tagsToMatch` groups such as "Suitable Ammunition for that Weapon" (`mode: MostSimilar`, `min: 1`, `max: 3`), so magazines that spawn beside a weapon get no credit.
- **`specialEvents` are not an input:** 5 event archetypes with `probabilityPerContainer` (Luminalia 0.033, Vara 0.05, Darkwater 0.03, ...).
- `maxResults` 0 (232 slots) counts as one roll (query/obtain.rs:814); `_assumptions` does not say so.

Sources the records give that it does not join:

- **NPC worn gear** from `game-npc-loadouts`: 2,447 item classes with a per-actor chance (an estimate there too). 90 of them have no row here at all.
- **Mission item rewards** (`ContractResult_Item`, about 253 results outside The Collector), once `game-item-sources` reads them (see that note).
- **Blueprint products not in `game-item-tags`**: 214 product classes (ship weapons such as `amrs_lasercannon_s1`..`s6`, shields, radars) and 29 Wikelo reward classes (27 vehicles such as `rsi_polaris_collector_military`, plus `foodorder` and `intro`) are dropped because only item-tags classes are listed ([dump-enhancements](../plans/specs/dump-enhancements.md) item 3, Open).
