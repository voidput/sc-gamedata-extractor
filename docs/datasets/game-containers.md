---
title: game-containers (game-containers.json)
tags: [docs, datasets]
updated: 2026-11-25
---

# game-containers (game-containers.json)

Every `InventoryContainer` record: cargo grids, personal storage, lockers, backpacks, armour pockets and loot boxes, with their interior dimensions and a volume and SCU figure computed from them. A record whose dimensions are a stand-in for "unbounded" (the landing zone's 999 × 999 × 999) is flagged instead of reported as capacity. On LIVE 4.10.201.48085: 596 containers, 581 with a volume, 1 unbounded, largest 21,952 SCU (by this file's formula); 165,713 bytes minified. PTU 5.0.202.35026: 593 containers, 164,969 bytes.

## Source

- Filter at `crates/sc-gamedata/src/domains/containers.rs:133`: `**/inventorycontainers/**`. Only records of type `InventoryContainer` are parsed (`parse_containers`, containers.rs:100); the other records in that folder (`LandingZoneInventory` ×3, `InventoryContainerManager` `starcitizen.json`) are skipped.
- Per record (`parse_container`, containers.rs:53): `interiorDimensions` (`Vec3`), `inventoryType` (its string, or its `_Type_`), `excludedItemSubTypes[]`.
- Domain at containers.rs:124, `parse` at containers.rs:137. No localization. Derives from no other dataset.

## Shape

Top-level keys: `_source`, `_extracted`, `_build`, `_schema`, `containers[]`, `summary{}`. `_schema: "game-containers@1"` sits after `_build` (since tags 4.10.201.48085 / 5.0.202.35026, commit `b9e3700d0`) and is counted in the byte sizes above. `build.json` has an `extractor` and a `files[]` entry with the file's hash (since tags 4.10.201.48085 / 5.0.202.35026).

### `containers[]` (largest `volumeCubicMetres` first, stable)

| Field | Type | Meaning | Units/range (LIVE) | Source |
|---|---|---|---|---|
| `name` | string | Record short name | 596 | record name |
| `recordName` | string | `InventoryContainer.<name>` | | |
| `id` | string | Record GUID | | `_RecordId_` |
| `inventoryType` | string? | Container type | `InventoryClosedContainerType` 358, `InventoryOpenContainerType` 234, `InventoryOpenAlwaysContainerType` 4 | `inventoryType._Type_` |
| `interiorDimensions` | object? | `{x, y, z}` | metres (doc comment, containers.rs:31); present on all 596, zeros on some | `interiorDimensions` |
| `volumeCubicMetres` | number? | x × y × z rounded to 0.001; absent when any side is 0 or the record is unbounded | 581 present, 0.007 to 42,875 | computed |
| `approxScu` | number? | volume ÷ 1.953125 (a 1.25 m cube), rounded to 0.01; absent when the volume is 0 | 581 present, 0 to 21,952 (2 round to 0) | computed (containers.rs:47) |
| `unbounded` | `true`? | Some side is 900 or more (containers.rs:51) | 1 (`landingZone`) | computed |
| `excludedSubTypes` | string[]? | The record's `excludedItemSubTypes` strings; omitted when empty. Meaning not established beyond the field name | 0 on LIVE | `excludedItemSubTypes` |

### `summary`

`containers` 596, `withDimensions` 581 (rows with a non-zero volume), `unbounded` 1, `largestScu` 21,952 (`approxScu` of the first sized row).

### Example (LIVE)

```json
{
  "name": "PersonalStorage_Locker_Spartan",
  "recordName": "InventoryContainer.PersonalStorage_Locker_Spartan",
  "id": "c20ea96e-6d31-44fb-aa89-7b87401edb7f",
  "inventoryType": "InventoryClosedContainerType",
  "interiorDimensions": {"x": 0.5, "y": 0.7, "z": 1.3},
  "volumeCubicMetres": 0.455,
  "approxScu": 0.23
}
```

## Derived, estimated and curated values

- `volumeCubicMetres` and `approxScu` are computed by this code, not read. `approxScu` is an **estimate** that does not carry the `Estimate` suffix: it assumes capacity equals interior volume over the SCU cube, and the records state a different capacity (see Parse gaps).
- `unbounded` rests on a threshold this code picked (900 m), not on a record field.

## Redaction

