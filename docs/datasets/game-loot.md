---
title: game-loot (game-loot.json)
tags: [docs, datasets]
updated: 2026-11-25
---

# game-loot (game-loot.json)

Loot tables and archetypes with their weights, in the game's own shape, compacted: a table lists weighted archetypes, and an archetype picks items **by tag** ("any common armour piece, not a helmet") rather than naming them, with secondary groups (rarity, manufacturer, faction) weighting the result. Two generations of the format coexist: the original `LootTable` / `LootArchetype` (`version: "v1"`) and the V3 records (`version: "v3"`), which add choice limits, stack sizes, spawn-with rules and pool filters. Expanding a tag into the items that carry it is left to the reader (it needs `game-item-tags`). On LIVE 4.10.201.48085: 220 tables (161 v1, 59 v3), 267 archetypes (233 v1, 34 v3), 25 secondary-choice records, 15 pool filters, 5 special events; 407,578 bytes minified. PTU 5.0.202.35026: 224 tables, 269 archetypes, 400,635 bytes.

## Source

- Filters at `crates/sc-gamedata/src/domains/loot.rs:93`: `**/lootgeneration/**`, `**/tagdatabase/**`. Raw record types under `lootgeneration/`: `LootTable` 161, `LootTableV3Record` 59, `LootArchetype` 233, `LootArchetypeV3Record` 34, `LootV3SecondaryChoicesMultiLayerRecord` 15, `LootV3SecondaryChoicesSingleLayerRecord` 10, `PoolFilterRecord` 15, `LootGenerationGlobalParams` 1.
- `parse_loot_record` (loot.rs:44): V3 records are unwrapped one level (`lootTable`, `lootArchetype`); the body goes through `compact_value` (`crates/sc-gamedata/src/helpers_ships.rs:74`), which drops `_Type_`, `_RecordPath_`, `_RecordId_`, `_Pointer_`, nulls and empty strings / lists / objects, turns `Tag.<guid>` references into tag paths and other record references and `file://` paths into short names.
- `tables` = types `LootTable` / `LootTableV3Record` (loot.rs:74); `archetypes` = `LootArchetype` / `LootArchetypeV3Record` (loot.rs:75); `secondaryChoices` and `filters` = every record under `lootgeneration/secondarychoices/` and `lootgeneration/filters/`; `specialEvents` = `LootGenerationGlobalParams.specialEventArchetypes` (loot.rs:137), compacted.
- Domain at loot.rs:84, `parse` at loot.rs:97. No localization. Derives from no other dataset; read by `game-item-obtain`.

## Shape

Top-level keys: `_source`, `_extracted`, `_build`, `_schema`, `tables[]`, `archetypes[]`, `secondaryChoices[]`, `filters[]`, `specialEvents[]`, `summary{}`. `_schema: "game-loot@1"` sits after `_build` (since tags 4.10.201.48085 / 5.0.202.35026, commit `b9e3700d0`) and is counted in the byte sizes above. `build.json` has an `extractor` and a `files[]` entry with the file's hash (since tags 4.10.201.48085 / 5.0.202.35026).

Every collection is sorted by `name`. Because `compact_value` drops nulls and empty values, a missing key means null or empty in the record, not zero; numbers, including 0, are kept.

### `tables[]`

