---
title: game-ship-components (game-ship-components.json)
tags: [docs, datasets]
updated: 2026-11-25
---

# game-ship-components (game-ship-components.json)

Everything that slots into a ship and carries a stat: shields, quantum drives, power plants, coolers, radars, thrusters, fuel and quantum fuel tanks, armour, cargo grids, missile racks, weapon mounts, ship guns and tools, missiles, countermeasures, jump drives, quantum enforcement devices, EMPs, self-destructs, batteries and life support. Each item has the same headline columns (power, EM/IR, health, mass, distortion, volume) plus a category-specific `stats` map and an optional `detail` block. On LIVE 4.10.201.48085 it holds 3465 items in 21 categories; the file is 2,295,384 bytes minified. The `_schema` key (since tags 4.10.201.48085 / 5.0.202.35026, commit `b9e3700d0`) is not in that figure; it adds 35 bytes (2,295,419). PTU 5.0.202.35026: 248 weapons, 80 missiles and 171 missile racks, otherwise the same counts. It is also the item index [game-ships](game-ships.md) resolves loadouts and totals against.

## Source

- DataCore: every `EntityClassDefinition` under `entities/scitem/ships/`, plus `inventorycontainers/` for cargo grid sizes (filters, `crates/sc-gamedata/src/domains/components.rs:670`).
- Category by first match in `CATEGORIES` (components.rs:207): the category's params struct must be present, and where `types` is set, `AttachDef.Type` must be one of them. `mounts` also needs a `/weapon_mounts/` path (components.rs:588). Items matching no category are left out.
- Shared columns (`crates/sc-gamedata/src/helpers_ships/item_common.rs`):
  - identity `item_identity` (:48) from `SAttachableComponentParams.AttachDef`;
  - `power`, `em`, `ir` from `resource_profile` (:109), the `ItemResourceComponentParams.states[]` entry named `Online` (else the first);
  - `health` `SHealthComponentParams.Health`; `mass` `SEntityPhysicsControllerParams.PhysType.Mass`; `distortionMax` `SDistortionParams.Maximum`;
  - `durability` `durability_of` (:235); `distortion` `distortion_of` (:297); `volume` `AttachDef.inventoryOccupancyVolume.microSCU`.
- Localization (`Data/Localization/english/global.ini`) for names.
- Code: `parse_component` (components.rs:568), `parse_components` (:635), `Domain::parse` (:678). Groups are sorted by size, then name.
- Derives from no other dataset. Cargo grids read `inventorycontainers` records through `domains/containers.rs` `parse_container`.

## Shape

Top-level keys: `_source`, `_extracted`, `_build`, then one array per category in first-seen order, then `summary`. `_schema: "game-ship-components@1"` after `_build` (since tags 4.10.201.48085 / 5.0.202.35026), and an entry in `build.json` `files[]`.

`summary` (LIVE): armor 225, batteries 6, missileRacks 173, cargoGrids 143, coolers 84, countermeasures 198, fuelTanks 217, quantumFuelTanks 157, jumpDrives 12, lifeSupport 12, powerPlants 89, quantumDrives 63, quantumEnforcement 7, radars 79, selfDestruct 7, shields 73, thrusters 1485, weapons 244, mounts 117, emp 7, missiles 67.

### Common fields (every category)

