---
title: Dump enhancements from starorg-codex
tags: [plans, specs, dumps]
updated: 2026-11-23
status: active
---

# Dump enhancements from starorg-codex

Add to the published dumps (voidput/sc-gamedata-dumps) what the starorg-codex app
(`~/git/starorg-strings`) already works out from the game data, and fix the prose leak. Source
of this plan: a read-only gap analysis of both repos on 2026-11-23 (sc-gamedata HEAD
`a9097c91f`).

## Rules for every item

- **Rust parity.** Every new field must also be added to the app's Rust port
  (`crates/starorg-gamedata/src/domains/*`, checked by `compare.rs`), or the app's Diff of a
  published build against a local one shows it as noise. Each item is one sc-gamedata PR and one
  starorg-strings PR.
- **Estimates say so.** A field that rests on an assumption the data does not state gets an
  `Estimate` suffix (`perRollEstimate`, `shareEstimate`) and an `_assumptions` note in the file.
- **Heuristics are tagged.** A field taken from a hand-written table carries
  `placeSource: "heuristic" | "curated" | "data"` (or the matching `*Source` field).
- **No prose, no assets.** Localisation keys and names only. Nothing UEX-derived.
- **Size.** Measure the growth per build with `npm run publish-dump -- --dry-run` and list it in
  the PR. Budget for everything here: about +3 MB on a published build of about 37 MB.
- **No publish from a working branch.** `make publish` only after merge, on a real build.

## Found during the analysis

- **Prose is published.** `scripts/publish-dump.mjs` (about line 125) redacts only the strings
  file. `description` text in `data/`: missions 917 KB, ships 411 KB, starmap 176 KB,
  commodities 63 KB, manufacturers 26 KB, refining 2 KB. That is about 1.6 MB per build, and it
  is already in published tags.
- **No localisation keys in any dataset.** `game-item-tags` replaces a placeholder name with
  `record.shortName` plus `isTemplate` (`src/domains/item-tags.ts` 111-120). All 12,896 items
  have a non-empty name, so consumers cannot see which names are missing.
- **`src/query/` has only obtain, where, shops and fuzzy.** The mined, quality, commodities and
  sells queries exist only in Rust.
- **`game-item-sources` areas have no place.** There are 167 areas, 2,989 configs and 6,879
  slots; the app's place table is `AREA_PLACES` in `crates/starorg-mcp/src/hints.rs` 73-106.
- **`oreSpawns[].system` says `"Aaronhalo"`** for the Aaron Halo belt; the app patches it in
  `system_name`.
- **Quality bands are raw.** There are no clipped steps and no shares.
- **Missions have no item links**, and their givers stay as `~mission(...)` tokens.

## Decisions (settled 2026-11-23)

1. **Short mission titles stay.** Item 2 keeps `title` when it is under 120 characters (it reads
   as a name) beside `titleKey`; longer mission text and every `description` go.
2. **`description` is stripped at once, no deprecation release.** SCMCP's only dump read is the
   vendored ship preview (`src/vendor/sc-gamedata/fuzzy.ts` line 249, from this repo's
   `src/query/fuzzy.ts`). It is guarded by `typeof`, so the preview just loses its text. The
   `description` read in SCMCP `src/index.ts` is UEX marketplace data, not the dump.
3. **Patch notes summarise `game-item-obtain`.** Counts plus the top movers by odds change; it
   is never skipped.

## Progress (2026-11-23)

TS side of items 1, 2, 7, 6 and 10 built in the working tree, uncommitted, measured on
LIVE 4.10.201.48085 (415 tests pass). The Rust parity for all five is on starorg-strings
`feat/dump-loc-keys`, uncommitted: 23 of 28 datasets are byte-identical. Ships, fps-weapons,
item-tags and search-index differ only by stale 2026-11-01 records left in sc-gamedata
`extracted-data` (fix: a clean re-extract), and strings-english has no golden.

