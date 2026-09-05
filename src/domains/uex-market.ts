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

export const UEX_DOMAINS: ApiDomain[] = [
  uexTerminalsDomain,
  uexCommoditiesDomain,
  uexVehiclePricesDomain,
];
