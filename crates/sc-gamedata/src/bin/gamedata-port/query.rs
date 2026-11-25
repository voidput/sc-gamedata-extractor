//! `gamedata-port query`: the query port's answers to a query file, in the shape
//! `scripts/query-golden.mjs` gives sc-gamedata's own answers, for a side-by-side compare.

use sc_gamedata::query::fuzzy::{self, EntryKind};
use sc_gamedata::query::obtain::{
    BlueprintLike, Datasets, LootData, ObtainOptions, WikeloTradeLike, build_loot_index,
    find_items, obtain,
};
use sc_gamedata::query::shops::{
    AsOf, SHOP_ITEM_LIMIT, SHOP_TERMINAL_LIMIT, ShopInputs, ShopPrice, UexTerminal, limit_shop,
    shop_inventory,
};
use sc_gamedata::query::types::{ContractAward, ItemTags, LootArea};
use sc_gamedata::query::where_::where_summary;
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::Path;

#[derive(Deserialize)]
struct Query {
    op: String,
    #[serde(default)]
    q: String,
    #[serde(default)]
    at: Option<String>,
    #[serde(default)]
    place: String,
}

fn load(dir: &Path, file: &str) -> Option<Value> {
    serde_json::from_slice(&std::fs::read(dir.join(file)).ok()?).ok()
}

fn field<T: serde::de::DeserializeOwned>(v: &Option<Value>, key: &str) -> Option<T> {
    v.as_ref()
        .and_then(|v| v.get(key))
        .and_then(|x| serde_json::from_value(x.clone()).ok())
}

pub fn run(data: &Path, queries: &Path) -> Result<String, String> {
    let queries: Vec<Query> = serde_json::from_slice(
        &std::fs::read(queries).map_err(|e| format!("{}: {e}", queries.display()))?,
    )
    .map_err(|e| e.to_string())?;
    let tags = load(data, "game-item-tags.json");
    let loot: Option<LootData> = load(data, "game-loot.json")
        .map(serde_json::from_value)
        .transpose()
        .map_err(|e| e.to_string())?;
    let sources = load(data, "game-item-sources.json");
    let prices = load(data, "uex-item-prices.json");
    let terminals = load(data, "uex-terminals.json");
    let as_of = prices
        .as_ref()
        .and_then(|p| p.get("_fetched"))
        .and_then(Value::as_str)
        .map(|s| AsOf::Iso(s.to_string()));
    let datasets = Datasets {
        items: field::<Vec<ItemTags>>(&tags, "items").unwrap_or_default(),
        loot,
        areas: field::<Vec<LootArea>>(&sources, "areas"),
        contract_awards: field::<Vec<ContractAward>>(&sources, "contractAwards"),
        blueprints: field::<Vec<BlueprintLike>>(&load(data, "game-blueprints.json"), "blueprints"),
        wikelo: field::<Vec<WikeloTradeLike>>(&load(data, "game-wikelo-trades.json"), "trades"),
        shops: field::<Vec<ShopPrice>>(&prices, "prices"),
        terminals: field::<Vec<UexTerminal>>(&terminals, "terminals"),
        shops_as_of: as_of.clone(),
    };
    let index = datasets.loot.as_ref().map(build_loot_index);
    let entries = match load(data, "game-search-index.json") {
        Some(doc) => fuzzy::from_index(&doc, false).map_err(|e| e.to_string())?,
        None => Vec::new(),
    };
    let item_entries = fuzzy::item_entries(&datasets.items, false);
    let shops_loaded = Some(datasets.shops.is_some());

    let resolve = |q: &str| -> Vec<&ItemTags> {
        let found = find_items(&datasets.items, q);
        if !found.is_empty() {
            return found;
        }
        let ranked: Vec<&fuzzy::Entry> = fuzzy::rank_entries(&item_entries, q)
            .into_iter()
            .filter(|e| e.kind == EntryKind::Item)
            .collect();
        let Some(best) = ranked
            .first()
            .and_then(|e| datasets.items.iter().find(|i| i.class_name == e.key))
        else {
            return Vec::new();
        };
        datasets
            .items
            .iter()
            .filter(|i| i.name == best.name)
            .collect()
    };

    let mut out = Vec::new();
    for query in &queries {
        let answer = match query.op.as_str() {
            "find" => Value::Array(
                fuzzy::rank_entries(&entries, &query.q)
                    .into_iter()
                    .take(25)
                    .map(|e| json!([e.kind, e.key, e.name]))
                    .collect(),
            ),
            "shop" => {
                let shops = datasets.shops.as_deref().unwrap_or(&[]);
                let report = shop_inventory(
                    &ShopInputs {
                        shops,
                        terminals: datasets.terminals.as_deref().unwrap_or(&[]),
                        items: &datasets.items,
                        as_of: as_of.clone(),
                        stale_days: None,
                    },
                    &query.place,
                );
                serde_json::to_value(limit_shop(
                    &report,
                    Some(SHOP_ITEM_LIMIT),
                    Some(SHOP_TERMINAL_LIMIT),
                ))
                .map_err(|e| e.to_string())?
            }
            op @ ("where" | "obtain") => {
                let found = resolve(&query.q);
                let opts = ObtainOptions {
                    at: query.at.as_deref(),
                    ..Default::default()
                };
                let reports: Vec<_> = found
                    .iter()
                    .take(3)
                    .map(|i| obtain(&datasets, i, index.as_ref(), &opts))
                    .collect();
                let list: Vec<Value> = if op == "where" {
                    reports
                        .iter()
                        .map(|r| serde_json::to_value(where_summary(r, shops_loaded)))
                        .collect::<Result<_, _>>()
                        .map_err(|e| e.to_string())?
                } else {
                    reports
                        .iter()
                        .map(|r| {
                            let mut v = serde_json::to_value(r)?;
                            v["blueprints"] =
                                json!(r.blueprints.iter().map(|b| &b.name).collect::<Vec<_>>());
                            v["wikeloInferred"] = json!(
                                r.wikelo_inferred
                                    .iter()
                                    .map(|t| &t.name)
                                    .collect::<Vec<_>>()
                            );
                            Ok(v)
                        })
                        .collect::<Result<_, serde_json::Error>>()
                        .map_err(|e| e.to_string())?
                };
                let key = if op == "where" {
                    "summaries"
                } else {
                    "reports"
                };
                let mut v = json!({
                    "matches": found.iter().map(|m| &m.class_name).collect::<Vec<_>>(),
                });
                v[key] = Value::Array(list);
                v
            }
            other => return Err(format!("unknown op {other:?}")),
        };
        out.push(json!({
            "query": {"op": query.op, "q": query.q, "place": query.place},
            "answer": answer
        }));
    }
    serde_json::to_string(&out).map_err(|e| e.to_string())
}
