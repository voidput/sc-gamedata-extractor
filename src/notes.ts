/**
 * Player-facing patch notes between two builds.
 *
 * `diff` lists every field that moved and `changes` gives chart rows; both are
 * too raw to read as a patch summary (a 4.10.197 -> 4.10.200 diff is ~70 KB of
 * JSON). This module turns the same comparison into a short document: new
 * ships and gear, buffs and nerfs, crafting and mission rewards, loot sources
 * and renames, with everything else reduced to per-dataset counts.
 *
 * Rules that keep it short and honest:
 *  - One source per kind of record: personal weapons come from
 *    game-weapons.fpsWeapons (melee from game-fps-weapons), ship weapons and
 *    missiles from game-weapons, the rest of the ship parts from
 *    game-ship-components minus its own weapons/missiles copies.
 *  - Variants collapse: weapons group by weapon family (from item tags) or by
 *    base class name with tint/store/camo/... suffixes stripped, armour pieces
 *    by set. Templates, props and AI/NPC hulls are only ever counted.
 *  - Numeric moves under `minChange` percent, and fields that only exist on
 *    one side (a parser that learned a new field), are hidden and counted.
 *  - Stat moves for tracked categories come from changes.ts (patchChanges), so
 *    buff/nerf direction is the same as the `changes` command. A stat that
 *    was 0 (a value the old build never filled) has no percent and no
 *    buff/nerf verdict. Tracked records that changed only in other fields
 *    (loadouts, volumes) get no line but are counted under their section.
 *  - Vanduul hulls and weapons (VNCL_*) are NPC-only and only ever counted.
 *  - A redacted (published) string table against a full local one is
 *    compared on published name keys only (diff.ts alignRedaction).
 */

import path from "node:path";

import {
  alignRedaction,
  datasetFile,
  diffableCollections,
  diffCollection,
  diffEntry,
  describeSource,
  listDatasetFiles,
  loadSnapshot,
  type BuildSource,
  type Entry,
} from "./diff.js";
import { CATEGORIES, patchChanges, type Direction, type PatchChanges, type StatChange } from "./changes.js";
import { weaponFamily } from "./query/fuzzy.js";

export interface NotesOptions {
  /** Hide numeric changes smaller than this percent of the old value (default 1). */
  minChange?: number;
}

export type Scalar = number | string | boolean | null;

/** One stat or field that moved on a line's records. */
export interface NoteChange {
  /** Stat name from changes.ts, or the flattened field path (e.g. "resistances.physical.multiplier"). */
  stat: string;
  before?: Scalar;
  after?: Scalar;
  /** Percent of the old value; absent when the old value was 0 or not a number. */
  percent?: number;
  direction: Direction;
}

/** One bullet: a record, or a group of variants that read as one thing. */
export interface NoteLine {
  /** Display label, e.g. "Verdict LMG" or "RSI Polaris". */
  label: string;
  status: "added" | "removed" | "changed";
  /** Sub-heading inside the section: changes.ts category, dataset collection, or a loot/crafting kind. */
  category?: string;
  /** Short record / class names this line covers (prefixes such as EntityClassDefinition. stripped). */
  records: string[];
  /** Names of the folded variants, e.g. ["Ferrite", "Relay"]. */
  variants?: string[];
  /** Buff/nerf/changed stats; only on "changed" lines. */
  changes?: NoteChange[];
  /** Free-text detail, e.g. "rewarded by 3 missions: ..." or "pieces: arms, core". */
  detail?: string;
}

export type SectionId =
  | "newShips"
  | "shipBalance"
  | "newShipItems"
  | "newPersonalWeapons"
  | "personalBalance"
  | "armour"
  | "crafting"
  | "missions"
  | "loot"
  | "renames"
  | "other";

export interface NoteSection {
  id: SectionId;
  title: string;
  lines: NoteLine[];
  /** One-line remarks rendered as italics under the section, e.g. "+12 AI/NPC variants not listed". */
  notes: string[];
}

/**
 * The machine form of a set of patch notes. `renderNotes` prints it as
 * Markdown; the JSON form is the same object, so a publish step can write it
 * next to the datasets and SCMCP can serve it unchanged.
 *
 *   {
 *     from: { source, build?, fallback? },  to: { source, build?, fallback? },
 *                                         // fallback: { source, files } read from a snapshot's published twin
 *     minChange: 1,                       // percent threshold used
 *     sections: [{ id, title, notes: string[], lines: [{
 *       label, status: "added"|"removed"|"changed", category?, records: string[],
 *       variants?: string[], detail?, changes?: [{ stat, before?, after?, percent?, direction }]
 *     }] }],                              // empty sections are omitted, order is fixed
 *     hidden: { minor, newlyTracked, untracked },
 *                                         // dropped by the threshold / one-sided fields /
 *                                         // records changed only outside the tracked stats
 *   }
 */
export interface NotesSide {
  source: string;
  build?: string;
  /** Datasets this side lacked and read from elsewhere (a snapshot's published tag). */
  fallback?: { source: string; files: string[] };
}

export interface NotesDoc {
  from: NotesSide;
  to: NotesSide;
  minChange: number;
  sections: NoteSection[];
  hidden: { minor: number; newlyTracked: number; untracked: number };
}

type Body = Record<string, unknown>;
type Rec = Record<string, unknown>;

/** Every dataset of a build that notes read, by file name. */
export type BuildData = Map<string, Body>;

const FILES = {
  weapons: "game-weapons.json",
  fps: "game-fps-weapons.json",
  ships: "game-ships.json",
  components: "game-ship-components.json",
  armor: "game-armor.json",
  blueprints: "game-blueprints.json",
  missions: "game-missions.json",
  wikelo: "game-wikelo-trades.json",
  tags: "game-item-tags.json",
  sources: "game-item-sources.json",
  strings: "game-strings-english.json",
};

/** Collections whose changes are itemised in a section; "Other" counts the rest. */
const ITEMISED: Record<string, string[] | "all"> = {
  [FILES.weapons]: "all",
  [FILES.fps]: "all",
  [FILES.ships]: "all",
  [FILES.armor]: "all",
  [FILES.blueprints]: ["blueprints", "rewardPools"],
  [FILES.missions]: "all",
  [FILES.wikelo]: ["trades"],
  [FILES.tags]: "all",
  [FILES.sources]: "all",
  [FILES.components]: [
    "weapons", "missiles", "shields", "powerPlants", "coolers", "quantumDrives", "radars", "armor", "thrusters",
  ],
};

/**
 * Collections whose additions and removals are itemised (listed or folded) by
 * a section; "Other" counts only their changed records. newShipItems covers
 * every ship-components collection.
 */
const ADDS_ITEMISED: Record<string, "all"> = { [FILES.components]: "all" };

/** NPC-only hulls and weapons with no player variant: the Vanduul. */
const NPC_ONLY = /^VNCL_/i;
const npcOnly = (record: string) => NPC_ONLY.test(shortRecord(record));

// ---------------------------------------------------------------------------
// Naming and grouping
// ---------------------------------------------------------------------------

/** "EntityClassDefinition.ksar_lmg_energy_01" -> "ksar_lmg_energy_01". */
export function shortRecord(record: string): string {
  return record.replace(/^[A-Za-z]+\./, "");
}

const VARIANT_SUFFIX =
  /_(tint|store|camo|lr|mr|sf|lumi|collector|iae|arctic|urban|desert|tundra|black|white|red|blue|green|tan|grey|gray|gold|purple|orange|yellow|pink|digital|shin|imp|xenothreat|showdown|luminalia|pirate|prop|civilian)\d*$/i;

/**
 * Class name with variant suffixes stripped, lower-cased:
 * "ksar_lmg_energy_01_store01_prop" -> "ksar_lmg_energy_01".
 */
export function baseClass(record: string): string {
  let name = shortRecord(record).toLowerCase();
  for (let prev = ""; prev !== name; ) {
    prev = name;
    name = name.replace(VARIANT_SUFFIX, "");
  }
  return name;
}

const SLOT_TOKEN = /_(arms|core|helmet|legs|backpack)(?=_|$)/i;
const SLOT_WORD = /\s+(Arms|Core|Helmet|Legs|Backpack)\b/;
const stripSlot = (name: string) => name.replace(SLOT_WORD, "").replace(/\s+/g, " ").trim();

/** Armour pieces of one set share a key: kap_combat_light_arms_02_03_01 -> kap_combat_light_02_03_01. */
export function armourSetKey(record: string): string {
  return shortRecord(record).toLowerCase().replace(SLOT_TOKEN, "");
}

