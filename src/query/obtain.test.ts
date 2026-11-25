import { describe, expect, it } from "vitest";
import type { ItemTags, LootArea } from "./types.js";
import {
  ammoTargets,
  buildLootIndex,
  findItems,
  hasTag,
  lootEligibility,
  obtain,
  rankAreas,
  renderObtain,
  selects,
  spawnHits,
  tableRolls,
  type Datasets,
  type LootData,
  weaponsLabel,
} from "./obtain.js";

const item = (className: string, tags: string[], extra: Partial<ItemTags> = {}): ItemTags => ({
  name: className,
  className,
  id: `id-${className}`,
  tags,
  ...extra,
});

const COMMON = "LootGeneration/LootRarity/Common";
const RARE = "LootGeneration/LootRarity/Rare";
const CAN = "LootGeneration/CanGenerateAsLoot";

const verdict = item("ksar_lmg_energy_01", [CAN, COMMON, "Weapon/FPS/Stocked/LMG/Verdict", "Manufacturer/KastakArms"], {
  name: "Verdict LMG",
});
const fs9 = item("behr_lmg_ballistic_01", [CAN, COMMON, "Weapon/FPS/Stocked/LMG/FS-9", "Manufacturer/Behring"], {
  name: "FS-9 LMG",
});
const rareLmg = item("gmni_lmg_rare", [CAN, RARE, "Weapon/FPS/Stocked/LMG/F55", "Manufacturer/Gemini"]);
const pistol = item("ksar_pistol", [CAN, COMMON, "Weapon/FPS/Pistol/Hellion", "Manufacturer/KastakArms"]);
const promo = item("ksar_lmg_energy_01_store01", [
  "LootGeneration/CannotGenerateAsLoot/PromotionalItem",
  COMMON,
  "Weapon/FPS/Stocked/LMG/Verdict",
]);
const eventLmg = item("event_lmg", [CAN, COMMON, "Weapon/FPS/Stocked/LMG", "LootGeneration/Event/Xmas"]);
const items = [verdict, fs9, rareLmg, pistol, promo, eventLmg];

const loot: LootData = {
  tables: [
    { name: "Crate_Weapons", version: "v1", lootArchetypes: [{ archetype: "Arch_Weapons", weight: 3 }, { archetype: "Arch_Ammo", weight: 1 }] },
    {
      name: "V3_Weapons",
      version: "v3",
      lootArchetypes: [{ name: "Guns", weight: 1, archetype: { lootArchetypeRecord: "V3Arch_Guns" } }],
    },
  ],
  archetypes: [
    {
      name: "Arch_Weapons",
      version: "v1",
      excludedTags: { tags: ["LootGeneration/Event"] },
      primaryOrGroup: {
        entries: [
          { name: "LMG", tag: "Weapon/FPS/Stocked/LMG", weight: 1 },
          { name: "Pistols", tag: "Weapon/FPS/Pistol", weight: 1 },
        ],
      },
      secondaryOrGroups: [{ groupName: "Rarity", entries: [{ tag: COMMON, weight: 3 }, { tag: RARE, weight: 1 }] }],
    },
    {
      name: "Arch_Ammo",
      version: "v1",
      primaryOrGroup: {
        entries: [{ name: "LMG ammo", tag: "Weapon/FPS/Attachment/Magazine", additionalTags: { positiveTags: ["Weapon/FPS/Stocked/LMG"] }, weight: 1 }],
      },
    },
    {
      name: "V3Arch_Guns",
      version: "v3",
      entries: [{ name: "LMG", weight: 1, selector: { tags: { positiveTags: ["Weapon/FPS/Stocked/LMG"] } } }],
    },
  ],
  filters: [
    { name: "V3PoolFilter_NoEvents", filter: { tagFilter: { negativeTags: ["LootGeneration/Event"] } } },
    {
      name: "V3PoolFilter_Generic",
      filter: { filters: [{ mode: "Cumulative", filter: { filterRecord: "v3poolfilter_noevents" } }] },
    },
  ],
  secondaryChoices: [
    {
      name: "V3ManufacturerWeights_Military",
      choices: [
        { name: "BEHR", weight: 99, selector: { tags: { positiveTags: ["Manufacturer/Behring"] } } },
        { name: "KSAR", weight: 1, selector: { tags: { positiveTags: ["Manufacturer/KastakArms"] } } },
      ],
    },
    {
      name: "V3ManufacturerWeights_Even",
      choices: [{ name: "Anything", weight: 1, selector: { tags: { negativeTags: ["Weapon/FPS/Melee"] } } }],
    },
    { name: "V3Multi_Military", layers: ["v3manufacturerweights_military"] },
  ],
};

