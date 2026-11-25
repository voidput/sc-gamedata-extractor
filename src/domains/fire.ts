/**
 * Fire: the global fire-hazard rules and what makes each ship item start one.
 *
 * FireHazardGlobalParams holds the rules every fire obeys: ignition chance
 * multipliers per damage type for explosions, projectiles and collisions (only
 * thermal explosion damage ignites in current builds), the oxygen a fire needs,
 * how far it spreads, how hot a burning room gets, and the damage it does to
 * players. Its per-damage-type arrays follow DamageInfo order.
 *
 * Per item, four paths can light a fire:
 * - `ignite`: entering a health state (Destroyed, below 1 % health on most
 *   components) dispatches FireHazardIgniteEvent, one roll at `chance`.
 * - `deathExplosion`: the explosion on death; its thermal part can ignite
 *   through the explosion multiplier (power plants have this and no roll).
 * - `misfire`: radar, shield and quantum drive misfires can pick
 *   SFireMisfireEffect; `fireByLevel` is the share of each level's weighted
 *   picks that are fires.
 * - `temperature`: overheat and misfire temperatures, and the equalization
 *   law (dT/dt = rate * gap / difference) plus own heat (`heatGen`, K/s while
 *   powered, highest per-state value) that decide how fast a part in a burning
 *   room reaches them.
 *
 * Only items with an ignite roll, a thermal death explosion or a fire misfire
 * are listed; their temperatures come along for the heat estimate.
 */

import { component, components } from "../entity.js";
import type { NameStatus } from "../localization.js";
import { at, num, str, type GameRecord } from "../records.js";
import { DAMAGE_TYPES, damageTypeMap, itemIdentity } from "./item-common.js";
import type { Domain } from "./types.js";

export interface FireRules {
  fixedFPS?: number;
  globalIgnitionChanceMultiplier?: number;
  globalFlashIgnitionThresholdMultiplier?: number;
  /** Ignition chance multiplier per damage type, by source. */
  explosionChanceMultiplier?: Record<string, number>;
  projectileChanceMultiplier?: Record<string, number>;
  collisionChanceMultiplier?: Record<string, number>;
  fallbackChanceMultiplier?: Record<string, number>;
  damageTypeIgnitionModifiers?: Record<string, number>;
  /** Oxygen pressure a fire needs, min and max. */
  oxygenPressureRange?: { min?: number; max?: number };
  fireMassThreshold?: number;
  /** Metres. */
  maximumRadiationRadius?: number;
  maximumConvectionDistance?: number;
  /** Kelvin. */
  standardConvectionTemperature?: number;
  /** Fire strength at which a door (room connector) lets it through. */
  roomConnectorIgniteThreshold?: number;
  damageToPlayers?: { minimumDamageTemperature?: number; baseDamage?: number; referenceHealth?: number };
  extinguishing?: { oxygenReduction?: number; heatTransferMultiplier?: number };
}

export interface IgniteEvent {
  state: string;
  /** Health ratio below which the state is entered. */
  belowHealthRatio?: number;
  /** 0..1 */
  chance: number;
  strength?: number;
  /** Metres. */
  radius?: number;
}

export interface Explosion {
  damage: Record<string, number>;
  minRadius?: number;
  maxRadius?: number;
}

export interface FireItem {
  name: string;
  /** True when the record has no real display name (template or stub). */
  isTemplate?: true;
  nameKey?: string;
  descriptionKey?: string;
  nameStatus?: NameStatus;
  recordName: string;
  id: string;
  type?: string;
  subType?: string;
  size?: number;
  grade?: number;
  manufacturer?: string;
  health?: number;
  /** Damage multipliers by type, 1 included. */
  damageMultipliers?: Record<string, number>;
  ignite?: IgniteEvent[];
  deathExplosion?: Explosion;
  misfire?: {
    fireByLevel: number[];
    trigger?: Record<string, number>;
    window?: { min?: number; max?: number };
  };
  overheat?: { warning?: number; at?: number; recovery?: number };
  misfireTemperature?: { min?: number; max?: number };
  temperature?: {
    enabled: boolean;
    coolingEqualizationRate: number;
    temperatureDifference: number;
    heatGen: number;
    minCoolingTemperature?: number;
  };
}

const perType = (value: unknown): Record<string, number> | undefined => {
  if (!Array.isArray(value)) return undefined;
  const out: Record<string, number> = {};
  value.forEach((x, i) => {
    const n = num(x);
    if (n !== undefined && DAMAGE_TYPES[i]) out[DAMAGE_TYPES[i]] = n;
  });
  return Object.keys(out).length > 0 ? out : undefined;
};

const ptr = (value: unknown) => String(value ?? "").replace("_PointsTo_:", "");
const asList = (value: unknown): Record<string, unknown>[] =>
  Array.isArray(value) ? value.filter((x): x is Record<string, unknown> => !!x && typeof x === "object") : [];

