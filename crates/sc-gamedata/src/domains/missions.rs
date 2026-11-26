//! Mission broker entries: the contracts offered through the mobiGlas (port of
//! `sc-gamedata/src/domains/missions.ts`).
//!
//! A `MissionBrokerEntry` carries its title, giver and description as localization keys,
//! its payout in `missionReward`, and reputation changes per outcome in
//! `missionResultReputationRewards`.
//!
//! Many entries are unfinished: `notForRelease` marks developer and test content, which is
//! kept but flagged rather than dropped.
//!
//! Joins (spec `cross-dataset-derivations.md` D2, `dump-enhancements.md` item 9), each
//! read from the mission record and resolved through records of the same extract:
//! reputation needed (`requiresSet`, standings as `game-reputation` names them) and gained
//! (`repRewardsSet`), the crime-stat range (`wantedLevel`), where it is offered
//! (`offeredAt`, `locality`, with `places{}` giving each place's system and jurisdiction
//! as `game-starmap` / `game-law` name them), the giver record (`giverRecord`, named in
//! `givers{}`, with the faction it names), mission chains (`chain`),
//! partial payouts, the subsumption module, and the items a mission spawns or hauls (with
//! the reverse `usedByMissions{}`).
//!
//! Values that repeat across thousands of missions are listed once and pointed at by
//! index (`requiresSets[]`, `repRewardsSets[]`, `partialPayouts[]`), as
//! `game-item-obtain` does with its spawns and tables.

use crate::collate::locale_compare;
use crate::domain::{Domain, DomainError, ParseContext, field};
use crate::domains::starmap::{Location, build_locations};
use crate::helpers_gameplay::{add_count, by_count_desc, file_ref_stem, ref_short};
use crate::localization::{
    Localization, NameStatus, is_placeholder_name, loc_key, localize, name_status_field,
};
use crate::records::{GameRecord, RecordSet, array, at, num, str_};
use serde::Serialize;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, HashMap};

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
    // ---- joins (all optional, appended so existing fields keep their order)
    /// `missionModule` below `libs/subsumption/missions/`, `.xml` removed:
    /// `PU/PU_Missions/DeliveryMission/PU_MultiStopDelivery`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
    /// `missionGiverRecord`: the `MissionGiver` record name (see `givers{}`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub giver_record: Option<String>,
    /// Index into `requiresSets[]`: the `reputationRequirements.expression` terms, in
    /// record order. Missions share few distinct sets, so each is listed once.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requires_set: Option<usize>,
    /// Index into `repRewardsSets[]`: the five `missionResultReputationRewards` lists in
    /// record order (see `_assumptions`); present when any list is non-empty.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rep_rewards_set: Option<usize>,
    /// `reputationPrerequisites.wantedLevel`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wanted_level: Option<WantedLevel>,
    /// `locationMissionAvailable` as `StarMapObject` record names (see `places{}`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offered_at: Option<Vec<String>>,
    /// `localityAvailable`: a `MissionLocality` short name (see `localities{}`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locality: Option<String>,
    /// `requiredMissions` / `associatedMissions` as mission record names.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chain: Option<Chain>,
    /// Index into `partialPayouts[]` (`partialRewardPayout.percentageThresholds`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub partial_payout: Option<usize>,
    /// Items put into inventories by `properties[]..autoSpawnSettings.inventoryItems`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spawns_items: Option<Vec<SpawnedItem>>,
    /// What the objectives' `haulingOrders[]` move.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hauling_items: Option<Vec<HauledItem>>,
}

/// One reputation term a mission needs.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RepRequirement {
    /// `FactionReputation` short name (see `factions{}`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub faction: Option<String>,
    /// The scope record's `scopeName`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    /// `GreaterThanOrEqualTo`, `GreaterThan`, `EqualTo`, `LessThanOrEqualTo`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comparison: Option<String>,
    /// Standing record name, as `game-reputation.standings[].recordName`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub standing: Option<String>,
}

