/**
 * Engagement maths: how much of a loadout's damage lands on a target, and out
 * to what distance.
 *
 * Grounded inputs only. Every number here comes from the parsed records:
 * projectile speed and lifetime (max range), per-projectile damage, falloff
 * (personal weapons only; ship ammo carries none and does full damage to the
 * end of its range), the target's armour deflection and multipliers, and the
 * target's bounding box.
 *
 * Effective range is a hit-chance question: how far out a stated share of
 * rounds still lands on a target that is moving. Ship rounds inherit the
 * shooter's velocity (`inheritVelocity` is set on every vehicle ammo record),
 * so only the relative motion of the two ships matters. The model:
 *
 *   flight time   t = d / (projectileSpeed + closingSpeed)
 *   miss radius   R = targetSpeed * t  +  d * tan(spread)
 *   hit chance    p = 1 when R <= r, else (r / R)^2
 *
 * The aim point is the lead pip, which is exact if the target holds course;
 * rounds then scatter only by the weapon's spread cone. A target that jinks
 * the moment you fire can be up to `0.5 * a * t^2` from the pip when the round
 * arrives, `a` being its sideways acceleration (capped at `targetSpeed * t`).
 * `r` is the target's hit radius: the radius of a circle with the target's
 * average projected bounding-box area (front, side, top). The chance is the
 * silhouette as a share of the disc the round and target could be in.
 *
 * Game data: projectile speed, spread (degrees), the target's SCM speed and
 * bounding box. Not game data: acceleration, which needs hull mass from the
 * vehicle XML. So `accel` defaults to 0 (a steady target, spread alone limits
 * hits) and results are tabulated at several g levels instead of guessing one.
 */

import type { DamageInfo } from "./domains/ammo.js";
import type { Armor } from "./domains/ships.js";
import type { Weapon } from "./domains/weapons.js";
import { checkWeapon } from "./deflection.js";

type Falloff = NonNullable<NonNullable<Weapon["ammo"]>["falloff"]>;

const round = (value: number, places = 2) => Math.round(value * 10 ** places) / 10 ** places;
export const G = 9.80665;

/** Per-projectile damage at a distance, after falloff. */
export function damageAt(damage: DamageInfo | undefined, falloff: Falloff | undefined, distance: number): DamageInfo {
  const out: DamageInfo = {};
  for (const [type, amount] of Object.entries(damage ?? {}) as [keyof DamageInfo, number][]) {
    const drop = falloff?.[type];
    if (!drop || distance <= drop.minDistance) {
      out[type] = amount;
      continue;
    }
    out[type] = round(Math.max(drop.floor, amount - drop.perMeter * (distance - drop.minDistance)));
  }
  return out;
}

/** Distance at which every falling damage type has reached its floor; undefined when nothing falls off. */
export function falloffRange(damage: DamageInfo | undefined, falloff: Falloff | undefined): number | undefined {
  let furthest: number | undefined;
  for (const [type, amount] of Object.entries(damage ?? {}) as [keyof DamageInfo, number][]) {
    const drop = falloff?.[type];
    if (!drop) continue;
    const reach = drop.minDistance + Math.max(0, amount - drop.floor) / drop.perMeter;
    furthest = Math.max(furthest ?? 0, reach);
  }
  return furthest !== undefined ? round(furthest, 1) : undefined;
}

export interface Target {
  name: string;
  armor?: Armor;
  /** Metres. Half the smaller of width and height when built from a ship. */
  hitRadius?: number;
  /** m/s; the target's SCM speed when built from a ship. */
  speed?: number;
}

export interface EngageOptions {
  /** Target speed, m/s. Defaults to the target's SCM speed. */
  targetSpeed?: number;
  /** Speed at which the two ships close, m/s; negative when opening. */
  closing?: number;
  /** Which spread to use: the first shot's ("min") or sustained fire's ("max", default). */
  spread?: "min" | "max";
  /** Target sideways acceleration while jinking, m/s^2. Not in the game data; 0 is a steady target. */
  accel?: number;
  /** Seconds before the target starts evading. */
  reaction?: number;
  /** Distance bands, metres. */
  bands?: number[];
  /** Hit-chance levels to report a range for, as fractions. */
  accuracy?: number[];
}

