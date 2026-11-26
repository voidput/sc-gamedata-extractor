//! Mission broker entries: the contracts offered through the mobiGlas (port of
//! `sc-gamedata/src/domains/missions.ts`).
//!
//! A `MissionBrokerEntry` carries its title, giver and description as localization keys,
//! its payout in `missionReward`, and reputation changes per outcome in
//! `missionResultReputationRewards`.
//!
//! Many entries are unfinished: `notForRelease` marks developer and test content, which is
//! kept but flagged rather than dropped.

use crate::collate::locale_compare;
use crate::domain::{Domain, DomainError, ParseContext, field};
use crate::helpers_gameplay::{add_count, by_count_desc, file_ref_stem, ref_short};
use crate::localization::{
    Localization, NameStatus, is_placeholder_name, loc_key, localize, name_status_field,
};
use crate::records::{GameRecord, at, num, str_};
use serde::Serialize;
use serde_json::{Map, Value};

pub static DOMAIN: Missions = Missions;

pub struct Missions;

/// One row of `missions`. Field order is output order.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Mission {
    pub name: String,
    pub record_name: String,
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub giver: Option<String>,
    /// String-table keys, as the table spells them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub giver_key: Option<String>,
    /// The title's; absent when it is real.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name_status: Option<NameStatus>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub mission_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub difficulty: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reward: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reward_max: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buy_in: Option<f64>,
    /// Developer or test content not offered in the live game (only ever `true`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub not_for_release: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lawful: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Summary {
    missions: usize,
    live: usize,
    not_for_release: usize,
    with_payout: usize,
    median_reward: f64,
    max_reward: f64,
}

/// `/^[0-9a-f-]{36}$/i`: a bare GUID, which is what a tag with no name exports as.
fn is_guid(name: &str) -> bool {
    name.len() == 36 && name.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
}

fn tag_names(value: Option<&Value>) -> Option<Vec<String>> {
    let tags = value?.get("tags")?.as_array()?;
    let names: Vec<String> = tags
        .iter()
        .filter_map(|t| ref_short(Some(t)))
        .filter(|n| !is_guid(n))
        .map(str::to_string)
        .collect();
    (!names.is_empty()).then_some(names)
}

/// `num(x) || undefined`: zero (and -0) count as unset.
fn non_zero(value: Option<&Value>) -> Option<f64> {
    num(value).filter(|x| *x != 0.0)
}

pub fn parse_mission(record: &GameRecord, localization: &Localization) -> Mission {
    let v = &record.value;
    let text = |key: &str| localize(localization, v.get(key));
    let key = |field: &str| loc_key(localization, v.get(field)).map(str::to_string);
    let title = text("title");
    let giver = text("missionGiver");
    let description = text("description");
    let real = |s: Option<&str>| (!is_placeholder_name(s)).then(|| s.map(str::to_string)).flatten();
    Mission {
        // Titles are frequently placeholders on unfinished entries.
        name: real(title).unwrap_or_else(|| record.short_name.clone()),
        record_name: record.name.clone(),
        id: record.id.clone(),
        title: real(title),
        description: real(description),
        giver: real(giver),
        title_key: key("title"),
        description_key: key("description"),
        giver_key: key("missionGiver"),
        name_status: name_status_field(localization, v.get("title")),
        mission_type: file_ref_stem(v.get("type")).map(str::to_string),
        // -1 means unset rather than "easiest".
        difficulty: num(v.get("missionDifficulty")).filter(|d| *d >= 0.0),
        reward: non_zero(at(v, "missionReward.reward")),
        reward_max: non_zero(at(v, "missionReward.max")),
        currency: str_(at(v, "missionReward.currencyType")).map(str::to_string),
        buy_in: non_zero(v.get("missionBuyInAmount")),
        not_for_release: (v.get("notForRelease") == Some(&Value::Bool(true))).then_some(true),
        lawful: v.get("lawfulMission").and_then(Value::as_bool),
        tags: tag_names(v.get("missionTags")),
    }
}

