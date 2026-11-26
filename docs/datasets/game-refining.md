---
title: game-refining (game-refining.json)
tags: [docs, datasets]
updated: 2026-11-25
---

# game-refining (game-refining.json)

What the client knows about refining. That covers the nine refining methods (`RefiningProcess`) with their speed and quality labels and the in-game yield, cost and speed ratings, the 30 raw-to-refined resource pairs, the refinery workload texts, and three crafting settings. Yields, costs, durations and station bonuses are server-side and are not in the files (module doc, `refining.rs` lines 4-6). On LIVE 4.10.201.48085: 9 processes (all rated), 30 refinables and 4 workload tiers. The parse output is 20,465 bytes minified (`jq -c . | wc -c`). The published file drops three prose fields (see Redaction), which leaves 9,194 bytes before `_redacted` is added. The `_schema` key (since tags 4.10.201.48085 / 5.0.202.35026, commit `b9e3700d0`) is not in that figure; with it the same command prints 20,493.

## Source

- Records:
  - `libs/foundry/records/refiningprocess/*.json`: 9 `RefiningProcess` records.
  - `libs/foundry/records/resourcetypedatabase/resourcetypedatabase.json`: `refinedVersion` links and densities.
  - `libs/foundry/records/crafting/globalparams/craftingglobalparams.json`: `CraftingGlobalParams`.
  - Localization: `<processName>_Desc`, `<processName>_Details,P`, and `refinery_ui_Capacity_{Low,Medium,High,Extreme}`.
- Rust: [`crates/sc-gamedata/src/domains/refining.rs`](../../crates/sc-gamedata/src/domains/refining.rs).
  - `impl Domain` at line 164, filters at line 173, `parse` at 185.
  - `parse_refining_process` at 102, `parse_ratings` at 73.
  - Reuses `commodities::parse_resource_database` without a record index, so quality references never resolve here (comment in `parse`).
- Derived from: the same resource database as [game-commodities](game-commodities.md). `refinables` is the subset with `refinesTo`.

## Shape

Top-level keys: `_source`, `_extracted`, `_build`, `processes[9]`, `refinables[30]`, `workloadTiers[4]`, `settings{3}`, `summary{processes, rated, refinables}`.

### processes[]

| Field | Type | Meaning | Units/range | Source |
|---|---|---|---|---|
| `name`, `recordName`, `id` | string | | e.g. `ProcessingType-SlowWasteful`, `RefineNormal` | record |
| `displayName` | string? | Method name | e.g. `Kazen Winnowing` | `processName` localized |
| `speed` | string? | `Slow`, `Normal` or `Fast`. The code comment says it tracks cost, not speed | 3 each | `refiningSpeed` |
| `quality` | string? | `Wasteful`, `Normal` or `Careful`. Tracks yield (code comment) | 3 each | `refiningQuality` |
| `description` | string? | Localized `<processName>_Desc` | redacted on publish | string table |
| `ratings` | object? | `{yield, cost, speed, yieldRank, costRank, speedRank}` from the details string | yield/cost Low-Moderate-High (rank 1-3); speed `Slowest`..`Fastest` (rank 1-9) | `<processName>_Details,P` parsed |

### refinables[]

| Field | Type | Meaning | Units/range | Source |
|---|---|---|---|---|
| `raw`, `refined` | string | ResourceType short names | 30 pairs | `ResourceType`, `refinedVersion` |
| `rawDisplayName`, `refinedDisplayName` | string? | | all present | `displayName` |
| `rawDescription`, `refinedDescription` | string? | | redacted on publish; equal in 23 of 30 pairs | `description` |
| `group` | string | Top group of the raw resource | Metal 14, Mineral 10, ProcessedGoods 3, Nonmetal 2, Organic 1 | group tree |
| `rawDensityGramsPerCc`, `refinedDensityGramsPerCc` | number? | Densities | g/cm³; `refined…` is 1 on all 30; `raw…` is 1 on `Raw_Ice` | `densityType…gramsPerCubicCentimeter` |

### workloadTiers[], settings

- `workloadTiers[]`: `{level, text}` for `Low`, `Medium`, `High` and `Extreme`. `text` is the kiosk sentence, e.g. "Refinery currently has a light workload. No surcharge at this time." The size of each surcharge is not in the files.
- `settings`:
  - `refiningQualityUnitMultiplier`: 2; its meaning is not established.
  - `defaultCompositionQuality`: 500, on the 0-1000 quality scale of [game-commodities](game-commodities.md).
  - `dismantleBlacklistResources[]`: resources that dismantling never gives back (code comment): `Quantanium`, `Stileron`, `Savrilium`, `Lindinium`, `Riccite`, `Ouratite`.

```json
{"name":"ProcessingType-SlowWasteful","recordName":"RefiningProcess.ProcessingType-SlowWasteful",
 "id":"89933668-9b60-4d68-9598-827631105277","displayName":"Kazen Winnowing","speed":"Slow","quality":"Wasteful",
 "ratings":{"yield":"Low","cost":"Low","speed":"Slightly Slow","yieldRank":1,"costRank":1,"speedRank":4}}
```

## Derived, estimated and curated values