const DEFAULT_ACCURACY = [0.9, 0.75, 0.5, 0.25];

export interface GunEnvelope {
  name: string;
  record: string;
  size?: number;
  count: number;
  mount?: string;
  speed?: number;
  maxRange?: number;
  /** Seconds for a round to reach max range. */
  maxFlightTime?: number;
  /** Listed DPS for all `count` guns. */
  dps?: number;
  /** DPS on the target's hull at point blank, all `count` guns; equals `dps` with no armour. */
  appliedDps?: number;
  penetrates: boolean;
  deflected?: string[];
  /** Personal weapons: distance where damage bottoms out. */
  falloffRange?: number;
  /** Spread used, degrees. */
  spread?: number;
  /** Farthest distance for each hit-chance level, keyed by percent ("50"). */
  rangeFor?: Record<string, number>;
}

export interface Band {
  distance: number;
  /** Guns whose rounds still reach this distance. */
  gunsInRange: number;
  /** Listed DPS of the guns in range. */
  dps: number;
  /** Hull DPS on the target from guns in range, after deflection, multipliers and falloff. */
  appliedDps: number;
  /** Hull DPS expected to land: applied DPS times each gun's hit chance. */
  expectedDps: number;
  /** DPS-weighted hit chance across the guns that hurt the target. */
  hitChance?: number;
  /** Seconds of flight for the fastest and slowest gun in range. */
  flightTime?: { min: number; max: number };
}

export interface Envelope {
  target?: string;
  guns: GunEnvelope[];
  bands: Band[];
  /** Point-blank applied DPS of the whole loadout. */
  appliedDps: number;
  /** Out to here every gun that hurts the target still reaches it. */
  allGunsRange?: number;
  /** Farthest band still carrying at least half the point-blank applied DPS. */
  halfDpsRange?: number;
  /** Farthest any gun that hurts the target reaches. */
  maxRange?: number;
  /**
   * Farthest distance at which the loadout still lands each share of its
   * point-blank hull DPS, keyed by percent: "50" is where half lands.
   */
  effectiveRange?: Record<string, number>;
  /** Target speed the hit chances assume, m/s. */
  targetSpeed?: number;
}

const DEFAULT_BANDS = [0, 250, 500, 750, 1000, 1500, 2000, 2500, 3000, 4000, 5000, 6000, 8000];

function hullDamage(damage: DamageInfo, armor: Armor | undefined): number {
  let applied = 0;
  for (const [type, amount] of Object.entries(damage) as [string, number][]) {
    if (!amount || type === "distortion") continue;
    if (amount <= (armor?.deflection?.[type] ?? 0)) continue;
    applied += amount * (armor?.damageMultiplier?.[type] ?? 1);
  }
  return applied;
}

function shotsPerSecond(weapon: Weapon): { perSecond?: number; pellets: number; multiplier: number } {
  const mode = weapon.modes.find((m) => m.dps === weapon.dps) ?? weapon.modes[0];
  return {
    perSecond: weapon.fireRate ? weapon.fireRate / 60 : undefined,
    pellets: mode?.pellets ?? 1,
    multiplier: mode?.damageMultiplier ?? 1,
  };
}

/** Hull DPS one gun does on a target at a distance; 0 out of range or deflected. */
export function appliedDpsAt(weapon: Weapon, target: Target | undefined, distance: number): number {
  const range = weapon.range ?? weapon.ammo?.range;
  if (range !== undefined && distance > range) return 0;
  const { perSecond, pellets, multiplier } = shotsPerSecond(weapon);
  if (!perSecond || !weapon.ammo?.damage) return 0;
  const hit = damageAt(weapon.ammo.damage, weapon.ammo.falloff, distance);
  return hullDamage(hit, target?.armor) * pellets * multiplier * perSecond;
}

function spreadOf(weapon: Weapon, options: EngageOptions): number {
  const mode = weapon.modes.find((m) => m.dps === weapon.dps) ?? weapon.modes[0];
  const pick = options.spread === "min" ? mode?.spread?.min ?? mode?.spread?.max : mode?.spread?.max ?? mode?.spread?.min;
  return pick ?? weapon.spread ?? 0;
}

