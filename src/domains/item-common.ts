/**
 * Facts every ship item shares, read the same way regardless of category.
 *
 * `SAttachableComponentParams.AttachDef` carries identity (type, size, grade,
 * manufacturer, localized name). `ItemResourceComponentParams` carries the
 * resource-network profile: what the item draws and generates while online and
 * the EM/IR signature it emits doing so. Health, mass and distortion ceiling
 * each live on their own struct.
 *
 * Rates come in two spellings. Weapons and most generators quote
 * `standardResourceUnits`; power plants, coolers and shields quote `units`.
 * Both are the same per-second figure.
 */

import { component, referencedName } from "../entity.js";
import { isPlaceholderName, localize, type Localization } from "../localization.js";
import { at, num, str, type GameRecord } from "../records.js";

export interface ItemIdentity {
  name: string;
  /** True when the record has no real display name, i.e. a template or stub. */
  isTemplate?: true;
  type?: string;
  subType?: string;
  size?: number;
  grade?: number;
  manufacturer?: string;
}

export function attachDef(record: GameRecord): Record<string, unknown> {
  const attach = component(record, "SAttachableComponentParams");
  const def = attach?.AttachDef;
  return def && typeof def === "object" ? (def as Record<string, unknown>) : {};
}

export function manufacturerOf(def: Record<string, unknown>): string | undefined {
  return referencedName(def.Manufacturer)?.toUpperCase();
}

export function itemIdentity(record: GameRecord, localization: Localization): ItemIdentity {
  const def = attachDef(record);
  const localized = localize(localization, at(def, "Localization.Name"));
  const placeholder = isPlaceholderName(localized);
  return {
    name: placeholder ? record.shortName : (localized as string),
    isTemplate: placeholder || undefined,
    type: str(def.Type),
    subType: str(def.SubType) === "UNDEFINED" ? undefined : str(def.SubType),
    size: num(def.Size),
    grade: num(def.Grade),
    manufacturer: manufacturerOf(def),
  };
}

export interface ResourceProfile {
  /** Power drawn per second while online. */
  power?: number;
  em?: number;
  ir?: number;
  /** Everything the item generates while online, keyed by resource. */
  generation?: Record<string, number>;
  /** Everything the item consumes while online, keyed by resource. */
  consumption?: Record<string, number>;
}

function rateOf(value: unknown): number | undefined {
  return (
    num(at(value, "resourceAmountPerSecond.standardResourceUnits")) ??
    num(at(value, "resourceAmountPerSecond.units"))
  );
}

/** Draw, output and signatures from the resource-network "Online" state. */
export function resourceProfile(record: GameRecord): ResourceProfile {
  const resources = component(record, "ItemResourceComponentParams");
  const states = Array.isArray(resources?.states) ? resources.states : [];
  const online =
    states.find((s) => s && typeof s === "object" && (s as Record<string, unknown>).name === "Online") ??
    states[0];
  if (!online || typeof online !== "object") return {};
  const state = online as Record<string, unknown>;

  const generation: Record<string, number> = {};
  const consumption: Record<string, number> = {};
  for (const delta of Array.isArray(state.deltas) ? state.deltas : []) {
    if (!delta || typeof delta !== "object") continue;
    const entry = delta as Record<string, unknown>;
    for (const [side, bucket] of [
      ["generation", generation],
      ["consumption", consumption],
    ] as const) {
      const block = entry[side] as Record<string, unknown> | undefined;
      const resource = str(block?.resource);
      const rate = rateOf(block);
      if (resource && rate !== undefined) bucket[resource] = (bucket[resource] ?? 0) + rate;
    }
  }

  return {
    power: consumption.Power,
    em: num(at(state, "signatureParams.EMSignature.nominalSignature")),
    ir: num(at(state, "signatureParams.IRSignature.nominalSignature")),
    generation: Object.keys(generation).length > 0 ? generation : undefined,
    consumption: Object.keys(consumption).length > 0 ? consumption : undefined,
  };
}

export function healthOf(record: GameRecord): number | undefined {
  return num(at(component(record, "SHealthComponentParams"), "Health"));
}

export function massOf(record: GameRecord): number | undefined {
  return num(at(component(record, "SEntityPhysicsControllerParams"), "PhysType.Mass"));
}

/** Distortion the item absorbs before it shuts down. */
export function distortionMaxOf(record: GameRecord): number | undefined {
  return num(at(component(record, "SDistortionParams"), "Maximum"));
}

/** The game's damage types, in the order its resistance arrays use. */
export const DAMAGE_TYPES = ["physical", "energy", "distortion", "thermal", "biochemical", "stun"] as const;

/** A `{ DamagePhysical: 0.75, DamageEnergy: 0.6, ... }` struct as a lower-case map. */
export function damageTypeMap(value: unknown): Record<string, number> | undefined {
  if (!value || typeof value !== "object") return undefined;
  const out: Record<string, number> = {};
  for (const [key, entry] of Object.entries(value as Record<string, unknown>)) {
    const match = /^Damage(\w+)$/.exec(key);
    if (match && typeof entry === "number") out[match[1].toLowerCase()] = entry;
  }
  return Object.keys(out).length > 0 ? out : undefined;
}

