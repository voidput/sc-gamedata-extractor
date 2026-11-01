/**
 * Refining: what the client knows about turning ore into refined material.
 *
 * That is less than a refinery screen shows. The nine RefiningProcess records
 * carry only a speed and quality label and a display name; the yield, cost and
 * duration each method produces, and every station's per-commodity bonus, are
 * worked out server-side and never ship. See the uex-refinery dataset for
 * player-reported values.
 *
 * What does ship: which raw resource refines into which (`refinedVersion` in the
 * ResourceTypeDatabase), the density of each, and CraftingGlobalParams'
 * `refiningQualityUnitMultiplier`.
 *
 * The method ratings the refinery screen shows also ship, as text: each
 * process's `<processName>_Details,P` string reads "High Yield // Low Cost //
 * Slowest". They are relative, not numbers. The record's labels map onto them
 * exactly, but not as named: `refiningSpeed` is the cost tier (Slow is Low
 * Cost, Fast is High Cost) and `refiningQuality` the yield, while the real speed
 * appears only in the text (Cormack is labelled Normal and rated Very Fast).
 * The workload tiers behind the capacity surcharge are likewise only text.
 */

import { lookupString, type Localization } from "../localization.js";
import { compareNames, num, refShortName, str, type GameRecord } from "../records.js";
import { displayNameOf, parseResourceDatabase } from "./commodities.js";
import type { Domain } from "./types.js";

export interface RefiningProcess {
  name: string;
  recordName: string;
  id: string;
  displayName?: string;
  /** "Slow", "Normal" or "Fast". Despite the name, this tracks cost, not speed. */
  speed?: string;
  /** "Wasteful", "Normal" or "Careful". Tracks yield. */
  quality?: string;
  description?: string;
  /** The in-game rating, from the process's details string. */
  ratings?: ProcessRatings;
}

export interface ProcessRatings {
  /** "Low", "Moderate" or "High". */
  yield?: string;
  cost?: string;
  /** "Slowest" .. "Fastest", nine steps. */
  speed?: string;
  /** 1 (low) .. 3 (high), the scale UEX uses. */
  yieldRank?: number;
  costRank?: number;
  /** 1 (slowest) .. 9 (fastest). */
  speedRank?: number;
}

const LEVELS = ["Low", "Moderate", "High"];
const SPEEDS = [
  "Slowest", "Very Slow", "Slow", "Slightly Slow", "Moderate",
  "Slightly Fast", "Fast", "Very Fast", "Fastest",
];

const rank = (scale: string[], value: string | undefined) =>
  value && scale.includes(value) ? scale.indexOf(value) + 1 : undefined;

/** "High Yield // Low Cost // Slowest\n" -> { yield: "High", cost: "Low", speed: "Slowest" }. */
export function parseRatings(details: string | undefined): ProcessRatings | undefined {
  if (!details) return undefined;
  const ratings: ProcessRatings = {};
  for (const raw of details.replace(/\\n/g, "").split("//")) {
    const part = raw.trim();
    const yieldMatch = part.match(/^(.+?) Yield$/);
    const costMatch = part.match(/^(.+?) Cost$/);
    if (yieldMatch) ratings.yield = yieldMatch[1];
    else if (costMatch) ratings.cost = costMatch[1];
    else if (part) ratings.speed = part.replace(/ Speed$/, "");
  }
  ratings.yieldRank = rank(LEVELS, ratings.yield);
  ratings.costRank = rank(LEVELS, ratings.cost);
  ratings.speedRank = rank(SPEEDS, ratings.speed);
  return ratings;
}

export function parseRefiningProcess(
  record: GameRecord,
  localization: Localization = new Map(),
): RefiningProcess {
  const v = record.value;
  const key = str(v.processName);
  return {
    name: record.shortName,
    recordName: record.name,
    id: record.id,
    displayName: displayNameOf(localization, v.processName),
    speed: str(v.refiningSpeed),
    quality: str(v.refiningQuality),
    description: key ? lookupString(localization, `${key}_Desc`)?.trim() : undefined,
    ratings: key ? parseRatings(lookupString(localization, `${key}_Details,P`)) : undefined,
  };
}

export interface Refinable {
  raw: string;
  rawDisplayName?: string;
  refined: string;
  refinedDisplayName?: string;
  rawDescription?: string;
  refinedDescription?: string;
  group: string;
  rawDensityGramsPerCc?: number;
  refinedDensityGramsPerCc?: number;
}

export const refiningDomain: Domain = {
  name: "game-refining",
  description: "Refining methods, raw-to-refined resource pairs, and refining quality settings.",
  filters: ["**/refiningprocess/**", "**/resourcetypedatabase/**", "**/crafting/globalparams/**"],
  needsLocalization: true,

  parse({ records, localization }) {
    const speedOrder = ["Slow", "Normal", "Fast"];
    const qualityOrder = ["Wasteful", "Normal", "Careful"];
    const processes = records
      .filter((r) => r.type === "RefiningProcess")
      .map((r) => parseRefiningProcess(r, localization))
      .sort(
        (a, b) =>
          speedOrder.indexOf(a.speed ?? "") - speedOrder.indexOf(b.speed ?? "") ||
          qualityOrder.indexOf(a.quality ?? "") - qualityOrder.indexOf(b.quality ?? "") ||
          compareNames(a.name, b.name),
      );

    const resources = records
      .filter((r) => r.type === "ResourceTypeDatabase")
      .flatMap((r) => parseResourceDatabase(r, localization));
    const byName = new Map(resources.map((r) => [r.name, r]));
    const refinables: Refinable[] = resources
      .filter((r) => r.refinesTo)
      .map((r) => {
        const refined = byName.get(r.refinesTo!);
        return {
          raw: r.name,
          rawDisplayName: r.displayName,
          refined: r.refinesTo!,
          refinedDisplayName: refined?.displayName,
          rawDescription: r.description,
          refinedDescription: refined?.description,
          group: r.group,
          rawDensityGramsPerCc: r.densityGramsPerCc,
          refinedDensityGramsPerCc: refined?.densityGramsPerCc,
        };
      })
      .sort((a, b) => compareNames(a.rawDisplayName ?? a.raw, b.rawDisplayName ?? b.raw));

    const params = records.find((r) => r.type === "CraftingGlobalParams")?.value ?? {};
    const dismantleBlacklist = Array.isArray(params.dismantleBlacklistResources)
      ? (params.dismantleBlacklistResources as unknown[]).map(refShortName).filter(Boolean)
      : [];

    // Surcharge text by workload, as the refinery kiosk words it.
    const workloadTiers = ["Low", "Medium", "High", "Extreme"]
      .map((level) => ({ level, text: lookupString(localization, `refinery_ui_Capacity_${level}`) }))
      .filter((tier) => tier.text);

    return {
      processes,
      refinables,
      workloadTiers,
      settings: {
        refiningQualityUnitMultiplier: num(params.refiningQualityUnitMultiplier),
        defaultCompositionQuality: num(params.defaultCompositionQuality),
        /** Resources that dismantling an item never gives back. */
        dismantleBlacklistResources: dismantleBlacklist,
      },
      summary: {
        processes: processes.length,
        rated: processes.filter((p) => p.ratings).length,
        refinables: refinables.length,
      },
    };
  },
};
