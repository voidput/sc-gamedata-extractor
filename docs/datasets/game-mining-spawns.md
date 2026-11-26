---
title: game-mining-spawns (game-mining-spawns.json)
tags: [docs, datasets]
updated: 2026-11-25
---

# game-mining-spawns (game-mining-spawns.json)

Where ores spawn. No single record links a place to an ore, so the domain walks a provider preset (one per body, belt or event) through its spawn groups, harvestable presets, rock entities and rock compositions down to the mineable elements. It then indexes the result per rock and per ore, and adds the hand-placed spots from loot-area slot presets. On LIVE 4.10.201.48085:
- 49 locations, 45 of them with ores and 27 named by a body;
- 1,014 spawn entries;
- 268 rocks, 260 with an RS signature;
- 801 `oreSpawns` rows;
- 33 ores in `oreLocations`;
- 41 ore keys, 33 with belts and 17 with spots.

Published size: 856,304 bytes minified (`jq -c . | wc -c`). Publish strips nothing here, so the parse output and the published file are the same size. The `_schema` key (since tags 4.10.201.48085 / 5.0.202.35026, commit `b9e3700d0`) is not in that figure; with it the same command prints 856,337.

## Source

- Records (filters `**/harvestable/**`, `**/entities/mineable/**`, `**/mining/**` and `**/starmap/**`):

  ```text
  HarvestableProviderPreset   harvestable/providerpresets/**/hpp_<key>.json  (harvestableGroups[])
    -> HarvestableElement       groupName, groupProbability, harvestables[].relativeProbability
      -> HarvestablePreset      harvestable/harvestablepresets/<file>.json  (entityClass)
        -> EntityClassDefinition  entities/mineable/<rock>.json  (MineableParams, SSCSignatureSystemParams)
          -> MineableComposition  mining/rockcompositionpresets/**  (compositionArray[], depositName)
            -> MineableElement    mining/mineableelements/<element>.json  (resourceType)
  ```

  - Star map records give a location's body, system and path when the provider key names a star map object (26 locations).
  - Loot-area slot presets (through `item_sources::loot_areas`) give the `spots`.
- Rust: [`crates/sc-gamedata/src/domains/mining_spawns.rs`](../../crates/sc-gamedata/src/domains/mining_spawns.rs).
  - `impl Domain` at line 608, filters at line 617, `parse` at 630.
  - `describe_provider` at 73, `describe_rock` at 556, `rock_signature` at 549 (slot 4 of the radar `baseSignatureParams.signatures`), `kind_of` at 534.
  - `system_name` at 339, `belt_place` at 372, `ore_belts` at 414, `ore_spots` at 461.
  - `mined_elements` at 329 (also used by [game-commodities](game-commodities.md) `minedAs`).
  - `ore_core` and `area_place` are in `src/query/mined.rs` at 115 and 136.
- Derived from: `domains::starmap::build_locations` (the `game-starmap` locations) and `domains::item_sources::loot_areas` (the `game-item-sources` areas).

## Shape

Top-level keys: `_source`, `_extracted`, `_build`, `_assumptions[4]`, `locations[49]`, `rocks[268]`, `oreSpawns[801]`, `oreLocations{33}`, `ores{41}`, `summary{10}`.

`summary` keys: `locations`, `locationsWithOres`, `locationsNamed`, `distinctOres`, `spawnEntries`, `rocks`, `rocksWithSignature`, `oreKeys`, `oreKeysWithBelts`, `oreKeysWithSpots`.

### locations[]

| Field | Type | Meaning | Units/range | Source |
|---|---|---|---|---|
| `key` | string | Provider key, `HPP_` stripped (`Stanton2c_Belt`, `Pyro5e`) | 49 | record short name |
| `recordName` | string | `HarvestableProviderPreset.HPP_<key>` | | record |
| `system` | string? | Star map system, else the key's stem when it starts with a system name. `AaronHalo` → `Stanton` is curated | absent on 14 (Lagrange, AsteroidCluster, ResourceRush, ShipGraveyard, SpaceDerelict) | star map / heuristic |
| `body` | string? | Localized body name (the key is a string key), else the star map name | 27 | string table / star map |
| `place` | string | Readable place: the body, else built from the key | | `belt_place` |
| `placeSource` | string | `data` (body) or `heuristic` (built from the key) | 27 / 22 | derived |
| `starmapRecord`, `path` | string, string[]? | Matching star map record and the chain above it | 26 | star map |
| `entries` | object[] | Spawn entries (below) | 1,014 total | `harvestableGroups[].harvestables[]` |
| `ores` | string[] | Element keys reachable here | empty on 4 | derived |

