/**
 * Mining: mineable element physics, and the laser modifiers that work them.
 *
 * A MineableElement holds the physics of breaking a rock: how violently it
 * reacts to power (instability), how much power it resists, and where the
 * optimal charge window sits. Those decide whether a given laser can crack a
 * given rock, which is what a mining tool actually needs.
 *
 * Laser and module tuning is not a top-level field. It sits at
 * `Components[].modifiers[].MiningLaserModifier`, a MiningLaserModifiers struct
 * whose entries are null on records that do not modify that stat.
 */

import { num, refShortName, str, type GameRecord } from "../records.js";
import type { Domain } from "./types.js";

export interface MineableElement {
  name: string;
  recordName: string;
  id: string;
  instability?: number;
  resistance?: number;
  optimalWindowMidpoint?: number;
  optimalWindowRandomness?: number;
  optimalWindowThinness?: number;
  explosionMultiplier?: number;
  clusterFactor?: number;
}

export function parseMineableElement(record: GameRecord): MineableElement {
  const v = record.value;
  return {
    // The resourceType reference carries the outward-facing name ("Raw_Gold");
    // the record's own name is the internal one ("Gold_Raw").
    name: refShortName(v.resourceType) ?? record.shortName,
    recordName: record.name,
    id: record.id,
    instability: num(v.elementInstability),
    resistance: num(v.elementResistance),
    optimalWindowMidpoint: num(v.elementOptimalWindowMidpoint),
    optimalWindowRandomness: num(v.elementOptimalWindowMidpointRandomness),
    optimalWindowThinness: num(v.elementOptimalWindowThinness),
    explosionMultiplier: num(v.elementExplosionMultiplier),
    clusterFactor: num(v.elementClusterFactor),
  };
}

/**
 * A single modifier. The game wraps these rather than storing bare numbers:
 *
 *   "laserInstability": {
 *     "_Type_": "FloatModifierMultiplicative", "showInUI": true, "value": -10.0
 *   }
 *
 * The wrapper type decides how the value applies, so both are kept. Reading
 * these as plain numbers yields nothing at all.
 */
export interface Modifier {
  value: number;
  /** "multiplicative" (a percentage) or "additive" (a flat offset). */
  kind: "multiplicative" | "additive" | "unknown";
  showInUI?: boolean;
}

/** Field names as they appear in the game data, including its own casing. */
export interface LaserModifiers {
  laserInstability?: Modifier;
  resistanceModifier?: Modifier;
  optimalChargeWindowSizeModifier?: Modifier;
  optimalChargeWindowRateModifier?: Modifier;
  catastrophicChargeWindowRateModifier?: Modifier;
  shatterDamageModifier?: Modifier;
  clusterFactorModifier?: Modifier;
  isOptimalRateGood?: boolean;
}

export function parseModifier(raw: unknown): Modifier | undefined {
  if (!raw || typeof raw !== "object") return undefined;
  const obj = raw as Record<string, unknown>;
  const value = num(obj.value);
  if (value === undefined) return undefined;

  const type = str(obj._Type_) ?? "";
  const kind = type.includes("Multiplicative")
    ? "multiplicative"
    : type.includes("Additive")
      ? "additive"
      : "unknown";

  return {
    value,
    kind,
    ...(typeof obj.showInUI === "boolean" ? { showInUI: obj.showInUI } : {}),
  };
}

export interface MiningModule {
  name: string;
  recordName: string;
  id: string;
  modifiers: LaserModifiers;
}

/** Collect every MiningLaserModifier struct nested anywhere in a record. */
export function collectLaserModifiers(value: unknown, found: LaserModifiers[] = []) {
  if (Array.isArray(value)) {
    for (const entry of value) collectLaserModifiers(entry, found);
    return found;
  }
  if (!value || typeof value !== "object") return found;

  const obj = value as Record<string, unknown>;
  const raw = obj.MiningLaserModifier;
  if (raw && typeof raw === "object") {
    const m = raw as Record<string, unknown>;
    const parsed: LaserModifiers = {
      laserInstability: parseModifier(m.laserInstability),
      resistanceModifier: parseModifier(m.resistanceModifier),
      optimalChargeWindowSizeModifier: parseModifier(m.optimalChargeWindowSizeModifier),
      optimalChargeWindowRateModifier: parseModifier(m.optimalChargeWindowRateModifier),
      catastrophicChargeWindowRateModifier: parseModifier(m.catastrophicChargeWindowRateModifier),
      // The game data spells this with a lowercase d.
      shatterDamageModifier: parseModifier(m.shatterdamageModifier ?? m.shatterDamageModifier),
      clusterFactorModifier: parseModifier(m.clusterFactorModifier),
      isOptimalRateGood: typeof m.isOptimalRateGood === "boolean" ? m.isOptimalRateGood : undefined,
    };
    // Records that modify nothing carry an all-null struct; skip those.
    if (Object.values(parsed).some((x) => x !== undefined && typeof x !== "boolean")) {
      found.push(parsed);
    }
  }

  for (const entry of Object.values(obj)) collectLaserModifiers(entry, found);
  return found;
}

/** Merge the modifier structs on a record, first defined value winning. */
function mergeModifiers(all: LaserModifiers[]): LaserModifiers {
  const merged: LaserModifiers = {};
  for (const one of all) {
    for (const [key, value] of Object.entries(one)) {
      if (value === undefined) continue;
      const typed = key as keyof LaserModifiers;
      if (merged[typed] === undefined) {
        (merged as Record<string, unknown>)[typed] = value;
      }
    }
  }
  return merged;
}

export function parseMiningModule(record: GameRecord): MiningModule | undefined {
  const modifiers = mergeModifiers(collectLaserModifiers(record.value));
  if (Object.keys(modifiers).length === 0) return undefined;
  return {
    name: record.shortName,
    recordName: record.name,
    id: record.id,
    modifiers,
  };
}

export const miningDomain: Domain = {
  name: "game-mining",
  description: "Mineable element physics and mining laser/module modifiers.",
  filters: ["**/mining/**"],

  parse({ records }) {
    const elements: MineableElement[] = [];
    const modules: MiningModule[] = [];

    for (const record of records) {
      if (record.type === "MineableElement") {
        elements.push(parseMineableElement(record));
        continue;
      }
      const module = parseMiningModule(record);
      if (module) modules.push(module);
    }

    elements.sort((a, b) => a.name.localeCompare(b.name));
    modules.sort((a, b) => a.name.localeCompare(b.name));

    // Resistance and instability decide whether a laser can break a rock, so
    // surface them as a flat lookup alongside the full records.
    const signatures: Record<string, { resistance?: number; instability?: number }> = {};
    for (const element of elements) {
      signatures[element.name] = {
        resistance: element.resistance,
        instability: element.instability,
      };
    }

    return {
      mineableElements: elements,
      miningModules: modules,
      elementSignatures: signatures,
      summary: { mineableElements: elements.length, miningModules: modules.length },
    };
  },
};