function slotOf(record: string): string | undefined {
  return SLOT_TOKEN.exec(shortRecord(record))?.[1]?.toLowerCase();
}

/** A display name worth printing: not empty, not the class name, not a localisation key. */
export function isRealName(name: unknown, record?: string): name is string {
  if (typeof name !== "string" || !name.trim()) return false;
  if (/PLACEHOLDER/i.test(name) || /^[@~]/.test(name)) return false;
  if (record && name.toLowerCase() === shortRecord(record).toLowerCase()) return false;
  return true;
}

/** `Verdict "Derivation" LMG` -> { base: "Verdict LMG", variant: "Derivation" }. */
export function splitVariant(name: string): { base: string; variant?: string } {
  const m = /\s*["“]([^"”]+)["”]\s*/.exec(name);
  if (!m) return { base: name };
  return { base: `${name.slice(0, m.index)} ${name.slice(m.index + m[0].length)}`.replace(/\s+/g, " ").trim(), variant: m[1] };
}

const mostCommon = (values: string[]): string => {
  const counts = new Map<string, number>();
  for (const v of values) counts.set(v, (counts.get(v) ?? 0) + 1);
  return [...counts.entries()].sort((a, b) => b[1] - a[1] || a[0].length - b[0].length || a[0].localeCompare(b[0]))[0][0];
};

interface Member {
  record: string;
  name?: string;
  template: boolean;
  family?: string;
}

const isTemplate = (r: Rec): boolean =>
  r.isTemplate === true || /_prop(_|$)/i.test(String(r.recordName ?? r.className ?? ""));

function memberOf(r: Rec, family?: string): Member {
  const record = String(r.recordName ?? r.className ?? "");
  return { record, name: isRealName(r.name, record) ? r.name : undefined, template: isTemplate(r), family };
}

/**
 * Collapse added or removed records into lines. Templates and props are only
 * counted. Members sharing `keyOf` become one line labelled by their most
 * common base name, with the rest listed as variants.
 */
function groupMembers(
  members: Member[],
  status: NoteLine["status"],
  keyOf: (m: Member) => string,
  options: { category?: string; armour?: boolean } = {},
): { lines: NoteLine[]; folded: number } {
  let folded = 0;
  const groups = new Map<string, Member[]>();
  for (const m of members) {
    if (m.template) {
      folded += 1;
      continue;
    }
    const key = keyOf(m);
    groups.set(key, [...(groups.get(key) ?? []), m]);
  }
  const lines: NoteLine[] = [];
  for (const group of groups.values()) {
    const named = group.filter((m) => m.name);
    const baseOf = (name: string) => {
      const base = splitVariant(name).base;
      return options.armour ? stripSlot(base) : base;
    };
    let label: string;
    let variants: string[] = [];
    if (named.length === 0) {
      const first = group[0];
      label = first.family ? `${first.family} (${shortRecord(first.record)})` : shortRecord(first.record);
    } else {
      label = mostCommon(named.map((m) => baseOf(m.name as string)));
      if (named.length === 1 && !options.armour) label = named[0].name as string;
      else if (!options.armour) {
        variants = [...new Set(named.filter((m) => m.name !== label).map((m) => splitVariant(m.name as string).variant ?? (m.name as string)))];
      }
    }
    const line: NoteLine = { label, status, category: options.category, records: group.map((m) => shortRecord(m.record)).sort() };
    if (variants.length > 0) line.variants = variants.sort();
    if (options.armour) {
      const slots = [...new Set(group.map((m) => slotOf(m.record)).filter((s): s is string => !!s))];
      if (group.length > 1) line.detail = slots.length > 1 ? `pieces: ${slots.sort().join(", ")}` : `${group.length} pieces`;
    }
    lines.push(line);
  }
  return { lines: lines.sort((a, b) => a.label.localeCompare(b.label)), folded };
}

// ---------------------------------------------------------------------------
// Field-level changes
// ---------------------------------------------------------------------------

/** Fields that never mean anything to a player: ids, build stamps, ordering, string-table keys. */
const BOOKKEEPING = /(^|\.)(_build|_extracted|_source|id|guid|uuid|recordName|className|reference|ref|order|index|sortOrder|sort|name|displayName|nameKey|descriptionKey|titleKey|giverKey|nameStatus)$/i;

/** Flavour text fields, never counted as a change on their own. */
const PROSE = /(^|\.)(description|desc|flavorText|lore)$/i;

interface Counters {
  minor: number;
  newlyTracked: number;
  untracked: number;
}

const round = (value: number, places = 2) => Math.round(value * 10 ** places) / 10 ** places;

/** Whether a numeric move clears the threshold; counts it as hidden otherwise. */
function significant(before: number, after: number, minChange: number, counters: Counters): number | undefined | false {
  if (before === 0) return undefined;
  const pct = round(((after - before) / Math.abs(before)) * 100);
  if (Math.abs(pct) < minChange) {
    counters.minor += 1;
    return false;
  }
  return pct;
}

/**
 * Player-visible field changes between two versions of a record: numbers past
 * the threshold, and short text or flag changes. A field present on only one
 * side is a parser change, not a game change, and is counted as newly tracked.
 */
function fieldChanges(
  before: unknown,
  after: unknown,
  minChange: number,
  counters: Counters,
  only?: RegExp,
): NoteChange[] {
  const out: NoteChange[] = [];
  for (const change of diffEntry(before, after)) {
    if (BOOKKEEPING.test(change.field) || (only && !only.test(change.field))) continue;
    if (change.from === undefined || change.to === undefined) {
      counters.newlyTracked += 1;
      continue;
    }
    if (typeof change.from === "number" && typeof change.to === "number") {
      const pct = significant(change.from, change.to, minChange, counters);
      if (pct === false) continue;
      out.push({ stat: change.field, before: change.from, after: change.to, percent: pct, direction: "changed" });
      continue;
    }
    const clip = (v: unknown): Scalar => (typeof v === "string" && v.length > 60 ? `${v.slice(0, 59)}…` : (v as Scalar));
    out.push({ stat: change.field, before: clip(change.from), after: clip(change.to), direction: "changed" });
  }
  return out;
}

/** Stat rows from changes.ts that survive the threshold; the rest are counted. */
function keepStat(row: StatChange, minChange: number, counters: Counters): NoteChange | undefined {
  if (row.before === undefined || row.after === undefined) {
    counters.newlyTracked += 1;
    return undefined;
  }
  if (row.percent !== undefined && Math.abs(row.percent) < minChange) {
    counters.minor += 1;
    return undefined;
  }
  // From 0 is a value the old build never filled (a radar's mass 0 -> 590), not a balance call.
  if (row.before === 0) return { stat: row.stat, before: row.before, after: row.after, direction: "changed" };
  return { stat: row.stat, before: row.before, after: row.after, percent: row.percent, direction: row.direction };
}

const signature = (changes: NoteChange[]) =>
  changes.map((c) => `${c.stat}=${JSON.stringify(c.before)}>${JSON.stringify(c.after)}`).sort().join("|");

/**
 * Turn per-record changes into lines. Records sharing `keyOf` merge when they
 * agree on every stat they share (the usual case for paint variants and
 * display copies); disagreeing records get a line per distinct change set.
 */
function groupChanged(
  records: { record: string; label: string; key: string; category?: string; changes: NoteChange[] }[],
): NoteLine[] {
  const groups = new Map<string, typeof records>();
  for (const r of records) {
    if (r.changes.length === 0) continue;
    const key = `${r.category ?? ""}|${r.key}`;
    groups.set(key, [...(groups.get(key) ?? []), r]);
  }
  const lines: NoteLine[] = [];
  for (const group of groups.values()) {
    const byStat = new Map<string, Set<string>>();
    for (const r of group) {
      for (const c of r.changes) {
        byStat.set(c.stat, (byStat.get(c.stat) ?? new Set()).add(JSON.stringify([c.before, c.after])));
      }
    }
    const consistent = [...byStat.values()].every((s) => s.size === 1);
    const buckets = new Map<string, typeof records>();
    for (const r of group) {
      const key = consistent ? "all" : signature(r.changes);
      buckets.set(key, [...(buckets.get(key) ?? []), r]);
    }
    for (const bucket of buckets.values()) {
      const merged = new Map<string, NoteChange>();
      for (const r of bucket) for (const c of r.changes) merged.set(c.stat, c);
      const changes = [...merged.values()].sort(
        (a, b) => Math.abs(b.percent ?? 0) - Math.abs(a.percent ?? 0) || a.stat.localeCompare(b.stat),
      );
      lines.push({
        label: mostCommon(bucket.map((r) => r.label)),
        status: "changed",
        category: bucket[0].category,
        records: bucket.map((r) => shortRecord(r.record)).sort(),
        changes,
      });
    }
  }
  return lines.sort((a, b) => (a.category ?? "").localeCompare(b.category ?? "") || a.label.localeCompare(b.label));
}

