/**
 * Salvage: the global numbers behind hull scraping, structural salvage and the
 * multitool repair beam.
 *
 * Per-tool stats (scraper heads, modifiers) live on the items and are in
 * game-ship-components. These two records hold the shared constants; most of
 * their bulk is highlight colours and VFX tags, which are left out.
 */

import { at, num, refShortName, type GameRecord } from "../records.js";
import type { Domain } from "./types.js";

export interface StructuralSalvage {
  /** Scale on the force that fractures a hull into salvageable pieces. */
  vehicleFractureForceScale?: number;
  sourceIdleFieldStrength?: number;
  sourceMinEngagedFieldStrength?: number;
  /** Seconds for a disintegrated piece to dissolve. */
  disintegrateDissolveTime?: number;
}

export interface RepairBeam {
  hitsPerSecond?: number;
  hitDuration?: number;
  /** Hull thickness in metres that repair material fills. */
  hullThicknessMeters?: number;
  /** Repair material units per unit of beam ammo. */
  ammoToMaterialFactor?: number;
  materialResource?: string;
}

export function parseStructuralSalvage(record: GameRecord): StructuralSalvage {
  const v = record.value;
  return {
    vehicleFractureForceScale: num(v.vehicleFractureForceScale),
    sourceIdleFieldStrength: num(at(v, "structuralVFX.sourceIdleFieldStrengthValue")),
    sourceMinEngagedFieldStrength: num(at(v, "structuralVFX.sourceMinEngagedFieldStrengthValue")),
    disintegrateDissolveTime: num(at(v, "structuralVFX.targetDisintegrateDissolveTime")),
  };
}

export function parseRepairBeam(record: GameRecord): RepairBeam {
  const v = record.value;
  return {
    hitsPerSecond: num(v.hitsPerSecond),
    hitDuration: num(v.hitDuration),
    hullThicknessMeters: num(at(v, "materialParams.hullThicknessMeters")),
    ammoToMaterialFactor: num(at(v, "materialParams.ammoToMaterialFactor")),
    materialResource: refShortName(at(v, "materialParams.RMCResourceType")),
  };
}

export const salvageDomain: Domain = {
  name: "game-salvage",
  description: "Global salvage constants: structural fracture and the repair beam.",
  filters: ["**/vehiclesalvageglobalparams/**", "**/sglobalsalvagerepairbeamparams/**"],

  parse({ records }) {
    const structural = records.find((r) => r.type === "VehicleSalvageGlobalParams");
    const beam = records.find((r) => r.type === "SGlobalSalvageRepairBeamParams");
    return {
      structural: structural ? parseStructuralSalvage(structural) : undefined,
      repairBeam: beam ? parseRepairBeam(beam) : undefined,
      summary: { records: [structural, beam].filter(Boolean).length },
    };
  },
};
