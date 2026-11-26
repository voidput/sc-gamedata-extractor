---
title: game-commodities (game-commodities.json)
tags: [docs, datasets]
updated: 2026-11-25
---

# game-commodities (game-commodities.json)

Every resource type the economy knows (metals, gases, processed goods and the placeholder groups for heat, power and life support), the cargo entities under `entities/commodities` that hold them, and the impact and explosion rules for mined and volatile cargo. On LIVE 4.10.201.48085 it has 207 resources, 135 cargo items and 3 damage configurations. The parse output is 270,672 bytes minified (`jq -c . | wc -c`). The published file drops `description` (see Redaction), which leaves 202,179 bytes before the `_redacted` key is added. The `_schema` key (since tags 4.10.201.48085 / 5.0.202.35026, commit `b9e3700d0`) is not in that figure; with it the same command prints 270,703.

## Source

- Records:
  - `libs/foundry/records/resourcetypedatabase/resourcetypedatabase.json`: one `ResourceTypeDatabase` record. Groups nest (`Metal > UnrefinedOres`), and each `ResourceType` sits inline. A raw ore carries its refined form inline under `refinedVersion`.
  - `libs/foundry/records/entities/commodities/**`: `EntityClassDefinition` cargo items. The domain reads their `SCItemPurchasableParams` (`displayName`, `displayType`) and `ResourceContainer` (`capacity`, `defaultComposition[0].entry`).
  - `libs/foundry/records/commodityconfiguration/*.json`: three `CommodityDamageConfiguration` records.
  - `libs/foundry/records/crafting/qualitydistribution/**` and `crafting/qualityquantization/**`: the quality curves, location overrides and display bands that `ResourceTypeCraftingData` points at.
  - `libs/foundry/records/starmap/pu/*`: systems and clusters, so a quality override can name its location.
  - `libs/foundry/records/mining/**`: mineable elements and rock compositions, for `minedAs`.
- Rust: [`crates/sc-gamedata/src/domains/commodities.rs`](../../crates/sc-gamedata/src/domains/commodities.rs).
  - `impl Domain` at line 683, filters at line 692.
  - `parse_resource` at 461, `parse_resource_database` at 503, `parse_quality` at 401.
  - `parse_commodity_item` at 589, `scu_of` at 572, `parse_commodity_damage` at 646.
  - Cross-record fields: `resource_links` at 236, `quality_steps` at 181, `damage_groups` at 291.
  - `group_label` is in `src/query/commodities.rs` at 219. The normal CDF `phi` is in `src/query/quality.rs`.
- Derived from other domains: `minedAs` calls `crate::domains::mining_spawns::mined_elements` (the `game-mining-spawns` element list, through `ore_core`).

## Shape

Top-level keys: `_source`, `_extracted`, `_build`, `_assumptions` (one entry), `resources[207]`, `items[135]`, `damage[3]`, `damageGroups[1]`, `byGroup{11}`, `summary`.

### resources[]

| Field | Type | Meaning | Units/range | Source |
|---|---|---|---|---|
| `name` | string | Resource short name (`Ore_Gold`) | | `ResourceType._RecordName_` |
| `recordName` | string | `ResourceType.<name>` | | `_RecordName_` |
| `id` | string | Record GUID | | `_RecordId_` |
| `displayName` | string? | Localized name with markup stripped; absent for placeholders | 3 of 207 absent | `displayName` via the string table |
| `description` | string? | Localized description; "X description." stubs dropped | redacted on publish | `description` |
| `nameKey`, `descriptionKey` | string? | String-table keys | | `displayName`, `description` |
| `nameStatus` | string? | `placeholder` when the name is a placeholder; absent when real | 3: `EvidenceBox`, `VIPCryopod`, `Vlk_Limpet` (`LOC_PLACEHOLDER`) | `loc_refs` |
| `group` | string | Top group | 11 values (`byGroup`) | group tree |
| `subgroups` | string[] | Group path below the top | | group tree |
| `densityGramsPerCc` | number | Density | g/cm³; present on all 207; 165 are 1.0 | `densityType.densityUnit.gramsPerCubicCentimeter` |
| `refinesTo` | string? | Short name of the refined resource | 30 resources | `refinedVersion` |
| `volatility` | object? | `{volatility, healthDecayPerSecond}` | only `Raw_Quantainium`: 1, 0.0675 | `properties[ResourceTypeVolatility]` |
| `quality` | object? | Quality curve, overrides and bands (below) | 44 resources | `properties[ResourceTypeCraftingData]` |
| `groupLabel` | string? | `group` made readable (`ProcessedGoods` → `Processed Goods`) | | derived |
| `subgroupLabel` | string? | `subgroups` made readable and joined with ` > ` | | derived |
| `tradable` | bool | False when the group ends in `placeholder` | false for `cold`, `hot`, `LifeSupport`, `Electricity` | derived |
| `raw` | bool | Has `refinesTo` | 30 true | derived |
| `rawSources` | string[]? | Resources whose `refinesTo` is this one (case-insensitive name match) | | derived |
| `volatile` | true? | `volatility.volatility > 0` | 1 resource | derived |
| `densityIsDefault` | true? | Density is exactly 1.0 | 165 resources | derived |
| `cargo` | object[]? | Cargo items holding it: `{className, kind, capacityScu}` | 131 resources | join with `items[]` |
| `minedAs` | object[]? | `{element, oreKey}` of the mineable elements whose `resourceType` this is | 40 resources, 42 elements | `game-mining-spawns` |

