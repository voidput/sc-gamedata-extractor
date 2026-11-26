---
title: Cross-dataset derivations
tags: [docs, plans, specs]
updated: 2026-11-25
status: active
---

# Cross-dataset derivations

New fields and datasets computed by joining datasets we already publish, plus the record fields
those joins need. The game data is CIG's: nothing here adds data the game doesn't ship.
Everything is either read from shipped records or computed from published datasets. Source:
the "Derivable data" sections of the [dataset notes](../../datasets/README.md), written and
fact-checked 2026-11-25 against LIVE 4.10.201.48085 (sc-gamedata `main` 8e19cca9b).

## Rules

- **Rust only** (`crates/sc-gamedata`), the same as every dataset since the switch-over
  ([rust-pipeline](rust-pipeline.md)).
- **Inputs are published datasets or shipped records.** A join reads the parsed datasets of the
  same extract: one build, one `_build`. A new record field is read from the record, never
  inferred.
- **Estimates say so:** an `Estimate` suffix and an `_assumptions` entry for anything the records
  don't state (for example even picks, the SCU→volume mapping, acceleration models).
- **No prose, no assets, no geometry.** Names and loc keys only. No logos, thumbnails or paths
  to art. No meshes or models ([tool-data-sync](tool-data-sync.md) Rules).
- **`_schema`:** adding an optional field doesn't bump the version. Renaming or retyping an
  existing field bumps the version, and the consumers in
  [consumer-datasets](consumer-datasets.md) "Contracts to keep" get told first.
- **Size:** measure with `make publish-dry` and list the growth per item. A derived field that
  only restates a join a consumer can do cheaply should justify its bytes.
- **Each item names its consumers.** Any item with no consumer waits.

## Items

### D1. Reverse links ("where it is used" / "where it comes from")

| New field | On | From | Consumers |
|---|---|---|---|
| `stockOn[]` (vehicle `recordName`s whose default loadout carries the item) | `game-ship-components` items | `game-ships` `loadout` | snares (QED and drive pickers), codex detail |
| `mountedOn[]` (ships), `firedBy[]` (weapons) | `game-weapons` guns and missiles, `game-ammo` | `game-ships` `loadout.weapons`, `game-weapons` ammo refs | codex, MCsim |
| `wornBy[]` (actor, best `chance`, as an estimate) | `game-item-obtain` items, as a new source next to `loot` | `game-npc-loadouts` `actors[].items[]` | codex "where to get", SCMCP |

- Evidence: 23 of 249 ammo are fired by no weapon. 90 of the 2,447 NPC gear classes have no
  `game-item-obtain` row at all.
- Accept when every reverse link round-trips (each `stockOn` ship lists that item), and the
  90 gear-only classes gain an obtain row.

**Built (2026-11-25, merged to main).** Built (Rust, branch feat/d1a-reverse-links): D1 reverse links as a derived on-disk pass (new crates/sc-gamedata/src/reverse_links.rs, 4 unit tests; `pub mod reverse_links;` in lib.rs; one call in bin/sc-gamedata.rs before item-obtain and snapshot, plus the same call in the dev harness bin/gamedata-port.rs). No domain code changed, no `_schema` bump, fields optional and omitted when empty, lists sorted + unique, deterministic across runs. Pass links a target only when its `_build` equals its input's `_build`; idempotent; files it rewrites are added to the snapshot list. New fields on LIVE 4.10.201.48085: `stockOn[]` on game-ship-components items = vehicle recordNames whose default loadout carries the item (1,384 items, 19,680 entries; batteries 0/6, selfDestruct 0/7, thrusters 0/1485 because game-ships loadout has no item refs for them yet); `mountedOn[]` on game-weapons = vehicle recordNames (138 items, 3,173 entries: shipWeapons 113/241, missiles 25/67, fpsWeapons 0/492); `firedBy[]` on game-ammo = weapon recordNames from `ammo.recordName` (226/249 ammo, 683 entries). 23 ammo fired by no game-weapons weapon confirmed (5 of them are countermeasure ammo fired by launchers in game-ship-components, outside firedBy scope). Loadout walk is generic (any string leaf except display-text keys), class match case-insensitive (loadouts name mounts in lower case); on LIVE every link comes from `item`, `rack` or `mount` keys. Review: independent re-implementation matched all 1,384 / 138 / 226 lists exactly (0 mismatches, both directions); 38 + 38 + 15 sampled links confirmed in raw 4.10.201.48085 ship / weapon / magazine records; 0 lowercase class collisions. Regression vs release data (jq -S, `_extracted` deleted): only game-ammo, game-ship-components, game-weapons differ; with the new key stripped textually each is byte-identical to baseline; all other game-*.json identical incl. game-search-index (vendored loader unaffected); none new or missing. Minified growth: game-ship-components 2,295,419 -> 3,304,237 (+1,008,818, +43.95%); game-weapons 968,352 -> 1,131,306 (+162,954, +16.83%); game-ammo 91,478 -> 130,212 (+38,734, +42.34%). Open decision: full recordNames per spec vs bare class names (components about +576 KB instead of +1.0 MB). cargo test -p sc-gamedata: 451 + 8 + 3 passed, 0 failed, 1 ignored. cargo clippy -p sc-gamedata --all-targets: 0 warnings. cargo fmt --check clean. Nothing committed; docs/ untouched.

