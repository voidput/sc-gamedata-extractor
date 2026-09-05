import { describe, it, expect } from "vitest";
import { isManufacturerRecord, manufacturersDomain, parseManufacturer } from "./manufacturers.js";
import { parseGlobalIni } from "../localization.js";
import type { GameRecord } from "../records.js";

const localization = parseGlobalIni(
  [
    "manufacturer_NameAEGS=Aegis Dynamics",
    "manufacturer_NameUBA=Universal Body Armor",
    "manufacturer_Name987=987",
  ].join("\n"),
);

function manufacturer(code: string, dir: string, nameKey: string): GameRecord {
  return {
    name: `SCItemManufacturer.${code}`,
    shortName: code,
    id: `id-${code}`,
    type: "SCItemManufacturer",
    recordPath: `libs/foundry/records/scitemmanufacturer/${dir}${code.toLowerCase()}`,
    value: { _Type_: "SCItemManufacturer", Localization: { Name: nameKey } },
  };
}

describe("isManufacturerRecord", () => {
  it("accepts top-level records", () => {
    expect(isManufacturerRecord(manufacturer("AEGS", "", "@x"))).toBe(true);
  });

  it("accepts clothing and armour brands, which are real manufacturers", () => {
    expect(isManufacturerRecord(manufacturer("987", "clothes/", "@x"))).toBe(true);
    expect(isManufacturerRecord(manufacturer("UBA", "armor/", "@x"))).toBe(true);
  });

  it("rejects paint logos, which merely carry their maker's name key", () => {
    // Over 900 of these exist; including them inflates the list roughly tenfold.
    expect(isManufacturerRecord(manufacturer("Paint_Raft", "paintcolorlogos/", "@x"))).toBe(false);
  });

  it("rejects shop kiosks and UI-only placeholders", () => {
    expect(isManufacturerRecord(manufacturer("Shop_Generic", "shop/shopkiosk/", "@x"))).toBe(false);
    expect(isManufacturerRecord(manufacturer("UILO_A", "uistyleonly/", "@x"))).toBe(false);
  });
});

describe("parseManufacturer", () => {
  it("resolves the localized name and keeps the code", () => {
    const parsed = parseManufacturer(manufacturer("AEGS", "", "@manufacturer_NameAEGS"), localization);
    expect(parsed).toMatchObject({ code: "AEGS", name: "Aegis Dynamics" });
  });
});

describe("manufacturersDomain.parse", () => {
  it("builds a code to name map from real manufacturers only", () => {
    const result = manufacturersDomain.parse({
      records: [
        manufacturer("AEGS", "", "@manufacturer_NameAEGS"),
        manufacturer("UBA", "armor/", "@manufacturer_NameUBA"),
        manufacturer("Paint_X", "paintcolorlogos/", "@manufacturer_NameAEGS"),
      ],
      localization,
    }) as Record<string, never>;

    const byCode = result.byCode as unknown as Record<string, string>;
    expect(byCode).toEqual({ AEGS: "Aegis Dynamics", UBA: "Universal Body Armor" });
  });

  it("reports codes that share a name rather than hiding them", () => {
    // FSKI really does carry Aegis's name key in the shipped data. Report it.
    const result = manufacturersDomain.parse({
      records: [
        manufacturer("AEGS", "", "@manufacturer_NameAEGS"),
        manufacturer("FSKI", "", "@manufacturer_NameAEGS"),
      ],
      localization,
    }) as Record<string, never>;

    expect(result.sharedNames).toEqual([{ name: "Aegis Dynamics", codes: ["AEGS", "FSKI"] }]);
  });

  it("drops rows whose name never resolved", () => {
    const result = manufacturersDomain.parse({
      records: [manufacturer("NOPE", "", "@manufacturer_NameMissing")],
      localization,
    }) as Record<string, never>;

    expect(result.manufacturers).toHaveLength(0);
    const summary = result.summary as unknown as Record<string, number>;
    expect(summary.unnamedOrPlaceholder).toBe(1);
  });

  it("yields empty output rather than throwing with no localization", () => {
    const result = manufacturersDomain.parse({
      records: [manufacturer("AEGS", "", "@manufacturer_NameAEGS")],
      localization: new Map(),
    }) as Record<string, never>;
    expect(result.manufacturers).toHaveLength(0);
  });
});
