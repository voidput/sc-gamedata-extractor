---
title: game-armor (game-armor.json)
tags: [docs, datasets]
updated: 2026-11-25
---

# game-armor (game-armor.json)

Personal armour and clothing: what a character wears, and what it protects against. Each row has its slot, weight class, mass, temperature range, radiation, g-force resistance and storage. Armour rows also have the damage-resistance profile they point at, with per-type multipliers, impact-force resistance and EM signature. LIVE 4.10.201.48085 has 2,497 `armor` rows and 1,874 `clothing` rows. Published size: 2,115,970 bytes minified (`jq -c . | wc -c`), the largest of the item datasets. The `_schema` key (since tags 4.10.201.48085 / 5.0.202.35026, commit `b9e3700d0`) is not in that figure; with it the same command prints 2,115,995.

## Source

- **Records** (filters, `crates/sc-gamedata/src/domains/armor.rs:223`):
  - `**/entities/scitem/characters/human/armor/**`, `.../starwear/**` and `.../clothing/**`: the worn items.
  - `**/damage/**`: `DamageResistanceMacro` records.
  - `**/inventorycontainers/**`: storage capacity.
- **Prune** (`armor.rs:238`): worn-item records keep only `SAttachableComponentParams.AttachDef`, `SCItemClothingParams`, `SCItemSuitArmorParams`, `SCItemInventoryContainerComponentParams` and `SEntityPhysicsControllerParams.PhysType`. Whole, they would take 3.0 GB on LIVE. `SCItemClothingParams`, `SCItemSuitArmorParams` and `SCItemInventoryContainerComponentParams` are kept whole. Any other component on these records cannot be read without widening the prune.
- **Split** (`Armor::parse`, `armor.rs:256`):
  - A record under `/entities/scitem/characters/` whose `AttachDef.Type` starts with `Char_Armor_` goes to `armor`.
  - One that starts with `Char_Clothing_` goes to `clothing`.
  - Templates are dropped (`item_identity`, `crates/sc-gamedata/src/helpers_items.rs:117`).
- **Resistances:** `SCItemSuitArmorParams.damageResistance` is a file path to a `DamageResistanceMacro`, which is resolved through `records.by_record_path` (`parse_resistances`, `armor.rs:83`). The module comment says a multiplier is the share of damage that gets through, so 0.8 blocks 20%.
- **Storage:** `SCItemInventoryContainerComponentParams.containerParams` gives an inventory container record. Its `inventoryType.capacity` is converted to micro-SCU (`container_micro_scu`, `armor.rs:113`): `microSCU` as is, `centiSCU × 10,000`, or `standardCargoUnits × 1,000,000`.
- **Inputs from other datasets:** none.

## Shape

Top-level keys: `_source`, `_extracted`, `_build`, `armor[]`, `clothing[]`, `bySlot` and `summary`. `_schema: "game-armor@1"` after `_build` (since tags 4.10.201.48085 / 5.0.202.35026).

- `summary`: `{armor: 2497, clothing: 1874}`.
- `bySlot`: rows per slot, in first-seen order.
  - `armor`: Arms 469, Backpack 157, Core 486, Helmet 690, Legs 478, Undersuit 217.
  - `clothing`: Backpack 1, Feet 310, Hands 129, Hat 203, Legs 364, Torso_0 335, Torso_1 532.

Order: `slot`, then `name` (`compare_names`).

### armor[] / clothing[] (struct `WornItem`, `armor.rs:37`)

