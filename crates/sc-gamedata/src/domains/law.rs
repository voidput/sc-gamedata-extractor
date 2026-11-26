//! Law: what counts as a crime, how it is punished, and where (port of
//! `sc-gamedata/src/domains/law.ts`).
//!
//! An `InfractionDefinition` is a crime: its triggers (KillActor, PadRamming...) and default
//! parameters (felony or not, prison merits, hours on record, the late-payment fine
//! multiplier). A `Jurisdiction` (UEE, ArcCorp, the Pyro gangs) lists the crimes it
//! enforces with per-jurisdiction overrides, its base fine, prohibited goods, controlled
//! substance classes with possession limits, and impound rules.
//!
//! Unset parameters are stored as -1 ("inherit") and dropped, so only real values remain.

use crate::collate::compare_names;
use crate::domain::{Domain, DomainError, ParseContext, field};
use crate::localization::{Localization, localize};
use crate::output::js_key_order;
use crate::records::{GameRecord, num, ref_path, ref_path_str, ref_short_name, str_};
use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::HashMap;

pub static DOMAIN: Law = Law;

pub struct Law;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Crime {
    pub name: String,
    pub record_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// `felonies`, `misdemeanours` or `special`.
    pub category: String,
    pub triggers: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<Map<String, Value>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EnforcedCrime {
    pub crime: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overrides: Option<Map<String, Value>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubstanceClass {
    pub commodities: Vec<String>,
    pub resources: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_possession_scu: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Impounding {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seconds: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fine: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ignore_if_against_party_member: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Jurisdiction {
    pub name: String,
    pub record_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub respects_parent_laws: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_prison: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_fine: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_stolen_goods_possession_scu: Option<f64>,
    /// Crimes enforced here, by crime name, with any overrides of their defaults.
    pub crimes: Vec<EnforcedCrime>,
    pub prohibited_goods: Vec<String>,
    pub prohibited_resources: Vec<String>,
    pub controlled_substance_classes: Vec<SubstanceClass>,
    pub impounding: Vec<Impounding>,
}

#[derive(Debug, Serialize)]
struct Summary {
    crimes: usize,
    jurisdictions: usize,
}

/// `setParameters`: the parameter fields that are set, -1 ("inherit") and non-numbers
/// dropped. Flags (`isFelony`, `ignore*`, `hide*`) are ints, 1 on, 0 off; they come out
/// as booleans. Other numbers are kept as stored.
pub fn set_parameters(value: Option<&Value>) -> Option<Map<String, Value>> {
    let obj = value?.as_object()?;
    let mut out = Map::new();
    for key in js_key_order(obj) {
        let raw = &obj[key];
        let Some(n) = num(Some(raw)) else { continue };
        if n == -1.0 {
            continue;
        }
        let flag = key == "isFelony" || key.starts_with("ignore") || key.starts_with("hide");
        out.insert(key.clone(), if flag { Value::Bool(n == 1.0) } else { raw.clone() });
    }
    (!out.is_empty()).then_some(out)
}

/// Last path segment of a reference: the crime's or commodity's file name.
fn file_of(reference: Option<&Value>) -> Option<String> {
    ref_path(reference).map(|p| p.rsplit('/').next().unwrap_or_default().to_string())
}

/// `list`: the truthy object (or array) entries of an array.
fn list(value: Option<&Value>) -> impl Iterator<Item = &Value> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|x| x.is_object() || x.is_array())
}

/// `names`: picked names of an array's entries, empty ones dropped.
fn names(value: Option<&Value>, pick: impl Fn(Option<&Value>) -> Option<String>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|x| pick(Some(x)))
        .filter(|s| !s.is_empty())
        .collect()
}

fn bool_of(value: Option<&Value>) -> Option<bool> {
    value.and_then(Value::as_bool)
}

pub fn parse_crime(record: &GameRecord, localization: &Localization) -> Crime {
    let v = &record.value;
    let segments: Vec<&str> = record.record_path.split('/').collect();
    // `segments[segments.indexOf("infractiondefinitions") + 1] ?? ""`: -1 + 1 reads the
    // first segment when the marker is missing.
    let at_index = segments
        .iter()
        .position(|s| *s == "infractiondefinitions")
        .map_or(0, |i| i + 1);
    Crime {
        name: record.short_name.clone(),
        record_name: record.name.clone(),
        display_name: localize(localization, v.get("name")).map(str::to_string),
        category: segments.get(at_index).copied().unwrap_or_default().to_string(),
        triggers: v
            .get("triggers")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|t| t.as_str().map(str::to_string))
            .collect(),
        parameters: set_parameters(v.get("defaultParameters")),
    }
}

