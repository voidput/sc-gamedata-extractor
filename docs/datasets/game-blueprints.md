---
title: game-blueprints (game-blueprints.json)
tags: [docs, datasets]
updated: 2026-11-25
---

# game-blueprints (game-blueprints.json)

Crafting blueprints: per `CraftingBlueprintRecord`, what it produces, its recipe tiers (craft time and a cost tree of refined resources in SCU, physical items by count, and "choose n of these" selections), the product's display name, and the missions whose blueprint-reward pools can award it. Also the reward pools themselves, how often each resource appears across recipes, and counts per category. On LIVE 4.10.201.48085: 1,634 blueprints (703 awarded by at least one mission), 176 reward pools, 26 resources, 14 categories; 3,269,569 bytes minified. PTU 5.0.202.35026: 1,639 blueprints, 181 pools, 28 resources, 3,340,036 bytes.

## Source

- Filters at `crates/sc-gamedata/src/domains/blueprints.rs:397`: `**/crafting/**`, `**/contracts/contractgenerator/**`, `**/contracts/contractscenarios/**`, and for product names `**/entities/scitem/ships/**`, `weapons/**`, `characters/human/**`, `carryables/**`.
- Blueprints (`parse_blueprint`, blueprints.rs:288): `blueprint.category`, `blueprint.blueprintName`, `blueprint.processSpecificData.entityClass` (the product), and per `tiers[].recipe.costs`: `craftTime` (`TimeValue_Partitioned`, `time_seconds` blueprints.rs:206) and `mandatoryCost` (`parse_cost`, blueprints.rs:161: `CraftingCost_Resource` `resource` / `quantity.standardCargoUnits` / `minQuality`, `CraftingCost_Item` `entityClass` / `quantity` / `minQuality`, `CraftingCost_Select` `count` / `nameInfo` / `options`). Records with no tiers (the dismantle record) are skipped.
- Display name: the product's `AttachDef` name (`item_display_name`, `crates/sc-gamedata/src/helpers_gameplay.rs:65`), because `blueprintName` is a placeholder on nearly every record.
- Reward pools (`parse_pools`, blueprints.rs:344): `BlueprintPoolRecord.blueprintRewards[]` (`blueprintRecord`, `weight`), 176 records under `crafting/blueprintrewards/`.
- Mission links (`collect_blueprint_rewards`, blueprints.rs:278): every `BlueprintRewards` (`blueprintPool`, `chance`) under a record path containing `/contracts/`; the contract is the nearest enclosing object with a `debugName`, the generator the one above it, the mission title the contract's `paramOverrides.stringParamOverrides` `Title` (`title_of`, blueprints.rs:215).
- Domain at blueprints.rs:388, `parse` at blueprints.rs:429. Needs the localization table. Derives from no other dataset; read by `game-item-obtain`.

## Shape

Top-level keys: `_source`, `_extracted`, `_build`, `_schema`, `blueprints[]`, `rewardPools[]`, `resourceUse{}`, `byCategory{}`, `summary{}`. `_schema: "game-blueprints@1"` sits after `_build` (since tags 4.10.201.48085 / 5.0.202.35026, commit `b9e3700d0`) and is counted in the byte sizes above. `build.json` has an `extractor` and a `files[]` entry with the file's hash (since tags 4.10.201.48085 / 5.0.202.35026).

### `blueprints[]` (sorted by `name`)

| Field | Type | Meaning | Units/range (LIVE) | Source |
|---|---|---|---|---|
| `name` | string | The localized `blueprintName` when it is a real name, else the product's display name, else the product class, else the record short name | | see Source |
| `recordName` | string | `CraftingBlueprintRecord.<name>` | 1,634 | |
| `id` | string | Record GUID | | |
| `category` | string? | Category record short name | 14 values: `FPSArmours` 937, `FPSWeapons` 213, `VehicleComponentS1` 120, ... `MissionItem` 4 | `blueprint.category` |
| `produces` | string? | Product entity class (file stem) | 1,633 present; 1 absent (`BP_CRAFT_COOL_S04_CNOU_Pioneer`) | `processSpecificData.entityClass` |
| `tiers` | array | Recipe tiers | exactly 1 on every LIVE record | `blueprint.tiers` |
| `tiers[].craftTimeSeconds` | number? | Total of days, hours, minutes, seconds, rounded to 0.01; omitted when zero | seconds; present on all 1,634; 10 to 9,060 | `recipe.costs.craftTime` |
| `tiers[].costs` | array | Cost nodes; a root `choice` with no `count` is unwrapped into its options | on LIVE every tier has exactly one root `choice` node | `recipe.costs.mandatoryCost` |
| `displayName` | string? | Product display name | 1,630 present | product `AttachDef` |
| `rewardedBy` | array? | Missions that can award it, one row per contract and pool, sorted by `mission` | 703 blueprints, 8,424 rows, 330 distinct `mission` values | see Source |

Cost nodes (`kind` first):

