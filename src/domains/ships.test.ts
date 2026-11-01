import { describe, it, expect } from "vitest";
import { parseGlobalIni } from "../localization.js";
import type { GameRecord } from "../records.js";
import { buildVehicleContext, countDuplicates, loadoutEntries, parseFlight, parsePowerPools, parseVehicle } from "./ships.js";

const localization = parseGlobalIni(
  [
    "vehicle_NameAEGS_Gladius=Aegis Gladius",
    "item_NameAllStop=AllStop",
    "item_NameRegulus=Regulus",
    "item_NamePanther=CF-337 Panther Repeater",
    "item_NameArrester=Arrester III Missile",
  ].join("\n"),
);

const record = (shortName: string, recordPath: string, components: Record<string, unknown>[]): GameRecord => ({
  name: `EntityClassDefinition.${shortName}`,
  shortName,
  id: `id-${shortName}`,
  type: "EntityClassDefinition",
  recordPath,
  value: { Components: components },
});

const shipItem = (shortName: string, dir: string, type: string, nameKey: string, extra: Record<string, unknown>[] = []) =>
  record(shortName, `libs/foundry/records/entities/scitem/ships/${dir}/${shortName.toLowerCase()}`, [
    { _Type_: "SAttachableComponentParams", AttachDef: { Type: type, SubType: "UNDEFINED", Size: 1, Localization: { Name: nameKey } } },
    ...extra,
  ]);

const entry = (itemPortName: string, item: string, children?: Record<string, unknown>[]) => ({
  itemPortName,
  entityClassName: item.endsWith(".json") ? "" : item,
  entityClassReference: item.endsWith(".json") ? `file://./x/${item}` : null,
  loadout: children ? { entries: children } : null,
});

const ammo: GameRecord = {
  name: "AmmoParams.klwe_laserrepeater_s3_ammo",
  shortName: "klwe_laserrepeater_s3_ammo",
  id: "ammo",
  type: "AmmoParams",
  recordPath: "libs/foundry/records/ammoparams/vehicle/ammoparams.klwe_laserrepeater_s3_ammo",
  value: {
    speed: 1000,
    lifetime: 2,
    projectileParams: { damage: { _Type_: "DamageInfo", DamageEnergy: 50 } },
  },
};

const gun = shipItem("KLWE_LaserRepeater_S3", "weapons", "WeaponGun", "@item_NamePanther", [
  {
    _Type_: "SCItemWeaponComponentParams",
    fireActions: [{ _Type_: "SWeaponActionFireSingleParams", name: "Fire", fireRate: 600, launchParams: { pelletCount: 1 } }],
  },
  { _Type_: "SAmmoContainerComponentParams", maxAmmoCount: 0, ammoParamsRecord: "file://./x/ammoparams.klwe_laserrepeater_s3_ammo.json" },
]);
const mount = shipItem("Mount_Gimbal_S3", "weapon_mounts/gimbal", "Turret", "@item_NameMount");
const shield = shipItem("SHLD_GODI_S01_AllStop_SCItem", "shieldgenerator", "Shield", "@item_NameAllStop", [
  { _Type_: "SCItemShieldGeneratorParams", MaxShieldHealth: 3168, MaxShieldRegen: 602 },
]);
const plant = shipItem("POWR_AEGS_S01_Regulus_SCItem", "powerplant", "PowerPlant", "@item_NameRegulus", [
  { _Type_: "ItemResourceComponentParams", states: [{ name: "Online", deltas: [{ generation: { resource: "Power", resourceAmountPerSecond: { units: 16 } } }] }] },
]);
const rack = shipItem("MRCK_S03_BEHR_Single_S03", "missile_racks", "MissileLauncher", "@item_NameRack");
const missile = shipItem("MISL_S03_CS_FSKI_Arrester", "weapons", "Missile", "@item_NameArrester", [
  { _Type_: "SCItemMissileParams", explosionParams: { damage: { _Type_: "DamageInfo", DamagePhysical: 3200 } } },
]);
const mainThruster = shipItem("AEGS_Gladius_Thruster_Main", "thrusters", "MainThruster", "@x", [
  { _Type_: "SCItemThrusterParams", thrustCapacity: 2324002 },
]);
const retroThruster = shipItem("AEGS_Gladius_Thruster_Retro", "thrusters", "ManneuverThruster", "@x", [
  { _Type_: "SCItemThrusterParams", thrustCapacity: 500000 },
]);
const seat = shipItem("AEGS_Gladius_SCItem_Seat_Pilot", "seat", "Seat", "@x");

