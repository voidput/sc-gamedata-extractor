import { describe, expect, it } from "vitest";
import { PROSE_FIELDS, redactDump } from "./redact.mjs";

const header = { _source: "test", _extracted: "2026-11-23T00:00:00Z", _build: "4.10.201.48085" };
const prose = "A long paragraph of in-game flavour text that must never reach the public repo.";

/** Every key anywhere under `value`. */
function keysOf(value, out = new Set()) {
  if (Array.isArray(value)) for (const v of value) keysOf(v, out);
  else if (value && typeof value === "object") {
    for (const [k, v] of Object.entries(value)) {
      out.add(k);
      keysOf(v, out);
    }
  }
  return out;
}

const fixtures = {
  "game-commodities.json": {
    ...header,
    resources: [{ name: "Gold", description: prose, descriptionKey: "items_commodities_gold_desc" }],
    items: [{ name: "gold_ore", description: prose, descriptionKey: "items_commodities_gold_desc" }],
  },
  "game-manufacturers.json": { ...header, manufacturers: [{ code: "AEGS", name: "Aegis", description: prose }] },
  "game-missions.json": {
    ...header,
    missions: [
      { name: "Short Title", recordName: "MissionBrokerEntry.short", title: "Short Title", description: prose },
      { name: "x".repeat(121), recordName: "MissionBrokerEntry.long_one", title: "x".repeat(121), titleKey: "k" },
    ],
  },
  "game-refining.json": {
    ...header,
    processes: [{ name: "Cascade", description: prose }],
    materials: [{ name: "Gold", rawDescription: prose, refinedDescription: prose }],
  },
  "game-ships.json": { ...header, vehicles: [{ name: "Gladius", description: prose, descriptionKey: "vehicle_DescAEGS_Gladius" }] },
  "game-starmap.json": { ...header, locations: [{ name: "Daymar", description: prose }] },
  "game-weapons.json": { ...header, shipWeapons: [{ name: "Gun" }] },
};

describe("redactDump", () => {
  const published = Object.fromEntries(Object.entries(fixtures).map(([f, d]) => [f, redactDump(f, d)]));

  it("leaves no prose field and no prose text in any published dataset", () => {
    for (const [file, dump] of Object.entries(published)) {
      const keys = keysOf(dump);
      for (const field of PROSE_FIELDS[file] ?? []) expect(keys.has(field), `${file} ${field}`).toBe(false);
      expect(keys.has("description"), file).toBe(false);
      expect(JSON.stringify(dump)).not.toContain(prose);
    }
  });

  it("keeps the description keys and lists what it removed", () => {
    expect(published["game-ships.json"].vehicles[0].descriptionKey).toBe("vehicle_DescAEGS_Gladius");
    expect(published["game-commodities.json"].items[0].descriptionKey).toBe("items_commodities_gold_desc");
    expect(published["game-refining.json"]._redacted).toEqual(["description", "rawDescription", "refinedDescription"]);
    expect(Object.keys(published["game-ships.json"]).slice(0, 4)).toEqual(["_source", "_extracted", "_build", "_redacted"]);
  });

  it("keeps short mission titles and drops long ones, renaming from the record", () => {
    const [short, long] = published["game-missions.json"].missions;
    expect(short.title).toBe("Short Title");
    expect(long.title).toBeUndefined();
    expect(long.name).toBe("long_one");
    expect(long.titleKey).toBe("k");
    expect(published["game-missions.json"]._redacted).toEqual(["description", "title over 120 characters"]);
  });

  it("does not touch datasets without prose fields, and does not mutate its input", () => {
    expect(published["game-weapons.json"]).toBe(fixtures["game-weapons.json"]);
    expect(fixtures["game-ships.json"].vehicles[0].description).toBe(prose);
  });

  it("still reduces the string table to name keys", () => {
    const strings = redactDump("game-strings-english.json", {
      ...header,
      strings: { item_NameGladius: "Gladius", item_DescGladius: prose, Dlg_hello: "Hi" },
    });
    expect(strings.strings).toEqual({ item_NameGladius: "Gladius" });
  });
});