| Field | Type | Meaning | Units/range (armor; clothing) | Source |
|---|---|---|---|---|
| `name` | string | Localized name | | `AttachDef.Localization.Name` |
| `recordName`, `id` | string | Record name and GUID | | record |
| `slot` | string | From `AttachDef.Type`: `Char_Armor_Torso` → `Core`, otherwise the `Char_Armor_` / `Char_Clothing_` prefix is dropped (`slot_of`, `armor.rs:125`) | see `bySlot` | AttachDef |
| `weight` | string? | `AttachDef.SubType`, with `UNDEFINED` dropped | armor: Light 779, Heavy 658, Medium 583, Personal 149, Helmet 128, LightArmor 7, absent 193 (186 undersuits, 7 cores). Clothing: 4 rows (Heavy 2, Medium 2). `Helmet` (all on `Helmet` slot rows), `Personal` and `LightArmor` (both only on `Backpack` rows) are not weight classes; what they mean is not established | AttachDef |
| `manufacturer` | string? | Manufacturer record name, upper case | absent on 144 armor rows and 23 clothing rows | AttachDef |
| `grade` | number | `AttachDef.Grade` | 1..2 (grade 2 on 18 armor and 3 clothing rows) | AttachDef |
| `mass` | number | `PhysType.Mass` | unit not stated in code. 1..160; 0.1..7 | physics |
| `temperatureMin`, `temperatureMax` | number | `SCItemClothingParams.TemperatureResistance.MinResistance` / `MaxResistance` | unit not stated in code. −225..3 / 0..225; −80..10 / 10..110. Like the next three fields, absent on 153 armor rows (all backpacks) | clothing params |
| `radiationCapacity` | number | `RadiationResistance.MaximumRadiationCapacity` | 0..52800; 0..26800 | clothing params |
| `radiationDissipationRate` | number | `RadiationResistance.RadiationDissipationRate` | 0..251.1; 0..145.8 | clothing params |
| `gForceResistance` | number | `Flight.gForceResistance` | −0.875..1; −0.5..0 | clothing params |
| `storageMicroScu` | number? | Capacity of the referenced inventory container | micro-SCU. 2000..180000 (1150 rows); 400..33800 (894 rows) | inventory container |
| `resistanceProfile` | string? | Short name of the resistance macro, or the referenced name when it does not resolve | armor: LightArmor 782, HeavyArmor 620, MediumArmor 579, UndersuitArmor 256, HeavyArmorUtility 51, CombatFlightsuitArmor 41, SuperHeavyArmor 8, `HeavyArmor_ai_exclusive` 3. Clothing: 34 rows | `SCItemSuitArmorParams.damageResistance` |
| `resistances` | object? | Per damage type (`physical`, `energy`, `distortion`, `thermal`, `biochemical`, `stun`): `{multiplier, threshold?, damageCap?}`. `threshold` and `damageCap` are kept only when non-zero. `threshold` never appears on LIVE. `damageCap` appears on the 8 `SuperHeavyArmor` rows: 45 for every type except `biochemical`, which is 10 | multiplier 0.125..0.95 | macro `damageResistance.<Type>Resistance` |
| `impactForceResistance` | number? | Macro `impactForceResistance.impactForceResistance` | 0.125..1; clothing 0.65..0.9 (34 rows) | macro |
| `emSignature` | number? | `signatureEmission` of the `Electromagnetic` entry in `SCItemSuitArmorParams.signatureParams` | 0..50; clothing 2..30 (10 rows) | suit armor params |

The domain computes `nameKey`, `descriptionKey`, `size` and `type` through `item_identity` but does not emit them.

### Example (LIVE)

```json
{"name":"ADP Core","recordName":"EntityClassDefinition.cds_legacy_armor_heavy_core_01_01_01",
 "id":"cf98d3f9-24d1-47b2-9cb9-f7ba09e3571f","slot":"Core","weight":"Heavy","manufacturer":"CDS",
 "grade":1,"mass":7,"temperatureMin":-75,"temperatureMax":105,"radiationCapacity":26800,
 "radiationDissipationRate":145.8,"gForceResistance":-0.5,"storageMicroScu":10500,
 "resistanceProfile":"HeavyArmor",
 "resistances":{"physical":{"multiplier":0.6},"energy":{"multiplier":0.6},"distortion":{"multiplier":0.6},
                "thermal":{"multiplier":0.6},"biochemical":{"multiplier":0.6},"stun":{"multiplier":0.4}},
 "impactForceResistance":0.65,"emSignature":30}
```

## Derived, estimated and curated values

- `slot` is renamed by this crate (`Core` for torso armour). `bySlot` and `summary` are counts.
- `storageMicroScu` is a unit conversion of the container capacity.
- There are no estimates, no `Estimate` fields and no `_assumptions`.
- The "share that gets through" reading of `multiplier` comes from the module comment (`armor.rs:5-6`), not from a field.

## Redaction

Nothing. The file is not in `PROSE_FIELDS` and has no `_redacted` key. One row carries the game's own unfinished string as its name: `DYL - jacket - name`. The shared placeholder rule does not catch it (see [dump-reuse](../plans/specs/dump-reuse.md)).

## Consumers

- **starorg-codex** (`feat/dataset-reuse`):
  - `crates/starorg-mcp/src/prebuilt.rs:155` needs `armor`, `clothing`, `bySlot` and `summary`, schema 1, verified on `4.10.200.58746`.
  - `crates/starorg-mcp/src/tools.rs:830` routes `Char_Armor*` and `Char_Clothing*` items here, and matches rows by `id` or the `recordName` tail.
  - `crates/starorg-cli/tests/memory_limit.rs:98-117` extracts this domain under a 1.5 GB cap. The prune is what keeps it near 0.9 GB.
