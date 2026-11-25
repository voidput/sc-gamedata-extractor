/**
 * game-search-index.json: the fuzzy-find entries, precomputed and small. Pure
 * half: build, serialise and load. File I/O is in src/search-index.ts; this
 * file lives in query/ so SCMCP can vendor it and read the published index.
 *
 * `find` (and SCMCP's `sc_find`) rank `Entry[]` from fuzzy.ts, which
 * `buildEntries` derives from game-item-tags.json (6 MB, every tag of every
 * item) and game-ships.json (6 MB, full ship records). Only six short fields
 * per entry survive that, so this file stores just those, for every entry
 * including world furniture and templates, each flagged as noise, so one file
 * serves both the default list and `--all`.
 *
 * Shape (format 1), chosen by measurement on 4.10.200 (14k entries): plain
 * objects with full field names came to 1.9 MB, short names 1.5 MB, so rows
 * are tuples with makers and types interned, about 0.85 MB:
 *
 *   { _source, _build, format: 1, columns, makers, types, items: Row[], ships: Row[] }
 *   Row = [key, name, maker, type, family, noise], trailing zeros trimmed
 *     name   0 when it equals key (unnamed records), else the display name
 *     maker  1-based index into makers, 0 for none
 *     type   1-based index into types, 0 for none
 *     family weapon family string, 0 for none
 *     noise  1 for an entry the default list drops
 *
 * `loadSearchIndexEntries(doc, { all })` gives back exactly
 * `buildEntries(items, ships, { all })`, same entries in the same order, so a
 * caller can read either source. The noise flag is not recomputed here: it is
 * read off the difference between the default and the `all` lists, so it can
 * never drift from fuzzy.ts's own filter.
 */

import { buildEntries, type Entry, type ShipLike } from "./fuzzy.js";
import type { ItemTags } from "./types.js";

export const SEARCH_INDEX_FILE = "game-search-index.json";
export const SEARCH_INDEX_FORMAT = 1;
const COLUMNS = ["key", "name", "maker", "type", "family", "noise"] as const;

/** [key, name|0, makerIndex, typeIndex, family|0, noise 0|1], trailing zeros trimmed. */
export type SearchIndexRow = [string, ...(string | number)[]];

export interface SearchIndexDoc {
  _source: string;
  _build?: string;
  format: number;
  columns: readonly string[];
  makers: string[];
  types: string[];
  items: SearchIndexRow[];
  ships: SearchIndexRow[];
}

const sameEntry = (a: Entry, b: Entry) =>
  a.kind === b.kind && a.key === b.key && a.name === b.name && a.maker === b.maker && a.type === b.type && a.family === b.family;

/** Builds the index document from the parsed datasets. Pure. */
export function buildSearchIndex(items: readonly ItemTags[], ships: readonly ShipLike[], build?: string): SearchIndexDoc {
  const all = buildEntries(items, ships, { all: true });
  const kept = buildEntries(items, ships);

  // The default list is a subsequence of the full one; whatever the walk skips is noise.
  const noise = new Array<boolean>(all.length).fill(true);
  for (let i = 0, j = 0; i < all.length && j < kept.length; i++) {
    if (sameEntry(all[i], kept[j])) {
      noise[i] = false;
      j++;
    }
  }

  const makers: string[] = [];
  const types: string[] = [];
  const makerIx = new Map<string, number>();
  const typeIx = new Map<string, number>();
  const intern = (value: string | undefined, list: string[], ix: Map<string, number>) => {
    if (value === undefined) return 0;
    let n = ix.get(value);
    if (n === undefined) {
      n = list.push(value);
      ix.set(value, n);
    }
    return n;
  };

  const doc: SearchIndexDoc = {
    _source: "Derived from game-item-tags.json and game-ships.json (see src/search-index.ts)",
    _build: build,
    format: SEARCH_INDEX_FORMAT,
    columns: COLUMNS,
    makers,
    types,
    items: [],
    ships: [],
  };
  all.forEach((e, i) => {
    const row: SearchIndexRow = [
      e.key,
      e.name === e.key ? 0 : e.name,
      intern(e.maker, makers, makerIx),
      intern(e.type, types, typeIx),
      e.family ?? 0,
      noise[i] ? 1 : 0,
    ];
    while (row.length > 1 && row[row.length - 1] === 0) row.pop();
    (e.kind === "item" ? doc.items : doc.ships).push(row);
  });
  return doc;
}

/**
 * Turns an index document back into the entries `buildEntries(items, ships, { all })`
 * returns. Throws on a format it does not know rather than guess.
 */
export function loadSearchIndexEntries(doc: SearchIndexDoc, options: { all?: boolean } = {}): Entry[] {
  if (doc?.format !== SEARCH_INDEX_FORMAT) throw new Error(`Unknown search index format ${String(doc?.format)}; expected ${SEARCH_INDEX_FORMAT}`);
  const entries: Entry[] = [];
  const decode = (kind: Entry["kind"], rows: readonly SearchIndexRow[]) => {
    for (const [key, name = 0, maker = 0, type = 0, family = 0, noise = 0] of rows) {
      if (noise && !options.all) continue;
      // Same key order and explicit undefineds as buildEntries, so the two are interchangeable.
      const entry: Entry = {
        kind,
        key,
        name: name === 0 ? key : String(name),
        maker: maker ? doc.makers[Number(maker) - 1] : undefined,
        type: type ? doc.types[Number(type) - 1] : undefined,
      };
      if (kind === "item") entry.family = family === 0 ? undefined : String(family);
      entries.push(entry);
    }
  };
  decode("item", doc.items ?? []);
  decode("ship", doc.ships ?? []);
  return entries;
}

/** One row per line: compact like a single-line dump, but a rebuild diffs line by line. */
export function serializeSearchIndex(doc: SearchIndexDoc): string {
  const rows = (list: SearchIndexRow[]) => (list.length ? `[\n${list.map((r) => JSON.stringify(r)).join(",\n")}\n]` : "[]");
  const head = ["_source", "_build", "format", "columns", "makers", "types"] as const;
  const fields = head.filter((k) => doc[k] !== undefined).map((k) => `${JSON.stringify(k)}: ${JSON.stringify(doc[k])}`);
  return `{\n${[...fields, `"items": ${rows(doc.items)}`, `"ships": ${rows(doc.ships)}`].join(",\n")}\n}\n`;
}
