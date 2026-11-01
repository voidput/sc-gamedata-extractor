/**
 * Law: what counts as a crime, how it is punished, and where.
 *
 * An InfractionDefinition is a crime: its triggers (KillActor, PadRamming...)
 * and default parameters, among them whether it is a felony, how many prison
 * merits it carries, how long it stays on record (`lifetime`, hours) and the
 * escalated-fine multiplier for paying late.
 *
 * A Jurisdiction (UEE, ArcCorp, the Pyro gangs) lists the crimes it enforces,
 * with per-jurisdiction overrides, plus its base fine, prohibited goods,
 * controlled substance classes with their possession limits, and impound rules.
 * A child jurisdiction may also respect its parent's laws.
 *
 * Unset parameters are stored as -1 throughout and mean "inherit"; they are
 * dropped here so only real values remain.
 */

import { isUnresolved, localize, type Localization } from "../localization.js";
import { compareNames, num, refShortName, str, type GameRecord } from "../records.js";
import { refPath } from "./armor.js";
import type { Domain } from "./types.js";

/** The parameter fields that are set, with -1 ("inherit") and empty lists dropped. */
export function setParameters(value: unknown): Record<string, number | boolean> | undefined {
  if (!value || typeof value !== "object") return undefined;
  const out: Record<string, number | boolean> = {};
  for (const [key, raw] of Object.entries(value as Record<string, unknown>)) {
    const n = num(raw);
    if (n === undefined || n === -1) continue;
    // Flags are ints: 1 on, 0 off, -1 unset.
    out[key] = key === "isFelony" || key.startsWith("ignore") || key.startsWith("hide") ? n === 1 : n;
  }
  return Object.keys(out).length > 0 ? out : undefined;
}

/** Some name keys exist only with a `,P` suffix in the string table. */
function localizeName(localization: Localization, key: unknown): string | undefined {
  const name = localize(localization, key);
  if (!isUnresolved(name) || typeof key !== "string") return name;
  const suffixed = localize(localization, `${key},P`);
  return isUnresolved(suffixed) ? name : suffixed;
}

/** Last path segment of a reference, the crime or commodity's file name. */
const fileOf = (ref: unknown) => refPath(ref)?.split("/").pop();

export interface Crime {
  name: string;
  recordName: string;
  displayName?: string;
  /** "felonies", "misdemeanours" or "special". */
  category: string;
  triggers: string[];
  parameters?: Record<string, number | boolean>;
}

export function parseCrime(record: GameRecord, localization: Localization = new Map()): Crime {
  const v = record.value;
  const segments = record.recordPath.split("/");
  return {
    name: record.shortName,
    recordName: record.name,
    displayName: localizeName(localization, v.name),
    category: segments[segments.indexOf("infractiondefinitions") + 1] ?? "",
    triggers: Array.isArray(v.triggers) ? v.triggers.filter((t): t is string => typeof t === "string") : [],
    parameters: setParameters(v.defaultParameters),
  };
}

export interface Jurisdiction {
  name: string;
  recordName: string;
  displayName?: string;
  parent?: string;
  respectsParentLaws?: boolean;
  isPrison?: boolean;
  baseFine?: number;
  maxStolenGoodsPossessionScu?: number;
  /** Crimes enforced here, by crime name, with any overrides of their defaults. */
  crimes: { crime: string; overrides?: Record<string, number | boolean> }[];
  prohibitedGoods: string[];
  prohibitedResources: string[];
  controlledSubstanceClasses: { commodities: string[]; resources: string[]; maxPossessionScu?: number }[];
  impounding: { name?: string; trigger?: string; seconds?: number; fine?: number; ignoreIfAgainstPartyMember?: boolean }[];
}

const list = (value: unknown): Record<string, unknown>[] =>
  Array.isArray(value) ? (value.filter((x) => x && typeof x === "object") as Record<string, unknown>[]) : [];

const names = (value: unknown, pick: (x: unknown) => string | undefined): string[] =>
  Array.isArray(value) ? value.map(pick).filter((x): x is string => Boolean(x)) : [];

export function parseJurisdiction(
  record: GameRecord,
  localization: Localization = new Map(),
  crimeNames: Map<string, string> = new Map(),
): Jurisdiction {
  const v = record.value;
  // Crimes are referenced by path; name them as the crimes list does.
  const crimeOf = (ref: unknown) => {
    const path = refPath(ref);
    return path ? (crimeNames.get(path) ?? path.split("/").pop()) : undefined;
  };
  return {
    name: record.shortName,
    recordName: record.name,
    displayName: localizeName(localization, v.name),
    parent: fileOf(v.parentJurisdiction),
    respectsParentLaws: typeof v.respectsParentJurisdictionLaws === "boolean" ? v.respectsParentJurisdictionLaws : undefined,
    isPrison: typeof v.isPrison === "boolean" ? v.isPrison : undefined,
    baseFine: num(v.baseFine),
    maxStolenGoodsPossessionScu: num(v.maxStolenGoodsPossessionSCU),
    crimes: list(v.infractions)
      .map((i) => ({ crime: crimeOf(i.definition) ?? "", overrides: setParameters(i.parameterOverrides) }))
      .filter((c) => c.crime),
    prohibitedGoods: names(v.prohibitedGoods, fileOf),
    prohibitedResources: names(v.prohibitedResources, refShortName),
    controlledSubstanceClasses: list(v.controlledSubstanceClasses).map((c) => ({
      commodities: names(c.commodities, fileOf),
      resources: names(c.resources, refShortName),
      maxPossessionScu: num(c.maxPossessionSCU),
    })),
    impounding: list(v.impoundingDefinitions).map((d) => ({
      name: localizeName(localization, d.name),
      trigger: str(d.trigger),
      seconds: num(d.impoundingTimeSeconds),
      fine: num(d.impoundingFineUEC),
      ignoreIfAgainstPartyMember: typeof d.ignoreIfAgainstPartyMember === "boolean" ? d.ignoreIfAgainstPartyMember : undefined,
    })),
  };
}

export const lawDomain: Domain = {
  name: "game-law",
  description: "Crimes with their penalties, and jurisdictions with fines, prohibited goods and impound rules.",
  filters: ["**/lawsystem/infractiondefinitions/**", "**/lawsystem/jurisdictions/**"],
  needsLocalization: true,

  parse({ records, localization }) {
    const definitions = records.filter((r) => r.type === "InfractionDefinition");
    const crimeNames = new Map(
      definitions.map((r) => [refPath(r.recordPath) ?? "", r.shortName] as [string, string]),
    );
    const crimes = definitions
      .map((r) => parseCrime(r, localization))
      .sort((a, b) => compareNames(a.category, b.category) || compareNames(a.name, b.name));
    const jurisdictions = records
      .filter((r) => r.type === "Jurisdiction")
      .map((r) => parseJurisdiction(r, localization, crimeNames))
      .sort((a, b) => compareNames(a.displayName ?? a.name, b.displayName ?? b.name));
    return {
      crimes,
      jurisdictions,
      summary: { crimes: crimes.length, jurisdictions: jurisdictions.length },
    };
  },
};