export function parseFireRules(record: GameRecord): FireRules {
  const v = record.value;
  return {
    fixedFPS: num(at(v, "update.fixedFPS")),
    globalIgnitionChanceMultiplier: num(at(v, "ignition.globalIgnitionChanceMultiplier")),
    globalFlashIgnitionThresholdMultiplier: num(at(v, "ignition.globalFlashIgnitionThresholdMultiplier")),
    explosionChanceMultiplier: perType(at(v, "ignition.explosionChanceMultiplier")),
    projectileChanceMultiplier: perType(at(v, "ignition.projectileChanceMultiplier")),
    collisionChanceMultiplier: perType(at(v, "ignition.collisionChanceMultiplier")),
    fallbackChanceMultiplier: perType(at(v, "ignition.fallbackChanceMultiplier")),
    damageTypeIgnitionModifiers: perType(at(v, "ignition.damageTypeIgnitionModifiers")),
    oxygenPressureRange: {
      min: num(at(v, "propagation.oxygenPressureRange.minimum")),
      max: num(at(v, "propagation.oxygenPressureRange.maximum")),
    },
    fireMassThreshold: num(at(v, "propagation.fireMassThreshold")),
    maximumRadiationRadius: num(at(v, "propagation.maximumRadiationRadius")),
    maximumConvectionDistance: num(at(v, "propagation.maximumConvectionDistance")),
    standardConvectionTemperature: num(at(v, "propagation.standardConvectionTemperature")),
    roomConnectorIgniteThreshold: num(at(v, "roomConnector.igniteThreshold")),
    damageToPlayers: {
      minimumDamageTemperature: num(at(v, "damageToHealth.minimumDamageTemperature")),
      baseDamage: num(at(v, "damageToHealth.baseDamage")),
      referenceHealth: num(at(v, "damageToHealth.referenceHealth")),
    },
    extinguishing: {
      oxygenReduction: num(at(v, "extinguishing.oxygenReduction")),
      heatTransferMultiplier: num(at(v, "extinguishing.heatTransferMultiplier")),
    },
  };
}

/** An ExplosionParams struct, or undefined when it deals no damage. */
export function parseExplosion(value: unknown): Explosion | undefined {
  const damage = damageTypeMap(at(value, "damage"));
  const dealt = damage && Object.fromEntries(Object.entries(damage).filter(([, x]) => x !== 0));
  if (!dealt || Object.keys(dealt).length === 0) return undefined;
  return { damage: dealt, minRadius: num(at(value, "minRadius")), maxRadius: num(at(value, "maxRadius")) };
}

function igniteEvents(record: GameRecord): IgniteEvent[] {
  const out: IgniteEvent[] = [];
  for (const machine of components(record).filter((c) => c._Type_ === "SInteractionStateMachineParams")) {
    for (const stateType of asList(machine.StateTypes)) {
      const below = new Map<string, number | undefined>();
      for (const mod of asList(stateType.StateModifiers))
        for (const range of asList(mod.stateRanges)) below.set(ptr(range.state), num(range.max));
      for (const state of asList(stateType.States)) {
        for (const ev of asList(state.EnterEventDispatchers)) {
          if (ev._Type_ !== "FireHazardIgniteEvent") continue;
          out.push({
            state: str(state.StateName) ?? "",
            belowHealthRatio: below.get(ptr(state._Pointer_)),
            chance: (num(ev.ignitionChance) ?? 0) / 100,
            strength: num(ev.ignitionStrength),
            radius: num(ev.ignitionRadius),
          });
        }
      }
    }
  }
  return out;
}

function misfireOf(record: GameRecord): FireItem["misfire"] {
  const params = component(record, "EntityComponentMisfireParams");
  if (!params) return undefined;
  const fire = new Set(asList(params.misfires).filter((m) => m._Type_ === "SFireMisfireEffect").map((m) => ptr(m._Pointer_)));
  if (fire.size === 0) return undefined;
  const fireByLevel = asList(params.misfireLevels).map((level) => {
    const picks = asList(level.misfireProbabilityArray);
    const total = picks.reduce((a, p) => a + (num(p.weight) ?? 0), 0);
    const fires = picks
      .filter((p) => Array.isArray(p.misfireArray) && p.misfireArray.some((x) => fire.has(ptr(x))))
      .reduce((a, p) => a + (num(p.weight) ?? 0), 0);
    return total > 0 ? Math.round((fires / total) * 1e4) / 1e4 : 0;
  });
  const first = asList(params.triggerConditions)[0] ?? {};
  const trigger: Record<string, number> = {};
  for (const key of ["degradation", "damage", "heat", "distortion"]) {
    const n = num(first[key]);
    if (n !== undefined) trigger[key] = n;
  }
  return {
    fireByLevel,
    trigger: Object.keys(trigger).length > 0 ? trigger : undefined,
    window: { min: num(params.minWindowLength), max: num(params.maxWindowLength) },
  };
}

