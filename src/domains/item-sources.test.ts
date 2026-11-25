import { describe, expect, it } from "vitest";
import { contractAwards, itemSourcesDomain, loadoutItems, parseLootConfig } from "./item-sources.js";
import { itemTagsDomain } from "./item-tags.js";
import { trimItemPriceRow } from "./uex-market.js";
import type { GameRecord } from "../records.js";

const ROOT = "file://./../../../../../libs/foundry/records/";

function rec(type: string, path: string, value: Record<string, unknown>): GameRecord {
  const shortName = path.split("/").pop() as string;
  return {
    name: `${type}.${shortName}`,
    shortName,
    id: `id-${shortName}`,
    type,
    recordPath: `libs/foundry/records/${path}`,
    value: { _Type_: type, ...value },
  };
}

const tag = (id: string) => ({ _RecordName_: `Tag.${id}`, _RecordId_: id });

const tagDatabase = rec("TagDatabase", "tagdatabase/tagdatabase.tagdatabase", {
  tags: [
    {
      tagName: "Global",
      children: [
        { tagName: "LootContents", _RecordName_: "Tag.lc", children: [{ tagName: "Weapons", _RecordName_: "Tag.lcw" }] },
        { tagName: "LootSize", children: [{ tagName: "Large", _RecordName_: "Tag.lsl" }] },
        {
          tagName: "LootGeneration",
          children: [{ tagName: "CanGenerateAsLoot", _RecordName_: "Tag.can" }],
        },
        { tagName: "Weapon", children: [{ tagName: "FPS", _RecordName_: "Tag.fps" }] },
      ],
    },
  ],
});

const crate = rec("EntityClassDefinition", "entities/scitem/lootables/lootable_container_large", {
  Components: [{ _Type_: "SAttachableComponentParams", AttachDef: { Localization: { Name: "@item_crate" } } }],
});
const rack = rec("EntityClassDefinition", "entities/scitem/usables/weapon_rack_behr_lmg", {
  Components: [
    {
      _Type_: "SEntityComponentDefaultLoadoutParams",
      loadout: {
        entries: [
          { _Type_: "SItemPortLoadoutEntryParams", entityClassReference: `${ROOT}entities/scitem/weapons/fps_weapons/behr_lmg_ballistic_01.json` },
          { _Type_: "SItemPortLoadoutEntryParams", entityClassReference: null, entityClassName: "" },
        ],
      },
    },
  ],
});
const rackPreset = rec("HarvestablePreset", "harvestable/harvestablepresets/harvestablepreset_rack_behr_lmg", {
  entityClass: `${ROOT}entities/scitem/usables/weapon_rack_behr_lmg.json`,
});
const sharedConfig = rec("SubHarvestableConfigRecord", "harvestable/subharvestableconfigs/racks", {
  subConfig: {
    _Type_: "SubHarvestableConfig",
    initialSlotsProbability: 0.5,
    subHarvestables: [{ _Type_: "SubHarvestableSlot", harvestable: `${ROOT}harvestable/harvestablepresets/harvestablepreset_rack_behr_lmg.json`, relativeProbability: 1 }],
  },
});
const lootConfig = {
  _Type_: "LootConfig",
  lootTable: null,
  lootTableV3: `${ROOT}lootgeneration/loottables/v3loottable_weapons_large.json`,
  lootConstraints: {
    poolFilter: `${ROOT}lootgeneration/filters/v3poolfilter_generic.json`,
    secondaryChoices: { multiLayerRecord: `${ROOT}lootgeneration/secondarychoices/v3multichoice_military_common.json` },
    totalResultsLimit: 15,
    chanceToGenerate: 1,
  },
};
const area = rec("SubHarvestableMultiConfigRecord", "harvestable/slotpresets/v3slotpreset_loot_military", {
  multiConfig: {
    defaultConfig: null,
    taggedConfigs: [
      {
        name: "Weapons Large Common",
        tagList: { tags: [tag("lsl"), tag("lcw")] },
        subConfig: {
          _Type_: "SubHarvestableConfigSingleManual",
          subConfigManual: {
            _Type_: "SubHarvestableConfig",
            initialSlotsProbability: 1,
            subHarvestables: [
              {
                _Type_: "SubHarvestableSlot",
                harvestableEntityClass: `${ROOT}entities/scitem/lootables/lootable_container_large.json`,
                relativeProbability: 2,
                lootConfig,
              },
            ],
          },
        },
      },
      {
        name: "Weapons Rack",
        tagList: { tags: [] },
        subConfig: { _Type_: "SubHarvestableConfigSingleRef", subConfigRef: `${ROOT}harvestable/subharvestableconfigs/racks.json` },
      },
    ],
  },
});
const collector = rec("ContractGenerator", "contracts/contractgenerator/thecollector", {
  generators: [
    {
      debugName: "TheCollector_Small_Items",
      contracts: [
        {
          debugName: "TheCollector_OrbArm",
          contractResults: {
            contractResults: [
              {
                itemAwardStructure: [
                  {
                    _Type_: "ItemAwardWeightings",
                    weighting: 2,
                    awards: [{ _Type_: "ItemAwardEntityClass", amountToAward: 1, entityClass: `${ROOT}entities/scitem/characters/arms_02.json` }],
                  },
                ],
              },
            ],
          },
        },
      ],
    },
  ],
});

