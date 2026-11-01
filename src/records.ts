/**
 * Loading and navigating extracted DataCore records.
 *
 * Every extracted file has the same envelope:
 *
 *   {
 *     "_RecordName_": "MineableElement.Quantainium_Raw",
 *     "_RecordId_":   "fc62043c-...",
 *     "_RecordValue_": { "_Type_": "MineableElement", ...fields }
 *   }
 *
 * References to other records appear as objects carrying `_RecordName_` and
 * `_RecordId_` rather than being inlined, so a parser resolves them by name.
 */

import fs from "node:fs/promises";
import path from "node:path";

export interface RawRecord {
  _RecordName_?: string;
  _RecordId_?: string;
  _RecordValue_?: Record<string, unknown>;
  [key: string]: unknown;
}

export interface GameRecord {
  /** Full record name, e.g. "MineableElement.Quantainium_Raw". */
  name: string;
  /** Record name with its type prefix removed, e.g. "Quantainium_Raw". */
  shortName: string;
  id: string;
  type: string;
  /** Path relative to the extraction root, without the .json suffix. */
  recordPath: string;
  value: Record<string, unknown>;
}

/** Recursively collect .json files under a directory. */
export async function findJsonFiles(dir: string): Promise<string[]> {
  const found: string[] = [];

  async function walk(current: string) {
    let entries;
    try {
      entries = await fs.readdir(current, { withFileTypes: true });
    } catch {
      return;
    }
    for (const entry of entries) {
      const full = path.join(current, entry.name);
      if (entry.isDirectory()) await walk(full);
      else if (entry.name.endsWith(".json")) found.push(full);
    }
  }

  await walk(dir);
  return found.sort();
}

export function normalizeRecord(raw: RawRecord, recordPath: string): GameRecord | undefined {
  const name = typeof raw._RecordName_ === "string" ? raw._RecordName_ : undefined;
  const value = raw._RecordValue_;
  if (!name || !value || typeof value !== "object") return undefined;

  const type = typeof value._Type_ === "string" ? value._Type_ : name.split(".")[0];
  const prefix = `${type}.`;
  return {
    name,
    shortName: name.startsWith(prefix) ? name.slice(prefix.length) : name,
    id: typeof raw._RecordId_ === "string" ? raw._RecordId_ : "",
    type,
    recordPath,
    value: value as Record<string, unknown>,
  };
}

/** Load every record under `dir`, skipping files that are not record envelopes. */
export async function loadRecords(dir: string): Promise<GameRecord[]> {
  const files = await findJsonFiles(dir);
  const records: GameRecord[] = [];

  for (const file of files) {
    let parsed: RawRecord;
    try {
      parsed = JSON.parse(await fs.readFile(file, "utf-8"));
    } catch {
      continue;
    }
    const relative = path.relative(dir, file).replace(/\\/g, "/").replace(/\.json$/, "");
    const record = normalizeRecord(parsed, relative);
    if (record) records.push(record);
  }

  return records;
}

/** The `_RecordName_` a reference field points at, if it is a reference. */
export function refName(value: unknown): string | undefined {
  if (value && typeof value === "object" && !Array.isArray(value)) {
    const name = (value as Record<string, unknown>)._RecordName_;
    if (typeof name === "string") return name;
  }
  return undefined;
}

/** Reference name with its type prefix removed: "ResourceType.Raw_Gold" -> "Raw_Gold". */
export function refShortName(value: unknown): string | undefined {
  const name = refName(value);
  if (!name) return undefined;
  const dot = name.indexOf(".");
  return dot >= 0 ? name.slice(dot + 1) : name;
}

/** Read a dotted path out of a record value. */
export function at(value: unknown, dottedPath: string): unknown {
  let current = value;
  for (const segment of dottedPath.split(".")) {
    if (!current || typeof current !== "object") return undefined;
    current = (current as Record<string, unknown>)[segment];
  }
  return current;
}

export function num(value: unknown): number | undefined {
  return typeof value === "number" && Number.isFinite(value) ? value : undefined;
}

export function str(value: unknown): string | undefined {
  return typeof value === "string" && value.length > 0 ? value : undefined;
}

/**
 * Order names as a person reads them: numbers by value ("_2" before "_10"),
 * case and punctuation ignored, so `"Quantainium" Water Bottle` files under Q
 * and `agricium_ore` sits beside `Agricium`.
 */
const NAME_COLLATOR = new Intl.Collator("en", { numeric: true, sensitivity: "base", ignorePunctuation: true });

export function compareNames(a: string, b: string): number {
  return NAME_COLLATOR.compare(a, b) || (a < b ? -1 : a > b ? 1 : 0);
}

/** Index records by full name for reference resolution. */
export function indexByName(records: GameRecord[]): Map<string, GameRecord> {
  return new Map(records.map((r) => [r.name, r]));
}