// ---------------------------------------------------------------------------
// Dataset access
// ---------------------------------------------------------------------------

const list = (data: BuildData, file: string, collection: string): Rec[] => {
  const value = data.get(file)?.[collection];
  return Array.isArray(value) ? (value as Rec[]) : [];
};

const byKey = (records: Rec[]): Map<string, Rec> => {
  const map = new Map<string, Rec>();
  for (const r of records) {
    const key = r.recordName ?? r.className;
    if (typeof key === "string") map.set(key, r);
  }
  return map;
};

/** Records only in `after`, records only in `before`, and pairs present in both. */
function split(before: Rec[], after: Rec[]) {
  const a = byKey(before);
  const b = byKey(after);
  return {
    added: [...b.entries()].filter(([k]) => !a.has(k)).map(([, v]) => v),
    removed: [...a.entries()].filter(([k]) => !b.has(k)).map(([, v]) => v),
    both: [...b.entries()].filter(([k]) => a.has(k)).map(([k, v]) => [a.get(k) as Rec, v] as const),
  };
}

const humanise = (collection: string) => collection.replace(/([a-z])([A-Z])/g, "$1 $2").toLowerCase();

const CATEGORY_LABELS: Record<string, string> = {
  ships: "Ships",
  shipWeapons: "Ship weapons",
  missiles: "Missiles",
  fpsWeapons: "Personal weapons",
  shields: "Shields",
  powerPlants: "Power plants",
  coolers: "Coolers",
  quantumDrives: "Quantum drives",
  radars: "Radars",
  armor: "Ship armour",
  thrusters: "Thrusters",
};

export function categoryLabel(category: string): string {
  return CATEGORY_LABELS[category] ?? humanise(category).replace(/^./, (c) => c.toUpperCase());
}

/** Lower-cased class name -> display name, across every item-bearing dataset of the builds. */
function nameIndex(...builds: BuildData[]): Map<string, string> {
  const index = new Map<string, string>();
  const add = (r: Rec) => {
    const record = String(r.recordName ?? r.className ?? "");
    if (record && isRealName(r.name, record) && !index.has(shortRecord(record).toLowerCase())) {
      index.set(shortRecord(record).toLowerCase(), r.name);
    }
  };
  for (const data of builds) {
    for (const r of list(data, FILES.tags, "items")) add(r);
    for (const c of ["shipWeapons", "fpsWeapons", "missiles"]) list(data, FILES.weapons, c).forEach(add);
    for (const c of ["armor", "clothing"]) list(data, FILES.armor, c).forEach(add);
    const components = data.get(FILES.components) ?? {};
    for (const c of diffableCollections(components)) list(data, FILES.components, c).forEach(add);
  }
  return index;
}

/** Lower-cased class name -> weapon family from its item tags. */
function familyIndex(...builds: BuildData[]): Map<string, string> {
  const index = new Map<string, string>();
  for (const data of builds) {
    for (const item of list(data, FILES.tags, "items")) {
      const family = Array.isArray(item.tags) ? weaponFamily(item.tags as string[]) : undefined;
      if (family && typeof item.className === "string" && !index.has(item.className.toLowerCase())) {
        index.set(item.className.toLowerCase(), family);
      }
    }
  }
  return index;
}

// ---------------------------------------------------------------------------
// Sections
// ---------------------------------------------------------------------------

interface Context {
  before: BuildData;
  after: BuildData;
  changes: PatchChanges;
  minChange: number;
  counters: Counters;
  names: Map<string, string>;
  families: Map<string, string>;
  renames: { from: string; to: string; record: string }[];
  /** siblingFamily cache, by class prefix. */
  siblings: Map<string, string | undefined>;
}

const section = (id: SectionId, title: string, lines: NoteLine[] = [], notes: string[] = []): NoteSection => ({
  id,
  title,
  lines,
  notes,
});

const foldedNote = (n: number, what: string) => (n > 0 ? [`+${n} ${what} not listed`] : []);

/**
 * Whether both builds carry `file`. When only the newer one does, everything
 * in it would read as new, so the section says so instead of listing it.
 */
function compared(ctx: Context, file: string, notes: string[]): boolean {
  if (ctx.before.has(file) && ctx.after.has(file)) return true;
  if (ctx.after.has(file)) notes.push(`first build with ${file.replace(/^game-|\.json$/g, "")} data; nothing to compare against`);
  return false;
}

const commonWordPrefix = (names: string[]): string => {
  const words = names.map((n) => n.split(/\s+/));
  const out: string[] = [];
  for (let i = 0; words.every((w) => i < w.length && w[i] === words[0][i]); i++) out.push(words[0][i]);
  return out.join(" ");
};

/**
 * A product-line name for an unnamed colour variant, from its named siblings:
 * omc_utility_heavy_arms_01_03_01 -> "Corbel Arms" (siblings "Corbel Arms Smolder", "Corbel Arms Halcyon").
 * Armour callers strip the slot word themselves.
 */
function siblingFamily(ctx: Context, record: string): string | undefined {
  const cls = shortRecord(record).toLowerCase();
  const prefix = cls.replace(/_\d+_\d+$/, "");
  if (prefix === cls) return undefined;
  if (!ctx.siblings.has(prefix)) {
    const names: string[] = [];
    for (const [key, name] of ctx.names) {
      if (key.startsWith(prefix) && /^_\d+_\d+$/.test(key.slice(prefix.length))) names.push(name);
    }
    const family = names.length > 0 ? commonWordPrefix(names) || splitVariant(names[0]).base : undefined;
    ctx.siblings.set(prefix, family);
  }
  return ctx.siblings.get(prefix);
}

function noteRenames(ctx: Context, pairs: (readonly [Rec, Rec])[]) {
  for (const [a, b] of pairs) {
    const record = String(b.recordName ?? b.className ?? "");
    if (isTemplate(b) || !isRealName(a.name, record) || !isRealName(b.name, record) || a.name === b.name) continue;
    ctx.renames.push({ from: a.name, to: b.name, record });
  }
}

function newShips(ctx: Context): NoteSection {
  const notes: string[] = [];
  if (!compared(ctx, FILES.ships, notes)) return section("newShips", "New ships", [], notes);
  const items = ctx.changes.items.filter((i) => i.category === "ships" && !npcOnly(i.record));
  const lines: NoteLine[] = [];
  for (const status of ["added", "removed"] as const) {
    const byName = new Map<string, string[]>();
    for (const i of items.filter((x) => x.change === status)) byName.set(i.name, [...(byName.get(i.name) ?? []), i.record]);
    for (const [label, records] of [...byName.entries()].sort()) lines.push({ label, status, records: records.sort() });
  }
  const { added } = split(list(ctx.before, FILES.ships, "vehicles"), list(ctx.after, FILES.ships, "vehicles"));
  const player = items.filter((i) => i.change === "added").length;
  return section("newShips", "New ships", lines, foldedNote(added.length - player, "AI/NPC/template variants"));
}

/** Stat-row lines from changes.ts for the given categories; NPC-only records are counted in `npc`. */
function balanceLines(
  ctx: Context,
  categories: string[],
  keyOf: (row: StatChange) => string,
  npc = new Set<string>(),
): NoteLine[] {
  const perRecord = new Map<string, { record: string; label: string; key: string; category: string; changes: NoteChange[] }>();
  for (const row of ctx.changes.stats) {
    if (!categories.includes(row.category)) continue;
    if (npcOnly(row.record)) {
      npc.add(`${row.category}|${row.record}`);
      continue;
    }
    const id = `${row.category}|${row.record}`;
    const entry = perRecord.get(id) ?? { record: row.record, label: row.name, key: keyOf(row), category: row.category, changes: [] };
    const kept = keepStat(row, ctx.minChange, ctx.counters);
    if (kept) entry.changes.push(kept);
    perRecord.set(id, entry);
  }
  const order = (c?: string) => categories.indexOf(c ?? "");
  return groupChanged([...perRecord.values()]).sort((a, b) => order(a.category) - order(b.category) || a.label.localeCompare(b.label));
}

const SHIP_BALANCE = ["ships", "shipWeapons", "missiles", "shields", "powerPlants", "coolers", "quantumDrives", "radars", "armor", "thrusters"];

/**
 * Where each balance category's records live, for finding records that changed
 * without a stat row: the changes.ts source, plus ship-components' own copies
 * of ship weapons and missiles (which carry fields game-weapons does not).
 */
