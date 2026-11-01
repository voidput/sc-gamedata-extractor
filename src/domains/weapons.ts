/**
 * Combat baseline: every weapon joined to its ammunition, with the derived
 * numbers a loadout tool shows (DPS, alpha, rate, range, spread, power, EM,
 * ammo, HP) so a patch-over-patch diff of this one file answers "what did
 * they do to my gun".
 *
 * Where each column comes from, since none of it sits in one place:
 *
 * - Damage is on the ammo (`AmmoParams.projectileParams.damage`), never the
 *   weapon. A ship weapon embeds an `SAmmoContainerComponentParams` whose
 *   `ammoParamsRecord` points at it. An FPS weapon has no ammo of its own: its
 *   default loadout names a magazine entity, and the magazine carries the
 *   container, so the join is weapon -> magazine -> ammo.
 * - Fire rate lives on the fire action. Most ship guns wrap it in an
 *   `SWeaponActionSequenceParams` whose entries carry a `delay` in RPM; that
 *   delay, not the nested action's `fireRate`, is the cadence the game runs
 *   (a 900 RPM sequence over two 500 RPM barrels fires at 900).
 * - Range is `speed * lifetime` of the projectile.
 * - Power draw and EM come from `ItemResourceComponentParams` (the resource
 *   network states), HP from `SHealthComponentParams.Health`.
 * - Penetration is `projectileParams.penetrationParams.basePenetrationDistance`.
 *
 * Checked against a loadout site for the S5 Behring gatling: 102.5 damage at
 * 900 RPM is 1537.5 DPS, 1251 m/s over 3.2 s is 4003 m, 5004 rounds, 1650 HP,
 * 36 EM, 0.1 power, 0.792 spread. Every column matched.
 */

import { component, referencedName } from "../entity.js";
import { isPlaceholderName, localize, type Localization } from "../localization.js";
import { at, num, str, type GameRecord } from "../records.js";
import { parseAmmo, type Ammo, type DamageInfo } from "./ammo.js";
import {
  distortionOf,
  durabilityOf,
  manufacturerOf,
  resourceProfile,
  volumeOf,
  type Distortion,
  type Durability,
} from "./item-common.js";
import type { Domain } from "./types.js";

export interface Spread {
  min?: number;
  max?: number;
  attack?: number;
  decay?: number;
}

/** One selectable fire mode, with its own cadence and per-shot numbers. */
export interface FireMode {
  name: string;
  kind: "single" | "rapid" | "burst" | "charged" | "beam" | "sequence" | "other";
  /** Rounds per minute the mode actually cycles at. */
  fireRate?: number;
  /** For bursts: rate including the cooldown between bursts. */
  sustainedFireRate?: number;
  shotCount?: number;
  pellets?: number;
  damageMultiplier?: number;
  heatPerShot?: number;
  chargeTime?: number;
  /** Damage multiplier at full charge, for charged modes. */
  chargedDamageMultiplier?: number;
  spread?: Spread;
  /** Damage of one trigger pull: per-projectile damage * pellets * multiplier. */
  alpha?: number;
  dps?: number;
  /** Beams: damage per second and the range it falls off over. */
  fullDamageRange?: number;
  zeroDamageRange?: number;
}

export interface WeaponAmmo {
  recordName: string;
  /** FPS only: the magazine entity the ammo was found through. */
  magazine?: string;
  speed?: number;
  lifetime?: number;
  range?: number;
  /** Per projectile. A scattergun's alpha is this times its pellet count. */
  damage?: DamageInfo;
  damagePerShot?: number;
  penetration?: number;
  penetrationRadius?: Ammo["penetrationRadius"];
  /** Personal weapons only: damage drop with distance, per type. */
  falloff?: Ammo["falloff"];
  pierce?: Ammo["pierce"];
  /** Rounds carried: the embedded container, or the default magazine. */
  capacity?: number;
}

