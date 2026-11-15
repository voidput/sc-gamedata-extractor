import { describe, it, expect } from "vitest";
import { ammoDomain, findDamage, parseAmmo } from "./ammo.js";
import { containersDomain, parseContainer } from "./containers.js";
import { buildAmenityNames, buildLocations, buildTypeNames, parseLocation, starmapDomain } from "./starmap.js";
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

  it("gives stations their zones and leaves a planet's surface zones off it", () => {
    const placed = (id: string) => ({ starMapRecord: id, position: [0, 0, 0] as [number, number, number], depth: 0, rotation: [1, 0, 0, 0] as [number, number, number, number] });
    const typeRecord = rec("StarMapObjectTypes", "StarMapObjectTypes", {
      types: [
        { _RecordId_: "t-planet", name: "Planet" },
        { _RecordId_: "t-station", name: "Manmade" },
      ],
    });
    const station = rec("StarMapObject", "Station", { type: { _RecordId_: "t-station", _RecordName_: "StarMapObjectType.t-station" } });
    const planet = rec("StarMapObject", "World", { type: { _RecordId_: "t-planet", _RecordName_: "StarMapObjectType.t-planet" } });
    const zone = { ...rec("EntityClassDefinition", "MonitoredZone", { Components: [{ _Type_: "MonitoredZoneParams", radiusKm: 30000 }] }) };
    const locations = buildLocations(
      [typeRecord, station, planet, zone],
      empty,
      new Map([
        [station.id, placed(station.id)],
        [planet.id, placed(planet.id)],
      ]),
      new Map([
        [station.id, { armisticeRadius: 12000, monitored: true as const }],
        [planet.id, { armisticeRadius: 12000, monitored: true as const }],
      ]),
    );
    const byName = new Map(locations.map((l) => [l.recordName, l]));
    expect(byName.get(station.name)).toMatchObject({ armisticeRadius: 12000, monitoredRadius: 30_000_000 });
    expect(byName.get(planet.name)?.armisticeRadius).toBeUndefined();
    expect(byName.get(planet.name)?.monitoredRadius).toBeUndefined();
    expect(byName.get(station.name)?.jammerRadius).toBeUndefined();
  });

  it("keeps a surface place's offset from its body and the body's rotation period", () => {
    const rotation = [1, 0, 0, 0] as [number, number, number, number];
    const typeRecord = rec("StarMapObjectTypes", "StarMapObjectTypes", {
      types: [
        { _RecordId_: "t-planet", name: "Planet" },
        { _RecordId_: "t-city", name: "LandingZone" },
      ],
    });
    const station = rec("StarMapObject", "Harbor", {});
    const moon = rec("StarMapObject", "Luna", {});
    const planet = rec("StarMapObject", "World", {
      type: { _RecordId_: "t-planet", _RecordName_: "StarMapObjectType.t-planet" },
      quantumTravelData: { obstructionRadius: 1_008_900 },
    });
    const city = rec("StarMapObject", "City", { type: { _RecordId_: "t-city", _RecordName_: "StarMapObjectType.t-city" } });
    const centre: [number, number, number] = [5_000_000, 0, 0];
    const locations = buildLocations(
      [typeRecord, planet, city, station, moon],
      empty,
      new Map([
        [planet.id, { starMapRecord: planet.id, position: centre, depth: 0, rotation }],
        [station.id, { starMapRecord: station.id, position: [5_000_000, 0, 1_149_000] as [number, number, number], depth: 2, rotation, parentRecord: planet.id, parentPosition: centre }],
        [moon.id, { starMapRecord: moon.id, position: [5_000_000, 90_000_000, 0] as [number, number, number], depth: 2, rotation, parentRecord: planet.id, parentPosition: centre }],
        [
          city.id,
          { starMapRecord: city.id, position: [5_000_000 - 328_989, -752_435, 572_120] as [number, number, number], depth: 2, rotation, parentRecord: planet.id, parentPosition: centre },
        ],
      ]),
      new Map(),
      new Map([[planet.id, { rotationHours: 2.48, gridRadius: 2_000_000 }]]),
    );
    const byName = new Map(locations.map((l) => [l.recordName, l]));
    expect(byName.get(planet.name)).toMatchObject({ position: { x: 5_000_000, y: 0, z: 0 }, rotationHours: 2.48, gridRadius: 2_000_000 });
    expect(byName.get(planet.name)?.local).toBeUndefined();
    // On the surface: no place in space, but where it sits on the body, and no period of its own.
    expect(byName.get(city.name)?.position).toBeUndefined();
    expect(byName.get(city.name)).toMatchObject({ local: { x: -328_989, y: -752_435, z: 572_120 }, turnsWith: planet.name });
    expect(byName.get(city.name)?.rotationHours).toBeUndefined();
    // In low orbit inside the rotating zone: a place in space that turns with the body.
    expect(byName.get(station.name)).toMatchObject({ position: { x: 5_000_000, y: 0, z: 1_149_000 }, local: { x: 0, y: 0, z: 1_149_000 }, turnsWith: planet.name });
    // Beyond the zone: a place in space that holds still.
    expect(byName.get(moon.name)?.position).toBeDefined();
    expect(byName.get(moon.name)?.local).toBeUndefined();
  });

  it("gives an outlaw station its jammer dead zone", () => {
    const placed = (id: string) => ({ starMapRecord: id, position: [0, 0, 0] as [number, number, number], depth: 0, rotation: [1, 0, 0, 0] as [number, number, number, number] });
    const typeRecord = rec("StarMapObjectTypes", "StarMapObjectTypes", { types: [{ _RecordId_: "t-station", name: "Manmade" }] });
    const type = { _RecordId_: "t-station", _RecordName_: "StarMapObjectType.t-station" };
    const own = rec("StarMapObject", "OutlawA", { type });
    const inherited = rec("StarMapObject", "OutlawB", { type });
    const jammer = rec("EntityClassDefinition", "MonitorZoneJammer", { Components: [{ _Type_: "MonitoredZoneParams", radiusKm: 40, strength: -10 }] });
    const locations = buildLocations(
      [typeRecord, own, inherited, jammer],
      empty,
      new Map([
        [own.id, placed(own.id)],
        [inherited.id, placed(inherited.id)],
      ]),
      new Map([
        [own.id, { jammer: { radius: 35_000 } }],
        [inherited.id, { jammer: {} }],
      ]),
    );
    const byName = new Map(locations.map((l) => [l.recordName, l]));
    expect(byName.get(own.name)?.jammerRadius).toBe(35_000);
    expect(byName.get(inherited.name)?.jammerRadius).toBe(40_000);
  });

  it("names amenities instead of leaving their ids", () => {
    const amenityTypes = buildAmenityNames([
      rec("StarMapAmenityTypes", "StarMapAmenityTypes", { amenityTypes: [{ _RecordId_: "a-1", name: "Refinery" }] }),
    ]);
    const location = parseLocation(
      rec("StarMapObject", "Station", {
        amenities: [{ _RecordName_: "StarMapAmenityTypeEntry.a-1" }, { _RecordName_: "StarMapAmenityTypeEntry.a-9" }],
      }),
      empty,
      new Map(),
      new Map(),
      amenityTypes,
    );
    expect(location.amenities).toEqual(["Refinery", "a-9"]);
  });

  it("carries a record's quantum travel geometry", () => {
    const location = parseLocation(
      rec("StarMapObject", "Hurston", {
        quantumTravelData: { obstructionRadius: 1008900, arrivalRadius: 1050000, adoptionRadius: 128354900 },
      }),
      empty,
    );
    expect(location.quantum).toMatchObject({ obstructionRadius: 1008900, arrivalRadius: 1050000 });
    expect(parseLocation(rec("StarMapObject", "Bare", {}), empty).quantum).toBeUndefined();
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