| `kind` | Fields | Units/range (LIVE) |
|---|---|---|
| `resource` | `resource` (ResourceType short name), `scu?`, `minQuality?` (omitted when 0) | 4,052 nodes; `scu` 0.01 to 15 SCU; `minQuality` on 1,231 |
| `item` | `item` (entity class), `quantity?`, `minQuality?` | 305 nodes; `minQuality` on 178; `quantity` 1 to 495; e.g. `harvestable_mineral_1h_glacosite` × 170 |
| `choice` | `count?` (how many options to supply), `label?` (localized `nameInfo.displayName`, or `debugName` such as `ASPECTS` when that is a placeholder), `options[]` | 5,991 nodes; `count` 1 (4,424), 2 (445), 3 (1,091), 4 (31) |

`minQuality` ranges 1 to 900 on LIVE (1,409 nodes). Its scale is the game's; `crafting/globalparams` sets `defaultCompositionQuality` 500, and the recipe modifier ranges run from `startQuality` 0 to `endQuality` 1000.

`rewardedBy[]` rows:

| Field | Type | Meaning | Units/range (LIVE) |
|---|---|---|---|
| `mission` | string | Localized contract title when real, else the contract `debugName`; titles keep the game's `~mission(Location)` tokens | |
| `contract` | string | Contract `debugName` | |
| `generator` | string? | `debugName` of the enclosing object above the contract | empty string on 22 rows |
| `pool` | string | Reward pool short name | |
| `chance` | number? | `BlueprintRewards.chance`: chance the pool is awarded on completion (doc comment, blueprints.rs:80) | 0.25, 0.75, 1 |
| `weight` | number? | This blueprint's weight in the pool | |
| `poolWeight` | number? | Sum of the pool's weights | |

### `rewardPools[]`, `resourceUse`, `byCategory`, `summary`

- `rewardPools[]`: `{name, blueprints[{blueprint, weight?}]}`, sorted by `name`; 176 pools of 1 to 18 blueprints, 838 entries, all with a `weight`.
- `resourceUse`: resource → number of resource cost nodes naming it across all recipes, most first (`Aslarite` 872, `Ouratite` 506, `Laranite` 361, ... `Quantanium` 3). Choice options count too, so it is not a count of blueprints.
- `byCategory`: category → number of blueprints.
- `summary`: `blueprints` 1,634, `distinctResources` 26, `categories` 14, `named` 1,630, `rewarded` 703, `rewardPools` 176.

### Example (LIVE, trimmed: `id`, one of the three ASPECTS options and all but the first `rewardedBy` row left out)

```json
{
  "name": "5CA 'Akura'",
  "recordName": "CraftingBlueprintRecord.BP_CRAFT_SHLD_BEHR_S03_5CA_SCItem",
  "category": "VehicleComponentS3",
  "produces": "shld_behr_s03_5ca_scitem",
  "tiers": [{"craftTimeSeconds": 780, "costs": [{"kind": "choice", "count": 3, "label": "ASPECTS", "options": [
    {"kind": "choice", "count": 1, "label": "Shell", "options": [{"kind": "resource", "resource": "Iron", "scu": 1.2, "minQuality": 1}]},
    {"kind": "choice", "count": 1, "label": "Frequency Controller",
     "options": [{"kind": "item", "item": "harvestable_mineral_1h_glacosite", "quantity": 170, "minQuality": 1}]}]}]}],
  "displayName": "5CA 'Akura'",
  "rewardedBy": [{"mission": "Defend ~mission(Location) From Mercs", "contract": "HeadHunters_DefendEntitiesAndEscort_Stanton_VeryHard",
                  "generator": "HeadHunters_DefendEntitiesAndEscort_Stanton", "pool": "BP_REWARDS_StantonHeadhuntersEasy",
                  "chance": 1, "weight": 1, "poolWeight": 13}]
}
```

## Derived, estimated and curated values

- `name` / `displayName` come from the product item, not the blueprint (its `blueprintName` is a placeholder on 1,633 of 1,634 records, [dump-enhancements](../plans/specs/dump-enhancements.md) item 1).
- `rewardedBy` is a join this code makes (pool path → contracts that reference it); `poolWeight` is a sum it computes. `generator` is positional (the `debugName` object above the contract), not a named field.
- `resourceUse` and `byCategory` are counts. No estimates, no curated values.

## Redaction

Nothing. Not in `PROSE_FIELDS`; mission titles are names. No string-table keys are written for blueprints (dump-enhancements item 1).

## Consumers

