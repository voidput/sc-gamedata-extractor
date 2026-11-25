//! Crafting blueprints (port of `sc-gamedata/src/domains/blueprints.ts`).
//!
//! A `CraftingBlueprintRecord` holds one or more tiers, and each tier's recipe has a cost
//! tree: `CraftingCost_Resource` (a refined resource, in SCU), `CraftingCost_Item` (a
//! physical item, a count) and `CraftingCost_Select` ("choose `count` of these
//! `options`", options may be Selects again). The tree is flattened to leaves while
//! keeping the choice structure.
//!
//! `blueprintName` is a placeholder on nearly every record, so the display name comes
//! from the item the blueprint produces (`itemIdentity`, here
//! [`crate::helpers_gameplay::item_display_name`]).
//!
//! Blueprints are earned from missions: a contract's results may include
//! `BlueprintRewards` pointing at a `BlueprintPoolRecord` of weighted blueprints. The
//! contract is the nearest enclosing object with a `debugName`, its handler the
//! generator group above it.

use crate::collate::compare_names;
use crate::domain::{Domain, DomainError, ParseContext, field};
use crate::helpers_gameplay::{add_count, by_count_desc, file_ref_stem, item_display_name, ref_short};
use crate::localization::{Localization, is_placeholder_name, localize};
use crate::output::js_key_order;
use crate::records::{GameRecord, at, num, ref_path, ref_path_str, str_};
use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::HashMap;

pub static DOMAIN: Blueprints = Blueprints;

pub struct Blueprints;

/// One cost node. `kind` is written first, then the TS literal's fields.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Cost {
    Resource {
        resource: String,
        /// Standard cargo units.
        #[serde(skip_serializing_if = "Option::is_none")]
        scu: Option<f64>,
        #[serde(rename = "minQuality", skip_serializing_if = "Option::is_none")]
        min_quality: Option<f64>,
    },
    Item {
        item: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        quantity: Option<f64>,
        #[serde(rename = "minQuality", skip_serializing_if = "Option::is_none")]
        min_quality: Option<f64>,
    },
    Choice {
        /// How many of `options` must be supplied.
        #[serde(skip_serializing_if = "Option::is_none")]
        count: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        options: Vec<Cost>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlueprintTier {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub craft_time_seconds: Option<f64>,
    pub costs: Vec<Cost>,
}

/// A mission that can award a blueprint.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlueprintReward {
    /// The mission's title, or its debug name when it has none.
    pub mission: String,
    pub contract: String,
    /// Generator handler the contract belongs to, e.g. `TheCollector_Small_Items`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generator: Option<String>,
    pub pool: String,
    /// Chance the pool is awarded on completion.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chance: Option<f64>,
    /// This blueprint's weight within the pool, and the pool's total.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weight: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pool_weight: Option<f64>,
}

/// One blueprint. `displayName` and `rewardedBy` are assigned after the TS literal is
/// built, so they come last.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Blueprint {
    pub name: String,
    pub record_name: String,
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// What the blueprint produces.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub produces: Option<String>,
    pub tiers: Vec<BlueprintTier>,
    /// The produced item's display name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rewarded_by: Option<Vec<BlueprintReward>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct PoolEntry {
    blueprint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    weight: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct RewardPool {
    name: String,
    blueprints: Vec<PoolEntry>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Summary {
    blueprints: usize,
    distinct_resources: usize,
    categories: usize,
    named: usize,
    rewarded: usize,
    reward_pools: usize,
}

/// A BlueprintRewards found in the contract generators (`collectBlueprintRewards`).
#[derive(Debug, Clone, PartialEq)]
pub struct FoundReward {
    pub pool: String,
    pub chance: Option<f64>,
    pub contract: String,
    pub mission: String,
    pub generator: Option<String>,
}

/// JavaScript truthiness of a JSON value.
fn truthy(value: Option<&Value>) -> Option<&Value> {
    value.filter(|v| match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|x| x != 0.0 && !x.is_nan()),
        Value::String(s) => !s.is_empty(),
        _ => true,
    })
}

/// `num(x) || undefined`.
fn non_zero(value: Option<&Value>) -> Option<f64> {
    num(value).filter(|x| *x != 0.0)
}

