---
title: game-fire (game-fire.json)
tags: [docs, datasets]
updated: 2026-11-25
---

# game-fire (game-fire.json)

The fire-hazard rules every fire obeys (ignition by damage type, oxygen window, spread, room heat, player damage, extinguishing) and every ship item that can start or feed one: ignite rolls on entering a health state, thermal death explosions, fire misfires, overheat and misfire temperatures, the item cooling law, fuel tanks with a health pool, and the hit detonation of missiles, bombs and mines. On LIVE 4.10.201.48085 it holds 1 rules object and 523 items; the file is 393,573 bytes minified. The `_schema` key (since tags 4.10.201.48085 / 5.0.202.35026, commit `b9e3700d0`) is not in that figure; it adds 24 bytes (393,597). PTU 5.0.202.35026 has 536 items (80 missiles instead of 67) and the same rules.

## Source

- DataCore records, picked by the filters `**/roomsystem/firehazard/**` and `**/entities/scitem/ships/**` (`crates/sc-gamedata/src/domains/fire.rs:525`):
  - `libs/foundry/records/roomsystem/firehazard/firehazardglobalparams.json` (`_Type_` `FireHazardGlobalParams`) → `rules`, `parse_fire_rules` (fire.rs:254).
  - Every `EntityClassDefinition` under `entities/scitem/ships/` → `items[]`, `parse_fire_item` (fire.rs:461). Components read: `SAttachableComponentParams.AttachDef` (identity), `SHealthComponentParams` (`Health`, `DamageResistances`, `DeathExplosionParams`), `SInteractionStateMachineParams` (`FireHazardIgniteEvent` dispatchers, fire.rs:306), `EntityComponentMisfireParams` (fire.rs:341), `SEntityPhysicsControllerParams.PhysType.temperature` (fire.rs:388), and `SCItemMissileParams` / `SCItemBombParams` / `SCItemSpaceMineParams` `explosionParams` (fire.rs:449).
- Localization (`Data/Localization/english/global.ini`) for item names (`needs_localization`, fire.rs:529).
- Inclusion rule (fire.rs:472): an item is listed when it has an ignite event, a death explosion with a thermal part, a misfire that can pick `SFireMisfireEffect`, a damaging hit explosion, or it is a `FuelTank` / `QuantumFuelTank` / `ExternalFuelTank` with health or a death explosion (`FUEL_TANK_TYPES`, fire.rs:446).
- Derives from no other dataset. Items are sorted by type, then size, then record name (fire.rs:544).

## Shape

Top-level keys: `_source`, `_extracted`, `_build`, `rules{}`, `items[]`, `summary{}`. `_schema: "game-fire@1"` after `_build` (since tags 4.10.201.48085 / 5.0.202.35026), and an entry in `build.json` `files[]`.

Per-damage-type maps use the six keys `physical`, `energy`, `distortion`, `thermal`, `biochemical`, `stun`. In `rules` they come from positional arrays in that order (`per_type`, fire.rs:224).

### `rules`

| Field | Type | Meaning | Units/range (LIVE) | Source (`FireHazardGlobalParams.`) |
|---|---|---|---|---|
| `fixedFPS` | number | Meaning not established beyond the name (MCsim reads it as the fire tick rate in Hz) | 1 | `update.fixedFPS` |
| `globalIgnitionChanceMultiplier` | number | Meaning not established beyond the name | 1 | `ignition.globalIgnitionChanceMultiplier` |
| `globalFlashIgnitionThresholdMultiplier` | number | Meaning not established | 1 | `ignition.globalFlashIgnitionThresholdMultiplier` |
| `explosionChanceMultiplier` | damage map | Per damage type; meaning not established | thermal 0.25, rest 0 | `ignition.explosionChanceMultiplier[6]` |
| `projectileChanceMultiplier` | damage map | Per damage type | all 0 | `ignition.projectileChanceMultiplier[6]` |
| `collisionChanceMultiplier` | damage map | Per damage type | all 0 | `ignition.collisionChanceMultiplier[6]` |
| `fallbackChanceMultiplier` | damage map | Per damage type | all 0 | `ignition.fallbackChanceMultiplier[6]` |
| `damageTypeIgnitionModifiers` | damage map | Per damage type | thermal 1, rest 0 | `ignition.damageTypeIgnitionModifiers[6]` |
| `oxygenPressureRange` | `{min, max}` | Always present, may be empty | 0.1 – 0.15; unit not stated | `propagation.oxygenPressureRange.minimum/maximum` |
| `fireMassThreshold` | number | Meaning not established | 0.75 | `propagation.fireMassThreshold` |
| `maximumRadiationRadius` | number | Meaning not established | 4 | `propagation.maximumRadiationRadius` |
| `maximumConvectionDistance` | number | Meaning not established | 10 | `propagation.maximumConvectionDistance` |
| `standardConvectionTemperature` | number | Meaning not established | 900 | `propagation.standardConvectionTemperature` |
| `roomConnectorIgniteThreshold` | number | Meaning not established (MCsim reads it as the door ignite threshold) | 200 | `roomConnector.igniteThreshold` |
| `damageToPlayers` | object | `minimumDamageTemperature`, `baseDamage`, `referenceHealth`; always present | 300, 12, 100 | `damageToHealth.*` |
| `extinguishing` | object | `oxygenReduction`, `heatTransferMultiplier`; always present | 0.21, 3000 | `extinguishing.*` |