function temperatureOf(record: GameRecord): Pick<FireItem, "overheat" | "misfireTemperature" | "temperature"> {
  const t = at(component(record, "SEntityPhysicsControllerParams"), "PhysType.temperature");
  if (!t || typeof t !== "object") return {};
  const out: Pick<FireItem, "overheat" | "misfireTemperature" | "temperature"> = {};
  const res = at(t, "itemResourceParams");
  if (at(res, "enableOverheat"))
    out.overheat = {
      warning: num(at(res, "overheatWarningTemperature")),
      at: num(at(res, "overheatTemperature")),
      recovery: num(at(res, "overheatRecoveryTemperature")),
    };
  const misMax = num(at(t, "misfireTemperatureRange.maximum"));
  if (misMax) out.misfireTemperature = { min: num(at(t, "misfireTemperatureRange.minimum")), max: misMax };
  const rate = num(at(t, "coolingEqualizationParams.coolingEqualizationRate"));
  const diff = num(at(t, "coolingEqualizationParams.temperatureDifference"));
  if (rate && diff) {
    // One entry per state (a quantum drive's Online and Travelling); only one is active, so the max.
    const heat = asList(at(res, "stateHeatGenerationValues")).map((h) => num(h.baselineTemperatureChange) ?? 0);
    out.temperature = {
      enabled: !!at(t, "enable"),
      coolingEqualizationRate: rate,
      temperatureDifference: diff,
      heatGen: Math.round(Math.max(0, ...heat) * 1e4) / 1e4,
      minCoolingTemperature: num(at(res, "minCoolingTemperature")),
    };
  }
  return out;
}

function damageMultipliers(record: GameRecord): Record<string, number> | undefined {
  const table = at(component(record, "SHealthComponentParams"), "DamageResistances");
  if (!table || typeof table !== "object") return undefined;
  const out: Record<string, number> = {};
  for (const [key, entry] of Object.entries(table as Record<string, unknown>)) {
    const m = /^(\w+)Resistance$/.exec(key);
    const x = num(at(entry, "Multiplier"));
    if (m && x !== undefined) out[m[1].toLowerCase()] = x;
  }
  return Object.keys(out).length > 0 ? out : undefined;
}

export function parseFireItem(record: GameRecord, localization: Parameters<typeof itemIdentity>[1]): FireItem | undefined {
  const ignite = igniteEvents(record);
  const deathExplosion = parseExplosion(at(component(record, "SHealthComponentParams"), "DeathExplosionParams"));
  const misfire = misfireOf(record);
  const temp = temperatureOf(record);
  if (!ignite.length && !deathExplosion?.damage.thermal && !misfire) return undefined;
  const identity = itemIdentity(record, localization);
  return {
    name: identity.name,
    isTemplate: identity.isTemplate,
    nameKey: identity.nameKey,
    descriptionKey: identity.descriptionKey,
    nameStatus: identity.nameStatus,
    recordName: record.name,
    id: record.id,
    type: identity.type,
    subType: identity.subType,
    size: identity.size,
    grade: identity.grade,
    manufacturer: identity.manufacturer,
    health: num(at(component(record, "SHealthComponentParams"), "Health")),
    damageMultipliers: damageMultipliers(record),
    ignite: ignite.length ? ignite : undefined,
    deathExplosion,
    misfire,
    ...temp,
  };
}

export const fireDomain: Domain = {
  name: "game-fire",
  description:
    "Fire-hazard rules (ignition by damage type, oxygen, spread, room heat) and what makes each ship item start a fire: ignite rolls on death, thermal death explosions, fire misfires, overheat and misfire temperatures.",
  filters: ["**/roomsystem/firehazard/**", "**/entities/scitem/ships/**"],
  needsLocalization: true,

  parse({ records, localization }) {
    const rulesRecord = records.find((r) => r.type === "FireHazardGlobalParams");
    const items: FireItem[] = [];
    for (const record of records) {
      if (record.type !== "EntityClassDefinition") continue;
      const item = parseFireItem(record, localization);
      if (item) items.push(item);
    }
    items.sort((a, b) => (a.type ?? "").localeCompare(b.type ?? "") || (a.size ?? 0) - (b.size ?? 0) || a.recordName.localeCompare(b.recordName));
    const byType: Record<string, number> = {};
    for (const i of items) byType[i.type ?? "unknown"] = (byType[i.type ?? "unknown"] ?? 0) + 1;
    return {
      rules: rulesRecord ? parseFireRules(rulesRecord) : undefined,
      items,
      summary: {
        rules: rulesRecord ? 1 : 0,
        items: items.length,
        withIgniteRoll: items.filter((i) => i.ignite).length,
        withThermalDeathExplosion: items.filter((i) => i.deathExplosion?.damage.thermal).length,
        withFireMisfire: items.filter((i) => i.misfire).length,
        byType,
      },
    };
  },
};
