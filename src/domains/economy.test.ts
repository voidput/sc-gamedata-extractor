import { describe, it, expect } from "vitest";
import { boxSizes, cargoDomain, parseCargoManifest } from "./cargo.js";
import { commoditiesDomain, damageGroups, descriptionOf, groupLabel, parseCommodityItem, parseResourceDatabase, qualitySteps, resourceLinks, scuOf, withSteps } from "./commodities.js";
import { quantumDomain } from "./quantum.js";
import { parseRatings, refiningDomain } from "./refining.js";
import { salvageDomain } from "./salvage.js";
import { parseCommodityRules, parseSelling } from "./shops.js";
import { compareNames, type GameRecord } from "../records.js";

const empty = new Map<string, string>();

function rec(
  type: string,
  shortName: string,
  value: Record<string, unknown>,
  recordPath = `libs/foundry/records/x/${shortName.toLowerCase()}`,
): GameRecord {
  return {
    name: `${type}.${shortName}`,
    shortName,
    id: `id-${shortName}`,
    type,
    recordPath,
    value: { _Type_: type, ...value },
  };
}

const ref = (name: string) => ({ _RecordName_: name, _RecordId_: `id-${name}` });

const resource = (name: string, density: number, extra: Record<string, unknown> = {}) => ({
  _RecordName_: `ResourceType.${name}`,
  _RecordId_: `id-${name}`,
  _Type_: "ResourceType",
  displayName: `@items_commodities_${name.toLowerCase()}`,
  densityType: { densityUnit: { _Type_: "GramsPerCubicCentimeter", gramsPerCubicCentimeter: density } },
  ...extra,
});

// The database nests groups and puts the refined resource inline under the
// raw one, with null entries scattered through both lists.
const database = rec("ResourceTypeDatabase", "ResourceTypeDatabase", {
  groups: [
    {
      _RecordName_: "ResourceTypeGroup.Metal",
      resources: [resource("Gold", 1)],
      groups: [
        null,
        {
          _RecordName_: "ResourceTypeGroup.UnrefinedOres",
          resources: [
            resource("Ore_Gold", 19.3, { refinedVersion: resource("Gold", 1) }),
            null,
          ],
          groups: [],
        },
      ],
    },
    {
      _RecordName_: "ResourceTypeGroup.Mineral",
      resources: [
        resource("Raw_Quantainium", 20.45, {
          properties: [null, { _Type_: "ResourceTypeVolatility", volatility: 1, healthDecayPerSecond: 0.0675 }],
        }),
      ],
      groups: [],
    },
  ],
});

const strings = new Map([
  ["items_commodities_gold", "Gold"],
  ["items_commodities_ore_gold", "Gold (Ore)"],
  ["refinery_ui_ProcessingType_SlowCareful", "Dinyx Solventation"],
]);

describe("compareNames", () => {
  it("orders numbers by value and ignores case and punctuation", () => {
    const names = ["Civilian_10", "[PH] Noodles", "agricium_ore", "\"Quantainium\" Water", "Civilian_2", "Agricium"];
    expect([...names].sort(compareNames)).toEqual([
      "Agricium",
      "agricium_ore",
      "Civilian_2",
      "Civilian_10",
      "[PH] Noodles",
      "\"Quantainium\" Water",
    ]);
  });
});

