---
title: game-item-sources (game-item-sources.json)
tags: [docs, datasets]
updated: 2026-11-25
---

# game-item-sources (game-item-sources.json)

Where items spawn and what hands them out. `areas[]` is one row per harvestable slot preset (the rulebook for one kind of place): for each spawn-point tag combination it lists what may spawn there by relative weight, either a container that rolls a loot table, a harvestable whose entity has a fixed loadout (a weapon rack), or a loose item. `contractAwards[]` lists items contract generators hand over (`ItemAwardEntityClass`). On LIVE 4.10.201.48085: 167 areas, 2,989 configs, 6,879 slots (2,161 loot, 553 fixed), 113 contract awards; 1,434,377 bytes minified. PTU 5.0.202.35026: 169 areas, 3,417 configs, 18,851 slots (2,211 loot, 12,218 fixed), 2,931,191 bytes.

## Source

- Filters at `crates/sc-gamedata/src/domains/item_sources.rs:611`: `**/harvestable/**`, `**/entities/scitem/**`, `**/contracts/contractgenerator/**`, `**/tagdatabase/**`.
- Areas (`loot_areas`, item_sources.rs:502): every record under `harvestable/slotpresets/`. `parse_area` (item_sources.rs:450) reads `multiConfig.defaultConfig` and `multiConfig.taggedConfigs[]` (`name`, `tagList.tags`, `subConfig`), or a plain `subConfig`. `sub_configs` follows `subConfigManual`, `subConfig` and `subConfigRef` down to a `SubHarvestableConfig`.
- Slots (`parse_slot`, item_sources.rs:376): `SubHarvestableSlot.harvestable` (a `harvestable/harvestablepresets` record, whose `entityClass` is used when the slot has no `harvestableEntityClass`), `relativeProbability`, `lootConfig` (`parse_loot_config`, item_sources.rs:329: `lootTableV3` or `lootTable`, `lootConstraints.poolFilter`, `secondaryChoices.multiLayerRecord` / `singleLayerRecord`, `totalResultsLimit`, `chanceToGenerate`). `contains` is every `SItemPortLoadoutEntryParams` class in the entity's default loadout (`loadout_items`, item_sources.rs:292).
- Contract awards (`contract_awards`, item_sources.rs:518): `generators[].contracts[].contractResults`, walked for `ItemAwardEntityClass` (`entityClass`, `amountToAward`) under an optional `ItemAwardWeightings.weighting`.
- Place classification (`classify_area`, item_sources.rs:167) from the preset name only, with the word table `AREA_PLACES` (item_sources.rs:105).
- Domain at item_sources.rs:602, `parse` at item_sources.rs:646. Needs the localization table (entity display names).
- Derives from no other dataset. `game-mining-spawns` reuses `loot_areas` (`crates/sc-gamedata/src/domains/mining_spawns.rs:837`); `game-item-obtain` reads the published file.

## Shape

Top-level keys: `_source`, `_extracted`, `_build`, `_schema`, `areas[]`, `contractAwards[]`, `summary{}`. `_schema: "game-item-sources@1"` sits after `_build` (since tags 4.10.201.48085 / 5.0.202.35026, commit `b9e3700d0`) and is counted in the byte sizes above. `build.json` has an `extractor` and a `files[]` entry with the file's hash (since tags 4.10.201.48085 / 5.0.202.35026).

### `areas[]` (sorted by `name`)

| Field | Type | Meaning | Units/range (LIVE) | Source |
|---|---|---|---|---|
| `name` | string | Preset path below `harvestable/slotpresets/` (`caves/loot_caves_occupied_rock`) | 167 | record path |
| `place` | object? | `{label, kind}`: the kind of place the name implies (`kind` is the matching name word, `label` a short English place name this code writes) | 154 present | `classify_area` |
| `family` | string? | How the preset was written: `v3`, `multi` or `classic`, from preset words in the name | v3 25, multi 40, classic 52, absent 50 | `classify_area` |
| `isTest` | `true`? | A test preset; omitted when false | 1 (`cave_full_test`) | `classify_area` |
| `placeSource` | string? | `heuristic` (a name word matched `AREA_PLACES`) or `curated` (the Rock Cracker rule) | heuristic 151, curated 3, absent 13 | `classify_area` |
| `configs` | array | One per default / tagged config | 2,989 in all | see below |

### `areas[].configs[]`

