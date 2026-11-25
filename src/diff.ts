/**
 * Patch-over-patch comparison of parsed datasets.
 *
 * A build is identified by where its datasets come from: a directory of
 * `game-*.json` (the local `data/` after a parse, or any other checkout), or a
 * tag in the public dumps repo, where `publish-dump` commits one build per tag
 * and `git show <tag>:data/<file>` serves any past patch without checking it
 * out. The two are interchangeable here, so the local build can be compared
 * against the last published one before it is pushed.
 *
 * Every parse also keeps a gzipped copy of its datasets under
 * `snapshots/<build>/`, so each patch the tool has ever parsed is a diff
 * target by build number with no publish step and no git. Resolution order:
 * "local", a directory, a local snapshot, then a dumps-repo tag. A snapshot
 * that lacks a dataset (the string table is written after parse, and older
 * snapshots predate some domains) falls back to the published tag of the same
 * build, when the dumps checkout has one.
 *
 * Derived files are not datasets: game-search-index.json restates item-tags
 * and ships as positional rows, so it is never snapshotted, listed or diffed
 * unless asked for by name. A published string table is redacted to name keys
 * (scripts/publish-dump.mjs); compared with a full local table, the full side
 * is cut to the same keys first, so redaction never reads as added strings.
 *
 * Records are matched by `recordName` (or `id`, or `name` as a last resort),
 * never by display name, which the game renames between patches. Field
 * changes are reported on the flattened object, so a nested change reads
 * `ammo.damage.energy: 55 -> 60` rather than "ammo changed".
 */

import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import zlib from "node:zlib";

import { SEARCH_INDEX_FILE } from "./query/search-index.js";

/**
 * Files beside the datasets that are derived from them. They are left out of
 * snapshots and of the default dataset list, so `diff` and `notes` never
 * count them (the search index's rows are positional: one new item shifts
 * every row after it).
 */
export const DERIVED_FILES: ReadonlySet<string> = new Set([SEARCH_INDEX_FILE]);

/** Where per-build copies live. Shared by parse (writes) and diff (reads). */
export function snapshotsDir(root: string): string {
  return process.env.SC_SNAPSHOTS_DIR || path.join(root, "snapshots");
}

export interface LocalSnapshot {
  build: string;
  path: string;
  datasets: number;
  saved: string;
}

/**
 * Copy the datasets stamped with `build` into `<snapshots>/<build>/`, gzipped.
 * A dataset carrying a different `_build` is skipped, so a partial re-parse of
 * one domain never mixes builds inside a snapshot. Re-saving a build replaces
 * only the datasets it writes. Derived files (DERIVED_FILES) are never kept.
 */
export function saveSnapshot(dataDir: string, root: string, build: string, files?: string[]): LocalSnapshot {
  const dir = path.join(root, build);
  fs.mkdirSync(dir, { recursive: true });
  const candidates = files ?? fs.readdirSync(dataDir).filter((f) => f.startsWith("game-") && f.endsWith(".json"));
  for (const file of candidates) {
    if (DERIVED_FILES.has(file)) continue;
    const text = fs.readFileSync(path.join(dataDir, file), "utf-8");
    if (stampOf(text) !== build) continue;
    fs.writeFileSync(path.join(dir, `${file}.gz`), zlib.gzipSync(text, { level: 6 }));
  }
  const saved = new Date().toISOString();
  fs.writeFileSync(path.join(dir, "_snapshot.json"), JSON.stringify({ build, saved }, null, 2));
  return { build, path: dir, datasets: fs.readdirSync(dir).filter((f) => f.endsWith(".json.gz")).length, saved };
}

function stampOf(text: string): string | undefined {
  return /"_build":\s*"([^"]+)"/.exec(text.slice(0, 4096))?.[1];
}

/**
 * Snapshot one dataset that was written outside parse (the string table, which
 * `strings --write` produces after parse has already saved its snapshot). The
 * build comes from the file's own `_build` stamp; an unstamped file is not
 * snapshotted, since it cannot be filed under a build.
 */
