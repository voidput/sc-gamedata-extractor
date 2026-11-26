---
title: game-quantum (game-quantum.json)
tags: [docs, datasets]
updated: 2026-11-25
---

# game-quantum (game-quantum.json)

The global quantum-travel rules every quantum drive obeys: where a jump may start (altitude, atmosphere), the party-link range, and a few spline constants. It holds one rules object read from the single `QuantumDriveGlobalParams` record; per-drive speed, spool and fuel live in [game-ship-components](game-ship-components.md) `quantumDrives[]`. On LIVE 4.10.201.48085 `summary.records` is 1 and the file is 443 bytes minified. The `_schema` key (since tags 4.10.201.48085 / 5.0.202.35026, commit `b9e3700d0`) is not in that figure; it adds 27 bytes (470). PTU 5.0.202.35026 has the same rules; only `_build` differs.

## Source

- DataCore record `libs/foundry/records/globalquantumdriveparams/quantumdriveglobalparams.json` (`_Type_` `QuantumDriveGlobalParams`), picked by the filter `**/globalquantumdriveparams/**` (`crates/sc-gamedata/src/domains/quantum.rs:81`).
- Code: `parse_quantum_rules` (quantum.rs:47), `Domain::parse` (quantum.rs:85). The module comment (quantum.rs:6) says notification, music and FX settings are left out on purpose, and field names follow the game's own where their meaning is not documented.
- Derives from no other dataset. No localization needed.

## Shape

Top-level keys: `_source`, `_extracted`, `_build`, `rules{}`, `summary{}`. `_schema: "game-quantum@1"` after `_build` (since tags 4.10.201.48085 / 5.0.202.35026), and an entry in `build.json` `files[]` with its hash.

`rules` is left out when no `QuantumDriveGlobalParams` record is found; `summary.records` is then 0 (test `no_record_omits_rules`, quantum.rs:143). Every field below is optional and dropped when the record has no number there.

### `rules`

| Field | Type | Meaning | Units/range (LIVE) | Source |
|---|---|---|---|---|
| `minimumAltitudeForQuantum` | number | Height above a surface before a drive will engage (doc comment, quantum.rs:22) | 2000; metres per the doc comment | `minimumAltitudeForQuantum` |
| `maximumAtmosphericPressureForQuantum` | number | Atmospheric pressure above which a drive will not engage (quantum.rs:25) | 0.4; unit not stated | `maximumAtmosphericPressureForQuantum` |
| `maxLinkingRange` | number | Range within which ships can link for a party jump (quantum.rs:28) | 10000; metres per the doc comment | `maxLinkingRange` |
| `maxRaycastDist` | number | Meaning not established | 9000 | `maxRaycastDist` |
| `maxIconScaleRange` | number | Meaning not established | 70,000,000 | `maxIconScaleRange` |
| `inputDelay` | number | Meaning not established | 0.5 | `inputDelay` |
| `scatterScale` | number | Meaning not established | 1 | `scatterScale` |
| `arrivalRadiusScalar` | number | Meaning not established (spline traversal param) | 1.1 | `splineTraversalParams.arrivalRadiusScalar` |
| `rollUnderFullRotationDistance` | number | Meaning not established | 250,000 | `splineTraversalParams.rollUnderFullRotationDistance` |
| `rollbackFullRotationDistance` | number | Meaning not established | 2,450,000 | `splineTraversalParams.rollbackParams.fullRotationDistance` |

### `summary`

| Field | Type | Meaning | Range |
|---|---|---|---|
| `records` | integer | 1 when the rules record was found, else 0 | 1 |

### Example (LIVE, whole file body)

```json
{
  "rules": {
    "minimumAltitudeForQuantum": 2000,
    "maximumAtmosphericPressureForQuantum": 0.4,
    "maxLinkingRange": 10000,
    "maxRaycastDist": 9000,
    "maxIconScaleRange": 70000000,
    "inputDelay": 0.5,
    "scatterScale": 1,
    "arrivalRadiusScalar": 1.1,
    "rollUnderFullRotationDistance": 250000,
    "rollbackFullRotationDistance": 2450000
  },
  "summary": { "records": 1 }
}
```

## Derived, estimated and curated values

None. Every value is copied as is from the record. No field carries an `Estimate` suffix or `_assumptions`.

## Redaction

Nothing. `game-quantum.json` is not in `PROSE_FIELDS` (`crates/sc-gamedata/src/localization.rs`, redaction module), and the file carries no prose.

## Consumers

