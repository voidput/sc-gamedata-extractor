//! SCMCP's item finder (`SCMCP/src/itemfinder.ts`), so the MCP server can run the Rust
//! query instead of its vendored TypeScript copy and get the same text back.
//!
//! SCMCP calls `sc-gamedata mcp-query` with one JSON request on stdin and returns stdout
//! as the tool reply. The request carries the live UEX rows SCMCP fetched (shop stock is
//! server-side, in no game file), so the Rust side does no network I/O:
//!
//! ```json
//! { "tool": "find" | "obtain" | "where" | "shop", "dataDir": "...", "query": "...",
//!   "kind": "item", "limit": 20, "includeAll": false, "format": "text" | "json",
//!   "at": "grimhex", "ammo": false, "shops": [...], "terminals": [...], "asOf": 1700000000000 }
//! ```
//!
//! Every reply mirrors itemfinder.ts byte for byte: `find` is `JSON.stringify(result,
//! null, 2)` as index.ts sends it, the rest are `obtainText`, `whereText` and `shopText`.
//! Loading follows `assemble`: the search index is used only when its `_build` matches
//! item-tags', and ships are read only without it.

use std::path::Path;

use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::output::to_js_json;
use crate::query::fuzzy::{self, Entry, EntryKind};
use crate::query::obtain::{
    BlueprintLike, Datasets, LootData, LootIndex, ObtainOptions, ObtainReport, WikeloTradeLike,
    ammo_targets, build_loot_index, find_items, obtain, pct, render_obtain, weapons_label,
};
use crate::query::shops::{
    AsOf, SHOP_ITEM_LIMIT, SHOP_TERMINAL_LIMIT, ShopInputs, ShopPrice, UexTerminal, limit_shop,
    render_shop, shop_inventory,
};
use crate::query::types::{ContractAward, ItemTags, LootArea};
use crate::query::where_::{pick_ammo, render_where, where_summary};
use crate::search_index;

const TAGS_FILE: &str = "game-item-tags.json";
const FILES: [&str; 5] = [
    TAGS_FILE,
    "game-loot.json",
    "game-item-sources.json",
    "game-blueprints.json",
    "game-wikelo-trades.json",
];
const SHIPS_FILE: &str = "game-ships.json";

const CAVEATS: &str = "Odds are rough: weight shares along table -> archetype -> entry -> rarity/manufacturer layers, an even pick inside each pool (not stated by the data), and up to maxResults rolls per box. Compare areas; do not read them as drop rates. Area names are preset names; which named locations use each is not in DataCore.";

/// One `mcp-query` request.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub tool: String,
    pub data_dir: String,
    #[serde(default)]
    pub query: String,
    pub kind: Option<String>,
    pub limit: Option<f64>,
    pub max_terminals: Option<usize>,
    #[serde(default)]
    pub include_all: bool,
    pub format: Option<String>,
    pub at: Option<String>,
    #[serde(default)]
    pub ammo: bool,
    /// UEX `items_prices_all` rows; absent when SCMCP could not fetch them.
    pub shops: Option<Vec<ShopPrice>>,
    pub terminals: Option<Vec<UexTerminal>>,
    /// Reference time for the stale flag (ms); now when absent.
    pub as_of: Option<f64>,
}

/// `GameData` in itemfinder.ts.
pub struct GameData {
    pub source: String,
    pub build: Option<String>,
    pub datasets: Datasets,
    ships: Vec<Value>,
    items_raw: Vec<Value>,
    index: Option<LootIndex>,
    pub entries: Vec<Entry>,
    pub entries_from: &'static str,
    search_index: Option<Value>,
    pub missing: Vec<String>,
}

fn read(dir: &Path, file: &str) -> Option<Value> {
    serde_json::from_slice(&std::fs::read(dir.join(file)).ok()?).ok()
}

fn rows(doc: Option<&Value>, key: &str) -> Option<Vec<Value>> {
    doc?.get(key)?.as_array().cloned()
}

fn typed<T: serde::de::DeserializeOwned>(doc: Option<&Value>, key: &str) -> Option<T> {
    let v = doc?.get(key)?;
    if !v.is_array() {
        return None;
    }
    serde_json::from_value(v.clone()).ok()
}

