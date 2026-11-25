//! Consumables: medpens, pills, food and drink, and what taking one does (port of
//! `sc-gamedata/src/domains/consumables.ts`).
//!
//! An item's `SCItemConsumableParams` gives its volume (micro-SCU) and its contents as
//! subtype references with a ratio. The effects sit on the subtype, in the
//! ConsumableTypeDatabase, quoted per micro-SCU; totals are worked out here. Buff effects
//! name a buff type with optional duration and value overrides; the buff definitions are
//! not in the game data, so a buff without an override has no duration.

use crate::collate::compare_names;
use crate::domain::{Domain, DomainError, ParseContext, field};
use crate::entity::component;
use crate::helpers_items::item_common::item_identity;
use crate::helpers_items::{number, round_to};
use crate::localization::Localization;
use crate::output::js_number;
use crate::records::{GameRecord, at, num, str_};
use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::HashMap;

pub static DOMAIN: Consumables = Consumables;

pub struct Consumables;

/// A subtype in the ConsumableTypeDatabase.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsumableSubtype {
    pub name: String,
    /// "Drink", "Food", "Drug"...
    #[serde(rename = "type")]
    pub ty: String,
    /// Effects per micro-SCU, compacted (raw fields after `kind`).
    pub effects_per_micro_scu: Vec<Map<String, Value>>,
}

/// Subtypes by record name, in first-insertion order (a JavaScript `Map`).
#[derive(Debug, Default)]
pub struct Subtypes {
    entries: Vec<ConsumableSubtype>,
    index: HashMap<String, usize>,
}

impl Subtypes {
    pub fn get(&self, name: &str) -> Option<&ConsumableSubtype> {
        self.index.get(name).map(|&i| &self.entries[i])
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn set(&mut self, key: &str, value: ConsumableSubtype) {
        match self.index.get(key) {
            Some(&i) => self.entries[i] = value,
            None => {
                self.index.insert(key.to_string(), self.entries.len());
                self.entries.push(value);
            }
        }
    }
}

/// `parseEffect`: `kind` (the type minus `ConsumableEffect`), then the raw fields.
fn parse_effect(raw: &Value) -> Map<String, Value> {
    let kind = str_(raw.get("_Type_"))
        .map(|t| t.strip_prefix("ConsumableEffect").unwrap_or(t).to_string())
        .unwrap_or_else(|| "Unknown".into());
    let mut out = Map::new();
    out.insert("kind".into(), Value::String(kind));
    if let Some(obj) = raw.as_object() {
        for (key, value) in obj {
            if key == "_Type_" || key == "effectDescription" || value.is_null() {
                continue;
            }
            out.insert(key.clone(), value.clone());
        }
    }
    out
}

/// JavaScript truthiness of a JSON value.
fn js_truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|x| x != 0.0 && !x.is_nan()),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}

