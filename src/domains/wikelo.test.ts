import { describe, it, expect } from "vitest";
import { collectHaulingOrders, splitRecordName, wikeloDomain, parseWikeloTrade } from "./wikelo.js";
import type { GameRecord } from "../records.js";

function template(shortName: string, orders: unknown[]): GameRecord {
  return {
    name: `ContractTemplate.${shortName}`,
    shortName,
    id: `id-${shortName}`,
    type: "ContractTemplate",
    recordPath: `libs/foundry/records/contracts/contracttemplates/${shortName.toLowerCase()}`,
    value: { objectiveTokens: [{ objectiveHandler: { haulingOrders: orders } }] },
  };
}

const order = (path: string, min: number, max = min) => ({
  _Type_: "HaulingOrder_EntityClass",
  entityClass: `file://./../libs/foundry/records/entities/scitem/carryables/1h/${path}.json`,
  minAmount: min,
  maxAmount: max,
});

describe("collectHaulingOrders", () => {
  it("finds orders nested under the objective handler", () => {
    const found = collectHaulingOrders(
      template("X", [order("harvestable_mineral_1h_carinite", 10)]).value,
    );
    expect(found).toEqual([{ item: "harvestable_mineral_1h_carinite", amount: 10, maxAmount: undefined }]);
  });

  it("falls back to maxAmount when minAmount is zero", () => {
    expect(collectHaulingOrders({ ...order("thing", 0, 7) })[0].amount).toBe(7);
  });

  it("uses minAmount when maxAmount is zero, as some records store it", () => {
    // Seen in the wild: minAmount 1, maxAmount 0.
    expect(collectHaulingOrders({ ...order("volt_rifle_energy_01", 1, 0) })[0].amount).toBe(1);
  });

  it("records a genuine range when the two differ", () => {
    const found = collectHaulingOrders({ ...order("thing", 5, 9) })[0];
    expect(found).toMatchObject({ amount: 5, maxAmount: 9 });
  });

  it("returns nothing when there are no hauling orders", () => {
    expect(collectHaulingOrders({ objectiveTokens: [{ other: true }] })).toHaveLength(0);
  });
});

describe("splitRecordName", () => {
  it("splits a category from the reward", () => {
    expect(splitRecordName("TheCollector_Vehicles_Polaris")).toEqual({
      category: "Vehicles",
      reward: "Polaris",
    });
  });

  it("joins a multi-word reward", () => {
    expect(splitRecordName("TheCollector_Vehicles_Starlancer_Tac").reward).toBe("Starlancer Tac");
  });

  it("ignores names that are not collector records", () => {
    expect(splitRecordName("SomethingElse_Foo")).toEqual({});
  });
});

describe("wikeloDomain.parse", () => {
  const records = [
    template("TheCollector_Vehicles_Polaris", [
      order("carryable_1h_cy_banu_favour_wikelo", 50),
      order("harvestable_ore_1h_jacliumore", 20),
    ]),
    template("TheCollector_ForFavours_Caranite", [order("carryable_1h_cy_banu_favour_wikelo", 10)]),
    // Flow scaffolding with no hand-ins should not become a trade.
    template("TheCollector_Flow", []),
  ];

  it("keeps only templates that actually ask for something", () => {
    const result = wikeloDomain.parse({ records, localization: new Map() }) as Record<string, never>;
    expect(result.summary).toMatchObject({ trades: 2, collectorRecordsWithoutHandIns: 1 });
  });

  it("totals demand across every trade", () => {
    const result = wikeloDomain.parse({ records, localization: new Map() }) as Record<string, never>;
    const demand = result.totalDemand as unknown as Record<string, number>;
    expect(demand.carryable_1h_cy_banu_favour_wikelo).toBe(60);
  });

  it("marks the reward as inferred, since no reward field exists", () => {
    const trade = parseWikeloTrade(records[0]);
    expect(trade?.rewardInferred).toBe("Polaris");
  });
});