### `items[]`

| Field | Type | Meaning | Units/range (LIVE) | Source |
|---|---|---|---|---|
| `name` | string | Localised name, or the record short name when the name is a placeholder | 523/523 | `AttachDef.Localization.Name` (`item_identity`, helpers_items.rs:117) |
| `isTemplate` | true? | Set when the name is a placeholder | 58 | identity |
| `nameKey`, `descriptionKey` | string? | String-table keys | 523 / 475 | `Localization.Name/Description` |
| `nameStatus` | string? | `placeholder` / `unresolved` / `empty`; absent when real | `placeholder` 55, `unresolved` 3, no `empty` | `loc_refs` |
| `recordName`, `id` | string | `EntityClassDefinition.<short>`, record GUID | 523 | record |
| `type`, `subType` | string? | `AttachDef.Type` / `SubType` (`UNDEFINED` dropped) | `type` 523 (13 values, see `summary.byType`); `subType` 240 | `AttachDef` |
| `size`, `grade` | number? | `AttachDef.Size` / `Grade` | 523 / 523 | `AttachDef` |
| `manufacturer` | string? | Referenced manufacturer record name, upper case | 471 of 523 | `AttachDef.Manufacturer` |
| `health` | number? | Item health pool | 25 – 420,000; absent on 2 | `SHealthComponentParams.Health` |
| `damageMultipliers` | damage map? | `DamageResistances.<Type>Resistance.Multiplier`, all six kept (unlike game-ship-components, which keeps only values ≠ 1) | 521 items, six keys each | fire.rs:427 |
| `ignite[]` | array? | One entry per `FireHazardIgniteEvent` on entering a state | 401 items; every LIVE entry has `state` "Destroyed" | fire.rs:306 |
| `ignite[].state` | string | `StateName` of the state the event fires on | "Destroyed" | |
| `ignite[].belowHealthRatio` | number? | The state's `stateRanges.max` from the state modifiers | 0.01 on LIVE | |
| `ignite[].chance` | number | `ignitionChance` ÷ 100 | 0.2 or 1 | converted |
| `ignite[].strength`, `ignite[].radius` | number? | `ignitionStrength`, `ignitionRadius` | strength 100 on every LIVE entry; radius 0.125 – 4 | |
| `deathExplosion` | explosion? | `DeathExplosionParams` with zero damage types dropped; omitted when nothing is dealt | 188 items, 155 with thermal | fire.rs:290 |
| `hitExplosion` | explosion? | Explosive's detonation on hit, from its missile / bomb / mine `explosionParams` | 72 items (67 missiles, 3 bombs, 2 mines) | fire.rs:456 |
| explosion `.damage` | damage map | Non-zero types only | e.g. thunderball 12500 physical + 14500 energy | |
| explosion `.minRadius`, `.maxRadius` | number? | | metres not stated | |
| `misfire` | object? | Present when the item's misfire list includes an `SFireMisfireEffect` | 215 items | fire.rs:341 |
| `misfire.fireByLevel` | number[4] | Per misfire level, the weight share of picks that include the fire effect (rounded 4 dp) | 0 – 1, always 4 levels on LIVE | derived |
| `misfire.trigger` | object? | First `triggerConditions` entry: `degradation`, `damage`, `heat`, `distortion` | e.g. 1, 0.9, 0.6, 0.6 | |
| `misfire.window` | `{min, max}` | `minWindowLength` / `maxWindowLength` | 15 – 120 or 45 – 600 on LIVE | |
| `overheat` | object? | `warning`, `at`, `recovery` temperatures; only when `enableOverheat` is truthy | 245 items | `itemResourceParams.overheat*Temperature` |
| `misfireTemperature` | `{min, max}`? | Only when `max` is non-zero | 506 items | `temperature.misfireTemperatureRange` |
| `temperature` | object? | Cooling law: `enabled`, `coolingEqualizationRate`, `temperatureDifference`, `heatGen`, `minCoolingTemperature` | 406 items; `heatGen` 0 – 12.5 | fire.rs:388 |

