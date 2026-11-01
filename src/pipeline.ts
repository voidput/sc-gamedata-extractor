/**
 * Orchestrates extract -> parse -> write.
 *
 * Extraction is the slow half (it reads a ~145GB archive), so each domain
 * extracts only the record globs it declares, and extracted records are kept on
 * disk so `parse` can be re-run without touching the archive again.
 */

import fs from "node:fs/promises";
import path from "node:path";

import { ammoDomain } from "./domains/ammo.js";
import { armorDomain } from "./domains/armor.js";
import { blueprintsDomain } from "./domains/blueprints.js";
import { cargoDomain } from "./domains/cargo.js";
import { commoditiesDomain } from "./domains/commodities.js";
import { containersDomain } from "./domains/containers.js";
import { fpsWeaponsDomain } from "./domains/fps-weapons.js";
import { lawDomain } from "./domains/law.js";
import { lootDomain } from "./domains/loot.js";
import { componentsDomain } from "./domains/components.js";
import { consumablesDomain } from "./domains/consumables.js";
import { miningDomain } from "./domains/mining.js";
import { manufacturersDomain } from "./domains/manufacturers.js";
import { miningSpawnsDomain } from "./domains/mining-spawns.js";
import { missionsDomain } from "./domains/missions.js";
import { quantumDomain } from "./domains/quantum.js";
import { refiningDomain } from "./domains/refining.js";
import { reputationDomain } from "./domains/reputation.js";
import { salvageDomain } from "./domains/salvage.js";
import { shipsDomain } from "./domains/ships.js";
import { shopsDomain } from "./domains/shops.js";
import { starmapDomain } from "./domains/starmap.js";
import { UEX_DOMAINS } from "./domains/uex-market.js";
import { weaponsDomain } from "./domains/weapons.js";
import { wikeloDomain } from "./domains/wikelo.js";
import type { ApiDomain, Domain } from "./domains/types.js";
import { matchesAny } from "./glob.js";
import { loadLocalization } from "./localization.js";
import { loadRecords } from "./records.js";
import { extractLocalization, extractRecords, type StarBreakerConfig } from "./starbreaker.js";

/**
 * Only domains verified against a real extraction are registered. A parser that
 * silently yields nothing is worse than an absent one, so a new domain lands
 * here after its output has been checked, not before.
 *
 * In progress, see src/domains/: wikelo, whose hand-in costs and rewards nest
 * deeper inside ContractTemplate than a shallow scan reaches.
 */
export const DOMAINS: Domain[] = [
  miningDomain,
  manufacturersDomain,
  componentsDomain,
  blueprintsDomain,
  wikeloDomain,
  missionsDomain,
  shipsDomain,
  ammoDomain,
  fpsWeaponsDomain,
  starmapDomain,
  reputationDomain,
  containersDomain,
  miningSpawnsDomain,
  weaponsDomain,
  commoditiesDomain,
  refiningDomain,
  shopsDomain,
  cargoDomain,
  salvageDomain,
  quantumDomain,
  armorDomain,
  lootDomain,
  lawDomain,
  consumablesDomain,
];

/** Datasets fetched from APIs. No game install or StarBreaker needed. */
export const API_DOMAINS: ApiDomain[] = [...UEX_DOMAINS];

export function selectApiDomains(names?: string[]): ApiDomain[] {
  if (!names || names.length === 0) return API_DOMAINS;
  return API_DOMAINS.filter((d) => names.includes(d.name));
}

/** Fetch API-sourced datasets and write them beside the extracted ones. */
export async function fetchApiDatasets(
  outDir: string,
  domains: ApiDomain[],
  options: { token?: string } = {},
  log: (msg: string) => void = () => {},
): Promise<{ dataset: string; file: string; collections: Record<string, number> }[]> {
  await fs.mkdir(outDir, { recursive: true });
  const written = [];

  for (const domain of domains) {
    log(`  fetching ${domain.name}`);
    const body = await domain.fetch(options);
    const file = path.join(outDir, `${domain.name}.json`);

    await fs.writeFile(
      file,
      JSON.stringify(
        {
          _source: "UEX Corp API (https://uexcorp.space)",
          _fetched: new Date().toISOString(),
          ...body,
        },
        null,
        2,
      ),
    );

    const collections: Record<string, number> = {};
    for (const [key, value] of Object.entries(body)) {
      if (Array.isArray(value)) collections[key] = value.length;
    }

    written.push({ dataset: domain.name, file, collections });
  }

  return written;
}

