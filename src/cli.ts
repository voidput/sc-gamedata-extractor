#!/usr/bin/env node
/**
 * sc-gamedata — extract Star Citizen game data into structured JSON.
 *
 *   sc-gamedata extract      pull DataCore records out of Data.p4k
 *   sc-gamedata parse        turn extracted records into dataset JSON
 *   sc-gamedata all          both, in order
 *   sc-gamedata domains      list what can be built
 */

import path from "node:path";

import {
  API_DOMAINS,
  DOMAINS,
  extract,
  fetchApiDatasets,
  parse,
  readBuildVersion,
  selectApiDomains,
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
  fetchSource,
  findInstalledOverride,
  loadSources,
  sourcesFile,
} from "./sources/overlays.js";
import { LANGUAGES } from "./starbreaker.js";
import { findDataP4k, findStarBreaker, starbreakerVersion } from "./starbreaker.js";

interface Args {
  command: string;
  flags: Map<string, string>;
  domains: string[];
}

/**
 * Flags that take no value. Without this list a boolean flag swallows the next
 * positional, so `strings --starstrings Ouratite` loses the search term.
 */
const BOOLEAN_FLAGS = new Set(["help", "write", "keys-only", "diff", "installed"]);

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

StarBreaker: https://github.com/diogotr7/StarBreaker/releases`;

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

    if (total === 0) {
      throw new Error(
        "Extraction produced no records. Check the filters: DataCore globs match the full record path and `*` does not cross a `/`, so directory-spanning patterns need `**`.",
      );
    }
  }

  if (command === "parse" || command === "all") {
    let build: string | undefined;
    try {
      build = await readBuildVersion(findDataP4k(flags.get("p4k"), flags.get("channel") ?? "LIVE"));
    } catch {
      build = undefined;
    }

    console.log(`parsing from ${extractDir}`);
    const written = await parse(extractDir, outDir, domains, build, (m) => console.log(m));

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
