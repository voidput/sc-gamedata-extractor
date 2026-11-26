---
title: game-starmap (game-starmap.json)
tags: [docs, datasets]
updated: 2026-11-25
---

# game-starmap (game-starmap.json)

Every `StarMapObject` record (star systems, stars, planets, moons, stations, outposts, points of interest, asteroids), with its parent chain, type, jurisdiction, amenities, quantum-travel geometry and, where the solar-system object containers place it, a position in metres. On LIVE 4.10.201.48085 it holds 2132 locations (958 visible, 1174 flagged `hiddenInStarmap`). The extract is 1,369,766 bytes minified; with `description` stripped as publish does, about 1,158,534 bytes.

## Source

- DataCore records, selected by the filters in `crates/sc-gamedata/src/domains/starmap.rs:451`:
  - `StarMapObject` under `**/starmap/**` and `**/starmapobject/**` (one per location).
  - `StarMapObjectTypes.types[]` (`_RecordId_` → `name`) for `type` (`build_type_names`, starmap.rs:205).
  - `StarMapAmenityTypes.amenityTypes[]` for `amenities` (starmap.rs:210).
  - `Jurisdiction` records under `**/lawsystem/jurisdictions/**`, so `jurisdiction` names match [game-law](game-law.md).
  - `EntityClassDefinition.MonitoredZone` and `MonitorZoneJammer` for zone reach (`entity_radius`, starmap.rs:129). The module comment (starmap.rs:15) says the `**/entities/monitoredzone` filter never selects anything, so `monitoredRadius` is always the 50,000 km default (`DEFAULT_MONITORED_KM`, starmap.rs:124).
- Archive files: `**/ObjectContainers/PU/system/*/*system.socpak` and the containers they reference (`Domain::files`, starmap.rs:464), read by `crates/sc-gamedata/src/domains/socpak.rs` (`Containers::extract` :661, `load_placements` :808, `load_zones` :853, `load_spins` :875). DataCore places nothing in space; positions, rotation and zone radii come only from these containers, joined to the record by id.
- Code: `parse_location` (starmap.rs:222), `build_locations` (starmap.rs:294), `Domain::parse` (starmap.rs:473).
- Derives from no other dataset. `build_locations` is shared with [game-mining-spawns](game-mining-spawns.md), which calls it without containers.

## Shape

Top-level keys: `_source`, `_extracted`, `_build`, `locations[]`, `byType{}`, `summary{}`. `_schema: "game-starmap@1"` after `_build` (since tags 4.10.201.48085 / 5.0.202.35026).

`summary` (LIVE): `locations` 2132, `visible` 958, `hidden` 1174, `positioned` 459, `turning` 548, `rotating` 27, `armistice` 26, `monitored` 101, `jammed` 1, `typeNamesResolved` 21.

`byType`: count of **visible** locations per `type`, largest first (Outpost 459, Asteroid 187, Manmade 83, Asteroid_ValidQT 70, PointOfInterest 68, Manmade_VisibleOnInteraction 42, Moon 19, Planet 14, LandingZone 4, Anomaly 4, Star 3, NavPoint 3, QuantumTracePoint 1, YouAreHere 1).

### `locations[]`

