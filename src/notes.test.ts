import { afterAll, describe, expect, it } from "vitest";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { armourSetKey, baseClass, buildNotes, foldNewSkins, renderNotes, splitVariant, type NoteLine, type NotesDoc } from "./notes.js";

const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "sc-notes-"));
afterAll(() => fs.rmSync(tmp, { recursive: true, force: true }));

/** Write a build directory of game-*.json fixtures, each stamped with `build`. */
function writeBuild(name: string, build: string, datasets: Record<string, Record<string, unknown>>) {
  const dir = path.join(tmp, name);
  fs.mkdirSync(dir, { recursive: true });
  for (const [file, body] of Object.entries(datasets)) {
    fs.writeFileSync(path.join(dir, `game-${file}.json`), JSON.stringify({ _build: build, _extracted: build, ...body }));
  }
  return { kind: "dir" as const, path: dir };
}

const ecd = (cls: string) => `EntityClassDefinition.${cls}`;
const ship = (cls: string, name: string, extra: Record<string, unknown> = {}) => ({ recordName: ecd(cls), name, ...extra });
const gun = (cls: string, name: string, extra: Record<string, unknown> = {}) => ({ recordName: ecd(cls), name, size: 3, ...extra });
const tags = (cls: string, name: string, list: string[], extra: Record<string, unknown> = {}) => ({ className: cls, name, id: cls, tags: list, ...extra });
const SKARNOV = ["Weapon/FPS/Stocked/Rifle/Skarnov", "Weapon/FPS/Stocked/Rifle", "Weapon/FPS/Stocked"];

const before = writeBuild("a", "1.0.1", {
  ships: {
    vehicles: [
      ship("AEGS_Gladius", "Aegis Gladius", { flight: { scmSpeed: 200, maxSpeed: 1000 }, totals: { weaponDps: 1000 } }),
      ship("AEGS_Gladius_PU_AI_CIV", "Aegis Gladius", { flight: { scmSpeed: 200 } }),
    ],
  },
  weapons: {
    shipWeapons: [gun("BEHR_LaserCannon_S3", "M5A Cannon", { dps: 500 })],
    fpsWeapons: [gun("behr_rifle_ballistic_01", "P4-AR Rifle", { dps: 100 })],
    missiles: [],
  },
  "fps-weapons": { weapons: [gun("behr_rifle_ballistic_01", "P4-AR Rifle")] },
  armor: {
    armor: [
      { recordName: ecd("rsi_explorer_armor_light_arms_01_02_01"), name: "Venture Arms Starwalker", mass: 5 },
      { recordName: ecd("rsi_explorer_armor_light_arms_01_03_01"), name: "Venture Arms Infrared", mass: 5 },
    ],
    clothing: [],
  },
  blueprints: {
    blueprints: [
      {
        recordName: "CraftingBlueprintRecord.BP_CRAFT_volt_pistol_energy_01",
        name: "Pulse Laser Pistol",
        produces: "volt_pistol_energy_01",
        tiers: [{ craftTimeSeconds: 100 }],
        rewardedBy: [
          { mission: "Recover Vanduul Tech", contract: "IDS_Tech" },
          { mission: "Operative Extraction", contract: "InterSec_Nyx" },
        ],
      },
      {
        recordName: "CraftingBlueprintRecord.BP_CRAFT_volt_lmg_energy_01",
        name: "Fresnel LMG",
        produces: "volt_lmg_energy_01",
        tiers: [{ craftTimeSeconds: 300 }],
        rewardedBy: [
          { mission: "Recover Vanduul Tech", contract: "IDS_Tech" },
          { mission: "Operative Extraction", contract: "InterSec_Nyx" },
        ],
      },
    ],
    rewardPools: [],
  },
  ammo: { ammo: [{ recordName: "AmmoParams.a", speed: 1 }] },
});

