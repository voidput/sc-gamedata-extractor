/**
 * Item tags: every item and the tag paths it carries.
 *
 * Loot never names items. A loot archetype asks for "anything tagged
 * Weapon/FPS/Stocked/LMG, rarity Common, not an event item", so answering
 * "where does X drop" means knowing X's tags. Those sit on the entity record as
 * GUIDs (`tags: [Tag.a019...]`), resolved here to paths through the tag
 * database, the same way game-loot resolves the archetypes' side.
 *
 * Only the record-level `tags` list is kept: it is what loot, law and shop
 * selectors match against. Tags on interaction points, carry states and UI
 * bindings describe behaviour, not the item, and are dropped.
 *
 * Weapons also get `ammoItems`: the magazines or canisters they load. The
 * weapon component names its ammo containers by record
 * (`SCItemWeaponComponentParams.ammoContainerRecord`, plus
 * `secondaryAmmoContainers` for tools like the Cambio SRT, which set
 * `ShouldIgnorePrimaryAmmoContainer` and refill from a canister instead). Each
 * named container is widened to every magazine-type item with the same attach
 * tags, the tag its port requires, so the empty and filled canister and any
 * other magazine that fits the same port are listed too.
 */

import { isPlaceholderName, localize, locRefs, type Localization } from "../localization.js";
import { component } from "../entity.js";
import { at, compareNames, indexByRecordPath, refPath, str, type GameRecord } from "../records.js";
import { buildTagIndex, type TagIndex } from "../tags.js";
import { attachDef, manufacturerOf } from "./item-common.js";
import type { ItemTags } from "../query/types.js";
import type { Domain } from "./types.js";

export type { ItemTags };


export function itemTagPaths(record: GameRecord, tags: TagIndex): string[] {
  const list = Array.isArray(record.value.tags) ? record.value.tags : [];
  const paths = new Set<string>();
  for (const ref of list) {
    const name = at(ref, "_RecordName_");
    if (typeof name !== "string") continue;
    const resolved = tags.get(name);
    if (resolved) paths.add(resolved);
  }
  return [...paths].sort();
}

const tagKey = (def: Record<string, unknown>): string =>
  (str(def.Tags) ?? "").split(/\s+/).filter(Boolean).sort().join(" ");

const isMagazine = (def: Record<string, unknown>) => str(def.SubType) === "Magazine";

/** Record paths (as refPath gives them) of the ammo containers a weapon names, primary first. */
export function weaponAmmoRefs(record: GameRecord): string[] {
  const weapon = component(record, "SCItemWeaponComponentParams");
  if (!weapon) return [];
  const refs: string[] = [];
  const primary = refPath(weapon.ammoContainerRecord);
  if (primary && weapon.ShouldIgnorePrimaryAmmoContainer !== true) refs.push(primary);
  for (const secondary of Array.isArray(weapon.secondaryAmmoContainers) ? weapon.secondaryAmmoContainers : []) {
    const ref = refPath(at(secondary, "ammoContainerRecord"));
    if (ref && !refs.includes(ref)) refs.push(ref);
  }
  return refs;
}

/**
 * Weapon class name -> class names of the magazines it takes. Only containers
 * that are items in their own right (records with tags, so they appear in
 * game-item-tags) are kept. Ship weapons name no container record in 4.10, so
 * this covers personal weapons and tools.
 */
export function ammoLinks(records: readonly GameRecord[], isItem: (record: GameRecord) => boolean): Map<string, string[]> {
  const byPath = indexByRecordPath(records as GameRecord[]);
  const byTagKey = new Map<string, GameRecord[]>();
  for (const record of records) {
    const def = attachDef(record);
    const key = tagKey(def);
    if (!isMagazine(def) || !key || !isItem(record)) continue;
    byTagKey.set(key, [...(byTagKey.get(key) ?? []), record]);
  }
  const links = new Map<string, string[]>();
  for (const record of records) {
    const refs = weaponAmmoRefs(record);
    if (refs.length === 0) continue;
    const ammo: string[] = [];
    const add = (r: GameRecord) => {
      if (!ammo.includes(r.shortName)) ammo.push(r.shortName);
    };
    for (const ref of refs) {
      const container = byPath.get(ref);
      if (!container || !isItem(container)) continue;
      add(container);
      const def = attachDef(container);
      const siblings = isMagazine(def) ? (byTagKey.get(tagKey(def)) ?? []) : [];
      for (const s of [...siblings].sort((a, b) => compareNames(a.shortName, b.shortName))) add(s);
    }
    if (ammo.length > 0) links.set(record.shortName, ammo);
  }
  return links;
}

export function parseItemTags(
  record: GameRecord,
  tags: TagIndex,
  localization: Localization,
): ItemTags | undefined {
  const paths = itemTagPaths(record, tags);
  if (paths.length === 0) return undefined;
  const def = attachDef(record);
  const localized = localize(localization, at(def, "Localization.Name"));
  const placeholder = isPlaceholderName(localized);
  const subType = str(def.SubType);
  return {
    name: placeholder ? record.shortName : (localized as string),
    className: record.shortName,
    id: record.id,
    type: str(def.Type),
    subType: subType === "UNDEFINED" ? undefined : subType,
    manufacturer: manufacturerOf(def),
    isTemplate: placeholder || undefined,
    ...locRefs(localization, at(def, "Localization.Name"), at(def, "Localization.Description")),
    tags: paths,
  };
}

export const itemTagsDomain: Domain = {
  name: "game-item-tags",
  description: "Every item's tag paths, the join between items and tag-driven loot.",
  filters: ["**/entities/scitem/**", "**/tagdatabase/**"],
  needsLocalization: true,

  parse({ records, localization }) {
    const tags = buildTagIndex(records.find((r) => r.type === "TagDatabase"));
    const entities = records.filter((r) => r.type === "EntityClassDefinition");
    const ammo = ammoLinks(entities, (r) => itemTagPaths(r, tags).length > 0);
    const items = entities
      .map((r) => {
        const item = parseItemTags(r, tags, localization);
        const ammoItems = item && ammo.get(item.className);
        return item && ammoItems ? { ...item, ammoItems } : item;
      })
      .filter((i): i is ItemTags => i !== undefined)
      .sort((a, b) => compareNames(a.className, b.className));

    const lootable = items.filter((i) => i.tags.includes("LootGeneration/CanGenerateAsLoot")).length;
    const barred = items.filter((i) => i.tags.some((t) => t.startsWith("LootGeneration/CannotGenerateAsLoot"))).length;
    return {
      items,
      summary: {
        items: items.length,
        withAmmo: items.filter((i) => i.ammoItems).length,
        canGenerateAsLoot: lootable, cannotGenerateAsLoot: barred,
        tags: tags.size,
      },
    };
  },
};
