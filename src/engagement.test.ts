import { describe, it, expect } from "vitest";
import {
  G,
  appliedDpsAt,
  damageAt,
  envelope,
  falloffRange,
  hitChance,
  hitRadiusOf,
  missRadius,
  rangeForChance,
  rangeTable,
  targetOf,
} from "./engagement.js";
import type { Armor } from "./domains/ships.js";
import type { Weapon } from "./domains/weapons.js";

const hammerhead: Armor = {
  item: "ARMR_AEGS_Hammerhead",
  health: 25740,
  deflection: { physical: 531, energy: 380 },
  damageMultiplier: { physical: 0.7, energy: 0.5 },
};

const gun = (name: string, damage: Record<string, number>, fireRate: number, speed: number, range: number, extra: Partial<Weapon> = {}): Weapon => ({
  name,
  recordName: `EntityClassDefinition.${name}`,
  id: name,
  class: "ship",
  size: 5,
  fireRate,
  speed,
  range,
  dps: Object.values(damage).reduce((a, b) => a + b, 0) * (fireRate / 60),
  ammo: { recordName: `AmmoParams.${name}`, damage },
  modes: [],
  ...extra,
});

// M7A and AD5B as the files have them.
const m7a = gun("BEHR_LaserCannon_S5", { energy: 921.78 }, 100, 1112, 4003, { spread: 0.4 });
const ad5b = gun("BEHR_BallisticGatling_S5", { physical: 102.5 }, 900, 1300, 4003);

describe("falloff", () => {
  // P4-AR style ammo: 18 physical, drops 0.1/m after 14 m, floor 15.
  const damage = { physical: 18 };
  const falloff = { physical: { minDistance: 14, perMeter: 0.1, floor: 15 } };

  it("holds damage to the start distance, then drops to the floor", () => {
    expect(damageAt(damage, falloff, 10)).toEqual({ physical: 18 });
    expect(damageAt(damage, falloff, 24)).toEqual({ physical: 17 });
    expect(damageAt(damage, falloff, 500)).toEqual({ physical: 15 });
  });

  it("reports where damage bottoms out", () => {
    expect(falloffRange(damage, falloff)).toBe(44);
    expect(falloffRange(damage, undefined)).toBeUndefined();
  });
});

describe("appliedDpsAt", () => {
  it("applies deflection, multipliers and max range", () => {
    expect(appliedDpsAt(m7a, { name: "Hammerhead", armor: hammerhead }, 0)).toBeCloseTo(921.78 * 0.5 * (100 / 60));
    expect(appliedDpsAt(ad5b, { name: "Hammerhead", armor: hammerhead }, 0)).toBe(0);
    expect(appliedDpsAt(m7a, { name: "Hammerhead", armor: hammerhead }, 4004)).toBe(0);
  });

  it("is listed DPS with no target armour", () => {
    expect(appliedDpsAt(m7a, undefined, 1000)).toBeCloseTo(m7a.dps!);
  });
});

describe("hit chance", () => {
  // A steady target 6 m in radius; M7A spread 0.4 degrees.
  const steady = { name: "x", hitRadius: 6, speed: 200 };

  it("is certain while the miss disc fits inside the target", () => {
    expect(hitChance(m7a, steady, 100)).toBe(1);
  });

  it("falls with the square of the miss radius past that", () => {
    const scatter = 2000 * Math.tan((0.4 * Math.PI) / 180);
    expect(missRadius(m7a, steady, 2000)).toBeCloseTo(scatter);
    expect(hitChance(m7a, steady, 2000)).toBeCloseTo((6 / scatter) ** 2);
  });

  it("widens the disc by how far a jinking target slides during flight", () => {
    const t = 1000 / 1112;
    const slide = 0.5 * 6 * G * t * t;
    expect(missRadius(m7a, steady, 1000, { accel: 6 * G })).toBeCloseTo(slide + 1000 * Math.tan((0.4 * Math.PI) / 180));
    // Never more than top speed allows.
    expect(missRadius(m7a, { ...steady, speed: 10 }, 1000, { accel: 50 * G })).toBeCloseTo(10 * t + 1000 * Math.tan((0.4 * Math.PI) / 180));
  });

  it("uses closing speed in the flight time and is zero past max range", () => {
    expect(missRadius(m7a, steady, 1000, { accel: 6 * G, closing: 200 })).toBeLessThan(missRadius(m7a, steady, 1000, { accel: 6 * G }));
    expect(hitChance(m7a, steady, 4004)).toBe(0);
  });

  it("finds the farthest distance for a hit-chance level", () => {
    const d = rangeForChance(m7a, steady, 0.5);
    expect(hitChance(m7a, steady, d)).toBeGreaterThanOrEqual(0.5);
    expect(hitChance(m7a, steady, d + 5)).toBeLessThan(0.5);
  });
});

describe("envelope", () => {
  const target = { name: "Hammerhead", armor: hammerhead, hitRadius: 10, speed: 150 };
  const env = envelope(
    [
      { weapon: m7a, count: 2 },
      { weapon: ad5b, count: 2 },
      { weapon: gun("Short", { energy: 500 }, 60, 1000, 1500), count: 1 },
    ],
    target,
    { bands: [0, 1000, 2000] },
  );

  it("sums hull DPS across guns and drops each gun past its range", () => {
    const at = (d: number) => env.bands.find((b) => b.distance === d)!;
    // Short gun: 500 energy clears 380, times 0.5, once a second.
    expect(at(0).appliedDps).toBeCloseTo(1536.3 + 250, 0);
    expect(at(2000).appliedDps).toBeCloseTo(1536.3, 0);
    expect(at(2000).gunsInRange).toBe(4);
    expect(env.guns.find((g) => g.record === "BEHR_BallisticGatling_S5")?.penetrates).toBe(false);
  });

  it("reports the ranges a reader acts on", () => {
    expect(env.allGunsRange).toBe(1500);
    expect(env.maxRange).toBe(4003);
    expect(env.halfDpsRange).toBe(4003);
    expect(env.effectiveRange?.["50"]).toBeGreaterThan(0);
  });

  it("weights hull DPS by hit chance and shrinks effective range as the target jinks harder", () => {
    const steady = envelope([{ weapon: m7a, count: 1 }], target, { bands: [0, 2000] });
    const far = steady.bands.find((b) => b.distance === 2000)!;
    expect(far.expectedDps).toBeCloseTo(far.appliedDps * hitChance(m7a, target, 2000), 1);
    const table = rangeTable([{ weapon: m7a, count: 1 }], target, [0, 6]);
    expect(table[1].ranges["50"]).toBeLessThan(table[0].ranges["50"]);
  });
});

describe("targetOf", () => {
  it("uses the circle of the box's average projected area, and the SCM speed", () => {
    const t = targetOf({ name: "Gladius", boundingBox: { x: 17.5, y: 21, z: 5.5 }, flight: { scmSpeed: 226 } });
    expect(t.hitRadius).toBe(hitRadiusOf({ x: 17.5, y: 21, z: 5.5 }));
    expect(t.hitRadius).toBeCloseTo(Math.sqrt((17.5 * 5.5 + 21 * 5.5 + 17.5 * 21) / 3 / Math.PI), 2);
    expect(t.speed).toBe(226);
  });
});
