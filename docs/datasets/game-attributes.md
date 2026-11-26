---
title: game-attributes (game-attributes.json)
tags: [docs, datasets]
updated: 2026-11-25
---

# game-attributes (game-attributes.json)

The game's default option values (graphics, audio, camera, flight, HUD, input), one row per `<Attr>` in the defaults `attributes.xml` that the in-game Options screen reads and writes, with the console variable each drives, its platforms and UI flags, and a label where the string table has a matching `pause_` key. On LIVE 4.10.201.48085 it holds 345 rows for 334 option names, file version 35. The file is 76,671 bytes minified.

## Source

- Not a DataCore record. The archive file `Data/Libs/Config/Profiles/default/attributes.xml` (binary CryXmlB), declared under `Domain::files` (`crates/sc-gamedata/src/domains/attributes.rs:191`, glob `**/Libs/Config/Profiles/default/attributes.xml`). It is read like `game-keybinds` (`read_profile_file` from `domains/keybinds.rs`).
- String table, only to look up `pause_Options<Name>` then `pause_<Name>` (case-insensitive, `,P` tolerated) (`attribute_label`, attributes.rs:98; `index_table`, attributes.rs:79).
- Code: `parse_attributes` (attributes.rs:111), `Domain::parse` (attributes.rs:199).
- Derives from no other dataset.

## Shape

Top-level keys: `_source`, `_extracted`, `_build`, `version`, `attributes[]`, `summary{}`. `_schema: "game-attributes@1"` follows `_build` (since tags 4.10.201.48085 / 5.0.202.35026).

- `version`: the root `Version` attribute as a number (35), `null` when absent.
- `summary`: `rows` 345, `names` 334, `withCvar` 339, `platformSpecific` 48, `labelled` 77 (distinct names with a label).

### `attributes[]` (file order)

| Field | Type | Meaning | Count/range | Source |
|---|---|---|---|---|
| `name` | string | Option name (`Resolution`, `FlightThrustInvertToggle`) | 345 rows; 8 names repeat per platform | `name` |
| `cvar` | string\|null | Console variable the option drives | null on 6: `Resolution`, `MSAAMode`, `AutoDetect`, `Stereo`, `ControllerStickLayout`, `ControllerButtonLayout` | `cvar` |
| `default` | string | Default value as written (always a string: `"1"`, `"-1"`, `"smaa_2tx"`, `"#1"`) | | `value` |
| `platform` | string[]\|null | Platforms the row applies to, `null` = all | `["pc"]` 39, `["xbox","ps3"]` 3, `["xbox_s"]` 2, others 1 each; null 297 | `platform` split on `\|` |
| `confirmation` | bool | The game asks to confirm a change (resolution, window mode) | true on 8 | `confirmation` non-zero |
| `preview` | bool | The option previews live while the slider moves | true on 16 | `preview` |
| `gameOnly` | bool | Copied flag; meaning not established | true on 4 | `gameOnly` |
| `nodefault` | bool | Copied flag; meaning not established | true on 1 | `nodefault` |
| `label` | string\|null | Display label when a `pause_Options<Name>`/`pause_<Name>` key exists | 78 rows | string table |
| `labelKey` | string\|null | That key as the table spells it, without a `,P` suffix (`pause_OptionsCrosshairMode` is `pause_OptionsCrosshairMode,P` in the table) | 78 rows | string table |

Example (real, the three `Height` rows):

```json
[{"name":"Height","cvar":"r_height","default":"720","platform":["pc"],"confirmation":true,"preview":false,"gameOnly":false,"nodefault":false,"label":null,"labelKey":null},
 {"name":"Height","cvar":"r_height","default":"720","platform":["xbox_s"],"confirmation":true,"preview":false,"gameOnly":false,"nodefault":false,"label":null,"labelKey":null},
 {"name":"Height","cvar":"r_height","default":"1080","platform":["ps5","xbox_x"],"confirmation":true,"preview":false,"gameOnly":false,"nodefault":false,"label":null,"labelKey":null}]
```

## Derived, estimated and curated values

- `label`/`labelKey` come from a name-pattern lookup, not from the file: "The file carries no labels and no ranges" (attributes.rs:26). Only the two `pause_` patterns are tried; anything else stays `null` rather than guessed.
- No estimates and no curated sources.

## Redaction

Nothing. `label`, `labelKey`, `platform`, `confirmation` and `version` stay unredacted (attributes.rs:29). The published string table drops every `pause_…` label key (0 of 78 match the name-key rule), so `label` here is the only published label text.

## Consumers

- **Bindscape** (`scsticks/scripts/import_sc_attributes.js`): keeps PC rows (`platform` null or containing `pc`, `:27`), merges rows by `name` collecting `cvar`s (`:30-36`), and writes `name`, `cvar`, `default`, `label`, `labelKey`, `confirmation` (`:40-46`) and `version` (`:53`).
- No other tool reads it.
- Contracts ([consumer-datasets](../plans/specs/consumer-datasets.md)): never redact `label`, `labelKey`, `platform`, `confirmation`, `version`; `_build` equals the tag; `_schema` stem `game-attributes`.

## Derivable data

The source file is read straight from `Data.p4k` and is not in the extracted record trees, so unread attributes cannot be listed from a raw copy. From the code, an `<Attr>` gives `name`, `cvar`, `value`, `platform`, `confirmation`, `preview`, `gameOnly` and `nodefault`; any other attribute is dropped (none is known; not checked).

Values computable:

- One row per option with per-platform defaults folded in (`defaults: {pc: "720", xbox_s: "720", "ps5|xbox_x": "1080"}`), which Bindscape rebuilds itself. 8 names have platform variants: `AnisotropicFiltering`, `AspectModifier`, `Height`, `OverscanBorderX`, `OverscanBorderY`, `StereoDevice`, `SysSpec`, `WindowMode`.
- Value type per option (boolean, integer, float, enum string) inferred from `default`: an estimate, since the file has no type or range.
- Grouping by name prefix (`IFCS_Setting_*`, `FlightController_Setting_*`, `Weapon_Setting_*`, `LookAhead*`, `VJoy*`): an estimate; the prefixes are naming habits, not fields.
- Labels for more options: 267 rows have no label, and for 8 of the unlabelled names (`VideoVolume`, `Subtitles`, `HeadtrackingToggle`, `Crosshair` …) some `pause_`/`ui_` key contains the option name as a substring. Using those would be a guess the domain refuses today (estimate).

Combining with other datasets:

- [game-keybinds](game-keybinds.md) option trees hold invert/curve defaults for the same input functions some attributes tune (`FlightThrustInvertToggle`, `VJoy*`); no shared key links them, so any join is an estimate.

Parse gaps:

- Only 77 of 334 names get a label; the Options screen shows labels for more, built from keys the two patterns miss (not established which).
- 6 rows have no `cvar` (listed above); how the game applies them is not in the file.

Related specs: [tool-data-sync](../plans/specs/tool-data-sync.md) (Bindscape step), [consumer-datasets](../plans/specs/consumer-datasets.md).
