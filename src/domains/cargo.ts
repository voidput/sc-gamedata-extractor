/**
 * Cargo: what NPC and mission ships spawn carrying, and how long loading takes.
 *
 * A CargoManifest fills a ship's grid. Its `cargoFillCapacity` picks a fill
 * mode (Random, Even or Custom) and a capacity range as a fraction of the grid,
 * then lists resources either with a probability (`CargoResource`) or with a
 * share of the hold (`CargoResourceAllocation`, whose `resources` is a single
 * entry despite the plural). `cargoSpawnRules` add specific items or
 * resources on top.
 *
 * GlobalCargoLoadingParams holds the freight elevator and auto-loading timings,
 * per box size. Box sizes are keyed by name (`one_eighthSCU` .. `thirtyTwoSCU`)
 * and are turned into a list ordered by SCU here: as object keys, "0.125"
 * would sort after "32", since JavaScript puts integer-like keys first.
 */

import { at, compareNames, num, refShortName, str, type GameRecord } from "../records.js";
import { entityName } from "./wikelo.js";
import type { Domain } from "./types.js";

const BOX_SIZES: Record<string, number> = {
  one_eighthSCU: 0.125,
  one_quarterSCU: 0.25,
  one_halfSCU: 0.5,
  oneSCU: 1,
  twoSCU: 2,
  fourSCU: 4,
  eightSCU: 8,
  sixteenSCU: 16,
  twentyFourSCU: 24,
  thirtyTwoSCU: 32,
};

export interface BoxSizeValue {
  scu: number;
  value: number;
}

/** `{ twoSCU: 55, oneSCU: 30 }` -> `[{ scu: 1, value: 30 }, { scu: 2, value: 55 }]`, smallest box first. */
export function boxSizes(value: unknown): BoxSizeValue[] | undefined {
  if (!value || typeof value !== "object") return undefined;
  const out: BoxSizeValue[] = [];
  for (const [key, scu] of Object.entries(BOX_SIZES)) {
    const n = num((value as Record<string, unknown>)[key]);
    if (n !== undefined) out.push({ scu, value: n });
  }
  out.sort((a, b) => a.scu - b.scu);
  return out.length > 0 ? out : undefined;
}

export interface ManifestResource {
  resource: string;
  probability?: number;
  /** Share of the hold, 0..1, for allocation entries. */
  minShare?: number;
  maxShare?: number;
}

export interface SpawnRule {
  kind: "entity" | "resource";
  probability?: number;
  items: string[];
  quantity?: number;
  /** Share of capacity, for proportion rules. */
  proportion?: number;
}

export interface CargoManifest {
  name: string;
  recordName: string;
  id: string;
  /** Subfolder, e.g. "civilian", or the name's first segment at top level. */
  category: string;
  /** "Random", "Even" or "Custom". */
  fillMode?: string;
  /** Fraction of the cargo grid filled, 0..1. */
  minCapacity?: number;
  maxCapacity?: number;
  resources: ManifestResource[];
  spawnRules: SpawnRule[];
  lockCargoGridsOnSpawn?: boolean;
}

function parseManifestResource(raw: Record<string, unknown>): ManifestResource | undefined {
  if (raw._Type_ === "CargoResourceAllocation") {
    const inner = (raw.resources ?? {}) as Record<string, unknown>;
    const resource = refShortName(inner.resource);
    if (!resource) return undefined;
    return {
      resource,
      probability: num(inner.probability),
      minShare: num(raw.minResourceAllocationPercentage),
      maxShare: num(raw.maxResourceAllocationPercentage),
    };
  }
  const resource = refShortName(raw.resource);
  return resource ? { resource, probability: num(raw.probability) } : undefined;
}

function parseSpawnRule(raw: Record<string, unknown>): SpawnRule {
  const isEntity = raw._Type_ === "CargoSpawnEntityDef";
  const list = (isEntity ? raw.entities : raw.resources) as unknown[] | undefined;
  const items = (list ?? [])
    .map((x) => (isEntity ? entityName(x) : refShortName(x)))
    .filter((x): x is string => Boolean(x));
  return {
    kind: isEntity ? "entity" : "resource",
    probability: num(raw.probability),
    items,
    quantity: num(at(raw, "quantityAlgorithmType.quantity")),
    proportion: num(at(raw, "quantityAlgorithmType.proportion")),
  };
}

