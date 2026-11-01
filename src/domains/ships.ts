/**
 * Ships and ground vehicles.
 *
 * A vehicle is an EntityClassDefinition carrying `VehicleComponentParams`, which
 * holds the identity fields: name, career, role, crew size, manufacturer and
 * bounding box.
 *
 * Career and role are localization keys (`@vehicle_focus_combat`,
 * `@vehicle_class_lightfighter`), so the string table is required.
 *
 * The stock loadout is `SEntityComponentDefaultLoadoutParams`: a tree of item
 * ports, each naming the item that ships in it (by class name or file
 * reference) and, for mounts and racks, the items nested inside. The port
 * names vary by ship, so items are sorted by what they are, read off the
 * resolved item's `AttachDef.Type`, not by what the port is called. Resolving
 * needs the ship item records and, for guns, their ammo, which is why this
 * domain pulls in the same records the weapons and components domains use.
 *
 * `totals` sums the loadout into the figures a patch is most likely to move:
 * stock DPS, shield pool, power output, fuel, cargo and thrust.
 *
 * Note what is *not* here. `SHealthComponentParams.Health` on the hull reads 1.0
 * because hull health is normalised and the real figure is distributed across
 * damageable parts in the vehicle XML, which DataCore does not carry. Anything
 * presenting a single hull HP number has computed it from part geometry rather
 * than read it, so this domain does not invent one; it surfaces the
 * normalisation value and the hull's damage multipliers, which do live here.
 * Vehicle mass lives in the same XML and is absent for the same reason.
 *
 * Flight numbers do live in DataCore, on the flight controller item the ship
 * ships with (`IFCSParams`): SCM, max and boost speeds, and angular rates.
 * `maxAngularVelocity` is the live block; `speedProfile.angularVelocity` reads
 * a 60/200/60 placeholder on most hulls and is ignored when the live one exists.
 */

import { component, referencedName, vec } from "../entity.js";
import { isPlaceholderName, localize, type Localization } from "../localization.js";
import { at, num, str, type GameRecord } from "../records.js";
import { indexContainers, parseComponent, type Component } from "./components.js";
import { attachDef, healthResistances } from "./item-common.js";
import type { Domain } from "./types.js";
import {
  indexAmmo,
  parseMissile,
  parseWeapon,
  totalDamage,
  type Missile,
  type Weapon,
  type WeaponContext,
} from "./weapons.js";

export interface Vehicle {
  name: string;
  recordName: string;
  id: string;
  displayName?: string;
  description?: string;
  manufacturer?: string;
  /** "Spaceship", "Gravlev", "Ground" and similar. */
  movementClass?: string;
  career?: string;
  role?: string;
  crewSize?: number;
  /** The game's own size class, 1 to 6. */
  size?: number;
  isGravlev?: boolean;
  /** Metres, as x/y/z. */
  boundingBox?: Record<string, number>;
  hull?: Hull;
  /** Radar cross section, metres squared per axis. */
  crossSection?: Record<string, number>;
  loadout?: Loadout;
  totals?: Totals;
  /** Flight envelope from the stock flight controller's IFCS parameters. */
  flight?: Flight;
  /** The stock armour: health, deflection thresholds and damage multipliers. */
  armor?: Armor;
  /** Power pips the ship can route to each item type, from its resource network. */
  powerPools?: Record<string, number>;
  isTemplate?: true;
}

export interface Flight {
  /** Item record of the flight controller the numbers came from. */
  controller: string;
  scmSpeed?: number;
  maxSpeed?: number;
  boostSpeedForward?: number;
  boostSpeedBackward?: number;
  /** Degrees per second. */
  pitch?: number;
  yaw?: number;
  roll?: number;
  /** Boost capacitor: pool size, regen per second, and delay before regen. */
  boostCapacitor?: { max?: number; regenPerSecond?: number; regenDelay?: number };
  /** G-load that stuns the pilot. */
  stunThresholdGs?: number;
}

