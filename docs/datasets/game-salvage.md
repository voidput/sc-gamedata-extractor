---
title: game-salvage (game-salvage.json)
tags: [docs, datasets]
updated: 2026-11-25
---

# game-salvage (game-salvage.json)

The global salvage constants: structural salvage (hull fracture and disintegration) and the multitool repair beam. It holds two objects read from two singleton records; most of those records' bulk (highlight colours, VFX tags) is left out on purpose. On LIVE 4.10.201.48085 `summary.records` is 2 and the file is 419 bytes minified. The `_schema` key (since tags 4.10.201.48085 / 5.0.202.35026, commit `b9e3700d0`) is not in that figure; it adds 27 bytes (446). PTU 5.0.202.35026 is identical apart from `_build` and `_extracted`. The file names no ships.

## Source

- DataCore records, picked by the filters in `crates/sc-gamedata/src/domains/salvage.rs:83`:
  - `libs/foundry/records/vehiclesalvageglobalparams/vehiclesalvageglobalparams.json` (`_Type_` `VehicleSalvageGlobalParams`) → `structural`, `parse_structural_salvage` (salvage.rs:49).
  - `libs/foundry/records/sglobalsalvagerepairbeamparams/sglobalsalvagerepairbeamparams.json` (`_Type_` `SGlobalSalvageRepairBeamParams`) → `repairBeam`, `parse_repair_beam` (salvage.rs:62).
- `Domain::parse` (salvage.rs:90) takes the first record of each type; a missing record drops its object.
- Derives from no other dataset. No localization needed.
- The module comment (salvage.rs:4) says per-tool stats (scraper heads, modifiers) "live on the items and are in `game-ship-components`". On LIVE that holds only for salvage heads, and only partly; see Parse gaps.

## Shape

Top-level keys: `_source`, `_extracted`, `_build`, `structural{}`, `repairBeam{}`, `summary{}`. `_schema: "game-salvage@1"` after `_build` (since tags 4.10.201.48085 / 5.0.202.35026), and an entry in `build.json` `files[]`.

### `structural`

| Field | Type | Meaning | Units/range (LIVE) | Source |
|---|---|---|---|---|
| `vehicleFractureForceScale` | number | Scale on the force that fractures a hull into salvageable pieces (doc comment, salvage.rs:20) | 0.25 | `vehicleFractureForceScale` |
| `sourceIdleFieldStrength` | number | Meaning not established | 0.1 | `structuralVFX.sourceIdleFieldStrengthValue` |
| `sourceMinEngagedFieldStrength` | number | Meaning not established | 0.2 | `structuralVFX.sourceMinEngagedFieldStrengthValue` |
| `disintegrateDissolveTime` | number | Time for a disintegrated piece to dissolve (salvage.rs:27) | 1; seconds per the doc comment | `structuralVFX.targetDisintegrateDissolveTime` |

### `repairBeam`

| Field | Type | Meaning | Units/range (LIVE) | Source |
|---|---|---|---|---|
| `hitsPerSecond` | number | Meaning not established | 10 | `hitsPerSecond` |
| `hitDuration` | number | Meaning not established | 0.3 | `hitDuration` |
| `hullThicknessMeters` | number | Hull thickness that repair material fills (salvage.rs:39) | 0.009; metres | `materialParams.hullThicknessMeters` |
| `ammoToMaterialFactor` | number | Repair material units per unit of beam ammo (salvage.rs:42) | 100 | `materialParams.ammoToMaterialFactor` |
| `materialResource` | string | Short name of the referenced `ResourceType` record | `"RMC"` | `materialParams.RMCResourceType` (`ref_short_name`) |

### `summary`

| Field | Type | Meaning | Range |
|---|---|---|---|
| `records` | integer | How many of the two records were found | 2 |

### Example (LIVE, whole file body)

```json
{
  "structural": {
    "vehicleFractureForceScale": 0.25,
    "sourceIdleFieldStrength": 0.1,
    "sourceMinEngagedFieldStrength": 0.2,
    "disintegrateDissolveTime": 1
  },
  "repairBeam": {
    "hitsPerSecond": 10,
    "hitDuration": 0.3,
    "hullThicknessMeters": 0.009,
    "ammoToMaterialFactor": 100,
    "materialResource": "RMC"
  },
  "summary": { "records": 2 }
}
```

## Derived, estimated and curated values

None. Values are copied as is; `materialResource` is the reference's short name.

## Redaction

Nothing. `game-salvage.json` is not in `PROSE_FIELDS` (`crates/sc-gamedata/src/localization.rs`, redaction module).

## Consumers

- **sc-telemetry** `sctelemetry/shipcaps.py:29` (`SALVAGE_FILE`) loads it next to `game-ships.json` (`:263-266`). `salvage_classes` (`:66`) looks for a `vehicles` or `ships` list of class or record names, or of objects with `recordName` / `class` / `className`. The current file has neither, so the salvage flag comes from `game-ships` alone. `gamedata_hub.py:35` fetches it from the dumps hub.
- Contracts from [consumer-datasets](../plans/specs/consumer-datasets.md): the file stays at `data/game-salvage.json`; `_build` equals the tag; the `_schema` stem is `game-salvage`; `build.json` `files[]` size and sha256 match the published bytes. A future `vehicles` list should use one of the shapes `salvage_classes` accepts.
- **starorg-codex** (branch `feat/dataset-reuse`): the dump reader lists it in `DUMP_USE` (`crates/starorg-mcp/src/prebuilt.rs:178`): schema version 1, top-level keys `structural`, `repairBeam` and `summary` required, no redaction accepted. Contract: keep all three keys.
- No other checked consumer (snares, MCsim, Bindscape, SCMCP) reads it.

