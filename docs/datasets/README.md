---
title: The data pack
tags: [docs, datasets]
updated: 2026-11-25
---

# The data pack

The data pack is what one published game build looks like: the parsed `game-*.json` datasets, patch notes against the previous build, and a `build.json` manifest, committed and tagged with the build string in the public dumps repo (`voidput/sc-gamedata-dumps`). The game data is CIG's. The pack adds nothing to it. Every value is read from the extracted records or computed from them, and computed values say so (see [Conventions](#conventions)). The Rust crate `crates/sc-gamedata` is the only producer. Each dataset has its own note in this folder, linked [at the end](#dataset-notes).

Figures below are for tag `4.10.201.48085` (LIVE) unless a line says otherwise. Published tags so far: `1.0.191.28374`, `4.10.197.2878`, `4.10.200.58746`, `4.10.201.48085` and `5.0.202.35026` (PTU).

## What a published tag contains

Written by `publish::stage` (`crates/sc-gamedata/src/publish.rs:654`):

- **`data/game-*.json`**: 33 datasets, redacted ([Redaction](#redaction)), written compact, one file per dataset. Together they come to 31,339,019 bytes at `4.10.201.48085`.
- **`notes/<build>.md` and `notes/<build>.json`**: patch notes against the previous published build on the same release line (same first version number; `notes_for`, publish.rs:442). The `.md` lists new and removed ships, weapons, armour and so on by display name. The `.json` has the same sections with record names. When there is no earlier build on that line, no notes are written.
- **`build.json`** (`build_manifest`, publish.rs:283): `build`, `channel` (from the extract's `_build.json` marker, or `--channel`, else `LIVE` with a warning), `patch` and `branch`. For `4.10.201.48085`: `LIVE`, `4.10.0`, `sc-alpha-4.10.0`.
  - `extractor.commit`, plus `files[]` with `name`, `sha256`, `bytes` and `schema` for every dataset (since tags 4.10.201.48085 / 5.0.202.35026; sc-gamedata main commit `b9e3700d0`).
- **Template files** from `scripts/dump-repo-template/`: `.github/workflows/fanout.yml` is rewritten on every publish. `README.md` and `NOTICE` are copied only when they are missing.

### Fan-out

`fanout.yml` runs when a GitHub release is published or edited, or when it is started by hand with a tag. It reads `channel` from `build.json`. Older tags have no `build.json`, so for them it falls back to the release title, then to the prerelease flag. Only `LIVE` and `PTU` builds are dispatched. For those it sends a `repository_dispatch` `gamedata-published` with `{tag, build, channel}` to `voidput/snares`, `voidput/MCsim` and `voidput/scsticks`, and each tool's `data-sync.yml` bakes the datasets at that tag. The GitHub release step itself (`release_title`, `release_flags`, `release_notes`, publish.rs:272-309) marks `PTU`, `EPTU` and `TECH-PREVIEW` as prereleases and only `LIVE` as latest. The launcher post-exit hook (`scripts/release-on-exit.sh`) publishes any installed build the dumps repo has no tag for. See [tool-data-sync](../plans/specs/tool-data-sync.md).

## Common header fields

Every dataset is a JSON object. A few underscore keys come first, followed by its named collections. Collections are arrays of records, and `by*` keys are indexes over those same records, so they do not add records. Most files also have a `summary` object of counts.

| Key | Type | In | Meaning |
|---|---|---|---|
| `_source` | string | all 33 | Where the data came from. Game-file datasets say `"Star Citizen game files via StarBreaker"` (game-strings-english adds the `global.ini` path). The two derived datasets name their inputs instead. |
| `_extracted` | ISO-8601 string | 32 (not game-search-index) | When the parse wrote the file. |
| `_build` | string | all 33 | The game build. It equals the tag; sc-telemetry and snares check this. |
| `_schema` | string `"<dataset>@1"` | all 33 | Comes right after `_build` (since tags 4.10.201.48085 / 5.0.202.35026). The part before `@` is the file stem, and the number is bumped only on a breaking change ([consumer-datasets](../plans/specs/consumer-datasets.md)). |
| `_redacted` | string[] or string | published files that strip something | The prose fields publish removed, or for game-strings-english a note that the table keeps name keys only. Not present in the parse output. |
| `_assumptions` | string[] | game-commodities (1), game-item-obtain (5), game-mining-spawns (4), game-npc-loadouts (1), game-paints (4) | Plain-language statements of what the derived numbers in that file assume, because the records do not state it. |

## Redaction

Publish runs `redact_dump` (`localization::redaction`, `crates/sc-gamedata/src/localization.rs:372`):

- `PROSE_FIELDS` are removed wherever they appear in a file. These are `description` in game-commodities, game-manufacturers, game-missions, game-ships and game-starmap, and `description`, `rawDescription` and `refinedDescription` in game-refining. Each record keeps its `descriptionKey`, so the text stays one string-table lookup away.
- game-missions: a `title` longer than 120 UTF-16 units counts as briefing text and goes. A `name` that copied it becomes the record's short name, and `"title over 120 characters"` is added to `_redacted`. That did not happen on `4.10.201.48085`, where `_redacted` is `["description"]`. starorg-codex refuses a file where it did.
- game-strings-english: kept only when the key is name-shaped (`Name`, `Title`, `DisplayName`, `ShortName`, `Nickname` as a word), the key is not prose-shaped (`desc`, `tooltip`, `hint`, `subtitle`, `Dlg_`, `PU_`, `DXS`/`DXM`), and the value is non-empty and at most 120 UTF-16 units long. About 15,227 of 91,358 strings stay.
- Never redacted, per the Bindscape contract: `label`, `labelKey`, `optionTrees`, `optionGroup`, `platform`, `confirmation`, `version` in game-keybinds and game-attributes.

## How the datasets relate

Two datasets are **built from other datasets' output files**, not from game records:

| Dataset | Reads |
|---|---|
| [game-item-obtain](game-item-obtain.md) | game-item-tags, game-loot, game-item-sources (required); game-blueprints, game-wikelo-trades (optional) (`read_inputs`, `item_obtain.rs:497`) |
| [game-search-index](game-search-index.md) | game-item-tags `items[]`, game-ships `vehicles[]` (`search_index.rs`) |

The others read game records, and some of them **share parse code or records** with another domain within the same extract:

| Dataset | Shares with | What |
|---|---|---|
| game-ships | game-ship-components, game-weapons, game-ammo, game-containers | Loadout items go through `parse_component`, gun DPS through the weapon helpers and `ammoparams`, cargo grids through `inventorycontainers` |
| game-weapons | game-ammo | `parse_ammo`. Damage always comes from the ammo. |
| game-ship-components | game-containers | Cargo grids through `parse_container` |
| game-mining-spawns | game-starmap, game-item-sources | `build_locations` and `loot_areas` |
| game-commodities | game-mining-spawns | `minedAs` from `mined_elements` |
| game-refining | game-commodities | The same resource database; `refinables` is the subset with `refinesTo` |
| game-starmap | game-law | Both load the `Jurisdiction` records, so the names match |
| game-paints | game-ships, game-manufacturers | Vehicles and their names through `is_vehicle_path` / `parse_vehicle`; manufacturer codes through `names_by_code` |
| game-cargo | game-shops | `box_sizes` |
| game-attributes | game-keybinds | `read_profile_file` (both are profile XML, not DataCore) |
| every localised domain | game-strings-english | The same `global.ini` table, read from the archive rather than from the published file |

## Conventions

- **Key casing.** Output keys are camelCase (`recordName`, `densityGramsPerCc`). Game identifiers inside values keep the game's spelling (`AEGS_Gladius`, `Char_Armor_Torso`, `EntityClassDefinition.AEGS_Gladius`). One exception: `game-ships.vehicles[].capabilities` uses sc-telemetry's `derive_caps` key names, so `cargo_scu` sits next to `cargoKnown`. Both stay as they are by contract.
- **Loc keys and names.** Where a record names something through the string table, the dataset carries `name`/`displayName` (the resolved English text), plus `nameKey` and `descriptionKey`. These are the table keys without the `@`, spelled as the table spells them (`loc_key`, `localization.rs`). Keys let a reader show the player's own language from their own `global.ini` and refill redacted prose ([dump-reuse](../plans/specs/dump-reuse.md) decision 2). game-keybinds and game-attributes use `label`/`labelKey` instead.
- **`nameStatus`** is present only when the name is not real (`NameStatus`, `localization.rs:260`). It is `placeholder` when the reference resolves but to a placeholder marker, `unresolved` when it is an `@` key the table lacks, and `empty` when it is unset, `@LOC_EMPTY`, `@LOC_UNINITIALIZED` or blank. This is the crate's own classification, not a game field. Where a domain drops placeholder rows instead (game-manufacturers), it is never set.
- **`Estimate` suffix.** A field whose value rests on an assumption the records do not state ends in `Estimate`: `shareEstimate` (game-commodities quality steps), and `scoreEstimate` and `perRollEstimate` (game-item-obtain). The assumption is spelled out in `_assumptions`. game-npc-loadouts `items[].chance` is also an estimate but keeps the name `chance` by contract ([dump-reuse](../plans/specs/dump-reuse.md) E.4), and its `_assumptions` says so.
- **`placeSource`** tells where a human place name came from. `data` means a record says it, `heuristic` means it was built from a record or preset name, and `curated` means it comes from a hand-written rule in the crate. It is used in game-mining-spawns (`belts[]`, `spots[]`) and game-item-sources (`areas[]`). Treat anything but `data` as a guess.
- **Computed, not copied.** Totals, DPS, SCU from dimensions and similar values are computed from record fields. Each note lists them under "Derived, estimated and curated values".
- **Templates.** Records the game marks as templates are either kept with `isTemplate: true` (game-ships, game-ship-components, game-fire, game-fps-weapons) or dropped (game-armor). Each note says which.

## Consumers

Who reads which file, from the consumer repos (read-only grep, file:line evidence in each note). `●` marks a named reader of the file's fields or top-level keys, `g` marks generic access only, and `in-crate` marks a reader inside sc-gamedata. SCMCP's `src/localdata.ts` lists and searches every `game-*.json` in its data dir, so every file is generically reachable there. `g` is shown only where a note calls it out. starorg-codex is the app in `~/git/starorg-strings`, branch `feat/dataset-reuse`, whose dump reader (`crates/starorg-mcp/src/prebuilt.rs`) takes a file only when its required top-level keys are present.

| Dataset | snares | MCsim | Bindscape | sc-telemetry | SCMCP | starorg-codex | in-crate |
|---|---|---|---|---|---|---|---|
| game-ammo | | | | | ● | ● | |
| game-armor | | | | | | ● | |
| game-attributes | | | ● | | | | |
| game-blueprints | | | | | ● | ● | item-obtain |
| game-cargo | | | | | g | ● | |
| game-commodities | | | | | g | ● | query |
| game-consumables | | | | | | ● | |
| game-containers | | | | | ● | ● | |
| game-fire | | ● | | | | | |
| game-fps-weapons | | | | | ● | ● | |
| game-item-obtain | | | | | | | |
| game-item-sources | | | | | ● | ● | item-obtain |
| game-item-tags | | | | | ● | ● | item-obtain, search-index |
| game-keybinds | | | ● | ● | | | |
| game-law | | | | | | ● | |
| game-loot | | | | | ● | ● | item-obtain |
| game-manufacturers | | | | | ● | ● | |
| game-mining | | | | | ● | ● | |
| game-mining-spawns | | | | | ● | ● | query |
| game-missions | | | | | ● | ● | |
| game-npc-loadouts | | | | | | ● | |
| game-paints | | | | | | | |
| game-quantum | ● | | | | | ● | |
| game-refining | | | | | g | ● | |
| game-reputation | | | | | ● | ● | |
| game-salvage | | | | ● (loaded, no field matches yet) | | ● | |
| game-search-index | | | | | ● | never from a dump | mcp |
| game-ship-components | ● | ● | | | ● | ● | ships (code) |
| game-ships | ● | ● | | ● | ● | ● | search-index |
| game-shops | | | | | g | ● | |
| game-starmap | ● | | | | ● | ● | |
| game-strings-english | | | | | ● | never from a dump | diff |
| game-weapons | | ● | | | | ● | |
| game-wikelo-trades | | | | | ● | ● | item-obtain |

Contracts that hold for every file ([consumer-datasets](../plans/specs/consumer-datasets.md), sc-telemetry's hub reader): `_build` equals the tag, and the tag matches `^\d+(\.\d+){2,3}$`. The `_schema` stem equals the file stem, and `build.json` `files[]` sizes and hashes match the published bytes. Per-file contracts are in each note's Consumers section.

## Dataset notes

Sizes are published bytes at `4.10.201.48085`, from that tag's `build.json` (which includes the `_schema` key).

| Note | Bytes | Hook |
|---|---:|---|
| [game-ammo](game-ammo.md) | 91,477 | 249 `AmmoParams` projectiles: where weapon damage actually lives |
| [game-armor](game-armor.md) | 2,115,994 | 2,497 armour and 1,874 clothing pieces, with resistance multipliers and storage |
| [game-attributes](game-attributes.md) | 76,700 | 345 default Options-screen values and the cvars they drive |
| [game-blueprints](game-blueprints.md) | 3,269,568 | 1,634 crafting blueprints, recipe cost trees and the missions that award them |
| [game-cargo](game-cargo.md) | 52,441 | 82 NPC and mission cargo manifests, plus freight elevator timings |
| [game-commodities](game-commodities.md) | 202,237 | 207 resource types, 135 cargo items and the quality-roll curves |
| [game-consumables](game-consumables.md) | 118,105 | 231 medpens, pills, food and drink, with effects scaled to volume |
| [game-containers](game-containers.md) | 165,712 | 596 inventory containers with interior dimensions and computed SCU |
| [game-fire](game-fire.md) | 393,596 | Global fire rules and 523 ship items that can ignite, explode or overheat |
| [game-fps-weapons](game-fps-weapons.md) | 113,166 | Thin catalogue of 520 personal weapons (492 ranged, 28 melee) |
| [game-item-obtain](game-item-obtain.md) | 2,242,589 | Derived: where each of 5,930 items comes from, with estimated odds |
| [game-item-sources](game-item-sources.md) | 1,434,376 | 167 loot-area presets: what spawns where, by weight, plus contract awards |
| [game-item-tags](game-item-tags.md) | 5,558,825 | 12,894 items with resolved tags: the join between items and tag-driven loot |
| [game-keybinds](game-keybinds.md) | 394,136 | 50 action maps, 1,104 actions and their default inputs per device |
| [game-law](game-law.md) | 23,934 | 43 crimes and 13 jurisdictions with fines and prohibited goods |
| [game-loot](game-loot.md) | 407,577 | 220 loot tables and 267 archetypes (v1 and v3), in the game's own shape |
| [game-manufacturers](game-manufacturers.md) | 29,950 | 131 in-fiction companies and a code → name map |
| [game-mining](game-mining.md) | 23,847 | Rock-breaking physics for 46 elements and 27 mining-module modifiers |
| [game-mining-spawns](game-mining-spawns.md) | 856,336 | Where ores spawn, walked from provider presets down to rock compositions |
| [game-missions](game-missions.md) | 1,198,085 | 2,584 contract-manager entries: giver, type, payout, buy-in |
| [game-npc-loadouts](game-npc-loadouts.md) | 2,673,263 | What 2,312 NPC types wear and wield, with estimated chances |
| [game-paints](game-paints.md) | not published (local parse 2,913,411) | 1,042 ship paints and the vehicles each fits, by the paint port's tag rule |
| [game-quantum](game-quantum.md) | 469 | Global quantum-travel rules (start altitude, party range, spline constants) |
| [game-refining](game-refining.md) | 9,287 | 9 refining methods and 30 raw-to-refined pairs; yields are server-side |
| [game-reputation](game-reputation.md) | 196,552 | 380 reputation ranks in 239 groups, with thresholds and drift |
| [game-salvage](game-salvage.md) | 445 | Global structural salvage and repair-beam constants |
| [game-search-index](game-search-index.md) | 869,406 | Derived: compact fuzzy-find rows for 12,894 items and 1,104 ships |
| [game-ship-components](game-ship-components.md) | 2,295,418 | 3,465 ship components in 21 categories with shared headline stats |
| [game-ships](game-ships.md) | 3,502,389 | 1,104 vehicles: identity, hull, flight, stock loadout, totals and capabilities |
| [game-shops](game-shops.md) | 3,974 | Global shop rules: sell-back percents, kiosk thresholds, box fees; prices are server-side |
| [game-starmap](game-starmap.md) | 1,158,588 | 2,132 star-map objects with parents, jurisdiction, amenities and positions |
| [game-strings-english](game-strings-english.md) | 851,569 | The English string table, cut to name keys on publish |
| [game-weapons](game-weapons.md) | 968,351 | Ship and FPS weapons joined to their ammo: DPS, alpha, range, sustain |
| [game-wikelo-trades](game-wikelo-trades.md) | 40,657 | 53 Wikelo barter trades and what they pay |

Related specs: [consumer-datasets](../plans/specs/consumer-datasets.md) (per-tool contracts and requested changes), [dump-enhancements](../plans/specs/dump-enhancements.md) (planned additions), [dump-reuse](../plans/specs/dump-reuse.md) (the starorg-codex dump reader), [tool-data-sync](../plans/specs/tool-data-sync.md) (fan-out and baking), [rust-pipeline](../plans/specs/rust-pipeline.md) (the Rust producer).