const TRACKED_SOURCES: { category: string; file: string; collection: string; include?: (r: Rec) => boolean }[] = [
  ...CATEGORIES.map((c) => ({ category: c.category, file: datasetFile(c.dataset), collection: c.collection, include: c.include })),
  { category: "shipWeapons", file: FILES.components, collection: "weapons", include: (r) => !isTemplate(r) },
  { category: "missiles", file: FILES.components, collection: "missiles", include: (r) => !isTemplate(r) },
];

/**
 * Records of the given categories that changed in some field both builds
 * carry, yet have no stat row from changes.ts (so no line, and not counted as
 * minor either): a ship whose loadout gained a turret, a missile whose volume
 * moved. Counted per category, NPC-only records left out; they add to
 * hidden.untracked.
 */
function untrackedNote(ctx: Context, categories: string[]): string[] {
  const covered = new Set<string>();
  for (const row of ctx.changes.stats) covered.add(`${row.category}|${shortRecord(row.record)}`);
  const found = new Map<string, Set<string>>();
  for (const src of TRACKED_SOURCES) {
    if (!categories.includes(src.category)) continue;
    for (const [a, b] of split(list(ctx.before, src.file, src.collection), list(ctx.after, src.file, src.collection)).both) {
      const record = shortRecord(String(b.recordName ?? b.className ?? ""));
      if (!record || npcOnly(record) || (src.include && !src.include(b)) || covered.has(`${src.category}|${record}`)) continue;
      // Only fields both sides carry (a field one side lacks is a parser change, counted as newly
      // tracked), and not flavour text: a reworded description is no game change.
      const moved = diffEntry(a, b).some(
        (c) => !BOOKKEEPING.test(c.field) && !PROSE.test(c.field) && c.from !== undefined && c.to !== undefined,
      );
      if (moved) found.set(src.category, (found.get(src.category) ?? new Set()).add(record));
    }
  }
  const parts = categories.filter((c) => found.has(c)).map((c) => `${categoryLabel(c).toLowerCase()} ×${found.get(c)?.size}`);
  const total = [...found.values()].reduce((n, set) => n + set.size, 0);
  ctx.counters.untracked += total;
  return total > 0 ? [`changed only outside the tracked stats (loadouts, volumes, …), not listed: ${parts.join(", ")}; see \`diff\``] : [];
}

function shipBalance(ctx: Context): NoteSection {
  const npc = new Set<string>();
  const lines = balanceLines(ctx, SHIP_BALANCE, (r) => r.name, npc);
  const notes = [...untrackedNote(ctx, SHIP_BALANCE), ...foldedNote(npc.size, "Vanduul (NPC-only) hulls and weapons with stat changes")];
  return section("shipBalance", "Ship balance", lines, notes);
}

/**
 * New skins of guns that already existed fold into one line per skin.
 *
 * A patch that adds a camo to a weapon line adds one class per size
 * (BEHR_LaserCannon_S1_mr02, _S2_mr02, _S3_mr02), and each would otherwise read
 * as a new weapon. When an added line's base class existed in the older build,
 * its skin name is the quoted part of the display name ('Coda "Piconalia"
 * Pistol' -> Piconalia) or the words after the base gun's name ("M3A Cannon
 * Raider Camo" -> Raider Camo). Lines sharing a skin become one line that names
 * each gun with its size (M3A, M4A and M5A are different guns) and says whether
 * the skin keeps the base gun's stats ("paint only") or not, which matters more.
 * Lines whose base is new too (a new gun with launch skins) are left as they are.
 */
export function foldNewSkins(lines: NoteLine[], before: readonly Rec[], after: readonly Rec[] = []): NoteLine[] {
  const base = new Map<string, Rec>();
  for (const r of before) {
    const key = baseClass(String(r.recordName));
    // The base record (no suffix) names the gun; skins of it must not.
    if (isRealName(r.name, String(r.recordName)) && shortRecord(String(r.recordName)).toLowerCase() === key) base.set(key, r);
  }
  const added = new Map(after.map((r) => [shortRecord(String(r.recordName)).toLowerCase(), r]));
  type Skin = { skin: string; guns: Map<string, number | undefined>; records: string[]; category?: string; sameStats: boolean };
  const skins = new Map<string, Skin>();
  const kept: NoteLine[] = [];
  for (const line of lines) {
    const bases = [...new Set(line.records.map((r) => baseClass(r)))];
    const baseRec = bases.length === 1 ? base.get(bases[0]) : undefined;
    const gun = baseRec ? String(baseRec.name) : undefined;
    let skin: string | undefined;
    if (line.status === "added" && !line.variants && gun) {
      const quoted = splitVariant(line.label).variant;
      if (quoted) skin = quoted;
      else if (line.label.toLowerCase().startsWith(`${gun.toLowerCase()} `)) skin = line.label.slice(gun.length).trim();
    }
    if (!skin || !gun || !baseRec) {
      kept.push(line);
      continue;
    }
    const key = `${line.category ?? ""}|${skin.toLowerCase()}`;
    const entry: Skin = skins.get(key) ?? { skin, guns: new Map(), records: [], category: line.category, sameStats: true };
    entry.guns.set(splitVariant(gun).base, typeof baseRec.size === "number" ? baseRec.size : undefined);
    entry.records.push(...line.records);
    for (const r of line.records) {
      const variant = added.get(r.toLowerCase());
      if (!variant || !sameStats(baseRec, variant)) entry.sameStats = false;
    }
    skins.set(key, entry);
  }
  const folded: NoteLine[] = [...skins.values()].map((e) => {
    const guns = [...e.guns.entries()]
      .sort((a, b) => (a[1] ?? 0) - (b[1] ?? 0) || a[0].localeCompare(b[0], "en", { numeric: true }))
      // Ship weapon sizes tell M3A/M4A/M5A apart; personal weapon size classes mean little to players.
      .map(([name, size]) => (size !== undefined && e.guns.size > 1 && e.category ? `${name} (S${size})` : name));
    const stats = e.sameStats ? "paint only, same stats as the base gun" : "stats differ from the base gun, see `diff`";
    return { label: `${e.skin} skin`, status: "added" as const, category: e.category, records: [...e.records].sort(), detail: `for ${guns.join(", ")}; ${stats}` };
  });
  return [...kept, ...folded.sort((a, b) => a.label.localeCompare(b.label))];
}

/** Combat stats a paint job must not change, compared where both records carry them. */
const SKIN_STATS = ["size", "dps", "alpha", "fireRate", "range", "speed", "ammoCapacity", "damageType", "heatPerShot", "spread"];

function sameStats(a: Rec, b: Rec): boolean {
  const ammo = (r: Rec) => (r.ammo && typeof r.ammo === "object" ? (r.ammo as Rec).recordName : r.ammo);
  if (ammo(a) !== ammo(b)) return false;
  return SKIN_STATS.every((k) => a[k] === undefined || b[k] === undefined || JSON.stringify(a[k]) === JSON.stringify(b[k]));
}

function newShipItems(ctx: Context): NoteSection {
  const lines: NoteLine[] = [];
  let folded = 0;
  const seen = new Set<string>();
  const take = (collection: string, before: Rec[], after: Rec[], grouping: "class" | "name") => {
    const { added, removed, both } = split(before.filter((r) => !seen.has(String(r.recordName))), after.filter((r) => !seen.has(String(r.recordName))));
    for (const r of [...before, ...after]) seen.add(String(r.recordName));
    noteRenames(ctx, both);
    for (const [status, records] of [["added", added], ["removed", removed]] as const) {
      const key = grouping === "class" ? (m: Member) => baseClass(m.record) : (m: Member) => m.name ?? baseClass(m.record);
      // NPC-only (Vanduul) parts are folded with the templates.
      const members = records.map((r) => {
        const m = memberOf(r);
        return npcOnly(m.record) ? { ...m, template: true } : m;
      });
      const g = groupMembers(members, status, key, { category: collection });
      const grouped = g.lines.map((l) => (l.records.length > 1 && grouping === "name" ? { ...l, variants: undefined } : l));
      lines.push(...(grouping === "class" ? foldNewSkins(grouped, before, after) : grouped));
      folded += g.folded;
    }
  };
  const notes: string[] = [];
  if (compared(ctx, FILES.weapons, notes)) {
    for (const c of ["shipWeapons", "missiles"]) take(c, list(ctx.before, FILES.weapons, c), list(ctx.after, FILES.weapons, c), "class");
  }
  if (compared(ctx, FILES.components, notes)) {
    for (const c of diffableCollections(ctx.after.get(FILES.components) ?? {})) {
      if (c !== "weapons" && c !== "missiles") take(c, list(ctx.before, FILES.components, c), list(ctx.after, FILES.components, c), "name");
    }
  }
  notes.push(...foldedNote(folded, "unnamed templates, NPC-only and ship-specific parts"));
  return section("newShipItems", "New ship weapons & components", lines, notes);
}