describe("resource database", () => {
  it("walks nested groups and skips null entries", () => {
    const resources = parseResourceDatabase(database, strings);
    expect(resources.map((r) => r.name)).toEqual(["Gold", "Ore_Gold", "Raw_Quantainium"]);
    expect(resources[1]).toMatchObject({
      group: "Metal",
      subgroups: ["UnrefinedOres"],
      densityGramsPerCc: 19.3,
      refinesTo: "Gold",
      displayName: "Gold (Ore)",
    });
  });

  it("keeps volatility, which only quantainium carries", () => {
    const quant = parseResourceDatabase(database).find((r) => r.name === "Raw_Quantainium");
    expect(quant?.volatility).toEqual({ volatility: 1, healthDecayPerSecond: 0.0675 });
  });

  it("strips UI markup from names", () => {
    const co2 = { ...database, value: { groups: [{ _RecordName_: "ResourceTypeGroup.Gas", resources: [resource("CarbonDioxide", 1)] }] } };
    const [r] = parseResourceDatabase(co2, new Map([["items_commodities_carbondioxide", 'CO<font size="%d">2</font>']]));
    expect(r.displayName).toBe("CO2");
  });

  it("drops a name it cannot resolve rather than emitting the key", () => {
    const quant = parseResourceDatabase(database, strings).find((r) => r.name === "Raw_Quantainium");
    expect(quant?.displayName).toBeUndefined();
  });
});

describe("descriptions", () => {
  it("strips markup, turns the literal \\n into line breaks and drops placeholders", () => {
    const strings = new Map([
      ["ore_des", "Found near <b>volcanic</b> outcroppings.\\nUsed as an insulator. "],
      ["ph_des", "<= PLACEHOLDER =>"],
    ]);
    expect(descriptionOf(strings, "@ore_des")).toBe("Found near volcanic outcroppings.\nUsed as an insulator.");
    expect(descriptionOf(strings, "@ph_des")).toBeUndefined();
    expect(descriptionOf(new Map([["t", "Tritium description."]]), "@t")).toBeUndefined();
  });
});

describe("commodity items", () => {
  it("reads SCU from both centi and micro units", () => {
    expect(scuOf({ _Type_: "SCentiCargoUnit", centiSCU: 1 })).toBe(0.01);
    expect(scuOf({ _Type_: "SMicroCargoUnit", microSCU: 750 })).toBe(0.00075);
  });

  it("links an item to its resource through the container composition", () => {
    const item = parseCommodityItem(
      rec(
        "EntityClassDefinition",
        "gold",
        {
          Components: [
            { _Type_: "SCItemPurchasableParams", displayName: "@items_commodities_gold" },
            {
              _Type_: "ResourceContainer",
              capacity: { centiSCU: 1 },
              defaultComposition: [{ entry: ref("ResourceType.Gold"), weight: 1 }],
            },
          ],
        },
        "libs/foundry/records/entities/commodities/metals/gold",
      ),
      strings,
    );
    expect(item).toMatchObject({ displayName: "Gold", category: "metals", resource: "Gold", capacityScu: 0.01 });
  });

  it("parses the whole domain", () => {
    const out = commoditiesDomain.parse({
      records: [database, rec("CommodityDamageConfiguration", "Base", { minimumSpeed: 15, gracePeriod: 10 })],
      localization: empty,
    });
    expect(out.summary).toEqual({ resources: 3, items: 0, damage: 1 });
    expect(out.byGroup).toEqual({ Metal: ["Gold", "Ore_Gold"], Mineral: ["Raw_Quantainium"] });
  });
});

describe("refining", () => {
  const process = (speed: string, quality: string) =>
    rec("RefiningProcess", `ProcessingType-${speed}${quality}`, {
      refiningSpeed: speed,
      refiningQuality: quality,
      processName: `@refinery_ui_ProcessingType_${speed}${quality}`,
    });

  it("orders methods by speed then quality and pairs raw with refined", () => {
    const out = refiningDomain.parse({
      records: [
        process("Fast", "Wasteful"),
        process("Slow", "Careful"),
        process("Slow", "Wasteful"),
        database,
        rec("CraftingGlobalParams", "CraftingGlobalParams", {
          refiningQualityUnitMultiplier: 2,
          dismantleBlacklistResources: [ref("ResourceType.Quantanium")],
        }),
      ],
      localization: strings,
    }) as Record<string, unknown> & { processes: { name: string; displayName?: string }[] };

    expect(out.processes.map((p) => p.name)).toEqual([
      "ProcessingType-SlowWasteful",
      "ProcessingType-SlowCareful",
      "ProcessingType-FastWasteful",
    ]);
    expect(out.processes[1].displayName).toBe("Dinyx Solventation");
    expect(out.refinables).toEqual([
      {
        raw: "Ore_Gold",
        rawDisplayName: "Gold (Ore)",
        refined: "Gold",
        refinedDisplayName: "Gold",
        group: "Metal",
        rawDensityGramsPerCc: 19.3,
        refinedDensityGramsPerCc: 1,
      },
    ]);
    expect(out.settings).toEqual({
      refiningQualityUnitMultiplier: 2,
      defaultCompositionQuality: undefined,
      dismantleBlacklistResources: ["Quantanium"],
    });
  });
});

