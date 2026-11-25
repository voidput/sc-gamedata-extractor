//! Market datasets from the UEX Corp API (port of `sc-gamedata/src/domains/uex-market.ts`
//! and `fetchApiDatasets` / `selectApiDomains` in `pipeline.ts`).
//!
//! API-sourced rather than extracted, so each carries a fetch step instead of a record
//! filter. They change constantly; the game files do not. Files are written as
//! `uex-<name>.json`: `{ _source, _fetched, ...body }`, `JSON.stringify(_, null, 2)`.

use crate::output;
use crate::uex::{Http, UexOptions, trim_row, uex_fetch};
use serde_json::{Map, Value, json};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// `_source` of every UEX dataset.
pub const SOURCE: &str = "UEX Corp API (https://uexcorp.space)";

type Fetch = fn(&UexOptions, &mut Http<'_>) -> Result<Map<String, Value>, String>;

/// `ApiDomain`.
pub struct ApiDomain {
    pub name: &'static str,
    pub description: &'static str,
    fetch: Fetch,
}

impl ApiDomain {
    /// The dataset body (everything after the header).
    pub fn fetch(
        &self,
        options: &UexOptions,
        http: &mut Http<'_>,
    ) -> Result<Map<String, Value>, String> {
        (self.fetch)(options, http)
    }
}

/// The columns a terminal row keeps: what places it. Ids, flags, screenshots and contacts go.
const TERMINAL_KEYS: &[&str] = &[
    "id",
    "name",
    "fullname",
    "nickname",
    "displayname",
    "code",
    "type",
    "star_system_name",
    "planet_name",
    "orbit_name",
    "moon_name",
    "space_station_name",
    "city_name",
    "outpost_name",
    "is_available",
    "is_available_live",
];

fn is_zero(v: &Value) -> bool {
    v.as_f64() == Some(0.0)
}

fn is_blank(v: Option<&Value>) -> bool {
    matches!(v, None | Some(Value::Null)) || v.and_then(Value::as_str) == Some("")
}

/// `trimTerminalRow`. Availability is a 0/1 flag, so its zero is data; elsewhere zero and
/// blank mean "not set".
pub fn trim_terminal_row(row: &Value) -> Value {
    let mut out = Map::new();
    for key in TERMINAL_KEYS {
        let value = row.get(key);
        if is_blank(value) {
            continue;
        }
        let value = value.expect("not blank");
        if is_zero(value) && !key.starts_with("is_") {
            continue;
        }
        out.insert((*key).into(), value.clone());
    }
    Value::Object(out)
}

/// The columns an item price row keeps; the rest is location ids and history.
const ITEM_PRICE_KEYS: &[&str] = &[
    "item_name",
    "item_uuid",
    "id_item",
    "id_category",
    "terminal_name",
    "id_terminal",
    "price_buy",
    "price_sell",
    "date_modified",
];

/// `trimItemPriceRow`.
pub fn trim_item_price_row(row: &Value) -> Value {
    let mut out = Map::new();
    for key in ITEM_PRICE_KEYS {
        if let Some(value) = row.get(key)
            && !is_blank(Some(value))
            && !is_zero(value)
        {
            out.insert((*key).into(), value.clone());
        }
    }
    Value::Object(out)
}

/// `trimRefineryRow`: location rows carry an id for every level of the hierarchy, zero
/// where it does not apply, and a reporter handle on audits. Neither is refinery data.
fn trim_refinery_row(row: &Value) -> Value {
    let trimmed = trim_row(row);
    let Value::Object(obj) = trimmed else {
        return trimmed;
    };
    Value::Object(
        obj.into_iter()
            .filter(|(k, v)| {
                !(k.starts_with("id_") && is_zero(v)) && k != "datarunner" && k != "id_report"
            })
            .collect(),
    )
}

fn body(entries: Vec<(&str, Value)>) -> Map<String, Value> {
    entries.into_iter().map(|(k, v)| (k.into(), v)).collect()
}

fn terminals(options: &UexOptions, http: &mut Http<'_>) -> Result<Map<String, Value>, String> {
    // Public endpoint. Price rows (uex-item-prices) join here on id_terminal.
    let rows: Vec<Value> = uex_fetch("terminals", &options.public(), http)?
        .iter()
        .map(trim_terminal_row)
        .collect();
    let n = rows.len();
    Ok(body(vec![
        ("terminals", Value::Array(rows)),
        ("summary", json!({ "terminals": n })),
    ]))
}

fn commodities(options: &UexOptions, http: &mut Http<'_>) -> Result<Map<String, Value>, String> {
    let rows: Vec<Value> = uex_fetch("commodities", options, http)?
        .iter()
        .map(trim_row)
        .collect();
    let n = rows.len();
    Ok(body(vec![
        ("commodities", Value::Array(rows)),
        ("summary", json!({ "commodities": n })),
    ]))
}

fn vehicle_prices(options: &UexOptions, http: &mut Http<'_>) -> Result<Map<String, Value>, String> {
    let vehicles = uex_fetch("vehicles", options, http)?;
    let purchases = uex_fetch("vehicles_purchases_prices", options, http)?;
    let rentals = uex_fetch("vehicles_rentals_prices", options, http)?;

    // Price rows carry only id_vehicle, never a name, so join them here rather than making
    // every consumer resolve the id. A JS Map keyed by the id value: first `set` wins the
    // slot, a later one the value.
    let mut name_by_id: Vec<(Value, String)> = Vec::new();
    for vehicle in &vehicles {
        let name = match vehicle.get("name_full") {
            None | Some(Value::Null) => vehicle.get("name"),
            some => some,
        };
        if let (Some(id), Some(Value::String(name))) = (vehicle.get("id"), name) {
            match name_by_id.iter_mut().find(|(k, _)| same_value(k, id)) {
                Some(slot) => slot.1 = name.clone(),
                None => name_by_id.push((id.clone(), name.clone())),
            }
        }
    }
    let with_name = |rows: &[Value]| -> Vec<Value> {
        rows.iter()
            .map(|row| {
                let trimmed = trim_row(row);
                let name = trimmed.get("id_vehicle").and_then(|id| {
                    name_by_id
                        .iter()
                        .find(|(k, _)| same_value(k, id))
                        .map(|(_, n)| n)
                });
                match (name, &trimmed) {
                    (Some(name), Value::Object(obj)) if !name.is_empty() => {
                        // `{ vehicle: name, ...trimmed }`: a row's own `vehicle` keeps the
                        // first slot but takes the row's value.
                        let mut out = Map::new();
                        out.insert("vehicle".into(), json!(name));
                        for (k, v) in obj {
                            out.insert(k.clone(), v.clone());
                        }
                        Value::Object(out)
                    }
                    _ => trimmed,
                }
            })
            .collect()
    };
    Ok(body(vec![
        ("purchases", Value::Array(with_name(&purchases))),
        ("rentals", Value::Array(with_name(&rentals))),
        (
            "summary",
            json!({
                "vehicles": vehicles.len(),
                "purchases": purchases.len(),
                "rentals": rentals.len(),
            }),
        ),
    ]))
}

/// `SameValueZero` on JSON values, as a `Map` key lookup compares them.
fn same_value(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        (Value::String(x), Value::String(y)) => x == y,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Null, Value::Null) => true,
        // Objects and arrays are compared by identity in JavaScript: two parsed rows never are.
        _ => false,
    }
}