export interface Armor {
  item: string;
  health?: number;
  /** Per-hit damage at or below which a hit of that type is deflected. */
  deflection?: Record<string, number>;
  /** Fraction of hull damage that gets through, by type. */
  damageMultiplier?: Record<string, number>;
  /** Signature multipliers the armour applies. */
  signals?: Record<string, number>;
}

export interface Hull {
  /** The figure hull damage is normalised against; the closest thing to a hull HP in DataCore. */
  damageNormalization?: number;
  /** Damage multipliers that differ from 1, by damage type. */
  resistances?: Record<string, number>;
}

/** One kind of item in the stock loadout; identical items in sibling ports are counted, not listed. */
export interface LoadoutItem {
  /** Record short name of the item. */
  item: string;
  name?: string;
  size?: number;
  /** Omitted when it is 1. */
  count?: number;
}

export interface LoadoutWeapon extends LoadoutItem {
  type?: string;
  /** The mount or turret the gun sits in, when it is not fixed to the hull. */
  mount?: string;
  dps?: number;
}

export interface LoadoutMissile extends LoadoutItem {
  rack?: string;
  /** Per missile. */
  damage?: number;
}

export interface LoadoutCountermeasure extends LoadoutItem {
  ammo?: string;
  capacity?: number;
}

export interface Loadout {
  weapons?: LoadoutWeapon[];
  missiles?: LoadoutMissile[];
  shields?: LoadoutItem[];
  powerPlants?: LoadoutItem[];
  coolers?: LoadoutItem[];
  quantumDrives?: LoadoutItem[];
  jumpDrives?: LoadoutItem[];
  radars?: LoadoutItem[];
  armor?: LoadoutItem;
  lifeSupport?: LoadoutItem[];
  countermeasures?: LoadoutCountermeasure[];
  fuelTanks?: LoadoutItem[];
  quantumFuelTanks?: LoadoutItem[];
  cargoGrids?: LoadoutItem[];
  /** Thruster count by role. */
  thrusters?: { main?: number; retro?: number; maneuver?: number };
  turrets?: number;
  missileRacks?: number;
}

export interface Totals {
  /** Stock guns, summed DPS. */
  weaponDps?: number;
  weapons?: number;
  missiles?: number;
  missileDamage?: number;
  shieldHealth?: number;
  shieldRegen?: number;
  armorHealth?: number;
  powerOutput?: number;
  /** Hydrogen fuel, SCU. */
  fuel?: number;
  quantumFuel?: number;
  cargoScu?: number;
  thrustMain?: number;
  thrustRetro?: number;
  thrustManeuver?: number;
  /** Decoys by ammo kind, e.g. { flare: 48, chaff: 48 }. */
  countermeasures?: Record<string, number>;
}

/** The records a loadout resolves against. */
export interface VehicleContext {
  localization: Localization;
  /** Ship item records by lower-case short name. */
  items: Map<string, GameRecord>;
  components: Map<string, Component>;
  weapons: Map<string, Weapon>;
  missiles: Map<string, Missile>;
}

/** Career and role keys read "@vehicle_focus_combat"; keep the meaningful tail. */
function tidyClassifier(value: string | undefined): string | undefined {
  if (!value) return undefined;
  if (!value.startsWith("@")) return value;
  const tail = value.replace(/^@vehicle_(focus|class)_/i, "").replace(/^@/, "");
  return tail.length > 0 ? tail : undefined;
}

function parseHull(record: GameRecord): Hull | undefined {
  const vehicle = component(record, "VehicleComponentParams");
  const health = component(record, "SHealthComponentParams");
  const hull: Hull = {
    damageNormalization: num(vehicle?.vehicleHullDamageNormalizationValue),
    resistances: healthResistances(health?.DamageResistances),
  };
  return hull.damageNormalization !== undefined || hull.resistances ? hull : undefined;
}

interface Entry {
  port: string;
  item: string;
  /** The entry this one sits inside, when nested in a mount or rack. */
  parent?: Entry;
  children: Entry[];
}

