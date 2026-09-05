import { describe, it, expect } from "vitest";
import { componentsDomain, isPlaceholderName, parseComponent } from "./components.js";
import { parseGlobalIni } from "../localization.js";
import type { GameRecord } from "../records.js";

const localization = parseGlobalIni("item_NameShield=5SA 'Rhada'");

function shipItem(
  shortName: string,
  paramsType: string,
  stats: Record<string, unknown>,
  nameKey = "@item_NameShield",
): GameRecord {
  return {
    name: `EntityClassDefinition.${shortName}`,
    shortName,
    id: `id-${shortName}`,
    type: "EntityClassDefinition",
    recordPath: `libs/foundry/records/entities/scitem/ships/x/${shortName}`,
    value: {
      Components: [
        {
          _Type_: "SAttachableComponentParams",
          AttachDef: {
            Type: "Shield",
            SubType: "UNDEFINED",
            Size: 2,
            Grade: 1,
            Localization: { Name: nameKey },
          },
        },
        { _Type_: paramsType, ...stats },
      ],
    },
  };
}

describe("isPlaceholderName", () => {
  it("rejects every shape of non-name the game uses", () => {
    expect(isPlaceholderName(undefined)).toBe(true);
    expect(isPlaceholderName("")).toBe(true);
    expect(isPlaceholderName("@item_NameMissing")).toBe(true);
    expect(isPlaceholderName("<= PLACEHOLDER =>")).toBe(true);
    expect(isPlaceholderName("@LOC_PLACEHOLDER")).toBe(true);
  });

  it("accepts a real name", () => {
    expect(isPlaceholderName("5SA 'Rhada'")).toBe(false);
  });
});

describe("parseComponent", () => {
  it("categorises by params struct and reads the stats", () => {
    const record = shipItem("SHLD_Test", "SCItemShieldGeneratorParams", {
      MaxShieldHealth: 7200,
      MaxShieldRegen: 1584,
      DownedRegenDelay: 9.17,
      Irrelevant: "ignored",
    });

    const parsed = parseComponent(record, localization);
    expect(parsed).toMatchObject({
      category: "shields",
      name: "5SA 'Rhada'",
      size: 2,
      grade: 1,
    });
    expect(parsed?.stats).toEqual({
      MaxShieldHealth: 7200,
      MaxShieldRegen: 1584,
      DownedRegenDelay: 9.17,
    });
  });

  it("falls back to the record code and flags templates", () => {
    const record = shipItem("QDRV_S01_Template", "SCItemQuantumDriveParams", { jumpRange: 1 }, "@LOC_PLACEHOLDER");
    const parsed = parseComponent(record, localization);
    expect(parsed?.name).toBe("QDRV_S01_Template");
    expect(parsed?.isTemplate).toBe(true);
  });

  it("ignores ship items that are not one of the known categories", () => {
    // A seat blast shield lives under a shield-ish path but has no shield params.
    const seat = shipItem("MISC_BlastShield_SeatAccess", "SCItemSeatAccessParams", {});
    expect(parseComponent(seat, localization)).toBeUndefined();
  });

  it("drops non-numeric stats rather than emitting them", () => {
    const record = shipItem("X", "SCItemShieldGeneratorParams", { MaxShieldHealth: null });
    expect(parseComponent(record, localization)?.stats).toEqual({});
  });
});

describe("componentsDomain.parse", () => {
  it("groups components by category and counts them", () => {
    const result = componentsDomain.parse({
      records: [
        shipItem("A", "SCItemShieldGeneratorParams", { MaxShieldHealth: 100 }),
        shipItem("B", "SCItemShieldGeneratorParams", { MaxShieldHealth: 200 }),
        shipItem("C", "SCItemQuantumDriveParams", { jumpRange: 5 }),
      ],
      localization,
    }) as Record<string, never>;

    expect(result.summary).toEqual({ shields: 2, quantumDrives: 1 });
  });
});
