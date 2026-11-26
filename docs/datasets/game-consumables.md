---
title: game-consumables (game-consumables.json)
tags: [docs, datasets]
updated: 2026-11-25
---

# game-consumables (game-consumables.json)

Medpens, pills, food and drink, and what taking one does. Each item's contents (subtype references with a ratio) are joined to the ConsumableTypeDatabase, where effects are quoted per micro-SCU. The totals are then worked out for the item's volume: health restored, stat changes (Hunger, Thirst, BloodDrugLevel and so on), buffs, gases and resources. LIVE 4.10.201.48085 has 231 consumables and 83 subtypes. Published size: 118,075 bytes minified (`jq -c . | wc -c`). The `_schema` key (since tags 4.10.201.48085 / 5.0.202.35026, commit `b9e3700d0`) is not in that figure; with it the same command prints 118,106.

## Source

- **Records** (filters, `crates/sc-gamedata/src/domains/consumables.rs:321`):
  - `**/entities/scitem/consumables/**`: medical items.
  - `**/entities/scitem/carryables/1h/**`: food and drink.
  - `**/consumabletypesdatabase/**`: the single `ConsumableTypeDatabase` record (`consumabletypes.json`).
- **Subtypes** (`parse_consumable_types`, `consumables.rs:99`): `types[].subtypes[]`, keyed by `_RecordName_`. Each gets `name` (from `typeName`), `type` (the parent type's `typeName`) and `effectsPerMicroScu` (each effect's `_Type_` minus `ConsumableEffect`, then its raw fields, without `effectDescription`).
- **Items** (`parse_consumable`, `consumables.rs:206`):
  - Any record with `SCItemConsumableParams` is read. Two kinds are dropped: templates (placeholder name), and records whose record name contains `template`. In the local extract 296 records under the two item filters carry the component, and 231 are published.
  - `consumableVolume.microSCU` gives the volume.
  - `defaultContents[]` is a list of `{consumableSubtype, ratio}`.
  - Each effect is scaled by `volume × ratio` (`ratio` defaults to 1).
  - Flags are `oneShotConsume` and `discardWhenConsumed`.
- **Inputs from other datasets:** none.

## Shape

Top-level keys: `_source`, `_extracted`, `_build`, `consumables[]`, `subtypes[]`, `byContentType` and `summary`. `_schema: "game-consumables@1"` after `_build` (since tags 4.10.201.48085 / 5.0.202.35026).

- `summary`: `{consumables: 231, subtypes: 83}`.
- `byContentType`: the `type` of each item's first content, with `Unknown` when there is none. LIVE: Food 114, Drink 54, Unknown 45, Medical 15, SQ42_Medical 3.
- Order: consumables by `name`. Subtypes by `type`, then `name` (`compare_names`).

### consumables[] (struct `Consumable`, `consumables.rs:151`)

| Field | Type | Meaning | Units/range | Source |
|---|---|---|---|---|
| `name` | string | Localized name. 19 names occur more than once (variants) | | AttachDef |
| `recordName`, `id` | string | Record name and GUID | | record |
| `itemType` | string? | `AttachDef.Type` | Food 124, Drink 79, FPS_Consumable 18, Misc 10 | AttachDef |
| `subType` | string? | `AttachDef.SubType`, with `UNDEFINED` dropped | Junk 47, Bottle 35, Tin 20, Can 19, Consumable 16, Medical 15, Sachet 14, Bar 13, Box 13, Harvestable 9, MedPack 2, Plant 2, Glass 1, OxygenCap 1. Absent on 24 | AttachDef |
| `volumeMicroScu` | number | `consumableVolume.microSCU` | micro-SCU. 1..900 | consumable params |
| `contents[]` | array | `{subtype, type, ratio?}`. `subtype` is the subtype's `typeName`, and `type` its parent type's `typeName`. Empty on 43 rows | | `defaultContents` |
| `health` | number? | Σ `healthChange × volume × ratio`, to 3 decimals, kept when non-zero | 3 rows: MedPen (Hemozal) 112.5, Treat Injuries 112.5, DeconPen Xtra 100 | derived |
| `stats` | object? | Per `statType`: Σ `statPointChange × volume × ratio` | Hunger 6..300 (133 rows), Thirst 6..300 (69), BloodDrugLevel 3..79.5 (23), Stun 1500 (1), BodyRadiation 0 (1) | derived |
| `buffs[]` | array? | `{buff, duration?, value?}` from `AddBuffEffect`: `buffType`, `buffDurationOverride.durationOverride` and `buffValueOverride.valueOverride` | 341 buffs: 173 with a duration, 5 with a value | subtype effects |
| `gases` | object? | Per gas (the file stem of `gasMass.Gas`): Σ `Mass × volume × ratio` | 1 row: OxyPen `{oxygen: 0.09}` | derived |
| `resources` | object? | Per `consumableResourceType`: Σ `amount × volume × ratio` | **0 rows on LIVE** | derived |
| `oneShot` | boolean | `oneShotConsume` | true on 149 | consumable params |
| `discardWhenConsumed` | boolean | `discardWhenConsumed` | true on 31 | consumable params |

