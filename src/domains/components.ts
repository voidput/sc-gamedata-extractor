/**
 * Ship components: everything that slots into a ship and carries a stat.
 *
 * Every ship item is an EntityClassDefinition whose `Components` array holds one
 * typed params struct per aspect. Two of those matter here:
 *
 * - `SAttachableComponentParams.AttachDef` carries the common item facts: Type,
 *   Size, Grade, Manufacturer and the localized name.
 * - A category-specific struct carries the stats, e.g.
 *   `SCItemShieldGeneratorParams` for shields.
 *
 * Category is decided by which params struct is present, falling back to
 * `AttachDef.Type` for items whose only stats live on the shared resource
 * struct (power plants, coolers, life support). Paths mislead: a glob for
 * "shield" under ships also matches a seat blast shield, which has no shield
 * generator at all.
 *
 * Every component carries the same headline columns a loadout site shows:
 * `power` drawn, `em`/`ir` emitted, `health`, `mass` and `distortionMax`, so a
 * patch to any of them reads as one line in a diff. Category stats sit in
 * `stats` as a flat numeric map; structured extras (resistance tables, armour
 * multipliers) in `detail`.
 */

import { component, components, referencedName } from "../entity.js";
import { isPlaceholderName, type Localization } from "../localization.js";
import { at, num, str, type GameRecord } from "../records.js";
import { parseContainer } from "./containers.js";
import {
  damageTypeMap,
  damageTypeRanges,
  distortionMaxOf,
  distortionOf,
  durabilityOf,
  healthOf,
  itemIdentity,
  massOf,
  resourceProfile,
  volumeOf,
  type Distortion,
  type Durability,
} from "./item-common.js";
import type { Domain } from "./types.js";

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
  manufacturer?: string;
  /** Power drawn per second while online. */
  power?: number;
  em?: number;
  ir?: number;
  health?: number;
  mass?: number;
  /** Distortion absorbed before the item shuts down. */
  distortionMax?: number;
  /** Resistances, self-repair, wear and misfire behaviour behind `health`. */
  durability?: Durability;
  /** How the distortion pool drains and when the item restarts. */
  distortion?: Distortion;
  /** Inventory volume, micro-SCU. */
  volume?: number;
  stats: Record<string, number>;
  /** Category-specific structure that does not flatten to one number. */
  detail?: Record<string, unknown>;
}

/** What a category parser gets to work with beyond the record itself. */
export interface ComponentContext {
  localization: Localization;
  /** Inventory containers by lower-case short name, for cargo grid capacity. */
  containers?: Map<string, GameRecord>;
}

interface Parsed {
  stats: Record<string, number>;
  detail?: Record<string, unknown>;
}

type Picker = (struct: Record<string, unknown>, record: GameRecord, context: ComponentContext) => Parsed;

interface Category {
  category: string;
  /** The stats struct that identifies the category, if it has one. */
  params?: string;
  /** Otherwise, or additionally, the `AttachDef.Type` values that do. */
  types?: string[];
  pick: Picker;
}

/** Only numeric stats are surfaced; the structs are full of unrelated scaffolding. */
function pickStats(params: Record<string, unknown>, fields: string[]): Record<string, number> {
  const stats: Record<string, number> = {};
  for (const field of fields) {
    const value = num(at(params, field));
    if (value !== undefined) stats[field.split(".").pop() as string] = value;
  }
  return stats;
}

/** Every top-level number on a struct, for categories with no curated list yet. */
function pickNumeric(params: Record<string, unknown>): Record<string, number> {
  const stats: Record<string, number> = {};
  for (const [key, value] of Object.entries(params)) {
    if (typeof value === "number") stats[key] = value;
  }
  return stats;
}

function ports(record: GameRecord): Record<string, unknown>[] {
  const list = component(record, "SItemPortContainerComponentParams")?.Ports;
  return Array.isArray(list) ? list.filter((p): p is Record<string, unknown> => !!p && typeof p === "object") : [];
}

