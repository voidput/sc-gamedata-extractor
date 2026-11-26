---
title: game-ammo (game-ammo.json)
tags: [docs, datasets]
updated: 2026-11-25
---

# game-ammo (game-ammo.json)

Every `AmmoParams` record: the projectile a weapon fires, which is where weapon damage lives. A weapon record has a fire rate and heat but no damage figure. Each row gives speed, lifetime, range, the damage split by type, penetration, falloff and pierce data. Join it to [game-weapons](game-weapons.md) for DPS; that file embeds the same parse per weapon. LIVE 4.10.201.48085 has 249 ammo records (235 with damage, maximum damage 200000). Published size: 91,454 bytes minified (`jq -c . | wc -c`). The `_schema` key (since tags 4.10.201.48085 / 5.0.202.35026, commit `b9e3700d0`) is not in that figure; with it the same command prints 91,478.

## Source

- **Records:** filter `**/ammoparams/**` (`crates/sc-gamedata/src/domains/ammo.rs:273`). Every record whose type is `AmmoParams` is read. The local extract holds 70 files under `ammoparams/fps/` and 179 under `ammoparams/vehicle/`.
- **Parse:** `parse_ammo` (`ammo.rs:220`):
  - **damage:** `find_damage` (`ammo.rs:162`). This is the first non-zero `DamageInfo` found anywhere under `projectileParams`, and if there is none, anywhere in the record. "First" means DataCore key order (`serde_json` `preserve_order`, `crates/sc-gamedata/Cargo.toml:24`).
  - **penetration:** `projectileParams.penetrationParams`.
  - **falloff:** `projectileParams.damageDropParams` (`parse_falloff`, `ammo.rs:178`).
  - **pierce:** `projectileParams.pierceabilityParams`.
- **Inputs from other datasets:** none. `game-weapons` imports `parse_ammo` and `DamageInfo` from this module.

## Shape

Top-level keys: `_source`, `_extracted`, `_build`, `ammo[]` and `summary`. `_schema: "game-ammo@1"` after `_build` (since tags 4.10.201.48085 / 5.0.202.35026).

`summary`: `{ammo: 249, withDamage: 235, maxDamage: 200000}`.
- `withDamage` counts rows with `damageTotal > 0`.
- `maxDamage` is the largest `damageTotal`. On LIVE that is `BEHR_MassDriver_S12_AMMO`.

Order: `name` by locale comparison.

### ammo[] (struct `Ammo`, `ammo.rs:130`)

| Field | Type | Meaning | Units/range | Source |
|---|---|---|---|---|
| `name` | string | Record short name. Not localized | | record |
| `recordName` | string | `AmmoParams.<short>` | | record |
| `id` | string | Record GUID | | record |
| `size` | number | Root `size` | 0..12. 172 rows are size 1 | record |
| `speed` | number | Root `speed` | 0..2000 per second. Length unit not stated in code | record |
| `lifetime` | number | Root `lifetime` | 0.8..16 s | record |
| `range` | number | `round(speed × lifetime)`, set only when both are non-zero | 144..12003. Absent on 3 rows | derived |
| `damage` | object | Non-zero damage types, in the order `physical`, `energy`, `distortion`, `thermal`, `biochemical`, `stun` | rows that carry each type: energy 119, physical 107, distortion 15, stun 5, thermal 1 | `find_damage` |
| `damageTotal` | number? | Sum of `damage`, to 2 decimals | 0.05..200000. Absent with `damage` on 14 rows | derived |
| `penetration` | number? | `penetrationParams.basePenetrationDistance` | 0..66.69. Absent on 6 rows | projectileParams |
| `penetrationRadius` | object? | `{near, far}` from `nearRadius` and `farRadius` | 243 rows | projectileParams |
| `falloff` | object | Per damage type that falls off: `{minDistance, perMeter, floor}`, from `damageDropMinDistance`, `damageDropPerMeter` and `damageDropMinDamage`. Kept only when `perMeter > 0` | 36 rows | projectileParams |
| `pierce` | object | `{maxThickness, level1, level2, level3}` from `maxPenetrationThickness` and `damageFalloffLevel1..3`. Kept when any value is non-zero | 238 rows | projectileParams |

