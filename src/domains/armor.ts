/**
 * Personal armour and clothing: what a character wears, and what it protects
 * against.
 *
 * Armour pieces (core, arms, legs, helmet, undersuit) carry
 * `SCItemSuitArmorParams`, which points at a DamageResistanceMacro record by
 * path rather than holding resistances itself: every light core shares
 * `damage/lightarmor.json`. A multiplier is the share of damage that gets
 * through, so 0.8 blocks 20%.
 *
 * Armour and clothing alike carry `SCItemClothingParams`: the temperature range
 * the piece is rated for, radiation capacity and how fast it sheds radiation,
 * and a g-force resistance. Storage is a reference to an inventory container,
 * resolved here into a capacity.
 */

import { component, referencedName } from "../entity.js";
import type { Localization } from "../localization.js";
import { at, compareNames, indexByRecordPath, num, refPath, str, type GameRecord } from "../records.js";
import { itemIdentity, massOf } from "./item-common.js";
import type { Domain } from "./types.js";

export interface Resistance {
  /** Share of damage that gets through: 0.8 blocks 20%. */
  multiplier?: number;
  threshold?: number;
  damageCap?: number;
}

export interface WornItem {
  name: string;
  recordName: string;
  id: string;
  /** "Core", "Arms", "Legs", "Helmet", "Undersuit", "Backpack", or a clothing slot. */
  slot: string;
  /** Light, Medium, Heavy for armour; the game's subtype otherwise. */
  weight?: string;
  manufacturer?: string;
  grade?: number;
  mass?: number;
  temperatureMin?: number;
  temperatureMax?: number;
  radiationCapacity?: number;
  radiationDissipationRate?: number;
  gForceResistance?: number;
  /** Inventory capacity in micro-SCU. */
  storageMicroScu?: number;
  /** Name of the DamageResistanceMacro the piece uses. */
  resistanceProfile?: string;
  resistances?: Record<string, Resistance>;
  impactForceResistance?: number;
  emSignature?: number;
}

const RESISTANCE_FIELDS: Record<string, string> = {
  physical: "PhysicalResistance",
  energy: "EnergyResistance",
  distortion: "DistortionResistance",
  thermal: "ThermalResistance",
  biochemical: "BiochemicalResistance",
  stun: "StunResistance",
};

export function parseResistances(macro: GameRecord | undefined): {
  resistances?: Record<string, Resistance>;
  impactForceResistance?: number;
} {
  if (!macro) return {};
  const table = (macro.value.damageResistance ?? {}) as Record<string, unknown>;
  const resistances: Record<string, Resistance> = {};
  for (const [key, field] of Object.entries(RESISTANCE_FIELDS)) {
    const entry = table[field];
    const multiplier = num(at(entry, "Multiplier"));
    if (multiplier === undefined) continue;
    // Threshold and cap are zero on every profile seen so far; keep them only when set.
    const threshold = num(at(entry, "Threshold")) || undefined;
    const damageCap = num(at(entry, "DamageCap")) || undefined;
    resistances[key] = { multiplier, threshold, damageCap };
  }
  return {
    resistances: Object.keys(resistances).length > 0 ? resistances : undefined,
    impactForceResistance: num(at(macro.value, "impactForceResistance.impactForceResistance")),
  };
}

/** Capacity of an inventory container record, in micro-SCU. */
export function containerMicroScu(container: GameRecord | undefined): number | undefined {
  const capacity = at(container?.value, "inventoryType.capacity");
  const micro = num(at(capacity, "microSCU"));
  if (micro !== undefined) return micro;
  const centi = num(at(capacity, "centiSCU"));
  if (centi !== undefined) return centi * 10_000;
  const scu = num(at(capacity, "standardCargoUnits"));
  return scu !== undefined ? scu * 1_000_000 : undefined;
}

/** "Char_Armor_Torso" -> "Core", "Char_Clothing_Torso_1" -> "Torso_1". */
export function slotOf(type: string | undefined): string {
  if (!type) return "Unknown";
  if (type === "Char_Armor_Torso") return "Core";
  return type.replace(/^Char_(Armor|Clothing)_/, "");
}

export function parseWornItem(
  record: GameRecord,
  byPath: Map<string, GameRecord>,
  localization: Localization = new Map(),
): WornItem | undefined {
  const identity = itemIdentity(record, localization);
  if (identity.isTemplate) return undefined;
  const clothing = component(record, "SCItemClothingParams");
  const armor = component(record, "SCItemSuitArmorParams");
  const inventory = component(record, "SCItemInventoryContainerComponentParams");
  const lookup = (ref: unknown) => {
    const path = refPath(ref);
    return path ? byPath.get(path) : undefined;
  };

  const signatures = Array.isArray(armor?.signatureParams) ? (armor.signatureParams as Record<string, unknown>[]) : [];
  const em = signatures.find((s) => s?.signatureType === "Electromagnetic");
  const macro = lookup(armor?.damageResistance);

  return {
    name: identity.name,
    recordName: record.name,
    id: record.id,
    slot: slotOf(identity.type),
    weight: identity.subType,
    manufacturer: identity.manufacturer,
    grade: identity.grade,
    mass: massOf(record),
    temperatureMin: num(at(clothing, "TemperatureResistance.MinResistance")),
    temperatureMax: num(at(clothing, "TemperatureResistance.MaxResistance")),
    radiationCapacity: num(at(clothing, "RadiationResistance.MaximumRadiationCapacity")),
    radiationDissipationRate: num(at(clothing, "RadiationResistance.RadiationDissipationRate")),
    gForceResistance: num(at(clothing, "Flight.gForceResistance")),
    storageMicroScu: containerMicroScu(lookup(inventory?.containerParams)),
    resistanceProfile: macro ? macro.shortName : referencedName(armor?.damageResistance),
    ...parseResistances(macro),
    emSignature: num(em?.signatureEmission),
  };
}

export const armorDomain: Domain = {
  name: "game-armor",
  description: "Personal armour, undersuits, helmets, backpacks and clothing with resistances, temperature range and storage.",
  filters: [
    "**/entities/scitem/characters/human/armor/**",
    "**/entities/scitem/characters/human/starwear/**",
    "**/entities/scitem/characters/human/clothing/**",
    "**/damage/**",
    "**/inventorycontainers/**",
  ],
  needsLocalization: true,

  parse({ records, localization }) {
    const byPath = indexByRecordPath(records);
    // Armour and clothing share slot names (both have Legs), so the type
    // prefix decides which list an item belongs to, not the slot.
    const armor: WornItem[] = [];
    const clothing: WornItem[] = [];
    for (const record of records) {
      if (!record.recordPath.includes("/entities/scitem/characters/")) continue;
      const type = str(at(component(record, "SAttachableComponentParams"), "AttachDef.Type"));
      const target = type?.startsWith("Char_Armor_") ? armor : type?.startsWith("Char_Clothing_") ? clothing : undefined;
      if (!target) continue;
      const item = parseWornItem(record, byPath, localization);
      if (item) target.push(item);
    }
    const order = (a: WornItem, b: WornItem) => compareNames(a.slot, b.slot) || compareNames(a.name, b.name);
    armor.sort(order);
    clothing.sort(order);

    const count = (items: WornItem[]) => {
      const out: Record<string, number> = {};
      for (const w of items) out[w.slot] = (out[w.slot] ?? 0) + 1;
      return out;
    };
    const bySlot = { armor: count(armor), clothing: count(clothing) };

    return {
      armor,
      clothing,
      bySlot,
      summary: { armor: armor.length, clothing: clothing.length },
    };
  },
};