| Field | Type | Meaning | Units/range (LIVE, all 3465) | Source |
|---|---|---|---|---|
| `name` | string | Localised name, or record short name when a placeholder | 3465 | `AttachDef.Localization.Name` |
| `isTemplate` | true? | Name is a placeholder (template or stub) | 1842 (1470 of them thrusters) | identity |
| `nameKey`, `descriptionKey`, `nameStatus` | string? | String-table keys; status absent when real | 3462 / 1324 / 1842 | `loc_refs` |
| `recordName`, `id` | string | `EntityClassDefinition.<short>`, GUID | 3465 | record |
| `category` | string | The category array it sits in | 21 values | `CATEGORIES` |
| `type`, `subType` | string? | `AttachDef.Type` / `SubType`. `UNDEFINED` is dropped from `subType` only: 108 thrusters have `type: "UNDEFINED"` | 3465 / 2667 | `AttachDef` |
| `size`, `grade` | number | `AttachDef.Size` / `Grade` | 3465 | `AttachDef` |
| `manufacturer` | string? | Manufacturer record name, upper case (`GODI`) | 2380 | `AttachDef.Manufacturer` |
| `power` | number? | Power drawn per second while online (doc comment, components.rs:66) | 0 – 12; 556 items; unit not stated | `Online` state `consumption` of `Power` |
| `em`, `ir` | number? | Nominal EM / IR signature in the online state | em 0 – 54,500, ir 0 – 15,000; 2702 | `signatureParams.EMSignature/IRSignature.nominalSignature` |
| `health` | number? | Item health | 0 – 750,000; 2964 | `SHealthComponentParams.Health` |
| `mass` | number? | Physics mass | −1 – 6,870,000; 2881; the −1 is read as is, meaning not established | `PhysType.Mass` |
| `distortionMax` | number? | Distortion absorbed before the item shuts down | 10 – 500,000; 733 | `SDistortionParams.Maximum` |
| `durability` | object? | `health`, `resistances` (multipliers ≠ 1 only), `selfRepair {count, seconds, healthRatio}`, `repairRestoreRatio`, `wear {maxLifetimeHours, damageConversionRate, overheatMultiplier}`, `misfireWindow {min, max}` | 3321 | health, `ItemResourceComponentParams.selfRepair`, first `SDegradationParams.accumulators[]`, `EntityComponentMisfireParams` |
| `distortion` | object? | `max`, `decayDelay`, `decayRate`, `warningRatio`, `recoveryRatio`, `secondsToRecover` | 733; `secondsToRecover` 1 – 1056.11 | `SDistortionParams` |
| `volume` | number? | Inventory volume, micro-SCU | 1 – 120,000,000; 3453 | `AttachDef.inventoryOccupancyVolume.microSCU` |
| `stats` | object | Category stats, flat numbers keyed by the last path segment | always present | per category |
| `detail` | object? | Category structure that does not flatten | 2196 | per category |

### `stats` / `detail` per category

