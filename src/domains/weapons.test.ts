import { describe, it, expect } from "vitest";
import {
  defaultMagazineName,
  dominantDamageType,
  indexAmmo,
  parseFireMode,
  parseMissile,
  parseWeapon,
  sequenceFireRate,
  weaponsDomain,
  type WeaponContext,
  sustain,
} from "./weapons.js";
import { parseGlobalIni } from "../localization.js";
import type { GameRecord } from "../records.js";

const localization = parseGlobalIni(
  [
    "item_NameBEHR_BallisticGatling_S5=AD5B Ballistic Gatling",
    "item_Namebehr_rifle_ballistic_01=P4-AR Rifle",
    "item_NameTALN_Valkyrie=Valkyrie",
  ].join("\n"),
);

function entity(shortName: string, recordPath: string, components: Record<string, unknown>[]): GameRecord {
  return {
    name: `EntityClassDefinition.${shortName}`,
    shortName,
    id: `id-${shortName}`,
    type: "EntityClassDefinition",
    recordPath,
    value: { _Type_: "EntityClassDefinition", Components: components },
  };
}

function ammoRecord(shortName: string, physical: number, speed = 1251, lifetime = 3.2): GameRecord {
  return {
    name: `AmmoParams.${shortName}`,
    shortName,
    id: `id-${shortName}`,
    type: "AmmoParams",
    recordPath: `libs/foundry/records/ammoparams/vehicle/${shortName.toLowerCase()}`,
    value: {
      _Type_: "AmmoParams",
      speed,
      lifetime,
      projectileParams: {
        damage: { _Type_: "DamageInfo", DamagePhysical: physical, DamageEnergy: 0 },
        penetrationParams: { basePenetrationDistance: 1 },
      },
    },
  };
}

const attach = (nameKey: string, Type = "WeaponGun", Size = 5) => ({
  _Type_: "SAttachableComponentParams",
  AttachDef: {
    Type,
    SubType: "Gun",
    Size,
    Grade: 1,
    Manufacturer: "file://./../../libs/foundry/records/scitemmanufacturer/behr.json",
    Localization: { Name: nameKey },
  },
});

const resources = (power: number, em: number) => ({
  _Type_: "ItemResourceComponentParams",
  states: [
    {
      name: "Online",
      deltas: [
        {
          consumption: {
            resource: "Power",
            resourceAmountPerSecond: { standardResourceUnits: power },
          },
        },
      ],
      signatureParams: { EMSignature: { nominalSignature: em }, IRSignature: { nominalSignature: 0 } },
    },
  ],
});

const rapid = (fireRate: number, pelletCount = 1) => ({
  _Type_: "SWeaponActionFireRapidParams",
  name: "Rapid",
  fireRate,
  heatPerShot: 0.57,
  launchParams: {
    pelletCount,
    damageMultiplier: 1,
    spreadParams: { min: 0.792, max: 0.792, attack: 0.008, decay: 0.01 },
  },
});

/** The S5 Behring gatling as the game ships it, checked against a loadout site. */
const gatling = entity("BEHR_BallisticGatling_S5", "libs/foundry/records/entities/scitem/ships/weapons/behr_ballisticgatling_s5", [
  attach("@item_NameBEHR_BallisticGatling_S5"),
  {
    _Type_: "SAmmoContainerComponentParams",
    maxAmmoCount: 5004,
    ammoParamsRecord: "file://./../../libs/foundry/records/ammoparams/vehicle/behr_ballisticgatling_s5_ammo.json",
  },
  { _Type_: "SCItemWeaponComponentParams", fireActions: [rapid(900)] },
  resources(0.1, 36),
  { _Type_: "SHealthComponentParams", Health: 1650 },
  { _Type_: "SEntityPhysicsControllerParams", PhysType: { Mass: 938 } },
]);

function context(records: GameRecord[]): WeaponContext {
  const entities = new Map<string, GameRecord>();
  for (const r of records) if (r.type === "EntityClassDefinition") entities.set(r.shortName.toLowerCase(), r);
  return { localization, ammoIndex: indexAmmo(records), entities };
}