/// `parseConsumableTypes`.
pub fn parse_consumable_types(database: Option<&GameRecord>) -> Subtypes {
    let mut index = Subtypes::default();
    let list = |v: Option<&Value>| -> Vec<Value> {
        v.and_then(Value::as_array).cloned().unwrap_or_default()
    };
    for ty in list(database.and_then(|d| d.value.get("types"))) {
        if !js_truthy(&ty) {
            continue;
        }
        for sub in list(ty.get("subtypes")) {
            let Some(record_name) = sub.get("_RecordName_").and_then(Value::as_str) else {
                continue;
            };
            let effects = list(sub.get("effectsPerMicroSCU"))
                .iter()
                .filter(|e| js_truthy(e))
                .map(parse_effect)
                .collect();
            index.set(
                record_name,
                ConsumableSubtype {
                    name: str_(sub.get("typeName")).unwrap_or(record_name).to_string(),
                    ty: str_(ty.get("typeName")).unwrap_or_default().to_string(),
                    effects_per_micro_scu: effects,
                },
            );
        }
    }
    index
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Content {
    pub subtype: String,
    #[serde(rename = "type")]
    pub ty: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ratio: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Buff {
    pub buff: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
}

/// One row of `consumables`. Field order is output order.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Consumable {
    pub name: String,
    pub record_name: String,
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume_micro_scu: Option<f64>,
    pub contents: Vec<Content>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stats: Option<Map<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buffs: Option<Vec<Buff>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gases: Option<Map<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<Map<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub one_shot: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub discard_when_consumed: Option<bool>,
}

/// `String(value)` for what a reference field can hold.
fn js_string(v: Option<&Value>) -> String {
    match v {
        None => "undefined".into(),
        Some(Value::Null) => "null".into(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Number(n)) => js_number(n.as_f64().unwrap_or(f64::NAN)),
        Some(Value::Array(_)) | Some(Value::Object(_)) => "[object Object]".into(),
    }
}

fn add(table: &mut Map<String, Value>, key: &str, amount: f64) {
    let sum = table.get(key).and_then(Value::as_f64).unwrap_or(0.0) + amount;
    table.insert(key.to_string(), number(sum));
}

/// `"file://.../gasparams.oxygen.json"` -> `oxygen`.
fn gas_name(effect: &Map<String, Value>) -> String {
    let raw = match effect.get("gasMass").and_then(|g| at(g, "Gas")) {
        None | Some(Value::Null) => "gas".to_string(),
        v => js_string(v),
    };
    let last = raw.rsplit('/').next().unwrap_or(&raw);
    let last = last.strip_prefix("gasparams.").unwrap_or(last);
    last.strip_suffix(".json").unwrap_or(last).to_string()
}

pub fn parse_consumable(
    record: &GameRecord,
    subtypes: &Subtypes,
    localization: &Localization,
) -> Option<Consumable> {
    let params = component(record, "SCItemConsumableParams")?;
    let identity = item_identity(record, localization);
    // Templates carry a real item's name, so the name check alone misses them.
    if identity.is_template.is_some() || record.name.to_lowercase().contains("template") {
        return None;
    }

    let volume = num(params.get("consumableVolume").and_then(|v| at(v, "microSCU")));
    let mut contents = Vec::new();
    let mut health = 0.0;
    let mut stats = Map::new();
    let mut buffs = Vec::new();
    let mut gases = Map::new();
    let mut resources = Map::new();

    let entries = params
        .get("defaultContents")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    for entry in entries {
        if !js_truthy(entry) {
            continue;
        }
        let reference = entry.get("consumableSubtype").and_then(|s| s.get("_RecordName_"));
        let sub = reference.and_then(Value::as_str).and_then(|r| subtypes.get(r));
        let ratio = num(entry.get("ratio"));
        contents.push(Content {
            subtype: sub.map_or_else(|| js_string(reference), |s| s.name.clone()),
            ty: sub.map(|s| s.ty.clone()).unwrap_or_default(),
            ratio,
        });
        let scale = volume.unwrap_or(0.0) * ratio.unwrap_or(1.0);
        for effect in sub.map_or(&[][..], |s| s.effects_per_micro_scu.as_slice()) {
            let kind = effect.get("kind").and_then(Value::as_str);
            let text = |k: &str| effect.get(k).and_then(Value::as_str);
            match kind {
                Some("Health") => {
                    health += num(effect.get("healthChange")).unwrap_or(0.0) * scale;
                }
                Some("ModifyActorStatus") if text("statType").is_some() => {
                    let amount = num(effect.get("statPointChange")).unwrap_or(0.0) * scale;
                    add(&mut stats, text("statType").unwrap_or_default(), amount);
                }
                Some("Gas") => {
                    let mass = num(effect.get("gasMass").and_then(|g| at(g, "Mass")));
                    add(&mut gases, &gas_name(effect), mass.unwrap_or(0.0) * scale);
                }
                Some("Resource") if text("consumableResourceType").is_some() => {
                    let amount = num(effect.get("amount")).unwrap_or(0.0) * scale;
                    add(
                        &mut resources,
                        text("consumableResourceType").unwrap_or_default(),
                        amount,
                    );
                }
                Some("AddBuffEffect") if text("buffType").is_some() => buffs.push(Buff {
                    buff: text("buffType").unwrap_or_default().to_string(),
                    duration: num(effect
                        .get("buffDurationOverride")
                        .and_then(|o| at(o, "durationOverride"))),
                    value: num(effect
                        .get("buffValueOverride")
                        .and_then(|o| at(o, "valueOverride"))),
                }),
                _ => {}
            }
        }
    }

    let round = |n: f64| round_to(n, 1000.0);
    for table in [&mut stats, &mut gases, &mut resources] {
        for v in table.values_mut() {
            *v = number(round(v.as_f64().unwrap_or(f64::NAN)));
        }
    }
    let boolean = |k: &str| params.get(k).and_then(Value::as_bool);

    Some(Consumable {
        name: identity.name,
        record_name: record.name.clone(),
        id: record.id.clone(),
        item_type: identity.ty,
        sub_type: identity.sub_type,
        volume_micro_scu: volume,
        contents,
        health: (health != 0.0 && !health.is_nan()).then(|| round(health)),
        stats: (!stats.is_empty()).then_some(stats),
        buffs: (!buffs.is_empty()).then_some(buffs),
        gases: (!gases.is_empty()).then_some(gases),
        resources: (!resources.is_empty()).then_some(resources),
        one_shot: boolean("oneShotConsume"),
        discard_when_consumed: boolean("discardWhenConsumed"),
    })
}

#[derive(Debug, Serialize)]
struct Summary {
    consumables: usize,
    subtypes: usize,
}

impl Domain for Consumables {
    fn name(&self) -> &'static str {
        "game-consumables"
    }

    fn description(&self) -> &'static str {
        "Medpens, drugs, food and drink with their total health, stat and buff effects."
    }

    fn filters(&self) -> &'static [&'static str] {
        &[
            "**/entities/scitem/consumables/**",
            "**/entities/scitem/carryables/1h/**",
            "**/consumabletypesdatabase/**",
        ]
    }

    fn needs_localization(&self) -> bool {
        true
    }

    fn parse(&self, ctx: &ParseContext<'_>) -> Result<Map<String, Value>, DomainError> {
        let subtypes =
            parse_consumable_types(ctx.records.iter().find(|r| r.ty == "ConsumableTypeDatabase"));
        let mut consumables: Vec<Consumable> = ctx
            .records
            .iter()
            .filter_map(|r| parse_consumable(r, &subtypes, ctx.localization))
            .collect();
        consumables.sort_by(|a, b| compare_names(&a.name, &b.name));

        let mut by_content_type = Map::new();
        for c in &consumables {
            let ty = c
                .contents
                .first()
                .map(|x| x.ty.as_str())
                .filter(|t| !t.is_empty())
                .unwrap_or("Unknown");
            add(&mut by_content_type, ty, 1.0);
        }

        let mut sorted_subtypes: Vec<&ConsumableSubtype> = subtypes.entries.iter().collect();
        sorted_subtypes
            .sort_by(|a, b| compare_names(&a.ty, &b.ty).then_with(|| compare_names(&a.name, &b.name)));

        let mut body = Map::new();
        field(&mut body, "consumables", &consumables)?;
        field(&mut body, "subtypes", &sorted_subtypes)?;
        body.insert("byContentType".into(), Value::Object(by_content_type));
        field(
            &mut body,
            "summary",
            &Summary {
                consumables: consumables.len(),
                subtypes: subtypes.len(),
            },
        )?;
        Ok(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::NoFiles;
    use crate::localization::parse_global_ini;
    use crate::records::{RecordSet, normalize_record};
    use serde_json::json;

    fn rec(ty: &str, short: &str, path: &str, value: Value) -> GameRecord {
        let mut v = json!({"_Type_": ty});
        if let (Some(o), Value::Object(extra)) = (v.as_object_mut(), value) {
            o.extend(extra);
        }
        normalize_record(
            json!({"_RecordName_": format!("{ty}.{short}"), "_RecordId_": format!("id-{short}"), "_RecordValue_": v}),
            &format!("libs/foundry/records/{path}/{short}"),
        )
        .unwrap()
    }

    fn database() -> GameRecord {
        rec(
            "ConsumableTypeDatabase",
            "consumabletypesdatabase",
            "consumabletypesdatabase",
            json!({"types": [
                {"typeName": "Drug", "subtypes": [
                    {"_RecordName_": "ConsumableSubtype.Coagulant", "typeName": "Coagulant",
                     "effectsPerMicroSCU": [
                        {"_Type_": "ConsumableEffectHealth", "healthChange": 3.75, "effectDescription": "x", "unused": null},
                        {"_Type_": "ConsumableEffectModifyActorStatus", "statType": "BloodDrugLevel", "statPointChange": 0.1},
                        {"_Type_": "ConsumableEffectAddBuffEffect", "buffType": "Energizing",
                         "buffDurationOverride": {"durationOverride": 30}},
                        null]}]},
                {"typeName": "Drink", "subtypes": [
                    {"_RecordName_": "ConsumableSubtype.Water", "typeName": "Water",
                     "effectsPerMicroSCU": [
                        {"_Type_": "ConsumableEffectModifyActorStatus", "statType": "Thirst", "statPointChange": -0.0333333},
                        {"_Type_": "ConsumableEffectGas", "gasMass": {"Gas": "file://./x/gasparams.oxygen.json", "Mass": 0.5}},
                        {"_Type_": "ConsumableEffectResource", "consumableResourceType": "SuitFuel", "amount": 2}]}]}
            ]}),
        )
    }

    fn item(short: &str, name_key: &str, contents: Value) -> GameRecord {
        rec(
            "EntityClassDefinition",
            short,
            "entities/scitem/consumables",
            json!({"Components": [
                {"_Type_": "SAttachableComponentParams", "AttachDef": {
                    "Type": "FPS_Consumable", "SubType": "Medical", "Localization": {"Name": name_key}}},
                {"_Type_": "SCItemConsumableParams", "consumableVolume": {"microSCU": 30},
                 "oneShotConsume": true, "defaultContents": contents}
            ]}),
        )
    }

    #[test]
    fn subtype_effects_are_compacted() {
        let subtypes = parse_consumable_types(Some(&database()));
        assert_eq!(subtypes.len(), 2);
        let c = subtypes.get("ConsumableSubtype.Coagulant").unwrap();
        assert_eq!(c.ty, "Drug");
        assert_eq!(
            Value::Object(c.effects_per_micro_scu[0].clone()),
            json!({"kind": "Health", "healthChange": 3.75})
        );
        assert_eq!(c.effects_per_micro_scu.len(), 3);
    }

    #[test]
    fn totals_scale_by_volume_and_ratio() {
        let subtypes = parse_consumable_types(Some(&database()));
        let loc = parse_global_ini("item_pen=MedPen\nitem_bottle=Water Bottle");
        let pen = parse_consumable(
            &item(
                "pen",
                "@item_pen",
                json!([{"consumableSubtype": {"_RecordName_": "ConsumableSubtype.Coagulant"}, "ratio": 1}]),
            ),
            &subtypes,
            &loc,
        )
        .unwrap();
        let v = serde_json::to_value(&pen).unwrap();
        assert_eq!(v["health"], json!(112.5));
        assert_eq!(v["stats"], json!({"BloodDrugLevel": 3.0}));
        assert_eq!(v["buffs"], json!([{"buff": "Energizing", "duration": 30.0}]));
        assert_eq!(v["oneShot"], json!(true));
        assert_eq!(v["contents"], json!([{"subtype": "Coagulant", "type": "Drug", "ratio": 1.0}]));

        let bottle = parse_consumable(
            &item(
                "bottle",
                "@item_bottle",
                json!([{"consumableSubtype": {"_RecordName_": "ConsumableSubtype.Water"}, "ratio": 0.5},
                       {"consumableSubtype": null}]),
            ),
            &subtypes,
            &loc,
        )
        .unwrap();
        let v = serde_json::to_value(&bottle).unwrap();
        assert_eq!(v["stats"], json!({"Thirst": -0.5}));
        assert_eq!(v["gases"], json!({"oxygen": 7.5}));
        assert_eq!(v["resources"], json!({"SuitFuel": 30.0}));
        assert_eq!(v["contents"][1], json!({"subtype": "undefined", "type": ""}));
        assert!(v.get("health").is_none());
    }

    #[test]
    fn templates_are_dropped_and_rows_sorted() {
        let loc = parse_global_ini("item_pen=MedPen\nitem_bottle=Water Bottle");
        let set = RecordSet::new(vec![
            database(),
            item("bottle", "@item_bottle", json!([{"consumableSubtype": {"_RecordName_": "ConsumableSubtype.Water"}}])),
            item("pen", "@item_pen", json!([{"consumableSubtype": {"_RecordName_": "ConsumableSubtype.Coagulant"}}])),
            item("pen_template", "@item_pen", json!([])),
            item("stub", "@missing", json!([])),
        ]);
        let body = DOMAIN
            .parse(&ParseContext::new(&set, &loc, &NoFiles))
            .unwrap();
        let names: Vec<&str> = body["consumables"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, vec!["MedPen", "Water Bottle"]);
        assert_eq!(body["byContentType"], json!({"Drink": 1.0, "Drug": 1.0}));
        assert_eq!(body["subtypes"][0]["name"], "Water");
        assert_eq!(body["summary"], json!({"consumables": 2, "subtypes": 2}));
    }
}
