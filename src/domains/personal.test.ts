import { describe, it, expect } from "vitest";
import { armorDomain, containerMicroScu, slotOf } from "./armor.js";
import { consumablesDomain } from "./consumables.js";
import { lawDomain, setParameters } from "./law.js";
import { lootDomain } from "./loot.js";
import { buildTagIndex, compactValue } from "../tags.js";
import { refPath, type GameRecord } from "../records.js";

const empty = new Map<string, string>();

function rec(type: string, recordPath: string, value: Record<string, unknown>, shortName?: string): GameRecord {
  const name = shortName ?? recordPath.split("/").pop()!;
  return {
    name: `${type}.${name}`,
    shortName: name,
    id: `id-${name}`,
    type,
    recordPath: `libs/foundry/records/${recordPath}`,
    value: { _Type_: type, ...value },
  };
}

const fileRef = (path: string) => `file://./../../../../libs/foundry/records/${path}.json`;
const tagRef = (guid: string) => ({ _RecordName_: `Tag.${guid}`, _RecordId_: guid });

const tagDatabase = rec(
  "TagDatabase",
  "tagdatabase/tagdatabase.tagdatabase",
  {
    tags: [
      {
        _RecordName_: "Tag.root",
        tagName: "Global",
        children: [
          {
            _RecordName_: "Tag.rarity",
            tagName: "Rarity",
            children: [{ _RecordName_: "Tag.common", tagName: "Common", children: [] }, null],
          },
        ],
      },
    ],
  },
  "TagDatabase",
);

describe("tags", () => {
  it("names tags by their path below Global", () => {
    const tags = buildTagIndex(tagDatabase);
    expect(tags.get("Tag.common")).toBe("Rarity/Common");
    expect(tags.has("Tag.root")).toBe(false);
  });

  it("compacts a record into data: tag paths, file names, no ids or empties", () => {
    const tags = buildTagIndex(tagDatabase);
    expect(
      compactValue(
        {
          _Type_: "X",
          archetype: fileRef("lootgeneration/lootarchetypes/lootarchetype_armorcache_rare"),
          tag: tagRef("common"),
          resource: { _RecordName_: "ResourceType.Gold", _RecordId_: "g" },
          weight: 0,
          empty: [],
          missing: null,
        },
        tags,
      ),
    ).toEqual({ archetype: "lootarchetype_armorcache_rare", tag: "Rarity/Common", resource: "Gold", weight: 0 });
  });
});

describe("armor", () => {
  it("resolves references by their path below the records root", () => {
    expect(refPath(fileRef("damage/lightarmor"))).toBe("damage/lightarmor");
    expect(refPath("libs/foundry/records/Damage/LightArmor")).toBe("damage/lightarmor");
  });

  it("names slots and reads container capacity in micro-SCU", () => {
    expect(slotOf("Char_Armor_Torso")).toBe("Core");
    expect(slotOf("Char_Clothing_Torso_1")).toBe("Torso_1");
    expect(containerMicroScu(rec("InventoryContainer", "c", { inventoryType: { capacity: { microSCU: 9000 } } }))).toBe(9000);
  });

  it("joins a piece to its resistance profile and storage", () => {
    const core = rec("EntityClassDefinition", "entities/scitem/characters/human/armor/pu_armor/light/core/x_core", {
      Components: [
        {
          _Type_: "SAttachableComponentParams",
          AttachDef: { Type: "Char_Armor_Torso", SubType: "Light", Grade: 1, Localization: { Name: "@x_core" } },
        },
        {
          _Type_: "SCItemClothingParams",
          TemperatureResistance: { MinResistance: -38, MaxResistance: 68 },
          RadiationResistance: { MaximumRadiationCapacity: 26000, RadiationDissipationRate: 145.8 },
        },
        { _Type_: "SCItemInventoryContainerComponentParams", containerParams: fileRef("inventorycontainers/armor/light/c") },
        {
          _Type_: "SCItemSuitArmorParams",
          damageResistance: fileRef("damage/lightarmor"),
          signatureParams: [{ signatureType: "Electromagnetic", signatureEmission: 2 }],
        },
      ],
    });
    const out = armorDomain.parse({
      records: [
        core,
        rec(
          "DamageResistanceMacro",
          "damage/lightarmor",
          {
            damageResistance: {
              PhysicalResistance: { Multiplier: 0.8, Threshold: 0, DamageCap: 0 },
              StunResistance: { Multiplier: 0.7, Threshold: 0, DamageCap: 0 },
            },
            impactForceResistance: { impactForceResistance: 0.9 },
          },
          "LightArmor",
        ),
        rec("InventoryContainer", "inventorycontainers/armor/light/c", { inventoryType: { capacity: { microSCU: 9000 } } }),
      ],
      localization: new Map([["x_core", "Explorer Core"]]),
    }) as { armor: Record<string, unknown>[]; clothing: unknown[] };

    expect(out.clothing).toEqual([]);
    expect(out.armor[0]).toMatchObject({
      name: "Explorer Core",
      slot: "Core",
      weight: "Light",
      temperatureMin: -38,
      temperatureMax: 68,
      storageMicroScu: 9000,
      resistanceProfile: "LightArmor",
      resistances: { physical: { multiplier: 0.8 }, stun: { multiplier: 0.7 } },
      impactForceResistance: 0.9,
      emSignature: 2,
    });
  });
});