`entries[]`:

| Field | Type | Meaning | Units/range | Source |
|---|---|---|---|---|
| `group` | string | Group name | 9 names, incl. `SpaceShip_Mineables`, `FPS_Mineables`, `Salvage_BrokenShips_*`, `Harvestables`/`Havestables` | `groupName` |
| `groupProbability` | number? | | 0.00033-45; scale differs by kind (see `_assumptions[0]`) | `groupProbability` |
| `relativeProbability` | number? | Weight within the group | | `relativeProbability` |
| `spawnShare` | number? | groupProbability × relativeProbability / sum of the group's weights | rounded 1e-5; max 35 | derived |
| `harvestable` | string | Harvestable preset file stem | | `harvestable` or `harvestableEntityClass` |
| `rock`, `kind`, `tier`, `deposit`, `signature`, `ores` | | As on `rocks[]` | absent when the chain does not reach a rock (663 entries) | resolved chain |

### rocks[]

| Field | Type | Meaning | Units/range | Source |
|---|---|---|---|---|
| `rock` | string | Rock entity class | 268 | entity short name |
| `kind` | string? | `ship` (235), `vehicle` (6), `hand` (26), from the `globalParams` file name | 1 absent | `MineableParams.globalParams` |
| `tier` | string? | `Common`..`Legendary`, from the composition name's prefix | 55 rocks | composition short name |
| `composition` | string | Composition short name | | `MineableParams.composition` |
| `deposit` | string | Localized deposit name | | `depositName` |
| `signature` | number? | RS signature on scan | 260 rocks; e.g. 4270 | `SSCSignatureSystemParams…signatures[4]` (0 = none) |
| `minimumDistinctElements` | number? | | e.g. 2 | `minimumDistinctElements` |
| `ores[]` | object[] | `{element, minPercent, maxPercent, probability, qualityScale}` | percent 0-100; `probability` 0-1 | `compositionArray[]` |
| `spawnsAt` | string[] | Bodies or keys whose entries reach this rock | empty on 212 rocks | derived |

### oreSpawns[], oreLocations, ores

