---
title: game-shops (game-shops.json)
tags: [docs, datasets]
updated: 2026-11-25
---

# game-shops (game-shops.json)

The global rules that every shop and commodity kiosk applies. Prices are set per shop by the server and are not in the files (module doc, `shops.rs` line 4). What ships is the rulebook:
- sell-back percentages, with per-item-type overrides and two unnamed four-point curves;
- the supply and demand thresholds behind the kiosk labels;
- the auto-loading fee per box size;
- licence-tier price modifiers and the tutorial buy limit.

On LIVE 4.10.201.48085 it has 46 item-type modifiers, 6 supply and 6 demand levels, 10 box prices and 2 licensed-item modifiers. Published size: 3,950 bytes minified (`jq -c . | wc -c`). Publish strips nothing here, so the parse output and the published file are the same size. The `_schema` key (since tags 4.10.201.48085 / 5.0.202.35026, commit `b9e3700d0`) is not in that figure; with it the same command prints 3,975.

## Source

- Records (filter `**/globalshopparams/**`), one of each:
  - `libs/foundry/records/globalshopparams/globalshopsellingdata.json`: `GlobalShopSellingParams`.
  - `globalshopcommoditydata.json`: `GlobalShopCommodityParams`.
  - `globalshopbuyingdata.json`: `GlobalShopBuyingParams`.
  - `globalshopterminaldata.json`: `GlobalShopTerminalParams` (only `maxBuyingMultiplier` is read).
- Rust: [`crates/sc-gamedata/src/domains/shops.rs`](../../crates/sc-gamedata/src/domains/shops.rs).
  - `impl Domain` at line 170, filters at line 179, `parse` at 183.
  - `parse_selling` at 109, `parse_commodity_rules` at 159, `levels` at 133.
  - `box_sizes` is re-exported from [`cargo.rs`](../../crates/sc-gamedata/src/domains/cargo.rs) line 47. `entity_name` at 27 is the file stem of a `file://` reference.
- No other dataset is read.

## Shape

Top-level keys: `_source`, `_extracted`, `_build`, `selling{6}`, `commodities{5}`, `buying{3}`, `summary{itemTypeModifiers}`.

### selling

| Field | Type | Meaning | Units/range | Source |
|---|---|---|---|---|
| `matchPercentage` | number | Sell-back adjustment; what "match" refers to is not established | -20 (percent, by name) | `matchPercentage` |
| `noMatchPercentage` | number | | -50 | `noMatchPercentage` |
| `missionItemSellPriceReductionPercentage` | number | | 0 | same name |
| `maxInventoryCurve` | number[4] | Unnamed curve; meaning not established | `[0, 15, 30, 55]` | `maxInventoryCurve` |
| `wearCurve` | number[4] | Unnamed curve; meaning not established | `[20, 40, 60, 80]` | `wearCurve` |
| `itemTypeModifiers[]` | object[] | `{type, subType?, matchPercentage}`; `SubType: "UNDEFINED"` is dropped | 46 rows; -75 on 42, -95 on 3 (`SalvageModifier`, `FPS_Consumable/OxygenCap`, `Misc/Fuse`), -90 on 1 (`WeaponAttachment/Magazine`) | `itemTypeModifiers[]` (`Type`, `SubType`, `matchPercentage`) |

### commodities

| Field | Type | Meaning | Units/range | Source |
|---|---|---|---|---|
| `supplyLevels` | object | Threshold behind each supply label (code comment: percent) | `no` 0, `veryLow` 10, `low` 25, `medium` 50, `high` 75, `veryHigh` 95 | `<Label>SupplyLevel` fields, first letter lower-cased |
| `demandLevels` | object | Threshold behind each demand label | `no` 100, `veryLow` 95, `low` 75, `medium` 50, `high` 25, `veryHigh` 10 | `<Label>DemandLevel` |
| `autoLoadingBasePrice` | number | | 150 | `autoLoadingBasePrice` |
| `autoLoadingBoxPrices` | `{scu, value}[]` | aUEC per box, by box size (code comment) | 0.125-32 SCU; 0, 0, 0, 30, 55, 100, 190, 360, 495, 680 | `autoLoadingBoxSizePrices` |
| `maxKioskCargoGridDisplaySize` | number | | 1000 | `MaxKioskCargoGridDisplaySize` |

### buying

| Field | Type | Meaning | Units/range | Source |
|---|---|---|---|---|
| `tutorialItemBuyLimitPerHour` | number | | 5 | `GlobalShopBuyingParams` |
| `licensedItemModifiers[]` | object[] | `{item, tier, percentageModifier}` | 2: `carryable_1h_sq_darkwater_1_a` Gold 0, `carryable_1h_sq_darkwater_1_b` Platinum 100 | `LicensedItem` (file stem), `Type`, `PercentageModifier` |
| `maxBuyingMultiplier` | number | Most items a single purchase can multiply up to (code comment) | 10 | `GlobalShopTerminalParams.maxBuyingMultiplier` |

