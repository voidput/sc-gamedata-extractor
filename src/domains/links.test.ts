import { describe, it, expect } from "vitest";
import { blueprintsDomain, collectBlueprintRewards } from "./blueprints.js";
import { parseQuality } from "./commodities.js";
import { describeRock, miningSpawnsDomain, rockSignature } from "./mining-spawns.js";
import { buildLocations } from "./starmap.js";
import { indexByRecordPath, type GameRecord } from "../records.js";

function rec(type: string, recordPath: string, value: Record<string, unknown>, shortName?: string): GameRecord {
  const name = shortName ?? recordPath.split("/").pop()!;
  return {
    name: `${type}.${name}`,
    shortName: name,
    id: `id-${name}`,
    type,
    recordPath: `libs/foundry/records/${recordPath}`,
    value: { _Type_: type, ...value },
  };
}

const fileRef = (path: string) => `file://./../../../../libs/foundry/records/${path}.json`;

describe("starmap hierarchy", () => {
  it("resolves path-string parents and walks them up to the system", () => {
    const locations = buildLocations(
      [
        rec("StarMapObject", "starmap/pu/system/stanton/starmapobject.stanton", { name: "Stanton" }, "Stanton"),
        rec("StarMapObject", "starmap/pu/system/stanton/stanton3/starmapobject.stanton3", {
          name: "ArcCorp",
          parent: fileRef("starmap/pu/system/stanton/starmapobject.stanton"),
        }, "Stanton3"),
        rec("StarMapObject", "starmap/pu/system/stanton/stanton3a/starmapobject.stanton3a", {
          name: "Lyria",
          parent: fileRef("starmap/pu/system/stanton/stanton3/starmapobject.stanton3"),
          jurisdiction: fileRef("lawsystem/jurisdictions/stanton/arccorp"),
        }, "Stanton3a"),
        rec("Jurisdiction", "lawsystem/jurisdictions/stanton/arccorp", {}, "ArcCorp"),
      ],
      new Map(),
    );
    const lyria = locations.find((l) => l.name === "Lyria");
    expect(lyria).toMatchObject({
      parent: "StarMapObject.Stanton3",
      path: ["Stanton", "ArcCorp"],
      system: "Stanton",
      jurisdiction: "ArcCorp",
    });
  });
});

const rock = (name: string, composition: string, signature: number) =>
  rec("EntityClassDefinition", `entities/mineable/${name.toLowerCase()}`, {
    Components: [
      {
        _Type_: "MineableParams",
        globalParams: fileRef("mining/miningglobalparamsship"),
        composition: fileRef(`mining/rockcompositionpresets/${composition.toLowerCase()}`),
      },
      {
        _Type_: "SSCSignatureSystemParams",
        radarProperties: { baseSignatureParams: { signatures: [0, 0, 0, 0, signature, 0, 0, 0] } },
      },
    ],
  }, name);

const composition = (name: string, element: string) =>
  rec("MineableComposition", `mining/rockcompositionpresets/${name.toLowerCase()}`, {
    depositName: "@quant",
    minimumDistinctElements: 3,
    compositionArray: [
      { mineableElement: fileRef(`mining/mineableelements/${element}`), minPercentage: 28.3, maxPercentage: 78.3, probability: 1, qualityScale: 0.49 },
    ],
  }, name);

describe("mining rocks", () => {
  const records = [
    rock("MineableRock_AsteroidLegendary_Quantainium", "LegendaryShipMineablesAsteroid_Quantainium", 3170),
    composition("LegendaryShipMineablesAsteroid_Quantainium", "quantainium_raw"),
    rock("MineableRock_Common_Iron", "UncommonShipMineables_Iron", 4270),
    composition("UncommonShipMineables_Iron", "iron_ore"),
  ];
  const byFile = new Map(records.map((r) => [r.recordPath.split("/").pop()!.toLowerCase(), r]));

  it("reads the RS signature from slot 4 and the tier from the composition", () => {
    expect(rockSignature(records[0])).toBe(3170);
    const described = describeRock(records[0], byFile, new Map([["quant", "Quantainium (Raw)"]]));
    expect(described).toMatchObject({
      kind: "ship",
      tier: "Legendary",
      deposit: "Quantainium (Raw)",
      signature: 3170,
      ores: [{ element: "quantainium_raw", qualityScale: 0.49 }],
    });
    // Case-sensitive, so Uncommon is not read as Common.
    expect(describeRock(records[2], byFile)?.tier).toBe("Uncommon");
  });

  it("works out each entry's share of its group and lists where each ore spawns", () => {
    const out = miningSpawnsDomain.parse({
      records: [
        ...records,
        rec("HarvestablePreset", "harvestable/harvestablepresets/mining_legendary_quantainium", {
          entityClass: fileRef("entities/mineable/mineablerock_asteroidlegendary_quantainium"),
        }),
        rec("HarvestablePreset", "harvestable/harvestablepresets/mining_iron", {
          entityClass: fileRef("entities/mineable/mineablerock_common_iron"),
        }),
        rec("HarvestableProviderPreset", "harvestable/providerpresets/hpp_aaronhalo", {
          harvestableGroups: [
            {
              groupName: "SpaceShip_Mineables",
              groupProbability: 0.3,
              harvestables: [
                { harvestable: fileRef("harvestable/harvestablepresets/mining_legendary_quantainium"), relativeProbability: 1 },
                { harvestable: fileRef("harvestable/harvestablepresets/mining_iron"), relativeProbability: 3 },
              ],
            },
          ],
        }, "HPP_AaronHalo"),
      ],
      localization: new Map([["AaronHalo", "Aaron Halo"]]),
    }) as { oreSpawns: Record<string, unknown>[]; rocks: Record<string, unknown>[] };

    expect(out.oreSpawns.find((s) => s.ore === "quantainium_raw")).toMatchObject({
      body: "Aaron Halo",
      tier: "Legendary",
      signature: 3170,
      spawnShare: 0.075,
    });
    expect(out.rocks.find((r) => r.rock === "MineableRock_Common_Iron")?.spawnsAt).toEqual(["Aaron Halo"]);
  });
});

