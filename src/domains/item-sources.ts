/**
 * Where items spawn and what hands them out: loot areas and contract awards.
 *
 * Loot reaches the world through harvestables. A location's object container
 * holds spawn points (HarvestableTagPoint) tagged with what they want, e.g.
 * `LootSize/Small + LootRarity/Common + LootContents/Weapons`. A slot preset
 * (harvestable/slotpresets) is the rulebook for one kind of area, a cave, a
 * distribution centre wing, a contested zone: for each tag combination it lists
 * what may spawn there, by relative weight. A slot is one of
 *
 *   - a container (`lootable_container_weapons_small`) with a LootConfig: the
 *     loot table it rolls, plus for V3 tables a pool filter and secondary
 *     choices (rarity, faction, manufacturer weights) that narrow the roll;
 *   - a harvestable preset whose entity has a fixed loadout, such as a weapon
 *     rack that always holds one particular rifle;
 *   - a loose item, the entity itself.
 *
 * Which concrete locations use which preset lives in the object containers,
 * not DataCore, so areas are named by their preset; the names are descriptive
 * (`caves/loot_caves_occupied_rock_stanton`, `distributioncentre_multiconfig_
 * lobby_high`).
 *
 * Contract awards are items a contract hands over on completion
 * (`ItemAwardEntityClass`), which is how Wikelo's rewards are stored.
 */

import { isPlaceholderName, localize, type Localization } from "../localization.js";
import { at, compareNames, indexByRecordPath, num, refPath, str, type GameRecord } from "../records.js";
import { buildTagIndex, type TagIndex } from "../tags.js";
import { attachDef } from "./item-common.js";
import type { ContractAward, LootArea, SpawnConfig, SpawnLoot, SpawnSlot } from "../query/types.js";
import type { Domain } from "./types.js";

export type { ContractAward, LootArea, SpawnConfig, SpawnLoot, SpawnSlot };

type Obj = Record<string, unknown>;

const SLOT_PRESETS = "harvestable/slotpresets/";

function recordNameOf(ref: unknown): string | undefined {
  const path = refPath(ref);
  return path ? path.split("/").pop() : undefined;
}

function tagPaths(list: unknown, tags: TagIndex): string[] {
  const out: string[] = [];
  for (const ref of Array.isArray(list) ? list : []) {
    const name = at(ref, "_RecordName_");
    const resolved = typeof name === "string" ? tags.get(name) : undefined;
    if (resolved) out.push(resolved);
  }
  return out;
}

/** Item class names an entity's default loadout holds, at any depth. */
export function loadoutItems(value: unknown, found = new Set<string>()): Set<string> {
  if (Array.isArray(value)) {
    for (const v of value) loadoutItems(v, found);
    return found;
  }
  if (!value || typeof value !== "object") return found;
  const obj = value as Obj;
  if (obj._Type_ === "SItemPortLoadoutEntryParams") {
    const byRef = recordNameOf(obj.entityClassReference);
    const byName = str(obj.entityClassName);
    if (byRef) found.add(byRef);
    else if (byName) found.add(byName.toLowerCase());
  }
  for (const v of Object.values(obj)) loadoutItems(v, found);
  return found;
}

export function parseLootConfig(value: unknown): SpawnLoot | undefined {
  if (!value || typeof value !== "object") return undefined;
  const config = value as Obj;
  const v3 = recordNameOf(config.lootTableV3);
  const v1 = recordNameOf(config.lootTable);
  const table = v3 ?? v1;
  if (!table) return undefined;
  const constraints = (config.lootConstraints ?? {}) as Obj;
  const choices = constraints.secondaryChoices as Obj | undefined;
  return {
    table,
    version: v3 ? "v3" : "v1",
    poolFilter: recordNameOf(constraints.poolFilter),
    secondaryChoices: recordNameOf(choices?.multiLayerRecord ?? choices?.singleLayerRecord),
    maxResults: num(constraints.totalResultsLimit),
    chance: num(constraints.chanceToGenerate),
  };
}

interface Context {
  byPath: Map<string, GameRecord>;
  tags: TagIndex;
  localization: Localization;
}

function entityDisplayName(record: GameRecord | undefined, localization: Localization): string | undefined {
  if (!record) return undefined;
  const name = localize(localization, at(attachDef(record), "Localization.Name"));
  return isPlaceholderName(name) ? undefined : name;
}

export function parseSlot(slot: Obj, ctx: Context): SpawnSlot | undefined {
  const presetPath = refPath(slot.harvestable);
  const preset = presetPath ? ctx.byPath.get(presetPath) : undefined;
  const entityPath = refPath(slot.harvestableEntityClass) ?? refPath(preset?.value.entityClass);
  if (!entityPath) return undefined;
  const entityRecord = ctx.byPath.get(entityPath);
  const contains = entityRecord ? [...loadoutItems(entityRecord.value)].sort() : [];
  return {
    entity: entityPath.split("/").pop() as string,
    entityName: entityDisplayName(entityRecord, ctx.localization),
    preset: presetPath?.split("/").pop(),
    weight: num(slot.relativeProbability) ?? 1,
    loot: parseLootConfig(slot.lootConfig),
    contains: contains.length > 0 ? contains : undefined,
  };
}