## Derivable data

The game data is CIG's: we don't add to it, we derive from it. More that the shipped records already hold:

### Fields the source records carry that the dataset does not read

From the two global records in `extracted-data` (files dated 2026-11-12, build 4.10.201.48085):

- `SGlobalSalvageRepairBeamParams.highlightParams.colors[].hullThreshold`: 0.9999, 0.66, 0.33, 0.0. These are the hull-health bands the repair HUD colours by. UI data, but they are the game's own condition bands.
- `SGlobalSalvageRepairBeamParams.highlightParams.filterItemTypes[].itemType`: Seat, Usable, SalvageFillerStation, Display, SeatAccess, SeatDashboard, NOITEM_Vehicle, WeaponPersonal, Relay. Whether this list excludes those types from repair or only from highlighting is not established.
- `globalSalvageAudioParams.friendlyFireMessageCooldownScale` 10.0. Meaning not established.
- `VehicleSalvageGlobalParams.structuralVFX` tag references (`sourceResonateTag`, `sourceDisintegrateTag`, `sourceFieldMaterialsTag`, ...) into `tagdatabase`. These could be resolved to tag names through [game-item-tags](game-item-tags.md) data.

### Per-ship salvage data the records hold (not read by any dataset)

Item records under `extracted-data/libs/foundry/records/entities/scitem/ships/` (files dated 2026-11-24):

- **Salvage modifiers.** `entities/scitem/ships/utility/salvage/salvagemodifiers/*.json` carry `EntityComponentAttachableModifierParams.modifiers[].weaponModifier.weaponStats.salvageModifier` (`SSalvageModifier`); their `AttachDef.Type` is `SalvageModifier`. Example, `salvage_modifier_scraper_large`: `salvageSpeedMultiplier` 0.05, `radiusMultiplier` 6.0, `extractionEfficiency` 0.6. None of these items is in game-ship-components (0 matches for `Salvage_Modifier` across all categories), so the module comment's claim does not hold for modifiers.
- **Salvage heads.** `Salvage_Head_standard` is in game-ship-components `weapons[]` (type `SalvageHead`, "Baler Salvage Head"), but only `maxAmmoCount` is kept. The record also carries five `SSalvageModifier` entries (all 1.0 on the standard head) and `SWeaponActionFireSalvageRepairParams` with `maxVehicleDamageRatio` 0.9, `materialEfficiency` 1.0, `repairedMaterialRatio` 1.0, `healthToAmmoRatio` 0.5, `maxHealthRepairRate` 10.0, `maxDamageMapRepairRate` 10.0. Meaning of each not established beyond the names.
- **Salvage buffer per ship.** `salvagemunching/salvageinternalstorage_<maker>_<ship>.json` (`AttachDef.Type` `SalvageInternalStorage`) has `ResourceContainer.capacity.standardCargoUnits` (`aegs_reclaimer` 448, `argo_moth` 240, `gama_hyun` 40, `drak_vulture` 16, `misc_fortune` 14, `rsi_salvation` 8) and `inclusiveResources` `ResourceType.RMC` and `ResourceType.ConstructionMaterialChunks`. Ships' default loadouts reference these storages, so a per-ship "salvage storage SCU" is a direct read.

### Values computable with other datasets

- **A `vehicles` list for sc-telemetry.** 34 of the 1104 LIVE vehicles in [game-ships](game-ships.md) carry `salvage_head_standard` or `salvage_head_salvation` in their stock loadout records. All but one already get `capabilities.salvage: true` from their role (a 34th `salvage: true` vehicle, `RSI_Salvation_Template`, has the role but no head). The exception is `GAMA_Hyun` (role "Generalist"), which carries two salvage arms with `salvage_head_standard` and gets `salvage: false`. Listing loadout-derived salvage ships here fills the slot `salvage_classes` (shipcaps.py:66) already reads.
- **Repair material per area** (estimate): `hullThicknessMeters` × area gives a fill volume, and `ammoToMaterialFactor` converts beam ammo to material units. How the game turns volume into RMC is not stated, so any RMC-per-m² figure is an estimate.
- **Salvage throughput per ship** (estimate): salvage head + fitted modifiers (`salvageSpeedMultiplier`, `extractionEfficiency`) + storage SCU, joined through game-ships loadout. The base scrape rate is not in these records, so absolute rates would be estimates; ratios between ships would not.

### Parse gaps

- Four of the eight published numbers (`sourceIdleFieldStrength`, `sourceMinEngagedFieldStrength`, `hitsPerSecond`, `hitDuration`) have no stated meaning in code.
- The salvage tooling stats the module comment points to game-ship-components for are mostly absent there (see above).
- `game-ships` drops `SalvageHead` items from `loadout.weapons` (its `GUN_TYPES`, ships.rs:364, has no `SalvageHead`), so a consumer cannot recover salvage heads from game-ships either. See [game-ships](game-ships.md).

Related specs: [consumer-datasets](../plans/specs/consumer-datasets.md) section 4 (capabilities), [tool-data-sync](../plans/specs/tool-data-sync.md).
