/**
 * Wikelo Emporium trades.
 *
 * Wikelo is internally "TheCollector", which is why no record path contains
 * "wikelo". A trade is a ContractTemplate whose objective handler carries a list
 * of `HaulingOrder_EntityClass` nodes: deliver this many of that entity.
 *
 * What you receive is *not* stored on the template. It sits on the contract
 * generator (`contractgenerator/thecollector`), whose contracts each name the
 * template they run and their results: `ContractResult_Item` (an entity and an
 * amount, e.g. the Polaris) or weighted `ItemAwardEntityClass` choices. Trades
 * join their contracts on that template path and list `rewards` with
 * `source: "award"`. A trade no contract runs falls back to the reward read
 * from its record name (`TheCollector_Vehicles_Polaris` gives up a Polaris),
 * kept as `rewardInferred` and listed with `source: "inferred"`.
 */

import { num, refPath, str, type GameRecord } from "../records.js";
import type { Domain } from "./types.js";

export interface HandIn {
  item: string;
  /** Amount required. min and max are equal on every trade seen so far. */
  amount?: number;
  maxAmount?: number;
}

export interface WikeloReward {
  /** Entity class, or for an inferred reward the name fragment read from the record name. */
  item: string;
  amount?: number;
  /** Relative chance among a contract's weighted choices. */
  weighting?: number;
  source: "award" | "inferred";
}

export interface WikeloTrade {
  name: string;
  recordName: string;
  id: string;
  /** Read from the record name, not from a reward field. */
  rewardInferred?: string;
  category?: string;
  handIns: HandIn[];
  /** Generator contracts that run this template (debug names). */
  contracts?: string[];
  rewards?: WikeloReward[];
}

/** A generator contract: what template it runs and what it awards on success. */
export interface CollectorContract {
  name: string;
  template?: string;
  awards: Omit<WikeloReward, "source">[];
}

const entityOf = (ref: unknown) => refPath(ref)?.split("/").pop();

/** Results that pay out on success: `missionResults[0]` (or no list at all). */
const onSuccess = (result: Record<string, unknown>) =>
  !Array.isArray(result.missionResults) || result.missionResults[0] === true;

/** Item results under a contract's contractResults, weighted choices included. */
export function contractRewards(results: unknown): Omit<WikeloReward, "source">[] {
  const out: Omit<WikeloReward, "source">[] = [];
  const walk = (value: unknown, weighting?: number) => {
    if (Array.isArray(value)) return value.forEach((v) => walk(v, weighting));
    if (!value || typeof value !== "object") return;
    const obj = value as Record<string, unknown>;
    if (Array.isArray(obj.missionResults) && !onSuccess(obj)) return;
    if (obj._Type_ === "ContractResult_Item") {
      const item = entityOf(obj.entityClass);
      if (item) out.push({ item, amount: num(obj.amount), weighting });
      return;
    }
    if (obj._Type_ === "ItemAwardWeightings") {
      const w = num(obj.weighting);
      for (const v of Object.values(obj)) walk(v, w);
      return;
    }
    if (obj._Type_ === "ItemAwardEntityClass") {
      const item = entityOf(obj.entityClass);
      if (item) out.push({ item, amount: num(obj.amountToAward), weighting });
      return;
    }
    for (const v of Object.values(obj)) walk(v, weighting);
  };
  walk(results);
  return out;
}

/** Every contract of the collector's generator records. */
export function collectorContracts(records: GameRecord[]): CollectorContract[] {
  const out: CollectorContract[] = [];
  const walk = (value: unknown) => {
    if (Array.isArray(value)) return value.forEach(walk);
    if (!value || typeof value !== "object") return;
    const obj = value as Record<string, unknown>;
    if (obj._Type_ === "Contract") {
      const name = str(obj.debugName);
      if (name) out.push({ name, template: refPath(obj.template), awards: contractRewards(obj.contractResults) });
      return;
    }
    for (const v of Object.values(obj)) walk(v);
  };
  for (const record of records) walk(record.value);
  return out;
}

