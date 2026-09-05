/**
 * Star map locations: systems, planets, moons, stations and points of interest.
 *
 * A StarMapObject carries its display name, a parent reference forming the
 * location hierarchy, jurisdiction, and the amenities available there. Objects
 * flagged `hideInStarmap` exist in the data but are not shown to players; they
 * are kept and flagged rather than dropped.
 *
 * An object's `type` reference resolves to a UUID rather than a name, because
 * every StarMapObjectType is named after its own id. The readable names live in
 * one `StarMapObjectTypes` record holding a `types` array, so that is loaded
 * first and used as a lookup. Without it the type field is a bare UUID.
 */

import { isPlaceholderName, localize, type Localization } from "../localization.js";
import { num, refShortName, str, type GameRecord } from "../records.js";
import type { Domain } from "./types.js";

export interface Location {
  name: string;
  recordName: string;
  id: string;
  displayName?: string;
  description?: string;
  /** Record name of the containing body, forming the hierarchy. */
  parent?: string;
  type?: string;
  affiliation?: string;
  jurisdiction?: string;
  size?: number;
  isScannable?: boolean;
  hiddenInStarmap?: true;
  amenities?: string[];
}

/** A type reference resolves through the lookup; the raw UUID is the fallback. */
function resolveType(value: unknown, typeNames: Map<string, string>): string | undefined {
  const raw = refShortName(value) ?? str(value);
  if (!raw) return undefined;
  return typeNames.get(raw) ?? raw;
}

function amenityNames(value: unknown): string[] | undefined {
  if (!Array.isArray(value)) return undefined;
  const names = value
    .map((entry) => refShortName(entry) ?? str(entry))
    .filter((n): n is string => typeof n === "string" && n.length > 0);
  return names.length > 0 ? names : undefined;
}

/** Build the UUID -> name lookup from the single StarMapObjectTypes record. */
export function buildTypeNames(records: GameRecord[]): Map<string, string> {
  const names = new Map<string, string>();

  for (const record of records) {
    if (record.type !== "StarMapObjectTypes") continue;
    const types = record.value.types;
    if (!Array.isArray(types)) continue;

    for (const entry of types) {
      if (!entry || typeof entry !== "object") continue;
      const obj = entry as Record<string, unknown>;
      const id = str(obj._RecordId_);
      const name = str(obj.name);
      if (id && name) names.set(id, name);
    }
  }

  return names;
}

export function parseLocation(
  record: GameRecord,
  localization: Localization,
  typeNames: Map<string, string> = new Map(),
): Location {
  const v = record.value;
  const displayName = localize(localization, v.name);
  const description = localize(localization, v.description);

  return {
    name: isPlaceholderName(displayName) ? record.shortName : (displayName as string),
    recordName: record.name,
    id: record.id,
    displayName: isPlaceholderName(displayName) ? undefined : displayName,
    description: isPlaceholderName(description) ? undefined : description,
    parent: refShortName(v.parent),
    type: resolveType(v.type, typeNames),
    affiliation: refShortName(v.affiliation),
    jurisdiction: refShortName(v.jurisdiction),
    size: num(v.size),
    isScannable: typeof v.isScannable === "boolean" ? v.isScannable : undefined,
    hiddenInStarmap: v.hideInStarmap === true ? true : undefined,
    amenities: amenityNames(v.amenities),
  };
}

export const starmapDomain: Domain = {
  name: "game-starmap",
  description: "Star map locations with their hierarchy, jurisdiction and amenities.",
  filters: ["**/starmap/**", "**/starmapobject/**"],
  needsLocalization: true,

  parse({ records, localization }) {
    const typeNames = buildTypeNames(records);

    const locations = records
      .filter((r) => r.type === "StarMapObject")
      .map((r) => parseLocation(r, localization, typeNames))
      .sort((a, b) => a.name.localeCompare(b.name));

    const visible = locations.filter((l) => !l.hiddenInStarmap);

    const byType: Record<string, number> = {};
    for (const location of visible) {
      if (location.type) byType[location.type] = (byType[location.type] ?? 0) + 1;
    }

    return {
      locations,
      byType: Object.fromEntries(Object.entries(byType).sort((a, b) => b[1] - a[1])),
      summary: {
        locations: locations.length,
        visible: visible.length,
        hidden: locations.length - visible.length,
        typeNamesResolved: typeNames.size,
      },
    };
  },
};
