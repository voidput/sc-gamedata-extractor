/**
 * The game's string table.
 *
 * Records store display text as keys, not text: a manufacturer's name is
 * `@manufacturer_NameSASU`, which resolves to "Sakura Sun". Without the table
 * every human-readable field comes out as an `@`-prefixed token.
 *
 * The file lives at `Data/Localization/english/global.ini` inside Data.p4k. It
 * is UTF-8 with a BOM, CRLF line endings, and `key=value` lines where the value
 * may itself contain `=`.
 */

import fs from "node:fs/promises";
import path from "node:path";

import { findJsonFiles } from "./records.js";

export type Localization = Map<string, string>;

/** Parse a global.ini into a key/value map. */
export function parseGlobalIni(text: string): Localization {
  const table: Localization = new Map();
  // Strip the BOM so the very first key does not carry an invisible prefix.
  const body = text.charCodeAt(0) === 0xfeff ? text.slice(1) : text;

  for (const rawLine of body.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (!line || line.startsWith(";") || line.startsWith("#") || line.startsWith("[")) continue;

    const split = line.indexOf("=");
    if (split <= 0) continue;

    // Values can contain '=', so only the first one separates key from value.
    table.set(line.slice(0, split), line.slice(split + 1));
  }

  return table;
}

/** Find global.ini under an extraction directory, wherever the p4k paths put it. */
export async function findGlobalIni(extractDir: string): Promise<string | undefined> {
  const stack = [extractDir];

  while (stack.length > 0) {
    const dir = stack.pop() as string;
    let entries;
    try {
      entries = await fs.readdir(dir, { withFileTypes: true });
    } catch {
      continue;
    }
    for (const entry of entries) {
      const full = path.join(dir, entry.name);
      if (entry.isDirectory()) stack.push(full);
      else if (entry.name.toLowerCase() === "global.ini") return full;
    }
  }

  return undefined;
}

export async function loadLocalization(extractDir: string): Promise<Localization> {
  const file = await findGlobalIni(extractDir);
  if (!file) return new Map();
  return parseGlobalIni(await fs.readFile(file, "utf-8"));
}

/**
 * Apply a string overlay on top of a base table.
 *
 * Players commonly run a community replacement for global.ini, such as
 * MrKraken's StarStrings, which rewrites text for readability: components gain a
 * `Cmp/2/C` type-size-grade prefix, ores carry their RS value, and contracts
 * list the blueprints they can award. What a player sees in game is then the
 * overlay, not the shipped string, so a tool reading only the base is describing
 * a game they are not playing.
 *
 * Overlays use the same `key=value` format, so they layer cleanly. Later
 * overlays win. The base is not mutated.
 */
export function applyOverlay(base: Localization, overlay: Localization): Localization {
  const merged = new Map(base);
  for (const [key, value] of overlay) merged.set(key, value);
  return merged;
}

/** Load every .ini under a file or directory as one overlay table. */
export async function loadOverlay(target: string): Promise<Localization> {
  const stat = await fs.stat(target);

  const files: string[] = [];
  if (stat.isDirectory()) {
    for (const entry of await fs.readdir(target, { withFileTypes: true })) {
      if (entry.isFile() && entry.name.toLowerCase().endsWith(".ini")) {
        files.push(path.join(target, entry.name));
      }
    }
    files.sort();
  } else {
    files.push(target);
  }

  let table: Localization = new Map();
  for (const file of files) {
    table = applyOverlay(table, parseGlobalIni(await fs.readFile(file, "utf-8")));
  }
  return table;
}

export interface StringChange {
  key: string;
  base?: string;
  overlay: string;
}

/** What an overlay changes relative to a base table. */
export function diffStrings(
  base: Localization,
  overlay: Localization,
): { changed: StringChange[]; added: StringChange[] } {
  const changed: StringChange[] = [];
  const added: StringChange[] = [];

  for (const [key, value] of overlay) {
    const original = base.get(key);
    if (original === undefined) added.push({ key, overlay: value });
    else if (original !== value) changed.push({ key, base: original, overlay: value });
  }

  return { changed, added };
}

export interface StringHit {
  key: string;
  value: string;
}

/**
 * Search the string table by key or value.
 *
 * Useful in both directions: finding what a `@key` on a record resolves to, and
 * finding the key behind a string seen in game.
 */
export function searchStrings(
  table: Localization,
  query: string,
  options: { limit?: number; keysOnly?: boolean } = {},
): { total: number; hits: StringHit[] } {
  const needle = query.toLowerCase();
  const limit = options.limit ?? 25;
  const hits: StringHit[] = [];
  let total = 0;

  for (const [key, value] of table) {
    const matched =
      key.toLowerCase().includes(needle) ||
      (!options.keysOnly && value.toLowerCase().includes(needle));
    if (!matched) continue;

    total += 1;
    if (hits.length < limit) hits.push({ key, value });
  }

  return { total, hits };
}

/** Exact lookup, accepting a key with or without its leading "@". */
export function lookupString(table: Localization, key: string): string | undefined {
  const bare = key.startsWith("@") ? key.slice(1) : key;
  const hit = table.get(bare) ?? table.get(key);
  if (hit !== undefined) return hit;

  const lower = bare.toLowerCase();
  for (const [candidate, value] of table) {
    if (candidate.toLowerCase() === lower) return value;
  }
  return undefined;
}

/**
 * Resolve a possibly-localized string.
 *
 * Keys appear with and without the leading `@`, and lookups are
 * case-insensitive in practice, so try the obvious variants before giving up.
 * Returns undefined for an unset value so callers can omit the field rather
 * than emitting a token.
 */