/** Follow manual, by-reference and record wrappers down to SubHarvestableConfig nodes. */
function subConfigs(node: unknown, ctx: Context, seen = new Set<string>()): Obj[] {
  if (!node || typeof node !== "object") return [];
  const obj = node as Obj;
  if (obj._Type_ === "SubHarvestableConfig") return [obj];
  if (obj.subConfigManual) return subConfigs(obj.subConfigManual, ctx, seen);
  if (obj.subConfig) return subConfigs(obj.subConfig, ctx, seen);
  const ref = refPath(obj.subConfigRef);
  if (ref && !seen.has(ref)) {
    seen.add(ref);
    return subConfigs(ctx.byPath.get(ref)?.value, ctx, seen);
  }
  return [];
}

function parseConfig(name: string | undefined, spawnTags: string[], node: unknown, ctx: Context): SpawnConfig | undefined {
  const configs = subConfigs(node, ctx);
  if (configs.length === 0) return undefined;
  const slots = configs
    .flatMap((c) => (Array.isArray(c.subHarvestables) ? c.subHarvestables : []))
    .filter((s): s is Obj => !!s && typeof s === "object")
    .map((s) => parseSlot(s, ctx))
    .filter((s): s is SpawnSlot => s !== undefined);
  if (slots.length === 0) return undefined;
  const fill = num(configs[0].initialSlotsProbability);
  return { name, spawnTags, fillChance: fill === 1 ? undefined : fill, slots };
}

export function parseArea(record: GameRecord, ctx: Context): LootArea | undefined {
  const path = refPath(record.recordPath) ?? "";
  const name = path.startsWith(SLOT_PRESETS) ? path.slice(SLOT_PRESETS.length) : record.shortName;
  const configs: SpawnConfig[] = [];
  const multi = record.value.multiConfig as Obj | undefined;
  if (multi) {
    const fallback = parseConfig("default", [], multi.defaultConfig, ctx);
    if (fallback) configs.push(fallback);
    for (const tagged of Array.isArray(multi.taggedConfigs) ? (multi.taggedConfigs as Obj[]) : []) {
      if (!tagged) continue;
      const config = parseConfig(str(tagged.name), tagPaths(at(tagged, "tagList.tags"), ctx.tags), tagged.subConfig, ctx);
      if (config) configs.push(config);
    }
  } else {
    const single = parseConfig(undefined, [], record.value.subConfig, ctx);
    if (single) configs.push(single);
  }
  return configs.length > 0 ? { name, configs } : undefined;
}

/** Every item a contract generator record awards, with the contract that awards it. */
export function contractAwards(record: GameRecord): ContractAward[] {
  const out: ContractAward[] = [];
  const generators = Array.isArray(record.value.generators) ? (record.value.generators as Obj[]) : [];
  for (const generator of generators) {
    if (!generator) continue;
    const generatorName = str(generator.debugName) ?? record.shortName;
    for (const contract of Array.isArray(generator.contracts) ? (generator.contracts as Obj[]) : []) {
      if (!contract) continue;
      const contractName = str(contract.debugName) ?? generatorName;
      const walk = (value: unknown, weighting?: number) => {
        if (Array.isArray(value)) return value.forEach((v) => walk(v, weighting));
        if (!value || typeof value !== "object") return;
        const obj = value as Obj;
        if (obj._Type_ === "ItemAwardWeightings") {
          const w = num(obj.weighting);
          for (const v of Object.values(obj)) walk(v, w);
          return;
        }
        if (obj._Type_ === "ItemAwardEntityClass") {
          const item = recordNameOf(obj.entityClass);
          if (item) {
            out.push({
              generator: generatorName,
              contract: contractName,
              item,
              amount: num(obj.amountToAward),
              weighting,
            });
          }
          return;
        }
        for (const v of Object.values(obj)) walk(v, weighting);
      };
      walk(contract.contractResults);
    }
  }
  return out;
}

export const itemSourcesDomain: Domain = {
  name: "game-item-sources",
  description: "Loot spawn areas (spawn-point tags -> containers, racks, loose items, loot tables) and contract item awards.",
  filters: [
    "**/harvestable/**",
    "**/entities/scitem/**",
    "**/contracts/contractgenerator/**",
    "**/tagdatabase/**",
  ],
  needsLocalization: true,

  parse({ records, localization }) {
    const tags = buildTagIndex(records.find((r) => r.type === "TagDatabase"));
    const ctx: Context = { byPath: indexByRecordPath(records), tags, localization };

    const areas = records
      .filter((r) => (refPath(r.recordPath) ?? "").startsWith(SLOT_PRESETS))
      .map((r) => parseArea(r, ctx))
      .filter((a): a is LootArea => a !== undefined)
      .sort((a, b) => compareNames(a.name, b.name));

    const awards = records
      .filter((r) => (refPath(r.recordPath) ?? "").startsWith("contracts/contractgenerator/"))
      .flatMap(contractAwards)
      .sort((a, b) => compareNames(a.item, b.item) || compareNames(a.contract, b.contract));

    const slots = areas.flatMap((a) => a.configs.flatMap((c) => c.slots));
    return {
      areas,
      contractAwards: awards,
      summary: {
        areas: areas.length,
        configs: areas.reduce((n, a) => n + a.configs.length, 0),
        slots: slots.length,
        lootSlots: slots.filter((s) => s.loot).length,
        fixedSlots: slots.filter((s) => s.contains).length,
        contractAwards: awards.length,
      },
    };
  },
};