Nothing. Not in `PROSE_FIELDS`.

## Consumers

- **SCMCP:** `src/versions.ts:41`, generic dataset access, collection `containers`. No field-level reader.
- **starorg-codex app** (`feat/dataset-reuse`): `crates/starorg-mcp/src/prebuilt.rs:160` needs `containers` and `summary` (no typed reader); verified on 4.10.200.58746. Its Developer-mode fixtures (`crates/starorg-core/tests/fixtures/dev-shapes/game-containers.json`) use `interiorDimensions`, `volumeCubicMetres`, `approxScu`.
- Not read by snares, MCsim, Bindscape or sc-telemetry.
- Contracts: the sc-telemetry rules for every dataset in [consumer-datasets](../plans/specs/consumer-datasets.md). [dump-reuse](../plans/specs/dump-reuse.md) E.5 lists containers as compatible as-is.

## Derivable data

Fields the records carry that this dataset does not read (raw values from `sc-gamedata/extracted-data`, LIVE 4.10.201.48085 per its `_build.json`, mixed with stale records):

- **The game's own capacity**, `inventoryType.capacity`: `SStandardCargoUnit.standardCargoUnits` on 264 records, `SMicroCargoUnit.microSCU` on 91, `SCentiCargoUnit` on 3, none on 238. Of the 264 SCU-typed records, 243 state a capacity different from `approxScu`, 7 the same, and 14 have no `approxScu`: `PersonalStorage_Locker_Spartan` states 1.06 SCU against `approxScu` 0.23; `PersonalStorage_Locker_060` 0.6 against 1.02; `ArmouryInventory` 500 against 21,952; `landingZone` 5000 (flagged unbounded here). `game-armor` already reads this field for backpacks and pockets (`container_micro_scu`, `crates/sc-gamedata/src/domains/armor.rs:113`), so the two datasets disagree today. A `capacityScu` field from the record, next to the estimate, would fix that.
- `InventoryContainerManager.StarCitizen` (`inventorycontainers/starcitizen.json`): global rules, among them `closedInventoryNonStorableItemTypes` (21 `itemType` entries, e.g. `Armor`, `CapacitorAssignmentController`), `openInventoryNonStorableItemTypes` (4, e.g. `Player`, `Debris`), `bigItemVolumeThresholdSCU` (1.0), `spawnLootGrid` (5 × 5 × 5), `lootBoxes` (the three destroyed-inventory crates). Meanings beyond the names not established.
- `LandingZoneInventory` records (`landingzoneinventory.json`, `pyro_`, `nyx_`): `locations[]` (309 in `landingzoneinventory.json`) with `location` (a starmap record, e.g. `stanton1_lorville`), `visibleToPlayer`, `lawful`, `impoundLocation`, plus `containerParams` and `locationRedirects`. By the names this lists the locations with a landing-zone inventory; the meaning of each flag is not established.

Values computable by joining:

- **Owner → container.** In the mixed extract 2,517 item entity records reference a container through `SCItemInventoryContainerComponentParams.containerParams` (272 distinct containers; for example `rrs_combat_heavy_backpack_01_01_03` → `armor/backpack/backpack_combat_heavy`), and 1,112 vehicle records (`entities/spaceships` 1,075, `entities/groundvehicles` 37) through `VehicleComponentParams.inventoryContainerParams` (for example `spaceship_template` → `ships/ship_snub`). Together 400 distinct container paths are referenced (the counts include stale files). A reverse `usedBy[]` (items, ships, loot boxes) would say what each grid or locker belongs to, and would let `game-item-sources` spawns carry box capacity.
- **What fits:** with `game-item-tags` once it carries `inventoryOccupancyVolume` (rifle: 13,000 µSCU), capacity ÷ item volume gives an upper bound on items per container. It is an estimate: grid packing and `excludedItemSubTypes` are not modelled by such a ratio.

Parse gaps:

- 14 containers have no volume because a side is 0 (12 ship `*_PersonalStorage` records such as `AEGS_Avenger_PersonalStorage` at 0 × 0 × 0, plus `PersonalStorage_Locker` and `PersonalStorage_Template`); the 15th row without a volume is the unbounded `landingZone`. For these, `inventoryType.capacity` is the only size the records give (0 SCU on the Avenger).
- `excludedSubTypes` is empty on every LIVE record, so the field never appears.