/** Personal weapons, once each: game-weapons.fpsWeapons first, melee from game-fps-weapons. */
function personalWeapons(data: BuildData): Rec[] {
  const primary = list(data, FILES.weapons, "fpsWeapons");
  const have = new Set(primary.map((r) => r.recordName));
  return [...primary, ...list(data, FILES.fps, "weapons").filter((r) => !have.has(r.recordName))];
}

function familyOf(ctx: Context, record: string): string | undefined {
  return ctx.families.get(shortRecord(record).toLowerCase()) ?? ctx.families.get(baseClass(record));
}

function newPersonalWeapons(ctx: Context): NoteSection {
  const notes: string[] = [];
  if (!compared(ctx, FILES.weapons, notes)) return section("newPersonalWeapons", "New personal weapons", [], notes);
  const { added, removed, both } = split(personalWeapons(ctx.before), personalWeapons(ctx.after));
  noteRenames(ctx, both);
  const lines: NoteLine[] = [];
  let folded = 0;
  for (const [status, records] of [["added", added], ["removed", removed]] as const) {
    const members = records.map((r) => memberOf(r, familyOf(ctx, String(r.recordName))));
    const g = groupMembers(members, status, (m) => (m.family ? `family:${m.family}` : baseClass(m.record)));
    lines.push(...foldNewSkins(g.lines, personalWeapons(ctx.before), personalWeapons(ctx.after)));
    folded += g.folded;
  }
  return section("newPersonalWeapons", "New personal weapons", lines, [...notes, ...foldedNote(folded, "unnamed templates and props")]);
}

function personalBalance(ctx: Context): NoteSection {
  const lines = balanceLines(ctx, ["fpsWeapons"], (r) => familyOf(ctx, r.record) ?? splitVariant(r.name).base);
  for (const line of lines) {
    if (line.records.length > 1) line.label = splitVariant(line.label).base;
  }
  return section("personalBalance", "Personal weapon balance", lines, untrackedNote(ctx, ["fpsWeapons"]));
}

/**
 * Added or removed armour and clothing as lines. Pieces of one colour set
 * (kap_combat_light_{arms,core,helmet,legs}_02_03_01) are one set; sets of one
 * product line (the class without its two trailing colour numbers) are one
 * line, with the colour sets as its variants.
 */
function armourLines(ctx: Context, records: Rec[], status: NoteLine["status"], category: string): { lines: NoteLine[]; folded: number } {
  let folded = 0;
  const productLines = new Map<string, Map<string, Rec[]>>();
  for (const r of records) {
    if (isTemplate(r)) {
      folded += 1;
      continue;
    }
    const set = armourSetKey(String(r.recordName));
    const product = set.replace(/_\d+_\d+$/, "");
    const sets = productLines.get(product) ?? new Map<string, Rec[]>();
    sets.set(set, [...(sets.get(set) ?? []), r]);
    productLines.set(product, sets);
  }
  const lines: NoteLine[] = [];
  for (const sets of productLines.values()) {
    const named = [...sets.entries()].map(([set, pieces]) => {
      const names = pieces.filter((p) => isRealName(p.name, String(p.recordName))).map((p) => stripSlot(p.name as string));
      const sibling = names.length > 0 ? undefined : siblingFamily(ctx, String(pieces[0].recordName));
      const family = sibling && stripSlot(sibling);
      const label = names.length > 0 ? mostCommon(names) : `${family ? `${family} ` : ""}(${set})`;
      return { label, pieces };
    });
    const prefix = named.length > 1 ? commonWordPrefix(named.map((n) => n.label)) : "";
    const groups = prefix ? [{ label: prefix, sets: named }] : named.map((n) => ({ label: n.label, sets: [n] }));
    for (const g of groups) {
      const pieces = g.sets.flatMap((n) => n.pieces);
      const slots = [...new Set(pieces.map((p) => slotOf(String(p.recordName))).filter((x): x is string => !!x))].sort();
      const line: NoteLine = { label: g.label, status, category, records: pieces.map((p) => shortRecord(String(p.recordName))).sort() };
      if (g.sets.length > 1) line.variants = g.sets.map((n) => n.label.slice(prefix.length).trim() || n.label).sort();
      if (pieces.length > 1) line.detail = slots.length > 1 ? `pieces: ${slots.join(", ")}` : `${pieces.length} pieces`;
      lines.push(line);
    }
  }
  return { lines: lines.sort((a, b) => a.label.localeCompare(b.label)), folded };
}

function armour(ctx: Context): NoteSection {
  const lines: NoteLine[] = [];
  const notes: string[] = [];
  let folded = 0;
  if (!compared(ctx, FILES.armor, notes)) return section("armour", "Armour & clothing", lines, notes);
  for (const collection of ["armor", "clothing"]) {
    const { added, removed, both } = split(list(ctx.before, FILES.armor, collection), list(ctx.after, FILES.armor, collection));
    noteRenames(ctx, both);
    const category = collection === "armor" ? "Armour" : "Clothing";
    for (const [status, records] of [["added", added], ["removed", removed]] as const) {
      const g = armourLines(ctx, records, status, category);
      lines.push(...g.lines);
      folded += g.folded;
    }
    const changed = both
      .filter(([, b]) => !isTemplate(b))
      .map(([a, b]) => {
        const record = String(b.recordName);
        const label = isRealName(b.name, record) ? stripSlot(b.name) : shortRecord(record);
        return { record, label, key: armourSetKey(record), category, changes: fieldChanges(a, b, ctx.minChange, ctx.counters) };
      });
    lines.push(...groupChanged(changed));
  }
  return section("armour", "Armour & clothing", lines, foldedNote(folded, "unnamed templates"));
}

const cleanTitle = (title: unknown): string | undefined =>
  typeof title === "string" && title.trim() && !/^[~@]/.test(title) ? title.replace(/\\n/g, " ").replace(/\s+/g, " ").trim() : undefined;

const listSome = (values: string[], max = 3) =>
  values.length <= max ? values.join(", ") : `${values.slice(0, max).join(", ")} +${values.length - max} more`;

/** Mission rewards of a blueprint, by contract (stable), labelled by mission title. */
function rewardMissions(bp: Rec): Map<string, string> {
  const out = new Map<string, string>();
  for (const r of Array.isArray(bp.rewardedBy) ? (bp.rewardedBy as Rec[]) : []) {
    const contract = String(r.contract ?? r.mission ?? "");
    if (contract) out.set(contract, cleanTitle(r.mission) ?? contract);
  }
  return out;
}

function blueprintLabel(ctx: Context, bp: Rec): string {
  const record = String(bp.recordName ?? "");
  const produces = typeof bp.produces === "string" ? bp.produces : shortRecord(record).replace(/^BP_CRAFT_/i, "");
  // An unnamed blueprint falls back to its product's class name, which is no name at all.
  for (const candidate of [bp.displayName, bp.name]) if (isRealName(candidate, record) && isRealName(candidate, produces)) return candidate;
  const named = ctx.names.get(produces.toLowerCase());
  if (named) return named;
  const family = siblingFamily(ctx, produces);
  return family ? `${family} (${produces})` : `${produces} (unnamed)`;
}