| Category | Identified by | `stats` keys (count with the key on LIVE) | `detail` | Rust |
|---|---|---|---|---|
| `shields` | `SCItemShieldGeneratorParams` | `MaxShieldHealth` (720 – 3,168,000), `MaxShieldRegen`, `DecayRatio`, `DownedRegenDelay`, `DamagedRegenDelay`, `ReservePoolDrainRateRatio`, `ReservePoolMaxHealthRatio`, `ReservePoolRegenRateRatio`, `ElectricalChargeDamageResistance`, `maxStunTime` (all 73) | `resistance`, `absorption`: per damage type `{min, max}` from `ShieldResistance[6]` / `ShieldAbsorption[6]` | :209 |
| `quantumDrives` | `SCItemQuantumDriveParams` | `quantumFuelRequirement`, `disconnectRange`, `driveSpeed` (161,410,900 – 1,262,910,000), `cooldownTime`, `stageOneAccelRate`, `stageTwoAccelRate`, `spoolUpTime`, `engageSpeed`, `calibrationRate`, `min/maxCalibrationRequirement`, `inFlightThermalEnergyDraw`, `splineDriveSpeed` (all 63). `jumpRange` is dropped when above 1e30 (the 3.4e38 "unbounded" sentinel): 63 of 63 on LIVE | none | :241 |
| `radars` | `SCItemRadarComponentParams` | `cooldownTime` (79). `detectionLifetime` and `physicalRadarRange` are asked for but present on 0 | `signatureDetection[]`: 8 entries each, `{sensitivity, piercing, passive, active}`; which signal each index is, is not established | :279 |
| `countermeasures` | type `WeaponDefensive` | `maxAmmoCount`, `maxRestockCount` (198) | `ammo`: ammo params record name (`behr_flare`) | :316 |
| `weapons` | `SCItemWeaponComponentParams` | `maxAmmoCount` (220), `overheatTemperature` (65) | none. Types: WeaponGun 198, WeaponMining 24, TractorBeam 12, SalvageHead 9, TowingBeam 1. Only 3 of the 9 `SalvageHead` items are salvage heads (`Salvage_Head_standard`, `_Salvation`, `_Template`); the other 6 are tractor and towing beam records whose `AttachDef.Type` is `SalvageHead` (`GRIN_TractorBeam_S3`, `WEP_TractorBeam_S3_*`, `WEP_TowingBeam_S3_template`) | :332 |
| `missiles` | `SCItemMissileParams` | `lockTime`, `lockRangeMax`, `maxRadius`, `linearSpeed`, `maxLifetime` (67) | none | :350 |
| `powerPlants` | type `PowerPlant` | `powerOutput` (10 – 10,000), online `generation` of `Power` | none | :367 |
| `coolers` | type `Cooler` | `coolantOutput` (generation of `Coolant`) | none | :373 |
| `lifeSupport` | type `LifeSupportGenerator` | `lifeSupportOutput` | none | :379 |
| `thrusters` | `SCItemThrusterParams` | `thrustCapacity`, `fuelBurnRatePer10KNewton`, `minHealthThrustMultiplier` (1485) | `thrusterType`: Main 380, Maneuver 810, Retro 295 | :387 |
| `quantumFuelTanks` | `SCItemFuelTankParams` + type `QuantumFuelTank` | `standardCargoUnits` (157) | none | :406 |
| `fuelTanks` | `SCItemFuelTankParams` | `standardCargoUnits` (214 of 217). Types: FuelTank 185, ExternalFuelTank 31, FuelIntake 1 | none | :412 |
| `armor` | `SCItemVehicleArmorParams` | `signalInfrared`, `signalElectromagnetic`, `signalCrossSection` (225) | `damageMultiplier`, `deflection` (`armorDeflection.deflectionValue`), `penetrationAbsorption` (`armorPenetrationResistance.penetrationAbsorptionForType`), each a damage-type map | :418 |
| `missileRacks` | `SCItemMissileRackParams` | `missileCount` (ports accepting `Missile`), `missileSize` (largest `MaxSize`; 155 of 173) | none | :452 |
| `cargoGrids` | `SCItemInventoryContainerComponentParams` + type `CargoGrid` | `scu` (136 of 143) | `container` (inventory container name), `interiorDimensions {x, y, z}` (metres) | :466 |
| `jumpDrives` | `SCItemJumpDriveParams` | `alignmentRate`, `alignmentDecayRate`, `tuningRate`, `tuningDecayRate`, `fuelUsageEfficiencyMultiplier` (12) | none | :494 |
| `quantumEnforcement` | `SCItemQuantumInterdictionGeneratorParams` | `jammerRange`, `maxPowerDraw`, `chargeTimeSecs`, `dischargeTimeSecs`, `cooldownTimeSecs`, `radiusMeters` (7) | none | :511 |
| `emp` | `SCItemEMPParams` | every top-level number: `chargeTime`, `cooldownTime`, `distortionDamage`, `empRadius`, `minEmpRadius`, `minPhysRadius`, `physRadius`, `pressure`, `unleashTime` (7) | none | :529 |
| `selfDestruct` | `SSCItemSelfDestructComponentParams` | `damage`, `minRadius`, `radius`, `time` (7) | none | :535 |
| `batteries` | type `Battery` | `standardCargoUnits` (6; all 6 are placeholder-named) | none | :541 |
| `mounts` | type `Turret` under `/weapon_mounts/` | `gunCount` (ports accepting `WeaponGun`), `gunSize` (100 of 117) | none | :548 |

Units of the category stats are not stated in the Rust source except `interiorDimensions` (metres) and `volume` (micro-SCU).

### Example (LIVE, one shield)