/// One reputation change (`SReputationAmountParams`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RepAmount {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub faction: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    /// `reputationAmount` of the `reward` record.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct WantedLevel {
    pub min: f64,
    pub max: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Chain {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub requires: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub associated: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SpawnedItem {
    /// Entity class file stem (lower case), as `game-wikelo-trades` names items.
    pub item: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quantity: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HauledItem {
    /// `HaulingOrder_Resource`: `ResourceType` short name, as `game-commodities` `name`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource: Option<String>,
    /// `HaulingOrder_EntityClass`: entity class file stem.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item: Option<String>,
    /// `minSCU` / `maxSCU` as the record gives them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_scu: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_scu: Option<f64>,
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
    /// Missions carrying each join.
    links: Map<String, Value>,
}

/// What the joins rest on that the records do not state.
pub const ASSUMPTIONS: &[&str] = &[
    "repRewards: the five lists of missionResultReputationRewards, in record order; which mission outcome each slot is for is not stated in the records.",
    "places{}.jurisdictionEstimate: the jurisdiction of the nearest starmap ancestor that states one; the records do not state that a jurisdiction is inherited down the hierarchy. jurisdiction (no suffix) is stated on the place itself.",
];

/// Missions listed per item or resource in `usedByMissions{}`; the rest are counted.
pub const MAX_USED_BY: usize = 20;

/// Expression operators that leave `requires` a plain conjunction.
const AND_OPERATORS: &[&str] = &[
    "SReputationMissionRequirementExpression_And",
    "SReputationMissionRequirementExpression_LeftParenthesis",
    "SReputationMissionRequirementExpression_RightParenthesis",
];

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
        module: module_of(str_(v.get("missionModule"))),
        giver_record: None,
        requires_set: None,
        rep_rewards_set: None,
        wanted_level: wanted_level(v),
        offered_at: None,
        locality: None,
        chain: None,
        partial_payout: None,
        spawns_items: spawned_items(v),
        hauling_items: hauled_items(v),
    }
}

/// `missionModule` without the `libs/subsumption/missions/` folder (any case) and `.xml`.
fn module_of(path: Option<&str>) -> Option<String> {
    let path = path?.replace('\\', "/");
    const ROOT: &str = "libs/subsumption/missions/";
    let rest = match path.get(..ROOT.len()) {
        Some(head) if head.eq_ignore_ascii_case(ROOT) => &path[ROOT.len()..],
        _ => path.as_str(),
    };
    let cut = rest.len().saturating_sub(4);
    let rest = match rest.get(cut..) {
        Some(ext) if rest.len() >= 4 && ext.eq_ignore_ascii_case(".xml") => &rest[..cut],
        _ => rest,
    };
    (!rest.is_empty()).then(|| rest.to_string())
}

fn wanted_level(v: &Value) -> Option<WantedLevel> {
    let range = at(v, "reputationPrerequisites.wantedLevel")?;
    Some(WantedLevel {
        min: num(range.get("minValue"))?,
        max: num(range.get("maxValue"))?,
    })
}

/// Visit every object below `v` (depth first, `v` included).
fn walk<'v>(v: &'v Value, f: &mut impl FnMut(&'v Map<String, Value>)) {
    match v {
        Value::Object(o) => {
            f(o);
            for x in o.values() {
                walk(x, f);
            }
        }
        Value::Array(a) => {
            for x in a {
                walk(x, f);
            }
        }
        _ => {}
    }
}

fn push_unique<T: PartialEq>(list: &mut Vec<T>, item: T) {
    if !list.contains(&item) {
        list.push(item);
    }
}

/// `properties[]` spawn descriptions' `autoSpawnSettings.inventoryItems`, distinct, in
/// record order.
fn spawned_items(v: &Value) -> Option<Vec<SpawnedItem>> {
    let mut out = Vec::new();
    for prop in array(v.get("properties")) {
        walk(prop, &mut |node| {
            let Some(auto) = node.get("autoSpawnSettings") else {
                return;
            };
            for entry in array(auto.get("inventoryItems")) {
                if let Some(item) = file_ref_stem(entry.get("entityClass")) {
                    push_unique(
                        &mut out,
                        SpawnedItem {
                            item: item.to_lowercase(),
                            quantity: num(entry.get("quantity")),
                        },
                    );
                }
            }
        });
    }
    (!out.is_empty()).then_some(out)
}

/// Every `HaulingOrder_Resource` / `HaulingOrder_EntityClass` in the record, distinct.
fn hauled_items(v: &Value) -> Option<Vec<HauledItem>> {
    let mut out = Vec::new();
    walk(v, &mut |node| {
        let (resource, item) = match node.get("_Type_").and_then(Value::as_str) {
            Some("HaulingOrder_Resource") => (ref_short(node.get("resource")), None),
            Some("HaulingOrder_EntityClass") => (None, file_ref_stem(node.get("entityClass"))),
            _ => return,
        };
        if resource.is_none() && item.is_none() {
            return;
        }
        push_unique(
            &mut out,
            HauledItem {
                resource: resource.map(str::to_string),
                item: item.map(str::to_lowercase),
                min_scu: num(node.get("minSCU")),
                max_scu: num(node.get("maxSCU")),
            },
        );
    });
    (!out.is_empty()).then_some(out)
}

