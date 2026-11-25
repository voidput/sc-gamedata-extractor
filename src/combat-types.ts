/**
 * The parts of the `game-weapons.json` and `game-ships.json` rows that
 * deflection.ts and engagement.ts read. The datasets are written by the Rust
 * binary (crates/sc-gamedata); these types follow its output and list only the
 * fields the combat maths needs. Rows carry more fields than this.
 */

/** The damage types the game models, per projectile. */
export interface DamageInfo {
  physical?: number;
  energy?: number;
  distortion?: number;
  thermal?: number;
  biochemical?: number;
  stun?: number;
}

/**
 * Damage falloff with distance, per type. Damage holds until `minDistance`,
 * then drops by `perMeter` until it reaches `floor`. Only personal-weapon
 * ammo carries this; ship ammo does full damage to the end of its range.
 */
export type Falloff = Record<string, { minDistance: number; perMeter: number; floor: number }>;

export interface Spread {
  min?: number;
  max?: number;
  attack?: number;
  decay?: number;
}

/** One selectable fire mode. */
export interface FireMode {
  name: string;
  /** Rounds per minute the mode actually cycles at. */
  fireRate?: number;
  pellets?: number;
  damageMultiplier?: number;
  spread?: Spread;
  alpha?: number;
  dps?: number;
}

export interface WeaponAmmo {
  recordName: string;
  speed?: number;
  lifetime?: number;
  range?: number;
  /** Per projectile. A scattergun's alpha is this times its pellet count. */
  damage?: DamageInfo;
  falloff?: Falloff;
}

/** A `game-weapons.json` row. */
export interface Weapon {
  name: string;
  recordName: string;
  id: string;
  class: "ship" | "fps";
  isTemplate?: true;
  type?: string;
  size?: number;
  dps?: number;
  alpha?: number;
  fireRate?: number;
  range?: number;
  speed?: number;
  spread?: number;
  ammo?: WeaponAmmo;
  modes: FireMode[];
}

/** A ship's armour, from its `game-ships.json` row. */
export interface Armor {
  item: string;
  health?: number;
  /** Per-hit damage at or below which a hit of that type is deflected. */
  deflection?: Record<string, number>;
  /** Fraction of hull damage that gets through, by type. */
  damageMultiplier?: Record<string, number>;
  /** Signature multipliers the armour applies. */
  signals?: Record<string, number>;
}
