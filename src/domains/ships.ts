/**
 * Ships and ground vehicles.
 *
 * A vehicle is an EntityClassDefinition carrying `VehicleComponentParams`, which
 * holds the identity fields: name, career, role, crew size, manufacturer and
 * bounding box.
 *
 * Career and role are localization keys (`@vehicle_focus_combat`,
 * `@vehicle_class_lightfighter`), so the string table is required.
 *
 * Note what is *not* here. `SHealthComponentParams.Health` on the hull reads 1.0
 * because hull health is normalised and the real figure is distributed across
 * damageable parts. Anything presenting a single hull HP number has computed it
 * from part geometry rather than read it, so this domain does not invent one.
 */

import { component, referencedName, vec } from "../entity.js";
import { isPlaceholderName, localize, type Localization } from "../localization.js";
import { num, str, type GameRecord } from "../records.js";
import type { Domain } from "./types.js";

export interface Vehicle {
  name: string;
  recordName: string;
  id: string;
  displayName?: string;
  description?: string;
  manufacturer?: string;
  /** "Spaceship", "Gravlev", "Ground" and similar. */
  movementClass?: string;
  career?: string;
  role?: string;
  crewSize?: number;
  isGravlev?: boolean;
  /** Metres, as x/y/z. */
  boundingBox?: Record<string, number>;
  isTemplate?: true;
}

/** Career and role keys read "@vehicle_focus_combat"; keep the meaningful tail. */
function tidyClassifier(value: string | undefined): string | undefined {
  if (!value) return undefined;
  if (!value.startsWith("@")) return value;
  const tail = value.replace(/^@vehicle_(focus|class)_/i, "").replace(/^@/, "");
  return tail.length > 0 ? tail : undefined;
}

export function parseVehicle(
  record: GameRecord,
  localization: Localization,
): Vehicle | undefined {
  const params = component(record, "VehicleComponentParams");
  if (!params) return undefined;

  const rawName = str(params.vehicleName);
  const displayName = localize(localization, rawName);
  const description = localize(localization, params.vehicleDescription);

  // Career and role resolve through the string table when present, and fall
  // back to the key's own tail when the table has no entry for them.
  const career = localize(localization, params.vehicleCareer);
  const role = localize(localization, params.vehicleRole);

  return {
    name: isPlaceholderName(displayName) ? record.shortName : (displayName as string),
    recordName: record.name,
    id: record.id,
    displayName: isPlaceholderName(displayName) ? undefined : displayName,
    description: isPlaceholderName(description) ? undefined : description,
    manufacturer: referencedName(params.manufacturer),
    movementClass: str(params.movementClass),
    career: isPlaceholderName(career) ? tidyClassifier(str(params.vehicleCareer)) : career,
    role: isPlaceholderName(role) ? tidyClassifier(str(params.vehicleRole)) : role,
    crewSize: num(params.crewSize),
    isGravlev: params.isGravlevVehicle === true ? true : undefined,
    boundingBox: vec(params.maxBoundingBoxSize),
    isTemplate: isPlaceholderName(displayName) || undefined,
  };
}

export const shipsDomain: Domain = {
  name: "game-ships",
  description: "Ships and ground vehicles with manufacturer, role, crew and dimensions.",
  filters: ["**/entities/spaceships/**", "**/entities/groundvehicles/**"],
  needsLocalization: true,

  parse({ records, localization }) {
    const vehicles = records
      .map((r) => parseVehicle(r, localization))
      .filter((v): v is Vehicle => v !== undefined)
      .sort((a, b) => a.name.localeCompare(b.name));

    const named = vehicles.filter((v) => !v.isTemplate);

    const byManufacturer: Record<string, number> = {};
    const byRole: Record<string, number> = {};
    for (const vehicle of named) {
      if (vehicle.manufacturer) {
        byManufacturer[vehicle.manufacturer] = (byManufacturer[vehicle.manufacturer] ?? 0) + 1;
      }
      if (vehicle.role) byRole[vehicle.role] = (byRole[vehicle.role] ?? 0) + 1;
    }

    return {
      vehicles,
      byManufacturer: Object.fromEntries(
        Object.entries(byManufacturer).sort((a, b) => b[1] - a[1]),
      ),
      byRole: Object.fromEntries(Object.entries(byRole).sort((a, b) => b[1] - a[1])),
      summary: {
        vehicles: vehicles.length,
        named: named.length,
        templates: vehicles.length - named.length,
      },
    };
  },
};