export function snapshotDataset(file: string, root: string): LocalSnapshot | undefined {
  const build = stampOf(fs.readFileSync(file, "utf-8"));
  if (!build) return undefined;
  return saveSnapshot(path.dirname(file), root, build, [path.basename(file)]);
}

/** Local snapshots, newest build first by version order. */
export function listSnapshots(root: string): LocalSnapshot[] {
  if (!fs.existsSync(root)) return [];
  return fs
    .readdirSync(root, { withFileTypes: true })
    .filter((e) => e.isDirectory())
    .map((e) => {
      const dir = path.join(root, e.name);
      let saved = "";
      try {
        saved = JSON.parse(fs.readFileSync(path.join(dir, "_snapshot.json"), "utf-8")).saved ?? "";
      } catch {
        // No marker: still usable, just undated.
      }
      return {
        build: e.name,
        path: dir,
        datasets: fs.readdirSync(dir).filter((f) => /^game-.*\.json(\.gz)?$/.test(f)).length,
        saved,
      };
    })
    .filter((s) => s.datasets > 0)
    .sort((a, b) => compareVersions(b.build, a.build));
}

/** Numeric, part by part: 4.10.196 sorts after 4.9.2. */
export function compareVersions(a: string, b: string): number {
  const pa = a.split(".").map(Number);
  const pb = b.split(".").map(Number);
  for (let i = 0; i < Math.max(pa.length, pb.length); i++) {
    const d = (pa[i] ?? 0) - (pb[i] ?? 0);
    if (d !== 0 && !Number.isNaN(d)) return d;
  }
  return a.localeCompare(b);
}

export type BuildSource =
  /** `fallback` serves any dataset the directory lacks (a snapshot's published twin). */
  | { kind: "dir"; path: string; fallback?: BuildSource }
  | { kind: "tag"; tag: string; repoDir: string };

export interface DumpsRepoOptions {
  /** Checkout of the dumps repo; shared with publish-dump. */
  workDir: string;
  remote: string;
}

export function dumpsRepoOptions(root: string): DumpsRepoOptions {
  const repo = process.env.SC_DUMPS_REPO || "voidput/sc-gamedata-dumps";
  return {
    workDir: process.env.SC_DUMPS_WORKDIR || path.join(root, ".publish-repo"),
    remote: process.env.SC_DUMPS_REMOTE || `git@github.com:${repo}.git`,
  };
}

function git(repoDir: string, args: string[], options: { maxBuffer?: number } = {}): string {
  return execFileSync("git", args, {
    cwd: repoDir,
    encoding: "utf-8",
    stdio: ["ignore", "pipe", "pipe"],
    maxBuffer: options.maxBuffer ?? 256 * 1024 * 1024,
  }).trim();
}

export interface PublishedBuild {
  version: string;
  date: string;
}

/** Builds in the dumps checkout, newest first. Fetches tags when asked. */
export function listPublishedBuilds(repo: DumpsRepoOptions, fetch = false): PublishedBuild[] {
  if (!fs.existsSync(path.join(repo.workDir, ".git"))) {
    if (!fetch) return [];
    fs.mkdirSync(repo.workDir, { recursive: true });
    execFileSync("git", ["clone", "--quiet", repo.remote, repo.workDir], { stdio: "inherit" });
  } else if (fetch) {
    git(repo.workDir, ["fetch", "--quiet", "--tags", "origin"]);
  }

  const out = git(repo.workDir, [
    "for-each-ref",
    "--sort=-creatordate",
    "--format=%(refname:short)\t%(creatordate:short)",
    "refs/tags",
  ]);
  if (!out) return [];
  return out.split("\n").map((line) => {
    const [version, date = ""] = line.split("\t");
    return { version, date };
  });
}

/**
 * Turn a build argument into a source. `local` is the parse output directory,
 * an existing directory is used as-is, and anything else is a tag in the
 * dumps repo, which is fetched once if the tag is not there yet.
 */
