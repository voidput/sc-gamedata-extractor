/**
 * Commodities: the resource types the economy trades in, the cargo items that
 * hold them, and what happens when volatile cargo is knocked about.
 *
 * Every tradable substance is a ResourceType inside one record, the
 * ResourceTypeDatabase, nested in groups (`Metal > UnrefinedOres`). A raw ore
 * names what it refines into through `refinedVersion`, which carries the whole
 * refined resource inline rather than a reference.
 *
 * The cargo items under `entities/commodities` are thin wrappers: a display name,
 * a type label, and a ResourceContainer whose default composition is one
 * resource type. That link is the only thing tying an item to its resource.
 *
 * CommodityDamageConfiguration covers mined cargo taking impact damage and
 * volatile cargo exploding. The Prospector and MOLE variants differ from the
 * base only in their explosion effect and offset, so only gameplay numbers are
 * kept.
 *
 * Quality runs 0-1000. A resource's ResourceTypeCraftingData points at three
 * records: the distribution its quality is rolled from (a normal curve with
 * min, max, mean and spread, shared by rarity tier: every legendary ship ore
 * rolls 501-1000), location overrides of that curve (Pyro, Nyx clusters), and
 * the quantization bands that snap a rolled value to what the game shows
 * (Stileron 950-998 shows as 972).
 */

import { isPlaceholderName, isUnresolved, localize, type Localization } from "../localization.js";
import { at, compareNames, indexByRecordPath, num, refPath, refShortName, str, type GameRecord } from "../records.js";
import type { Domain } from "./types.js";

/** Localize, retrying with the `,P` suffix some keys carry only in that form. */
function localizeKey(localization: Localization, value: unknown): string | undefined {
  const text = localize(localization, value);
  if (!isUnresolved(text) || typeof value !== "string") return text;
  const suffixed = localize(localization, `${value},P`);
  return isUnresolved(suffixed) ? text : suffixed;
}

/**
 * A localized name, or undefined for placeholders and unresolved keys. UI markup
 * is stripped: carbon dioxide is `CO<font size="%d">2</font>` in the table.
 */
export function displayNameOf(localization: Localization, value: unknown): string | undefined {
  const name = localizeKey(localization, value)?.replace(/<[^>]*>/g, "").trim();
  return isPlaceholderName(name) ? undefined : name;
}

/**
 * A localized description: markup stripped, the table's literal `\\n` turned
 * into line breaks, placeholders and "<Name> description." stubs dropped.
 */
export function descriptionOf(localization: Localization, value: unknown): string | undefined {
  const text = localizeKey(localization, value)
    ?.replace(/<[^>]*>/g, "")
    .replace(/\\n/g, "\n")
    .trim();
  // "Tritium description." is a stub left in the table, not a description.
  return isPlaceholderName(text) || /^\S+ description\.?$/i.test(text ?? "") ? undefined : text;
}

export interface ResourceType {
  name: string;
  recordName: string;
  id: string;
  displayName?: string;
  description?: string;
  /** Top-level group, e.g. "Metal". */
  group: string;
  /** Nested group path below the top level, e.g. ["UnrefinedOres"]. */
  subgroups: string[];
  densityGramsPerCc?: number;
  /** The resource this refines into, for raw ores. */
  refinesTo?: string;
  volatility?: { volatility?: number; healthDecayPerSecond?: number };
  quality?: ResourceQuality;
}

export interface QualityDistribution {
  min?: number;
  max?: number;
  mean?: number;
  stddev?: number;
}

export interface ResourceQuality {
  /** The distribution record, e.g. "LegendaryShipMineable_QualityDistribution_Default". */
  profile?: string;
  /** Rarity tier the profile belongs to: "Legendary", "Common", "FPSMineable", "Gatherable"... */
  tier?: string;
  distribution?: QualityDistribution;
  /** The same curve, replaced at these locations. */
  locationOverrides?: (QualityDistribution & { location: string })[];
  /** A rolled quality in [start, end] shows as `value`. */
  bands?: { start?: number; end?: number; value?: number }[];
}

function distributionOf(value: unknown): QualityDistribution | undefined {
  if (!value || typeof value !== "object") return undefined;
  const v = value as Record<string, unknown>;
  const out = { min: num(v.min), max: num(v.max), mean: num(v.mean), stddev: num(v.stddev) };
  return Object.values(out).some((n) => n !== undefined) ? out : undefined;
}

