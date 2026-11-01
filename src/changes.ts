/**
 * Patch changes as chart-ready rows: one row per item per stat that moved.
 *
 * `diff` answers "what changed" for a person reading a terminal. This answers
 * it for a chart: a fixed set of stats per category, before and after values,
 * absolute and percent change, and whether the change is a buff or a nerf for
 * the item's owner. Direction is a property of the stat (lower EM is better,
 * higher DPS is better); stats without an obvious better side read "changed".
 *
 * Items are matched by record name, so a rename is a change, not an add plus a
 * remove, and the row carries both names.
 */

import { datasetFile, loadSnapshot, type BuildSource } from "./diff.js";

export type Direction = "buff" | "nerf" | "changed";

/** One tracked stat: where it lives on the item and which way is better. */
export interface StatSpec {
  stat: string;
  path: string;
  /** True when a bigger number helps the item's owner; undefined when neither side is better. */
  higherIsBetter?: boolean;
}

export interface CategorySpec {
  category: string;
  dataset: string;
  collection: string;
  stats: StatSpec[];
  /** Skip items this returns false for (templates, AI hulls). */
  include?: (item: Record<string, unknown>) => boolean;
}

const up = (stat: string, path = stat): StatSpec => ({ stat, path, higherIsBetter: true });
const down = (stat: string, path = stat): StatSpec => ({ stat, path, higherIsBetter: false });
const neutral = (stat: string, path = stat): StatSpec => ({ stat, path });

const notTemplate = (item: Record<string, unknown>) => item.isTemplate !== true;
const PLAYER_HULL =
  /(_ai_|_ai$|_pu_|_ea_|template|_pir|_crim|unmanned|_test|_s42|_tutorial|_npc|_derelict|_wreck|_boarded|_drone|_hijacked|_qig|_fw_|_mission|_dummy|_prop|_showdown|_cinematic|_nointerior|_display|_temp|_indestructible|_fleetweek|_drug|_tier_\d|_piano|_swarm|_advocacy|_fw\d|_plat$)/i;
const playerShip = (item: Record<string, unknown>) =>
  notTemplate(item) && !PLAYER_HULL.test(String(item.recordName ?? ""));

const WEAPON_STATS: StatSpec[] = [
  up("dps"),
  up("sustainedDps"),
  up("alpha"),
  up("fireRate"),
  up("range"),
  up("speed"),
  down("spread"),
  up("penetration"),
  up("ammoCapacity"),
  up("burstDamage", "burst.damage"),
  up("burstSeconds", "burst.seconds"),
  up("capacitorRegen", "capacitor.regenPerSecond"),
  down("capacitorRegenDelay", "capacitor.regenCooldown"),
  down("power"),
  down("em"),
  down("ir"),
  up("health"),
  down("mass"),
  up("damagePhysical", "ammo.damage.physical"),
  up("damageEnergy", "ammo.damage.energy"),
  up("damageDistortion", "ammo.damage.distortion"),
];

const COMPONENT_COMMON: StatSpec[] = [
  down("power"),
  down("em"),
  down("ir"),
  up("health"),
  down("mass"),
  up("distortionMax"),
  down("distortionRecover", "distortion.secondsToRecover"),
  down("selfRepairSeconds", "durability.selfRepair.seconds"),
];

const component = (collection: string, stats: StatSpec[]): CategorySpec => ({
  category: collection,
  dataset: "ship-components",
  collection,
  stats: [...stats, ...COMPONENT_COMMON],
  include: notTemplate,
});