/** Lower-case letters and digits only, for loose name comparison. */
const squash = (s: string) => s.toLowerCase().replace(/[^a-z0-9]/g, "");

/**
 * Join a trade to the contracts running its template. Awards win; the reward
 * read from the record name is the fallback. Duplicate awards (two contracts on
 * one template paying the same) are listed once.
 */
export function withRewards(trade: WikeloTrade, templatePath: string | undefined, contracts: CollectorContract[]): WikeloTrade {
  const running = templatePath ? contracts.filter((c) => c.template === templatePath) : [];
  const seen = new Set<string>();
  const awards: WikeloReward[] = [];
  for (const c of running) {
    for (const a of c.awards) {
      const key = `${a.item}|${a.amount}|${a.weighting}`;
      if (seen.has(key)) continue;
      seen.add(key);
      awards.push({ ...a, source: "award" });
    }
  }
  const rewards: WikeloReward[] =
    awards.length > 0 ? awards : trade.rewardInferred ? [{ item: trade.rewardInferred, source: "inferred" }] : [];
  return {
    ...trade,
    contracts: running.length > 0 ? running.map((c) => c.name).sort() : undefined,
    rewards: rewards.length > 0 ? rewards : undefined,
  };
}

/** Categories whose record names spell out the reward; others use codenames (AD_HushNow). */
const NAMED_REWARD_CATEGORIES = new Set(["vehicles", "vehicle", "ships"]);

/**
 * A vehicle trade whose name implies one reward while its awards name another:
 * no word of the inferred name (squashed, over two letters) is in any awarded
 * entity class. Other categories name trades by codename and are not checked.
 */
export function rewardMismatch(trade: WikeloTrade): { trade: string; inferred: string; awarded: string[] } | undefined {
  const awarded = (trade.rewards ?? []).filter((r) => r.source === "award").map((r) => r.item);
  if (!trade.rewardInferred || awarded.length === 0) return undefined;
  if (!NAMED_REWARD_CATEGORIES.has((trade.category ?? "").toLowerCase())) return undefined;
  const words = trade.rewardInferred.split(/\s+/).map(squash).filter((w) => w.length > 2);
  const matches = awarded.some((item) => words.some((w) => squash(item).includes(w)));
  return matches ? undefined : { trade: trade.name, inferred: trade.rewardInferred, awarded };
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
  filters: ["**/contracts/contracttemplates/**", "**/contracts/contractgenerator/**"],

  parse({ records }) {
    const contracts = collectorContracts(records.filter((r) => /contractgenerator\/thecollector/i.test(r.recordPath)));
    const collectorRecords = records.filter(
      (r) =>
        r.type === "ContractTemplate" &&
        `${r.name} ${r.recordPath}`.toLowerCase().includes("thecollector"),
    );

    const trades = collectorRecords
      .map((r) => {
        const trade = parseWikeloTrade(r);
        return trade && withRewards(trade, refPath(r.recordPath), contracts);
      })
      .filter((t): t is WikeloTrade => t !== undefined)
      .sort((a, b) => a.name.localeCompare(b.name));
    const used = new Set(trades.flatMap((t) => t.contracts ?? []));

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
        withAwardedReward: trades.filter((t) => t.rewards?.some((r) => r.source === "award")).length,
        withInferredRewardOnly: trades.filter((t) => t.rewards?.every((r) => r.source === "inferred")).length,
        rewardMismatch: trades.map(rewardMismatch).filter((m) => m !== undefined),
        // Contracts on a shared template (itemresourcegathering_thecollector) set their own
        // hand-ins through overrides, so no template trade carries them.
        contractsWithoutTemplateTrade: contracts.filter((c) => !used.has(c.name)).map((c) => c.name).sort(),
      },
    };
  },
};