```json
{"selling":{"matchPercentage":-20,"noMatchPercentage":-50,"missionItemSellPriceReductionPercentage":0,
  "maxInventoryCurve":[0,15,30,55],"wearCurve":[20,40,60,80],
  "itemTypeModifiers":[{"type":"Cooler","matchPercentage":-75},{"type":"PowerPlant","subType":"Power","matchPercentage":-75},"…"]},
 "commodities":{"supplyLevels":{"no":0,"veryLow":10,"low":25,"medium":50,"high":75,"veryHigh":95},"…":"…"},
 "buying":{"tutorialItemBuyLimitPerHour":5,"licensedItemModifiers":[{"item":"carryable_1h_sq_darkwater_1_a","tier":"Gold","percentageModifier":0}],"maxBuyingMultiplier":10}}
```

## Derived, estimated and curated values

- Supply and demand labels are derived from the field names (`VeryLowSupplyLevel` → `veryLow`).
- Box sizes come from the fixed key table in `cargo.rs` (`BOX_SIZES`).
- `item` is the referenced file's stem, lower-case as the path spells it.
- Nothing is estimated, curated or heuristic. There is no `_assumptions`.

## Redaction

Nothing. `game-shops.json` is not in `PROSE_FIELDS`, and the file holds no prose.

## Consumers

- **starorg-codex** (branch `feat/dataset-reuse`): `crates/starorg-mcp/src/prebuilt.rs:181` needs top-level `selling`, `commodities`, `buying` and `summary`, schema 1, with no typed check. No typed field reader was found. The app's shop and kiosk prices come from UEX, which [dump-enhancements](../plans/specs/dump-enhancements.md) keeps out of the dumps ("Keep in the app").
- **sc-gamedata itself:** `src/query/shops.rs` and `src/query/sells.rs` answer shop queries from UEX rows. They do not read this file (no `game-shops` reference in `src/query/`).
- **SCMCP:** generic `src/localdata.ts` access only.
- No reads in snares, MCsim, Bindscape or sc-telemetry.
- Contracts to keep: the starorg `needs` keys and the general `_build` rule in [consumer-datasets](../plans/specs/consumer-datasets.md). `_schema: "game-shops@1"` (since tags 4.10.201.48085 / 5.0.202.35026), and `build.json` `extractor` plus `files[]` hashes (same tags).

## Derivable data

- **Fields the source carries but the dataset does not read:**
  - `GlobalShopCommodityParams.genericCrates`: the box entity per size used for generic cargo. Example: `oneSCU` → `entities/scitem/carryables/tractorbeamonly/carryable_tbo_fl_1scu_commodity_waste_waste.json`. Sizes 1-32 SCU are set, and the 1/8, 1/4 and 1/2 SCU entries are null.
  - `GlobalShopCommodityParams.transactionSupportedResourceContainerTypes`: `[{Type: "Container", SubTypes: ["Cargo"]}]`, the item port types a kiosk transacts with.
  - `GlobalShopCommodityParams.RMC_ResourceType` (`ResourceType.RMC`) and `RMC_SalvageCannisterEntity` (`grin_multitool_resource_salvage_repair_01`): how RMC is sold. The meaning of this pairing is not established.
  - `GlobalShopTerminalParams`: `displayedItemsPerPage` 50, `confirmationAutoCloseTime` 3.0, `inventoryQueryItemsType` (`Cargo`, `Misc`, `WeaponAttachment`, `Container`). Its `shopErrors` holds only localisation keys, so it stays out under the no-prose rule.
  - `LicensedItemModifier.DisableDuplicateBadgeCheck`: false on both.
- **Joins with other datasets:**
  - `selling.itemTypeModifiers` × `game-item-tags` `items[]{type, subType}` gives a sell-back percent per item. A modifier without `subType` matches any subtype. On LIVE that is 3,617 of 12,894 items. A shop-sell query (the starorg app, or SCMCP next to UEX prices) would use it. **Estimate:** whether `matchPercentage` is added to or replaces the base `matchPercentage`/`noMatchPercentage` is not stated.
  - `buying.licensedItemModifiers[].item` → `game-item-tags` `items[].className`, case-insensitively (`carryable_1h_sq_darkwater_1_a` → `Carryable_1H_SQ_Darkwater_1_a`, "CDF F8C Certification Pass (Gold)"). This gives display names and keys.
  - `commodities.autoLoadingBoxPrices` with [game-cargo](game-cargo.md) `loading.autoLoadingBoxLoadingTime` gives aUEC and seconds per box size side by side, e.g. a 32 SCU box costs 680 and loads in 18. **Estimate:** a total for a given hold (base price + per-box prices × box count) assumes a fee formula the records don't state.
  - `commodities.supplyLevels`/`demandLevels` with live kiosk inventory (UEX) would map a stock percentage to the game's label. The UEX side stays in the app.
- **Parse gaps:**
  - `itemTypeModifiers` lists `Char_Armor_Helmet/Helmet` twice (both -75). The dataset keeps both rows as the record does. A note in `summary` (`duplicateTypeModifiers`) would make it visible without dropping data.
  - `maxInventoryCurve` and `wearCurve` are kept as bare numbers. The records give no axis, so their meaning is not established and must stay so until a record names it.

Related: [dump-enhancements](../plans/specs/dump-enhancements.md) ("Keep in the app": UEX joins) and [tool-data-sync](../plans/specs/tool-data-sync.md).
