import { describe, it, expect } from "vitest";
import type { GameRecord } from "../records.js";
import { damageTypeMap, damageTypeRanges, distortionOf, durabilityOf, resourceProfile, volumeOf } from "./item-common.js";

const item = (components: Record<string, unknown>[]): GameRecord => ({
  name: "EntityClassDefinition.X",
  shortName: "X",
  id: "x",
  type: "EntityClassDefinition",
  recordPath: "libs/foundry/records/entities/scitem/ships/x/x",
  value: { Components: components },
});

describe("resourceProfile", () => {
  it("reads draw, output and signatures off the Online state in either unit spelling", () => {
    const record = item([
      {
        _Type_: "ItemResourceComponentParams",
        states: [
          { name: "Offline", deltas: [] },
          {
            name: "Online",
            deltas: [
              {
                consumption: { resource: "Power", resourceAmountPerSecond: { units: 3 } },
                generation: { resource: "Coolant", resourceAmountPerSecond: { standardResourceUnits: 34 } },
              },
              { consumption: { resource: "Coolant", resourceAmountPerSecond: { standardResourceUnits: 0 } } },
            ],
            signatureParams: {
              EMSignature: { nominalSignature: 1490 },
              IRSignature: { nominalSignature: 7260 },
            },
          },
        ],
      },
    ]);
    expect(resourceProfile(record)).toEqual({
      power: 3,
      em: 1490,
      ir: 7260,
      generation: { Coolant: 34 },
      consumption: { Power: 3, Coolant: 0 },
    });
  });

  it("is empty for an item outside the resource network", () => {
    expect(resourceProfile(item([]))).toEqual({});
  });
});

describe("durabilityOf", () => {
  it("rolls health, resistances, self-repair, wear and misfire into one block", () => {
    const record = item([
      {
        _Type_: "SHealthComponentParams",
        Health: 210,
        DamageResistances: {
          PhysicalResistance: { Multiplier: 0.9 },
          EnergyResistance: { Multiplier: 1 },
          ThermalResistance: { Multiplier: 0.1 },
        },
      },
      {
        _Type_: "ItemResourceComponentParams",
        states: [],
        selfRepair: { maxRepairCount: 1, timeToRepair: 40, healthRatio: 0.2 },
        repairRestoreRatio: 0.1,
      },
      {
        _Type_: "SDegradationParams",
        accumulators: [
          {
            MaxLifetimeHours: 0,
            DamageConversionRate: 0.5,
            HeatMultipliers: { OverheatTemperatureMultiplier: 2 },
          },
        ],
      },
      { _Type_: "EntityComponentMisfireParams", minWindowLength: 45, maxWindowLength: 600 },
    ]);
    expect(durabilityOf(record)).toEqual({
      health: 210,
      resistances: { physical: 0.9, thermal: 0.1 },
      selfRepair: { count: 1, seconds: 40, healthRatio: 0.2 },
      repairRestoreRatio: 0.1,
      wear: { maxLifetimeHours: 0, damageConversionRate: 0.5, overheatMultiplier: 2 },
      misfireWindow: { min: 45, max: 600 },
    });
  });

  it("is absent when the item carries none of it", () => {
    expect(durabilityOf(item([]))).toBeUndefined();
  });
});

describe("damage type tables", () => {
  it("keys a DamageX struct by lower-case type", () => {
    expect(damageTypeMap({ DamagePhysical: 0.75, DamageEnergy: 0.6, _Type_: "x" })).toEqual({
      physical: 0.75,
      energy: 0.6,
    });
  });

  it("keys a positional Min/Max array in the game's damage type order", () => {
    expect(damageTypeRanges([{ Min: 0, Max: 0.25 }, { Min: 0, Max: 0 }, { Min: 0.75, Max: 0.95 }])).toEqual({
      physical: { min: 0, max: 0.25 },
      energy: { min: 0, max: 0 },
      distortion: { min: 0.75, max: 0.95 },
    });
    expect(damageTypeRanges("nope")).toBeUndefined();
  });
});

describe("distortionOf", () => {
  it("reads the pool and works out the time to drain it", () => {
    // Endurance power plant, as the files have it.
    const record = item([
      { _Type_: "SDistortionParams", Maximum: 4050, DecayDelay: 1.5, DecayRate: 270, WarningRatio: 0.75, RecoveryRatio: 0 },
    ]);
    expect(distortionOf(record)).toEqual({
      max: 4050,
      decayDelay: 1.5,
      decayRate: 270,
      warningRatio: 0.75,
      recoveryRatio: 0,
      secondsToRecover: 16.5,
    });
  });

  it("only drains to the recovery point", () => {
    const record = item([{ _Type_: "SDistortionParams", Maximum: 1000, DecayDelay: 0, DecayRate: 100, RecoveryRatio: 0.5 }]);
    expect(distortionOf(record)?.secondsToRecover).toBe(5);
  });

  it("is absent on an item without a distortion pool", () => {
    expect(distortionOf(item([]))).toBeUndefined();
  });
});

describe("volumeOf", () => {
  it("reads the inventory volume in micro-SCU", () => {
    const record = item([
      { _Type_: "SAttachableComponentParams", AttachDef: { inventoryOccupancyVolume: { microSCU: 4200000 } } },
    ]);
    expect(volumeOf(record)).toBe(4200000);
  });
});
