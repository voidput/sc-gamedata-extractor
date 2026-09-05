import { describe, it, expect } from "vitest";
import { missionsDomain, parseMission, referencedName } from "./missions.js";
import { parseGlobalIni } from "../localization.js";
import type { GameRecord } from "../records.js";

const localization = parseGlobalIni(
  ["Heist_Title=Data Heist", "Heist_Giver=Ruto", "Heist_Desc=Steal the data."].join("\n"),
);

function mission(shortName: string, value: Record<string, unknown>): GameRecord {
  return {
    name: `MissionBrokerEntry.${shortName}`,
    shortName,
    id: `id-${shortName}`,
    type: "MissionBrokerEntry",
    recordPath: `libs/foundry/records/missionbroker/pu_missions/${shortName.toLowerCase()}`,
    value: { _Type_: "MissionBrokerEntry", ...value },
  };
}

describe("referencedName", () => {
  it("takes the record name off a file reference", () => {
    expect(referencedName("file://./../missiontype/pu/race.json")).toBe("race");
  });
});

describe("parseMission", () => {
  it("resolves the localized title, giver and description", () => {
    const parsed = parseMission(
      mission("Heist", {
        title: "@Heist_Title",
        missionGiver: "@Heist_Giver",
        description: "@Heist_Desc",
        missionReward: { reward: 500000, currencyType: "UEC" },
      }),
      localization,
    );
    expect(parsed).toMatchObject({
      name: "Data Heist",
      giver: "Ruto",
      reward: 500000,
      currency: "UEC",
    });
  });

  it("falls back to the record name for spaced placeholder titles", () => {
    // "< = TEST NAME = >" is spaced, so a plain "<=" check misses it.
    const parsed = parseMission(
      mission("LootGenerationTestMissionPU", { title: "< = TEST NAME = >" }),
      localization,
    );
    expect(parsed.name).toBe("LootGenerationTestMissionPU");
    expect(parsed.title).toBeUndefined();
  });

  it("treats difficulty -1 as unset rather than easiest", () => {
    expect(parseMission(mission("X", { missionDifficulty: -1 }), localization).difficulty)
      .toBeUndefined();
    expect(parseMission(mission("Y", { missionDifficulty: 0 }), localization).difficulty).toBe(0);
  });

  it("flags developer content instead of dropping it", () => {
    expect(parseMission(mission("X", { notForRelease: true }), localization).notForRelease).toBe(true);
    expect(parseMission(mission("Y", { notForRelease: false }), localization).notForRelease)
      .toBeUndefined();
  });
});

describe("missionsDomain.parse", () => {
  const records = [
    mission("Live1", {
      title: "@Heist_Title",
      missionGiver: "@Heist_Giver",
      missionReward: { reward: 500000, currencyType: "UEC" },
    }),
    mission("Live2", {
      missionGiver: "@Heist_Giver",
      missionReward: { reward: 10000, currencyType: "UEC" },
    }),
    mission("DevOnly", { notForRelease: true, missionReward: { reward: 999999 } }),
  ];

  it("separates live missions from developer content", () => {
    const result = missionsDomain.parse({ records, localization }) as Record<string, never>;
    expect(result.summary).toMatchObject({ missions: 3, live: 2, notForRelease: 1, withPayout: 2 });
  });

  it("excludes developer content from the payout stats", () => {
    const result = missionsDomain.parse({ records, localization }) as Record<string, never>;
    const summary = result.summary as unknown as Record<string, number>;
    expect(summary.maxReward).toBe(500000);
  });

  it("counts live missions per giver", () => {
    const result = missionsDomain.parse({ records, localization }) as Record<string, never>;
    expect((result.byGiver as unknown as Record<string, number>).Ruto).toBe(2);
  });
});
