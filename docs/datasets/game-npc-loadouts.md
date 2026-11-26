---
title: game-npc-loadouts (game-npc-loadouts.json)
tags: [docs, datasets]
updated: 2026-11-25
---

# game-npc-loadouts (game-npc-loadouts.json)

What each NPC type (actor) can spawn wearing and wielding, which is what its body can be looted for besides the pockets its loot table fills (`game-loot`). Per actor: faction, body loot table, whether the body is lootable, the loadout's wear range, and every gear item with the chance it is there. The chances are **estimates**: the records give no weights, so every pick is assumed even (`_assumptions`). On LIVE 4.10.201.48085: 2,312 actors with a loadout, 2,162 with gear, 2,447 distinct items, 291 kits, 2,311 loadout files read (65 named but missing); 2,673,972 bytes minified. PTU 5.0.202.35026: 2,698 actors, 2,552 with gear, 2,773 items, 3,424,897 bytes.

## Source

- Filters at `crates/sc-gamedata/src/domains/npc_loadouts.rs:1236`: `**/actor/actors/**`, `**/loadoutkits/**`, `**/entities/scitem/**`, `**/tagdatabase/**`; archive files `Data/Scripts/Loadouts/**` (`files()`, CryXmlB or text XML, npc_loadouts.rs:1245).
- Actors are `EntityClassDefinition` records under `libs/foundry/records/actor/actors/` (`is_actor`, npc_loadouts.rs:1207) with `SEntityComponentDefaultLoadoutParams.loadout`. The module comment (npc_loadouts.rs:6-40) lists the loadout shapes followed: `SItemPortLoadoutXMLParams` (a loose loadout file), `SItemPortLoadoutManualParams`, `SItemPortLoadoutRandomGeneratedParams` (`LoadoutAssortments` → `SLoadoutAssortment` kits under `loadoutkits/`, gated by `Requirement` tag rules), `SItemPortLoadoutOutfitParams` (`BaseLoadout` plus `Outfits`) and `SItemPortLoadoutRecordParams`, plus `InventoryItems` on any of them; nesting is cut at depth 8.
- Requirements are evaluated against the actor's own tags (record `tags` plus its geometry's model tags).
- Per actor: `faction` = `StaticEntityClassData` `SOrganizationRelationships.primaryOrganization.organization` (`faction_of`, :1211); `lootTable` = `LootGenerationComponentParams.lootConfig.lootTableV3`, else `lootTable` (`loot_table_of`, :1220); `lootable` = the record has `SEntityLootableContainerParams`; `wear` = the loadout's `WearRange` (`minimum`, `maximum`; `wear_of`, :1199).
- Items: class, `AttachDef.Type` / `SubType` (for the slot, `slot_of` :317), display name, and the `Item/Inventory/Unlootable` tag.
- Domain at npc_loadouts.rs:1227, `parse` at :1292. Ported from starorg-codex `origin/dev` ([dump-reuse](../plans/specs/dump-reuse.md) C). Derives from no other dataset.

## Shape

Top-level keys: `_source`, `_extracted`, `_build`, `_schema`, `_assumptions[]` (1 string), `actors[]`, `byLootTable{}`, `summary{}`. `_schema: "game-npc-loadouts@1"` sits after `_build` (since tags 4.10.201.48085 / 5.0.202.35026, commit `b9e3700d0`) and is counted in the byte sizes above. `build.json` has an `extractor` and a `files[]` entry with the file's hash (since tags 4.10.201.48085 / 5.0.202.35026).

### `actors[]` (sorted by `class`; only actors with at least one gear item)

| Field | Type | Meaning | Units/range (LIVE) | Source |
|---|---|---|---|---|
| `class` | string | Actor record short name | 2,162 | record name |
| `name` | string | Words made from the class name by this code (`readable_class`, :398); not a string from the records | | derived |
| `faction` | string? | Primary organization record short name | 2,075 present; `genericcivilians` 1,508, `genericcriminal` 128, `crusaderindustries` 109, ... | see Source |
| `lootTable` | string? | Body loot table, lower case as referenced | 107 present (19 distinct) | see Source |
| `lootable` | boolean | Whether the body can be looted | true 2,161, false 1 | `SEntityLootableContainerParams` present |
| `wear` | object? | `{min, max}`: the loadout's wear range as given; widened to the union when nested loadouts give several | 112 present; 0 to 1 | `WearRange` |
| `items` | array | Gear items, by slot order then likeliest first | 22,247 rows in all | worked out |

