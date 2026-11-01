/**
 * Deflection check: which ship weapons get through a ship's stock armour.
 *
 * Armour carries a per-type deflection value (`armorDeflection.deflectionValue`)
 * and a per-type damage multiplier. A hit whose damage of a type does not
 * exceed that type's deflection is treated as deflected for that type; damage
 * that does exceed it is scaled by the multiplier. The check is per projectile,
 * since that is the unit of a hit: a scattergun's pellets are tested one by one.
 *
 * Distortion is kept apart. It drains shields and fills component distortion
 * pools rather than damaging hull, and armour's distortion deflection reads 0,
 * so counting it as hull damage would make every distortion gun "penetrate".
 *
 * Two simplifications, both stated so the numbers are read correctly:
 * - Deflection is treated as a threshold, not subtracted from the hit.
 * - Armour at full health. The game scales deflection down as armour health
 *   drops, which this check does not model.
 */

import type { Armor } from "./domains/ships.js";
import type { Weapon } from "./domains/weapons.js";

export interface DeflectionRow {
  name: string;
  recordName: string;
  size?: number;
  /** Per projectile, by damage type. */
  damage: Record<string, number>;
  /** Types whose damage is at or below the armour's deflection value. */
  deflected: string[];
  penetrates: boolean;
  /** Hull damage one projectile does after deflection and multipliers. */
  appliedPerHit: number;
  /** Distortion per projectile: never hull damage, reported on its own. */
  distortion?: number;
  /** `appliedPerHit` at the weapon's rate, times pellets. */
  appliedDps?: number;
  dps?: number;
}

const round = (value: number) => Math.round(value * 100) / 100;

export function checkWeapon(weapon: Weapon, armor: Armor): DeflectionRow | undefined {
  const damage = weapon.ammo?.damage as Record<string, number> | undefined;
  if (!damage || Object.keys(damage).length === 0) return undefined;

  const deflected: string[] = [];
  let applied = 0;
  for (const [type, amount] of Object.entries(damage)) {
    if (!amount || type === "distortion") continue;
    const threshold = armor.deflection?.[type] ?? 0;
    if (amount <= threshold) {
      deflected.push(type);
      continue;
    }
    applied += amount * (armor.damageMultiplier?.[type] ?? 1);
  }

  const mode = weapon.modes.find((m) => m.dps === weapon.dps) ?? weapon.modes[0];
  const pellets = mode?.pellets ?? 1;
  const multiplier = mode?.damageMultiplier ?? 1;
  const perSecond = weapon.fireRate ? weapon.fireRate / 60 : undefined;

  return {
    name: weapon.name,
    recordName: weapon.recordName,
    size: weapon.size,
    damage,
    deflected,
    distortion: damage.distortion || undefined,
    penetrates: applied > 0,
    appliedPerHit: round(applied * multiplier),
    appliedDps: perSecond !== undefined ? round(applied * multiplier * pellets * perSecond) : undefined,
    dps: weapon.dps,
  };
}

export function deflectionCheck(
  armor: Armor,
  weapons: Weapon[],
  options: { size?: number } = {},
): DeflectionRow[] {
  return weapons
    .filter((w) => !w.isTemplate && (options.size === undefined || w.size === options.size))
    .map((w) => checkWeapon(w, armor))
    .filter((r): r is DeflectionRow => r !== undefined)
    .sort((a, b) => (b.appliedDps ?? 0) - (a.appliedDps ?? 0) || a.name.localeCompare(b.name));
}

export function renderDeflection(ship: string, armor: Armor, rows: DeflectionRow[], limit = 30): string {
  const defl = Object.entries(armor.deflection ?? {})
    .filter(([, v]) => v > 0)
    .map(([k, v]) => `${k} ${v}`)
    .join(", ");
  const mult = Object.entries(armor.damageMultiplier ?? {})
    .filter(([, v]) => v !== 1)
    .map(([k, v]) => `${k} x${v}`)
    .join(", ");
  const lines = [
    `${ship}  armour ${armor.item}  health ${armor.health ?? "?"}`,
    `  deflects at or below: ${defl || "nothing"}`,
    `  hull damage multipliers: ${mult || "none"}`,
    "",
    "  size  weapon                                   per hit   applied  applied dps  listed dps",
  ];
  const penetrating = rows.filter((r) => r.penetrates);
  const distortion = rows.filter((r) => !r.penetrates && r.distortion);
  const blocked = rows.filter((r) => !r.penetrates && !r.distortion);
  for (const row of penetrating.slice(0, limit)) {
    const perHit = Object.entries(row.damage)
      .filter(([type]) => type !== "distortion")
      .reduce((a, [, b]) => a + b, 0);
    lines.push(
      `  S${String(row.size ?? "?").padEnd(3)} ${row.name.slice(0, 40).padEnd(40)} ${String(round(perHit)).padStart(8)} ${String(row.appliedPerHit).padStart(9)} ${String(row.appliedDps ?? "").padStart(12)} ${String(row.dps ?? "").padStart(11)}${row.deflected.length > 0 ? `  (${row.deflected.join(", ")} deflected)` : ""}`,
    );
  }
  if (penetrating.length > limit) lines.push(`  ... ${penetrating.length - limit} more penetrate`);
  lines.push("", `  ${penetrating.length} penetrate, ${blocked.length} fully deflected, ${distortion.length} distortion only`);
  if (blocked.length > 0) {
    lines.push(`  deflected: ${blocked.slice(0, limit).map((r) => `${r.name} (S${r.size})`).join(", ")}${blocked.length > limit ? ", ..." : ""}`);
  }
  if (distortion.length > 0) {
    lines.push(`  distortion only (shields and components, not hull): ${distortion.slice(0, limit).map((r) => `${r.name} (S${r.size})`).join(", ")}`);
  }
  lines.push("", "  Full-health armour; deflection treated as a per-projectile threshold.");
  return lines.join("\n");
}
