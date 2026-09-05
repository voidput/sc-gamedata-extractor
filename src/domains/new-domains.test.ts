import { describe, it, expect } from "vitest";
import { ammoDomain, findDamage, parseAmmo } from "./ammo.js";
import { containersDomain, parseContainer } from "./containers.js";
import { buildTypeNames, parseLocation, starmapDomain } from "./starmap.js";
import { organisationOf, reputationDomain } from "./reputation.js";
import { parseVehicle } from "./ships.js";
import { parseGlobalIni } from "../localization.js";
import type { GameRecord } from "../records.js";

const empty = new Map<string, string>();

function rec(type: string, shortName: string, value: Record<string, unknown>): GameRecord {
  return {
    name: `${type}.${shortName}`,
    shortName,
    id: `id-${shortName}`,
    type,
    recordPath: `libs/foundry/records/x/${shortName.toLowerCase()}`,
    value: { _Type_: type, ...value },
  };
}

describe("ammo damage", () => {
  it("finds damage nested under projectileParams", () => {
    // Damage is on the ammo, never on the weapon.
    const damage = findDamage({
      projectileParams: { damage: { _Type_: "DamageInfo", DamagePhysical: 52.7, DamageEnergy: 0 } },
    });
    expect(damage).toEqual({ physical: 52.7 });
  });

  it("ignores an all-zero DamageInfo, as weapon death explosions are", () => {
    expect(
      findDamage({ DeathExplosionParams: { damage: { _Type_: "DamageInfo", DamagePhysical: 0 } } }),
    ).toBeUndefined();
  });

  it("derives range from speed and lifetime", () => {
    const ammo = parseAmmo(
      rec("AmmoParams", "Test", {
        speed: 1000,
        lifetime: 4,
        projectileParams: { damage: { _Type_: "DamageInfo", DamagePhysical: 10, DamageEnergy: 5 } },
      }),
    );
    expect(ammo.range).toBe(4000);
    expect(ammo.damageTotal).toBe(15);
  });

  it("counts only ammo that actually deals damage", () => {
    const result = ammoDomain.parse({
      records: [
        rec("AmmoParams", "A", {
          projectileParams: { damage: { _Type_: "DamageInfo", DamagePhysical: 10 } },
        }),
        rec("AmmoParams", "B", {}),
      ],
      localization: empty,
    }) as Record<string, never>;
    expect(result.summary).toMatchObject({ ammo: 2, withDamage: 1, maxDamage: 10 });
  });
});

describe("containers", () => {
  it("derives volume and SCU from interior dimensions", () => {
    const c = parseContainer(
      rec("InventoryContainer", "Grid", { interiorDimensions: { x: 2, y: 2, z: 2 } }),
    );
    expect(c.volumeCubicMetres).toBe(8);
    expect(c.approxScu).toBeCloseTo(4.1, 1);
  });

  it("flags sentinel dimensions instead of reporting half a billion SCU", () => {
    // A landing zone reads 999x999x999, meaning unbounded rather than measured.
    const c = parseContainer(
      rec("InventoryContainer", "landingZone", { interiorDimensions: { x: 999, y: 999, z: 999 } }),
    );
    expect(c.unbounded).toBe(true);
    expect(c.approxScu).toBeUndefined();
  });

  it("excludes unbounded containers from the largest-capacity figure", () => {
    const result = containersDomain.parse({
      records: [
        rec("InventoryContainer", "Real", { interiorDimensions: { x: 2, y: 2, z: 2 } }),
        rec("InventoryContainer", "Zone", { interiorDimensions: { x: 999, y: 999, z: 999 } }),
      ],
      localization: empty,
    }) as Record<string, never>;
    const summary = result.summary as unknown as Record<string, number>;
    expect(summary.unbounded).toBe(1);
    expect(summary.largestScu).toBeCloseTo(4.1, 1);
  });
});