const after = writeBuild("b", "1.0.2", {
  ships: {
    vehicles: [
      // +20% scm speed, +0.5% max speed (minor), weaponDps unchanged, armour newly tracked.
      ship("AEGS_Gladius", "Aegis Gladius", { flight: { scmSpeed: 240, maxSpeed: 1005 }, totals: { weaponDps: 1000, armorHealth: 900 } }),
      ship("AEGS_Gladius_PU_AI_CIV", "Aegis Gladius", { flight: { scmSpeed: 240 } }),
      ship("RSI_Galaxy", "RSI Galaxy"),
      ship("RSI_Galaxy_PU_AI_CRIM", "RSI Galaxy"),
      ship("RSI_Galaxy_AI_Template", "RSI Galaxy", { isTemplate: true }),
    ],
  },
  weapons: {
    shipWeapons: [gun("BEHR_LaserCannon_S3", "M5A Cannon", { dps: 400 }), gun("BEHR_LaserCannon_S3_mr02", "M5A Cannon Raider Camo", { dps: 400 })],
    fpsWeapons: [
      gun("behr_rifle_ballistic_01", "P4-AR Rifle", { dps: 100 }),
      gun("lbco_rifle_energy_01", "Skarnov Rifle"),
      gun("lbco_rifle_energy_01_tint01", 'Skarnov "Ferrite" Rifle'),
      gun("lbco_rifle_energy_01_mr01", 'Skarnov "Raider Camo" Rifle'),
      gun("lbco_rifle_energy_01_store01_prop", "lbco_rifle_energy_01_store01_prop", { isTemplate: true }),
    ],
    missiles: [],
  },
  // The same rifles again, as game-fps-weapons lists them, plus a knife only it has.
  "fps-weapons": {
    weapons: [
      gun("behr_rifle_ballistic_01", "P4-AR Rifle"),
      gun("lbco_rifle_energy_01", "Skarnov Rifle"),
      gun("lbco_rifle_energy_01_tint01", 'Skarnov "Ferrite" Rifle'),
      gun("banu_melee_05", "Kamba Knife", { kind: "melee" }),
    ],
  },
  armor: {
    armor: [
      { recordName: ecd("rsi_explorer_armor_light_arms_01_02_01"), name: "Venture Arms Infrared", mass: 5 },
      { recordName: ecd("rsi_explorer_armor_light_arms_01_03_01"), name: "Venture Arms Starwalker", mass: 5 },
      { recordName: ecd("kap_combat_light_arms_02_03_01"), name: "Geist Armor Arms Epoque II" },
      { recordName: ecd("kap_combat_light_core_02_03_01"), name: "Geist Armor Core Epoque II" },
      { recordName: ecd("kap_combat_light_helmet_02_03_01"), name: "Geist Armor Helmet Epoque" },
    ],
    clothing: [],
  },
  blueprints: {
    blueprints: [
      {
        // Same rewards in a new order, plus one new contract.
        recordName: "CraftingBlueprintRecord.BP_CRAFT_volt_pistol_energy_01",
        name: "Pulse Laser Pistol",
        produces: "volt_pistol_energy_01",
        tiers: [{ craftTimeSeconds: 100 }],
        rewardedBy: [
          { mission: "Living Vanduul Sample", contract: "IDS_CryoP_Blue" },
          { mission: "Operative Extraction", contract: "InterSec_Nyx" },
          { mission: "Recover Vanduul Tech", contract: "IDS_Tech" },
        ],
      },
      {
        // Reordered only: no line.
        recordName: "CraftingBlueprintRecord.BP_CRAFT_volt_lmg_energy_01",
        name: "Fresnel LMG",
        produces: "volt_lmg_energy_01",
        tiers: [{ craftTimeSeconds: 300 }],
        rewardedBy: [
          { mission: "Operative Extraction", contract: "InterSec_Nyx" },
          { mission: "Recover Vanduul Tech", contract: "IDS_Tech" },
        ],
      },
    ],
    rewardPools: [{ name: "BP_MISSIONREWARD_New", blueprints: [] }],
  },
  ammo: { ammo: [{ recordName: "AmmoParams.a", speed: 1 }, { recordName: "AmmoParams.b", speed: 2 }] },
  "item-tags": {
    items: [
      tags("lbco_rifle_energy_01", "Skarnov Rifle", SKARNOV),
      tags("lbco_rifle_energy_01_tint01", 'Skarnov "Ferrite" Rifle', SKARNOV),
      tags("lbco_rifle_energy_01_mr01", 'Skarnov "Raider Camo" Rifle', SKARNOV),
    ],
  },
});

const notes = (from = before, to = after, minChange?: number): NotesDoc => buildNotes(from, to, { minChange });
const sectionOf = (doc: NotesDoc, id: string) => doc.sections.find((s) => s.id === id);

describe("naming helpers", () => {
  it("strips variant suffixes off class names", () => {
    expect(baseClass("EntityClassDefinition.ksar_lmg_energy_01_store01_prop")).toBe("ksar_lmg_energy_01");
    expect(baseClass("lbco_rifle_energy_01_tint03")).toBe("lbco_rifle_energy_01");
    expect(baseClass("BEHR_LaserCannon_S1_mr02")).toBe("behr_lasercannon_s1");
  });
  it("splits a quoted variant out of a display name", () => {
    expect(splitVariant('Verdict "Derivation" LMG')).toEqual({ base: "Verdict LMG", variant: "Derivation" });
    expect(splitVariant("Vanduul Mauler 'STRIKER' Repeater")).toEqual({ base: "Vanduul Mauler 'STRIKER' Repeater" });
  });
  it("keys armour pieces of one set together", () => {
    expect(armourSetKey(ecd("kap_combat_light_arms_02_03_01"))).toBe(armourSetKey("kap_combat_light_helmet_02_03_01"));
  });
});

