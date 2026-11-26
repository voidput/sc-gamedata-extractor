---
title: game-weapons (game-weapons.json)
tags: [docs, datasets]
updated: 2026-11-25
---

# game-weapons (game-weapons.json)

The combat baseline. Every ship weapon and personal (FPS) weapon is joined to the ammunition it fires, with the derived numbers a loadout tool shows: DPS, alpha, fire rate, range, spread, power, EM, ammo capacity, HP, heat and capacitor sustain. Missiles and torpedoes are listed too. On LIVE 4.10.201.48085 it holds 241 `shipWeapons` (219 with a DPS), 492 `fpsWeapons` (360 with a DPS) and 67 `missiles`, indexed against 249 ammo records. Published size: 968,325 bytes minified (`jq -c . | wc -c`). The `_schema` key (since tags 4.10.201.48085 / 5.0.202.35026, commit `b9e3700d0`) is not in that figure; with it the same command prints 968,352.

## Source

- **Records** (domain filters, `crates/sc-gamedata/src/domains/weapons.rs:818`):
  - `**/entities/scitem/ships/weapons/**`: ship guns, mining lasers, tractor and salvage heads, rocket pods, and the `missiles/` folder.
  - `**/entities/scitem/weapons/**`: FPS weapons, magazines, melee and gadgets.
  - `**/ammoparams/**`: `AmmoParams` records, indexed by lower-cased short name (`index_ammo`, `weapons.rs:557`).
- **Split** (`Weapons::parse`, `weapons.rs:830`):
  - A record under `/scitem/ships/weapons/` that has `SCItemMissileParams` becomes a missile (`parse_missile`, `weapons.rs:747`).
  - Any other record there that has `SCItemWeaponComponentParams` becomes a `ship` weapon.
  - A record under `/scitem/weapons/` that has `SCItemWeaponComponentParams` becomes an `fps` weapon (`parse_weapon`, `weapons.rs:611`).
  - Records with neither component are dropped silently.
- **Ammo link** (`weapons.rs:618-636`):
  - A ship gun's own `SAmmoContainerComponentParams.ammoParamsRecord` comes first.
  - Otherwise, the FPS default loadout entry whose `itemPortName` contains `magazine` (`default_magazine_name`, `weapons.rs:578`) gives the magazine entity by `entityClassName`. That magazine's `SAmmoContainerComponentParams` gives the ammo and `maxAmmoCount`.
- **Ammo parsing:** shared with [game-ammo](game-ammo.md) (`parse_ammo`, `crates/sc-gamedata/src/domains/ammo.rs:220`). Damage always comes from the ammo, never from the weapon.
- **Shared helpers** (`crates/sc-gamedata/src/helpers_items.rs`):
  - `display_name` and `manufacturer_of` (`:97`).
  - `resource_profile` (`:167`): the `ItemResourceComponentParams` state named `Online`, or the first state.
  - `durability_of` (`:362`), `volume_of` (`:396`) and `distortion_of` (`:418`).
- **Inputs from other datasets:** none. It shares parsing code with `game-ammo`. `game-fps-weapons` reads the same `/scitem/weapons/` records separately.

## Shape

Top-level keys: `_source`, `_extracted`, `_build`, `shipWeapons[]`, `fpsWeapons[]`, `missiles[]` and `summary`. `_schema: "game-weapons@1"` after `_build` (since tags 4.10.201.48085 / 5.0.202.35026).

`summary`: `{shipWeapons, shipWeaponsWithDps, fpsWeapons, fpsWeaponsWithDps, missiles, ammo}`. On LIVE it is `241, 219, 492, 360, 67, 249`. `summary.ammo` is the size of the ammo index, not the number of ammo records that a weapon uses (226 are used).

Order: `size` ascending, then `name` by locale comparison (`weapons.rs:797`).

### shipWeapons[] / fpsWeapons[] (struct `Weapon`, `weapons.rs:141`)

Optional fields are left out when unset. Ranges are LIVE values.

