/**
 * Consumables: medpens, pills, food and drink, and what taking one does.
 *
 * An item's `SCItemConsumableParams` gives its volume (micro-SCU) and its
 * contents as subtype references with a ratio. The effects sit on the subtype,
 * in the ConsumableTypeDatabase, quoted per micro-SCU: a 30 µSCU healing pen of
 * pure Coagulant at 3.75 health per µSCU heals 112.5. Totals are worked out here.
 *
 * Buff effects name a buff type ("Energizing") with optional duration and value
 * overrides. The buff definitions themselves are not in the game data, so a
 * buff without an override has no duration here.
 */

import { component } from "../entity.js";
import type { Localization } from "../localization.js";
import { at, compareNames, num, str, type GameRecord } from "../records.js";
import { itemIdentity } from "./item-common.js";
import type { Domain } from "./types.js";

export interface ConsumableSubtype {
  name: string;
  /** "Drink", "Food", "Drug"... */
  type: string;
  /** Effects per micro-SCU, compacted. */
  effectsPerMicroScu: Record<string, unknown>[];
}

function parseEffect(raw: Record<string, unknown>): Record<string, unknown> {
  const kind = str(raw._Type_)?.replace(/^ConsumableEffect/, "") ?? "Unknown";
  const out: Record<string, unknown> = { kind };
  for (const [key, value] of Object.entries(raw)) {
    if (key === "_Type_" || key === "effectDescription" || value === null) continue;
    out[key] = value;
  }
  return out;
}

/** Subtypes by record name, with their parent type's name. */
export function parseConsumableTypes(database: GameRecord | undefined): Map<string, ConsumableSubtype> {
  const index = new Map<string, ConsumableSubtype>();
  for (const type of (database?.value.types as Record<string, unknown>[] | undefined) ?? []) {
    if (!type) continue;
    for (const sub of (type.subtypes as Record<string, unknown>[] | undefined) ?? []) {
      if (!sub || typeof sub._RecordName_ !== "string") continue;
      index.set(sub._RecordName_, {
        name: str(sub.typeName) ?? sub._RecordName_,
        type: str(type.typeName) ?? "",
        effectsPerMicroScu: ((sub.effectsPerMicroSCU as Record<string, unknown>[] | undefined) ?? [])
          .filter(Boolean)
          .map(parseEffect),
      });
    }
  }
  return index;
}

export interface Consumable {
  name: string;
  recordName: string;
  id: string;
  itemType?: string;
  subType?: string;
  volumeMicroScu?: number;
  contents: { subtype: string; type: string; ratio?: number }[];
  /** Health restored by the whole item. */
  health?: number;
  /** Change to each actor stat (Hunger, Thirst, BloodDrugLevel...) for the whole item. */
  stats?: Record<string, number>;
  /** Duration in seconds and value, only where the subtype overrides the buff's own. */
  buffs?: { buff: string; duration?: number; value?: number }[];
  /** Gas released by the whole item, in kg, by gas (an OxyPen's oxygen). */
  gases?: Record<string, number>;
  /** Resources restored by the whole item, e.g. suit fuel. */
  resources?: Record<string, number>;
  oneShot?: boolean;
  discardWhenConsumed?: boolean;
}