export interface Weapon {
  name: string;
  recordName: string;
  id: string;
  class: "ship" | "fps";
  isTemplate?: true;
  type?: string;
  subType?: string;
  size?: number;
  grade?: number;
  manufacturer?: string;
  /** Dominant damage type of the ammo, as a loadout site would label it. */
  damageType?: string;
  // The headline columns, flattened so a diff reads "dps: 1537 -> 1600".
  dps?: number;
  alpha?: number;
  fireRate?: number;
  range?: number;
  speed?: number;
  spread?: number;
  penetration?: number;
  power?: number;
  em?: number;
  ir?: number;
  ammoCapacity?: number;
  health?: number;
  /** Resistances, self-repair and wear behind `health`. */
  durability?: Durability;
  mass?: number;
  heatPerShot?: number;
  overheatTemperature?: number;
  coolingPerSecond?: number;
  /** Seconds of continuous fire before overheating; absent when cooling keeps up. */
  overheatSeconds?: number;
  /** Energy weapons: the shot pool and how it refills. */
  capacitor?: Capacitor;
  /** One full trigger hold: until the capacitor or magazine empties, or the gun overheats. */
  burst?: Burst;
  /** Damage per second over repeated fire-and-recover cycles. See `sustainedDps`. */
  sustainedDps?: number;
  /** Inventory volume, micro-SCU. */
  volume?: number;
  distortion?: Distortion;
  ammo?: WeaponAmmo;
  modes: FireMode[];
}

export interface Capacitor {
  /** Shots the pool holds when full. */
  shots?: number;
  /** Shots regained per second while not firing. */
  regenPerSecond?: number;
  /** Seconds after the last shot before regeneration starts. */
  regenCooldown?: number;
  /** Energy cost of one shot, and the energy rate the gun asks the power network for. */
  costPerShot?: number;
  requestedRegenPerSecond?: number;
}

export interface Burst {
  shots: number;
  seconds: number;
  damage: number;
  /** What ends it. */
  limit: "capacitor" | "magazine" | "heat";
}

export interface SustainInput {
  alpha?: number;
  /** Trigger pulls per minute. */
  fireRate?: number;
  capacitor?: Capacitor;
  /** Rounds carried, for ballistics. */
  magazine?: number;
  heatPerShot?: number;
  overheatTemperature?: number;
  coolingPerSecond?: number;
  /** Seconds locked out after an overheat. */
  overheatFixTime?: number;
}

/**
 * Burst and sustained output from the pool that limits a gun.
 *
 * Energy guns fire from a shot pool that only refills when the trigger is
 * released, after `regenCooldown`. One cycle is: empty the pool, wait the
 * cooldown, refill it. Sustained DPS is the pool's damage over that cycle.
 *
 * Ballistic guns are limited by heat or the magazine. Heat builds by
 * `heatPerShot` and bleeds by `coolingPerSecond`; when the net rate is
 * positive the gun overheats after `overheatTemperature / net` seconds and is
 * locked out for `overheatFixTime`. Cooling while firing is the assumption
 * here; if the game pauses it until `timeTillCoolingStarts`, the true burst is
 * shorter. Magazine-limited guns have no recovery cycle short of a restock, so
 * their sustained figure is their DPS until the magazine is gone.
 */
export function sustain(input: SustainInput): { burst?: Burst; sustainedDps?: number; overheatSeconds?: number } {
  const { alpha, fireRate } = input;
  if (!alpha || !fireRate) return {};
  const perSecond = fireRate / 60;
  const shotsIn = (seconds: number) => Math.floor(seconds * perSecond + 1e-9) || 1;

  let overheatSeconds: number | undefined;
  if (input.heatPerShot && input.overheatTemperature) {
    const net = input.heatPerShot * perSecond - (input.coolingPerSecond ?? 0);
    if (net > 0) overheatSeconds = round(input.overheatTemperature / net);
  }

  const pool = input.capacitor?.shots;
  if (pool) {
    const seconds = pool / perSecond;
    const regen = input.capacitor?.regenPerSecond;
    const burst: Burst = { shots: pool, seconds: round(seconds), damage: round(pool * alpha), limit: "capacitor" };
    const cycle = regen ? seconds + (input.capacitor?.regenCooldown ?? 0) + pool / regen : undefined;
    return { burst, sustainedDps: cycle ? round((pool * alpha) / cycle) : undefined, overheatSeconds };
  }

  const magazineSeconds = input.magazine ? input.magazine / perSecond : undefined;
  if (overheatSeconds !== undefined && (magazineSeconds === undefined || overheatSeconds < magazineSeconds)) {
    const shots = shotsIn(overheatSeconds);
    const cycle = overheatSeconds + (input.overheatFixTime ?? 0);
    return {
      burst: { shots, seconds: overheatSeconds, damage: round(shots * alpha), limit: "heat" },
      sustainedDps: round((shots * alpha) / cycle),
      overheatSeconds,
    };
  }
  if (input.magazine && magazineSeconds !== undefined) {
    return {
      burst: { shots: input.magazine, seconds: round(magazineSeconds), damage: round(input.magazine * alpha), limit: "magazine" },
      sustainedDps: round(alpha * perSecond),
      overheatSeconds,
    };
  }
  return { overheatSeconds };
}