- `ratings` is parsed from localized UI text, not from numeric fields. The ranks are positions in two fixed scales in the code (`LEVELS`, `SPEEDS`, lines 48-59), so they are ordinal, not measurements.
- `processes` are sorted by speed, then quality, then name.
- `refinedDensityGramsPerCc` is the refined resource's own density, which is 1.0 for all 30. [game-commodities](game-commodities.md) flags that value as `densityIsDefault` (the engine default, not a measurement), but this dataset does not flag it.
- Nothing is estimated with a formula, curated or heuristic. There is no `_assumptions`.

## Redaction

`PROSE_FIELDS` (`crates/sc-gamedata/src/localization.rs`, `redaction` module) lists `description`, `rawDescription` and `refinedDescription` for `game-refining.json`. Publish removes them at any depth and writes `_redacted: ["description","rawDescription","refinedDescription"]`. `workloadTiers[].text` is not listed and stays in the published file.

## Consumers

- **starorg-codex** (branch `feat/dataset-reuse`): `crates/starorg-mcp/src/prebuilt.rs:176` needs top-level `processes`, `refinables`, `workloadTiers`, `settings` and `summary`, allows the three description fields to be stripped, schema 1, with no typed check. `crates/starorg-mcp/src/stamps.rs:268` records that `game-refining` depends on the `domains/commodities` module. No typed field reader was found.
- **SCMCP:** generic `src/localdata.ts` access only.
- No reads in snares, MCsim, Bindscape or sc-telemetry. snares `build-data.mjs:84` reads a `Refinery` amenity from `game-starmap`, not this file.
- Contracts to keep: the starorg `needs` keys, the general `_build` rule from [consumer-datasets](../plans/specs/consumer-datasets.md), and decision 2 of [dump-reuse](../plans/specs/dump-reuse.md) (prose stays stripped; the app refills it from keys). `_schema: "game-refining@1"` (since tags 4.10.201.48085 / 5.0.202.35026), and `build.json` `extractor` plus `files[]` hashes (same tags).

## Derivable data

- **Parse gap, the main one: no keys for the redacted prose.** Unlike commodities, refining emits no `descriptionKey`, `rawDescriptionKey` or `refinedDescriptionKey`. The `[.. | keys[] | select(endswith("Key"))]` set over the file is empty. Once publish strips the three fields, starorg's `fill_prose` (`prebuilt.rs`, which looks for `<field>Key`) has nothing to fill them from. The keys are known at parse time:
  - `processes[]`: the localized `processName` plus `_Desc`, e.g. `refinery_ui_ProcessingType_SlowWasteful_Desc`;
  - `refinables[]`: `descriptionKey` of the raw and refined `ResourceType` (e.g. `items_commodities_agricium_desc`).

  Emitting `descriptionKey`, `rawDescriptionKey`, `refinedDescriptionKey` and a `nameKey` per process would make the published file self-sufficient.
- **Fields the source carries but the dataset does not read:**
  - `RefiningProcess.processDetail`: the record names its details key itself (`"@refinery_ui_ProcessingType_FastCareful_Details"`). The domain builds `<processName>_Details,P` instead. That resolves for all 9 today, but reading `processDetail` follows the record rather than a naming convention.
  - `CraftingGlobalParams.dismantleBlacklistEntityClasses`: 2 entity classes never given back by dismantling (`harvestable_mineral_1h_janalite`, `harvestable_ore_1h_saldyniumore`).
  - `CraftingGlobalParams.defaultBlueprintSelection` (`DefaultBlueprintSelection_Whitelist`): 9 blueprint records, e.g. `crafting/blueprints/dismantle/globalgenericdismantle.json` and `bp_craft_behr_pistol_ballistic_01.json`. These are probably the blueprints known from the start, but that meaning is not established. `game-blueprints` could carry them as a `defaultSelected` flag.
- **Joins with other datasets:**
  - `refinables[].raw` → [game-mining-spawns](game-mining-spawns.md) `ores` (through `game-commodities` `minedAs`) gives where each refinable ore is mined. The commodity detail and the Mining tab would use it.
  - `refinables[].raw` → `game-commodities` `quality` (tier, curve) gives the raw quality a refinery receives.
  - `dismantleBlacklistResources` → `game-commodities` `resources[].displayName`, and → `game-blueprints` `resourceUse` (which blueprints consume a resource that dismantling won't return).
  - **Estimate:** the in-game ranks (yield, cost, speed: 1-3, 1-3, 1-9) are the only ordering the client holds. Any numeric yield or time per method would rest on outside data, not on these records.
- **Other gaps:**
  - The refined density of 1.0 is the engine default (see above) but carries no `densityIsDefault` flag here. A consumer computing a mass ratio between raw and refined would get a wrong ratio.
  - `refinables` reads only top-level resources. Refined forms that exist only inline under `refinedVersion` are looked up by name, and all 30 resolve on LIVE (`refinedDisplayName` is present on every pair).

Related: [dump-reuse](../plans/specs/dump-reuse.md) (decision 2 and the redaction notes) and [dump-enhancements](../plans/specs/dump-enhancements.md) item 2.