export function resolveSource(
  spec: string,
  options: { dataDir: string; repo: DumpsRepoOptions; snapshots?: string },
): BuildSource {
  if (spec === "local") return { kind: "dir", path: options.dataDir };
  if (fs.existsSync(spec) && fs.statSync(spec).isDirectory()) {
    return { kind: "dir", path: path.resolve(spec) };
  }
  if (options.snapshots) {
    const local = listSnapshots(options.snapshots);
    const hit = local.find((s) => s.build === spec) ?? local.find((s) => s.build.startsWith(spec + "."));
    if (hit) {
      const fallback = publishedTag(hit.build, options.repo);
      return fallback ? { kind: "dir", path: hit.path, fallback } : { kind: "dir", path: hit.path };
    }
  }

  const { repo } = options;
  let builds = listPublishedBuilds(repo);
  if (!builds.some((b) => b.version === spec)) builds = listPublishedBuilds(repo, true);
  if (!builds.some((b) => b.version === spec)) {
    // Allow a prefix: "4.10" picks the newest 4.10.x build.
    const match = builds.find((b) => b.version.startsWith(spec + "."));
    if (match) return { kind: "tag", tag: match.version, repoDir: repo.workDir };
    throw new Error(
      `"${spec}" is neither a directory nor a published build. Published: ${
        builds.map((b) => b.version).join(", ") || "(none)"
      }. Use "local" for ${options.dataDir}.`,
    );
  }
  return { kind: "tag", tag: spec, repoDir: repo.workDir };
}

/**
 * The published tag for exactly this build in the existing checkout, if any.
 * Never fetches: a snapshot is a local diff target and should stay one.
 */
function publishedTag(build: string, repo: DumpsRepoOptions): BuildSource | undefined {
  try {
    return listPublishedBuilds(repo).some((b) => b.version === build)
      ? { kind: "tag", tag: build, repoDir: repo.workDir }
      : undefined;
  } catch {
    return undefined;
  }
}

export function describeSource(source: BuildSource): string {
  return source.kind === "dir" ? source.path : `tag ${source.tag}`;
}

export interface Snapshot {
  /** Where the file was actually read from; differs from the source on a fallback. */
  source: string;
  build?: string;
  extracted?: string;
  body: Record<string, unknown>;
}

const META_KEYS = new Set(["_source", "_extracted", "_build", "_redacted", "_assumptions", "summary"]);

/** Read one dataset file from a source; undefined when the build lacks it. */
export function loadSnapshot(source: BuildSource, file: string): Snapshot | undefined {
  let text: string;
  if (source.kind === "dir") {
    const full = path.join(source.path, file);
    if (fs.existsSync(full)) text = fs.readFileSync(full, "utf-8");
    else if (fs.existsSync(`${full}.gz`)) text = zlib.gunzipSync(fs.readFileSync(`${full}.gz`)).toString("utf-8");
    else return source.fallback ? loadSnapshot(source.fallback, file) : undefined;
  } else {
    try {
      text = git(source.repoDir, ["show", `${source.tag}:data/${file}`]);
    } catch {
      return undefined;
    }
  }
  const parsed = JSON.parse(text) as Record<string, unknown>;
  return {
    source: describeSource(source),
    build: typeof parsed._build === "string" ? parsed._build : undefined,
    extracted: typeof parsed._extracted === "string" ? parsed._extracted : undefined,
    body: parsed,
  };
}

/**
 * The published-string rule of scripts/publish-dump.mjs (KEEP_KEY, DROP_KEY,
 * MAX_VALUE_LENGTH there; diff.test.ts fails if the two drift apart): name-
 * shaped keys only, no prose namespaces, no long values.
 */
const PUBLISHED_KEY = /(^|_)(Names?|Titles?|DisplayName|ShortName|Nickname)([A-Z0-9_]|$)/;
const UNPUBLISHED_KEY = /desc|tooltip|hint|subtitle|^Dlg_|^PU_|^DX[SM]/i;
const MAX_PUBLISHED_VALUE = 120;

