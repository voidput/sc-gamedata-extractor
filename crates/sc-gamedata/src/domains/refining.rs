//! Refining: what the client knows about turning ore into refined material (port of
//! `sc-gamedata/src/domains/refining.ts`).
//!
//! The nine `RefiningProcess` records carry only a speed and quality label and a display
//! name; yields, costs, durations and station bonuses are server-side. What ships: which
//! raw resource refines into which (`refinedVersion` in the `ResourceTypeDatabase`), the
//! density of each, `CraftingGlobalParams.refiningQualityUnitMultiplier`, and the method
//! ratings the refinery screen shows, as text (`<processName>_Details,P`: "High Yield //
//! Low Cost // Slowest"). `refiningSpeed` tracks cost and `refiningQuality` yield; the real
//! speed appears only in the text.

use crate::collate::compare_names;
use crate::domain::{Domain, DomainError, ParseContext, field};
use crate::domains::commodities::{ResourceType, display_name_of, parse_resource_database};
use crate::helpers_world::js_trim;
use crate::localization::{Localization, lookup_string};
use crate::records::{GameRecord, RecordSet, array, num, ref_short_name, str_};
use serde::Serialize;
use serde_json::{Map, Value, json};
use std::collections::HashMap;

pub static DOMAIN: Refining = Refining;

pub struct Refining;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefiningProcess {
    pub name: String,
    pub record_name: String,
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// "Slow", "Normal" or "Fast". Despite the name, this tracks cost, not speed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed: Option<String>,
    /// "Wasteful", "Normal" or "Careful". Tracks yield.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The in-game rating, from the process's details string. Keys in the order the
    /// string names them (a JS object built by assignment), then the ranks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ratings: Option<Map<String, Value>>,
}

const LEVELS: [&str; 3] = ["Low", "Moderate", "High"];
const SPEEDS: [&str; 9] = [
    "Slowest",
    "Very Slow",
    "Slow",
    "Slightly Slow",
    "Moderate",
    "Slightly Fast",
    "Fast",
    "Very Fast",
    "Fastest",
];

fn rank(scale: &[&str], value: Option<&Value>) -> Option<usize> {
    let v = value?.as_str().filter(|s| !s.is_empty())?;
    scale.iter().position(|s| *s == v).map(|i| i + 1)
}

/// `/^(.+?) <suffix>$/`: a non-empty prefix without line terminators (`.`).
fn prefix_before<'a>(part: &'a str, suffix: &str) -> Option<&'a str> {
    let head = part.strip_suffix(suffix)?;
    (!head.is_empty() && !head.contains(['\n', '\r', '\u{2028}', '\u{2029}'])).then_some(head)
}

/// `parseRatings`: "High Yield // Low Cost // Slowest\n" -> yield/cost/speed and ranks.
pub fn parse_ratings(details: Option<&str>) -> Option<Map<String, Value>> {
    let details = details.filter(|d| !d.is_empty())?;
    let mut ratings = Map::new();
    for raw in details.replace("\\n", "").split("//") {
        let part = js_trim(raw);
        if let Some(y) = prefix_before(part, " Yield") {
            ratings.insert("yield".into(), json!(y));
        } else if let Some(c) = prefix_before(part, " Cost") {
            ratings.insert("cost".into(), json!(c));
        } else if !part.is_empty() {
            let speed = part.strip_suffix(" Speed").unwrap_or(part);
            ratings.insert("speed".into(), json!(speed));
        }
    }
    let yield_rank = rank(&LEVELS, ratings.get("yield"));
    let cost_rank = rank(&LEVELS, ratings.get("cost"));
    let speed_rank = rank(&SPEEDS, ratings.get("speed"));
    for (k, r) in [
        ("yieldRank", yield_rank),
        ("costRank", cost_rank),
        ("speedRank", speed_rank),
    ] {
        if let Some(r) = r {
            ratings.insert(k.into(), json!(r));
        }
    }
    Some(ratings)
}