### D2. Missions joined to reputation, starmap and law

Read these fields from `missionbroker` records, all present today but unread
([game-missions](../../datasets/game-missions.md)):

| Field | Present on |
|---|---|
| `reputationRequirements` (faction, scope, comparison, standing) | 1571 missions |
| `missionResultReputationRewards[]` | 1801 missions |
| `reputationPrerequisites.wantedLevel` | 2178 + 393 missions |
| `locationMissionAvailable` / `localityAvailable` | 1836 / 407 missions |
| `missionGiverRecord` | 1459 missions |
| `requiredMissions[]` / `associatedMissions[]` | 425 non-empty |
| `missionModule` | all |
| `partialRewardPayout` thresholds | 849 missions |

Then join them:
- `requires: [{faction, scope, standing → game-reputation standing}]`: the rank needed.
- `repReward: [{faction, scope, amount}]`. **Open:** the record doesn't say which of the five
  outcome lists is which. Keep the list index until a record states it.
- `offeredAt[]`: places from `game-starmap` by record path, plus their jurisdiction and system
  through `game-law`.
- `chain{requires[], associated[]}`.
- Wikelo: rep per trade from `ContractResult_LegacyReputation`
  ([game-wikelo-trades](../../datasets/game-wikelo-trades.md)).

Overlaps [dump-enhancements](dump-enhancements.md) item 9 (mission item links) and the giver
resolution there. Do both in one pass.

Consumers: codex (missions tab, MCP), SCMCP.

**Built (2026-11-25, merged to main).** `game-missions` and `game-wikelo-trades` gain optional fields only; no `_schema` bump. Counts are LIVE 4.10.201.48085 (2584 missions, 1978 live).

Per mission: `module` (`missionModule` without `libs/subsumption/missions/` and `.xml`, 2574), `giverRecord` (`MissionGiver` record name, 1459), `requiresSet` (1570), `repRewardsSet` (1801), `wantedLevel {min, max}` (2584), `offeredAt[]` (`StarMapObject` record names from `locationMissionAvailable`, 1836), `locality` (`MissionLocality` short name, 407), `chain {requires[], associated[]}` (mission record names, 423), `partialPayout` (849), `spawnsItems[] {item, quantity}` (135) and `haulingItems[] {resource, minScu, maxScu}` (853).

Two departures from the wording above. Reputation needs and rewards are indices into top-level `requiresSets[]` (169 sets of `{faction, scope, comparison, standing}`) and `repRewardsSets[]` (269 sets), because inline they cost about 870 KB against about 230 KB; `partialPayouts[]` (2 tables of `{minPercent, maxPercent, rewardMultiplier}`) works the same way. Hauling is resources, not items: `missionbroker` records have no `haulingOrderContent`, their orders are `HaulingOrder_Resource` (2010 orders on 853 missions). A `HaulingOrder_EntityClass` branch exists for `item`, but the type has no instance on LIVE, so its field name is unverified.

`repRewardsSets[]` follows Decision 1: 5 slots in record order, entries `{faction, scope, amount}`, with an `_assumptions` entry saying the outcome per slot is not stated. `standing` is spelled as `game-reputation.standings[].recordName`; all 67 used are present there. Only And and parentheses appear in `reputationRequirements`; any other operator warns. One mission (`pu_delivery_local_legal_outpost_pyro`) has a requirement with no terms, hence 1570 not 1571.

New top-level tables: `givers{}` (21 rows of `{name, nameKey, faction}`), `factions{}` (24 rows of `{name, nameKey}`), `places{}` (98 rows of `{name, system, jurisdiction | jurisdictionEstimate}`), `localities{}` (9, each a list of place record names), `usedByMissions{items, resources}` (11 items, 32 resources, each `{missions[≤20, live first], total}`), `summary.links` (per-join counts) and `_assumptions` (2 entries).

