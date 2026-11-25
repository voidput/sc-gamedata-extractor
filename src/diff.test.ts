import { describe, it, expect } from "vitest";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { SEARCH_INDEX_FILE } from "./query/search-index.js";
import {
  alignRedaction,
  compareVersions,
  DERIVED_FILES,
  datasetFile,
  diffableCollections,
  diffCollection,
  diffDataset,
  diffEntry,
  flatten,
  isPublishedString,
  keyOf,
  listDatasetFiles,
  listSnapshots,
  loadSnapshot,
  renderDiff,
  resolveSource,
  saveSnapshot,
  snapshotDataset,
} from "./diff.js";

const weapon = (recordName: string, name: string, dps: number, extra: Record<string, unknown> = {}) => ({
  name,
  recordName,
  dps,
  ammo: { damage: { energy: dps / 10 } },
  ...extra,
});

describe("identity", () => {
  it("keys on recordName before anything a patch might rename", () => {
    expect(keyOf({ name: "M7A Cannon", recordName: "EntityClassDefinition.BEHR_LaserCannon_S5" })).toBe(
      "EntityClassDefinition.BEHR_LaserCannon_S5",
    );
    expect(keyOf({ name: "Only a name" })).toBe("Only a name");
    expect(keyOf("not an object")).toBeUndefined();
  });

  it("flattens nested fields into dotted paths", () => {
    expect(flatten({ a: { b: [1, { c: 2 }] }, d: "x" })).toEqual({
      "a.b[0]": 1,
      "a.b[1].c": 2,
      d: "x",
    });
  });
});

describe("diffEntry", () => {
  it("reports each changed leaf, including fields that appear or vanish", () => {
    const changes = diffEntry(
      { dps: 100, ammo: { damage: { energy: 10 } }, gone: true },
      { dps: 120, ammo: { damage: { energy: 12 } }, fresh: 1 },
    );
    expect(changes).toEqual([
      { field: "ammo.damage.energy", from: 10, to: 12 },
      { field: "dps", from: 100, to: 120 },
      { field: "fresh", from: undefined, to: 1 },
      { field: "gone", from: true, to: undefined },
    ]);
  });
});

describe("diffCollection", () => {
  const before: [string, unknown][] = [
    ["0", weapon("W.A", "Alpha", 100)],
    ["1", weapon("W.B", "Bravo", 200)],
    ["2", weapon("W.C", "Charlie", 300)],
  ];
  const after: [string, unknown][] = [
    ["0", weapon("W.A", "Alpha Renamed", 100)],
    ["1", weapon("W.B", "Bravo", 250)],
    ["2", weapon("W.D", "Delta", 400)],
  ];

  it("matches by identity so a renamed item counts as changed, not removed", () => {
    const diff = diffCollection("weapons", before, after);
    expect(diff.summary).toEqual({ added: 1, removed: 1, changed: 2, unchanged: 0 });
    expect(diff.added).toEqual(["Delta (W.D)"]);
    expect(diff.removed).toEqual(["Charlie (W.C)"]);
    // Most-changed first; Bravo touches dps and the nested damage.
    expect(diff.changed.map((c) => c.key)).toEqual(["W.B", "W.A"]);
    // No field detail unless asked for.
    expect(diff.changed[0].changes).toBeUndefined();
  });

  it("gives field detail when an item is asked for", () => {
    const diff = diffCollection("weapons", before, after, { item: "bravo" });
    expect(diff.summary.changed).toBe(1);
    expect(diff.changed[0].changes).toEqual([
      { field: "ammo.damage.energy", from: 20, to: 25 },
      { field: "dps", from: 200, to: 250 },
    ]);
  });

  it("narrows to the fields that matter", () => {
    const diff = diffCollection("weapons", before, after, { fields: ["dps"], detail: true });
    // Alpha only changed its name, which is outside the asked-for fields.
    expect(diff.summary).toMatchObject({ changed: 1, unchanged: 1 });
    expect(diff.changed[0].changes).toEqual([{ field: "dps", from: 200, to: 250 }]);
  });

  it("diffs keyed maps the same way as arrays", () => {
    const diff = diffCollection(
      "strings",
      [["@a", "one"], ["@b", "two"]],
      [["@a", "one"], ["@b", "deux"], ["@c", "three"]],
      { detail: true },
    );
    expect(diff.summary).toEqual({ added: 1, removed: 0, changed: 1, unchanged: 1 });
    expect(diff.changed[0]).toMatchObject({ key: "@b", changes: [{ field: "", from: "two", to: "deux" }] });
  });
});

