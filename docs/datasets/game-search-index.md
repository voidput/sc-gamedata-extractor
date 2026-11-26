---
title: game-search-index (game-search-index.json)
tags: [docs, datasets]
updated: 2026-11-25
---

# game-search-index (game-search-index.json)

The fuzzy-find entries, precomputed: one compact row per item and per ship (`[key, name, maker, type, family, noise]`), with makers and types interned into lists. It is derived from two other datasets, not from game files, and lets a client search without loading the full item and ship datasets. On LIVE 4.10.201.48085 it holds 12,894 item rows and 1,104 ship rows, with 155 makers and 134 types. The file is 869,375 bytes minified (written one row per line).

## Source

- Derived from [game-item-tags](game-item-tags.md) (`items[]`) and [game-ships](game-ships.md) (`vehicles[]`). No game records are read.
- Code: `crates/sc-gamedata/src/search_index.rs`: `build_entries` (:130), `build_search_index` (:221), `serialize` (:354), `write_search_index` (:398). The CLI calls `write_search_index` after a parse (`crates/sc-gamedata/src/bin/sc-gamedata.rs:1888`). It is rebuilt whenever both inputs are on disk and removed when either is missing, so a stale index never sits beside new data (search_index.rs:6).
- `diff.rs` lists it in `DERIVED_FILES`: never snapshotted or diffed unless named.

## Shape

Top-level keys, in order: `_source`, `_build`, `format`, `columns`, `makers`, `types`, `items`, `ships`. There is no `_extracted`. `_build` is taken from the inputs and left out when neither has one. `_schema: "game-search-index@1"` follows `_build` (since tags 4.10.201.48085 / 5.0.202.35026).

| Field | Type | Meaning | LIVE |
|---|---|---|---|
| `_source` | string | `"Derived from game-item-tags.json and game-ships.json (see src/search-index.ts)"` | |
| `format` | number | Row format; readers refuse any other value (`load_search_index_entries`, search_index.rs:303) | 1 |
| `columns` | string[] | `["key","name","maker","type","family","noise"]` | |
| `makers` | (string\|null)[] | Interned maker values; rows use 1-based indices, 0 = none | 155 |
| `types` | (string\|null)[] | Interned item types and ship roles; 1-based, 0 = none | 134 |
| `items` | row[] | One per `game-item-tags` item | 12,894 |
| `ships` | row[] | One per `game-ships` vehicle | 1,104 |

Row columns (trailing zeros trimmed, so rows can be shorter than six):

| # | Column | Items | Ships |
|---|---|---|---|
| 0 | `key` | `className` | `recordName` without `EntityClassDefinition.` |
| 1 | `name` | `name`; 0 when equal to `key`; for a template item with a weapon family, `"<family> (unnamed: <className>)"` (38 rows) | `name` |
| 2 | `maker` | Index into `makers`: the `Manufacturer/<X>` tag if present, else `manufacturer`; 0 on 5,918 items | Index of the upper-cased `manufacturer` |
| 3 | `type` | Index into `types`: the item `type` | Index of `role`, else `career` |
| 4 | `family` | Leaf of the deepest `Weapon/FPS/{Stocked,Pistol,Shouldered,Melee}/…` tag (`Verdict`, `Scourge`); 0 when none. 522 items, 74 families | always 0 |
| 5 | `noise` | 1 when the default list drops the entry: an item with no `type` or a type in `NOISE_TYPES` (`Cargo`, `Door`, `Seat`, `Usable`, `UNDEFINED`, `Light`, `Room`, `Display`, `Elevator`); 4,616 items | 1 for template ships; 5 ships |

Example (real rows; `makers` is cut to its first 5 of 155 and `types` is left out, so the indices 11, 19 and 76 point past what is shown: `makers[10]` is `ApocalypseArms`, `types[18]` is `WeaponPersonal`, `types[75]` is `Interceptor`):