describe("law", () => {
  it("drops -1, which means inherit, and reads int flags as booleans", () => {
    expect(
      setParameters({ _Type_: "InfractionParameters", isFelony: 1, lifetime: 168, gracePeriod: -1, hideCrimeInJournal: 0, graceWarnings: [] }),
    ).toEqual({ isFelony: true, lifetime: 168, hideCrimeInJournal: false });
    expect(setParameters({ isFelony: -1 })).toBeUndefined();
  });

  it("names enforced crimes as the crime list does and keeps impound rules", () => {
    const out = lawDomain.parse({
      records: [
        rec("InfractionDefinition", "lawsystem/infractiondefinitions/felonies/murder", {
          triggers: ["KillActor"],
          defaultParameters: { isFelony: 1, felonyMerits: 5000, lifetime: 168 },
        }, "Murder"),
        rec("Jurisdiction", "lawsystem/jurisdictions/uee", {
          baseFine: 125,
          infractions: [{ definition: fileRef("lawsystem/infractiondefinitions/felonies/murder"), parameterOverrides: { lifetime: -1 } }],
          prohibitedResources: [{ _RecordName_: "ResourceType.OsoianHides" }],
          controlledSubstanceClasses: [{ commodities: [fileRef("entities/commodities/vice/slam")], resources: [], maxPossessionSCU: 0.125 }],
          impoundingDefinitions: [{ trigger: "PadRamming", impoundingTimeSeconds: 3600, impoundingFineUEC: 50000 }],
        }, "UEE"),
      ],
      localization: empty,
    }) as { crimes: Record<string, unknown>[]; jurisdictions: Record<string, unknown>[] };

    expect(out.crimes[0]).toMatchObject({ category: "felonies", parameters: { isFelony: true, felonyMerits: 5000 } });
    expect(out.jurisdictions[0]).toMatchObject({
      baseFine: 125,
      crimes: [{ crime: "Murder", overrides: undefined }],
      prohibitedResources: ["OsoianHides"],
      controlledSubstanceClasses: [{ commodities: ["slam"], resources: [], maxPossessionScu: 0.125 }],
      impounding: [{ trigger: "PadRamming", seconds: 3600, fine: 50000 }],
    });
  });
});

describe("loot", () => {
  it("unwraps V3 records and resolves tags in both generations", () => {
    const out = lootDomain.parse({
      records: [
        tagDatabase,
        rec("LootTable", "lootgeneration/loottables/armor", {
          lootArchetypes: [{ archetype: fileRef("lootgeneration/lootarchetypes/a"), weight: 0.4 }],
        }),
        rec("LootTableV3Record", "lootgeneration/loottables/v3loottable_medical", {
          lootTable: { lootArchetypes: [{ name: "Medpen", weight: 100, optionalData: [{ choiceLimit: 5 }] }] },
        }),
        rec("LootArchetype", "lootgeneration/lootarchetypes/a", {
          primaryOrGroup: { entries: [{ name: "Any common", tag: tagRef("common"), weight: 1 }] },
        }),
      ],
      localization: empty,
    }) as { tables: Record<string, unknown>[]; archetypes: Record<string, unknown>[] };

    expect(out.tables).toEqual([
      { name: "armor", recordName: "LootTable.armor", version: "v1", lootArchetypes: [{ archetype: "a", weight: 0.4 }] },
      {
        name: "v3loottable_medical",
        recordName: "LootTableV3Record.v3loottable_medical",
        version: "v3",
        lootArchetypes: [{ name: "Medpen", weight: 100, optionalData: [{ choiceLimit: 5 }] }],
      },
    ]);
    expect(out.archetypes[0]).toMatchObject({ primaryOrGroup: { entries: [{ tag: "Rarity/Common" }] } });
  });
});

describe("consumables", () => {
  it("totals effects from per-micro-SCU rates, volume and ratio", () => {
    const database = rec("ConsumableTypeDatabase", "consumabletypesdatabase/consumabletypes", {
      types: [
        {
          typeName: "Drug",
          subtypes: [
            {
              _RecordName_: "ConsumableSubtype.coag",
              typeName: "Coagulant",
              effectsPerMicroSCU: [
                { _Type_: "ConsumableEffectHealth", healthChange: 3.75 },
                { _Type_: "ConsumableEffectModifyActorStatus", statType: "BloodDrugLevel", statPointChange: 1 },
                {
                  _Type_: "ConsumableEffectAddBuffEffect",
                  buffType: "Energizing",
                  buffDurationOverride: { durationOverride: 1200 },
                  buffValueOverride: null,
                },
              ],
            },
          ],
        },
      ],
    }, "ConsumableTypes");
    const pen = rec("EntityClassDefinition", "entities/scitem/consumables/crlf_consumable_healing_01", {
      Components: [
        { _Type_: "SAttachableComponentParams", AttachDef: { Type: "FPS_Consumable", SubType: "Medical", Localization: { Name: "@pen" } } },
        {
          _Type_: "SCItemConsumableParams",
          consumableVolume: { microSCU: 30 },
          defaultContents: [{ consumableSubtype: { _RecordName_: "ConsumableSubtype.coag" }, ratio: 1 }],
          oneShotConsume: true,
        },
      ],
    });
    const out = consumablesDomain.parse({ records: [database, pen], localization: new Map([["pen", "MedPen"]]) }) as {
      consumables: Record<string, unknown>[];
      byContentType: Record<string, number>;
    };

    expect(out.consumables[0]).toMatchObject({
      name: "MedPen",
      volumeMicroScu: 30,
      contents: [{ subtype: "Coagulant", type: "Drug", ratio: 1 }],
      health: 112.5,
      stats: { BloodDrugLevel: 30 },
      buffs: [{ buff: "Energizing", duration: 1200, value: undefined }],
      oneShot: true,
    });
    expect(out.byContentType).toEqual({ Drug: 1 });
  });
});
