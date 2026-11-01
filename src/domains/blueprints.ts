/**
 * Crafting blueprints.
 *
 * A CraftingBlueprintRecord holds one or more tiers, and each tier's recipe has
 * a cost tree rather than a flat ingredient list. Three node types matter:
 *
 * - `CraftingCost_Resource` — a refined resource, quantity in SCU.
 * - `CraftingCost_Item`     — a physical item, quantity as a count.
 * - `CraftingCost_Select`   — "choose `count` of these `options`", and options
 *                             may themselves be Select nodes.
 *
 * The tree is flattened to leaves while keeping the choice structure, because a
 * recipe that lets you pick two of five aspects is not the same as one needing
 * all five.
 *
 * `blueprintName` is a placeholder on nearly every record, so the display name
 * comes from the item the blueprint produces.
 *
 * Blueprints are earned from missions: a contract's results may include
 * `BlueprintRewards`, which point at a BlueprintPoolRecord of weighted
 * blueprints. The contract is the nearest enclosing object with a debugName
 * (a Contract or CareerContract), its handler the generator group above it.
 */

import { isPlaceholderName, localize, type Localization } from "../localization.js";
import { at, compareNames, indexByRecordPath, num, refPath, refShortName, str, type GameRecord } from "../records.js";
import { itemIdentity } from "./item-common.js";
import type { Domain } from "./types.js";

export interface ResourceCost {
  kind: "resource";
  resource: string;
  /** Standard cargo units. */
  scu?: number;
  minQuality?: number;
}

export interface ItemCost {
  kind: "item";
  item: string;
  quantity?: number;
  minQuality?: number;
}

export interface ChoiceCost {
  kind: "choice";
  /** How many of `options` must be supplied. */
  count?: number;
  label?: string;
  options: Cost[];
}

export type Cost = ResourceCost | ItemCost | ChoiceCost;

/** The last path segment of an entityClass reference, which names the item. */
export function entityClassName(value: unknown): string | undefined {
  const path = str(value);
  if (!path) return undefined;
  const file = path.split("/").pop();
  return file ? file.replace(/\.json$/i, "") : undefined;
}

export function parseCost(node: unknown, localization: Localization): Cost | undefined {
  if (!node || typeof node !== "object") return undefined;
  const obj = node as Record<string, unknown>;

  switch (obj._Type_) {
    case "CraftingCost_Resource": {
      const resource = refShortName(obj.resource);
      if (!resource) return undefined;
      return {
        kind: "resource",
        resource,
        scu: num(at(obj, "quantity.standardCargoUnits")),
        minQuality: num(obj.minQuality) || undefined,
      };
    }

    case "CraftingCost_Item": {
      const item = entityClassName(obj.entityClass);
      if (!item) return undefined;
      return {
        kind: "item",
        item,
        quantity: num(obj.quantity),
        minQuality: num(obj.minQuality) || undefined,
      };
    }

    case "CraftingCost_Select": {
      const options = Array.isArray(obj.options)
        ? obj.options
            .map((option) => parseCost(option, localization))
            .filter((c): c is Cost => c !== undefined)
        : [];
      if (options.length === 0) return undefined;

      const label = localize(localization, at(obj, "nameInfo.displayName"));
      const debug = str(at(obj, "nameInfo.debugName"));

      return {
        kind: "choice",
        count: num(obj.count),
        // displayName is usually a placeholder; debugName ("ASPECTS") is real.
        label: isPlaceholderName(label) ? debug : label,
        options,
      };
    }

    default:
      return undefined;
  }
}

/** Total seconds from the game's partitioned time struct. */
export function timeSeconds(value: unknown): number | undefined {
  if (!value || typeof value !== "object") return undefined;
  const t = value as Record<string, unknown>;
  const total =
    (num(t.days) ?? 0) * 86400 +
    (num(t.hours) ?? 0) * 3600 +
    (num(t.minutes) ?? 0) * 60 +
    (num(t.seconds) ?? 0);
  return total > 0 ? Math.round(total * 100) / 100 : undefined;
}

export interface BlueprintTier {
  craftTimeSeconds?: number;
  costs: Cost[];
}

export interface Blueprint {
  name: string;
  recordName: string;
  id: string;
  category?: string;
  /** What the blueprint produces. */
  produces?: string;
  /** The produced item's display name. */
  displayName?: string;
  tiers: BlueprintTier[];
  /** Missions that can award this blueprint. */
  rewardedBy?: BlueprintReward[];
}