```json
{
  "name": "AllStop",
  "nameKey": "item_NameSHLD_GODI_S01_AllStop",
  "descriptionKey": "item_DescSHLD_GODI_S01_AllStop",
  "recordName": "EntityClassDefinition.SHLD_GODI_S01_AllStop_SCItem",
  "id": "339bd641-8dfb-4bc1-b273-5ffc5f702fbb",
  "category": "shields", "type": "Shield", "size": 1, "grade": 3, "manufacturer": "GODI",
  "power": 3, "em": 1240, "ir": 0, "health": 210, "mass": 120, "distortionMax": 3150,
  "durability": {"health": 210, "resistances": {"physical": 0.9, "energy": 0.9, "thermal": 0.1},
    "selfRepair": {"count": 1, "seconds": 40, "healthRatio": 0.2}, "repairRestoreRatio": 0.1,
    "wear": {"maxLifetimeHours": 0, "damageConversionRate": 0.5, "overheatMultiplier": 2},
    "misfireWindow": {"min": 45, "max": 600}},
  "distortion": {"max": 3150, "decayDelay": 1.5, "decayRate": 210, "warningRatio": 0.75, "recoveryRatio": 0, "secondsToRecover": 16.5},
  "volume": 84000,
  "stats": {"MaxShieldHealth": 3168, "MaxShieldRegen": 602, "DecayRatio": 0.25, "DownedRegenDelay": 11.5, "DamagedRegenDelay": 5.75,
    "ReservePoolDrainRateRatio": 2.5, "ReservePoolMaxHealthRatio": 1, "ReservePoolRegenRateRatio": 1,
    "ElectricalChargeDamageResistance": 0, "maxStunTime": 12},
  "detail": {
    "resistance": {"physical": {"min": 0, "max": 0.25}, "energy": {"min": 0, "max": 0}, "distortion": {"min": 0.75, "max": 0.95},
      "thermal": {"min": 0, "max": 0}, "biochemical": {"min": 0, "max": 0}, "stun": {"min": 0, "max": 0}},
    "absorption": {"physical": {"min": 0, "max": 0.45}, "energy": {"min": 1, "max": 1}, "distortion": {"min": 1, "max": 1},
      "thermal": {"min": 1, "max": 1}, "biochemical": {"min": 1, "max": 1}, "stun": {"min": 1, "max": 1}}}
}
```

## Derived, estimated and curated values

No `Estimate` fields and no `_assumptions`. Computed rather than copied:

- `distortion.secondsToRecover` = `decayDelay` + `Maximum` × (1 − `RecoveryRatio`) ÷ `DecayRate`, 2 dp (item_common.rs:297). It assumes a linear decay at `DecayRate`, which the record does not state.
- `cargoGrids[].stats.scu` is approximate: interior volume (x × y × z, metres) ÷ 1.953125 (one SCU as a 1.25 m cube), 2 dp (`approx_scu`, `domains/containers.rs:91`). It is not a capacity the record states. [game-ships](game-ships.md) `totals.cargoScu` and `capabilities.cargo_scu` sum it.
- `missileRacks.missileCount` / `mounts.gunCount` count ports by accepted type; `missileSize` / `gunSize` take the largest `MaxSize`.
- `power` sums every `Power` consumption delta in the online state; `powerOutput` / `coolantOutput` / `lifeSupportOutput` sum the generation deltas.
- `durability.resistances` keeps only multipliers that differ from 1 (`health_resistances`, item_common.rs:219).
- `jumpRange` is removed when it is the unbounded sentinel.

## Redaction

Nothing. `game-ship-components.json` is not in `PROSE_FIELDS`; items carry `descriptionKey` only.

## Consumers

