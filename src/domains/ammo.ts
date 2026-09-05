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

import { num, type GameRecord } from "../records.js";
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
