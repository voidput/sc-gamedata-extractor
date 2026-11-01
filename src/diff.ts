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
 * "local", a directory, a local snapshot, then a dumps-repo tag.
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
 * only the datasets it writes.
 */
export function saveSnapshot(dataDir: string, root: string, build: string, files?: string[]): LocalSnapshot {
  const dir = path.join(root, build);
  fs.mkdirSync(dir, { recursive: true });
  const candidates = files ?? fs.readdirSync(dataDir).filter((f) => f.startsWith("game-") && f.endsWith(".json"));
  for (const file of candidates) {
    const text = fs.readFileSync(path.join(dataDir, file), "utf-8");
    const stamp = /"_build":\s*"([^"]+)"/.exec(text.slice(0, 4096))?.[1];
    if (stamp !== build) continue;
    fs.writeFileSync(path.join(dir, `${file}.gz`), zlib.gzipSync(text, { level: 6 }));
  }
  const saved = new Date().toISOString();
  fs.writeFileSync(path.join(dir, "_snapshot.json"), JSON.stringify({ build, saved }, null, 2));
  return { build, path: dir, datasets: fs.readdirSync(dir).filter((f) => f.endsWith(".json.gz")).length, saved };
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
  | { kind: "dir"; path: string }
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
    if (hit) return { kind: "dir", path: hit.path };
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

export function describeSource(source: BuildSource): string {
  return source.kind === "dir" ? source.path : `tag ${source.tag}`;
}

export interface Snapshot {
  build?: string;
  extracted?: string;
  body: Record<string, unknown>;
}

const META_KEYS = new Set(["_source", "_extracted", "_build", "_redacted", "summary"]);

/** Read one dataset file from a source; undefined when the build lacks it. */
export function loadSnapshot(source: BuildSource, file: string): Snapshot | undefined {
  let text: string;
  if (source.kind === "dir") {
    const full = path.join(source.path, file);
    if (fs.existsSync(full)) text = fs.readFileSync(full, "utf-8");
    else if (fs.existsSync(`${full}.gz`)) text = zlib.gunzipSync(fs.readFileSync(`${full}.gz`)).toString("utf-8");
    else return undefined;
  } else {
    try {
      text = git(source.repoDir, ["show", `${source.tag}:data/${file}`]);
    } catch {
      return undefined;
    }
  }
  const parsed = JSON.parse(text) as Record<string, unknown>;
  return {
    build: typeof parsed._build === "string" ? parsed._build : undefined,
    extracted: typeof parsed._extracted === "string" ? parsed._extracted : undefined,
    body: parsed,
  };
}

/** Dataset files present in either source. */
export function listDatasetFiles(a: BuildSource, b: BuildSource): string[] {
  const files = new Set<string>();
  for (const source of [a, b]) {
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
      if (plain.startsWith("game-") && plain.endsWith(".json")) files.add(plain);
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
  /** Set when one side lacks the file entirely. */
  missing?: "from" | "to";
  collections: CollectionDiff[];
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
    from: { source: describeSource(from), build: before?.build },
    to: { source: describeSource(to), build: after?.build },
    collections: [],
  };
  if (!before || !after) {
    result.missing = before ? "to" : "from";
    return result;
  }

  const names = options.collection
    ? [options.collection]
    : [...new Set([...diffableCollections(before.body), ...diffableCollections(after.body)])];

  for (const name of names) {
    if (!(name in before.body) && !(name in after.body)) {
      throw new Error(
        `${file} has no collection "${name}". Available: ${diffableCollections(after.body).join(", ")}`,
      );
    }
    result.collections.push(
      diffCollection(name, entriesOf(before.body[name]), entriesOf(after.body[name]), options),
    );
  }
  return result;
}

/** Dataset argument -> file: "weapons" and "game-weapons" both work. */
export function datasetFile(name: string): string {
  const base = name.replace(/\.json$/, "");
  return `${base.startsWith("game-") ? base : `game-${base}`}.json`;
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

  if (diff.missing) {
    lines.push(`  (not present in ${diff.missing === "from" ? "the older" : "the newer"} build)`);
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