| Field | Type | Meaning | Units/range (LIVE) | Source |
|---|---|---|---|---|
| `name` | string? | `default`, or the tagged config's name (`PenDispenser - Medical`); absent for a preset with a plain `subConfig` instead of a `multiConfig` | 2,926 present, 63 absent | `taggedConfigs[].name` |
| `spawnTags` | string[] | Spawn-point tag paths the config applies to (empty for `default`) | | `taggedConfigs[].tagList.tags` |
| `fillChance` | number? | Omitted when 1 | 301 present, 0.005 to 0.997 | first sub-config's `initialSlotsProbability`; meaning beyond the field name not established |
| `slots` | array | What may spawn | 6,879 in all | `subHarvestables[]` |

### `areas[].configs[].slots[]`

| Field | Type | Meaning | Units/range (LIVE) | Source |
|---|---|---|---|---|
| `entity` | string | Entity class short name | | `harvestableEntityClass` or the preset's `entityClass` |
| `entityName` | string? | Entity display name when real | 5,896 present | `AttachDef.Localization.Name` |
| `preset` | string? | Harvestable preset short name | 1,586 present | `harvestable` |
| `weight` | number | Relative weight among the config's slots (1 when absent) | 0 to 100; 4 slots are 0 | `relativeProbability` |
| `loot` | object? | Loot the slot's container rolls | 2,161 slots: v1 777, v3 1,384 | `lootConfig` |
| `loot.table` | string | Loot table record short name | | `lootTableV3`, else `lootTable` |
| `loot.version` | string | `v3` when `lootTableV3` is set, else `v1` | | |
| `loot.poolFilter` | string? | Pool filter record (`game-loot` `filters[]`) | 1,379 present | `lootConstraints.poolFilter` |
| `loot.secondaryChoices` | string? | Secondary choice record (`game-loot` `secondaryChoices[]`) | 1,379 present | `lootConstraints.secondaryChoices.multiLayerRecord` ?? `singleLayerRecord` |
| `loot.maxResults` | number? | Meaning not established beyond the field name | present on all 2,161; 0 (232 slots), 1 to 15 | `lootConstraints.totalResultsLimit` |
| `loot.chance` | number? | Meaning not established beyond the field name | 1 on all 2,161 | `lootConstraints.chanceToGenerate` |
| `contains` | string[]? | Item classes in the entity's default loadout, sorted (a rack's guns) | 553 slots | `loadout_items` |

### `contractAwards[]` (sorted by `item`, then `contract`)

| Field | Type | Meaning | Units/range (LIVE) | Source |
|---|---|---|---|---|
| `generator` | string | Generator `debugName` | only `TheCollector_Small_Items`, `TheCollector_Vehicles` | `generators[].debugName` |
| `contract` | string | Contract `debugName` | | `contracts[].debugName` |
| `item` | string | Awarded entity class | | `ItemAwardEntityClass.entityClass` |
| `amount` | number? | Count awarded | 1 or 20 | `amountToAward` |
| `weighting` | number? | Weight of the enclosing weighted group; meaning beyond the field name not established | 0.15, 0.25, 1 | `ItemAwardWeightings.weighting` |

### `summary`

`areas` 167, `configs` 2,989, `slots` 6,879, `lootSlots` 2,161, `fixedSlots` 553, `contractAwards` 113, `unplacedAreas[]` (areas with no place that are not tests; 12 on LIVE: the generic, food and drink, hightech and lowtech themes, `v3slotpreset_soo`, `v3slotpreset_tsg` and four `special_*` presets).

### Example (LIVE, trimmed)

```json
{
  "name": "asddelving_multiconfig",
  "place": {"label": "ASD facilities", "kind": "asddelving"},
  "family": "multi",
  "placeSource": "heuristic",
  "configs": [{
    "name": "PenDispenser - Medical",
    "spawnTags": ["LootGeneration/LootType/Consumables/Medical"],
    "slots": [{"entity": "lootablecontainer_pendispenser_3slot_template", "entityName": "Small Container",
               "preset": "dispenser_med_itemslot_dcd", "weight": 1,
               "contains": ["resource_relay_switch_template"]}]
  }]
}
```

## Derived, estimated and curated values

- `place`, `family`, `isTest` and `placeSource` are inferred from the preset **name**, not read from any record ([dump-enhancements](../plans/specs/dump-enhancements.md) item 4a). `placeSource: heuristic` means a word of the name matched `AREA_PLACES`; `curated` is the hand-checked Rock Cracker rule. `place.label` is English text this code writes.
- `contains` is the entity's default loadout flattened; whether every loadout entry spawns is not stated by the records.
- No estimates (the odds built on these rows live in `game-item-obtain`).

## Redaction

Nothing. Not in `PROSE_FIELDS`; the file holds names only.

## Consumers