export function selectDomains(names?: string[]): Domain[] {
  if (!names || names.length === 0) return DOMAINS;
  const selected: Domain[] = [];
  for (const name of names) {
    const domain = DOMAINS.find((d) => d.name === name || d.name === `game-${name}`);
    if (!domain) {
      throw new Error(`Unknown domain "${name}". Known: ${DOMAINS.map((d) => d.name).join(", ")}`);
    }
    selected.push(domain);
  }
  return selected;
}

/** Extract the records each selected domain needs. */
export async function extract(
  config: StarBreakerConfig,
  extractDir: string,
  domains: Domain[],
  log: (msg: string) => void = () => {},
  language?: string,
): Promise<{ filter: string; records: number }[]> {
  // Domains share globs (both mining datasets want **/mining/**); extract once.
  const filters = [...new Set(domains.flatMap((d) => d.filters))];
  const results: { filter: string; records: number }[] = [];

  await fs.mkdir(extractDir, { recursive: true });
  for (const filter of filters) {
    log(`  extracting ${filter}`);
    const result = await extractRecords(config, filter, extractDir);
    log(`    ${result.records} records`);
    results.push(result);
  }

  // Display names are localization keys, so any domain surfacing them needs the
  // string table alongside the records.
  if (language || domains.some((d) => d.needsLocalization)) {
    const chosen = language ?? "english";
    log(`  extracting localization (${chosen}/global.ini)`);
    await extractLocalization(config, extractDir, chosen);
  }

  return results;
}

/** Parse extracted records into one dataset file per domain. */
export async function parse(
  extractDir: string,
  outDir: string,
  domains: Domain[],
  buildVersion?: string,
  log: (msg: string) => void = () => {},
): Promise<{ dataset: string; file: string; collections: Record<string, number> }[]> {
  const warnings: string[] = [];
  const records = await loadRecords(extractDir);
  if (records.length === 0) {
    throw new Error(
      `No records found under ${extractDir}. Run extract first, or check the path.`,
    );
  }
  log(`  loaded ${records.length} records`);

  const localization = await loadLocalization(extractDir);
  if (localization.size > 0) log(`  loaded ${localization.size} localized strings`);

  const needsStrings = domains.filter((d) => d.needsLocalization);
  if (needsStrings.length > 0 && localization.size === 0) {
    warnings.push(
      `no localization loaded, so display names will come out as @keys for: ${needsStrings
        .map((d) => d.name)
        .join(", ")}. Re-run extract to fetch global.ini.`,
    );
  }

  await fs.mkdir(outDir, { recursive: true });
  const written = [];

  for (const domain of domains) {
    // Hand a domain only the records its own filters select. Extraction pools
    // every domain's records together, so without this, registering a new domain
    // silently widens what every existing one sees.
    const scoped = records.filter((r) => matchesAny(r.recordPath, domain.filters));
    if (scoped.length === 0 && records.length > 0) {
      warnings.push(
        `${domain.name}: none of the loaded records match its filters (${domain.filters.join(", ")}).`,
      );
    }

    const body = domain.parse({ records: scoped, localization });
    const file = path.join(outDir, `${domain.name}.json`);

    await fs.writeFile(
      file,
      JSON.stringify(
        {
          _source: "Star Citizen game files via StarBreaker",
          _extracted: new Date().toISOString(),
          ...(buildVersion ? { _build: buildVersion } : {}),
          ...body,
        },
        null,
        2,
      ),
    );

    const collections: Record<string, number> = {};
    for (const [key, value] of Object.entries(body)) {
      if (Array.isArray(value)) collections[key] = value.length;
      else if (value && typeof value === "object") collections[key] = Object.keys(value).length;
    }

    // A parser whose selectors stopped matching still writes a well-formed file
    // full of nothing. Say so loudly: silent empties are how bad data ships.
    const empty = Object.entries(collections)
      .filter(([key, count]) => key !== "summary" && count === 0)
      .map(([key]) => key);
    const populated = Object.entries(collections).filter(
      ([key, count]) => key !== "summary" && count > 0,
    );

    if (populated.length === 0) {
      warnings.push(
        `${domain.name}: produced no data at all. Its record selectors likely no longer match; check the extraction filters and record schema.`,
      );
    } else if (empty.length > 0) {
      warnings.push(`${domain.name}: empty collection(s): ${empty.join(", ")}`);
    }

    log(`  wrote ${domain.name}.json`);
    written.push({ dataset: domain.name, file, collections });
  }

  if (warnings.length > 0) {
    log("");
    for (const warning of warnings) log(`  WARNING  ${warning}`);
  }

  return written;
}