export function parseCargoManifest(record: GameRecord): CargoManifest {
  const v = record.value;
  const fill = (v.cargoFillCapacity ?? {}) as Record<string, unknown>;
  const segments = record.recordPath.split("/");
  const folder = segments.indexOf("cargomanifest");
  const category =
    folder >= 0 && segments.length > folder + 2
      ? segments[folder + 1]
      : record.shortName.split("_")[0].toLowerCase();
  return {
    name: record.shortName,
    recordName: record.name,
    id: record.id,
    category,
    fillMode: str(fill._Type_)?.replace(/^CargoFillCapacityValue_/, ""),
    minCapacity: num(fill.minCapacityRange),
    maxCapacity: num(fill.maxCapacityRange),
    resources: ((fill.resources as Record<string, unknown>[] | undefined) ?? [])
      .map((r) => (r ? parseManifestResource(r) : undefined))
      .filter((r): r is ManifestResource => Boolean(r)),
    spawnRules: ((v.cargoSpawnRules as Record<string, unknown>[] | undefined) ?? [])
      .filter(Boolean)
      .map(parseSpawnRule),
    lockCargoGridsOnSpawn: typeof v.lockCargoGridsOnSpawn === "boolean" ? v.lockCargoGridsOnSpawn : undefined,
  };
}

export interface CargoLoading {
  /** Freight elevator seconds per SCU. */
  cargoDeckLoadingTimePerSCU?: number;
  autoLoadingBaseLoadingTime?: number;
  autoLoadingBaseUnloadingTime?: number;
  /** Seconds per box, by box size in SCU. */
  autoLoadingBoxLoadingTime?: BoxSizeValue[];
  autoLoadingBoxUnloadingTime?: BoxSizeValue[];
  /** Seconds before an unclaimed loading area is revoked. */
  revokeTimeDelay?: number;
  /** Seconds of grace before cargo left in a loading area is forfeit. */
  forfeitTimeBuffer?: number;
  uiTimeRemainingForTimeOutWarning?: number;
}

export function parseCargoLoading(record: GameRecord): CargoLoading {
  const v = record.value;
  return {
    cargoDeckLoadingTimePerSCU: num(v.cargoDeckLoadingTimePerSCU),
    autoLoadingBaseLoadingTime: num(v.autoLoadingBaseLoadingTime),
    autoLoadingBaseUnloadingTime: num(v.autoLoadingBaseUnloadingTime),
    autoLoadingBoxLoadingTime: boxSizes(v.autoLoadingBoxSizeLoadingTime),
    autoLoadingBoxUnloadingTime: boxSizes(v.autoLoadingBoxSizeUnloadingTime),
    revokeTimeDelay: num(v.revokeTimeDelay),
    forfeitTimeBuffer: num(v.forfeitTimeBuffer),
    uiTimeRemainingForTimeOutWarning: num(v.uiTimeRemainingForTimeOutWarning),
  };
}

export const cargoDomain: Domain = {
  name: "game-cargo",
  description: "NPC and mission cargo manifests, and freight loading timings.",
  filters: ["**/cargomanifest/**", "**/globalcargoloadingparams/**"],

  parse({ records }) {
    const manifests = records
      .filter((r) => r.type === "CargoManifest")
      .map(parseCargoManifest)
      .sort((a, b) => compareNames(a.category, b.category) || compareNames(a.name, b.name));
    const loadingRecord = records.find((r) => r.type === "GlobalCargoLoadingParams");

    const byCategory: Record<string, string[]> = {};
    for (const m of manifests) (byCategory[m.category] ??= []).push(m.name);

    return {
      manifests,
      loading: loadingRecord ? parseCargoLoading(loadingRecord) : undefined,
      byCategory,
      summary: { manifests: manifests.length },
    };
  },
};