function crafting(ctx: Context): NoteSection {
  const lines: NoteLine[] = [];
  const notes: string[] = [];
  if (!compared(ctx, FILES.blueprints, notes)) return section("crafting", "Crafting", lines, notes);
  // A build whose blueprints carry no rewardedBy at all predates reward tracking; every reward would read as new.
  const rewardsTracked = list(ctx.before, FILES.blueprints, "blueprints").some((bp) => Array.isArray(bp.rewardedBy));
  if (!rewardsTracked) notes.push("first build with blueprint mission rewards; reward changes not compared");
  const { added, removed, both } = split(list(ctx.before, FILES.blueprints, "blueprints"), list(ctx.after, FILES.blueprints, "blueprints"));
  const productKey = (bp: Rec) => armourSetKey(typeof bp.produces === "string" ? bp.produces : String(bp.recordName));

  for (const [status, records] of [["added", added], ["removed", removed]] as const) {
    const groups = new Map<string, Rec[]>();
    for (const bp of records) groups.set(productKey(bp), [...(groups.get(productKey(bp)) ?? []), bp]);
    for (const group of groups.values()) {
      const labels = group.map((bp) => blueprintLabel(ctx, bp));
      const armourSet = group.length > 1 && group.every((bp) => slotOf(String(bp.produces ?? "")));
      const label = armourSet ? mostCommon(labels.map((l) => l.replace(SLOT_WORD, "").replace(/_(arms|core|helmet|legs|backpack)_/i, "_"))) : labels[0];
      const missions = [...new Set(group.flatMap((bp) => [...rewardMissions(bp).values()]))].sort();
      const line: NoteLine = { label, status, category: "Blueprints", records: group.map((bp) => shortRecord(String(bp.recordName))).sort() };
      if (group.length > 1) line.variants = armourSet ? undefined : labels.slice(1);
      const parts: string[] = [];
      if (armourSet) parts.push(`pieces: ${group.map((bp) => slotOf(String(bp.produces))).sort().join(", ")}`);
      if (status === "added") parts.push(missions.length > 0 ? `rewarded by ${missions.length} mission${missions.length === 1 ? "" : "s"}: ${listSome(missions)}` : "no mission reward");
      if (parts.length > 0) line.detail = parts.join("; ");
      lines.push(line);
    }
  }

  // Changed blueprints: reward gains/losses compared as sets (array order shifts every patch), and recipe edits.
  const changed = new Map<string, { labels: string[]; records: string[]; detail: string; changes: NoteChange[] }>();
  for (const [a, b] of both) {
    const ra = rewardMissions(a);
    const rb = rewardsTracked ? rewardMissions(b) : ra;
    const gained = [...new Set([...rb].filter(([k]) => !ra.has(k)).map(([, v]) => v))].sort();
    const lost = [...new Set([...ra].filter(([k]) => !rb.has(k)).map(([, v]) => v))].sort();
    const recipe = fieldChanges(a.tiers, b.tiers, ctx.minChange, ctx.counters).map((c) => ({ ...c, stat: `tiers${c.stat.startsWith("[") ? "" : "."}${c.stat}` }));
    const parts: string[] = [];
    if (gained.length > 0) parts.push(`now also from ${listSome(gained)}`);
    if (lost.length > 0) parts.push(`no longer from ${listSome(lost)}`);
    const timeOnly = recipe.every((c) => /craftTimeSeconds$/.test(c.stat));
    if (recipe.length > 0 && !timeOnly) parts.push("recipe changed");
    if (parts.length === 0 && recipe.length === 0) continue;
    const detail = parts.join("; ");
    const changes = recipe.filter((c) => /craftTimeSeconds$/.test(c.stat));
    const key = `${detail}|${signature(changes)}`;
    const entry = changed.get(key) ?? { labels: [], records: [], detail, changes };
    entry.labels.push(blueprintLabel(ctx, b));
    entry.records.push(shortRecord(String(b.recordName)));
    changed.set(key, entry);
  }
  for (const entry of changed.values()) {
    const line: NoteLine = { label: listSome(entry.labels.sort(), 4), status: "changed", category: "Blueprints", records: entry.records.sort() };
    if (entry.detail) line.detail = entry.detail;
    if (entry.changes.length > 0) line.changes = entry.changes;
    lines.push(line);
  }

  // Reward pools carry only a name, so they are matched by it.
  const poolsA = new Set(list(ctx.before, FILES.blueprints, "rewardPools").map((p) => String(p.name)));
  const newPools = list(ctx.after, FILES.blueprints, "rewardPools").filter((p) => !poolsA.has(String(p.name))).length;
  if (Array.isArray(ctx.before.get(FILES.blueprints)?.rewardPools) && newPools > 0) notes.push(`${newPools} new mission reward pool${newPools === 1 ? "" : "s"}`);
  return section("crafting", "Crafting", lines, notes);
}

function missions(ctx: Context): NoteSection {
  const lines: NoteLine[] = [];
  const notes: string[] = [];
  if (ctx.before.has(FILES.missions) && ctx.after.has(FILES.missions)) {
    const { added, removed, both } = split(list(ctx.before, FILES.missions, "missions"), list(ctx.after, FILES.missions, "missions"));
    let unreleased = 0;
    for (const [status, records] of [["added", added], ["removed", removed]] as const) {
      const groups = new Map<string, Rec[]>();
      for (const m of records) {
        if (m.notForRelease === true) {
          unreleased += 1;
          continue;
        }
        const label = cleanTitle(m.title) ?? shortRecord(String(m.recordName));
        groups.set(label, [...(groups.get(label) ?? []), m]);
      }
      for (const [label, group] of [...groups.entries()].sort()) {
        const rewards = [...new Set(group.map((m) => m.reward).filter((r): r is number => typeof r === "number"))].sort((x, y) => x - y);
        const line: NoteLine = { label, status, category: "Missions", records: group.map((m) => shortRecord(String(m.recordName))).sort() };
        const parts: string[] = [];
        if (group.length > 1) parts.push(`${group.length} contracts`);
        if (rewards.length > 0) parts.push(`${fmtNumber(rewards[0])}${rewards.length > 1 ? `–${fmtNumber(rewards[rewards.length - 1])}` : ""} ${String(group[0].currency ?? "aUEC")}`);
        if (parts.length > 0) line.detail = parts.join(", ");
        lines.push(line);
      }
    }
    if (unreleased > 0) notes.push(`${unreleased} unreleased (not-for-release) mission record${unreleased === 1 ? "" : "s"} added or removed`);
    const changed = both.map(([a, b]) => ({
      record: String(b.recordName),
      label: cleanTitle(b.title) ?? shortRecord(String(b.recordName)),
      key: cleanTitle(b.title) ?? String(b.recordName),
      category: "Missions",
      changes: fieldChanges(a, b, ctx.minChange, ctx.counters, /^(reward|rewardMax|buyIn|lawful|difficulty|type|currency)$/),
    }));
    lines.push(...groupChanged(changed));
  }
  if (ctx.before.has(FILES.wikelo) && ctx.after.has(FILES.wikelo)) {
    const { added, removed, both } = split(list(ctx.before, FILES.wikelo, "trades"), list(ctx.after, FILES.wikelo, "trades"));
    const label = (t: Rec) => (typeof t.rewardInferred === "string" ? t.rewardInferred : shortRecord(String(t.recordName)));
    for (const [status, records] of [["added", added], ["removed", removed]] as const) {
      for (const t of records) lines.push({ label: label(t), status, category: "Wikelo", records: [shortRecord(String(t.recordName))] });
    }
    for (const [a, b] of both) {
      const handIns = (t: Rec) => JSON.stringify([...(Array.isArray(t.handIns) ? (t.handIns as Rec[]) : [])].map((h) => `${h.item}x${h.amount}`).sort());
      if (handIns(a) !== handIns(b)) {
        lines.push({ label: label(b), status: "changed", category: "Wikelo", records: [shortRecord(String(b.recordName))], detail: "hand-ins changed" });
      }
    }
  }
  return section("missions", "Missions & Wikelo", lines, notes);
}

type LootState = "lootable" | "not lootable" | "unset";

function lootState(tags: unknown): LootState {
  const list = Array.isArray(tags) ? (tags as string[]) : [];
  const has = (root: string) => list.some((t) => t === root || t.startsWith(`${root}/`));
  if (has("LootGeneration/CannotGenerateAsLoot")) return "not lootable";
  if (has("LootGeneration/CanGenerateAsLoot")) return "lootable";
  return "unset";
}

/** Items a loot area always holds (racks, displays, loose placements), item -> areas. */
function fixedPlacements(data: BuildData): Map<string, Set<string>> {
  const out = new Map<string, Set<string>>();
  const add = (item: string, area: string) => out.set(item.toLowerCase(), (out.get(item.toLowerCase()) ?? new Set()).add(area));
  for (const area of list(data, FILES.sources, "areas")) {
    for (const config of Array.isArray(area.configs) ? (area.configs as Rec[]) : []) {
      for (const slot of Array.isArray(config.slots) ? (config.slots as Rec[]) : []) {
        if (slot.loot) continue;
        if (Array.isArray(slot.contains) && slot.contains.length > 0) for (const item of slot.contains as string[]) add(item, String(area.name));
        else if (!slot.preset && typeof slot.entity === "string") add(slot.entity, String(area.name));
      }
    }
  }
  return out;
}