```json
{"format":1,"columns":["key","name","maker","type","family","noise"],
 "makers":["987","AEGS","FFSY","MISC","AlejoBrothers"],
 "items":[
   ["3_seat_bench_constellation","Seat",0,1,0,1],
   ["987_hat_01_01_01","Bantam Hat Orange",1,2],
   ["apar_melee_01","Demon Fang Combat Knife",11,19,"DemonFang"] ],
 "ships":[
   ["AEGS_Avenger_Stalker","Aegis Avenger Stalker",2,76] ]}
```

## Derived, estimated and curated values

The whole file is derived. The curated parts are code rules:

- `NOISE_TYPES` (search_index.rs:35), a fixed list of world-furniture item types.
- The weapon-family rule (`weapon_family`, search_index.rs:108: depth 5 under `Stocked`, depth 4 otherwise).
- The `(unnamed: …)` names for template items.

No `Estimate` fields and no `_assumptions`.

## Redaction

Nothing (no `PROSE_FIELDS` entry). Publish takes every `game-*.json` in `data/` (`crates/sc-gamedata/src/publish.rs:146`), so the index is published as is.

## Consumers

- **SCMCP** `src/itemfinder.ts:37` / `:75`: `sc_find` loads the index through the vendored `loadSearchIndexEntries` (`src/vendor/sc-gamedata/search-index.ts:114`, which throws on a `format` other than 1) when the data source has it, and otherwise builds entries from the datasets.
- **sc-gamedata**: `crates/sc-gamedata/src/mcp.rs:106` builds the index in memory with the same code when the file is absent (and reads the file at `:126` when present); `finder.rs` reuses `ship_key`.
- **starorg-codex** (`feat/dataset-reuse`): never takes it from a dump ("the derived search index is always extracted locally", `crates/starorg-mcp/src/prebuilt.rs:150`).
- Contracts: `format` stays 1 unless readers are updated (SCMCP refuses other formats); `_build` equals the tag. Note that the `_schema` check (stem before `@` equals the file stem) applies here too.

## Derivable data

Everything here comes from other datasets, so "more" means more columns or rows from datasets the build already has:

- **Maker names**: 93 of the 155 `makers` are manufacturer codes (`AEGS`, `ANVL`) found in [game-manufacturers](game-manufacturers.md) `byCode`; the other 62 are tag-derived names (`AlejoBrothers`, `ApocalypseArms`, `MICROTECH`, `Shubin`). A `makerNames` list (or normalising tag names to codes) would let a search for "Aegis" hit `AEGS` items. Mapping tag names to codes is an estimate where no record links them. Consumer: SCMCP `sc_find`.
- **Name keys**: `game-item-tags` items carry `nameKey`. A `nameKey` column would let a client show names in the player's own language from their `global.ini` (the approach [dump-reuse](../plans/specs/dump-reuse.md) decision 2, option b, takes).
- **More entry kinds**: commodities ([game-commodities](game-commodities.md)) and places ([game-starmap](game-starmap.md), 958 visible locations, 871 of them with a real `displayName`) could be indexed the same way, so one search covers items, ships, commodities and places.
- **Component class/size/grade** from [game-ship-components](game-ship-components.md) as extra columns for ship parts. Consumer: SCMCP.

Parse gaps:

- `_source` still names `src/search-index.ts`, the deleted TypeScript; the producer is `crates/sc-gamedata/src/search_index.rs`.
- No `_extracted`, unlike every other dataset.
- 5,918 item rows have no maker (0): `game-item-tags` has no `manufacturer` and no `Manufacturer/` tag for them.
- `makers` mixes two vocabularies (codes and tag names) for the same companies, for example `MICROTECH` and `Shubin`, which are not `byCode` codes, beside codes such as `AEGS`.

Related specs: [rust-pipeline](../plans/specs/rust-pipeline.md), [dump-reuse](../plans/specs/dump-reuse.md), [dump-enhancements](../plans/specs/dump-enhancements.md).