describe("dataset files", () => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "sc-diff-"));
  const older = path.join(tmp, "older");
  const newer = path.join(tmp, "newer");
  fs.mkdirSync(older);
  fs.mkdirSync(newer);
  const envelope = (build: string, body: Record<string, unknown>) =>
    JSON.stringify({ _source: "test", _extracted: "now", _build: build, ...body, summary: { n: 1 } });
  fs.writeFileSync(
    path.join(older, "game-weapons.json"),
    envelope("1.0.1", { shipWeapons: [weapon("W.A", "Alpha", 100)], byName: { Alpha: 0 } }),
  );
  fs.writeFileSync(
    path.join(newer, "game-weapons.json"),
    envelope("1.0.2", { shipWeapons: [weapon("W.A", "Alpha", 110)], byName: { Alpha: 0 } }),
  );
  fs.writeFileSync(path.join(newer, "game-new.json"), envelope("1.0.2", { things: [] }));

  it("accepts either spelling of a dataset name", () => {
    expect(datasetFile("weapons")).toBe("game-weapons.json");
    expect(datasetFile("game-weapons")).toBe("game-weapons.json");
    expect(datasetFile("game-weapons.json")).toBe("game-weapons.json");
    expect(datasetFile("strings")).toBe("game-strings-english.json");
    expect(datasetFile("game-strings-german_(germany)")).toBe("game-strings-german_(germany).json");
  });

  it("reads the build stamp off a directory snapshot", () => {
    expect(loadSnapshot({ kind: "dir", path: older }, "game-weapons.json")?.build).toBe("1.0.1");
    expect(loadSnapshot({ kind: "dir", path: older }, "game-missing.json")).toBeUndefined();
  });

  it("skips meta keys and derived by* indexes", () => {
    const snap = loadSnapshot({ kind: "dir", path: older }, "game-weapons.json")!;
    expect(diffableCollections(snap.body)).toEqual(["shipWeapons"]);
  });

  it("diffs every collection of a dataset between two directories", () => {
    const diff = diffDataset("game-weapons.json", { kind: "dir", path: older }, { kind: "dir", path: newer }, {
      detail: true,
    });
    expect(diff.from.build).toBe("1.0.1");
    expect(diff.to.build).toBe("1.0.2");
    expect(diff.collections).toHaveLength(1);
    expect(diff.collections[0].changed[0].changes).toContainEqual({ field: "dps", from: 100, to: 110 });
    expect(renderDiff(diff)).toContain("dps: 100 -> 110");
  });

  it("says when a dataset only exists on one side", () => {
    const diff = diffDataset("game-new.json", { kind: "dir", path: older }, { kind: "dir", path: newer });
    expect(diff.missing).toBe("from");
    expect(renderDiff(diff)).toContain("not present in the older build");
  });

  it("summarises a dataset new in the newer build with per-collection counts", () => {
    fs.writeFileSync(
      path.join(newer, "game-tags.json"),
      envelope("1.0.2", { items: [{ id: "a" }, { id: "b" }], tags: { x: 1 }, byTag: { x: [0] } }),
    );
    const diff = diffDataset("game-tags.json", { kind: "dir", path: older }, { kind: "dir", path: newer });
    expect(diff.missing).toBe("from");
    expect(diff.counts).toEqual({ items: 2, tags: 1 });
    expect(diff.to.build).toBe("1.0.2");
    const text = renderDiff(diff);
    expect(text).toContain("new dataset (not present in the older build): 3 record(s)");
    expect(text).toContain("  items: 2");
    // Survives the --json path as a plain field.
    expect(JSON.parse(JSON.stringify(diff)).counts).toEqual({ items: 2, tags: 1 });

    const dropped = diffDataset("game-tags.json", { kind: "dir", path: newer }, { kind: "dir", path: older });
    expect(dropped).toMatchObject({ missing: "to", counts: { items: 2, tags: 1 } });
    expect(renderDiff(dropped)).toContain("dropped (not present in the newer build)");

    const neither = diffDataset("game-nope.json", { kind: "dir", path: older }, { kind: "dir", path: newer });
    expect(neither.missing).toBe("both");
    expect(neither.counts).toBeUndefined();
    expect(renderDiff(neither)).toContain("not present in either build");
  });

  it("rejects an unknown collection with the available ones", () => {
    expect(() =>
      diffDataset("game-weapons.json", { kind: "dir", path: older }, { kind: "dir", path: newer }, {
        collection: "nope",
      }),
    ).toThrow(/Available: shipWeapons/);
  });

  it("resolves 'local' and directories without touching git", () => {
    const repo = { workDir: path.join(tmp, "no-such-checkout"), remote: "invalid" };
    expect(resolveSource("local", { dataDir: older, repo })).toEqual({ kind: "dir", path: older });
    expect(resolveSource(newer, { dataDir: older, repo })).toEqual({ kind: "dir", path: newer });
  });
});

