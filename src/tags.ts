/**
 * The tag database, and a compactor that makes tag-heavy records readable.
 *
 * Every Tag lives in one record, TagDatabase, as a tree: `Global > Rarity >
 * Common`. Other records reference tags by GUID (`Tag.109ce0a4-...`), which is
 * meaningless on its own. Loot, law and mission records select items by tag
 * almost everywhere, so resolving the GUID to its path is what makes them usable.
 */

import type { GameRecord } from "./records.js";

/** `Tag.<guid>` record name -> path below the root, e.g. "Rarity/Common". */
export type TagIndex = Map<string, string>;

export function buildTagIndex(database: GameRecord | undefined): TagIndex {
  const index: TagIndex = new Map();
  const walk = (tag: Record<string, unknown>, path: string[]) => {
    const name = typeof tag.tagName === "string" ? tag.tagName : "?";
    // The root is always "Global"; leaving it in prefixes every path for nothing.
    const here = path.length === 0 && name === "Global" ? [] : [...path, name];
    if (typeof tag._RecordName_ === "string" && here.length > 0) index.set(tag._RecordName_, here.join("/"));
    for (const child of (tag.children as Record<string, unknown>[] | undefined) ?? []) {
      if (child) walk(child, here);
    }
  };
  for (const tag of (database?.value.tags as Record<string, unknown>[] | undefined) ?? []) {
    if (tag) walk(tag, []);
  }
  return index;
}

const DROPPED_KEYS = new Set(["_Type_", "_RecordPath_", "_RecordId_", "_Pointer_"]);

/** Last path segment of a `file://` reference, minus `.json`. */
function fileName(ref: string): string {
  return (ref.split("/").pop() ?? ref).replace(/\.json$/i, "");
}

/**
 * Strip a record value down to its data: type tags and record ids go, tag
 * references become tag paths, record references and `file://` paths become
 * names, and nulls and empty lists disappear.
 */
export function compactValue(value: unknown, tags: TagIndex): unknown {
  if (value === null || value === undefined) return undefined;
  if (typeof value === "string") {
    if (value.startsWith("file://")) return fileName(value);
    return value === "" ? undefined : value;
  }
  if (Array.isArray(value)) {
    const out = value.map((v) => compactValue(v, tags)).filter((v) => v !== undefined);
    return out.length > 0 ? out : undefined;
  }
  if (typeof value !== "object") return value;

  const obj = value as Record<string, unknown>;
  const recordName = obj._RecordName_;
  if (typeof recordName === "string") {
    if (recordName.startsWith("Tag.")) return tags.get(recordName) ?? recordName;
    // An inline or referenced record with no fields of its own beyond identity.
    const fields = Object.keys(obj).filter((k) => !DROPPED_KEYS.has(k) && k !== "_RecordName_");
    if (fields.length === 0) return recordName.slice(recordName.indexOf(".") + 1);
  }

  const out: Record<string, unknown> = {};
  for (const [key, entry] of Object.entries(obj)) {
    if (DROPPED_KEYS.has(key)) continue;
    const compacted = compactValue(entry, tags);
    if (compacted !== undefined) out[key] = compacted;
  }
  return Object.keys(out).length > 0 ? out : undefined;
}