/**
 * The build an extraction came from, written beside the records at extract
 * time. The installed game updates independently of the records on disk, so
 * a parse a week after an extract must not stamp its output with whatever
 * build the launcher has since downloaded: the dumps repo refuses a mixed
 * build, and a diff against it would be labelled wrong.
 */
const BUILD_MARKER = "_build.json";

/**
 * The channel and branch ride along so publishing can label the release: a PTU
 * build is a prerelease, and a HOTFIX build reports a `1.0.x` version that
 * only its branch name ties back to a patch.
 */
export async function writeExtractedBuild(
  extractDir: string,
  build: string,
  origin: { channel?: string; branch?: string } = {},
): Promise<void> {
  await fs.writeFile(
    path.join(extractDir, BUILD_MARKER),
    JSON.stringify({ build, ...origin, extracted: new Date().toISOString() }, null, 2),
  );
}

export async function readExtractedBuild(extractDir: string): Promise<string | undefined> {
  try {
    const parsed = JSON.parse(await fs.readFile(path.join(extractDir, BUILD_MARKER), "utf-8"));
    return typeof parsed?.build === "string" ? parsed.build : undefined;
  } catch {
    return undefined;
  }
}

/** Read the installed build id, which sits beside Data.p4k. */
export async function readBuildVersion(p4kPath: string): Promise<string | undefined> {
  const manifest = path.join(path.dirname(p4kPath), "build_manifest.id");
  try {
    const parsed = JSON.parse(await fs.readFile(manifest, "utf-8"));
    const data = parsed?.Data ?? {};
    return data.RequestedP4kVersion ?? data.Version ?? data.Branch ?? undefined;
  } catch {
    return undefined;
  }
}

/** The installed build's branch, e.g. `sc-alpha-4.10.2` or `sc-alpha-4.10.0-hotfix`. */
export async function readBuildBranch(p4kPath: string): Promise<string | undefined> {
  const manifest = path.join(path.dirname(p4kPath), "build_manifest.id");
  try {
    const parsed = JSON.parse(await fs.readFile(manifest, "utf-8"));
    return parsed?.Data?.Branch ?? undefined;
  } catch {
    return undefined;
  }
}

/**
 * Write the string table as a dataset so it can be searched without re-reading
 * the archive. This is the only string source the game ships: one global.ini per
 * language, with no separate table.
 */
export async function writeStrings(
  extractDir: string,
  outDir: string,
  language = "english",
): Promise<{ file: string; strings: number }> {
  const table = await loadLocalization(extractDir);
  if (table.size === 0) {
    throw new Error(
      `No string table found under ${extractDir}. Run extract first; it fetches ${language}/global.ini.`,
    );
  }

  await fs.mkdir(outDir, { recursive: true });
  const file = path.join(outDir, `game-strings-${language}.json`);
  // Stamped like every other dataset, so publishing refuses a string table
  // left over from an older extract.
  const build = await readExtractedBuild(extractDir);

  await fs.writeFile(
    file,
    JSON.stringify(
      {
        _source: `Star Citizen game files via StarBreaker (Data/Localization/${language}/global.ini)`,
        _extracted: new Date().toISOString(),
        ...(build ? { _build: build } : {}),
        language,
        strings: Object.fromEntries(table),
        summary: { strings: table.size },
      },
      null,
      2,
    ),
  );

  return { file, strings: table.size };
}
