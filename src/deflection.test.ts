import { describe, it, expect } from "vitest";
import { checkWeapon, deflectionCheck } from "./deflection.js";
import type { Armor } from "./domains/ships.js";
import type { Weapon } from "./domains/weapons.js";

// Perseus stock armour, as the files have it.
const perseus: Armor = {
  item: "ARMR_RSI_Perseus",
  health: 48672,
  deflection: { physical: 495, energy: 418, distortion: 0 },
  damageMultiplier: { physical: 0.7, energy: 0.5, distortion: 1 },
};

const gun = (name: string, damage: Record<string, number>, fireRate: number, extra: Partial<Weapon> = {}): Weapon => ({
  name,
  recordName: `EntityClassDefinition.${name}`,
  id: name,
  class: "ship",
  size: 3,
  fireRate,
  dps: Object.values(damage).reduce((a, b) => a + b, 0) * (fireRate / 60),
  ammo: { recordName: `AmmoParams.${name}`, damage },
  modes: [],
  ...extra,
});

describe("checkWeapon", () => {
  it("deflects a hit at or below the threshold for its type", () => {
    // Omnisky VI alpha against Perseus energy deflection 418.
    const row = checkWeapon(gun("Omnisky_VI", { energy: 145.8 }, 150), perseus)!;
    expect(row.penetrates).toBe(false);
    expect(row.deflected).toEqual(["energy"]);
    expect(row.appliedPerHit).toBe(0);
  });

  it("scales what gets through by the hull multiplier", () => {
    const row = checkWeapon(gun("Sledge_III", { physical: 1125 }, 30), perseus)!;
    expect(row.penetrates).toBe(true);
    expect(row.appliedPerHit).toBe(787.5);
    expect(row.appliedDps).toBe(393.75);
  });

  it("checks each type on its own", () => {
    const row = checkWeapon(gun("Mixed", { physical: 600, energy: 100 }, 60), perseus)!;
    expect(row.deflected).toEqual(["energy"]);
    expect(row.appliedPerHit).toBe(420);
  });

  it("never counts distortion as hull damage", () => {
    const row = checkWeapon(gun("Suckerpunch", { distortion: 182.52 }, 100), perseus)!;
    expect(row.penetrates).toBe(false);
    expect(row.distortion).toBe(182.52);
  });

  it("is undefined for a weapon with no ammo damage", () => {
    expect(checkWeapon(gun("Tractor", {}, 0), perseus)).toBeUndefined();
  });
});

describe("deflectionCheck", () => {
  it("filters by size, skips templates and ranks by applied DPS", () => {
    const rows = deflectionCheck(
      perseus,
      [
        gun("Slow", { physical: 1125 }, 30),
        gun("Fast", { physical: 600 }, 120),
        gun("Small", { physical: 2000 }, 60, { size: 1 }),
        gun("Template", { physical: 2000 }, 60, { isTemplate: true }),
      ],
      { size: 3 },
    );
    expect(rows.map((r) => r.name)).toEqual(["Fast", "Slow"]);
  });
});