| Field | Type | Meaning | Units/range | Source |
|---|---|---|---|---|
| `name` | string | Display name, or the record short name when the name is a placeholder | 2132/2132 | `name` via string table, else record short name |
| `recordName` | string | `StarMapObject.<short>` | 2132 | record |
| `id` | string | Record GUID; join key to the object containers | 2132 | `_RecordId_` |
| `displayName` | string? | Localised name when it is real | 1980 | `name` |
| `description` | string? | Localised description (stripped on publish) | 1901 | `description` |
| `nameKey`, `descriptionKey` | string? | String-table keys | 2072 / 1939 | `name` / `description` refs |
| `nameStatus` | string? | `placeholder`, `unresolved` or `empty`; absent when the name is real | empty 69, unresolved 66, placeholder 17 | `loc_refs` (localization.rs:357) |
| `parent` | string? | `recordName` of the containing object | 2038 | `parent` (`file://` path) |
| `type` | string | Object type name; the raw UUID when the lookup misses | 2132 | `type` → `StarMapObjectTypes` |
| `affiliation` | string? | Short name of the referenced record (lower case: `citizensforpyro`, `headhunters`, `outcasts`, `privatesecurity`, `xenothreat`) | 53 | `affiliation`; meaning not established beyond the reference |
| `jurisdiction` | string? | Short name of the `Jurisdiction` record, same as [game-law](game-law.md) `jurisdictions[].name` | 101; 13 distinct | `jurisdiction` |
| `size` | number | Read as is; the Rust source does not say what it measures | 0.1 to 696,000,000 | `size`. Meaning not established in code; snares reads it as a planet/moon radius (see Consumers) |
| `isScannable` | bool | Copied | 2132 | `isScannable` |
| `hiddenInStarmap` | true? | Present only when `hideInStarmap` is true | 1174 | `hideInStarmap` |
| `amenities` | string[]? | Amenity names | 281; 22 distinct (e.g. Vehicle Services 348, Commodity Trading - Freight Elevator 225, Hangar L 204) | `amenities[]` → `StarMapAmenityTypes` |
| `quantum` | object? | Quantum-travel geometry, metres from the centre (struct doc, starmap.rs:91) | 1638 | `quantumTravelData` |
| `quantum.obstructionRadius` | number? | A route passing closer is blocked (starmap.rs:44) | m | same |
| `quantum.arrivalRadius` | number? | Drop-out distance from the centre (starmap.rs:47) | m | same |
| `quantum.adoptionRadius` | number? | Within it, the object's children become targets (starmap.rs:50) | m | same |
| `quantum.arrivalPointDetectionOffset` | number? | Meaning not established | m (per struct doc) | same |
| `quantum.subPointRadiusMultiplier` | number? | Meaning not established | unitless; 1.5 on 1634, else 1.512, 7.5 or 25 | same |
| `rotationHours` | number? | Hours per turn, planets and moons only | 27; float32 noise (`3.1099999`) | container `planetRotationSpeed` (socpak.rs `BodySpin`) |
| `gridRadius` | number? | Radius of the body's rotating zone | m; 27 | container `gridRadius` |
| `local` | {x,y,z}? | Offset from the body centre for places inside the body's grid radius | m, rounded to mm; 548 | container position minus parent position |
| `turnsWith` | string? | With `local`: `recordName` of that body | 548 | container parent |
| `position` | {x,y,z}? | Absolute position, system frame, star at origin | m, rounded to mm; 459 | container position. Left out for places inside their body's obstruction radius |
| `armisticeRadius` | number? | Armistice zone radius | m; 26 | container zones |
| `monitoredRadius` | number? | Monitored zone reach | always 50,000,000 m (default, see Source); 101 | container has a MonitoredZone |
| `jammerRadius` | number? | Comms-jammer reach | m; 1 | container jammer, else `MonitorZoneJammer` entity, else 40 km default |
| `path` | string[]? | Names from the root down to the parent | 2038 | walk of `parent` |
| `system` | string? | First entry of `path` | Stanton 960, Pyro 584, Nyx 494, none 94 | walk of `parent` |

Example (trimmed, real):

```json
{"name":"Port Tressler","recordName":"StarMapObject.RR_MIC_LEO","id":"233238ee-adeb-4405-8045-49fa07370f37",
 "displayName":"Port Tressler","nameKey":"Stanton4_Transfer","descriptionKey":"Stanton4_Transfer_Desc",
 "parent":"StarMapObject.Stanton4","type":"Manmade","size":1200,"isScannable":true,
 "amenities":["Hangar L","Landing Pad M","Vehicle Services","Docking","Clinic"],
 "quantum":{"obstructionRadius":8000,"arrivalRadius":24000,"adoptionRadius":25000,"arrivalPointDetectionOffset":10000,"subPointRadiusMultiplier":1.5},
 "local":{"x":-561471.046,"y":-545853.017,"z":808831.763},"turnsWith":"StarMapObject.Stanton4",
 "position":{"x":22461523781.439,"y":37185199111.813,"z":808831.763},
 "armisticeRadius":12000,"monitoredRadius":50000000,"path":["Stanton","microTech"],"system":"Stanton"}
```

## Derived, estimated and curated values

- `monitoredRadius` is a constant default (50,000 km), not read per zone: the entity record is never in scope (starmap.rs:15). `jammerRadius` falls back to a 40 km default (`DEFAULT_JAMMER_KM`, starmap.rs:126) when neither the container nor the entity gives one.
- `local`/`turnsWith` versus `position` is a rule of the domain, not a record field: a place is "turning" when its distance from the parent container is under the parent's `gridRadius`, and it loses `position` when that distance is under the parent's `obstructionRadius` (starmap.rs:346-371).
- `path` and `system` come from walking `parent` with a cycle guard (starmap.rs:400).
- `byType` counts visible locations only.
- No `Estimate` fields, no `_assumptions`, no curated sources.

## Redaction

`description` is removed everywhere in the file and listed in `_redacted: ["description"]` (`PROSE_FIELDS`, localization.rs:445). `descriptionKey` stays. The published string table keeps only name-shaped keys (see [game-strings-english](game-strings-english.md)); only 374 of the 2072 starmap `nameKey`s survive it (most are bare record codes like `Stanton2b`), so the dataset's own `name`/`displayName` is the published name.

## Consumers