pub fn parse_jurisdiction(
    record: &GameRecord,
    localization: &Localization,
    crime_names: &HashMap<String, String>,
) -> Jurisdiction {
    let v = &record.value;
    // Crimes are referenced by path; name them as the crimes list does.
    let crime_of = |reference: Option<&Value>| {
        let path = ref_path(reference).filter(|p| !p.is_empty())?;
        Some(
            crime_names
                .get(&path)
                .cloned()
                .unwrap_or_else(|| path.rsplit('/').next().unwrap_or_default().to_string()),
        )
    };
    let short = |x: Option<&Value>| ref_short_name(x).filter(|n| !n.is_empty()).map(str::to_string);
    Jurisdiction {
        name: record.short_name.clone(),
        record_name: record.name.clone(),
        display_name: localize(localization, v.get("name")).map(str::to_string),
        parent: file_of(v.get("parentJurisdiction")),
        respects_parent_laws: bool_of(v.get("respectsParentJurisdictionLaws")),
        is_prison: bool_of(v.get("isPrison")),
        base_fine: num(v.get("baseFine")),
        max_stolen_goods_possession_scu: num(v.get("maxStolenGoodsPossessionSCU")),
        crimes: list(v.get("infractions"))
            .map(|i| EnforcedCrime {
                crime: crime_of(i.get("definition")).unwrap_or_default(),
                overrides: set_parameters(i.get("parameterOverrides")),
            })
            .filter(|c| !c.crime.is_empty())
            .collect(),
        prohibited_goods: names(v.get("prohibitedGoods"), file_of),
        prohibited_resources: names(v.get("prohibitedResources"), short),
        controlled_substance_classes: list(v.get("controlledSubstanceClasses"))
            .map(|c| SubstanceClass {
                commodities: names(c.get("commodities"), file_of),
                resources: names(c.get("resources"), short),
                max_possession_scu: num(c.get("maxPossessionSCU")),
            })
            .collect(),
        impounding: list(v.get("impoundingDefinitions"))
            .map(|d| Impounding {
                name: localize(localization, d.get("name")).map(str::to_string),
                trigger: str_(d.get("trigger")).map(str::to_string),
                seconds: num(d.get("impoundingTimeSeconds")),
                fine: num(d.get("impoundingFineUEC")),
                ignore_if_against_party_member: bool_of(d.get("ignoreIfAgainstPartyMember")),
            })
            .collect(),
    }
}

