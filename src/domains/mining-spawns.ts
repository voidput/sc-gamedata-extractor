/**
 * Where ores actually spawn.
 *
 * Answering "what can I mine at Hurston" means walking four record types, since
 * no single record connects a place to an ore:
 *
 *   HarvestableProviderPreset   HPP_Stanton1, one per body or belt
 *     -> HarvestableElement       a spawn entry with a relative probability
 *       -> HarvestablePreset      Mining_Common_Iron
 *         -> EntityClassDefinition  MineableRock_SurfaceCommon_Iron
 *           -> MineableComposition  CommonShipMineables_Iron
 *             -> MineableElement    iron_ore at 34.3-84.3%
 *
 * Provider presets are named after the location they cover, using internal keys
 * like `Stanton1` and `Stanton1c`. Those keys are themselves localization keys,
 * so the string table resolves them: Stanton1 is Hurston and Stanton1c is Magda,
 * one of its moons. Deriving the name from the numeric prefix instead is wrong,
 * because it labels every sub-zone with its parent planet's name.
 *
 * Keys with no entry keep the raw key rather than being guessed at.
 */

import { component } from "../entity.js";
import { lookupString, type Localization } from "../localization.js";
import { num, str, type GameRecord } from "../records.js";
import type { Domain } from "./types.js";

/**
 * "HPP_Stanton1c" -> { key: "Stanton1c", system: "Stanton", body: "Magda" }.
 *
 * The body name comes from the string table keyed by the location key itself.
 * Without it, only the system can be inferred.
 */
export function describeProvider(
  shortName: string,
  localization: Localization = new Map(),
): { key: string; system?: string; body?: string } {
  const key = shortName.replace(/^HPP_/i, "");
  const body = lookupString(localization, key);

  const match = key.match(/^([a-z]+)\d+[a-z]*$/i);
  const system = match ? match[1] : key.split("_")[0];

  return {
    key,
    system: system ? system.charAt(0).toUpperCase() + system.slice(1).toLowerCase() : undefined,
    body: body && body.length > 0 ? body : undefined,
  };
}

/** The file name a `file://...json` reference ends with, minus the suffix. */
function refFile(value: unknown): string | undefined {
  const path = str(value);
  if (!path) return undefined;
  const file = path.split("/").pop();
  return file ? file.replace(/\.json$/i, "").toLowerCase() : undefined;
}

export interface OreShare {
  element: string;
  minPercent?: number;
  maxPercent?: number;
  probability?: number;
}

export interface SpawnEntry {
  group: string;
  groupProbability?: number;
  relativeProbability?: number;
  harvestable: string;
  /** Ores in the rock this entry spawns, when the chain resolves. */
  ores?: OreShare[];
}

export interface SpawnLocation {
  key: string;
  recordName: string;
  system?: string;
  body?: string;
  entries: SpawnEntry[];
  /** Every ore reachable at this location. */
  ores: string[];
}

/**
 * Index the records each hop of the chain needs, keyed by file name because
 * references are paths rather than record names.
 */
function buildIndexes(records: GameRecord[]) {
  const byFile = new Map<string, GameRecord>();
  for (const record of records) {
    const file = record.recordPath.split("/").pop()?.toLowerCase();
    if (file) byFile.set(file, record);
  }
  return byFile;
}

/** Resolve a rock composition to the ores it contains. */
function oresOfComposition(composition: GameRecord | undefined): OreShare[] | undefined {
  if (!composition) return undefined;
  const parts = composition.value.compositionArray;
  if (!Array.isArray(parts)) return undefined;

  const ores: OreShare[] = [];
  for (const part of parts) {
    if (!part || typeof part !== "object") continue;
    const obj = part as Record<string, unknown>;
    const element = refFile(obj.mineableElement);
    if (!element) continue;
    ores.push({
      element,
      minPercent: num(obj.minPercentage),
      maxPercent: num(obj.maxPercentage),
      probability: num(obj.probability),
    });
  }
  return ores.length > 0 ? ores : undefined;
}

/** Walk harvestable preset -> mineable entity -> composition -> ores. */
function oresOfHarvestable(
  harvestableFile: string | undefined,
  byFile: Map<string, GameRecord>,
): OreShare[] | undefined {
  if (!harvestableFile) return undefined;

  const preset = byFile.get(`${harvestableFile}.json`) ?? byFile.get(harvestableFile);
  if (!preset) return undefined;

  const entityFile = refFile(preset.value.entityClass);
  const entity = entityFile ? byFile.get(entityFile) : undefined;
  if (!entity) return undefined;

  const mineable = component(entity, "MineableParams");
  const compositionFile = refFile(mineable?.composition);
  return oresOfComposition(compositionFile ? byFile.get(compositionFile) : undefined);
}

export const miningSpawnsDomain: Domain = {
  name: "game-mining-spawns",
  description: "Which ores spawn where, resolved from provider presets through to compositions.",
  filters: ["**/harvestable/**", "**/entities/mineable/**", "**/mining/**"],

  needsLocalization: true,

  parse({ records, localization }) {
    const byFile = buildIndexes(records);

    const locations: SpawnLocation[] = [];

    for (const record of records) {
      if (record.type !== "HarvestableProviderPreset") continue;

      const groups = record.value.harvestableGroups;
      if (!Array.isArray(groups)) continue;

      const entries: SpawnEntry[] = [];
      const ores = new Set<string>();

      for (const group of groups) {
        if (!group || typeof group !== "object") continue;
        const g = group as Record<string, unknown>;
        const groupName = str(g.groupName) ?? "unnamed";
        const groupProbability = num(g.groupProbability);

        const harvestables = Array.isArray(g.harvestables) ? g.harvestables : [];
        for (const entry of harvestables) {
          if (!entry || typeof entry !== "object") continue;
          const e = entry as Record<string, unknown>;
          const harvestable = refFile(e.harvestable) ?? refFile(e.harvestableEntityClass);
          if (!harvestable) continue;

          const resolved = oresOfHarvestable(harvestable, byFile);
          if (resolved) for (const ore of resolved) ores.add(ore.element);

          entries.push({
            group: groupName,
            groupProbability,
            relativeProbability: num(e.relativeProbability),
            harvestable,
            ores: resolved,
          });
        }
      }

      const { key, system, body } = describeProvider(record.shortName, localization);
      locations.push({
        key,
        recordName: record.name,
        system,
        body,
        entries,
        ores: [...ores].sort(),
      });
    }

    locations.sort((a, b) => a.key.localeCompare(b.key));

    // The inverse index is the question people actually ask: where is this ore?
    const oreLocations: Record<string, string[]> = {};
    for (const location of locations) {
      for (const ore of location.ores) {
        (oreLocations[ore] ??= []).push(location.body ?? location.key);
      }
    }
    for (const list of Object.values(oreLocations)) list.sort();

    const resolved = locations.filter((l) => l.ores.length > 0);

    return {
      locations,
      oreLocations: Object.fromEntries(Object.entries(oreLocations).sort()),
      summary: {
        locations: locations.length,
        locationsWithOres: resolved.length,
        locationsNamed: locations.filter((l) => l.body).length,
        distinctOres: Object.keys(oreLocations).length,
        spawnEntries: locations.reduce((sum, l) => sum + l.entries.length, 0),
      },
    };
  },
};