- `oreSpawns[]`: one row per composition part per spawn entry. Fields: `ore` (element file stem), `oreKey`, `resource` (the element's ResourceType), `location`, `system`, `body`, `rock`, `kind`, `tier`, `signature`, `spawnShare`, `minPercent`, `maxPercent` and `qualityScale` (not 1 on 444 of 801 rows). By kind: ship 672, vehicle 54, hand 75. 147 rows have no system.
- `oreLocations`: `{element: [body or key, …]}`. Starorg's Dev table reads it as an index.
- `ores`: `{oreKey: {elements[], resources[], belts[], spots[]}}`.
  - `belts[]` (567 total) are summed per (location, rock): `location`, `place`, `placeSource`, `system`, `rock`, `kind`, `tier`, `spawnShare`, `minPercent` and `maxPercent`.
  - `spots[]` (253 total): `area`, `rock`, `slotShare` (0-1, rounded 1e-8), `configs[]`, and for 12 spots `place` with `placeSource: "curated"`.
  - Ore keys with no belt: `carinite`, `carinitepure`, `flowstone`, `jaclium`, `sadaryx`, `saldynium`, `testelementbalance` and `vlklimpet`.

```json
{"ore":"agricium_ore","oreKey":"agricium","resource":"Ore_Agricium","location":"AaronHalo","system":"Stanton",
 "body":"Aaron Halo","rock":"MineableRock_AsteroidUncommon_Aslarite","kind":"ship","tier":"Uncommon",
 "signature":3840,"spawnShare":0.0426,"minPercent":2,"maxPercent":5,"qualityScale":0.789}
```

## Derived, estimated and curated values

`_assumptions` has four entries (`MINING_ASSUMPTIONS`, line 285):

1. **Estimate:** `ores[].belts[].spawnShare` sums groupProbability × share of group over a location's groups, once per spawn entry. Group probabilities are not normalised across mining kinds (hand groups are in the tens, ship groups below 1), so compare it only within one kind.
2. `belts[].minPercent`/`maxPercent` are the lowest and highest bound over the parts of a rock that hold the ore, not a total. 55 of 268 rocks list an element twice.
3. **Heuristic:** `place` with `placeSource: "heuristic"` is built from the key (`Lagrange_B` → `Lagrange points`, `Pyro_Cool01` → `Pyro asteroid fields`, `Stanton2c_Belt` → `Yela Belt`). **Curated:** spots with `placeSource: "curated"` come from `area_place` (`rockcracker` → `Rock Cracker, Nyx`).
4. **Heuristic/curated:** `system` comes from the star map when the key names a body, else from the key stem; `AaronHalo` → `Stanton` is curated, and Lagrange, AsteroidCluster and ResourceRush keys have no system.

Also derived, with no assumption: `entries[].spawnShare` (the per-entry share before summing), `kind` (from the global-params file name), `tier` (from the composition name), `oreKey` (`ore_core`: `MinableElement_FPS_Sadaryx` → `sadaryx`), `spawnsAt`, `oreLocations` and `summary`.

## Redaction

Nothing. `game-mining-spawns.json` is not in `PROSE_FIELDS`. `deposit` and `body` are localized names.

## Consumers

- **SCMCP:** `src/versions.ts:37` registers `"mining-spawns": { file: "game-mining-spawns.json", collection: "locations" }` as the default cross-patch diff collection. Keep `locations` an array of records.
- **starorg-codex** (branch `feat/dataset-reuse`):
  - `crates/starorg-mcp/src/prebuilt.rs:170` needs top-level `locations`, `rocks`, `oreSpawns`, `oreLocations` and `summary`, schema 1, and checks the type with `typed_mining_spawns` (line 136), which deserialises `query::mined::MiningSpawns`.
  - `crates/starorg-gamedata/src/query/mined.rs:17-61` reads:
    - `rocks[]` `rock`, `kind`, `tier`, `signature`, and `ores[]{element, minPercent, maxPercent, probability, qualityScale}`;
    - `oreSpawns[]` `ore`, `location`, `system`, `body`, `rock`, `kind`, `tier`, `signature`, `spawnShare`, `minPercent`, `maxPercent` and `qualityScale`.
  - `crates/starorg-mcp/src/tools.rs:554` loads it. `app/src/components/commodities/MiningDetailView.svelte:21` and `app/src/lib/contract/types.ts:885` gate the Mining view on it.
  - The app's `crates/starorg-mcp/src/hints.rs` `system_name`/`belt_place` are due to be replaced by the dump's `system` and `place` ([dump-enhancements](../plans/specs/dump-enhancements.md), "Follow-ups").
- **sc-gamedata itself:** `src/query/mined.rs` (same fields) and `domains::commodities` (`mined_elements`).
- No reads in snares, MCsim, Bindscape or sc-telemetry.
- Contracts to keep:
  - `oreLocations` stays a list of names (Dev table index; dump-enhancements item 5 deviation).
  - `spawnShare` is not normalised across kinds.
  - The SCMCP and starorg keys above, and the general `_build` rule in [consumer-datasets](../plans/specs/consumer-datasets.md).
  - `_schema: "game-mining-spawns@1"` (since tags 4.10.201.48085 / 5.0.202.35026), and `build.json` `extractor` plus `files[]` hashes (same tags).

## Derivable data

- **Parse gaps:**
  - **663 of 1,014 entries resolve to no rock.** By group: `Salvage_BrokenShips_Elite`/`Normal`/`Poor` 198 each, `Salvage_FreshDerelicts` 48, `Harvestables` 13, `Havestables` 6, `SpaceShip_Mineables` 2.
    - The salvage entries point at `salvageablerepairable_shipdebris_*` presets, present at 22 belt and cluster locations.
    - The plant entries (`plant_decari`, `plant_prota`, …, at Pyro1, Pyro2, Pyro4, Stanton1, Stanton1a, Stanton2b and Stanton4) point at entities under `entities/scitem/harvestables/`, outside the `entities/mineable` filter. `harvestable_base_decari` has `HarvestableParams.looseSubConfigBase.subHarvestables[0].harvestable` → `harvestablepresets/fruit_decari.json`.

    Following these would give where salvage debris and gatherables spawn, with the same share maths. These are different activities, so they need their own top-level lists, not `oreSpawns`.
  - The 2 unresolved `SpaceShip_Mineables` entries are `mining_asteroidgoldonly` (the ResourceRush Gold locations). Its preset has `entityClass: null` and an empty `subHarvestables`, so the records themselves give no ore. That is why `ResourceRush_Gold` and `ResourceRush_Gold_HighDensity` have no ores, and it is not a parse error.
  - `oreLocations` lists the raw key where there is no body (`Lagrange_B`, `Stanton2c_Belt`, `Pyro_Cool01`) although `locations[].place` already reads `Lagrange points` / `Yela Belt`. The value cannot change (it is a contract), but a parallel `orePlaces` could carry `place`.
  - 241 of 253 `ores[].spots[]` have no `place`. `game-item-sources` `areas[].place` now exists (item 4a), and 54 of the 60 distinct unplaced spot areas have one there (e.g. `cave_aberdeen_medium` → `Caves`, `asteroidbases/asteroidbase_dungeon_001` → `Asteroid bases`). That join is the open point of [dump-enhancements](../plans/specs/dump-enhancements.md) item 5. These places are **heuristic** (item-sources `placeSource`).
- **Fields the source carries but the dataset does not read:**
  - `HarvestableElement.clustering` → `harvestable/clusteringpresets/*.json`. Example: `miningcluster_sml_med` has `probabilityOfClustering` 100 and `clusterParamsArray` `[{relativeProbability 0.2, minSize 10, maxSize 18, minProximity 2, maxProximity 4}, …]`. That gives how many rocks spawn together per entry.
  - `MineableCompositionPart.curveExponent`: 1.0 on 1,165 parts, and 1.5, 2, 2, 3, 4 and 5 on 6 others. The percent roll's shape is not established.
  - `HarvestableProviderPreset.areas[]` (`HarvestableAreaPreset`): biome modifiers on 6 providers (Pyro1, Pyro2, Pyro3, Stanton1, Stanton1a, Stanton4). Example: `debugGroupName: "Deserts"`, `globalModifier` 1.0, and `modifiers[]` that set `harvestableModifier` 0.0 on elements referenced by `_PointsTo_:ptr:N`. Resolving the pointers would say which ores a biome suppresses. Pointer resolution is not done anywhere in the crate yet.
  - `HarvestablePreset.respawnInSlotTime` (3600) and `harvestBehaviour.despawnTimer.despawnTimeSeconds` (600, plus `additionalWaitForNearbyPlayersSeconds` 300). All 207 `mining_*` presets share these values.
  - Rock `MineableParams.filledFactor` (1.0) and `SMineableHealthComponentParams.damageMapParamsCenter.laserDamageFullValue` (2500 on `MineableRock_AsteroidCommon_Iron`). What they change is not established.
- **Joins with other datasets:**
  - `rocks[].ores[].element` → [game-mining](game-mining.md) `mineableElements` (by lower-cased `recordName` short name; every element matches) gives per-rock resistance and instability. The Mining tab grading in starorg `query/mining.rs` would use it. **Estimate** if reduced to one rock difficulty.
  - `oreSpawns[].system` → `game-commodities` `resources[].quality.locationOverrides[].location` (`Pyro System`) gives which quality curve a belt rolls. The `qualityScale` effect on quality stays deferred (the files don't state how it applies; dump-enhancements item 6).
  - `oreSpawns[].resource` → `game-refining` `refinables[].raw` gives the refined product per belt.
- **Rock mass:** the entity's `SEntityPhysicsControllerParams.PhysType.Mass` is 1.0 on the sampled rock, while `MiningGlobalParams.defaultMass` is 100 (ship). A size or SCU per rock would rest on `cSCUPerVolume` and a runtime volume, which is an **estimate** the records cannot finish.

Related: [dump-enhancements](../plans/specs/dump-enhancements.md) items 4a, 5 and 6, and [dump-reuse](../plans/specs/dump-reuse.md).