/** How far from the lead point the round can end up, metres, at a distance. */
export function missRadius(weapon: Weapon, target: Target | undefined, distance: number, options: EngageOptions = {}): number {
  const speed = (weapon.speed ?? 0) + (options.closing ?? 0);
  if (speed <= 0) return Infinity;
  const t = distance / speed;
  const moving = Math.max(0, t - (options.reaction ?? 0));
  const targetSpeed = options.targetSpeed ?? target?.speed ?? 0;
  // Displacement from the pip: bounded by acceleration, and by top speed.
  const jink = options.accel ? Math.min(targetSpeed * moving, 0.5 * options.accel * moving * moving) : 0;
  const scatter = distance * Math.tan((spreadOf(weapon, options) * Math.PI) / 180);
  return jink + scatter;
}

/** Share of rounds that land on the target at a distance. */
export function hitChance(weapon: Weapon, target: Target | undefined, distance: number, options: EngageOptions = {}): number {
  const range = weapon.range ?? weapon.ammo?.range;
  if (range !== undefined && distance > range) return 0;
  const r = target?.hitRadius;
  if (!r) return 1;
  const R = missRadius(weapon, target, distance, options);
  return R <= r ? 1 : (r / R) ** 2;
}

/** Farthest distance, within max range, where the hit chance is still at least `p`. */
export function rangeForChance(weapon: Weapon, target: Target | undefined, p: number, options: EngageOptions = {}): number {
  const max = weapon.range ?? weapon.ammo?.range ?? 0;
  if (hitChance(weapon, target, max, options) >= p) return max;
  let lo = 0;
  let hi = max;
  for (let i = 0; i < 40; i++) {
    const mid = (lo + hi) / 2;
    if (hitChance(weapon, target, mid, options) >= p) lo = mid;
    else hi = mid;
  }
  return Math.round(lo);
}

/** One gun, `count` of them, against a target. */
export function gunEnvelope(
  weapon: Weapon,
  count: number,
  target: Target | undefined,
  options: EngageOptions = {},
  mount?: string,
): GunEnvelope {
  const maxRange = weapon.range ?? weapon.ammo?.range;
  const check = target?.armor ? checkWeapon(weapon, target.armor) : undefined;
  const applied = appliedDpsAt(weapon, target, 0) * count;
  const levels = options.accuracy ?? DEFAULT_ACCURACY;
  const rangeFor = target?.hitRadius
    ? Object.fromEntries(levels.map((p) => [String(Math.round(p * 100)), rangeForChance(weapon, target, p, options)]))
    : undefined;
  return {
    name: weapon.name,
    record: weapon.recordName.replace(/^EntityClassDefinition\./, ""),
    size: weapon.size,
    count,
    mount,
    speed: weapon.speed,
    maxRange,
    maxFlightTime: maxRange && weapon.speed ? round(maxRange / weapon.speed) : undefined,
    dps: weapon.dps !== undefined ? round(weapon.dps * count) : undefined,
    appliedDps: round(applied),
    penetrates: applied > 0,
    deflected: check && check.deflected.length > 0 ? check.deflected : undefined,
    falloffRange: falloffRange(weapon.ammo?.damage, weapon.ammo?.falloff),
    spread: round(spreadOf(weapon, options), 3),
    rangeFor,
  };
}