| Field | Type | Meaning | Units/range (LIVE) | Source |
|---|---|---|---|---|
| `name` | string | Record short name | 220 | record name |
| `recordName` | string | Full record name (`LootTable.Ammo`) | | |
| `version` | string | `v1` or `v3` (record type contains `V3`) | v1 161, v3 59 | |
| `lootArchetypes` | array? | Weighted archetypes | absent on 3 (`LootTable_B`, `_C`, `_D`) | `lootArchetypes` |
| `lootArchetypes[].weight` | number | Relative weight | 998 entries; 0.01 to 100 | |
| v1: `lootArchetypes[].archetype` | string | Archetype record short name | all resolve to an `archetypes[].name` | |
| v1: `lootArchetypes[].numberOfResultsConstraints` | object? | `{minResults?, maxResults?}`; meaning not established beyond the names (0-0 on 447 entries) | 709 objects | |
| v3: `lootArchetypes[].name` | string | Entry label (`Ammo`, `Unique item`) | 289 entries | |
| v3: `lootArchetypes[].archetype` | object? | Inline `{entries[]}` or `{lootArchetypeRecord}` (a V3 archetype name) | inline 159, record 126; absent on 4 (the "Unique item" entry of the four `V3LootTable_Actor_*_UniqueItem_TEMPLATE` tables) | |
| v3: `lootArchetypes[].optionalData` | array? | Constraint objects: `choiceLimit` (202), `priorityCount` (34), `dupeLimit` (19), `chanceToExist` (12). Meanings not established beyond the names | 222 entries | |

### `archetypes[]`

| Field | Type | Meaning | Units/range (LIVE) | Source |
|---|---|---|---|---|
| `name`, `recordName`, `version` | string | As for tables | v1 233, v3 34 | |
| v1: `excludedTags` | object? | `{tags[]}`: tag paths an item must not carry | 226 | |
| v1: `primaryOrGroup` | object? | `{entries[]}`: each `{name?, tag, weight, additionalTags?{positiveTags,negativeTags}, optionalData?[{min, max}]}` (438 entries; `optionalData` on 199, e.g. `[{"min": 3, "max": 3}]`, meaning not established) | 229 | |
| v1: `secondaryOrGroups` | array? | `{groupName, entries[{tag, weight}]}` (`Rarity`, ...) | 200 | |
| v3: `entries` | array? | `{name, weight, selector{tags{positiveTags[], negativeTags[]}} or selector{entityClasses{entityClasses[]}}, optionalData?}` | 34 (110 entries) | |

Other keys seen at some depth: `amountToSpawn{min,max}`, `chanceToSpawnWith`, `stackSize{min,max}`, `tagsToMatch{positiveTags,negativeTags}`, `mode` (`MostSimilar`), `min`, `max`, `groupName`. Their meaning is the game's and is not established here beyond the names.

### `secondaryChoices[]` (25)

- Single layer (`LootV3SecondaryChoicesSingleLayerRecord`, 10): `{name, recordName, choices[{name, weight, selector{tags{positiveTags}}}]}`, e.g. `V3ManufacturerWeights_Criminal` (`APAR weapons` 100, `BANU weapons` 0.1, ...).
- Multi layer (`LootV3SecondaryChoicesMultiLayerRecord`, 15): `{name, recordName, layers[]}`, names of single-layer records (`v3secondarychoices_faction`, `v3manufacturerweights_criminal`, `v3secondarychoices_rarity_common`).

### `filters[]` (15, all `PoolFilterRecord`)

`{name, recordName, filter{filters[{name, mode, filter{filterRecord} or filter{tagFilter{positiveTags, negativeTags}}}]}}`; `mode` is `Cumulative` or `Additive`. How the modes combine is the game's rule; the obtain query treats `Additive` specially (`crates/sc-gamedata/src/query/obtain.rs:315`).

### `specialEvents[]` (5)

`{eventString, probabilityPerContainer, minEntriesPerContainer, maxEntriesPerContainer, archetype}`: `ChristmasEvent` 0.033, `HalloweenEvent` 0.05, `Darkwater` 0.03, `DarkwaterHalloweenEvent` 0.05, `DarkwaterChristmasEvent` 0.033 (archetypes `lootarchetype_specialevent_luminalia`, `_vara`, `_darkwater`, ...). Meaning beyond the field names not established.

### `summary`

`tables` 220, `archetypes` 267, `tags` 18,960 (entries in the tag index, not tags used here).

### Example (LIVE, V3 table)