| Field | Type | Meaning | Units/range | Source |
|---|---|---|---|---|
| `name` | string | Localized display name. The record short name for a template. | | `AttachDef.Localization.Name` |
| `recordName` | string | `EntityClassDefinition.<short>` | | record |
| `id` | string | Record GUID | | record |
| `class` | `"ship"` \| `"fps"` | Which folder the record came from | | path |
| `isTemplate` | true? | Placeholder name, or the short name ends in `_prop` (store display props) | ship 6, fps 76 | `weapons.rs:616` |
| `nameKey`, `descriptionKey`, `nameStatus` | string | String-table keys, and `nameStatus` when the name is not real | | `loc_refs` |
| `type` | string? | `AttachDef.Type` | ship: WeaponGun 198, WeaponMining 24, TractorBeam 12, SalvageHead 6, TowingBeam 1. fps: WeaponPersonal 458, WeaponGun 4, absent on 30 (the props) | AttachDef |
| `subType` | string? | `AttachDef.SubType`, with `UNDEFINED` dropped | ship: Gun 213, Rocket 9, absent 19. fps: Medium 318, Small 81, Gadget 39, Large 20, Gun 4, absent 30 | AttachDef |
| `size`, `grade` | number | `AttachDef.Size` / `Grade` | ship size 0..12, fps 1..5. Grade is 1 everywhere on LIVE. Both absent on the 30 fps props | AttachDef |
| `manufacturer` | string | Referenced manufacturer record name, upper case | | AttachDef |
| `damageType` | string | Largest damage type of the ammo, the first on a tie | physical, energy, distortion, thermal | derived |
| `dps` | number | Headline mode's DPS: alpha × rate / 60. For beams it is the summed `damagePerSecond` | ship 0.8..200000, fps 0.68..341666.67 | derived |
| `alpha` | number | Damage per trigger pull: ammo `damageTotal` × pellets × `damageMultiplier` | | derived |
| `fireRate` | number | Headline mode's `sustainedFireRate`, or else its `fireRate` | per minute (the code calls it trigger pulls per minute). ship 6..1600 | fire action |
| `range` | number | Ammo `speed × lifetime`, or else the beam's `zeroDamageRange` | ship 15..12003 | derived |
| `speed` | number | Ammo `speed` | per second. Length unit not stated in code | ammo |
| `spread` | number | Headline mode's `spreadParams.min`, or else `.max` | Unit not stated in code. ship 0..4, fps 0..20 | `launchParams.spreadParams` |
| `penetration` | number | Ammo `basePenetrationDistance` | ship 0..66.69 | ammo |
| `power` | number | Consumption of resource `Power` in the Online state | `standardResourceUnits` per second. ship 0.1..5.6 | `ItemResourceComponentParams` |
| `em`, `ir` | number | Online state `EMSignature` / `IRSignature.nominalSignature` | em 0..3020. ir is 0 everywhere | `ItemResourceComponentParams` |
| `ammoCapacity` | number | Magazine or container `maxAmmoCount` when above 0, or else the capacitor `maxAmmoLoad` | | derived |
| `health` | number | `SHealthComponentParams.Health` | ship 100..750000. Absent on every FPS row | component |
| `durability` | object | `{health, resistances{type: multiplier ≠ 1}, selfRepair{count, seconds, healthRatio}, repairRestoreRatio, wear{maxLifetimeHours, damageConversionRate, overheatMultiplier}, misfireWindow{min, max}}`. `misfireWindow` is never present on LIVE weapon rows | | `durability_of` |
| `mass` | number | `SEntityPhysicsControllerParams.PhysType.Mass` | Unit not stated in code. ship 0..6870000 | component |
| `heatPerShot` | number | Headline mode's `heatPerShot` (`heatPerSecond` for beams) | | fire action |
| `overheatTemperature`, `coolingPerSecond` | number | `connectionParams.simplifiedHeatParams` | temperature is 100 on every row that has it | weapon params |
| `overheatSeconds` | number | `overheatTemperature / (heatPerShot × shots/s − coolingPerSecond)`, only when the net heat is above 0 | s. ship 8 rows, fps 47 | derived (`sustain`, `weapons.rs:283`) |
| `capacitor` | object | `{shots: maxAmmoLoad, regenPerSecond: maxRegenPerSec, regenCooldown: regenerationCooldown, costPerShot: regenerationCostPerBullet, requestedRegenPerSecond}`, kept only when `shots` is set | | `weaponRegenConsumerParams` |
| `burst` | object | `{shots, seconds, damage, limit}`. `limit` is `capacitor`, `heat` or `magazine` | ship: 119 capacitor, 8 heat, 63 magazine | derived (`sustain`) |
| `sustainedDps` | number | Damage over a full empty-and-refill cycle. See the next section | | derived |
| `volume` | number | `AttachDef.inventoryOccupancyVolume.microSCU` | micro-SCU | AttachDef |
| `distortion` | object | `{max, decayDelay, decayRate, warningRatio, recoveryRatio, secondsToRecover}`. `secondsToRecover` is derived (see the next section) | | `SDistortionParams` |
| `ammo` | object | The joined ammo (below) | | |
| `modes` | array | Every non-null entry of `fireActions` (below) | | `SCItemWeaponComponentParams.fireActions` |

