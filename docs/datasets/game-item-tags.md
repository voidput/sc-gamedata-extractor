---
title: game-item-tags (game-item-tags.json)
tags: [docs, datasets]
updated: 2026-11-25
---

# game-item-tags (game-item-tags.json)

Every item record that carries record-level tags, with those tags resolved from GUIDs to paths (`Weapon/FPS/Stocked/Rifle`). Loot archetypes select items by tag rather than by name, so this file is the join between items and tag-driven loot (`game-loot`, `game-item-sources`, `game-item-obtain`). Weapons also list the magazines or canisters they load (`ammoItems`). On LIVE 4.10.201.48085 it holds 12,894 items (448 with `ammoItems`) and is 5,558,826 bytes minified. PTU 5.0.202.35026: 13,649 items, 458 with ammo, 5,694,521 bytes.

## Source

- DataCore records matched by the filters at `crates/sc-gamedata/src/domains/item_tags.rs:237`: `**/entities/scitem/**` (`EntityClassDefinition`) and `**/tagdatabase/**` (the `TagDatabase` record).
- Tag paths: `build_tag_index` (`crates/sc-gamedata/src/helpers_items.rs:477`) walks the `TagDatabase` tree; each `Tag.<guid>` record name maps to its path below the root `Global`.
- Per item (`parse_item_tags`, item_tags.rs:184): the record's own `tags[]` list (`item_tag_paths`, item_tags.rs:61), plus `SAttachableComponentParams.AttachDef` for `Type`, `SubType`, `Manufacturer` and `Localization.Name` / `Localization.Description`.
- `ammoItems` (`ammo_links`, item_tags.rs:130): `SCItemWeaponComponentParams.ammoContainerRecord` (skipped when `ShouldIgnorePrimaryAmmoContainer` is true) and `secondaryAmmoContainers[].ammoContainerRecord`, each widened to every other item whose `AttachDef.SubType` is `Magazine` and whose sorted `AttachDef.Tags` string is the same.
- Domain: `Domain for ItemTagsDomain` at item_tags.rs:228, `parse` at item_tags.rs:267. Needs the localization table.
- Derives from no other dataset. Read by `game-item-obtain`, `game-search-index` (`crates/sc-gamedata/src/search_index.rs`) and the obtain query.

## Shape

Top-level keys: `_source`, `_extracted`, `_build`, `_schema`, `items[]`, `summary{}`. `_schema: "game-item-tags@1"` sits after `_build` (since tags 4.10.201.48085 / 5.0.202.35026, commit `b9e3700d0`) and is counted in the byte sizes above. `build.json` has an `extractor` and a `files[]` entry with the file's hash (since tags 4.10.201.48085 / 5.0.202.35026).

### `items[]`

Sorted by `className` (`compare_names`). Only records with at least one resolvable record-level tag are listed.

| Field | Type | Meaning | Units/range (LIVE) | Source |
|---|---|---|---|---|
| `name` | string | Localized display name; the class name when the name is a placeholder, empty or unresolved | always present | `AttachDef.Localization.Name` via the string table |
| `className` | string | Record short name | 12,894 unique | record name |
| `id` | string | Record GUID | | `_RecordId_` |
| `type` | string? | Item type | 12,163 present, 731 absent; 75 values, e.g. `Cargo` 1,549, `Misc` 1,013, `Usable` 947, `Char_Armor_Helmet` 704, `WeaponPersonal` 509, `UNDEFINED` 468 | `AttachDef.Type` |
| `subType` | string? | Item subtype; `UNDEFINED` is dropped | 5,891 present | `AttachDef.SubType` |
| `manufacturer` | string? | Referenced manufacturer record's name, upper case (`BEHR`) | 6,921 present | `AttachDef.Manufacturer` (`manufacturer_of`, helpers_items.rs:97) |
| `isTemplate` | `true`? | Set when the display name is not a real one, so `name` fell back to the class name | 2,603 rows | `display_name` (helpers_items.rs:109) |
| `nameKey` | string? | String-table key of the name | 12,122 present | `Localization.Name` (`loc_refs`, localization.rs) |
| `descriptionKey` | string? | String-table key of the description | 6,933 present | `Localization.Description` |
| `nameStatus` | string? | `placeholder`, `unresolved` or `empty`; absent when the name is real. On LIVE it is set on the same 2,603 rows as `isTemplate` | placeholder 1,770, empty 772, unresolved 61, absent 10,291 | `name_status_of` (localization.rs:300) |
| `tags` | string[] | Record-level tag paths, deduplicated, sorted by UTF-16 code units | 1 to 17 per item; 1,404 distinct paths across all items | `tags[]` resolved through the tag index |
| `ammoItems` | string[]? | Class names of the magazines or canisters the weapon loads, the named container first, then same-attach-tag magazines by name | 448 rows | see Source |

Top-level tag roots by occurrence: `Item` 15,249, `LootGeneration` 11,327, `Armor` 10,190, `Clothing` 8,083, `Missions` 6,485, `Manufacturer` 5,020, `Weapon` 3,752, `EntityArea` 3,607, `AI` 3,176, `Race` 2,538.

### `summary`

| Field | Type | Meaning | LIVE |
|---|---|---|---|
| `items` | number | Rows in `items` | 12,894 |
| `withAmmo` | number | Rows with `ammoItems` | 448 |
| `canGenerateAsLoot` | number | Rows tagged exactly `LootGeneration/CanGenerateAsLoot` | 3,779 |
| `cannotGenerateAsLoot` | number | Rows with a tag starting `LootGeneration/CannotGenerateAsLoot` | 1,100 |
| `tags` | number | Entries in the tag index (tag records with a path), not tags used by items | 18,960 |