pub fn parse_cost(node: Option<&Value>, localization: &Localization) -> Option<Cost> {
    let obj = node?.as_object()?;
    match obj.get("_Type_").and_then(Value::as_str)? {
        "CraftingCost_Resource" => {
            let resource = ref_short(obj.get("resource")).filter(|r| !r.is_empty())?;
            Some(Cost::Resource {
                resource: resource.to_string(),
                scu: num(obj.get("quantity").and_then(|q| q.get("standardCargoUnits"))),
                min_quality: non_zero(obj.get("minQuality")),
            })
        }
        "CraftingCost_Item" => {
            let item = file_ref_stem(obj.get("entityClass")).filter(|i| !i.is_empty())?;
            Some(Cost::Item {
                item: item.to_string(),
                quantity: num(obj.get("quantity")),
                min_quality: non_zero(obj.get("minQuality")),
            })
        }
        "CraftingCost_Select" => {
            let options: Vec<Cost> = obj
                .get("options")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|o| parse_cost(Some(o), localization))
                .collect();
            if options.is_empty() {
                return None;
            }
            let name_info = obj.get("nameInfo");
            let label = localize(localization, name_info.and_then(|n| at(n, "displayName")));
            let debug = str_(name_info.and_then(|n| at(n, "debugName")));
            Some(Cost::Choice {
                count: num(obj.get("count")),
                // displayName is usually a placeholder; debugName ("ASPECTS") is real.
                label: if is_placeholder_name(label) { debug } else { label }.map(str::to_string),
                options,
            })
        }
        _ => None,
    }
}

/// Total seconds from the game's partitioned time struct, to 2 decimals; `None` for zero.
pub fn time_seconds(value: Option<&Value>) -> Option<f64> {
    let t = value?.as_object()?;
    let part = |k: &str| num(t.get(k)).unwrap_or(0.0);
    let total = part("days") * 86400.0 + part("hours") * 3600.0 + part("minutes") * 60.0 + part("seconds");
    // `Math.round` on a positive number: half up, as `f64::round` does there.
    (total > 0.0).then(|| (total * 100.0).round() / 100.0)
}

/// The `stringParamOverrides` Title of a contract, localized, when it is a real name.
fn title_of(contract: &Map<String, Value>, localization: &Localization) -> Option<String> {
    let params = contract
        .get("paramOverrides")
        .and_then(|p| p.get("stringParamOverrides"))
        .and_then(Value::as_array)?;
    let title = params
        .iter()
        .find(|p| p.get("param").and_then(Value::as_str) == Some("Title"))?
        .get("value");
    localize(localization, title)
        .filter(|t| !is_placeholder_name(Some(t)))
        .map(str::to_string)
}

fn walk<'a>(
    value: &'a Value,
    named: &mut Vec<&'a Map<String, Value>>,
    localization: &Localization,
    found: &mut Vec<FoundReward>,
) {
    match value {
        Value::Array(entries) => {
            for entry in entries {
                walk(entry, named, localization, found);
            }
        }
        Value::Object(obj) => {
            let pushed = obj.get("debugName").is_some_and(Value::is_string);
            if pushed {
                named.push(obj);
            }
            if obj.get("_Type_").and_then(Value::as_str) == Some("BlueprintRewards")
                && let Some(contract) = named.last()
                && let Some(pool) = ref_path(obj.get("blueprintPool")).filter(|p| !p.is_empty())
            {
                let debug_name = |o: &Map<String, Value>| {
                    o.get("debugName")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string()
                };
                let contract_name = debug_name(contract);
                found.push(FoundReward {
                    pool,
                    chance: num(obj.get("chance")),
                    mission: title_of(contract, localization).unwrap_or_else(|| contract_name.clone()),
                    contract: contract_name,
                    generator: (named.len() > 1).then(|| debug_name(named[named.len() - 2])),
                });
            }
            for key in js_key_order(obj) {
                walk(&obj[key], named, localization, found);
            }
            if pushed {
                named.pop();
            }
        }
        _ => {}
    }
}