`ammo` (struct `WeaponAmmo`, `weapons.rs:88`):

| Field | Type | Meaning | Units/range | Source |
|---|---|---|---|---|
| `recordName` | string | `AmmoParams.<short>` | | ammo record |
| `magazine` | string? | Magazine short name. FPS rows only | | default loadout |
| `speed`, `lifetime`, `range` | number | As in [game-ammo](game-ammo.md) | | ammo |
| `damage` | object | Non-zero damage types: `physical`, `energy`, `distortion`, `thermal`, `biochemical`, `stun` | | ammo |
| `damagePerShot` | number | Ammo `damageTotal` | | ammo |
| `penetration`, `penetrationRadius{near, far}`, `falloff{type: {minDistance, perMeter, floor}}`, `pierce{maxThickness, level1..3}` | | As in game-ammo | | ammo |
| `capacity` | number | Rounds in the container or magazine, when above 0 | | `maxAmmoCount` |

`modes[]` (struct `FireMode`, `weapons.rs:54`, built by `parse_fire_mode`, `weapons.rs:448`):

| Field | Type | Meaning | Source |
|---|---|---|---|
| `name` | string | Action `name`. For an unnamed sequence it is the nested action's name, or else `Sequence`. Any other unnamed action gets its `_Type_` with the `SWeaponActionFire` prefix and `Params` suffix removed (for example `SWeaponActionGatheringBeam`) | fire action |
| `kind` | string | `single`, `rapid`, `burst`, `charged`, `beam`, `sequence` or `other`. `other` covers tractor, salvage, healing, gathering and similar actions, which have no damage figure | `_Type_` |
| `fireRate` | number | Action `fireRate`. For a sequence it is 60 × total shots / total seconds over its entries (`sequence_fire_rate`, `weapons.rs:407`) | fire action |
| `heatPerShot`, `pellets`, `damageMultiplier`, `spread{min, max, attack, decay}` | number | `heatPerShot` and `launchParams` | fire action |
| `shotCount`, `sustainedFireRate` | number | Burst only: `60·n / (n·60/rate + cooldownTime)` | burst action |
| `chargeTime`, `chargedDamageMultiplier` | number | Charged only. `maxChargeModifier.damageMultiplier` is not applied to `alpha` | charged action |
| `fullDamageRange`, `zeroDamageRange` | number | Beam only | beam action |
| `alpha`, `dps` | number | As on the row, but for this mode | derived |

LIVE mode kinds. Ship: sequence 148, beam 48, other 24, rapid 21, single 15, charged 9. FPS: other 168, rapid 165, single 156, charged 66, burst 52, sequence 52, beam 23.

### missiles[] (struct `Missile`, `weapons.rs:218`)

| Field | Type | Meaning | Units/range | Source |
|---|---|---|---|---|
| `name`, `recordName`, `id`, `isTemplate`, `nameKey`, `descriptionKey`, `nameStatus` | | As above | `isTemplate` on 1 row | |
| `size`, `manufacturer` | | AttachDef | size 0..12 | AttachDef |
| `damage` | object | Non-zero `explosionParams.damage` types | | `SCItemMissileParams` |
| `damageTotal` | number | Sum of `damage`, to 2 decimals | 300..1230000 | derived |
| `damageType` | string | Largest type | | derived |
| `trackingSignal` | string | `targetingParams.trackingSignalType` | Infrared 25, CrossSection 24, Electromagnetic 18 | targeting |
| `lockTime`, `lockRangeMin`, `lockRangeMax`, `lockingAngle` | number | `targetingParams` | lockTime 0..10, lockRangeMin −1..3500, lockRangeMax −1..40000, lockingAngle 12..60. Both lock ranges are `-1` on 3 rows: the Venom rocket, `MISL_S10_Template` and `MISL_S12_Template` (meaning not established) | targeting |
| `speed`, `boostDuration` | number | `GCSParams.linearSpeed`, `boostPhaseDuration` | 230..1500. 0.5..2 s | GCS |
| `maxLifetime` | number | `maxLifetime` | 10..165 | missile params |
| `explosionRadius` | number | `explosionParams.maxRadius` | 0.8..65 | missile params |
| `health` | number | `SHealthComponentParams.Health` | 25..1000 | component |