The 14 rows without damage are flares and chaff (`BEHR_Flare`, `JOKR_*`, `NOVA_Chaff`, `TALN_Chaff`, `VehicleCounterMeasureFlares`), four beam ammo records (`HRST_LaserBeam_S10*`, `Vanduul_LaserBeam_S12_AMMO`, `VNCL_Gen2_LaserBeam_S9_AMMO`), `BEHR_LaserCannon_S7_AMMO_Idris_M_Dummy`, `toy_pistol`, `VehicleBullets` and `VehicleExplosiveBullets`. Beam damage is on the weapon's beam fire action instead, which `game-weapons` reads.

### Example (LIVE)

```json
{"name":"behr_rifle_ballistic_01_ammo_5mm","recordName":"AmmoParams.behr_rifle_ballistic_01_ammo_5mm",
 "id":"453257c9-19cc-454e-8cb2-3cb01a7f30e0","size":1,"speed":550,"lifetime":2,"range":1100,
 "damage":{"physical":12},"damageTotal":12,"penetration":0,"penetrationRadius":{"near":0.5,"far":5},
 "falloff":{"physical":{"minDistance":40,"perMeter":0.05,"floor":10}},
 "pierce":{"maxThickness":5000,"level1":0,"level2":0,"level3":0}}
```

## Derived, estimated and curated values

- `range`, `damageTotal` and `summary` are computed by this crate. `range` is a straight-line upper bound: it ignores `falloff` and gravity, and the records do not say whether gravity applies.
- Which `DamageInfo` counts as "the" damage is a heuristic of this crate (the first non-zero one). See Parse gaps.
- There is no `Estimate` field and no `_assumptions`.

## Redaction

Nothing. The file is not in `PROSE_FIELDS`, has no `_redacted` key, and has no text fields.

## Consumers

- **SCMCP** `src/versions.ts:35`: `ammo: { file: "game-ammo.json", collection: "ammo" }`. Rows are keyed by `recordName`, the first of `className`, `ClassName`, `recordName`, `reference`, `id` and `name` that a row has (`keyOf`, `src/versions.ts:340-343`).
- **starorg-codex** (`feat/dataset-reuse`):
  - `crates/starorg-mcp/src/prebuilt.rs:154` needs `ammo` and `summary`, schema 1, verified on `4.10.200.58746`.
  - `crates/starorg-core/src/model/items.rs:3` names this file as a source of its field names.
- **MCsim:** does not read this file. `build_data.py:205-212` opens the raw ammo record instead, for `inheritVelocity`.
- **snares, Bindscape, sc-telemetry:** no reference.
- **Contracts:**
  - `_build` equals the tag, and the `_schema` stem is `game-ammo` ([consumer-datasets](../plans/specs/consumer-datasets.md)).
  - Keep `ammo` as the collection name and `recordName` as the row key.

## Derivable data

These come from records the game already ships. Nothing here adds to CIG's data.

**Fields the source records carry but the dataset does not read**

