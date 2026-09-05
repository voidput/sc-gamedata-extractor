import { describe, it, expect } from "vitest";
import {
  collectLaserModifiers,
  miningDomain,
  parseMineableElement,
  parseMiningModule,
  parseModifier,
} from "./mining.js";
import type { GameRecord } from "../records.js";

function record(partial: Partial<GameRecord> & { value: Record<string, unknown> }): GameRecord {
  return {
    name: "MineableElement.Test",
    shortName: "Test",
    id: "id-1",
    type: "MineableElement",
    recordPath: "libs/foundry/records/mining/mineableelements/test",
    ...partial,
  };
}

/** Shaped exactly as StarBreaker emits it. */
const quantainium = record({
  name: "MineableElement.Quantainium_Raw",
  shortName: "Quantainium_Raw",
  id: "fc62043c-3f29-4fff-b754-2301762723f7",
  value: {
    _Type_: "MineableElement",
    resourceType: {
      _RecordName_: "ResourceType.Raw_Quantainium",
      _RecordId_: "501d9b2e-136c-483b-a4d9-282fe6a5d37a",
    },
    elementInstability: 1000.0,
    elementResistance: 0.95,
    elementOptimalWindowMidpoint: 0.5,
    elementOptimalWindowMidpointRandomness: 0.2,
    elementOptimalWindowThinness: 2.3,
    elementExplosionMultiplier: 260.0,
    elementClusterFactor: 0.1,
  },
});

describe("parseMineableElement", () => {
  it("reads the physics fields off a real record shape", () => {
    const element = parseMineableElement(quantainium);
    expect(element).toMatchObject({
      instability: 1000,
      resistance: 0.95,
      explosionMultiplier: 260,
      clusterFactor: 0.1,
    });
  });

  it("prefers the resourceType name over the internal record name", () => {
    // The record is "Quantainium_Raw"; players know it as "Raw_Quantainium".
    expect(parseMineableElement(quantainium).name).toBe("Raw_Quantainium");
  });

  it("falls back to the record name when there is no resourceType", () => {
    const orphan = record({ shortName: "Orphan_Raw", value: { _Type_: "MineableElement" } });
    expect(parseMineableElement(orphan).name).toBe("Orphan_Raw");
  });
});

describe("parseModifier", () => {
  it("unwraps a modifier object and keeps how it applies", () => {
    expect(
      parseModifier({ _Type_: "FloatModifierMultiplicative", showInUI: true, value: -10 }),
    ).toEqual({ value: -10, kind: "multiplicative", showInUI: true });
  });

  it("distinguishes additive from multiplicative", () => {
    expect(parseModifier({ _Type_: "FloatModifierAdditive", value: 3 })?.kind).toBe("additive");
  });

  it("rejects a null modifier, which is how unset stats are stored", () => {
    expect(parseModifier(null)).toBeUndefined();
    expect(parseModifier({ _Type_: "FloatModifierMultiplicative", value: null })).toBeUndefined();
  });

  it("does not read a bare number, because the game never stores one", () => {
    // Treating these as plain numbers is why the first parser found nothing.
    expect(parseModifier(15.5)).toBeUndefined();
  });
});

describe("collectLaserModifiers", () => {
  const nested = {
    Components: [
      { _Type_: "SomethingElse" },
      {
        _Type_: "EntityComponentAttachableModifierParams",
        modifiers: [
          { _Type_: "ItemWeaponModifiersParams" },
          {
            MiningLaserModifier: {
              _Type_: "MiningLaserModifiers",
              laserInstability: null,
              resistanceModifier: {
                _Type_: "FloatModifierMultiplicative",
                showInUI: true,
                value: 15.5,
              },
              shatterdamageModifier: {
                _Type_: "FloatModifierMultiplicative",
                showInUI: true,
                value: -30,
              },
              isOptimalRateGood: true,
            },
          },
        ],
      },
    ],
  };

  it("finds modifiers nested inside component arrays", () => {
    const found = collectLaserModifiers(nested);
    expect(found).toHaveLength(1);
    expect(found[0].resistanceModifier).toEqual({
      value: 15.5,
      kind: "multiplicative",
      showInUI: true,
    });
  });

  it("reads shatterdamageModifier despite the game's lowercase d", () => {
    expect(collectLaserModifiers(nested)[0].shatterDamageModifier?.value).toBe(-30);
  });

  it("skips an all-null struct, which means the record modifies nothing", () => {
    const inert = {
      MiningLaserModifier: {
        _Type_: "MiningLaserModifiers",
        laserInstability: null,
        resistanceModifier: null,
        isOptimalRateGood: true,
      },
    };
    expect(collectLaserModifiers(inert)).toHaveLength(0);
  });

  it("returns nothing for a record with no modifiers at all", () => {
    expect(collectLaserModifiers({ Components: [{ _Type_: "Nothing" }] })).toHaveLength(0);
    expect(parseMiningModule(record({ type: "EntityClassDefinition", value: {} }))).toBeUndefined();
  });
});

describe("miningDomain.parse", () => {
  it("splits elements from modules and builds the signature lookup", () => {
    const module = record({
      name: "EntityClassDefinition.Mining_Modules_Active_Brandt",
      shortName: "Mining_Modules_Active_Brandt",
      type: "EntityClassDefinition",
      value: {
        Components: [
          {
            modifiers: [
              {
                MiningLaserModifier: {
                  _Type_: "MiningLaserModifiers",
                  resistanceModifier: { _Type_: "FloatModifierMultiplicative", value: 15.5 },
                },
              },
            ],
          },
        ],
      },
    });

    const result = miningDomain.parse({ records: [quantainium, module], localization: new Map() }) as Record<string, never>;
    expect(result.summary).toEqual({ mineableElements: 1, miningModules: 1 });

    const signatures = result.elementSignatures as unknown as Record<string, unknown>;
    expect(signatures.Raw_Quantainium).toEqual({ resistance: 0.95, instability: 1000 });
  });

  it("produces empty collections rather than throwing on no input", () => {
    const result = miningDomain.parse({ records: [], localization: new Map() }) as Record<string, never>;
    expect(result.summary).toEqual({ mineableElements: 0, miningModules: 0 });
  });
});