describe("buildNotes", () => {
  const doc = notes();

  it("lists player ships once and folds AI and template hulls", () => {
    const s = sectionOf(doc, "newShips");
    expect(s?.lines.map((l) => l.label)).toEqual(["RSI Galaxy"]);
    expect(s?.notes).toEqual(["+2 AI/NPC/template variants not listed"]);
  });

  it("keeps buffs and nerfs over the threshold and counts the rest", () => {
    const lines = sectionOf(doc, "shipBalance")?.lines ?? [];
    const gladius = lines.find((l) => l.label === "Aegis Gladius");
    expect(gladius?.changes).toEqual([{ stat: "scmSpeed", before: 200, after: 240, percent: 20, direction: "buff" }]);
    const cannon = lines.find((l) => l.label === "M5A Cannon");
    expect(cannon?.changes?.[0]).toMatchObject({ stat: "dps", percent: -20, direction: "nerf" });
    // maxSpeed +0.5% is minor; armorHealth exists only in the newer build.
    expect(doc.hidden.minor).toBeGreaterThanOrEqual(1);
    expect(doc.hidden.newlyTracked).toBeGreaterThanOrEqual(1);
    expect(notes(before, after, 0.1).hidden.minor).toBeLessThan(doc.hidden.minor);
  });

  it("collapses weapon variants by family, drops templates, and lists each weapon once", () => {
    const s = sectionOf(doc, "newPersonalWeapons");
    expect(s?.lines.map((l) => l.label).sort()).toEqual(["Kamba Knife", "Skarnov Rifle"]);
    const skarnov = s?.lines.find((l) => l.label === "Skarnov Rifle");
    expect(skarnov?.variants).toEqual(["Ferrite", "Raider Camo"]);
    expect(skarnov?.records).toHaveLength(3);
    expect(s?.notes).toEqual(["+1 unnamed templates and props not listed"]);
    const all = doc.sections.flatMap((x) => x.lines.flatMap((l) => l.records));
    expect(all.filter((r) => r === "lbco_rifle_energy_01")).toHaveLength(1);
  });

  it("groups armour pieces into a set", () => {
    const geist = sectionOf(doc, "armour")?.lines.find((l) => l.status === "added");
    expect(geist?.label).toBe("Geist Armor Epoque II");
    expect(geist?.detail).toBe("pieces: arms, core, helmet");
  });

  it("compares blueprint rewards as sets, so reordering is not a change", () => {
    const lines = sectionOf(doc, "crafting")?.lines ?? [];
    expect(lines).toHaveLength(1);
    expect(lines[0]).toMatchObject({ label: "Pulse Laser Pistol", detail: "now also from Living Vanduul Sample" });
    expect(sectionOf(doc, "crafting")?.notes).toEqual(["1 new mission reward pool"]);
  });

  it("reports two records trading names as one swap", () => {
    const lines = sectionOf(doc, "renames")?.lines ?? [];
    expect(lines).toHaveLength(1);
    expect(lines[0].records).toHaveLength(2);
    expect(lines[0].detail).toMatch(/swapped/);
  });

  it("says when the older build has no loot data instead of listing everything as new", () => {
    expect(sectionOf(doc, "loot")?.notes[0]).toMatch(/first build with loot data/);
  });

  it("counts datasets without a section of their own", () => {
    const line = sectionOf(doc, "other")?.lines.find((l) => l.label === "ammo");
    expect(line?.detail).toBe("ammo +1");
    expect(sectionOf(doc, "other")?.notes).toContain("item-tags: only in the newer build");
  });

  it("renders Markdown without bookkeeping fields", () => {
    const md = renderNotes(doc);
    expect(md).toMatch(/^# Patch notes: 1\.0\.1 → 1\.0\.2/);
    expect(md).toContain("## New ships");
    expect(md).toContain('**Skarnov Rifle** (+2 variants: Ferrite, Raider Camo)');
    expect(md).toContain("scmSpeed 200 → 240 (+20%)");
    expect(md).not.toMatch(/_build|_extracted|recordName/);
    expect(md).toMatch(/minor change.* under 1% hidden/);
  });
});

describe("loot changes", () => {
  const lootTags = (lootable: boolean) => ({
    items: [
      tags("volt_rifle_energy_01", "Parallax Rifle", [lootable ? "LootGeneration/CanGenerateAsLoot" : "LootGeneration/CannotGenerateAsLoot/InGameReward"]),
      tags("cbd_hat_01", "Hat", ["LootGeneration/CanGenerateAsLoot"]),
    ],
  });
  const area = (contains: string[]) => ({
    areas: [{ name: "caves/loot_caves_occupied", configs: [{ spawnTags: [], slots: [{ entity: "rack_weapons", weight: 1, contains }] }] }],
    contractAwards: [],
  });
  const a = writeBuild("loot-a", "2.0.1", { "item-tags": lootTags(true), "item-sources": area(["cbd_hat_01"]) });
  const b = writeBuild("loot-b", "2.0.2", {
    "item-tags": lootTags(false),
    "item-sources": { ...area(["volt_rifle_energy_01"]), contractAwards: [{ generator: "g", contract: "TheCollector_Rifle", item: "cbd_hat_01" }] },
  });

  it("reports loot-flag flips, fixed placements and contract awards", () => {
    const lines = sectionOf(notes(a, b), "loot")?.lines ?? [];
    const by = (category: string) => lines.filter((l) => l.category === category).map((l) => l.label);
    expect(by("No longer lootable")).toEqual(["Parallax Rifle"]);
    expect(by("Now placed in the world")).toEqual(["Parallax Rifle"]);
    expect(by("No longer placed in the world")).toEqual(["Hat"]);
    expect(lines.find((l) => l.category === "Contract awards")?.detail).toBe("awarded by TheCollector_Rifle");
  });
});

describe("notes review fixes", () => {
  const radar = (mass: number) => ({ recordName: ecd("RADR_CHCO_S02_Observer"), name: "Observer", mass });
  const harbinger = (turrets: number) => ship("AEGS_Vanguard_Harbinger", "Aegis Vanguard Harbinger", { loadout: { turrets } });
  const a = writeBuild("fix-a", "3.0.1", {
    ships: { vehicles: [harbinger(6), ship("VNCL_Mauler", "Vanduul Mauler Destroyer", { totals: { weaponDps: 100 } })] },
    weapons: { shipWeapons: [gun("VNCL_Gen2_Cannon_S5", "Vanduul Mauler 'ANNIHILATOR' Cannon", { dps: 100 })], fpsWeapons: [], missiles: [] },
    "ship-components": { radars: [radar(0)], fuelTanks: [] },
    "strings-english": { _redacted: "name keys only", strings: { vehicle_NameA: "Gladius" } },
  });
  const b = writeBuild("fix-b", "3.0.2", {
    ships: {
      vehicles: [
        harbinger(7),
        ship("VNCL_Mauler", "Vanduul Mauler Destroyer", { totals: { weaponDps: 300 } }),
        ship("VNCL_Scythe", "Vanduul Scythe"),
      ],
    },
    weapons: { shipWeapons: [gun("VNCL_Gen2_Cannon_S5", "Vanduul Mauler 'ANNIHILATOR' Cannon", { dps: 200 })], fpsWeapons: [], missiles: [] },
    "ship-components": { radars: [radar(590)], fuelTanks: [{ recordName: ecd("FUEL_Tank_S1"), name: "Internal Tank" }] },
    // Full local table: only vehicle_NameB is a published key.
    "strings-english": { strings: { vehicle_NameA: "Gladius", vehicle_NameB: "Galaxy", vehicle_DescA: "Prose.", Dlg_Line_1: "Hello" } },
    "search-index": { format: 1, items: [], ships: [] },
  });
  const doc = notes(a, b);

  it("yields no sections when a build is compared with itself, search index included", () => {
    expect(notes(b, b).sections).toEqual([]);
    expect(renderNotes(notes(b, b))).toContain("No player-facing changes.");
  });

  it("never reports the search index", () => {
    expect(JSON.stringify(doc)).not.toContain("search-index");
  });

  it("compares a redacted string table on published keys only", () => {
    const other = sectionOf(doc, "other");
    expect(other?.lines.find((l) => l.label === "strings-english")?.detail).toBe("strings +1");
    expect(other?.notes.some((n) => /redacted published one/.test(n))).toBe(true);
  });

  it("counts ship-component additions once, under their section", () => {
    expect(sectionOf(doc, "newShipItems")?.lines.map((l) => l.label)).toEqual(["Internal Tank"]);
    expect(sectionOf(doc, "other")?.lines.find((l) => l.label === "ship-components")).toBeUndefined();
  });

  it("shows a stat filled in from 0 without a verdict", () => {
    const observer = sectionOf(doc, "shipBalance")?.lines.find((l) => l.label === "Observer");
    expect(observer?.changes).toEqual([{ stat: "mass", before: 0, after: 590, direction: "changed" }]);
    expect(renderNotes(doc)).toContain("- **Observer**: mass 0 → 590");
  });

  it("only counts Vanduul hulls and weapons", () => {
    const records = doc.sections.flatMap((s) => s.lines.flatMap((l) => l.records));
    expect(records.filter((r) => r.startsWith("VNCL_"))).toEqual([]);
    expect(sectionOf(doc, "newShips")?.notes).toEqual(["+1 AI/NPC/template variants not listed"]);
    expect(sectionOf(doc, "shipBalance")?.notes).toContain("+2 Vanduul (NPC-only) hulls and weapons with stat changes not listed");
  });

  it("counts tracked records that changed outside the tracked stats", () => {
    expect(sectionOf(doc, "shipBalance")?.notes[0]).toMatch(/changed only outside the tracked stats.*ships ×1/);
    expect(doc.hidden.untracked).toBe(1);
    expect(renderNotes(doc)).toContain("1 record changed only outside the tracked stats");
  });
});

describe("foldNewSkins", () => {
  const before = [
    { name: "M3A Cannon", recordName: "EntityClassDefinition.BEHR_LaserCannon_S1", size: 1, dps: 303.75 },
    { name: "M4A Cannon", recordName: "EntityClassDefinition.BEHR_LaserCannon_S2", size: 2, dps: 455.63 },
    { name: "Coda Pistol", recordName: "EntityClassDefinition.ksar_pistol_ballistic_01" },
  ];
  const added = (label: string, record: string, extra: Partial<NoteLine> = {}): NoteLine => ({
    label,
    status: "added",
    category: "shipWeapons",
    records: [record],
    ...extra,
  });

  it("folds one skin added across sizes of existing guns into one line", () => {
    const after = [
      { name: "M3A Cannon Raider Camo", recordName: "EntityClassDefinition.BEHR_LaserCannon_S1_mr02", size: 1, dps: 303.75 },
      { name: "M4A Cannon Raider Camo", recordName: "EntityClassDefinition.BEHR_LaserCannon_S2_mr02", size: 2, dps: 455.63 },
    ];
    const lines = foldNewSkins(
      [added("M3A Cannon Raider Camo", "BEHR_LaserCannon_S1_mr02"), added("M4A Cannon Raider Camo", "BEHR_LaserCannon_S2_mr02")],
      before,
      after,
    );
    expect(lines).toEqual([
      {
        label: "Raider Camo skin",
        status: "added",
        category: "shipWeapons",
        records: ["BEHR_LaserCannon_S1_mr02", "BEHR_LaserCannon_S2_mr02"],
        detail: "for M3A Cannon (S1), M4A Cannon (S2); paint only, same stats as the base gun",
      },
    ]);
    // A "skin" that changes stats is called out, not passed off as paint.
    const buffed = foldNewSkins([added("M3A Cannon Raider Camo", "BEHR_LaserCannon_S1_mr02")], before, [{ ...after[0], dps: 400 }]);
    expect(buffed[0].detail).toContain("stats differ from the base gun");
    expect(renderNotes({
      from: { source: "a" },
      to: { source: "b" },
      sections: [{ id: "newShipItems", title: "New ship weapons & components", lines, notes: [] }],
      hidden: { minor: 0, untracked: 0 },
    } as unknown as NotesDoc)).toContain("- **Raider Camo skin** — for M3A Cannon (S1), M4A Cannon (S2); paint only");
  });

  it("reads a quoted skin name, and leaves new guns and removals alone", () => {
    const lines = foldNewSkins(
      [
        added('Coda "Piconalia" Pistol', "ksar_pistol_ballistic_01_lumi01", { category: undefined }),
        added("Skarnov Rifle", "new_rifle_01", { variants: ["Ferrite"] }),
        { ...added("M3A Cannon Raider Camo", "BEHR_LaserCannon_S1_mr02"), status: "removed" },
      ],
      before,
    );
    expect(lines.map((l) => l.label)).toEqual(["Skarnov Rifle", "M3A Cannon Raider Camo", "Piconalia skin"]);
    // No new record to compare against: never claim "paint only" without checking.
    expect(lines[2].detail).toBe("for Coda Pistol; stats differ from the base gun, see `diff`");
  });
});