function loot(ctx: Context): NoteSection {
  const lines: NoteLine[] = [];
  const notes: string[] = [];
  const tagsBoth = ctx.before.has(FILES.tags) && ctx.after.has(FILES.tags);
  const sourcesBoth = ctx.before.has(FILES.sources) && ctx.after.has(FILES.sources);
  if ((ctx.after.has(FILES.tags) && !ctx.before.has(FILES.tags)) || (ctx.after.has(FILES.sources) && !ctx.before.has(FILES.sources))) {
    notes.push("first build with loot data; nothing to compare against");
  }
  const label = (cls: string) => ctx.names.get(cls.toLowerCase()) ?? cls;
  /** Several classes with one change: group by base class into lines. */
  const emit = (classes: string[], status: NoteLine["status"], category: string, detailOf?: (group: string[]) => string | undefined) => {
    const groups = new Map<string, string[]>();
    for (const cls of classes) {
      const key = familyOf(ctx, cls) ? `family:${familyOf(ctx, cls)}` : baseClass(cls);
      groups.set(key, [...(groups.get(key) ?? []), cls]);
    }
    for (const group of groups.values()) {
      const named = group.map(label).filter((n, i) => n !== group[i]);
      const base = named.length > 0 ? mostCommon(named.map((n) => splitVariant(n).base)) : group[0];
      const line: NoteLine = { label: named.length === 1 && group.length === 1 ? named[0] : base, status, category, records: [...group].sort() };
      if (group.length > 1) line.variants = [...new Set(named.map((n) => splitVariant(n).variant ?? n).filter((v) => v !== base))].sort();
      if (line.variants?.length === 0) delete line.variants;
      const detail = detailOf?.(group);
      if (detail) line.detail = detail;
      lines.push(line);
    }
  };

  if (tagsBoth) {
    const before = new Map(list(ctx.before, FILES.tags, "items").map((i) => [String(i.className), i]));
    const flips = { lootable: [] as string[], "not lootable": [] as string[], unset: [] as string[] };
    let foldedTemplates = 0;
    for (const item of list(ctx.after, FILES.tags, "items")) {
      const prior = before.get(String(item.className));
      if (!prior) continue;
      const was = lootState(prior.tags);
      const now = lootState(item.tags);
      if (was === now) continue;
      if (item.isTemplate === true || !isRealName(item.name, String(item.className))) {
        foldedTemplates += 1;
        continue;
      }
      flips[now].push(String(item.className));
    }
    emit(flips.lootable, "changed", "Now lootable");
    emit(flips["not lootable"], "changed", "No longer lootable");
    emit(flips.unset, "changed", "Loot flag removed");
    notes.push(...foldedNote(foldedTemplates, "templates with a changed loot flag"));
  }

  if (sourcesBoth) {
    const a = fixedPlacements(ctx.before);
    const b = fixedPlacements(ctx.after);
    const areas = (map: Map<string, Set<string>>) => (group: string[]) => {
      const all = new Set(group.flatMap((cls) => [...(map.get(cls.toLowerCase()) ?? [])]));
      return `${all.size} area${all.size === 1 ? "" : "s"}: ${listSome([...all].sort(), 2)}`;
    };
    emit([...b.keys()].filter((k) => !a.has(k)), "added", "Now placed in the world", areas(b));
    emit([...a.keys()].filter((k) => !b.has(k)), "removed", "No longer placed in the world", areas(a));

    const awards = (data: BuildData) => {
      const out = new Map<string, Set<string>>();
      for (const award of list(data, FILES.sources, "contractAwards")) {
        const item = String(award.item);
        out.set(item, (out.get(item) ?? new Set()).add(String(award.contract)));
      }
      return out;
    };
    const wa = awards(ctx.before);
    const wb = awards(ctx.after);
    const delta = (x: Map<string, Set<string>>, y: Map<string, Set<string>>) =>
      [...x.entries()].map(([item, contracts]) => [item, [...contracts].filter((c) => !y.get(item)?.has(c))] as const).filter(([, c]) => c.length > 0);
    for (const [item, contracts] of delta(wb, wa)) emit([item], "added", "Contract awards", () => `awarded by ${listSome(contracts.sort())}`);
    for (const [item, contracts] of delta(wa, wb)) emit([item], "removed", "Contract awards", () => `no longer awarded by ${listSome(contracts.sort())}`);
  }
  return section("loot", "Loot & where to find things", lines, notes);
}

function renames(ctx: Context): NoteSection {
  const pairs = [...ctx.renames];
  const sa = ctx.before.get(FILES.strings)?.strings;
  const sb = ctx.after.get(FILES.strings)?.strings;
  if (sa && sb && typeof sa === "object" && typeof sb === "object") {
    const A = sa as Record<string, unknown>;
    for (const [key, value] of Object.entries(sb as Record<string, unknown>)) {
      if (!/^item_name/i.test(key)) continue;
      const prior = A[key];
      if (isRealName(prior) && isRealName(value) && prior !== value) pairs.push({ from: prior, to: value, record: key });
    }
  }
  const groups = new Map<string, { from: string; to: string; records: Set<string> }>();
  for (const p of pairs) {
    const from = stripSlot(p.from);
    const to = stripSlot(p.to);
    const key = `${from}\u0000${to}`;
    const entry = groups.get(key) ?? { from, to, records: new Set<string>() };
    entry.records.add(shortRecord(p.record));
    groups.set(key, entry);
  }
  // Two records that trade names (a common fix-up) read as one swap, not two renames.
  const lines: NoteLine[] = [];
  const done = new Set<string>();
  for (const [key, g] of [...groups.entries()].sort((x, y) => x[1].from.localeCompare(y[1].from))) {
    if (done.has(key)) continue;
    const reverseKey = `${g.to}\u0000${g.from}`;
    const reverse = groups.get(reverseKey);
    const records = new Set([...g.records, ...(reverse?.records ?? [])]);
    if (reverse) done.add(reverseKey);
    const line: NoteLine = {
      label: g.to,
      status: "changed",
      records: [...records].sort(),
      changes: [{ stat: "name", before: g.from, after: g.to, direction: "changed" }],
    };
    if (reverse) line.detail = "swapped: each now carries the other name";
    lines.push(line);
  }
  return section("renames", "Renames", lines);
}

function entriesOf(collection: unknown): Entry[] {
  if (Array.isArray(collection)) return collection.map((v, i) => [String(i), v]);
  if (collection && typeof collection === "object") return Object.entries(collection as Record<string, unknown>);
  return [];
}

function other(ctx: Context, files: string[], extraNotes: string[] = []): NoteSection {
  const lines: NoteLine[] = [];
  const notes: string[] = [...extraNotes];
  for (const file of files) {
    const a = ctx.before.get(file);
    const b = ctx.after.get(file);
    const dataset = file.replace(/^game-|\.json$/g, "");
    if (!a || !b) {
      notes.push(`${dataset}: only in the ${a ? "older" : "newer"} build`);
      continue;
    }
    const itemised = ITEMISED[file];
    if (itemised === "all") continue;
    const parts: string[] = [];
    const counted: string[] = [];
    for (const collection of [...new Set([...diffableCollections(a), ...diffableCollections(b)])]) {
      if (itemised?.includes(collection)) continue;
      if (!(collection in a)) {
        parts.push(`${humanise(collection)} first tracked`);
        counted.push(collection);
        continue;
      }
      const d = diffCollection(collection, entriesOf(a[collection]), entriesOf(b[collection]), { limit: 0 });
      const s = { ...d.summary };
      if (ADDS_ITEMISED[file] === "all") s.added = s.removed = 0;
      if (s.added + s.removed + s.changed === 0) continue;
      const bits = [s.added && `+${s.added}`, s.removed && `−${s.removed}`, s.changed && `~${s.changed}`].filter(Boolean).join(" ");
      parts.push(`${humanise(collection)} ${bits}`);
      counted.push(collection);
    }
    if (parts.length > 0) lines.push({ label: dataset, status: "changed", category: "Datasets", records: counted, detail: parts.join(", ") });
  }
  return section("other", "Other changes", lines, notes);
}

// ---------------------------------------------------------------------------
// Entry points
// ---------------------------------------------------------------------------

/**
 * Notes from data already in memory. `changes` is the patchChanges result for
 * the same two builds; it supplies every buff/nerf stat row.
 */
