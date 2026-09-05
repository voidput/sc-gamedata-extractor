/**
 * Inventory containers: cargo grids, personal inventories and storage.
 *
 * Capacity is expressed as interior dimensions rather than a single number,
 * because what fits depends on box size as well as total volume. Volume and an
 * approximate SCU figure are derived for convenience.
 *
 * Some containers carry sentinel dimensions meaning "unbounded" rather than a
 * real size: a landing zone reads 999x999x999, which computes to half a billion
 * SCU. Those are flagged instead of being reported as capacity.
 */

import { vec } from "../entity.js";
import { str, type GameRecord } from "../records.js";
import type { Domain } from "./types.js";

export interface Container {
  name: string;
  recordName: string;
  id: string;
  inventoryType?: string;
  /** Metres, as x/y/z. */
  interiorDimensions?: Record<string, number>;
  /** Cubic metres, derived from the dimensions. */
  volumeCubicMetres?: number;
  /** One SCU is a 1.25m cube, so 1.953125 cubic metres. */
  approxScu?: number;
  /** Dimensions are a stand-in for "unbounded", not a real capacity. */
  unbounded?: true;
  excludedSubTypes?: string[];
}

const CUBIC_METRES_PER_SCU = 1.25 ** 3;

/**
 * Above this, a dimension is a sentinel rather than a measurement. The largest
 * real cargo grids are tens of metres; 999 is the game's stand-in for unbounded.
 */
const SENTINEL_DIMENSION = 900;

export function parseContainer(record: GameRecord): Container {
  const v = record.value;
  const dims = vec(v.interiorDimensions);

  const unbounded =
    !!dims && Object.values(dims).some((d) => d >= SENTINEL_DIMENSION) ? true : undefined;

  let volume: number | undefined;
  if (dims && dims.x && dims.y && dims.z && !unbounded) {
    volume = Math.round(dims.x * dims.y * dims.z * 1000) / 1000;
  }

  const excluded = Array.isArray(v.excludedItemSubTypes)
    ? v.excludedItemSubTypes.map((e) => str(e)).filter((e): e is string => !!e)
    : undefined;

  return {
    name: record.shortName,
    recordName: record.name,
    id: record.id,
    inventoryType: str(v.inventoryType) ?? str((v.inventoryType as Record<string, unknown>)?._Type_),
    interiorDimensions: dims,
    volumeCubicMetres: volume,
    approxScu: volume ? Math.round((volume / CUBIC_METRES_PER_SCU) * 100) / 100 : undefined,
    unbounded,
    excludedSubTypes: excluded && excluded.length > 0 ? excluded : undefined,
  };
}

export const containersDomain: Domain = {
  name: "game-containers",
  description: "Cargo grids and inventory containers with their dimensions and capacity.",
  filters: ["**/inventorycontainers/**"],

  parse({ records }) {
    const containers = records
      .filter((r) => r.type === "InventoryContainer")
      .map(parseContainer)
      .sort((a, b) => (b.volumeCubicMetres ?? 0) - (a.volumeCubicMetres ?? 0));

    const sized = containers.filter((c) => c.volumeCubicMetres);

    return {
      containers,
      summary: {
        containers: containers.length,
        withDimensions: sized.length,
        unbounded: containers.filter((c) => c.unbounded).length,
        largestScu: sized.length > 0 ? (sized[0].approxScu ?? 0) : 0,
      },
    };
  },
};