**Open:** the app's Diff (`starorg-core/src/dev/diff.rs`) has no redaction alignment, for
strings or for prose, so a published build diffed against a local one shows removed text as
noise. It needs an `alignProse`/`alignRedaction` port.
Deviations from the item text:

- **1:** `nameStatus` is omitted when `real`, the way `isTemplate` is omitted when false.
  - Blueprints get no keys: 1,633 of 1,634 `blueprintName` refs are placeholders, and the
    real name comes from the product item.
  - Missions use `titleKey` and `giverKey` in place of `nameKey`.
  - `localize` itself now retries `,P`, so the per-domain retries in commodities and law
    are gone. This recovers 394 item names and some ship, component, starmap and weapon
    names (names change in the next notes).
  - "PH - …" now counts as a placeholder.
  - Cost: +1.82 MB minified (item-tags alone is +0.94 MB), over the 0.8 to 1.0 MB
    estimate, but net of item 2 it is about +0.2 MB.
- **2:** the rules live in `scripts/redact.mjs`, the sizes are −1.64 MB on 4.10.200 data,
  and `diff` aligns a full side with a prose-redacted one through `_redacted`.
- **7:** there is also a `subgroupLabel`.
  - `configs[]` became a top-level `damageGroups[]`: the 3 configs are identical, and only 1
    resource is volatile.
  - +65 KB together with item 6.
- **6:** steps carry the range clipped to min-max, and shares are rounded to 1e-8 for
  cross-implementation byte parity.
  - The per-rock `qualityScale` effect is deferred, because the game files don't state how
    it applies.
- **10:** `trades[].rewards[]` (a list, since weighted choices exist) joins generator
  contracts by template path, reading `ContractResult_Item` and `ItemAwardEntityClass`
  (success results only). 51 of 53 trades now carry the award.
  - `rewardMismatch` checks vehicle trades only; the others are named by codename.
  - **Gap found:** 37 generator contracts on the shared
    `itemresourcegathering_thecollector` template (Idris, Asgard, apar HMG, …) carry their
    hand-ins in overrides and appear in no trade. They are listed in
    `summary.contractsWithoutTemplateTrade`. Turning them into trades is a candidate
    follow-up.

**Item 5 (2026-11-25), Rust only** (`crates/sc-gamedata`, branch `feat/rust-mining`, no TS
per the switchover), measured on LIVE 4.10.201.48085. Accepted: `ores.sadaryx` has no belt
and two spots, `v3slotpreset_rockcracker` and `_sandbox` (slotShare 1, place "Rock Cracker,
Nyx", `curated`). Size: +190 KB compact on mining-spawns (666,497 to 856,304 B; gzip 25,808
to 35,890), +2.7 KB on commodities.

- **Deviation:** the per-ore data is a new top-level `ores` map keyed by `oreKey`, not
  `oreLocations[ore].belts[]`. `oreLocations` stays a list of names, because the app's Dev
  table reads it as an index.
  - Each entry has `elements[]`, `resources[]`, `belts[]` and `spots[]`.
  - `belts[]` sums the spawn groups per (location, rock). Each belt has
    `place`/`placeSource`, `system`, `kind`, `tier`, `spawnShare` and the percent range.
  - A spawn entry counts once. 55 of 268 rocks list an element twice in their composition
    (Quantainium at 5.7-11.7 and 28.3-78.3), so summing `oreSpawns` rows doubles the share:
    the app's `query/mined.rs` `report` still does that and needs the same fix. The percent
    range is the lowest and highest bound over the parts, not their total.
  - `spots[]` gives the best rock per slot preset: `area`, `rock`, `slotShare`, `configs`.
    It skips slots of weight 0.
- `oreKey` is `ore_core` of the element key. `resource` and `minedAs[]` come from the
  element's own `resourceType`, not from name matching, since the spellings differ:
  `aluminium_ore` is `Ore_Aluminum`, `sileron_ore` is `Ore_Stileron`. The data also says
  that `flowstone` is `Aphorite` and `testelement_balance` is `Copper`.
