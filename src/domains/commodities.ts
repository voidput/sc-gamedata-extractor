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

import { isPlaceholderName, localize, locKey, locRefs, nameStatusField, type Localization, type NameStatus } from "../localization.js";
import { at, compareNames, indexByRecordPath, num, refPath, refShortName, str, type GameRecord } from "../records.js";
import type { Domain } from "./types.js";

/**
 * A localized name, or undefined for placeholders and unresolved keys. UI markup
 * is stripped: carbon dioxide is `CO<font size="%d">2</font>` in the table.
 */
export function displayNameOf(localization: Localization, value: unknown): string | undefined {
  const name = localize(localization, value)?.replace(/<[^>]*>/g, "").trim();
  return isPlaceholderName(name) ? undefined : name;
}

/**
 * A localized description: markup stripped, the table's literal `\\n` turned
 * into line breaks, placeholders and "<Name> description." stubs dropped.
 */
export function descriptionOf(localization: Localization, value: unknown): string | undefined {
  const text = localize(localization, value)
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
  /** String-table keys, as the table spells them; `nameStatus` (absent when real) flags a missing name. */
  nameKey?: string;
  descriptionKey?: string;
  /** Absent when the name is real. */
  nameStatus?: NameStatus;
  /** Top-level group, e.g. "Metal". */
  group: string;
  /** Nested group path below the top level, e.g. ["UnrefinedOres"]. */
  subgroups: string[];
  densityGramsPerCc?: number;
  /** The resource this refines into, for raw ores. */
  refinesTo?: string;
  volatility?: { volatility?: number; healthDecayPerSecond?: number };
  quality?: ResourceQuality;
  // Derived across the dataset (resourceLinks), so consumers need not recompute them.
  /** The top group, readable: "UnrefinedOres" -> "Unrefined Ores". */
  groupLabel?: string;
  /** The subgroups, readable and joined: "Unrefined Ores > Gems". */
  subgroupLabel?: string;
  /** False for the placeholder groups (HeatPlaceholder, PowerPlaceholder): ship systems, not trade. */
  tradable?: boolean;
  /** A raw form: it refines into something (`refinesTo`). */
  raw?: boolean;
  /** Resources that refine into this one, by exact name. */
  rawSources?: string[];
  /** Volatile cargo: volatility above zero (it decays and can explode). */
  volatile?: true;
  /** Density is 1.0, the engine default, not a measurement. */
  densityIsDefault?: true;
  /** Cargo items holding it. */
  cargo?: { className: string; kind?: string; capacityScu?: number }[];
}

/** `UnrefinedOres` -> `Unrefined Ores`, `Raw_Minerals` -> `Raw Minerals`. */
export function groupLabel(key: string): string {
  let out = "";
  let prev: string | undefined;
  for (const c of key) {
    if (c === "_") {
      if (out.length > 0 && !out.endsWith(" ")) out += " ";
    } else {
      if (/\p{Lu}/u.test(c) && prev !== undefined && /\p{Ll}/u.test(prev)) out += " ";
      out += c;
    }
    prev = c;
  }
  return out.trim();
}

/** The cross-record fields of each resource: labels, raw links, cargo items. */
export function resourceLinks(resources: ResourceType[], items: CommodityItem[]): ResourceType[] {
  const lower = (s: string) => s.toLowerCase();
  return resources.map((r) => {
    const rawSources = resources
      .filter((x) => x.refinesTo !== undefined && lower(x.refinesTo) === lower(r.name))
      .map((x) => x.name)
      .sort(compareNames);
    const cargo = items
      .filter((i) => i.resource !== undefined && lower(i.resource) === lower(r.name))
      .map((i) => ({ className: i.name, kind: i.type, capacityScu: i.capacityScu }))
      .sort((a, b) => compareNames(a.className, b.className));
    const subgroupLabel = r.subgroups.map(groupLabel).join(" > ");
    return {
      ...r,
      groupLabel: groupLabel(r.group) || undefined,
      subgroupLabel: subgroupLabel || undefined,
      tradable: !lower(r.group).endsWith("placeholder"),
      raw: r.refinesTo !== undefined,
      rawSources: rawSources.length > 0 ? rawSources : undefined,
      volatile: (r.volatility?.volatility ?? 0) > 0 || undefined,
      densityIsDefault: r.densityGramsPerCc === 1 || undefined,
      cargo: cargo.length > 0 ? cargo : undefined,
      quality: withSteps(r.quality),
    };
  });
}