/// `FactionReputation` short name of a reference, else its file stem.
pub fn faction_of(records: &RecordSet, reference: Option<&Value>) -> Option<String> {
    records
        .follow(reference)
        .map(|r| r.short_name.clone())
        .or_else(|| file_ref_stem(reference).map(str::to_string))
}

/// A reputation scope's `scopeName`, else its short name, else the file stem.
pub fn scope_of(records: &RecordSet, reference: Option<&Value>) -> Option<String> {
    match records.follow(reference) {
        Some(r) => Some(
            str_(r.value.get("scopeName"))
                .map_or_else(|| r.short_name.clone(), str::to_string),
        ),
        None => file_ref_stem(reference).map(str::to_string),
    }
}

/// One `SReputationAmountParams`: faction, scope and the `reward` record's
/// `reputationAmount`.
pub fn rep_amount(records: &RecordSet, params: &Value) -> RepAmount {
    RepAmount {
        faction: faction_of(records, params.get("factionReputation")),
        scope: scope_of(records, params.get("reputationScope")),
        amount: records
            .follow(params.get("reward"))
            .and_then(|r| num(r.value.get("reputationAmount"))),
    }
}

/// A record's `displayName`, localized, when it is a real name; with its key.
fn display_name(record: &GameRecord, localization: &Localization) -> Option<(String, Option<String>)> {
    let text = localize(localization, record.value.get("displayName"));
    if is_placeholder_name(text) || text.is_some_and(|t| t.starts_with("~mission(")) {
        return None;
    }
    Some((
        text?.to_string(),
        loc_key(localization, record.value.get("displayName")).map(str::to_string),
    ))
}

/// What the joins resolve against, and the tables they fill.
struct Joins<'a> {
    records: &'a RecordSet,
    localization: &'a Localization,
    locations: HashMap<String, Location>,
    factions: BTreeMap<String, Value>,
    places: BTreeMap<String, Value>,
    localities: BTreeMap<String, Vec<String>>,
    givers: BTreeMap<String, Value>,
    payouts: Vec<Value>,
    require_sets: Vec<Value>,
    reward_sets: Vec<Value>,
    /// References that did not resolve, by field.
    unresolved: BTreeMap<&'static str, usize>,
    /// Requirement operators other than And and parentheses.
    other_operators: BTreeMap<String, usize>,
}

impl<'a> Joins<'a> {
    fn new(records: &'a RecordSet, localization: &'a Localization) -> Self {
        let locations = build_locations(
            records,
            localization,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        )
            .into_iter()
            .map(|l| (l.record_name.clone(), l))
            .collect();
        Self {
            records,
            localization,
            locations,
            factions: BTreeMap::new(),
            places: BTreeMap::new(),
            localities: BTreeMap::new(),
            givers: BTreeMap::new(),
            payouts: Vec::new(),
            require_sets: Vec::new(),
            reward_sets: Vec::new(),
            unresolved: BTreeMap::new(),
            other_operators: BTreeMap::new(),
        }
    }

    fn miss(&mut self, what: &'static str) {
        *self.unresolved.entry(what).or_default() += 1;
    }

    /// A faction reference, its name noted in `factions{}`.
    fn faction(&mut self, reference: Option<&Value>) -> Option<String> {
        if let Some(r) = self.records.follow(reference) {
            self.factions.entry(r.short_name.clone()).or_insert_with(|| {
                let mut row = Map::new();
                if let Some((name, key)) = display_name(r, self.localization) {
                    row.insert("name".into(), json!(name));
                    if let Some(k) = key {
                        row.insert("nameKey".into(), json!(k));
                    }
                }
                Value::Object(row)
            });
        } else if reference.is_some_and(|r| !r.is_null()) {
            self.miss("faction");
        }
        faction_of(self.records, reference)
    }

    /// Counts a scope reference that is set but names no record in the extract.
    fn scope(&mut self, reference: Option<&Value>) -> Option<String> {
        if reference.is_some_and(|r| !r.is_null()) && self.records.follow(reference).is_none() {
            self.miss("reputation scope");
        }
        scope_of(self.records, reference)
    }

    fn amount(&mut self, params: &Value) -> RepAmount {
        self.faction(params.get("factionReputation"));
        self.scope(params.get("reputationScope"));
        let amount = rep_amount(self.records, params);
        // A null `reward` (24 on LIVE 4.10.201) names no amount; only a dangling one counts.
        if amount.amount.is_none() && params.get("reward").is_some_and(|r| !r.is_null()) {
            self.miss("reputation reward");
        }
        amount
    }