/** Flatten the loadout tree, keeping each entry's parent for mounts and racks. */
export function loadoutEntries(record: GameRecord): Entry[] {
  const out: Entry[] = [];
  const walk = (loadout: unknown, parent?: Entry) => {
    const entries = at(loadout, "entries");
    if (!Array.isArray(entries)) return;
    for (const raw of entries) {
      if (!raw || typeof raw !== "object") continue;
      const e = raw as Record<string, unknown>;
      const item = str(e.entityClassName) || referencedName(e.entityClassReference);
      const port = str(e.itemPortName);
      if (!item || !port) continue;
      const entry: Entry = { port, item, parent, children: [] };
      parent?.children.push(entry);
      out.push(entry);
      walk(e.loadout, entry);
    }
  };
  walk(component(record, "SEntityComponentDefaultLoadoutParams")?.loadout);
  return out;
}

const round = (value: number, places = 2): number => {
  const factor = 10 ** places;
  return Math.round(value * factor) / factor;
};

function sum(values: (number | undefined)[]): number | undefined {
  const present = values.filter((v): v is number => v !== undefined);
  return present.length > 0 ? round(present.reduce((a, b) => a + b, 0)) : undefined;
}

function push<K extends keyof Loadout>(
  loadout: Loadout,
  key: K,
  value: Loadout[K] extends (infer T)[] | undefined ? T : never,
): void {
  ((loadout[key] as unknown[] | undefined) ?? ((loadout[key] as unknown) = [] as unknown[]) as unknown[]).push(value);
}

/** Collapse identical entries (same item in the same mount or rack) into one with a count. */
export function countDuplicates<T extends LoadoutItem>(items: T[]): T[] {
  const identity = (item: T) => JSON.stringify({ ...item, count: undefined });
  const out: T[] = [];
  for (const item of items) {
    const twin = out.find((o) => identity(o) === identity(item));
    if (twin) twin.count = (twin.count ?? 1) + 1;
    else out.push({ ...item });
  }
  return out;
}

const GUN_TYPES = new Set(["WeaponGun", "WeaponMining", "TractorBeam", "EMP", "WeaponDefensive"]);

export function parseFlight(controller: GameRecord): Flight | undefined {
  const ifcs = component(controller, "IFCSParams");
  if (!ifcs) return undefined;
  // `maxAngularVelocity` is the live figure. `speedProfile.angularVelocity`
  // reads a 60/200/60 placeholder on most hulls, so it is only a fallback.
  const angular = ifcs.maxAngularVelocity ?? at(ifcs, "speedProfile.angularVelocity");
  const boost = at(ifcs, "afterburnerNew");
  const capacitor = {
    max: num(at(boost, "capacitorMax")),
    regenPerSecond: num(at(boost, "capacitorRegenPerSec")),
    regenDelay: num(at(boost, "capacitorRegenDelayAfterUse")),
  };
  return {
    controller: controller.shortName,
    scmSpeed: num(ifcs.scmSpeed),
    maxSpeed: num(ifcs.maxSpeed),
    boostSpeedForward: num(ifcs.boostSpeedForward),
    boostSpeedBackward: num(ifcs.boostSpeedBackward),
    // IFCS axes: x is pitch, y is roll, z is yaw.
    pitch: num(at(angular, "x")),
    yaw: num(at(angular, "z")),
    roll: num(at(angular, "y")),
    boostCapacitor: Object.values(capacitor).some((v) => v !== undefined) ? capacitor : undefined,
    stunThresholdGs: num(at(ifcs, "ifcsCoreParams.stunThresholdGs")),
  };
}

function parseArmor(component: Component | undefined): Armor | undefined {
  if (!component) return undefined;
  const detail = component.detail ?? {};
  return {
    item: component.recordName.replace(/^[^.]+\./, ""),
    health: component.health,
    deflection: detail.deflection as Record<string, number> | undefined,
    damageMultiplier: detail.damageMultiplier as Record<string, number> | undefined,
    signals: Object.keys(component.stats).length > 0 ? component.stats : undefined,
  };
}

