import { describe, it, expect } from "vitest";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { compareCategory, directionOf, patchChanges, toCsv, type CategorySpec } from "./changes.js";

const spec: CategorySpec = {
  category: "shipWeapons",
  dataset: "weapons",
  collection: "shipWeapons",
  stats: [
    { stat: "dps", path: "dps", higherIsBetter: true },
    { stat: "em", path: "em", higherIsBetter: false },
    { stat: "burstDamage", path: "burst.damage", higherIsBetter: true },
    { stat: "crew", path: "crew" },
  ],
  include: (i) => i.isTemplate !== true,
};

const gun = (record: string, name: string, extra: Record<string, unknown>) => ({
  recordName: `EntityClassDefinition.${record}`,
  name,
  size: 5,
  ...extra,
});

describe("directionOf", () => {
  it("reads buff or nerf off which side of the stat is better", () => {
    expect(directionOf(spec.stats[0], 100, 120)).toBe("buff");
    expect(directionOf(spec.stats[0], 120, 100)).toBe("nerf");
    expect(directionOf(spec.stats[1], 900, 1100)).toBe("nerf");
    expect(directionOf(spec.stats[3], 1, 2)).toBe("changed");
    expect(directionOf(spec.stats[0], undefined, 2)).toBe("changed");
  });
});

describe("compareCategory", () => {
  const before = [
    gun("BEHR_LaserCannon_S5", "M7A Cannon", { dps: 1536.3, em: 941, burst: { damage: 23044.5 } }),
    gun("KLWE_LaserRepeater_S3", "CF-337 Panther Repeater", { dps: 545.63 }),
    gun("GATS_BallisticGatling_S3", "Mantis GT-220 Gatling", { dps: 853.33 }),
    gun("Unchanged", "Steady", { dps: 10 }),
  ];
  const after = [
    gun("BEHR_LaserCannon_S5", "M7A Cannon", { dps: 1400, em: 1100, burst: { damage: 23044.5 } }),
    gun("KLWE_LaserRepeater_S3", "CF-337 Panther Mk II", { dps: 545.63 }),
    gun("Unchanged", "Steady", { dps: 10 }),
    gun("New_Gun", "Fresh", { dps: 1 }),
    gun("Template_Gun", "Template", { dps: 1, isTemplate: true }),
  ];
  const { items, stats } = compareCategory(spec, before, after);

  it("emits one row per moved stat with delta, percent and direction", () => {
    expect(stats).toEqual([
      expect.objectContaining({ record: "BEHR_LaserCannon_S5", stat: "dps", before: 1536.3, after: 1400, delta: -136.3, percent: -8.87, direction: "nerf" }),
      expect.objectContaining({ record: "BEHR_LaserCannon_S5", stat: "em", delta: 159, percent: 16.9, direction: "nerf" }),
    ]);
  });

  it("matches by record, so a rename is a change carrying the old name", () => {
    expect(items).toContainEqual(
      expect.objectContaining({ record: "KLWE_LaserRepeater_S3", change: "changed", name: "CF-337 Panther Mk II", previousName: "CF-337 Panther Repeater", statsChanged: 0 }),
    );
  });

  it("lists adds and removes, skips unchanged items and filtered ones", () => {
    const by = (record: string) => items.find((i) => i.record === record)?.change;
    expect(by("New_Gun")).toBe("added");
    expect(by("GATS_BallisticGatling_S3")).toBe("removed");
    expect(by("Unchanged")).toBeUndefined();
    expect(by("Template_Gun")).toBeUndefined();
    expect(items.find((i) => i.record === "BEHR_LaserCannon_S5")).toMatchObject({ statsChanged: 2, buffs: 0, nerfs: 2 });
  });
});

describe("patchChanges", () => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "sc-changes-"));
  const write = (dir: string, build: string, dps: number) => {
    fs.mkdirSync(dir, { recursive: true });
    fs.writeFileSync(
      path.join(dir, "game-weapons.json"),
      JSON.stringify({ _build: build, shipWeapons: [gun("A", "Alpha", { dps })], fpsWeapons: [], missiles: [] }),
    );
  };
  write(path.join(tmp, "old"), "1.0.1", 100);
  write(path.join(tmp, "new"), "1.0.2", 125);

  it("reads both builds, stamps them, and skips datasets one side lacks", () => {
    const result = patchChanges({ kind: "dir", path: path.join(tmp, "old") }, { kind: "dir", path: path.join(tmp, "new") });
    expect(result.from).toBe("1.0.1");
    expect(result.to).toBe("1.0.2");
    expect(result.stats).toEqual([expect.objectContaining({ stat: "dps", percent: 25, direction: "buff" })]);
    expect(result.skipped).toEqual(expect.arrayContaining(["ships", "ship-components"]));
  });
});

describe("toCsv", () => {
  it("quotes cells that need it and leaves blanks empty", () => {
    expect(toCsv([{ a: 'say "hi", ok', b: undefined }], ["a", "b"])).toBe('a,b\n"say ""hi"", ok",\n');
  });
});