/** Damage configurations with identical settings, as one row listing every config. */
export function damageGroups(damage: CommodityDamage[]): (Omit<CommodityDamage, "name" | "recordName"> & { configs: string[] })[] {
  const groups = new Map<string, Omit<CommodityDamage, "name" | "recordName"> & { configs: string[] }>();
  for (const d of damage) {
    const settings: Partial<CommodityDamage> = { ...d };
    delete settings.name;
    delete settings.recordName;
    const key = JSON.stringify(settings);
    const group = groups.get(key);
    if (group) group.configs.push(d.name);
    else groups.set(key, { configs: [d.name], ...settings });
  }
  return [...groups.values()];
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
  locationOverrides?: (QualityDistribution & { location: string; steps?: QualityStep[] })[];
  /** A rolled quality in [start, end] shows as `value`. */
  bands?: { start?: number; end?: number; value?: number }[];
  /** The bands a roll on `distribution` can reach, clipped to its min-max. */
  steps?: QualityStep[];
}

/** A shown value: every roll in start..end (clipped to the curve's min-max) shows as `shown`. */
export interface QualityStep {
  start: number;
  end: number;
  shown: number;
  /** Estimated share of rolls, 0-1; absent without a spread. See `_assumptions`. */
  shareEstimate?: number;
}

/** What `shareEstimate` rests on; the records give the curve's numbers, not how the game samples it. */
export const QUALITY_ASSUMPTIONS =
  "quality steps[].shareEstimate assumes rolls are integers drawn from a normal curve (mean, stddev) truncated to min-max; a band covers start-0.5 to end+0.5.";

/** Abramowitz-Stegun 7.1.26, error below 1.5e-7; the same formula the app uses. */
function erf(x: number): number {
  const t = 1 / (1 + 0.3275911 * Math.abs(x));
  const y =
    1 -
    ((((1.061405429 * t - 1.453152027) * t + 1.421413741) * t - 0.284496736) * t + 0.254829592) *
      t *
      Math.exp(-x * x);
  return x < 0 ? -y : y;
}

const phi = (x: number, mean: number, sd: number) => 0.5 * (1 + erf((x - mean) / (sd * Math.SQRT2)));

/** Rounded so float noise cannot differ between implementations. */
const share = (n: number) => Math.round(n * 1e8) / 1e8;

/** The steps a roll on `curve` reaches: bands clipped to min-max (0-1000 when unset), lowest first. */
export function qualitySteps(bands: ResourceQuality["bands"], curve: QualityDistribution | undefined): QualityStep[] | undefined {
  if (!bands || bands.length === 0) return undefined;
  const lo = curve?.min ?? 0;
  const hi = curve?.max ?? 1000;
  const { mean, stddev } = curve ?? {};
  const mass = (a: number, b: number) =>
    mean !== undefined && stddev !== undefined && stddev > 0 ? phi(b + 0.5, mean, stddev) - phi(a - 0.5, mean, stddev) : undefined;
  const total = mass(lo, hi);
  const steps: QualityStep[] = [];
  for (const band of bands) {
    if (band.start === undefined || band.end === undefined || band.value === undefined) continue;
    const start = Math.max(band.start, lo);
    const end = Math.min(band.end, hi);
    if (start > end) continue;
    const m = mass(start, end);
    steps.push({
      start,
      end,
      shown: band.value,
      shareEstimate: total !== undefined && total > 0 && m !== undefined ? share(m / total) : undefined,
    });
  }
  steps.sort((a, b) => a.start - b.start);
  return steps.length > 0 ? steps : undefined;
}

/** A quality block with steps for its own curve and each location's. */
export function withSteps(quality: ResourceQuality | undefined): ResourceQuality | undefined {
  if (!quality) return undefined;
  return {
    ...quality,
    locationOverrides: quality.locationOverrides?.map((o) => ({ ...o, steps: qualitySteps(quality.bands, o) })),
    steps: qualitySteps(quality.bands, quality.distribution),
  };
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
    ...locRefs(localization, raw.displayName, raw.description),
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
  nameKey?: string;
  /** The resource's description key, as with `description`. */
  descriptionKey?: string;
  /** Absent when the name is real. */
  nameStatus?: NameStatus;
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
    nameKey: locKey(localization, purchasable?.displayName),
    nameStatus: nameStatusField(localization, purchasable?.displayName),
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
        // A resource owns its description key; the cargo items share it.
        const resource = resources.find((r) => r.name === item.resource);
        const { description, descriptionKey } = resource ?? {};
        return description || descriptionKey ? { ...item, description, descriptionKey } : item;
      })
      .sort((a, b) => compareNames(a.displayName ?? a.name, b.displayName ?? b.name));
    const damage = records
      .filter((r) => r.type === "CommodityDamageConfiguration")
      .map(parseCommodityDamage)
      .sort((a, b) => compareNames(a.name, b.name));

    const byGroup: Record<string, string[]> = {};
    for (const r of resources) (byGroup[r.group] ??= []).push(r.name);

    const linked = resourceLinks(resources, items);
    return {
      _assumptions: linked.some((r) => r.quality?.steps?.some((s) => s.shareEstimate !== undefined))
        ? [QUALITY_ASSUMPTIONS]
        : undefined,
      resources: linked,
      items,
      damage,
      damageGroups: damageGroups(damage),
      byGroup,
      summary: { resources: resources.length, items: items.length, damage: damage.length },
    };
  },
};