### `actors[].items[]`

| Field | Type | Meaning | Units/range (LIVE) | Source |
|---|---|---|---|---|
| `item` | string | Item class name | 2,447 distinct | loadout entry / file |
| `name` | string? | Item display name when real | 21,827 of 22,247 rows | `AttachDef.Localization.Name` |
| `slot` | string | One of 16 slots (`SLOTS`, :82): `Undersuit`, `Helmet`, `Torso`, `Arms`, `Legs`, `Backpack`, `Clothing`, `Primary weapon`, `Sidearm`, `Utility`, `Optics`, `Weapon attachments`, `Magazines`, `Grenades`, `Medical`, `Carried` | `Clothing` 13,621 rows, `Undersuit` 1,401, `Torso` 1,224, ... `Carried` 2 | item type, else port name |
| `chance` | number | **Estimate:** chance the item is there, even picks assumed, compounded through nesting | 0.00833 to 1 (29 values print as 0.9999999999999999 or ...98) | `item_chances` (:1104) |
| `requires` | string[]? | Conditions the chance holds under, English text this code writes | present on 6 rows (1 or 2 labels each); 2 labels on LIVE: `outfit picked by the mission (Outfits/S42/Work)` (5 times), `... (Outfits/S42/OffDuty)` (4) | derived |

### `byLootTable{}`

Loot table → actor classes whose body rolls it. 19 tables on LIVE, e.g. `military` 34 actors, `generalloot_ammoandmedics` 30, `v3loottable_actor_prisoner` 14. Every key is a `game-loot` table name (case-insensitive).

### `summary`

