/**
 * Helpers for EntityClassDefinition records.
 *
 * An entity is a bag of typed component structs in a `Components` array. Nearly
 * every domain needs to pull one out by type, so that lives here rather than
 * being reimplemented per domain.
 */

import type { GameRecord } from "./records.js";

export function components(record: GameRecord): Record<string, unknown>[] {
  const list = record.value.Components;
  if (!Array.isArray(list)) return [];
  return list.filter((c): c is Record<string, unknown> => !!c && typeof c === "object");
}

/** The first component of the given type, if the entity has one. */
export function component(record: GameRecord, type: string): Record<string, unknown> | undefined {
  return components(record).find((c) => c._Type_ === type);
}

/** Every component whose type contains the given substring. */
export function componentsLike(record: GameRecord, fragment: string): Record<string, unknown>[] {
  return components(record).filter(
    (c) => typeof c._Type_ === "string" && c._Type_.includes(fragment),
  );
}

/** True when the entity carries a component of the given type. */
export function hasComponent(record: GameRecord, type: string): boolean {
  return component(record, type) !== undefined;
}

/** A Vec3 or Vec2 struct as a plain object, dropping the type tag. */
export function vec(value: unknown): Record<string, number> | undefined {
  if (!value || typeof value !== "object") return undefined;
  const out: Record<string, number> = {};
  for (const [key, entry] of Object.entries(value as Record<string, unknown>)) {
    if (key !== "_Type_" && typeof entry === "number") out[key] = entry;
  }
  return Object.keys(out).length > 0 ? out : undefined;
}

/**
 * The record name a `file://...json` reference points at.
 *
 * References inside components are plain path strings rather than the
 * `_RecordName_` objects used elsewhere, so they need their own accessor.
 */
export function referencedName(value: unknown): string | undefined {
  if (typeof value !== "string" || value.length === 0) return undefined;
  const file = value.split("/").pop();
  if (!file) return undefined;
  // Some references carry a "scitemmanufacturer.aegs.json" style double name.
  return file.replace(/\.json$/i, "").split(".").pop();
}
