/**
 * Wikelo Emporium trades.
 *
 * Wikelo is internally "TheCollector", which is why no record path contains
 * "wikelo". A trade is a ContractTemplate whose objective handler carries a list
 * of `HaulingOrder_EntityClass` nodes: deliver this many of that entity.
 *
 * What you receive is *not* stored on the contract. The template carries only a
 * `rewardContribution` percentage, with the actual payout resolved elsewhere, so
 * the reward here is read from the record name (`TheCollector_Vehicles_Polaris`
 * gives up a Polaris) and marked as inferred rather than presented as fact.
 */

import { num, str, type GameRecord } from "../records.js";
import type { Domain } from "./types.js";

export interface HandIn {
  item: string;
  /** Amount required. min and max are equal on every trade seen so far. */
  amount?: number;
  maxAmount?: number;
}

export interface WikeloTrade {
  name: string;
  recordName: string;
  id: string;
  /** Read from the record name, not from a reward field. */
  rewardInferred?: string;
  category?: string;
  handIns: HandIn[];
}

/** Last path segment of an entityClass reference, minus the .json suffix. */
export function entityName(value: unknown): string | undefined {
  const path = str(value);
  if (!path) return undefined;
  const file = path.split("/").pop();
  return file ? file.replace(/\.json$/i, "") : undefined;
}

/** Collect every hauling order nested anywhere in a record. */
export function collectHaulingOrders(value: unknown, found: HandIn[] = []): HandIn[] {
  if (Array.isArray(value)) {
    for (const entry of value) collectHaulingOrders(entry, found);
    return found;
  }
  if (!value || typeof value !== "object") return found;

  const obj = value as Record<string, unknown>;
  if (obj._Type_ === "HaulingOrder_EntityClass") {
    const item = entityName(obj.entityClass);
    if (item) {
      const min = num(obj.minAmount);
      const max = num(obj.maxAmount);
      found.push({
        // maxAmount is sometimes 0 where minAmount is the real requirement.
        amount: min && min > 0 ? min : max,
        maxAmount: max && max !== min ? max : undefined,
        item,
      });
    }
  }

  for (const entry of Object.values(obj)) collectHaulingOrders(entry, found);
  return found;
}

/**
 * Split "TheCollector_Vehicles_Polaris" into its category and reward.
 * Returns undefined for names that carry neither.
 */
export function splitRecordName(shortName: string): { category?: string; reward?: string } {
  const parts = shortName.split("_");
  if (parts.length < 2 || parts[0].toLowerCase() !== "thecollector") return {};

  const rest = parts.slice(1);
  const KNOWN_CATEGORIES = ["vehicles", "forfavours", "smallitems", "ships", "intro"];
  const first = rest[0]?.toLowerCase();

  if (KNOWN_CATEGORIES.includes(first) && rest.length > 1) {
    return { category: rest[0], reward: rest.slice(1).join(" ") };
  }
  return { category: rest.length === 1 ? undefined : rest[0], reward: rest.join(" ") };
}

export function parseWikeloTrade(record: GameRecord): WikeloTrade | undefined {
  const handIns = collectHaulingOrders(record.value);
  if (handIns.length === 0) return undefined;

  const { category, reward } = splitRecordName(record.shortName);
  return {
    name: record.shortName,
    recordName: record.name,
    id: record.id,
    category,
    rewardInferred: reward,
    handIns: handIns.sort((a, b) => a.item.localeCompare(b.item)),
  };
}

export const wikeloDomain: Domain = {
  name: "game-wikelo-trades",
  description: "Wikelo Emporium (TheCollector) barter trades and what each one asks for.",
  filters: ["**/contracts/contracttemplates/**"],

  parse({ records }) {
    const collectorRecords = records.filter(
      (r) =>
        r.type === "ContractTemplate" &&
        `${r.name} ${r.recordPath}`.toLowerCase().includes("thecollector"),
    );

    const trades = collectorRecords
      .map(parseWikeloTrade)
      .filter((t): t is WikeloTrade => t !== undefined)
      .sort((a, b) => a.name.localeCompare(b.name));

    // What Wikelo asks for most, across every trade, is the useful roll-up.
    const demand: Record<string, number> = {};
    for (const trade of trades) {
      for (const handIn of trade.handIns) {
        demand[handIn.item] = (demand[handIn.item] ?? 0) + (handIn.amount ?? 0);
      }
    }

    return {
      trades,
      totalDemand: Object.fromEntries(Object.entries(demand).sort((a, b) => b[1] - a[1])),
      summary: {
        trades: trades.length,
        // Templates that carry no hauling orders are flow or intro scaffolding.
        collectorRecordsWithoutHandIns: collectorRecords.length - trades.length,
        distinctItemsRequested: Object.keys(demand).length,
      },
    };
  },
};