`actors` 2,312 (actors with a loadout), `withGear` 2,162, `items` 2,447, `kits` 291, `loadoutFiles` 2,311, `missingLoadoutFiles` 65, `unlootableDropped` 698, `groupsFailed` 2,146 (kit groups dropped because the actor's tags fail their requirement), `requirementsNotRead` 0, `depthCut` 0.

### Example (LIVE, trimmed)

```json
{
  "class": "PU_Human_Enemy_GroundCombat_NPC_GenericCriminal_AcePilot",
  "name": "Generic criminal ace pilot",
  "faction": "genericcriminal",
  "lootTable": "loottable_actor_acepilot",
  "lootable": true,
  "wear": {"min": 0, "max": 0.5},
  "items": [
    {"item": "basl_combat_light_flightsuit_02_01_01", "name": "Ace Interceptor Racing Flight Suit", "slot": "Undersuit", "chance": 1},
    {"item": "basl_combat_light_helmet_02_01_01", "name": "Ace Interceptor Helmet", "slot": "Helmet", "chance": 1},
    {"item": "volt_smg_energy_01", "name": "Quartz Energy SMG", "slot": "Primary weapon", "chance": 1}
  ]
}
```

## Derived, estimated and curated values

- `items[].chance` is an estimate. The `_assumptions` note (`PICK_ASSUMPTIONS`, :70) says so: each pick among a group's options is assumed even (1/n, a repeated option counting as weight), compounded through nesting; of same-named groups, gated ones whose requirement holds are the pick and ungated ones the fallback, which is inferred from the data, not stated by it. The field keeps the codex name `chance` instead of `chanceEstimate`, because the app reads it as is ([dump-reuse](../plans/specs/dump-reuse.md) C).
- `slot` is this code's classification (item type, else port name).
- `name` (actor) and the `requires` labels are text this code writes.
- Left out by rule: body, head, hair, skin and mobiGlas ports (`COSMETIC_PORTS` :287, `COSMETIC_TYPES` :305), items tagged `Item/Inventory/Unlootable` (698 dropped), and items that are no personal gear.

## Redaction

Nothing. Not in `PROSE_FIELDS`; there are no prose fields.

## Consumers

- **starorg-codex app** (`feat/dataset-reuse`): `crates/starorg-mcp/src/prebuilt.rs:174` needs `actors`, `byLootTable`, `summary` and the file must read as `domains::npc_loadouts::NpcLoadouts`; not yet verified against a tag (`verified: &[]`). The Loot screen reads it in `crates/starorg-mcp/src/loot.rs`: `gear_rows` (:306) keeps actors with `lootable` (:315) and merges `wear` (:352); item chances and slots per class at :557. `app/src-tauri/src/contract/real/mod.rs:2625` loads it for the Loot screen.
- Not read by SCMCP, snares, MCsim, Bindscape or sc-telemetry.
- Contracts that must not break:
  - [dump-reuse](../plans/specs/dump-reuse.md) E.4 and C: top-level `actors`, `byLootTable`, `summary`, and the `NpcActor` / `GearItem` / `Wear` field names as in the codex (`class`, `name`, `faction`, `lootTable`, `lootable`, `wear{min,max}`, `items[]{item, name, slot, chance, requires}`). `chance` stays `chance`.
  - The sc-telemetry rules for every dataset in [consumer-datasets](../plans/specs/consumer-datasets.md).

## Derivable data

Actor records are not in the LIVE mixed extract (`sc-gamedata/extracted-data`); the raw values below are from `sc-gamedata/extracted-data-full` (files dated 2026-09-05, build not recorded), actor `pu_human_enemy_groundcombat_npc_genericcriminal_acepilot`.

Fields the records carry that this dataset does not read:

- The rest of the body's `LootGenerationComponentParams.lootConfig.lootConstraints`: `fullnessFactorRange` (`0.7429` to `0.7622`), `chanceToGenerate` (1.0), `totalResultsLimit` (0), `chanceToGenerateAdditionalAttachedInventories`, `poolFilter`, `secondaryChoices`. Only the table is kept, so a reader cannot apply the actor's own pool filter or choices to its body loot.
- `SEntityComponentHostilityParams.factionSC` (`factions/faction_unlawful_outlaws`) and `faction` (`factions_legacy/criminal`), beside the organization `faction` this file reads.
- `SLawComponentParams.minCrimeStat` 3 / `maxCrimeStat` 5, and `SAISkillsComponentParams.skills` / `profile` references. Meanings not established.
- `InventoryItems[].amount` is read only to skip non-positive amounts (:796); the count itself is not published (values seen in that extract: 0, 1, 2, 3).

Values computable by joining:

- **Body loot, expanded:** `lootTable` → `game-loot` tables → `game-item-tags` items gives what an actor's pockets can hold, next to what it wears; the odds would be the obtain estimates.
- **Item → actors** reverse index (which NPC types spawn wearing item X, with the best `chance`): a new source for `game-item-obtain`, which today has no NPC gear. 90 of the 2,447 gear classes have no `game-item-obtain` row at all.
- **Faction → gear:** per `faction`, the items its actors wear, for "who drops this armour" questions.

Parse gaps:

- 2,055 of 2,162 actors with gear have no `lootTable`; whether their bodies roll loot by another route is not established from these records.
- 65 loadout files named by the records are not in the archive (creature loadouts such as `Qausi_graser/tauroch_*.xml`) and are left out ([dump-reuse](../plans/specs/dump-reuse.md) C).
- Creature trophies held in ports (`Harvestable_Trophy_1H_KopionHorn`, Valakkar fangs, Quasigrazer eggs; 36 entries of type `Misc/Harvestable`) are left out as no personal gear ([dump-reuse](../plans/specs/dump-reuse.md) C, known gaps).
- 55 gear classes have no `game-item-tags` row (`basl_undersuit_01_01_01`, `bautista_outfit_01`, ...), so their tags and loot flags are unknown to readers.
- `chance` is not rounded (29 values just under 1), as in the codex.