describe("tags", () => {
  it("counts a tag when the item has it or a tag below it", () => {
    expect(hasTag(["Weapon/FPS/Stocked/LMG/Verdict"], "Weapon/FPS/Stocked/LMG")).toBe(true);
    expect(hasTag(["Weapon/FPS/Stocked/LMGX"], "Weapon/FPS/Stocked/LMG")).toBe(false);
  });

  it("matches an empty selector, which is how 'anything else' choices read", () => {
    expect(selects({ positive: [], negative: ["Weapon/FPS"] }, ["Item/Food"])).toBe(true);
    expect(selects({ positive: [], negative: ["Weapon/FPS"] }, ["Weapon/FPS/Pistol"])).toBe(false);
  });

  it("reads loot eligibility and the reason an item is barred", () => {
    expect(lootEligibility(verdict.tags)).toEqual({ canLoot: true, rarity: "Common" });
    expect(lootEligibility(promo.tags)).toEqual({ canLoot: false, reason: "PromotionalItem", rarity: "Common" });
    expect(lootEligibility(["Weapon/FPS"]).canLoot).toBeUndefined();
  });
});

describe("table rolls", () => {
  const index = buildLootIndex(loot);

  it("multiplies archetype, entry and rarity shares, and sizes the pool", () => {
    const [path] = tableRolls(index, "Crate_Weapons", verdict, { pool: items });
    // archetype 3/4, entry 1/2, rarity Common 3/4.
    expect(path.share).toBeCloseTo(0.75 * 0.5 * 0.75);
    // Common LMGs not excluded: verdict and fs9. The promo is barred, the event LMG excluded, the rare one off-rarity.
    expect(path.poolSize).toBe(2);
    expect(path.perRoll).toBeCloseTo(path.share / 2);
  });

  it("does not let an ammo entry produce the gun it is for", () => {
    const paths = tableRolls(index, "Crate_Weapons", verdict);
    expect(paths.map((p) => p.archetype)).toEqual(["Arch_Weapons"]);
  });

  it("never rolls a barred, unflagged or excluded item", () => {
    expect(tableRolls(index, "Crate_Weapons", promo)).toEqual([]);
    // Random loot is opt-in: no CanGenerateAsLoot, no roll.
    expect(tableRolls(index, "Crate_Weapons", item("npc_lmg_lr01", [COMMON, "Weapon/FPS/Stocked/LMG"]))).toEqual([]);
    expect(tableRolls(index, "Crate_Weapons", eventLmg)).toEqual([]);
  });

  it("applies a slot's pool filter and manufacturer weights to a V3 table", () => {
    const filter = index.filters.get("v3poolfilter_generic");
    const layers = index.choices.get("v3multi_military");
    const kastak = tableRolls(index, "V3_Weapons", verdict, { filter, extraLayers: layers, pool: items });
    const behring = tableRolls(index, "V3_Weapons", fs9, { filter, extraLayers: layers, pool: items });
    expect(kastak[0].share).toBeCloseTo(0.01);
    expect(behring[0].share).toBeCloseTo(0.99);
    expect(tableRolls(index, "V3_Weapons", eventLmg, { filter })).toEqual([]);
  });
});

