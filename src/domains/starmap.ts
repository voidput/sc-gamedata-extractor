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
 *
 * DataCore places nothing in space. Positions come from the solar system
 * object containers (see ../socpak.ts), joined by record id, and cover what a
 * system container places directly: stars, planets, moons, Lagrange points,
 * stations, jump points and asteroid bases, plus what each of those bodies'
 * own containers place around them. Surface locations ride a rotating body and
 * are left without one. `quantum` is the record's own quantum travel geometry.
 */

import { isPlaceholderName, localize, type Localization } from "../localization.js";
import { at, compareNames, indexByRecordPath, num, refPath, refShortName, str, type GameRecord } from "../records.js";
import {
  loadPlacements,
  loadZones,
  unextractedLocationContainers,
  type ContainerZones,
  type PlacedContainer,
} from "../socpak.js";
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
  /** Absolute position in the system frame, metres, star at the origin. */
  position?: { x: number; y: number; z: number };
  /** Quantum travel geometry, metres from the object's centre. */
  quantum?: QuantumGeometry;
  /** Radius of the armistice zone around the location, metres. */
  armisticeRadius?: number;
  /** Radius inside which crime is seen and reported (a comm array, a station), metres. */
  monitoredRadius?: number;
  /**
   * Radius of a monitor zone jammer here, metres: a dead zone where crime goes unseen
   * whatever comm array or station covers the space around it.
   */
  jammerRadius?: number;
}

/** Used when the MonitoredZone entity record was not extracted. */
const DEFAULT_MONITORED_KM = 50_000;

/** Reach of a monitored zone, metres: the default every placed zone inherits. */
export function monitoredRadius(records: GameRecord[]): number {
  for (const record of records) {
    if (record.name !== "EntityClassDefinition.MonitoredZone") continue;
    const components = record.value.Components;
    if (!Array.isArray(components)) continue;
    for (const component of components) {
      const km = num(at(component, "radiusKm"));
      if (at(component, "_Type_") === "MonitoredZoneParams" && km) return km * 1000;
    }
  }
  return DEFAULT_MONITORED_KM * 1000;
}

export interface QuantumGeometry {
  /** A route passing closer than this is blocked by the object. */
  obstructionRadius?: number;
  /** Ships travelling here drop out this far from the centre. */
  arrivalRadius?: number;
  /** Within this range the object's own children become travel targets. */
  adoptionRadius?: number;
  arrivalPointDetectionOffset?: number;
  subPointRadiusMultiplier?: number;
}

function quantumGeometry(value: unknown): QuantumGeometry | undefined {
  const geometry: QuantumGeometry = {
    obstructionRadius: num(at(value, "obstructionRadius")),
    arrivalRadius: num(at(value, "arrivalRadius")),
    adoptionRadius: num(at(value, "adoptionRadius")),
    arrivalPointDetectionOffset: num(at(value, "arrivalPointDetectionOffset")),
    subPointRadiusMultiplier: num(at(value, "subPointRadiusMultiplier")),
  };
  return Object.values(geometry).some((v) => v !== undefined) ? geometry : undefined;
}

/** Metres to the millimetre: the containers carry float noise past that. */
const round = (value: number) => Math.round(value * 1000) / 1000;

/** A type reference resolves through the lookup; the raw UUID is the fallback. */
function resolveType(value: unknown, typeNames: Map<string, string>): string | undefined {
  const raw = refShortName(value) ?? str(value);
  if (!raw) return undefined;
  return typeNames.get(raw) ?? raw;
}

/** An amenity reference resolves through the lookup; the raw UUID is the fallback. */
function amenityNames(value: unknown, names: Map<string, string>): string[] | undefined {
  if (!Array.isArray(value)) return undefined;
  const found = value
    .map((entry) => refShortName(entry) ?? str(entry))
    .filter((n): n is string => typeof n === "string" && n.length > 0)
    .map((n) => names.get(n) ?? n);
  return found.length > 0 ? found : undefined;
}

/**
 * The UUID -> name lookup for amenities ("Refinery", "Hospital", "Hangar XL").
 * Like object types, every amenity type is named after its own id, and the
 * readable names sit in one `StarMapAmenityTypes` record.
 */