describe("refining ratings", () => {
  it("reads the details string the refinery screen shows", () => {
    expect(parseRatings("High Yield // Low Cost // Slowest")).toEqual({
      yield: "High",
      cost: "Low",
      speed: "Slowest",
      yieldRank: 3,
      costRank: 1,
      speedRank: 1,
    });
  });

  it("strips the literal \\n and the Speed suffix some strings carry", () => {
    expect(parseRatings("Moderate Yield // Moderate Cost // Moderate Speed\\n")).toMatchObject({
      speed: "Moderate",
      speedRank: 5,
    });
  });

  it("attaches ratings, description and workload tiers from the string table", () => {
    const out = refiningDomain.parse({
      records: [
        rec("RefiningProcess", "ProcessingType-NormalWasteful", {
          refiningSpeed: "Normal",
          refiningQuality: "Wasteful",
          processName: "@refinery_ui_ProcessingType_NormalWasteful",
        }),
      ],
      localization: new Map([
        ["refinery_ui_ProcessingType_NormalWasteful", "Cormack Method"],
        ["refinery_ui_ProcessingType_NormalWasteful_Desc", "Quick, low yield. "],
        ["refinery_ui_ProcessingType_NormalWasteful_Details,P", "Low Yield // Moderate Cost // Very Fast"],
        ["refinery_ui_Capacity_Low", "No surcharge at this time."],
      ]),
    }) as Record<string, unknown> & { processes: Record<string, unknown>[] };

    // Labelled Normal speed, but rated the second fastest of the nine.
    expect(out.processes[0]).toMatchObject({
      speed: "Normal",
      description: "Quick, low yield.",
      ratings: { yield: "Low", cost: "Moderate", speed: "Very Fast", speedRank: 8 },
    });
    expect(out.workloadTiers).toEqual([{ level: "Low", text: "No surcharge at this time." }]);
  });
});

describe("shops", () => {
  it("names supply and demand bands from their field names", () => {
    const rules = parseCommodityRules(
      rec("GlobalShopCommodityParams", "Data", {
        noSupplyLevel: 0,
        VeryLowSupplyLevel: 10,
        VeryHighDemandLevel: 10,
        autoLoadingBoxSizePrices: { oneSCU: 30, thirtyTwoSCU: 680 },
      }),
    );
    expect(rules.supplyLevels).toEqual({ no: 0, veryLow: 10 });
    expect(rules.demandLevels).toEqual({ veryHigh: 10 });
    expect(rules.autoLoadingBoxPrices).toEqual([
      { scu: 1, value: 30 },
      { scu: 32, value: 680 },
    ]);
  });

  it("drops the UNDEFINED subtype on sell-back overrides", () => {
    const selling = parseSelling(
      rec("GlobalShopSellingParams", "Data", {
        matchPercentage: -20,
        itemTypeModifiers: [{ Type: "Cooler", SubType: "UNDEFINED", matchPercentage: -75 }],
      }),
    );
    expect(selling.itemTypeModifiers).toEqual([{ type: "Cooler", subType: undefined, matchPercentage: -75 }]);
  });
});