describe("starmap types", () => {
  const typesRecord = rec("StarMapObjectTypes", "StarMapObjectTypes", {
    types: [
      { _RecordId_: "uuid-1", name: "SolarSystem" },
      { _RecordId_: "uuid-2", name: "Planet" },
    ],
  });

  it("builds the UUID to name lookup", () => {
    expect(buildTypeNames([typesRecord]).get("uuid-1")).toBe("SolarSystem");
  });

  it("resolves a type reference that is otherwise a bare UUID", () => {
    const names = buildTypeNames([typesRecord]);
    const location = parseLocation(
      rec("StarMapObject", "Hurston", {
        name: "Hurston",
        type: { _RecordName_: "StarMapObjectType.uuid-2", _RecordId_: "uuid-2" },
      }),
      empty,
      names,
    );
    expect(location.type).toBe("Planet");
  });

  it("keeps the UUID when the lookup has no entry", () => {
    const location = parseLocation(
      rec("StarMapObject", "X", { type: { _RecordName_: "StarMapObjectType.uuid-9" } }),
      empty,
      new Map(),
    );
    expect(location.type).toBe("uuid-9");
  });

  it("separates hidden objects from visible ones", () => {
    const result = starmapDomain.parse({
      records: [
        rec("StarMapObject", "Visible", { name: "Visible" }),
        rec("StarMapObject", "Hidden", { name: "Hidden", hideInStarmap: true }),
      ],
      localization: empty,
    }) as Record<string, never>;
    expect(result.summary).toMatchObject({ locations: 2, visible: 1, hidden: 1 });
  });
});

describe("reputation", () => {
  it("groups ranks by organisation, dropping the rank number", () => {
    expect(organisationOf("ReputationStanding_Guild_04")).toBe("Guild");
    expect(organisationOf("ReputationStanding_Affinity_Ally_02")).toBe("Affinity_Ally");
  });

  it("orders each organisation's ranks by threshold", () => {
    const result = reputationDomain.parse({
      records: [
        rec("SReputationStandingParams", "ReputationStanding_Guild_02", { minReputation: 2051 }),
        rec("SReputationStandingParams", "ReputationStanding_Guild_01", { minReputation: 0 }),
      ],
      localization: empty,
    }) as Record<string, never>;
    const byOrg = result.byOrganisation as unknown as Record<string, { minReputation: number }[]>;
    expect(byOrg.Guild.map((s) => s.minReputation)).toEqual([0, 2051]);
  });
});

describe("ships", () => {
  const localization = parseGlobalIni("vehicle_Name_Gladius=Aegis Gladius");

  it("reads identity fields off VehicleComponentParams", () => {
    const vehicle = parseVehicle(
      rec("EntityClassDefinition", "AEGS_Gladius", {
        Components: [
          {
            _Type_: "VehicleComponentParams",
            vehicleName: "@vehicle_Name_Gladius",
            vehicleCareer: "@vehicle_focus_combat",
            vehicleRole: "@vehicle_class_lightfighter",
            crewSize: 1,
            movementClass: "Spaceship",
            manufacturer: "file://./x/scitemmanufacturer.aegs.json",
            maxBoundingBoxSize: { _Type_: "Vec3", x: 17.5, y: 21, z: 5.5 },
          },
        ],
      }),
      localization,
    );

    expect(vehicle).toMatchObject({
      name: "Aegis Gladius",
      manufacturer: "aegs",
      crewSize: 1,
      movementClass: "Spaceship",
    });
    expect(vehicle?.boundingBox).toEqual({ x: 17.5, y: 21, z: 5.5 });
  });

  it("falls back to the key tail when career has no localized string", () => {
    const vehicle = parseVehicle(
      rec("EntityClassDefinition", "X", {
        Components: [
          { _Type_: "VehicleComponentParams", vehicleCareer: "@vehicle_focus_combat" },
        ],
      }),
      empty,
    );
    expect(vehicle?.career).toBe("combat");
  });

  it("ignores entities that are not vehicles", () => {
    expect(parseVehicle(rec("EntityClassDefinition", "X", { Components: [] }), empty))
      .toBeUndefined();
  });
});