### Example (trimmed, LIVE)

```json
{"name":"CF-337 Panther Repeater","recordName":"EntityClassDefinition.KLWE_LaserRepeater_S3",
 "id":"bd13235d-1261-4b71-b32f-aed1a8beb526","class":"ship","type":"WeaponGun","subType":"Gun",
 "size":3,"grade":1,"manufacturer":"KLWE","damageType":"energy","dps":545.63,"alpha":43.65,
 "fireRate":750,"range":1924,"speed":1480,"spread":0.6,"penetration":0.085,"power":1,"em":0,
 "ammoCapacity":75,"health":1500,"mass":200,"heatPerShot":0,
 "capacitor":{"shots":75,"regenPerSecond":15,"regenCooldown":0.74,"costPerShot":48.5,"requestedRegenPerSecond":3031},
 "burst":{"shots":75,"seconds":6,"damage":3273.75,"limit":"capacitor"},"sustainedDps":278.85,
 "volume":756000,
 "ammo":{"recordName":"AmmoParams.KLWE_LaserRepeater_S3_AMMO","speed":1480,"lifetime":1.3,"range":1924,
         "damage":{"energy":43.65},"damagePerShot":43.65},
 "modes":[{"name":"Single","kind":"sequence","fireRate":750,"pellets":1,"damageMultiplier":1,
           "spread":{"min":0.6,"max":0.6,"attack":0.022,"decay":2},"alpha":43.65,"dps":545.63}]}
```

## Derived, estimated and curated values

There is no `Estimate` suffix and no `_assumptions` block. The numbers below are computed by this crate, not read from the records:

- **Headline mode:** the mode with the highest `dps`, or the first mode when none has a DPS (`weapons.rs:646`). The row-level `dps`, `alpha`, `fireRate`, `spread` and `heatPerShot` all come from that one mode.
- **`alpha` and `dps`:** these assume every pellet hits and that no falloff applies.
- **`range`:** `speed × lifetime`. This ignores the `falloff` that the ammo row carries.
- **`sustain()`** (`weapons.rs:283`) chooses the limit in this order:
  - A capacitor with `shots` set gives `burst = shots / rate`, and `sustainedDps = pool × alpha / (burst + regenCooldown + pool / regenPerSecond)`.
  - Otherwise, heat, when overheating comes before the magazine empties: the cycle is `overheatSeconds + overheatFixTime`.
  - Otherwise, the magazine: `sustainedDps = alpha × shots/s`. The domain reads no reload time, and the weapon records checked carry none (the only `reload` key is the boolean `hasReloadModesOnUI`), so reload time is not counted. This is why one-round launchers show `sustainedDps` equal to `dps`. For example, the Boomtube shows 341666.67 at `fireRate` 500 with `ammoCapacity` 1.
- **`isTemplate`:** a placeholder name check, plus a name rule (`_prop` suffix).
- **`damageType`:** "the largest type wins" is a rule chosen by this crate, not a game field.
- **`distortion.secondsToRecover`:** `decayDelay + max × (1 − recoveryRatio) / decayRate`, to 2 decimals (`distortion_of`, `helpers_items.rs:418`). It is computed, not read.
- **Burst `sustainedFireRate`** and sequence `fireRate` are computed from shot counts and delays (see the modes table).

## Redaction

Nothing. `game-weapons.json` is not in `PROSE_FIELDS` (`crates/sc-gamedata/src/localization.rs:445`). The published file has no `_redacted` key, and records carry only `nameKey` and `descriptionKey`.

## Consumers

- **MCsim** (`/home/zedwil/git/persim`):
  - `scripts/build_fire.py:131`: `missiles[]` by `recordName` stem, for `name` and `damage`. These are used when `game-fire.hitExplosion` is absent (`:144-146`).
  - `scripts/build_fire.py:230-255`: `shipWeapons[]` with `type == "WeaponGun"` and `size >= 1`. It reads `ammo.damage`, `ammo.damagePerShot`, `modes[0].alpha`, `alpha`, `name`, `recordName`, `manufacturer`, `fireRate` and `ammo.penetration`.
  - `scripts/build_data.py:205-233, 289`: `shipWeapons[]` by `recordName`. It reads `ammo.recordName`, `ammo.speed`, `ammo.lifetime`, `ammo.range`, `modes[].spread`, `spread`, `name`, `size`, `fireRate`, `alpha`, `dps` and `ammoCapacity`. It opens the raw ammo record for `inheritVelocity`, because this dataset drops that field.
  - `scripts/build_ordnance.py:72-75`: `missiles[]` `recordName` to `name`.
