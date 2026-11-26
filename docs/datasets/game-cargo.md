---
title: game-cargo (game-cargo.json)
tags: [docs, datasets]
updated: 2026-11-25
---

# game-cargo (game-cargo.json)

What NPC and mission ships spawn carrying (`CargoManifest` records), and the freight elevator and auto-loading timings (`GlobalCargoLoadingParams`). On LIVE 4.10.201.48085 it has 82 manifests in 10 categories and one loading block. Published size: 52,417 bytes minified (`jq -c . | wc -c`). Publish strips nothing here, so the parse output and the published file are the same size. The `_schema` key (since tags 4.10.201.48085 / 5.0.202.35026, commit `b9e3700d0`) is not in that figure; with it the same command prints 52,442.

## Source

- Records:
  - `libs/foundry/records/cargomanifest/**`: 82 `CargoManifest` records, in subfolders such as `civilian`, `criminal` and `legalcargo`.
  - `libs/foundry/records/globalcargoloadingparams/globalcargoloadingdata.json`: one `GlobalCargoLoadingParams`.
- Rust: [`crates/sc-gamedata/src/domains/cargo.rs`](../../crates/sc-gamedata/src/domains/cargo.rs).
  - `impl Domain` at line 236, filters at line 245.
  - `parse_cargo_manifest` at 151, `parse_manifest_resource` at 106, `parse_spawn_rule` at 127.
  - `parse_cargo_loading` at 217, `box_sizes` at 47 (shared with [game-shops](game-shops.md)).
  - `entity_name` (spawn-rule entities) comes from `crate::helpers_ships`.
- No other dataset is read. Resource names are short names of `ResourceType` references; they are not joined to [game-commodities](game-commodities.md).

## Shape

Top-level keys: `_source`, `_extracted`, `_build`, `manifests[82]`, `loading{8}`, `byCategory{10}`, `summary{manifests}`.

### manifests[]

| Field | Type | Meaning | Units/range | Source |
|---|---|---|---|---|
| `name` | string | Manifest short name | | record short name |
| `recordName`, `id` | string | | | record |
| `category` | string | Subfolder under `cargomanifest/`, else the name's first `_` segment, lower-cased | 10 values, e.g. `criminal` 19, `legalcargo` 19 | record path |
| `fillMode` | string? | `Random` (44), `Even` (34) or `Custom` (4) | | `cargoFillCapacity._Type_` minus `CargoFillCapacityValue_` |
| `minCapacity`, `maxCapacity` | number? | Fraction of the cargo grid filled | 0-1; absent on 4 (`LegalCargo_TSG` and the three `ShipHeist_Freelancer_*` Custom manifests) | `cargoFillCapacity.minCapacityRange` / `maxCapacityRange` |
| `resources` | object[] | What fills the hold (below) | 515 rows; empty on 7 manifests | `cargoFillCapacity.resources` |
| `spawnRules` | object[] | Extra items or resources spawned on top (below) | 1 manifest has any | `cargoSpawnRules` |
| `lockCargoGridsOnSpawn` | bool? | | true on 1 of 82; meaning not established | `lockCargoGridsOnSpawn` |

`resources[]`: `resource` (ResourceType short name; 29 distinct, all present in `game-commodities` `resources[].name`), `probability` (0-1), and for `CargoResourceAllocation` entries `minShare` and `maxShare` (the share of the hold, 0-1, from `min/maxResourceAllocationPercentage`). The same resource can repeat: `ShipHeist_Freelancer_Medium` lists Gold 46 times at a share of 0.03, 39 of them at probability 1 and 7 at 0.5.

`spawnRules[]`: `kind` (`entity` or `resource`), `probability`, `items[]` (entity file stems or resource names), and `quantity` or `proportion` (from `quantityAlgorithmType`). Only `LegalCargo_TSG` has rules, two of them: an `entity` rule (probability 0, quantity 30, 2 missile entities) and a `resource` rule (probability 1, proportion 0.035, 12 resources).

### loading

| Field | Type | Meaning | Units/range | Source |
|---|---|---|---|---|
| `cargoDeckLoadingTimePerSCU` | number | Freight elevator time per SCU (code comment) | 0.024; seconds per the code comment | same name |
| `autoLoadingBaseLoadingTime`, `autoLoadingBaseUnloadingTime` | number | | 120 and 120; unit not stated | same name |
| `autoLoadingBoxLoadingTime`, `autoLoadingBoxUnloadingTime` | `{scu, value}[]` | Time per box, by box size | 10 sizes, 0.125-32 SCU; values 0-18 (seconds per the code comment) | `autoLoadingBoxSize*Time`, keys `one_eighthSCU`..`thirtyTwoSCU` |
| `revokeTimeDelay` | number | Seconds before an unclaimed loading area is revoked (code comment) | 60 | same name |
| `forfeitTimeBuffer` | number | Grace seconds before cargo left in a loading area is forfeit (code comment) | 300 | same name |
| `uiTimeRemainingForTimeOutWarning` | number | | 120 | same name |

`byCategory` maps each category to its manifest names.