Giver names come only from the giver record's own `displayName`: 10 of 21 givers have one, 12 name a faction through their `reputation` field, 3 have neither (Ling Family Hauling, Red Wind Linehaul, UDM). The faction's name is not borrowed as the giver's name (BlacJac Bounty Department points at ArcCorp); a consumer wanting a fallback reads `givers[x].faction` → `factions{}`. Of the 346 live missions whose `giver` is still a `~mission(...)` token, 28 get a giver name and 97 get a faction; across all missions `summary.links` gives `giverNamed` 184, `giverFaction` 1290, `tokenGiverNamed` 41, `tokenGiverFaction` 108. The existing `giver`, `name`, `title` and `byGiver` are unchanged.

`places{}`: all 98 are in `game-starmap` with the same name and `system`; 31 state their jurisdiction and 67 carry `jurisdictionEstimate` from the nearest ancestor that states one (an `_assumptions` entry covers it); all jurisdiction names are in `game-law`.

`game-wikelo-trades`: `trades[].repRewards[] {faction, scope, amount, missionResults[5 flags]}` from each contract's `ContractResult_LegacyReputation`, deduplicated. 37 of 53 trades have it (amounts 10–1000, e.g. `FactionReputation_Wikelo` / `Wikelo` / 10). The other 24 of the 61 results belong to override contracts with no template trade. Adds `_assumptions` (1 entry) and `summary.withRepRewards` 37.

The missions domain now also reads `**/starmap/**`, `**/starmapobject/**`, `**/lawsystem/jurisdictions/**`, `**/reputation/**`, `**/factions/factionreputation/**`, `**/missiongiver/**` and `**/missiondata/pu_missionlocality/**`; wikelo also reads `**/reputation/**` and `**/factions/factionreputation/**`. `GOLDEN_EXTRACTION_FILTERS` gains `factions/factionreputation`, `missiondata/pu_missionlocality` and `missiongiver`.

Regression against the release baseline (`jq -S`, `_extracted` removed): only these two datasets differ, and with the new keys removed `game-missions` is identical. Sizes minified: `game-missions.json` 2,194,398 → 3,039,980 bytes (+845,582), `game-wikelo-trades.json` 40,618 → 45,738 (+5,120). Published total grows about 0.81 MB (missions 1.14 → 1.95 MB redacted, measured before the giver-name fix removed about 0.8 KB). Every new field was recomputed from the raw records for all 2584 missions and 53 trades with no mismatch. `cargo test -p sc-gamedata`: 453 passed, 0 failed. `cargo clippy -p sc-gamedata --all-targets`: 0 warnings.

Consumers: codex (missions tab, MCP), SCMCP.

### D3. A tag → items index, published once

- New dataset `game-tag-index`. For each tag path: the item classes carrying it, and the
  `game-loot` archetypes and secondary choices that select it.
- Today the obtain query rebuilds this on every call: SCMCP `obtain.ts`, the codex app, and
  `item_obtain.rs`.
- Same pass, fix two obtain parse gaps:
  - **V3 `entityClasses` selectors** are read as "anything" (`query/obtain.rs:176/196`): 64 loot
    objects name 26 classes. This is a correctness fix for `game-item-obtain`.
  - **Classes with no `game-item-tags` row:** 214 blueprint products and 55 NPC gear classes.
    Give them minimal identity rows, so that D1 `wornBy` and blueprint sources have somewhere
    to live. The codex doesn't need these (it said so 2026-11-25), so this is for SCMCP and
    completeness. **Open:** do we want it?
- V3 `optionalData` (`choiceLimit`, `chanceToExist`, `stackSize`, …), spawn-with rules (111
  `tagsToMatch` groups) and `specialEvents` probabilities could refine the odds. Each rests on
  an assumption about how the game applies it, so they come later, as estimates.

**Built (2026-11-25, merged to main).** LIVE 4.10.201.48085. No `_schema` bumps: fields and rows are added, nothing is renamed or retyped.