function parseCapacitor(params: Record<string, unknown>): Capacitor | undefined {
  const regen = params.weaponRegenConsumerParams;
  if (!regen || typeof regen !== "object") return undefined;
  const r = regen as Record<string, unknown>;
  const capacitor: Capacitor = {
    shots: num(r.maxAmmoLoad),
    regenPerSecond: num(r.maxRegenPerSec),
    regenCooldown: num(r.regenerationCooldown),
    costPerShot: num(r.regenerationCostPerBullet),
    requestedRegenPerSecond: num(r.requestedRegenPerSec),
  };
  return capacitor.shots ? capacitor : undefined;
}

export interface Missile {
  name: string;
  recordName: string;
  id: string;
  isTemplate?: true;
  size?: number;
  manufacturer?: string;
  damage?: DamageInfo;
  damageTotal?: number;
  damageType?: string;
  trackingSignal?: string;
  lockTime?: number;
  lockRangeMin?: number;
  lockRangeMax?: number;
  lockingAngle?: number;
  speed?: number;
  boostDuration?: number;
  maxLifetime?: number;
  explosionRadius?: number;
  health?: number;
}

const round = (value: number, places = 2): number => {
  const factor = 10 ** places;
  return Math.round(value * factor) / factor;
};

export function totalDamage(damage: DamageInfo | undefined): number | undefined {
  if (!damage) return undefined;
  return round(Object.values(damage).reduce((sum, x) => sum + x, 0));
}

export function dominantDamageType(damage: DamageInfo | undefined): string | undefined {
  if (!damage) return undefined;
  let best: [string, number] | undefined;
  for (const [type, amount] of Object.entries(damage)) {
    if (!best || amount > best[1]) best = [type, amount];
  }
  return best?.[0];
}


function parseSpread(value: unknown): Spread | undefined {
  if (!value || typeof value !== "object") return undefined;
  const v = value as Record<string, unknown>;
  const spread: Spread = {
    min: num(v.min),
    max: num(v.max),
    attack: num(v.attack),
    decay: num(v.decay),
  };
  return Object.values(spread).some((x) => x !== undefined) ? spread : undefined;
}

/**
 * Effective rounds per minute of a sequence. Entries alternate (barrels, or a
 * sniper's shot then bolt cycle), each with a delay in RPM or seconds, so the
 * cadence is total shots over total time rather than any one entry's figure.
 */
export function sequenceFireRate(entries: Record<string, unknown>[]): number | undefined {
  let shots = 0;
  let seconds = 0;
  for (const entry of entries) {
    const delay = num(entry.delay);
    const repetitions = num(entry.repetitions) ?? 1;
    if (delay === undefined || delay <= 0) continue;
    const perShot = str(entry.unit) === "Seconds" ? delay : 60 / delay;
    shots += repetitions;
    seconds += perShot * repetitions;
  }
  return shots > 0 && seconds > 0 ? round((60 * shots) / seconds) : undefined;
}

/** The numbers one projectile-launching action contributes. */
function launchStats(action: Record<string, unknown>) {
  const launch = (action.launchParams ?? {}) as Record<string, unknown>;
  return {
    fireRate: num(action.fireRate),
    heatPerShot: num(action.heatPerShot),
    pellets: num(launch.pelletCount),
    damageMultiplier: num(launch.damageMultiplier),
    spread: parseSpread(launch.spreadParams),
  };
}