/** Resolve a resource's crafting quality references. Undefined when it has none. */
export function parseQuality(
  props: Record<string, unknown>[],
  byPath: Map<string, GameRecord>,
  localization: Localization = new Map(),
): ResourceQuality | undefined {
  const crafting = props.find((p) => p?._Type_ === "ResourceTypeCraftingData");
  if (!crafting) return undefined;
  const lookup = (ref: unknown) => {
    const path = refPath(ref);
    return path ? byPath.get(path) : undefined;
  };

  const distribution = lookup(at(crafting, "qualityDistribution.qualityDistributionRecord"));
  const overrides = lookup(at(crafting, "qualityLocationOverride.locationOverrideRecord"));
  const quantization = lookup(at(crafting, "qualityQuantization.qualityQuantizationRecord"));

  const overrideList = at(overrides?.value, "locationOverride.locationOverrideList");
  // Sixty Nyx clusters share one display name and curve, so identical entries collapse.
  const seen = new Set<string>();
  const locationOverrides = (Array.isArray(overrideList) ? overrideList : [])
    .filter((e): e is Record<string, unknown> => !!e && typeof e === "object")
    .map((e) => {
      // Name the place as the star map does when its record was extracted.
      const place = lookup(e.location);
      const location =
        displayNameOf(localization, place?.value.name) ?? place?.shortName ?? refPath(e.location)?.split("/").pop() ?? "?";
      return { location, ...distributionOf(e.qualityDistribution) };
    })
    .filter((o) => {
      const key = JSON.stringify(o);
      if (seen.has(key)) return false;
      seen.add(key);
      return true;
    });

  const bandList = at(quantization?.value, "qualityQuantization.bands");
  const bands = (Array.isArray(bandList) ? bandList : [])
    .filter((b): b is Record<string, unknown> => !!b && typeof b === "object")
    .map((b) => ({ start: num(b.start), end: num(b.end), value: num(b.mappedValue) }));

  const quality: ResourceQuality = {
    profile: distribution?.shortName,
    tier: distribution?.shortName.replace(/(ShipMineable)?_QualityDistribution.*$/, ""),
    distribution: distributionOf(at(distribution?.value, "qualityDistribution")),
    locationOverrides: locationOverrides.length > 0 ? locationOverrides : undefined,
    bands: bands.length > 0 ? bands : undefined,
  };
  return quality.profile || quality.bands ? quality : undefined;
}

function parseResource(
  raw: Record<string, unknown>,
  groups: string[],
  localization: Localization,
  byPath: Map<string, GameRecord>,
): ResourceType | undefined {
  const recordName = str(raw._RecordName_);
  if (!recordName) return undefined;
  const props = Array.isArray(raw.properties) ? (raw.properties as Record<string, unknown>[]) : [];
  const volatile = props.find((p) => p?._Type_ === "ResourceTypeVolatility");
  return {
    name: refShortName(raw) ?? recordName,
    recordName,
    id: str(raw._RecordId_) ?? "",
    displayName: displayNameOf(localization, raw.displayName),
    description: descriptionOf(localization, raw.description),
    group: groups[0] ?? "",
    subgroups: groups.slice(1),
    densityGramsPerCc: num(at(raw, "densityType.densityUnit.gramsPerCubicCentimeter")),
    refinesTo: refShortName(raw.refinedVersion),
    volatility: volatile
      ? { volatility: num(volatile.volatility), healthDecayPerSecond: num(volatile.healthDecayPerSecond) }
      : undefined,
    quality: parseQuality(props, byPath, localization),
  };
}

/** Walk the group tree. Groups and resources both contain null entries. */
export function parseResourceDatabase(
  record: GameRecord,
  localization: Localization = new Map(),
  byPath: Map<string, GameRecord> = new Map(),
): ResourceType[] {
  const found: ResourceType[] = [];
  const walk = (group: Record<string, unknown>, path: string[]) => {
    const here = [...path, refShortName(group) ?? "?"];
    for (const raw of (group.resources as Record<string, unknown>[] | undefined) ?? []) {
      if (!raw) continue;
      const resource = parseResource(raw, here, localization, byPath);
      if (resource) found.push(resource);
    }
    for (const child of (group.groups as Record<string, unknown>[] | undefined) ?? []) {
      if (child) walk(child, here);
    }
  };
  for (const group of (record.value.groups as Record<string, unknown>[] | undefined) ?? []) {
    if (group) walk(group, []);
  }
  return found;
}

export interface CommodityItem {
  name: string;
  recordName: string;
  id: string;
  displayName?: string;
  /** The game's type label, e.g. "Gas", "Metal". */
  type?: string;
  /** The resource's description; the cargo items carry none of their own. */
  description?: string;
  /** Folder under entities/commodities, e.g. "halogens". */
  category: string;
  resource?: string;
  /** Capacity of one item in SCU. */
  capacityScu?: number;
}