describe("cargo", () => {
  it("lists box sizes smallest first, fractions included", () => {
    // As object keys "0.125" would land after "16"; a list keeps SCU order.
    expect(boxSizes({ sixteenSCU: 12, label: "x", one_eighthSCU: 0, oneSCU: 2 })).toEqual([
      { scu: 0.125, value: 0 },
      { scu: 1, value: 2 },
      { scu: 16, value: 12 },
    ]);
  });

  it("reads both probability and allocation resource entries", () => {
    const manifest = parseCargoManifest(
      rec(
        "CargoManifest",
        "ShipHeist_Freelancer_Medium",
        {
          cargoFillCapacity: {
            _Type_: "CargoFillCapacityValue_Random",
            minCapacityRange: 0.4,
            maxCapacityRange: 0.6,
            resources: [
              { _Type_: "CargoResource", resource: ref("ResourceType.Nitrogen"), probability: 0.4 },
              {
                // `resources` is a single entry here, despite the name.
                _Type_: "CargoResourceAllocation",
                resources: { _Type_: "CargoResource", resource: ref("ResourceType.Gold"), probability: 1 },
                minResourceAllocationPercentage: 0.03,
                maxResourceAllocationPercentage: 0.05,
              },
            ],
          },
          cargoSpawnRules: [
            {
              _Type_: "CargoSpawnEntityDef",
              probability: 0.5,
              entities: ["file://./../libs/foundry/records/entities/scitem/ships/weapons/missiles/misl_s02.json"],
              quantityAlgorithmType: { _Type_: "CargoManifestFixedQuantityDef", quantity: 30 },
            },
          ],
        },
        "libs/foundry/records/cargomanifest/shipheist/shipheist_freelancer_medium",
      ),
    );
    expect(manifest).toMatchObject({
      category: "shipheist",
      fillMode: "Random",
      minCapacity: 0.4,
      maxCapacity: 0.6,
      resources: [
        { resource: "Nitrogen", probability: 0.4 },
        { resource: "Gold", probability: 1, minShare: 0.03, maxShare: 0.05 },
      ],
      spawnRules: [{ kind: "entity", probability: 0.5, items: ["misl_s02"], quantity: 30 }],
    });
  });

  it("takes the category from the name for manifests at the top level", () => {
    const out = cargoDomain.parse({
      records: [rec("CargoManifest", "LegalCargo_TSG", {}, "libs/foundry/records/cargomanifest/legalcargo_tsg")],
      localization: empty,
    });
    expect(out.byCategory).toEqual({ legalcargo: ["LegalCargo_TSG"] });
  });
});

describe("salvage and quantum", () => {
  it("keeps the gameplay constants and nothing cosmetic", () => {
    const salvage = salvageDomain.parse({
      records: [
        rec("SGlobalSalvageRepairBeamParams", "Beam", {
          hitsPerSecond: 10,
          materialParams: { hullThicknessMeters: 0.009, RMCResourceType: ref("ResourceType.RMC") },
          highlightParams: { colors: [] },
        }),
      ],
      localization: empty,
    });
    expect(salvage.repairBeam).toMatchObject({ hitsPerSecond: 10, hullThicknessMeters: 0.009, materialResource: "RMC" });
    expect(salvage.structural).toBeUndefined();

    const quantum = quantumDomain.parse({
      records: [
        rec("QuantumDriveGlobalParams", "Q", {
          minimumAltitudeForQuantum: 2000,
          splineTraversalParams: { rollbackParams: { fullRotationDistance: 2450000 } },
        }),
      ],
      localization: empty,
    });
    expect(quantum.rules).toMatchObject({ minimumAltitudeForQuantum: 2000, rollbackFullRotationDistance: 2450000 });
  });
});