const localization = new Map([["item_crate", "Standard Container"]]);

describe("item-sources", () => {
  it("reads a loot config's V3 table, filter, secondary choices and limits", () => {
    expect(parseLootConfig(lootConfig)).toEqual({
      table: "v3loottable_weapons_large",
      version: "v3",
      poolFilter: "v3poolfilter_generic",
      secondaryChoices: "v3multichoice_military_common",
      maxResults: 15,
      chance: 1,
    });
    expect(parseLootConfig(null)).toBeUndefined();
  });

  it("collects a rack's fixed loadout, skipping empty ports", () => {
    expect([...loadoutItems(rack.value)]).toEqual(["behr_lmg_ballistic_01"]);
  });

  it("builds areas from tagged configs, following preset and config references", () => {
    const out = itemSourcesDomain.parse({
      records: [tagDatabase, crate, rack, rackPreset, sharedConfig, area, collector],
      localization,
    });
    const [military] = out.areas as { name: string; configs: { name: string; spawnTags: string[]; fillChance?: number; slots: Record<string, unknown>[] }[] }[];
    expect(military.name).toBe("v3slotpreset_loot_military");
    expect(military.configs[0].spawnTags).toEqual(["LootSize/Large", "LootContents/Weapons"]);
    expect(military.configs[0].slots[0]).toMatchObject({
      entity: "lootable_container_large",
      entityName: "Standard Container",
      weight: 2,
      loot: { table: "v3loottable_weapons_large" },
    });
    expect(military.configs[1]).toMatchObject({ name: "Weapons Rack", fillChance: 0.5 });
    expect(military.configs[1].slots[0]).toMatchObject({
      entity: "weapon_rack_behr_lmg",
      preset: "harvestablepreset_rack_behr_lmg",
      contains: ["behr_lmg_ballistic_01"],
    });
  });

  it("lists contract item awards with their weighting", () => {
    expect(contractAwards(collector)).toEqual([
      { generator: "TheCollector_Small_Items", contract: "TheCollector_OrbArm", item: "arms_02", amount: 1, weighting: 2 },
    ]);
  });
});

describe("item-tags", () => {
  it("resolves an item's record-level tags to paths and skips untagged records", () => {
    const gun = rec("EntityClassDefinition", "entities/scitem/weapons/fps_weapons/ksar_lmg_energy_01", {
      tags: [tag("fps"), tag("can"), tag("missing")],
      Components: [{ _Type_: "SAttachableComponentParams", AttachDef: { Type: "WeaponPersonal", Localization: { Name: "@nope" } } }],
    });
    const out = itemTagsDomain.parse({ records: [tagDatabase, gun, crate], localization });
    expect(out.items).toEqual([
      {
        name: "ksar_lmg_energy_01",
        className: "ksar_lmg_energy_01",
        id: "id-ksar_lmg_energy_01",
        type: "WeaponPersonal",
        isTemplate: true,
        nameKey: "nope",
        nameStatus: "unresolved",
        tags: ["LootGeneration/CanGenerateAsLoot", "Weapon/FPS"],
      },
    ]);
  });
});

describe("uex item prices", () => {
  it("keeps the join key and prices, drops zeroes and location ids", () => {
    expect(
      trimItemPriceRow({ item_name: "FS-9 LMG", item_uuid: "u", id_planet: 4, terminal_name: "Guns Ruin", price_buy: 7029, price_sell: 0 }),
    ).toEqual({ item_name: "FS-9 LMG", item_uuid: "u", terminal_name: "Guns Ruin", price_buy: 7029 });
  });
});
