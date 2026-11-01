import { describe, it, expect } from "vitest";
import { componentsDomain, isPlaceholderName, parseComponent } from "./components.js";
import { parseGlobalIni } from "../localization.js";
import type { GameRecord } from "../records.js";

const localization = parseGlobalIni("item_NameShield=5SA 'Rhada'");

function shipItem(
  shortName: string,
  paramsType: string,
  stats: Record<string, unknown>,
  nameKey = "@item_NameShield",
): GameRecord {
  return {
    name: `EntityClassDefinition.${shortName}`,
    shortName,
    id: `id-${shortName}`,
    type: "EntityClassDefinition",
    recordPath: `libs/foundry/records/entities/scitem/ships/x/${shortName}`,
    value: {
      Components: [
        {
          _Type_: "SAttachableComponentParams",
          AttachDef: {
            Type: "Shield",
            SubType: "UNDEFINED",
            Size: 2,
            Grade: 1,
            Localization: { Name: nameKey },
          },
        },
        { _Type_: paramsType, ...stats },
      ],
    },
  };
}

describe("isPlaceholderName", () => {
  it("rejects every shape of non-name the game uses", () => {
    expect(isPlaceholderName(undefined)).toBe(true);
    expect(isPlaceholderName("")).toBe(true);
    expect(isPlaceholderName("@item_NameMissing")).toBe(true);
    expect(isPlaceholderName("<= PLACEHOLDER =>")).toBe(true);
    expect(isPlaceholderName("@LOC_PLACEHOLDER")).toBe(true);
  });

  it("accepts a real name", () => {
    expect(isPlaceholderName("5SA 'Rhada'")).toBe(false);
  });
});

describe("parseComponent", () => {
  it("categorises by params struct and reads the stats", () => {
    const record = shipItem("SHLD_Test", "SCItemShieldGeneratorParams", {
      MaxShieldHealth: 7200,
      MaxShieldRegen: 1584,
      DownedRegenDelay: 9.17,
      Irrelevant: "ignored",
    });

    const parsed = parseComponent(record, localization);
    expect(parsed).toMatchObject({
      category: "shields",
      name: "5SA 'Rhada'",
      size: 2,
      grade: 1,
    });
    expect(parsed?.stats).toEqual({
      MaxShieldHealth: 7200,
      MaxShieldRegen: 1584,
      DownedRegenDelay: 9.17,
    });
  });

  it("falls back to the record code and flags templates", () => {
    const record = shipItem("QDRV_S01_Template", "SCItemQuantumDriveParams", { jumpRange: 1 }, "@LOC_PLACEHOLDER");
    const parsed = parseComponent(record, localization);
    expect(parsed?.name).toBe("QDRV_S01_Template");
    expect(parsed?.isTemplate).toBe(true);
  });

  it("ignores ship items that are not one of the known categories", () => {
    // A seat blast shield lives under a shield-ish path but has no shield params.
    const seat = shipItem("MISC_BlastShield_SeatAccess", "SCItemSeatAccessParams", {});
    expect(parseComponent(seat, localization)).toBeUndefined();
  });

  it("drops non-numeric stats rather than emitting them", () => {
    const record = shipItem("X", "SCItemShieldGeneratorParams", { MaxShieldHealth: null });
    expect(parseComponent(record, localization)?.stats).toEqual({});
  });
});

describe("componentsDomain.parse", () => {
  it("groups components by category and counts them", () => {
    const result = componentsDomain.parse({
      records: [
        shipItem("A", "SCItemShieldGeneratorParams", { MaxShieldHealth: 100 }),
        shipItem("B", "SCItemShieldGeneratorParams", { MaxShieldHealth: 200 }),
        shipItem("C", "SCItemQuantumDriveParams", { jumpRange: 5 }),
      ],
      localization,
    }) as Record<string, never>;

    expect(result.summary).toEqual({ shields: 2, quantumDrives: 1 });
  });
});

function shipItemOfType(
  shortName: string,
  type: string,
  extra: Record<string, unknown>[],
  path = "x",
): GameRecord {
  return {
    name: `EntityClassDefinition.${shortName}`,
    shortName,
    id: `id-${shortName}`,
    type: "EntityClassDefinition",
    recordPath: `libs/foundry/records/entities/scitem/ships/${path}/${shortName}`,
    value: {
      Components: [
        {
          _Type_: "SAttachableComponentParams",
          AttachDef: {
            Type: type,
            SubType: "UNDEFINED",
            Size: 1,
            Grade: 3,
            Manufacturer: "file://./x/scitemmanufacturer.aegs.json",
            Localization: { Name: "@item_NameShield" },
          },
        },
        ...extra,
      ],
    },
  };
}