impl Domain for Law {
    fn name(&self) -> &'static str {
        "game-law"
    }
    fn schema_version(&self) -> u32 {
        1
    }

    fn description(&self) -> &'static str {
        "Crimes with their penalties, and jurisdictions with fines, prohibited goods and impound rules."
    }

    fn filters(&self) -> &'static [&'static str] {
        &[
            "**/lawsystem/infractiondefinitions/**",
            "**/lawsystem/jurisdictions/**",
        ]
    }

    fn needs_localization(&self) -> bool {
        true
    }

    fn parse(&self, ctx: &ParseContext<'_>) -> Result<Map<String, Value>, DomainError> {
        let definitions: Vec<&GameRecord> = ctx.records.of_type("InfractionDefinition").collect();
        let crime_names: HashMap<String, String> = definitions
            .iter()
            .map(|r| {
                (
                    ref_path_str(&r.record_path).unwrap_or_default(),
                    r.short_name.clone(),
                )
            })
            .collect();
        let mut crimes: Vec<Crime> = definitions
            .iter()
            .map(|r| parse_crime(r, ctx.localization))
            .collect();
        crimes.sort_by(|a, b| {
            compare_names(&a.category, &b.category).then_with(|| compare_names(&a.name, &b.name))
        });
        let mut jurisdictions: Vec<Jurisdiction> = ctx
            .records
            .of_type("Jurisdiction")
            .map(|r| parse_jurisdiction(r, ctx.localization, &crime_names))
            .collect();
        jurisdictions.sort_by(|a, b| {
            compare_names(
                a.display_name.as_deref().unwrap_or(&a.name),
                b.display_name.as_deref().unwrap_or(&b.name),
            )
        });

        let mut body = Map::new();
        field(&mut body, "crimes", &crimes)?;
        field(&mut body, "jurisdictions", &jurisdictions)?;
        field(
            &mut body,
            "summary",
            &Summary {
                crimes: crimes.len(),
                jurisdictions: jurisdictions.len(),
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

    fn rec(ty: &str, path: &str, value: Value, short_name: &str) -> GameRecord {
        let mut v = json!({"_Type_": ty});
        if let (Some(o), Some(extra)) = (v.as_object_mut(), value.as_object()) {
            o.extend(extra.clone());
        }
        normalize_record(
            json!({"_RecordName_": format!("{ty}.{short_name}"), "_RecordId_": format!("id-{short_name}"), "_RecordValue_": v}),
            &format!("libs/foundry/records/{path}"),
        )
        .unwrap()
    }

    fn file_ref(path: &str) -> Value {
        json!(format!("file://./../../../../libs/foundry/records/{path}.json"))
    }

    // Cases from sc-gamedata src/domains/personal.test.ts ("law").
    #[test]
    fn drops_minus_one_and_reads_int_flags_as_booleans() {
        let p = set_parameters(Some(&json!({"_Type_": "InfractionParameters", "isFelony": 1, "lifetime": 168, "gracePeriod": -1, "hideCrimeInJournal": 0, "graceWarnings": []})));
        assert_eq!(
            Value::Object(p.unwrap()),
            json!({"isFelony": true, "lifetime": 168, "hideCrimeInJournal": false})
        );
        assert_eq!(set_parameters(Some(&json!({"isFelony": -1}))), None);
    }

    #[test]
    fn names_enforced_crimes_as_the_crime_list_does_and_keeps_impound_rules() {
        let set = RecordSet::new(vec![
            rec(
                "InfractionDefinition",
                "lawsystem/infractiondefinitions/felonies/murder",
                json!({"triggers": ["KillActor"], "defaultParameters": {"isFelony": 1, "felonyMerits": 5000, "lifetime": 168}}),
                "Murder",
            ),
            rec(
                "Jurisdiction",
                "lawsystem/jurisdictions/uee",
                json!({
                    "baseFine": 125,
                    "infractions": [{"definition": file_ref("lawsystem/infractiondefinitions/felonies/murder"), "parameterOverrides": {"lifetime": -1}}],
                    "prohibitedResources": [{"_RecordName_": "ResourceType.OsoianHides"}],
                    "controlledSubstanceClasses": [{"commodities": [file_ref("entities/commodities/vice/slam")], "resources": [], "maxPossessionSCU": 0.125}],
                    "impoundingDefinitions": [{"trigger": "PadRamming", "impoundingTimeSeconds": 3600, "impoundingFineUEC": 50000}]
                }),
                "UEE",
            ),
        ]);
        let body = DOMAIN
            .parse(&ParseContext::new(&set, &Localization::new(), &NoFiles))
            .unwrap();
        assert_eq!(body["crimes"][0]["category"], "felonies");
        assert_eq!(body["crimes"][0]["parameters"]["isFelony"], true);
        assert_eq!(body["crimes"][0]["parameters"]["felonyMerits"], 5000);
        let j = &body["jurisdictions"][0];
        assert_eq!(j["baseFine"], json!(125.0));
        assert_eq!(j["crimes"], json!([{"crime": "Murder"}]));
        assert_eq!(j["prohibitedResources"], json!(["OsoianHides"]));
        assert_eq!(
            j["controlledSubstanceClasses"],
            json!([{"commodities": ["slam"], "resources": [], "maxPossessionScu": 0.125}])
        );
        assert_eq!(
            j["impounding"],
            json!([{"trigger": "PadRamming", "seconds": 3600.0, "fine": 50000.0}])
        );
        assert_eq!(body["summary"], json!({"crimes": 1, "jurisdictions": 1}));
    }

    #[test]
    fn falls_back_to_the_comma_p_key() {
        let loc = parse_global_ini("crime_Murder,P=Homicide\njur_UEE=United Empire of Earth");
        let c = parse_crime(
            &rec(
                "InfractionDefinition",
                "lawsystem/infractiondefinitions/special/x",
                json!({"name": "@crime_Murder"}),
                "X",
            ),
            &loc,
        );
        assert_eq!(c.display_name.as_deref(), Some("Homicide"));
        assert_eq!(c.category, "special");
        let missing = parse_crime(
            &rec("InfractionDefinition", "elsewhere/x", json!({"name": "@nope"}), "X"),
            &loc,
        );
        assert_eq!(missing.display_name.as_deref(), Some("@nope"));
        // No "infractiondefinitions" segment: indexOf -1 + 1 reads the first one.
        assert_eq!(missing.category, "libs");
    }

    #[test]
    fn jurisdiction_shape_and_order() {
        let j = parse_jurisdiction(
            &rec(
                "Jurisdiction",
                "lawsystem/jurisdictions/stanton/arccorp",
                json!({"parentJurisdiction": file_ref("lawsystem/jurisdictions/uee"), "isPrison": false,
                       "infractions": [{"definition": file_ref("lawsystem/infractiondefinitions/x/unknown")}, {"definition": 3}],
                       "prohibitedGoods": [file_ref("entities/commodities/a"), "", 7]}),
                "ArcCorp",
            ),
            &Localization::new(),
            &HashMap::new(),
        );
        assert_eq!(j.parent.as_deref(), Some("uee"));
        assert_eq!(j.crimes, vec![EnforcedCrime { crime: "unknown".into(), overrides: None }]);
        assert_eq!(j.prohibited_goods, vec!["a".to_string()]);
        let row = serde_json::to_value(&j).unwrap();
        let keys: Vec<&str> = row.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            vec![
                "name",
                "recordName",
                "parent",
                "isPrison",
                "crimes",
                "prohibitedGoods",
                "prohibitedResources",
                "controlledSubstanceClasses",
                "impounding"
            ]
        );
    }
}
