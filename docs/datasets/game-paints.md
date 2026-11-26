---
title: game-paints (game-paints.json)
tags: [docs, datasets]
updated: 2026-11-25
---

# game-paints (game-paints.json)

Every ship and ground-vehicle paint (livery) item, and the vehicle records it fits, worked out with the game's own item-port tag rule rather than from the paint's title. It holds 1042 paints, 1060 vehicles with a paint port, a `byVehicle` index and the ports themselves (`paintPorts`), on LIVE 4.10.201.48085. Local parse only, not yet in a published tag: 2,913,411 bytes as the parse writes it (indented), 1,997,824 minified. Names, keys and tags only: no texture, material or icon paths, and no description prose.

## Source

- DataCore, filters in `crates/sc-gamedata/src/domains/paints.rs:487`: `**/entities/spaceships/**`, `**/entities/groundvehicles/**`, `**/entities/scitem/ships/**`, `**/scitemmanufacturer/**`.
  - A **paint** is an `EntityClassDefinition` under `entities/scitem/ships/` with `AttachDef.Type` `Paints` (`is_paint`, paints.rs:402). On LIVE every one sits in `entities/scitem/ships/paints/`; the `paint_300_*` dash and seat trims there are `Misc` and not paints.
  - A **vehicle** is what [game-ships](game-ships.md) calls one (`is_vehicle_path` plus `VehicleComponentParams`), named by its `parse_vehicle`, so `vehicles[].name` equals game-ships `name` for the same record. 1104 on LIVE, as in game-ships.
- Archive files, glob `**/Scripts/Entities/Vehicles/Implementations/Xml/**` (CryXmlB, 153 definitions plus 28 `Modifications/` patch files on LIVE). The paint port is defined in one of two places:
  - **Entity** (21 vehicles: 890 Jump, Gladiator, Paladin and their variants): `SItemPortContainerComponentParams.Ports[]` entries whose `Types[].Type` is `Paints`; `PortTags`, `RequiredPortTags`, `MinSize`, `MaxSize` (`entity_paint_ports`, paints.rs:120).
  - **Vehicle definition** (1039 vehicles): the XML named by `VehicleComponentParams.vehicleDefinition`, matched without regard to case. A `<Part>` whose `<ItemPort>` lists `<Type type="Paints"/>`, with `portTags`, `requiredTags`, `minSize`, `maxSize` (`xml_paint_ports`, paints.rs:209). The entity's `VehicleComponentParams.modification` picks a `<Modification>`: its `patchFile` replaces whole sections (`apply_patch`, paints.rs:169), and its `<Elem idRef name value>` entries set one attribute on the element with that `id` (`apply_elems`, paints.rs:184). A Part with `skipPart="1"` is left out. Idris, Ballista, Pisces, Starlifter, Cutlass, Mustang Omega and Apollo/Hermes variants change their paint port's tags this way.
  - No vehicle has both. The entity's own `SItemPortContainerComponentParams` on the others (the 400i, the 100 series) lists other ports but not the paint port.
- Manufacturer: `AttachDef.Manufacturer` of the paint, which on LIVE points at a `scitemmanufacturer/paintcolorlogos/` record carrying the maker's name key (`Makers::of`, paints.rs:455).
- Localization (`global.ini`): paint and vehicle names, manufacturer names.
- Code: `fits` (paints.rs:100), `Domain::parse` (paints.rs:519). Reads no other dataset file.

## Shape

Top-level keys: `_source`, `_extracted`, `_build`, `_schema` (`"game-paints@1"`), `_assumptions[]` (4), `paints[]`, `byVehicle{}`, `paintPorts[]`, `summary{}`.

### `paints[]` (sorted by `className`)

