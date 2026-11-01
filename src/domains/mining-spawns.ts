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
 *
 * The rock entity also carries what a scanner shows: its RS (resource)
 * signature is slot 4 of the radar `signatures` array (legendary Quantainium
 * reads 3170, as in game). The composition name carries the deposit tier
 * (Common .. Legendary) for the tiered ship rocks, and each composition part a
 * `qualityScale` applied to the rolled quality of that ore.
 */

import { component } from "../entity.js";
import { localize, lookupString, type Localization } from "../localization.js";
import { at, compareNames, num, str, type GameRecord } from "../records.js";
import { buildLocations } from "./starmap.js";
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
  /** Multiplier on the rolled quality of this ore in this rock. */
  qualityScale?: number;
}

/** The rock behind a harvestable, as a scanner and a mining laser see it. */
export interface RockInfo {
  rock: string;
  /** "ship", "vehicle" or "hand", from the mining parameters the rock uses. */
  kind?: string;
  /** "Common" .. "Legendary" for tiered ship rocks. */
  tier?: string;
  composition?: string;
  deposit?: string;
  /** RS signature shown on scan, per rock. */
  signature?: number;
  minimumDistinctElements?: number;
}

export interface SpawnEntry extends Partial<RockInfo> {
  group: string;
  groupProbability?: number;
  relativeProbability?: number;
  /** groupProbability x this entry's share of its group's relative weights. */
  spawnShare?: number;
  harvestable: string;
  /** Ores in the rock this entry spawns, when the chain resolves. */
  ores?: OreShare[];
}

export interface SpawnLocation {
  key: string;
  recordName: string;
  system?: string;
  body?: string;
  /** The matching star map object, when the key names one (planets, moons). */
  starmapRecord?: string;
  /** Star map chain above the body: ["Pyro", "Pyro V"] for Fuego. */
  path?: string[];
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
      qualityScale: num(obj.qualityScale),
    });
  }
  return ores.length > 0 ? ores : undefined;
}

const TIERS = ["Common", "Uncommon", "Rare", "Epic", "Legendary"];

/** "miningglobalparamsship" -> "ship"; FPS obstacle and organic rocks are hand-mined too. */
function kindOf(globalParams: string | undefined): string | undefined {
  if (!globalParams) return undefined;
  if (globalParams.includes("ship")) return "ship";
  if (globalParams.includes("groundvehicle")) return "vehicle";
  if (globalParams.includes("fps")) return "hand";
  return undefined;
}

/** RS signature: slot 4 of the radar base signatures. */
export function rockSignature(entity: GameRecord): number | undefined {
  const signatures = at(component(entity, "SSCSignatureSystemParams"), "radarProperties.baseSignatureParams.signatures");
  return Array.isArray(signatures) ? num(signatures[4]) || undefined : undefined;
}

/** A mineable rock entity's scan, tier and composition, or undefined if it is not one. */
export function describeRock(
  entity: GameRecord,
  byFile: Map<string, GameRecord>,
  localization: Localization = new Map(),
): (RockInfo & { ores?: OreShare[] }) | undefined {
  const mineable = component(entity, "MineableParams");
  if (!mineable) return undefined;
  const compositionFile = refFile(mineable.composition);
  const composition = compositionFile ? byFile.get(compositionFile) : undefined;
  return {
    rock: entity.shortName,
    kind: kindOf(refFile(mineable.globalParams)),
    // "CommonShipMineables_Iron"; case-sensitive, so "Uncommon" never matches "Common".
    tier: composition ? TIERS.find((t) => composition.shortName.startsWith(t)) : undefined,
    composition: composition?.shortName,
    deposit: composition ? localize(localization, composition.value.depositName) : undefined,
    signature: rockSignature(entity),
    minimumDistinctElements: composition ? num(composition.value.minimumDistinctElements) : undefined,
    ores: oresOfComposition(composition),
  };
}

/** Walk harvestable preset -> mineable entity, then describe the rock. */
function rockOfHarvestable(
  harvestableFile: string | undefined,
  byFile: Map<string, GameRecord>,
  localization: Localization,
): (RockInfo & { ores?: OreShare[] }) | undefined {
  if (!harvestableFile) return undefined;

  const preset = byFile.get(`${harvestableFile}.json`) ?? byFile.get(harvestableFile);
  if (!preset) return undefined;

  const entityFile = refFile(preset.value.entityClass);
  const entity = entityFile ? byFile.get(entityFile) : undefined;
  return entity ? describeRock(entity, byFile, localization) : undefined;
}

