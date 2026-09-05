/**
 * Ship components: shields, quantum drives, radars, ship weapons and missiles.
 *
 * Every ship item is an EntityClassDefinition whose `Components` array holds one
 * typed params struct per aspect. Two of those matter here:
 *
 * - `SAttachableComponentParams.AttachDef` carries the common item facts: Type,
 *   Size, Grade, Manufacturer and the localized name.
 * - A category-specific struct carries the stats, e.g.
 *   `SCItemShieldGeneratorParams` for shields.
 *
 * Category is decided by which params struct is present, not by the record's
 * directory. Paths mislead: a glob for "shield" under ships also matches a seat
 * blast shield, which has no shield generator at all.
 */

import { isPlaceholderName, localize, type Localization } from "../localization.js";
import { at, num, str, type GameRecord } from "../records.js";
import type { Domain } from "./types.js";

/** Stat struct -> output category, and the fields worth surfacing from each. */
const CATEGORIES: { params: string; category: string; stats: string[] }[] = [
  {
    params: "SCItemShieldGeneratorParams",
    category: "shields",
    stats: [
      "MaxShieldHealth",
      "MaxShieldRegen",
      "DecayRatio",
      "DownedRegenDelay",
      "DamagedRegenDelay",
      "ReservePoolDrainRateRatio",
    ],
  },
  {
    params: "SCItemQuantumDriveParams",
    category: "quantumDrives",
    stats: ["quantumFuelRequirement", "jumpRange", "disconnectRange"],
  },
  {
    params: "SCItemRadarComponentParams",
    category: "radars",
    stats: ["detectionLifetime", "physicalRadarRange", "signatureDetection"],
  },
  {
    params: "SCItemWeaponComponentParams",
    category: "weapons",
    stats: ["heatPerShot", "ammoContainerCapacity"],
  },
  { params: "SCItemMissileParams", category: "missiles", stats: ["lockTime", "lockRange"] },
];

export interface Component {
  name: string;
  recordName: string;
  id: string;
  category: string;
  /** True when the record has no real display name, i.e. a template or stub. */
  isTemplate?: true;
  type?: string;
  subType?: string;
  size?: number;
  grade?: number;
  stats: Record<string, number>;
}

function componentsOf(record: GameRecord): Record<string, unknown>[] {
  const list = record.value.Components;
  return Array.isArray(list) ? (list.filter((c) => c && typeof c === "object") as Record<string, unknown>[]) : [];
}

function findParams(record: GameRecord, type: string): Record<string, unknown> | undefined {
  return componentsOf(record).find((c) => c._Type_ === type);
}

/** Only numeric stats are surfaced; the structs are full of unrelated scaffolding. */
function pickStats(
  params: Record<string, unknown>,
  fields: string[],
): Record<string, number> {
  const stats: Record<string, number> = {};
  for (const field of fields) {
    const value = num(params[field]);
    if (value !== undefined) stats[field] = value;
  }
  return stats;
}

export { isPlaceholderName };

export function parseComponent(
  record: GameRecord,
  localization: Localization,
): Component | undefined {
  for (const { params, category, stats } of CATEGORIES) {
    const struct = findParams(record, params);
    if (!struct) continue;

    const attach = findParams(record, "SAttachableComponentParams");
    const def = (attach?.AttachDef ?? {}) as Record<string, unknown>;
    const localized = localize(localization, at(def, "Localization.Name"));

    return {
      name: isPlaceholderName(localized) ? record.shortName : (localized as string),
      isTemplate: isPlaceholderName(localized) || undefined,
      recordName: record.name,
      id: record.id,
      category,
      type: str(def.Type),
      subType: str(def.SubType) === "UNDEFINED" ? undefined : str(def.SubType),
      size: num(def.Size),
      grade: num(def.Grade),
      stats: pickStats(struct, stats),
    };
  }

  return undefined;
}

export const componentsDomain: Domain = {
  name: "game-ship-components",
  description: "Ship shields, quantum drives, radars, weapons and missiles with their stats.",
  filters: ["**/entities/scitem/ships/**"],
  needsLocalization: true,

  parse({ records, localization }) {
    const byCategory: Record<string, Component[]> = {};

    for (const record of records) {
      const component = parseComponent(record, localization);
      if (!component) continue;
      (byCategory[component.category] ??= []).push(component);
    }

    const summary: Record<string, number> = {};
    for (const [category, list] of Object.entries(byCategory)) {
      list.sort((a, b) => (a.size ?? 0) - (b.size ?? 0) || a.name.localeCompare(b.name));
      summary[category] = list.length;
    }

    return { ...byCategory, summary };
  },
};