### subtypes[] (struct `ConsumableSubtype`, `consumables.rs:29`)

| Field | Type | Meaning | Units/range | Source |
|---|---|---|---|---|
| `name` | string | Subtype `typeName` (for example `Coagulant`, `Water`) | | database |
| `type` | string | Parent type's `typeName` | Food 35, Drink 20, Medical 16, SQ42_Medical 10, Utility 1, SQ42_Utility 1 | database |
| `effectsPerMicroScu[]` | array | `{kind, ...raw fields}` | kinds: AddBuffEffect 205, ModifyActorStatus 97, Health 5, Resource 2, Gas 1 | `effectsPerMicroSCU` |

The raw effect fields per kind:
- `ModifyActorStatus`: `statType`, `statPointChange`, `statCooldownChange`.
- `AddBuffEffect`: `buffType`, and optionally `buffDurationOverride` and `buffValueOverride`.
- `Health`: `healthChange`.
- `Gas`: `gasMass{Gas, Mass}`.
- `Resource`: `consumableResourceType`, `amount`.

### Example (LIVE)

```json
{"name":"MedPen (Hemozal)","recordName":"EntityClassDefinition.crlf_consumable_healing_01",
 "id":"7d50411f-088c-4c99-b85a-a6eaf95504c3","itemType":"FPS_Consumable","subType":"MedPack",
 "volumeMicroScu":30,"contents":[{"subtype":"Coagulant","type":"Medical","ratio":1}],
 "health":112.5,"stats":{"BloodDrugLevel":30},"oneShot":true,"discardWhenConsumed":true}
```

One subtype:

```json
{"name":"Coffee","type":"Drink","effectsPerMicroScu":[
  {"kind":"ModifyActorStatus","statType":"Thirst","statPointChange":0.04,"statCooldownChange":0.04},
  {"kind":"AddBuffEffect","buffType":"CognitiveBoost"},{"kind":"AddBuffEffect","buffType":"Dehydrating"}]}
```

## Derived, estimated and curated values

- **Totals:** `health`, `stats`, `gases` and `resources` are totals computed by this crate. Each is the effect per micro-SCU × `volumeMicroScu` × `ratio`, rounded to 3 decimals. The records quote effects per micro-SCU; scaling them to the whole item assumes the item is consumed in full. The code treats `oneShot: false` items the same way.
- **Buff durations:**
  - The module comment says the buff definitions are not in the game data (`consumables.rs:7-8`). So a buff without an override has no duration, and the dataset does not estimate one.
  - In the local extract, no record outside the consumable database mentions a buff type name (for example `CognitiveImpair`). That extract holds only the domains' record folders, so whether a buff database exists elsewhere in DataCore is not established.
- **`byContentType`:** classifies by the first content only.
- There are no `Estimate` fields and no `_assumptions`.

## Redaction

Nothing. The file is not in `PROSE_FIELDS` and has no `_redacted` key. `effectDescription` is dropped by the parser itself (`consumables.rs:78`), so the file carries no effect prose.

## Consumers

- **starorg-codex** (`feat/dataset-reuse`):
  - `crates/starorg-mcp/src/prebuilt.rs:159` needs `consumables`, `subtypes` and `summary`, schema 1, verified on `4.10.200.58746`.
  - `crates/starorg-mcp/src/tools.rs:861` routes `FPS_Consumable`, `Food`, `Drink` and `Bottle` items here, and matches by `id` or the `recordName` tail.