```json
{"name":"BlockadeRunner_Caterpillar_45-50","recordName":"CargoManifest.BlockadeRunner_Caterpillar_45-50",
 "id":"2057d681-0232-41e7-8536-346e090e401f","category":"blockaderunner","fillMode":"Even",
 "minCapacity":0.45,"maxCapacity":0.5,"resources":[{"resource":"ProcyonCanister_Time","probability":1}],
 "spawnRules":[],"lockCargoGridsOnSpawn":false}
```

## Derived, estimated and curated values

- `category` is taken from the folder, or from the name when the record sits at the top level.
- `fillMode` is the wrapper type name with its prefix cut.
- Box-size lists are ordered by SCU; the SCU values come from a fixed key table (`BOX_SIZES`, line 27).
- Nothing is estimated, curated or heuristic. There is no `_assumptions`.

## Redaction

Nothing. `game-cargo.json` is not in `PROSE_FIELDS`, and the file holds no prose.

## Consumers

- **starorg-codex** (branch `feat/dataset-reuse`): `crates/starorg-mcp/src/prebuilt.rs:157` needs top-level `manifests`, `loading` and `summary`, schema 1, with no typed check. No typed reader of manifest fields was found. Dev mode shows whole records (shape fixture `crates/starorg-core/tests/fixtures/dev-shapes/game-cargo.json`).
- **SCMCP:** not in `src/versions.ts` `DATASETS`. It is reachable through the generic `src/localdata.ts` listing and search.
- No reads in snares, MCsim, Bindscape or sc-telemetry. sc-telemetry's `cargo`/`cargo_scu` come from `game-ships`, not this file.
- Contracts to keep: only the general ones from [consumer-datasets](../plans/specs/consumer-datasets.md) (`_build` equals the tag) and the starorg `needs` keys above. `_schema: "game-cargo@1"` (since tags 4.10.201.48085 / 5.0.202.35026), and `build.json` `extractor` plus `files[]` hashes (same tags).

## Derivable data

- **Fields the source carries but the dataset does not read:**
  - `CargoManifest.descriptionTags.tags[]`: tag references that `tagdatabase` resolves to names. Examples: `Tag.2b3c1abe-…` → `General` (39 manifests), `Tag.4b9a647c-…` → `Legal` (19), `Tag.ea9a5d42-…` → `Mixed` (14), and one tag per manifest named after the manifest (`Tag.c3fd2d41-…` → `BlockadeRunner_Caterpillar_45-50`). The tags are the likely handle missions or spawners use to pick a manifest, but no referencing record turned up in the extract, so that meaning is not established.
  - `CargoManifest.tags[]` (top level, separate from `descriptionTags`): set on 24 of 82 manifests with 1-4 tag references each, empty on the other 58. They resolve to commodity-like tag names: `ProcessedFood` (13 manifests), `ConsumerGoods` (9), `Stims` (7), `DistilledSpirits` (7), `MedicalSupplies` (6), `Scrap` (6). Example: `civilian_01_medium_001.json` → `Scrap`, `ProcessedFood`, `ConsumerGoods`. What the game uses them for is not established.
  - `GlobalCargoLoadingParams`: `initialMovementHintTimeBuffer` 10, `uiDisplayTimeLimit` 15, `vehicleMovementTimeLimit` 1, `vehicleVelocityEpsilon` 0.01. The rest are notification references.
- **Joins with other datasets:**
  - Each manifest resource against `game-commodities` `resources[]` (by `name`; all 29 resolve): `displayName`, `volatile`, `densityGramsPerCc` and `tradable` per row. That would show which NPC holds carry volatile cargo without a second file.
  - **Estimate:** `cargoDeckLoadingTimePerSCU` × `game-ships` `totals.cargoScu` gives elevator seconds for a full hold, and the box tables with [game-shops](game-shops.md) `commodities.autoLoadingBoxPrices` give time and aUEC per box size. That the times add linearly per SCU or per box is an assumption the records don't state.
  - Expected share per resource for `Random` manifests and for `Custom` allocations (sum of `minShare`..`maxShare` × `probability`). The `Random` probabilities sum to 1 on 39 manifests and to 3 on 3 (e.g. `IllegalCargo_Generic`); 2 have no resources. So the probabilities are not always a distribution, and **estimate:** how the game draws from these lists is not stated.
  - Ship ↔ manifest: names like `BlockadeRunner_Caterpillar_45-50` embed a ship. **Heuristic:** matching that to `game-ships` by name is not a record link.
- **Parse gaps:**
  - The `CargoFillCapacityValue_Custom` manifests have no capacity range in the record (`cargoFillCapacity` carries only `resources`), so `minCapacity`/`maxCapacity` are absent for 4 manifests. This is faithful to the records.
  - 7 manifests have no resources (`CargoTest`, `LegalCargo_CleanAir`, `LegalCargo_TSG`, `MixedCargo_Template`, `Security_10_Medium_001`, `Security_25_Medium_001`, `ShipHeist_Freelancer`). Test and template manifests could be flagged (`isTemplate`) instead of being listed as cargo.
  - Repeated allocation rows (`ShipHeist_Freelancer_Medium`: 46 Gold rows) are kept one by one. A grouped form, `{resource, probability, minShare, maxShare, count}`, would be smaller and loses nothing.

Related: [tool-data-sync](../plans/specs/tool-data-sync.md) and [dump-reuse](../plans/specs/dump-reuse.md) (cargo is "compatible as-is").