/// Entries from rows, as `buildEntries(items, ships, { all })`.
fn built_entries(items: &[Value], ships: &[Value], all: bool) -> Vec<Entry> {
    let doc = search_index::build_search_index(items, ships, None);
    fuzzy::from_index(&search_index::to_value(&doc), all).unwrap_or_default()
}

/// `assemble` over a local directory. `None` when it has no item tags, the case SCMCP
/// sends to its dump fallback instead.
pub fn load(dir: &str) -> Option<GameData> {
    let path = Path::new(dir);
    let docs: Vec<Option<Value>> = FILES.iter().map(|f| read(path, f)).collect();
    let tags = docs[0].as_ref()?;
    let mut missing: Vec<String> = FILES
        .iter()
        .zip(&docs)
        .filter(|(_, d)| d.is_none())
        .map(|(f, _)| f.to_string())
        .collect();
    let build = tags
        .get("_build")
        .and_then(Value::as_str)
        .map(str::to_string);
    let index_doc = read(path, search_index::FILE);
    let from_index = match (&build, &index_doc) {
        (Some(b), Some(doc)) if doc.get("_build").and_then(Value::as_str) == Some(b) => {
            fuzzy::from_index(doc, false).ok()
        }
        _ => None,
    };
    let ships = if from_index.is_some() {
        Vec::new()
    } else {
        let file = read(path, SHIPS_FILE);
        if file.is_none() {
            missing.push(SHIPS_FILE.into());
        }
        rows(file.as_ref(), "vehicles").unwrap_or_default()
    };
    let items_raw = rows(Some(tags), "items").unwrap_or_default();
    let loot = docs[1]
        .as_ref()
        .filter(|d| d.get("tables").is_some_and(Value::is_array))
        .and_then(|d| serde_json::from_value::<LootData>(d.clone()).ok());
    let datasets = Datasets {
        items: typed::<Vec<ItemTags>>(Some(tags), "items").unwrap_or_default(),
        loot,
        areas: typed::<Vec<LootArea>>(docs[2].as_ref(), "areas"),
        contract_awards: typed::<Vec<ContractAward>>(docs[2].as_ref(), "contractAwards"),
        blueprints: typed::<Vec<BlueprintLike>>(docs[3].as_ref(), "blueprints"),
        wikelo: typed::<Vec<WikeloTradeLike>>(docs[4].as_ref(), "trades"),
        ..Default::default()
    };
    let index = datasets.loot.as_ref().map(build_loot_index);
    let (entries, entries_from, search_index) = match from_index {
        Some(e) => (e, "search-index", index_doc),
        None => (built_entries(&items_raw, &ships, false), "datasets", None),
    };
    Some(GameData {
        source: dir.to_string(),
        build,
        datasets,
        ships,
        items_raw,
        index,
        entries,
        entries_from,
        search_index,
        missing,
    })
}

fn opt_str(map: &mut Map<String, Value>, key: &str, v: &Option<String>) {
    if let Some(s) = v {
        map.insert(key.into(), json!(s));
    }
}

fn to_value<T: serde::Serialize>(v: &T) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Null)
}

/// `find`: the object index.ts stringifies.
pub fn find(
    data: &GameData,
    query: &str,
    kind: Option<&str>,
    limit: Option<f64>,
    all: bool,
) -> Value {
    let wide;
    let entries = if !all {
        &data.entries
    } else {
        wide = match &data.search_index {
            Some(doc) => fuzzy::from_index(doc, true).unwrap_or_default(),
            None => built_entries(&data.items_raw, &data.ships, true),
        };
        &wide
    };
    let ranked: Vec<&Entry> = fuzzy::rank_entries(entries, query)
        .into_iter()
        .filter(|e| kind.is_none_or(|k| kind_str(e.kind) == k))
        .collect();
    let limit = limit.unwrap_or(20.0).clamp(1.0, 200.0).trunc() as usize;
    let results: Vec<Value> = ranked
        .iter()
        .take(limit)
        .map(|e| {
            let mut m = Map::new();
            m.insert("kind".into(), json!(kind_str(e.kind)));
            m.insert("key".into(), json!(e.key));
            m.insert("name".into(), json!(e.name));
            opt_str(&mut m, "type", &e.ty);
            opt_str(&mut m, "maker", &e.maker);
            Value::Object(m)
        })
        .collect();
    let mut out = Map::new();
    out.insert("source".into(), json!(data.source));
    opt_str(&mut out, "build", &data.build);
    out.insert("entriesFrom".into(), json!(data.entries_from));
    out.insert("total".into(), json!(ranked.len()));
    out.insert("results".into(), Value::Array(results));
    Value::Object(out)
}