### `summary`

`rules` (0/1), `items` 523, `withIgniteRoll` 401, `withThermalDeathExplosion` 155, `withFireMisfire` 215, `withHitExplosion` 72, `byType` {Battery 6, Bomb 3, Cooler 84, ExternalFuelTank 31, FuelTank 2, JumpDrive 12, LifeSupportGenerator 12, Missile 67, PowerPlant 89, QuantumDrive 63, Radar 79, Shield 73, SpaceMine 2}.

### Example (LIVE, one item)

```json
{
  "name": "FoxFire",
  "nameKey": "item_Name_QRDV_ACAS_S01_Foxfire",
  "descriptionKey": "item_Desc_QRDV_ACAS_S01_Foxfire",
  "recordName": "EntityClassDefinition.QDRV_ACAS_S01_FoxFire_SCItem",
  "id": "dadc9318-d0f2-48c2-aad4-f2dd0c5f7fd3",
  "type": "QuantumDrive", "size": 1, "grade": 2, "manufacturer": "ACAS",
  "health": 120,
  "damageMultipliers": {"physical": 0.85, "energy": 0.85, "distortion": 1, "thermal": 0.1, "biochemical": 1, "stun": 1},
  "ignite": [{"state": "Destroyed", "belowHealthRatio": 0.01, "chance": 0.2, "strength": 100, "radius": 0.5}],
  "misfire": {"fireByLevel": [0, 0, 0, 1], "trigger": {"degradation": 1, "damage": 0.9, "heat": 0.6, "distortion": 0.6}, "window": {"min": 15, "max": 120}},
  "overheat": {"warning": 353, "at": 372, "recovery": 367},
  "misfireTemperature": {"min": 369, "max": 372},
  "temperature": {"enabled": true, "coolingEqualizationRate": 3.75, "temperatureDifference": 400, "heatGen": 0.95, "minCoolingTemperature": 308}
}
```

## Derived, estimated and curated values

No `Estimate` fields, no `_assumptions`, no curated sources. Values the domain computes rather than copies:

