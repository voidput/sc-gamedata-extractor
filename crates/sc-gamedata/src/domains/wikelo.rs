//! Wikelo Emporium trades (port of `sc-gamedata/src/domains/wikelo.ts`, written as
//! `game-wikelo-trades`).
//!
//! Wikelo is internally "TheCollector", which is why no record path contains "wikelo". A
//! trade is a `ContractTemplate` whose objective handler carries `HaulingOrder_EntityClass`
//! nodes: deliver this many of that entity.
//!
//! What you receive is not stored on the template. It sits on the contract generator
//! (`contractgenerator/thecollector`), whose contracts each name the template they run and
//! their results: `ContractResult_Item` (an entity and an amount) or weighted
//! `ItemAwardEntityClass` choices. Trades join their contracts on that template path and
//! list `rewards` with `source: "award"`. A trade no contract runs falls back to the reward
//! read from its record name (`TheCollector_Vehicles_Polaris` gives up a Polaris), kept as
//! `rewardInferred` and listed with `source: "inferred"`.

use crate::collate::locale_compare;
use crate::domain::{Domain, DomainError, ParseContext, field};
use crate::helpers_gameplay::{add_count, by_count_desc, file_ref_stem};
use crate::output::js_key_order;
use crate::records::{GameRecord, num, ref_path, ref_path_str, str_};
use serde::Serialize;
use serde_json::{Map, Value};

pub static DOMAIN: Wikelo = Wikelo;

pub struct Wikelo;

/// One requirement. Field order is the TS object literal's (`amount, maxAmount, item`).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HandIn {
    /// Amount required.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_amount: Option<f64>,
    pub item: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WikeloTrade {
    pub name: String,
    pub record_name: String,
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// Read from the record name, not from a reward field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reward_inferred: Option<String>,
    pub hand_ins: Vec<HandIn>,
    /// Generator contracts that run this template (debug names).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contracts: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rewards: Option<Vec<WikeloReward>>,
}

/// One reward. Field order is the TS object's (`item, amount, weighting, source`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WikeloReward {
    /// Entity class, or for an inferred reward the name fragment read from the record name.
    pub item: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount: Option<f64>,
    /// Relative chance among a contract's weighted choices.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weighting: Option<f64>,
    /// `award` or `inferred`.
    pub source: &'static str,
}

/// A generator contract: what template it runs and what it awards on success.
#[derive(Debug, Clone, PartialEq)]
pub struct CollectorContract {
    pub name: String,
    pub template: Option<String>,
    /// Awards with `source` still unset (`Omit<WikeloReward, "source">`).
    pub awards: Vec<WikeloReward>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Summary {
    trades: usize,
    collector_records_without_hand_ins: usize,
    distinct_items_requested: usize,
    with_awarded_reward: usize,
    with_inferred_reward_only: usize,
    reward_mismatch: Vec<RewardMismatch>,
    contracts_without_template_trade: Vec<String>,
}

/// A vehicle trade whose name implies one reward while its awards name another.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RewardMismatch {
    pub trade: String,
    pub inferred: String,
    pub awarded: Vec<String>,
}

/// `entityOf`: the last segment of a reference's record path (lower case).
fn entity_of(reference: Option<&Value>) -> Option<String> {
    ref_path(reference).map(|p| p.rsplit('/').next().unwrap_or_default().to_string())
}