export function parseFireMode(
  action: Record<string, unknown>,
  ammo: Ammo | undefined,
): FireMode | undefined {
  const type = str(action._Type_);
  if (!type) return undefined;
  const name = str(action.name) ?? type.replace(/^SWeaponActionFire|Params$/g, "");
  const perShot = ammo?.damageTotal;

  const finish = (mode: FireMode, rateForDps = mode.fireRate): FireMode => {
    if (perShot !== undefined) {
      const alpha = perShot * (mode.pellets ?? 1) * (mode.damageMultiplier ?? 1);
      mode.alpha = round(alpha);
      if (rateForDps) mode.dps = round((alpha * rateForDps) / 60);
    }
    return mode;
  };

  switch (type) {
    case "SWeaponActionFireSingleParams":
    case "SWeaponActionFireRapidParams":
      return finish({
        name,
        kind: type.includes("Rapid") ? "rapid" : "single",
        ...launchStats(action),
      });

    case "SWeaponActionFireBurstParams": {
      const stats = launchStats(action);
      const shotCount = num(action.shotCount);
      const cooldown = num(action.cooldownTime) ?? 0;
      let sustained: number | undefined;
      if (stats.fireRate && shotCount) {
        const burstSeconds = (shotCount * 60) / stats.fireRate + cooldown;
        sustained = round((60 * shotCount) / burstSeconds);
      }
      return finish(
        { name, kind: "burst", ...stats, shotCount, sustainedFireRate: sustained },
        sustained ?? stats.fireRate,
      );
    }

    case "SWeaponActionFireChargedParams": {
      const inner = (action.weaponAction ?? {}) as Record<string, unknown>;
      const stats = launchStats(inner);
      return finish({
        name,
        kind: "charged",
        ...stats,
        chargeTime: num(action.chargeTime),
        chargedDamageMultiplier: num(at(action, "maxChargeModifier.damageMultiplier")),
      });
    }

    case "SWeaponActionSequenceParams": {
      const entries = (Array.isArray(action.sequenceEntries) ? action.sequenceEntries : []).filter(
        (e): e is Record<string, unknown> => !!e && typeof e === "object",
      );
      const first = entries
        .map((e) => e.weaponAction)
        .find((a): a is Record<string, unknown> => !!a && typeof a === "object");
      const stats = first ? launchStats(first) : ({} as ReturnType<typeof launchStats>);
      const rate = sequenceFireRate(entries) ?? stats.fireRate;
      // Sequences are often unnamed; the nested action usually says "Single".
      const label = str(action.name) ?? (first ? str(first.name) : undefined) ?? "Sequence";
      return finish({ name: label, kind: "sequence", ...stats, fireRate: rate });
    }

    case "SWeaponActionFireBeamParams": {
      const damage = parseBeamDamage(action.damagePerSecond);
      return {
        name,
        kind: "beam",
        heatPerShot: num(action.heatPerSecond),
        fullDamageRange: num(action.fullDamageRange),
        zeroDamageRange: num(action.zeroDamageRange),
        dps: totalDamage(damage),
      };
    }

    default:
      // Tractor, salvage, healing, mining and the like: present, but no
      // damage figure to derive. Named so the mode list stays honest.
      return { name, kind: "other" };
  }
}

function parseBeamDamage(value: unknown): DamageInfo | undefined {
  if (!value || typeof value !== "object") return undefined;
  const v = value as Record<string, unknown>;
  const out: DamageInfo = {};
  const fields: [keyof DamageInfo, string][] = [
    ["physical", "DamagePhysical"],
    ["energy", "DamageEnergy"],
    ["distortion", "DamageDistortion"],
    ["thermal", "DamageThermal"],
    ["biochemical", "DamageBiochemical"],
    ["stun", "DamageStun"],
  ];
  for (const [key, field] of fields) {
    const amount = num(v[field]);
    if (amount) out[key] = amount;
  }
  return Object.keys(out).length > 0 ? out : undefined;
}

/** Index ammo by the short name a `file://.../x_ammo.json` reference yields. */
export function indexAmmo(records: GameRecord[]): Map<string, Ammo> {
  const index = new Map<string, Ammo>();
  for (const record of records) {
    if (record.type !== "AmmoParams") continue;
    index.set(record.shortName.toLowerCase(), parseAmmo(record));
  }
  return index;
}

function ammoFromContainer(
  container: Record<string, unknown> | undefined,
  ammoIndex: Map<string, Ammo>,
): { ammo?: Ammo; capacity?: number } {
  const name = referencedName(container?.ammoParamsRecord);
  if (!name) return {};
  return { ammo: ammoIndex.get(name.toLowerCase()), capacity: num(container?.maxAmmoCount) };
}

/** The magazine an FPS weapon ships with, named in its default loadout. */
export function defaultMagazineName(record: GameRecord): string | undefined {
  const loadout = component(record, "SEntityComponentDefaultLoadoutParams");
  const entries = at(loadout, "loadout.entries");
  if (!Array.isArray(entries)) return undefined;
  for (const entry of entries) {
    if (!entry || typeof entry !== "object") continue;
    const e = entry as Record<string, unknown>;
    if (str(e.itemPortName)?.toLowerCase().includes("magazine")) {
      return str(e.entityClassName);
    }
  }
  return undefined;
}