impl Domain for Missions {
    fn name(&self) -> &'static str {
        "game-missions"
    }
    fn schema_version(&self) -> u32 {
        1
    }

    fn description(&self) -> &'static str {
        "Mission broker contracts with payouts, givers and difficulty."
    }

    fn filters(&self) -> &'static [&'static str] {
        &["**/missionbroker/**"]
    }

    fn needs_localization(&self) -> bool {
        true
    }

    fn parse(&self, ctx: &ParseContext<'_>) -> Result<Map<String, Value>, DomainError> {
        let mut missions: Vec<Mission> = ctx
            .records
            .of_type("MissionBrokerEntry")
            .map(|r| parse_mission(r, ctx.localization))
            .collect();
        missions.sort_by(|a, b| locale_compare(&a.name, &b.name));

        let live: Vec<&Mission> = missions.iter().filter(|m| m.not_for_release.is_none()).collect();
        let paying: Vec<&Mission> = live
            .iter()
            .copied()
            .filter(|m| m.reward.unwrap_or(0.0) > 0.0)
            .collect();

        let mut by_giver = Map::new();
        for mission in &live {
            if let Some(giver) = mission.giver.as_deref().filter(|g| !g.is_empty()) {
                add_count(&mut by_giver, giver, 1.0);
            }
        }

        let mut rewards: Vec<f64> = paying.iter().filter_map(|m| m.reward).collect();
        rewards.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let mut body = Map::new();
        field(&mut body, "missions", &missions)?;
        body.insert("byGiver".into(), Value::Object(by_count_desc(&by_giver)));
        field(
            &mut body,
            "summary",
            &Summary {
                missions: missions.len(),
                live: live.len(),
                not_for_release: missions.len() - live.len(),
                with_payout: paying.len(),
                // A median says more about typical pay than an average skewed by outliers.
                median_reward: rewards.get(rewards.len() / 2).copied().unwrap_or(0.0),
                max_reward: rewards.last().copied().unwrap_or(0.0),
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

    fn localization() -> Localization {
        parse_global_ini("Heist_Title=Data Heist\nHeist_Giver=Ruto\nHeist_Desc=Steal the data.")
    }

    fn mission(short_name: &str, value: Value) -> GameRecord {
        let mut v = json!({"_Type_": "MissionBrokerEntry"});
        if let (Some(o), Some(extra)) = (v.as_object_mut(), value.as_object()) {
            o.extend(extra.clone());
        }
        normalize_record(
            json!({"_RecordName_": format!("MissionBrokerEntry.{short_name}"), "_RecordId_": format!("id-{short_name}"), "_RecordValue_": v}),
            &format!(
                "libs/foundry/records/missionbroker/pu_missions/{}",
                short_name.to_lowercase()
            ),
        )
        .unwrap()
    }

    fn parse(records: Vec<GameRecord>) -> Map<String, Value> {
        let set = RecordSet::new(records);
        DOMAIN
            .parse(&ParseContext::new(&set, &localization(), &NoFiles))
            .unwrap()
    }

    // Cases from sc-gamedata src/domains/missions.test.ts.
    #[test]
    fn resolves_the_localized_title_giver_and_description() {
        let m = parse_mission(
            &mission(
                "Heist",
                json!({"title": "@Heist_Title", "missionGiver": "@Heist_Giver", "description": "@Heist_Desc",
                       "missionReward": {"reward": 500000, "currencyType": "UEC"}}),
            ),
            &localization(),
        );
        assert_eq!(m.name, "Data Heist");
        assert_eq!(m.giver.as_deref(), Some("Ruto"));
        assert_eq!(m.description.as_deref(), Some("Steal the data."));
        assert_eq!(m.reward, Some(500000.0));
        assert_eq!(m.currency.as_deref(), Some("UEC"));
    }

    #[test]
    fn falls_back_to_the_record_name_for_spaced_placeholder_titles() {
        let m = parse_mission(
            &mission("LootGenerationTestMissionPU", json!({"title": "< = TEST NAME = >"})),
            &localization(),
        );
        assert_eq!(m.name, "LootGenerationTestMissionPU");
        assert_eq!(m.title, None);
    }

    #[test]
    fn treats_difficulty_minus_one_as_unset_rather_than_easiest() {
        let d = |x: i64| parse_mission(&mission("X", json!({"missionDifficulty": x})), &localization()).difficulty;
        assert_eq!(d(-1), None);
        assert_eq!(d(0), Some(0.0));
    }

    #[test]
    fn flags_developer_content_instead_of_dropping_it() {
        let f = |x: bool| parse_mission(&mission("X", json!({"notForRelease": x})), &localization()).not_for_release;
        assert_eq!(f(true), Some(true));
        assert_eq!(f(false), None);
    }

    #[test]
    fn type_tags_and_zero_amounts() {
        let m = parse_mission(
            &mission(
                "X",
                json!({"type": "file://./../missiontype/pu/race.json", "missionBuyInAmount": 0,
                       "missionReward": {"reward": 0, "max": 0},
                       "missionTags": {"tags": [{"_RecordName_": "Tag.Combat"}, {"_RecordName_": "Tag.0b9c061e-46f9-4fea-b375-105e3f097221"}, "x"]},
                       "lawfulMission": false}),
            ),
            &localization(),
        );
        assert_eq!(m.mission_type.as_deref(), Some("race"));
        assert_eq!((m.reward, m.reward_max, m.buy_in), (None, None, None));
        assert_eq!(m.tags, Some(vec!["Combat".to_string()]));
        assert_eq!(m.lawful, Some(false));
        let row = serde_json::to_value(&m).unwrap();
        let keys: Vec<&str> = row.as_object().unwrap().keys().map(String::as_str).collect();
        // No title: the name status says so.
        assert_eq!(
            keys,
            vec!["name", "recordName", "id", "nameStatus", "type", "lawful", "tags"]
        );
        assert_eq!(row["nameStatus"], "empty");
    }

    fn records() -> Vec<GameRecord> {
        vec![
            mission(
                "Live1",
                json!({"title": "@Heist_Title", "missionGiver": "@Heist_Giver", "missionReward": {"reward": 500000, "currencyType": "UEC"}}),
            ),
            mission(
                "Live2",
                json!({"missionGiver": "@Heist_Giver", "missionReward": {"reward": 10000, "currencyType": "UEC"}}),
            ),
            mission("DevOnly", json!({"notForRelease": true, "missionReward": {"reward": 999999}})),
        ]
    }

    #[test]
    fn separates_live_missions_from_developer_content() {
        let body = parse(records());
        assert_eq!(body["summary"]["missions"], 3);
        assert_eq!(body["summary"]["live"], 2);
        assert_eq!(body["summary"]["notForRelease"], 1);
        assert_eq!(body["summary"]["withPayout"], 2);
    }

    #[test]
    fn excludes_developer_content_from_the_payout_stats() {
        let body = parse(records());
        assert_eq!(body["summary"]["maxReward"], json!(500000.0));
        // [10000, 500000][floor(2 / 2)]
        assert_eq!(body["summary"]["medianReward"], json!(500000.0));
    }

    #[test]
    fn counts_live_missions_per_giver() {
        let body = parse(records());
        assert_eq!(body["byGiver"]["Ruto"], json!(2.0));
        let keys: Vec<_> = body.keys().cloned().collect();
        assert_eq!(keys, vec!["missions", "byGiver", "summary"]);
        // Sorted by name: "Data Heist" < "DevOnly" < "Live2".
        assert_eq!(body["missions"][0]["name"], "Data Heist");
        assert_eq!(body["missions"][2]["name"], "Live2");
    }
}