/** Whether publish keeps this localisation entry. */
export function isPublishedString(key: string, value: unknown): boolean {
  if (typeof value !== "string" || !value || value.length > MAX_PUBLISHED_VALUE) return false;
  return !UNPUBLISHED_KEY.test(key) && PUBLISHED_KEY.test(key);
}

type Body = Record<string, unknown>;

const isRedactedTable = (body: Body) => !!body._redacted && !!body.strings && typeof body.strings === "object";

/**
 * Make two string tables comparable. A published (redacted) table holds only
 * name keys, so against a full local table every other key would read as
 * added or removed. When exactly one side is redacted, the other is cut to the
 * keys publish would keep; `note` says so. Any other pair comes back as is.
 */
export function alignRedaction(before: Body, after: Body): { before: Body; after: Body; note?: string } {
  const prose = alignProse(before, after);
  if (prose) return prose;
  const ra = isRedactedTable(before);
  const rb = isRedactedTable(after);
  if (ra === rb) return { before, after };
  const full = ra ? after : before;
  if (!full.strings || typeof full.strings !== "object") return { before, after };
  const strings = Object.fromEntries(
    Object.entries(full.strings as Record<string, unknown>).filter(([k, v]) => isPublishedString(k, v)),
  );
  const cut = { ...full, strings };
  const note = `${ra ? "older" : "newer"} string table is the redacted published one; the ${ra ? "newer" : "older"} table is compared on published name keys only`;
  return ra ? { before, after: cut, note } : { before: cut, after, note };
}

/** Fields a published dataset dropped, from its `_redacted` list (scripts/redact.mjs). */
const proseRedaction = (body: Body): string[] | undefined =>
  Array.isArray(body._redacted) ? body._redacted.filter((f): f is string => typeof f === "string") : undefined;

/** Remove `fields` from every object under `value`. */
function stripFields(value: unknown, fields: Set<string>): unknown {
  if (Array.isArray(value)) return value.map((v) => stripFields(v, fields));
  if (!value || typeof value !== "object") return value;
  return Object.fromEntries(
    Object.entries(value as Body)
      .filter(([k]) => !fields.has(k))
      .map(([k, v]) => [k, stripFields(v, fields)]),
  );
}

/**
 * Make a full dataset comparable with one published without its prose. The
 * published side lists the fields it lost in `_redacted`; the other side loses
 * the same, including the long-mission-title rule ("title over N characters":
 * the title goes and `name`, its copy, becomes the record's short name).
 * Undefined when neither or both sides are prose-redacted.
 */
function alignProse(before: Body, after: Body): { before: Body; after: Body; note: string } | undefined {
  const ra = proseRedaction(before);
  const rb = proseRedaction(after);
  if (!ra === !rb) return undefined;
  const list = (ra ?? rb) as string[];
  const full = ra ? after : before;
  const limit = list.map((f) => /^title over (\d+) characters$/.exec(f)?.[1]).find(Boolean);
  let cut = stripFields(full, new Set(list)) as Body;
  if (limit && Array.isArray(cut.missions)) {
    const max = Number(limit);
    cut = {
      ...cut,
      missions: cut.missions.map((m: Body) => {
        if (typeof m.title !== "string" || m.title.length <= max) return m;
        const rest = { ...m };
        delete rest.title;
        const short = String(m.recordName ?? "").split(".").pop() || m.id;
        return m.name === m.title ? { ...rest, name: short } : rest;
      }),
    };
  }
  const note = `${ra ? "older" : "newer"} side is the published dataset without its prose (${list.join(", ")}); the ${ra ? "newer" : "older"} side is compared without it too`;
  return ra ? { before, after: cut, note } : { before: cut, after, note };
}

