---
title: Cross-dataset derivations
tags: [docs, plans, specs]
updated: 2026-11-25
status: draft
---

# Cross-dataset derivations

New fields and datasets computed by joining datasets we already publish, plus the record fields
those joins need. The game data is CIG's: nothing here adds data the game doesn't ship.
Everything is either read from shipped records or computed from published datasets. Source:
the "Derivable data" sections of the [dataset notes](../../datasets/README.md), written and
fact-checked 2026-11-25 against LIVE 4.10.201.48085 (sc-gamedata `main` 8e19cca9b).

## Rules

- **Rust only** (`crates/sc-gamedata`), the same as every dataset since the switch-over
  ([rust-pipeline](rust-pipeline.md)).
- **Inputs are published datasets or shipped records.** A join reads the parsed datasets of the
  same extract: one build, one `_build`. A new record field is read from the record, never
  inferred.
- **Estimates say so:** an `Estimate` suffix and an `_assumptions` entry for anything the records
  don't state (for example even picks, the SCU→volume mapping, acceleration models).
- **No prose, no assets, no geometry.** Names and loc keys only. No logos, thumbnails or paths
  to art. No meshes or models ([tool-data-sync](tool-data-sync.md) Rules).
- **`_schema`:** adding an optional field doesn't bump the version. Renaming or retyping an
  existing field bumps the version, and the consumers in
  [consumer-datasets](consumer-datasets.md) "Contracts to keep" get told first.
- **Size:** measure with `make publish-dry` and list the growth per item. A derived field that
  only restates a join a consumer can do cheaply should justify its bytes.
- **Each item names its consumers.** Any item with no consumer waits.

## Items

### D1. Reverse links ("where it is used" / "where it comes from")

| New field | On | From | Consumers |
|---|---|---|---|
| `stockOn[]` (vehicle `recordName`s whose default loadout carries the item) | `game-ship-components` items | `game-ships` `loadout` | snares (QED and drive pickers), codex detail |
| `mountedOn[]` (ships), `firedBy[]` (weapons) | `game-weapons` guns and missiles, `game-ammo` | `game-ships` `loadout.weapons`, `game-weapons` ammo refs | codex, MCsim |
| `wornBy[]` (actor, best `chance`, as an estimate) | `game-item-obtain` items, as a new source next to `loot` | `game-npc-loadouts` `actors[].items[]` | codex "where to get", SCMCP |

- Evidence: 23 of 249 ammo are fired by no weapon. 90 of the 2,447 NPC gear classes have no
  `game-item-obtain` row at all.
- Accept when every reverse link round-trips (each `stockOn` ship lists that item), and the
  90 gear-only classes gain an obtain row.

### D2. Missions joined to reputation, starmap and law

Read these fields from `missionbroker` records, all present today but unread
([game-missions](../../datasets/game-missions.md)):

| Field | Present on |
|---|---|
| `reputationRequirements` (faction, scope, comparison, standing) | 1571 missions |
| `missionResultReputationRewards[]` | 1801 missions |
| `reputationPrerequisites.wantedLevel` | 2178 + 393 missions |
| `locationMissionAvailable` / `localityAvailable` | 1836 / 407 missions |
| `missionGiverRecord` | 1459 missions |
| `requiredMissions[]` / `associatedMissions[]` | 425 non-empty |
| `missionModule` | all |
| `partialRewardPayout` thresholds | 849 missions |

Then join them:
- `requires: [{faction, scope, standing → game-reputation standing}]`: the rank needed.
- `repReward: [{faction, scope, amount}]`. **Open:** the record doesn't say which of the five
  outcome lists is which. Keep the list index until a record states it.
- `offeredAt[]`: places from `game-starmap` by record path, plus their jurisdiction and system
  through `game-law`.
- `chain{requires[], associated[]}`.
- Wikelo: rep per trade from `ContractResult_LegacyReputation`
  ([game-wikelo-trades](../../datasets/game-wikelo-trades.md)).

Overlaps [dump-enhancements](dump-enhancements.md) item 9 (mission item links) and the giver
resolution there. Do both in one pass.

Consumers: codex (missions tab, MCP), SCMCP.

### D3. A tag → items index, published once

- New dataset `game-tag-index`. For each tag path: the item classes carrying it, and the
  `game-loot` archetypes and secondary choices that select it.
- Today the obtain query rebuilds this on every call: SCMCP `obtain.ts`, the codex app, and
  `item_obtain.rs`.
- Same pass, fix two obtain parse gaps:
  - **V3 `entityClasses` selectors** are read as "anything" (`query/obtain.rs:176/196`): 64 loot
    objects name 26 classes. This is a correctness fix for `game-item-obtain`.
  - **Classes with no `game-item-tags` row:** 214 blueprint products and 55 NPC gear classes.
    Give them minimal identity rows, so that D1 `wornBy` and blueprint sources have somewhere
    to live. The codex doesn't need these (it said so 2026-11-25), so this is for SCMCP and
    completeness. **Open:** do we want it?
- V3 `optionalData` (`choiceLimit`, `chanceToExist`, `stackSize`, …), spawn-with rules (111
  `tagsToMatch` groups) and `specialEvents` probabilities could refine the odds. Each rests on
  an assumption about how the game applies it, so they come later, as estimates.

### D4. The resource → ore → refinery → blueprint chain

Per `game-commodities` resource:

