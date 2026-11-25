import { describe, expect, it } from "vitest";
import { ammoLinks, itemTagsDomain, weaponAmmoRefs } from "./item-tags.js";
import type { GameRecord } from "../records.js";

const ROOT = "file://./../../../../../../../libs/foundry/records/";
const MAGS = "entities/scitem/weapons/magazines/";

function rec(path: string, value: Record<string, unknown>): GameRecord {
  const shortName = path.split("/").pop() as string;
  return {
    name: `EntityClassDefinition.${shortName}`,
    shortName,
    id: `id-${shortName}`,
    type: "EntityClassDefinition",
    recordPath: `libs/foundry/records/${path}`,
    value: { _Type_: "EntityClassDefinition", tags: [{ _RecordName_: "Tag.item" }], ...value },
  };
}

const attach = (def: Record<string, unknown>) => ({ _Type_: "SAttachableComponentParams", AttachDef: def });
const mag = (name: string, tags: string, extra: Record<string, unknown> = {}) =>
  rec(`${MAGS}${name}`, { Components: [attach({ Type: "WeaponAttachment", SubType: "Magazine", Tags: tags, ...extra })] });

// Shapes as in 4.10.200: the Cambio SRT ignores its internal battery and
// refills from a canister; the P4-AR names one magazine, and the CQ7 magazine
// carries the same port tag.
const battery = mag("grin_tool_01_mag", "grin_salvage_repair_01");
const canister = mag("grin_tool_resource_salvage_repair_01", "grin_tool_salvage_01_canister");
const filled = mag("grin_tool_resource_salvage_repair_01_filled", "grin_tool_salvage_01_canister");
const lite = mag("grin_multitool_resource_salvage_repair_01", "grin_multitool_01_canister");
const p4Mag = mag("behr_rifle_ballistic_01_mag", "behr_rifle_ballistic_01");
const cq7Mag = mag("behr_rifle_ballistic_03_mag", "behr_rifle_ballistic_01");
const untagged = { ...mag("internal_box", ""), value: { _Type_: "EntityClassDefinition", Components: [] } };

const weapon = (name: string, params: Record<string, unknown>) =>
  rec(`entities/scitem/weapons/fps_weapons/${name}`, {
    Components: [attach({ Type: "WeaponPersonal" }), { _Type_: "SCItemWeaponComponentParams", ...params }],
  });

const cambio = weapon("grin_salvage_repair_01", {
  ammoContainerRecord: `${ROOT}${MAGS}grin_tool_01_mag.json`,
  ShouldIgnorePrimaryAmmoContainer: true,
  secondaryAmmoContainers: [
    { _Type_: "SSecondaryAmmoContainerParams", ammoContainerRecord: `${ROOT}${MAGS}grin_tool_resource_salvage_repair_01_filled.json` },
  ],
});
const p4 = weapon("behr_rifle_ballistic_01", { ammoContainerRecord: `${ROOT}${MAGS}behr_rifle_ballistic_01_mag.json` });
const shipGun = weapon("ship_gun", { ammoContainerRecord: `${ROOT}${MAGS}internal_box.json` });

const records = [battery, canister, filled, lite, p4Mag, cq7Mag, untagged, cambio, p4, shipGun];
const isItem = (r: GameRecord) => Array.isArray(r.value.tags) && r.value.tags.length > 0;

describe("weapon ammo links", () => {
  it("reads the containers a weapon names, dropping an ignored primary", () => {
    expect(weaponAmmoRefs(cambio)).toEqual([`${MAGS}grin_tool_resource_salvage_repair_01_filled`]);
    expect(weaponAmmoRefs(p4)).toEqual([`${MAGS}behr_rifle_ballistic_01_mag`]);
    expect(weaponAmmoRefs(canister)).toEqual([]);
  });

  it("widens each container to the magazines with the same port tag", () => {
    const links = ammoLinks(records, isItem);
    expect(links.get("grin_salvage_repair_01")).toEqual([
      "grin_tool_resource_salvage_repair_01_filled",
      "grin_tool_resource_salvage_repair_01",
    ]);
    expect(links.get("behr_rifle_ballistic_01")).toEqual(["behr_rifle_ballistic_01_mag", "behr_rifle_ballistic_03_mag"]);
    // Containers that are not items (no tags) are not linked.
    expect(links.has("ship_gun")).toBe(false);
  });

  it("writes ammoItems on the weapon's item-tags row", () => {
    const tagDatabase: GameRecord = {
      name: "TagDatabase.tagdatabase",
      shortName: "tagdatabase",
      id: "db",
      type: "TagDatabase",
      recordPath: "libs/foundry/records/tagdatabase/tagdatabase.tagdatabase",
      value: { _Type_: "TagDatabase", tags: [{ tagName: "Global", children: [{ tagName: "Item", _RecordName_: "Tag.item" }] }] },
    };
    const out = itemTagsDomain.parse({ records: [tagDatabase, ...records], localization: new Map() }) as {
      items: { className: string; ammoItems?: string[] }[];
      summary: { withAmmo: number };
    };
    expect(out.items.find((i) => i.className === "grin_salvage_repair_01")?.ammoItems).toHaveLength(2);
    expect(out.items.find((i) => i.className === "grin_tool_resource_salvage_repair_01")?.ammoItems).toBeUndefined();
    expect(out.summary.withAmmo).toBe(2);
  });
});
