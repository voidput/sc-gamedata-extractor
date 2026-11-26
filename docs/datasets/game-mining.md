---
title: game-mining (game-mining.json)
tags: [docs, datasets]
updated: 2026-11-25
---

# game-mining (game-mining.json)

The physics of breaking rock, per mineable element: instability, resistance, the optimal charge window, explosion multiplier and cluster factor. It also holds the laser modifiers of mining modules (`MiningLaserModifier`). On LIVE 4.10.201.48085 it has 46 mineable elements, 27 mining modules and 42 `elementSignatures` keys. Published size: 23,822 bytes minified (`jq -c . | wc -c`). Publish strips nothing here, so the parse output and the published file are the same size. The `_schema` key (since tags 4.10.201.48085 / 5.0.202.35026, commit `b9e3700d0`) is not in that figure; with it the same command prints 23,848.

## Source

- Records (filter `**/mining/**`):
  - `libs/foundry/records/mining/mineableelements/*.json`: `MineableElement`, which references a `resourceType`.
  - Every other record on a `/mining/` path that holds a non-null `MiningLaserModifier` anywhere in its value. On LIVE these are the module entities under `entities/scitem/ships/utility/mining/miningarm/`.
- Rust: [`crates/sc-gamedata/src/domains/mining.rs`](../../crates/sc-gamedata/src/domains/mining.rs).
  - `impl Domain` at line 189, filters at line 198, `parse` at 202.
  - `parse_mineable_element` at 47.
  - `collect_laser_modifiers` at 130, `parse_laser_modifiers` at 91, `parse_modifier` at 68, `merge_modifiers` at 153 (the first defined value wins), `parse_mining_module` at 174.
- No other dataset is read.

## Shape

Top-level keys: `_source`, `_extracted`, `_build`, `mineableElements[46]`, `miningModules[27]`, `elementSignatures{42}`, `summary{mineableElements, miningModules}`.

### mineableElements[]

| Field | Type | Meaning | Units/range | Source |
|---|---|---|---|---|
| `name` | string | Short name of the element's `resourceType` (`Raw_Quantainium`), else the record short name | not unique: see Derivable data | `resourceType` |
| `recordName` | string | `MineableElement.<record>` (`MineableElement.Quantainium_Raw`). Lower-cased, its short name is the element key in [game-mining-spawns](game-mining-spawns.md) | | `_RecordName_` |
| `id` | string | | | `_RecordId_` |
| `instability` | number | | 0-1000 on LIVE (Quantainium 1000); unit not stated | `elementInstability` |
| `resistance` | number | | -1 to 1 on LIVE (Quantainium 0.95) | `elementResistance` |
| `optimalWindowMidpoint` | number | | 0.3-1 | `elementOptimalWindowMidpoint` |
| `optimalWindowRandomness` | number | | 0-0.3 | `elementOptimalWindowMidpointRandomness` |
| `optimalWindowThinness` | number | | -1 to 10 (Quantainium 2.3) | `elementOptimalWindowThinness` |
| `explosionMultiplier` | number | | -1000 to 260 (Quantainium 260) | `elementExplosionMultiplier` |
| `clusterFactor` | number | | 0-1 | `elementClusterFactor` |

The records give no units for these values, and how the game combines them is not established. The `MiningGlobalParams` they combine with are not in this dataset (see Derivable data).

### miningModules[]

`name`, `recordName`, `id`, and `modifiers`, an object holding only the keys a module defines, in this order:
- `laserInstability`, `resistanceModifier`, `optimalChargeWindowSizeModifier`, `optimalChargeWindowRateModifier`, `catastrophicChargeWindowRateModifier`, `shatterDamageModifier` (the record spells it `shatterdamageModifier`) and `clusterFactorModifier`. Each is `{value, kind, showInUI?}`, where `kind` is `multiplicative` or `additive` from the wrapper `_Type_` (`FloatModifierMultiplicative`), else `unknown`.
- `isOptimalRateGood`: a bool.

