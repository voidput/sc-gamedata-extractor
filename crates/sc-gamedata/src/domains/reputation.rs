//! Reputation standings: the ranks each faction and guild grants (port of
//! `sc-gamedata/src/domains/reputation.ts`).
//!
//! A `SReputationStandingParams` record is one rank, with `minReputation` as the threshold
//! to reach it. Ranks of one organisation share a record-name prefix, so they are grouped
//! by that and ordered by threshold. `driftReputation` and `driftTimeHours` describe decay
//! back toward neutral.

use crate::collate::locale_compare;
use crate::domain::{Domain, DomainError, ParseContext, field};
use crate::localization::{Localization, is_placeholder_name, localize};
use crate::records::{GameRecord, num};
use serde::Serialize;
use serde_json::{Map, Value};

pub static DOMAIN: Reputation = Reputation;

pub struct Reputation;

/// One rank. Field order is output order.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Standing {
    pub name: String,
    pub record_name: String,
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub organisation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_reputation: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drift_reputation: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drift_time_hours: Option<f64>,
    /// Only ever `true`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gated: Option<bool>,
}

#[derive(Debug, Serialize)]
struct Summary {
    standings: usize,
    organisations: usize,
}

/// `ReputationStanding_Guild_04` belongs to `Guild`: a leading `reputationstanding...`
/// part and a trailing rank number are dropped, the rest names the organisation.
/// As in the TypeScript, a name that is only a prefix gives `Some("")`.
pub fn organisation_of(short_name: &str) -> Option<String> {
    let mut parts: Vec<&str> = short_name.split('_').collect();
    if parts
        .first()
        .is_some_and(|p| p.to_lowercase().starts_with("reputationstanding"))
    {
        parts.remove(0);
    }
    if parts.is_empty() {
        return None;
    }
    if parts.len() > 1
        && parts
            .last()
            .is_some_and(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
    {
        parts.pop();
    }
    (!parts.is_empty()).then(|| parts.join("_"))
}

/// `num(x) || undefined`.
fn non_zero(value: Option<&Value>) -> Option<f64> {
    num(value).filter(|x| *x != 0.0)
}

pub fn parse_standing(record: &GameRecord, localization: &Localization) -> Standing {
    let v = &record.value;
    let display_name = localize(localization, v.get("displayName"));
    Standing {
        name: record.short_name.clone(),
        record_name: record.name.clone(),
        id: record.id.clone(),
        display_name: (!is_placeholder_name(display_name))
            .then(|| display_name.map(str::to_string))
            .flatten(),
        organisation: organisation_of(&record.short_name),
        min_reputation: num(v.get("minReputation")),
        drift_reputation: non_zero(v.get("driftReputation")),
        drift_time_hours: non_zero(v.get("driftTimeHours")),
        gated: (v.get("gated") == Some(&Value::Bool(true))).then_some(true),
    }
}

impl Domain for Reputation {
    fn name(&self) -> &'static str {
        "game-reputation"
    }

    fn description(&self) -> &'static str {
        "Faction and guild reputation ranks with their thresholds."
    }

    fn filters(&self) -> &'static [&'static str] {
        &["**/reputation/**"]
    }

    fn needs_localization(&self) -> bool {
        true
    }

    fn parse(&self, ctx: &ParseContext<'_>) -> Result<Map<String, Value>, DomainError> {
        let mut standings: Vec<Standing> = ctx
            .records
            .of_type("SReputationStandingParams")
            .map(|r| parse_standing(r, ctx.localization))
            .collect();

        // Ranks only make sense as an ordered progression per organisation. Grouped in a
        // JS object: insertion order, which the writer prints integer-like keys first of.
        let mut groups: Vec<(String, Vec<Standing>)> = Vec::new();
        for standing in &standings {
            let org = standing.organisation.as_deref().unwrap_or("unknown");
            match groups.iter_mut().find(|(k, _)| k == org) {
                Some((_, list)) => list.push(standing.clone()),
                None => groups.push((org.to_string(), vec![standing.clone()])),
            }
        }
        let mut by_organisation = Map::new();
        for (org, mut list) in groups {
            list.sort_by(|a, b| {
                let (x, y) = (a.min_reputation.unwrap_or(0.0), b.min_reputation.unwrap_or(0.0));
                x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal)
            });
            by_organisation.insert(org, serde_json::to_value(list)?);
        }

        standings.sort_by(|a, b| locale_compare(&a.name, &b.name));

        let mut body = Map::new();
        field(&mut body, "standings", &standings)?;
        let organisations = by_organisation.len();
        body.insert("byOrganisation".into(), Value::Object(by_organisation));
        field(
            &mut body,
            "summary",
            &Summary {
                standings: standings.len(),
                organisations,
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

    fn standing(short_name: &str, value: Value) -> GameRecord {
        let mut v = json!({"_Type_": "SReputationStandingParams"});
        if let (Some(o), Some(extra)) = (v.as_object_mut(), value.as_object()) {
            o.extend(extra.clone());
        }
        normalize_record(
            json!({"_RecordName_": format!("SReputationStandingParams.{short_name}"), "_RecordId_": format!("id-{short_name}"), "_RecordValue_": v}),
            &format!(
                "libs/foundry/records/reputation/standings/{}",
                short_name.to_lowercase()
            ),
        )
        .unwrap()
    }

    #[test]
    fn organisation_from_the_record_name() {
        assert_eq!(organisation_of("ReputationStanding_Guild_04").as_deref(), Some("Guild"));
        assert_eq!(
            organisation_of("ReputationStanding_Affinity_Ally_025").as_deref(),
            Some("Affinity_Ally")
        );
        assert_eq!(organisation_of("Bounty_Hunter").as_deref(), Some("Bounty_Hunter"));
        // A lone number is the organisation, not a rank.
        assert_eq!(organisation_of("ReputationStanding_12").as_deref(), Some("12"));
        assert_eq!(organisation_of("ReputationStanding").as_deref(), None);
        assert_eq!(organisation_of("ReputationStandingX_").as_deref(), Some(""));
    }

    #[test]
    fn parses_a_standing() {
        let loc = parse_global_ini("rep_Friendly=Friendly");
        let s = parse_standing(
            &standing(
                "ReputationStanding_Guild_04",
                json!({"displayName": "@rep_Friendly", "minReputation": 400, "driftReputation": 0, "driftTimeHours": 2.5, "gated": true}),
            ),
            &loc,
        );
        assert_eq!(s.display_name.as_deref(), Some("Friendly"));
        assert_eq!(s.organisation.as_deref(), Some("Guild"));
        assert_eq!(
            (s.min_reputation, s.drift_reputation, s.drift_time_hours, s.gated),
            (Some(400.0), None, Some(2.5), Some(true))
        );
        let none = parse_standing(&standing("X", json!({"displayName": "@missing", "gated": false})), &loc);
        assert_eq!((none.display_name, none.gated), (None, None));
    }

    // sc-gamedata src/domains/new-domains.test.ts ("reputation").
    #[test]
    fn orders_each_organisations_ranks_by_threshold() {
        let set = RecordSet::new(vec![
            standing("ReputationStanding_Guild_02", json!({"minReputation": 2051})),
            standing("ReputationStanding_Guild_01", json!({"minReputation": 0})),
        ]);
        let body = DOMAIN
            .parse(&ParseContext::new(&set, &Localization::new(), &NoFiles))
            .unwrap();
        let mins: Vec<f64> = body["byOrganisation"]["Guild"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["minReputation"].as_f64().unwrap())
            .collect();
        assert_eq!(mins, vec![0.0, 2051.0]);
    }

    #[test]
    fn groups_by_organisation_in_threshold_order() {
        let set = RecordSet::new(vec![
            standing("ReputationStanding_Guild_02", json!({"minReputation": 200})),
            standing("ReputationStanding_Guild_01", json!({"minReputation": 100})),
            standing("ReputationStanding_Other_01", json!({})),
            standing("ReputationStanding_Guild_00", json!({})),
        ]);
        let body = DOMAIN
            .parse(&ParseContext::new(&set, &Localization::new(), &NoFiles))
            .unwrap();
        let keys: Vec<_> = body.keys().cloned().collect();
        assert_eq!(keys, vec!["standings", "byOrganisation", "summary"]);
        let guild: Vec<&str> = body["byOrganisation"]["Guild"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            guild,
            vec![
                "ReputationStanding_Guild_00",
                "ReputationStanding_Guild_01",
                "ReputationStanding_Guild_02"
            ]
        );
        assert_eq!(body["standings"][0]["name"], "ReputationStanding_Guild_00");
        assert_eq!(body["summary"], json!({"standings": 4, "organisations": 2}));
    }
}
