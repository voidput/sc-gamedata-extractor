/**
 * Community string overlays.
 *
 * Players commonly replace the game's `global.ini` with a community pack that
 * rewrites text for readability. StarStrings is the best known, but it is not
 * the only one and anyone can build their own, so nothing here is specific to it
 * beyond one entry in the default registry.
 *
 * An overlay is just `key=value` lines in the same format as the base table, so
 * a pack needs no special support to be usable: point `--overlay` at a file or
 * directory. The registry exists so a pack can be fetched by name and refetched
 * after a patch without anyone remembering a URL.
 */

import fs from "node:fs/promises";
import path from "node:path";

export interface OverlaySource {
  name: string;
  description: string;
  /** Where to read about it. */
  homepage?: string;
  /**
   * Files to download, as full URLs. A pack that ships one merged file lists
   * one; a pack split by category lists several, applied in order.
   */
  files: string[];
  /**
   * Packs are usually rebuilt per game patch, so an old copy should not be
   * layered onto a new build. Set false for a pack that is build-independent.
   */
  perPatch?: boolean;
}

const RAW = "https://raw.githubusercontent.com";

/**
 * Known packs. This is a convenience, not a gate: any pack works through
 * `--overlay`, and users can add their own sources without touching this file.
 */
export const BUILT_IN_SOURCES: OverlaySource[] = [
  {
    name: "starstrings",
    description: "MrKraken's StarStrings. Component type/size/grade prefixes, ore RS values, blueprint pools on contracts.",
    homepage: "https://github.com/MrKraken/StarStrings",
    // The project publishes these for tool authors, rather than only a merged file.
    files: [
      `${RAW}/MrKraken/StarStrings/master/src/For_Tool_Creators/components.ini`,
      `${RAW}/MrKraken/StarStrings/master/src/For_Tool_Creators/contracts.ini`,
      `${RAW}/MrKraken/StarStrings/master/src/For_Tool_Creators/mining.ini`,
      `${RAW}/MrKraken/StarStrings/master/src/For_Tool_Creators/ordnance.ini`,
    ],
    perPatch: true,
  },
];

/** Where user-defined sources live, so the registry is not code-only. */
export function sourcesFile(configDir = "."): string {
  return path.join(configDir, "overlay-sources.json");
}

/**
 * Load the registry: built-ins plus anything the user has defined. A user entry
 * with the same name replaces the built-in, so a pack can be repointed at a fork
 * or a pinned tag without editing the source.
 */
export async function loadSources(configDir = "."): Promise<OverlaySource[]> {
  const merged = new Map(BUILT_IN_SOURCES.map((s) => [s.name, s]));

  try {
    const raw = await fs.readFile(sourcesFile(configDir), "utf-8");
    const parsed = JSON.parse(raw);
    const entries: unknown[] = Array.isArray(parsed) ? parsed : (parsed?.sources ?? []);

    for (const entry of entries) {
      const source = validateSource(entry);
      merged.set(source.name, source);
    }
  } catch (error) {
    // A missing file is the normal case; a malformed one is worth reporting.
    if ((error as NodeJS.ErrnoException)?.code !== "ENOENT") {
      throw new Error(
        `Could not read ${sourcesFile(configDir)}: ${error instanceof Error ? error.message : String(error)}`,
        { cause: error },
      );
    }
  }

  return [...merged.values()].sort((a, b) => a.name.localeCompare(b.name));
}

export function validateSource(entry: unknown): OverlaySource {
  if (!entry || typeof entry !== "object") throw new Error("Overlay source must be an object.");
  const obj = entry as Record<string, unknown>;

  const name = typeof obj.name === "string" ? obj.name.trim() : "";
  if (!name) throw new Error("Overlay source needs a name.");
  if (!/^[a-z0-9._-]+$/i.test(name)) {
    throw new Error(`Overlay source name "${name}" may only contain letters, digits, . _ and -.`);
  }

  const files = Array.isArray(obj.files) ? obj.files.filter((f) => typeof f === "string") : [];
  if (files.length === 0) throw new Error(`Overlay source "${name}" lists no files.`);

  for (const file of files) {
    // Only fetch over HTTPS; a local pack should be used with --overlay instead.
    if (!/^https:\/\//i.test(file as string)) {
      throw new Error(`Overlay source "${name}" has a non-HTTPS URL: ${file}`);
    }
  }

  return {
    name,
    description: typeof obj.description === "string" ? obj.description : "",
    homepage: typeof obj.homepage === "string" ? obj.homepage : undefined,
    files: files as string[],
    perPatch: obj.perPatch !== false,
  };
}

/** The file name a URL ends with, used as the local name. */
function fileNameOf(url: string, index: number): string {
  const tail = url.split("?")[0].split("/").pop();
  if (tail && /\.ini$/i.test(tail)) return tail;
  return `${String(index).padStart(2, "0")}-${tail || "overlay"}.ini`;
}

/**
 * Download a source's files into `outDir`.
 *
 * Files keep their published order in their names so a directory load applies
 * them the same way every time.
 */
export async function fetchSource(
  source: OverlaySource,
  outDir: string,
  fetchImpl: typeof fetch = fetch,
): Promise<{ file: string; bytes: number }[]> {
  await fs.mkdir(outDir, { recursive: true });
  const written: { file: string; bytes: number }[] = [];

  for (const [index, url] of source.files.entries()) {
    const response = await fetchImpl(url);
    if (!response.ok) {
      throw new Error(`${source.name}: ${url} returned HTTP ${response.status}`);
    }

    const text = await response.text();
    // Prefix with the index so lexical order matches published order.
    const file = path.join(outDir, `${String(index).padStart(2, "0")}-${fileNameOf(url, index)}`);
    await fs.writeFile(file, text);
    written.push({ file, bytes: text.length });
  }

  return written;
}

/**
 * Find a string pack already installed in the game directory.
 *
 * A pack is installed as a loose `data/Localization/<language>/global.ini` beside
 * the archive, which the game reads in preference to the archived table. If one
 * is there, that is what the player actually sees, whichever pack it came from.
 */
export async function findInstalledOverride(
  p4kPath: string,
  language = "english",
): Promise<string | undefined> {
  const gameDir = path.dirname(p4kPath);
  const candidates = [
    path.join(gameDir, "data", "Localization", language, "global.ini"),
    path.join(gameDir, "Data", "Localization", language, "global.ini"),
  ];

  for (const candidate of candidates) {
    try {
      const stat = await fs.stat(candidate);
      if (stat.isFile()) return candidate;
    } catch {
      continue;
    }
  }

  return undefined;
}
