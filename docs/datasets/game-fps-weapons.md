---
title: game-fps-weapons (game-fps-weapons.json)
tags: [docs, datasets]
updated: 2026-11-25
---

# game-fps-weapons (game-fps-weapons.json)

A thin catalogue of personal weapons: every ranged gun, gadget and melee weapon under `entities/scitem/weapons/`, with its kind, type, sub-type and size. Combat figures (DPS, ammo, modes) for the same ranged weapons are in [game-weapons](game-weapons.md) `fpsWeapons[]`. The 492 ranged ids are identical in both files. LIVE 4.10.201.48085 has 520 weapons: 492 ranged and 28 melee, of which 76 are templates. Published size: 113,136 bytes minified (`jq -c . | wc -c`). The `_schema` key (since tags 4.10.201.48085 / 5.0.202.35026, commit `b9e3700d0`) is not in that figure; with it the same command prints 113,167.

## Source

- **Records:** filter `**/entities/scitem/weapons/**` (`crates/sc-gamedata/src/domains/fps_weapons.rs:91`). This covers `fps_weapons/`, `melee/`, `magazines/`, `weapon_modifier/`, `appearance_modifier/`, `devices/`, `mines/`, `throwable/`, `turret_unmanned/` and `weapon_mounted/`.
- **Selection** (`parse_fps_weapon`, `fps_weapons.rs:50`):
  - Any record with `SCItemWeaponComponentParams` is `ranged`.
  - A record with only `SMeleeWeaponComponentParams` is `melee`. 446 of the 492 ranged records carry both components (for example `behr_rifle_ballistic_01`), and are counted as ranged.
  - Records with neither (magazines, attachments) are skipped.
- **Fields:** `AttachDef` through `attach_def_or_empty`, `display_name` and `sub_type_of` (`crates/sc-gamedata/src/helpers_items.rs:89-115`).
- **Inputs from other datasets:** none.

## Shape

Top-level keys: `_source`, `_extracted`, `_build`, `weapons[]` and `summary`. `_schema: "game-fps-weapons@1"` after `_build` (since tags 4.10.201.48085 / 5.0.202.35026).

`summary`: `{weapons: 520, ranged: 492, melee: 28, templates: 76}`.

Order: `name` by locale comparison.

### weapons[] (struct `FpsWeapon`, `fps_weapons.rs:30`)

| Field | Type | Meaning | Units/range | Source |
|---|---|---|---|---|
| `name` | string | Localized name. The record short name for a template | | `AttachDef.Localization.Name` |
| `recordName` | string | `EntityClassDefinition.<short>` | | record |
| `id` | string | Record GUID | | record |
| `kind` | `"ranged"` \| `"melee"` | Which weapon component the record has | ranged 492, melee 28 | component |
| `type` | string? | `AttachDef.Type` | WeaponPersonal 486, WeaponGun 4 (`GATS_BallisticGatling_Mounted_S1*`). Absent on 30 | AttachDef |
| `subType` | string? | `AttachDef.SubType`, with `UNDEFINED` dropped | Medium 318, Small 81, Gadget 39, Knife 28, Large 20, Gun 4 | AttachDef |
| `size` | number? | `AttachDef.Size` | 1..5 | AttachDef |
| `ammoCapacity` | number? | `SCItemWeaponComponentParams.ammoContainerCapacity` | **never present on LIVE** (0 of 520) | weapon params |
| `heatPerShot` | number? | `SCItemWeaponComponentParams.heatPerShot` | **never present on LIVE** (0 of 520) | weapon params |
| `isTemplate` | true? | The name is a placeholder | 76 | `display_name` |

The 30 rows without `type`, `subType` and `size` are all `*_prop` / `*_Prop` store or collector display props, with `isTemplate: true`. They have no `AttachDef`.

### Example (LIVE)

```json
{"name":"Demon Fang Combat Knife","recordName":"EntityClassDefinition.apar_melee_01",
 "id":"9e8d6cf1-17d6-453e-bfe3-44529969eebf","kind":"melee","type":"WeaponPersonal",
 "subType":"Knife","size":1}
```

## Derived, estimated and curated values

- `kind` and `isTemplate` are classifications made by this crate:
  - `kind` comes from which component is present.
  - `isTemplate` comes from the shared placeholder-name rule.
- Unlike `game-weapons`, a `_prop` short name alone does not make a row a template here. The 30 props are templates because their name is a placeholder.
- There are no estimates and no `_assumptions`.

## Redaction

Nothing. The file is not in `PROSE_FIELDS`, has no `_redacted` key and carries no prose. It also carries no `nameKey` or `descriptionKey`: the domain does not call `loc_refs`.

## Consumers

