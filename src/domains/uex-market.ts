/**
 * Market datasets from the UEX Corp API.
 *
 * These are API-sourced rather than extracted, so they carry a fetch step
 * instead of a record filter. They change constantly; the game files do not.
 */

import { trimRow, uexFetch, type UexOptions } from "../sources/uex.js";
import type { ApiDomain } from "./types.js";

/** The columns a terminal row keeps: what places it. Ids, flags, screenshots and contacts go. */
const TERMINAL_KEYS = [
  "id",
  "name",
  "fullname",
  "nickname",
  "displayname",
  "code",
  "type",
  "star_system_name",
  "planet_name",
  "orbit_name",
  "moon_name",
  "space_station_name",
  "city_name",
  "outpost_name",
  "is_available",
  "is_available_live",
];

export function trimTerminalRow(row: unknown): Record<string, unknown> {
  const source = (row ?? {}) as Record<string, unknown>;
  const out: Record<string, unknown> = {};
  for (const key of TERMINAL_KEYS) {
    const value = source[key];
    // Availability is a 0/1 flag, so its zero is data; elsewhere zero and blank mean "not set".
    if (value === null || value === undefined || value === "") continue;
    if (value === 0 && !key.startsWith("is_")) continue;
    out[key] = value;
  }
  return out;
}

export const uexTerminalsDomain: ApiDomain = {
  name: "uex-terminals",
  description: "Every terminal, shop and station with its place: station or city, moon, planet, system. Public, no token needed.",

  async fetch(options: UexOptions) {
    // Public endpoint. Price rows (uex-item-prices) join here on id_terminal.
    const rows = (await uexFetch("terminals", { ...options, tokenOptional: true })).map(trimTerminalRow);
    return { terminals: rows, summary: { terminals: rows.length } };
  },
};

export const uexCommoditiesDomain: ApiDomain = {
  name: "uex-commodities",
  description: "Commodity definitions: what is tradable and how it is classified.",

  async fetch(options: UexOptions) {
    const rows = (await uexFetch("commodities", options)).map(trimRow);
    return { commodities: rows, summary: { commodities: rows.length } };
  },
};

export const uexVehiclePricesDomain: ApiDomain = {
  name: "uex-vehicle-prices",
  description: "Where ships can be bought and rented, with prices.",

  async fetch(options: UexOptions) {
    const [vehicles, purchases, rentals] = await Promise.all([
      uexFetch("vehicles", options),
      uexFetch("vehicles_purchases_prices", options),
      uexFetch("vehicles_rentals_prices", options),
    ]);

    // Price rows carry only id_vehicle, never a name, so join them here rather
    // than making every consumer resolve the id themselves.
    const nameById = new Map<unknown, string>();
    for (const vehicle of vehicles as Record<string, unknown>[]) {
      const name = vehicle.name_full ?? vehicle.name;
      if (vehicle.id !== undefined && typeof name === "string") nameById.set(vehicle.id, name);
    }

    const withName = (rows: unknown[]) =>
      rows.map((row) => {
        const trimmed = trimRow(row) as Record<string, unknown>;
        const name = nameById.get(trimmed.id_vehicle);
        return name ? { vehicle: name, ...trimmed } : trimmed;
      });

    return {
      purchases: withName(purchases),
      rentals: withName(rentals),
      summary: {
        vehicles: vehicles.length,
        purchases: purchases.length,
        rentals: rentals.length,
      },
    };
  },
};

/**
 * Location rows carry an id for every level of the hierarchy, zero where it does
 * not apply (a station has no city), and a reporter handle on audits. Neither is
 * data about the refinery.
 */
function trimRefineryRow(row: unknown): unknown {
  const trimmed = trimRow(row);
  if (!trimmed || typeof trimmed !== "object") return trimmed;
  const out: Record<string, unknown> = {};
  for (const [key, value] of Object.entries(trimmed as Record<string, unknown>)) {
    if (key.startsWith("id_") && value === 0) continue;
    if (key === "datarunner" || key === "id_report") continue;
    out[key] = value;
  }
  return out;
}

/**
 * Refinery yields, capacity and methods as players report them.
 *
 * None of this is in the game files: the client ships the nine method names and
 * the raw-to-refined pairs (see game-refining), while the yield bonus per station
 * and commodity, and each method's yield, cost and speed, are server-side. UEX is
 * crowdsourced, so treat these as observed values, not ground truth.
 */
export const uexRefineryDomain: ApiDomain = {
  name: "uex-refinery",
  description: "Refinery yield bonuses by station and commodity, capacities, methods and audited jobs.",

  async fetch(options: UexOptions) {
    const public_ = { ...options, tokenOptional: true };
    const [yields, capacities, methods, audits] = await Promise.all([
      uexFetch("refineries_yields", public_),
      uexFetch("refineries_capacities", public_),
      uexFetch("refineries_methods", public_),
      uexFetch("refineries_audits", public_),
    ]);
    return {
      // Yield value is the station's bonus or penalty in percent for that commodity.
      yields: yields.map(trimRefineryRow),
      capacities: capacities.map(trimRefineryRow),
      // Ratings run 1 (low) to 3 (high): Dinyx is yield 3, cost 1, speed 1.
      methods: methods.map(trimRefineryRow),
      audits: audits.map(trimRefineryRow),
      summary: {
        yields: yields.length,
        capacities: capacities.length,
        methods: methods.length,
        audits: audits.length,
      },
    };
  },
};

/** The columns an item price row keeps; the rest is location ids and history. */
const ITEM_PRICE_KEYS = [
  "item_name",
  "item_uuid",
  "id_item",
  "id_category",
  "terminal_name",
  "id_terminal",
  "price_buy",
  "price_sell",
  "date_modified",
];

export function trimItemPriceRow(row: unknown): Record<string, unknown> {
  const source = (row ?? {}) as Record<string, unknown>;
  const out: Record<string, unknown> = {};
  for (const key of ITEM_PRICE_KEYS) {
    const value = source[key];
    if (value !== null && value !== undefined && value !== "" && value !== 0) out[key] = value;
  }
  return out;
}

export const uexItemPricesDomain: ApiDomain = {
  name: "uex-item-prices",
  description: "Where personal and ship items are sold, by terminal. Shop stock is server-side, not in the game files.",

  async fetch(options: UexOptions) {
    // Public endpoint; `item_uuid` is the game record id, the join to every other dataset.
    const rows = (await uexFetch("items_prices_all", { ...options, tokenOptional: true })).map(trimItemPriceRow);
    return {
      prices: rows,
      summary: { prices: rows.length, items: new Set(rows.map((r) => r.item_uuid)).size },
    };
  },
};

export const UEX_DOMAINS: ApiDomain[] = [
  uexTerminalsDomain,
  uexCommoditiesDomain,
  uexVehiclePricesDomain,
  uexRefineryDomain,
  uexItemPricesDomain,
];