describe("ship weapon baseline", () => {
  const ammo = ammoRecord("BEHR_BallisticGatling_S5_AMMO", 102.5);
  const weapon = parseWeapon(gatling, "ship", context([gatling, ammo]))!;

  it("reproduces every loadout-site column from the raw records", () => {
    expect(weapon).toMatchObject({
      name: "AD5B Ballistic Gatling",
      manufacturer: "BEHR",
      damageType: "physical",
      dps: 1537.5,
      alpha: 102.5,
      fireRate: 900,
      range: 4003,
      speed: 1251,
      spread: 0.792,
      penetration: 1,
      power: 0.1,
      em: 36,
      ammoCapacity: 5004,
      health: 1650,
      mass: 938,
    });
    expect(weapon.isTemplate).toBeUndefined();
  });

  it("resolves the ammo through the embedded container, whatever the file is called", () => {
    expect(weapon.ammo?.recordName).toBe("AmmoParams.BEHR_BallisticGatling_S5_AMMO");
    expect(weapon.ammo?.magazine).toBeUndefined();
  });

  it("still describes a weapon whose ammo is missing, without a DPS", () => {
    const lonely = parseWeapon(gatling, "ship", context([gatling]))!;
    expect(lonely.dps).toBeUndefined();
    expect(lonely.ammo).toBeUndefined();
    expect(lonely.modes[0]).toMatchObject({ kind: "rapid", fireRate: 900 });
  });

  it("multiplies a scattergun's alpha by its pellets but keeps ammo per projectile", () => {
    const scatter = entity("Scatter", "libs/foundry/records/entities/scitem/ships/weapons/scatter", [
      attach("@none"),
      { _Type_: "SAmmoContainerComponentParams", ammoParamsRecord: "file://x/scatter_ammo.json" },
      { _Type_: "SCItemWeaponComponentParams", fireActions: [rapid(60, 8)] },
    ]);
    const w = parseWeapon(scatter, "ship", context([scatter, ammoRecord("scatter_ammo", 90)]))!;
    expect(w.alpha).toBe(720);
    expect(w.dps).toBe(720);
    expect(w.ammo?.damagePerShot).toBe(90);
    expect(w.isTemplate).toBe(true);
  });
});

describe("fire modes", () => {
  const ammo = { recordName: "x", name: "x", id: "x", damageTotal: 10 };

  it("takes a sequence's cadence from its entries, not the nested action", () => {
    // Two barrels at 500 each, sequenced at 900: the gun fires at 900.
    const mode = parseFireMode(
      {
        _Type_: "SWeaponActionSequenceParams",
        name: "",
        sequenceEntries: [
          { delay: 900, unit: "RPM", repetitions: 1, weaponAction: { _Type_: "SWeaponActionFireSingleParams", name: "Single", fireRate: 500, launchParams: { pelletCount: 1 } } },
          { delay: 900, unit: "RPM", repetitions: 1, weaponAction: { _Type_: "SWeaponActionFireSingleParams", name: "Single", fireRate: 500, launchParams: { pelletCount: 1 } } },
        ],
      },
      ammo,
    )!;
    expect(mode).toMatchObject({ name: "Single", kind: "sequence", fireRate: 900, alpha: 10, dps: 150 });
  });

  it("averages mixed RPM and second delays over the whole sequence", () => {
    // A shot at 60 RPM (1 s) followed by a 2 s bolt cycle: 2 shots in 3 s.
    expect(
      sequenceFireRate([
        { delay: 60, unit: "RPM", repetitions: 1 },
        { delay: 2, unit: "Seconds", repetitions: 1 },
      ]),
    ).toBe(40);
    expect(sequenceFireRate([])).toBeUndefined();
  });

  it("derives a burst's sustained rate including its cooldown", () => {
    // 3 shots at 900 RPM take 0.2 s, plus 0.3 s cooldown: 3 shots per 0.5 s.
    const mode = parseFireMode(
      { _Type_: "SWeaponActionFireBurstParams", name: "Burst", fireRate: 900, shotCount: 3, cooldownTime: 0.3, launchParams: { pelletCount: 1 } },
      ammo,
    )!;
    expect(mode).toMatchObject({ kind: "burst", fireRate: 900, sustainedFireRate: 360, dps: 60 });
  });

  it("reads a charged mode through its inner action", () => {
    const mode = parseFireMode(
      {
        _Type_: "SWeaponActionFireChargedParams",
        name: "Charge",
        chargeTime: 10,
        maxChargeModifier: { damageMultiplier: 2 },
        weaponAction: { _Type_: "SWeaponActionFireSingleParams", fireRate: 6, launchParams: { pelletCount: 1 } },
      },
      ammo,
    )!;
    expect(mode).toMatchObject({ kind: "charged", fireRate: 6, chargeTime: 10, chargedDamageMultiplier: 2, dps: 1 });
  });

  it("carries a beam's damage per second and falloff without any ammo", () => {
    const mode = parseFireMode(
      {
        _Type_: "SWeaponActionFireBeamParams",
        name: "Beam",
        damagePerSecond: { _Type_: "DamageInfo", DamageEnergy: 15000 },
        fullDamageRange: 4000,
        zeroDamageRange: 5000,
      },
      undefined,
    )!;
    expect(mode).toMatchObject({ kind: "beam", dps: 15000, fullDamageRange: 4000, zeroDamageRange: 5000 });
  });

  it("keeps non-damage actions in the list rather than dropping them", () => {
    expect(parseFireMode({ _Type_: "SWeaponActionFireTractorBeamParams", name: "Tractor" }, ammo)).toEqual({
      name: "Tractor",
      kind: "other",
    });
  });
});