/** Ports that accept the given item type, with the size they take. */
function portsOfType(record: GameRecord, type: string): { count: number; size?: number } {
  const matching = ports(record).filter((p) =>
    (Array.isArray(p.Types) ? p.Types : []).some((t) => at(t, "Type") === type),
  );
  const sizes = matching.map((p) => num(p.MaxSize)).filter((s): s is number => s !== undefined);
  return { count: matching.length, size: sizes.length > 0 ? Math.max(...sizes) : undefined };
}

/** What the item generates per second of one resource, under a stat name. */
function generated(record: GameRecord, resource: string, as: string): Record<string, number> {
  const output = resourceProfile(record).generation?.[resource];
  return output !== undefined ? { [as]: output } : {};
}

/** Unbounded ranges read 3.4e38; that is a sentinel, not a figure. */
const UNBOUNDED = 1e30;

const CATEGORIES: Category[] = [
  {
    category: "shields",
    params: "SCItemShieldGeneratorParams",
    pick: (s) => ({
      stats: pickStats(s, [
        "MaxShieldHealth",
        "MaxShieldRegen",
        "DecayRatio",
        "DownedRegenDelay",
        "DamagedRegenDelay",
        "ReservePoolDrainRateRatio",
        "ReservePoolMaxHealthRatio",
        "ReservePoolRegenRateRatio",
        "ElectricalChargeDamageResistance",
        "stunParams.maxStunTime",
      ]),
      detail: {
        resistance: damageTypeRanges(s.ShieldResistance),
        absorption: damageTypeRanges(s.ShieldAbsorption),
      },
    }),
  },
  {
    category: "quantumDrives",
    params: "SCItemQuantumDriveParams",
    pick: (s) => {
      const stats = pickStats(s, [
        "quantumFuelRequirement",
        "jumpRange",
        "disconnectRange",
        "params.driveSpeed",
        "params.cooldownTime",
        "params.stageOneAccelRate",
        "params.stageTwoAccelRate",
        "params.spoolUpTime",
        "params.engageSpeed",
        "params.calibrationRate",
        "params.minCalibrationRequirement",
        "params.maxCalibrationRequirement",
        "heatParams.inFlightThermalEnergyDraw",
      ]);
      if ((stats.jumpRange ?? 0) > UNBOUNDED) delete stats.jumpRange;
      const spline = num(at(s, "splineJumpParams.driveSpeed"));
      if (spline !== undefined) stats.splineDriveSpeed = spline;
      return { stats };
    },
  },
  {
    category: "radars",
    params: "SCItemRadarComponentParams",
    pick: (s) => {
      const detection = Array.isArray(s.signatureDetection)
        ? s.signatureDetection.map((d) => ({
            sensitivity: num(at(d, "sensitivity")),
            piercing: num(at(d, "piercing")),
            passive: at(d, "permitPassiveDetection") === true,
            active: at(d, "permitActiveDetection") === true,
          }))
        : undefined;
      return {
        stats: pickStats(s, ["detectionLifetime", "physicalRadarRange", "pingProperties.cooldownTime"]),
        detail: { signatureDetection: detection },
      };
    },
  },
  {
    category: "countermeasures",
    types: ["WeaponDefensive"],
    pick: (_s, record) => {
      const container = component(record, "SAmmoContainerComponentParams");
      return {
        stats: pickStats(container ?? {}, ["maxAmmoCount", "maxRestockCount"]),
        detail: { ammo: referencedName(container?.ammoParamsRecord) },
      };
    },
  },
  {
    category: "weapons",
    params: "SCItemWeaponComponentParams",
    pick: (s, record) => ({
      stats: {
        ...pickStats(s, ["connectionParams.simplifiedHeatParams.overheatTemperature"]),
        ...pickStats(component(record, "SAmmoContainerComponentParams") ?? {}, ["maxAmmoCount"]),
      },
    }),
  },
  {
    category: "missiles",
    params: "SCItemMissileParams",
    pick: (s) => ({
      stats: pickStats(s, [
        "targetingParams.lockTime",
        "targetingParams.lockRangeMax",
        "explosionParams.maxRadius",
        "GCSParams.linearSpeed",
        "maxLifetime",
      ]),
    }),
  },
  {
    category: "powerPlants",
    types: ["PowerPlant"],
    pick: (_s, record) => ({ stats: generated(record, "Power", "powerOutput") }),
  },
  {
    category: "coolers",
    types: ["Cooler"],
    pick: (_s, record) => ({ stats: generated(record, "Coolant", "coolantOutput") }),
  },
  {
    category: "lifeSupport",
    types: ["LifeSupportGenerator"],
    pick: (_s, record) => ({ stats: generated(record, "LifeSupport", "lifeSupportOutput") }),
  },
  {
    category: "thrusters",
    params: "SCItemThrusterParams",
    pick: (s) => ({
      stats: pickStats(s, ["thrustCapacity", "fuelBurnRatePer10KNewton", "minHealthThrustMultiplier"]),
      detail: { thrusterType: str(s.thrusterType) },
    }),
  },
  {
    category: "quantumFuelTanks",
    params: "SCItemFuelTankParams",
    types: ["QuantumFuelTank"],
    pick: (_s, record) => ({ stats: pickStats(component(record, "ResourceContainer") ?? {}, ["capacity.standardCargoUnits"]) }),
  },
  {
    category: "fuelTanks",
    params: "SCItemFuelTankParams",
    pick: (_s, record) => ({ stats: pickStats(component(record, "ResourceContainer") ?? {}, ["capacity.standardCargoUnits"]) }),
  },
  {
    category: "armor",
    params: "SCItemVehicleArmorParams",
    pick: (s) => ({
      stats: pickStats(s, ["signalInfrared", "signalElectromagnetic", "signalCrossSection"]),
      detail: {
        damageMultiplier: damageTypeMap(s.damageMultiplier),
        deflection: damageTypeMap(at(s, "armorDeflection.deflectionValue")),
        penetrationAbsorption: damageTypeMap(at(s, "armorPenetrationResistance.penetrationAbsorptionForType")),
      },
    }),
  },
  {
    category: "missileRacks",
    params: "SCItemMissileRackParams",
    pick: (_s, record) => {
      const slots = portsOfType(record, "Missile");
      const stats: Record<string, number> = { missileCount: slots.count };
      if (slots.size !== undefined) stats.missileSize = slots.size;
      return { stats };
    },
  },
  {
    category: "cargoGrids",
    params: "SCItemInventoryContainerComponentParams",
    types: ["CargoGrid"],
    pick: (s, _record, context) => {
      const name = referencedName(s.containerParams);
      const container = name ? context.containers?.get(name.toLowerCase()) : undefined;
      const parsed = container ? parseContainer(container) : undefined;
      const stats: Record<string, number> = {};
      if (parsed?.approxScu !== undefined) stats.scu = parsed.approxScu;
      return { stats, detail: { container: name, interiorDimensions: parsed?.interiorDimensions } };
    },
  },
  {
    category: "jumpDrives",
    params: "SCItemJumpDriveParams",
    pick: (s) => ({
      stats: pickStats(s, [
        "alignmentRate",
        "alignmentDecayRate",
        "tuningRate",
        "tuningDecayRate",
        "fuelUsageEfficiencyMultiplier",
      ]),
    }),
  },
  {
    category: "quantumEnforcement",
    params: "SCItemQuantumInterdictionGeneratorParams",
    pick: (s) => ({
      stats: pickStats(s, [
        "jammerSettings.jammerRange",
        "jammerSettings.maxPowerDraw",
        "quantumInterdictionPulseSettings.chargeTimeSecs",
        "quantumInterdictionPulseSettings.dischargeTimeSecs",
        "quantumInterdictionPulseSettings.cooldownTimeSecs",
        "quantumInterdictionPulseSettings.radiusMeters",
      ]),
    }),
  },
  {
    category: "emp",
    params: "SCItemEMPParams",
    pick: (s) => ({ stats: pickNumeric(s) }),
  },
  {
    category: "selfDestruct",
    params: "SSCItemSelfDestructComponentParams",
    pick: (s) => ({ stats: pickStats(s, ["damage", "minRadius", "radius", "time"]) }),
  },
  {
    category: "batteries",
    types: ["Battery"],
    pick: (_s, record) => ({ stats: pickStats(component(record, "ResourceContainer") ?? {}, ["capacity.standardCargoUnits"]) }),
  },
  {
    // Gimbal and fixed mounts: no stats struct, but the gun size they take matters.
    category: "mounts",
    types: ["Turret"],
    pick: (_s, record) => {
      const guns = portsOfType(record, "WeaponGun");
      const stats: Record<string, number> = { gunCount: guns.count };
      if (guns.size !== undefined) stats.gunSize = guns.size;
      return { stats };
    },
  },
];

