//! Cargo: what NPC and mission ships spawn carrying, and how long loading takes (port of
//! `sc-gamedata/src/domains/cargo.ts`).
//!
//! A `CargoManifest` fills a ship's grid. Its `cargoFillCapacity` picks a fill mode
//! (Random, Even or Custom) and a capacity range as a fraction of the grid, then lists
//! resources either with a probability (`CargoResource`) or with a share of the hold
//! (`CargoResourceAllocation`, whose `resources` is a single entry despite the plural).
//! `cargoSpawnRules` add specific items or resources on top.
//!
//! `GlobalCargoLoadingParams` holds the freight elevator and auto-loading timings, per box
//! size. Box sizes are keyed by name (`one_eighthSCU` .. `thirtyTwoSCU`) and are turned
//! into a list ordered by SCU.
//!
//! `entityName` (from `wikelo.ts`) lives in [`crate::helpers_ships::entity_name`].

use crate::collate::compare_names;
use crate::domain::{Domain, DomainError, ParseContext, field};
use crate::helpers_ships::entity_name;
use crate::records::{GameRecord, array, at, num, ref_short_name, str_};
use serde::Serialize;
use serde_json::{Map, Value};

pub static DOMAIN: Cargo = Cargo;

pub struct Cargo;

const BOX_SIZES: [(&str, f64); 10] = [
    ("one_eighthSCU", 0.125),
    ("one_quarterSCU", 0.25),
    ("one_halfSCU", 0.5),
    ("oneSCU", 1.0),
    ("twoSCU", 2.0),
    ("fourSCU", 4.0),
    ("eightSCU", 8.0),
    ("sixteenSCU", 16.0),
    ("twentyFourSCU", 24.0),
    ("thirtyTwoSCU", 32.0),
];

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BoxSizeValue {
    pub scu: f64,
    pub value: f64,
}