export interface BlueprintReward {
  /** The mission's title, or its debug name when it has none. */
  mission: string;
  contract: string;
  /** Generator handler the contract belongs to, e.g. "TheCollector_Small_Items". */
  generator?: string;
  pool: string;
  /** Chance the pool is awarded on completion. */
  chance?: number;
  /** This blueprint's weight within the pool, and the pool's total. */
  weight?: number;
  poolWeight?: number;
}

export interface RewardPool {
  name: string;
  blueprints: { blueprint: string; weight?: number }[];
}

/** Blueprint pools by record path, with each entry's blueprint path. */
function parsePools(records: GameRecord[]): Map<string, { record: GameRecord; entries: { path: string; weight?: number }[] }> {
  const pools = new Map<string, { record: GameRecord; entries: { path: string; weight?: number }[] }>();
  for (const record of records) {
    if (record.type !== "BlueprintPoolRecord") continue;
    const list = Array.isArray(record.value.blueprintRewards) ? record.value.blueprintRewards : [];
    const entries = list
      .filter((e): e is Record<string, unknown> => !!e && typeof e === "object")
      .map((e) => ({ path: refPath(e.blueprintRecord) ?? "", weight: num(e.weight) }))
      .filter((e) => e.path);
    const path = refPath(record.recordPath);
    if (path) pools.set(path, { record, entries });
  }
  return pools;
}

/** Every BlueprintRewards in the contract generators, with the contract it sits on. */
export function collectBlueprintRewards(
  records: GameRecord[],
  localization: Localization = new Map(),
): { pool: string; chance?: number; contract: string; mission: string; generator?: string }[] {
  const found: { pool: string; chance?: number; contract: string; mission: string; generator?: string }[] = [];
  const titleOf = (contract: Record<string, unknown>) => {
    const params = at(contract, "paramOverrides.stringParamOverrides");
    const title = Array.isArray(params)
      ? (params as Record<string, unknown>[]).find((p) => p?.param === "Title")?.value
      : undefined;
    const text = localize(localization, title);
    return text && !isPlaceholderName(text) ? text : undefined;
  };
  const walk = (value: unknown, named: Record<string, unknown>[]) => {
    if (Array.isArray(value)) {
      for (const entry of value) walk(entry, named);
      return;
    }
    if (!value || typeof value !== "object") return;
    const obj = value as Record<string, unknown>;
    const here = typeof obj.debugName === "string" ? [...named, obj] : named;
    if (obj._Type_ === "BlueprintRewards") {
      const contract = here[here.length - 1];
      const pool = refPath(obj.blueprintPool);
      if (contract && pool) {
        const debugName = String(contract.debugName);
        found.push({
          pool,
          chance: num(obj.chance),
          contract: debugName,
          mission: titleOf(contract) ?? debugName,
          generator: here.length > 1 ? String(here[here.length - 2].debugName) : undefined,
        });
      }
    }
    for (const entry of Object.values(obj)) walk(entry, here);
  };
  for (const record of records) {
    if (record.recordPath.includes("/contracts/")) walk(record.value, []);
  }
  return found;
}

export function parseBlueprint(
  record: GameRecord,
  localization: Localization,
): Blueprint | undefined {
  const blueprint = record.value.blueprint as Record<string, unknown> | undefined;
  if (!blueprint) return undefined;

  const tiers: BlueprintTier[] = [];
  const rawTiers = Array.isArray(blueprint.tiers) ? blueprint.tiers : [];

  for (const tier of rawTiers) {
    const costsNode = at(tier, "recipe.costs");
    if (!costsNode) continue;

    const mandatory = parseCost(at(costsNode, "mandatoryCost"), localization);
    tiers.push({
      craftTimeSeconds: timeSeconds(at(costsNode, "craftTime")),
      // A single Select at the root is the recipe itself; unwrap it so the tier
      // reads as a list of requirements rather than one nested choice.
      costs: mandatory
        ? mandatory.kind === "choice" && mandatory.count === undefined
          ? mandatory.options
          : [mandatory]
        : [],
    });
  }

  // Dismantle records share the type but craft nothing; they have no tiers.
  if (tiers.length === 0) return undefined;

  const displayName = localize(localization, blueprint.blueprintName);
  const produces = entityClassName(at(blueprint, "processSpecificData.entityClass"));

  return {
    name: isPlaceholderName(displayName)
      ? (produces ?? record.shortName)
      : (displayName as string),
    recordName: record.name,
    id: record.id,
    category: refShortName(blueprint.category),
    produces,
    tiers,
  };
}