- **SCMCP:** `src/versions.ts:47` (collection `areas`); `src/itemfinder.ts:67`, `:147`-`:148` (`areas`, `contractAwards`); `vendor/sc-gamedata/types.ts:29`-`:75` (`SpawnLoot`, `SpawnSlot`, `SpawnConfig`, `LootArea` with `name` and `configs` only, `ContractAward`); `vendor/sc-gamedata/obtain.ts:361`-`:375` (`fillChance`, `contains`, `poolFilter`), `:526` (`contractAwards`).
- **starorg-codex app** (`feat/dataset-reuse`): `crates/starorg-mcp/src/prebuilt.rs:162` needs `areas`, `contractAwards`, `summary`, and both arrays must read as `query::types::LootArea` / `ContractAward`; verified on 4.10.200.58746. Mining spots join `areas[].name` (`crates/starorg-gamedata/src/query/mining.rs`).
- **In this crate:** `game-item-obtain` (`item_obtain.rs:500`).
- Contracts: the sc-telemetry rules for every dataset in [consumer-datasets](../plans/specs/consumer-datasets.md) (`_build` equals the tag; `_schema` stem equals the file stem). [dump-reuse](../plans/specs/dump-reuse.md) E.5 lists item-sources as compatible as-is, so the field names above stay.

## Derivable data

Fields the records carry that this dataset does not read (raw values from `sc-gamedata/extracted-data`, LIVE 4.10.201.48085 per its `_build.json`, mixed with stale records):

- Respawn and despawn timers on `harvestable/harvestablepresets/*` (572 records): `respawnInSlotTime` (402 presets at `3600.0`, 49 at `7200.0`, 43 at `1200.0`, 40 at `0.0`, 31 at `1800.0`) and `despawnTimeSeconds` / `additionalWaitForNearbyPlayersSeconds` (`dispenser_med_itemslot_dcd`: 600 and 300, with `respawnInSlotTime` 1800.0). The slot and config multipliers `harvestableRespawnTimeMultiplier` (1.0 on 3,109 slots, 0.5 on 68, 0.1 on 1) and `configRespawnTimeMultiplier` (1.0 on 2,261, 0.005 on 1) sit in the slot presets next to the fields already read. The unit of `respawnInSlotTime` is not stated by the record.
- The rest of `lootConstraints`: `fullnessFactorRange` (`{min: 0.2, max: 0.8}` on the ASD pen dispenser), `chanceToGenerateAdditionalAttachedInventories` (1.0), `advanced.pruningLevel` (`containerSize`) and `advanced.fullnessMode` (`stopAfterExceed`). Meanings not established.
- `initialSlotsProbabilityDeepest` and `relativeProbabilityDeepest` beside the probabilities already read (3,178 `relativeProbabilityDeepest` keys across slot presets, 3,141 of them null; `initialSlotsProbabilityDeepest` is null on 2,260 of 2,262). Meaning not established.
- Item awards of type `ContractResult_Item` (`entityClass`, `amount`, `missionResults`). `contract_awards` only walks `ItemAwardEntityClass`, which only The Collector uses, so `contractAwards` has two generators. 253 `ContractResult_Item` results sit in 19 other generator files (`thecollector.json` itself holds 51 more), for example `thecouncil_guild/headhunters/headhunters_shipambush.json` (28) and `mercenary_guild/foxwellenforcement/shipbattles/foxwell_defendentitiesandescort.json` (21), such as Council scrip `carryable_1h_cy_physical_currency_scrip_council_1` with `amount` 1 to 18. `game-wikelo-trades` already reads this type (`crates/sc-gamedata/src/domains/wikelo.rs:134`). Reading it here would give `game-item-obtain` mission rewards.

Values computable by joining:

- Slot `entity` → its `SCItemInventoryContainerComponentParams.containerParams` → `game-containers` capacity, so each spawn point says how big the box is (`lootablecontainer_*` entities).
- Per area, the share of slots by kind (loot, fixed, loose) and the loot tables it can roll, for the app's area screens.
- Area → concrete locations from the object containers: item 4b of [dump-enhancements](../plans/specs/dump-enhancements.md) (`usedAt[]`, `placeSource: "data"`), which would replace the name heuristic where it confirms it.

Parse gaps:

- `loot.chance` is 1 on every loot slot and `loot.maxResults` is 0 on 232; the dataset copies them as the records give them. `game-item-obtain` treats 0 as one roll (`crates/sc-gamedata/src/query/obtain.rs:814`), which no record states.
- 50 areas have no `family`, and 13 no place; `summary.unplacedAreas` lists 12 of them.
- PTU 5.0.202.35026 has 12,218 fixed slots against 553 on LIVE; worth checking whether that is new content or a change in how presets nest before the next LIVE build.