const gladius = record("AEGS_Gladius", "libs/foundry/records/entities/spaceships/aegs_gladius", [
  {
    _Type_: "VehicleComponentParams",
    vehicleName: "@vehicle_NameAEGS_Gladius",
    crewSize: 1,
    vehicleHullDamageNormalizationValue: 2134,
    manufacturer: "file://./x/scitemmanufacturer.aegs.json",
  },
  { _Type_: "SAttachableComponentParams", AttachDef: { Type: "NOITEM_Vehicle", Size: 2 } },
  {
    _Type_: "SHealthComponentParams",
    Health: 1,
    DamageResistances: { PhysicalResistance: { Multiplier: 1 }, EnergyResistance: { Multiplier: 0.8 } },
  },
  {
    _Type_: "SSCSignatureSystemParams",
    radarProperties: { crossSectionParams: { crossSection: { _Type_: "Vec3", x: 6923, y: 1731, z: 8654 } } },
  },
  {
    _Type_: "SEntityComponentDefaultLoadoutParams",
    loadout: {
      entries: [
        entry("hardpoint_seat_pilot", "AEGS_Gladius_SCItem_Seat_Pilot"),
        entry("hardpoint_power_plant", "POWR_AEGS_S01_Regulus_SCItem"),
        entry("hardpoint_shield_generator_left", "SHLD_GODI_S01_AllStop_SCItem"),
        entry("hardpoint_shield_generator_right", "SHLD_GODI_S01_AllStop_SCItem"),
        entry("hardpoint_gun_nose", "mount_gimbal_s3.json", [entry("hardpoint_class_2", "klwe_laserrepeater_s3.json")]),
        entry("hardpoint_gun_left_wing", "mount_gimbal_s3.json", [entry("hardpoint_class_2", "klwe_laserrepeater_s3.json")]),
        // Fixed gun with the stale self-reference AI variants carry.
        entry("hardpoint_gun_right_wing", "KLWE_LaserRepeater_S3", [entry("hardpoint_class_2", "klwe_laserrepeater_s3.json")]),
        entry("hardpoint_missilerack_left", "MRCK_S03_BEHR_Single_S03", [entry("missile_01_attach", "MISL_S03_CS_FSKI_Arrester")]),
        entry("hardpoint_thruster_main_left", "AEGS_Gladius_Thruster_Main"),
        entry("hardpoint_thruster_retro_left", "AEGS_Gladius_Thruster_Retro"),
        entry("hardpoint_unknown", "Something_Not_Extracted"),
        entry("hardpoint_paint", ""),
      ],
    },
  },
]);

const records = [gladius, gun, mount, shield, plant, rack, missile, mainThruster, retroThruster, seat, ammo];

describe("loadoutEntries", () => {
  it("flattens the tree, naming items by class or file reference, and keeps parents", () => {
    const entries = loadoutEntries(gladius);
    const panther = entries.filter((e) => e.item === "klwe_laserrepeater_s3");
    // Two in gimbals plus the stale self-reference under the fixed gun.
    expect(panther).toHaveLength(3);
    expect(panther[0].parent?.item).toBe("mount_gimbal_s3");
    expect(entries.some((e) => e.port === "hardpoint_paint")).toBe(false);
  });
});

describe("countDuplicates", () => {
  it("collapses identical items into one entry with a count", () => {
    expect(countDuplicates([{ item: "A" }, { item: "B" }, { item: "A" }, { item: "A", size: 2 }])).toEqual([
      { item: "A", count: 2 },
      { item: "B" },
      { item: "A", size: 2 },
    ]);
  });
});