- `system` is now fixed on both `locations[]` and `oreSpawns[]` (this is not additive):
  - `Aaronhalo` and `Stanton2c` become `Stanton`.
  - The non-system stems (`Lagrange`, `Asteroidcluster`, `Resourcerush`, …) are dropped,
    so 147 oreSpawns rows have no system.
  - A stem counts as a system when it starts with a star map system's name (with Stanton,
    Pyro and Nyx as the fallback), so a system a build adds is kept. `AaronHalo` to
    `Stanton` is curated.
  - The records don't say which system the Lagrange points are in.
- `locations[].place` is new, and `belt_place` keeps stems that aren't systems:
  `ResourceRush_Gold` reads `Resource Rush Gold` (the app gives `Gold`).
- `_assumptions` has four entries: `spawnShare` can't be compared across mining kinds, the
  percent range spans a rock's parts, how `place` is built per `placeSource`, and where
  `system` comes from.
- **Open:**
  - `spots[].place` uses only the app's rock-cracker rule, so the other area places wait
    on 4a (join `spots[].area` to `game-item-sources` `areas[].name`).
  - The starorg-strings Rust parity change is not done.

## Items, in build order

| # | Item | Effort | Size per build |
|---|---|---|---|
| 1 | Localisation key refs on every named record | S-M | +0.8 to 1.0 MB |
| 2 | Strip prose from non-string datasets at publish | S | -1.6 MB |
| 7 | Commodity derived flags and reverse links | S | +30 KB |
| 6 | Quality steps and share estimates | S | +40 KB |
| 10 | Wikelo reward resolved at parse time | S | under 10 KB |
| 4a | Area place classification | S | +15 KB |
| 5 | Mining: one ore key, totals per belt, hand spots | M | +150 to 250 KB |
| 3 | `game-item-obtain`: loot odds per item | L | +2 to 3 MB |
| 8 | Generated-name fallback | M | +0.3 MB |
| 9 | Mission and quest item links | M | +200 to 400 KB |
| 4b | Area placement from object containers | L | +0.3 to 0.5 MB |

### 1. Localisation key refs on every named record

- **Add:** `nameKey`, `descriptionKey` and `nameStatus: real | placeholder | unresolved | empty`.
- **Where:** item-tags, ships, ship-components, weapons, commodities (resources and items),
  missions (title, giver, description), starmap, manufacturers and blueprints.
- **Port from:**
  - `crates/starorg-data/src/gamedata/mod.rs` pass 6 (`item_names`, `item_descs`,
    `resource_descs`, including the rule that a resource owns its key);
  - `crates/starorg-gamedata/src/localization.rs` (`is_unresolved`, the `,P` retry).
- **Accept when:**
  - every record with a name has `nameKey`;
  - `nameStatus` counts per dataset appear in the dry-run output;
  - a test covers a placeholder, an unresolved key and a `,P` key.

### 2. Strip prose at publish

- **Change:** extend `redactStrings` in `scripts/publish-dump.mjs` to drop `description` (and
  long mission text) from missions, ships, starmap, commodities, manufacturers and refining.
  - Keep `descriptionKey`.
  - Stamp `_redacted: [fields]` in each file.
  - The local `data/` copy stays full.
- **Depends on:** 1.
- **Accept when:**
  - a test publishes a fixture and finds no `description` string in any published dataset;
  - the dry run shows the size drop;
  - the README's redaction paragraph states exactly what is removed.

### 7. Commodity derived flags and reverse links

- **Add to `resources[]`:**
  - `groupLabel`;
  - `tradable` (false for HeatPlaceholder and PowerPlaceholder);
  - `raw`, `rawSources[]` (exact name matches only), `volatile` and `densityIsDefault`
    (density 1.0 is the engine default, not a measurement);
  - `cargo[]{className, kind, capacityScu}`;
  - `configs[]` deduplicated from `damage[]`.
- **Port from:** `crates/starorg-gamedata/src/query/commodities.rs`.
- **Accept when:** tests cover a placeholder (not tradable), a raw ore with its refined
  counterpart, and a default density.