    /// A starmap reference as its record name, the place noted in `places{}`.
    fn place(&mut self, reference: Option<&Value>) -> Option<String> {
        let Some(record) = self
            .records
            .follow(reference)
            .filter(|r| r.ty == "StarMapObject")
        else {
            self.miss("place");
            return None;
        };
        let name = record.name.clone();
        if !self.places.contains_key(&name) {
            let row = self.place_row(&name);
            self.places.insert(name.clone(), row);
        }
        Some(name)
    }

    fn place_row(&self, record_name: &str) -> Value {
        let mut row = Map::new();
        let Some(l) = self.locations.get(record_name) else {
            return Value::Object(row);
        };
        row.insert("name".into(), json!(l.name));
        if let Some(s) = &l.system {
            row.insert("system".into(), json!(s));
        }
        if let Some(j) = &l.jurisdiction {
            row.insert("jurisdiction".into(), json!(j));
        } else {
            // Nearest ancestor stating one (guarded against a cycle).
            let mut parent = l.parent.as_deref();
            for _ in 0..64 {
                let Some(p) = parent.and_then(|p| self.locations.get(p)) else {
                    break;
                };
                if let Some(j) = &p.jurisdiction {
                    row.insert("jurisdictionEstimate".into(), json!(j));
                    break;
                }
                parent = p.parent.as_deref();
            }
        }
        Value::Object(row)
    }

    fn link(&mut self, m: &mut Mission, record: &GameRecord) {
        let v = &record.value;
        let records = self.records;

        if let Some(giver) = v.get("missionGiverRecord").filter(|g| !g.is_null()) {
            match records.follow(Some(giver)) {
                Some(g) => {
                    m.giver_record = Some(g.name.clone());
                    if g.value.get("reputation").is_some_and(|f| !f.is_null()) {
                        self.faction(g.value.get("reputation"));
                    }
                    let localization = self.localization;
                    self.givers.entry(g.name.clone()).or_insert_with(|| {
                        // Only the giver record's own `displayName` names it. The faction
                        // its `reputation` field points at is listed beside it, not
                        // borrowed as its name: no record says the two are named alike
                        // (BlacJac's bounty department points at ArcCorp).
                        let mut row = Map::new();
                        if let Some((name, key)) = display_name(g, localization) {
                            row.insert("name".into(), json!(name));
                            if let Some(k) = key {
                                row.insert("nameKey".into(), json!(k));
                            }
                        }
                        if let Some(f) = records.follow(g.value.get("reputation")) {
                            row.insert("faction".into(), json!(f.short_name));
                        }
                        Value::Object(row)
                    });
                }
                None => self.miss("giver"),
            }
        }

        if let Some(rr) = v.get("reputationRequirements").filter(|r| r.is_object()) {
            let mut terms = Vec::new();
            for e in array(rr.get("expression")) {
                match e.get("_Type_").and_then(Value::as_str) {
                    Some("SReputationMissionGiverRequirementParams") => {
                        let standing = records.follow(e.get("standing")).map(|r| r.name.clone());
                        if standing.is_none() {
                            self.miss("standing");
                        }
                        terms.push(RepRequirement {
                            faction: self.faction(e.get("factionReputation")),
                            scope: self.scope(e.get("reputationScope")),
                            comparison: str_(e.get("comparison")).map(str::to_string),
                            standing,
                        });
                    }
                    Some(op) if AND_OPERATORS.contains(&op) => {}
                    other => {
                        *self
                            .other_operators
                            .entry(other.unwrap_or("?").to_string())
                            .or_default() += 1;
                    }
                }
            }
            if !terms.is_empty() {
                let set = serde_json::to_value(&terms).unwrap_or(Value::Null);
                m.requires_set = Some(intern(&mut self.require_sets, set));
            }
        }

        let slots: Vec<Vec<RepAmount>> = array(v.get("missionResultReputationRewards"))
            .iter()
            .map(|list| {
                array(list.get("reputationAmounts"))
                    .iter()
                    .map(|p| self.amount(p))
                    .collect()
            })
            .collect();
        if slots.iter().any(|s| !s.is_empty()) {
            let set = serde_json::to_value(&slots).unwrap_or(Value::Null);
            m.rep_rewards_set = Some(intern(&mut self.reward_sets, set));
        }

        if let Some(at_place) = v.get("locationMissionAvailable").filter(|x| !x.is_null()) {
            m.offered_at = self.place(Some(at_place)).map(|p| vec![p]);
        }
        if let Some(loc) = v.get("localityAvailable").filter(|x| !x.is_null()) {
            match records.follow(Some(loc)) {
                Some(l) => {
                    let short = l.short_name.clone();
                    if !self.localities.contains_key(&short) {
                        let mut places = Vec::new();
                        for p in array(l.value.get("availableLocations")) {
                            if let Some(name) = self.place(Some(p)) {
                                push_unique(&mut places, name);
                            }
                        }
                        self.localities.insert(short.clone(), places);
                    }
                    m.locality = Some(short);
                }
                None => self.miss("locality"),
            }
        }

        let missions = |key: &str, unresolved: &mut usize| -> Vec<String> {
            let mut out = Vec::new();
            for r in array(v.get(key)).iter().filter(|r| !r.is_null()) {
                match records.follow(Some(r)) {
                    Some(target) => push_unique(&mut out, target.name.clone()),
                    None => *unresolved += 1,
                }
            }
            out
        };
        let mut lost = 0;
        let chain = Chain {
            requires: missions("requiredMissions", &mut lost),
            associated: missions("associatedMissions", &mut lost),
        };
        *self.unresolved.entry("chained mission").or_default() += lost;
        if !chain.requires.is_empty() || !chain.associated.is_empty() {
            m.chain = Some(chain);
        }

        let thresholds: Vec<Value> = array(at(v, "partialRewardPayout.percentageThresholds"))
            .iter()
            .map(|t| {
                json!({
                    "minPercent": num(t.get("minPercentage")),
                    "maxPercent": num(t.get("maxPercentage")),
                    "rewardMultiplier": num(t.get("currencyRewardMultiplier")),
                })
            })
            .collect();
        if !thresholds.is_empty() {
            m.partial_payout = Some(intern(&mut self.payouts, Value::Array(thresholds)));
        }
    }
}

