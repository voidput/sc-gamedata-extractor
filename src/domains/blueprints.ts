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
 */

import { isPlaceholderName, localize, type Localization } from "../localization.js";
import { at, num, refShortName, str, type GameRecord } from "../records.js";
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
  tiers: BlueprintTier[];
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
  description: "Crafting blueprints with their recipe costs and craft times.",
  filters: ["**/crafting/**"],
  needsLocalization: true,

  parse({ records, localization }) {
    const blueprints = records
      .filter((r) => r.type === "CraftingBlueprintRecord")
      .map((r) => parseBlueprint(r, localization))
      .filter((b): b is Blueprint => b !== undefined)
      .sort((a, b) => a.name.localeCompare(b.name));

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
      resourceUse: Object.fromEntries(
        Object.entries(resourceUse).sort((a, b) => b[1] - a[1]),
      ),
      byCategory,
      summary: {
        blueprints: blueprints.length,
        distinctResources: Object.keys(resourceUse).length,
        categories: Object.keys(byCategory).length,
      },
    };
  },
};