function findParams(record: GameRecord, type: string): Record<string, unknown> | undefined {
  return components(record).find((c) => c._Type_ === type);
}

export { isPlaceholderName };

export function parseComponent(
  record: GameRecord,
  localization: Localization,
  context: ComponentContext = { localization },
): Component | undefined {
  const identity = itemIdentity(record, localization);

  for (const { params, types, category, pick } of CATEGORIES) {
    const struct = params ? findParams(record, params) : undefined;
    if (params && !struct) continue;
    if (types && (!identity.type || !types.includes(identity.type))) continue;
    // Swappable mounts live under weapon_mounts; a ship's own turrets share the
    // Turret type but are part of the hull, not a component anyone swaps.
    if (category === "mounts" && !record.recordPath.toLowerCase().includes("/weapon_mounts/")) continue;

    const picked = pick(struct ?? {}, record, context);
    const detail = picked.detail
      ? Object.fromEntries(Object.entries(picked.detail).filter(([, v]) => v !== undefined))
      : undefined;
    const resources = resourceProfile(record);

    return {
      name: identity.name,
      isTemplate: identity.isTemplate,
      recordName: record.name,
      id: record.id,
      category,
      type: identity.type,
      subType: identity.subType,
      size: identity.size,
      grade: identity.grade,
      manufacturer: identity.manufacturer,
      power: resources.power,
      em: resources.em,
      ir: resources.ir,
      health: healthOf(record),
      mass: massOf(record),
      distortionMax: distortionMaxOf(record),
      durability: durabilityOf(record),
      distortion: distortionOf(record),
      volume: volumeOf(record),
      stats: picked.stats,
      detail: detail && Object.keys(detail).length > 0 ? detail : undefined,
    };
  }

  return undefined;
}

/** Inventory containers by lower-case short name, for cargo grid lookups. */
export function indexContainers(records: GameRecord[]): Map<string, GameRecord> {
  const index = new Map<string, GameRecord>();
  for (const record of records) {
    if (record.recordPath.toLowerCase().includes("/inventorycontainers/")) {
      index.set(record.shortName.toLowerCase(), record);
    }
  }
  return index;
}

export const componentsDomain: Domain = {
  name: "game-ship-components",
  description:
    "Ship shields, quantum drives, power plants, coolers, radars, thrusters, fuel tanks, armour, cargo grids and more, with their stats.",
  filters: ["**/entities/scitem/ships/**", "**/inventorycontainers/**"],
  needsLocalization: true,

  parse({ records, localization }) {
    const context: ComponentContext = { localization, containers: indexContainers(records) };
    const byCategory: Record<string, Component[]> = {};

    for (const record of records) {
      if (record.type !== "EntityClassDefinition") continue;
      const component = parseComponent(record, localization, context);
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
