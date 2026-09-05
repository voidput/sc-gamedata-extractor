/**
 * FPS weapons: guns and melee.
 *
 * Personal weapons use the same `SCItemWeaponComponentParams` struct as ship
 * weapons, so they are told apart by where the record lives rather than by its
 * components. Melee carries `SMeleeWeaponComponentParams` instead, which has no
 * ammo and so no projectile.
 *
 * As with ship weapons, damage is on the ammunition rather than the weapon.
 * See game-ammo, and join on the ammo reference.
 */

import { component } from "../entity.js";
import { isPlaceholderName, localize, type Localization } from "../localization.js";
import { at, num, str, type GameRecord } from "../records.js";
import type { Domain } from "./types.js";

export interface FpsWeapon {
  name: string;
  recordName: string;
  id: string;
  kind: "ranged" | "melee";
  type?: string;
  subType?: string;
  size?: number;
  /** Rounds the magazine holds, for ranged weapons. */
  ammoCapacity?: number;
  heatPerShot?: number;
  isTemplate?: true;
}

export function parseFpsWeapon(
  record: GameRecord,
  localization: Localization,
): FpsWeapon | undefined {
  const ranged = component(record, "SCItemWeaponComponentParams");
  const melee = component(record, "SMeleeWeaponComponentParams");
  const params = ranged ?? melee;
  if (!params) return undefined;

  const attach = component(record, "SAttachableComponentParams");
  const def = (attach?.AttachDef ?? {}) as Record<string, unknown>;
  const displayName = localize(localization, at(def, "Localization.Name"));

  return {
    name: isPlaceholderName(displayName) ? record.shortName : (displayName as string),
    recordName: record.name,
    id: record.id,
    kind: ranged ? "ranged" : "melee",
    type: str(def.Type),
    subType: str(def.SubType) === "UNDEFINED" ? undefined : str(def.SubType),
    size: num(def.Size),
    ammoCapacity: num(params.ammoContainerCapacity),
    heatPerShot: num(params.heatPerShot),
    isTemplate: isPlaceholderName(displayName) || undefined,
  };
}

export const fpsWeaponsDomain: Domain = {
  name: "game-fps-weapons",
  description: "Personal ranged and melee weapons.",
  filters: ["**/entities/scitem/weapons/**"],
  needsLocalization: true,

  parse({ records, localization }) {
    const weapons = records
      .map((r) => parseFpsWeapon(r, localization))
      .filter((w): w is FpsWeapon => w !== undefined)
      .sort((a, b) => a.name.localeCompare(b.name));

    return {
      weapons,
      summary: {
        weapons: weapons.length,
        ranged: weapons.filter((w) => w.kind === "ranged").length,
        melee: weapons.filter((w) => w.kind === "melee").length,
        templates: weapons.filter((w) => w.isTemplate).length,
      },
    };
  },
};