### 6. Quality steps and share estimates

- **Add:** `resources[].quality.steps[]{start, end, shown, shareEstimate}`, the same per
  `locationOverrides[]`, and the `qualityScale` effect per rock where it applies.
- **Port from:** `crates/starorg-gamedata/src/query/quality.rs`: clip bands to min-max, use the
  erf CDF, and use the half-step integer window.
- **Rules:**
  - omit `shareEstimate` when there is no stddev;
  - `_assumptions` states that sampling is assumed to be a truncated normal.
- **Accept when:**
  - the shares sum to 1 within 1e-6;
  - Aphorite reproduces the app's known example: a roll in 201-399 shows as 348.

### 10. Wikelo reward resolved at parse time

- **Add:**
  - `trades[].reward{item, amount, source: award | inferred}`, joining `contractAwards` in the
    domain and keeping `rewardInferred`;
  - `summary.rewardMismatch[]` wherever the inferred reward and the award disagree.
- **Port from:** `crates/starorg-gamedata/src/query/obtain.rs` (award first, inferred as the
  fallback) and `crates/starorg-gamedata/src/query/where_.rs`.

### 4a. Area place classification

- **Add:** `areas[].place{label, kind}`, `family` (v3, multi, classic), `isTest`,
  `placeSource: "heuristic"` and `summary.unplacedAreas[]`.
- **Port from:**
  - `crates/starorg-mcp/src/hints.rs` (`AREA_PLACES`, where the first prefix match wins, so
    order matters; `area_place_name`);
  - `crates/starorg-mcp/src/loot_tree.rs` (`place_of`, the misspelling word map,
    `PLACE_QUALIFIERS`, `ACRONYMS`);
  - `crates/starorg-gamedata/src/query/mined.rs` (`area_place`, the Rock Cracker rule, which is
    `curated`).
- **Patch notes:** list `unplacedAreas`, so a new CIG preset is flagged on the build it appears.

### 5. Mining: one ore key, totals per belt, hand spots

- **Add to `game-mining-spawns`:**
  - `oreKey`, normalised, so `MinableElement_FPS_Sadaryx`, `quantainium_raw`, `Raw_X` and
    `ResourceType.X` become one key;
  - `resource`, the ResourceType name;
  - a `place` label (Keeger Belt, Lagrange points, "<System> asteroid fields");
  - a fixed `system`;
  - `oreLocations[ore].belts[]`, summed per (location, rock);
  - `spots[]{area, rock, slotShare}` from loot-area slots.
- **Add to commodity resources:** a reverse `minedAs[]`.
- **Port from:** `crates/starorg-gamedata/src/query/mined.rs` (`ore_core`, `ores_for`, belt
  summing, spots) and `crates/starorg-mcp/src/hints.rs` (`belt_place`, `system_name`).
- **Rules:**
  - `spawnShare` is not comparable across mining kinds: document that and do not normalise it;
  - the last-word fallback in `ores_for` stays in queries, not in the dump.
- **Depends on:** 4a, for the spot place names.
- **Accept when:** Sadaryx appears with a spot (the Rock Cracker in Nyx) and no belt.

### 3. `game-item-obtain`: loot odds per item

- **New dataset, keyed by class:**
  - `eligibility{canLoot, reason, rarity}`;
  - `loot[]{area, config, kind: loot|fixed|loose, entity, table, slotShare, perRollEstimate,
    maxResults, scoreEstimate, contents, size, rarity}` for the top 12 areas, plus
    `areasTotal`;
  - `otherTables[]` (tables no container uses: enemies, bosses);
  - `contractAwards[]`, `wikelo[]` and `blueprints[]`;
  - `_assumptions`: an even pick inside a loot pool.
- **Port from:**
  - `crates/starorg-gamedata/src/query/obtain.rs` (`BulkObtain`, the memoised pool sizes,
    `rank_areas`);
  - `crates/starorg-mcp/src/sources.rs` (`loot_container` tag reads);
  - `crates/starorg-mcp/src/hints.rs` (the rarity fallback from the config name).

  TypeScript has only the per-item path (`src/query/obtain.ts`); port the bulk path, and memoise
  it, since Rust takes 0.6 s with rayon.
