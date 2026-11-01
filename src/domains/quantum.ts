/**
 * Quantum travel: the global rules every quantum drive obeys.
 *
 * Drive speed, spool and fuel use are per item and live in
 * game-ship-components. QuantumDriveGlobalParams holds what applies to all of
 * them: where a jump may start, how far a target can be locked, and the spline
 * the trip follows. Notification, music and FX settings are left out. Field
 * names follow the game's own where their meaning is not documented.
 */

import { at, num, type GameRecord } from "../records.js";
import type { Domain } from "./types.js";

export interface QuantumRules {
  /** Metres above a surface before a drive will engage. */
  minimumAltitudeForQuantum?: number;
  /** Atmospheric pressure above which a drive will not engage. */
  maximumAtmosphericPressureForQuantum?: number;
  /** Metres within which ships can link for a party jump. */
  maxLinkingRange?: number;
  maxRaycastDist?: number;
  maxIconScaleRange?: number;
  inputDelay?: number;
  scatterScale?: number;
  arrivalRadiusScalar?: number;
  rollUnderFullRotationDistance?: number;
  rollbackFullRotationDistance?: number;
}

export function parseQuantumRules(record: GameRecord): QuantumRules {
  const v = record.value;
  const spline = (v.splineTraversalParams ?? {}) as Record<string, unknown>;
  return {
    minimumAltitudeForQuantum: num(v.minimumAltitudeForQuantum),
    maximumAtmosphericPressureForQuantum: num(v.maximumAtmosphericPressureForQuantum),
    maxLinkingRange: num(v.maxLinkingRange),
    maxRaycastDist: num(v.maxRaycastDist),
    maxIconScaleRange: num(v.maxIconScaleRange),
    inputDelay: num(v.inputDelay),
    scatterScale: num(v.scatterScale),
    arrivalRadiusScalar: num(spline.arrivalRadiusScalar),
    rollUnderFullRotationDistance: num(spline.rollUnderFullRotationDistance),
    rollbackFullRotationDistance: num(at(spline, "rollbackParams.fullRotationDistance")),
  };
}

export const quantumDomain: Domain = {
  name: "game-quantum",
  description: "Global quantum travel rules: minimum altitude, atmosphere limit, link range.",
  filters: ["**/globalquantumdriveparams/**"],

  parse({ records }) {
    const record = records.find((r) => r.type === "QuantumDriveGlobalParams");
    return {
      rules: record ? parseQuantumRules(record) : undefined,
      summary: { records: record ? 1 : 0 },
    };
  },
};
