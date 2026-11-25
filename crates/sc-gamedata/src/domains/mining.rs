//! Mining: mineable element physics, and the laser modifiers that work them (port of
//! `sc-gamedata/src/domains/mining.ts`).
//!
//! A `MineableElement` holds the physics of breaking a rock: how violently it reacts to
//! power (instability), how much power it resists, and where the optimal charge window
//! sits.
//!
//! Laser and module tuning is not a top-level field. It sits at
//! `Components[].modifiers[].MiningLaserModifier`, a `MiningLaserModifiers` struct whose
//! entries are null on records that do not modify that stat. The game wraps every value
//! (`{"_Type_": "FloatModifierMultiplicative", "value": -10}`); the wrapper type decides
//! how it applies, so both are kept.

use crate::collate::locale_compare;
use crate::domain::{Domain, DomainError, ParseContext, field};
use crate::output::js_key_order;
use crate::records::{GameRecord, num, ref_short_name, str_};
use serde::Serialize;
use serde_json::{Map, Value, json};

pub static DOMAIN: Mining = Mining;

pub struct Mining;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MineableElement {
    pub name: String,
    pub record_name: String,
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instability: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resistance: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub optimal_window_midpoint: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub optimal_window_randomness: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub optimal_window_thinness: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explosion_multiplier: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cluster_factor: Option<f64>,
}

pub fn parse_mineable_element(record: &GameRecord) -> MineableElement {
    let v = &record.value;
    MineableElement {
        // The resourceType reference carries the outward-facing name ("Raw_Gold"); the
        // record's own name is the internal one ("Gold_Raw").
        name: ref_short_name(v.get("resourceType"))
            .map(str::to_string)
            .unwrap_or_else(|| record.short_name.clone()),
        record_name: record.name.clone(),
        id: record.id.clone(),
        instability: num(v.get("elementInstability")),
        resistance: num(v.get("elementResistance")),
        optimal_window_midpoint: num(v.get("elementOptimalWindowMidpoint")),
        optimal_window_randomness: num(v.get("elementOptimalWindowMidpointRandomness")),
        optimal_window_thinness: num(v.get("elementOptimalWindowThinness")),
        explosion_multiplier: num(v.get("elementExplosionMultiplier")),
        cluster_factor: num(v.get("elementClusterFactor")),
    }
}

/// `parseModifier`: `{value, kind, showInUI?}`, `None` unless an object with a number.
pub fn parse_modifier(raw: Option<&Value>) -> Option<Value> {
    let obj = raw?.as_object()?;
    let value = num(obj.get("value"))?;
    let ty = str_(obj.get("_Type_")).unwrap_or("");
    let kind = if ty.contains("Multiplicative") {
        "multiplicative"
    } else if ty.contains("Additive") {
        "additive"
    } else {
        "unknown"
    };
    let mut out = Map::new();
    out.insert("value".into(), json!(value));
    out.insert("kind".into(), json!(kind));
    if let Some(b) = obj.get("showInUI").and_then(Value::as_bool) {
        out.insert("showInUI".into(), json!(b));
    }
    Some(Value::Object(out))
}