describe("commodity derived fields", () => {
  const base = { id: "", subgroups: [] as string[] };
  const resources = [
    { ...base, name: "Raw_Quantainium", recordName: "ResourceType.Raw_Quantainium", group: "Metal", subgroups: ["UnrefinedOres"], refinesTo: "Quantainium", densityGramsPerCc: 2.4, volatility: { volatility: 0.6 } },
    { ...base, name: "Quantainium", recordName: "ResourceType.Quantainium", group: "Metal", densityGramsPerCc: 1 },
    { ...base, name: "Heat", recordName: "ResourceType.Heat", group: "HeatPlaceholder" },
  ];
  const items = [
    { name: "quantainium_raw", recordName: "EntityClassDefinition.quantainium_raw", id: "", category: "metals", resource: "Raw_Quantainium", type: "Metal", capacityScu: 1 },
  ];
  const [raw, refined, heat] = resourceLinks(resources, items);

  it("labels groups and flags placeholders as not tradable", () => {
    expect(groupLabel("UnrefinedOres")).toBe("Unrefined Ores");
    expect(groupLabel("Raw_Minerals")).toBe("Raw Minerals");
    expect(raw.groupLabel).toBe("Metal");
    expect(raw.subgroupLabel).toBe("Unrefined Ores");
    expect(heat.tradable).toBe(false);
    expect(raw.tradable).toBe(true);
  });

  it("links a raw ore and its refined form both ways, with its cargo", () => {
    expect(raw.raw).toBe(true);
    expect(raw.volatile).toBe(true);
    expect(raw.cargo).toEqual([{ className: "quantainium_raw", kind: "Metal", capacityScu: 1 }]);
    expect(refined.raw).toBe(false);
    expect(refined.rawSources).toEqual(["Raw_Quantainium"]);
    expect(refined.volatile).toBeUndefined();
  });

  it("marks the engine-default density", () => {
    expect(refined.densityIsDefault).toBe(true);
    expect(raw.densityIsDefault).toBeUndefined();
  });

  it("groups damage configurations with identical settings", () => {
    const same = { minimumSpeed: 15, gracePeriod: 10 };
    const groups = damageGroups([
      { name: "Base", recordName: "C.Base", ...same },
      { name: "Mole", recordName: "C.Mole", ...same },
      { name: "Odd", recordName: "C.Odd", minimumSpeed: 5 },
    ]);
    expect(groups).toEqual([
      { configs: ["Base", "Mole"], ...same },
      { configs: ["Odd"], minimumSpeed: 5 },
    ]);
  });
});

describe("quality steps", () => {
  const aphorite = {
    distribution: { min: 201, max: 1000, mean: 201, stddev: 298 },
    locationOverrides: [{ location: "Pyro System", min: 201, max: 1000, mean: 209, stddev: 308 }],
    bands: [
      { start: 0, end: 399, value: 348 },
      { start: 400, end: 599, value: 523 },
      { start: 600, end: 1000, value: 800 },
    ],
  };

  it("clips bands to the curve: a roll in 201-399 shows as 348", () => {
    const steps = qualitySteps(aphorite.bands, aphorite.distribution) ?? [];
    expect(steps.map((s) => [s.start, s.end, s.shown])).toEqual([[201, 399, 348], [400, 599, 523], [600, 1000, 800]]);
  });

  it("shares sum to 1 and match the app's estimate", () => {
    const steps = qualitySteps(aphorite.bands, aphorite.distribution) ?? [];
    const sum = steps.reduce((n, s) => n + (s.shareEstimate ?? 0), 0);
    expect(Math.abs(sum - 1)).toBeLessThan(1e-6);
    expect(steps[0].shareEstimate).toBeCloseTo(0.497, 2);
    expect(steps[0].shareEstimate! > steps[1].shareEstimate! && steps[1].shareEstimate! > steps[2].shareEstimate!).toBe(true);
  });

  it("omits the share without a spread and drops unreachable bands", () => {
    const steps = qualitySteps([{ start: 0, end: 499, value: 250 }, { start: 500, end: 999, value: 750 }], { min: 0, max: 400 }) ?? [];
    expect(steps).toEqual([{ start: 0, end: 400, shown: 250, shareEstimate: undefined }]);
  });

  it("gives each location override its own steps", () => {
    const q = withSteps(aphorite);
    expect(q?.steps?.length).toBe(3);
    expect(q?.locationOverrides?.[0].steps?.length).toBe(3);
    expect(q?.locationOverrides?.[0].steps?.[0].shareEstimate).not.toBe(q?.steps?.[0].shareEstimate);
  });
});