/** Dataset files present in either source, counting a snapshot's fallback; derived files excluded. */
export function listDatasetFiles(a: BuildSource, b: BuildSource): string[] {
  const files = new Set<string>();
  const sources: BuildSource[] = [];
  for (let s: BuildSource | undefined = a; s; s = s.kind === "dir" ? s.fallback : undefined) sources.push(s);
  for (let s: BuildSource | undefined = b; s; s = s.kind === "dir" ? s.fallback : undefined) sources.push(s);
  for (const source of sources) {
    const names =
      source.kind === "dir"
        ? fs.existsSync(source.path)
          ? fs.readdirSync(source.path)
          : []
        : (() => {
            try {
              return git(source.repoDir, ["ls-tree", "--name-only", `${source.tag}:data`]).split("\n");
            } catch {
              return [];
            }
          })();
    for (const name of names) {
      const plain = name.replace(/\.gz$/, "");
      if (plain.startsWith("game-") && plain.endsWith(".json") && !DERIVED_FILES.has(plain)) files.add(plain);
    }
  }
  return [...files].sort();
}

/**
 * Collections worth diffing in a dump: the arrays and maps of records. `by*`
 * keys are derived indexes over the same records and would double-count.
 */
export function diffableCollections(body: Record<string, unknown>): string[] {
  return Object.keys(body).filter((key) => {
    if (META_KEYS.has(key) || key.startsWith("by")) return false;
    const value = body[key];
    return Array.isArray(value) || (!!value && typeof value === "object");
  });
}

export type Entry = [key: string, record: unknown];

function entriesOf(collection: unknown): Entry[] {
  if (Array.isArray(collection)) return collection.map((v, i) => [String(i), v]);
  if (collection && typeof collection === "object") {
    return Object.entries(collection as Record<string, unknown>);
  }
  return [];
}

/** Stable identity of a record across patches. Display names do not qualify. */
export function keyOf(entry: unknown): string | undefined {
  if (!entry || typeof entry !== "object") return undefined;
  const e = entry as Record<string, unknown>;
  for (const field of ["recordName", "className", "id", "reference", "key", "name"]) {
    if (typeof e[field] === "string" && e[field]) return e[field] as string;
  }
  return undefined;
}

function indexBy(entries: Entry[]): Map<string, unknown> {
  const map = new Map<string, unknown>();
  for (const [key, record] of entries) map.set(keyOf(record) ?? key, record);
  return map;
}

export function flatten(
  value: unknown,
  prefix = "",
  out: Record<string, unknown> = {},
): Record<string, unknown> {
  if (Array.isArray(value)) {
    value.forEach((v, i) => flatten(v, `${prefix}[${i}]`, out));
  } else if (value && typeof value === "object") {
    for (const [k, v] of Object.entries(value as Record<string, unknown>)) {
      flatten(v, prefix ? `${prefix}.${k}` : k, out);
    }
  } else {
    out[prefix] = value;
  }
  return out;
}

export interface FieldChange {
  field: string;
  from: unknown;
  to: unknown;
}

export function diffEntry(before: unknown, after: unknown): FieldChange[] {
  const fa = flatten(before);
  const fb = flatten(after);
  const changes: FieldChange[] = [];
  for (const field of new Set([...Object.keys(fa), ...Object.keys(fb)])) {
    if (JSON.stringify(fa[field]) !== JSON.stringify(fb[field])) {
      changes.push({ field, from: fa[field], to: fb[field] });
    }
  }
  return changes.sort((a, b) => a.field.localeCompare(b.field));
}

export function labelOf(entry: unknown, key: string): string {
  if (!entry || typeof entry !== "object") return key;
  const e = entry as Record<string, unknown>;
  for (const field of ["name", "displayName", "title"]) {
    const value = e[field];
    if (typeof value === "string" && value && !value.includes("PLACEHOLDER")) {
      return value === key ? key : `${value} (${key})`;
    }
  }
  return key;
}

export interface ChangedEntry {
  key: string;
  item: string;
  changeCount: number;
  changes?: FieldChange[];
}