- `ignite[].chance`: the record's percentage divided by 100.
- `ignite[].belowHealthRatio`: joined from the state machine's `StateModifiers[].stateRanges` by state pointer.
- `misfire.fireByLevel`: a weight share, Σ weight of picks naming the fire effect ÷ Σ all weights, per level (fire.rs:351).
- `temperature.heatGen`: the largest `stateHeatGenerationValues[].baselineTemperatureChange` across states (one per state, e.g. a quantum drive's Online and Travelling; the code assumes only one is active, fire.rs:409).
- Explosion damage maps drop zero entries; `damageMultipliers` keeps all six.

## Redaction

Nothing. `game-fire.json` is not in `PROSE_FIELDS`. Items carry `nameKey` / `descriptionKey` only, no text beyond `name`.

## Consumers

- **MCsim** `persim/scripts/build_fire.py`:
  - `:265` needs `game-ships`, `game-ship-components`, `game-fire` and `game-weapons` in one data dir; `:268-271` aborts unless `game-ships._build == game-fire._build`.
  - `rules()` `:54` reads every `rules` field listed above by name.
  - `item_fire()` `:86` reads `health`, `damageMultipliers`, `ignite[]` (`state`, `belowHealthRatio`, `chance`, `strength`, `radius`), `deathExplosion`, `overheat` (`warning`, `at`, `recovery`), `misfireTemperature`, `temperature` (`coolingEqualizationRate`, `temperatureDifference`, `heatGen`, `minCoolingTemperature`, `enabled`) and `misfire` (`fireByLevel`, `trigger`, `window`).
  - `fire_index()` `:114` keys items by lower-cased `recordName` stem.
  - `explosives()` `:136` reads `hitExplosion` (falling back to `game-weapons` missile damage), `deathExplosion` and `size`.
  - `ships()` `:156` joins game-ships loadout slots to these items; parts without `health` are skipped.
- Contracts from [consumer-datasets](../plans/specs/consumer-datasets.md): `game-ships._build` must equal `game-fire._build` (publish writes one extract, so this holds). Sections 1 (fuel tanks) and 2 (`hitExplosion`) are built and in the LIVE data above; keep `hitExplosion` and the fuel-tank rows in that shape.
- No other checked consumer reads this file.

## Derivable data

The game data is CIG's: we don't add to it, we derive from it. More that the shipped records already hold:

### Fields the source records carry that the dataset does not read

From `firehazardglobalparams.json` (`extracted-data`, dated 2026-11-23, build 4.10.201.48085):

- `damageToHealth.curveAngle` 7.3 and `damageToHealth.curveOffset` 0.7. They sit beside `baseDamage` / `referenceHealth`, which MCsim already reads for player damage; the curve shape they define is not established, but a player-damage model is incomplete without them.
- `propagation.radiativeFactor` 0.3, `propagation.minimumHeatIntensityOutput` 5.0, `propagation.surfaceAreaBurnRateCurveVoxels` 1.0 and `surfaceAreaBurnRateCurveEntities` 1.0. Spread inputs next to the ones already read.
- `extinguishing.radiationAbsorption` 500.0.
- `smoke.dynamicPart` 100.0 and the gas records `smoke.defaultSmokeComposition` (`roomsystem/gascompositiontemplates/defaultsmoke.json`), `particulateMatter`, `waterVapor` (`roomsystem/gasparams/*`). Smoke composition is a join into `roomsystem` records the dataset does not read.
- `lightParams.maxHeatReleaseRate` 10000.0. Possibly the heat-release ceiling, but it sits in the light block; meaning not established.

Per item:

- `FireHazardIgniteEvent.offset` (Vec3, 0,0,0 on the FoxFire): where on the item the fire starts.
- `SHealthComponentParams.DamageResistances.<Type>Resistance.Threshold`, `DamageCap` and `DamageCapLimit` (all 0.0 on the FoxFire's `PhysicalResistance`), beside the `Multiplier` already read.

### Values computable with other datasets

- **Fire-relevant parts per ship** (reverse link): join [game-ships](game-ships.md) `loadout` slots (`powerPlants`, `coolers`, `shields`, `quantumDrives`, `jumpDrives`, `radars`, `lifeSupport`, `fuelTanks`, `quantumFuelTanks`, `missiles`) to `items[]` by record stem, with counts. MCsim builds exactly this in `build_fire.py:156`. Its `SLOTS` (`build_fire.py:39`) also asks for a `batteries` slot, which game-ships never writes (`Battery` items have no arm in `parse_loadout`); no LIVE vehicle record references a `BATT_*` item, so nothing is lost today. Bombs (3 `Bomb` items here) are not in game-ships `loadout` either, see [game-ships](game-ships.md). Publishing the per-ship list upstream would let other tools show "what can burn on this ship" without the join.
- **Hit vs death damage per explosive**: `hitExplosion` vs `deathExplosion` per missile, bomb and mine. MCsim computes both totals in `explosives()`; the 67 missile hit totals equal [game-weapons](game-weapons.md) `missiles[].damageTotal` (consumer-datasets section 2).
- **Time to overheat** (estimate): `temperature.heatGen` with the cooling law (`coolingEqualizationRate`, `temperatureDifference`, `minCoolingTemperature`) and `overheat.at`. MCsim's comment at `build_fire.py:104` reads it as dT/dt = rate × (Tenv − T) / diff; that law is MCsim's reading, not stated by the record, so any time-to-overheat figure is an estimate.
- **Self-repair after a fire**: game-ship-components `durability.selfRepair` joined by record stem (MCsim already joins `selfRepair` and `distortionMax` at `build_fire.py:183-194`).

### Parse gaps

- **Ship internal tanks.** 340 tanks (183 `FuelTank`, all 157 `QuantumFuelTank`, each ship's `HTNK_*` / `QTNK_*`) have no `SHealthComponentParams`, so they are not listed (consumer-datasets section 1). That is the records, not the parse: there is nothing to read.
- **Placeholder names.** 58 of 523 items are `isTemplate` (e.g. `MISC_FuelRefinery_L`, `BATT_AMRS_S00_FortCell`); MCsim falls back to its own `pretty()` names. game-ship-components or the string table may hold nothing better either.
- **Only "Destroyed" ignites.** Every LIVE `ignite[]` entry is on the `Destroyed` state. The parser reads any state, so this reflects the records.
- **Manufacturer missing** on 52 items (38 of them placeholder-named templates).

Related specs: [consumer-datasets](../plans/specs/consumer-datasets.md) sections 1–3, [tool-data-sync](../plans/specs/tool-data-sync.md) step 6, [rust-pipeline](../plans/specs/rust-pipeline.md).
