---
title: game-wikelo-trades (game-wikelo-trades.json)
tags: [docs, datasets]
updated: 2026-11-25
---

# game-wikelo-trades (game-wikelo-trades.json)

Wikelo Emporium barter trades. Internally Wikelo is "TheCollector": a trade is a `ContractTemplate` whose objective handler lists `HaulingOrder_EntityClass` hand-ins, joined to the collector's contract generator for what it pays out. On LIVE 4.10.201.48085 it holds 53 trades asking for 76 distinct items in total. The file is 40,625 bytes minified.

## Source

- DataCore, filters at `crates/sc-gamedata/src/domains/wikelo.rs:370`:
  - `ContractTemplate` records under `**/contracts/contracttemplates/**` whose record name or path contains `thecollector` (wikelo.rs:389).
  - The generator `contracts/contractgenerator/thecollector.json`: every nested `Contract` (`debugName`, `template`, `contractResults`), read by `collector_contracts` (wikelo.rs:160).
- Code: `collect_hauling_orders` (wikelo.rs:291), `split_record_name` (wikelo.rs:326), `parse_wikelo_trade` (wikelo.rs:341), `contract_rewards` (wikelo.rs:109), `with_rewards` (wikelo.rs:195), `reward_mismatch` (wikelo.rs:258), `Domain::parse` (wikelo.rs:377).
- Derives from no other dataset. [game-item-obtain](game-item-obtain.md) reads it (`crates/sc-gamedata/src/item_obtain.rs:502`).

## Shape

Top-level keys: `_source`, `_extracted`, `_build`, `trades[]`, `totalDemand{}`, `summary{}`. `_schema: "game-wikelo-trades@1"` follows `_build` (since tags 4.10.201.48085 / 5.0.202.35026).

`totalDemand`: `{ <item entity class>: sum of hand-in amount over all trades }`, largest first. 76 keys (`carryable_1h_cy_banu_favour_wikelo` 423, `carryable_1h_cy_physical_currency_scrip_merc_1` 190, `harvestable_ore_1h_jacliumore` 170 …).

`summary` (LIVE):

| Key | Value | Meaning |
|---|---|---|
| `trades` | 53 | Templates with at least one hand-in |
| `collectorRecordsWithoutHandIns` | 16 | Collector templates without hauling orders ("flow or intro scaffolding", wikelo.rs:432) |
| `distinctItemsRequested` | 76 | Keys of `totalDemand` |
| `withAwardedReward` | 51 | Trades with a reward read from a contract |
| `withInferredRewardOnly` | 2 | Trades whose only reward is the record-name guess |
| `rewardMismatch[]` | 1 | `{trade, inferred, awarded[]}`: `TheCollector_Vehicles_Sprirt_C1` (the record name misspells "Spirit"), awarded `crus_spirit_c1_civilian` |
| `contractsWithoutTemplateTrade[]` | 37 names | Generator contracts whose template is no trade here (see Parse gaps) |

### `trades[]`

| Field | Type | Meaning | Units/range | Source |
|---|---|---|---|---|
| `name` | string | Template short name | 53 | record |
| `recordName` | string | `ContractTemplate.<short>` | 53 | record |
| `id` | string | Record GUID | 53 | `_RecordId_` |
| `category` | string? | Second `_`-part of a `TheCollector_…` name (`Vehicles` 25, `GG` 6, `Recipie` 6, `AD2` 3 …); absent on 6 | | record name |
| `rewardInferred` | string? | Rest of the record name (`Polaris`), the fallback reward | on 49 | record name (heuristic) |
| `handIns[]` | `{amount?, maxAmount?, item}` | What to deliver | 235 hand-ins | `HaulingOrder_EntityClass` nodes |
| `handIns[].item` | string | Entity class file stem | | `entityClass` |
| `handIns[].amount` | number? | `minAmount` when > 0, else `maxAmount` | count | `minAmount`/`maxAmount` |
| `handIns[].maxAmount` | number? | `maxAmount` when non-zero and different from `minAmount` | count; on none of the 235 hand-ins on LIVE | `maxAmount` |
| `contracts` | string[]? | `debugName`s of generator contracts running this template | on 52 | generator `Contract.template` |
| `rewards[]` | `{item, amount?, weighting?, source}` | What the trade pays | | see below |
| `rewards[].item` | string | Entity class file stem | | `entityClass` |
| `rewards[].amount` | number? | `amount` / `amountToAward` | count; on 97 of 99 rows | `ContractResult_Item` / `ItemAwardEntityClass` |
| `rewards[].weighting` | number? | Weight of an `ItemAwardWeightings` choice | on 68 of 99 rows | `weighting` |
| `rewards[].source` | string | `award` (97 rows) from the contract, `inferred` (2 rows) from the record name | | domain |

Example (real):

```json
{"name":"ItemResourceGathering_Recipe_TheCollector_Lightnin_KopSkull",
 "recordName":"ContractTemplate.ItemResourceGathering_Recipe_TheCollector_Lightnin_KopSkull",
 "id":"f6e9c747-40a9-4e78-814d-8bf4b7dc48dc",
 "handIns":[{"amount":10,"item":"harvestable_mineral_1h_carinite"},{"amount":20,"item":"harvestable_ore_1h_jacliumore"},
            {"amount":15,"item":"harvestable_ore_1h_saldyniumore"},{"amount":20,"item":"harvestable_trophy_1h_kopionhorn_tundra"},
            {"amount":1,"item":"volt_rifle_energy_01"}],
 "contracts":["TheCollector_OrbVolt_KopSkull"],
 "rewards":[{"item":"volt_rifle_energy_01_collector02","amount":1,"source":"award"},
            {"item":"volt_rifle_energy_01_mag","amount":20,"source":"award"}]}
```