| Field | Type | Meaning | LIVE (of 1042) | Source |
|---|---|---|---|---|
| `className` | string | Record short name | 1042 | record |
| `recordName`, `id` | string | `EntityClassDefinition.<className>`, GUID | 1042 | record |
| `name` | string | Display name, or `className` when the name is a placeholder | 1042 | `AttachDef.Localization.Name` |
| `nameKey`, `descriptionKey` | string? | String-table keys | 1042 / 1042 | `loc_refs` |
| `nameStatus` | string? | Absent when the name is real | 0 | `loc_refs` |
| `manufacturer` | string? | Manufacturer code (`ORIG`, `DRAK`) when exactly one manufacturer record has the logo's name key | 482 | see `_assumptions` |
| `manufacturerName` | string? | The logo record's localised name | 1014 | paintcolorlogos `Localization.Name` |
| `type` | string | Always `Paints` | 1042 | `AttachDef.Type` |
| `subType` | string? | Absent for `UNDEFINED` | 0 | `AttachDef.SubType` |
| `size`, `grade` | number? | | size 1 on all; grade 1 (1036), 2 (6: X1, one 600i) | `AttachDef.Size`, `.Grade` |
| `tags` | string[] | Words of `AttachDef.Tags` (`Paint_400i`, `@Paint_400i_Black_Gold`) | 1042 | |
| `requiredTags` | string[] | Words of `AttachDef.RequiredTags` | 998 non-empty; 1 with two | |
| `vehicles` | `{className, name}[]` | Vehicle records it fits, by name then class | 1025 non-empty; up to 51 | `fits` |

### `byVehicle`

Vehicle `className` → paint `className`s, for every vehicle with a paint port (1060 keys), in `paintPorts` order. An empty list means the vehicle has a port and no paint fits it.

### `paintPorts[]` (sorted by `className`)

`{className, name, vehicleTags[], ports[]}`; each port `{port, source, portTags[], requiredTags[]}` with `source` `entity` or `vehicleDefinition`. `vehicleTags` is the vehicle's `AttachDef.Tags`. Every LIVE vehicle has one paint port, always `hardpoint_paint`.

### `summary` (LIVE)

`paints` 1042, `templatesSkipped` 125, `paintsFittingNoVehicle` 17, `vehicles` 1104, `vehiclesWithPaintPort` 1060, `vehiclesWithNoPaints` 9, `vehiclesWithoutPaintPort` 44, `portsFromEntity` 21, `portsFromVehicleDefinition` 1039, `missingVehicleDefinitions` 1 (`VNCL_Hunter_AI_VAN`), `missingPatchFiles` 3 (`MISC_Starfarer_Wreck*`; base definition used).

### Example (LIVE)

```json
{
  "className": "Paint_400i_Black_Gold",
  "recordName": "EntityClassDefinition.Paint_400i_Black_Gold",
  "id": "0b98f172-5ab8-403b-8a82-093a96911833",
  "name": "400i Penumbra Livery",
  "nameKey": "item_Name400i_Paint_Penumbra_Black_Gold",
  "descriptionKey": "item_Desc400i_Paint_Penumbra_Black_Gold",
  "manufacturer": "ORIG", "manufacturerName": "Origin Jumpworks",
  "type": "Paints", "size": 1, "grade": 1,
  "tags": ["Paint_400i", "@Paint_400i_Black_Gold"],
  "requiredTags": ["Paint_400i"],
  "vehicles": [
    {"className": "ORIG_400i", "name": "Origin 400i"},
    {"className": "ORIG_400i_AI_Template_ShipBoarded", "name": "Origin 400i"},
    {"className": "ORIG_400i_PU_AI_CIV", "name": "Origin 400i"},
    {"className": "ORIG_400i_PU_AI_CIV_DEF", "name": "Origin 400i"},
    {"className": "ORIG_400i_PU_AI_CRIM", "name": "Origin 400i"},
    {"className": "ORIG_400i_Unmanned_Salvage", "name": "Origin 400i"}
  ]
}
```

## Derived, estimated and curated values

`vehicles`, `byVehicle` and `summary` are derived. No `Estimate` fields; no curated sources. `_assumptions` states:

1. **The rule** (`RULE`, paints.rs:48). A paint fits when the vehicle has a paint port and (1) the paint's `AttachDef.Type` is a type the port lists, with its `SubType` among that type's subtypes when the port names any; (2) the paint's `Size` lies within the port's `minSize`/`maxSize` when given; (3) every word of the port's required tags (XML `requiredTags`, entity `RequiredPortTags`) is in the paint's `AttachDef.Tags`; (4) every word of the paint's `AttachDef.RequiredTags` is in the port's own tags (XML `portTags`, entity `PortTags`) or the vehicle's `AttachDef.Tags`. Case-insensitive; `@` words are kept as written.
2. **Where the port is read from**, and how modifications, patch files and `skipPart` are applied. That is how the files read; the engine code that applies them is not in the data.
3. **Case.** Comparing without regard to case changes one result: the Argo MOTH's port asks for `Paint_MOTH` while its 11 paints carry `Paint_Moth`. Compared exactly, they would fit nothing.
4. **Manufacturer.** Code only when the logo's name key belongs to one manufacturer; a name shared by several codes (Aegis Dynamics: `AEGS`, `FSKI`, ...) gives `manufacturerName` alone.

