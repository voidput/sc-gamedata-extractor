import { describe, it, expect } from "vitest";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import {
  compareVersions,
  datasetFile,
  diffableCollections,
  diffCollection,
  diffDataset,
  diffEntry,
  flatten,
  keyOf,
  listDatasetFiles,
  listSnapshots,
  loadSnapshot,
  renderDiff,
  resolveSource,
  saveSnapshot,
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

  it("orders versions numerically", () => {
    expect(compareVersions("4.10.0", "4.9.9")).toBeGreaterThan(0);
    expect(compareVersions("4.10.196.2", "4.10.196.10")).toBeLessThan(0);
  });
});