export interface WeaponContext {
  localization: Localization;
  ammoIndex: Map<string, Ammo>;
  /** Entities by lower-cased short name, for resolving magazine loadouts. */
  entities: Map<string, GameRecord>;
}

export function parseWeapon(
  record: GameRecord,
  weaponClass: "ship" | "fps",
  context: WeaponContext,
): Weapon | undefined {
  const params = component(record, "SCItemWeaponComponentParams");
  if (!params) return undefined;

  const attach = component(record, "SAttachableComponentParams");
  const def = (attach?.AttachDef ?? {}) as Record<string, unknown>;
  const localized = localize(context.localization, at(def, "Localization.Name"));
  // Store display props reuse a real gun's name but fire nothing.
  const placeholder = isPlaceholderName(localized) || /_prop$/i.test(record.shortName);

  // Ship guns carry their ammo container; FPS guns borrow the magazine's.
  let linked = ammoFromContainer(component(record, "SAmmoContainerComponentParams"), context.ammoIndex);
  let magazine: string | undefined;
  if (!linked.ammo) {
    magazine = defaultMagazineName(record);
    const magazineRecord = magazine ? context.entities.get(magazine.toLowerCase()) : undefined;
    if (magazineRecord) {
      linked = ammoFromContainer(
        component(magazineRecord, "SAmmoContainerComponentParams"),
        context.ammoIndex,
      );
    }
  }
  const { ammo, capacity } = linked;

  const actions = (Array.isArray(params.fireActions) ? params.fireActions : []).filter(
    (a): a is Record<string, unknown> => !!a && typeof a === "object",
  );
  const modes = actions
    .map((a) => parseFireMode(a, ammo))
    .filter((m): m is FireMode => m !== undefined);

  const best = modes.reduce<FireMode | undefined>(
    (top, mode) => (mode.dps !== undefined && (top?.dps ?? -1) < mode.dps ? mode : top),
    undefined,
  );
  const headline = best ?? modes[0];

  const resources = resourceProfile(record);
  const heat = at(params, "connectionParams.simplifiedHeatParams") as Record<string, unknown> | undefined;
  const capacitor = parseCapacitor(params);
  const rounds = capacity && capacity > 0 ? capacity : undefined;
  const sustained = sustain({
    alpha: headline?.alpha,
    fireRate: headline?.sustainedFireRate ?? headline?.fireRate,
    capacitor,
    magazine: rounds,
    heatPerShot: headline?.heatPerShot,
    overheatTemperature: num(heat?.overheatTemperature),
    coolingPerSecond: num(heat?.coolingPerSecond),
    overheatFixTime: num(heat?.overheatFixTime),
  });

  const weapon: Weapon = {
    name: placeholder ? record.shortName : (localized as string),
    recordName: record.name,
    id: record.id,
    class: weaponClass,
    isTemplate: placeholder || undefined,
    type: str(def.Type),
    subType: str(def.SubType) === "UNDEFINED" ? undefined : str(def.SubType),
    size: num(def.Size),
    grade: num(def.Grade),
    manufacturer: manufacturerOf(def),
    damageType: dominantDamageType(ammo?.damage),
    dps: headline?.dps,
    alpha: headline?.alpha,
    fireRate: headline?.sustainedFireRate ?? headline?.fireRate,
    range: ammo?.range ?? headline?.zeroDamageRange,
    speed: ammo?.speed,
    spread: headline?.spread?.min ?? headline?.spread?.max,
    penetration: ammo?.penetration,
    power: resources.power,
    em: resources.em,
    ir: resources.ir,
    ammoCapacity: rounds ?? capacitor?.shots,
    health: num(at(component(record, "SHealthComponentParams"), "Health")),
    durability: durabilityOf(record),
    mass: num(at(component(record, "SEntityPhysicsControllerParams"), "PhysType.Mass")),
    heatPerShot: headline?.heatPerShot,
    overheatTemperature: num(heat?.overheatTemperature),
    coolingPerSecond: num(heat?.coolingPerSecond),
    overheatSeconds: sustained.overheatSeconds,
    capacitor,
    burst: sustained.burst,
    sustainedDps: sustained.sustainedDps,
    volume: volumeOf(record),
    distortion: distortionOf(record),
    ammo: ammo
      ? {
          recordName: ammo.recordName,
          magazine,
          speed: ammo.speed,
          lifetime: ammo.lifetime,
          range: ammo.range,
          damage: ammo.damage,
          damagePerShot: ammo.damageTotal,
          penetration: ammo.penetration,
          penetrationRadius: ammo.penetrationRadius,
          falloff: ammo.falloff,
          pierce: ammo.pierce,
          capacity: capacity && capacity > 0 ? capacity : undefined,
        }
      : undefined,
    modes,
  };

  return weapon;
}