export function parseConsumable(
  record: GameRecord,
  subtypes: Map<string, ConsumableSubtype>,
  localization: Localization = new Map(),
): Consumable | undefined {
  const params = component(record, "SCItemConsumableParams");
  if (!params) return undefined;
  const identity = itemIdentity(record, localization);
  // Templates carry a real item's name, so the name check alone misses them.
  if (identity.isTemplate || /template/i.test(record.name)) return undefined;

  const volume = num(at(params, "consumableVolume.microSCU"));
  const contents: Consumable["contents"] = [];
  let health = 0;
  const stats: Record<string, number> = {};
  const buffs: NonNullable<Consumable["buffs"]> = [];
  const gases: Record<string, number> = {};
  const resources: Record<string, number> = {};

  for (const entry of (params.defaultContents as Record<string, unknown>[] | undefined) ?? []) {
    if (!entry) continue;
    const ref = (entry.consumableSubtype as Record<string, unknown> | undefined)?._RecordName_;
    const sub = typeof ref === "string" ? subtypes.get(ref) : undefined;
    const ratio = num(entry.ratio);
    contents.push({ subtype: sub?.name ?? String(ref), type: sub?.type ?? "", ratio });
    const scale = (volume ?? 0) * (ratio ?? 1);
    for (const effect of sub?.effectsPerMicroScu ?? []) {
      if (effect.kind === "Health") health += (num(effect.healthChange) ?? 0) * scale;
      else if (effect.kind === "ModifyActorStatus" && typeof effect.statType === "string") {
        stats[effect.statType] = (stats[effect.statType] ?? 0) + (num(effect.statPointChange) ?? 0) * scale;
      } else if (effect.kind === "Gas") {
        const gas = String(at(effect, "gasMass.Gas") ?? "gas").split("/").pop()!.replace(/^gasparams\.|\.json$/g, "");
        gases[gas] = (gases[gas] ?? 0) + (num(at(effect, "gasMass.Mass")) ?? 0) * scale;
      } else if (effect.kind === "Resource" && typeof effect.consumableResourceType === "string") {
        const type = effect.consumableResourceType;
        resources[type] = (resources[type] ?? 0) + (num(effect.amount) ?? 0) * scale;
      } else if (effect.kind === "AddBuffEffect" && typeof effect.buffType === "string") {
        buffs.push({
          buff: effect.buffType,
          duration: num(at(effect.buffDurationOverride, "durationOverride")),
          value: num(at(effect.buffValueOverride, "valueOverride")),
        });
      }
    }
  }

  const round = (n: number) => Math.round(n * 1000) / 1000;
  for (const table of [stats, gases, resources]) {
    for (const key of Object.keys(table)) table[key] = round(table[key]);
  }

  return {
    name: identity.name,
    recordName: record.name,
    id: record.id,
    itemType: identity.type,
    subType: identity.subType,
    volumeMicroScu: volume,
    contents,
    health: health ? round(health) : undefined,
    stats: Object.keys(stats).length > 0 ? stats : undefined,
    buffs: buffs.length > 0 ? buffs : undefined,
    gases: Object.keys(gases).length > 0 ? gases : undefined,
    resources: Object.keys(resources).length > 0 ? resources : undefined,
    oneShot: typeof params.oneShotConsume === "boolean" ? params.oneShotConsume : undefined,
    discardWhenConsumed: typeof params.discardWhenConsumed === "boolean" ? params.discardWhenConsumed : undefined,
  };
}

export const consumablesDomain: Domain = {
  name: "game-consumables",
  description: "Medpens, drugs, food and drink with their total health, stat and buff effects.",
  filters: [
    "**/entities/scitem/consumables/**",
    "**/entities/scitem/carryables/1h/**",
    "**/consumabletypesdatabase/**",
  ],
  needsLocalization: true,

  parse({ records, localization }) {
    const subtypes = parseConsumableTypes(records.find((r) => r.type === "ConsumableTypeDatabase"));
    const consumables = records
      .map((r) => parseConsumable(r, subtypes, localization))
      .filter((c): c is Consumable => Boolean(c))
      .sort((a, b) => compareNames(a.name, b.name));

    const byContentType: Record<string, number> = {};
    for (const c of consumables) {
      const type = c.contents[0]?.type || "Unknown";
      byContentType[type] = (byContentType[type] ?? 0) + 1;
    }

    return {
      consumables,
      subtypes: [...subtypes.values()].sort((a, b) => compareNames(a.type, b.type) || compareNames(a.name, b.name)),
      byContentType,
      summary: { consumables: consumables.length, subtypes: subtypes.size },
    };
  },
};