fn kind_str(k: EntryKind) -> &'static str {
    match k {
        EntryKind::Item => "item",
        EntryKind::Ship => "ship",
    }
}

/// `resolveItem`: an exact name or class name, else the only fuzzy match, else candidates.
fn resolve_item<'a>(data: &'a GameData, query: &str) -> (Option<&'a ItemTags>, Vec<&'a Entry>) {
    let q = query.trim().to_lowercase();
    let exact: Vec<&ItemTags> = find_items(&data.datasets.items, query)
        .into_iter()
        .filter(|i| i.class_name.to_lowercase() == q || i.name.to_lowercase() == q)
        .collect();
    if exact.len() == 1 {
        return (Some(exact[0]), Vec::new());
    }
    let items: Vec<Entry> = data
        .entries
        .iter()
        .filter(|e| e.kind == EntryKind::Item)
        .cloned()
        .collect();
    let ranked: Vec<String> = fuzzy::rank_entries(&items, query)
        .iter()
        .map(|e| e.key.clone())
        .collect();
    let by_key = |k: &str| {
        data.entries
            .iter()
            .find(|e| e.kind == EntryKind::Item && e.key == k)
    };
    if ranked.len() == 1 {
        let item = data
            .datasets
            .items
            .iter()
            .find(|i| i.class_name == ranked[0]);
        return (item, Vec::new());
    }
    (
        None,
        ranked.iter().take(10).filter_map(|k| by_key(k)).collect(),
    )
}

/// `weaponsFor`: every word match that loads something, else the resolved item, else the
/// best fuzzy hit that loads something.
fn weapons_for<'a>(data: &'a GameData, query: &str) -> Vec<&'a ItemTags> {
    let loads = |i: &ItemTags| i.ammo_items.as_ref().is_some_and(|a| !a.is_empty());
    let words: Vec<&ItemTags> = find_items(&data.datasets.items, query)
        .into_iter()
        .filter(|i| loads(i))
        .collect();
    if !words.is_empty() {
        return words;
    }
    let (item, candidates) = resolve_item(data, query);
    if let Some(i) = item.filter(|i| loads(i)) {
        return vec![i];
    }
    candidates
        .iter()
        .find_map(|c| {
            data.datasets
                .items
                .iter()
                .find(|i| i.class_name == c.key)
                .filter(|i| loads(i))
        })
        .map(|i| vec![i])
        .unwrap_or_default()
}

struct Resolved<'a> {
    reports: Vec<ObtainReport>,
    candidates: Vec<&'a Entry>,
    dropped: Option<Vec<String>>,
    problem: Option<String>,
}

fn with_shops(data: &GameData, req: &Request) -> Datasets {
    Datasets {
        shops: req.shops.clone(),
        terminals: req.terminals.clone(),
        shops_as_of: req.as_of.map(AsOf::Millis),
        ..data.datasets.clone()
    }
}

fn resolve_reports<'a>(data: &'a GameData, req: &Request) -> Resolved<'a> {
    let datasets = with_shops(data, req);
    let at = req.at.as_deref();
    if !req.ammo {
        let (item, candidates) = resolve_item(data, &req.query);
        let reports = item
            .map(|i| {
                vec![obtain(
                    &datasets,
                    i,
                    data.index.as_ref(),
                    &ObtainOptions { at, ammo_for: &[] },
                )]
            })
            .unwrap_or_default();
        return Resolved {
            reports,
            candidates,
            dropped: None,
            problem: None,
        };
    }
    let weapons = weapons_for(data, &req.query);
    let targets = ammo_targets(&data.datasets.items, &weapons);
    if targets.is_empty() {
        return Resolved {
            reports: Vec::new(),
            candidates: Vec::new(),
            dropped: None,
            problem: Some(format!(
                "No weapon matching \"{}\" names a magazine or canister (needs an item-tags dataset with ammoItems). Try sc_find, or drop ammo.",
                req.query
            )),
        };
    }
    let reports: Vec<ObtainReport> = targets
        .iter()
        .map(|t| {
            obtain(
                &datasets,
                t.ammo,
                data.index.as_ref(),
                &ObtainOptions {
                    at,
                    ammo_for: &t.weapons,
                },
            )
        })
        .collect();
    let shops_loaded = req.shops.is_some();
    let summaries: Vec<_> = reports
        .iter()
        .map(|r| where_summary(r, Some(shops_loaded)))
        .collect();
    let (keep, dropped) = pick_ammo(&summaries);
    let dropped_names: Vec<String> = dropped
        .iter()
        .map(|&i| reports[i].item.class_name.clone())
        .collect();
    let mut slots: Vec<Option<ObtainReport>> = reports.into_iter().map(Some).collect();
    Resolved {
        reports: keep.iter().filter_map(|&i| slots[i].take()).collect(),
        candidates: Vec::new(),
        dropped: (!dropped_names.is_empty()).then_some(dropped_names),
        problem: None,
    }
}