/// Refinery yields, capacity and methods as players report them. None of this is in the
/// game files (the yield bonus per station and each method's yield, cost and speed are
/// server-side); UEX is crowdsourced, so these are observed values, not ground truth.
fn refinery(options: &UexOptions, http: &mut Http<'_>) -> Result<Map<String, Value>, String> {
    let public = options.public();
    let yields = uex_fetch("refineries_yields", &public, http)?;
    let capacities = uex_fetch("refineries_capacities", &public, http)?;
    let methods = uex_fetch("refineries_methods", &public, http)?;
    let audits = uex_fetch("refineries_audits", &public, http)?;
    let trim = |rows: &[Value]| Value::Array(rows.iter().map(trim_refinery_row).collect());
    Ok(body(vec![
        // Yield value is the station's bonus or penalty in percent for that commodity.
        ("yields", trim(&yields)),
        ("capacities", trim(&capacities)),
        // Ratings run 1 (low) to 3 (high): Dinyx is yield 3, cost 1, speed 1.
        ("methods", trim(&methods)),
        ("audits", trim(&audits)),
        (
            "summary",
            json!({
                "yields": yields.len(),
                "capacities": capacities.len(),
                "methods": methods.len(),
                "audits": audits.len(),
            }),
        ),
    ]))
}

