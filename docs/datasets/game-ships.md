---
title: game-ships (game-ships.json)
tags: [docs, datasets]
updated: 2026-11-25
---

# game-ships (game-ships.json)

Every ship and ground vehicle record: identity (name, manufacturer, career, role, crew, size, bounding box), hull and armour, radar cross section, flight numbers from the stock flight controller, the stock loadout sorted by item type, `totals` summed from that loadout, power pips, and `capabilities` flags in sc-telemetry's `derive_caps` rules. On LIVE 4.10.201.48085 it holds 1104 vehicles (1099 named, 5 templates, 1088 with a loadout). The extract is 3,926,188 bytes minified; as published (`description` stripped, `_redacted` added) 3,502,365 bytes. The `_schema` key (since tags 4.10.201.48085 / 5.0.202.35026, commit `b9e3700d0`) is not in those figures; it adds 25 bytes to each (3,926,213 / 3,502,390); the build's `data/` holds the extract shape, with `description` still in. PTU 5.0.202.35026: 1108 vehicles, 1092 with a loadout.

## Source

- DataCore, filters in `crates/sc-gamedata/src/domains/ships.rs:929`: `**/entities/spaceships/**`, `**/entities/groundvehicles/**`, `**/entities/scitem/ships/**`, `**/ammoparams/**`, `**/inventorycontainers/**`.
- A vehicle is an `EntityClassDefinition` under `entities/spaceships/` or `entities/groundvehicles/` (`is_vehicle_path`, ships.rs:883) that has `VehicleComponentParams` (`parse_vehicle`, ships.rs:756). Components read from the vehicle record:
  - `VehicleComponentParams`: `vehicleName`, `vehicleDescription`, `vehicleCareer`, `vehicleRole`, `manufacturer`, `movementClass`, `crewSize`, `isGravlevVehicle`, `maxBoundingBoxSize`, `vehicleHullDamageNormalizationValue`.
  - `SAttachableComponentParams.AttachDef.Size`; `SHealthComponentParams.DamageResistances` (`parse_hull`, ships.rs:258).
  - `SSCSignatureSystemParams.radarProperties.crossSectionParams.crossSection`.
  - `SItemPortContainerComponentParams.resourceNetworkPowerPools.itemPools` (`parse_power_pools`, ships.rs:428).
  - `SEntityComponentDefaultLoadoutParams.loadout` (flattened depth first, `loadout_entries`, ships.rs:279).
- Loadout items resolve against every ship item under `entities/scitem/ships/` (`build_vehicle_context`, ships.rs:824): [game-ship-components](game-ship-components.md) `parse_component` for components, `helpers_ships/weapons.rs` for gun DPS and missile damage (with `ammoparams`), and `inventorycontainers` for cargo grids. The flight block comes from the first `FlightController` item's `IFCSParams` (`parse_flight`, ships.rs:373).
- Localization (`Data/Localization/english/global.ini`): names, descriptions, career and role.
- Code: `parse_loadout` (ships.rs:472), `derive_capabilities` (ships.rs:147), `Domain::parse` (ships.rs:943).
- Shares its item parse with game-ship-components; reads no other dataset file. `game-search-index.json` is derived from this file and [game-item-tags](game-item-tags.md) (`write_search_index`, `crates/sc-gamedata/src/search_index.rs:398`).

## Shape

Top-level keys: `_source`, `_extracted`, `_build`, (published: `_redacted`), `vehicles[]`, `byManufacturer{}`, `byRole{}`, `summary{}`. `_schema: "game-ships@1"` after `_build` (since tags 4.10.201.48085 / 5.0.202.35026), and an entry in `build.json` `files[]`.

- `summary`: `vehicles` 1104, `named` 1099, `templates` 5, `withLoadout` 1088.
- `byManufacturer` / `byRole`: count of named vehicles per `manufacturer` / `role`, largest first (aegs 176, anvl 165, drak 143, ...; Light Fighter 104, Heavy Fighter 83, ...). 23 manufacturer keys, 59 role keys, including `procedural_text_null` 14 and `rsi_2025` / `mrai_2025` / `gama_2026` manufacturer records.
- `vehicles[]` sorted by `name` (locale compare).