- **SCMCP** `src/versions.ts:34`: `"fps-weapons": { file: "game-fps-weapons.json", collection: "weapons" }`. Version diffs key each row by `className`, `ClassName`, `recordName`, `reference`, `id` or `name`, whichever it finds first (`keyOf`, `src/versions.ts:340-343`). `recordName` is the key that applies here.
- **starorg-codex** (`feat/dataset-reuse`):
  - `crates/starorg-mcp/src/prebuilt.rs:161` needs `weapons` and `summary` with schema 1. It is not yet verified against any tag.
  - `prebuilt.rs` `KNOWN_BAD` refuses this file for tag `4.10.200.58746`. That build's dump held a stale `vncl_melee_01_tint03` from an old `extracted-data/` (see [dump-reuse](../plans/specs/dump-reuse.md), item 1).
  - `crates/starorg-mcp/src/tools.rs:828` sends `WeaponPersonal` items to `game-fps-weapons` first, then `game-weapons`.
- **MCsim, snares, Bindscape, sc-telemetry:** no reference to this file.
- **Contracts:**
  - `_build` equals the tag, and the `_schema` stem is `game-fps-weapons` ([consumer-datasets](../plans/specs/consumer-datasets.md)).
  - Keep `weapons` as the collection name and `recordName` as the row key, for SCMCP.

## Derivable data

These come from records the game already ships. Nothing here adds to CIG's data.

**Fields the source records carry but the dataset does not read**

- **Melee damage:**
  - `SMeleeWeaponComponentParams.meleeCombatConfig` points at a `MeleeCombatConfig` record: `entities/scitem/weapons/melee/knifemeleecombat.json` (25 of the 28 melee records) or `knifemeleecombat_gungame.json` (3). The stale `vncl_melee_01_tint03` file also points at the first.
  - Its `attackCategoryParams[]` give per-attack `actionCategory`, `damageInfo`, `attackImpulse` and `forceKnockdown`. For example, `BladeSlash` and `BladeStab` both have `DamagePhysical` 30, `attackImpulse` 20 and `forceKnockdown` `None`.
  - Today no dataset has a damage figure for any melee row.
- **Melee flags:** `canBlock`, `canDodge`, `canBeUsedForTakeDown`, `canBeUsedInProne` and `stanceTransitionMeleeDelay` (0.6 on all 28), on the same component. On `apar_melee_01` the four flags are true, true, true and false.
- **Localization keys and identity:** `nameKey`, `descriptionKey`, `manufacturer` and `grade` are on `AttachDef`, and `game-weapons` already reads them through the same helper. They would let SCMCP and starorg label melee rows without a second dataset.
- **Weapon class label:** `SCItemPurchasableParams.displayType` gives the class key. Over the 520 records: `@item_displayType_Rifle` 144, `Pistol` 72, `SMG` 59, `Shotgun` 45, `Sniper` 43, `LMG` 13, `Shouldered` 11, `Special` 8, `MeleeWeapon` 25, and `@item_TypeGadget` 37. On 26 records it is `@LOC_PLACEHOLDER`, and 37 records (34 ranged, 3 melee) have no `SCItemPurchasableParams`. It replaces the vague `subType` (Small, Medium, Large).
- **Attachment ports:** `SItemPortContainerComponentParams.Ports[]` holds `Name`, `MinSize` and `MaxSize`. For `behr_rifle_ballistic_01` they are `magazine_attach` 1..1, `optics_attach` 1..2, `barrel_attach` 2..2 and `underbarrel_attach` 1..2. This states which attachment sizes fit.
- **Attachments:** 111 of the 125 records in `entities/scitem/weapons/weapon_modifier/` carry `SWeaponModifierComponentParams` with `SWeaponStats` multipliers. For example, `arma_barrel_comp_s1` has `soundRadiusMultiplier` 1.2, and most of its `recoilModifier` multipliers are 0.7. No dataset publishes their stats today; only `game-item-tags` and the search index name them.
- **Other folders under the filter:** `devices/`, `mines/` and `throwable/` hold records that this domain reads only when they carry a weapon component. Their own parameters are not published. Whether they matter to players is not established.

**Values computable by joining with other datasets**

- `kind`, `subType` and `size` joined to `game-weapons.fpsWeapons[]` by `id` give one row per gun with its combat figures. The two files agree on all 492 ranged ids.
- Melee rows joined to the melee config records would give a melee damage table. Consumer: starorg item detail.

**Parse gaps (with evidence)**

- **`ammoCapacity` and `heatPerShot` are never set.**
  - The domain reads `ammoContainerCapacity` and `heatPerShot` from the root of `SCItemWeaponComponentParams`.
  - Neither key exists there in 4.10 records. The root keys of `behr_rifle_ballistic_01` are `fireActions`, `connectionParams`, `weaponRegenConsumerParams`, `ammoContainerRecord` and so on.
  - `heatPerShot` lives on each fire action, and the capacity lives on the magazine's `SAmmoContainerComponentParams.maxAmmoCount`. `game-weapons` reads both correctly.
  - Both fields are null on all 520 rows.
- **Melee count:** the local `extracted-data/.../melee/` holds 31 files and 28 rows come out. The other three are:
  - the two `MeleeCombatConfig` records;
  - `vncl_melee_01_tint03.json`, dated 2026-11-01, a stale file that is not in this build's DataCore (see the module comment, `fps_weapons.rs:8-12`).

  There is no gap here; the count is correct.