/// One `LaserModifiers` object: only defined keys, in the TS literal's key order.
type LaserModifiers = Vec<(&'static str, Value)>;

fn parse_laser_modifiers(m: &Map<String, Value>) -> LaserModifiers {
    let shatter = match m.get("shatterdamageModifier") {
        // `m.shatterdamageModifier ?? m.shatterDamageModifier`
        Some(v) if !v.is_null() => Some(v),
        _ => m.get("shatterDamageModifier"),
    };
    let entries: [(&'static str, Option<Value>); 8] = [
        ("laserInstability", parse_modifier(m.get("laserInstability"))),
        ("resistanceModifier", parse_modifier(m.get("resistanceModifier"))),
        (
            "optimalChargeWindowSizeModifier",
            parse_modifier(m.get("optimalChargeWindowSizeModifier")),
        ),
        (
            "optimalChargeWindowRateModifier",
            parse_modifier(m.get("optimalChargeWindowRateModifier")),
        ),
        (
            "catastrophicChargeWindowRateModifier",
            parse_modifier(m.get("catastrophicChargeWindowRateModifier")),
        ),
        // The game data spells this with a lowercase d.
        ("shatterDamageModifier", parse_modifier(shatter)),
        ("clusterFactorModifier", parse_modifier(m.get("clusterFactorModifier"))),
        (
            "isOptimalRateGood",
            m.get("isOptimalRateGood")
                .and_then(Value::as_bool)
                .map(Value::Bool),
        ),
    ];
    entries
        .into_iter()
        .filter_map(|(k, v)| v.map(|v| (k, v)))
        .collect()
}

/// `collectLaserModifiers`: every `MiningLaserModifier` struct nested anywhere, in JS
/// traversal order. All-null structs (a record that modifies nothing) are skipped.
pub fn collect_laser_modifiers(value: &Value, found: &mut Vec<LaserModifiers>) {
    match value {
        Value::Array(a) => {
            for entry in a {
                collect_laser_modifiers(entry, found);
            }
        }
        Value::Object(obj) => {
            if let Some(Value::Object(m)) = obj.get("MiningLaserModifier") {
                let parsed = parse_laser_modifiers(m);
                if parsed.iter().any(|(_, v)| !v.is_boolean()) {
                    found.push(parsed);
                }
            }
            for key in js_key_order(obj) {
                collect_laser_modifiers(&obj[key.as_str()], found);
            }
        }
        _ => {}
    }
}

/// `mergeModifiers`: first defined value wins; keys keep first-seen order.
fn merge_modifiers(all: &[LaserModifiers]) -> Map<String, Value> {
    let mut merged = Map::new();
    for one in all {
        for (k, v) in one {
            if !merged.contains_key(*k) {
                merged.insert((*k).to_string(), v.clone());
            }
        }
    }
    merged
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MiningModule {
    pub name: String,
    pub record_name: String,
    pub id: String,
    pub modifiers: Map<String, Value>,
}

pub fn parse_mining_module(record: &GameRecord) -> Option<MiningModule> {
    let mut found = Vec::new();
    collect_laser_modifiers(&record.value, &mut found);
    let modifiers = merge_modifiers(&found);
    if modifiers.is_empty() {
        return None;
    }
    Some(MiningModule {
        name: record.short_name.clone(),
        record_name: record.name.clone(),
        id: record.id.clone(),
        modifiers,
    })
}

impl Domain for Mining {
    fn name(&self) -> &'static str {
        "game-mining"
    }

    fn description(&self) -> &'static str {
        "Mineable element physics and mining laser/module modifiers."
    }

    fn filters(&self) -> &'static [&'static str] {
        &["**/mining/**"]
    }

    fn parse(&self, ctx: &ParseContext<'_>) -> Result<Map<String, Value>, DomainError> {
        let mut elements = Vec::new();
        let mut modules = Vec::new();
        for record in ctx.records.iter() {
            if record.ty == "MineableElement" {
                elements.push(parse_mineable_element(record));
                continue;
            }
            if let Some(m) = parse_mining_module(record) {
                modules.push(m);
            }
        }
        elements.sort_by(|a, b| locale_compare(&a.name, &b.name));
        modules.sort_by(|a, b| locale_compare(&a.name, &b.name));

        // Resistance and instability decide whether a laser can break a rock, so surface
        // them as a flat lookup alongside the full records. A later duplicate name keeps
        // the first one's position, as a JS object does.
        let mut signatures = Map::new();
        for e in &elements {
            let mut s = Map::new();
            if let Some(r) = e.resistance {
                s.insert("resistance".into(), json!(r));
            }
            if let Some(i) = e.instability {
                s.insert("instability".into(), json!(i));
            }
            signatures.insert(e.name.clone(), Value::Object(s));
        }

        let mut body = Map::new();
        field(&mut body, "mineableElements", &elements)?;
        field(&mut body, "miningModules", &modules)?;
        body.insert("elementSignatures".into(), Value::Object(signatures));
        body.insert(
            "summary".into(),
            json!({"mineableElements": elements.len(), "miningModules": modules.len()}),
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

    fn record(name: &str, value: Value) -> GameRecord {
        normalize_record(
            json!({"_RecordName_": name, "_RecordId_": "id-1", "_RecordValue_": value}),
            "libs/foundry/records/mining/mineableelements/test",
        )
        .unwrap()
    }

    /// Shaped exactly as StarBreaker emits it.
    fn quantainium() -> GameRecord {
        record(
            "MineableElement.Quantainium_Raw",
            json!({
                "_Type_": "MineableElement",
                "resourceType": {"_RecordName_": "ResourceType.Raw_Quantainium", "_RecordId_": "501d"},
                "elementInstability": 1000.0, "elementResistance": 0.95,
                "elementOptimalWindowMidpoint": 0.5, "elementOptimalWindowMidpointRandomness": 0.2,
                "elementOptimalWindowThinness": 2.3, "elementExplosionMultiplier": 260.0,
                "elementClusterFactor": 0.1
            }),
        )
    }

    // Cases from sc-gamedata src/domains/mining.test.ts.
    #[test]
    fn mineable_element() {
        let e = parse_mineable_element(&quantainium());
        assert_eq!(e.instability, Some(1000.0));
        assert_eq!(e.resistance, Some(0.95));
        assert_eq!(e.explosion_multiplier, Some(260.0));
        assert_eq!(e.cluster_factor, Some(0.1));
        assert_eq!(e.name, "Raw_Quantainium");
        let orphan = record("MineableElement.Orphan_Raw", json!({"_Type_": "MineableElement"}));
        assert_eq!(parse_mineable_element(&orphan).name, "Orphan_Raw");
    }

    #[test]
    fn modifiers() {
        assert_eq!(
            parse_modifier(Some(
                &json!({"_Type_": "FloatModifierMultiplicative", "showInUI": true, "value": -10})
            )),
            Some(json!({"value": -10.0, "kind": "multiplicative", "showInUI": true}))
        );
        assert_eq!(
            parse_modifier(Some(&json!({"_Type_": "FloatModifierAdditive", "value": 3})))
                .unwrap()["kind"],
            "additive"
        );
        assert_eq!(parse_modifier(Some(&Value::Null)), None);
        assert_eq!(
            parse_modifier(Some(
                &json!({"_Type_": "FloatModifierMultiplicative", "value": null})
            )),
            None
        );
        assert_eq!(parse_modifier(Some(&json!(15.5))), None);
    }

    fn nested() -> Value {
        json!({"Components": [
            {"_Type_": "SomethingElse"},
            {"_Type_": "EntityComponentAttachableModifierParams", "modifiers": [
                {"_Type_": "ItemWeaponModifiersParams"},
                {"MiningLaserModifier": {
                    "_Type_": "MiningLaserModifiers", "laserInstability": null,
                    "resistanceModifier": {"_Type_": "FloatModifierMultiplicative", "showInUI": true, "value": 15.5},
                    "shatterdamageModifier": {"_Type_": "FloatModifierMultiplicative", "showInUI": true, "value": -30},
                    "isOptimalRateGood": true}}
            ]}
        ]})
    }

    #[test]
    fn collects_nested_modifiers() {
        let mut found = Vec::new();
        collect_laser_modifiers(&nested(), &mut found);
        assert_eq!(found.len(), 1);
        let get = |k: &str| found[0].iter().find(|(n, _)| *n == k).map(|(_, v)| v.clone());
        assert_eq!(
            get("resistanceModifier"),
            Some(json!({"value": 15.5, "kind": "multiplicative", "showInUI": true}))
        );
        assert_eq!(get("shatterDamageModifier").unwrap()["value"], json!(-30.0));

        let inert = json!({"MiningLaserModifier": {"_Type_": "MiningLaserModifiers",
            "laserInstability": null, "resistanceModifier": null, "isOptimalRateGood": true}});
        let mut none = Vec::new();
        collect_laser_modifiers(&inert, &mut none);
        assert!(none.is_empty());
        collect_laser_modifiers(&json!({"Components": [{"_Type_": "Nothing"}]}), &mut none);
        assert!(none.is_empty());
        assert!(parse_mining_module(&record("EntityClassDefinition.X", json!({"_Type_": "EntityClassDefinition"}))).is_none());
    }

    #[test]
    fn merge_keeps_first_seen_key_order() {
        let a = vec![("resistanceModifier", json!(1))];
        let b = vec![("laserInstability", json!(2)), ("resistanceModifier", json!(3))];
        let m = merge_modifiers(&[a, b]);
        assert_eq!(Value::Object(m), json!({"resistanceModifier": 1, "laserInstability": 2}));
        let keys: Vec<_> = merge_modifiers(&[
            vec![("resistanceModifier", json!(1))],
            vec![("laserInstability", json!(2))],
        ])
        .keys()
        .cloned()
        .collect();
        assert_eq!(keys, vec!["resistanceModifier", "laserInstability"]);
    }

    #[test]
    fn domain_splits_elements_from_modules() {
        let module = record(
            "EntityClassDefinition.Mining_Modules_Active_Brandt",
            json!({"_Type_": "EntityClassDefinition", "Components": [{"modifiers": [{"MiningLaserModifier": {
                "_Type_": "MiningLaserModifiers",
                "resistanceModifier": {"_Type_": "FloatModifierMultiplicative", "value": 15.5}}}]}]}),
        );
        let set = RecordSet::new(vec![quantainium(), module]);
        let loc = Localization::new();
        let out = DOMAIN.parse(&ParseContext::new(&set, &loc, &NoFiles)).unwrap();
        assert_eq!(out["summary"], json!({"mineableElements": 1, "miningModules": 1}));
        assert_eq!(
            out["elementSignatures"]["Raw_Quantainium"],
            json!({"resistance": 0.95, "instability": 1000.0})
        );
        let empty = RecordSet::new(vec![]);
        let out = DOMAIN.parse(&ParseContext::new(&empty, &loc, &NoFiles)).unwrap();
        assert_eq!(out["summary"], json!({"mineableElements": 0, "miningModules": 0}));
    }
}