/** A positional `[{Min, Max}, ...]` array keyed by damage type. */
export function damageTypeRanges(
  value: unknown,
): Record<string, { min: number; max: number }> | undefined {
  if (!Array.isArray(value)) return undefined;
  const out: Record<string, { min: number; max: number }> = {};
  value.forEach((entry, index) => {
    const type = DAMAGE_TYPES[index];
    const min = num(at(entry, "Min"));
    const max = num(at(entry, "Max"));
    if (type && min !== undefined && max !== undefined) out[type] = { min, max };
  });
  return Object.keys(out).length > 0 ? out : undefined;
}

/** How an item takes damage and wears: the stats a "durability" column rolls up. */
export interface Durability {
  health?: number;
  /** Damage multipliers that differ from 1, by damage type. */
  resistances?: Record<string, number>;
  /** Automatic repair after a knockout: how often, how long, and to what fraction. */
  selfRepair?: { count?: number; seconds?: number; healthRatio?: number };
  /** Fraction of health a field repair restores. */
  repairRestoreRatio?: number;
  /** Wear: hours of life, how much damage converts to wear, and the overheat penalty. */
  wear?: { maxLifetimeHours?: number; damageConversionRate?: number; overheatMultiplier?: number };
  /** Seconds between misfire checks once degraded, min and max. */
  misfireWindow?: { min?: number; max?: number };
}

/** Multipliers from a `DamageResistances` table that differ from 1, by damage type. */
export function healthResistances(table: unknown): Record<string, number> | undefined {
  if (!table || typeof table !== "object") return undefined;
  const out: Record<string, number> = {};
  for (const [key, entry] of Object.entries(table as Record<string, unknown>)) {
    const match = /^(\w+)Resistance$/.exec(key);
    const multiplier = num(at(entry, "Multiplier"));
    if (match && multiplier !== undefined && multiplier !== 1) out[match[1].toLowerCase()] = multiplier;
  }
  return Object.keys(out).length > 0 ? out : undefined;
}

function compact<T extends Record<string, unknown>>(value: T): T | undefined {
  const entries = Object.entries(value).filter(([, v]) => v !== undefined);
  return entries.length > 0 ? (Object.fromEntries(entries) as T) : undefined;
}

export function durabilityOf(record: GameRecord): Durability | undefined {
  const health = component(record, "SHealthComponentParams");
  const resources = component(record, "ItemResourceComponentParams");
  const wear = at(component(record, "SDegradationParams"), "accumulators[0]") ??
    (Array.isArray(component(record, "SDegradationParams")?.accumulators)
      ? (component(record, "SDegradationParams")?.accumulators as unknown[])[0]
      : undefined);
  const misfire = component(record, "EntityComponentMisfireParams");

  return compact({
    health: num(health?.Health),
    resistances: healthResistances(health?.DamageResistances),
    selfRepair: compact({
      count: num(at(resources, "selfRepair.maxRepairCount")),
      seconds: num(at(resources, "selfRepair.timeToRepair")),
      healthRatio: num(at(resources, "selfRepair.healthRatio")),
    }),
    repairRestoreRatio: num(resources?.repairRestoreRatio),
    wear: compact({
      maxLifetimeHours: num(at(wear, "MaxLifetimeHours")),
      damageConversionRate: num(at(wear, "DamageConversionRate")),
      overheatMultiplier: num(at(wear, "HeatMultipliers.OverheatTemperatureMultiplier")),
    }),
    misfireWindow: compact({ min: num(misfire?.minWindowLength), max: num(misfire?.maxWindowLength) }),
  });
}

/** Inventory volume the item occupies, in micro-SCU. */
export function volumeOf(record: GameRecord): number | undefined {
  return num(at(attachDef(record), "inventoryOccupancyVolume.microSCU"));
}

/** How distortion builds and drains on an item: the pool behind a distortion shutdown. */
export interface Distortion {
  /** Distortion absorbed before the item shuts down. */
  max?: number;
  /** Seconds after the last hit before distortion starts draining. */
  decayDelay?: number;
  /** Distortion drained per second once decay starts. */
  decayRate?: number;
  /** Fraction of the pool at which the warning shows. */
  warningRatio?: number;
  /** Fraction the pool must drain to before the item restarts. */
  recoveryRatio?: number;
  /** Seconds from a full pool to the restart point, with no further hits. */
  secondsToRecover?: number;
}

export function distortionOf(record: GameRecord): Distortion | undefined {
  const params = component(record, "SDistortionParams");
  if (!params) return undefined;
  const max = num(params.Maximum);
  const decayDelay = num(params.DecayDelay);
  const decayRate = num(params.DecayRate);
  const recoveryRatio = num(params.RecoveryRatio);
  const drain = max !== undefined ? max * (1 - (recoveryRatio ?? 0)) : undefined;
  const secondsToRecover =
    drain !== undefined && decayRate ? Math.round(((decayDelay ?? 0) + drain / decayRate) * 100) / 100 : undefined;
  return compact({
    max,
    decayDelay,
    decayRate,
    warningRatio: num(params.WarningRatio),
    recoveryRatio,
    secondsToRecover,
  });
}
