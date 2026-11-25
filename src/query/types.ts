/**
 * Shapes of the parsed datasets the query layer reads: game-item-tags and
 * game-item-sources. Shared by the domains that write them and by anything that
 * reads them, including SCMCP, which vendors src/query/ unchanged. Keep this
 * directory free of Node and repo imports so it copies anywhere.
 */

export interface ItemTags {
  name: string;
  /** Record name without its type prefix, e.g. "ksar_lmg_energy_01". */
  className: string;
  id: string;
  type?: string;
  subType?: string;
  manufacturer?: string;
  isTemplate?: true;
  /** String-table key of the name, as the table spells it (`,P` included). */
  nameKey?: string;
  descriptionKey?: string;
  /** Set when the game has no real name for the item (absent = real); `name` then falls back to the class name. */
  nameStatus?: "real" | "placeholder" | "unresolved" | "empty";
  /** Tag paths below Global, e.g. "Weapon/FPS/Stocked/LMG". */
  tags: string[];
  /** Weapons: class names of the magazines or canisters it loads, the ones it names first. */
  ammoItems?: string[];
}

export interface SpawnLoot {
  /** Loot table record name (v1 or V3). */
  table: string;
  version: "v1" | "v3";
  poolFilter?: string;
  /** Single-layer secondary choice records, in order. Multi-layer refs are expanded at query time. */
  secondaryChoices?: string;
  maxResults?: number;
  chance?: number;
}

export interface SpawnSlot {
  /** Entity class that spawns: a container, a rack, or the item itself. */
  entity: string;
  /** Display name of that entity, when it has one. */
  entityName?: string;
  /** Harvestable preset the slot names, when it goes through one. */
  preset?: string;
  weight: number;
  loot?: SpawnLoot;
  /** Items the entity's default loadout always holds (racks, displays). */
  contains?: string[];
}

export interface SpawnConfig {
  /** The config's own label, e.g. "Weapons Large Common". */
  name?: string;
  /** Tags a spawn point needs to use this config; empty for the default. */
  spawnTags: string[];
  /** Chance each slot is filled at all. */
  fillChance?: number;
  slots: SpawnSlot[];
}

export interface LootArea {
  /** Slot preset path below harvestable/slotpresets, e.g. "caves/loot_caves_occupied". */
  name: string;
  configs: SpawnConfig[];
}

export interface ContractAward {
  generator: string;
  contract: string;
  item: string;
  amount?: number;
  /** Weight of this award bundle among the contract's alternatives, when it has several. */
  weighting?: number;
}