- **`inheritVelocity`:** on the root. It is 1.0 on 248 records and 0.0 on 1. MCsim reads it raw today (`persim/scripts/build_data.py:207`). Adding it removes that consumer's raw-record dependency.
- **Explosion block:** `projectileParams.detonationParams` holds `armTime`, `safeDistance`, `explodeOnImpact`, `explodeOnExpire`, and `explosionParams.{minRadius, maxRadius, damage}`. For example, `behr_glauncher_ballistic_ammo_01_40mm` has `minRadius` 0, `maxRadius` 5, `explodeOnImpact` true and explosion `DamagePhysical` 75. This should be published as its own `explosion` object, next to the impact `damage`.
- **Countermeasure signatures:** for flares and chaff, `projectileParams.typeParams` (`CounterMeasureFlareParams` / `CounterMeasureChaffParams`) holds `Start` and `End` values for `Infrared`, `Electromagnetic` and `CrossSection`. `ammoparams.behr_flare.json` has `StartInfrared` / `EndInfrared` 20000 / 20000, `StartElectromagnetic` / `EndElectromagnetic` 20000 / 20000 and `StartCrossSection` / `EndCrossSection` 6500 / 6500 (plus `StartDecibel` / `EndDecibel` 0 / 0). Three flare and three chaff records carry the block. These rows are in this dataset today, but with no figures. See the planned `game-ordnance` in [consumer-datasets](../plans/specs/consumer-datasets.md) section 3.
- **Other projectile fields:**
  - `projectileParams.impactRadius`: 7.0 on one record and 0 elsewhere.
  - `electronParams`: `residualChargeMultiplier`, `maximumJumps` and `jumpRange`, on 6 records (the five `*_ammo_electron` records and `ksar_pistol_energy_01_ammo_plasma`). It is null on the rest.
  - `ignitionChanceOverride`: −1 on 239.
  - Root `ammoCategory`: `None` on 246; `Laser`, `Plasma` and `_5mm` on one each.
  - Root `bulletType`: −1 on 175 and 2 on 74.
  - What these mean in play is not established.

**Values computable by joining with other datasets**

- **Reverse link:** the weapons that fire each ammo, from `game-weapons` `shipWeapons[]` and `fpsWeapons[]` `ammo.recordName`. 226 of 249 ammo records are used. The 23 that no weapon fires could be flagged as unused.
- **Damage after armour, per type:** `damage × game-armor resistances[type].multiplier`. The armor module states that a multiplier is the share of damage that gets through. This is an estimate, because thresholds, caps and body-part coverage are not applied.
- **Shots to break** a ship part, against `game-fire` / `game-ship-components` health. This is an estimate, because penetration and pierce rules are not modelled.

**Parse gaps (with evidence)**

- **Explosion damage published as the damage on 26 records; a non-zero impact is lost on 25 of them.**
  - In DataCore order, `projectileParams` lists `detonationParams` before `damage` (on all 243 records that have both keys). `find_damage` returns the first non-zero `DamageInfo`, so wherever the explosion damage is non-zero, it is published as `damage`, and the impact damage is lost. On all 26 the published `damageTotal` equals the explosion total.
  - Impact → published (= explosion) totals:

    | Ammo record | Impact | Published |
    |---|---|---|
    | `ksar_pistol_energy_01_ammo_plasma` | 41 | 1 |
    | `HRST_Nova_BallisticCannon_S5_AMMO` | 18500 | 13500 |
    | `KLWE_LaserRepeater_S1_IKTI_NTLS_AMMO` | 28 | 12 |
    | `vlk_spewgun_ballistic_01_ammo_special` | 9500 | 35 |
    | `vlk_spewgun_ballistic_01_ammo_special_vehicle` | 800 | 35 |
    | `apar_special_ballistic_01_ammo_20mm` | 20 | 60 |
    | `None_special_ballistic_01_AMMO_FPS` | 20 | 41000 |
    | `behr_glauncher_ballistic_ammo_01_40mm` | 5 | 75 |

  - The rest are:
    - the 11 distortion repeaters, cannons and scatterguns (`ASAD_*`, `JOKR_*`, `PRAR_*`, `BEHR_Distortion*`), with impact 0.0001 or 0.00009 and the explosion published;
    - six other `vlk_spewgun_*` variants (impact 5..25, published 9..35);
    - `apar_special_ballistic_02_ammo_rockets`, which has no impact damage at all (explosion 150), so nothing is lost there.
  - The fix is to read `projectileParams.damage` as impact and `detonationParams.explosionParams.damage` as explosion. This changes `damageTotal`, and `game-weapons` `alpha` and `dps`.
- **Values computed from the explosion:** the Boomtube's 41000 alpha (see [game-weapons](game-weapons.md)) comes from this. The 200000 `maxDamage` does not: `BEHR_MassDriver_S12_AMMO` has no explosion damage.
- **Rows without a range:** `VehicleBullets`, `VehicleCounterMeasureFlares` and `VehicleExplosiveBullets` have no `range`, because their `speed` is 0 (`lifetime` 1). What these generic records are used for is not established.