export interface CollectionDiff {
  collection: string;
  summary: { added: number; removed: number; changed: number; unchanged: number };
  added: string[];
  removed: string[];
  changed: ChangedEntry[];
}

export interface DiffOptions {
  /** Case-insensitive substring over keys and labels; implies field detail. */
  item?: string;
  /** Only report changes touching these fields (prefix match on the flat path). */
  fields?: string[];
  /** Most-changed entries to list; the rest are counted. */
  limit?: number;
  /** Include per-field changes even without an item filter. */
  detail?: boolean;
}

export function diffCollection(
  collection: string,
  beforeEntries: Entry[],
  afterEntries: Entry[],
  options: DiffOptions = {},
): CollectionDiff {
  const before = indexBy(beforeEntries);
  const after = indexBy(afterEntries);
  const needle = options.item?.toLowerCase();
  const limit = options.limit ?? 50;
  const detail = options.detail || needle !== undefined;

  const wanted = (key: string, entry: unknown) =>
    needle === undefined ||
    key.toLowerCase().includes(needle) ||
    labelOf(entry, key).toLowerCase().includes(needle);

  const touchesWanted = (change: FieldChange) =>
    !options.fields || options.fields.some((f) => change.field === f || change.field.startsWith(f + ".") || change.field.startsWith(f + "["));

  const added: string[] = [];
  const removed: string[] = [];
  const changed: ChangedEntry[] = [];
  let unchanged = 0;

  for (const [key, entry] of after) {
    if (!before.has(key) && wanted(key, entry)) added.push(labelOf(entry, key));
  }
  for (const [key, entry] of before) {
    if (!after.has(key) && wanted(key, entry)) removed.push(labelOf(entry, key));
  }
  for (const [key, b] of before) {
    const a = after.get(key);
    if (a === undefined || !wanted(key, a)) continue;
    const changes = diffEntry(b, a).filter(touchesWanted);
    if (changes.length === 0) {
      unchanged += 1;
      continue;
    }
    changed.push({
      key,
      item: labelOf(a, key),
      changeCount: changes.length,
      changes: detail ? changes : undefined,
    });
  }

  changed.sort((x, y) => y.changeCount - x.changeCount || x.item.localeCompare(y.item));

  return {
    collection,
    summary: { added: added.length, removed: removed.length, changed: changed.length, unchanged },
    added: added.sort(),
    removed: removed.sort(),
    changed: changed.slice(0, limit),
  };
}

export interface DatasetDiff {
  file: string;
  from: { source: string; build?: string };
  to: { source: string; build?: string };
  /** Set when one side lacks the file entirely, or both do. */
  missing?: "from" | "to" | "both";
  /**
   * Caveats about what was compared: a side served by a snapshot's published
   * fallback, or a string table cut to published keys (alignRedaction).
   */
  notes?: string[];
  /**
   * Records per collection on the side that has the file, when only one does:
   * a dataset new in the newer build (missing "from") or dropped from it
   * (missing "to") is summarised rather than skipped.
   */
  counts?: Record<string, number>;
  collections: CollectionDiff[];
}

/** Records per diffable collection, or just the asked-for one. */
function collectionCounts(body: Record<string, unknown>, only?: string): Record<string, number> {
  const names = only ? [only].filter((n) => n in body) : diffableCollections(body);
  return Object.fromEntries(names.map((name) => [name, entriesOf(body[name]).length]));
}

