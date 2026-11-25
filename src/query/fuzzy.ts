/**
 * Fuzzy find over items and ships: entries, an fzf-style scorer, and ranking.
 *
 * Pure and dependency-free, so the CLI's `find` and SCMCP's `sc_find` rank the
 * same way. The scorer favours a contiguous hit over a scattered one, a word
 * start over mid-word, the display name over the class name, and player hulls
 * over the AI and faction variants that share their name.
 *
 * A term with no contiguous hit may still match a word of the display name
 * within a small edit distance (a typo: "cambino" for "Cambio"). Tiers per
 * term: contiguous hit, then typo of a name word, then scattered letters. A
 * typo ranks above a scattered hit because a scattered hit is usually noise
 * ("gallent" is scattered through many long names but a typo of "Gallant").
 */

import type { ItemTags } from "./types.js";

export type EntryKind = "item" | "ship";

export interface Entry {
  kind: EntryKind;
  /** Class name for items, short record name for ships: what the other commands take. */
  key: string;
  name: string;
  maker?: string;
  /** Item type, or the ship's role. */
  type?: string;
  /** Weapon family from its tag (Weapon/FPS/Stocked/LMG/Verdict -> "Verdict"); names unnamed variants. */
  family?: string;
}

/** Leaf of the most specific Weapon/FPS family tag, e.g. "Verdict" or "Model-K2". */
export function weaponFamily(tags: readonly string[]): string | undefined {
  const families = tags.filter((t) => /^Weapon\/FPS\/(Stocked|Pistol|Shouldered|Melee)\/.+/.test(t));
  const deepest = families.sort((a, b) => b.split("/").length - a.split("/").length)[0];
  const parts = deepest?.split("/");
  // Weapon/FPS/Pistol/Hellion has the family at depth 4, Weapon/FPS/Stocked/LMG/Verdict at 5.
  if (!parts) return undefined;
  const minDepth = parts[2] === "Stocked" ? 5 : 4;
  return parts.length >= minDepth ? parts[parts.length - 1] : undefined;
}

/** AI, NPC and faction hulls share a player ship's name; they sort below it. */
const NPC_SHIP = /_(AI|PU_AI|CIV|CRIM|PIR|Template|Unmanned|EA)(_|$)/i;

export interface ShipLike {
  name: string;
  recordName: string;
  manufacturer?: string;
  role?: string;
  career?: string;
  isTemplate?: boolean | null;
}

/**
 * Item types that are world furniture rather than things a player looks up.
 * `--all` keeps them.
 */
const NOISE_TYPES = new Set(["Cargo", "Door", "Seat", "Usable", "UNDEFINED", "Light", "Room", "Display", "Elevator"]);

export function shipKey(recordName: string): string {
  return recordName.replace(/^EntityClassDefinition\./, "");
}

export function buildEntries(
  items: readonly ItemTags[],
  ships: readonly ShipLike[],
  options: { all?: boolean } = {},
): Entry[] {
  const entries: Entry[] = [];
  for (const item of items) {
    if (!options.all && (!item.type || NOISE_TYPES.has(item.type))) continue;
    const maker = item.tags.find((t) => t.startsWith("Manufacturer/"))?.slice("Manufacturer/".length) ?? item.manufacturer;
    const family = weaponFamily(item.tags);
    const name = item.isTemplate && family ? `${family} (unnamed: ${item.className})` : item.name;
    entries.push({ kind: "item", key: item.className, name, maker, type: item.type, family });
  }
  for (const ship of ships) {
    if (ship.isTemplate && !options.all) continue;
    entries.push({ kind: "ship", key: shipKey(ship.recordName), name: ship.name, maker: ship.manufacturer?.toUpperCase(), type: ship.role ?? ship.career });
  }
  return entries;
}

const fit = (s: string | undefined, width: number) => {
  const clean = (s ?? "").replace(/[\t\n]/g, " ");
  return clean.length > width ? `${clean.slice(0, width - 1)}…` : clean.padEnd(width);
};

/** Tab-separated: kind and key (hidden from view), then one padded column the user reads and searches. */
export function toLine(entry: Entry): string {
  const type = entry.kind === "ship" ? `ship · ${entry.type ?? ""}` : (entry.type ?? "");
  const shown = `${fit(entry.name, 46)} ${fit(type, 24)} ${fit(entry.maker, 16)} ${entry.key}`;
  return [entry.kind, entry.key, shown].join("\t");
}

export function parseLine(line: string): { kind: EntryKind; key: string } | undefined {
  const [kind, key] = line.split("\t");
  return (kind === "item" || kind === "ship") && key ? { kind, key } : undefined;
}

/**
 * fzf-style score for one term. A contiguous hit beats any scattered one, a
 * hit at a word start beats one mid-word, and shorter texts win ties; a term
 * whose characters only appear scattered in order still matches, weakly.
 * Undefined means no match.
 */