describe("parseVehicle with a loadout", () => {
  const context = buildVehicleContext(records, localization);
  const vehicle = parseVehicle(gladius, localization, context)!;

  it("reads hull, size and cross section off the hull entity", () => {
    expect(vehicle.size).toBe(2);
    expect(vehicle.hull).toEqual({ damageNormalization: 2134, resistances: { energy: 0.8 } });
    expect(vehicle.crossSection).toEqual({ x: 6923, y: 1731, z: 8654 });
  });

  it("sorts stock items by what they are, not by port name", () => {
    expect(vehicle.loadout).toMatchObject({
      powerPlants: [{ item: "POWR_AEGS_S01_Regulus_SCItem", name: "Regulus" }],
      shields: [{ item: "SHLD_GODI_S01_AllStop_SCItem", name: "AllStop", count: 2 }],
      weapons: [
        { item: "KLWE_LaserRepeater_S3", name: "CF-337 Panther Repeater", mount: "mount_gimbal_s3", dps: 500, count: 2 },
        { item: "KLWE_LaserRepeater_S3", dps: 500 },
      ],
      missiles: [{ item: "MISL_S03_CS_FSKI_Arrester", rack: "MRCK_S03_BEHR_Single_S03", damage: 3200 }],
      thrusters: { main: 1, retro: 1 },
      missileRacks: 1,
    });
    // The seat and the unknown item are not loadout facts anyone compares.
    expect(JSON.stringify(vehicle.loadout)).not.toMatch(/Seat_Pilot|Something_Not_Extracted/);
    expect(vehicle.loadout?.weapons?.some((w) => w.mount === w.item)).toBe(false);
  });

  it("sums the loadout into totals, honouring counts", () => {
    expect(vehicle.totals).toEqual({
      weaponDps: 1500,
      weapons: 3,
      missiles: 1,
      missileDamage: 3200,
      shieldHealth: 6336,
      shieldRegen: 1204,
      powerOutput: 16,
      thrustMain: 2324002,
      thrustRetro: 500000,
    });
  });

  it("leaves loadout and totals off when no context is given", () => {
    const bare = parseVehicle(gladius, localization)!;
    expect(bare.loadout).toBeUndefined();
    expect(bare.totals).toBeUndefined();
    expect(bare.name).toBe("Aegis Gladius");
  });
});

describe("parseFlight", () => {
  const controller = (ifcs: Record<string, unknown>) =>
    record("Controller_Flight_ORIG_100i", "libs/foundry/records/entities/scitem/ships/controller/controller_flight_orig_100i", [
      { _Type_: "SAttachableComponentParams", AttachDef: { Type: "FlightController" } },
      { _Type_: "IFCSParams", ...ifcs },
    ]);

  it("reads speeds and the live angular block, mapping IFCS axes to pitch, yaw and roll", () => {
    const flight = parseFlight(
      controller({
        scmSpeed: 223,
        maxSpeed: 1230,
        boostSpeedForward: 610,
        boostSpeedBackward: 290,
        maxAngularVelocity: { x: 60, y: 155, z: 52 },
        // Placeholder block most hulls carry; must not win.
        speedProfile: { angularVelocity: { x: 60, y: 200, z: 60 } },
        afterburnerNew: { capacitorMax: 20, capacitorRegenPerSec: 0.75, capacitorRegenDelayAfterUse: 0.2 },
        ifcsCoreParams: { stunThresholdGs: 50 },
      }),
    );
    expect(flight).toEqual({
      controller: "Controller_Flight_ORIG_100i",
      scmSpeed: 223,
      maxSpeed: 1230,
      boostSpeedForward: 610,
      boostSpeedBackward: 290,
      pitch: 60,
      yaw: 52,
      roll: 155,
      boostCapacitor: { max: 20, regenPerSecond: 0.75, regenDelay: 0.2 },
      stunThresholdGs: 50,
    });
  });

  it("is undefined for a controller without IFCS parameters", () => {
    expect(parseFlight(record("X", "x", []))).toBeUndefined();
  });
});

describe("parsePowerPools", () => {
  it("keeps item types with a pip pool and drops unlimited entries", () => {
    const ship = record("S", "libs/foundry/records/entities/spaceships/s", [
      {
        _Type_: "SItemPortContainerComponentParams",
        resourceNetworkPowerPools: {
          itemPools: [{ itemType: "WeaponGun", poolSize: 4 }, { itemType: "Shield", maxItemCount: 2 }, null],
        },
      },
    ]);
    expect(parsePowerPools(ship)).toEqual({ WeaponGun: 4 });
  });
});