- **snares** `scripts/build-data.mjs:34`: `interdictors()` (`:131-151`) reads `quantumEnforcement[]` `isTemplate`, `recordName`, `name` and `stats.radiusMeters` (> 100 means a snare; "no snare" is stored as a 1 m radius), `jammerRange`, `chargeTimeSecs`, `dischargeTimeSecs`, `cooldownTimeSecs`. `drives` (`:242-255`) reads `quantumDrives[]` `isTemplate`, `manufacturer`, `recordName`, `name`, `size`, `grade`, `stats.driveSpeed`, `stageOneAccelRate`, `stageTwoAccelRate`, `spoolUpTime`, `cooldownTime`.
- **MCsim** `persim/scripts/build_ordnance.py:231-242` indexes every category by `recordName` stem and sums `em` / `ir` for `powerPlants`, `coolers`, `shields`, `quantumDrives`, `radars`, `lifeSupport` (`EMIT_SLOTS`, `:65`). `build_fire.py:118` (`comp_index`, lower-cased stem) reads `name`, `size`, `durability.selfRepair` (`count`, `seconds`, `healthRatio`) and `distortionMax` (`:183-194`).
- **SCMCP** `src/versions.ts:33` diffs the file by its `weapons` collection.
- **starorg-codex** (branch `feat/dataset-reuse`): the dump reader lists the file in `DUMP_USE` (`crates/starorg-mcp/src/prebuilt.rs:179`): schema version 1, top-level keys `armor`, `missiles`, `mounts`, `quantumDrives`, `shields`, `weapons` and `summary` required, no redaction accepted. `crates/starorg-core/src/model/items.rs:204` models a component by these field names; `crates/starorg-core/src/enhance/items.rs` (`:132-191`) writes lines from `shields` stats (`MaxShieldHealth`, `MaxShieldRegen`, `DownedRegenDelay`, `DamagedRegenDelay`, `detail.resistance`) and `quantumDrives` stats (`driveSpeed`, `spoolUpTime`, `cooldownTime`, `quantumFuelRequirement`, `stageOne/TwoAccelRate`, `min/maxCalibrationRequirement`). It labels `quantumFuelRequirement` "per Gm" and `driveSpeed` ÷ 1e6 as Mm/s; the Rust source states neither unit.
- **sc-gamedata** itself: `domains/ships.rs` resolves every loadout item through `parse_component`.
- [consumer-datasets](../plans/specs/consumer-datasets.md) lists no contract specific to this file; the general ones apply (`_build` equals the tag; the `_schema` stem and `build.json` `files[]` hashes). Do not rename the stat keys above without telling snares and starorg-codex, and keep the seven top-level keys starorg-codex's `DUMP_USE` requires.

## Derivable data

The game data is CIG's: we don't add to it, we derive from it. More that the shipped records already hold:

### Fields the source records carry that the dataset does not read

From `extracted-data` (build 4.10.201.48085, files dated 2026-11-24):

- **Radar.** `SCItemRadarComponentParams` has no `detectionLifetime` or `physicalRadarRange` (the two keys the picker asks for; 0 of 79 hit). What it does carry: `aimAssist.distanceMinAssignment` 845, `distanceMaxAssignment` 1105, `outsideRangeBufferDistance` 60 (Ecouter); `sensitivityModifiers[]` (`sensitivityAddition` −0.55 for a named contact group); `deltaSignatureSpike[8]`; `signatureDetection[].deltaSignatureSensitivity` / `deltaSignaturePierce`; `sharedParams` → `radarsystem/vehicleradarsystemsharedparams.json`.
- **Gun heat.** `overheatTemperature` is read only from `connectionParams.simplifiedHeatParams`, which is `null` on most guns (65 of 198 `WeaponGun` have it). The same items carry `SEntityPhysicsControllerParams.PhysType.temperature.itemResourceParams.overheatTemperature` (an `EntityTemperatureItemResource`; 450 on `KLWE_LaserRepeater_S3`), the path [game-fire](game-fire.md) already reads for other item types, and `connectionParams` holds `heatRateOnline` (60), `heatStats`, `overclockStats`, `overpowerStats`, `underpowerStats`.
- **Quantum drive.** `params.interdictionEffectTime` 2.5, `calibrationProcessAngleLimit` 5.0, `calibrationWarningAngleLimit` 8.0, `calibrationDelayInSeconds` 1.5; all of `splineJumpParams` except `driveSpeed` (on `QDRV_WETK_S01_Beacon_SCItem`: spline `spoolUpTime` 4.0, `cooldownTime` 10, `stageOneAccelRate` 250, `stageTwoAccelRate` 50000, `interdictionEffectTime` 5.0); `heatParams.preRampUp/rampUp/rampDown/postRampDownThermalEnergyDraw` (all 1350 on the Beacon).
- **Missile racks.** `SItemPortContainerComponentParams.Ports[].Types[].SubTypes` (`["Missile"]` on `MRCK_S03_BEHR_Dual_S02`) and `MinSize` (2), and `SCItemMissileRackParams.launchDelay` (0.125): the fields [consumer-datasets](../plans/specs/consumer-datasets.md) section 3 lists for `game-ordnance`.
- **Cargo containers.** `InventoryContainer.inventoryType.maxPermittedItemSize` (1.25 × 2.5 × 1.25 on `aegs_avenger_cargogrid_stalker`, i.e. the largest box that fits), `minPermittedItemSize`, `maxPercentageErasedOnParentDestruction` 90, `isExternalContainer` false (all under `inventoryType`).
- **Salvage tooling.** Salvage heads (`weapons[]` type `SalvageHead`) keep only `maxAmmoCount`; the records carry `SSalvageModifier` and `SWeaponActionFireSalvageRepairParams` numbers. Salvage modifiers (`utility/salvage/salvagemodifiers/*`) and salvage storages (`salvagemunching/salvageinternalstorage_*`, Reclaimer 448 SCU) are in no category. See [game-salvage](game-salvage.md).
- **Resource profile.** `resource_profile` builds the full online `generation` / `consumption` lists but only `Power` draw and the per-category output reach the file; e.g. coolant draw and other states (`Offline`, `Travelling` for quantum drives) are dropped.