/// `{ twoSCU: 55, oneSCU: 30 }` -> `[{scu: 1, value: 30}, {scu: 2, value: 55}]`.
pub fn box_sizes(value: Option<&Value>) -> Option<Vec<BoxSizeValue>> {
    let obj = value?.as_object()?;
    let mut out: Vec<BoxSizeValue> = BOX_SIZES
        .iter()
        .filter_map(|&(key, scu)| num(obj.get(key)).map(|value| BoxSizeValue { scu, value }))
        .collect();
    out.sort_by(|a, b| a.scu.total_cmp(&b.scu));
    (!out.is_empty()).then_some(out)
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestResource {
    pub resource: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub probability: Option<f64>,
    /// Share of the hold, 0..1, for allocation entries.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_share: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_share: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SpawnRule {
    /// `entity` or `resource`.
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub probability: Option<f64>,
    pub items: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quantity: Option<f64>,
    /// Share of capacity, for proportion rules.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proportion: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CargoManifest {
    pub name: String,
    pub record_name: String,
    pub id: String,
    /// Subfolder, e.g. `civilian`, or the name's first segment at top level.
    pub category: String,
    /// `Random`, `Even` or `Custom`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fill_mode: Option<String>,
    /// Fraction of the cargo grid filled, 0..1.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_capacity: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_capacity: Option<f64>,
    pub resources: Vec<ManifestResource>,
    pub spawn_rules: Vec<SpawnRule>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lock_cargo_grids_on_spawn: Option<bool>,
}

fn parse_manifest_resource(raw: &Map<String, Value>) -> Option<ManifestResource> {
    if raw.get("_Type_").and_then(Value::as_str) == Some("CargoResourceAllocation") {
        let inner = raw.get("resources");
        let get = |k: &str| inner.and_then(|i| i.get(k));
        let resource = ref_short_name(get("resource"))?;
        return Some(ManifestResource {
            resource: resource.to_string(),
            probability: num(get("probability")),
            min_share: num(raw.get("minResourceAllocationPercentage")),
            max_share: num(raw.get("maxResourceAllocationPercentage")),
        });
    }
    let resource = ref_short_name(raw.get("resource"))?;
    Some(ManifestResource {
        resource: resource.to_string(),
        probability: num(raw.get("probability")),
        min_share: None,
        max_share: None,
    })
}

fn parse_spawn_rule(raw: &Value) -> SpawnRule {
    let is_entity = raw.get("_Type_").and_then(Value::as_str) == Some("CargoSpawnEntityDef");
    let list = array(raw.get(if is_entity { "entities" } else { "resources" }));
    let items = list
        .iter()
        .filter_map(|x| {
            if is_entity {
                entity_name(Some(x))
            } else {
                ref_short_name(Some(x))
            }
        })
        .filter(|x| !x.is_empty())
        .map(str::to_string)
        .collect();
    SpawnRule {
        kind: if is_entity { "entity" } else { "resource" },
        probability: num(raw.get("probability")),
        items,
        quantity: num(at(raw, "quantityAlgorithmType.quantity")),
        proportion: num(at(raw, "quantityAlgorithmType.proportion")),
    }
}

pub fn parse_cargo_manifest(record: &GameRecord) -> CargoManifest {
    let v = &record.value;
    let fill = v.get("cargoFillCapacity");
    let f = |k: &str| fill.and_then(|f| f.get(k));
    let segments: Vec<&str> = record.record_path.split('/').collect();
    let category = match segments.iter().position(|s| *s == "cargomanifest") {
        Some(folder) if segments.len() > folder + 2 => segments[folder + 1].to_string(),
        _ => record
            .short_name
            .split('_')
            .next()
            .unwrap_or_default()
            .to_lowercase(),
    };
    CargoManifest {
        name: record.short_name.clone(),
        record_name: record.name.clone(),
        id: record.id.clone(),
        category,
        fill_mode: str_(f("_Type_")).map(|t| {
            t.strip_prefix("CargoFillCapacityValue_")
                .unwrap_or(t)
                .to_string()
        }),
        min_capacity: num(f("minCapacityRange")),
        max_capacity: num(f("maxCapacityRange")),
        resources: array(f("resources"))
            .iter()
            .filter_map(Value::as_object)
            .filter_map(parse_manifest_resource)
            .collect(),
        spawn_rules: array(v.get("cargoSpawnRules"))
            .iter()
            .filter(|r| r.is_object())
            .map(parse_spawn_rule)
            .collect(),
        lock_cargo_grids_on_spawn: v.get("lockCargoGridsOnSpawn").and_then(Value::as_bool),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CargoLoading {
    /// Freight elevator seconds per SCU.
    #[serde(rename = "cargoDeckLoadingTimePerSCU")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cargo_deck_loading_time_per_scu: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_loading_base_loading_time: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_loading_base_unloading_time: Option<f64>,
    /// Seconds per box, by box size in SCU.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_loading_box_loading_time: Option<Vec<BoxSizeValue>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_loading_box_unloading_time: Option<Vec<BoxSizeValue>>,
    /// Seconds before an unclaimed loading area is revoked.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revoke_time_delay: Option<f64>,
    /// Seconds of grace before cargo left in a loading area is forfeit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub forfeit_time_buffer: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ui_time_remaining_for_time_out_warning: Option<f64>,
}

pub fn parse_cargo_loading(record: &GameRecord) -> CargoLoading {
    let v = &record.value;
    CargoLoading {
        cargo_deck_loading_time_per_scu: num(v.get("cargoDeckLoadingTimePerSCU")),
        auto_loading_base_loading_time: num(v.get("autoLoadingBaseLoadingTime")),
        auto_loading_base_unloading_time: num(v.get("autoLoadingBaseUnloadingTime")),
        auto_loading_box_loading_time: box_sizes(v.get("autoLoadingBoxSizeLoadingTime")),
        auto_loading_box_unloading_time: box_sizes(v.get("autoLoadingBoxSizeUnloadingTime")),
        revoke_time_delay: num(v.get("revokeTimeDelay")),
        forfeit_time_buffer: num(v.get("forfeitTimeBuffer")),
        ui_time_remaining_for_time_out_warning: num(v.get("uiTimeRemainingForTimeOutWarning")),
    }
}

#[derive(Debug, Serialize)]
struct Summary {
    manifests: usize,
}

impl Domain for Cargo {
    fn name(&self) -> &'static str {
        "game-cargo"
    }

    fn description(&self) -> &'static str {
        "NPC and mission cargo manifests, and freight loading timings."
    }

    fn filters(&self) -> &'static [&'static str] {
        &["**/cargomanifest/**", "**/globalcargoloadingparams/**"]
    }

    fn parse(&self, ctx: &ParseContext<'_>) -> Result<Map<String, Value>, DomainError> {
        let mut manifests: Vec<CargoManifest> = ctx
            .records
            .of_type("CargoManifest")
            .map(parse_cargo_manifest)
            .collect();
        manifests.sort_by(|a, b| {
            compare_names(&a.category, &b.category).then_with(|| compare_names(&a.name, &b.name))
        });
        let loading = ctx
            .records
            .iter()
            .find(|r| r.ty == "GlobalCargoLoadingParams");

        // A JS object: insertion order, integer-like keys first (the writer moves those).
        let mut by_category: Map<String, Value> = Map::new();
        for m in &manifests {
            if let Value::Array(a) = by_category
                .entry(m.category.clone())
                .or_insert_with(|| Value::Array(Vec::new()))
            {
                a.push(Value::String(m.name.clone()));
            }
        }

        let mut body = Map::new();
        field(&mut body, "manifests", &manifests)?;
        if let Some(r) = loading {
            field(&mut body, "loading", &parse_cargo_loading(r))?;
        }
        body.insert("byCategory".into(), Value::Object(by_category));
        field(
            &mut body,
            "summary",
            &Summary {
                manifests: manifests.len(),
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

    fn record(name: &str, path: &str, value: Value) -> GameRecord {
        normalize_record(
            json!({"_RecordName_": name, "_RecordId_": "id", "_RecordValue_": value}),
            path,
        )
        .unwrap()
    }

    #[test]
    fn box_sizes_are_ordered_by_scu() {
        let v = json!({"twoSCU": 55, "oneSCU": 30, "_Type_": "X", "one_eighthSCU": 1});
        assert_eq!(
            box_sizes(Some(&v)).unwrap(),
            vec![
                BoxSizeValue {
                    scu: 0.125,
                    value: 1.0
                },
                BoxSizeValue {
                    scu: 1.0,
                    value: 30.0
                },
                BoxSizeValue {
                    scu: 2.0,
                    value: 55.0
                },
            ]
        );
        assert!(box_sizes(Some(&json!({}))).is_none());
    }

    #[test]
    fn parses_manifests_resources_and_rules() {
        let m = parse_cargo_manifest(&record(
            "CargoManifest.Civ_Small",
            "libs/foundry/records/cargomanifest/civilian/civ_small",
            json!({"_Type_": "CargoManifest", "lockCargoGridsOnSpawn": true,
                "cargoFillCapacity": {"_Type_": "CargoFillCapacityValue_Custom",
                    "minCapacityRange": 0.2, "maxCapacityRange": 0.4,
                    "resources": [
                        {"_Type_": "CargoResource", "resource": {"_RecordName_": "ResourceType.Gold"}, "probability": 0.5},
                        null,
                        {"_Type_": "CargoResourceAllocation", "minResourceAllocationPercentage": 0.1,
                            "maxResourceAllocationPercentage": 0.3,
                            "resources": {"resource": {"_RecordName_": "ResourceType.Tin"}, "probability": 1}},
                        {"_Type_": "CargoResource"}
                    ]},
                "cargoSpawnRules": [
                    {"_Type_": "CargoSpawnEntityDef", "probability": 1,
                        "entities": ["file://./../x/Crate_A.json", ""],
                        "quantityAlgorithmType": {"quantity": 2}},
                    null,
                    {"_Type_": "CargoSpawnResourceDef", "resources": [{"_RecordName_": "ResourceType.Ice"}],
                        "quantityAlgorithmType": {"proportion": 0.25}}
                ]}),
        ));
        assert_eq!(m.category, "civilian");
        assert_eq!(m.fill_mode.as_deref(), Some("Custom"));
        assert_eq!(m.lock_cargo_grids_on_spawn, Some(true));
        assert_eq!(
            serde_json::to_value(&m.resources).unwrap(),
            json!([{"resource": "Gold", "probability": 0.5},
                {"resource": "Tin", "probability": 1.0, "minShare": 0.1, "maxShare": 0.3}])
        );
        assert_eq!(
            serde_json::to_value(&m.spawn_rules).unwrap(),
            json!([{"kind": "entity", "probability": 1.0, "items": ["Crate_A"], "quantity": 2.0},
                {"kind": "resource", "items": ["Ice"], "proportion": 0.25}])
        );
    }

    #[test]
    fn top_level_manifests_take_the_name_prefix_and_sort() {
        let set = RecordSet::new(vec![
            record(
                "CargoManifest.Pyro_B",
                "libs/foundry/records/cargomanifest/pyro_b",
                json!({"_Type_": "CargoManifest"}),
            ),
            record(
                "CargoManifest.Civ_A",
                "libs/foundry/records/cargomanifest/civilian/civ_a",
                json!({"_Type_": "CargoManifest"}),
            ),
            record(
                "GlobalCargoLoadingParams.G",
                "libs/foundry/records/globalcargoloadingparams/g",
                json!({"_Type_": "GlobalCargoLoadingParams", "revokeTimeDelay": 30}),
            ),
        ]);
        let loc = Localization::new();
        let body = DOMAIN
            .parse(&ParseContext::new(&set, &loc, &NoFiles))
            .unwrap();
        let keys: Vec<_> = body.keys().cloned().collect();
        assert_eq!(keys, vec!["manifests", "loading", "byCategory", "summary"]);
        assert_eq!(
            body["byCategory"],
            json!({"civilian": ["Civ_A"], "pyro": ["Pyro_B"]})
        );
        assert_eq!(body["loading"], json!({"revokeTimeDelay": 30.0}));
        assert_eq!(body["summary"], json!({"manifests": 2}));
    }
}
