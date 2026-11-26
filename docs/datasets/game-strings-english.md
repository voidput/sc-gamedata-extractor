---
title: game-strings-english (game-strings-english.json)
tags: [docs, datasets]
updated: 2026-11-25
---

# game-strings-english (game-strings-english.json)

The English string table (`global.ini`) as a dataset: `key → text` for every entry, the table the other domains resolve `@key` references against. On LIVE 4.10.201.48085 the extract holds 91,358 strings and is 11,024,246 bytes minified. Publishing cuts it to name-shaped keys only: re-applying the rule to the LIVE extract keeps about 15,227 strings, about 851 KB minified (`publish-dry` gives the exact figure).

## Source

- Archive file `Data/Localization/english/global.ini` (`key=value` lines; `;`, `#` and `[` lines skipped, BOM stripped), parsed by `parse_global_ini` (`crates/sc-gamedata/src/localization.rs:88`).
- Not a record domain: `crates/sc-gamedata/src/strings.rs` (`dataset`, strings.rs:13) builds it, and the harness writes it by name (`write_strings`, `crates/sc-gamedata/src/bin/sc-gamedata.rs:1565`).
- Derives from no other dataset; every localised domain reads the same table.

## Shape

Top-level keys, in order: `_source` (`"Star Citizen game files via StarBreaker (Data/Localization/english/global.ini)"`), `_extracted`, `_build`, `language` (`"english"`), `strings{}`, `summary{}`. Published files add `_redacted` (last) and `summary.published`. `_schema: "game-strings-english@1"` follows `_build` (since tags 4.10.201.48085 / 5.0.202.35026).

| Field | Type | Meaning | Count/range | Source |
|---|---|---|---|---|
| `language` | string | `english` | | constant |
| `strings` | object | `{ <key>: <text> }`, keys as the file spells them | 91,358 in the extract | `global.ini` |
| `summary.strings` | number | Entries in the full table | 91,358 | count |
| `summary.published` | number | Entries kept on publish (published files only) | about 15,227 | count |
| `_redacted` | string | `"Localisation limited to name keys; descriptive and dialogue text is not published."` (published files only) | | `redaction::NOTE` |

Key families on LIVE (first `_` part): `PU` 19,981, `item` 16,875, `Human` 9,190, `ui` 4,269, `Dlg` 3,124, `DXSM` 2,354, `mission` 2,225, `PH` 1,592, `mg` 1,389, `vehicle` 1,326. 13,612 keys carry a `,P` suffix (`ui_CIMiningMode,P`), which lookups tolerate. 813 values are empty and 95 contain `placeholder` in any case (21 in capitals).

Example (trimmed, real keys that publishing keeps):

```json
{"language":"english",
 "strings":{"manufacturer_NameAEGS":"Aegis Dynamics","manufacturer_Name987":"987"},
 "summary":{"strings":91358,"published":15227},
 "_redacted":"Localisation limited to name keys; descriptive and dialogue text is not published."}
```

## Derived, estimated and curated values

None: the values are the file's text. The published key set is a rule (below), not a judgement per key.

## Redaction

`redact_strings` (`crates/sc-gamedata/src/localization.rs:571`) keeps an entry only when all of these hold (`keeps`, localization.rs:426):

- the value is non-empty and at most 120 UTF-16 units (`MAX_VALUE_LENGTH`);
- the key does not contain `desc`, `tooltip`, `hint` or `subtitle` (any case) and does not start with `Dlg_`, `PU_`, `DXS` or `DXM` (any case);
- the key has `Name`, `Names`, `Title`, `Titles`, `DisplayName`, `ShortName` or `Nickname` at the start or after `_`, followed by an upper-case letter, digit, `_` or the end (case-sensitive).

Effect on other datasets' keys (LIVE): manufacturers keep 128 of 131 `nameKey`s; starmap 374 of 2072 `nameKey`s; missions 190 of 2502 `titleKey`s and 97 of 2446 `giverKey`s; keybinds 0 of 754 `labelKey`s; attributes 0 of 78 `labelKey`s. Those datasets therefore carry their resolved names themselves.

## Consumers

- **SCMCP** `src/versions.ts:45`: diffs `strings` between builds.
- **starorg-codex** (`feat/dataset-reuse`) `crates/starorg-mcp/src/prebuilt.rs:150`: never takes it from a dump ("published with name keys only"); the app reads the player's own `global.ini` instead ([dump-reuse](../plans/specs/dump-reuse.md) decision 2, option b).
- **sc-gamedata** itself reads the table from the archive, not from this file; `diff.rs` loads snapshots of it to diff strings between builds.
- Contracts: `_build` equals the tag; `_schema` stem `game-strings-english` once added ([consumer-datasets](../plans/specs/consumer-datasets.md)).

## Derivable data

- **Other languages**: the CLI knows 12 string-table languages (`LANGUAGES`, `crates/sc-gamedata/src/bin/sc-gamedata.rs:1241`: english, german_(germany), french_(france), italian_(italy), spanish_(latin_america), spanish_(spain), portuguese_(brazil), polish_(poland), korean_(south_korea), japanese_(japan), chinese_(simplified), chinese_(traditional)), and `strings --language` writes `game-strings-<language>.json` in the same shape. Which ones this build's `Data.p4k` ships was not checked here (only `english` is in the extracted tree). Publishing name keys per language would give localised names for every dataset.
- **Keys referenced but missing**: the datasets record `nameStatus: "unresolved"` (starmap 66, missions 103) and keybinds lists 10 `unresolvedLabelKeys`. A reverse report key → datasets that reference it, with the missing ones flagged, is computable from the datasets alone.
- **Name keys outside the rule**: the rule drops display names whose keys are spelled otherwise (lower-case `_title`, `_from`, `ui_CI…`, `pause_Options…`, bare codes such as `Stanton2b`). A published `keys{}` list of every key a published dataset references as a name (its `nameKey`/`titleKey`/`giverKey`/`labelKey`) would keep those names one lookup away without publishing prose. Counted over starmap `nameKey`, missions `titleKey`/`giverKey`, keybinds and attributes `labelKey` on LIVE: 2,850 distinct keys referenced, 2,686 present in the table, 2,610 of those dropped by publish.
- **`~mission(...)` tokens**: 4,223 values contain mission template tokens. Listing the token names used per key is computable and would show which values are templates, not names.
- **Duplicate names**: 480 of the kept name values occur under more than one key (1,122 keys in all); a value → keys index is computable.

Parse gaps:

- The table is read as `key=value` lines; values that span lines or contain `=` before the key ends are not handled specially (only the first `=` splits, localization.rs:101). No case found on LIVE; not checked exhaustively.
- `summary` in the extract has only `strings`; `published` appears only in published files.

Related specs: [dump-enhancements](../plans/specs/dump-enhancements.md) (items 1, 2), [dump-reuse](../plans/specs/dump-reuse.md).