/** A whole loadout against a target, by distance band. */
export function envelope(
  loadout: { weapon: Weapon; count: number; mount?: string }[],
  target: Target | undefined,
  options: EngageOptions = {},
): Envelope {
  const guns = loadout.map(({ weapon, count, mount }) => gunEnvelope(weapon, count, target, options, mount));
  const hurting = loadout.filter((_, i) => guns[i].penetrates);
  const bandList = [...new Set([...(options.bands ?? DEFAULT_BANDS), ...guns.map((g) => g.maxRange ?? 0)])]
    .filter((d) => d >= 0)
    .sort((a, b) => a - b);
  const maxRange = Math.max(0, ...guns.filter((g) => g.penetrates).map((g) => g.maxRange ?? 0)) || undefined;
  const reach = Math.max(0, ...guns.map((g) => g.maxRange ?? 0)) || undefined;
  const bands: Band[] = bandList
    .filter((d) => reach === undefined || d <= reach)
    .map((distance) => {
      const inRange = loadout.filter((_, i) => (guns[i].maxRange ?? Infinity) >= distance);
      const flights = inRange
        .map(({ weapon }) => (weapon.speed ? distance / weapon.speed : undefined))
        .filter((t): t is number => t !== undefined);
      const applied = loadout.reduce((sum, { weapon, count }) => sum + appliedDpsAt(weapon, target, distance) * count, 0);
      const expected = loadout.reduce(
        (sum, { weapon, count }) =>
          sum + appliedDpsAt(weapon, target, distance) * count * hitChance(weapon, target, distance, options),
        0,
      );
      return {
        distance,
        gunsInRange: inRange.reduce((n, g) => n + g.count, 0),
        dps: round(inRange.reduce((sum, { weapon, count }) => sum + (weapon.dps ?? 0) * count, 0)),
        appliedDps: round(applied),
        expectedDps: round(expected),
        hitChance: applied > 0 ? round(expected / applied, 3) : undefined,
        flightTime: flights.length > 0 ? { min: round(Math.min(...flights)), max: round(Math.max(...flights)) } : undefined,
      };
    });

  const pointBlank = bands[0]?.appliedDps ?? 0;
  const farthest = (pick: (b: Band) => number | undefined) =>
    [...bands].reverse().find((b) => pointBlank > 0 && (pick(b) ?? 0) >= pointBlank / 2)?.distance;

  return {
    target: target?.name,
    guns,
    bands,
    appliedDps: round(pointBlank),
    allGunsRange: hurting.length > 0 ? Math.min(...guns.filter((g) => g.penetrates).map((g) => g.maxRange ?? Infinity)) : undefined,
    halfDpsRange: farthest((b) => b.appliedDps),
    maxRange,
    effectiveRange: target?.hitRadius && pointBlank > 0
      ? Object.fromEntries(
          (options.accuracy ?? DEFAULT_ACCURACY).map((p) => [
            String(Math.round(p * 100)),
            loadoutRangeFor(loadout, target, p * pointBlank, options, maxRange ?? 0),
          ]),
        )
      : undefined,
    targetSpeed: options.targetSpeed ?? target?.speed,
  };
}

/** Expected hull DPS of a loadout at a distance. */
export function expectedDpsAt(
  loadout: { weapon: Weapon; count: number }[],
  target: Target | undefined,
  distance: number,
  options: EngageOptions = {},
): number {
  return loadout.reduce(
    (sum, { weapon, count }) => sum + appliedDpsAt(weapon, target, distance) * count * hitChance(weapon, target, distance, options),
    0,
  );
}

/** Farthest distance where the loadout's expected hull DPS is still at least `dps`. */
function loadoutRangeFor(
  loadout: { weapon: Weapon; count: number }[],
  target: Target,
  dps: number,
  options: EngageOptions,
  max: number,
): number {
  // Expected DPS never rises with distance (guns only drop out, hit chance
  // only falls), so the threshold crossing can be found by bisection.
  if (expectedDpsAt(loadout, target, max, options) >= dps) return max;
  if (expectedDpsAt(loadout, target, 0, options) < dps) return 0;
  let lo = 0;
  let hi = max;
  for (let i = 0; i < 40 && hi - lo > 0.5; i++) {
    const mid = (lo + hi) / 2;
    if (expectedDpsAt(loadout, target, mid, options) >= dps) lo = mid;
    else hi = mid;
  }
  return Math.round(lo);
}

/**
 * Radius of a circle with the box's average projected area across its three
 * faces. A bounding box overstates a winged hull, so this is generous for
 * fighters seen nose-on; pass an explicit radius when that matters.
 */
export function hitRadiusOf(box: Record<string, number> | undefined): number | undefined {
  if (!box || !(box.x > 0 && box.y > 0 && box.z > 0)) return undefined;
  const mean = (box.x * box.z + box.y * box.z + box.x * box.y) / 3;
  return round(Math.sqrt(mean / Math.PI), 2);
}

/** Range for each hit-chance level at each acceleration, for one loadout. */
export function rangeTable(
  loadout: { weapon: Weapon; count: number }[],
  target: Target,
  accelsG: number[],
  options: EngageOptions = {},
): { accelG: number; ranges: Record<string, number> }[] {
  return accelsG.map((g) => {
    const env = envelope(loadout, target, { ...options, accel: g * G, bands: [0] });
    return { accelG: g, ranges: env.effectiveRange ?? {} };
  });
}

