#!/usr/bin/env node
/**
 * sc-gamedata — extract Star Citizen game data into structured JSON.
 *
 *   sc-gamedata extract      pull DataCore records out of Data.p4k
 *   sc-gamedata parse        turn extracted records into dataset JSON
 *   sc-gamedata all          both, in order
 *   sc-gamedata domains      list what can be built
 *   sc-gamedata builds       list builds that can be compared
 *   sc-gamedata diff         what changed between two builds
 */

import fs from "node:fs/promises";
import path from "node:path";

import {
  API_DOMAINS,
  DOMAINS,
  extract,
  fetchApiDatasets,
  parse,
  readBuildBranch,
  readBuildVersion,
  readExtractedBuild,
  selectApiDomains,
  writeExtractedBuild,
  selectDomains,
  writeStrings,
} from "./pipeline.js";
import {
  applyOverlay,
  diffStrings,
  loadLocalization,
  loadOverlay,
  lookupString,
  searchStrings,
} from "./localization.js";
import {
  datasetFile,
  diffDataset,
  dumpsRepoOptions,
  listDatasetFiles,
  listPublishedBuilds,
  listSnapshots,
  renderDiff,
  resolveSource,
  saveSnapshot,
  snapshotsDir,
} from "./diff.js";
import {
  fetchSource,
  findInstalledOverride,
  loadSources,
  sourcesFile,
} from "./sources/overlays.js";
import { LANGUAGES } from "./starbreaker.js";
import { findDataP4k, findStarBreaker, starbreakerVersion } from "./starbreaker.js";
import { deflectionCheck, renderDeflection } from "./deflection.js";
import { ITEM_COLUMNS, STAT_COLUMNS, patchChanges, toCsv } from "./changes.js";
import { G, envelope, rangeTable, renderEnvelope, targetOf, type Target } from "./engagement.js";
import type { Armor, Vehicle } from "./domains/ships.js";
import type { Weapon } from "./domains/weapons.js";

interface Args {
  command: string;
  flags: Map<string, string>;
  domains: string[];
}

/**
 * Flags that take no value. Without this list a boolean flag swallows the next
 * positional, so `strings --starstrings Ouratite` loses the search term.
 */
const BOOLEAN_FLAGS = new Set(["help", "write", "keys-only", "diff", "installed", "json", "detail", "fetch"]);

function parseArgs(argv: string[]): Args {
  const [command = "help", ...rest] = argv;
  const flags = new Map<string, string>();
  const domains: string[] = [];

  for (let i = 0; i < rest.length; i += 1) {
    const token = rest[i];
    if (!token.startsWith("--")) {
      domains.push(token);
      continue;
    }

    const key = token.slice(2);
    if (BOOLEAN_FLAGS.has(key)) {
      flags.set(key, "true");
      continue;
    }

    const next = rest[i + 1];
    if (next && !next.startsWith("--")) {
      flags.set(key, next);
      i += 1;
    } else {
      flags.set(key, "true");
    }
  }

  return { command, flags, domains };
}

