/**
 * Loot: what containers, corpses and caches roll, and with what weight.
 *
 * A loot table lists archetypes with weights; an archetype picks items by tag
 * ("any common armour piece, not a helmet") rather than naming them, with
 * secondary groups such as Rarity weighting the result. Two generations of the
 * format coexist: the original LootTable/LootArchetype and the V3 records,
 * which add choice limits, stack sizes, spawn-with rules and pool filters.
 *
 * Both are kept in the game's own shape, compacted: tag GUIDs become tag paths
 * ("Rarity/Common"), record references become names. Expanding a tag into the
 * concrete items carrying it is left to the consumer, since it needs every
 * item's tags.
 */

import { compareNames, type GameRecord } from "../records.js";
import { buildTagIndex, compactValue, type TagIndex } from "../tags.js";
import type { Domain } from "./types.js";

export interface LootRecord {
  name: string;
  recordName: string;
  /** "v1" for LootTable/LootArchetype, "v3" for the V3 records. */
  version: "v1" | "v3";
  [field: string]: unknown;
}

export function parseLootRecord(record: GameRecord, tags: TagIndex): LootRecord {
  const v3 = record.type.includes("V3");
  // V3 records wrap the content one level down (`lootTable`, `lootArchetype`).
  const body = (v3 ? (record.value.lootTable ?? record.value.lootArchetype ?? record.value) : record.value) as Record<
    string,
    unknown
  >;
  const compacted = (compactValue(body, tags) ?? {}) as Record<string, unknown>;
  return { name: record.shortName, recordName: record.name, version: v3 ? "v3" : "v1", ...compacted };
}

const TABLE_TYPES = new Set(["LootTable", "LootTableV3Record"]);
const ARCHETYPE_TYPES = new Set(["LootArchetype", "LootArchetypeV3Record"]);

export const lootDomain: Domain = {
  name: "game-loot",
  description: "Loot tables and archetypes with weights, by tag, plus pool filters and holiday loot.",
  filters: ["**/lootgeneration/**", "**/tagdatabase/**"],

  parse({ records }) {
    const tags = buildTagIndex(records.find((r) => r.type === "TagDatabase"));
    const loot = records.filter((r) => r.recordPath.includes("/lootgeneration/"));
    const byName = (a: { name: string }, b: { name: string }) => compareNames(a.name, b.name);

    const tables = loot.filter((r) => TABLE_TYPES.has(r.type)).map((r) => parseLootRecord(r, tags)).sort(byName);
    const archetypes = loot.filter((r) => ARCHETYPE_TYPES.has(r.type)).map((r) => parseLootRecord(r, tags)).sort(byName);
    const other = (dir: string) =>
      loot
        .filter((r) => r.recordPath.includes(`/lootgeneration/${dir}/`))
        .map((r) => ({ name: r.shortName, recordName: r.name, ...((compactValue(r.value, tags) ?? {}) as object) }))
        .sort(byName);

    const global = loot.find((r) => r.type === "LootGenerationGlobalParams");
    const specialEvents = (compactValue(global?.value.specialEventArchetypes, tags) as unknown[] | undefined) ?? [];

    return {
      tables,
      archetypes,
      secondaryChoices: other("secondarychoices"),
      filters: other("filters"),
      specialEvents,
      summary: {
        tables: tables.length,
        archetypes: archetypes.length,
        tags: tags.size,
      },
    };
  },
};