/** A ship's target profile from its parsed vehicle entry. */
export function targetOf(vehicle: {
  name: string;
  armor?: Armor;
  boundingBox?: Record<string, number>;
  flight?: { scmSpeed?: number };
}): Target {
  return {
    name: vehicle.name,
    armor: vehicle.armor,
    hitRadius: hitRadiusOf(vehicle.boundingBox),
    speed: vehicle.flight?.scmSpeed,
  };
}

export function renderEnvelope(
  attacker: string,
  env: Envelope,
  options: EngageOptions,
  target?: Target,
  table?: { accelG: number; ranges: Record<string, number> }[],
): string {
  const levels = (options.accuracy ?? DEFAULT_ACCURACY).map((p) => String(Math.round(p * 100)));
  const lines = [`${attacker}${env.target ? ` vs ${env.target}` : ""}`];
  if (target) {
    lines.push(
      `  target moving ${env.targetSpeed ?? 0} m/s, hit radius ${target.hitRadius ?? "?"} m${options.closing ? `, closing ${options.closing} m/s` : ""}${options.accel ? `, accel ${round(options.accel / G, 1)} g` : ""}${options.reaction ? `, reaction ${options.reaction} s` : ""}`,
    );
  }
  lines.push(
    "",
    `  guns                                     n  size speed spread  max range  hull dps  ${target?.hitRadius ? levels.map((l) => `${l}% hits`.padStart(9)).join("") : ""}`,
  );
  for (const g of env.guns) {
    lines.push(
      `  ${g.name.slice(0, 40).padEnd(40)} ${String(g.count).padStart(2)}  S${String(g.size ?? "?").padEnd(3)}${String(g.speed ?? "").padStart(5)} ${String(g.spread ?? "").padStart(6)} ${String(g.maxRange ?? "").padStart(10)} ${String(g.appliedDps ?? "").padStart(9)}  ${g.rangeFor ? levels.map((l) => `${g.rangeFor?.[l]} m`.padStart(9)).join("") : ""}${g.penetrates ? "" : "  deflected"}${g.falloffRange ? `  falloff to ${g.falloffRange} m` : ""}`,
    );
  }
  lines.push("", "  distance  guns  listed dps  hull dps  hit chance  expected dps  flight s");
  for (const b of env.bands) {
    lines.push(
      `  ${String(b.distance).padStart(7)} m ${String(b.gunsInRange).padStart(4)} ${String(b.dps).padStart(11)} ${String(b.appliedDps).padStart(9)} ${(b.hitChance !== undefined ? `${round(b.hitChance * 100, 1)}%` : "").padStart(11)} ${String(b.expectedDps).padStart(13)}  ${b.flightTime ? `${b.flightTime.min}-${b.flightTime.max}` : ""}`,
    );
  }
  lines.push("");
  lines.push(`  point-blank hull dps   ${env.appliedDps}`);
  if (env.effectiveRange) {
    for (const l of levels) lines.push(`  ${l.padStart(3)}% of it lands to    ${env.effectiveRange[l]} m`);
  }
  if (env.allGunsRange !== undefined) lines.push(`  all guns reach         ${env.allGunsRange} m`);
  if (env.maxRange !== undefined) lines.push(`  any damage to          ${env.maxRange} m`);
  if (target && env.appliedDps === 0) {
    lines.push("", "  Nothing in this loadout gets through the target's armour, so there is no effective range.");
  } else if (table && table.length > 0) {
    lines.push("", `  effective range by target jink (share of point-blank hull dps that lands)`);
    lines.push(`  ${"jink".padEnd(8)}${levels.map((l) => `${l}%`.padStart(9)).join("")}`);
    for (const row of table) {
      lines.push(`  ${`${row.accelG} g`.padEnd(8)}${levels.map((l) => `${row.ranges[l] ?? ""} m`.padStart(9)).join("")}`);
    }
    lines.push("  0 g is a target holding course: only spread limits hits. Jink needs hull mass, not in game data.");
  }
  if (!target) lines.push("", "  No target: hit chance needs one (--target), for its speed and size.");
  return lines.join("\n");
}
