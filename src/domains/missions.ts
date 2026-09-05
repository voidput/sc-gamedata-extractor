/**
 * Mission broker entries: the contracts offered through the mobiGlas.
 *
 * A MissionBrokerEntry carries its title, giver and description as localization
 * keys, its payout in `missionReward`, and reputation changes per outcome in
 * `missionResultReputationRewards`.
 *
 * Many entries are unfinished: `notForRelease` marks developer and test content,
 * which is kept but flagged rather than dropped, since whether a mission is live
 * is exactly the kind of thing a reader wants to know.
 */

import { isPlaceholderName, localize, type Localization } from "../localization.js";
import { at, num, refShortName, str, type GameRecord } from "../records.js";
import type { Domain } from "./types.js";

export interface Mission {
  name: string;
  recordName: string;
  id: string;
  title?: string;
  description?: string;
  giver?: string;
  type?: string;
  difficulty?: number;
  reward?: number;
  rewardMax?: number;
  currency?: string;
  buyIn?: number;
  /** Developer or test content not offered in the live game. */
  notForRelease?: true;
  lawful?: boolean;
  tags?: string[];
}

/** The record name a `file://...json` reference points at. */
export function referencedName(value: unknown): string | undefined {
  const path = str(value);
  if (!path) return undefined;
  const file = path.split("/").pop();
  return file ? file.replace(/\.json$/i, "") : undefined;
}

function tagNames(value: unknown): string[] | undefined {
  const tags = at(value, "tags");
  if (!Array.isArray(tags)) return undefined;
  const names = tags
    .map((tag) => refShortName(tag))
    .filter((n): n is string => typeof n === "string" && !n.match(/^[0-9a-f-]{36}$/i));
  return names.length > 0 ? names : undefined;
}

export function parseMission(record: GameRecord, localization: Localization): Mission {
  const v = record.value;
  const title = localize(localization, v.title);
  const giver = localize(localization, v.missionGiver);
  const description = localize(localization, v.description);
  const difficulty = num(v.missionDifficulty);

  return {
    // Titles are frequently placeholders on unfinished entries.
    name: isPlaceholderName(title) ? record.shortName : (title as string),
    recordName: record.name,
    id: record.id,
    title: isPlaceholderName(title) ? undefined : title,
    description: isPlaceholderName(description) ? undefined : description,
    giver: isPlaceholderName(giver) ? undefined : giver,
    type: referencedName(v.type),
    // -1 means unset rather than "easiest".
    difficulty: difficulty !== undefined && difficulty >= 0 ? difficulty : undefined,
    reward: num(at(v, "missionReward.reward")) || undefined,
    rewardMax: num(at(v, "missionReward.max")) || undefined,
    currency: str(at(v, "missionReward.currencyType")),
    buyIn: num(v.missionBuyInAmount) || undefined,
    notForRelease: v.notForRelease === true ? true : undefined,
    lawful: typeof v.lawfulMission === "boolean" ? v.lawfulMission : undefined,
    tags: tagNames(v.missionTags),
  };
}

export const missionsDomain: Domain = {
  name: "game-missions",
  description: "Mission broker contracts with payouts, givers and difficulty.",
  filters: ["**/missionbroker/**"],
  needsLocalization: true,

  parse({ records, localization }) {
    const missions = records
      .filter((r) => r.type === "MissionBrokerEntry")
      .map((r) => parseMission(r, localization))
      .sort((a, b) => a.name.localeCompare(b.name));

    const live = missions.filter((m) => !m.notForRelease);
    const paying = live.filter((m) => (m.reward ?? 0) > 0);

    const byGiver: Record<string, number> = {};
    for (const mission of live) {
      if (mission.giver) byGiver[mission.giver] = (byGiver[mission.giver] ?? 0) + 1;
    }

    const rewards = paying.map((m) => m.reward as number).sort((a, b) => a - b);

    return {
      missions,
      byGiver: Object.fromEntries(Object.entries(byGiver).sort((a, b) => b[1] - a[1])),
      summary: {
        missions: missions.length,
        live: live.length,
        notForRelease: missions.length - live.length,
        withPayout: paying.length,
        // A median says more about typical pay than an average skewed by outliers.
        medianReward: rewards.length > 0 ? rewards[Math.floor(rewards.length / 2)] : 0,
        maxReward: rewards.length > 0 ? rewards[rewards.length - 1] : 0,
      },
    };
  },
};