/// The index of `value` in `list`, appended when new.
fn intern(list: &mut Vec<Value>, value: Value) -> usize {
    match list.iter().position(|v| *v == value) {
        Some(i) => i,
        None => {
            list.push(value);
            list.len() - 1
        }
    }
}

/// `usedByMissions{items, resources}`: per item (file stem) or resource, the missions that
/// spawn or haul it, live ones first, at most [`MAX_USED_BY`] listed plus the total.
fn used_by_missions(missions: &[Mission]) -> Value {
    fn add<'m>(map: &mut BTreeMap<String, Vec<&'m str>>, key: &str, name: &'m str) {
        let list = map.entry(key.to_string()).or_default();
        if !list.contains(&name) {
            list.push(name);
        }
    }
    let mut items: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    let mut resources: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    let live_first = missions
        .iter()
        .filter(|m| m.not_for_release.is_none())
        .chain(missions.iter().filter(|m| m.not_for_release.is_some()));
    for m in live_first {
        let name = m.record_name.as_str();
        for s in m.spawns_items.iter().flatten() {
            add(&mut items, &s.item, name);
        }
        for h in m.hauling_items.iter().flatten() {
            if let Some(i) = &h.item {
                add(&mut items, i, name);
            }
            if let Some(r) = &h.resource {
                add(&mut resources, r, name);
            }
        }
    }
    let table = |map: BTreeMap<String, Vec<&str>>| -> Value {
        map.into_iter()
            .map(|(k, list)| {
                let total = list.len();
                let shown: Vec<&str> = list.into_iter().take(MAX_USED_BY).collect();
                (k, json!({"missions": shown, "total": total}))
            })
            .collect::<Map<String, Value>>()
            .into()
    };
    json!({"items": table(items), "resources": table(resources)})
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
        // The rest are what the joins resolve against: starmap places and the
        // jurisdictions they name (as game-starmap reads them), standings, scopes and
        // reward amounts, factions, givers and localities.
        &[
            "**/missionbroker/**",
            "**/starmap/**",
            "**/starmapobject/**",
            "**/lawsystem/jurisdictions/**",
            "**/reputation/**",
            "**/factions/factionreputation/**",
            "**/missiongiver/**",
            "**/missiondata/pu_missionlocality/**",
        ]
    }

    fn needs_localization(&self) -> bool {
        true
    }

    fn parse(&self, ctx: &ParseContext<'_>) -> Result<Map<String, Value>, DomainError> {
        let mut joins = Joins::new(ctx.records, ctx.localization);
        let mut missions: Vec<Mission> = ctx
            .records
            .of_type("MissionBrokerEntry")
            .map(|r| {
                let mut m = parse_mission(r, ctx.localization);
                joins.link(&mut m, r);
                m
            })
            .collect();
        missions.sort_by(|a, b| locale_compare(&a.name, &b.name));
        for (what, n) in joins.unresolved.iter().filter(|(_, n)| **n > 0) {
            ctx.warn(format!("game-missions: {n} {what} reference(s) did not resolve"));
        }
        for (op, n) in &joins.other_operators {
            ctx.warn(format!(
                "game-missions: reputationRequirements uses {op} ({n}x); requires lists the terms as if joined by And"
            ));
        }

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

        let count = |f: fn(&Mission) -> bool| json!(missions.iter().filter(|m| f(m)).count());
        let mut links = Map::new();
        links.insert("requires".into(), count(|m| m.requires_set.is_some()));
        links.insert("repRewards".into(), count(|m| m.rep_rewards_set.is_some()));
        links.insert("wantedLevel".into(), count(|m| m.wanted_level.is_some()));
        links.insert("offeredAt".into(), count(|m| m.offered_at.is_some()));
        links.insert("locality".into(), count(|m| m.locality.is_some()));
        links.insert("giverRecord".into(), count(|m| m.giver_record.is_some()));
        // Missions whose giver record has its own name / names a faction; the `token*`
        // counts are those whose `giver` is an unresolved `~mission(...)` token.
        let has = |m: &Mission, key: &str| {
            m.giver_record
                .as_ref()
                .and_then(|g| joins.givers.get(g))
                .is_some_and(|row| row.get(key).is_some())
        };
        let token = |m: &Mission| m.giver.as_deref().is_some_and(|g| g.starts_with("~mission("));
        let n = |f: &dyn Fn(&Mission) -> bool| json!(missions.iter().filter(|m| f(m)).count());
        links.insert("giverNamed".into(), n(&|m| has(m, "name")));
        links.insert("giverFaction".into(), n(&|m| has(m, "faction")));
        links.insert("tokenGiverNamed".into(), n(&|m| token(m) && has(m, "name")));
        links.insert("tokenGiverFaction".into(), n(&|m| token(m) && has(m, "faction")));
        links.insert("chain".into(), count(|m| m.chain.is_some()));
        links.insert("partialPayout".into(), count(|m| m.partial_payout.is_some()));
        links.insert("module".into(), count(|m| m.module.is_some()));
        links.insert("spawnsItems".into(), count(|m| m.spawns_items.is_some()));
        links.insert("haulingItems".into(), count(|m| m.hauling_items.is_some()));

        let mut body = Map::new();
        body.insert("_assumptions".into(), json!(ASSUMPTIONS));
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
                links,
            },
        )?;
        field(&mut body, "requiresSets", &joins.require_sets)?;
        field(&mut body, "repRewardsSets", &joins.reward_sets)?;
        field(&mut body, "givers", &joins.givers)?;
        field(&mut body, "factions", &joins.factions)?;
        field(&mut body, "places", &joins.places)?;
        field(&mut body, "localities", &joins.localities)?;
        field(&mut body, "partialPayouts", &joins.payouts)?;
        body.insert("usedByMissions".into(), used_by_missions(&missions));
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
        assert_eq!(
            keys,
            vec![
                "_assumptions", "missions", "byGiver", "summary", "requiresSets",
                "repRewardsSets", "givers", "factions", "places", "localities",
                "partialPayouts", "usedByMissions"
            ]
        );
        // Sorted by name: "Data Heist" < "DevOnly" < "Live2".
        assert_eq!(body["missions"][0]["name"], "Data Heist");
        assert_eq!(body["missions"][2]["name"], "Live2");
    }

    // ---- joins (D2, dump-enhancements item 9)

    use crate::domains::starmap::tests::{file_ref, rec};

    fn join_records() -> Vec<GameRecord> {
        let loc = |p: &str| file_ref(p);
        vec![
            rec("Jurisdiction", "lawsystem/jurisdictions/stanton/crusader", json!({}), Some("CrusaderIndustries")),
            rec("StarMapObject", "starmap/pu/system/stanton/stantonsystem", json!({"name": "Stanton"}), Some("Stanton")),
            rec("StarMapObject", "starmap/pu/system/stanton/stanton2",
                json!({"name": "Crusader", "parent": loc("starmap/pu/system/stanton/stantonsystem"),
                       "jurisdiction": loc("lawsystem/jurisdictions/stanton/crusader")}), Some("Stanton2")),
            rec("StarMapObject", "starmap/pu/station/seraphim",
                json!({"name": "Seraphim", "parent": loc("starmap/pu/system/stanton/stanton2")}), Some("Seraphim")),
            rec("MissionLocality", "missiondata/pu_missionlocality/stanton2",
                json!({"availableLocations": [loc("starmap/pu/system/stanton/stanton2"), loc("starmap/pu/station/seraphim")]}), Some("Stanton2")),
            rec("SReputationStandingParams", "reputation/standings/sec/reputationstanding_sec_rank2", json!({"minReputation": 500}), Some("ReputationStanding_Sec_Rank2")),
            rec("SReputationScopeParams", "reputation/scopes/reputationscope_security", json!({"scopeName": "Security"}), Some("ReputationScope_Security")),
            rec("SReputationRewardAmount", "reputation/rewards/missionrewards_reputation/plus_s", json!({"reputationAmount": 1000}), Some("Plus_S")),
            rec("FactionReputation", "factions/factionreputation/factionreputation_crus",
                json!({"displayName": "@Heist_Giver"}), Some("FactionReputation_Crus")),
            rec("MissionGiver", "missiongiver/bounty/crusaderbounty",
                json!({"displayName": "@LOC_UNINITIALIZED", "reputation": loc("factions/factionreputation/factionreputation_crus")}), Some("CrusaderBounty")),
        ]
    }

    fn joined() -> Map<String, Value> {
        let loc = |p: &str| file_ref(p);
        let amount = json!({"_Type_": "SReputationAmountParams",
            "factionReputation": loc("factions/factionreputation/factionreputation_crus"),
            "reputationScope": loc("reputation/scopes/reputationscope_security"),
            "reward": loc("reputation/rewards/missionrewards_reputation/plus_s")});
        let mut records = join_records();
        records.push(mission("First", json!({})));
        records.push(mission(
            "Bounty",
            json!({
                "missionGiver": "~mission(Contractor|BountyFrom)",
                "missionModule": "Libs/Subsumption/Missions/PU/Missions/PU_FPSCombat.xml",
                "missionGiverRecord": loc("missiongiver/bounty/crusaderbounty"),
                "reputationRequirements": {"expression": [
                    {"_Type_": "SReputationMissionRequirementExpression_LeftParenthesis"},
                    {"_Type_": "SReputationMissionGiverRequirementParams",
                     "factionReputation": loc("factions/factionreputation/factionreputation_crus"),
                     "reputationScope": loc("reputation/scopes/reputationscope_security"),
                     "comparison": "GreaterThanOrEqualTo",
                     "standing": loc("reputation/standings/sec/reputationstanding_sec_rank2")},
                    {"_Type_": "SReputationMissionRequirementExpression_RightParenthesis"}]},
                "missionResultReputationRewards": [
                    {"reputationAmounts": [amount]}, {"reputationAmounts": []}, {"reputationAmounts": []},
                    {"reputationAmounts": []}, {"reputationAmounts": []}],
                "reputationPrerequisites": {"wantedLevel": {"minValue": 0.0, "maxValue": 2.0}},
                "locationMissionAvailable": loc("starmap/pu/station/seraphim"),
                "localityAvailable": loc("missiondata/pu_missionlocality/stanton2"),
                "requiredMissions": [loc("missionbroker/pu_missions/first"), null],
                "partialRewardPayout": {"percentageThresholds": [
                    {"minPercentage": 76, "maxPercentage": 99, "currencyRewardMultiplier": 0.76}]},
                "properties": [{"value": {"autoSpawnSettings": {"inventoryItems": [
                    {"entityClass": loc("entities/scitem/carryables/1h/Datapad_X"), "quantity": 1},
                    {"entityClass": loc("entities/scitem/carryables/1h/Datapad_X"), "quantity": 1}]}}}],
                "objectiveTokens": [{"objectiveHandler": {"haulingOrders": [
                    {"_Type_": "HaulingOrder_Resource", "resource": {"_RecordName_": "ResourceType.Aluminum"}, "minSCU": 4.0, "maxSCU": 8.0},
                    {"_Type_": "HaulingOrder_DropOff"}]}}]
            }),
        ));
        parse(records)
    }

    fn bounty(body: &Map<String, Value>) -> &Value {
        body["missions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["recordName"] == "MissionBrokerEntry.Bounty")
            .unwrap()
    }

    #[test]
    fn reads_reputation_needed_and_gained() {
        let body = joined();
        let m = bounty(&body);
        assert_eq!(m["requiresSet"], 0);
        assert_eq!(
            body["requiresSets"][0],
            json!([{"faction": "FactionReputation_Crus", "scope": "Security", "comparison": "GreaterThanOrEqualTo",
                    "standing": "SReputationStandingParams.ReputationStanding_Sec_Rank2"}])
        );
        // Five slots in record order, no outcome labels.
        assert_eq!(m["repRewardsSet"], 0);
        assert_eq!(
            body["repRewardsSets"][0],
            json!([[{"faction": "FactionReputation_Crus", "scope": "Security", "amount": 1000.0}], [], [], [], []])
        );
        assert_eq!(m["wantedLevel"], json!({"min": 0.0, "max": 2.0}));
        assert_eq!(body["factions"]["FactionReputation_Crus"], json!({"name": "Ruto", "nameKey": "Heist_Giver"}));
        assert_eq!(body["_assumptions"], json!(ASSUMPTIONS));
    }

    #[test]
    fn links_the_giver_to_its_faction_without_borrowing_the_faction_name() {
        let body = joined();
        let m = bounty(&body);
        assert_eq!(m["giver"], "~mission(Contractor|BountyFrom)", "giver itself is unchanged");
        assert_eq!(m["giverRecord"], "MissionGiver.CrusaderBounty");
        assert_eq!(
            body["givers"]["MissionGiver.CrusaderBounty"],
            json!({"faction": "FactionReputation_Crus"}),
            "a placeholder displayName leaves the giver unnamed"
        );
        assert_eq!(body["factions"]["FactionReputation_Crus"]["name"], "Ruto");
        assert_eq!(body["summary"]["links"]["tokenGiverNamed"], 0);
        assert_eq!(body["summary"]["links"]["tokenGiverFaction"], 1);
    }

    #[test]
    fn places_offers_with_system_and_jurisdiction() {
        let body = joined();
        let m = bounty(&body);
        assert_eq!(m["offeredAt"], json!(["StarMapObject.Seraphim"]));
        assert_eq!(m["locality"], "Stanton2");
        assert_eq!(
            body["localities"]["Stanton2"],
            json!(["StarMapObject.Stanton2", "StarMapObject.Seraphim"])
        );
        assert_eq!(
            body["places"]["StarMapObject.Stanton2"],
            json!({"name": "Crusader", "system": "Stanton", "jurisdiction": "CrusaderIndustries"})
        );
        // Not stated on the station: inherited from its parent, as an estimate.
        assert_eq!(
            body["places"]["StarMapObject.Seraphim"],
            json!({"name": "Seraphim", "system": "Stanton", "jurisdictionEstimate": "CrusaderIndustries"})
        );
    }

    #[test]
    fn chains_payouts_modules_and_item_links() {
        let body = joined();
        let m = bounty(&body);
        assert_eq!(m["chain"], json!({"requires": ["MissionBrokerEntry.First"]}));
        assert_eq!(m["module"], "PU/Missions/PU_FPSCombat");
        assert_eq!(m["partialPayout"], 0);
        assert_eq!(
            body["partialPayouts"],
            json!([[{"minPercent": 76.0, "maxPercent": 99.0, "rewardMultiplier": 0.76}]])
        );
        assert_eq!(m["spawnsItems"], json!([{"item": "datapad_x", "quantity": 1.0}]));
        assert_eq!(
            m["haulingItems"],
            json!([{"resource": "Aluminum", "minScu": 4.0, "maxScu": 8.0}])
        );
        assert_eq!(
            body["usedByMissions"],
            json!({"items": {"datapad_x": {"missions": ["MissionBrokerEntry.Bounty"], "total": 1}},
                   "resources": {"Aluminum": {"missions": ["MissionBrokerEntry.Bounty"], "total": 1}}})
        );
        // A mission without the fields gets none of the join keys.
        let first = body["missions"].as_array().unwrap().iter().find(|m| m["name"] == "First").unwrap();
        let keys: Vec<&str> = first.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(keys, vec!["name", "recordName", "id", "nameStatus"]);
    }

    #[test]
    fn module_paths_lose_the_shared_folder_and_extension() {
        assert_eq!(module_of(Some("libs/subsumption/missions/PU/X.xml")).as_deref(), Some("PU/X"));
        assert_eq!(module_of(Some("Libs\\Subsumption\\Missions\\A\\B.XML")).as_deref(), Some("A/B"));
        assert_eq!(module_of(Some("Other/Y.xml")).as_deref(), Some("Other/Y"));
        assert_eq!(module_of(None), None);
    }
}
