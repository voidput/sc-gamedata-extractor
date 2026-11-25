import { describe, it, expect } from "vitest";
import { fireDomain, parseExplosion, parseFireItem, parseFireRules } from "./fire.js";
import type { GameRecord } from "../records.js";

const empty = new Map<string, string>();
const rec = (type: string, shortName: string, value: Record<string, unknown>): GameRecord => ({
  name: `${type}.${shortName}`,
  shortName,
  id: `id-${shortName}`,
  type,
  recordPath: `libs/foundry/records/entities/scitem/ships/${shortName.toLowerCase()}`,
  value: { _Type_: type, ...value },
});

const rules = rec("FireHazardGlobalParams", "Global", {
  update: { fixedFPS: 1 },
  ignition: {
    globalIgnitionChanceMultiplier: 1,
    explosionChanceMultiplier: [0, 0, 0, 0.25, 0, 0],
    projectileChanceMultiplier: [0, 0, 0, 0, 0, 0],
  },
  propagation: { oxygenPressureRange: { minimum: 0.1, maximum: 0.15 }, standardConvectionTemperature: 900, maximumConvectionDistance: 10 },
  roomConnector: { igniteThreshold: 200 },
});

const shield = (extra: Record<string, unknown>[] = []) =>
  rec("EntityClassDefinition", "SHLD_Test", {
    Components: [
      { _Type_: "SAttachableComponentParams", AttachDef: { Type: "Shield", Size: 4 } },
      {
        _Type_: "SHealthComponentParams",
        Health: 43000,
        DamageResistances: { PhysicalResistance: { Multiplier: 0.9 }, ThermalResistance: { Multiplier: 0.1 } },
      },
      {
        _Type_: "SInteractionStateMachineParams",
        StateTypes: [
          {
            StateModifiers: [{ stateRanges: [{ state: "_PointsTo_:S1", min: 0, max: 0.01 }] }],
            States: [
              {
                _Pointer_: "S1",
                StateName: "Destroyed",
                EnterEventDispatchers: [{ _Type_: "FireHazardIgniteEvent", ignitionChance: 20, ignitionStrength: 100, ignitionRadius: 2 }],
              },
            ],
          },
        ],
      },
      {
        _Type_: "EntityComponentMisfireParams",
        misfires: [{ _Type_: "SFireMisfireEffect", _Pointer_: "F" }, { _Type_: "SOtherMisfire", _Pointer_: "O" }],
        misfireLevels: [
          { misfireProbabilityArray: [{ weight: 1, misfireArray: ["_PointsTo_:O"] }] },
          { misfireProbabilityArray: [{ weight: 1, misfireArray: ["_PointsTo_:F"] }, { weight: 3, misfireArray: ["_PointsTo_:O"] }] },
        ],
        triggerConditions: [{ heat: 0.8 }],
        minWindowLength: 45,
        maxWindowLength: 600,
      },
      {
        _Type_: "SEntityPhysicsControllerParams",
        PhysType: {
          temperature: {
            enable: true,
            misfireTemperatureRange: { minimum: 450, maximum: 453 },
            coolingEqualizationParams: { coolingEqualizationRate: 3.75, temperatureDifference: 400 },
            itemResourceParams: {
              enableOverheat: true,
              overheatTemperature: 453,
              minCoolingTemperature: 303,
              stateHeatGenerationValues: [{ baselineTemperatureChange: 0.45 }, { baselineTemperatureChange: 0.3 }],
            },
          },
        },
      },
      ...extra,
    ],
  });

describe("fire rules", () => {
  it("maps per-damage-type arrays in DamageInfo order", () => {
    const r = parseFireRules(rules);
    expect(r.explosionChanceMultiplier).toEqual({ physical: 0, energy: 0, distortion: 0, thermal: 0.25, biochemical: 0, stun: 0 });
    expect(r.oxygenPressureRange).toEqual({ min: 0.1, max: 0.15 });
    expect(r.standardConvectionTemperature).toBe(900);
    expect(r.roomConnectorIgniteThreshold).toBe(200);
  });
});

describe("fire items", () => {
  it("reads the ignite roll, fire misfire share and heat", () => {
    const i = parseFireItem(shield(), empty)!;
    expect(i.ignite).toEqual([{ state: "Destroyed", belowHealthRatio: 0.01, chance: 0.2, strength: 100, radius: 2 }]);
    expect(i.misfire?.fireByLevel).toEqual([0, 0.25]);
    expect(i.misfire?.trigger).toEqual({ heat: 0.8 });
    expect(i.misfireTemperature).toEqual({ min: 450, max: 453 });
    // per-state heat values are alternatives: the max, not the sum
    expect(i.temperature).toMatchObject({ heatGen: 0.45, minCoolingTemperature: 303, enabled: true });
    expect(i.damageMultipliers).toEqual({ physical: 0.9, thermal: 0.1 });
    expect(i.health).toBe(43000);
  });

  it("keeps a thermal death explosion without an ignite roll (power plants)", () => {
    const pp = rec("EntityClassDefinition", "POWR_Test", {
      Components: [
        {
          _Type_: "SHealthComponentParams",
          Health: 1000,
          DeathExplosionParams: { damage: { DamageEnergy: 100, DamageThermal: 100, DamagePhysical: 0 }, minRadius: 3, maxRadius: 5 },
        },
      ],
    });
    const i = parseFireItem(pp, empty)!;
    expect(i.ignite).toBeUndefined();
    expect(i.deathExplosion).toEqual({ damage: { energy: 100, thermal: 100 }, minRadius: 3, maxRadius: 5 });
  });

  it("skips items with no fire path", () => {
    const seat = rec("EntityClassDefinition", "Seat", {
      Components: [{ _Type_: "SHealthComponentParams", Health: 10, DeathExplosionParams: { damage: { DamagePhysical: 50 } } }],
    });
    expect(parseFireItem(seat, empty)).toBeUndefined();
    expect(parseExplosion({ damage: { DamagePhysical: 0 } })).toBeUndefined();
  });

  it("domain: rules plus listed items and summary", () => {
    const out = fireDomain.parse({ records: [rules, shield()], localization: empty }) as Record<string, never>;
    expect((out.rules as { fixedFPS: number }).fixedFPS).toBe(1);
    expect(out.summary).toMatchObject({ rules: 1, items: 1, withIgniteRoll: 1, withFireMisfire: 1, withThermalDeathExplosion: 0 });
  });
});