/// `contractRewards`: item results under a contract's `contractResults`, weighted choices
/// included, success results only (`missionResults[0]`, or no list at all).
pub fn contract_rewards(results: Option<&Value>) -> Vec<WikeloReward> {
    fn walk(value: &Value, weighting: Option<f64>, out: &mut Vec<WikeloReward>) {
        match value {
            Value::Array(items) => {
                for v in items {
                    walk(v, weighting, out);
                }
            }
            Value::Object(obj) => {
                if let Some(Value::Array(results)) = obj.get("missionResults")
                    && results.first() != Some(&Value::Bool(true))
                {
                    return;
                }
                let award = |amount: &str, weighting: Option<f64>, out: &mut Vec<WikeloReward>| {
                    if let Some(item) = entity_of(obj.get("entityClass")).filter(|i| !i.is_empty()) {
                        out.push(WikeloReward {
                            item,
                            amount: num(obj.get(amount)),
                            weighting,
                            source: "",
                        });
                    }
                };
                match obj.get("_Type_").and_then(Value::as_str) {
                    Some("ContractResult_Item") => award("amount", weighting, out),
                    Some("ItemAwardEntityClass") => award("amountToAward", weighting, out),
                    Some("ItemAwardWeightings") => {
                        let w = num(obj.get("weighting"));
                        for key in js_key_order(obj) {
                            walk(&obj[key], w, out);
                        }
                    }
                    _ => {
                        for key in js_key_order(obj) {
                            walk(&obj[key], weighting, out);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    if let Some(v) = results {
        walk(v, None, &mut out);
    }
    out
}

/// `collectorContracts`: every `Contract` of the collector's generator records.
pub fn collector_contracts<'a>(records: impl IntoIterator<Item = &'a GameRecord>) -> Vec<CollectorContract> {
    fn walk(value: &Value, out: &mut Vec<CollectorContract>) {
        match value {
            Value::Array(items) => {
                for v in items {
                    walk(v, out);
                }
            }
            Value::Object(obj) => {
                if obj.get("_Type_").and_then(Value::as_str) == Some("Contract") {
                    if let Some(name) = str_(obj.get("debugName")) {
                        out.push(CollectorContract {
                            name: name.to_string(),
                            template: ref_path(obj.get("template")),
                            awards: contract_rewards(obj.get("contractResults")),
                        });
                    }
                    return;
                }
                for key in js_key_order(obj) {
                    walk(&obj[key], out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    for record in records {
        walk(&record.value, &mut out);
    }
    out
}

/// `withRewards`: join a trade to the contracts running its template. Awards win; the
/// reward read from the record name is the fallback. Duplicate awards are listed once.
pub fn with_rewards(
    mut trade: WikeloTrade,
    template_path: Option<&str>,
    contracts: &[CollectorContract],
) -> WikeloTrade {
    let running: Vec<&CollectorContract> = match template_path {
        Some(path) => contracts
            .iter()
            .filter(|c| c.template.as_deref() == Some(path))
            .collect(),
        None => Vec::new(),
    };
    let mut awards: Vec<WikeloReward> = Vec::new();
    for c in &running {
        for a in &c.awards {
            // `${item}|${amount}|${weighting}`: -0 and 0 print alike, as `==` treats them.
            if awards
                .iter()
                .any(|x| x.item == a.item && x.amount == a.amount && x.weighting == a.weighting)
            {
                continue;
            }
            awards.push(WikeloReward {
                source: "award",
                ..a.clone()
            });
        }
    }
    let rewards = if !awards.is_empty() {
        awards
    } else if let Some(item) = trade.reward_inferred.clone().filter(|r| !r.is_empty()) {
        vec![WikeloReward {
            item,
            amount: None,
            weighting: None,
            source: "inferred",
        }]
    } else {
        Vec::new()
    };
    trade.contracts = (!running.is_empty()).then(|| {
        let mut names: Vec<String> = running.iter().map(|c| c.name.clone()).collect();
        names.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
        names
    });
    trade.rewards = (!rewards.is_empty()).then_some(rewards);
    trade
}

/// Categories whose record names spell out the reward; others use codenames (AD_HushNow).
const NAMED_REWARD_CATEGORIES: [&str; 3] = ["vehicles", "vehicle", "ships"];

/// `squash`: lower-case ASCII letters and digits only.
fn squash(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        .collect()
}

/// `rewardMismatch`: a vehicle trade whose name implies one reward while its awards name
/// another (no word of the inferred name, squashed and over two letters, is in any awarded
/// entity class).
pub fn reward_mismatch(trade: &WikeloTrade) -> Option<RewardMismatch> {
    let awarded: Vec<String> = trade
        .rewards
        .iter()
        .flatten()
        .filter(|r| r.source == "award")
        .map(|r| r.item.clone())
        .collect();
    let inferred = trade.reward_inferred.as_deref().filter(|r| !r.is_empty())?;
    if awarded.is_empty() {
        return None;
    }
    let category = trade.category.as_deref().unwrap_or("").to_lowercase();
    if !NAMED_REWARD_CATEGORIES.contains(&category.as_str()) {
        return None;
    }
    // `split(/\s+/)`: empty pieces squash to "" and drop out with the length filter.
    let words: Vec<String> = inferred
        .split(|c: char| c.is_whitespace() || c == '\u{feff}')
        .map(squash)
        .filter(|w| w.encode_utf16().count() > 2)
        .collect();
    let matches = awarded
        .iter()
        .any(|item| words.iter().any(|w| squash(item).contains(w.as_str())));
    (!matches).then(|| RewardMismatch {
        trade: trade.name.clone(),
        inferred: inferred.to_string(),
        awarded,
    })
}

/// Every hauling order nested anywhere in a value, in JS enumeration order.
pub fn collect_hauling_orders(value: &Value, found: &mut Vec<HandIn>) {
    match value {
        Value::Array(entries) => {
            for entry in entries {
                collect_hauling_orders(entry, found);
            }
        }
        Value::Object(obj) => {
            if obj.get("_Type_").and_then(Value::as_str) == Some("HaulingOrder_EntityClass")
                && let Some(item) = file_ref_stem(obj.get("entityClass")).filter(|i| !i.is_empty())
            {
                    let min = num(obj.get("minAmount"));
                    let max = num(obj.get("maxAmount"));
                    found.push(HandIn {
                        // maxAmount is sometimes 0 where minAmount is the real requirement.
                        // `min && min > 0 ? min : max`
                        amount: match min {
                            Some(m) if m > 0.0 => Some(m),
                            _ => max,
                        },
                        // `max && max !== min ? max : undefined`
                        max_amount: max.filter(|x| *x != 0.0 && Some(*x) != min),
                        item: item.to_string(),
                    });
            }
            for key in js_key_order(obj) {
                collect_hauling_orders(&obj[key], found);
            }
        }
        _ => {}
    }
}

/// `splitRecordName`: "TheCollector_Vehicles_Polaris" -> (Some("Vehicles"), Some("Polaris")).
/// Both `None` for names that are not collector records.
pub fn split_record_name(short_name: &str) -> (Option<String>, Option<String>) {
    const KNOWN_CATEGORIES: [&str; 5] = ["vehicles", "forfavours", "smallitems", "ships", "intro"];
    let parts: Vec<&str> = short_name.split('_').collect();
    if parts.len() < 2 || parts[0].to_lowercase() != "thecollector" {
        return (None, None);
    }
    let rest = &parts[1..];
    let first = rest[0].to_lowercase();
    if KNOWN_CATEGORIES.contains(&first.as_str()) && rest.len() > 1 {
        return (Some(rest[0].to_string()), Some(rest[1..].join(" ")));
    }
    let category = (rest.len() != 1).then(|| rest[0].to_string());
    (category, Some(rest.join(" ")))
}

pub fn parse_wikelo_trade(record: &GameRecord) -> Option<WikeloTrade> {
    let mut hand_ins = Vec::new();
    collect_hauling_orders(&record.value, &mut hand_ins);
    if hand_ins.is_empty() {
        return None;
    }
    hand_ins.sort_by(|a, b| locale_compare(&a.item, &b.item));
    let (category, reward) = split_record_name(&record.short_name);
    Some(WikeloTrade {
        name: record.short_name.clone(),
        record_name: record.name.clone(),
        id: record.id.clone(),
        category,
        reward_inferred: reward,
        hand_ins,
        contracts: None,
        rewards: None,
    })
}

impl Domain for Wikelo {
    fn name(&self) -> &'static str {
        "game-wikelo-trades"
    }
    fn schema_version(&self) -> u32 {
        1
    }

    fn description(&self) -> &'static str {
        "Wikelo Emporium (TheCollector) barter trades and what each one asks for."
    }

    fn filters(&self) -> &'static [&'static str] {
        &[
            "**/contracts/contracttemplates/**",
            "**/contracts/contractgenerator/**",
        ]
    }

    fn parse(&self, ctx: &ParseContext<'_>) -> Result<Map<String, Value>, DomainError> {
        let contracts = collector_contracts(ctx.records.iter().filter(|r| {
            r.record_path
                .to_lowercase()
                .contains("contractgenerator/thecollector")
        }));
        let collector: Vec<&GameRecord> = ctx
            .records
            .of_type("ContractTemplate")
            .filter(|r| {
                format!("{} {}", r.name, r.record_path)
                    .to_lowercase()
                    .contains("thecollector")
            })
            .collect();

        let mut trades: Vec<WikeloTrade> = collector
            .iter()
            .filter_map(|r| {
                let trade = parse_wikelo_trade(r)?;
                Some(with_rewards(
                    trade,
                    ref_path_str(&r.record_path).as_deref(),
                    &contracts,
                ))
            })
            .collect();
        trades.sort_by(|a, b| locale_compare(&a.name, &b.name));
        let used: std::collections::HashSet<&str> = trades
            .iter()
            .flat_map(|t| t.contracts.iter().flatten().map(String::as_str))
            .collect();
        let mut without_trade: Vec<String> = contracts
            .iter()
            .filter(|c| !used.contains(c.name.as_str()))
            .map(|c| c.name.clone())
            .collect();
        without_trade.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));

        // What Wikelo asks for most, across every trade, is the useful roll-up.
        let mut demand = Map::new();
        for trade in &trades {
            for hand_in in &trade.hand_ins {
                add_count(&mut demand, &hand_in.item, hand_in.amount.unwrap_or(0.0));
            }
        }

        let mut body = Map::new();
        field(&mut body, "trades", &trades)?;
        body.insert("totalDemand".into(), Value::Object(by_count_desc(&demand)));
        field(
            &mut body,
            "summary",
            &Summary {
                trades: trades.len(),
                // Templates that carry no hauling orders are flow or intro scaffolding.
                collector_records_without_hand_ins: collector.len() - trades.len(),
                distinct_items_requested: demand.len(),
                with_awarded_reward: trades
                    .iter()
                    .filter(|t| t.rewards.iter().flatten().any(|r| r.source == "award"))
                    .count(),
                // `rewards?.every(...)`: a trade without rewards is not counted.
                with_inferred_reward_only: trades
                    .iter()
                    .filter(|t| {
                        t.rewards
                            .as_ref()
                            .is_some_and(|r| r.iter().all(|r| r.source == "inferred"))
                    })
                    .count(),
                reward_mismatch: trades.iter().filter_map(reward_mismatch).collect(),
                // Contracts on a shared template (itemresourcegathering_thecollector) set
                // their own hand-ins through overrides, so no template trade carries them.
                contracts_without_template_trade: without_trade,
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

    fn template(short_name: &str, orders: Vec<Value>) -> GameRecord {
        normalize_record(
            json!({"_RecordName_": format!("ContractTemplate.{short_name}"), "_RecordId_": format!("id-{short_name}"),
                   "_RecordValue_": {"_Type_": "ContractTemplate", "objectiveTokens": [{"objectiveHandler": {"haulingOrders": orders}}]}}),
            &format!(
                "libs/foundry/records/contracts/contracttemplates/{}",
                short_name.to_lowercase()
            ),
        )
        .unwrap()
    }

    fn order(path: &str, min: f64, max: f64) -> Value {
        json!({"_Type_": "HaulingOrder_EntityClass",
               "entityClass": format!("file://./../libs/foundry/records/entities/scitem/carryables/1h/{path}.json"),
               "minAmount": min, "maxAmount": max})
    }

    fn collect(value: &Value) -> Vec<HandIn> {
        let mut found = Vec::new();
        collect_hauling_orders(value, &mut found);
        found
    }

    // Cases from sc-gamedata src/domains/wikelo.test.ts.
    #[test]
    fn finds_orders_nested_under_the_objective_handler() {
        let t = template("X", vec![order("harvestable_mineral_1h_carinite", 10.0, 10.0)]);
        assert_eq!(
            collect(&t.value),
            vec![HandIn {
                amount: Some(10.0),
                max_amount: None,
                item: "harvestable_mineral_1h_carinite".into()
            }]
        );
    }

    #[test]
    fn amount_fallbacks_and_ranges() {
        // Falls back to maxAmount when minAmount is zero.
        assert_eq!(collect(&order("thing", 0.0, 7.0))[0].amount, Some(7.0));
        // Uses minAmount when maxAmount is zero, as some records store it.
        let h = &collect(&order("volt_rifle_energy_01", 1.0, 0.0))[0];
        assert_eq!((h.amount, h.max_amount), (Some(1.0), None));
        // Records a genuine range when the two differ.
        let h = &collect(&order("thing", 5.0, 9.0))[0];
        assert_eq!((h.amount, h.max_amount), (Some(5.0), Some(9.0)));
    }

    #[test]
    fn returns_nothing_when_there_are_no_hauling_orders() {
        assert!(collect(&json!({"objectiveTokens": [{"other": true}]})).is_empty());
    }

    #[test]
    fn splits_record_names() {
        let s = |n: &str| split_record_name(n);
        assert_eq!(
            s("TheCollector_Vehicles_Polaris"),
            (Some("Vehicles".into()), Some("Polaris".into()))
        );
        assert_eq!(s("TheCollector_Vehicles_Starlancer_Tac").1.as_deref(), Some("Starlancer Tac"));
        assert_eq!(s("SomethingElse_Foo"), (None, None));
        assert_eq!(s("TheCollector_Polaris"), (None, Some("Polaris".into())));
        assert_eq!(s("TheCollector_Odd_Thing"), (Some("Odd".into()), Some("Odd Thing".into())));
    }

    fn records() -> Vec<GameRecord> {
        vec![
            template(
                "TheCollector_Vehicles_Polaris",
                vec![
                    order("carryable_1h_cy_banu_favour_wikelo", 50.0, 50.0),
                    order("harvestable_ore_1h_jacliumore", 20.0, 20.0),
                ],
            ),
            template(
                "TheCollector_ForFavours_Caranite",
                vec![order("carryable_1h_cy_banu_favour_wikelo", 10.0, 10.0)],
            ),
            // Flow scaffolding with no hand-ins should not become a trade.
            template("TheCollector_Flow", vec![]),
        ]
    }

    fn parse() -> Map<String, Value> {
        let set = RecordSet::new(records());
        DOMAIN
            .parse(&ParseContext::new(&set, &Localization::new(), &NoFiles))
            .unwrap()
    }

    #[test]
    fn keeps_only_templates_that_actually_ask_for_something() {
        let body = parse();
        assert_eq!(body["summary"]["trades"], 2);
        assert_eq!(body["summary"]["collectorRecordsWithoutHandIns"], 1);
        assert_eq!(body["summary"]["distinctItemsRequested"], 2);
    }

    #[test]
    fn totals_demand_across_every_trade() {
        let body = parse();
        assert_eq!(body["totalDemand"]["carryable_1h_cy_banu_favour_wikelo"], json!(60.0));
        let keys: Vec<_> = body["totalDemand"].as_object().unwrap().keys().cloned().collect();
        assert_eq!(keys[0], "carryable_1h_cy_banu_favour_wikelo");
    }

    #[test]
    fn marks_the_reward_as_inferred() {
        let trade = parse_wikelo_trade(&records()[0]).unwrap();
        assert_eq!(trade.reward_inferred.as_deref(), Some("Polaris"));
        let row = serde_json::to_value(&trade).unwrap();
        let keys: Vec<&str> = row.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(keys, vec!["name", "recordName", "id", "category", "rewardInferred", "handIns"]);
        assert_eq!(
            row["handIns"][0],
            json!({"amount": 50.0, "item": "carryable_1h_cy_banu_favour_wikelo"})
        );
    }

    // Cases from sc-gamedata src/domains/wikelo.test.ts, "wikelo rewards".
    const ROOT: &str = "file://./../../../../../libs/foundry/records/";

    fn reward_template(short: &str) -> GameRecord {
        normalize_record(
            json!({"_RecordName_": format!("ContractTemplate.{short}"), "_RecordId_": format!("id-{short}"),
                   "_RecordValue_": {"_Type_": "ContractTemplate", "handler": {"orders": [
                       {"_Type_": "HaulingOrder_EntityClass", "entityClass": format!("{ROOT}entities/x/wikelo_favor.json"), "minAmount": 1}]}}}),
            &format!("{ROOT}contracts/contracttemplates/{}.json", short.to_lowercase()),
        )
        .unwrap()
    }

    fn generator() -> GameRecord {
        normalize_record(
            json!({"_RecordName_": "ContractGenerator.thecollector", "_RecordId_": "gen",
                   "_RecordValue_": {"_Type_": "ContractGenerator", "generators": [{"contracts": [
                {"_Type_": "Contract", "debugName": "TheCollector_Vehicle_Polaris",
                 "template": format!("{ROOT}contracts/contracttemplates/thecollector_vehicles_polaris.json"),
                 "contractResults": {"contractResults": [
                    {"_Type_": "ContractResult_Item", "missionResults": [true, false], "entityClass": format!("{ROOT}entities/spaceships/rsi_polaris_collector_military.json"), "amount": 1},
                    {"_Type_": "ContractResult_Item", "missionResults": [false, true], "entityClass": format!("{ROOT}entities/x/consolation.json"), "amount": 1}]}},
                {"_Type_": "Contract", "debugName": "TheCollector_Spirit",
                 "template": format!("{ROOT}contracts/contracttemplates/thecollector_vehicles_sprirt_c1.json"),
                 "contractResults": {"contractResults": [{"_Type_": "ContractResult_ItemsWeighting", "awards": [
                    {"_Type_": "ItemAwardWeightings", "weighting": 0.5, "items": [
                        {"_Type_": "ItemAwardEntityClass", "entityClass": format!("{ROOT}entities/x/drak_cutter.json"), "amountToAward": 1}]}]}]}},
                {"_Type_": "Contract", "debugName": "TheCollector_Idris",
                 "template": format!("{ROOT}contracts/contracttemplates/itemresourcegathering_thecollector.json")}
            ]}]}}),
            &format!("{ROOT}contracts/contractgenerator/thecollector.json"),
        )
        .unwrap()
    }

    fn rewards_body() -> Map<String, Value> {
        let set = RecordSet::new(vec![
            reward_template("TheCollector_Vehicles_Polaris"),
            reward_template("TheCollector_Vehicles_Sprirt_C1"),
            reward_template("TheCollector_Vehicles_Nox"),
            generator(),
        ]);
        DOMAIN
            .parse(&ParseContext::new(&set, &Localization::new(), &NoFiles))
            .unwrap()
    }

    fn trade<'a>(body: &'a Map<String, Value>, name: &str) -> &'a Value {
        body["trades"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == name)
            .unwrap()
    }

    #[test]
    fn takes_the_award_from_the_contract_running_the_template() {
        let body = rewards_body();
        let polaris = trade(&body, "TheCollector_Vehicles_Polaris");
        assert_eq!(
            polaris["rewards"],
            json!([{"item": "rsi_polaris_collector_military", "amount": 1.0, "source": "award"}])
        );
        assert_eq!(polaris["contracts"], json!(["TheCollector_Vehicle_Polaris"]));
        let keys: Vec<&str> = polaris.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(keys[keys.len() - 3..], ["handIns", "contracts", "rewards"]);
    }

    #[test]
    fn keeps_weighted_choices_and_falls_back_to_the_inferred_reward() {
        let body = rewards_body();
        assert_eq!(
            trade(&body, "TheCollector_Vehicles_Sprirt_C1")["rewards"],
            json!([{"item": "drak_cutter", "amount": 1.0, "weighting": 0.5, "source": "award"}])
        );
        let nox = trade(&body, "TheCollector_Vehicles_Nox");
        assert_eq!(nox["rewards"], json!([{"item": "Nox", "source": "inferred"}]));
        assert_eq!(nox["rewardInferred"], "Nox");
        assert!(nox.get("contracts").is_none());
    }

    #[test]
    fn flags_mismatched_vehicle_trades_and_uncovered_contracts() {
        let summary = &rewards_body()["summary"];
        assert_eq!(
            summary["rewardMismatch"],
            json!([{"trade": "TheCollector_Vehicles_Sprirt_C1", "inferred": "Sprirt C1", "awarded": ["drak_cutter"]}])
        );
        assert_eq!(summary["contractsWithoutTemplateTrade"], json!(["TheCollector_Idris"]));
        assert_eq!(summary["withAwardedReward"], 2);
        assert_eq!(summary["withInferredRewardOnly"], 1);
    }
}