const HELP = `sc-gamedata — extract Star Citizen game data into structured JSON

Usage:
  sc-gamedata <command> [domains...] [options]

Commands:
  extract          Pull DataCore records out of Data.p4k
  parse            Turn already-extracted records into dataset JSON
  all              Run extract then parse
  fetch            Fetch API-sourced datasets (no game install needed)
  strings          Search or look up localized strings
  overlays         List known community string packs
  overlays fetch   Download a pack by name
  domains          List available domains
  languages        List string-table languages
  builds           List builds available to diff (local parse, snapshots, published tags)
  snapshot         Save the current data/ as a snapshot of its build (parse does this)
  diff <a> <b>     What changed between two builds, for one or every dataset
  deflect <ship>   Which ship weapons get through a ship's stock armour
  changes <a> <b>  Buffs and nerfs between two builds as chart-ready CSV and JSON
  engage <ship> [target]
                   A loadout's hull DPS on a target by distance, and its effective ranges
  help             Show this message

Options:
  --p4k <path>          Path to Data.p4k        (env SC_DATA_P4K)
  --starbreaker <path>  StarBreaker executable  (env STARBREAKER_BIN)
  --channel <name>      Game channel when auto-detecting (default LIVE)
  --extract-dir <path>  Where records land      (default ./extracted-data)
  --out <path>          Where datasets land     (default ./data)
  --uex-token <token>   UEX API token           (env UEXTOKEN)
  --language <name>     String table language   (default english)
  --key <name>          strings: exact key lookup, with or without "@"
  --limit <n>           strings: max results (default 25)
  --keys-only           strings: match keys only, not values
  --write               strings: also write the table as a dataset
  --overlay <path>      strings: layer an .ini file or directory (repeatable
                        with commas) over the base
  --pack <name>         strings: layer a fetched pack by name
  --installed           strings: layer the pack installed in the game folder
  --diff                strings: show what the overlay changes
  --item <text>         diff: one item by name or record name, with every
                        changed field
  --collection <name>   diff: one collection of the dataset (default: all)
  --fields <a,b>        diff: only changes under these fields (e.g. dps,alpha)
  --detail              diff: list changed fields for every item
  --limit <n>           diff: items listed per collection (default 20)
  --json                diff: machine-readable output
  --fetch               builds: fetch tags from the dumps repo first
  --size <n>            deflect: only weapons of this size
  --export-dir <path>   changes: where to write (default ./exports)
  --target <ship>       engage: the ship being shot (or give it as a second name)
  --weapons <list>      engage: guns instead of the stock loadout, by record,
                        "name S<size>" or name, each optionally *count
  --target-speed <m/s>  engage: target speed (default its SCM speed)
  --closing <m/s>       engage: closing speed, negative when opening
  --accuracy <a,b,...>  engage: hit-chance levels in percent (default 90,75,50,25)
  --spread min|max      engage: first-shot or sustained spread (default max)
  --accel <g,g,...>     engage: target jink acceleration in g; the first sets
                        the distance table, all are tabulated (default 0,3,6,9,13;
                        not in game data)
  --reaction <s>        engage: seconds before the target starts jinking
  --hit-radius <m>      engage: override the target's hit radius
  --bands <a,b,...>     engage: distances in metres to tabulate

Examples:
  sc-gamedata all
  sc-gamedata all mining
  sc-gamedata extract --channel PTU
  sc-gamedata parse --extract-dir ./extracted-data --out ./data
  sc-gamedata fetch
  sc-gamedata fetch uex-terminals
  sc-gamedata strings Quantainium
  sc-gamedata strings --key @manufacturer_NameAEGS
  sc-gamedata strings --write --language german_(germany)
  sc-gamedata overlays
  sc-gamedata overlays fetch starstrings
  sc-gamedata strings --pack starstrings --diff
  sc-gamedata strings --installed Ouratite
  sc-gamedata strings --overlay ./mypack,./extra.ini
  sc-gamedata builds
  sc-gamedata diff 1.0.191.28374 local weapons
  sc-gamedata diff 1.0.191.28374 local weapons --item "M7A"
  sc-gamedata diff 4.10 local weapons --fields dps,alpha,fireRate --detail
  sc-gamedata diff ./data-old ./data --json
  sc-gamedata deflect perseus --size 3
  sc-gamedata changes 4.10 local
  sc-gamedata engage gladius --target perseus
  sc-gamedata engage --weapons "M7A Cannon*2,AD5B Ballistic Gatling" --target "Hammerhead" --accel 6

A build is "local" (the --out directory), a directory of game-*.json, or a
tag in the dumps repo. A tag prefix such as 4.10 picks the newest matching
build.

StarBreaker: https://github.com/diogotr7/StarBreaker/releases`;

/** The checkout root: dist/cli.js sits one level below it. */
function repoRoot(): string {
  return path.resolve(path.dirname(new URL(import.meta.url).pathname), "..");
}