- **snares** `scripts/build-data.mjs:35` reads the file and `:232` copies `quantum.rules` whole into `public/data/universe.json` (`universe-ptu.json` for PTU). The app types it as `Record<string, number>` (`src/lib/types.ts:113`) and no app code reads a named field from it today. Contract: keep `rules` an object of plain numbers.
- **starorg-codex** (branch `feat/dataset-reuse`): the dump reader lists it in `DUMP_USE` (`crates/starorg-mcp/src/prebuilt.rs:175`): schema version 1, top-level keys `rules` and `summary` required, no redaction accepted. Contract: keep both keys.
- No other checked consumer (MCsim, Bindscape, sc-telemetry, SCMCP) reads this file.
- [consumer-datasets](../plans/specs/consumer-datasets.md) lists no contract for `game-quantum` beyond the general ones: `_build` equal to the tag, and the `_schema` stem matching the file stem.

## Derivable data

The game data is CIG's: we don't add to it, we derive from it. More that the shipped records already hold:

### Fields the record carries that the dataset does not read

From `quantumdriveglobalparams.json` (`extracted-data`, file dated 2026-11-24, build 4.10.201.48085):

- **Spline shaping.** Outside its `splineFX` block, `splineTraversalParams` holds 26 numbers; the dataset takes 3, leaving 23: `nearAlignmentThreshold` 0.6, `nearTensionScalar` 1.0, `midTensionScalar` 1.0, `farTensionScalar` 1.2, `originNear/Mid/FarTensionScalar` 1.025 / 1.1 / 1.35, `targetNear/Mid/FarTensionScalar` 1.025 / 1.1 / 1.4, `baseTensionMin` 1.0, `baseTensionMax` 1.6, `tangentPlanetScalar` 1.1, `maxAlignmentToUseTangentDirection` 0.7, `angleDerivedTensionExponent` 1.0, `tensionModifiersExponent` 1.0, `near/mid/farEndingTargetOrientationRatio` 1.0 each, `nearArrivalOrientationThreshold` 0.05, `mid/farDerivedTargetPositionAlignment` 0.05 each, `rollbackParams.targetAlignmentToBeginRollBack` 0.9, plus the enums `rollUnderStartPhase` ("Aligning") and `rotationBehavior` ("NoRollback"). Their meaning is not established; a route-shape tool would need them all.
- **Interdiction nav point.** `interdictionNavPointClass` = `libs/foundry/records/entities/navpoint_interdictedentity.json`, the entity spawned where a ship is pulled out of quantum. Meaning beyond the name not established.
- **Calibration notifications.** `notifications.calibrationStarted/Completed/Failed` each carry `screenTimer` 5.0, `hurryScreenTimer` 3.0, `blocking` false, plus title/message string keys. UI-only, but the keys would let a tool show the game's own calibration wording.
- **Trip-length bands.** `musicParams.tripCategory[]` names four bands by `minDistance`: Minimum 0, Short 2,000,000,000, Medium 10,000,000,000, Long 30,000,000,000 (with `preArrivalDurationSecs` 5 / 10 / 21 / 50). These drive music only; using them as a trip-length scale would be an estimate.

### Values computable with other datasets

- **Minimum quantum start distance per body**: `minimumAltitudeForQuantum` + the body radius from [game-starmap](game-starmap.md) (`size` on planets and moons, the field snares reads as a radius). snares already places bodies from game-starmap and could show "climb to X m before spooling". The radius reading of `size` is snares' interpretation, so the result is an estimate until the code says what `size` measures.
- **Arrival distance per location**: `arrivalRadiusScalar` × game-starmap `quantum.arrivalRadius`. How the game applies the scalar is not stated in the record, so this is an estimate.
- **Trip time and fuel per route**: game-starmap positions give distances; game-ship-components `quantumDrives[].stats` gives `driveSpeed`, `stageOneAccelRate`, `stageTwoAccelRate`, `spoolUpTime` and `quantumFuelRequirement`. A trip-time figure needs an acceleration model the records do not state, so it is an estimate. snares (route planner) and SCMCP (route questions) would use it.
- **Spline jumps**: game-ship-components keeps only `splineDriveSpeed` of each drive's `splineJumpParams`. The record also carries spline `spoolUpTime` (4.0 vs 4.4 on the Beacon), `cooldownTime` 10, `stageOneAccelRate` 250, `stageTwoAccelRate` 50000 and `interdictionEffectTime` 5.0 (2.5 for the normal jump). These belong in game-ship-components, but pair with the spline rules here.

### Parse gaps

- None found in what the domain reads: all 10 fields are present on LIVE and PTU.
- Seven of the ten published fields have no stated meaning in code (only the first three have doc comments). Consumers get numbers whose meaning they must guess.

Related specs: [dump-enhancements](../plans/specs/dump-enhancements.md), [tool-data-sync](../plans/specs/tool-data-sync.md).