fn dropped_line(r: &Resolved) -> String {
    match &r.dropped {
        Some(d) => format!(
            "\nleft out, no known source: {} more compatible magazine(s): {}",
            d.len(),
            d.join(", ")
        ),
        None => String::new(),
    }
}

fn source_line(data: &GameData) -> String {
    format!(
        "source: {}{}{}",
        data.source,
        data.build
            .as_ref()
            .map(|b| format!(" (build {b})"))
            .unwrap_or_default(),
        if data.missing.is_empty() {
            String::new()
        } else {
            format!("; missing: {}", data.missing.join(", "))
        }
    )
}

fn unresolved(data: &GameData, query: &str, r: &Resolved) -> String {
    let header = source_line(data);
    if let Some(p) = &r.problem {
        return format!("{p}\n{header}");
    }
    if r.candidates.is_empty() {
        return format!("No item matches \"{query}\". Try sc_find.\n{header}");
    }
    let list: Vec<String> = r
        .candidates
        .iter()
        .map(|c| {
            format!(
                "  {}  {}  ({}{})",
                c.key,
                c.name,
                c.ty.as_deref().unwrap_or("?"),
                c.maker
                    .as_deref()
                    .filter(|m| !m.is_empty())
                    .map(|m| format!(", {m}"))
                    .unwrap_or_default()
            )
        })
        .collect();
    format!(
        "\"{query}\" matches several items; call again with one key:\n{}\n{header}",
        list.join("\n")
    )
}