/** SCU from the game's unit wrappers: centi (1/100) and micro (1/1,000,000). */
export function scuOf(capacity: unknown): number | undefined {
  const centi = num(at(capacity, "centiSCU"));
  if (centi !== undefined) return centi / 100;
  const micro = num(at(capacity, "microSCU"));
  if (micro !== undefined) return micro / 1_000_000;
  return num(at(capacity, "standardCargoUnits"));
}

export function parseCommodityItem(
  record: GameRecord,
  localization: Localization = new Map(),
): CommodityItem {
  const components = (record.value.Components as Record<string, unknown>[] | undefined) ?? [];
  const purchasable = components.find((c) => c?._Type_ === "SCItemPurchasableParams");
  const container = components.find((c) => c?._Type_ === "ResourceContainer");
  const composition = (container?.defaultComposition as Record<string, unknown>[] | undefined) ?? [];
  const segments = record.recordPath.split("/");
  const folder = segments.indexOf("commodities");
  return {
    name: record.shortName,
    recordName: record.name,
    id: record.id,
    displayName: displayNameOf(localization, purchasable?.displayName),
    type: displayNameOf(localization, purchasable?.displayType),
    category: folder >= 0 && segments.length > folder + 2 ? segments[folder + 1] : "",
    resource: refShortName(composition[0]?.entry),
    capacityScu: scuOf(container?.capacity),
  };
}

export interface CommodityDamage {
  name: string;
  recordName: string;
  /** Impact speed in m/s below which cargo takes no damage. */
  minimumSpeed?: number;
  /** Damage per (m/s)^2 of impact speed above the minimum. */
  speedSquaredToDamage?: number;
  /** Seconds after loading before damage applies. */
  gracePeriod?: number;
  explosionDamage?: number;
  explosionMinRadius?: number;
  explosionMaxRadius?: number;
  explosionPressure?: number;
  volatilityRadiusFactor?: number;
  volatilityDamageFactor?: number;
  volatilityForceFactor?: number;
  volatilityCommodityDamageFactor?: number;
}

export function parseCommodityDamage(record: GameRecord): CommodityDamage {
  const v = record.value;
  return {
    name: record.shortName,
    recordName: record.name,
    minimumSpeed: num(v.minimumSpeed),
    speedSquaredToDamage: num(v.speedSquaredToDamage),
    gracePeriod: num(v.gracePeriod),
    explosionDamage: num(at(v, "defaultExplosionParams.damage.damageTotal")),
    explosionMinRadius: num(at(v, "defaultExplosionParams.minRadius")),
    explosionMaxRadius: num(at(v, "defaultExplosionParams.maxRadius")),
    explosionPressure: num(at(v, "defaultExplosionParams.pressure")),
    volatilityRadiusFactor: num(v.volatilityRadiusFactor),
    volatilityDamageFactor: num(v.volatilityDamageFactor),
    volatilityForceFactor: num(v.volatilityForceFactor),
    volatilityCommodityDamageFactor: num(v.volatilityCommodityDamageFactor),
  };
}

export const commoditiesDomain: Domain = {
  name: "game-commodities",
  description: "Resource types with density and refined form, commodity cargo items, and cargo damage.",
  filters: [
    "**/resourcetypedatabase/**",
    "**/entities/commodities/**",
    "**/commodityconfiguration/**",
    "**/crafting/qualitydistribution/**",
    "**/crafting/qualityquantization/**",
    // Systems and clusters directly under pu/, which quality overrides name.
    "**/starmap/pu/*",
  ],
  needsLocalization: true,

  parse({ records, localization }) {
    // The database's own order is its group tree, so sort by group, then name.
    const resources = records
      .filter((r) => r.type === "ResourceTypeDatabase")
      .flatMap((r) => parseResourceDatabase(r, localization, indexByRecordPath(records)))
      .sort((a, b) => compareNames(a.group, b.group) || compareNames(a.displayName ?? a.name, b.displayName ?? b.name));
    const items = records
      .filter((r) => r.recordPath.includes("/entities/commodities/"))
      .map((r) => parseCommodityItem(r, localization))
      .map((item) => {
        const description = resources.find((r) => r.name === item.resource)?.description;
        return description ? { ...item, description } : item;
      })
      .sort((a, b) => compareNames(a.displayName ?? a.name, b.displayName ?? b.name));
    const damage = records
      .filter((r) => r.type === "CommodityDamageConfiguration")
      .map(parseCommodityDamage)
      .sort((a, b) => compareNames(a.name, b.name));

    const byGroup: Record<string, string[]> = {};
    for (const r of resources) (byGroup[r.group] ??= []).push(r.name);

    return {
      resources,
      items,
      damage,
      byGroup,
      summary: { resources: resources.length, items: items.length, damage: damage.length },
    };
  },
};