describe("fps weapon baseline", () => {
  const rifle = entity("behr_rifle_ballistic_01", "libs/foundry/records/entities/scitem/weapons/fps_weapons/behr_rifle_ballistic_01", [
    attach("@item_Namebehr_rifle_ballistic_01", "WeaponPersonal", 2),
    { _Type_: "SCItemWeaponComponentParams", fireActions: [rapid(810), null] },
    {
      _Type_: "SEntityComponentDefaultLoadoutParams",
      loadout: {
        entries: [
          { itemPortName: "optics_attach", entityClassName: "" },
          { itemPortName: "magazine_attach", entityClassName: "behr_rifle_ballistic_01_mag" },
        ],
      },
    },
  ]);
  const magazine = entity("behr_rifle_ballistic_01_mag", "libs/foundry/records/entities/scitem/weapons/magazines/behr_rifle_ballistic_01_mag", [
    {
      _Type_: "SAmmoContainerComponentParams",
      maxAmmoCount: 40,
      ammoParamsRecord: "file://./../../libs/foundry/records/ammoparams/fps/behr_rifle_ballistic_01_ammo_5mm.json",
    },
  ]);
  const ammo = ammoRecord("behr_rifle_ballistic_01_ammo_5mm", 12, 550, 2);

  it("names the default magazine from the loadout", () => {
    expect(defaultMagazineName(rifle)).toBe("behr_rifle_ballistic_01_mag");
    expect(defaultMagazineName(gatling)).toBeUndefined();
  });

  it("joins weapon -> magazine -> ammo", () => {
    const w = parseWeapon(rifle, "fps", context([rifle, magazine, ammo]))!;
    expect(w).toMatchObject({ class: "fps", dps: 162, alpha: 12, fireRate: 810, range: 1100, ammoCapacity: 40 });
    expect(w.ammo?.magazine).toBe("behr_rifle_ballistic_01_mag");
    // A null fire action in the list is skipped, not fatal.
    expect(w.modes).toHaveLength(1);
  });

  it("marks store props, which share a gun's name but fire nothing", () => {
    const prop = entity("behr_rifle_ballistic_01_store01_prop", "libs/foundry/records/entities/scitem/weapons/fps_weapons/x", [
      attach("@item_Namebehr_rifle_ballistic_01", "WeaponPersonal", 2),
      { _Type_: "SCItemWeaponComponentParams", fireActions: [] },
    ]);
    const w = parseWeapon(prop, "fps", context([prop]))!;
    expect(w.isTemplate).toBe(true);
    expect(w.name).toBe("behr_rifle_ballistic_01_store01_prop");
  });
});