export const CATEGORIES: CategorySpec[] = [
  { category: "shipWeapons", dataset: "weapons", collection: "shipWeapons", stats: WEAPON_STATS, include: notTemplate },
  { category: "fpsWeapons", dataset: "weapons", collection: "fpsWeapons", stats: WEAPON_STATS, include: notTemplate },
  {
    category: "missiles",
    dataset: "weapons",
    collection: "missiles",
    stats: [
      up("damage", "damageTotal"),
      down("lockTime"),
      up("lockRangeMax"),
      up("lockingAngle"),
      up("speed"),
      up("maxLifetime"),
      up("explosionRadius"),
    ],
    include: notTemplate,
  },
  {
    category: "ships",
    dataset: "ships",
    collection: "vehicles",
    stats: [
      up("scmSpeed", "flight.scmSpeed"),
      up("maxSpeed", "flight.maxSpeed"),
      up("boostForward", "flight.boostSpeedForward"),
      up("pitch", "flight.pitch"),
      up("yaw", "flight.yaw"),
      up("roll", "flight.roll"),
      up("shieldHealth", "totals.shieldHealth"),
      up("shieldRegen", "totals.shieldRegen"),
      up("armorHealth", "totals.armorHealth"),
      up("deflectPhysical", "armor.deflection.physical"),
      up("deflectEnergy", "armor.deflection.energy"),
      down("armorMultPhysical", "armor.damageMultiplier.physical"),
      down("armorMultEnergy", "armor.damageMultiplier.energy"),
      up("weaponDps", "totals.weaponDps"),
      up("missiles", "totals.missiles"),
      up("missileDamage", "totals.missileDamage"),
      up("powerOutput", "totals.powerOutput"),
      up("weaponPips", "powerPools.WeaponGun"),
      up("fuel", "totals.fuel"),
      up("quantumFuel", "totals.quantumFuel"),
      up("cargoScu", "totals.cargoScu"),
      up("thrustMain", "totals.thrustMain"),
      up("thrustManeuver", "totals.thrustManeuver"),
      neutral("crew", "crewSize"),
    ],
    include: playerShip,
  },
  component("shields", [
    up("shieldHealth", "stats.MaxShieldHealth"),
    up("shieldRegen", "stats.MaxShieldRegen"),
    down("downedRegenDelay", "stats.DownedRegenDelay"),
    down("damagedRegenDelay", "stats.DamagedRegenDelay"),
    up("absorbPhysicalMax", "detail.absorption.physical.max"),
    up("resistPhysicalMax", "detail.resistance.physical.max"),
    up("resistEnergyMax", "detail.resistance.energy.max"),
  ]),
  component("powerPlants", [up("powerOutput", "stats.powerOutput")]),
  component("coolers", [up("coolantOutput", "stats.coolantOutput")]),
  component("quantumDrives", [
    up("driveSpeed", "stats.driveSpeed"),
    down("spoolUpTime", "stats.spoolUpTime"),
    down("cooldownTime", "stats.cooldownTime"),
    down("fuelRequirement", "stats.quantumFuelRequirement"),
    up("stageOneAccel", "stats.stageOneAccelRate"),
  ]),
  component("radars", [down("pingCooldown", "stats.cooldownTime")]),
  component("armor", [
    up("deflectPhysical", "detail.deflection.physical"),
    up("deflectEnergy", "detail.deflection.energy"),
    down("multPhysical", "detail.damageMultiplier.physical"),
    down("multEnergy", "detail.damageMultiplier.energy"),
    down("signalEm", "stats.signalElectromagnetic"),
    down("signalIr", "stats.signalInfrared"),
  ]),
  component("thrusters", [up("thrust", "stats.thrustCapacity"), down("fuelBurn", "stats.fuelBurnRatePer10KNewton")]),
];

export interface StatChange {
  category: string;
  record: string;
  name: string;
  /** Present when the item was renamed. */
  previousName?: string;
  size?: number;
  stat: string;
  before?: number;
  after?: number;
  delta?: number;
  /** Percent of the old value; undefined when the old value was 0 or absent. */
  percent?: number;
  direction: Direction;
}

export interface ItemChange {
  category: string;
  record: string;
  name: string;
  previousName?: string;
  size?: number;
  change: "added" | "removed" | "changed";
  statsChanged: number;
  buffs: number;
  nerfs: number;
}

export interface PatchChanges {
  from?: string;
  to?: string;
  items: ItemChange[];
  stats: StatChange[];
  /** Datasets one build lacked, so their categories were skipped. */
  skipped: string[];
}

function read(value: unknown, dotted: string): number | undefined {
  let current = value;
  for (const part of dotted.split(".")) {
    if (!current || typeof current !== "object") return undefined;
    current = (current as Record<string, unknown>)[part];
  }
  return typeof current === "number" && Number.isFinite(current) ? current : undefined;
}

const round = (value: number, places = 3) => Math.round(value * 10 ** places) / 10 ** places;

export function directionOf(spec: StatSpec, before?: number, after?: number): Direction {
  if (spec.higherIsBetter === undefined || before === undefined || after === undefined) return "changed";
  return (after > before) === spec.higherIsBetter ? "buff" : "nerf";
}

