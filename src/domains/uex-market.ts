/**
 * Market datasets from the UEX Corp API.
 *
 * These are API-sourced rather than extracted, so they carry a fetch step
 * instead of a record filter. They change constantly; the game files do not.
 */

import { trimRow, uexFetch, type UexOptions } from "../sources/uex.js";
import type { ApiDomain } from "./types.js";

export const uexTerminalsDomain: ApiDomain = {
  name: "uex-terminals",
  description: "Every trading terminal, shop and station with its system and planet.",

  async fetch(options: UexOptions) {
    const rows = (await uexFetch("terminals", options)).map(trimRow);
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

export const UEX_DOMAINS: ApiDomain[] = [
  uexTerminalsDomain,
  uexCommoditiesDomain,
  uexVehiclePricesDomain,
  uexRefineryDomain,
];