- **snares** `scripts/build-data.mjs`: reads `locations[]` (`:33`), refuses a file with no `position` (`:52`), and uses `type`/`name` (`kindOf`, `:40`), `system`, `parent`, `position`, `quantum.arrivalRadius`/`obstructionRadius`/`adoptionRadius` (`:77`), `size` as a planet/moon radius (`:80`), `hiddenInStarmap` (`:82`), `amenities` (`Refinery`, `Hospital`, `Hangar XL`, `Commodity Trading - Loading Dock`, `:84-88`), `armisticeRadius`, `monitoredRadius`, `jammerRadius`, `local`/`turnsWith` (`:60`), `rotationHours`, `gridRadius` (`:94`), and `_build`/`_extracted` (`:209`, `:230`).
- **SCMCP** `src/versions.ts:42`: diffs `locations` between builds, keyed by `recordName` (`keyOf`, `:340`).
- **starorg-codex** (branch `feat/dataset-reuse`) `crates/starorg-mcp/src/prebuilt.rs:184`: needs `locations`, `byType`, `summary`; accepts `description` stripped; takes dumps only from 2026-11-26 on (positions for plain containers at system level, sc-gamedata `350e7053b`).
- Contracts that must not break ([consumer-datasets](../plans/specs/consumer-datasets.md)): keep `_build` (snares reads `starmap._build`); `_build` equals the tag and, once added, `_schema` starts with `game-starmap` (sc-telemetry hub reader rules).

## Derivable data

Fields the records carry that the domain does not read (counts over the 2132 `StarMapObject` records in `~/git/sc-gamedata/extracted-data/libs/foundry/records/starmap`):

- `respawnLocationType`: `None` 1780, `Hospital` 266, `Other` 82, and one each of `Prison`, `PrisonExit`, `CriminalLocation`, `CriminalHospital`. Gives a "where can I respawn" list joined with `amenities`.
- `navIcon` (`Default` 1361, `Outpost` 624, `Station` 105, `Moon` 19, `Planet` 14, `LandingZone` 6, `Star` 3): a second classification snares could use instead of its name regexes in `kindOf`.
- `blockTravel` (true on 7), `hideInWorld` (true on 1052), `onlyShowWhenParentSelected`, `hideWhenInAdoptionRadius`: visibility and travel flags beside `hiddenInStarmap`.
- `locationParams.exposeForPlayerCreatedMissions` (true on 347).
- `locationHierarchyTag` (a `Tag` reference on 599 records, e.g. `Tag.65ae3316-…` on 54): resolvable through the tag database to group places the way missions select them.
- `radarProperties` (an object on 106), `asteroidRings` (2 records with one ring), `overrideRotationSpeed` (true on 15, with `overrideRotationSpeedValue`), `callout1-3` (real text on 6).

Values computable by combining datasets:

- Effective jurisdiction for every place by walking `parent` until a location with `jurisdiction` is found (only 101 carry one). Estimate: the records do not state that a jurisdiction is inherited down the hierarchy. Consumer: starorg-codex, SCMCP.
- Jurisdiction → crimes/fines/prohibited goods by joining `jurisdiction` to [game-law](game-law.md) `jurisdictions[].name` (all 13 names match).
- Distances between positioned locations (exact for the 459 with `position`); surface places only through their body (`turnsWith`), since `local` turns with it. Consumer: snares route weights.
- Missions offered per place: [game-missions](game-missions.md) records carry `locationMissionAvailable` (unread there, see that note), which points at starmap records (Stanton star 537, Hurston 361, microTech 338, Crusader 265, ArcCorp 209).
- Refinery locations joined with [game-refining](game-refining.md) via the `Refinery` amenity (31 places).

Parse gaps:

- 94 locations have no `parent`, so no `path`/`system` (36 Outpost, 32 Manmade, 4 SolarSystem, 3 Star, and templates such as `StarMapObject.ASD_TEMPLATE`).
- Of the 958 visible locations, 271 have neither `position` nor `local` (88 Outpost, 68 PointOfInterest, 40 Manmade, 31 Asteroid…): no container placed them.
- Nyx I, II and III have `size` 1,000,000, no `quantum`, no `rotationHours`; meaning not established (they may be unfinished records).
- `rotationHours` carries float32 noise (`3.1099999`, `5.0300002`); positions are rounded to the millimetre but rotation is not.
- `monitoredRadius` is a default for every monitored zone (see Derived values).
- `type` falls back to the raw UUID when `StarMapObjectTypes` misses; `typeNamesResolved` is 21, and `CardinalPoint`, `SolarSystem`, `Outpost_InvalidQT` appear only on hidden locations.

Related specs: [dump-enhancements](../plans/specs/dump-enhancements.md) (items 1, 2, 4b), [dump-reuse](../plans/specs/dump-reuse.md), [tool-data-sync](../plans/specs/tool-data-sync.md).
