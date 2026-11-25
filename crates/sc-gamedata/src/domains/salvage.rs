//! Salvage: the global numbers behind hull scraping, structural salvage and the multitool
//! repair beam (port of `sc-gamedata/src/domains/salvage.ts`).
//!
//! Per-tool stats (scraper heads, modifiers) live on the items and are in
//! `game-ship-components`. These two records hold the shared constants; most of their bulk
//! is highlight colours and VFX tags, which are left out.

use crate::domain::{Domain, DomainError, ParseContext, field};
use crate::records::{GameRecord, at, num, ref_short_name};
use serde::Serialize;
use serde_json::{Map, Value};

pub static DOMAIN: Salvage = Salvage;

pub struct Salvage;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StructuralSalvage {
    /// Scale on the force that fractures a hull into salvageable pieces.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vehicle_fracture_force_scale: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_idle_field_strength: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_min_engaged_field_strength: Option<f64>,
    /// Seconds for a disintegrated piece to dissolve.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disintegrate_dissolve_time: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepairBeam {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hits_per_second: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hit_duration: Option<f64>,
    /// Hull thickness in metres that repair material fills.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hull_thickness_meters: Option<f64>,
    /// Repair material units per unit of beam ammo.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ammo_to_material_factor: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub material_resource: Option<String>,
}

pub fn parse_structural_salvage(record: &GameRecord) -> StructuralSalvage {
    let v = &record.value;
    StructuralSalvage {
        vehicle_fracture_force_scale: num(v.get("vehicleFractureForceScale")),
        source_idle_field_strength: num(at(v, "structuralVFX.sourceIdleFieldStrengthValue")),
        source_min_engaged_field_strength: num(at(
            v,
            "structuralVFX.sourceMinEngagedFieldStrengthValue",
        )),
        disintegrate_dissolve_time: num(at(v, "structuralVFX.targetDisintegrateDissolveTime")),
    }
}

pub fn parse_repair_beam(record: &GameRecord) -> RepairBeam {
    let v = &record.value;
    RepairBeam {
        hits_per_second: num(v.get("hitsPerSecond")),
        hit_duration: num(v.get("hitDuration")),
        hull_thickness_meters: num(at(v, "materialParams.hullThicknessMeters")),
        ammo_to_material_factor: num(at(v, "materialParams.ammoToMaterialFactor")),
        material_resource: ref_short_name(at(v, "materialParams.RMCResourceType"))
            .map(str::to_string),
    }
}

impl Domain for Salvage {
    fn name(&self) -> &'static str {
        "game-salvage"
    }

    fn description(&self) -> &'static str {
        "Global salvage constants: structural fracture and the repair beam."
    }

    fn filters(&self) -> &'static [&'static str] {
        &[
            "**/vehiclesalvageglobalparams/**",
            "**/sglobalsalvagerepairbeamparams/**",
        ]
    }

    fn parse(&self, ctx: &ParseContext<'_>) -> Result<Map<String, Value>, DomainError> {
        let structural = ctx
            .records
            .iter()
            .find(|r| r.ty == "VehicleSalvageGlobalParams");
        let beam = ctx
            .records
            .iter()
            .find(|r| r.ty == "SGlobalSalvageRepairBeamParams");
        let mut body = Map::new();
        if let Some(s) = structural {
            field(&mut body, "structural", &parse_structural_salvage(s))?;
        }
        if let Some(b) = beam {
            field(&mut body, "repairBeam", &parse_repair_beam(b))?;
        }
        let count = usize::from(structural.is_some()) + usize::from(beam.is_some());
        body.insert(
            "summary".into(),
            serde_json::json!({ "records": count }),
        );
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

    fn rec(name: &str, ty: &str, value: Value) -> GameRecord {
        let mut v = value;
        v["_Type_"] = json!(ty);
        normalize_record(
            json!({"_RecordName_": name, "_RecordId_": "id", "_RecordValue_": v}),
            "libs/foundry/records/x",
        )
        .unwrap()
    }

    #[test]
    fn parses_both_records_in_ts_shape() {
        let set = RecordSet::new(vec![
            rec(
                "SGlobalSalvageRepairBeamParams.Default",
                "SGlobalSalvageRepairBeamParams",
                json!({"hitsPerSecond": 10, "hitDuration": 0.3,
                       "materialParams": {"hullThicknessMeters": 0.009, "ammoToMaterialFactor": 100,
                                          "RMCResourceType": {"_RecordName_": "ResourceType.RMC"}}}),
            ),
            rec(
                "VehicleSalvageGlobalParams.Default",
                "VehicleSalvageGlobalParams",
                json!({"vehicleFractureForceScale": 0.25,
                       "structuralVFX": {"sourceIdleFieldStrengthValue": 0.1,
                                         "targetDisintegrateDissolveTime": 1}}),
            ),
        ]);
        let loc = Localization::new();
        let body = DOMAIN
            .parse(&ParseContext::new(&set, &loc, &NoFiles))
            .unwrap();
        assert_eq!(
            Value::Object(body),
            json!({
                "structural": {"vehicleFractureForceScale": 0.25, "sourceIdleFieldStrength": 0.1,
                               "disintegrateDissolveTime": 1.0},
                "repairBeam": {"hitsPerSecond": 10.0, "hitDuration": 0.3, "hullThicknessMeters": 0.009,
                               "ammoToMaterialFactor": 100.0, "materialResource": "RMC"},
                "summary": {"records": 2}
            })
        );
    }

    #[test]
    fn missing_records_are_omitted() {
        let set = RecordSet::new(vec![]);
        let loc = Localization::new();
        let body = DOMAIN
            .parse(&ParseContext::new(&set, &loc, &NoFiles))
            .unwrap();
        assert_eq!(Value::Object(body), json!({"summary": {"records": 0}}));
    }
}