- **SCMCP:** `src/versions.ts:38` (collection `blueprints`); `src/itemfinder.ts:68`, `:149`; `vendor/sc-gamedata/obtain.ts:46`-`:51` (`BlueprintLike`: `name`, `displayName`, `produces`, `rewardedBy[].mission/contract/pool/chance`), `:547`, `:621`-`:622`.
- **starorg-codex app** (`feat/dataset-reuse`): `crates/starorg-mcp/src/prebuilt.rs:156` needs `blueprints`, `rewardPools`, `resourceUse`, `summary`, and `blueprints` must read as `query::obtain::BlueprintLike`; verified on 4.10.200.58746. `crates/starorg-core/src/model/missions.rs:136` (`Blueprint`: `recordName`, `id`, `category`, `produces`, `tiers[].craftTimeSeconds`) and `:174` (`BlueprintPool`: `name`, `blueprints[{blueprint, weight}]`) are the shapes it deserialises. [dump-enhancements](../plans/specs/dump-enhancements.md) plans to replace the app's own blueprint/contract/pool join with `rewardedBy` and `rewardPools`.
- **In this crate:** `game-item-obtain` (`item_obtain.rs:501`; `recordName`, `produces`, `rewardedBy` count).
- Contracts: the sc-telemetry rules for every dataset in [consumer-datasets](../plans/specs/consumer-datasets.md). [dump-reuse](../plans/specs/dump-reuse.md) E.5 lists blueprints as compatible as-is.

## Derivable data

Fields the records carry that this dataset does not read (raw values from `sc-gamedata/extracted-data`, LIVE 4.10.201.48085 per its `_build.json`, mixed with stale records):

- **Quality → stat modifiers per recipe slot.** Each `CraftingCost_Select` option carries `context[]` of type `CraftingCostContext_ResultGameplayPropertyModifiers` (4,237 contexts in 1,569 blueprint files) listing `gameplayPropertyRecord` and `valueRanges[]` (`CraftingGameplayPropertyModifierValueRange_Linear` ×6,000, `..._LinearIntegerAdditive` ×598). The P4-AR's Frame slot: `gpp_weapon_recoil_smoothness`, `_handling`, `_kick`, each `startQuality 0 → modifierAtStart 1.2`, `endQuality 1000 → modifierAtEnd 0.8`. Most used properties: `gpp_armor_damagemitigation` 1,368, `gpp_armor_temperaturemin` / `max` 915, `gpp_weapon_damage` 473, `gpp_health_maxhealth` 465. This is what material quality does to the crafted item, and nothing publishes it.
- `crafting/craftedproperties/*` (29 `CraftingGameplayPropertyDef` records): `propertyName` (a string key, `@StatName_GPP_Weapon_Recoil_Kick`), `unitFormat` (`@StatUnits_Percent`), `displayTransformation` (`ConvertFactorToPercentChange`). These name the properties above.
- Other cost contexts: `CraftingCostContext_ResultCompositionInclusion` (1,624: `option` `Include` 1,587, `Exclude` 37) and `CraftingCostContext_QuantityMultiplier` (3). Meanings not established.
- **Dismantling:** `blueprints/dismantle/globalgenericdismantle.json` (`GenericCraftingProcess_Dismantle`, `efficiency 0.5`, `dismantleTime` 15 s) is skipped because it has no tiers. `crafting/globalparams/craftingglobalparams.json` adds `dismantleBlacklistResources` (Quantanium, Stileron, Savrilium, Lindinium, Riccite, Ouratite), `dismantleBlacklistEntityClasses`, `refiningQualityUnitMultiplier 2.0` and `defaultCompositionQuality 500`.
- `crafting/legacy/` (39 records: 34 `LegacyCraftingRecipeDefRecord` under `recipes`, 5 `LegacyCraftingRecipeListRecord` under `recipelists`): not read; meaning not established.

Values computable by joining:

- **Resource totals per blueprint:** the SCU of every mandatory resource (choices taken at their `count`), for cost comparisons. Taking the cheapest option of each choice is a reader's choice, so totals over choices are estimates.
- **Resource → where to get it:** `resource` names join `game-commodities` (which already carries the quality distributions and quantization bands, `crates/sc-gamedata/src/domains/commodities.rs:413`), `game-refining` and `game-mining-spawns`, so a blueprint can show the chance a mined batch meets `minQuality` (an estimate: it assumes the stated normal distribution).
- **Dismantle return** = recipe resources × `efficiency` 0.5, without the blacklisted resources. The records state the factor; whether it applies to every resource node equally is not stated, so it is an estimate.
- **Mission → blueprints** reverse index (330 missions), and per blueprint the chance per completion = `chance` × `weight` / `poolWeight` (if one pool draw per completion is assumed, which the records do not state).

Parse gaps:

- 22 `rewardedBy` rows have `generator: ""` (for example `RCD_Exclusive`, "QV Breaker Station Mining Rights (Exclusive)"): the object above the contract has an empty `debugName`.
- 4 blueprints have no `displayName`: `BP_CRAFT_COOL_S04_CNOU_Pioneer` (no product reference at all) and three whose product has no real name (`cool_aegs_s04_javelin_scitem`, `radr_gnrp_s03_idris_temp`, `radr_rsi_s04_polaris`).
- 214 of 1,633 product classes have no `game-item-tags` row (ship weapons such as `amrs_lasercannon_s1`..`s6`, shields, radars), so `game-item-obtain` cannot list them.