- **MCsim, SCMCP, snares, Bindscape, sc-telemetry:** no reference.
- **Contracts:**
  - `_build` equals the tag, and the `_schema` stem is `game-consumables` ([consumer-datasets](../plans/specs/consumer-datasets.md)).
  - starorg's needed top-level keys must stay.

## Derivable data

These come from records the game already ships. Nothing here adds to CIG's data.

**Fields the source records carry but the dataset does not read**

- **`statCooldownChange`:**
  - Every `ModifyActorStatus` effect carries `statCooldownChange` next to `statPointChange`, but only the latter is totaled.
  - For DeconPen (Canoiodide), subtype `RadiationBlocker` has `BodyRadiation` with `statPointChange` 0 and `statCooldownChange` 300. The item therefore shows `stats.BodyRadiation: 0`, and its only radiation effect is lost.
  - On most food the two values are equal (Coffee 0.04 / 0.04). The RadiationBlocker effect is the only one where `statCooldownChange` is non-zero and differs from `statPointChange`.
  - What the cooldown change does in play is not established. It should be totaled alongside as its own figure.
- **Consumable items outside the filters:** `SCItemConsumableParams` also appears under `entities/scitem/human/un/` (10 records), `entities/scitem/prop/` (1 record) and `entities/scitem/usables/` (1 record, `CleaningSpot_Janitor_CleanUp_Puddle`, with empty contents), which the filters do not cover. Three of them have real contents:
  - `food_bar_karoby_01_a` and `food_bar_karoby_01_cranberry_a`: subtype `ConsumableSubtype.834142c6-…`, which is `Bar_Energy_Karoby`.
  - `food_ration_pack_h`: subtype `Sachet_MRE_ReadyMeal`.

  None is in the dataset. Whether `human/un` items are available in the PU is not established.
- **Unused subtypes:** 15 of 83 subtypes are referenced by no published item: `Bar_Energy_Karoby`, `Coffee`, `Cola`, `EVA_SuitFuel`, `Gin`, `Orange_Juice`, `Puree`, `Rum`, `SQ42_MedicateAll`, `SQ42_OverdoseRevival`, `SQ42_Sedative`, `SQ42_SuitFuel`, `Steak_And_Veg`, `SuperCoagulant` and `Whiskey`. `EVA_SuitFuel` is the only PU `Resource` effect (`Fuel`, amount 1), which is why `resources` is empty on every row. The item that carries it, if any, is outside the filters.
- **Other consumable fields:** `SCItemConsumableParams` also holds `containerClosed`, `canBeReclosed`, `isTwoHandedConsume`, `canConsumeInProne` and `containerTypeTag` (for example `food_box_bbk_01_agrodolce_a`: false, false, false, false, ""). These are usability flags with no prose.

**Values computable by joining with other datasets**

- **Healing per micro-SCU of inventory:** `health ÷ volumeMicroScu`, and Hunger or Thirst per item, for comparing food. Consumer: starorg item detail.
- **Where to loot each item:** `game-loot` and `game-item-obtain`, by `recordName`. Per-shop stock and prices are server-side; `game-shops` gives only the sell-back percent per item type.
- **Overdose headroom:** `stats.BloodDrugLevel` per pen against a BloodDrugLevel limit. The limit is not in this dataset, and no record found in the local extract states it, so this is an estimate until the limit is located.

**Parse gaps (with evidence)**

- **Empty contents:** 43 items have empty `defaultContents` in the records. For example, `Food_box_bbk_01_agrodolce_a` has `defaultContents: []`, as do the mugs, plates and skewers. They are published with `contents: []` and counted as `Unknown`. That is correct for the records, but these props could be flagged as having no effect.
- **Null subtype shows as `"undefined"`:**
  - 2 items have a content whose `consumableSubtype` is `null`: `Food_noodle_ball_1_a` and `Food_box_noodle_01_a_Animation`. `food_noodle_ball_1_a` has `{"consumableSubtype": null, "ratio": 0.0}`.
  - The JavaScript-compatible `js_string` turns this into `subtype: "undefined"`, with `type: ""` (`consumables.rs:238`).
  - Such a content should be skipped, or kept as `null`.
- **Buffs keep only two override fields:** a buff keeps only its two overrides. A buff without a duration override (168 of 341) has no length in the dataset.