fn item_prices(options: &UexOptions, http: &mut Http<'_>) -> Result<Map<String, Value>, String> {
    // Public endpoint; `item_uuid` is the game record id, the join to every other dataset.
    let rows: Vec<Value> = uex_fetch("items_prices_all", &options.public(), http)?
        .iter()
        .map(trim_item_price_row)
        .collect();
    // `new Set(rows.map((r) => r.item_uuid)).size`: undefined counts as one value.
    let items: HashSet<String> = rows
        .iter()
        .map(|r| match r.get("item_uuid") {
            None => "undefined".to_string(),
            Some(v) => format!("{}:{}", type_tag(v), v),
        })
        .collect();
    let n = rows.len();
    Ok(body(vec![
        ("prices", Value::Array(rows)),
        ("summary", json!({ "prices": n, "items": items.len() })),
    ]))
}

fn type_tag(v: &Value) -> &'static str {
    match v {
        Value::Number(_) => "n",
        Value::String(_) => "s",
        _ => "o",
    }
}

/// `UEX_DOMAINS` / `API_DOMAINS`, in registry order.
pub static API_DOMAINS: [ApiDomain; 5] = [
    ApiDomain {
        name: "uex-terminals",
        description: "Every terminal, shop and station with its place: station or city, moon, planet, system. Public, no token needed.",
        fetch: terminals,
    },
    ApiDomain {
        name: "uex-commodities",
        description: "Commodity definitions: what is tradable and how it is classified.",
        fetch: commodities,
    },
    ApiDomain {
        name: "uex-vehicle-prices",
        description: "Where ships can be bought and rented, with prices.",
        fetch: vehicle_prices,
    },
    ApiDomain {
        name: "uex-refinery",
        description: "Refinery yield bonuses by station and commodity, capacities, methods and audited jobs.",
        fetch: refinery,
    },
    ApiDomain {
        name: "uex-item-prices",
        description: "Where personal and ship items are sold, by terminal. Shop stock is server-side, not in the game files.",
        fetch: item_prices,
    },
];

/// `selectApiDomains`: every domain when `names` is empty, else those named (registry order).
pub fn select_api_domains(names: &[String]) -> Vec<&'static ApiDomain> {
    API_DOMAINS
        .iter()
        .filter(|d| names.is_empty() || names.iter().any(|n| n == d.name))
        .collect()
}

/// One written dataset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    pub dataset: &'static str,
    pub file: PathBuf,
    /// Array lengths of the body's collections, in body order.
    pub collections: Vec<(String, usize)>,
}

/// The file text of a fetched body: `{ _source, _fetched, ...body }`.
pub fn dataset_text(body: &Map<String, Value>, fetched: &str) -> String {
    let mut out = Map::new();
    out.insert("_source".into(), json!(SOURCE));
    out.insert("_fetched".into(), json!(fetched));
    for (k, v) in body {
        out.insert(k.clone(), v.clone());
    }
    output::to_js_json(&Value::Object(out))
}

