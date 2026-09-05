/**
 * Reputation standings: the ranks each faction and guild grants.
 *
 * A SReputationStandingParams record is one rank, with `minReputation` as the
 * threshold to reach it. Ranks belonging to one organisation share a record-name
 * prefix, so they are grouped by that and ordered by threshold, which is what
 * turns a flat list into a progression.
 *
 * `driftReputation` and `driftTimeHours` describe decay back toward neutral.
 */

import { isPlaceholderName, localize, type Localization } from "../localization.js";
import { num, type GameRecord } from "../records.js";
import type { Domain } from "./types.js";

export interface Standing {
  name: string;
  recordName: string;
  id: string;
  displayName?: string;
  organisation?: string;
  minReputation?: number;
  driftReputation?: number;
  driftTimeHours?: number;
  gated?: boolean;
}

/**
 * "ReputationStanding_Guild_04" belongs to "Guild".
 * The trailing rank number is dropped; anything before it names the org.
 */
export function organisationOf(shortName: string): string | undefined {
  const parts = shortName.split("_");
  if (parts[0]?.toLowerCase().startsWith("reputationstanding")) parts.shift();
  if (parts.length === 0) return undefined;
  if (parts.length > 1 && /^\d+$/.test(parts[parts.length - 1])) parts.pop();
  return parts.length > 0 ? parts.join("_") : undefined;
}

export function parseStanding(record: GameRecord, localization: Localization): Standing {
  const v = record.value;
  const displayName = localize(localization, v.displayName);

  return {
    name: record.shortName,
    recordName: record.name,
    id: record.id,
    displayName: isPlaceholderName(displayName) ? undefined : displayName,
    organisation: organisationOf(record.shortName),
    minReputation: num(v.minReputation),
    driftReputation: num(v.driftReputation) || undefined,
    driftTimeHours: num(v.driftTimeHours) || undefined,
    gated: v.gated === true ? true : undefined,
  };
}

export const reputationDomain: Domain = {
  name: "game-reputation",
  description: "Faction and guild reputation ranks with their thresholds.",
  filters: ["**/reputation/**"],
  needsLocalization: true,

  parse({ records, localization }) {
    const standings = records
      .filter((r) => r.type === "SReputationStandingParams")
      .map((r) => parseStanding(r, localization));

    // Ranks only make sense as an ordered progression per organisation.
    const byOrganisation: Record<string, Standing[]> = {};
    for (const standing of standings) {
      const org = standing.organisation ?? "unknown";
      (byOrganisation[org] ??= []).push(standing);
    }
    for (const list of Object.values(byOrganisation)) {
      list.sort((a, b) => (a.minReputation ?? 0) - (b.minReputation ?? 0));
    }

    return {
      standings: standings.sort((a, b) => a.name.localeCompare(b.name)),
      byOrganisation,
      summary: {
        standings: standings.length,
        organisations: Object.keys(byOrganisation).length,
      },
    };
  },
};