### `vehicles[]`

| Field | Type | Meaning | Units/range (LIVE, of 1104) | Source |
|---|---|---|---|---|
| `name` | string | Display name, or record short name when the name is a placeholder | 1104 | `vehicleName` |
| `recordName`, `id` | string | `EntityClassDefinition.<short>`, GUID | 1104 | record |
| `displayName` | string? | Localised name when real | 1099 | `vehicleName` |
| `description` | string? | Localised description. **Stripped on publish** | 1084 in the extract, 0 published | `vehicleDescription` |
| `nameKey`, `descriptionKey` | string? | String-table keys | 1100 / 1100 | `loc_refs` |
| `nameStatus` | string? | `empty` 4, `unresolved` 1; absent when real | 5 | `loc_refs` |
| `manufacturer` | string? | Manufacturer record short name, **lower case** (`aegs`, `rsi_2025`), unlike components' upper-case codes | 1103 | `VehicleComponentParams.manufacturer` |
| `movementClass` | string? | Spaceship 1064, ArcadeWheeled 40 | 1104 | `movementClass` |
| `career`, `role` | string? | Localised; when the text is a placeholder, the key tail (`@vehicle_focus_combat` → `combat`, `tidy_classifier` ships.rs:240) | 1104; includes `procedural_text_null` (14 each) and `LOC_PLACEHOLDER` (career 2, role 3) | `vehicleCareer`, `vehicleRole` |
| `crewSize` | number? | | 1 – 35 | `crewSize` |
| `size` | number? | The game's size class; doc comment says 1 to 6 | 1–6, plus 10 on the three `VNCL_Mauler*` records | `AttachDef.Size` |
| `isGravlev` | true? | | 15 | `isGravlevVehicle` |
| `boundingBox` | `{x, y, z}` | Metres (doc comment) | 1104 | `maxBoundingBoxSize` |
| `hull.damageNormalization` | number? | Hull damage normalisation; real hull HP is in the vehicle XML, not DataCore (ships.rs:20) | 800 – 5,000,000; on all 1104 | `vehicleHullDamageNormalizationValue` |
| `hull.resistances` | damage map? | Hull multipliers that differ from 1 | 12 vehicles (all 0, e.g. `AEGS_Idris_P_FW_25`) | `SHealthComponentParams.DamageResistances` |
| `crossSection` | `{x, y, z}`? | Radar cross section per axis, m² per the doc comment | 1053 | `crossSectionParams.crossSection` |
| `loadout` | object? | Stock loadout by item type, see below | 1088 | default loadout |
| `totals` | object? | Sums over the loadout, see below | 1088 | derived |
| `flight` | object? | Stock flight controller numbers, see below | 1029 (35 spaceships and all 40 ground vehicles have none) | `IFCSParams` |
| `armor` | object? | Armour item: `item`, `health`, `deflection`, `damageMultiplier`, `signals {signalInfrared, signalElectromagnetic, signalCrossSection}` | 1074 | armour component (`parse_armor`, ships.rs:407) |
| `powerPools` | object? | Power pips per item type, positive fixed pools only | 1097, all `{"WeaponGun": n}` | `itemPools[].poolSize` |
| `capabilities` | object | Always present, see below | 1104 | derived |
| `isTemplate` | true? | Name is a placeholder | 5 | |

### `loadout`

List slots hold `{item, name?, size?, count?}` (identical entries collapse with `count`, `count_duplicates` ships.rs:336). `item` is the item record short name.