export const miningSpawnsDomain: Domain = {
  name: "game-mining-spawns",
  description: "Which ores spawn where, resolved from provider presets through to compositions.",
  filters: ["**/harvestable/**", "**/entities/mineable/**", "**/mining/**", "**/starmap/**"],

  needsLocalization: true,

  parse({ records, localization }) {
    const byFile = buildIndexes(records);
    // Spawn zone keys match star map record names for planets and moons
    // ("Pyro5e" is Fuego), which names zones the string table does not.
    const starmap = new Map(
      buildLocations(records, localization).map((l) => [l.recordName.slice(l.recordName.indexOf(".") + 1).toLowerCase(), l]),
    );

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

        const harvestables = (Array.isArray(g.harvestables) ? g.harvestables : []).filter(
          (e): e is Record<string, unknown> => !!e && typeof e === "object",
        );
        const groupWeight = harvestables.reduce((sum, e) => sum + (num(e.relativeProbability) ?? 0), 0);
        for (const e of harvestables) {
          const harvestable = refFile(e.harvestable) ?? refFile(e.harvestableEntityClass);
          if (!harvestable) continue;

          const rock = rockOfHarvestable(harvestable, byFile, localization);
          const resolved = rock?.ores;
          if (resolved) for (const ore of resolved) ores.add(ore.element);
          const relativeProbability = num(e.relativeProbability);

          entries.push({
            group: groupName,
            groupProbability,
            relativeProbability,
            spawnShare:
              groupProbability !== undefined && relativeProbability !== undefined && groupWeight > 0
                ? Math.round(((groupProbability * relativeProbability) / groupWeight) * 1e5) / 1e5
                : undefined,
            harvestable,
            rock: rock?.rock,
            kind: rock?.kind,
            tier: rock?.tier,
            deposit: rock?.deposit,
            signature: rock?.signature,
            ores: resolved,
          });
        }
      }

      const { key, system, body } = describeProvider(record.shortName, localization);
      const place = starmap.get(key.toLowerCase());
      locations.push({
        key,
        recordName: record.name,
        system: place?.system ?? system,
        body: body ?? place?.name,
        starmapRecord: place?.recordName,
        path: place?.path,
        entries,
        ores: [...ores].sort(compareNames),
      });
    }

    locations.sort((a, b) => compareNames(a.key, b.key));

    // The inverse index is the question people actually ask: where is this ore?
    const oreLocations: Record<string, string[]> = {};
    for (const location of locations) {
      for (const ore of location.ores) {
        (oreLocations[ore] ??= []).push(location.body ?? location.key);
      }
    }
    for (const list of Object.values(oreLocations)) list.sort(compareNames);

    const resolved = locations.filter((l) => l.ores.length > 0);

    // One row per ore per spawn entry: where a resource is, in which rock, how often.
    const oreSpawns = locations.flatMap((location) =>
      location.entries.flatMap((entry) =>
        (entry.ores ?? []).map((ore) => ({
          ore: ore.element,
          location: location.key,
          system: location.system,
          body: location.body,
          rock: entry.rock,
          kind: entry.kind,
          tier: entry.tier,
          signature: entry.signature,
          spawnShare: entry.spawnShare,
          minPercent: ore.minPercent,
          maxPercent: ore.maxPercent,
          qualityScale: ore.qualityScale,
        })),
      ),
    );
    oreSpawns.sort((a, b) => compareNames(a.ore, b.ore) || compareNames(a.location, b.location));

    // Every mineable rock, spawned or not, with where it spawns.
    const spawnsAt = new Map<string, Set<string>>();
    for (const location of locations) {
      for (const entry of location.entries) {
        if (!entry.rock) continue;
        const places = spawnsAt.get(entry.rock) ?? new Set<string>();
        places.add(location.body ?? location.key);
        spawnsAt.set(entry.rock, places);
      }
    }
    const rocks = records
      .filter((r) => r.recordPath.includes("/entities/mineable/"))
      .map((r) => describeRock(r, byFile, localization))
      .filter((r): r is NonNullable<typeof r> => Boolean(r))
      .map((r) => ({ ...r, spawnsAt: [...(spawnsAt.get(r.rock) ?? [])].sort(compareNames) }))
      .sort((a, b) => compareNames(a.rock, b.rock));

    return {
      locations,
      rocks,
      oreSpawns,
      oreLocations: Object.fromEntries(Object.entries(oreLocations).sort(([a], [b]) => compareNames(a, b))),
      summary: {
        locations: locations.length,
        locationsWithOres: resolved.length,
        locationsNamed: locations.filter((l) => l.body).length,
        distinctOres: Object.keys(oreLocations).length,
        spawnEntries: locations.reduce((sum, l) => sum + l.entries.length, 0),
        rocks: rocks.length,
        rocksWithSignature: rocks.filter((r) => r.signature).length,
      },
    };
  },
};