/// `compactReport`: the best spawn per area, no per-path detail.
fn compact_report(r: &ObtainReport, area_limit: usize) -> Value {
    let mut item = Map::new();
    item.insert("key".into(), json!(r.item.class_name));
    item.insert("name".into(), json!(r.item.name));
    opt_str(&mut item, "maker", &r.item.manufacturer);
    opt_str(&mut item, "type", &r.item.ty);
    let areas: Vec<Value> = r
        .areas
        .iter()
        .take(area_limit)
        .map(|a| {
            let b = &a.best;
            let mut m = Map::new();
            m.insert("area".into(), json!(a.area));
            opt_str(&mut m, "spawnPoint", &b.config);
            m.insert(
                "box".into(),
                json!(b.entity_name.as_ref().unwrap_or(&b.entity)),
            );
            m.insert("kind".into(), to_value(&b.kind));
            opt_str(&mut m, "table", &b.table);
            m.insert("boxShare".into(), json!(pct(b.slot_share)));
            if let Some(p) = b.per_roll {
                m.insert("perRoll".into(), json!(pct(p)));
            }
            m.insert("odds".into(), json!(pct(b.score)));
            Value::Object(m)
        })
        .collect();
    let other: Vec<Value> = r
        .other_tables
        .iter()
        .take(10)
        .map(|t| json!({ "table": t.table, "perRoll": pct(t.per_roll) }))
        .collect();
    let blueprints: Vec<Value> = r
        .blueprints
        .iter()
        .map(|b| {
            let mut missions: Vec<&str> = Vec::new();
            for rw in b.rewarded_by.iter().flatten() {
                if let Some(m) = rw.mission.as_deref().or(rw.contract.as_deref())
                    && !m.is_empty()
                    && !missions.contains(&m)
                {
                    missions.push(m);
                }
            }
            json!({ "name": b.display_name.as_ref().unwrap_or(&b.name), "missions": missions })
        })
        .collect();
    let awards: Vec<Value> = r
        .contract_awards
        .iter()
        .map(|a| {
            let mut m = Map::new();
            m.insert("contract".into(), json!(a.contract));
            if let Some(n) = a.amount {
                m.insert("amount".into(), json!(n));
            }
            Value::Object(m)
        })
        .collect();
    let mut shops: Vec<_> = r
        .shops
        .iter()
        .filter(|s| s.row.price_buy.is_some_and(|p| p != 0.0 && !p.is_nan()))
        .collect();
    shops.sort_by(|a, b| {
        let p = |s: &&crate::query::shops::LocatedShop| s.row.price_buy.unwrap_or(0.0);
        p(a).partial_cmp(&p(b)).unwrap_or(std::cmp::Ordering::Equal)
    });
    let shops: Vec<Value> = shops
        .iter()
        .map(|s| {
            let mut m = Map::new();
            m.insert("terminal".into(), json!(s.terminal));
            m.insert("location".into(), json!(s.location));
            m.insert("buy".into(), to_value(&s.row.price_buy));
            opt_str(&mut m, "updated", &s.updated);
            m.insert("stale".into(), json!(s.stale));
            if s.live == Some(false) {
                m.insert("live".into(), json!(false));
            }
            Value::Object(m)
        })
        .collect();

    let mut out = Map::new();
    out.insert("item".into(), Value::Object(item));
    out.insert("lootable".into(), to_value(&r.eligibility));
    out.insert("bestAreas".into(), Value::Array(areas));
    out.insert("areasTotal".into(), json!(r.areas.len()));
    out.insert("otherTables".into(), Value::Array(other));
    out.insert("blueprints".into(), Value::Array(blueprints));
    if let Some(w) = &r.ammo_for {
        let keys: Vec<&str> = w.iter().map(|w| w.class_name.as_str()).collect();
        out.insert("ammoFor".into(), json!(keys));
    }
    out.insert("contractAwards".into(), Value::Array(awards));
    let wikelo: Vec<&str> = r.wikelo_inferred.iter().map(|t| t.name.as_str()).collect();
    out.insert("wikeloInferred".into(), json!(wikelo));
    out.insert("shops".into(), Value::Array(shops));
    if let Some(at) = r.at.as_ref().filter(|a| !a.is_empty()) {
        out.insert("shopsAt".into(), json!(at));
        if let Some(n) = r.shops_elsewhere {
            out.insert("shopsElsewhere".into(), json!(n));
        }
    }
    out.insert("caveats".into(), json!(CAVEATS));
    Value::Object(out)
}

fn meta(out: &mut Map<String, Value>, data: &GameData) {
    out.insert("source".into(), json!(data.source));
    opt_str(out, "build", &data.build);
}

/// A positive whole limit, as the TypeScript `slice` would use it; SCMCP sends nothing else.
fn whole(limit: Option<f64>, fallback: usize) -> usize {
    limit
        .filter(|l| l.is_finite() && *l >= 1.0)
        .map_or(fallback, |l| l.trunc() as usize)
}

/// `obtainText`.
pub fn obtain_text(data: &GameData, req: &Request) -> String {
    let resolved = resolve_reports(data, req);
    if resolved.reports.is_empty() {
        return unresolved(data, &req.query, &resolved);
    }
    let limit = whole(req.limit, 10);
    let shops_loaded = req.shops.is_some();
    if req.format.as_deref() == Some("json") {
        let mut out = Map::new();
        if req.ammo {
            let names: Vec<&str> = resolved.reports[0]
                .ammo_for
                .iter()
                .flatten()
                .map(|w| w.name.as_str())
                .collect();
            out.insert("ammoFor".into(), json!(weapons_label(&names)));
            let list: Vec<Value> = resolved
                .reports
                .iter()
                .map(|r| compact_report(r, limit))
                .collect();
            out.insert("reports".into(), Value::Array(list));
            if let Some(d) = &resolved.dropped {
                out.insert("droppedNoSource".into(), json!(d));
            }
        } else {
            if let Value::Object(o) = compact_report(&resolved.reports[0], limit) {
                out.extend(o);
            }
            let also: Vec<&str> = resolved.candidates.iter().map(|c| c.key.as_str()).collect();
            out.insert("alsoMatched".into(), json!(also));
        }
        meta(&mut out, data);
        return to_js_json(&Value::Object(out));
    }
    let mut also = String::new();
    if !resolved.candidates.is_empty() {
        let keys: Vec<&str> = resolved.candidates.iter().map(|c| c.key.as_str()).collect();
        also = format!("\nalso matched: {}", keys.join(", "));
    }
    also.push_str(&dropped_line(&resolved));
    let text: Vec<String> = resolved
        .reports
        .iter()
        .map(|r| render_obtain(r, limit, Some(shops_loaded)))
        .collect();
    format!("{}{also}\n{}", text.join("\n\n"), source_line(data))
}