/** Power pips by item type from the ship's `resourceNetworkPowerPools`. */
export function parsePowerPools(record: GameRecord): Record<string, number> | undefined {
  const pools = at(component(record, "SItemPortContainerComponentParams"), "resourceNetworkPowerPools.itemPools");
  if (!Array.isArray(pools)) return undefined;
  const out: Record<string, number> = {};
  for (const pool of pools) {
    const type = str(at(pool, "itemType"));
    const size = num(at(pool, "poolSize"));
    if (type && size !== undefined && size > 0) out[type] = size;
  }
  return Object.keys(out).length > 0 ? out : undefined;
}

export function parseLoadout(
  record: GameRecord,
  context: VehicleContext,
): { loadout?: Loadout; totals?: Totals; flight?: Flight; armor?: Armor } {
  const loadout: Loadout = {};
  let flight: Flight | undefined;
  const thrusters = { main: 0, retro: 0, maneuver: 0 };
  const countermeasures: Record<string, number> = {};
  let turrets = 0;
  let racks = 0;

  for (const entry of loadoutEntries(record)) {
    const itemRecord = context.items.get(entry.item.toLowerCase());
    if (!itemRecord) continue;
    // AI variants fix a gun straight to the hardpoint and keep a stale child
    // entry naming the same gun under it. A gun holds attachments, not guns.
    const parentRecord = entry.parent ? context.items.get(entry.parent.item.toLowerCase()) : undefined;
    if (parentRecord && GUN_TYPES.has(str(attachDef(parentRecord).Type) ?? "")) continue;
    const def = attachDef(itemRecord);
    const type = str(def.Type);
    const subType = str(def.SubType);
    const key = itemRecord.shortName.toLowerCase();
    const component = context.components.get(key);
    const base: LoadoutItem = {
      item: itemRecord.shortName,
      name: component && !component.isTemplate ? component.name : undefined,
      size: component?.size ?? num(def.Size),
    };

    switch (type) {
      case "Shield":
        push(loadout, "shields", base);
        break;
      case "PowerPlant":
        push(loadout, "powerPlants", base);
        break;
      case "Cooler":
        push(loadout, "coolers", base);
        break;
      case "QuantumDrive":
        push(loadout, "quantumDrives", base);
        break;
      case "JumpDrive":
        push(loadout, "jumpDrives", base);
        break;
      case "Radar":
        push(loadout, "radars", base);
        break;
      case "Armor":
        loadout.armor = base;
        break;
      case "FlightController":
        flight ??= parseFlight(itemRecord);
        break;
      case "LifeSupportGenerator":
        push(loadout, "lifeSupport", base);
        break;
      case "FuelTank":
      case "ExternalFuelTank":
        push(loadout, "fuelTanks", base);
        break;
      case "QuantumFuelTank":
        push(loadout, "quantumFuelTanks", base);
        break;
      case "CargoGrid":
        push(loadout, "cargoGrids", base);
        break;
      case "MainThruster":
      case "ManneuverThruster":
        if (subType === "Retro" || /retro/i.test(entry.port)) thrusters.retro += 1;
        else if (type === "MainThruster") thrusters.main += 1;
        else thrusters.maneuver += 1;
        break;
      case "Turret":
      case "TurretBase":
      case "UtilityTurret":
        turrets += 1;
        break;
      case "MissileLauncher":
      case "GroundVehicleMissileLauncher":
      case "BombLauncher":
        racks += 1;
        break;
      case "Missile": {
        const missile = context.missiles.get(key);
        push(loadout, "missiles", {
          ...base,
          name: missile?.name ?? base.name,
          size: missile?.size ?? base.size,
          rack: entry.parent?.item,
          damage: totalDamage(missile?.damage),
        });
        break;
      }
      case "WeaponDefensive": {
        const ammo = str(component?.detail?.ammo);
        const kind = ammo?.split("_").pop();
        const capacity = component?.stats.maxAmmoCount;
        if (kind && capacity !== undefined) countermeasures[kind] = (countermeasures[kind] ?? 0) + capacity;
        push(loadout, "countermeasures", { ...base, ammo, capacity });
        break;
      }
      default:
        if (type && GUN_TYPES.has(type)) {
          const weapon = context.weapons.get(key);
          push(loadout, "weapons", {
            ...base,
            name: weapon?.name ?? base.name,
            size: weapon?.size ?? base.size,
            type,
            mount: entry.parent?.item,
            dps: weapon?.dps,
          });
        }
    }
  }

  for (const key of Object.keys(loadout) as (keyof Loadout)[]) {
    const list = loadout[key];
    if (Array.isArray(list)) (loadout[key] as unknown) = countDuplicates(list);
  }

  if (thrusters.main + thrusters.retro + thrusters.maneuver > 0) {
    loadout.thrusters = {
      main: thrusters.main || undefined,
      retro: thrusters.retro || undefined,
      maneuver: thrusters.maneuver || undefined,
    };
  }
  if (turrets > 0) loadout.turrets = turrets;
  if (racks > 0) loadout.missileRacks = racks;
  if (Object.keys(loadout).length === 0) return { flight };

  const times = (value: number | undefined, count: number | undefined) =>
    value === undefined ? undefined : value * (count ?? 1);
  const stat = (items: LoadoutItem[] | undefined, field: string) =>
    sum((items ?? []).map((i) => times(context.components.get(i.item.toLowerCase())?.stats[field], i.count)));
  const countOf = (items: LoadoutItem[] | undefined) =>
    items ? items.reduce((n, i) => n + (i.count ?? 1), 0) : undefined;
  const thrust = (role: "Main" | "Retro" | "Maneuver") =>
    sum(
      loadoutEntries(record).map((e) => {
        const component = context.components.get(e.item.toLowerCase());
        if (component?.category !== "thrusters") return undefined;
        const isRetro = component.subType === "Retro" || /retro/i.test(e.port);
        const isMain = component.type === "MainThruster";
        const matches = role === "Retro" ? isRetro : role === "Main" ? isMain && !isRetro : !isMain && !isRetro;
        return matches ? component.stats.thrustCapacity : undefined;
      }),
    );

  const totals: Totals = {
    weaponDps: sum((loadout.weapons ?? []).map((w) => times(w.dps, w.count))),
    weapons: countOf(loadout.weapons),
    missiles: countOf(loadout.missiles),
    missileDamage: sum((loadout.missiles ?? []).map((m) => times(m.damage, m.count))),
    shieldHealth: stat(loadout.shields, "MaxShieldHealth"),
    shieldRegen: stat(loadout.shields, "MaxShieldRegen"),
    armorHealth: loadout.armor ? context.components.get(loadout.armor.item.toLowerCase())?.health : undefined,
    powerOutput: stat(loadout.powerPlants, "powerOutput"),
    fuel: stat(loadout.fuelTanks, "standardCargoUnits"),
    quantumFuel: stat(loadout.quantumFuelTanks, "standardCargoUnits"),
    cargoScu: stat(loadout.cargoGrids, "scu"),
    thrustMain: thrust("Main"),
    thrustRetro: thrust("Retro"),
    thrustManeuver: thrust("Maneuver"),
    countermeasures: Object.keys(countermeasures).length > 0 ? countermeasures : undefined,
  };

  const armor = loadout.armor ? parseArmor(context.components.get(loadout.armor.item.toLowerCase())) : undefined;
  return { loadout, totals, flight, armor };
}