describe("snapshots", () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "sc-snap-"));
  const data = path.join(root, "data");
  const store = path.join(root, "snapshots");
  fs.mkdirSync(data);
  fs.writeFileSync(path.join(data, "game-weapons.json"), JSON.stringify({ _build: "4.10.196.1", shipWeapons: [weapon("W.A", "Alpha", 100)] }));
  // Stale dataset from another build: must not leak into this build's snapshot.
  fs.writeFileSync(path.join(data, "game-strings.json"), JSON.stringify({ _build: "1.0.0", strings: {} }));

  it("saves only datasets stamped with the build, gzipped", () => {
    const saved = saveSnapshot(data, store, "4.10.196.1");
    expect(saved.datasets).toBe(1);
    expect(fs.existsSync(path.join(store, "4.10.196.1", "game-weapons.json.gz"))).toBe(true);
    expect(fs.existsSync(path.join(store, "4.10.196.1", "game-strings.json.gz"))).toBe(false);
  });

  it("lists builds in version order and resolves them by number or prefix", () => {
    fs.mkdirSync(path.join(data, "..", "data9"));
    fs.writeFileSync(path.join(root, "data9", "game-weapons.json"), JSON.stringify({ _build: "4.9.2.1", shipWeapons: [] }));
    saveSnapshot(path.join(root, "data9"), store, "4.9.2.1");
    expect(listSnapshots(store).map((s) => s.build)).toEqual(["4.10.196.1", "4.9.2.1"]);
    const repo = { workDir: path.join(root, "no-checkout"), remote: "invalid" };
    expect(resolveSource("4.10.196.1", { dataDir: data, repo, snapshots: store })).toEqual({
      kind: "dir",
      path: path.join(store, "4.10.196.1"),
    });
    expect(resolveSource("4.9", { dataDir: data, repo, snapshots: store })).toEqual({
      kind: "dir",
      path: path.join(store, "4.9.2.1"),
    });
  });

  it("reads a gzipped snapshot like a plain directory", () => {
    const source = { kind: "dir" as const, path: path.join(store, "4.10.196.1") };
    expect(loadSnapshot(source, "game-weapons.json")?.build).toBe("4.10.196.1");
    expect(listDatasetFiles(source, source)).toEqual(["game-weapons.json"]);
  });

  it("snapshots a dataset written after parse under its own build stamp", () => {
    const later = path.join(root, "later");
    fs.mkdirSync(later);
    const file = path.join(later, "game-strings-english.json");
    fs.writeFileSync(file, JSON.stringify({ _build: "4.10.196.1", strings: { "@a": "one" } }));
    const kept = snapshotDataset(file, store);
    expect(kept?.build).toBe("4.10.196.1");
    // Joins the existing snapshot instead of replacing it.
    expect(fs.readdirSync(path.join(store, "4.10.196.1")).filter((f) => f.endsWith(".gz")).sort()).toEqual([
      "game-strings-english.json.gz",
      "game-weapons.json.gz",
    ]);
    const source = { kind: "dir" as const, path: path.join(store, "4.10.196.1") };
    expect(loadSnapshot(source, "game-strings-english.json")?.body.strings).toEqual({ "@a": "one" });

    const unstamped = path.join(later, "game-plain.json");
    fs.writeFileSync(unstamped, JSON.stringify({ strings: {} }));
    expect(snapshotDataset(unstamped, store)).toBeUndefined();
  });

  it("orders versions numerically", () => {
    expect(compareVersions("4.10.0", "4.9.9")).toBeGreaterThan(0);
    expect(compareVersions("4.10.196.2", "4.10.196.10")).toBeLessThan(0);
  });
});