On LIVE the 27 modules carry: `isOptimalRateGood` 27 (`true` on all of them), `optimalChargeWindowSizeModifier` 13, `optimalChargeWindowRateModifier` 8, `resistanceModifier` 7, `laserInstability` 4, `catastrophicChargeWindowRateModifier` 4, `shatterDamageModifier` 4 and `clusterFactorModifier` 0 (no module sets it). Of the 40 modifier objects, 39 are `multiplicative`, 1 is `additive` and none is `unknown`.

The unit of `value` (e.g. `15.5`, `-30`) is not established.

### elementSignatures

`{<name>: {resistance, instability}}`, keyed by `mineableElements[].name`.

```json
{"name":"Mining_Modules_Active_Brandt","recordName":"EntityClassDefinition.Mining_Modules_Active_Brandt",
 "id":"18a62ae8-e1aa-453f-8631-df928a71abda",
 "modifiers":{"resistanceModifier":{"value":15.5,"kind":"multiplicative","showInUI":true},
  "shatterDamageModifier":{"value":-30,"kind":"multiplicative","showInUI":true},"isOptimalRateGood":true}}
```

## Derived, estimated and curated values

- `kind` on each modifier is derived from the wrapper type name.
- `elementSignatures` is a flat copy of `resistance` and `instability`, keyed by `name`.
- Nothing is estimated, curated or heuristic. There is no `_assumptions`.

## Redaction

Nothing. `game-mining.json` is not in `PROSE_FIELDS`.

## Consumers

- **SCMCP:** `src/versions.ts:36` registers `mining: { file: "game-mining.json", collection: "mineableElements" }` as the default collection for cross-patch diffs. `src/localdata.ts` (tested in `src/localdata.test.ts:15-136`) lists, searches and pages `mineableElements` by name. Keep the key `mineableElements` and keep it an array.
- **starorg-codex** (branch `feat/dataset-reuse`):
  - `crates/starorg-mcp/src/prebuilt.rs:169` needs top-level `mineableElements`, `miningModules`, `elementSignatures` and `summary`, schema 1, with no typed check.
  - `crates/starorg-gamedata/src/query/mining.rs:38-49` (`ElementIn`) reads `name`, `recordName`, `instability`, `resistance`, `optimalWindowMidpoint`, `optimalWindowRandomness`, `optimalWindowThinness`, `explosionMultiplier` and `clusterFactor`. The comment there says `recordName`'s short name is the spawn data's ore key, so keep the `MineableElement.<file stem>` form.
  - `crates/starorg-mcp/src/tools.rs:2116` and `2141` load it for the Mining tab.
  - `app/src-tauri/src/contract/mock_commodities.rs:182` mocks it.
- No reads in snares, MCsim, Bindscape or sc-telemetry.
- Contracts to keep: the SCMCP collection name, the starorg `needs` keys and the `recordName` form above, plus the general `_build` rule in [consumer-datasets](../plans/specs/consumer-datasets.md). `_schema: "game-mining@1"` (since tags 4.10.201.48085 / 5.0.202.35026), and `build.json` `extractor` plus `files[]` hashes (same tags).

## Derivable data

