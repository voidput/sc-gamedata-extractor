import { describe, it, expect } from "vitest";
import { blueprintsDomain, entityClassName, parseCost, timeSeconds } from "./blueprints.js";
import type { GameRecord } from "../records.js";

const localization = new Map<string, string>();

describe("timeSeconds", () => {
  it("totals the partitioned time struct", () => {
    expect(timeSeconds({ days: 0, hours: 0, minutes: 1, seconds: 35 })).toBe(95);
    expect(timeSeconds({ days: 1, hours: 2, minutes: 0, seconds: 0 })).toBe(93600);
  });

  it("returns undefined for an all-zero or missing time", () => {
    expect(timeSeconds({ days: 0, hours: 0, minutes: 0, seconds: 0 })).toBeUndefined();
    expect(timeSeconds(null)).toBeUndefined();
  });
});

describe("entityClassName", () => {
  it("takes the file name off a reference path", () => {
    expect(entityClassName("file://./../a/b/harvestable_mineral_1h_janalite.json")).toBe(
      "harvestable_mineral_1h_janalite",
    );
  });

  it("returns undefined for a non-string", () => {
    expect(entityClassName(null)).toBeUndefined();
  });
});

describe("parseCost", () => {
  it("reads a resource cost with its SCU quantity", () => {
    expect(
      parseCost(
        {
          _Type_: "CraftingCost_Resource",
          resource: { _RecordName_: "ResourceType.Taranite" },
          quantity: { standardCargoUnits: 0.06 },
          minQuality: 0,
        },
        localization,
      ),
    ).toEqual({ kind: "resource", resource: "Taranite", scu: 0.06, minQuality: undefined });
  });

  it("reads an item cost as a count", () => {
    expect(
      parseCost(
        { _Type_: "CraftingCost_Item", entityClass: "file://./x/janalite.json", quantity: 2 },
        localization,
      ),
    ).toMatchObject({ kind: "item", item: "janalite", quantity: 2 });
  });

  it("keeps the choice structure, since picking 2 of 5 is not needing all 5", () => {
    const cost = parseCost(
      {
        _Type_: "CraftingCost_Select",
        count: 2,
        nameInfo: { debugName: "ASPECTS", displayName: "@LOC_PLACEHOLDER" },
        options: [
          {
            _Type_: "CraftingCost_Resource",
            resource: { _RecordName_: "ResourceType.Aslarite" },
            quantity: { standardCargoUnits: 0.02 },
          },
          {
            _Type_: "CraftingCost_Resource",
            resource: { _RecordName_: "ResourceType.Ouratite" },
            quantity: { standardCargoUnits: 0.03 },
          },
        ],
      },
      localization,
    );

    expect(cost).toMatchObject({ kind: "choice", count: 2, label: "ASPECTS" });
    expect((cost as { options: unknown[] }).options).toHaveLength(2);
  });

  it("falls back to debugName when the display name is a placeholder", () => {
    const cost = parseCost(
      {
        _Type_: "CraftingCost_Select",
        count: 1,
        nameInfo: { debugName: "ARMOURED CARAPACE", displayName: "<= PLACEHOLDER =>" },
        options: [
          {
            _Type_: "CraftingCost_Resource",
            resource: { _RecordName_: "ResourceType.Ouratite" },
            quantity: { standardCargoUnits: 0.03 },
          },
        ],
      },
      localization,
    );
    expect((cost as { label: string }).label).toBe("ARMOURED CARAPACE");
  });

  it("drops a choice with no usable options", () => {
    expect(
      parseCost({ _Type_: "CraftingCost_Select", count: 1, options: [] }, localization),
    ).toBeUndefined();
  });

  it("ignores context nodes that are not costs", () => {
    expect(
      parseCost({ _Type_: "CraftingCostContext_ResultCompositionInclusion" }, localization),
    ).toBeUndefined();
  });
});

function blueprintRecord(shortName: string, tiers: unknown[]): GameRecord {
  return {
    name: `CraftingBlueprintRecord.${shortName}`,
    shortName,
    id: `id-${shortName}`,
    type: "CraftingBlueprintRecord",
    recordPath: `libs/foundry/records/crafting/blueprints/${shortName}`,
    value: {
      blueprint: {
        blueprintName: "@LOC_PLACEHOLDER",
        category: { _RecordName_: "BlueprintCategoryRecord.VehicleComponentS0" },
        processSpecificData: { entityClass: "file://./x/mining_module_stampede.json" },
        tiers,
      },
    },
  };
}

describe("blueprintsDomain.parse", () => {
  const tier = {
    recipe: {
      costs: {
        craftTime: { minutes: 1, seconds: 35 },
        mandatoryCost: {
          _Type_: "CraftingCost_Select",
          count: 1,
          options: [
            {
              _Type_: "CraftingCost_Resource",
              resource: { _RecordName_: "ResourceType.Aslarite" },
              quantity: { standardCargoUnits: 0.02 },
            },
          ],
        },
      },
    },
  };

  it("names a blueprint after what it produces when the name is a placeholder", () => {
    const result = blueprintsDomain.parse({
      records: [blueprintRecord("BP_CRAFT_Stampede", [tier])],
      localization,
    }) as Record<string, never>;
    const blueprints = result.blueprints as unknown as { name: string; category?: string }[];
    expect(blueprints[0].name).toBe("mining_module_stampede");
    expect(blueprints[0].category).toBe("VehicleComponentS0");
  });

  it("counts resource use across recipes, including inside choices", () => {
    const result = blueprintsDomain.parse({
      records: [blueprintRecord("A", [tier]), blueprintRecord("B", [tier])],
      localization,
    }) as Record<string, never>;
    expect((result.resourceUse as unknown as Record<string, number>).Aslarite).toBe(2);
  });

  it("skips dismantle records, which have no tiers", () => {
    const result = blueprintsDomain.parse({
      records: [blueprintRecord("GlobalGenericDismantle", [])],
      localization,
    }) as Record<string, never>;
    expect(result.blueprints).toHaveLength(0);
  });
});
