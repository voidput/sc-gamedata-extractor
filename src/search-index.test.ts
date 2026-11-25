import { describe, expect, it } from "vitest";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

import { buildEntries } from "./query/fuzzy.js";
import type { ItemTags } from "./query/types.js";
import { buildSearchIndex, loadSearchIndexEntries, SEARCH_INDEX_FILE, serializeSearchIndex, writeSearchIndex, type SearchIndexDoc } from "./search-index.js";

const item = (className: string, name: string, tags: string[], extra: Partial<ItemTags> = {}): ItemTags => ({
  className,
  name,
  id: className,
  type: "WeaponPersonal",
  tags,
  ...extra,
});

const items: ItemTags[] = [
  item("ksar_lmg_energy_01", "ksar_lmg_energy_01", ["Weapon/FPS/Stocked/LMG/Verdict", "Manufacturer/KastakArms"], { isTemplate: true }),
  item("ksar_lmg_energy_01_store01", 'Verdict "Derivation" LMG', ["Weapon/FPS/Stocked/LMG/Verdict", "Manufacturer/KastakArms"]),
  item("door_01", "Door", [], { type: "Door" }),
  item("no_type_01", "no_type_01", [], { type: undefined, manufacturer: "Acme" }),
  item("outlaw_arms_01", "Lynx Arms Grey", ["Manufacturer/KastakArms"], { type: "Char_Armor_Arms" }),
];
const ships = [
  { name: "Aegis Gladius", recordName: "EntityClassDefinition.AEGS_Gladius", manufacturer: "aegs", role: "Light Fighter" },
  { name: "Gladius Template", recordName: "EntityClassDefinition.AEGS_Gladius_Template", isTemplate: true },
  { name: "Cutter", recordName: "EntityClassDefinition.DRAK_Cutter", manufacturer: "drak", career: "Exploration" },
];

describe("search index", () => {
  const doc = buildSearchIndex(items, ships, "4.10.0.1");

  it("loads back exactly what buildEntries gives, in both modes", () => {
    expect(loadSearchIndexEntries(doc)).toStrictEqual(buildEntries(items, ships));
    expect(loadSearchIndexEntries(doc, { all: true })).toStrictEqual(buildEntries(items, ships, { all: true }));
  });

  it("survives a round trip through the file encoding", () => {
    const parsed = JSON.parse(serializeSearchIndex(doc)) as SearchIndexDoc;
    expect(parsed._build).toBe("4.10.0.1");
    expect(loadSearchIndexEntries(parsed, { all: true })).toStrictEqual(buildEntries(items, ships, { all: true }));
  });

  it("flags furniture and templates as noise and trims empty columns", () => {
    expect(doc.items.find((r) => r[0] === "door_01")?.at(-1)).toBe(1);
    expect(doc.ships.find((r) => r[0] === "AEGS_Gladius_Template")?.at(-1)).toBe(1);
    // A named ship with maker and role, no family, not noise: four columns.
    expect(doc.ships[0]).toEqual(["AEGS_Gladius", "Aegis Gladius", doc.makers.indexOf("AEGS") + 1, doc.types.indexOf("Light Fighter") + 1]);
  });

  it("rejects a format it does not know", () => {
    expect(() => loadSearchIndexEntries({ ...doc, format: 2 })).toThrow(/format 2/);
  });

  it("writes beside its inputs, and skips quietly when one is missing", () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), "search-index-"));
    expect(writeSearchIndex(dir)).toBeUndefined();
    fs.writeFileSync(path.join(dir, "game-item-tags.json"), JSON.stringify({ _build: "4.10.0.1", items }));
    expect(writeSearchIndex(dir)).toBeUndefined();
    expect(fs.existsSync(path.join(dir, SEARCH_INDEX_FILE))).toBe(false);
    fs.writeFileSync(path.join(dir, "game-ships.json"), JSON.stringify({ vehicles: ships }));
    const written = writeSearchIndex(dir);
    expect(written).toMatchObject({ file: path.join(dir, SEARCH_INDEX_FILE), entries: 8 });
    expect(written?.bytes).toBe(fs.statSync(written!.file).size);
  });

  it("removes a stale index when an input goes missing", () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), "search-index-"));
    fs.writeFileSync(path.join(dir, "game-item-tags.json"), JSON.stringify({ _build: "4.10.0.1", items }));
    fs.writeFileSync(path.join(dir, "game-ships.json"), JSON.stringify({ vehicles: ships }));
    expect(writeSearchIndex(dir)).toBeDefined();
    fs.rmSync(path.join(dir, "game-ships.json"));
    expect(writeSearchIndex(dir)).toBeUndefined();
    expect(fs.existsSync(path.join(dir, SEARCH_INDEX_FILE))).toBe(false);
  });
});

const dataDir = path.join(import.meta.dirname, "..", "data");
const haveData = fs.existsSync(path.join(dataDir, "game-item-tags.json")) && fs.existsSync(path.join(dataDir, "game-ships.json"));

describe.skipIf(!haveData)("search index on the parsed data", () => {
  it("is equivalent to buildEntries over the real datasets", () => {
    const tags = JSON.parse(fs.readFileSync(path.join(dataDir, "game-item-tags.json"), "utf8")) as { _build?: string; items: ItemTags[] };
    const ships = JSON.parse(fs.readFileSync(path.join(dataDir, "game-ships.json"), "utf8")) as { vehicles: Parameters<typeof buildEntries>[1] };
    const doc = JSON.parse(serializeSearchIndex(buildSearchIndex(tags.items, ships.vehicles, tags._build))) as SearchIndexDoc;
    expect(loadSearchIndexEntries(doc)).toStrictEqual(buildEntries(tags.items, ships.vehicles));
    expect(loadSearchIndexEntries(doc, { all: true })).toStrictEqual(buildEntries(tags.items, ships.vehicles, { all: true }));
  });
});
