import { describe, expect, it } from "vitest";
import { buildEntries, editDistance, fuzzyScore, parseLine, rankEntries, renderShip, toLine, typoBudget, typoScore, weaponFamily } from "./fuzzy.js";
import type { ItemTags } from "./types.js";

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
  item("behr_lmg_ballistic_01", "FS-9 LMG", ["Weapon/FPS/Stocked/LMG/FS-9", "Manufacturer/Behring"]),
  item("outlaw_arms_01", "Lynx Arms Grey", ["Manufacturer/KastakArms"], { type: "Char_Armor_Arms" }),
  item("door_01", "Door", [], { type: "Door" }),
];
const ships = [
  { name: "Aegis Gladius", recordName: "EntityClassDefinition.AEGS_Gladius_AI_Super", manufacturer: "aegs", role: "Light Fighter" },
  { name: "Aegis Gladius", recordName: "EntityClassDefinition.AEGS_Gladius", manufacturer: "aegs", role: "Light Fighter" },
  { name: "Gladius Template", recordName: "EntityClassDefinition.AEGS_Gladius_Template", isTemplate: true },
];

describe("fuzzy find", () => {
  const entries = buildEntries(items, ships);

  it("drops world furniture and templates unless asked", () => {
    expect(entries.map((e) => e.key)).not.toContain("door_01");
    expect(entries.map((e) => e.key)).not.toContain("AEGS_Gladius_Template");
    expect(buildEntries(items, ships, { all: true }).map((e) => e.key)).toContain("door_01");
  });

  it("names an unnamed gun by its weapon family", () => {
    expect(weaponFamily(["Weapon/FPS/Stocked/LMG/Verdict"])).toBe("Verdict");
    expect(weaponFamily(["Weapon/FPS/Pistol/Hellion"])).toBe("Hellion");
    expect(weaponFamily(["Weapon/FPS/Stocked/LMG"])).toBeUndefined();
    expect(entries.find((e) => e.key === "ksar_lmg_energy_01")?.name).toBe("Verdict (unnamed: ksar_lmg_energy_01)");
  });

  it("scores contiguous hits above scattered ones", () => {
    expect(fuzzyScore("lmg", "FS-9 LMG")!).toBeGreaterThan(fuzzyScore("lmg", "Lynx Arms Grey")!);
    expect(fuzzyScore("xyz", "FS-9 LMG")).toBeUndefined();
  });

  it("ranks every term, display name first, player hulls above AI ones", () => {
    expect(rankEntries(entries, "kastak lmg").slice(0, 2).map((e) => e.key).sort()).toEqual([
      "ksar_lmg_energy_01",
      "ksar_lmg_energy_01_store01",
    ]);
    expect(rankEntries(entries, "verdict").map((e) => e.key)).toContain("ksar_lmg_energy_01");
    expect(rankEntries(entries, "gladius")[0].key).toBe("AEGS_Gladius");
    expect(rankEntries(entries, "fs9")[0].key).toBe("behr_lmg_ballistic_01");
  });

  it("measures edit distance with adjacent swaps as one edit", () => {
    expect(editDistance("cambino", "cambio")).toBe(1);
    expect(editDistance("verdcit", "verdict")).toBe(1);
    expect(editDistance("kitten", "sitting")).toBe(3);
    expect(editDistance("abc", "abc")).toBe(0);
    // Bounded: gives up once past max.
    expect(editDistance("gladius", "zzzzzzz", 2)).toBe(3);
    expect(typoBudget(3)).toBe(0);
    expect(typoBudget(5)).toBe(1);
    expect(typoBudget(9)).toBe(2);
  });

  it("matches a typo against a name word, below every real hit", () => {
    const cambio = item("grin_salvage_repair_01", "Cambio SRT", ["Manufacturer/GreyCat"]);
    const lite = item("grin_multitool_resource_salvage_repair_01", "Cambio-Lite SRT Canister", ["Manufacturer/GreyCat"], { type: "WeaponAttachment" });
    const pool = buildEntries([...items, cambio, lite], []);
    expect(rankEntries(pool, "cambino srt")[0].key).toBe("grin_salvage_repair_01");
    expect(rankEntries(pool, "verdcit").map((e) => e.key)).toContain("ksar_lmg_energy_01_store01");
    // Short terms get no typo budget: "lgm" is not "lmg".
    expect(typoScore("lgm", ["lmg"])).toBeUndefined();
    expect(typoScore("cambino", ["cambio", "srt"])!).toBeLessThan(fuzzyScore("srt", "Lynx Arms Grey Cambio-Lite SRT Canister")!);
    // Too far: two edits on a six-letter word.
    expect(typoScore("cxmbxo", ["cambio"])).toBeUndefined();
  });

  it("ranks a typo of a name word above letters scattered through other entries", () => {
    const gallant = item("klwe_rifle_energy_01", "Gallant Rifle", ["Manufacturer/Klaus&Werner"]);
    // Every letter of "gallent" appears in order in the key; no word is close to it.
    const junk = item("grin_all_lens_ent_01", "Remote Turret", [], { type: "Turret" });
    const devastator = item("ksar_shotgun_energy_01", "Devastator Shotgun", ["Manufacturer/KastakArms"]);
    const stampede = item("devs_stampede_module_ator", "Stampede Module", [], { type: "Misc" });
    const pool = buildEntries([junk, stampede, ...items, gallant, devastator], []);
    expect(rankEntries(pool, "gallent")[0].key).toBe("klwe_rifle_energy_01");
    expect(rankEntries(pool, "devestator")[0].key).toBe("ksar_shotgun_energy_01");
    expect(rankEntries(pool, "rifel")[0].key).toBe("klwe_rifle_energy_01");
    // Scattered hits still match when nothing better does.
    expect(rankEntries(pool, "gllnt").map((e) => e.key)).toContain("grin_all_lens_ent_01");
    // A correctly spelled hit still beats a typo.
    expect(rankEntries(pool, "gallant")[0].key).toBe("klwe_rifle_energy_01");
  });

  it("round-trips an fzf line to its kind and key", () => {
    const line = toLine(entries[0]);
    expect(parseLine(line)).toEqual({ kind: entries[0].kind, key: entries[0].key });
    expect(parseLine("garbage")).toBeUndefined();
  });

  it("renders a ship profile", () => {
    expect(renderShip({ name: "Aegis Gladius", recordName: "EntityClassDefinition.AEGS_Gladius", role: "Light Fighter", size: 2 })).toContain(
      "role      Light Fighter",
    );
  });
});
