/**
 * Manufacturers: the in-fiction companies behind ships, components and gear.
 *
 * The records are SCItemManufacturer, and they live under
 * `libs/foundry/records/scitemmanufacturer` — not under any path containing
 * "manufacturers", which holds tint palettes instead.
 *
 * Names are localization keys (`@manufacturer_NameSASU`), so this domain needs
 * the string table to produce anything readable.
 *
 * Not every record in that directory is a manufacturer. Real ones sit at the top
 * level or under `clothes` and `armor` (clothing and armour brands). The rest
 * merely reference a manufacturer: `paintcolorlogos` alone holds over 900 paint
 * records carrying their maker's name key, which is why an unfiltered read
 * returns roughly ten times too many rows, mostly duplicate names.
 */

import { localize } from "../localization.js";
import { str, type GameRecord } from "../records.js";
import type { Domain, ParseContext } from "./types.js";

export interface Manufacturer {
  /** Record short name, which doubles as the manufacturer code: "AEGS". */
  code: string;
  recordName: string;
  id: string;
  name?: string;
  description?: string;
}

export function parseManufacturer(
  record: GameRecord,
  localization: ParseContext["localization"],
): Manufacturer {
  const loc = record.value.Localization as Record<string, unknown> | undefined;
  return {
    code: record.shortName,
    recordName: record.name,
    id: record.id,
    name: localize(localization, loc?.Name),
    description: localize(localization, loc?.Description),
  };
}

/**
 * Subdirectories that hold something other than a manufacturer:
 * paint logos, shop kiosk names, and UI-only style placeholders.
 */
const NON_MANUFACTURER_DIRS = ["paintcolorlogos", "shop", "uistyleonly"];

export function isManufacturerRecord(record: GameRecord): boolean {
  const marker = "scitemmanufacturer/";
  const path = record.recordPath.toLowerCase();
  const index = path.lastIndexOf(marker);
  if (index < 0) return false;

  const rest = path.slice(index + marker.length);
  const slash = rest.indexOf("/");
  if (slash < 0) return true; // top level

  return !NON_MANUFACTURER_DIRS.includes(rest.slice(0, slash));
}

/** Placeholder and test rows outnumber real manufacturers; drop the obvious ones. */
function looksReal(manufacturer: Manufacturer): boolean {
  if (!manufacturer.name) return false;
  if (manufacturer.name.startsWith("@")) return false;
  const lower = manufacturer.name.toLowerCase();
  return !lower.includes("placeholder") && !lower.includes("<=") && lower !== "unknown";
}

export const manufacturersDomain: Domain = {
  name: "game-manufacturers",
  description: "Manufacturer codes and display names.",
  filters: ["**/scitemmanufacturer/**"],
  needsLocalization: true,

  parse({ records, localization }) {
    const all = records
      .filter((r) => r.type === "SCItemManufacturer" && isManufacturerRecord(r))
      .map((r) => parseManufacturer(r, localization));

    const named = all.filter(looksReal).sort((a, b) => (a.name ?? "").localeCompare(b.name ?? ""));

    // Records reference manufacturers by code, so a code -> name map is the
    // shape most consumers actually want.
    const byCode: Record<string, string> = {};
    for (const manufacturer of named) {
      if (manufacturer.name) byCode[manufacturer.code] = manufacturer.name;
    }

    // The game's own data maps some codes onto another company's name (FSKI
    // carries Aegis's name key). That is reported as found rather than
    // corrected, so keep the code to make such rows traceable.
    const sharedNames = Object.entries(
      named.reduce<Record<string, string[]>>((acc, m) => {
        if (m.name) (acc[m.name] ??= []).push(m.code);
        return acc;
      }, {}),
    )
      .filter(([, codes]) => codes.length > 1)
      .map(([name, codes]) => ({ name, codes: codes.sort() }));

    return {
      manufacturers: named,
      byCode,
      sharedNames,
      summary: {
        manufacturers: named.length,
        unnamedOrPlaceholder: all.length - named.length,
        namesUsedByMultipleCodes: sharedNames.length,
      },
    };
  },
};

export const _internal = { looksReal, str };