describe("quality", () => {
  it("joins distribution, deduplicated location overrides and bands", () => {
    const records = [
      rec("CraftingQualityDistributionRecord", "crafting/qualitydistribution/shipmineables/legendaryshipmineable_qualitydistribution_default", {
        qualityDistribution: { min: 501, max: 1000, mean: 500, stddev: 203 },
      }, "LegendaryShipMineable_QualityDistribution_Default"),
      rec("CraftingQualityLocationOverrideRecord", "crafting/qualitydistribution/shipmineables/legendary_override", {
        locationOverride: {
          locationOverrideList: [
            { location: fileRef("starmap/pu/pyrosolarsystem"), qualityDistribution: { min: 501, max: 1000, mean: 500, stddev: 225 } },
            { location: fileRef("starmap/pu/pyrosolarsystem"), qualityDistribution: { min: 501, max: 1000, mean: 500, stddev: 225 } },
          ],
        },
      }),
      rec("StarMapObject", "starmap/pu/pyrosolarsystem", { name: "@pyro" }, "PyroSolarSystem"),
      rec("CraftingQualityQuantizationRecord", "crafting/qualityquantization/quantization_stileron", {
        qualityQuantization: { bands: [{ start: 950, end: 998, mappedValue: 972 }] },
      }),
    ];
    const quality = parseQuality(
      [
        {
          _Type_: "ResourceTypeCraftingData",
          qualityDistribution: { qualityDistributionRecord: fileRef("crafting/qualitydistribution/shipmineables/legendaryshipmineable_qualitydistribution_default") },
          qualityLocationOverride: { locationOverrideRecord: fileRef("crafting/qualitydistribution/shipmineables/legendary_override") },
          qualityQuantization: { qualityQuantizationRecord: fileRef("crafting/qualityquantization/quantization_stileron") },
        },
      ],
      indexByRecordPath(records),
      new Map([["pyro", "Pyro System"]]),
    );
    expect(quality).toEqual({
      profile: "LegendaryShipMineable_QualityDistribution_Default",
      tier: "Legendary",
      distribution: { min: 501, max: 1000, mean: 500, stddev: 203 },
      locationOverrides: [{ location: "Pyro System", min: 501, max: 1000, mean: 500, stddev: 225 }],
      bands: [{ start: 950, end: 998, value: 972 }],
    });
  });
});

describe("blueprint rewards", () => {
  const generator = rec("ContractGenerator", "contracts/contractgenerator/thecollector", {
    generators: [
      {
        debugName: "TheCollector_Small_Items",
        contracts: [
          {
            _Type_: "Contract",
            debugName: "TheCollector_vglFlightsuit",
            paramOverrides: { stringParamOverrides: [{ param: "Title", value: "@title" }] },
            contractResults: {
              contractResults: [
                { _Type_: "BlueprintRewards", chance: 1, blueprintPool: fileRef("crafting/blueprintrewards/pool_suit") },
              ],
            },
          },
        ],
      },
    ],
  });

  it("finds rewards with the contract and generator above them", () => {
    expect(collectBlueprintRewards([generator], new Map([["title", "Flight Suit Recipe"]]))).toEqual([
      {
        pool: "crafting/blueprintrewards/pool_suit",
        chance: 1,
        contract: "TheCollector_vglFlightsuit",
        mission: "Flight Suit Recipe",
        generator: "TheCollector_Small_Items",
      },
    ]);
  });

  it("names a blueprint after its product and lists the missions that award it", () => {
    const out = blueprintsDomain.parse({
      records: [
        generator,
        rec("BlueprintPoolRecord", "crafting/blueprintrewards/pool_suit", {
          blueprintRewards: [
            { weight: 1, blueprintRecord: fileRef("crafting/blueprints/bp_suit") },
            { weight: 3, blueprintRecord: fileRef("crafting/blueprints/bp_other") },
          ],
        }, "BP_REWARDS_Suit"),
        rec("CraftingBlueprintRecord", "crafting/blueprints/bp_suit", {
          blueprint: {
            blueprintName: "@LOC_PLACEHOLDER",
            processSpecificData: { entityClass: fileRef("entities/scitem/characters/human/armor/vgl_flightsuit") },
            tiers: [{ recipe: { costs: { craftTime: { seconds: 90 }, mandatoryCost: { _Type_: "CraftingCost_Resource" } } } }],
          },
        }),
        rec("EntityClassDefinition", "entities/scitem/characters/human/armor/vgl_flightsuit", {
          Components: [{ _Type_: "SAttachableComponentParams", AttachDef: { Type: "Char_Armor_Undersuit", Localization: { Name: "@suit" } } }],
        }),
      ],
      localization: new Map([["suit", "VGL Flightsuit"], ["title", "Flight Suit Recipe"]]),
    }) as { blueprints: Record<string, unknown>[] };

    expect(out.blueprints[0]).toMatchObject({
      name: "VGL Flightsuit",
      displayName: "VGL Flightsuit",
      rewardedBy: [{ mission: "Flight Suit Recipe", pool: "BP_REWARDS_Suit", chance: 1, weight: 1, poolWeight: 4 }],
    });
  });
});