export function parseVehicle(
  record: GameRecord,
  localization: Localization,
  context?: VehicleContext,
): Vehicle | undefined {
  const params = component(record, "VehicleComponentParams");
  if (!params) return undefined;

  const rawName = str(params.vehicleName);
  const displayName = localize(localization, rawName);
  const description = localize(localization, params.vehicleDescription);

  // Career and role resolve through the string table when present, and fall
  // back to the key's own tail when the table has no entry for them.
  const career = localize(localization, params.vehicleCareer);
  const role = localize(localization, params.vehicleRole);

  const resolved = context ? parseLoadout(record, context) : {};

  return {
    name: isPlaceholderName(displayName) ? record.shortName : (displayName as string),
    recordName: record.name,
    id: record.id,
    displayName: isPlaceholderName(displayName) ? undefined : displayName,
    description: isPlaceholderName(description) ? undefined : description,
    manufacturer: referencedName(params.manufacturer),
    movementClass: str(params.movementClass),
    career: isPlaceholderName(career) ? tidyClassifier(str(params.vehicleCareer)) : career,
    role: isPlaceholderName(role) ? tidyClassifier(str(params.vehicleRole)) : role,
    crewSize: num(params.crewSize),
    size: num(attachDef(record).Size),
    isGravlev: params.isGravlevVehicle === true ? true : undefined,
    boundingBox: vec(params.maxBoundingBoxSize),
    hull: parseHull(record),
    crossSection: vec(at(component(record, "SSCSignatureSystemParams"), "radarProperties.crossSectionParams.crossSection")),
    loadout: resolved.loadout,
    totals: resolved.totals,
    flight: resolved.flight,
    armor: resolved.armor,
    powerPools: parsePowerPools(record),
    isTemplate: isPlaceholderName(displayName) || undefined,
  };
}