What the rule rests on, from the records:

- **(3)** is what separates hulls: `Paint_400i` paints carry `Paint_400i`, which the 400i port requires.
- **(4)** narrows a family. Cutlass Black's port offers `Paint_Cutlass Paint_Cutlass_Black` and requires `Paint_Cutlass`, so a paint that requires `Paint_Cutlass_Black` fits the Black and not the Blue or Red. `Paint_Hornet_F7A_Mk2_Desert_Camo` requires `ANVL_Hornet_F7A_Mk2`, which no port offers but the F7A Mk II's own `AttachDef.Tags` does, so it fits the F7A Mk II and its PYAM Exec variants and no other Mk II. Without the vehicle tags in (4) it would fit nothing.
- **Templates** are skipped as the other item domains do: a placeholder name (`item_identity` `is_template`, as game-armor drops them) or `template` in the record name (as game-consumables). 125 on LIVE.

## Redaction

Nothing to strip: no prose is emitted (`descriptionKey` only), so the file is not in `PROSE_FIELDS` (`crates/sc-gamedata/src/localization.rs`, redaction module).

## Consumers

- **RSI+** (`~/git/rsi-plus`, browser extension): the intended reader. It guesses a store paint's hull from the paint title today; `paints[].name` / `vehicles[]` or `byVehicle` replace the guess. Not wired yet.
- No other tool reads it. Not yet published, so no `build.json` entry.

## Derivable data

### Fields the records carry that the dataset does not read

- **Port flags.** Ports carry `flags` (`$editable`, `uneditable`, `invisible`). The Mustang Omega's modification sets its paint port `uneditable`. Whether a player can change the paint is not in the output.
- **Paint geometry tags.** The vehicle's `SGeometryResourceParams.Geometry.SubGeometry[].Tags` list one entry per paint (`Paint_400i_Black_Gold` on the 400i), matching the paint's `@` tag. That is a second, asset-side listing of which paints a hull renders. Not read, by the no-assets rule.
- **Default paint.** `SEntityComponentDefaultLoadoutParams` names the stock paint in `hardpoint_paint` on some vehicles (empty on the 400i).

### Parse gaps

- **17 paints fit no vehicle.** 16 ATLS paints (`Paint_ATLS`): the ATLS is not a vehicle record under `entities/spaceships` or `entities/groundvehicles`, and its port was not found in scope. `Paint_Mustang_Omega`: requires `Paint_Omega`, which the Omega's port (`portTags` `Paint_Mustang`, required `Paint_Omega` after its modification) and vehicle tags do not offer. It would fit if the port's required tags also counted as offered tags; no record says they do.
- **9 vehicles with a port and no paint**: the six Ballista records (their port requires `Ballista_Paint`, `Ballista_Snowblind` or `Ballista_Dunestalker`; the only Ballista paint, `Paint_Ballista_Base`, is a template), `CNOU_Mustang_Omega` (above) and the two `DRAK_Command_Module` records (their port requires `Paint_Command_Module`, which no paint carries).
- **44 vehicles without a paint port**: 17 Vanduul records (Blade, Glaive, Hunter, Mauler, Scythe), 7 orbital sentries, 4 comms probes, 4 Javelins, 3 85X, 3 Bengal AI records, 2 `EAObjectiveDestructable_*` and 4 templates (`AEGS_Gladius_Template`, `DRAK_Golem_Template`, `MISC_Starlite_Template`, `Spaceship_Template`). Their definitions (or entities) have no Paints port.
- **Local game-ships.json is stale.** `data/game-ships.json` in this checkout has 1158 vehicles; the 54 extra are stale 2026-11-01 files from `extracted-data/` (see [game-ships](game-ships.md)). A p4k run of game-ships has 1104, the same records this dataset names.
- **Missing files**: `VNCL_Hunter_AI_VAN` names a definition the archive lacks; the three `MISC_Starfarer_Wreck*` modifications name a patch file that is not there (base definition used). `ANVL_Hornet` lists `Modifications/ANVL_Hornet_F7A` and `ANVL_Hornet_F7X` patch files that do not exist either, but no vehicle record picks those modifications through `ANVL_Hornet.xml`.

Related specs: [consumer-datasets](../plans/specs/consumer-datasets.md), [dump-enhancements](../plans/specs/dump-enhancements.md).