describe("spawns", () => {
  const areas: LootArea[] = [
    {
      name: "military",
      configs: [
        {
          name: "Weapons Large Common",
          spawnTags: ["LootSize/Large", "LootContents/Weapons"],
          slots: [
            { entity: "crate_large", weight: 1, loot: { table: "V3_Weapons", version: "v3", secondaryChoices: "V3Multi_Military", maxResults: 3 } },
          ],
        },
        { name: "Rack", spawnTags: [], slots: [{ entity: "rack_fs9", weight: 1, contains: ["behr_lmg_ballistic_01"] }] },
      ],
    },
    {
      name: "caves",
      configs: [
        {
          name: "Weapons Small Common",
          spawnTags: [],
          slots: [
            { entity: "crate_small", weight: 3, loot: { table: "Crate_Weapons", version: "v1", maxResults: 2 } },
            { entity: "ksar_lmg_energy_01", weight: 1 },
          ],
        },
      ],
    },
  ];
  const datasets: Datasets = { items, loot, areas };
  const index = buildLootIndex(loot);

  it("finds loot, fixed and loose spawns and ranks them", () => {
    const hits = spawnHits(datasets, index, verdict);
    const kinds = hits.map((h) => `${h.area}:${h.kind}`);
    expect(kinds).toContain("caves:loose");
    expect(kinds).toContain("caves:loot");
    expect(kinds).toContain("military:loot");
    // Loose verdict is 1 of 4 spawns; it outranks a 1% manufacturer roll.
    expect(hits[0].kind).toBe("loose");
    expect(hits[0].score).toBeCloseTo(0.25);
    expect(rankAreas(hits).map((a) => a.area)).toEqual(["caves", "military"]);
  });

  it("finds a gun that only sits on racks", () => {
    const hits = spawnHits(datasets, index, fs9).filter((h) => h.kind === "fixed");
    expect(hits).toHaveLength(1);
    expect(hits[0].entity).toBe("rack_fs9");
  });

  it("reports blueprints, awards and shops in one place", () => {
    const report = obtain(
      {
        ...datasets,
        blueprints: [{ name: "BP_FS9", produces: "behr_lmg_ballistic_01", rewardedBy: [{ mission: "Bounty" }] }],
        contractAwards: [{ generator: "TheCollector", contract: "TheCollector_FS9", item: "behr_lmg_ballistic_01" }],
        shops: [
          { item_uuid: "id-behr_lmg_ballistic_01", terminal_name: "Guns Ruin", price_buy: 7029 },
          { item_name: "FS-9 LMG", terminal_name: "No uuid row", price_buy: 1 },
          { item_uuid: "someone-else", item_name: "FS-9 LMG", terminal_name: "Wrong", price_buy: 2 },
        ],
      },
      fs9,
      index,
    );
    expect(report.blueprints).toHaveLength(1);
    expect(report.contractAwards[0].contract).toBe("TheCollector_FS9");
    expect(report.shops.map((s) => s.terminal_name)).toEqual(["Guns Ruin", "No uuid row"]);
    const text = renderObtain(report, { shopsLoaded: true });
    expect(text).toContain("Guns Ruin");
    expect(text).toContain("TheCollector_FS9");
  });

  it("places, dates and filters shop rows", () => {
    const withShops = {
      ...datasets,
      shops: [
        { item_uuid: "id-behr_lmg_ballistic_01", id_terminal: 1, terminal_name: "Guns Ruin", price_buy: 7029, date_modified: Date.UTC(2026, 10, 10) / 1000 },
        { item_uuid: "id-behr_lmg_ballistic_01", id_terminal: 2, terminal_name: "Dumper's GrimHEX", price_buy: 6900, date_modified: Date.UTC(2026, 8, 1) / 1000 },
      ],
      terminals: [
        { id: 1, nickname: "Guns Ruin", displayname: "Ruin Station", orbit_name: "Terminus", star_system_name: "Pyro" },
        { id: 2, nickname: "Dumper's GrimHEX", displayname: "Grim HEX", moon_name: "Yela", orbit_name: "Crusader", star_system_name: "Stanton" },
      ],
      shopsAsOf: "2026-11-14T00:00:00Z",
    };
    const all = obtain(withShops, fs9, index);
    expect(all.shops.map((s) => [s.location, s.updated, s.stale])).toEqual([
      ["Ruin Station, Terminus, Pyro", "2026-11-10", false],
      ["Grim HEX, Yela, Crusader, Stanton", "2026-09-01", true],
    ]);
    expect(all.at).toBeUndefined();

    const there = obtain(withShops, fs9, index, { at: "grim hex" });
    expect(there.shops.map((s) => s.terminal)).toEqual(["Dumper's GrimHEX"]);
    expect(there).toMatchObject({ at: "grim hex", shopsElsewhere: 1 });
    const text = renderObtain(there, { shopsLoaded: true });
    expect(text).toContain('shops at "grim hex"');
    expect(text).toMatch(/6,900 aUEC {2}2026-09-01 stale Dumper's GrimHEX \(Grim HEX, Yela, Crusader, Stanton\)/);

    const nowhere = renderObtain(obtain(withShops, fs9, index, { at: "orison" }), { shopsLoaded: true });
    expect(nowhere).toContain('not sold at "orison" as far as UEX knows; 2 shop row(s) elsewhere');
  });

  it("says why a barred item never drops", () => {
    const text = renderObtain(obtain(datasets, promo, index));
    expect(text).toContain("CannotGenerateAsLoot/PromotionalItem");
    expect(text).not.toContain("best areas");
  });
});

describe("findItems", () => {
  it("prefers exact matches, else needs every word", () => {
    expect(findItems(items, "verdict lmg").map((i) => i.className)).toEqual(["ksar_lmg_energy_01"]);
    expect(findItems(items, "ksar_pistol").map((i) => i.className)).toEqual(["ksar_pistol"]);
    // The manufacturer's full name comes from its tag.
    expect(findItems(items, "kastak lmg").map((i) => i.className)).toEqual(["ksar_lmg_energy_01"]);
    expect(findItems(items, "kastak").map((i) => i.className)).toEqual(["ksar_lmg_energy_01", "ksar_pistol"]);
  });
});

describe("ammo", () => {
  // The Cambio SRT names its filled canister; the empty one fits the same port. The
  // Cambio-Lite canister belongs to the multitool and must never come back.
  const canister = item("grin_tool_resource_salvage_repair_01", [], { name: "Cambio SRT Canister", subType: "Magazine" });
  const filled = item("grin_tool_resource_salvage_repair_01_filled", [], { name: "Cambio SRT Canister", subType: "Magazine" });
  const lite = item("grin_multitool_resource_salvage_repair_01", [], { name: "Cambio-Lite SRT Canister", subType: "Magazine" });
  const cambio = item("grin_salvage_repair_01", [], { name: "Cambio SRT", type: "WeaponPersonal", ammoItems: [filled.className, canister.className] });
  const skin = item("grin_salvage_repair_01_lumi01", [], { name: 'Cambio "Piconalia" SRT', type: "WeaponPersonal", ammoItems: [filled.className, canister.className] });
  const multitool = item("grin_multitool_01", [], { name: "Pyro RYT Multi-Tool", ammoItems: [lite.className] });
  const all = [canister, filled, lite, cambio, skin, multitool];

  it("resolves matched weapons to the ammo they load, skipping non-weapons", () => {
    const targets = ammoTargets(all, [cambio, skin, canister]);
    expect(targets.map((t) => t.ammo.className)).toEqual([filled.className, canister.className]);
    expect(targets[0].weapons).toEqual([cambio, skin]);
    expect(ammoTargets(all, [canister])).toEqual([]);
  });

  it("labels the report with the weapon", () => {
    expect(weaponsLabel([cambio])).toBe("Cambio SRT");
    expect(weaponsLabel([cambio, skin])).toBe("Cambio SRT (+1 variant)");
    const report = obtain(
      {
        items: all,
        shops: [{ item_uuid: canister.id, id_terminal: 2, terminal_name: "Dumper's GrimHEX", price_buy: 275 }],
        terminals: [{ id: 2, nickname: "Dumper's GrimHEX", displayname: "Grim HEX", moon_name: "Yela", star_system_name: "Stanton" }],
      },
      canister,
      undefined,
      { at: "grimhex", ammoFor: [cambio] },
    );
    expect(report.ammoFor).toEqual([{ className: cambio.className, name: "Cambio SRT" }]);
    const text = renderObtain(report, { shopsLoaded: true });
    expect(text.split("\n")[0]).toBe("ammo for Cambio SRT: Cambio SRT Canister  (grin_tool_resource_salvage_repair_01)");
    expect(text).toContain("275 aUEC");
    expect(text).not.toContain("Cambio-Lite");
  });
});