async function main() {
  const { command, flags, domains: requested } = parseArgs(process.argv.slice(2));

  if (command === "help" || flags.has("help")) {
    console.log(HELP);
    return;
  }

  if (command === "domains") {
    console.log("From game files (needs a Star Citizen install and StarBreaker):");
    for (const domain of DOMAINS) {
      console.log(`  ${domain.name}\n      ${domain.description}`);
      console.log(`      records: ${domain.filters.join(", ")}`);
    }
    console.log("\nFrom APIs (needs network access):");
    for (const domain of API_DOMAINS) {
      console.log(`  ${domain.name}\n      ${domain.description}`);
    }
    return;
  }

  if (command === "languages") {
    console.log("String-table languages (one global.ini each, no separate table):");
    for (const language of LANGUAGES) console.log(`  ${language}`);
    return;
  }

  if (command === "overlays") {
    const sources = await loadSources();
    const [action, wanted] = requested;

    if (action !== "fetch") {
      console.log("Known string packs:\n");
      for (const source of sources) {
        console.log(`  ${source.name}`);
        if (source.description) console.log(`      ${source.description}`);
        if (source.homepage) console.log(`      ${source.homepage}`);
        console.log(`      ${source.files.length} file(s)`);
      }
      console.log(`\nAdd your own in ${sourcesFile()} as {name, description, files:[url]}.`);
      console.log("Any pack also works without registering: --overlay <file or dir>.");
      return;
    }

    if (!wanted) {
      throw new Error(`Name a pack to fetch. Known: ${sources.map((s) => s.name).join(", ")}`);
    }
    const source = sources.find((s) => s.name === wanted);
    if (!source) {
      throw new Error(`Unknown pack "${wanted}". Known: ${sources.map((s) => s.name).join(", ")}`);
    }

    const outDir = path.resolve(flags.get("out") ?? path.join("overlays", source.name));
    console.log(`fetching ${source.name}${source.homepage ? ` from ${source.homepage}` : ""}`);
    const written = await fetchSource(source, outDir);
    for (const { file, bytes } of written) {
      console.log(`  ${path.basename(file)}  ${(bytes / 1024).toFixed(1)} KB`);
    }
    console.log(`\nwrote ${written.length} file(s) to ${outDir}`);
    if (source.perPatch) {
      console.log("This pack is rebuilt per patch; refetch after a game update.");
    }
    return;
  }

  if (command === "strings") {
    const extractDir = path.resolve(flags.get("extract-dir") ?? "extracted-data");
    const language = flags.get("language") ?? "english";
    const base = await loadLocalization(extractDir);

    if (base.size === 0) {
      throw new Error(
        `No string table under ${extractDir}. Run "extract" first, which fetches ${language}/global.ini.`,
      );
    }
    // Overlays layer in the order given: named packs, then the installed
    // override, then explicit paths. Later ones win.
    const overlayPaths: string[] = [];

    const pack = flags.get("pack");
    if (pack) {
      const sources = await loadSources();
      if (!sources.some((source) => source.name === pack)) {
        throw new Error(
          `Unknown pack "${pack}". Known: ${sources.map((source) => source.name).join(", ")}. Fetch it with "overlays fetch ${pack}".`,
        );
      }
      overlayPaths.push(path.resolve(flags.get("overlay-dir") ?? path.join("overlays", pack)));
    }

    if (flags.has("installed")) {
      const installed = await findInstalledOverride(
        findDataP4k(flags.get("p4k"), flags.get("channel") ?? "LIVE"),
        language,
      );
      if (!installed) {
        throw new Error(
          `No string pack is installed in the game folder for ${language}. A pack installs as data/Localization/${language}/global.ini.`,
        );
      }
      console.log(`installed pack: ${installed}`);
      overlayPaths.push(installed);
    }

    const explicit = flags.get("overlay");
    if (explicit) {
      for (const entry of explicit.split(",")) {
        const trimmed = entry.trim();
        if (trimmed) overlayPaths.push(path.resolve(trimmed));
      }
    }

    let table = base;
    let overlayTable;

    if (overlayPaths.length > 0) {
      overlayTable = new Map<string, string>();
      for (const overlayPath of overlayPaths) {
        const loaded = await loadOverlay(overlayPath).catch(() => {
          throw new Error(
            `Could not read overlay ${overlayPath}. For a named pack, run "overlays fetch <name>" first.`,
          );
        });
        overlayTable = applyOverlay(overlayTable, loaded);
      }
      table = applyOverlay(base, overlayTable);
      console.log(
        `${base.size} base strings, ${overlayTable.size} from ${overlayPaths.length} overlay(s)`,
      );
    } else {
      console.log(`${table.size} strings loaded from ${extractDir}`);
    }

    if (flags.has("diff")) {
      if (!overlayTable) {
        throw new Error("--diff needs an overlay: --pack <name>, --installed, or --overlay <path>.");
      }
      const { changed, added } = diffStrings(base, overlayTable);
      console.log(`\noverlay changes ${changed.length} string(s) and adds ${added.length}`);
      for (const change of changed.slice(0, Number(flags.get("limit") ?? 10))) {
        console.log(`  ${change.key}`);
        console.log(`      base    ${(change.base ?? "").slice(0, 90)}`);
        console.log(`      overlay ${change.overlay.slice(0, 90)}`);
      }
    }

    const key = flags.get("key");
    if (key) {
      const value = lookupString(table, key);
      console.log(value === undefined ? `${key} -> (not found)` : `${key} -> ${value}`);
    }

    const query = requested.join(" ");
    if (query) {
      const { total, hits } = searchStrings(table, query, {
        limit: Number(flags.get("limit") ?? 25),
        keysOnly: flags.has("keys-only"),
      });
      console.log(`\n${total} match(es) for "${query}"${total > hits.length ? `, showing ${hits.length}` : ""}`);
      for (const hit of hits) console.log(`  ${hit.key}\n      ${hit.value}`);
    }

    if (flags.has("write")) {
      const written = await writeStrings(
        extractDir,
        path.resolve(flags.get("out") ?? "data"),
        language,
      );
      console.log(`\nwrote ${written.strings} strings to ${written.file}`);
    }

    if (!key && !query && !flags.has("write") && !flags.has("diff")) {
      console.log('\nPass a search term, --key <name>, or --write.');
    }
    return;
  }

  if (command === "builds") {
    const dataDir = path.resolve(flags.get("out") ?? "data");
    const repo = dumpsRepoOptions(path.resolve(path.dirname(new URL(import.meta.url).pathname), ".."));

    const localFiles = (await fs.readdir(dataDir).catch(() => [] as string[])).filter(
      (f) => f.startsWith("game-") && f.endsWith(".json"),
    );
    const localBuilds = new Set<string>();
    for (const file of localFiles) {
      try {
        const parsed = JSON.parse(await fs.readFile(path.join(dataDir, file), "utf-8"));
        if (typeof parsed._build === "string") localBuilds.add(parsed._build);
      } catch {
        // Not a dataset; skip.
      }
    }
    console.log(`local (${dataDir})`);
    console.log(
      localFiles.length === 0
        ? "  no datasets; run parse first"
        : `  ${localFiles.length} dataset(s), build ${[...localBuilds].join(", ") || "unknown"}`,
    );

    const snapshots = listSnapshots(snapshotsDir(repoRoot()));
    console.log(`\nsnapshots (${snapshotsDir(repoRoot())})`);
    if (snapshots.length === 0) console.log("  none yet; every parse saves one");
    for (const snap of snapshots) {
      console.log(`  ${snap.build}  ${snap.datasets} dataset(s)  ${snap.saved.slice(0, 10)}`);
    }

    const published = listPublishedBuilds(repo, flags.has("fetch"));
    console.log(`\npublished (${repo.workDir})`);
    if (published.length === 0) {
      console.log("  none found; pass --fetch to clone or update the dumps checkout");
    }
    for (const build of published) console.log(`  ${build.version}  ${build.date}`);
    return;
  }

  if (command === "engage") {
    const dataDir = path.resolve(flags.get("out") ?? "data");
    const ships = JSON.parse(await fs.readFile(path.join(dataDir, "game-ships.json"), "utf8")) as { vehicles: Vehicle[] };
    const arsenal = JSON.parse(await fs.readFile(path.join(dataDir, "game-weapons.json"), "utf8")) as {
      shipWeapons: Weapon[];
      fpsWeapons: Weapon[];
    };
    const allWeapons = [...arsenal.shipWeapons, ...arsenal.fpsWeapons].filter((w) => !w.isTemplate);
    const short = (r: string) => r.replace(/^EntityClassDefinition\./, "").toLowerCase();

    const findShip = (wanted: string): Vehicle => {
      const q = wanted.toLowerCase();
      const pool = ships.vehicles.filter((v) => !v.isTemplate);
      const exact = pool.filter((v) => v.name.toLowerCase() === q || short(v.recordName) === q);
      const matches = exact.length > 0 ? exact : pool.filter((v) => `${v.name} ${v.recordName}`.toLowerCase().includes(q));
      if (matches.length === 0) throw new Error(`No ship matches "${wanted}".`);
      return (
        matches.find((v) => !/_(ai|pu|ea|template|pir|crim|unmanned)/i.test(v.recordName) && v.armor) ?? matches[0]
      );
    };
    const findWeapon = (spec: string): Weapon => {
      const q = spec.trim().toLowerCase();
      const sized = /^(.*)\s+s(\d+)$/.exec(q);
      const hit =
        allWeapons.find((w) => short(w.recordName) === q) ??
        (sized ? allWeapons.find((w) => w.name.toLowerCase() === sized[1] && w.size === Number(sized[2])) : undefined) ??
        allWeapons.find((w) => w.name.toLowerCase() === q) ??
        allWeapons.find((w) => w.name.toLowerCase().includes(q));
      if (!hit) throw new Error(`No weapon matches "${spec}". Use a record name, or "name S<size>".`);
      return hit;
    };

    const [attackerName, targetArg] = requested.length > 0 ? [requested[0], requested.slice(1).join(" ")] : [undefined, ""];
    const targetName = flags.get("target") ?? (targetArg || undefined);
    let label: string;
    let loadout: { weapon: Weapon; count: number; mount?: string }[];
    if (flags.has("weapons")) {
      loadout = (flags.get("weapons") ?? "").split(",").filter((x) => x.trim()).map((part) => {
        const [spec, count] = part.split("*");
        return { weapon: findWeapon(spec), count: count ? Number(count) : 1 };
      });
      label = attackerName ? `${findShip(attackerName).name} (custom guns)` : "custom loadout";
    } else {
      if (!attackerName) throw new Error("engage needs a ship for its stock loadout, or --weapons.");
      const attacker = findShip(attackerName);
      label = `${attacker.name} stock`;
      loadout = (attacker.loadout?.weapons ?? []).flatMap((entry) => {
        const weapon = arsenal.shipWeapons.find((w) => short(w.recordName) === entry.item.toLowerCase());
        return weapon && weapon.type === "WeaponGun" ? [{ weapon, count: entry.count ?? 1, mount: entry.mount }] : [];
      });
      if (loadout.length === 0) throw new Error(`${attacker.name} has no stock guns in the data.`);
    }
    const target: Target | undefined = targetName ? targetOf(findShip(targetName)) : undefined;
    if (target && flags.has("hit-radius")) target.hitRadius = Number(flags.get("hit-radius"));
    const accelList = (flags.get("accel") ?? "0,3,6,9,13").split(",").map(Number).filter((n) => Number.isFinite(n) && n >= 0);
    const accelG = accelList[0];
    const numberFlag = (name: string) => (flags.has(name) ? Number(flags.get(name)) : undefined);
    const options = {
      accel: accelG !== undefined ? accelG * G : undefined,
      reaction: numberFlag("reaction"),
      targetSpeed: numberFlag("target-speed"),
      closing: numberFlag("closing"),
      spread: flags.get("spread") === "min" ? ("min" as const) : ("max" as const),
      bands: flags.get("bands")?.split(",").map(Number).filter((n) => Number.isFinite(n)),
      accuracy: flags.get("accuracy")?.split(",").map((n) => Number(n) / 100).filter((n) => n > 0 && n <= 1),
    };
    const result = envelope(loadout, target, options);
    const table = target ? rangeTable(loadout, target, accelList, options) : undefined;
    if (flags.has("json")) {
      console.log(JSON.stringify({ attacker: label, ...result, rangeByAccel: table }, null, 2));
      return;
    }
    console.log(renderEnvelope(label, result, options, target, table));
    return;
  }

  if (command === "changes") {
    const [fromSpec, toSpec] = requested;
    if (!fromSpec || !toSpec) throw new Error("changes needs two builds: sc-gamedata changes <from> <to>");
    const dataDir = path.resolve(flags.get("out") ?? "data");
    const repo = dumpsRepoOptions(repoRoot());
    const snapshots = snapshotsDir(repoRoot());
    const from = resolveSource(fromSpec, { dataDir, repo, snapshots });
    const to = resolveSource(toSpec, { dataDir, repo, snapshots });
    const result = patchChanges(from, to);
    const label = `${result.from ?? fromSpec}_to_${result.to ?? toSpec}`;
    const dir = path.resolve(flags.get("export-dir") ?? "exports", "changes", label);
    await fs.mkdir(dir, { recursive: true });
    await fs.writeFile(path.join(dir, "stat_changes.csv"), toCsv(result.stats, STAT_COLUMNS));
    await fs.writeFile(path.join(dir, "item_changes.csv"), toCsv(result.items, ITEM_COLUMNS));
    await fs.writeFile(path.join(dir, "changes.json"), JSON.stringify(result, null, 1));

    if (flags.has("json")) {
      console.log(JSON.stringify(result, null, 2));
      return;
    }
    console.log(`${result.from ?? fromSpec} -> ${result.to ?? toSpec}`);
    const categories = [...new Set(result.items.map((i) => i.category))];
    for (const category of categories) {
      const items = result.items.filter((i) => i.category === category);
      const count = (c: string) => items.filter((i) => i.change === c).length;
      const stats = result.stats.filter((s) => s.category === category);
      console.log(
        `  ${category.padEnd(15)} +${count("added")} -${count("removed")} ~${count("changed")}  buffs ${stats.filter((s) => s.direction === "buff").length}  nerfs ${stats.filter((s) => s.direction === "nerf").length}`,
      );
    }
    if (categories.length === 0) console.log("  no tracked stat changed");
    if (result.skipped.length > 0) console.log(`  skipped (missing in one build): ${result.skipped.join(", ")}`);
    const top = result.stats
      .filter((s) => s.percent !== undefined)
      .sort((a, b) => Math.abs(b.percent ?? 0) - Math.abs(a.percent ?? 0))
      .slice(0, Number(flags.get("limit") ?? 10));
    if (top.length > 0) console.log("\n  largest moves:");
    for (const s of top) {
      console.log(`    ${s.direction.padEnd(7)} ${s.name} (S${s.size ?? "?"}) ${s.stat}: ${s.before} -> ${s.after} (${(s.percent ?? 0) > 0 ? "+" : ""}${s.percent}%)`);
    }
    console.log(`\nwrote ${dir}`);
    return;
  }

  if (command === "snapshot") {
    const dataDir = path.resolve(flags.get("out") ?? "data");
    const stamps = new Set<string>();
    for (const file of (await fs.readdir(dataDir)).filter((f) => f.startsWith("game-") && f.endsWith(".json"))) {
      const head = (await fs.readFile(path.join(dataDir, file), "utf-8")).slice(0, 4096);
      const stamp = /"_build":\s*"([^"]+)"/.exec(head)?.[1];
      if (stamp) stamps.add(stamp);
    }
    if (stamps.size === 0) throw new Error(`No build-stamped datasets in ${dataDir}; run parse first.`);
    for (const build of stamps) {
      const kept = saveSnapshot(dataDir, snapshotsDir(repoRoot()), build);
      console.log(`saved ${build}: ${kept.datasets} dataset(s) in ${kept.path}`);
    }
    return;
  }

  if (command === "diff") {
    const [fromSpec, toSpec, datasetArg] = requested;
    if (!fromSpec || !toSpec) {
      throw new Error(
        'diff needs two builds: sc-gamedata diff <from> <to> [dataset]. Use "local" for the parse output, a directory, or a published tag (see "builds").',
      );
    }
    const dataDir = path.resolve(flags.get("out") ?? "data");
    const repo = dumpsRepoOptions(path.resolve(path.dirname(new URL(import.meta.url).pathname), ".."));
    const snapshots = snapshotsDir(repoRoot());
    const from = resolveSource(fromSpec, { dataDir, repo, snapshots });
    const to = resolveSource(toSpec, { dataDir, repo, snapshots });

    const fields = flags.get("fields")?.split(",").map((f) => f.trim()).filter(Boolean);
    const options = {
      item: flags.get("item"),
      collection: flags.get("collection"),
      fields: fields && fields.length > 0 ? fields : undefined,
      detail: flags.has("detail"),
      limit: Number(flags.get("limit") ?? 20),
    };

    const files = datasetArg ? [datasetFile(datasetArg)] : listDatasetFiles(from, to);
    if (files.length === 0) throw new Error("Neither build has any game-*.json dataset.");

    const results = files.map((file) => diffDataset(file, from, to, options));

    if (flags.has("json")) {
      console.log(JSON.stringify(results.length === 1 ? results[0] : results, null, 2));
      return;
    }
    for (const result of results) {
      console.log(renderDiff(result, { limit: options.limit }));
      console.log("");
    }
    return;
  }

  if (command === "deflect") {
    const wanted = requested.join(" ").trim().toLowerCase();
    if (!wanted) throw new Error("deflect needs a ship: sc-gamedata deflect <name or record name>");
    const dataDir = path.resolve(flags.get("out") ?? "data");
    const ships = JSON.parse(await fs.readFile(path.join(dataDir, "game-ships.json"), "utf8")) as {
      vehicles: Vehicle[];
    };
    const weapons = JSON.parse(await fs.readFile(path.join(dataDir, "game-weapons.json"), "utf8")) as {
      shipWeapons: Weapon[];
    };
    const armored = ships.vehicles.filter((v) => v.armor);
    const exact = armored.filter(
      (v) => v.name.toLowerCase() === wanted || v.recordName.toLowerCase().endsWith(`.${wanted}`),
    );
    const matches = exact.length > 0
      ? exact
      : armored.filter((v) => `${v.name} ${v.recordName}`.toLowerCase().includes(wanted));
    if (matches.length === 0) throw new Error(`No ship with armour matches "${wanted}".`);
    // Prefer the player hull over AI, template and event variants of the same name.
    const ship =
      matches.find((v) => !v.isTemplate && !/_(ai|pu|ea|template|pir|crim|unmanned)/i.test(v.recordName)) ??
      matches[0];
    const size = flags.has("size") ? Number(flags.get("size")) : undefined;
    const rows = deflectionCheck(ship.armor as Armor, weapons.shipWeapons, { size });
    if (flags.has("json")) {
      console.log(JSON.stringify({ ship: ship.recordName, armor: ship.armor, rows }, null, 2));
      return;
    }
    if (matches.length > 1) {
      console.log(`${matches.length} ships match; showing ${ship.recordName}. Use the record name to pick another.\n`);
    }
    console.log(renderDeflection(ship.name, ship.armor as Armor, rows, Number(flags.get("limit") ?? 30)));
    return;
  }

  if (command === "fetch") {
    const selected = selectApiDomains(requested);
    if (selected.length === 0) {
      throw new Error(
        `No API dataset matched. Known: ${API_DOMAINS.map((d) => d.name).join(", ")}`,
      );
    }

    console.log(`fetching ${selected.length} API dataset(s)`);
    const written = await fetchApiDatasets(
      path.resolve(flags.get("out") ?? "data"),
      selected,
      { token: flags.get("uex-token") },
      (m) => console.log(m),
    );

    console.log(`\nwrote ${written.length} dataset(s)`);
    for (const { dataset, collections } of written) {
      const parts = Object.entries(collections)
        .map(([key, count]) => `${key}=${count}`)
        .join(" ");
      console.log(`  ${dataset}  ${parts}`);
    }
    return;
  }

  const extractDir = path.resolve(flags.get("extract-dir") ?? "extracted-data");
  const outDir = path.resolve(flags.get("out") ?? "data");
  const domains = selectDomains(requested);

  if (command === "extract" || command === "all") {
    const bin = findStarBreaker(flags.get("starbreaker"));
    const p4k = findDataP4k(flags.get("p4k"), flags.get("channel") ?? "LIVE");

    console.log(`StarBreaker ${await starbreakerVersion(bin)}`);
    console.log(`Data.p4k    ${p4k}`);
    const build = await readBuildVersion(p4k);
    if (build) console.log(`build       ${build}`);
    console.log(`extracting  ${domains.map((d) => d.name).join(", ")}`);

    const results = await extract(
      { bin, p4k },
      extractDir,
      domains,
      (m) => console.log(m),
      flags.get("language"),
    );
    const total = results.reduce((sum, r) => sum + r.records, 0);
    console.log(`extracted ${total} records to ${extractDir}`);
    if (build) {
      await writeExtractedBuild(extractDir, build, {
        channel: (flags.get("channel") ?? "LIVE").toUpperCase(),
        branch: await readBuildBranch(p4k),
      });
    }

    if (total === 0) {
      throw new Error(
        "Extraction produced no records. Check the filters: DataCore globs match the full record path and `*` does not cross a `/`, so directory-spanning patterns need `**`.",
      );
    }
  }

  if (command === "parse" || command === "all") {
    // Prefer the build recorded at extract time; the installed game may have
    // moved on since. Fall back to the install for extractions made before the
    // marker existed.
    let build = await readExtractedBuild(extractDir);
    if (!build) {
      try {
        build = await readBuildVersion(findDataP4k(flags.get("p4k"), flags.get("channel") ?? "LIVE"));
      } catch {
        build = undefined;
      }
      if (build) {
        console.log(
          `build       ${build} (from the installed game; ${extractDir} carries no build marker)`,
        );
      }
    } else {
      console.log(`build       ${build} (recorded at extract time)`);
    }

    console.log(`parsing from ${extractDir}`);
    const written = await parse(extractDir, outDir, domains, build, (m) => console.log(m));
    if (build) {
      const kept = saveSnapshot(
        outDir,
        snapshotsDir(repoRoot()),
        build,
        written.map((w) => path.basename(w.file)),
      );
      console.log(`snapshot    ${kept.path} (${kept.datasets} dataset(s); diff against it as "${build}")`);
    }

    console.log(`\nwrote ${written.length} dataset(s) to ${outDir}`);
    for (const { dataset, collections } of written) {
      const parts = Object.entries(collections)
        .filter(([key]) => key !== "summary")
        .map(([key, count]) => `${key}=${count}`)
        .join(" ");
      console.log(`  ${dataset}  ${parts}`);
    }
    return;
  }

  if (command !== "extract") {
    console.error(`Unknown command "${command}"\n`);
    console.log(HELP);
    process.exitCode = 1;
  }
}

main().catch((error: unknown) => {
  console.error(`\nerror: ${error instanceof Error ? error.message : String(error)}`);
  process.exitCode = 1;
});
