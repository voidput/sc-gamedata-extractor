/**
 * Ammunition, which is where weapon damage actually lives.
 *
 * A weapon record carries fire rate and heat but no damage figure. The damage is
 * on the ammo it fires, at `projectileParams.damage`, as a `DamageInfo` split
 * across five types. Joining the two is what produces a real DPS number, which
 * is why this is its own dataset rather than a field on the weapon.
 *
 * The `DeathExplosionParams.damage` found on weapon records is the explosion
 * when the weapon itself is destroyed, and reads zero on nearly everything. It
 * is not the weapon's damage.
 */

import { at, num, type GameRecord } from "../records.js";
import type { Domain } from "./types.js";

/** The five damage types the game models. */
export interface DamageInfo {
  physical?: number;
  energy?: number;
  distortion?: number;
  thermal?: number;
  biochemical?: number;
  stun?: number;
}

export interface Ammo {
  name: string;
  recordName: string;
  id: string;
  size?: number;
  speed?: number;
  lifetime?: number;
  /** Metres, derived from speed and lifetime, which is the effective range. */
  range?: number;
  damage?: DamageInfo;
  damageTotal?: number;
  /** Metres of material a round punches through, the "pen" a loadout site shows. */
  penetration?: number;
  /** Radius of the penetration cone at the entry and far end, metres. */
  penetrationRadius?: { near?: number; far?: number };
  /**
   * Damage falloff with distance, per type. Damage holds until `minDistance`,
   * then drops by `perMeter` until it reaches `floor`. Only personal-weapon
   * ammo carries this; ship ammo does full damage to the end of its range.
   */
  falloff?: Record<string, { minDistance: number; perMeter: number; floor: number }>;
  /**
   * How a round passes through thin geometry: the thickest material it can
   * cross, and the damage fraction kept at each pierce level.
   */
  pierce?: { maxThickness?: number; level1?: number; level2?: number; level3?: number };
}

const DAMAGE_FIELDS: [keyof DamageInfo, string][] = [
  ["physical", "DamagePhysical"],
  ["energy", "DamageEnergy"],
  ["distortion", "DamageDistortion"],
  ["thermal", "DamageThermal"],
  ["biochemical", "DamageBiochemical"],
  ["stun", "DamageStun"],
];

export function parseDamageInfo(value: unknown): DamageInfo | undefined {
  if (!value || typeof value !== "object") return undefined;
  const obj = value as Record<string, unknown>;

  const damage: DamageInfo = {};
  for (const [key, field] of DAMAGE_FIELDS) {
    const amount = num(obj[field]);
    // Zero is the norm across most types; only carry what actually deals damage.
    if (amount !== undefined && amount !== 0) damage[key] = amount;
  }

  return Object.keys(damage).length > 0 ? damage : undefined;
}

/** Find the first non-zero DamageInfo nested anywhere under a value. */
export function findDamage(value: unknown): DamageInfo | undefined {
  if (Array.isArray(value)) {
    for (const entry of value) {
      const hit = findDamage(entry);
      if (hit) return hit;
    }
    return undefined;
  }
  if (!value || typeof value !== "object") return undefined;

  const obj = value as Record<string, unknown>;
  if (obj._Type_ === "DamageInfo") {
    const parsed = parseDamageInfo(obj);
    if (parsed) return parsed;
  }

  for (const entry of Object.values(obj)) {
    const hit = findDamage(entry);
    if (hit) return hit;
  }
  return undefined;
}

/** Per-type falloff from `BulletDamageDropParams`, keeping only types that fall off. */
export function parseFalloff(value: unknown): Ammo["falloff"] {
  if (!value || typeof value !== "object") return undefined;
  const out: NonNullable<Ammo["falloff"]> = {};
  for (const [key, field] of DAMAGE_FIELDS) {
    const perMeter = num(at(value, `damageDropPerMeter.${field}`)) ?? 0;
    if (perMeter <= 0) continue;
    out[key] = {
      minDistance: num(at(value, `damageDropMinDistance.${field}`)) ?? 0,
      perMeter,
      floor: num(at(value, `damageDropMinDamage.${field}`)) ?? 0,
    };
  }
  return Object.keys(out).length > 0 ? out : undefined;
}

function parsePierce(value: unknown): Ammo["pierce"] {
  if (!value || typeof value !== "object") return undefined;
  const pierce = {
    maxThickness: num(at(value, "maxPenetrationThickness")),
    level1: num(at(value, "damageFalloffLevel1")),
    level2: num(at(value, "damageFalloffLevel2")),
    level3: num(at(value, "damageFalloffLevel3")),
  };
  return Object.values(pierce).some((v) => v) ? pierce : undefined;
}

export function parseAmmo(record: GameRecord): Ammo {
  const v = record.value;
  const damage = findDamage(v.projectileParams) ?? findDamage(v);
  const speed = num(v.speed);
  const lifetime = num(v.lifetime);

  const total = damage
    ? Math.round(Object.values(damage).reduce((sum, x) => sum + x, 0) * 100) / 100
    : undefined;

  return {
    name: record.shortName,
    recordName: record.name,
    id: record.id,
    size: num(v.size),
    speed,
    lifetime,
    range: speed && lifetime ? Math.round(speed * lifetime) : undefined,
    damage,
    damageTotal: total,
    penetration: num(at(v, "projectileParams.penetrationParams.basePenetrationDistance")),
    penetrationRadius: (() => {
      const near = num(at(v, "projectileParams.penetrationParams.nearRadius"));
      const far = num(at(v, "projectileParams.penetrationParams.farRadius"));
      return near !== undefined || far !== undefined ? { near, far } : undefined;
    })(),
    falloff: parseFalloff(at(v, "projectileParams.damageDropParams")),
    pierce: parsePierce(at(v, "projectileParams.pierceabilityParams")),
  };
}

export const ammoDomain: Domain = {
  name: "game-ammo",
  description: "Ammunition damage, projectile speed and range. Join to weapons for DPS.",
  filters: ["**/ammoparams/**"],

  parse({ records }) {
    const ammo = records
      .filter((r) => r.type === "AmmoParams")
      .map(parseAmmo)
      .sort((a, b) => a.name.localeCompare(b.name));

    const damaging = ammo.filter((a) => (a.damageTotal ?? 0) > 0);

    return {
      ammo,
      summary: {
        ammo: ammo.length,
        withDamage: damaging.length,
        maxDamage: damaging.reduce((max, a) => Math.max(max, a.damageTotal ?? 0), 0),
      },
    };
  },
};