export function fuzzyScore(term: string, text: string): number | undefined {
  const q = term.toLowerCase().replace(/\s+/g, "");
  if (!q) return 0;
  const t = text.toLowerCase();
  const boundaryAt = (i: number) => i === 0 || /[\s_\-"(/.]/.test(t[i - 1]);

  let best: number | undefined;
  for (let i = t.indexOf(q); i >= 0; i = t.indexOf(q, i + 1)) {
    const s = 100 + (boundaryAt(i) ? 50 : 0) + (i + q.length === t.length || boundaryAt(i + q.length) ? 10 : 0);
    if (best === undefined || s > best) best = s;
  }
  if (best !== undefined) return best - t.length * 0.1;

  // Scattered: every character in order, rewarding runs and word starts.
  let score = 0;
  let from = 0;
  let run = 0;
  for (const ch of q) {
    const found = t.indexOf(ch, from);
    if (found < 0) return undefined;
    run = found === from && from > 0 ? run + 1 : 0;
    score += 1 + run * 2 + (boundaryAt(found) ? 3 : 0);
    from = found + 1;
  }
  return score - t.length * 0.1;
}

/**
 * Optimal string alignment distance (Damerau-Levenshtein where an adjacent swap
 * costs 1 and no substring is edited twice). Stops early and returns max + 1
 * once every alignment is past `max`.
 */
export function editDistance(a: string, b: string, max = Infinity): number {
  if (Math.abs(a.length - b.length) > max) return max + 1;
  let prev2: number[] = [];
  let prev = Array.from({ length: b.length + 1 }, (_, j) => j);
  for (let i = 1; i <= a.length; i++) {
    const row = [i];
    let rowMin = i;
    for (let j = 1; j <= b.length; j++) {
      const cost = a[i - 1] === b[j - 1] ? 0 : 1;
      let d = Math.min(prev[j] + 1, row[j - 1] + 1, prev[j - 1] + cost);
      if (i > 1 && j > 1 && a[i - 1] === b[j - 2] && a[i - 2] === b[j - 1]) d = Math.min(d, prev2[j - 2] + 1);
      row.push(d);
      if (d < rowMin) rowMin = d;
    }
    if (rowMin > max) return max + 1;
    prev2 = prev;
    prev = row;
  }
  return prev[b.length];
}

/** Edits a typo may carry: none under 4 characters, 1 for 4-6, 2 for 7 and up. */
export const typoBudget = (length: number): number => (length >= 7 ? 2 : length >= 4 ? 1 : 0);

/** Below every contiguous hit (those stay above about 80 for any real name); minus 20 per edit. */
const TYPO_BASE = 0;

/** A scattered-only hit is capped and shifted below every typo hit (-40 at worst). */
const SCATTER_CAP = 50;
const SCATTER_SHIFT = -100;

/** Lower-case words of a name: letters and digits. */
export const nameWords = (name: string): string[] => name.toLowerCase().split(/[^a-z0-9]+/).filter(Boolean);

/** Score for a term that is a typo of one of `words`; undefined when none is close enough. */
export function typoScore(term: string, words: readonly string[]): number | undefined {
  const q = term.toLowerCase().replace(/\s+/g, "");
  const budget = typoBudget(q.length);
  if (budget === 0) return undefined;
  let best: number | undefined;
  for (const word of words) {
    const d = editDistance(q, word, budget);
    if (d <= budget && (best === undefined || d < best)) best = d;
    if (best === 1) break;
  }
  return best === undefined ? undefined : TYPO_BASE - best * 20;
}

const wordCache = new WeakMap<Entry, string[]>();
function entryWords(entry: Entry): string[] {
  let words = wordCache.get(entry);
  if (!words) {
    words = nameWords(entry.name);
    wordCache.set(entry, words);
  }
  return words;
}

/** Entries matching every space-separated term, best first. The display name counts most; typos beat scattered letters. */
export function rankEntries(entries: readonly Entry[], query: string): Entry[] {
  const terms = query.trim().split(/\s+/).filter(Boolean);
  if (terms.length === 0) return [...entries];
  const scored: { entry: Entry; score: number }[] = [];
  for (const entry of entries) {
    const rest = `${entry.key} ${entry.family ?? ""} ${entry.maker ?? ""} ${entry.type ?? ""}`;
    let total = (entry.name === entry.key ? -5 : 0) + (entry.kind === "ship" && NPC_SHIP.test(entry.key) ? -15 : 0);
    let ok = true;
    for (const term of terms) {
      const score = termScore(term, entry, rest);
      if (score === undefined) {
        ok = false;
        break;
      }
      total += score;
    }
    if (ok) scored.push({ entry, score: total });
  }
  return scored.sort((a, b) => b.score - a.score).map((s) => s.entry);
}

/** One term against one entry: contiguous hit, else typo of a name word, else scattered (shifted down). */
function termScore(term: string, entry: Entry, rest: string): number | undefined {
  const q = term.toLowerCase();
  const byName = fuzzyScore(term, entry.name);
  const byRest = fuzzyScore(term, rest);
  const contiguous = entry.name.toLowerCase().includes(q) || rest.toLowerCase().includes(q);
  const best = Math.max(byName === undefined ? -Infinity : byName + 10, byRest ?? -Infinity);
  if (contiguous) return best;
  const typo = typoScore(term, entryWords(entry));
  // Shorter names win ties, as with contiguous hits.
  if (typo !== undefined) return typo - entry.name.length * 0.1;
  return best === -Infinity ? undefined : Math.min(best, SCATTER_CAP) + SCATTER_SHIFT;
}

export function renderShip(ship: Record<string, unknown>): string {
  const fields: [string, unknown][] = [
    ["maker", typeof ship.manufacturer === "string" ? ship.manufacturer.toUpperCase() : undefined],
    ["career", ship.career],
    ["role", ship.role],
    ["size", ship.size],
    ["crew", ship.crewSize],
    ["movement", ship.movementClass],
    ["record", typeof ship.recordName === "string" ? shipKey(ship.recordName) : undefined],
  ];
  const lines = [String(ship.displayName ?? ship.name)];
  for (const [label, value] of fields) {
    if (value !== undefined && value !== null && value !== "") lines.push(`  ${label.padEnd(9)} ${String(value)}`);
  }
  if (typeof ship.description === "string") lines.push("", ship.description.replace(/\\n/g, "\n"));
  lines.push("", "ctrl-e engage · ctrl-d deflection · ctrl-j JSON");
  return lines.join("\n");
}