/// Every BlueprintRewards in the contract records (record path containing `/contracts/`),
/// with the contract it sits on.
pub fn collect_blueprint_rewards(records: &[GameRecord], localization: &Localization) -> Vec<FoundReward> {
    let mut found = Vec::new();
    for record in records {
        if record.record_path.contains("/contracts/") {
            walk(&record.value, &mut Vec::new(), localization, &mut found);
        }
    }
    found
}

pub fn parse_blueprint(record: &GameRecord, localization: &Localization) -> Option<Blueprint> {
    let blueprint = truthy(record.value.get("blueprint"))?;

    let mut tiers = Vec::new();
    for tier in blueprint.get("tiers").and_then(Value::as_array).into_iter().flatten() {
        let Some(costs) = truthy(at(tier, "recipe.costs")) else {
            continue;
        };
        let mandatory = parse_cost(costs.get("mandatoryCost"), localization);
        tiers.push(BlueprintTier {
            craft_time_seconds: time_seconds(costs.get("craftTime")),
            // A single Select at the root is the recipe itself; unwrap it so the tier reads
            // as a list of requirements rather than one nested choice.
            costs: match mandatory {
                Some(Cost::Choice {
                    count: None,
                    options,
                    ..
                }) => options,
                Some(cost) => vec![cost],
                None => Vec::new(),
            },
        });
    }

    // Dismantle records share the type but craft nothing; they have no tiers.
    if tiers.is_empty() {
        return None;
    }

    let display_name = localize(localization, blueprint.get("blueprintName"));
    let produces = file_ref_stem(at(blueprint, "processSpecificData.entityClass")).map(str::to_string);
    let name = if is_placeholder_name(display_name) {
        produces.clone().unwrap_or_else(|| record.short_name.clone())
    } else {
        display_name.unwrap_or_default().to_string()
    };
    Some(Blueprint {
        name,
        record_name: record.name.clone(),
        id: record.id.clone(),
        category: ref_short(blueprint.get("category")).map(str::to_string),
        produces,
        tiers,
        display_name: None,
        rewarded_by: None,
    })
}

struct Pool<'a> {
    record: &'a GameRecord,
    entries: Vec<(String, Option<f64>)>,
}

/// Blueprint pools by record path (a JS `Map`: a repeated path keeps its first position and
/// takes the last value), with each entry's blueprint path and weight.
fn parse_pools(records: &[GameRecord]) -> (Vec<(String, Pool<'_>)>, HashMap<String, usize>) {
    let mut pools: Vec<(String, Pool<'_>)> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for record in records.iter().filter(|r| r.ty == "BlueprintPoolRecord") {
        let entries: Vec<(String, Option<f64>)> = record
            .value
            .get("blueprintRewards")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|e| e.is_object())
            .map(|e| {
                (
                    ref_path(e.get("blueprintRecord")).unwrap_or_default(),
                    num(e.get("weight")),
                )
            })
            .filter(|(p, _)| !p.is_empty())
            .collect();
        let Some(path) = ref_path_str(&record.record_path).filter(|p| !p.is_empty()) else {
            continue;
        };
        let pool = Pool { record, entries };
        match index.get(&path) {
            Some(&i) => pools[i].1 = pool,
            None => {
                index.insert(path.clone(), pools.len());
                pools.push((path, pool));
            }
        }
    }
    (pools, index)
}

fn count_resources(costs: &[Cost], tally: &mut Map<String, Value>) {
    for cost in costs {
        match cost {
            Cost::Resource { resource, .. } => add_count(tally, resource, 1.0),
            Cost::Choice { options, .. } => count_resources(options, tally),
            Cost::Item { .. } => {}
        }
    }
}