describe("missiles", () => {
  const missile = entity("TALN_Valkyrie", "libs/foundry/records/entities/scitem/ships/weapons/missiles/x", [
    attach("@item_NameTALN_Valkyrie", "Missile", 5),
    {
      _Type_: "SCItemMissileParams",
      maxLifetime: 60,
      explosionParams: { maxRadius: 32, damage: { _Type_: "DamageInfo", DamagePhysical: 12000 } },
      targetingParams: { trackingSignalType: "Infrared", lockTime: 1, lockRangeMin: 2000, lockRangeMax: 30000, lockingAngle: 20 },
      GCSParams: { linearSpeed: 560, boostPhaseDuration: 2 },
    },
    { _Type_: "SHealthComponentParams", Health: 300 },
  ]);

  it("reads damage, lock and flight numbers", () => {
    expect(parseMissile(missile, localization)).toMatchObject({
      name: "Valkyrie",
      damageTotal: 12000,
      damageType: "physical",
      trackingSignal: "Infrared",
      lockTime: 1,
      lockRangeMax: 30000,
      speed: 560,
      explosionRadius: 32,
      health: 300,
    });
    expect(parseMissile(gatling, localization)).toBeUndefined();
  });

  it("labels mixed damage by its largest component", () => {
    expect(dominantDamageType({ physical: 100, energy: 200 })).toBe("energy");
    expect(dominantDamageType(undefined)).toBeUndefined();
  });
});

describe("weaponsDomain", () => {
  it("splits ship guns, missiles and FPS guns by where they live, and joins the ammo", () => {
    const missile = entity("TALN_Valkyrie", "libs/foundry/records/entities/scitem/ships/weapons/missiles/x", [
      attach("@item_NameTALN_Valkyrie", "Missile", 5),
      { _Type_: "SCItemMissileParams", explosionParams: { damage: { _Type_: "DamageInfo", DamagePhysical: 1 } } },
    ]);
    const result = weaponsDomain.parse({
      records: [gatling, ammoRecord("BEHR_BallisticGatling_S5_AMMO", 102.5), missile],
      localization,
    }) as { shipWeapons: unknown[]; fpsWeapons: unknown[]; missiles: unknown[]; summary: Record<string, number> };

    expect(result.summary).toEqual({
      shipWeapons: 1,
      shipWeaponsWithDps: 1,
      fpsWeapons: 0,
      fpsWeaponsWithDps: 0,
      missiles: 1,
      ammo: 1,
    });
    expect(result.shipWeapons[0]).toMatchObject({ dps: 1537.5 });
  });
});

describe("sustain", () => {
  it("cycles an energy gun through empty, cooldown and refill", () => {
    // CF-337 Panther: 75 shots, 750 RPM, 15 shots/s regen after 0.74 s.
    const result = sustain({
      alpha: 43.65,
      fireRate: 750,
      capacitor: { shots: 75, regenPerSecond: 15, regenCooldown: 0.74 },
    });
    expect(result.burst).toEqual({ shots: 75, seconds: 6, damage: 3273.75, limit: "capacitor" });
    // 3273.75 over 6 + 0.74 + 5 seconds.
    expect(result.sustainedDps).toBe(278.85);
  });

  it("limits a ballistic gun by heat when it overheats before the magazine empties", () => {
    const result = sustain({
      alpha: 100,
      fireRate: 600,
      magazine: 1000,
      heatPerShot: 2,
      overheatTemperature: 100,
      coolingPerSecond: 10,
      overheatFixTime: 2,
    });
    // Net heat 2 * 10 - 10 = 10/s, so 10 s to overheat, 100 shots, then 2 s locked out.
    expect(result.overheatSeconds).toBe(10);
    expect(result.burst).toEqual({ shots: 100, seconds: 10, damage: 10000, limit: "heat" });
    expect(result.sustainedDps).toBe(833.33);
  });

  it("falls back to the magazine when cooling keeps up", () => {
    // Deadbolt V: heat 6/shot at 100 RPM is 10/s against 18.3/s cooling.
    const result = sustain({
      alpha: 950,
      fireRate: 100,
      magazine: 369,
      heatPerShot: 6,
      overheatTemperature: 100,
      coolingPerSecond: 18.3,
    });
    expect(result.overheatSeconds).toBeUndefined();
    expect(result.burst).toMatchObject({ shots: 369, limit: "magazine" });
    expect(result.sustainedDps).toBe(1583.33);
  });

  it("returns nothing without alpha or rate", () => {
    expect(sustain({ fireRate: 600 })).toEqual({});
  });
});