`quality` object:

| Field | Type | Meaning | Units/range | Source |
|---|---|---|---|---|
| `profile` | string? | Distribution record short name | e.g. `RareShipMineable_QualityDistribution_Default` | `qualityDistributionRecord` |
| `tier` | string? | `profile` cut before `ShipMineable_QualityDistribution` or `_QualityDistribution` | Common 10, Uncommon 6, Rare 5, Epic 3, Legendary 3, FPSMineable 12, Gatherable 5 | derived from the name |
| `distribution` | object? | `{min, max, mean, stddev}` | quality 0-1000 | `qualityDistribution` |
| `locationOverrides` | object[]? | `{location, min, max, mean, stddev, steps}`; identical entries collapse | 35 resources; locations `Pyro System`, `QV Breaker Station` | `locationOverrideList` |
| `bands` | object[]? | `{start, end, value}`: rolls start..end show as `value` | 0-1000 | `qualityQuantization.bands` (`mappedValue`) |
| `steps` | object[]? | Bands clipped to `distribution` min-max: `{start, end, shown, shareEstimate}` | 43 resources | derived |

### items[]

| Field | Type | Meaning | Units/range | Source |
|---|---|---|---|---|
| `name` | string | Entity class (`gold_ore`) | | record short name |
| `recordName`, `id` | string | | | record |
| `displayName`, `nameKey`, `nameStatus` | string? | As on resources | `nameStatus` absent on all 135 | `SCItemPurchasableParams.displayName` |
| `type` | string? | Localized `displayType` | 16 labels, plus 2 that are prose (see Derivable data) | `SCItemPurchasableParams.displayType` |
| `category` | string | Folder below `entities/commodities` | 18 values | record path |
| `resource` | string? | Resource short name in the default composition | null on `Special_HolidayLetter` | `ResourceContainer.defaultComposition[0].entry` |
| `capacityScu` | number? | Capacity of one item | SCU; 103 items are 0.01, 3 are 1, the rest are 0.00035-0.0012 | `capacity.centiSCU / 100`, `microSCU / 1e6` or `standardCargoUnits` |
| `description`, `descriptionKey` | string? | The resource's description and key, copied in | redacted on publish | from `resources[]` |

### damage[] and damageGroups[]

`damage[]` has `name`, `recordName`, `minimumSpeed`, `speedSquaredToDamage`, `gracePeriod`, `explosionDamage` (`defaultExplosionParams.damage.damageTotal`), `explosionMinRadius`, `explosionMaxRadius`, `explosionPressure`, `volatilityRadiusFactor`, `volatilityDamageFactor`, `volatilityForceFactor` and `volatilityCommodityDamageFactor`. Units are not stated in the records, so the meaning of each unit is not established. `damageGroups[]` merges configurations whose settings are identical into `{configs[], ...settings}`. On LIVE all three (`Base`, `Mole`, `Prospector`) are identical, so there is one group.

`byGroup` maps each top group to its resource names. `summary` is `{resources, items, damage}`.