pub fn parse_refining_process(record: &GameRecord, localization: &Localization) -> RefiningProcess {
    let v = &record.value;
    let key = str_(v.get("processName"));
    RefiningProcess {
        name: record.short_name.clone(),
        record_name: record.name.clone(),
        id: record.id.clone(),
        display_name: display_name_of(localization, v.get("processName")),
        speed: str_(v.get("refiningSpeed")).map(str::to_string),
        quality: str_(v.get("refiningQuality")).map(str::to_string),
        description: key.and_then(|k| {
            lookup_string(localization, &format!("{k}_Desc")).map(|d| js_trim(d).to_string())
        }),
        ratings: key.and_then(|k| parse_ratings(lookup_string(localization, &format!("{k}_Details,P")))),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Refinable {
    pub raw: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_display_name: Option<String>,
    pub refined: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refined_display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refined_description: Option<String>,
    pub group: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_density_grams_per_cc: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refined_density_grams_per_cc: Option<f64>,
}

#[derive(Debug, Serialize)]
struct WorkloadTier {
    level: &'static str,
    text: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Settings {
    #[serde(skip_serializing_if = "Option::is_none")]
    refining_quality_unit_multiplier: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    default_composition_quality: Option<f64>,
    /// Resources that dismantling an item never gives back.
    dismantle_blacklist_resources: Vec<String>,
}

fn order_index(order: &[&str], value: Option<&str>) -> i64 {
    let v = value.unwrap_or("");
    order
        .iter()
        .position(|s| *s == v)
        .map_or(-1, |i| i as i64)
}

impl Domain for Refining {
    fn name(&self) -> &'static str {
        "game-refining"
    }

    fn description(&self) -> &'static str {
        "Refining methods, raw-to-refined resource pairs, and refining quality settings."
    }

    fn filters(&self) -> &'static [&'static str] {
        &[
            "**/refiningprocess/**",
            "**/resourcetypedatabase/**",
            "**/crafting/globalparams/**",
        ]
    }

    fn needs_localization(&self) -> bool {
        true
    }

    fn parse(&self, ctx: &ParseContext<'_>) -> Result<Map<String, Value>, DomainError> {
        let loc = ctx.localization;
        let speed_order = ["Slow", "Normal", "Fast"];
        let quality_order = ["Wasteful", "Normal", "Careful"];
        let mut processes: Vec<RefiningProcess> = ctx
            .records
            .of_type("RefiningProcess")
            .map(|r| parse_refining_process(r, loc))
            .collect();
        processes.sort_by(|a, b| {
            order_index(&speed_order, a.speed.as_deref())
                .cmp(&order_index(&speed_order, b.speed.as_deref()))
                .then_with(|| {
                    order_index(&quality_order, a.quality.as_deref())
                        .cmp(&order_index(&quality_order, b.quality.as_deref()))
                })
                .then_with(|| compare_names(&a.name, &b.name))
        });

        // The TS calls `parseResourceDatabase(r, localization)` without a path index, so
        // quality references never resolve here (and are not output anyway).
        let no_records = RecordSet::new(Vec::new());
        let resources: Vec<ResourceType> = ctx
            .records
            .of_type("ResourceTypeDatabase")
            .flat_map(|r| parse_resource_database(r, loc, &no_records))
            .collect();
        let by_name: HashMap<&str, &ResourceType> =
            resources.iter().map(|r| (r.name.as_str(), r)).collect();
        let mut refinables: Vec<Refinable> = resources
            .iter()
            .filter_map(|r| {
                let to = r.refines_to.as_ref().filter(|t| !t.is_empty())?;
                let refined = by_name.get(to.as_str());
                Some(Refinable {
                    raw: r.name.clone(),
                    raw_display_name: r.display_name.clone(),
                    refined: to.clone(),
                    refined_display_name: refined.and_then(|x| x.display_name.clone()),
                    raw_description: r.description.clone(),
                    refined_description: refined.and_then(|x| x.description.clone()),
                    group: r.group.clone(),
                    raw_density_grams_per_cc: r.density_grams_per_cc,
                    refined_density_grams_per_cc: refined.and_then(|x| x.density_grams_per_cc),
                })
            })
            .collect();
        refinables.sort_by(|a, b| {
            compare_names(
                a.raw_display_name.as_deref().unwrap_or(&a.raw),
                b.raw_display_name.as_deref().unwrap_or(&b.raw),
            )
        });

        let params = ctx
            .records
            .iter()
            .find(|r| r.ty == "CraftingGlobalParams")
            .map(|r| &r.value);
        let dismantle: Vec<String> = array(params.and_then(|p| p.get("dismantleBlacklistResources")))
            .iter()
            .filter_map(|v| ref_short_name(Some(v)).filter(|s| !s.is_empty()))
            .map(str::to_string)
            .collect();

        // Surcharge text by workload, as the refinery kiosk words it.
        let workload: Vec<WorkloadTier> = ["Low", "Medium", "High", "Extreme"]
            .into_iter()
            .filter_map(|level| {
                let text = lookup_string(loc, &format!("refinery_ui_Capacity_{level}"))
                    .filter(|t| !t.is_empty())?;
                Some(WorkloadTier {
                    level,
                    text: text.to_string(),
                })
            })
            .collect();

        let rated = processes.iter().filter(|p| p.ratings.is_some()).count();
        let mut body = Map::new();
        field(&mut body, "processes", &processes)?;
        field(&mut body, "refinables", &refinables)?;
        field(&mut body, "workloadTiers", &workload)?;
        field(
            &mut body,
            "settings",
            &Settings {
                refining_quality_unit_multiplier: num(
                    params.and_then(|p| p.get("refiningQualityUnitMultiplier")),
                ),
                default_composition_quality: num(
                    params.and_then(|p| p.get("defaultCompositionQuality")),
                ),
                dismantle_blacklist_resources: dismantle,
            },
        )?;
        body.insert(
            "summary".into(),
            json!({"processes": processes.len(), "rated": rated, "refinables": refinables.len()}),
        );
        Ok(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::NoFiles;
    use crate::domains::commodities::tests::{database, rec, ref_, strings};

    fn process(speed: &str, quality: &str) -> GameRecord {
        rec(
            "RefiningProcess",
            &format!("ProcessingType-{speed}{quality}"),
            json!({"refiningSpeed": speed, "refiningQuality": quality,
                   "processName": format!("@refinery_ui_ProcessingType_{speed}{quality}")}),
            None,
        )
    }

    // Cases from sc-gamedata src/domains/economy.test.ts.
    #[test]
    fn orders_methods_and_pairs_raw_with_refined() {
        let set = RecordSet::new(vec![
            process("Fast", "Wasteful"),
            process("Slow", "Careful"),
            process("Slow", "Wasteful"),
            database(),
            rec(
                "CraftingGlobalParams",
                "CraftingGlobalParams",
                json!({"refiningQualityUnitMultiplier": 2,
                       "dismantleBlacklistResources": [ref_("ResourceType.Quantanium")]}),
                None,
            ),
        ]);
        let loc = strings();
        let out = DOMAIN.parse(&ParseContext::new(&set, &loc, &NoFiles)).unwrap();
        let names: Vec<_> = out["processes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["name"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(
            names,
            vec![
                "ProcessingType-SlowWasteful",
                "ProcessingType-SlowCareful",
                "ProcessingType-FastWasteful"
            ]
        );
        assert_eq!(out["processes"][1]["displayName"], "Dinyx Solventation");
        assert_eq!(
            out["refinables"],
            json!([{"raw": "Ore_Gold", "rawDisplayName": "Gold (Ore)", "refined": "Gold",
                    "refinedDisplayName": "Gold", "group": "Metal",
                    "rawDensityGramsPerCc": 19.3, "refinedDensityGramsPerCc": 1.0}])
        );
        assert_eq!(
            out["settings"],
            json!({"refiningQualityUnitMultiplier": 2.0, "dismantleBlacklistResources": ["Quantanium"]})
        );
    }

    #[test]
    fn ratings() {
        assert_eq!(
            Value::Object(parse_ratings(Some("High Yield // Low Cost // Slowest")).unwrap()),
            json!({"yield": "High", "cost": "Low", "speed": "Slowest",
                   "yieldRank": 3, "costRank": 1, "speedRank": 1})
        );
        let r = parse_ratings(Some("Moderate Yield // Moderate Cost // Moderate Speed\\n")).unwrap();
        assert_eq!(r["speed"], "Moderate");
        assert_eq!(r["speedRank"], 5);
        assert_eq!(parse_ratings(Some("")), None);
        // Keys follow the string's order, as JS assignment does.
        let r = parse_ratings(Some("Fast // Low Cost")).unwrap();
        assert_eq!(r.keys().collect::<Vec<_>>(), vec!["speed", "cost", "costRank", "speedRank"]);
    }

    #[test]
    fn attaches_ratings_description_and_workload_tiers() {
        let set = RecordSet::new(vec![rec(
            "RefiningProcess",
            "ProcessingType-NormalWasteful",
            json!({"refiningSpeed": "Normal", "refiningQuality": "Wasteful",
                   "processName": "@refinery_ui_ProcessingType_NormalWasteful"}),
            None,
        )]);
        let mut loc = Localization::new();
        loc.set("refinery_ui_ProcessingType_NormalWasteful", "Cormack Method");
        loc.set("refinery_ui_ProcessingType_NormalWasteful_Desc", "Quick, low yield. ");
        loc.set(
            "refinery_ui_ProcessingType_NormalWasteful_Details,P",
            "Low Yield // Moderate Cost // Very Fast",
        );
        loc.set("refinery_ui_Capacity_Low", "No surcharge at this time.");
        let out = DOMAIN.parse(&ParseContext::new(&set, &loc, &NoFiles)).unwrap();
        let p = &out["processes"][0];
        assert_eq!(p["speed"], "Normal");
        assert_eq!(p["description"], "Quick, low yield.");
        assert_eq!(p["ratings"]["yield"], "Low");
        assert_eq!(p["ratings"]["cost"], "Moderate");
        assert_eq!(p["ratings"]["speed"], "Very Fast");
        assert_eq!(p["ratings"]["speedRank"], 8);
        assert_eq!(
            out["workloadTiers"],
            json!([{"level": "Low", "text": "No surcharge at this time."}])
        );
    }
}
