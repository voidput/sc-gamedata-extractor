/**
 * Writes game-search-index.json, the precomputed fuzzy-find entries, beside
 * the datasets it is derived from. The format and the pure build/load code are
 * in query/search-index.ts (vendored by SCMCP); this file only does the I/O.
 */

import fs from "node:fs";
import path from "node:path";

import type { ShipLike } from "./query/fuzzy.js";
import { buildSearchIndex, SEARCH_INDEX_FILE, serializeSearchIndex } from "./query/search-index.js";
import type { ItemTags } from "./query/types.js";

export {
  buildSearchIndex,
  loadSearchIndexEntries,
  SEARCH_INDEX_FILE,
  SEARCH_INDEX_FORMAT,
  serializeSearchIndex,
  type SearchIndexDoc,
  type SearchIndexRow,
} from "./query/search-index.js";

const readJson = <T>(file: string): T | undefined => {
  try {
    return JSON.parse(fs.readFileSync(file, "utf8")) as T;
  } catch {
    return undefined;
  }
};

/**
 * Reads game-item-tags.json and game-ships.json from dataDir and writes
 * game-search-index.json beside them. Returns undefined, writing nothing, when
 * either input is missing or unreadable: half an index would publish a find
 * list silently missing all items or all ships. An index already there is
 * removed in that case, since it no longer restates the files beside it and a
 * reader (SCMCP's sc_find) would otherwise serve its stale entries.
 */
export function writeSearchIndex(dataDir: string): { file: string; entries: number; bytes: number } | undefined {
  const tags = readJson<{ _build?: string; items?: ItemTags[] }>(path.join(dataDir, "game-item-tags.json"));
  const ships = readJson<{ _build?: string; vehicles?: ShipLike[] }>(path.join(dataDir, "game-ships.json"));
  const file = path.join(dataDir, SEARCH_INDEX_FILE);
  if (!Array.isArray(tags?.items) || !Array.isArray(ships?.vehicles)) {
    fs.rmSync(file, { force: true });
    return undefined;
  }

  const doc = buildSearchIndex(tags.items, ships.vehicles, tags._build ?? ships._build);
  const body = serializeSearchIndex(doc);
  fs.writeFileSync(file, body);
  return { file, entries: doc.items.length + doc.ships.length, bytes: Buffer.byteLength(body) };
}