export const blueprintsDomain: Domain = {
  name: "game-blueprints",
  description: "Crafting blueprints with their recipe costs, craft times, item names and the missions that award them.",
  filters: [
    "**/crafting/**",
    "**/contracts/contractgenerator/**",
    "**/contracts/contractscenarios/**",
    // What blueprints produce, for display names.
    "**/entities/scitem/ships/**",
    "**/entities/scitem/weapons/**",
    "**/entities/scitem/characters/human/**",
    "**/entities/scitem/carryables/**",
  ],
  needsLocalization: true,

  parse({ records, localization }) {
    const byPath = indexByRecordPath(records);
    const pools = parsePools(records);

    // Blueprint path -> the missions that can award it.
    const rewards = new Map<string, BlueprintReward[]>();
    for (const reward of collectBlueprintRewards(records, localization)) {
      const pool = pools.get(reward.pool);
      if (!pool) continue;
      const poolWeight = pool.entries.reduce((sum, e) => sum + (e.weight ?? 0), 0);
      for (const entry of pool.entries) {
        const list = rewards.get(entry.path) ?? [];
        list.push({
          mission: reward.mission,
          contract: reward.contract,
          generator: reward.generator,
          pool: pool.record.shortName,
          chance: reward.chance,
          weight: entry.weight,
          poolWeight: poolWeight || undefined,
        });
        rewards.set(entry.path, list);
      }
    }

    const blueprints: Blueprint[] = [];
    for (const record of records) {
      if (record.type !== "CraftingBlueprintRecord") continue;
      const blueprint = parseBlueprint(record, localization);
      if (!blueprint) continue;
      const product = byPath.get(refPath(at(record.value, "blueprint.processSpecificData.entityClass")) ?? "");
      const identity = product ? itemIdentity(product, localization) : undefined;
      if (identity && !identity.isTemplate) {
        blueprint.displayName = identity.name;
        if (blueprint.name === blueprint.produces) blueprint.name = identity.name;
      }
      const awarded = rewards.get(refPath(record.recordPath) ?? "");
      if (awarded) {
        // One row per contract and pool; repeated pool entries are the same reward.
        const unique = new Map(awarded.map((a) => [`${a.contract}|${a.pool}`, a]));
        blueprint.rewardedBy = [...unique.values()].sort((a, b) => compareNames(a.mission, b.mission));
      }
      blueprints.push(blueprint);
    }
    blueprints.sort((a, b) => compareNames(a.name, b.name));

    const rewardPools: RewardPool[] = [...pools.values()]
      .map(({ record, entries }) => ({
        name: record.shortName,
        blueprints: entries.map((e) => ({ blueprint: byPath.get(e.path)?.shortName ?? e.path.split("/").pop()!, weight: e.weight })),
      }))
      .sort((a, b) => compareNames(a.name, b.name));

    // Which resources appear across recipes, and in how many, answers "what is
    // worth mining" without walking every blueprint.
    const resourceUse: Record<string, number> = {};
    const countResources = (costs: Cost[]) => {
      for (const cost of costs) {
        if (cost.kind === "resource") resourceUse[cost.resource] = (resourceUse[cost.resource] ?? 0) + 1;
        else if (cost.kind === "choice") countResources(cost.options);
      }
    };
    for (const blueprint of blueprints) {
      for (const tier of blueprint.tiers) countResources(tier.costs);
    }

    const byCategory: Record<string, number> = {};
    for (const blueprint of blueprints) {
      if (blueprint.category) byCategory[blueprint.category] = (byCategory[blueprint.category] ?? 0) + 1;
    }

    return {
      blueprints,
      rewardPools,
      resourceUse: Object.fromEntries(
        Object.entries(resourceUse).sort((a, b) => b[1] - a[1]),
      ),
      byCategory,
      summary: {
        blueprints: blueprints.length,
        distinctResources: Object.keys(resourceUse).length,
        categories: Object.keys(byCategory).length,
        named: blueprints.filter((b) => b.displayName).length,
        rewarded: blueprints.filter((b) => b.rewardedBy).length,
        rewardPools: rewardPools.length,
      },
    };
  },
};