### Example (LIVE; `tags` trimmed to 6 of the row's 12)

```json
{
  "name": "P4-AR Rifle",
  "className": "behr_rifle_ballistic_01",
  "id": "02d4cd2e-fa98-4086-aee1-6b2dfce8ea27",
  "type": "WeaponPersonal",
  "subType": "Medium",
  "manufacturer": "BEHR",
  "nameKey": "item_Namebehr_rifle_ballistic_01",
  "descriptionKey": "item_Descbehr_rifle_ballistic_01",
  "tags": ["Item/Small/Weapon", "LootGeneration/CanGenerateAsLoot", "LootGeneration/LootRarity/Common",
           "Manufacturer/Behring", "Weapon/FPS/Stocked/Rifle", "Weapon/WeaponSize/S2"],
  "ammoItems": ["behr_rifle_ballistic_01_mag", "behr_rifle_ballistic_03_mag"]
}
```

## Derived, estimated and curated values

- `ammoItems` is derived: the record names one container (or a few), and the widening to every magazine with the same `AttachDef.Tags` string is this domain's rule (item_tags.rs:9), not a list the game stores.
- `isTemplate` and `nameStatus` are classifications of the name reference by this code (`is_placeholder_name`, localization.rs:232), not game fields.
- No estimates, no curated values.

## Redaction

Nothing. `game-item-tags.json` is not in `PROSE_FIELDS` (`crates/sc-gamedata/src/localization.rs`, redaction module), and the file holds no prose: names plus `nameKey` / `descriptionKey`.

## Consumers

- **SCMCP** (`/home/zedwil/git/SCMCP/src`):
  - `versions.ts:46`: generic dataset access, collection `items`.
  - `itemfinder.ts:65`, `:131`: loads `items` for `sc_obtain_item`; `className` / `name` lookups at `:243`, `manufacturer` at `:262`, `ammoItems` at `:337`.
  - `vendor/sc-gamedata/types.ts:9`: the `ItemTags` shape read (every field above).
  - `vendor/sc-gamedata/obtain.ts:282`: `tags` decide loot eligibility and archetype matches.
  - `vendor/sc-gamedata/search-index.ts:8`: search entries are built from it when no published index fits.
- **starorg-codex app** (`starorg-strings`, branch `feat/dataset-reuse`): `crates/starorg-mcp/src/prebuilt.rs:165` takes the dump when it has `items`, `summary.withAmmo` and `summary.tags` and `items` reads as `query::types::ItemTags`. Not yet verified against a tag there (`verified: &[]`).
- **In this crate:** `game-item-obtain` (`crates/sc-gamedata/src/item_obtain.rs:498`) and `game-search-index` (`search_index.rs`).
- Not read by snares, MCsim, Bindscape or sc-telemetry.
- Contracts that must not break:
  - [consumer-datasets](../plans/specs/consumer-datasets.md), sc-telemetry rules for every dataset: `_build` equals the tag; the `_schema` stem equals the file stem.
  - [dump-reuse](../plans/specs/dump-reuse.md) E.1: the app refuses an item-tags file without `ammoItems` / `summary.withAmmo` (as on tag 4.10.200.58746), so both stay.

## Derivable data

Fields the records carry that this dataset does not read (raw values from `sc-gamedata/extracted-data`, LIVE 4.10.201.48085 per its `_build.json`, mixed with stale records):

- `AttachDef.Size` and `AttachDef.Grade`: `behr_rifle_ballistic_01` has `Size: 2`, `Grade: 1`. `item_identity` (helpers_items.rs:117) already reads both for other domains.
- `AttachDef.inventoryOccupancyVolume.microSCU` and `inventoryOccupancyDimensions`: the rifle is `13000` µSCU and 0.0811 × 0.8528 × 0.2487 (the record does not state the unit of the dimensions). With `game-containers` this answers what fits where (see that note; the fit count would be an estimate, the game's packing rule is not in the records).
- `AttachDef.Tags` / `RequiredTags`, the attach tag strings (the rifle's `Tags` is `"stocked behr_rifle_ballistic_01 rifle"`, its `RequiredTags` is empty): `Tags` is used internally for the ammo widening but not published. Publishing both would let a consumer try joining items to item ports; how the game matches them is not established here, so such a join would be an estimate.

Values computable by joining:

- Reverse tag index, tag path → item count / classes, and tag → the `game-loot` archetypes and secondary choices that select it. The obtain query rebuilds this per call (SCMCP `obtain.ts`, the app); publishing it would save every consumer the walk.
- Per item, the NPC actors that spawn wearing it (`game-npc-loadouts` `actors[].items[].item`), for the "where to get" readers.

Parse gaps:

- Items with no record-level tags are not listed at all. The mixed extract holds 24,914 `entities/scitem` record files against 12,894 rows (that count includes stale files, so the gap is approximate). 55 of the 2,447 item classes `game-npc-loadouts` lists have no row here (for example `basl_undersuit_01_01_01`, `bautista_outfit_01`), so those items cannot be joined to loot or names through this file.
- 731 rows have no `type` (no `AttachDef.Type`), and 2,603 rows have a non-real name. The strict generated-name fallback is planned as item 8 of [dump-enhancements](../plans/specs/dump-enhancements.md) (`nameGenerated`, `nameRule`).
- `summary.tags` counts the tag index (18,960), not the 1,404 paths items use; a reader expecting the latter gets a number 13 times larger.