- **Parse gaps:**
  - **4 modules dropped.** `mining_modules_passive_fltr_mk1/2/3` and `mining_modules_vehiclemod_argogeo` (in `entities/scitem/ships/utility/mining/miningarm/`, 31 module files there against 27 output) are missing. Their `MiningLaserModifier` holds only `isOptimalRateGood: true`, and `collect_laser_modifiers` skips a struct whose values are all booleans (line 140). FLTR MK1 carries `MiningFilterItemModifierParams.filterParams.filterModifier` (`FloatModifierMultiplicative`, value 20) plus a `weaponStats.damageMultiplier` of 0.85 (on `fireActionIndex` 1), and neither is read. ArgoGeo has `damageMultiplier` 0.15 (on `fireActionIndex` 0).
  - **`elementSignatures` collapses names.** `name` is the resource name, which 7 records share in 3 groups:
    - `Aphorite`: `MinableElement_FPS_Aphorite` resistance 0, `MinableElement_FPS_Flowstone` resistance -1, `MinableElement_FPS_template` resistance 0;
    - `Beradom`: `MinableElement_GroundVehicle_Beradom` resistance 0, `MinableElement_GroundVehicle_template` resistance 1;
    - `Carinite`: FPS and GroundVehicle.

    `Map::insert` keeps the first key's position but the last value, so `elementSignatures.Beradom` is `{resistance: 1}`, the template's value, not the real element's 0. Keying by `recordName` (or the element file key) would fix this.
  - **Templates are listed as elements.** `MinableElement_Ship_template` (shown as `Aluminum`), `_FPS_template` and `_GroundVehicle_template` appear in `mineableElements`. No rock composition names them (checked against `game-mining-spawns` `rocks[].ores[].element`), nor does it name `diamond_raw`. A `template: true` or `inCompositions: false` flag would say so. `TestElement_Balance` (shown as `Copper`) is a test record too; one composition names it (`asteroid_qtype_test.json`).
- **Fields the source carries but the dataset does not read:**
  - `MiningGlobalParams` (7 records under `mining/`: base, `Ship`, `GroundVehicle`, `FPS`, `FPS_Organic`, `FPS_Obstacle`, `Flowstone_Test`). Each rock entity's `MineableParams.globalParams` points to one of them, and the values differ by kind. Ship has `powerCapacityPerMass` 10, `optimalWindowSize` 0.1, `resistanceCurveFactor` 0.6, `cSCUPerVolume` 0.7 and `defaultMass` 100. Ground vehicle has 4, 0.4, 0.33, 3.0 and 0.001. The records also carry `decayPerMass`, the breaking fill and decay rates, `absorbableVolumeThreshold`, `mineableInstabilityParams` (`instabilityWavePeriod` 3), `modifierPersistenceTime`, the gadget thresholds and `wasteResourceType` (`ResourceType.InertMaterials`). These are the constants a break-chance or charge simulation needs, so the starorg Mining tab's grading (`query/mining.rs`) would use them.
  - `MiningLaserGlobalParams` (`mininglaserglobalparams.json`, `_grinroc.json`): throttle step and acceleration (`throttleChangePerAction` 0.01, `throttleAccFactor` 2.0).
  - Module entities beyond `MiningLaserModifier`:
    - `EntityComponentAttachableModifierParams.charges`: Brandt has 5.
    - `modifiers[].modifierLifetime.lifetime`: 60 s on active modules.
    - `modifiers[].weaponModifier.weaponStats.damageMultiplier`: each module has two `ItemWeaponModifiersParams` entries, `fireActionIndex` 0 and 1. Brandt has 1.35 on index 0 and 1.0 on index 1; Focus MK1 has 0.85 and 1.0; FLTR MK1 has 1.0 and 0.85. Which laser mode each fire action index is, is not established in these records.
    - `SAttachableComponentParams.AttachDef`: `Type` `MiningModifier`, `SubType` `Gun`, `Size` 1, `Grade` 1.
    - `SCItemPurchasableParams.displayName` (`@item_Mining_Modules_Passive_FLTR_MK1`).

    Damage multiplier, charges and lifetime are per-module numbers the records ship and the dataset drops.
- **Joins with other datasets:**
  - `miningModules[].name` → `game-item-tags` `items[].className` (32 `Mining_Modules*` rows there, with `name`, `manufacturer` and `nameKey`) gives display names without a new read.
  - `mineableElements` → `game-mining-spawns` `rocks[].ores[].element` (by lower-cased `recordName` short name; every composed element matches) gives, per rock, the hardest element to break, i.e. its max `resistance` and `instability`. **Estimate:** that a rock's difficulty is set by its hardest element is an assumption the records don't state.
  - `mineableElements` → `game-commodities` `resources[]` by `name` (the resource) gives the quality tier per element.

Related: [dump-enhancements](../plans/specs/dump-enhancements.md) item 5 (one ore key), [game-mining-spawns](game-mining-spawns.md) and [game-commodities](game-commodities.md).