export function parseMissile(record: GameRecord, localization: Localization): Missile | undefined {
  const params = component(record, "SCItemMissileParams");
  if (!params) return undefined;

  const attach = component(record, "SAttachableComponentParams");
  const def = (attach?.AttachDef ?? {}) as Record<string, unknown>;
  const localized = localize(localization, at(def, "Localization.Name"));
  const placeholder = isPlaceholderName(localized);

  const damage = parseBeamDamage(at(params, "explosionParams.damage"));
  const targeting = (params.targetingParams ?? {}) as Record<string, unknown>;
  const gcs = (params.GCSParams ?? {}) as Record<string, unknown>;

  return {
    name: placeholder ? record.shortName : (localized as string),
    recordName: record.name,
    id: record.id,
    isTemplate: placeholder || undefined,
    size: num(def.Size),
    manufacturer: manufacturerOf(def),
    damage,
    damageTotal: totalDamage(damage),
    damageType: dominantDamageType(damage),
    trackingSignal: str(targeting.trackingSignalType),
    lockTime: num(targeting.lockTime),
    lockRangeMin: num(targeting.lockRangeMin),
    lockRangeMax: num(targeting.lockRangeMax),
    lockingAngle: num(targeting.lockingAngle),
    speed: num(gcs.linearSpeed),
    boostDuration: num(gcs.boostPhaseDuration),
    maxLifetime: num(params.maxLifetime),
    explosionRadius: num(at(params, "explosionParams.maxRadius")),
    health: num(at(component(record, "SHealthComponentParams"), "Health")),
  };
}

const SHIP_WEAPONS = "**/entities/scitem/ships/weapons/**";
const FPS_WEAPONS = "**/entities/scitem/weapons/**";

export const weaponsDomain: Domain = {
  name: "game-weapons",
  description:
    "Combat baseline: ship and FPS weapons joined to their ammo (DPS, alpha, rate, range, power, EM, HP) plus missiles.",
  filters: [SHIP_WEAPONS, FPS_WEAPONS, "**/ammoparams/**"],
  needsLocalization: true,

  parse({ records, localization }) {
    const ammoIndex = indexAmmo(records);
    const entities = new Map<string, GameRecord>();
    for (const record of records) {
      if (record.type === "EntityClassDefinition") entities.set(record.shortName.toLowerCase(), record);
    }
    const context: WeaponContext = { localization, ammoIndex, entities };

    const shipWeapons: Weapon[] = [];
    const fpsWeapons: Weapon[] = [];
    const missiles: Missile[] = [];

    for (const record of records) {
      if (record.type !== "EntityClassDefinition") continue;
      const path = record.recordPath.toLowerCase();

      if (path.includes("/scitem/ships/weapons/")) {
        const missile = parseMissile(record, localization);
        if (missile) {
          missiles.push(missile);
          continue;
        }
        const weapon = parseWeapon(record, "ship", context);
        if (weapon) shipWeapons.push(weapon);
      } else if (path.includes("/scitem/weapons/")) {
        const weapon = parseWeapon(record, "fps", context);
        if (weapon) fpsWeapons.push(weapon);
      }
    }

    const byName = <T extends { name: string; size?: number }>(a: T, b: T) =>
      (a.size ?? 0) - (b.size ?? 0) || a.name.localeCompare(b.name);
    shipWeapons.sort(byName);
    fpsWeapons.sort(byName);
    missiles.sort(byName);

    const withDps = (list: Weapon[]) => list.filter((w) => w.dps !== undefined).length;

    return {
      shipWeapons,
      fpsWeapons,
      missiles,
      summary: {
        shipWeapons: shipWeapons.length,
        shipWeaponsWithDps: withDps(shipWeapons),
        fpsWeapons: fpsWeapons.length,
        fpsWeaponsWithDps: withDps(fpsWeapons),
        missiles: missiles.length,
        ammo: ammoIndex.size,
      },
    };
  },
};