describe("published fallback", () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "sc-fallback-"));
  const store = path.join(root, "snapshots");
  const data = path.join(root, "data");
  const dumps = path.join(root, "dumps");
  fs.mkdirSync(data);
  fs.writeFileSync(path.join(data, "game-weapons.json"), JSON.stringify({ _build: "4.10.197.1", shipWeapons: [] }));
  saveSnapshot(data, store, "4.10.197.1");

  // A dumps checkout with one tag for the same build, carrying a dataset the
  // snapshot lacks (published strings are a redacted subset; any shape works).
  fs.mkdirSync(path.join(dumps, "data"), { recursive: true });
  fs.writeFileSync(
    path.join(dumps, "data", "game-strings-english.json"),
    JSON.stringify({ _build: "4.10.197.1", _redacted: "name keys only", strings: { vehicle_NameA: "one", vehicle_NameB: "two" } }),
  );
  const run = (args: string[]) =>
    execFileSync("git", ["-c", "user.name=test", "-c", "user.email=test@example.invalid", ...args], {
      cwd: dumps,
      stdio: "ignore",
      // Ignore the machine's git config (signing, hooks, default branch).
      env: { ...process.env, GIT_CONFIG_GLOBAL: os.devNull, GIT_CONFIG_NOSYSTEM: "1" },
    });
  run(["init", "--quiet"]);
  run(["add", "."]);
  run(["commit", "--quiet", "-m", "dump"]);
  run(["tag", "4.10.197.1"]);
  const repo = { workDir: dumps, remote: "invalid" };

  it("attaches the published tag of the same build to a snapshot source", () => {
    expect(resolveSource("4.10.197.1", { dataDir: data, repo, snapshots: store })).toEqual({
      kind: "dir",
      path: path.join(store, "4.10.197.1"),
      fallback: { kind: "tag", tag: "4.10.197.1", repoDir: dumps },
    });
  });

  it("serves a dataset the snapshot lacks from the tag, and diffs it", () => {
    const from = resolveSource("4.10.197.1", { dataDir: data, repo, snapshots: store });
    const newer = path.join(root, "newer");
    fs.mkdirSync(newer);
    fs.writeFileSync(
      path.join(newer, "game-strings-english.json"),
      // A full local table: name keys plus prose that publish would strip.
      JSON.stringify({
        _build: "4.10.200.1",
        strings: { vehicle_NameA: "uno", vehicle_NameB: "two", vehicle_NameC: "three", vehicle_DescA: "A long story.", Dlg_Name_1: "Hi" },
      }),
    );
    const to = { kind: "dir" as const, path: newer };
    expect(listDatasetFiles(from, to)).toEqual(["game-strings-english.json", "game-weapons.json"]);

    const diff = diffDataset("game-strings-english.json", from, to);
    expect(diff.missing).toBeUndefined();
    expect(diff.from).toEqual({ source: "tag 4.10.197.1", build: "4.10.197.1" });
    // Compared on published keys only: the description and dialogue keys are not "added".
    expect(diff.collections[0].summary).toEqual({ added: 1, removed: 0, changed: 1, unchanged: 1 });
    expect(diff.notes).toEqual([
      "older side read from tag 4.10.197.1 (the snapshot lacks this dataset)",
      "older string table is the redacted published one; the newer table is compared on published name keys only",
    ]);
    expect(renderDiff(diff)).toContain("note: older side read from tag 4.10.197.1");
    // Snapshot copies still win over the tag.
    expect(loadSnapshot(from, "game-weapons.json")?.source).toBe(path.join(store, "4.10.197.1"));
  });

  it("adds no fallback when the checkout has no matching tag", () => {
    const other = path.join(root, "data2");
    fs.mkdirSync(other);
    fs.writeFileSync(path.join(other, "game-weapons.json"), JSON.stringify({ _build: "4.10.198.1", shipWeapons: [] }));
    saveSnapshot(other, store, "4.10.198.1");
    expect(resolveSource("4.10.198.1", { dataDir: data, repo, snapshots: store })).toStrictEqual({
      kind: "dir",
      path: path.join(store, "4.10.198.1"),
    });
  });
});