export function composeNotes(
  before: BuildData,
  after: BuildData,
  changes: PatchChanges,
  options: NotesOptions & { from?: NotesDoc["from"]; to?: NotesDoc["to"] } = {},
): NotesDoc {
  const minChange = options.minChange ?? 1;
  // A redacted published string table against a full local one: compare on published keys only.
  const otherNotes: string[] = [];
  const sa = before.get(FILES.strings);
  const sb = after.get(FILES.strings);
  if (sa && sb) {
    const aligned = alignRedaction(sa, sb);
    if (aligned.note) {
      before = new Map(before).set(FILES.strings, aligned.before);
      after = new Map(after).set(FILES.strings, aligned.after);
      otherNotes.push(aligned.note);
    }
  }
  const ctx: Context = {
    before,
    after,
    changes,
    minChange,
    counters: { minor: 0, newlyTracked: 0, untracked: 0 },
    names: nameIndex(after, before),
    families: familyIndex(after, before),
    renames: [],
    siblings: new Map(),
  };
  const files = [...new Set([...before.keys(), ...after.keys()])].sort();
  // The record sections collect renames as they compare records; the Renames section is built after them.
  const built = [
    newShips(ctx),
    shipBalance(ctx),
    newShipItems(ctx),
    newPersonalWeapons(ctx),
    personalBalance(ctx),
    armour(ctx),
    crafting(ctx),
    missions(ctx),
    loot(ctx),
  ];
  // changes.ts reports renames of tracked items too (ships, components); fold them in.
  for (const item of changes.items) {
    if (item.previousName && isRealName(item.previousName) && isRealName(item.name)) {
      ctx.renames.push({ from: item.previousName, to: item.name, record: item.record });
    }
  }
  built.push(renames(ctx), other(ctx, files, otherNotes));
  return {
    from: options.from ?? { source: "before", build: stamp(before) },
    to: options.to ?? { source: "after", build: stamp(after) },
    minChange,
    sections: built.filter((s) => s.lines.length > 0 || s.notes.length > 0),
    hidden: { minor: ctx.counters.minor, newlyTracked: ctx.counters.newlyTracked, untracked: ctx.counters.untracked },
  };
}

/** The build stamp of the first dataset that carries one. */
function stamp(data: BuildData): string | undefined {
  for (const body of data.values()) if (typeof body._build === "string") return body._build;
  return undefined;
}

/**
 * Every game-*.json the two sources hold, loaded into memory. `served`, when
 * given, receives where each file was actually read from (Snapshot.source),
 * which differs from the source when a snapshot's published fallback served it.
 */
export function loadBuildData(source: BuildSource, files: string[], served?: Map<string, string>): BuildData {
  const data: BuildData = new Map();
  for (const file of files) {
    const snap = loadSnapshot(source, file);
    if (snap) {
      data.set(file, snap.body);
      served?.set(file, snap.source);
    }
  }
  return data;
}

/** The side as notes print it, with any files a fallback served. */
function side(source: BuildSource, data: BuildData, served: Map<string, string>): NotesSide {
  const out: NotesSide = { source: sourceLabel(source), build: stamp(data) };
  const own = describeSource(source);
  const elsewhere = [...served.entries()].filter(([, from]) => from !== own);
  if (elsewhere.length > 0) {
    out.fallback = { source: elsewhere[0][1], files: elsewhere.map(([file]) => file).sort() };
  }
  return out;
}

/**
 * Where a side came from, without the local path: the notes get published, and
 * a home directory means nothing to their readers.
 */
function sourceLabel(source: BuildSource): string {
  return source.kind === "dir" ? `dir ${path.basename(source.path)}` : describeSource(source);
}

/** Patch notes between two builds, read from their datasets. */
export function buildNotes(from: BuildSource, to: BuildSource, options: NotesOptions = {}): NotesDoc {
  const files = listDatasetFiles(from, to);
  const servedFrom = new Map<string, string>();
  const servedTo = new Map<string, string>();
  const before = loadBuildData(from, files, servedFrom);
  const after = loadBuildData(to, files, servedTo);
  return composeNotes(before, after, patchChanges(from, to), {
    ...options,
    from: side(from, before, servedFrom),
    to: side(to, after, servedTo),
  });
}

// ---------------------------------------------------------------------------
// Markdown
// ---------------------------------------------------------------------------

export function fmtNumber(value: number): string {
  const abs = Math.abs(value);
  const places = abs >= 100 ? 0 : abs >= 1 ? 2 : 3;
  return value.toLocaleString("en-US", { maximumFractionDigits: places });
}

function fmtValue(value: Scalar | undefined): string {
  if (value === undefined || value === null) return "–";
  if (typeof value === "number") return fmtNumber(value);
  return typeof value === "string" ? `"${value}"` : String(value);
}

function fmtChange(c: NoteChange): string {
  let pct = "";
  if (c.percent !== undefined) {
    const ratio = 1 + c.percent / 100;
    pct = Math.abs(c.percent) >= 1000 ? ` (×${fmtNumber(ratio)})` : ` (${c.percent > 0 ? "+" : "−"}${fmtNumber(Math.abs(round(c.percent, 1)))}%)`;
  }
  return `${c.stat} ${fmtValue(c.before)} → ${fmtValue(c.after)}${pct}`;
}

function verdict(changes: NoteChange[] = []): string {
  const dirs = new Set(changes.map((c) => c.direction).filter((d) => d !== "changed"));
  if (dirs.size === 0) return "";
  return dirs.size === 2 ? " (mixed)" : dirs.has("buff") ? " (buff)" : " (nerf)";
}

const MAX_CHANGES = 6;

function renderLine(line: NoteLine): string {
  let text = `**${line.label}**`;
  if (line.variants && line.variants.length > 0) {
    text += ` (+${line.variants.length} variant${line.variants.length === 1 ? "" : "s"}: ${listSome(line.variants, 5)})`;
  } else if (line.records.length > 1 && line.status === "changed" && line.changes && line.category !== undefined) {
    text += ` (+${line.records.length - 1} variant${line.records.length === 2 ? "" : "s"})`;
  } else if (line.records.length > 1 && line.status !== "changed" && !line.detail?.includes("pieces") && !line.detail?.startsWith("for ")) {
    text += ` ×${line.records.length}`;
  }
  if (line.status === "removed") text = `Removed: ${text}`;
  if (line.changes && line.changes.length > 0) {
    if (line.changes.length === 1 && line.changes[0].stat === "name") {
      text = `${fmtValue(line.changes[0].before).replace(/^"|"$/g, "")} → **${line.label}**`;
    } else {
      const shown = line.changes.slice(0, MAX_CHANGES).map(fmtChange);
      const more = line.changes.length > MAX_CHANGES ? `; +${line.changes.length - MAX_CHANGES} more` : "";
      text += `${verdict(line.changes)}: ${shown.join("; ")}${more}`;
    }
  }
  if (line.detail) text += ` — ${line.detail}`;
  return `- ${text}`;
}

const STATUS_ORDER = { added: 0, changed: 1, removed: 2 } as const;

/** Markdown for people: sections in fixed order, sub-headed by category where a section mixes kinds. */
export function renderNotes(doc: NotesDoc): string {
  const out: string[] = [];
  const name = (side: NotesDoc["from"]) => side.build ?? side.source;
  out.push(`# Patch notes: ${name(doc.from)} → ${name(doc.to)}`, "");
  for (const s of [doc.from, doc.to]) {
    if (!s.fallback) continue;
    const files = s.fallback.files.map((f) => f.replace(/^game-|\.json$/g, "")).join(", ");
    out.push(`_${name(s)}: ${files} read from ${s.fallback.source} (not in its snapshot)._`, "");
  }
  for (const s of doc.sections) {
    out.push(`## ${s.title}`, "");
    const categories = [...new Set(s.lines.map((l) => l.category ?? ""))];
    const sub = categories.length > 1 || (categories[0] !== "" && s.id === "shipBalance");
    for (const category of categories) {
      const lines = s.lines
        .filter((l) => (l.category ?? "") === category)
        .sort((a, b) => STATUS_ORDER[a.status] - STATUS_ORDER[b.status]);
      if (sub && category) out.push(`### ${categoryLabel(category)}`, "");
      for (const line of lines) out.push(renderLine(line));
      out.push("");
    }
    for (const note of s.notes) out.push(`_${note}._`);
    if (s.notes.length > 0) out.push("");
  }
  if (doc.sections.length === 0) out.push("No player-facing changes.", "");
  const hidden: string[] = [];
  if (doc.hidden.minor > 0) hidden.push(`${doc.hidden.minor} minor change${doc.hidden.minor === 1 ? "" : "s"} under ${doc.minChange}% hidden`);
  if (doc.hidden.untracked > 0) hidden.push(`${doc.hidden.untracked} record${doc.hidden.untracked === 1 ? "" : "s"} changed only outside the tracked stats`);
  if (doc.hidden.newlyTracked > 0) hidden.push(`${doc.hidden.newlyTracked} newly tracked field${doc.hidden.newlyTracked === 1 ? "" : "s"} (no old value) hidden`);
  if (hidden.length > 0) out.push("---", "", `_${hidden.join("; ")}._`, "");
  return out.join("\n");
}