/** Index every ship item, component, gun and missile a loadout can name. */
export function buildVehicleContext(records: GameRecord[], localization: Localization): VehicleContext {
  const items = new Map<string, GameRecord>();
  const entities = new Map<string, GameRecord>();
  for (const record of records) {
    if (record.type !== "EntityClassDefinition") continue;
    entities.set(record.shortName.toLowerCase(), record);
    if (record.recordPath.toLowerCase().includes("/entities/scitem/ships/")) {
      items.set(record.shortName.toLowerCase(), record);
    }
  }

  const containers = indexContainers(records);
  const components = new Map<string, Component>();
  const weapons = new Map<string, Weapon>();
  const missiles = new Map<string, Missile>();
  const weaponContext: WeaponContext = { localization, ammoIndex: indexAmmo(records), entities };

  for (const [key, record] of items) {
    const component = parseComponent(record, localization, { localization, containers });
    if (component) components.set(key, component);
    if (!record.recordPath.toLowerCase().includes("/scitem/ships/weapons/")) continue;
    const missile = parseMissile(record, localization);
    if (missile) {
      missiles.set(key, missile);
      continue;
    }
    const weapon = parseWeapon(record, "ship", weaponContext);
    if (weapon) weapons.set(key, weapon);
  }

  return { localization, items, components, weapons, missiles };
}

export const shipsDomain: Domain = {
  name: "game-ships",
  description:
    "Ships and ground vehicles with manufacturer, role, crew, dimensions, hull resistances, stock loadout and its totals.",
  filters: [
    "**/entities/spaceships/**",
    "**/entities/groundvehicles/**",
    "**/entities/scitem/ships/**",
    "**/ammoparams/**",
    "**/inventorycontainers/**",
  ],
  needsLocalization: true,

  parse({ records, localization }) {
    const context = buildVehicleContext(records, localization);
    const vehicles = records
      .filter((r) => /\/entities\/(spaceships|groundvehicles)\//i.test(r.recordPath))
      .map((r) => parseVehicle(r, localization, context))
      .filter((v): v is Vehicle => v !== undefined)
      .sort((a, b) => a.name.localeCompare(b.name));

    const named = vehicles.filter((v) => !v.isTemplate);

    const byManufacturer: Record<string, number> = {};
    const byRole: Record<string, number> = {};
    for (const vehicle of named) {
      if (vehicle.manufacturer) {
        byManufacturer[vehicle.manufacturer] = (byManufacturer[vehicle.manufacturer] ?? 0) + 1;
      }
      if (vehicle.role) byRole[vehicle.role] = (byRole[vehicle.role] ?? 0) + 1;
    }

    return {
      vehicles,
      byManufacturer: Object.fromEntries(
        Object.entries(byManufacturer).sort((a, b) => b[1] - a[1]),
      ),
      byRole: Object.fromEntries(Object.entries(byRole).sort((a, b) => b[1] - a[1])),
      summary: {
        vehicles: vehicles.length,
        named: named.length,
        templates: vehicles.length - named.length,
        withLoadout: vehicles.filter((v) => v.loadout).length,
      },
    };
  },
};