impl Domain for Blueprints {
    fn name(&self) -> &'static str {
        "game-blueprints"
    }

    fn description(&self) -> &'static str {
        "Crafting blueprints with their recipe costs, craft times, item names and the missions that award them."
    }

    fn filters(&self) -> &'static [&'static str] {
        &[
            "**/crafting/**",
            "**/contracts/contractgenerator/**",
            "**/contracts/contractscenarios/**",
            // What blueprints produce, for display names.
            "**/entities/scitem/ships/**",
            "**/entities/scitem/weapons/**",
            "**/entities/scitem/characters/human/**",
            "**/entities/scitem/carryables/**",
        ]
    }

    fn needs_localization(&self) -> bool {
        true
    }

    /// Item records are only looked up for the product's display name (`AttachDef`); keep
    /// just that (6.5 GB whole on LIVE). Contract records are walked whole.
    fn prune(&self, record: &mut GameRecord) {
        if record.ty == "EntityClassDefinition"
            && record.record_path.contains("/entities/scitem/")
            && !record.record_path.contains("/contracts/")
        {
            crate::entity::prune_entity(
                &mut record.value,
                &[],
                &[("SAttachableComponentParams", Some(&["AttachDef"]))],
            );
        }
    }

    fn parse(&self, ctx: &ParseContext<'_>) -> Result<Map<String, Value>, DomainError> {
        let records = ctx.records.all();
        let (pools, pool_index) = parse_pools(records);

        // Blueprint path -> the missions that can award it.
        let mut rewards: HashMap<String, Vec<BlueprintReward>> = HashMap::new();
        for reward in collect_blueprint_rewards(records, ctx.localization) {
            let Some(&i) = pool_index.get(&reward.pool) else {
                continue;
            };
            let pool = &pools[i].1;
            let pool_weight: f64 = pool.entries.iter().map(|(_, w)| w.unwrap_or(0.0)).sum();
            for (path, weight) in &pool.entries {
                rewards.entry(path.clone()).or_default().push(BlueprintReward {
                    mission: reward.mission.clone(),
                    contract: reward.contract.clone(),
                    generator: reward.generator.clone(),
                    pool: pool.record.short_name.clone(),
                    chance: reward.chance,
                    weight: *weight,
                    pool_weight: (pool_weight != 0.0).then_some(pool_weight),
                });
            }
        }

        let mut blueprints = Vec::new();
        for record in records.iter().filter(|r| r.ty == "CraftingBlueprintRecord") {
            let Some(mut blueprint) = parse_blueprint(record, ctx.localization) else {
                continue;
            };
            let product_path =
                ref_path(at(&record.value, "blueprint.processSpecificData.entityClass")).unwrap_or_default();
            if let Some(name) = ctx
                .records
                .by_record_path(&product_path)
                .and_then(|p| item_display_name(p, ctx.localization))
            {
                if blueprint.produces.as_deref() == Some(blueprint.name.as_str()) {
                    blueprint.name = name.clone();
                }
                blueprint.display_name = Some(name);
            }
            let own_path = ref_path_str(&record.record_path).unwrap_or_default();
            if let Some(awarded) = rewards.get(&own_path) {
                // One row per contract and pool; repeated pool entries are the same reward.
                // A JS Map: first position, last value.
                let mut unique: Vec<(String, BlueprintReward)> = Vec::new();
                for a in awarded {
                    let key = format!("{}|{}", a.contract, a.pool);
                    match unique.iter_mut().find(|(k, _)| *k == key) {
                        Some(slot) => slot.1 = a.clone(),
                        None => unique.push((key, a.clone())),
                    }
                }
                let mut list: Vec<BlueprintReward> = unique.into_iter().map(|(_, a)| a).collect();
                list.sort_by(|a, b| compare_names(&a.mission, &b.mission));
                blueprint.rewarded_by = Some(list);
            }
            blueprints.push(blueprint);
        }
        blueprints.sort_by(|a, b| compare_names(&a.name, &b.name));

        let mut reward_pools: Vec<RewardPool> = pools
            .iter()
            .map(|(_, pool)| RewardPool {
                name: pool.record.short_name.clone(),
                blueprints: pool
                    .entries
                    .iter()
                    .map(|(path, weight)| PoolEntry {
                        blueprint: ctx.records.by_record_path(path).map_or_else(
                            || path.rsplit('/').next().unwrap_or_default().to_string(),
                            |r| r.short_name.clone(),
                        ),
                        weight: *weight,
                    })
                    .collect(),
            })
            .collect();
        reward_pools.sort_by(|a, b| compare_names(&a.name, &b.name));

        // Which resources appear across recipes, and in how many.
        let mut resource_use = Map::new();
        for blueprint in &blueprints {
            for tier in &blueprint.tiers {
                count_resources(&tier.costs, &mut resource_use);
            }
        }

        let mut by_category = Map::new();
        for blueprint in &blueprints {
            if let Some(category) = blueprint.category.as_deref().filter(|c| !c.is_empty()) {
                add_count(&mut by_category, category, 1.0);
            }
        }

        let summary = Summary {
            blueprints: blueprints.len(),
            distinct_resources: resource_use.len(),
            categories: by_category.len(),
            named: blueprints
                .iter()
                .filter(|b| b.display_name.as_deref().is_some_and(|n| !n.is_empty()))
                .count(),
            rewarded: blueprints.iter().filter(|b| b.rewarded_by.is_some()).count(),
            reward_pools: reward_pools.len(),
        };

        let mut body = Map::new();
        field(&mut body, "blueprints", &blueprints)?;
        field(&mut body, "rewardPools", &reward_pools)?;
        body.insert("resourceUse".into(), Value::Object(by_count_desc(&resource_use)));
        body.insert("byCategory".into(), Value::Object(by_category));
        field(&mut body, "summary", &summary)?;
        Ok(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::localization::parse_global_ini;
    use crate::records::{RecordSet, normalize_record};
    use serde_json::json;

    fn empty() -> Localization {
        Localization::new()
    }

    fn cost(v: Value) -> Option<Cost> {
        parse_cost(Some(&v), &empty())
    }

    // Cases from sc-gamedata src/domains/blueprints.test.ts.
    #[test]
    fn time_seconds_totals_the_partitioned_struct() {
        let t = |v: Value| time_seconds(Some(&v));
        assert_eq!(t(json!({"days": 0, "hours": 0, "minutes": 1, "seconds": 35})), Some(95.0));
        assert_eq!(t(json!({"days": 1, "hours": 2, "minutes": 0, "seconds": 0})), Some(93600.0));
        assert_eq!(t(json!({"days": 0, "hours": 0, "minutes": 0, "seconds": 0})), None);
        assert_eq!(t(json!(null)), None);
        assert_eq!(t(json!({"seconds": 1.005})), Some(1.0));
        assert_eq!(t(json!({"seconds": 2.125})), Some(2.13));
    }

    #[test]
    fn reads_a_resource_cost_with_its_scu_quantity() {
        assert_eq!(
            cost(json!({"_Type_": "CraftingCost_Resource", "resource": {"_RecordName_": "ResourceType.Taranite"},
                        "quantity": {"standardCargoUnits": 0.06}, "minQuality": 0})),
            Some(Cost::Resource {
                resource: "Taranite".into(),
                scu: Some(0.06),
                min_quality: None
            })
        );
        assert_eq!(cost(json!({"_Type_": "CraftingCost_Resource"})), None);
    }

    #[test]
    fn reads_an_item_cost_as_a_count() {
        let c = cost(json!({"_Type_": "CraftingCost_Item", "entityClass": "file://./x/janalite.json", "quantity": 2}));
        assert_eq!(
            serde_json::to_value(c).unwrap(),
            json!({"kind": "item", "item": "janalite", "quantity": 2.0})
        );
    }

    #[test]
    fn keeps_the_choice_structure() {
        let c = cost(json!({
            "_Type_": "CraftingCost_Select", "count": 2,
            "nameInfo": {"debugName": "ASPECTS", "displayName": "@LOC_PLACEHOLDER"},
            "options": [
                {"_Type_": "CraftingCost_Resource", "resource": {"_RecordName_": "ResourceType.Aslarite"}, "quantity": {"standardCargoUnits": 0.02}},
                {"_Type_": "CraftingCost_Resource", "resource": {"_RecordName_": "ResourceType.Ouratite"}, "quantity": {"standardCargoUnits": 0.03}}
            ]
        }))
        .unwrap();
        let v = serde_json::to_value(&c).unwrap();
        assert_eq!(v["kind"], "choice");
        assert_eq!(v["count"], json!(2.0));
        assert_eq!(v["label"], "ASPECTS");
        assert_eq!(v["options"].as_array().unwrap().len(), 2);
        let keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(keys, vec!["kind", "count", "label", "options"]);
    }

    #[test]
    fn falls_back_to_debug_name_when_the_display_name_is_a_placeholder() {
        let c = cost(json!({
            "_Type_": "CraftingCost_Select", "count": 1,
            "nameInfo": {"debugName": "ARMOURED CARAPACE", "displayName": "<= PLACEHOLDER =>"},
            "options": [{"_Type_": "CraftingCost_Resource", "resource": {"_RecordName_": "ResourceType.Ouratite"}}]
        }));
        assert!(matches!(c, Some(Cost::Choice { label: Some(l), .. }) if l == "ARMOURED CARAPACE"));
    }

    #[test]
    fn drops_empty_choices_and_non_cost_nodes() {
        assert_eq!(cost(json!({"_Type_": "CraftingCost_Select", "count": 1, "options": []})), None);
        assert_eq!(
            cost(json!({"_Type_": "CraftingCostContext_ResultCompositionInclusion"})),
            None
        );
    }

    fn blueprint_record(short_name: &str, tiers: Value) -> GameRecord {
        normalize_record(
            json!({"_RecordName_": format!("CraftingBlueprintRecord.{short_name}"), "_RecordId_": format!("id-{short_name}"),
                   "_RecordValue_": {"_Type_": "CraftingBlueprintRecord", "blueprint": {
                       "blueprintName": "@LOC_PLACEHOLDER",
                       "category": {"_RecordName_": "BlueprintCategoryRecord.VehicleComponentS0"},
                       "processSpecificData": {"entityClass": "file://./x/mining_module_stampede.json"},
                       "tiers": tiers}}}),
            &format!("libs/foundry/records/crafting/blueprints/{short_name}"),
        )
        .unwrap()
    }

    fn tier() -> Value {
        json!({"recipe": {"costs": {"craftTime": {"minutes": 1, "seconds": 35}, "mandatoryCost": {
            "_Type_": "CraftingCost_Select", "count": 1,
            "options": [{"_Type_": "CraftingCost_Resource", "resource": {"_RecordName_": "ResourceType.Aslarite"}, "quantity": {"standardCargoUnits": 0.02}}]
        }}}})
    }

    fn parse(records: Vec<GameRecord>, loc: &Localization) -> Map<String, Value> {
        let set = RecordSet::new(records);
        crate::fixtures::parse_checked(&DOMAIN, &set, loc)
    }

    #[test]
    fn names_a_blueprint_after_what_it_produces_when_the_name_is_a_placeholder() {
        let body = parse(vec![blueprint_record("BP_CRAFT_Stampede", json!([tier()]))], &empty());
        assert_eq!(body["blueprints"][0]["name"], "mining_module_stampede");
        assert_eq!(body["blueprints"][0]["category"], "VehicleComponentS0");
        assert_eq!(body["blueprints"][0]["tiers"][0]["craftTimeSeconds"], json!(95.0));
    }

    #[test]
    fn counts_resource_use_across_recipes_including_inside_choices() {
        let body = parse(
            vec![
                blueprint_record("A", json!([tier()])),
                blueprint_record("B", json!([tier()])),
            ],
            &empty(),
        );
        assert_eq!(body["resourceUse"]["Aslarite"], json!(2.0));
        assert_eq!(body["byCategory"]["VehicleComponentS0"], json!(2.0));
        let keys: Vec<_> = body.keys().cloned().collect();
        assert_eq!(
            keys,
            vec!["blueprints", "rewardPools", "resourceUse", "byCategory", "summary"]
        );
    }

    #[test]
    fn skips_dismantle_records_which_have_no_tiers() {
        let body = parse(vec![blueprint_record("GlobalGenericDismantle", json!([]))], &empty());
        assert_eq!(body["blueprints"], json!([]));
    }

    // Cases from sc-gamedata src/domains/links.test.ts ("blueprint rewards").
    fn rec(ty: &str, path: &str, value: Value, short_name: Option<&str>) -> GameRecord {
        let name = short_name.unwrap_or_else(|| path.rsplit('/').next().unwrap());
        let mut v = json!({"_Type_": ty});
        if let (Some(o), Some(extra)) = (v.as_object_mut(), value.as_object()) {
            o.extend(extra.clone());
        }
        normalize_record(
            json!({"_RecordName_": format!("{ty}.{name}"), "_RecordId_": format!("id-{name}"), "_RecordValue_": v}),
            &format!("libs/foundry/records/{path}"),
        )
        .unwrap()
    }

    fn file_ref(path: &str) -> Value {
        json!(format!("file://./../../../../libs/foundry/records/{path}.json"))
    }

    fn generator() -> GameRecord {
        rec(
            "ContractGenerator",
            "contracts/contractgenerator/thecollector",
            json!({"generators": [{"debugName": "TheCollector_Small_Items", "contracts": [{
                "_Type_": "Contract", "debugName": "TheCollector_vglFlightsuit",
                "paramOverrides": {"stringParamOverrides": [{"param": "Title", "value": "@title"}]},
                "contractResults": {"contractResults": [
                    {"_Type_": "BlueprintRewards", "chance": 1, "blueprintPool": file_ref("crafting/blueprintrewards/pool_suit")}
                ]}
            }]}]}),
            None,
        )
    }

    #[test]
    fn finds_rewards_with_the_contract_and_generator_above_them() {
        let loc = parse_global_ini("title=Flight Suit Recipe");
        assert_eq!(
            collect_blueprint_rewards(&[generator()], &loc),
            vec![FoundReward {
                pool: "crafting/blueprintrewards/pool_suit".into(),
                chance: Some(1.0),
                contract: "TheCollector_vglFlightsuit".into(),
                mission: "Flight Suit Recipe".into(),
                generator: Some("TheCollector_Small_Items".into()),
            }]
        );
    }

    #[test]
    fn names_a_blueprint_after_its_product_and_lists_the_missions_that_award_it() {
        let loc = parse_global_ini("suit=VGL Flightsuit\ntitle=Flight Suit Recipe");
        let body = parse(
            vec![
                generator(),
                rec(
                    "BlueprintPoolRecord",
                    "crafting/blueprintrewards/pool_suit",
                    json!({"blueprintRewards": [
                        {"weight": 1, "blueprintRecord": file_ref("crafting/blueprints/bp_suit")},
                        {"weight": 3, "blueprintRecord": file_ref("crafting/blueprints/bp_other")}
                    ]}),
                    Some("BP_REWARDS_Suit"),
                ),
                rec(
                    "CraftingBlueprintRecord",
                    "crafting/blueprints/bp_suit",
                    json!({"blueprint": {
                        "blueprintName": "@LOC_PLACEHOLDER",
                        "processSpecificData": {"entityClass": file_ref("entities/scitem/characters/human/armor/vgl_flightsuit")},
                        "tiers": [{"recipe": {"costs": {"craftTime": {"seconds": 90}, "mandatoryCost": {"_Type_": "CraftingCost_Resource"}}}}]
                    }}),
                    None,
                ),
                rec(
                    "EntityClassDefinition",
                    "entities/scitem/characters/human/armor/vgl_flightsuit",
                    json!({"Components": [{"_Type_": "SAttachableComponentParams", "AttachDef": {"Type": "Char_Armor_Undersuit", "Localization": {"Name": "@suit"}}}]}),
                    None,
                ),
            ],
            &loc,
        );
        let b = &body["blueprints"][0];
        assert_eq!(b["name"], "VGL Flightsuit");
        assert_eq!(b["displayName"], "VGL Flightsuit");
        assert_eq!(b["tiers"], json!([{"craftTimeSeconds": 90.0, "costs": []}]));
        assert_eq!(
            b["rewardedBy"],
            json!([{"mission": "Flight Suit Recipe", "contract": "TheCollector_vglFlightsuit", "generator": "TheCollector_Small_Items",
                    "pool": "BP_REWARDS_Suit", "chance": 1.0, "weight": 1.0, "poolWeight": 4.0}])
        );
        assert_eq!(
            body["rewardPools"],
            json!([{"name": "BP_REWARDS_Suit", "blueprints": [{"blueprint": "bp_suit", "weight": 1.0}, {"blueprint": "bp_other", "weight": 3.0}]}])
        );
        let keys: Vec<&str> = b.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            vec!["name", "recordName", "id", "produces", "tiers", "displayName", "rewardedBy"]
        );
        assert_eq!(body["summary"]["named"], 1);
        assert_eq!(body["summary"]["rewarded"], 1);
    }
}