## Derived, estimated and curated values

- `category` and `rewardInferred` are parsed from the record name (`split_record_name`), not read from a field. `rewards[]` with `source: "inferred"` comes from that name and names no entity class. Only 2 trades rely on it.
- `rewardMismatch` is a check by word overlap between the name and the awarded class (wikelo.rs:258).
- Only success results count: a result whose `missionResults[0]` is not `true` is skipped (wikelo.rs:118).
- `totalDemand` adds `amount` and ignores `maxAmount`.

## Redaction

Nothing (no `PROSE_FIELDS` entry).

## Consumers

- **starorg-codex** (`feat/dataset-reuse`): `crates/starorg-mcp/src/prebuilt.rs:186` needs `trades`, `totalDemand`, `summary` and type-checks `trades` as `WikeloTradeLike` (`prebuilt.rs:134`); `crates/starorg-mcp/src/tools.rs:532` loads `trades` for reward lookups.
- **SCMCP**: `src/itemfinder.ts:69`/`:150` loads `trades`; `src/vendor/sc-gamedata/obtain.ts:53` reads `name` and `rewardInferred` (`:552` matches the squashed `rewardInferred` to an item name); `src/versions.ts:44` diffs `trades`.
- **sc-gamedata** `crates/sc-gamedata/src/item_obtain.rs:242`: `rewards[].item`, `amount`, `source` and the trade `name` for `game-item-obtain` `wikelo[]` rows.
- Contracts: `_build` equals the tag; `_schema` stem `game-wikelo-trades` once added ([consumer-datasets](../plans/specs/consumer-datasets.md)).

## Derivable data

What the collector generator (`~/git/sc-gamedata/extracted-data/libs/foundry/records/contracts/contractgenerator/thecollector.json`, 89 `Contract`s; that file is dated 2026-11-14, so re-check it against a fresh extract) carries beyond what the domain reads:

- **Hand-ins set by override**: 36 contracts (33 run the shared template `itemresourcegathering_thecollector`, 3 run `thecollector_forfavours_caranite`/`_scripcouncil`/`_scripmerc`) set their hand-ins in `paramOverrides.propertyOverrides[].value.haulingOrderContent[]` as `HaulingOrderContent_EntityClass` (a different `_Type_` from the `HaulingOrder_EntityClass` the domain looks for): 150 hand-ins over 73 distinct items. Example: `TheCollector_Vehicle_Super_Idris` asks 50 × `carryable_1h_cy_banu_favour_wikelo`, 50 × `harvestable_mineral_1h_carinite`, 30 × `carryable_2h_fl_vlk_pearl_irradiated_super_01` … and awards `aegs_idris_p_collector_military`. They are 36 of the 37 `contractsWithoutTemplateTrade`; the 37th, `TheCollector_Favours_PolarisParts`, carries no hand-in override. Reading them would add most vehicle trades with their real costs, and fix `totalDemand`.
- **Reputation per trade**: 61 `ContractResult_LegacyReputation` results with `SReputationAmountParams` (scope `reputationscope_wikelo`, reward e.g. `wikelo_25` = 25). The amounts live in [game-reputation](game-reputation.md)'s unread reward records.
- **Blueprint rewards**: 15 `BlueprintRewards` results (`chance`, `blueprintPool`, e.g. `crafting/blueprintrewards/collectorwikelo/bp_reward_vgl_flightsuit_helmet_01_02_01.json`). [game-blueprints](game-blueprints.md) may already cover them; not checked here.
- **Prerequisites**: `additionalPrerequisites[]` with `ContractPrerequisite_CompletedContractTags` (6) and `ContractPrerequisite_Reputation` (2): which trades unlock after others or at a Wikelo rank.
- **Availability**: `notForRelease`/`workInProgress` (18 contracts flagged; names like `TheCollector_F55(DO_NOT_USE_NOW_LOOT)`), `contractLifeTime{instanceLifeTime, instanceLifeTimeVariation}` (10 and 5 in the first contract; unit not stated), `required_active_scenarios`, `generationParams`.

Values computable by combining datasets:

- Display names for every `item` and reward by joining entity classes with [game-item-tags](game-item-tags.md) / [game-ships](game-ships.md). Consumer: SCMCP, starorg-codex.
- Where each requested item comes from (loot, shops, mining) by joining `handIns[].item` with [game-item-obtain](game-item-obtain.md), giving a cost-to-complete per trade. Estimate if prices or drop odds are combined.
- Reverse link item → trades that ask for it (the per-trade part of `totalDemand`).

Parse gaps:

- The 36 override contracts above: their hand-ins are missing, and `totalDemand` undercounts (the Idris contract alone asks for 50 more `carryable_1h_cy_banu_favour_wikelo` beyond the 423 counted).
- `contracts` keeps `(DO_NOT_USE_NOW_LOOT)` names without a flag, so a disabled contract looks live; the generator's `notForRelease`/`workInProgress` are not read.
- `category` mixes spellings and codenames (`Vehicles`/`Vehicle`, `Recipie`, `AD`/`AD2`), since it is taken from record names.

Related specs: [dump-enhancements](../plans/specs/dump-enhancements.md) (item 10, built), [dump-reuse](../plans/specs/dump-reuse.md).
