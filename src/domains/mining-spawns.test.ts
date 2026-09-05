import { describe, it, expect } from "vitest";
import { describeProvider, miningSpawnsDomain } from "./mining-spawns.js";
import type { GameRecord } from "../records.js";

function rec(
  type: string,
  shortName: string,
  path: string,
  value: Record<string, unknown>,
): GameRecord {
  return {
    name: `${type}.${shortName}`,
    shortName,
    id: `id-${shortName}`,
    type,
    recordPath: path,
    value: { _Type_: type, ...value },
  };
}

describe("describeProvider", () => {
  // The location key is itself a localization key.
  const strings = new Map([
    ["Stanton1", "Hurston"],
    ["Stanton1c", "Magda"],
  ]);

  it("resolves the body name from the string table", () => {
    expect(describeProvider("HPP_Stanton1", strings)).toEqual({
      key: "Stanton1",
      system: "Stanton",
      body: "Hurston",
    });
  });

  it("names a sub-zone as itself, not as its parent planet", () => {
    // Stanton1c is Magda, a moon. Deriving from the "Stanton1" prefix would
    // label every sub-zone "Hurston", which is wrong.
    expect(describeProvider("HPP_Stanton1c", strings)).toMatchObject({
      key: "Stanton1c",
      body: "Magda",
    });
  });

  it("handles belts and rings that carry no body number", () => {
    expect(describeProvider("HPP_Nyx_GlaciemRing", strings)).toMatchObject({
      key: "Nyx_GlaciemRing",
      system: "Nyx",
    });
  });

  it("leaves the body undefined rather than guessing when unlisted", () => {
    expect(describeProvider("HPP_Sol9", strings).body).toBeUndefined();
  });
});

describe("miningSpawnsDomain.parse", () => {
  // Location keys resolve through the string table, same as in the real data.
  const strings = new Map([["Stanton1", "Hurston"]]);

  // The four hops a location-to-ore lookup has to walk.
  const records = [
    rec("HarvestableProviderPreset", "HPP_Stanton1", "harvestable/providerpresets/hpp_stanton1", {
      harvestableGroups: [
        {
          groupName: "SpaceShip_Mineables",
          groupProbability: 6,
          harvestables: [
            {
              harvestable: "file://./x/harvestable/harvestablepresets/mining_common_iron.json",
              relativeProbability: 34.8,
            },
          ],
        },
      ],
    }),
    rec(
      "HarvestablePreset",
      "Mining_Common_Iron",
      "harvestable/harvestablepresets/mining_common_iron",
      { entityClass: "file://./x/entities/mineable/mineablerock_surfacecommon_iron.json" },
    ),
    rec(
      "EntityClassDefinition",
      "MineableRock_SurfaceCommon_Iron",
      "entities/mineable/mineablerock_surfacecommon_iron",
      {
        Components: [
          {
            _Type_: "MineableParams",
            composition: "file://./x/mining/rockcompositionpresets/commonshipmineables_iron.json",
          },
        ],
      },
    ),
    rec(
      "MineableComposition",
      "CommonShipMineables_Iron",
      "mining/rockcompositionpresets/commonshipmineables_iron",
      {
        compositionArray: [
          {
            mineableElement: "file://./x/mining/mineableelements/iron_ore.json",
            minPercentage: 34.3,
            maxPercentage: 84.3,
            probability: 1,
          },
        ],
      },
    ),
  ];

  it("resolves a location all the way through to its ores", () => {
    const result = miningSpawnsDomain.parse({ records, localization: strings }) as Record<
      string,
      never
    >;
    const locations = result.locations as unknown as { body?: string; ores: string[] }[];
    expect(locations[0].body).toBe("Hurston");
    expect(locations[0].ores).toEqual(["iron_ore"]);
  });

  it("keeps the composition percentages on the spawn entry", () => {
    const result = miningSpawnsDomain.parse({ records, localization: strings }) as Record<
      string,
      never
    >;
    const locations = result.locations as unknown as {
      entries: { relativeProbability?: number; ores?: { minPercent?: number }[] }[];
    }[];
    expect(locations[0].entries[0].relativeProbability).toBe(34.8);
    expect(locations[0].entries[0].ores?.[0].minPercent).toBe(34.3);
  });

  it("builds the inverse index, which is the question people actually ask", () => {
    const result = miningSpawnsDomain.parse({ records, localization: strings }) as Record<
      string,
      never
    >;
    expect((result.oreLocations as unknown as Record<string, string[]>).iron_ore).toEqual([
      "Hurston",
    ]);
  });

  it("still records a location when the chain cannot be resolved", () => {
    // A broken or missing link should not drop the location silently.
    const result = miningSpawnsDomain.parse({
      records: [records[0]],
      localization: strings,
    }) as Record<string, never>;
    const summary = result.summary as unknown as Record<string, number>;
    expect(summary.locations).toBe(1);
    expect(summary.locationsWithOres).toBe(0);
    expect(summary.spawnEntries).toBe(1);
  });
});