describe("derived files", () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "sc-derived-"));
  const data = path.join(root, "data");
  fs.mkdirSync(data);
  fs.writeFileSync(path.join(data, "game-weapons.json"), JSON.stringify({ _build: "4.10.200.1", shipWeapons: [] }));
  fs.writeFileSync(path.join(data, SEARCH_INDEX_FILE), JSON.stringify({ _build: "4.10.200.1", format: 1, items: [], ships: [] }));

  it("leaves the search index out of snapshots and dataset lists", () => {
    expect(DERIVED_FILES.has(SEARCH_INDEX_FILE)).toBe(true);
    const source = { kind: "dir" as const, path: data };
    expect(listDatasetFiles(source, source)).toEqual(["game-weapons.json"]);
    const saved = saveSnapshot(data, path.join(root, "snapshots"), "4.10.200.1");
    expect(saved.datasets).toBe(1);
    expect(fs.readdirSync(saved.path).sort()).toEqual(["_snapshot.json", "game-weapons.json.gz"]);
  });

  it("still diffs the index when asked for by name", () => {
    const source = { kind: "dir" as const, path: data };
    expect(diffDataset(SEARCH_INDEX_FILE, source, source).missing).toBeUndefined();
  });
});

describe("string redaction", () => {
  it("cuts a full table to published keys when only the other side is redacted", () => {
    const redacted = { _redacted: "x", strings: { ship_Name: "Gladius" } };
    const full = { strings: { ship_Name: "Gladius", ship_Desc: "Prose.", ship_NameLong: "x".repeat(121), PU_Title: "Dialogue" } };
    const aligned = alignRedaction(redacted, full);
    expect(aligned.after.strings).toEqual({ ship_Name: "Gladius" });
    expect(aligned.before).toBe(redacted);
    expect(aligned.note).toMatch(/older string table is the redacted/);
    expect(alignRedaction(full, full)).toEqual({ before: full, after: full });
    expect(alignRedaction(redacted, redacted).note).toBeUndefined();
  });

  it("matches the rule scripts/redact.mjs publishes with", () => {
    const script = fs.readFileSync(path.join(import.meta.dirname, "..", "scripts", "redact.mjs"), "utf-8");
    const source = fs.readFileSync(path.join(import.meta.dirname, "diff.ts"), "utf-8");
    const grab = (text: string, name: string) => new RegExp(`const ${name} = (.+);`).exec(text)?.[1];
    expect(grab(source, "PUBLISHED_KEY")).toBe(grab(script, "KEEP_KEY"));
    expect(grab(source, "UNPUBLISHED_KEY")).toBe(grab(script, "DROP_KEY"));
    expect(grab(source, "MAX_PUBLISHED_VALUE")).toBe(grab(script, "MAX_VALUE_LENGTH"));
    expect(grab(source, "PUBLISHED_KEY")).toBeDefined();
    for (const [key, value, kept] of [
      ["vehicle_Name", "Gladius", true],
      ["vehicle_Desc", "Prose", false],
      ["PU_Mission_Title", "Hello", false],
      ["item_name_x", "lower-case name", false],
    ] as const) {
      expect(isPublishedString(key, value)).toBe(kept);
    }
  });
});

describe("prose redaction alignment", () => {
  const full = {
    _build: "b",
    missions: [
      { id: "1", name: "Short", recordName: "MissionBrokerEntry.short", title: "Short", description: "prose" },
      { id: "2", name: "x".repeat(130), recordName: "MissionBrokerEntry.long_one", title: "x".repeat(130) },
    ],
  };
  const published = {
    _build: "b",
    _redacted: ["description", "title over 120 characters"],
    missions: [
      { id: "1", name: "Short", recordName: "MissionBrokerEntry.short", title: "Short" },
      { id: "2", name: "long_one", recordName: "MissionBrokerEntry.long_one" },
    ],
  };

  it("cuts the full side to what publish keeps, either way round", () => {
    const newer = alignRedaction(full, published);
    expect(newer.before.missions).toEqual(published.missions);
    expect(newer.note).toMatch(/newer side is the published dataset without its prose/);
    const older = alignRedaction(published, full);
    expect(older.after.missions).toEqual(published.missions);
  });

  it("leaves two published or two full datasets alone", () => {
    expect(alignRedaction(published, published).note).toBeUndefined();
    expect(alignRedaction(full, full).note).toBeUndefined();
  });
});