```json
{"name":"Ore_Gold","recordName":"ResourceType.Ore_Gold","displayName":"Gold (Ore)",
 "nameKey":"items_commodities_gold_ore","descriptionKey":"items_commodities_gold_ore_desc",
 "group":"Metal","subgroups":["UnrefinedOres"],"densityGramsPerCc":19.32,"refinesTo":"Gold",
 "quality":{"profile":"RareShipMineable_QualityDistribution_Default","tier":"Rare",
  "distribution":{"min":501,"max":1000,"mean":500,"stddev":163},
  "locationOverrides":[{"location":"Pyro System","min":501,"max":1000,"mean":500,"stddev":173,"steps":["..."]}],
  "bands":[{"start":0,"end":399,"value":264},{"start":400,"end":599,"value":553},"..."],
  "steps":[{"start":501,"end":599,"shown":553,"shareEstimate":0.45807517},"..."]},
 "groupLabel":"Metal","subgroupLabel":"Unrefined Ores","tradable":true,"raw":true,
 "cargo":[{"className":"gold_ore","kind":"Metal","capacityScu":0.01}],
 "minedAs":[{"element":"gold_ore","oreKey":"gold"}]}
```

## Derived, estimated and curated values

- **Estimate:** `quality.steps[].shareEstimate` and `locationOverrides[].steps[].shareEstimate`. `_assumptions[0]` says: "rolls are integers drawn from a normal curve (mean, stddev) truncated to min-max; a band covers start-0.5 to end+0.5". The records give the curve's numbers, not how the game samples it. Shares are rounded to 1e-8 and omitted when there is no stddev.
- Derived from records (no assumption): `groupLabel`, `subgroupLabel`, `tradable`, `raw`, `rawSources`, `volatile`, `densityIsDefault`, `cargo`, `minedAs`, `steps` (without the share), `damageGroups`, `byGroup`, `tier`.
- `densityIsDefault` marks 1.0 as "the engine default, not a measurement" (code comment, `commodities.rs` line 130). Of the 207 resources, 165 have it, including every refined metal (`Gold` is 1.0 and `Ore_Gold` 19.32).
- `tier` comes from the profile record's name, not from a field.
- `rawSources` and `cargo` join by name, case-insensitively.
- Nothing here is curated or heuristic.

## Redaction

`PROSE_FIELDS` (`crates/sc-gamedata/src/localization.rs`, `redaction` module, line 445) lists `description` for `game-commodities.json`. Publish removes every `description` key at any depth, from resources and items, and adds `_redacted: ["description"]` after `_build`. `nameKey` and `descriptionKey` stay, so the text is one string-table lookup away. The published string table keeps name keys only, so `*_desc` keys are not in it. Consumers fill the text from the player's own `global.ini`.

## Consumers

- **starorg-codex** (`~/git/starorg-strings`, branch `feat/dataset-reuse`):
  - `crates/starorg-mcp/src/prebuilt.rs:158` (DumpUse) needs top-level `resources`, `items`, `damage`, `byGroup` and `summary`, allows `description` to be stripped, schema 1, and checks the type with `typed_commodities` (line 139, `query::quality::Commodities`).
  - `prebuilt.rs:363`: `fill_prose` refills `description` from `descriptionKey` with `domains::commodities::description_of`, so the stub rule must stay the same on both sides.
  - `crates/starorg-gamedata/src/query/commodities.rs:32-79`: `resources[]` `name`, `recordName`, `displayName`, `description`, `group`, `subgroups`, `densityGramsPerCc`, `refinesTo`, `volatility{volatility, healthDecayPerSecond}`, `quality`; `items[]` `name`, `recordName`, `displayName`, `type`, `resource`, `capacityScu`; `damage[]` `name`, `minimumSpeed`, `speedSquaredToDamage`, `gracePeriod`, `explosionDamage`, `explosionMinRadius`, `explosionMaxRadius`, `explosionPressure`.
  - `crates/starorg-gamedata/src/query/quality.rs:16-73`: `resources[]` `name`, `recordName`, `displayName`, and `quality.tier`, `distribution{min,max,mean,stddev}`, `locationOverrides[]{location,min,max,mean,stddev}`, `bands[]{start,end,value}`; `items[]` `name`, `resource`.
  - `crates/starorg-mcp/src/tools.rs:561` and `2023` deserialize the whole file.
