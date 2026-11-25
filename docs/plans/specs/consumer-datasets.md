---
title: Dataset changes requested by the tool repos
tags: [docs, plans, specs]
updated: 2026-11-25
status: draft
---

# Dataset changes requested by the tool repos

Changes to the published datasets that the tool repos asked for while moving onto the dumps
hub ([tool-data-sync](tool-data-sync.md)), collected 2026-11-25. They land after the Rust
switch-over ([rust-pipeline](rust-pipeline.md) step 4, branch `feat/rust-switchover`). If the TS
parse is gone by then, they are Rust-only; if not, the TS + Rust rule applies. The
starorg-codex app's asks are in [dump-reuse](dump-reuse.md).

Rules, as in [dump-enhancements](dump-enhancements.md):
- Values come from extracted records.
- Fields that rest on an assumption are marked as estimates.
- No prose.
- Measure the size growth with `publish-dry`.
- Once `_schema` exists, bump it only on a breaking change.

## Contracts to keep (no change, never break)

- **snares:** `game-starmap.json` keeps `_build`, because `build-data.mjs` reads `starmap._build`.
- **Bindscape:** `game-keybinds` and `game-attributes` keep `label`, `labelKey`, `optionTrees`
  (`showInvert`, `showCurve`, `invert`, `exponent`, `parent`), `optionGroup`, `platform`,
  `confirmation` and `version` unredacted. Never add them to `PROSE_FIELDS` or to the Rust
  redaction list.
- **MCsim:** `game-ships._build` must equal `game-fire._build` (`build_fire.py` fails
  otherwise). Publish always writes one extract, so this holds.
- **sc-telemetry** (branch `feat/gamedata-hub`, `b2125ad`). Its optional hub fetch
  (`--gamedata-fetch`) checks the following, so publish has to keep them true:
  - `build.json.build` equals the tag.
  - Each `files[]` entry (`name` or `data/<name>`, `sha256`, `bytes`) matches the bytes
    actually published.
  - Every dataset has `_build` equal to the tag.
  - The text before `@` in `_schema` equals the dataset's file stem (`game-ships@1` in
    `game-ships.json`).
  - Tag names match `^\d+(\.\d+){2,3}$`.
  - `game-keybinds` keeps `defaults` per device kind, and keeps `label` even when descriptions
    are redacted. `game-keybinds` is published in every build's tag; that needs
    `feat/keybinds-domain` merged.
  - `game-ships`, `game-salvage` and `game-keybinds` stay at `data/<name>.json`.

## 1. `game-fire`: fuel tanks (MCsim)

