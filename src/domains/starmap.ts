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
 *
 * Parent, jurisdiction and affiliation are `file://` paths, not record
 * references, so they resolve through a path index. Following parents up gives
 * each location its chain from the system down (`path`).
 */

import { isPlaceholderName, localize, type Localization } from "../localization.js";
import { compareNames, indexByRecordPath, num, refPath, refShortName, str, type GameRecord } from "../records.js";
import type { Domain } from "./types.js";

export interface Location {
  name: string;
  recordName: string;
  id: string;
  displayName?: string;
  description?: string;
  /** Record name of the containing body, forming the hierarchy. */
  parent?: string;
  /** Names from the system down to the parent: ["Stanton", "Crusader", "Daymar"]. */
  path?: string[];
  /** The root of the chain, normally the star system. */
  system?: string;
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

/** A reference that is either a record object or a `file://` path. */
function resolveRecord(value: unknown, byPath: Map<string, GameRecord>): GameRecord | undefined {
  const recordPath = value && typeof value === "object" ? (value as Record<string, unknown>)._RecordPath_ : undefined;
  const path = refPath(value) ?? refPath(recordPath);
  return path ? byPath.get(path) : undefined;
}

export function parseLocation(
  record: GameRecord,
  localization: Localization,
  typeNames: Map<string, string> = new Map(),
  byPath: Map<string, GameRecord> = new Map(),
): Location {
  const v = record.value;
  // Fall back to the referenced file's name when the target was not extracted.
  const nameOf = (value: unknown) =>
    resolveRecord(value, byPath)?.shortName ?? refShortName(value) ?? refPath(value)?.split("/").pop();
  const displayName = localize(localization, v.name);
  const description = localize(localization, v.description);

  return {
    name: isPlaceholderName(displayName) ? record.shortName : (displayName as string),
    recordName: record.name,
    id: record.id,
    displayName: isPlaceholderName(displayName) ? undefined : displayName,
    description: isPlaceholderName(description) ? undefined : description,
    parent: resolveRecord(v.parent, byPath)?.name ?? refShortName(v.parent),
    type: resolveType(v.type, typeNames),
    affiliation: nameOf(v.affiliation),
    jurisdiction: nameOf(v.jurisdiction),
    size: num(v.size),
    isScannable: typeof v.isScannable === "boolean" ? v.isScannable : undefined,
    hiddenInStarmap: v.hideInStarmap === true ? true : undefined,
    amenities: amenityNames(v.amenities),
  };
}

/**
 * Every StarMapObject parsed, sorted, with `path` and `system` filled in by
 * walking parents up to the root. Shared with game-mining-spawns, which names
 * its spawn zones by the same keys.
 */
export function buildLocations(records: GameRecord[], localization: Localization): Location[] {
  const typeNames = buildTypeNames(records);
  const byPath = indexByRecordPath(records);

  const locations = records
    .filter((r) => r.type === "StarMapObject")
    .map((r) => parseLocation(r, localization, typeNames, byPath))
    .sort((a, b) => compareNames(a.name, b.name));

  // Walk parents up to the root, guarding against a cycle in the data.
  const byRecord = new Map(locations.map((l) => [l.recordName, l]));
  for (const location of locations) {
    const chain: string[] = [];
    const seen = new Set([location.recordName]);
    let parent = location.parent ? byRecord.get(location.parent) : undefined;
    while (parent && !seen.has(parent.recordName)) {
      seen.add(parent.recordName);
      chain.unshift(parent.name);
      parent = parent.parent ? byRecord.get(parent.parent) : undefined;
    }
    if (chain.length > 0) {
      location.path = chain;
      location.system = chain[0];
    }
  }
  return locations;
}

export const starmapDomain: Domain = {
  name: "game-starmap",
  description: "Star map locations with their hierarchy, jurisdiction and amenities.",
  // Jurisdictions so a location's jurisdiction is named as game-law names it.
  filters: ["**/starmap/**", "**/starmapobject/**", "**/lawsystem/jurisdictions/**"],
  needsLocalization: true,

  parse({ records, localization }) {
    const typeNames = buildTypeNames(records);
    const locations = buildLocations(records, localization);

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