```json
{
  "name": "V3LootTable_Actor_CQC_CouncilScrip",
  "recordName": "LootTableV3Record.V3LootTable_Actor_CQC_CouncilScrip",
  "version": "v3",
  "lootArchetypes": [
    {"name": "Ammo", "weight": 10, "archetype": {"entries": [{"name": "Ammo", "weight": 1,
      "selector": {"tags": {"positiveTags": ["Weapon/FPS/Attachment/Magazine", "Weapon/FPS/Stocked/Shotgun"]}}}]}},
    {"name": "Medpens", "weight": 1, "archetype": {"lootArchetypeRecord": "v3lootarchetype_medpen_healing"}},
    {"name": "Unique item", "weight": 1,
     "archetype": {"entries": [{"name": "Council scrip", "weight": 1,
       "selector": {"entityClasses": {"entityClasses": ["carryable_1h_cy_physical_currency_scrip_council_1"]}},
       "optionalData": [{"stackSize": {"min": 1, "max": 2}}]}]},
     "optionalData": [{"priorityCount": 1}, {"choiceLimit": 1}, {"chanceToExist": 0.75}]}
  ]
}
```

## Derived, estimated and curated values

None. Values are the records' own, compacted; the only rewriting is references to names and tag GUIDs to paths.

## Redaction

Nothing. Not in `PROSE_FIELDS`.

## Consumers

- **SCMCP:** `src/versions.ts:48` (collection `tables`); `src/itemfinder.ts:66`, `:143`; `vendor/sc-gamedata/obtain.ts:148`-`:152` (`primaryOrGroup`, `entries`, `secondaryOrGroups`, `excludedTags`), `:116`-`:124` (selectors, `additionalTags`), `:175` (`lootArchetypeRecord`), `:194` (`filters`), `:208`-`:241` (`archetypes`, `tables`, `filters`, `secondaryChoices`).
- **starorg-codex app** (`feat/dataset-reuse`): `crates/starorg-mcp/src/prebuilt.rs:167` needs `tables`, `archetypes`, `secondaryChoices`, `filters`, `specialEvents`, `summary`, and the file must read as `query::obtain::LootData`; verified on 4.10.200.58746.
- **In this crate:** `game-item-obtain` (`item_obtain.rs:499`) through `crates/sc-gamedata/src/query/obtain.rs`.
- Contracts: the sc-telemetry rules for every dataset in [consumer-datasets](../plans/specs/consumer-datasets.md). [dump-reuse](../plans/specs/dump-reuse.md) E.5 lists loot as compatible as-is, so the compacted game shape and the six top-level keys stay.

## Derivable data

The domain keeps every field of the loot records, so nothing in them is unread (apart from `_Type_`, which `compact_value` drops). The additions are joins and the loot configs outside `lootgeneration/`.

Values computable by joining:

- **Who rolls each table.** 168 of 220 tables are named by a `game-item-sources` slot or a `game-npc-loadouts` actor (`byLootTable`); 52 by neither, among them `Weapons_AndRandomLoot`, the `*_CouncilScrip` actor tables and `LootTable_Container_Antiques_*`. A `usedBy` list per table (areas, actors, entities) would show which tables are live and which are templates.
- **Expanded pools:** per archetype entry, the `game-item-tags` items its selector matches (and the count), which every obtain reader recomputes today. Item counts are exact; odds per item are not (see `game-item-obtain` `_assumptions`).
- **Table → archetype → table reverse links** for V3 `lootArchetypeRecord` references and multi-layer `layers[]`.

Loot configs the records carry outside this dataset's filters:

- `LootGenerationComponentParams.lootConfig` on item entities: 30 records under `entities/` in the mixed extract (`sc-gamedata/extracted-data`), for example `scitem/mission_entities/corpse_prison_default` → `prison_loot`, `scitem/carryables/2h/carryable_2h_fl_05x05x05_carrayblelootableitem_pico_withinventory` → `contrabandpico`, nine → `generalloot`, eight → `v3loottable_food_small`. Neither this dataset nor `game-item-sources` reads them, so these tables look unused.

Parse gaps:

- V3 `selector.entityClasses` entries (64 objects, 26 classes) name items directly. They are kept here, but the obtain query does not read them and treats them as "anything" (see the `game-item-obtain` note).
- `summary.tags` is the tag index size (18,960), not a count of tags these records use.