`items[]` has no `FuelTank` / `QuantumFuelTank` entries, so 10 parts are lost:
`fuelpod_misc_standard`, `fuelpod_stor_secure`, `misc_fuelrefinery_l` and `misc_fuelrefinery_r`
(on the Starfarer, Starfarer Gemini, Starfarer Teach's and Starlite). Add them in the same shape
as the other items: `health`, `damageMultipliers`, `ignite`, `deathExplosion`, and so on.

**Built (Rust, branch `feat/rust-fire-caps`).** Items whose `AttachDef.Type` is `FuelTank`,
`QuantumFuelTank` or `ExternalFuelTank` are listed whatever their fire path, when the record
has a health pool or a death explosion. The fuel pods (`fuelpod_misc_standard`,
`fuelpod_stor_secure`) are `ExternalFuelTank`, so that type is included too. LIVE
4.10.201.48085: 33 tanks added (31 `ExternalFuelTank`, 2 `FuelTank`), items 490 → 523. All four
named parts are present, e.g. `misc_fuelrefinery_l`: health 10000, `deathExplosion` physical
500, radius 2.5–25. The other 340 tanks (183 `FuelTank`, all 157 `QuantumFuelTank`: each ship's
internal `HTNK_*` / `QTNK_*`) have no `SHealthComponentParams`, so they are left out: a row for
them would hold a name and nothing else, and `build_fire.py` skips parts without health.

## 2. `game-fire`: hit detonation for explosives (MCsim)

For Missile, Bomb and SpaceMine items, add
`hitExplosion: { damage: {…}, minRadius, maxRadius }` from the item's missile component
`explosionParams`.

Bombs are not in `game-weapons.missiles`, so today `bomb_s03_fski_thunderball`, `bomb_s05_fski`
and `bomb_s10_fski_colossus` bake with a hit total of 0. The raw values were 27000, 46702 and
568297. With `hitExplosion`, MCsim no longer needs `game-weapons` for explosives.

**Built (Rust, branch `feat/rust-fire-caps`).** `hitExplosion` is read from
`SCItemMissileParams`, `SCItemBombParams` or `SCItemSpaceMineParams` (by item type),
keeping only the damage types that are not zero, and the item is listed when it has one.
`summary.withHitExplosion` counts them. LIVE 4.10.201.48085: 72 items (67 missiles, 3 bombs,
2 mines). Hit totals: thunderball 27000, s05 46702, colossus 568296.99. All 67 missile
totals equal `game-weapons.missiles[].damageTotal`.
Size with item 1 (extract, not `publish-dry`): `game-fire.json` 597885 → 634404 bytes
(+6.1%), 33212 → 35363 gzip -9.

## 3. `game-ordnance` (MCsim, new dataset)

MCsim's ordnance bake (`build_ordnance.py`, `missile_controllers.py`) stays local until this
lands. These are the raw fields it reads today:

- **Missiles**
  - Source: `entities/scitem/ships/weapons/missiles/misl_*.json`. Skip templates and any record
    with no `targetingParams`.
  - `SAttachableComponentParams.AttachDef`: `Size`, `SubType` (Missile | Torpedo), `Type`.
  - `SCItemMissileParams.targetingParams`:
    - tracking and lock signal: `trackingSignalType`, `trackingSignalMin`,
      `lockSignalAmplifier`, `minRatioForLock`;
    - lock geometry: `lockTime`, `lockingAngle`, `lockRangeMin`, `lockRangeMax`,
      `lockResolutionRadius`, `lockResolutionFalloffPower`;
    - resilience and miss handling: `signalResilienceMin`, `signalResilienceMax`,
      `maxTimesCanMiss`, `lockIncreaseRate`, `lockDecreaseRate`.
  - `SCItemMissileParams`: `explosionSafetyDistance`, `igniteTime`, `maxLifetime`, `armTime`.
  - `GCSParams`: `linearSpeed`, `interceptPhase.angularSpeed`, `terminalPhase.angularSpeed`,
    `terminalPhaseEngagementTime`, `terminalPhaseEngagementAngle`.
  - `explosionParams`: the non-zero `damage.Damage*` values and `maxRadius`.
- **Countermeasure ammo**
  - Source: `ammoparams/vehicle/*flare*` and `*chaff*` (not `vehiclecountermeasure`), where
    `projectileParams.typeParams._Type_` is `CounterMeasureFlareParams` or
    `CounterMeasureChaffParams`. `id` is the file stem.
  - Root: `speed`, `lifetime`, `inheritVelocity`.
  - `typeParams`: `StartInfrared`, `EndInfrared`, `StartElectromagnetic`, `EndElectromagnetic`,
    `StartCrossSection`, `EndCrossSection`.
  - Chaff only: `radiusRange.maximum`, `volumeSpawnDelay`, `volumeLifetime`.
  - `radarObjectParams.radarProperties.baseSignatureParams.signatures[0..2]` (IR, EM, CS).
- **Countermeasure launchers**
  - Source: `entities/scitem/ships/countermeasures/<item>.json`.
  - Take the first `SWeaponActionFire{Single,Burst}Params` with a `fireRate`, wherever it
    appears in the record: `_Type_`, `fireRate`, `cooldownTime`, `shotCount`,
    `launchParams.spreadParams.{min,max}`.
  - Per launcher, `ammo`, `count` and `capacity` already come from game-ships
    `loadout.countermeasures`.
- **Missile controllers**
  - Source: `entities/scitem/ships/controller/controller_missile*.json`.
  - `SCItemMissileControllerParams`: `maxArmedMissiles`, `launchCooldownTime`,
    `lockAngleAtMin`, `lockAngleAtMax`.
  - Also emit, per ship, the controller ids its entity record references. The torpedo/general
    split stays in MCsim.
- **Missile racks**
  - Source: `entities/scitem/ships/missile_racks/**/mrck_*.json`.
  - `SCItemMissileRackParams.launchDelay`.
  - `SItemPortContainerComponentParams.Ports[]`: `MinSize`, `MaxSize`, and `Types[]` where
    `Type == "Missile"` → `SubTypes` (default `["Missile"]`).
  - Also emit, per ship, **every** rack id its entity record references.
    `game-ships loadout.missiles[].rack` covers only the loadout racks, and fitting uses every
    rack.

Resolve the ship → controller and ship → rack links from record references, not by regex over
the JSON as MCsim does today.

## 4. `game-ships`: `capabilities` (sc-telemetry)

Add a `capabilities` object per vehicle, so sc-telemetry's `shipcaps.py` stops duplicating the
derivation. Its keys are those of `derive_caps`:

| Key | Rule |
|---|---|
| `mining` | any `WeaponMining`, or the role contains "Mining" |
| `salvage` | the role contains "Salvage", or `game-salvage` lists the class |
| `refuel` | the role contains "Refuel" |
| `tractor` | `TractorBeam` |
| `guns` | `WeaponGun` or turrets |
| `scan` | radars |
| `quantum` | `quantumDrives` |
| `qed` | `quantumEnforcement` |
| `emp` | `EMP` |
| `missiles` | `missileRacks` |
| `cargo` | `totals.cargoScu > 0` or cargo grids exist |
| `cargo_scu` | `totals.cargoScu` |

All are derived from the default loadout. The exact rules from sc-telemetry
(`sctelemetry/shipcaps.py:80`), received 2026-11-25:

- **Inputs:** `lo` = `vehicle.loadout`; `wtypes` = the set of `loadout.weapons[].type`; `role` =
  the lowercased `vehicle.role`; `cls` = `recordName` without `EntityClassDefinition.`.
- **`present(v)`:** a bool is itself; a number is true when > 0 (turrets and missileRacks are
  ints); a list or dict is true when non-empty; anything else is false.
- **Rules:**
  - `mining` is true when `WeaponMining` ∈ `wtypes` or the role contains "mining".
  - `salvage` is true when the role contains "salvage" or `cls` is in `game-salvage`'s
    vehicles/ships. Never match on weapon names: "Ardor-2 Salvaged Repeater" is a gun.
  - `refuel` is true when the role contains "refuel".
  - `tractor` is true when `TractorBeam` ∈ `wtypes`.
  - `guns` is true when `WeaponGun` ∈ `wtypes` or `present(lo.turrets)`.
  - `scan`, `quantum`, `qed` and `missiles` are `present()` of `lo.radars`,
    `lo.quantumDrives`, `lo.quantumEnforcement` and `lo.missileRacks`.
  - `emp` is true when `EMP` ∈ `wtypes`.
  - `cargo_scu` is `round(totals.cargoScu, 2)` when that is a number (not a bool) and > 0,
    otherwise `null`.
  - `cargo` is true when `cargo_scu` is not null or `present(lo.cargoGrids)`.
- **Upstream additions:** emit the booleans plus `cargo_scu`, and add `cargoKnown: false` when
  the dataset has no cargo data. Today `cargo: false` is ambiguous: it can mean unknown or none.
- **Variants:** sc-telemetry merges variants that share a class (OR each flag, max
  `cargo_scu`). Upstream emits one object per vehicle record and leaves the merge to the client.
- sc-telemetry prefers `vehicle.capabilities` when it is present and falls back to
  `derive_caps` otherwise.

**Built (Rust, branch `feat/rust-fire-caps`).** Every vehicle has `capabilities` with the
eleven booleans and `cargo_scu` (a number, or `null`). `cargoKnown: false` is emitted when
`cargo` is false, because then there is no cargo grid and no SCU, so "none" and "unknown"
cannot be told apart. `game-salvage` lists no vehicles, so `salvage` comes from the role
alone. LIVE 4.10.201.48085: across 1104 vehicles, sc-telemetry's `derive_caps` (run on the same
`game-ships.json`) gives 0 mismatches on every key. 668 vehicles have `cargoKnown: false`,
among them `RSI_Perseus` and `MISC_Starlite`, which have no cargo grid in their stock loadout.
Size: `game-ships.json` 6311719 → 6687862 bytes (+6%), 230141 → 235814 gzip -9.

## 5. Telemetry action names ↔ `game-keybinds` (sc-telemetry): no change needed

Checked 2026-11-25. All 112 `ListTriggerableActions` names (from the cached
`sc-telemetry/.tmp/triggerable-actions.json`) match action `name`s in `game-keybinds`
(feat-branch data, 4.10.193.11644, 1025 actions). The suspect names were checked too:
`_on`/`_off`/`_set_*` and `v_enter_remote_turret_N` are all real actions, so no alias field is
needed.

Ten names appear in more than one actionmap, so sc-telemetry keys on
`(actionmap, name)`, or keeps the list of actionmaps per name:
- in `spaceship_general` and `vehicle_general`: `v_toggle_all_doors`, `v_open_all_doors`,
  `v_close_all_doors`, `v_toggle_all_doorlocks`, `v_toggle_all_portlocks`,
  `v_unlock_all_ports`, `v_flightready`;
- in `player`, `spaceship_hud` and `vehicle_general`: `mobiglas`, `visor_wipe`, `v_starmap`.

Re-check on each new build once `game-keybinds` is published.

## Order

After the switch-over: 1 and 2 (small, and unblock MCsim's fire publish) → 4 → 3.