/** Changes for one category between two lists of items. */
export function compareCategory(
  spec: CategorySpec,
  before: Record<string, unknown>[],
  after: Record<string, unknown>[],
): { items: ItemChange[]; stats: StatChange[] } {
  const keep = (list: Record<string, unknown>[]) =>
    new Map(
      list
        .filter((i) => typeof i.recordName === "string" && (spec.include?.(i) ?? true))
        .map((i) => [i.recordName as string, i]),
    );
  const old = keep(before);
  const now = keep(after);
  const items: ItemChange[] = [];
  const stats: StatChange[] = [];
  const short = (record: string) => record.replace(/^EntityClassDefinition\./, "");

  for (const [record, item] of now) {
    const prior = old.get(record);
    const name = String(item.name ?? record);
    const size = typeof item.size === "number" ? item.size : undefined;
    if (!prior) {
      items.push({ category: spec.category, record: short(record), name, size, change: "added", statsChanged: 0, buffs: 0, nerfs: 0 });
      continue;
    }
    const previousName = prior.name !== item.name ? String(prior.name) : undefined;
    const moved: StatChange[] = [];
    for (const stat of spec.stats) {
      const a = read(prior, stat.path);
      const b = read(item, stat.path);
      if (a === b) continue;
      const delta = a !== undefined && b !== undefined ? round(b - a) : undefined;
      moved.push({
        category: spec.category,
        record: short(record),
        name,
        previousName,
        size,
        stat: stat.stat,
        before: a,
        after: b,
        delta,
        percent: a && delta !== undefined ? round((delta / Math.abs(a)) * 100, 2) : undefined,
        direction: directionOf(stat, a, b),
      });
    }
    if (moved.length === 0 && !previousName) continue;
    stats.push(...moved);
    items.push({
      category: spec.category,
      record: short(record),
      name,
      previousName,
      size,
      change: "changed",
      statsChanged: moved.length,
      buffs: moved.filter((m) => m.direction === "buff").length,
      nerfs: moved.filter((m) => m.direction === "nerf").length,
    });
  }
  for (const [record, item] of old) {
    if (now.has(record)) continue;
    items.push({
      category: spec.category,
      record: short(record),
      name: String(item.name ?? record),
      size: typeof item.size === "number" ? item.size : undefined,
      change: "removed",
      statsChanged: 0,
      buffs: 0,
      nerfs: 0,
    });
  }
  return { items, stats };
}

/** Every tracked category between two builds. */
export function patchChanges(from: BuildSource, to: BuildSource, categories = CATEGORIES): PatchChanges {
  const result: PatchChanges = { items: [], stats: [], skipped: [] };
  const cache = new Map<string, ReturnType<typeof loadSnapshot>>();
  const load = (source: BuildSource, dataset: string) => {
    const key = `${JSON.stringify(source)}|${dataset}`;
    if (!cache.has(key)) cache.set(key, loadSnapshot(source, datasetFile(dataset)));
    return cache.get(key);
  };

  for (const spec of categories) {
    const a = load(from, spec.dataset);
    const b = load(to, spec.dataset);
    result.from ??= a?.build;
    result.to ??= b?.build;
    if (!a || !b) {
      if (!result.skipped.includes(spec.dataset)) result.skipped.push(spec.dataset);
      continue;
    }
    const list = (snap: typeof a, name: string) => {
      const value = snap?.body[name];
      return Array.isArray(value) ? (value as Record<string, unknown>[]) : [];
    };
    const { items, stats } = compareCategory(spec, list(a, spec.collection), list(b, spec.collection));
    result.items.push(...items);
    result.stats.push(...stats);
  }

  result.stats.sort((x, y) => x.category.localeCompare(y.category) || Math.abs(y.percent ?? 0) - Math.abs(x.percent ?? 0));
  return result;
}

function cell(value: unknown): string {
  if (value === undefined || value === null) return "";
  const text = String(value);
  return /[",\n]/.test(text) ? `"${text.replace(/"/g, '""')}"` : text;
}

export function toCsv<T>(rows: T[], columns: (keyof T & string)[]): string {
  return [columns.join(","), ...rows.map((r) => columns.map((c) => cell(r[c])).join(","))].join("\n") + "\n";
}

export const STAT_COLUMNS: (keyof StatChange & string)[] = [
  "category", "name", "size", "record", "stat", "before", "after", "delta", "percent", "direction", "previousName",
];
export const ITEM_COLUMNS: (keyof ItemChange & string)[] = [
  "category", "name", "size", "record", "change", "statsChanged", "buffs", "nerfs", "previousName",
];