/// `fetchApiDatasets`: fetch each domain and write it into `out_dir`.
pub fn fetch_api_datasets(
    out_dir: &Path,
    domains: &[&'static ApiDomain],
    options: &UexOptions,
    http: &mut Http<'_>,
    log: &mut dyn FnMut(&str),
) -> Result<Vec<Written>, String> {
    std::fs::create_dir_all(out_dir).map_err(|e| format!("{}: {e}", out_dir.display()))?;
    let mut written = Vec::new();
    for domain in domains {
        log(&format!("  fetching {}", domain.name));
        let body = domain.fetch(options, http)?;
        let file = out_dir.join(format!("{}.json", domain.name));
        std::fs::write(&file, dataset_text(&body, &output::iso_now()))
            .map_err(|e| format!("{}: {e}", file.display()))?;
        let collections = body
            .iter()
            .filter_map(|(k, v)| v.as_array().map(|a| (k.clone(), a.len())))
            .collect();
        written.push(Written {
            dataset: domain.name,
            file,
            collections,
        });
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::uex::HttpResponse;

    /// Answers each endpoint (the URL's last segment) from `rows`, recording the token.
    fn server(
        rows: Value,
        tokens: &mut Vec<Option<String>>,
    ) -> impl FnMut(&str, Option<&str>) -> Result<HttpResponse, String> + '_ {
        move |url, token| {
            tokens.push(token.map(str::to_string));
            let endpoint = url.rsplit('/').next().unwrap();
            Ok(HttpResponse {
                status: 200,
                body: json!({"status": "ok", "data": rows.get(endpoint).cloned().unwrap_or(json!([]))})
                    .to_string(),
            })
        }
    }

    fn no_token() -> UexOptions {
        UexOptions {
            token: Some(String::new()),
            ..Default::default()
        }
    }

    #[test]
    fn refinery_fetches_without_a_token_and_drops_empty_ids_and_reporters() {
        let mut tokens = Vec::new();
        let rows = json!({
            "refineries_yields": [{"id": 1, "id_space_station": 55, "id_city": 0, "id_report": 9, "value": -5, "commodity_name": "Iron (Ore)"}],
            "refineries_capacities": [],
            "refineries_methods": [{"id": 2, "name": "Dinyx Solventation", "rating_yield": 3}],
            "refineries_audits": [{"id": 3, "datarunner": "someone", "quantity_yield": 2417}],
        });
        let out = API_DOMAINS[3]
            .fetch(&no_token(), &mut server(rows, &mut tokens))
            .unwrap();
        assert_eq!(
            out["yields"],
            json!([{"id": 1, "id_space_station": 55, "value": -5, "commodity_name": "Iron (Ore)"}])
        );
        assert_eq!(out["audits"], json!([{"id": 3, "quantity_yield": 2417}]));
        assert_eq!(
            out["summary"],
            json!({"yields": 1, "capacities": 0, "methods": 1, "audits": 1})
        );
        assert!(tokens.iter().all(Option::is_none));
    }

    #[test]
    fn terminals_fetch_without_a_token_and_keep_only_what_places_a_terminal() {
        let mut tokens = Vec::new();
        let rows = json!({"terminals": [{
            "id": 115, "id_moon": 75, "name": "Dumper's Depot - GrimHEX",
            "nickname": "Dumper's GrimHEX", "displayname": "Grim HEX",
            "screenshot": "https://cdn/x.jpg", "moon_name": "Yela", "city_name": null,
            "outpost_name": "", "is_available": 1, "is_available_live": 0, "is_refinery": 0,
        }]});
        let out = API_DOMAINS[0]
            .fetch(&no_token(), &mut server(rows, &mut tokens))
            .unwrap();
        assert_eq!(tokens, vec![None]);
        // The availability zero is a flag, so it stays.
        assert_eq!(
            out["terminals"],
            json!([{
                "id": 115, "name": "Dumper's Depot - GrimHEX", "nickname": "Dumper's GrimHEX",
                "displayname": "Grim HEX", "moon_name": "Yela", "is_available": 1,
                "is_available_live": 0,
            }])
        );
        assert_eq!(out["summary"], json!({"terminals": 1}));
    }

    #[test]
    fn vehicle_prices_join_names_first_and_need_a_token() {
        let rows = json!({
            "vehicles": [{"id": 7, "name": "Gladius", "name_full": "Aegis Gladius"}, {"id": 8, "name": "Arrow", "name_full": null}],
            "vehicles_purchases_prices": [{"id_vehicle": 7, "price_buy": 100, "price_buy_min": 1}],
            "vehicles_rentals_prices": [{"id_vehicle": 8, "price_rent": 5}, {"id_vehicle": 9}],
        });
        let mut tokens = Vec::new();
        assert!(
            API_DOMAINS[2]
                .fetch(&no_token(), &mut server(rows.clone(), &mut tokens))
                .is_err()
        );
        let options = UexOptions {
            token: Some("t".into()),
            ..Default::default()
        };
        let out = API_DOMAINS[2]
            .fetch(&options, &mut server(rows, &mut tokens))
            .unwrap();
        let text = output::to_js_json(&out["purchases"]);
        assert!(text.find("vehicle").unwrap() < text.find("id_vehicle").unwrap());
        assert_eq!(
            out["purchases"],
            json!([{"vehicle": "Aegis Gladius", "id_vehicle": 7, "price_buy": 100}])
        );
        assert_eq!(
            out["rentals"],
            json!([{"vehicle": "Arrow", "id_vehicle": 8, "price_rent": 5}, {"id_vehicle": 9}])
        );
        assert_eq!(
            out["summary"],
            json!({"vehicles": 2, "purchases": 1, "rentals": 2})
        );
    }

    #[test]
    fn item_prices_keep_their_columns_and_count_distinct_items() {
        let rows = json!({"items_prices_all": [
            {"item_name": "A", "item_uuid": "u1", "price_buy": 0, "price_sell": 5, "id_city": 3},
            {"item_name": "A", "item_uuid": "u1", "price_buy": 10},
            {"item_name": "B", "terminal_name": ""},
        ]});
        let mut tokens = Vec::new();
        let out = API_DOMAINS[4]
            .fetch(&no_token(), &mut server(rows, &mut tokens))
            .unwrap();
        assert_eq!(
            out["prices"],
            json!([
                {"item_name": "A", "item_uuid": "u1", "price_sell": 5},
                {"item_name": "A", "item_uuid": "u1", "price_buy": 10},
                {"item_name": "B"},
            ])
        );
        assert_eq!(out["summary"], json!({"prices": 3, "items": 2}));
    }

    #[test]
    fn select_keeps_registry_order() {
        let names = |v: Vec<&ApiDomain>| v.iter().map(|d| d.name).collect::<Vec<_>>();
        assert_eq!(select_api_domains(&[]).len(), 5);
        assert_eq!(
            names(select_api_domains(&[
                "uex-item-prices".into(),
                "uex-terminals".into(),
                "nope".into()
            ])),
            vec!["uex-terminals", "uex-item-prices"]
        );
    }

    #[test]
    fn writes_the_header_first() {
        let dir = tempfile::tempdir().unwrap();
        let mut tokens = Vec::new();
        let mut http = server(json!({"terminals": [{"id": 1}]}), &mut tokens);
        let mut log = Vec::new();
        let written = fetch_api_datasets(
            dir.path(),
            &[&API_DOMAINS[0]],
            &no_token(),
            &mut http,
            &mut |m| log.push(m.to_string()),
        )
        .unwrap();
        assert_eq!(log, vec!["  fetching uex-terminals"]);
        assert_eq!(written[0].collections, vec![("terminals".to_string(), 1)]);
        let text = std::fs::read_to_string(&written[0].file).unwrap();
        assert!(text.starts_with(
            "{\n  \"_source\": \"UEX Corp API (https://uexcorp.space)\",\n  \"_fetched\": \""
        ));
        assert!(!text.ends_with('\n'));
    }
}