### Values computable with other datasets

- **Where each component ships** (reverse link): [game-ships](game-ships.md) `vehicles[].loadout.*[].item` → this file's `recordName` stem. A `stockOn: [vehicle]` list per item would serve SCMCP's item questions and starorg-codex's component tooltips. Plain join, no estimate.
- **Where to get**: join to [game-item-obtain](game-item-obtain.md) by record id or name (loot, contract awards, Wikelo, blueprints). Per-shop stock and prices are server-side; [game-shops](game-shops.md) gives only the sell-back percent per item type.
- **Per-ship signature totals**: MCsim's `build_ordnance.py` already sums `em` / `ir` over a ship's stock emitters; the same sum per vehicle would be a plain derived total. Applying the armour `signals` multipliers on top is MCsim's model, so the product would be an estimate.
- **Shield time to full**: `MaxShieldHealth` ÷ `MaxShieldRegen` (+ `DownedRegenDelay`). Assumes a linear regen; estimate.
- **Cooling and power balance per ship**: Σ `coolantOutput`, Σ `powerOutput`, Σ `power` over a stock loadout. Plain totals; game-ships `totals` has `powerOutput` but no draw or coolant.

### Parse gaps

- **Radar stats** (above): `detectionLifetime` and `physicalRadarRange` are stale field names; 0 of 79 radars get them.
- **Bomb racks**: `missileCount` counts ports whose `Types[].Type` is `Missile`. Bomb racks declare `Type: "Bomb"` ports, so all 16 `BombLauncher` racks (15 `BMBRCK_*` and `MRCK_S03_TEMP_BOMB`) get `missileCount: 0` and no `missileSize`; e.g. `BMBRCK_S03_AEGS_Eclipse` has 20 size-3 `Bomb` ports (`SubTypes` `["Utility"]`). 18 of 173 racks have no size: those 16 plus two `MissileLauncher` records with no `Missile` port (`VNCL_MissileRack_Blade`, `MRCK_S08_BEHR_Non_S05_PUDefenseTurret_NoAnim`).
- **Gun overheat** (above): 133 of 198 guns lack `overheatTemperature` though the value is on the record.
- **Cargo grids without size**: the 7 grids with no `scu` (`AEGS_Javelin_CargoGrid*` ×6, `ANVL_Hornet_CargoGrid_F7C`) have `containerParams: null` on the record; nothing to read.
- **Fuel tanks without capacity**: `KRIG_P72_Archimedes_Intake` (a `FuelIntake`), `MISC_FuelRefinery_L` / `_R` have no `ResourceContainer`; 214 of 217 have `standardCargoUnits`.
- **Placeholder names**: 1842 items are `isTemplate`, including 1470 of 1485 thrusters, all 143 cargo grids and all 6 batteries. Names fall back to record short names.
- **`mass` −1**: one item (`GLSN_Basher_Thruster`) carries −1; read as is.

Related specs: [consumer-datasets](../plans/specs/consumer-datasets.md) section 3 (`game-ordnance`), [dump-enhancements](../plans/specs/dump-enhancements.md), [dump-reuse](../plans/specs/dump-reuse.md).