export function localize(table: Localization, value: unknown): string | undefined {
  if (typeof value !== "string" || value.length === 0) return undefined;
  if (value === "@LOC_EMPTY") return undefined;

  if (!value.startsWith("@")) return value;

  const key = value.slice(1);
  const hit = table.get(key) ?? table.get(value);
  if (hit !== undefined) return hit.length > 0 ? hit : undefined;

  // Any case, then the `,P` form some keys exist only in (the game resolves
  // `@item_NameX` to `item_NameX,P`).
  const found = tableKey(table, key);
  if (found !== undefined) {
    const text = table.get(found) as string;
    return text.length > 0 ? text : undefined;
  }

  // Unresolved: hand back the token so the caller can see what is missing.
  return value;
}

/** True when localize returned a token it could not resolve. */
export function isUnresolved(value: string | undefined): boolean {
  return typeof value === "string" && value.startsWith("@");
}

/**
 * True when a resolved string is not actually a name.
 *
 * Unfinished records carry placeholders in several shapes, and each was found
 * by seeing one ship as a display name:
 *
 *   "@item_NameFoo"      an unresolved key
 *   "@LOC_PLACEHOLDER"   an explicit placeholder key
 *   "<= PLACEHOLDER =>"  the marker CIG uses in item records
 *   "< = TEST NAME = >"  the same marker, spaced, used in mission records
 *   ""                   simply unset
 *   "PH - shoes_01"      a "PH -" prefix on unfinished items (seen once `,P` keys resolve)
 *
 * Matching on "<=" alone lets the spaced variant through, so the angle brackets
 * are checked with the interior whitespace removed.
 */
export function isPlaceholderName(name: string | undefined): boolean {
  if (!name) return true;
  if (name.startsWith("@")) return true;

  const lower = name.toLowerCase();
  if (lower.includes("placeholder") || /^ph\s*-/.test(lower)) return true;

  const squashed = lower.replace(/\s+/g, "");
  return squashed.includes("<=") || squashed.includes("=>") || squashed.includes("testname");
}

export { findJsonFiles };

/**
 * What a record's name reference resolves to, for consumers that need to tell a
 * real name from a gap. Domains swap placeholders for the class name, so the
 * `name` field alone never says whether the game has a name for the record.
 *
 *   real         resolves to a usable name
 *   placeholder  resolves, or is literal text, but is a placeholder marker
 *   unresolved   an `@` key the table has no entry for, even with `,P`
 *   empty        unset, `@LOC_EMPTY`, `@LOC_UNINITIALIZED`, or a blank value
 */
export type NameStatus = "real" | "placeholder" | "unresolved" | "empty";

const EMPTY_KEYS = new Set(["", "loc_empty", "loc_uninitialized"]);

const lowerIndexes = new WeakMap<Localization, Map<string, string>>();

/** Lower-case key -> first key with that form, built once per table. */
function lowerIndex(table: Localization): Map<string, string> {
  let index = lowerIndexes.get(table);
  if (!index) {
    index = new Map();
    for (const key of table.keys()) {
      const lower = key.toLowerCase();
      if (!index.has(lower)) index.set(lower, key);
    }
    lowerIndexes.set(table, index);
  }
  return index;
}

/** The table's own spelling of a bare key: exact, then any case, then the same with `,P`. */
function tableKey(table: Localization, bare: string): string | undefined {
  if (table.has(bare)) return bare;
  const index = lowerIndex(table);
  const lower = bare.toLowerCase();
  return index.get(lower) ?? (table.has(`${bare},P`) ? `${bare},P` : index.get(`${lower},p`));
}

/**
 * The string-table key a field references, without its `@`, spelled as the
 * table spells it (including a `,P` suffix the reference leaves off). An
 * unresolved key is returned as referenced, so the gap stays visible. Literal
 * text, unset values and the empty markers give undefined.
 */
export function locKey(table: Localization, value: unknown): string | undefined {
  if (typeof value !== "string" || !value.startsWith("@")) return undefined;
  const bare = value.slice(1);
  if (EMPTY_KEYS.has(bare.toLowerCase())) return undefined;
  return tableKey(table, bare) ?? bare;
}

/** Classify a name reference; see NameStatus. */
export function nameStatusOf(table: Localization, value: unknown): NameStatus {
  if (typeof value !== "string" || value.length === 0) return "empty";
  if (!value.startsWith("@")) return isPlaceholderName(value) ? "placeholder" : "real";
  const bare = value.slice(1);
  if (EMPTY_KEYS.has(bare.toLowerCase())) return "empty";
  const key = tableKey(table, bare);
  if (key === undefined) return "unresolved";
  const text = (table.get(key) as string).trim();
  if (text.length === 0) return "empty";
  return isPlaceholderName(text) ? "placeholder" : "real";
}

export interface LocRefs {
  nameKey?: string;
  descriptionKey?: string;
  /** Absent when the name is real, the common case, to keep dumps small. */
  nameStatus?: NameStatus;
}

/** A NameStatus as a dataset field: omitted when "real". */
export function nameStatusField(table: Localization, value: unknown): NameStatus | undefined {
  const status = nameStatusOf(table, value);
  return status === "real" ? undefined : status;
}

/** Key refs for a record's name and description references. */
export function locRefs(table: Localization, name: unknown, description?: unknown): LocRefs {
  return {
    nameKey: locKey(table, name),
    descriptionKey: locKey(table, description),
    nameStatus: nameStatusField(table, name),
  };
}