| Key | Holds | Extra fields | Vehicles with it |
|---|---|---|---|
| `shields`, `powerPlants`, `coolers`, `quantumDrives`, `jumpDrives`, `radars`, `lifeSupport`, `quantumFuelTanks`, `cargoGrids` | items of that `AttachDef.Type` | | 1065, 1087, 1075, 986, 924, 1073, 1032, 985, 436 |
| `quantumEnforcement` | `QuantumInterdictionGenerator` items | | 41 |
| `fuelTanks` | `FuelTank` and `ExternalFuelTank` | | 1035 |
| `armor` | single object, the `Armor` item | | 1074 |
| `weapons` | `WeaponGun`, `WeaponMining`, `TractorBeam`, `EMP` | `type`, `mount` (parent item), `dps` | 1038 |
| `missiles` | `Missile` items | `rack` (parent item), `damage` (total of the missile's damage map) | 758 |
| `countermeasures` | `WeaponDefensive` | `ammo` (ammo record), `capacity` (`maxAmmoCount`) | 1026 |
| `thrusters` | counts `{main, retro, maneuver}` | retro = `SubType` Retro or port name containing "retro" | 1039 |
| `turrets` | count of `Turret` / `TurretBase` / `UtilityTurret` items | | 1007 |
| `missileRacks` | count of `MissileLauncher` / `GroundVehicleMissileLauncher` / `BombLauncher` items | | 775 |

A gun nested under another gun is skipped (AI variants keep a stale child entry, ships.rs:485).

### `totals` (all derived, rounded to 2 dp)

`weaponDps` (Σ `dps` × count; 0.8 – 373,726.8), `weapons` and `missiles` (item counts), `missileDamage` (Σ damage × count), `shieldHealth` / `shieldRegen` (Σ `MaxShieldHealth` / `MaxShieldRegen`; 720 – 8,448,000 / 228 – 619,520), `armorHealth` (armour item `health`; 360 – 220,320), `powerOutput` (Σ `powerOutput`; 10 – 290), `fuel` / `quantumFuel` (Σ `standardCargoUnits`), `cargoScu` (Σ cargo grid `scu`; 1 – 4608), `thrustMain` / `thrustRetro` / `thrustManeuver` (Σ `thrustCapacity` by role, counted per loadout entry), `countermeasures` (`{flare, chaff, ...}`: Σ capacity by the ammo name's last `_` word).

### `flight`

| Field | Meaning | Range | Source (`IFCSParams.`) |
|---|---|---|---|
| `controller` | Flight controller record short name | | |
| `scmSpeed`, `maxSpeed`, `boostSpeedForward`, `boostSpeedBackward` | Speeds; unit not stated | scm 22 – 900, max 523 – 1575 | same names |
| `pitch`, `yaw`, `roll` | Degrees per second (doc comment); IFCS x = pitch, y = roll, z = yaw | | `maxAngularVelocity`, falling back to `speedProfile.angularVelocity` (a 60/200/60 placeholder on most hulls, ships.rs:376) |
| `boostCapacitor {max, regenPerSecond, regenDelay}` | | | `afterburnerNew.capacitorMax / capacitorRegenPerSec / capacitorRegenDelayAfterUse` |
| `stunThresholdGs` | G-load that stuns the pilot | 50 on every LIVE vehicle | `ifcsCoreParams.stunThresholdGs` |

### `capabilities`

Eleven booleans plus `cargo_scu` (number or `null`) and `cargoKnown: false` when `cargo` is false. Rules (ships.rs:147): `mining` = a `WeaponMining` in `loadout.weapons` or role contains "mining"; `salvage` / `refuel` = role contains "salvage" / "refuel"; `tractor` / `emp` = `TractorBeam` / `EMP` in weapons; `guns` = `WeaponGun` or turrets; `scan`, `quantum`, `qed`, `missiles` = non-empty `radars`, `quantumDrives`, `quantumEnforcement`, `missileRacks`; `cargo_scu` = `totals.cargoScu` when > 0; `cargo` = `cargo_scu` set or `cargoGrids` non-empty. LIVE true counts: mining 27, salvage 34, refuel 23, tractor 136, guns 1056, scan 1073, quantum 986, qed 41, emp 21, missiles 775, cargo 436; `cargoKnown: false` 668.

### Example (LIVE, published shape, trimmed)

Trimmed: `loadout` keeps 7 of the Gladius's 16 keys and one entry per list, `totals` 7 of its 14 keys, and `flight` drops `boostSpeedForward` / `boostSpeedBackward`.

```json
{
  "name": "Aegis Gladius",
  "recordName": "EntityClassDefinition.AEGS_Gladius",
  "id": "b6b59889-7226-458e-a6b0-1c9392128a3c",
  "displayName": "Aegis Gladius",
  "nameKey": "vehicle_NameAEGS_Gladius", "descriptionKey": "vehicle_DescAEGS_Gladius",
  "manufacturer": "aegs", "movementClass": "Spaceship", "career": "Combat", "role": "Light Fighter",
  "crewSize": 1, "size": 2,
  "boundingBox": {"x": 17.5, "y": 21, "z": 5.5},
  "hull": {"damageNormalization": 2134},
  "crossSection": {"x": 6923, "y": 1731, "z": 8654},
  "loadout": {
    "shields": [{"item": "SHLD_GODI_S01_AllStop_SCItem", "name": "AllStop", "size": 1, "count": 2}],
    "weapons": [{"item": "KLWE_LaserRepeater_S3", "name": "CF-337 Panther Repeater", "size": 3, "type": "WeaponGun", "mount": "mount_gimbal_s3", "dps": 545.63, "count": 2}],
    "missiles": [{"item": "MISL_S03_CS_FSKI_Arrester", "name": "Arrester III Missile", "size": 3, "rack": "MRCK_S03_BEHR_Single_S03", "damage": 3200, "count": 2}],
    "countermeasures": [{"item": "AEGS_Gladius_CML_Flare", "name": "Aegis Gladius - Decoy Launcher", "size": 1, "ammo": "behr_flare", "capacity": 48}],
    "thrusters": {"main": 2, "retro": 2, "maneuver": 8}, "turrets": 3, "missileRacks": 4
  },
  "totals": {"weaponDps": 1944.59, "shieldHealth": 6336, "armorHealth": 3300, "powerOutput": 16, "fuel": 1.6, "quantumFuel": 0.6, "countermeasures": {"flare": 48, "chaff": 5}},
  "flight": {"controller": "Controller_Flight_AEGS_Gladius", "scmSpeed": 226, "maxSpeed": 1193, "pitch": 68, "yaw": 52, "roll": 200,
    "boostCapacitor": {"max": 20, "regenPerSecond": 0.75, "regenDelay": 0.2}, "stunThresholdGs": 50},
  "armor": {"item": "ARMR_AEGS_Gladius", "health": 3300, "deflection": {"physical": 11, "energy": 9, "distortion": 0, "thermal": 0, "biochemical": 0, "stun": 0},
    "damageMultiplier": {"physical": 0.75, "energy": 0.6, "distortion": 1, "thermal": 1, "biochemical": 1, "stun": 1},
    "signals": {"signalInfrared": 1.13, "signalElectromagnetic": 1.13, "signalCrossSection": 1.13}},
  "powerPools": {"WeaponGun": 4},
  "capabilities": {"mining": false, "salvage": false, "refuel": false, "tractor": false, "guns": true, "scan": true, "quantum": true,
    "qed": false, "emp": false, "missiles": true, "cargo": false, "cargo_scu": null, "cargoKnown": false}
}
```

## Derived, estimated and curated values

No `Estimate` fields and no `_assumptions`; no curated sources. Derived (not copied):

- **`totals`**: sums over the stock loadout, through game-ship-components stats. `cargoScu` rests on cargo grid `scu`, itself approximate (interior volume ÷ 1.953125 m³; see [game-ship-components](game-ship-components.md)).
- **`loadout.weapons[].dps`** and so `totals.weaponDps`: per-shot damage × pellets × damage multiplier × fire rate ÷ 60, the best fire mode, bursts averaged over their cooldown (`fire_mode_dps`, `helpers_ships/weapons.rs:198`). Heat, ammo and power limits are not applied, so it is a peak figure.
- **`loadout.missiles[].damage`**: total of the missile's damage map.
- **`capabilities`**: rule-derived flags (sc-telemetry's rules, not the game's); `cargoKnown: false` marks "no cargo data", not "no cargo".
- **`career` / `role` fallback**: when the string is a placeholder, the key tail stands in.
- **`flight` angular rates**: prefer `maxAngularVelocity`, fall back to the speed profile.

## Redaction

Publish removes `description` from every object (only `vehicles[].description` exists) and inserts `"_redacted": ["description"]` after `_build` (`PROSE_FIELDS` and `redact_prose`, `crates/sc-gamedata/src/localization.rs`, redaction module). `descriptionKey` stays, so the text is one string-table lookup away; the published string table keeps name keys only.

## Consumers

- **sc-telemetry** `sctelemetry/shipcaps.py`: `build_table` (`:167`) reads `recordName`, `displayName` / `name`, `role`, `career`, `manufacturer`, `size`, `crewSize`; `vehicle_caps` (`:129`) prefers `capabilities` per key and honours `cargoKnown: false`; `derive_caps` (`:82`) falls back to `loadout.weapons[].type`, `loadout.turrets/radars/quantumDrives/quantumEnforcement/missileRacks/cargoGrids`, `totals.cargoScu`. `gamedata_hub.py:35` fetches it from the hub; `server.py:1149` takes it as `--gamedata`.
- **MCsim** `persim/scripts/build_data.py:291` (`ship_record`: `name`, `flight.scmSpeed/boostSpeedForward/maxSpeed/pitch/yaw/roll`, `totals.shieldHealth/armorHealth`) and `:310` (`ship_bbox`: `boundingBox`). `build_ordnance.py:231` (`movementClass`, `crossSection`, `displayName`, `size`, `role`, `boundingBox`, `loadout` emitter slots + `countermeasures` `ammo/capacity/count` + `missiles` `item/count/rack`, `armor.signals`, `flight` rates and `scmSpeed`, `totals`). `build_fire.py:156` (`movementClass`, `loadout` slots, `armor.health/deflection/damageMultiplier`, `totals.shieldHealth`) and `:268` (`_build`).
- **snares** `scripts/build-data.mjs:36`, `interdictors()` `:135-151`: `recordName`, `displayName`, `loadout.quantumEnforcement[0].item`, `loadout.quantumDrives[0].item`.
- **SCMCP** `src/versions.ts:32` diffs by `vehicles`; `src/itemfinder.ts:74` builds search entries from `name`, `recordName`, `manufacturer`, `role`, `career`, `isTemplate` (`ShipLike`, `src/vendor/sc-gamedata/fuzzy.ts:47`) when no search index is published.
- **sc-gamedata** itself: `search_index.rs:168-185` (same six fields) and `mcp.rs:46`.
- **starorg-codex** (branch `feat/dataset-reuse`): the dump reader lists the file in `DUMP_USE` (`crates/starorg-mcp/src/prebuilt.rs:180`): schema version 1, top-level keys `vehicles`, `byManufacturer`, `byRole` and `summary` required, `description` the only field `_redacted` may name (the app fills it from its local string table). `crates/starorg-core/src/model/ships.rs:33` models the record (`recordName`, `id`, `manufacturer`, `movementClass`, `crewSize`, `size`, `isGravlev`, `boundingBox`, `hull`, `loadout`, `totals`, `flight`, `armor`, `isTemplate`).
- Contracts from [consumer-datasets](../plans/specs/consumer-datasets.md):
  - stays at `data/game-ships.json`; `_build` equals the tag and equals `game-fire._build` (MCsim);
  - `_schema` stem `game-ships`; `build.json` `files[]` size and sha256 match;
  - starorg-codex (not in the spec; from its `DUMP_USE`): the four top-level keys stay, and `_redacted` names nothing but `description`;
  - `vehicles[].capabilities` keeps the key names `cargo_scu` and `cargoKnown`; tell sc-telemetry before renaming (verified 2026-11-25: 0 mismatches against `derive_caps` over 1104 vehicles).

## Derivable data

The game data is CIG's: we don't add to it, we derive from it. More that the shipped records already hold:

### Fields the source records carry that the dataset does not read

From `extracted-data/libs/foundry/records/entities/spaceships/aegs_gladius.json` (dated 2026-11-24, build 4.10.201.48085) unless noted:

- **Role and career by reference.** `VehicleComponentParams.vehicleRoleRef` / `vehicleCareerRef` point to `vehicle/roles/*.json` / `vehicle/careers/*.json` (Gladius: `roles/lightfighter.json`, `careers/combat.json`). Where the localised text is `procedural_text_null` (global.ini has `procedural_text_null=` empty), the reference still names one: `MISC_Starfarer_Gemini_Derelict_Nose` → `roles/heavyrefuelling.json`, `careers/support.json`. The `vehicle/` records are outside the filters today and are not in `extracted-data`, so what a role record holds (a display-name key or only its file name) is not established; the file stem alone already gives a classifier.
- **Dynamic power pools.** `resourceNetworkPowerPools.itemPools[]` mixes `FixedPowerPool` (`WeaponGun` `poolSize` 4) with `DynamicPowerPool` entries that have `maxItemCount` instead of `poolSize` (Gladius: `Shield` 2; `FlightController`, `TractorBeam`, `TowingBeam`, `WeaponMining`, `SalvageHead` −1). `parse_power_pools` keeps positive `poolSize` only, which is why every LIVE `powerPools` is `{"WeaponGun": n}`.
- **Emergency thresholds.** `emergencyStatusDamageThreshold` 0.25 and `emergencyStatusChargeThreshold` 0.8. Meaning not established beyond the names.
- **Other `VehicleComponentParams`**: `landingSystem` (→ `scitem/ships/landingsystem/aegs_gladius_landingsystem.json`), `dogfightEnabled`, `allowSoftDestruction`, `aiDamageModifiers` (→ `vehiclecombat/aidamagemodifiers/vehicleaidamagemodifier_one.json`), `objectContainers[].fileName` (interior socpaks, e.g. `objectcontainers\ships\aegs\gladius\base_int_all_main.socpak`), `cargoEntrySize` (0, 0 on the Gladius).
- **Damage model inputs.** `componentPenetrationDamageMultiplier` 0.4, `fusePenetrationDamageMultiplier` 0.0, `criticalPartExplosionChance` 0.0, `vehicleArmourModifiers[]` (`modifier` 0.0 for `Shield`, `Radar`, `PowerPlant`, `Cooler`, `LifeSupportGenerator`; 0.2 for turrets, thrusters, guns and `WeaponDefensive`), `typesToHitWithExplosions[]` (11 item types) and `aiEngineeringOptions` (`penetrationCanDamagePowerPlants` true, `powerPlantsCanExplodeOnDeath` false, `canExplodeFromHullDamage` false). Meaning not established beyond the names; MCsim's fire and damage pages would be the consumer.
- **Hull HP.** `vehicleDefinition` = `scripts/entities/vehicles/implementations/xml/aegs_gladius.xml`, an archive file. The module comment (ships.rs:20) says the real hull figure is there; the extractor does not read it, so hull part health is a derivable gap, not an invention.
- **Wear.** `SDegradationParams.accumulators[0].MaxLifetimeHours` 25.0 (vehicle-level; components already expose theirs).
- **Cargo handling.** `CargoControllerParams.cargoRequiresDocking`, `loadBatchAmount.microSCU` 1, `unloadBatchAmount.microSCU` 1.
- **Shop classification.** `SCItemPurchasableParams.displayType` (`@vehicle_class_lightfighter`).

### Values computable with other datasets

- **Manufacturer names**: `manufacturer` (lower-case record name) → [game-manufacturers](game-manufacturers.md). sc-telemetry keeps its own `MANUFACTURERS` table (`shipcaps.py`) for this; a resolved `manufacturerCode` / `manufacturerName` would retire it.
- **Variant families**: vehicles sharing a class (`AEGS_Reclaimer`, `_PU_AI_CIV`, `_Teach`, ...) are merged by sc-telemetry (`_merge`) and de-duplicated by MCsim (shortest record id per display name). A `baseClass` derived from the record (e.g. shared `vehicleDefinition` XML path) would give all tools one grouping. Grouping by name pattern instead would be curated, not derived.
- **Where a ship is rewarded**: join `recordName` / `id` to [game-wikelo-trades](game-wikelo-trades.md) rewards. [game-item-obtain](game-item-obtain.md) drops the 27 Wikelo vehicle rewards because they have no `game-item-tags` row (see that note). Shop stock and prices are set by the server ([game-shops](game-shops.md)), so where a ship is sold cannot be derived from the files.
- **Per-ship fire parts, salvage storage, signature totals**: see [game-fire](game-fire.md), [game-salvage](game-salvage.md), [game-ship-components](game-ship-components.md).
- **Missile and bomb fits**: every rack a vehicle references and the sizes it takes ([consumer-datasets](../plans/specs/consumer-datasets.md) section 3, `game-ordnance`).

### Parse gaps

- **Salvage heads dropped.** `GUN_TYPES` (ships.rs:364) has no `SalvageHead`, so salvage heads never reach `loadout.weapons` and cannot set `capabilities`. 34 LIVE vehicles carry `salvage_head_standard` / `salvage_head_salvation` in their stock loadout; 33 get `salvage: true` from the role anyway, but `GAMA_Hyun` (role "Generalist", two salvage arms with `salvage_head_standard`) gets `salvage: false`.
- **Bombs dropped.** Only `Missile` items are listed (`parse_loadout`, ships.rs:556); `Bomb` items fall through. `CRUS_Spirit_A1` references `bomb_s05_fski` on its `bmbrck_crus_spirit_a1` rack, yet `loadout.missiles` lists only 4 Arrester III and `totals.missileDamage` is 12,800. Bomb racks still count in `missileRacks` (3 on the A1).
- **Other item types without a slot.** `parse_loadout` has no arm for `SalvageModifier`, `SalvageInternalStorage` or `Battery` either. `GAMA_Hyun` references `salvage_modifier_scraper_medium` twice and `salvageinternalstorage_gama_hyun`; neither reaches `loadout`. No LIVE vehicle record references a `BATT_*` item, though MCsim asks for a `batteries` slot (`build_fire.py:39`).
- **Placeholder roles.** 14 derelict / wreck records (`DRAK_Cutlass_Black_AI_Derelict_*`, `MISC_Starfarer_Gemini_Derelict_*`) have career and role `procedural_text_null`, and 3 templates `LOC_PLACEHOLDER`; they land in `byRole`. The role references above would fix them.
- **Size outside the documented range.** `size` is documented as 1 to 6 but the three `VNCL_Mauler*` records have 10.
- **No flight block** on 35 spaceships (templates, comms probes, derelicts, wrecks, `DRAK_Cutlass_Black_PU_Boarding`, `VNCL_Hunter_AI_VAN`, five `RSI_Apollo_Medivac_*` variants, `EAObjectiveDestructable_Satellite`) and on all 40 `ArcadeWheeled` ground vehicles: `flight` is set only from a `FlightController` item with `IFCSParams` in the stock loadout.
- **`powerPools`** shows fixed pools only (above).

Related specs: [consumer-datasets](../plans/specs/consumer-datasets.md) sections 3–4, [dump-enhancements](../plans/specs/dump-enhancements.md), [dump-reuse](../plans/specs/dump-reuse.md), [tool-data-sync](../plans/specs/tool-data-sync.md).