- **Depends on:** 4a.
- **Accept when:**
  - output for 20 sample items matches the app's WHERE TO GET data for the same build
    (`obtain-hints.json`, game-only parts) within rounding;
  - the parse-time increase is reported.

### 8. Generated-name fallback

- **Add:** `nameGenerated` and `nameRule: variant | base | description | readable`, on item-tags
  and search-index rows whose `nameStatus` is not `real`. Never overwrite `name`.
- **Port from:** `crates/starorg-core/src/enhance/names.rs` (`Resolver`, `readable`,
  `variant_base`, `maker_codes`, `HIDDEN_TYPES`).
  - Port only the strict rules (exactly one match).
  - Loose sibling guessing was measured wrong; do not port it.
  - The curated `missing-names.patch.toml` stays in the app.
- **Patch notes:** count the visible items with no real name per build (88 on LIVE 4.10).

### 9. Mission and quest item links

- **Add:**
  - `missions[].spawnsItems[]` (from `autoSpawnSettings.inventoryItems`);
  - `missions[].haulingItems[]` (from `haulingOrderContent`);
  - a reverse `usedByMissions[]`, capped with a total, or a small `game-quest-items` dataset
    that groups siblings sharing a description key.
- **Port from:**
  - `crates/starorg-data/src/gamedata/mod.rs` pass 5;
  - `crates/starorg-data/src/gamedata/missions.rs`;
  - `crates/starorg-core/src/model/quest.rs`.
- **Also:** resolve the `~mission(Contractor|...)` giver tokens where the contractor record is
  known.

### 4b. Area placement from object containers

- **Add:** `areas[].usedAt[]{location, system, spawnPoints}`, read with the existing
  `src/socpak.ts`, with `placeSource: "data"` wherever it confirms or replaces 4a.
- **Risk:** coverage of procedural spawns is unknown. Report how many areas it places and how
  many it misses.

## Keep in the app (do not dump)

- **UEX joins** (shops, kiosk prices, sell and buy, `ShopCategory`, stale flags): third-party,
  player-reported data that changes with fetch time, not with the build. Only `commodity_key`
  could ship, as a plain field.
- **The curated `missing-names.patch.toml`:** editorial content.
- **The WHERE TO GET hint text:** it is prose and depends on the locale.
- **UI shaping:** loot tree levels, `unique_labels`, the open budgets, the row caps and container
  label wording.
- **Fuzzy matching:** place-query tiers and the `ores_for` last-word fallback.
- **Per-build caches:** `obtain-hints.json` and the store layer.

## Follow-ups in starorg-strings once fields exist

Replace the app's own computation with dump fields, keeping the old code as a fallback for older
builds:

| Dump field | Replaces in the app |
|---|---|
| mining `system` and `place` (after 5) | `system_name` and `belt_place` in `crates/starorg-mcp/src/hints.rs` |
| `areas[].place` (after 4a) | `AREA_PLACES` |
| `blueprints[].rewardedBy` and `rewardPools` | the blueprint, contract and pool join used for hints |
| search-index `family` and `noise` | overlapping parts of `item_kind` in `loot_tree.rs` |
| `isTemplate` and `nameStatus` (after 1) | placeholder detection in `crates/starorg-mcp/src/loot.rs` `item_name` |
| `oreSpawns[].qualityScale` (already in the dump, not yet used) | adds a per-rock effective quality in `quality.rs` |

## Kickoff

A new session in `~/git/sc-gamedata` can start with:

> Implement `docs/plans/specs/dump-enhancements.md` (decisions are settled). Do items 1, 2, 7,
> 6 and 10 as separate branches with tests and a dry-run size report,
> each with its matching starorg-strings Rust parity change. Use workflows. Do not publish.

Flip `status` to `active` when work starts and to `done` when the last item merges.