- **starorg-codex** (`/home/zedwil/git/starorg-strings`, branch `feat/dataset-reuse`):
  - `crates/starorg-mcp/src/prebuilt.rs:185` needs `shipWeapons`, `fpsWeapons`, `missiles` and `summary` with schema 1.
  - `crates/starorg-mcp/src/tools.rs:826-863` routes `WeaponPersonal`, `WeaponGun`, `Missile`, `MissileLauncher`, `WeaponMining` and `Bomb` items here. It matches rows by `id`, or by the `recordName` tail.
  - `crates/starorg-gamedata/src/query/mining.rs:59-110` reads `shipWeapons[]` `name`, `recordName`, `type`, `size` and `modes[].kind/dps`. It takes the first `beam` mode's `dps` of S1/S2 `WeaponMining` rows as laser power.
- **SCMCP, snares, Bindscape, sc-telemetry:** no reference to this file.
- **Contracts** from [consumer-datasets](../plans/specs/consumer-datasets.md):
  - `_build` equals the tag (sc-telemetry checks every dataset).
  - The `_schema` stem must be `game-weapons`.
  - `game-fire` `hitExplosion` totals are checked to equal `missiles[].damageTotal` for all 67 missiles. Keep the explosion damage semantics in step.
  - The top-level keys starorg `needs` must stay.

## Derivable data

These come from records the game already ships. Nothing here adds to CIG's data.

**Fields the source records carry but the dataset does not read**

- **Bombs:**
  - `entities/scitem/ships/weapons/missiles/bomb_*.json` carry `SCItemBombParams`, not `SCItemMissileParams`, so `parse_missile` returns `None`. They are also not ship weapons, so all 3 bombs (`bomb_s03_fski_thunderball`, `bomb_s05_fski`, `bomb_s10_fski_colossus`) are dropped.
  - They carry `armTime`, `igniteTime`, `maxLifetime`, `explosionSafetyDistance`, and `explosionParams.{minRadius, maxRadius, damage}`. For example, `bomb_s05_fski` has `minRadius` 100, `maxRadius` 100 and `DamagePhysical` 22346 + `DamageEnergy` 24356.
  - The two `mine_test*` records are also skipped.
  - See the planned `game-ordnance` in [consumer-datasets](../plans/specs/consumer-datasets.md) section 3.
- **EMP devices:** `entities/scitem/ships/weapons/emp/*.json` (7 records) carry `SCItemEMPParams`. For example, `aegs_emp_device_s4` has `chargeTime` 20, `distortionDamage` 2750, `empRadius` 750, `minEmpRadius` 250, `unleashTime` 1.5 and `cooldownTime` 22. None of the three domain branches reads them.
- **More missile fields:** `SCItemMissileParams` has more than this dataset reads.
  - `targetingParams`: `trackingSignalMin`, `lockSignalAmplifier`, `minRatioForLock`, `lockIncreaseRate`, `lockResolutionRadius`, `signalResilienceMin/Max` and `maxTimesCanMiss`. `misl_s05_ir_taln_valkyrie` has 22, 2.5, 1, 0.6, 700, 1/1.7 and −1.
  - Root: `armTime`, `igniteTime` and `explosionSafetyDistance`.
  - `GCSParams`: `terminalPhaseEngagementTime`, `terminalPhaseEngagementAngle`, and the `boostPhase`, `interceptPhase` and `terminalPhase` blocks (`angularSpeed` 75/150/175).
  - `explosionParams.minRadius`: 20 on the Valkyrie.
  - `AttachDef.SubType`, which MCsim reads raw today (`persim/scripts/missile_fit.py:59`). Over the 72 files in the `missiles/` folder it is Torpedo 23, Missile 22, GroundVehicleMissile 21, Utility 3, UNDEFINED 2 and Rocket 1.
- **Weapon class label:** `SCItemPurchasableParams.displayType` gives the class as a localization key.
  - Ship: `@item_displayType_Cannon` 98, `Repeater` 51, `MiningLaser` 24, `Beam` 23, `Gatling` 21, `ScatterGun` 15.
  - Ship also has `Rocket` 9.
  - FPS (the 492 ranged records): `Rifle` 144, `Pistol` 72, `SMG` 59, `Shotgun` 45, `Sniper` 43, `LMG` 13, `Shouldered` 11, `Special` 8, `@item_TypeGadget` 37, `@LOC_PLACEHOLDER` 26, and no `SCItemPurchasableParams` on 34.
  - This gives a weapon class without parsing names. It is a key, so it resolves through the string table and adds no prose.