- **MCsim, SCMCP, snares, Bindscape, sc-telemetry:** no reference.
- **Contracts:**
  - `_build` equals the tag, and the `_schema` stem is `game-armor` ([consumer-datasets](../plans/specs/consumer-datasets.md)).
  - starorg's needed top-level keys must stay.
  - Any new field must keep the domain within the memory budget. Widen the prune for named components only.

## Derivable data

These come from records the game already ships. Nothing here adds to CIG's data. `SCItemSuitArmorParams` is kept whole by the prune (`armor.rs:238`), so its fields (body parts, aim limits) are already in memory. Carry slots and EVA thrust sit on other components and need the prune widened for those components. Counts below are over the local `extracted-data` character records (5,044 files, 4,512 of them `Char_Armor_*` or `Char_Clothing_*`).

**Fields the source records carry but the dataset does not read**

- **Body-part coverage:**
  - `SCItemSuitArmorParams.protectedBodyParts[]` lists references to `actorhealthcomponent/bodymapping/bodypart/*.json`.
  - Over the local character records: `head` 713, `torso` 492, `rightleg+leftleg` 487, `rightarm+leftarm` 469, the full body (torso, arms, legs) 227, `torso+arms` 14, and an empty list on 1.
  - This is the missing link for damage calculations by hit location.
- **Carry slots:**
  - `SItemPortContainerComponentParams.Ports[]` holds `Name`, `MinSize`, `MaxSize` and `Types`. On `cds_legacy_armor_heavy_core_01_01_01` that is 8 `magazine_attach_*`, 4 `grenade_attach_*`, `wep_stocked_2` (2..4) and `wep_stocked_3` (2..5) for `WeaponPersonal`, `backpack` (max size 3), and `gadget_attach_1`.
  - Across the 4,512 worn-item records, ports named `magazine*` number 0 (3786 records), 1 (5), 2 (200), 4 (204), 6 (138) or 8 (178). One record has no port component.
  - These counts show how much a kit carries.
- **Other suit-armor fields:** `SCItemSuitArmorParams` also has `actorAimLimits` and `restrictedMoveViewPenalty` (record references, for example `aimlimits_armor_heavy` and `moveviewrestriction_armor_heavy` on the ADP Core), `integrityMilestoneToBreak` (0.1 on every record that has the component), and `wearMovementMultipliers`. What they do in play is not established. They are references and numbers, not prose.
- **EVA thrust:** `SCItemSuitThrusterPackParams.LegacyThrustParams` appears on 489 `Char_Armor_Torso`, 222 `Char_Armor_Undersuit` and 2 `Char_Clothing_Torso_1` records. `MaxLinearVelocity` is 6.0 on all 713, so the value adds little today.
- **Identity:** `nameKey`, `descriptionKey` and `size` are already computed through `item_identity` and then discarded. Emitting them costs no parsing.

**Values computable by joining with other datasets**

- **Armour set totals:** sum `mass` and `storageMicroScu`, and take the lowest `temperatureMin` and the highest `temperatureMax`, across one helmet, core, arms, legs and undersuit. The records do not group pieces into sets, so any set grouping by record-name stem (for example `cds_legacy_armor_heavy_*_01_01_01`) is an estimate.
- **Effective damage taken:** per hit, [game-ammo](game-ammo.md) `damage[type] × resistances[type].multiplier` on the covered body part. This is an estimate, because the records do not state how `multiplier` and `damageCap` (set on `SuperHeavyArmor` only) combine, and the impact versus explosion split is not resolved in game-ammo yet.
- **Where to get each piece:** `game-loot`, `game-npc-loadouts` and `game-item-obtain`, by `recordName`. `game-shops` holds only global shop rules (per-shop stock and prices are server-side), so it adds sell-back percent by item type, not a place to buy. Consumer: starorg item detail.

**Parse gaps (with evidence)**

- **Backpacks without resistances:** 157 armor rows have no `resistances` or `resistanceProfile`, and all of them are `Backpack`. Backpacks carry no `SCItemSuitArmorParams`, so this is expected, not a loss.
- **`weight` holds non-weight values:** 128 `Helmet` and 149 `Personal` rows, because the field is simply `AttachDef.SubType`. The 7 `LightArmor` rows are backpacks too, so a consumer that filters on Light/Medium/Heavy must decide how to treat them.
- **Duplicate body parts:** one record lists the full body twice in `protectedBodyParts` (`torso+leftarm+rightarm+leftleg+rightleg+leftarm+rightarm+leftleg+rightleg`). Deduplicate it if coverage is added.
