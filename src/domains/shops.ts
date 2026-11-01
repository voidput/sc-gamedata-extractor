/**
 * Shops: the global rules every shop and commodity kiosk applies.
 *
 * Prices themselves are not here: they are set per shop by the server and
 * change constantly (see the uex-* datasets). What ships is the rulebook around
 * them:
 *
 * - Selling: the price adjustment, in percent, when a shop buys an item back.
 *   `match` presumably applies when the shop stocks that item type and
 *   `noMatch` when it does not, with per-type overrides; the wear and
 *   max-inventory curves are four-point tables whose inputs are not named.
 * - Commodities: the thresholds, in percent, behind the supply and demand
 *   labels kiosks show, and the auto-loading fee per box size.
 * - Buying: per-item price modifiers by licence tier, and the tutorial cap.
 */

import { num, str, type GameRecord } from "../records.js";
import { boxSizes, type BoxSizeValue } from "./cargo.js";
import { entityName } from "./wikelo.js";
import type { Domain } from "./types.js";

export interface SellingRules {
  matchPercentage?: number;
  noMatchPercentage?: number;
  missionItemSellPriceReductionPercentage?: number;
  maxInventoryCurve?: number[];
  wearCurve?: number[];
  itemTypeModifiers: { type?: string; subType?: string; matchPercentage?: number }[];
}

export interface CommodityRules {
  /** Threshold in percent behind each supply label. */
  supplyLevels: Record<string, number>;
  /** Threshold in percent behind each demand label. */
  demandLevels: Record<string, number>;
  autoLoadingBasePrice?: number;
  /** aUEC per box, by box size in SCU. */
  autoLoadingBoxPrices?: BoxSizeValue[];
  maxKioskCargoGridDisplaySize?: number;
}

export interface BuyingRules {
  tutorialItemBuyLimitPerHour?: number;
  licensedItemModifiers: { item?: string; tier?: string; percentageModifier?: number }[];
  /** Most items a single purchase can multiply up to. */
  maxBuyingMultiplier?: number;
}

const numbers = (value: unknown): number[] | undefined =>
  Array.isArray(value) ? value.map(num).filter((n): n is number => n !== undefined) : undefined;

const objects = (value: unknown): Record<string, unknown>[] =>
  Array.isArray(value) ? (value.filter((x) => x && typeof x === "object") as Record<string, unknown>[]) : [];

export function parseSelling(record: GameRecord): SellingRules {
  const v = record.value;
  return {
    matchPercentage: num(v.matchPercentage),
    noMatchPercentage: num(v.noMatchPercentage),
    missionItemSellPriceReductionPercentage: num(v.missionItemSellPriceReductionPercentage),
    maxInventoryCurve: numbers(v.maxInventoryCurve),
    wearCurve: numbers(v.wearCurve),
    itemTypeModifiers: objects(v.itemTypeModifiers).map((m) => ({
      type: str(m.Type),
      subType: str(m.SubType) === "UNDEFINED" ? undefined : str(m.SubType),
      matchPercentage: num(m.matchPercentage),
    })),
  };
}

/** Levels named `VeryLowSupplyLevel` or `noSupplyLevel` -> `{ veryLow: 10, no: 0 }`. */
function levels(v: Record<string, unknown>, suffix: string): Record<string, number> {
  const out: Record<string, number> = {};
  for (const [key, value] of Object.entries(v)) {
    const match = key.match(new RegExp(`^(.+)${suffix}Level$`));
    const n = num(value);
    if (match && n !== undefined) out[match[1][0].toLowerCase() + match[1].slice(1)] = n;
  }
  return out;
}

export function parseCommodityRules(record: GameRecord): CommodityRules {
  const v = record.value;
  return {
    supplyLevels: levels(v, "Supply"),
    demandLevels: levels(v, "Demand"),
    autoLoadingBasePrice: num(v.autoLoadingBasePrice),
    autoLoadingBoxPrices: boxSizes(v.autoLoadingBoxSizePrices),
    maxKioskCargoGridDisplaySize: num(v.MaxKioskCargoGridDisplaySize),
  };
}

export const shopsDomain: Domain = {
  name: "game-shops",
  description: "Global shop rules: sell-back cuts, supply and demand bands, auto-loading fees.",
  filters: ["**/globalshopparams/**"],

  parse({ records }) {
    const byType = (type: string) => records.find((r) => r.type === type);
    const selling = byType("GlobalShopSellingParams");
    const commodity = byType("GlobalShopCommodityParams");
    const buying = byType("GlobalShopBuyingParams")?.value ?? {};
    const terminal = byType("GlobalShopTerminalParams")?.value ?? {};

    const buyingRules: BuyingRules = {
      tutorialItemBuyLimitPerHour: num(buying.tutorialItemBuyLimitPerHour),
      licensedItemModifiers: objects(buying.licensedItemModifiers).map((m) => ({
        item: entityName(m.LicensedItem),
        tier: str(m.Type),
        percentageModifier: num(m.PercentageModifier),
      })),
      maxBuyingMultiplier: num(terminal.maxBuyingMultiplier),
    };
    const sellingRules = selling ? parseSelling(selling) : undefined;

    return {
      selling: sellingRules,
      commodities: commodity ? parseCommodityRules(commodity) : undefined,
      buying: buyingRules,
      summary: { itemTypeModifiers: sellingRules?.itemTypeModifiers.length ?? 0 },
    };
  },
};