- **V3 `entityClasses` selectors** (`query/obtain.rs`): a selector naming classes now picks only those classes. 32 selectors (64 `entityClasses` objects) name 26 classes, all tagged CanGenerateAsLoot and all class-only (no tags beside them). In `game-item-obtain`, `withOtherTables` goes 3,856 → 1,724; from this fix alone `withLoot` goes 4,033 → 3,888 and `lootRows` 10,508 → 9,468. Example: `Carryable_1H_CY_Physical_Currency_Scrip_Council_1` is 0.08333 per roll (1 of weights 10+1+1) on the 9 CouncilScrip actor tables, where it was 0.00002138.
- **Identity rows in `game-item-tags`** (decision 2): 229 rows with empty `tags`, `size` and `grade` from `AttachDef`, and `source`: 214 `blueprint` (all really named) and 15 `npcGear` (all placeholder-named, `isTemplate`). All 229 match their item records on id, type, subType, size and grade. The 12,894 existing rows are byte-identical and in the same order; `summary.items` is 13,123 and `summary.identityRows{blueprint, npcGear}` is new. The other 40 of the 55 NPC gear classes have no item record anywhere in the DataCore (e.g. `basl_undersuit_01_01_01`, `brunt_outfit_04`, `klwe_pistol_ballistic_01`), so they get no row.
- **`game-tag-index`** (`game-tag-index@1`, 1,052,503 bytes minified): `classes[]` (the 12,894 tagged classes, in `game-item-tags` order) and `tags{<path>: {items[], matchCount, archetypes[], secondaryChoices[]}}`. `items` are indices into `classes` of items carrying the path itself; `matchCount` counts items carrying it or a path below it, which is how a loot selector matches. `archetypes` are the `game-loot` archetypes whose primary entries or secondary groups (weight above 0) name the path as a positive tag, with inline V3 table archetypes named `<table>:<entry>`; `secondaryChoices` are the single-layer choice records (the 15 multi-layer records only name those). Summary: 1,680 tags, 1,404 with items, 168 selected, 394 archetypes, 10 choice records. Exact matches only, no estimates.
- **`wornBy` in `game-item-obtain`** (D1): `wornBy[[actors[], chanceEstimate]]` with `wornByColumns` and a top-level `actors[]` (2,004 names, sorted); top 12 actors per item, `wornByTotal` when there are more. The chance is the actor's best `game-npc-loadouts` `items[].chance` for the item; the 1 unlootable actor is left out. 2,407 items, 4,720 rows, 14,286 actor references, 480 with `wornByTotal`. A sixth `_assumptions` entry covers it, including that a loadout row's `requires` condition is not carried (4 items on LIVE). A template item is now listed when an NPC wears it (44 on LIVE).
- **Resulting `game-item-obtain`:** items 5,930 → 6,194, `withBlueprints` 1,416 → 1,630, `withWikelo` 65 → 66, `bestFixed` 105 → 110, `bestLoose` 178 → 193, `withLoot` 3,904, `lootRows` 9,492. Of the 90 gear-only classes, 50 gain a row; the other 40 are the record-less ones above. 1,630 of 1,633 blueprint products carry `blueprints`; the 3 left are unnamed templates no NPC wears (`COOL_AEGS_S04_Javelin_SCItem`, `RADR_GNRP_S03_Idris_TEMP`, `RADR_RSI_S04_Polaris`).
- **Sizes (minified):** `game-item-obtain` 2,242,590 → 2,202,736 (−39,854); `game-item-tags` 5,558,826 → 5,629,790 (+70,964); `game-search-index` 869,407 → 881,819 (+12,412: the 229 rows, and `makers`/`types` indices shift); `game-tag-index` new, 1,052,503. All other datasets are identical to the published build.
- **Not applied:** V3 `optionalData` (`chanceToExist`, `choiceLimit`, `stackSize`), spawn-with rules and `specialEvents`.
- **Checks:** `cargo test -p sc-gamedata` 452 passed, 0 failed; `cargo clippy -p sc-gamedata --all-targets` 0 warnings; two full runs give byte-identical output apart from `_extracted`.

### D4. The resource → ore → refinery → blueprint chain

Per `game-commodities` resource:

| Field | Derived from |
|---|---|
| `minedAt[]` | `game-mining-spawns` belts and spots, via `minedAs` |
| `refinedBy[]` | `game-refining` processes, plus the raw quality a refinery receives, from `quality` |
| `usedByBlueprints` (count and ids) | `game-blueprints` `resourceUse`: 26 resources, e.g. Aslarite 872 |
| `inManifests[]` | `game-cargo` manifests: all 29 names resolve |
| `qualityOverrideBelts[]` | `quality.locationOverrides` matched to belts by system |

Also emit `descriptionKey` / `rawDescriptionKey` / `refinedDescriptionKey` and a per-process
`nameKey` in `game-refining`. Today the published file has no keys for its redacted prose, so
the codex's local `global.ini` fill can't work for it.
[dump-reuse](dump-reuse.md) decision 2 depends on this, so it is the most urgent part of D4.

Consumers: codex commodity detail and mining tab, SCMCP.