| Field | Derived from |
|---|---|
| `minedAt[]` | `game-mining-spawns` belts and spots, via `minedAs` |
| `refinedBy[]` | `game-refining` processes, plus the raw quality a refinery receives, from `quality` |
| `usedByBlueprints` (count and ids) | `game-blueprints` `resourceUse`: 26 resources, e.g. Aslarite 872 |
| `inManifests[]` | `game-cargo` manifests: all 29 names resolve |
| `qualityOverrideBelts[]` | `quality.locationOverrides` matched to belts by system |

Also emit `descriptionKey` / `rawDescriptionKey` / `refinedDescriptionKey` and a per-process
`nameKey` in `game-refining`. Today the published file has no keys for its redacted prose, so
the codex's local `global.ini` fill can't work for it.
[dump-reuse](dump-reuse.md) decision 2 depends on this, so it is the most urgent part of D4.

Consumers: codex commodity detail and mining tab, SCMCP.

### D5. Per-ship totals from loadout × components

- **Parse fixes first:**
  - `parse_loadout` drops `SalvageHead` (not in `GUN_TYPES`, ships.rs:364), `SalvageModifier`,
    `SalvageInternalStorage`, `Battery` and bombs (only `Missile` is listed, ships.rs:556).
    Today `GAMA_Hyun`, with two salvage arms, gets `capabilities.salvage: false`, and
    `CRUS_Spirit_A1`'s bomb is missing.
  - `DynamicPowerPool.maxItemCount` entries are skipped, so `powerPools` is only `WeaponGun`.
- **Then per vehicle:**
  - `totals.coolantDraw`, `powerDraw` by state, and `signature{em, ir}` from the stock
    components;
  - `salvage{heads, modifiers, storageScu}` (Reclaimer 448 SCU);
  - `fireParts[]`, the item ids from `game-fire`.
  - **Estimate:** summing draws assumes every stock part is on, so the `_assumptions` entry
    says so.
- Give `game-salvage` the `vehicles[]` list sc-telemetry's `salvage_classes` already reads.
  `capabilities.salvage` then uses it too, with no key change for sc-telemetry.
- Consumers: sc-telemetry (salvage gating), MCsim (`build_fire.py` builds `fireParts` itself
  today), codex.

### D6. Trip, quantum and fit estimates (all `Estimate`)

- `minimumQuantumStartEstimate` per body: from `game-quantum` `minimumAltitudeForQuantum` and
  `game-starmap` body `size`. "`size` is a radius" is snares' reading, not stated by the record.
- Route time and fuel: starmap positions plus drive `splineJumpParams` (read the full set;
  only `driveSpeed` is read today). The acceleration model is not stated, so this stays an
  estimate, or waits until the records state it.
- "What fits": a container's `maxPermittedItemSize` / capacity against an item's
  `inventoryOccupancyVolume`. The packing rule is not stated.
- Consumers: snares for the first two. The third has no consumer yet, so it waits under the
  rules above.

### D7. Manufacturer resolution everywhere

- Read the `Code` field of `scitemmanufacturer` records. It differs from the short name on 38
  of them (`AEGS` → `AEG`) and is empty on 15.
- Emit `manufacturerCode` + `manufacturerName` on `game-ships`, `game-item-tags` and
  `game-search-index`.
- Normalise the 62 tag-derived search-index makers (`MICROTECH`, `Shubin`) to codes, or add a
  `makerNames` list.
- **Open:** which code is canonical per maker. Use `Code` where it decides; otherwise mark it as
  an estimate.
- This lets sc-telemetry drop its own `MANUFACTURERS` table.

### D8. More search-index entry kinds

- Add commodities and places to `game-search-index`: 958 visible starmap locations, 871 with a
  real `displayName`.
- Add a `nameKey` column, so clients can show names from the player's own `global.ini`.
- Same pass, two fixes: `_source` still names the deleted `src/search-index.ts`, and the file has
  no `_extracted`.
- Consumers: SCMCP, codex search.

## Order

1. **D4 keys** (small; unblocks the codex prose fill for refining).
2. **D5 parse fixes**, which also fix `capabilities.salvage`.
3. **D1.**
4. **D2**, together with dump-enhancements item 9.
5. **D3**, starting with the V3 selector fix.
6. **D7.**
7. **D8.**
8. **D6**, last: it is all estimates, and only snares consumes it.

## Decisions (2026-11-25)

1. **D2 reputation rewards:** keep the index. `repRewards` is published as a 5-slot list in the
   record's order, and an `_assumptions` entry says the outcome per slot is not stated. No
   labels are inferred. They get added if a later build or a record states them.
2. **D3 missing items: add minimal rows.** The ~269 classes missing from `game-item-tags` (214
   blueprint products, 55 NPC gear classes) get identity-only rows: class, name / `nameKey`,
   type, size and grade from `AttachDef`. Each carries `source: "blueprint" | "npcGear"`, so
   `game-item-obtain` can list their blueprint and NPC-gear sources.
3. **D5 totals: per power state.** `totals.byState: {Online: {powerDraw, coolantDraw, em, ir},
   Travelling: {...}, ...}` for each state the component records give. It is marked as an
   estimate (every stock part is assumed to be in that state).
4. **D7 manufacturer code: keep the short name, add `Code`.** `manufacturerCode` stays the short
   name every consumer keys on today (`AEGS`). The record's own `Code` is published beside it as
   `recordCode` (`AEG`, or `null` when empty). No ids change and there's no `_schema` bump.