- **Restock and repool:** `SAmmoContainerComponentParams.maxRestockCount` is 3 on all 217 ship weapon records that have an ammo container (198 `WeaponGun`, 12 `TractorBeam`, 6 `SalvageHead`, 1 `TowingBeam`). The 24 mining lasers have no container. `SCItemWeaponComponentParams.ammoRepoolParams` holds `{bulletsPerSecond: 30, unstowMagDuration: 3, fullMagMergeDuration: 0.01}` on `klwe_laserrepeater_s3`.
- **Ammo `inheritVelocity`:** on the root of the `AmmoParams` record. MCsim reads it raw (`build_data.py:207`). It belongs on `ammo`.
- **FPS attachment ports:** `SItemPortContainerComponentParams.Ports[]` holds `Name`, `MinSize` and `MaxSize`. For `behr_rifle_ballistic_01` they are `magazine_attach` 1..1, `optics_attach` 1..2, `barrel_attach` 2..2 and `underbarrel_attach` 1..2.
- **FPS fire-action fields:** `spinUpTime`, `spinDownTime`, `wearPerShot`, `recoil` and `localisedName` are present on FPS fire actions.
- **AI ranges:** `weaponAIData.idealCombatRange` and `maxFiringRange` are 30 and 50 on the P4-AR. The meaning for players is not established.

**Values computable by joining with other datasets**

- **Reverse links:**
  - Which ships mount each weapon, from `game-ships` loadouts by `recordName`.
  - Which weapons fire each ammo: 226 of the 249 ammo records are used by some weapon, so 23 are not.
  - Consumers: starorg item detail, MCsim.
- **Charged-mode alpha:** `alpha × chargedDamageMultiplier` is not computed today, although both inputs are on the mode. It is an estimate, because the records do not state that the multiplier is applied to the whole shot.
- **Falloff-adjusted DPS at distance:** `ammo.falloff` (`minDistance`, `perMeter`, `floor`) combined with `dps`. This is an estimate, because the falloff formula is inferred from the field names.
- **Time to kill** against `game-ship-components` shields, armor and hull. This is an estimate.

**Parse gaps (with evidence)**

- **Impact damage replaced by explosion damage:**
  - The ammo damage comes from the first non-zero `DamageInfo` found under `projectileParams` (`find_damage`, `ammo.rs:162`). In DataCore key order, `detonationParams` comes before `damage`, so for the 26 ammo records with a non-zero explosion the explosion damage is reported. On 25 of them a non-zero impact damage is dropped (0.0001 or less on 11 of those); the 26th has no impact damage. This feeds `alpha`, `dps` and `damageType` here.
  - For example, `ksar_pistol_energy_01_ammo_plasma` has impact 41 but `damageTotal` 1, and `HRST_Nova_BallisticCannon_S5_AMMO` has impact 18500 but `damageTotal` 13500.
  - The full list is in [game-ammo](game-ammo.md).
- **FPS magazine referenced only by GUID path:** 7 FPS weapons name their magazine only by `entityClassReference`, with `entityClassName` empty. `default_magazine_name` reads only `entityClassName`, so these rows have no ammo, DPS or capacity. They are `apar_special_ballistic_02` (Animus launcher), `apar_special_ballistic_02_mat01`, and the 5 Cambio SRT records (`grin_salvage_repair_01`, `_01_store01`, `_01_store01_prop`, `_01_lumi01` and `grin_salvage_repair_IASI`). Their `entityClassReference` points at `magazines/apar_special_ballistic_02_mag.json` and `magazines/grin_tool_01_mag.json`.
- **Missile descriptionKey with a `,P` suffix:** 1 missile (`NOVP_Rocket_Venom_S1_Strike_TL_IR`) has `descriptionKey` `item_DescNOVP_Rocket_Venom_S1_Strike_TL_IR,P` and `size` 0. Across `shipWeapons` and `fpsWeapons`, 15 rows carry a `,P` description key. What the suffix means is not established. The string table resolves such keys (see [dump-reuse](../plans/specs/dump-reuse.md)).
- **Mining laser figures:** `dps` is the beam's `damagePerSecond`, which is energy 0.8 on S0 lasers. The gathering beam is mode kind `other`, with no figures.
- **Weapon `health`:** absent on every FPS row, although `durability` exists on 428 of them.