**Built (2026-11-25, merged to main).** Keys first: `game-refining` `processes[]` gain `nameKey` (from `processName`, in the string table's spelling) and `descriptionKey` (`<processName>_Desc`, built by convention because the record names no description key, and written only when the key resolves); `refinables[]` gain `rawDescriptionKey` / `refinedDescriptionKey`, copied from the raw and refined resource's `descriptionKey`. Ratings now read the record's `processDetail` key, with `<processName>_Details,P` as fallback; they are unchanged on LIVE. LIVE 4.10.201.48085: all 78 keys (9 + 9 + 30 + 30) resolve in the 91,358-string table, and each refinable key equals the `game-commodities` `descriptionKey` of the same resource. The chain is a post-run join (`resource_chain.rs`, same pattern as `game-item-obtain`): after every run `game-commodities.json` is rewritten in place from the `game-mining-spawns`, `game-refining`, `game-blueprints` and `game-cargo` files beside it. A join runs only when its input has the same `_build`; otherwise it is skipped with a warning and its fields are left out. Reruns drop and recompute every added field (a `cargo`-only rerun is byte-identical). Per resource, each field absent when empty:
- `minedAt[]`: 38 resources, 775 rows. 522 belt rows `{via:"belt", oreKey, location, place, placeSource, system?, kind?, spawnShare, rocks}`, summed per (oreKey, location, kind) from `ores[oreKey].belts` (all 567 source rows reached), and 253 spot rows `{via:"spot", oreKey, area, place?, placeSource?, slotShare}`. Belts first, by share descending then name. Copper and Vlk_Limpet have `minedAs` but no belts or spots.
- `refinedBy[]`: 28 refined resources, 30 rows `{raw, rawQuality?{tier, min, max, mean, stddev}, processesEstimate[]}`. `rawQuality` is the raw resource's base curve (location overrides not applied) and is absent on the 3 ConstructionMaterial raws. `processesEstimate` lists all 9 processes, because no record ties a method to a resource.
- `usedByBlueprints`: `{count, ids[]}`, 26 resources, 3,983 pairs, once per blueprint over the recipe cost tree including choice options (Aslarite 872). `count` is blueprints, not cost nodes, so it can be below `resourceUse` (Iron 239 vs 259).
- `inManifests[]`: 29 resources, 375 pairs of sorted distinct `game-cargo` manifest names.
- `qualityOverrideBelts[]`: 28 resources, 200 rows `{location, override}`: the `minedAt` belts whose system a `quality.locationOverrides[]` entry names ("Pyro System" / "Pyro"). The 2 "QV Breaker Station" overrides match nothing, and the 88 belt rows with no system (Lagrange points, asteroid clusters) are never matched.

`_assumptions` grows from 1 to 4 entries (minedAt `spawnShare` is the mining-spawns estimate, comparable only within one kind; `processesEstimate`; the override system-name match). No `_schema` bump: every field is optional and additive, and with them removed both files equal the baseline. Minified size: `game-refining` 20,493 → 25,044 (+4,551); `game-commodities` 270,703 → 571,821 (+301,118), of which `usedByBlueprints` is about 156 KB, `minedAt` 107 KB, `inManifests` 14 KB, `refinedBy` 11 KB, `qualityOverrideBelts` 10 KB. No new field is prose, so the published size grows by the same amount; `make publish-dry` was not run. For the codex refill: process `description` is the trimmed raw string, while refinable descriptions follow the commodities rule (markup stripped, "X description." stubs dropped). Tests 452 passed, clippy 0 warnings.

### D5. Per-ship totals from loadout × components

- **Parse fixes first:**
  - `parse_loadout` drops `SalvageHead` (not in `GUN_TYPES`, ships.rs:364), `SalvageModifier`,
    `SalvageInternalStorage`, `Battery` and bombs (only `Missile` is listed, ships.rs:556).
    Today `GAMA_Hyun`, with two salvage arms, gets `capabilities.salvage: false`, and
    `CRUS_Spirit_A1`'s bomb is missing.
  - `DynamicPowerPool.maxItemCount` entries are skipped, so `powerPools` is only `WeaponGun`.
- **Then per vehicle:**
  - `totals.coolantDraw`, `powerDraw` by state, and `signature{em, ir}` from the stock
    components;
  - `salvage{heads, modifiers, storageScu}` (Reclaimer 448 SCU);
  - `fireParts[]`, the item ids from `game-fire`.
  - **Estimate:** summing draws assumes every stock part is on, so the `_assumptions` entry
    says so.
- Give `game-salvage` the `vehicles[]` list sc-telemetry's `salvage_classes` already reads.
  `capabilities.salvage` then uses it too, with no key change for sc-telemetry.
- Consumers: sc-telemetry (salvage gating), MCsim (`build_fire.py` builds `fireParts` itself
  today), codex.

**Built (2026-11-25, merged to main).** `parse_loadout` now files `Bomb`, `Battery`, `SalvageHead`, `SalvageModifier` and `SalvageInternalStorage` items: `loadout.bombs[]` (`item`, `name`, `size`, `rack`, `damage` from `SCItemBombParams`, `count`), `loadout.batteries[]`, `loadout.salvageHeads[]` (`mount`), `loadout.salvageModifiers[]` (`head`) and `loadout.salvageStorage[]` (`scu` = the record's `ResourceContainer.capacity.standardCargoUnits`). `totals` gains `bombs`, `bombDamage`, `salvage{heads, modifiers, storageScu}` and `byStateEstimate{<state>: {powerDraw, coolantDraw, em, ir}}`; `missiles`, `missileDamage` and `totals.weapons` keep their meaning (bombs are counted apart). `capabilities.salvage` is now the role or a stock salvage head, and `game-salvage` gains `vehicles[]` (sorted record names, the shape sc-telemetry's `salvage_classes` reads) and `summary.vehicles`. Each vehicle also gets `dynamicPowerPools{itemType: maxItemCount}` (a sibling of `powerPools`, -1 kept as recorded, so `powerPools` stays pool sizes only) and `fireParts[]` (stock item ids that `game-fire` lists with nonzero health, over MCsim `build_fire.py`'s slots plus bombs). `game-ships` gets a top-level `_assumptions` (2 entries). Two deviations from the text above: Decision 3's `totals.byState` is published as `totals.byStateEstimate` (Rules' Estimate suffix), and a state other than `Online` gets a row only when its sums differ from Online's. LIVE 4.10.201.48085, 1104 vehicles: salvage lists and `totals.salvage` on 34 (Reclaimer 2 heads / 4 modifiers / 448 SCU, `GAMA_Hyun` 2 / 2 / 40); bombs on 15 (`CRUS_Spirit_A1`: 10 × `BOMB_S05_FSKI`, 46702 each, `bombDamage` 467020); batteries on 0 (no stock `Battery` item); `byStateEstimate` on 1088, `Online` only, because no ship item record has two states that differ in Power or Coolant consumption or EM / IR signature (a quantum drive's `Travelling` differs only in `QuantumFuel`); `coolantDraw` is 0 on 1085 and 87 on 3; `dynamicPowerPools` on 1104 (1064 carry the same six entries); `fireParts` on 1088 (8513 ids). `game-salvage.vehicles` has 34 names, equal to the vehicles with `loadout.salvageHeads`; `capabilities.salvage` is true on 35 (those plus `RSI_Salvation_Template`, by role), and `GAMA_Hyun` is the only vehicle whose capabilities changed (`salvage` false → true). Review recomputed every new field for all 1104 vehicles from the raw records and from `game-fire` / `game-ship-components`: 0 mismatches. Against the published baseline, 31 of 33 datasets are identical; `game-ships` grows 3926173 → 4447884 bytes minified (+521711, +13.3%: `fireParts` 266 KB, `dynamicPowerPools` 136 KB, `byStateEstimate` 93 KB, salvage ~22 KB, bombs ~4 KB) and `game-salvage` 406 → 2082. All new fields are optional: no `_schema` bump, no registry change, capability key names unchanged. `cargo test`: 448 + 8 + 3 passed, 0 failed; clippy `--all-targets`: 0 warnings.

### D6. Trip, quantum and fit estimates (all `Estimate`)

- `minimumQuantumStartEstimate` per body: from `game-quantum` `minimumAltitudeForQuantum` and
  `game-starmap` body `size`. "`size` is a radius" is snares' reading, not stated by the record.
- Route time and fuel: starmap positions plus drive `splineJumpParams` (read the full set;
  only `driveSpeed` is read today). The acceleration model is not stated, so this stays an
  estimate, or waits until the records state it.
- "What fits": a container's `maxPermittedItemSize` / capacity against an item's
  `inventoryOccupancyVolume`. The packing rule is not stated.
- Consumers: snares for the first two. The third has no consumer yet, so it waits under the
  rules above.

**Built (2026-11-25, merged to main).** D6 (estimates, snares only): `game-starmap` `locations[].minimumQuantumStartEstimate` = `size` + `QuantumDriveGlobalParams.minimumAltitudeForQuantum` (2000 on LIVE) on all 33 planets and moons (14 Planet, 19 Moon), appended as the last location field. A dataset `_assumptions` entry says that reading `size` as a radius is snares' reading, not something the record states, and that terrain and atmosphere are not counted; four bodies (Green, Nyx I/II/III) carry a 1,000,000 `size` that may be a default. `game-ship-components` `quantumDrives[].stats` now has the full gameplay set of `splineJumpParams` as raw `spline*` keys after `splineDriveSpeed`: cooldown, both stage accel rates, engage speed, spool, interdiction effect time, calibration rate and min/max, angle limits and delay. That is 12 keys on all 63 drives, all 756 values checked equal to the raw records; only cooldown and spool vary per drive, the other ten are the same on every drive. VFX and shader velocities are left out. The route time estimate is not built because the records don't state the stage hand-over, the braking model, or when the spline params apply; snares keeps its own mean-accel model. "What fits" waits for a consumer. Additive, no `_schema` bump (`game-starmap@1` unchanged); only these two datasets differ from the published baseline and both are identical to it once the new fields are removed. Growth: starmap +1,561 B minified (+0.6 KB gz), ship-components +24,616 B minified (+0.5 KB gz). 449 tests pass, clippy 0 warnings, fmt clean. Not committed.

### D7. Manufacturer resolution everywhere

- Read the `Code` field of `scitemmanufacturer` records. It differs from the short name on 38
  of them (`AEGS` → `AEG`) and is empty on 15.
- Emit `manufacturerCode` + `manufacturerName` on `game-ships`, `game-item-tags` and
  `game-search-index`.
- Normalise the 62 tag-derived search-index makers (`MICROTECH`, `Shubin`) to codes, or add a
  `makerNames` list.
- **Open:** which code is canonical per maker. Use `Code` where it decides; otherwise mark it as
  an estimate.
- This lets sc-telemetry drop its own `MANUFACTURERS` table.

### D8. More search-index entry kinds

- Add commodities and places to `game-search-index`: 958 visible starmap locations, 871 with a
  real `displayName`.
- Add a `nameKey` column, so clients can show names from the player's own `global.ini`.
- Same pass, two fixes: `_source` still names the deleted `src/search-index.ts`, and the file has
  no `_extracted`.
- Consumers: SCMCP, codex search.

**Built (2026-11-25, merged to main, covers D7 and D8).** Built (Rust, branch feat/d7d8-makers-search; worktree /home/zedwil/git/worktrees/sc-gamedata-d7d8-makers-search; not committed; docs/ untouched). Verified on LIVE 4.10.201.48085.

D7 — manufacturers
- game-manufacturers: `manufacturers[].recordCode` is the record's own `Code`, or `null` when empty. Set on 126 of 131 rows; differs from `code` on 23 (`AEGS`→`AEG`, `MISC`→`MIS`, `GREY`→`GLSN`, ...); null on `Paint_Test_01`, `Paint_Test_02`, `XIAN`, `QRT`, `STOR`. `code` stays the key (Decision 4).
- game-ships: `vehicles[].manufacturerCode` (the `byCode` key, e.g. `AEGS` for `aegs`) and `manufacturerName`, inserted right after `manufacturer`, matched to `byCode` ignoring case. 1103 of 1103 vehicles with a manufacturer resolve.
- game-item-tags: `items[].manufacturerCode` and `manufacturerName`, same placement. 6738 of 6921 items with a manufacturer resolve; the code differs from `manufacturer` on 41 (`CRLF_MEDGUN`→`CRLF_Medgun`, `GRIN_WEAPONS`→`GRIN_Weapons`). 183 stay unresolved because their code is not in `byCode`: VOLT 70, UNKN 52, LBCO 30, MICROTECH 14, UILO_C 6, UILO_C_2 2, PLACEHOLDER 2, SHOP_KLESCHER 2, TRAVELER 2, FTA 1, GRIMHEX 1, NYX_A 1.
- Implementation: `Vehicle` and `ItemTags` structs are untouched; the fields are inserted into the serialized rows by new helpers in `domains/manufacturers.rs` (`MANUFACTURER_FILTER`, `names_by_code`, `add_manufacturer_names`). Each domain gets one appended filter entry and two lines in `parse()`.

D7/D8 — game-search-index (still format 1, `game-search-index@1`)
- `_source` now names `crates/sc-gamedata/src/search_index.rs` and all five inputs; `_extracted` is added.
- `columns` gains a 7th column, `nameKey`: the string-table key when the name is a real localized string, else 0. Filled on 10,291 item rows and 1,099 ship rows.
- `commodities`: 204 rows, the resource types with a real `displayName` (3 placeholders skipped). Row is `[resourceName, displayName|0, 0, type=groupLabel, 0, noise, nameKey]`; `noise` is 1 on the 4 with `tradable: false`.
- `places`: 871 rows, the starmap locations not hidden in the starmap that have a real `displayName` (of 958 visible). Row is `[recordName without "StarMapObject.", displayName, 0, type, 0, 0, nameKey]`.
- `makerNames`: 155 entries, one per `makers` entry; the `byCode` name when the maker is a manufacturer code ignoring case, else `null`. 94 named.
- `makerNamesEstimate`: 36 makers not in `byCode` mapped to the one manufacturer whose display name equals them ignoring case, spaces and punctuation (`KastakArms`→`Kastak Arms`, `MICROTECH`→`microTech`). Explained in a new `_assumptions` entry. This is a name match only: an item's own manufacturer can differ from its tag (all 16 `Mirai`-tagged items are `SASU`). 25 makers stay unmatched.
- `types` gains 24 commodity and place types after the existing 134; old indexes do not move.
- `makers`, the first 134 `types`, and the first six columns of every `items`/`ships` row are unchanged from baseline. `build_search_index(items, ships, build)` keeps its signature; `Kind` has no new variants. `write_search_index` reads `game-manufacturers.json`, `game-commodities.json` and `game-starmap.json` as optional inputs.

Consumers
- SCMCP vendored TS loader (`loadSearchIndexEntries`) returns identical entries on new and baseline: 9,377 default, 13,998 with `all`.
- The starorg-strings port has no strict column or key checks.

Regression (`jq -S`, `_extracted` deleted, vs /home/zedwil/git/worktrees/sc-gamedata-release/data)
- 29 of 33 datasets identical; 4 changed, all additive only (deleting the new fields reproduces the baseline). No `_schema` bump.
- Minified bytes: game-item-tags 5,558,826 → 5,970,227 (+411,401, +7.4%); game-ships 3,926,213 → 4,000,239 (+74,026, +1.9%); game-manufacturers 60,821 → 63,373 (+2,552); game-search-index 869,407 → 1,401,663 (+532,256, +61%).
- `manufacturerCode` on item-tags accounts for about 172 KB of that and equals `manufacturer` on 6,697 of 6,738 rows; it could be limited to ships under the size rule.

Checks
- `cargo build --release -p sc-gamedata` and the `all` run are clean; output is identical across two runs.
- `cargo test -p sc-gamedata`: 451 + 8 + 3 passed, 0 failed, 1 ignored.
- `cargo clippy -p sc-gamedata --all-targets`: 0 warnings. `cargo fmt --check`: clean.

Files changed
- crates/sc-gamedata/src/domains/manufacturers.rs
- crates/sc-gamedata/src/domains/ships.rs
- crates/sc-gamedata/src/domains/item_tags.rs
- crates/sc-gamedata/src/search_index.rs
- crates/sc-gamedata/tests/pipeline.rs

## Order

1. **D4 keys** (small; unblocks the codex prose fill for refining).
2. **D5 parse fixes**, which also fix `capabilities.salvage`.
3. **D1.**
4. **D2**, together with dump-enhancements item 9.
5. **D3**, starting with the V3 selector fix.
6. **D7.**
7. **D8.**
8. **D6**, last: it is all estimates, and only snares consumes it.

## Decisions (2026-11-25)

1. **D2 reputation rewards:** keep the index. `repRewards` is published as a 5-slot list in the
   record's order, and an `_assumptions` entry says the outcome per slot is not stated. No
   labels are inferred. They get added if a later build or a record states them.
2. **D3 missing items: add minimal rows.** The ~269 classes missing from `game-item-tags` (214
   blueprint products, 55 NPC gear classes) get identity-only rows: class, name / `nameKey`,
   type, size and grade from `AttachDef`. Each carries `source: "blueprint" | "npcGear"`, so
   `game-item-obtain` can list their blueprint and NPC-gear sources.
3. **D5 totals: per power state.** `totals.byState: {Online: {powerDraw, coolantDraw, em, ir},
   Travelling: {...}, ...}` for each state the component records give. It is marked as an
   estimate (every stock part is assumed to be in that state).
4. **D7 manufacturer code: keep the short name, add `Code`.** `manufacturerCode` stays the short
   name every consumer keys on today (`AEGS`). The record's own `Code` is published beside it as
   `recordCode` (`AEG`, or `null` when empty). No ids change and there's no `_schema` bump.
