---
title: game-manufacturers (game-manufacturers.json)
tags: [docs, datasets]
updated: 2026-11-25
---

# game-manufacturers (game-manufacturers.json)

The in-fiction companies behind ships, components and gear: one row per real `SCItemManufacturer` record, with its code, localised name and string-table keys, plus a code → name map and a list of names that several codes share. On LIVE 4.10.201.48085 it holds 131 manufacturers (14 more records were dropped as unnamed or placeholder). The extract is 60,788 bytes minified; with `description` stripped as publish does, about 29,890 bytes.

## Source

- DataCore: `SCItemManufacturer` records under `**/scitemmanufacturer/**` (filter, `crates/sc-gamedata/src/domains/manufacturers.rs:107`). Records in the subfolders `paintcolorlogos`, `shop` and `uistyleonly` are skipped (`is_manufacturer_record`, manufacturers.rs:73); every other record is kept: on LIVE 89 at the top level, 26 in `clothes/`, 13 in `armor/`, 9 in `personalweapons/`, 4 in `cockpitflairlogos/` and 4 in `hangars/` (145 in all).
- String table for `Localization.Name` and `Localization.Description`.
- Code: `parse_manufacturer` (manufacturers.rs:53), `looks_real` (manufacturers.rs:87), `Domain::parse` (manufacturers.rs:115).
- Derives from no other dataset.

## Shape

Top-level keys: `_source`, `_extracted`, `_build`, `manufacturers[]`, `byCode{}`, `sharedNames[]`, `summary{}`. `_schema: "game-manufacturers@1"` follows `_build` (since tags 4.10.201.48085 / 5.0.202.35026).

- `byCode`: `{ <code>: <name> }`, 131 entries.
- `sharedNames[]`: `{name, codes[]}` for names used by more than one code; 15 on LIVE (`Aegis Dynamics`: `AEGS`, `FSKI`, `MXOX`, `PRAR`, `TRAS`; `Ace Astrogation`: `ACAS`, `Paint_Test_01`, `Paint_Test_02`; `ArcCorp`: `ARCC`, `GHEX` …). They are reported, not fixed (manufacturers.rs:140).
- `summary`: `manufacturers` 131, `unnamedOrPlaceholder` 14, `namesUsedByMultipleCodes` 15.

### `manufacturers[]` (sorted by name)

| Field | Type | Meaning | Units/range | Source |
|---|---|---|---|---|
| `code` | string | Record short name, used as the manufacturer code (`AEGS`) | 131 | record name, not the record's `Code` field (see Parse gaps) |
| `recordName` | string | `SCItemManufacturer.<code>` | 131 | record |
| `id` | string | Record GUID | 131 | `_RecordId_` |
| `name` | string | Localised name | 131 | `Localization.Name` |
| `description` | string? | Localised description (stripped on publish) | 128 | `Localization.Description` |
| `nameKey`, `descriptionKey` | string? | String-table keys (`manufacturer_NameAEGS`) | 131 / 128 | same refs |
| `nameStatus` | string? | Absent on all 131 (every kept name is real) | | `loc_refs` |

Example (real):

```json
{"code":"987","recordName":"SCItemManufacturer.987","id":"3b54a8cc-5700-45f1-940a-2436b213926f","name":"987",
 "description":"Known for their more aggressive urban styling. Asymmetrical. Bold graphic prints.",
 "nameKey":"manufacturer_Name987","descriptionKey":"manufacturer_Desc987"}
```

## Derived, estimated and curated values

- Which records count as manufacturers is a path rule (`NON_MANUFACTURER_DIRS`), and which names count as real is a text rule (`looks_real`: not empty, not `@…`, no `placeholder`, no `<=`, not `unknown`).
- `byCode` and `sharedNames` are built from the kept rows.
- No estimates and no curated sources.

## Redaction

`description` is removed everywhere and listed in `_redacted: ["description"]` (`PROSE_FIELDS`, `crates/sc-gamedata/src/localization.rs:445`). `descriptionKey` stays. 128 of the 131 `nameKey`s survive the published string table (`manufacturer_Name…` matches the name-key rule).

## Consumers

- **starorg-codex** (`feat/dataset-reuse`) `crates/starorg-mcp/src/prebuilt.rs:168`: needs `manufacturers`, `byCode`, `summary`; accepts `description` stripped.
- **SCMCP** `src/versions.ts:43`: diffs `manufacturers` between builds; `keyOf` (`:340`) picks `recordName`.
- snares, MCsim, Bindscape and sc-telemetry do not read it.
- Contracts: `_build` equals the tag; `_schema` stem `game-manufacturers` once added ([consumer-datasets](../plans/specs/consumer-datasets.md)).

## Derivable data

Fields the records carry that the domain does not read (145 records outside the skipped folders in `~/git/sc-gamedata/extracted-data/libs/foundry/records/scitemmanufacturer`):

- `Code`: the record's own code field. It differs from the short name on 38 records: `AEGS` → `AEG`, `MISC` → `MIS`, `JOKR` → `JOK`, `GATS` → `GAT`, `MRAI` and `MRAI_2025` → `MIS`, and it is empty on 15 (`UCAG_HI`, `SELA_LO`, `PLACEHOLDER`, `Paint_Test_02` …). Which one item records reference is not established here; [game-ships](game-ships.md) and [game-item-tags](game-item-tags.md) both use the short-name form.
- `Logo` (a path on 133 of 145, e.g. `UI/SharedAssets/ManufacturerLogos/AceAstrogation_256.tif`), `LogoFullColor`, `LogoSimplifiedWhite`.
- `Localization.ShortName` and `Localization.displayFeatures.History`/`Callout1-3`: `@LOC_EMPTY` on all 145, so nothing to gain in this build.
- The `paintcolorlogos` records (1028 files in this extract; "900+ paint records carrying their maker's name key" per manufacturers.rs:7): a paint → manufacturer link.

Values computable by combining datasets:

- Products per manufacturer: [game-ships](game-ships.md) `vehicles[].manufacturer` is the lower-cased code (23 codes in `byManufacturer`; every one is in `byCode` case-insensitively), and [game-item-tags](game-item-tags.md) `items[].manufacturer` uses 120 codes (CDS and RSI 569 items each, KSAR 450, AEGS 322). Counts per code would give each manufacturer a product line. Consumer: SCMCP, starorg-codex.
- Name for every maker the [game-search-index](game-search-index.md) lists: 93 of its 155 makers are codes in `byCode`, and the other 62 are tag-derived names (`AlejoBrothers`, `MICROTECH`, `Shubin`).
- Canonical code per shared name (for example `AEGS` for Aegis Dynamics). Estimate unless the records say which code is primary; `Code` above may decide some.

Parse gaps:

- 14 item manufacturer codes are not in `byCode` (`CRLF_MEDGUN`, `FTA`, `GRIMHEX`, `GRIN_WEAPONS`, `LBCO`, `MICROTECH`, `NYX_A`, `PLACEHOLDER` …): records that were dropped as unnamed or are absent.
- Test and variant records pass `looks_real` and appear as manufacturers: `Paint_Test_01`, `Paint_Test_02`, `GAMA_2026`, `MRAI_2025`, `RSI_2025`, and `ASD_LO` named "PH Associated Science and Development" (a `PH` placeholder prefix that `looks_real` does not test, unlike `is_placeholder_name` in localization.rs:232).
- `nameStatus` is never set, because placeholder rows are dropped before output; the 14 dropped rows are only a count.

Related specs: [dump-enhancements](../plans/specs/dump-enhancements.md) (items 1, 2), [dump-reuse](../plans/specs/dump-reuse.md).