export function buildAmenityNames(records: GameRecord[]): Map<string, string> {
  const names = new Map<string, string>();
  for (const record of records) {
    if (record.type !== "StarMapAmenityTypes") continue;
    const types = record.value.amenityTypes;
    if (!Array.isArray(types)) continue;
    for (const entry of types) {
      const id = str(at(entry, "_RecordId_"));
      const name = str(at(entry, "name"));
      if (id && name) names.set(id, name);
    }
  }
  return names;
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
  amenityTypes: Map<string, string> = new Map(),
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
    amenities: amenityNames(v.amenities, amenityTypes),
    quantum: quantumGeometry(v.quantumTravelData),
  };
}

/** Used when the MonitorZoneJammer entity record was not extracted. */
const DEFAULT_JAMMER_KM = 40;

/** Reach of a monitor zone jammer that sets no radius of its own, metres. */
export function jammerRadius(records: GameRecord[]): number {
  for (const record of records) {
    if (record.name !== "EntityClassDefinition.MonitorZoneJammer") continue;
    const components = record.value.Components;
    if (!Array.isArray(components)) continue;
    for (const component of components) {
      const km = num(at(component, "radiusKm"));
      if (at(component, "_Type_") === "MonitoredZoneParams" && km) return km * 1000;
    }
  }
  return DEFAULT_JAMMER_KM * 1000;
}

/**
 * Every StarMapObject parsed, sorted, with `path` and `system` filled in by
 * walking parents up to the root. Shared with game-mining-spawns, which names
 * its spawn zones by the same keys.
 */
export function buildLocations(
  records: GameRecord[],
  localization: Localization,
  placements: Map<string, PlacedContainer> = new Map(),
  zones: Map<string, ContainerZones> = new Map(),
): Location[] {
  const monitored = zones.size > 0 ? monitoredRadius(records) : 0;
  const typeNames = buildTypeNames(records);
  const amenityTypes = buildAmenityNames(records);
  const byPath = indexByRecordPath(records);

  const locations = records
    .filter((r) => r.type === "StarMapObject")
    .map((r) => parseLocation(r, localization, typeNames, byPath, amenityTypes))
    .sort((a, b) => compareNames(a.name, b.name));

  const byId = new Map(locations.map((l) => [l.id, l]));
  for (const location of locations) {
    const placed = placements.get(location.id);
    if (!placed) continue;
    const [x, y, z] = placed.position;

    // Anything inside its body's obstruction radius is on or near the surface,
    // which turns with the body: its container position is one instant of a
    // rotation, not a place in space.
    const body = placed.parentRecord ? byId.get(placed.parentRecord) : undefined;
    const bodyRadius = body?.quantum?.obstructionRadius;
    if (bodyRadius && placed.parentPosition) {
      const [px, py, pz] = placed.parentPosition;
      if (Math.hypot(x - px, y - py, z - pz) < bodyRadius) continue;
    }
    location.position = { x: round(x), y: round(y), z: round(z) };

    // A planet's or moon's own container holds the zones of places on its
    // surface, which say nothing about the space around it.
    const zone = location.type === "Planet" || location.type === "Moon" ? undefined : zones.get(location.id);
    if (zone?.armisticeRadius) location.armisticeRadius = zone.armisticeRadius;
    if (zone?.monitored) location.monitoredRadius = monitored;
    if (zone?.jammer) location.jammerRadius = zone.jammer.radius ?? jammerRadius(records);
  }

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
  // The MonitoredZone entity carries the reach every placed monitored zone inherits.
  filters: ["**/starmap/**", "**/starmapobject/**", "**/lawsystem/jurisdictions/**", "**/entities/monitoredzone", "**/mission_entities/monitorzonejammer", "**/starmapamenitytypes/**"],
  // Solar system containers, the only source of positions.
  files: ["**/ObjectContainers/PU/system/*/*system.socpak"],
  // Then each body's own container, which places its stations.
  moreFiles: unextractedLocationContainers,
  needsLocalization: true,

  parse({ records, localization, extractDir }) {
    const typeNames = buildTypeNames(records);
    const placements = extractDir ? loadPlacements(extractDir) : new Map<string, PlacedContainer>();
    const zones = extractDir ? loadZones(extractDir, placements) : new Map<string, ContainerZones>();
    const locations = buildLocations(records, localization, placements, zones);

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
        positioned: locations.filter((l) => l.position).length,
        armistice: locations.filter((l) => l.armisticeRadius).length,
        monitored: locations.filter((l) => l.monitoredRadius).length,
        jammed: locations.filter((l) => l.jammerRadius).length,
        typeNamesResolved: typeNames.size,
      },
    };
  },
};