/// `whereText`.
pub fn where_text(data: &GameData, req: &Request) -> String {
    let resolved = resolve_reports(data, req);
    if resolved.reports.is_empty() {
        return unresolved(data, &req.query, &resolved);
    }
    let shops_loaded = req.shops.is_some();
    let summaries: Vec<_> = resolved
        .reports
        .iter()
        .map(|r| where_summary(r, Some(shops_loaded)))
        .collect();
    let also: Vec<&str> = resolved.candidates.iter().map(|c| c.key.as_str()).collect();
    if req.format.as_deref() == Some("json") {
        let mut out = Map::new();
        out.insert("summaries".into(), to_value(&summaries));
        if !also.is_empty() {
            out.insert("alsoMatched".into(), json!(also));
        }
        if let Some(d) = &resolved.dropped {
            out.insert("droppedNoSource".into(), json!(d));
        }
        out.insert(
            "shops".into(),
            json!(if shops_loaded {
                "UEX, player-reported"
            } else {
                "not loaded"
            }),
        );
        meta(&mut out, data);
        return to_js_json(&Value::Object(out));
    }
    let mut lines = vec![
        summaries
            .iter()
            .map(render_where)
            .collect::<Vec<_>>()
            .join("\n\n"),
    ];
    if !also.is_empty() {
        lines.push(format!("also matched: {}", also.join(", ")));
    }
    if resolved.dropped.is_some() {
        lines.push(dropped_line(&resolved).trim_start().to_string());
    }
    if shops_loaded {
        lines.push("shop rows: UEX, player-reported, not game files".into());
    }
    lines.push(source_line(data));
    lines.join("\n")
}

/// `shopText`. `data` is optional: a missing game dataset must not stop a shop listing.
pub fn shop_text(data: Option<&GameData>, req: &Request) -> Result<String, String> {
    let shops = req
        .shops
        .as_deref()
        .ok_or("UEX item prices are unavailable right now.")?;
    let report = shop_inventory(
        &ShopInputs {
            shops,
            terminals: req.terminals.as_deref().unwrap_or(&[]),
            items: data.map_or(&[][..], |d| &d.datasets.items[..]),
            as_of: req.as_of.map(AsOf::Millis),
            stale_days: None,
        },
        &req.query,
    );
    let note = format!(
        "source: UEX (player-reported){}{}",
        if req.terminals.is_some() {
            ""
        } else {
            "; terminals unavailable, only terminal names matched"
        },
        data.map(|d| format!("; names from {}", d.source))
            .unwrap_or_default()
    );
    let limit = Some(whole(req.limit, SHOP_ITEM_LIMIT));
    let max = Some(req.max_terminals.unwrap_or(SHOP_TERMINAL_LIMIT));
    if req.format.as_deref() == Some("json") {
        let mut out = match to_value(&limit_shop(&report, limit, max)) {
            Value::Object(o) => o,
            _ => Map::new(),
        };
        out.insert("note".into(), json!(note));
        return Ok(to_js_json(&Value::Object(out)));
    }
    Ok(format!("{}\n{note}", render_shop(&report, limit, max)))
}

/// Answer one request: the reply text, or an error message for SCMCP to raise.
pub fn answer(req: &Request) -> Result<String, String> {
    let data = load(&req.data_dir);
    if req.tool == "shop" {
        return shop_text(data.as_ref(), req);
    }
    let data = data.ok_or_else(|| format!("no {TAGS_FILE} in {}", req.data_dir))?;
    match req.tool.as_str() {
        "find" => Ok(to_js_json(&find(
            &data,
            &req.query,
            req.kind.as_deref(),
            req.limit,
            req.include_all,
        ))),
        "obtain" => Ok(obtain_text(&data, req)),
        "where" => Ok(where_text(&data, req)),
        other => Err(format!("unknown mcp-query tool \"{other}\"")),
    }
}