const online = (deltas: Record<string, unknown>[]) => ({
  _Type_: "ItemResourceComponentParams",
  states: [{ name: "Online", deltas, signatureParams: { EMSignature: { nominalSignature: 7920 } } }],
});

describe("parseComponent categories without a stats struct", () => {
  it("reads a power plant's output off the resource network", () => {
    const plant = shipItemOfType("POWR_Test", "PowerPlant", [
      online([{ generation: { resource: "Power", resourceAmountPerSecond: { units: 16 } } }]),
      { _Type_: "SHealthComponentParams", Health: 270 },
      { _Type_: "SEntityPhysicsControllerParams", PhysType: { Mass: 250 } },
      { _Type_: "SDistortionParams", Maximum: 4100 },
    ]);
    expect(parseComponent(plant, localization)).toMatchObject({
      category: "powerPlants",
      manufacturer: "AEGS",
      em: 7920,
      health: 270,
      mass: 250,
      distortionMax: 4100,
      durability: { health: 270 },
      stats: { powerOutput: 16 },
    });
  });

  it("sorts a countermeasure launcher before the weapon struct it also carries", () => {
    const launcher = shipItemOfType("CML_Test", "WeaponDefensive", [
      { _Type_: "SCItemWeaponComponentParams", fireActions: [] },
      { _Type_: "SAmmoContainerComponentParams", maxAmmoCount: 48, ammoParamsRecord: "file://./x/ammoparams.behr_flare.json" },
    ]);
    expect(parseComponent(launcher, localization)).toMatchObject({
      category: "countermeasures",
      stats: { maxAmmoCount: 48 },
      detail: { ammo: "behr_flare" },
    });
  });

  it("keeps a shield's resistance table as structured detail", () => {
    const shield = shipItem("SHLD_Test", "SCItemShieldGeneratorParams", {
      MaxShieldHealth: 3168,
      ShieldResistance: [{ Min: 0, Max: 0.25 }],
      ShieldAbsorption: [{ Min: 0, Max: 0.45 }, { Min: 1, Max: 1 }],
    });
    expect(parseComponent(shield, localization)?.detail).toEqual({
      resistance: { physical: { min: 0, max: 0.25 } },
      absorption: { physical: { min: 0, max: 0.45 }, energy: { min: 1, max: 1 } },
    });
  });

  it("drops the unbounded jump range sentinel", () => {
    const drive = shipItem("QDRV_Test", "SCItemQuantumDriveParams", {
      jumpRange: 3.402823e38,
      params: { driveSpeed: 161410900, spoolUpTime: 4.4 },
    });
    expect(parseComponent(drive, localization)?.stats).toEqual({ driveSpeed: 161410900, spoolUpTime: 4.4 });
  });

  it("resolves a cargo grid's capacity through its inventory container", () => {
    const grid = shipItemOfType("DRAK_Cutlass_CargoGrid_Main", "CargoGrid", [
      {
        _Type_: "SCItemInventoryContainerComponentParams",
        containerParams: "file://./x/inventorycontainers/ships/drak_cutlass_cargogrid_main.json",
      },
    ]);
    const container: GameRecord = {
      name: "InventoryContainer.drak_cutlass_cargogrid_main",
      shortName: "drak_cutlass_cargogrid_main",
      id: "c",
      type: "InventoryContainer",
      recordPath: "libs/foundry/records/inventorycontainers/ships/drak_cutlass_cargogrid_main",
      value: { interiorDimensions: { _Type_: "Vec3", x: 5, y: 6.25, z: 2.5 } },
    };
    const parsed = parseComponent(grid, localization, {
      localization,
      containers: new Map([["drak_cutlass_cargogrid_main", container]]),
    });
    expect(parsed?.stats).toEqual({ scu: 40 });
    expect(parsed?.detail).toMatchObject({ container: "drak_cutlass_cargogrid_main" });
  });

  it("counts the gun slots on a swappable mount and ignores a ship's own turret", () => {
    const ports = {
      _Type_: "SItemPortContainerComponentParams",
      Ports: [{ Name: "hardpoint_class_2", MaxSize: 3, Types: [{ Type: "WeaponGun" }] }],
    };
    const mount = shipItemOfType("Mount_Gimbal_S3", "Turret", [ports], "weapon_mounts/gimbal");
    expect(parseComponent(mount, localization)).toMatchObject({
      category: "mounts",
      stats: { gunCount: 1, gunSize: 3 },
    });
    const turret = shipItemOfType("DRAK_Cutlass_Turret", "Turret", [ports, { _Type_: "SCItemTurretParams" }], "turret");
    expect(parseComponent(turret, localization)).toBeUndefined();
  });
});