export function diffDataset(
  file: string,
  from: BuildSource,
  to: BuildSource,
  options: DiffOptions & { collection?: string } = {},
): DatasetDiff {
  const before = loadSnapshot(from, file);
  const after = loadSnapshot(to, file);
  const result: DatasetDiff = {
    file,
    from: { source: before?.source ?? describeSource(from), build: before?.build },
    to: { source: after?.source ?? describeSource(to), build: after?.build },
    collections: [],
  };
  const notes: string[] = [];
  if (before && before.source !== describeSource(from)) notes.push(`older side read from ${before.source} (the snapshot lacks this dataset)`);
  if (after && after.source !== describeSource(to)) notes.push(`newer side read from ${after.source} (the snapshot lacks this dataset)`);
  if (!before || !after) {
    if (notes.length > 0) result.notes = notes;
    result.missing = before ? "to" : after ? "from" : "both";
    const present = before ?? after;
    if (present) result.counts = collectionCounts(present.body, options.collection);
    return result;
  }

  const aligned = alignRedaction(before.body, after.body);
  if (aligned.note) notes.push(aligned.note);
  if (notes.length > 0) result.notes = notes;

  const names = options.collection
    ? [options.collection]
    : [...new Set([...diffableCollections(aligned.before), ...diffableCollections(aligned.after)])];

  for (const name of names) {
    if (!(name in aligned.before) && !(name in aligned.after)) {
      throw new Error(
        `${file} has no collection "${name}". Available: ${diffableCollections(aligned.after).join(", ")}`,
      );
    }
    result.collections.push(
      diffCollection(name, entriesOf(aligned.before[name]), entriesOf(aligned.after[name]), options),
    );
  }
  return result;
}

/**
 * Dataset argument -> file: "weapons" and "game-weapons" both work. Bare
 * "strings" means the English table, the only one parse and publish write.
 */
export function datasetFile(name: string): string {
  let base = name.replace(/\.json$/, "").replace(/^game-/, "");
  if (base === "strings") base = "strings-english";
  return `game-${base}.json`;
}

function fmt(value: unknown): string {
  if (value === undefined) return "(absent)";
  if (typeof value === "string") return JSON.stringify(value);
  return String(value);
}

/** Plain-text rendering for the terminal. */
export function renderDiff(diff: DatasetDiff, options: { limit?: number } = {}): string {
  const lines: string[] = [];
  const header = `${diff.file}  ${diff.from.build ?? diff.from.source} -> ${diff.to.build ?? diff.to.source}`;
  lines.push(header);
  for (const note of diff.notes ?? []) lines.push(`  note: ${note}`);

  if (diff.missing) {
    const counts = Object.entries(diff.counts ?? {});
    const total = counts.reduce((sum, [, n]) => sum + n, 0);
    if (diff.missing === "both") lines.push("  (not present in either build)");
    else if (diff.missing === "from") lines.push(`  new dataset (not present in the older build): ${total} record(s)`);
    else lines.push(`  dropped (not present in the newer build): ${total} record(s) in the older build`);
    for (const [name, n] of counts) lines.push(`  ${name}: ${n}`);
    return lines.join("\n");
  }

  for (const c of diff.collections) {
    const s = c.summary;
    const quiet = s.added + s.removed + s.changed === 0;
    lines.push(`  ${c.collection}: +${s.added} -${s.removed} ~${s.changed} =${s.unchanged}${quiet ? "  (no change)" : ""}`);
    const listLimit = options.limit ?? 20;

    for (const item of c.added.slice(0, listLimit)) lines.push(`    + ${item}`);
    if (c.added.length > listLimit) lines.push(`    + ... ${c.added.length - listLimit} more added`);
    for (const item of c.removed.slice(0, listLimit)) lines.push(`    - ${item}`);
    if (c.removed.length > listLimit) lines.push(`    - ... ${c.removed.length - listLimit} more removed`);

    for (const entry of c.changed.slice(0, listLimit)) {
      lines.push(`    ~ ${entry.item}  (${entry.changeCount} field${entry.changeCount === 1 ? "" : "s"})`);
      for (const change of entry.changes ?? []) {
        lines.push(`        ${change.field}: ${fmt(change.from)} -> ${fmt(change.to)}`);
      }
    }
    if (c.changed.length > listLimit) {
      lines.push(`    ~ ... ${c.changed.length - listLimit} more changed`);
    } else if (s.changed > c.changed.length) {
      lines.push(`    ~ ... ${s.changed - c.changed.length} more changed (raise --limit)`);
    }
  }
  return lines.join("\n");
}