- **sc-gamedata itself:** `src/query/commodities.rs` and `src/query/quality.rs` read the same fields, as above.
- **SCMCP:** not in `src/versions.ts` `DATASETS`. `src/localdata.ts` lists and searches any `game-*.json` in `SCMCP_GAME_DATA_DIR`, treating each top-level array or object as a collection. That includes `_assumptions`, which is not in its `META_KEYS`.
- No reads found in snares `scripts/build-data.mjs`, MCsim `scripts/*.py`, Bindscape `scripts/import_sc_*.js` or sc-telemetry `sctelemetry/*.py`.
- Contracts to keep:
  - [consumer-datasets](../plans/specs/consumer-datasets.md) has no commodity-specific rule. The sc-telemetry rules apply to every dataset: `_build` equals the tag, and its `_schema` stem equals the file stem.
  - From [dump-reuse](../plans/specs/dump-reuse.md) (decision 2, option b): `description` stays stripped and `descriptionKey` must stay on every record that had a description.
  - `_schema: "game-commodities@1"` after `_build` (since tags 4.10.201.48085 / 5.0.202.35026), and `build.json` `extractor` plus `files[]` hashes (same tags).

## Derivable data

Records the game already ships, and what more could be derived from them:

- **Fields the source carries but the dataset does not read:**
  - `ResourceType.defaultCargoContainers`: the carryable box entity per box size. Example for `ResourceType.Gold`: `oneSCU` → `entities/scitem/carryables/tractorbeamonly/carryable_tbo_fl_1scu_commodity_metal_gold.json`, and `one_eighthSCU` → `carryable_2h_fl_eighthscu_commodity_metal_gold.json`. 194 of 207 resources name boxes for 1-32 SCU, and 26 also name a 1/8 SCU box. Today `cargo[]` covers only `entities/commodities` (131 resources), so the boxes players actually move are missing. `validateDefaultCargoBox` (true on 200, false on 7) comes with it.
  - `ResourceType.rttThumbnailEntityClass`: the thumbnail entity, set on 49 resources (e.g. `thumbnailentity_resource_rock_gold.json`). `defaultThumbnailPath` is an asset path, which the house rule against assets probably excludes.
  - `ResourceContainer` on cargo items: `immutable`, `generateRandomQuality` and the composition `weight` (`gold_ore`: `immutable: true`, `generateRandomQuality: false`, `weight: 1.0`). What `generateRandomQuality` changes is not established.
  - `CommodityDamageConfiguration.defaultExplosionParams`: `damage.damageMacro` (`damagemacro.highexplosivedamage.json`), `minPhysRadius` 2.5, `maxPhysRadius` 5.0, `friendlyFire` `Team`, and `volatilityParticleStrengthFactor` 1.0. The macro names the damage type, which `game-fire` could join.
- **Joins with other datasets:**
  - Reverse links from `game-blueprints` `resourceUse` (26 resources, e.g. `Aslarite: 872`) give a per-resource `usedByBlueprints` count. The app's commodity detail would use it.
  - `game-cargo` `manifests[].resources[].resource` resolves for all 29 distinct names. That gives a reverse `inManifests[]` (NPC and mission cargo that carries the resource). Today the cargo dataset holds names only, so `displayName` and `volatile` could be joined in.
  - `resources[].quality.locationOverrides[].location` (`Pyro System`) against `game-mining-spawns` `oreSpawns[].system`: which belts roll the override curve. The per-rock `qualityScale` effect stays out: [dump-enhancements](../plans/specs/dump-enhancements.md) item 6 deferred it because the files don't say how it applies.
  - `items[].capacityScu` × `densityGramsPerCc` would give a mass per item. **Estimate:** that SCU maps to a fixed volume is an assumption the records don't state.
- **Parse gaps:**
  - `items[].type` holds prose for 2 items. `Special_HolidayLetter` has `displayType: "@items_commodities_special_lunar_envelope_1_dog_a_desc"` in its record, a description key, so `type` carries the whole "A gilded red envelope…" paragraph. `Special_HolidayBox` does the same (`@items_commodities_special_holidaybox_desc`). `type` is not in `PROSE_FIELDS`, so this text is published. A fix could drop a `displayType` whose key ends in `_desc`, or redact `type` when it is over 120 characters.
  - `Special_HolidayLetter` has an empty `defaultComposition`, so `resource` is null. That is what the record says, not a parse error.
  - `Raw_Diamond` has `quality` without `bands` or `steps`. Its `ResourceTypeCraftingData` has `qualityQuantization: null`, and no rock composition names `diamond_raw`. Both are record facts worth flagging (e.g. `summary.qualityWithoutBands`).
  - `tradable` only checks the group name. `LifeSupportPlaceholder` is caught because it ends in `placeholder`, but a future placeholder group with another name would not be.

Related: [dump-enhancements](../plans/specs/dump-enhancements.md) items 6 and 7, and [dump-reuse](../plans/specs/dump-reuse.md).
