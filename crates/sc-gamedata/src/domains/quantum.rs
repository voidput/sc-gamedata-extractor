//! Quantum travel: the global rules every quantum drive obeys (port of
//! `sc-gamedata/src/domains/quantum.ts`).
//!
//! Drive speed, spool and fuel use are per item and live in `game-ship-components`.
//! `QuantumDriveGlobalParams` holds what applies to all of them: where a jump may start,
//! how far a target can be locked, and the spline the trip follows. Notification, music
//! and FX settings are left out. Field names follow the game's own where their meaning is
//! not documented.

use crate::domain::{Domain, DomainError, ParseContext, field};
use crate::records::{GameRecord, at, num};
use serde::Serialize;
use serde_json::{Map, Value};

pub static DOMAIN: Quantum = Quantum;

pub struct Quantum;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuantumRules {
    /// Metres above a surface before a drive will engage.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minimum_altitude_for_quantum: Option<f64>,
    /// Atmospheric pressure above which a drive will not engage.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maximum_atmospheric_pressure_for_quantum: Option<f64>,
    /// Metres within which ships can link for a party jump.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_linking_range: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_raycast_dist: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_icon_scale_range: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_delay: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scatter_scale: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arrival_radius_scalar: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub roll_under_full_rotation_distance: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rollback_full_rotation_distance: Option<f64>,
}

pub fn parse_quantum_rules(record: &GameRecord) -> QuantumRules {
    let v = &record.value;
    let spline = v.get("splineTraversalParams");
    let s = |k: &str| num(spline.and_then(|s| at(s, k)));
    QuantumRules {
        minimum_altitude_for_quantum: num(v.get("minimumAltitudeForQuantum")),
        maximum_atmospheric_pressure_for_quantum: num(
            v.get("maximumAtmosphericPressureForQuantum"),
        ),
        max_linking_range: num(v.get("maxLinkingRange")),
        max_raycast_dist: num(v.get("maxRaycastDist")),
        max_icon_scale_range: num(v.get("maxIconScaleRange")),
        input_delay: num(v.get("inputDelay")),
        scatter_scale: num(v.get("scatterScale")),
        arrival_radius_scalar: s("arrivalRadiusScalar"),
        roll_under_full_rotation_distance: s("rollUnderFullRotationDistance"),
        rollback_full_rotation_distance: s("rollbackParams.fullRotationDistance"),
    }
}

#[derive(Debug, Serialize)]
struct Summary {
    records: usize,
}

impl Domain for Quantum {
    fn name(&self) -> &'static str {
        "game-quantum"
    }

    fn description(&self) -> &'static str {
        "Global quantum travel rules: minimum altitude, atmosphere limit, link range."
    }

    fn filters(&self) -> &'static [&'static str] {
        &["**/globalquantumdriveparams/**"]
    }

    fn parse(&self, ctx: &ParseContext<'_>) -> Result<Map<String, Value>, DomainError> {
        let record = ctx
            .records
            .iter()
            .find(|r| r.ty == "QuantumDriveGlobalParams");
        let mut body = Map::new();
        if let Some(r) = record {
            field(&mut body, "rules", &parse_quantum_rules(r))?;
        }
        field(
            &mut body,
            "summary",
            &Summary {
                records: usize::from(record.is_some()),
            },
        )?;
        Ok(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::NoFiles;
    use crate::localization::Localization;
    use crate::records::{RecordSet, normalize_record};
    use serde_json::json;

    fn parse(records: Vec<GameRecord>) -> Map<String, Value> {
        let set = RecordSet::new(records);
        let loc = Localization::new();
        DOMAIN.parse(&ParseContext::new(&set, &loc, &NoFiles)).unwrap()
    }

    #[test]
    fn reads_the_rules_and_the_spline() {
        let r = normalize_record(
            json!({"_RecordName_": "QuantumDriveGlobalParams.Q", "_RecordValue_": {
                "_Type_": "QuantumDriveGlobalParams",
                "minimumAltitudeForQuantum": 2000, "maxLinkingRange": 10000.0,
                "inputDelay": 0.5, "scatterScale": "x",
                "splineTraversalParams": {"arrivalRadiusScalar": 1.1,
                    "rollbackParams": {"fullRotationDistance": 2450000}}
            }}),
            "libs/foundry/records/globalquantumdriveparams/q",
        )
        .unwrap();
        let body = parse(vec![r]);
        assert_eq!(
            body["rules"],
            json!({"minimumAltitudeForQuantum": 2000.0, "maxLinkingRange": 10000.0,
                "inputDelay": 0.5, "arrivalRadiusScalar": 1.1,
                "rollbackFullRotationDistance": 2450000.0})
        );
        assert_eq!(body["summary"], json!({"records": 1}));
    }

    #[test]
    fn no_record_omits_rules() {
        let body = parse(vec![]);
        assert!(!body.contains_key("rules"));
        assert_eq!(body["summary"], json!({"records": 0}));
    }
}
