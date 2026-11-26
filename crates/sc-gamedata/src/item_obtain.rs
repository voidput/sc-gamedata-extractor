//! `game-item-obtain.json`: where each item comes from, worked out once per build
//! (spec `dump-enhancements.md` item 3).
//!
//! Derived from datasets, not game files, like [`crate::search_index`]: it needs
//! `game-item-tags.json`, `game-loot.json` and `game-item-sources.json` beside it (and reads
//! `game-blueprints.json` and `game-wikelo-trades.json` when they are there), is rebuilt
//! after every extract that leaves them on disk and removed when one is missing, so it never
//! restates older inputs.
//!
//! The odds are [`crate::query::obtain`]'s ([`BulkObtain`]: memoised pool sizes, one roll
//! per loot group, items with the same tags rolled once, in parallel), the same numbers the
//! starorg-codex app's WHERE TO GET hints show.
//!
//! Spawn points and table names repeat across thousands of items, so they are listed once
//! (`spawns[]`, `tables[]`) and rows point at them by index; rows are arrays named by
//! `lootColumns` / `otherTableColumns` (the search index does the same). Rows of one item
//! that differ only in that index are merged: the first column is the sorted list of
//! indices (`spawns` / `tables`), and merged rows are ranked by score (per-roll odds for
//! `otherTables`) descending, then by their first index. `areasTotal` / `otherTablesTotal`
//! still count areas and tables, not rows. Shape:
//!
//! ```text
//! { _source, _extracted, _build, _schema, _assumptions[],
//!   lootColumns: ["spawns", "kind", "scoreEstimate", "perRollEstimate"],
//!   otherTableColumns: ["tables", "perRollEstimate"],
//!   spawns[]{area, config, entity, entityName, table, slotShare, fillChance, chance,
//!            maxResults, contents, size, rarity, raritySource},
//!   tables[],
//!   items: { <className>: {
//!     eligibility{canLoot, reason, rarity},
//!     loot[[spawns[], kind, scoreEstimate, perRollEstimate]], (best spawn of the top 12 areas)
//!     areasTotal, otherTables[[tables[], perRollEstimate]], otherTablesTotal,
//!     contractAwards[]{generator, contract, amount, weighting},
//!     wikelo[]{trade, amount, source}, blueprints[]{recordName, rewardContracts} } },
//!   summary }
//! ```
//!
//! Only items with something to say are listed (a source, or a loot flag). Names, class
//! and record names only: no prose, nothing UEX-derived. Odds keep 4 significant digits.

use crate::output::{to_js_json, to_js_json_compact};
use crate::query::obtain::{
    AreaSummary, BlueprintLike, BulkObtain, Datasets, LootData, RollPath, SpawnKind,
    build_loot_index, loot_eligibility, used_tables,
};
use crate::query::types::{ContractAward, ItemTags, LootArea};
use rayon::prelude::*;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

/// Output base name.
pub const NAME: &str = "game-item-obtain";
/// Output file name.
pub const FILE: &str = "game-item-obtain.json";
/// `_schema` version (`game-item-obtain@1`; bump rule:
/// [`crate::domain::Domain::schema_version`]).
pub const SCHEMA_VERSION: u32 = 1;
/// Loot rows (areas) kept per item; the rest are counted in `areasTotal`.
pub const MAX_LOOT: usize = 12;
/// Tables no area rolls kept per item; the rest are counted in `otherTablesTotal`.
pub const MAX_OTHER_TABLES: usize = 12;
const SOURCE: &str = "Derived from game-item-tags, game-loot, game-item-sources, game-blueprints and game-wikelo-trades";

/// What the odds rest on that the data does not state.
pub const ASSUMPTIONS: &[&str] = &[
    "perRollEstimate: one roll picks evenly among the lootable items of the pool it reaches (the item's share of the table's weights divided by the pool size); the game files give weights only down to the tag selector.",
    "scoreEstimate: odds one spawned container (or rack, or loose spot) of the area's best spawn holds the item: slotShare x fillChance x chance x (1 - (1 - perRollEstimate)^maxResults); fixed and loose spawns are slotShare x fillChance.",
    "Random loot is opt-in: only items tagged LootGeneration/CanGenerateAsLoot roll from tables; fixed and loose spawns ignore the flag.",
    "rarity: the spawn's LootRarity tag, else (raritySource: name) the last word of the spawn config's name.",
    "loot / otherTables rows of one item that differ only in the spawn or table index are one row whose first column (spawns / tables) lists the indices, sorted; rows rank by scoreEstimate (perRollEstimate for otherTables) descending, then first index. areasTotal / otherTablesTotal count areas and tables, not rows.",
];

const RARITIES: &[&str] = &["Common", "Uncommon", "Rare", "Epic", "Legendary"];

/// What [`write_item_obtain`] wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    pub file: PathBuf,
    pub items: usize,
    pub bytes: usize,
}

/// `x` to 4 significant digits (the app shows 2), so the file carries no float noise.
fn sig(x: f64) -> f64 {
    if x == 0.0 || !x.is_finite() {
        return x;
    }
    format!("{x:.3e}").parse().unwrap_or(x)
}

/// The value after `LootGeneration/<what>/` in spawn tags (`LootSize` -> `Large`).
fn spawn_tag<'a>(tags: &'a [String], what: &str) -> Option<&'a str> {
    let p = format!("LootGeneration/{what}/");
    tags.iter().find_map(|t| t.strip_prefix(p.as_str()))
}

fn kind_name(k: SpawnKind) -> &'static str {
    match k {
        SpawnKind::Loot => "loot",
        SpawnKind::Fixed => "fixed",
        SpawnKind::Loose => "loose",
    }
}

/// The spawn point of an area's best spawn (the codex `loot_container` reads, as data):
/// what [`AreaSummary::best`] says that does not depend on the item.
fn spawn_of(a: &AreaSummary, area: Option<&LootArea>) -> Value {
    let h = &a.best;
    let mut row = Map::new();
    row.insert("area".into(), json!(a.area));
    if let Some(c) = &h.config {
        row.insert("config".into(), json!(c));
    }
    row.insert("entity".into(), json!(h.entity));
    if let Some(n) = &h.entity_name {
        row.insert("entityName".into(), json!(n));
    }
    if let Some(t) = &h.table {
        row.insert("table".into(), json!(t));
    }
    row.insert("slotShare".into(), json!(sig(h.slot_share)));
    // The config's fill chance and the slot's chance to roll, as the odds use them.
    let config = area.and_then(|ar| {
        ar.configs
            .iter()
            .find(|c| c.name == h.config && c.spawn_tags == h.spawn_tags)
    });
    if let Some(f) = config.and_then(|c| c.fill_chance) {
        row.insert("fillChance".into(), json!(f));
    }
    if h.kind == SpawnKind::Loot {
        let chance = config.and_then(|c| {
            c.slots
                .iter()
                .find(|s| {
                    s.entity == h.entity
                        && s.loot
                            .as_ref()
                            .is_some_and(|l| Some(&l.table) == h.table.as_ref())
                })
                .and_then(|s| s.loot.as_ref()?.chance)
        });
        if let Some(c) = chance {
            row.insert("chance".into(), json!(c));
        }
    }
    if let Some(m) = h.max_results {
        row.insert("maxResults".into(), json!(m));
    }
    if let Some(c) = spawn_tag(&h.spawn_tags, "LootContents") {
        row.insert("contents".into(), json!(c));
    }
    if let Some(z) = spawn_tag(&h.spawn_tags, "LootSize") {
        row.insert("size".into(), json!(z));
    }
    match spawn_tag(&h.spawn_tags, "LootRarity") {
        Some(r) => {
            row.insert("rarity".into(), json!(r));
        }
        None => {
            let from_name = h.config.as_deref().and_then(|c| {
                let last = c.rsplit(' ').next()?;
                RARITIES.iter().find(|r| r.eq_ignore_ascii_case(last))
            });
            if let Some(r) = from_name {
                row.insert("rarity".into(), json!(r));
                row.insert("raritySource".into(), json!("name"));
            }
        }
    }
    Value::Object(row)
}

/// The columns of one loot row after the spawn index: `[kind, scoreEstimate,
/// perRollEstimate]` (no odds per roll for fixed and loose spawns).
fn loot_rest(a: &AreaSummary) -> Vec<Value> {
    let h = &a.best;
    let mut row = vec![json!(kind_name(h.kind)), json!(sig(h.score))];
    if let Some(p) = h.per_roll {
        row.push(json!(sig(p)));
    }
    row
}

/// Merge `(index, rest)` rows whose `rest` is equal into `[[indices...], ...rest]`, the
/// indices sorted; rows ranked by `rest[rank]` descending, then by their first index.
fn merge_rows(rows: impl IntoIterator<Item = (usize, Vec<Value>)>, rank: usize) -> Vec<Value> {
    let mut groups: Vec<(Vec<usize>, Vec<Value>)> = Vec::new();
    let mut by_rest: HashMap<String, usize> = HashMap::new();
    for (ix, rest) in rows {
        let key = to_js_json_compact(&Value::Array(rest.clone()));
        match by_rest.get(&key) {
            Some(&g) => groups[g].0.push(ix),
            None => {
                by_rest.insert(key, groups.len());
                groups.push((vec![ix], rest));
            }
        }
    }
    for g in &mut groups {
        g.0.sort_unstable();
    }
    let num = |r: &[Value]| r.get(rank).and_then(Value::as_f64).unwrap_or(0.0);
    groups.sort_by(|a, b| {
        num(&b.1)
            .partial_cmp(&num(&a.1))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.0[0].cmp(&b.0[0]))
    });
    groups
        .into_iter()
        .map(|(ixs, rest)| {
            let mut row = vec![json!(ixs)];
            row.extend(rest);
            Value::Array(row)
        })
        .collect()
}

fn award_row(a: &ContractAward) -> Value {
    let mut row = Map::new();
    row.insert("generator".into(), json!(a.generator));
    row.insert("contract".into(), json!(a.contract));
    if let Some(n) = a.amount {
        row.insert("amount".into(), json!(n));
    }
    if let Some(w) = a.weighting {
        row.insert("weighting".into(), json!(w));
    }
    Value::Object(row)
}

/// The inputs as read from disk: datasets plus the Wikelo trades in full (their resolved
/// `rewards[]`, which [`Datasets::wikelo`] does not keep).
pub struct Inputs {
    pub datasets: Datasets,
    pub trades: Vec<Value>,
    pub extracted: Option<Value>,
    pub build: Option<Value>,
}

/// Wikelo rows of one item: trades whose resolved rewards hand it out, else (no award
/// record) trades whose name implies it (`source: "inferred"`).
fn wikelo_rows(trades: &[Value], cls: &str) -> Vec<Value> {
    let mut out = Vec::new();
    for t in trades {
        let name = t.get("name").and_then(Value::as_str).unwrap_or("");
        for r in t
            .get("rewards")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if r.get("item")
                .and_then(Value::as_str)
                .is_some_and(|i| i.eq_ignore_ascii_case(cls))
            {
                let mut row = Map::new();
                row.insert("trade".into(), json!(name));
                if let Some(a) = r.get("amount") {
                    row.insert("amount".into(), a.clone());
                }
                if let Some(s) = r.get("source") {
                    row.insert("source".into(), s.clone());
                }
                out.push(Value::Object(row));
            }
        }
    }
    out
}

/// Squashed name, as `obtain` matches an inferred Wikelo reward.
fn squash(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        .collect()
}

/// The dataset body (`items`, `summary`) for every non-template item.
pub fn build_body(inputs: &Inputs) -> Map<String, Value> {
    let d = &inputs.datasets;
    let index = d.loot.as_ref().map(build_loot_index);
    let bulk = BulkObtain::new(d, index.as_ref());
    let used = used_tables(d);
    let list: Vec<&ItemTags> = d.items.iter().filter(|i| !i.template()).collect();
    let all_areas = bulk.areas_all(&list);
    let others: Vec<Vec<RollPath>> = list
        .par_iter()
        .map(|i| bulk.other_tables(i, &used))
        .collect();

    let mut awards_by: HashMap<String, Vec<&ContractAward>> = HashMap::new();
    for a in d.contract_awards.iter().flatten() {
        awards_by.entry(a.item.to_lowercase()).or_default().push(a);
    }
    let mut blueprints_by: HashMap<String, Vec<&BlueprintLike>> = HashMap::new();
    for b in d.blueprints.iter().flatten() {
        if let Some(p) = &b.produces {
            blueprints_by.entry(p.to_lowercase()).or_default().push(b);
        }
    }

    // Spawn points of every kept row, once each, in area order; table names likewise.
    let area_of: HashMap<&str, (usize, &LootArea)> = d
        .areas
        .iter()
        .flatten()
        .enumerate()
        .map(|(i, a)| (a.name.as_str(), (i, a)))
        .collect();
    let mut spawn_set: BTreeMap<(usize, String), Value> = BTreeMap::new();
    let keys: Vec<Vec<(usize, String)>> = all_areas
        .iter()
        .map(|areas| {
            areas
                .iter()
                .take(MAX_LOOT)
                .map(|a| {
                    let (ix, area) = area_of
                        .get(a.area.as_str())
                        .map_or((usize::MAX, None), |(i, ar)| (*i, Some(*ar)));
                    let v = spawn_of(a, area);
                    let key = (ix, to_js_json_compact(&v));
                    spawn_set.entry(key.clone()).or_insert(v);
                    key
                })
                .collect()
        })
        .collect();
    let spawn_ix: HashMap<&(usize, String), usize> =
        spawn_set.keys().enumerate().map(|(i, k)| (k, i)).collect();
    let table_names: BTreeSet<&str> = others
        .iter()
        .flat_map(|o| o.iter().take(MAX_OTHER_TABLES))
        .map(|p| p.table.as_str())
        .collect();
    let table_ix: HashMap<&str, usize> = table_names
        .iter()
        .enumerate()
        .map(|(i, t)| (*t, i))
        .collect();

    let mut items = Map::new();
    let mut counts = [0usize; 9];
    for (((item, areas), other), keys) in list.iter().zip(&all_areas).zip(&others).zip(&keys) {
        let cls = item.class_name.to_lowercase();
        let mut row = Map::new();
        let el = loot_eligibility(&item.tags);
        if el != Default::default() {
            row.insert(
                "eligibility".into(),
                serde_json::to_value(&el).unwrap_or_default(),
            );
        }
        if !areas.is_empty() {
            let loot = merge_rows(
                areas
                    .iter()
                    .zip(keys)
                    .map(|(a, k)| (spawn_ix[k], loot_rest(a))),
                1,
            );
            // Rows written, after merging.
            counts[8] += loot.len();
            row.insert("loot".into(), Value::Array(loot));
            row.insert("areasTotal".into(), json!(areas.len()));
            counts[0] += 1;
            for (k, slot) in [
                (SpawnKind::Loot, 1),
                (SpawnKind::Fixed, 2),
                (SpawnKind::Loose, 3),
            ] {
                if areas.iter().any(|a| a.best.kind == k) {
                    counts[slot] += 1;
                }
            }
        }
        if !other.is_empty() {
            row.insert(
                "otherTables".into(),
                Value::Array(merge_rows(
                    other
                        .iter()
                        .take(MAX_OTHER_TABLES)
                        .map(|p| (table_ix[p.table.as_str()], vec![json!(sig(p.per_roll))])),
                    0,
                )),
            );
            if other.len() > MAX_OTHER_TABLES {
                row.insert("otherTablesTotal".into(), json!(other.len()));
            }
            counts[4] += 1;
        }
        let awards = awards_by.get(&cls).map(Vec::as_slice).unwrap_or_default();
        if !awards.is_empty() {
            row.insert(
                "contractAwards".into(),
                Value::Array(awards.iter().map(|a| award_row(a)).collect()),
            );
            counts[5] += 1;
        }
        let mut wikelo = wikelo_rows(&inputs.trades, &cls);
        if wikelo.is_empty() && awards.is_empty() {
            let key = squash(&item.name);
            if key.chars().count() > 3 {
                for t in &inputs.trades {
                    let inferred = t.get("rewardInferred").and_then(Value::as_str);
                    if inferred.is_some_and(|r| !r.is_empty() && squash(r) == key) {
                        wikelo.push(json!({
                            "trade": t.get("name").and_then(Value::as_str).unwrap_or(""),
                            "source": "inferred",
                        }));
                    }
                }
            }
        }
        if !wikelo.is_empty() {
            row.insert("wikelo".into(), Value::Array(wikelo));
            counts[6] += 1;
        }
        let bps = blueprints_by
            .get(&cls)
            .map(Vec::as_slice)
            .unwrap_or_default();
        if !bps.is_empty() {
            row.insert(
                "blueprints".into(),
                Value::Array(
                    bps.iter()
                        .map(|b| {
                            let mut r = Map::new();
                            r.insert(
                                "recordName".into(),
                                json!(b.record_name.as_deref().unwrap_or(&b.name)),
                            );
                            let n = b.rewarded_by.as_ref().map_or(0, Vec::len);
                            if n > 0 {
                                r.insert("rewardContracts".into(), json!(n));
                            }
                            Value::Object(r)
                        })
                        .collect(),
                ),
            );
            counts[7] += 1;
        }
        if !row.is_empty() {
            items.insert(item.class_name.clone(), Value::Object(row));
        }
    }
    let mut body = Map::new();
    body.insert(
        "_assumptions".into(),
        Value::Array(ASSUMPTIONS.iter().map(|a| json!(a)).collect()),
    );
    let summary = json!({
        "items": items.len(),
        "withLoot": counts[0],
        "bestRandomLoot": counts[1],
        "bestFixed": counts[2],
        "bestLoose": counts[3],
        "withOtherTables": counts[4],
        "withContractAwards": counts[5],
        "withWikelo": counts[6],
        "withBlueprints": counts[7],
        "lootRows": counts[8],
        "spawns": spawn_set.len(),
        "tables": table_names.len(),
    });
    body.insert(
        "lootColumns".into(),
        json!(["spawns", "kind", "scoreEstimate", "perRollEstimate"]),
    );
    body.insert(
        "otherTableColumns".into(),
        json!(["tables", "perRollEstimate"]),
    );
    body.insert(
        "spawns".into(),
        Value::Array(spawn_set.into_values().collect()),
    );
    body.insert("tables".into(), json!(table_names));
    body.insert("items".into(), Value::Object(items));
    body.insert("summary".into(), summary);
    body
}

fn read_json(file: &Path) -> Option<Value> {
    serde_json::from_slice(&std::fs::read(file).ok()?).ok()
}

fn typed<T: serde::de::DeserializeOwned>(doc: Option<&Value>, key: &str) -> Option<T> {
    serde_json::from_value(doc?.get(key)?.clone()).ok()
}

/// Read the inputs from `dir`; `None` when item-tags, loot or item-sources is missing.
pub fn read_inputs(dir: &Path) -> Option<Inputs> {
    let tags = read_json(&dir.join("game-item-tags.json"))?;
    let loot = read_json(&dir.join("game-loot.json"))?;
    let sources = read_json(&dir.join("game-item-sources.json"))?;
    let blueprints = read_json(&dir.join("game-blueprints.json"));
    let wikelo = read_json(&dir.join("game-wikelo-trades.json"));
    let items: Vec<ItemTags> = typed(Some(&tags), "items")?;
    let loot: LootData = serde_json::from_value(loot).ok()?;
    let areas: Vec<LootArea> = typed(Some(&sources), "areas")?;
    let trades: Vec<Value> = typed(wikelo.as_ref(), "trades").unwrap_or_default();
    Some(Inputs {
        datasets: Datasets {
            items,
            loot: Some(loot),
            areas: Some(areas),
            contract_awards: typed(Some(&sources), "contractAwards"),
            blueprints: typed(blueprints.as_ref(), "blueprints"),
            wikelo: None,
            shops: None,
            terminals: None,
            shops_as_of: None,
        },
        trades,
        extracted: sources.get("_extracted").cloned(),
        build: sources
            .get("_build")
            .or_else(|| tags.get("_build"))
            .cloned(),
    })
}

/// The whole document: header, then [`build_body`].
pub fn build_document(inputs: &Inputs) -> Value {
    let mut doc = Map::new();
    doc.insert("_source".into(), json!(SOURCE));
    if let Some(e) = &inputs.extracted {
        doc.insert("_extracted".into(), e.clone());
    }
    if let Some(b) = &inputs.build {
        doc.insert("_build".into(), b.clone());
    }
    doc.insert(
        "_schema".into(),
        json!(crate::output::schema_tag(NAME, SCHEMA_VERSION)),
    );
    doc.extend(build_body(inputs));
    Value::Object(doc)
}

/// Rebuild `game-item-obtain.json` in `dir` from the datasets beside it; when an input is
/// missing, write nothing and remove a file already there.
pub fn write_item_obtain(dir: &Path) -> std::io::Result<Option<Written>> {
    let file = dir.join(FILE);
    let Some(inputs) = read_inputs(dir) else {
        return match std::fs::remove_file(&file) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(None),
        };
    };
    let doc = build_document(&inputs);
    let text = to_js_json(&doc);
    std::fs::write(&file, &text)?;
    let items = doc
        .get("items")
        .and_then(Value::as_object)
        .map_or(0, Map::len);
    Ok(Some(Written {
        file,
        items,
        bytes: text.len(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::types::{SpawnConfig, SpawnLoot, SpawnSlot};

    fn item(class: &str, tags: &[&str]) -> ItemTags {
        ItemTags {
            name: class.to_uppercase(),
            class_name: class.into(),
            id: class.into(),
            tags: tags.iter().map(|t| t.to_string()).collect(),
            ..Default::default()
        }
    }

    fn inputs() -> Inputs {
        let can = "LootGeneration/CanGenerateAsLoot";
        let items = vec![
            item("rifle_a", &[can, "Weapon/FPS/Rifle"]),
            item("rifle_b", &[can, "Weapon/FPS/Rifle"]),
            item("helmet", &["LootGeneration/CannotGenerateAsLoot/Reward"]),
            item("plain", &[]),
        ];
        let loot: LootData = serde_json::from_value(json!({
            "tables": [
                {"name": "weapons", "lootArchetypes": [{"weight": 1, "archetype": "arch"}]},
                {"name": "boss", "lootArchetypes": [{"weight": 1, "archetype": "arch"}]}
            ],
            "archetypes": [{"name": "arch", "primaryOrGroup": {"entries": [{"tag": "Weapon/FPS", "weight": 1}]}}]
        }))
        .unwrap();
        let slot = |entity: &str,
                    weight: f64,
                    loot: Option<SpawnLoot>,
                    contains: Option<Vec<String>>| SpawnSlot {
            entity: entity.into(),
            entity_name: None,
            preset: None,
            weight,
            loot,
            contains,
        };
        let areas = vec![LootArea {
            name: "v3slotpreset_caves".into(),
            configs: vec![SpawnConfig {
                name: Some("Weapons Rare".into()),
                spawn_tags: vec!["LootGeneration/LootContents/Weapons".into()],
                fill_chance: None,
                slots: vec![
                    slot(
                        "crate",
                        3.0,
                        Some(SpawnLoot {
                            table: "weapons".into(),
                            max_results: Some(2.0),
                            ..Default::default()
                        }),
                        None,
                    ),
                    slot("rack", 1.0, None, Some(vec!["helmet".into()])),
                ],
            }],
        }];
        Inputs {
            datasets: Datasets {
                items,
                loot: Some(loot),
                areas: Some(areas),
                contract_awards: Some(vec![ContractAward {
                    generator: "gen".into(),
                    contract: "c1".into(),
                    item: "helmet".into(),
                    amount: Some(1.0),
                    weighting: None,
                }]),
                ..Default::default()
            },
            trades: vec![
                json!({"name": "Trade_Helmet", "rewards": [{"item": "helmet", "amount": 1, "source": "award"}]}),
            ],
            extracted: None,
            build: Some(json!("4.10.201.1")),
        }
    }

    #[test]
    fn rolls_fixed_awards_and_other_tables_per_item() {
        let doc = build_document(&inputs());
        let items = doc["items"].as_object().unwrap();
        assert!(!items.contains_key("plain"), "nothing to say");
        let a = &items["rifle_a"];
        assert_eq!(a["eligibility"], json!({"canLoot": true}));
        // Pool of 2 rifles: 0.5 per roll; 2 rolls: 1 - 0.5^2 = 0.75; x 0.75 slot share.
        assert_eq!(a["loot"], json!([[[0], "loot", 0.5625, 0.5]]));
        let spawn = &doc["spawns"][a["loot"][0][0][0].as_u64().unwrap() as usize];
        assert_eq!(spawn["entity"], "crate");
        assert_eq!(spawn["slotShare"], json!(0.75));
        assert_eq!(spawn["maxResults"], json!(2.0));
        assert_eq!(spawn["contents"], "Weapons");
        assert_eq!(
            (spawn["rarity"].clone(), spawn["raritySource"].clone()),
            (json!("Rare"), json!("name"))
        );
        assert_eq!(doc["tables"], json!(["boss"]));
        assert_eq!(a["otherTables"], json!([[[0], 0.5]]));
        // The second rifle shares the spawn row.
        assert_eq!(items["rifle_b"]["loot"][0][0], json!([0]));
        let h = &items["helmet"];
        assert_eq!(
            h["eligibility"],
            json!({"canLoot": false, "reason": "Reward"})
        );
        assert_eq!(h["loot"], json!([[[1], "fixed", 0.25]]));
        assert_eq!(doc["spawns"][1]["entity"], "rack");
        assert!(h.get("otherTables").is_none());
        assert_eq!(
            h["contractAwards"],
            json!([{"generator": "gen", "contract": "c1", "amount": 1.0}])
        );
        assert_eq!(
            h["wikelo"],
            json!([{"trade": "Trade_Helmet", "amount": 1, "source": "award"}])
        );
        assert_eq!(doc["summary"]["items"], json!(3));
        assert_eq!(doc["_build"], "4.10.201.1");
        assert_eq!(doc["_schema"], "game-item-obtain@1");
    }

    #[test]
    fn spawns_with_identical_numbers_share_one_row() {
        let mut inp = inputs();
        // A second area with the same config: the rifles roll there with the same odds.
        let areas = inp.datasets.areas.as_mut().unwrap();
        let mut twin = areas[0].clone();
        twin.name = "v3slotpreset_bunker".into();
        areas.push(twin);
        let doc = build_document(&inp);
        let a = &doc["items"]["rifle_a"];
        assert_eq!(a["loot"], json!([[[0, 2], "loot", 0.5625, 0.5]]));
        assert_eq!(a["areasTotal"], json!(2), "areas, not rows");
        assert_eq!(doc["spawns"][0]["area"], "v3slotpreset_caves");
        assert_eq!(doc["spawns"][2]["area"], "v3slotpreset_bunker");
        assert_eq!(doc["lootColumns"][0], "spawns");
        assert_eq!(doc["otherTableColumns"][0], "tables");
    }

    #[test]
    fn merged_rows_rank_by_number_then_first_index() {
        let rows = vec![
            (5, vec![json!("loot"), json!(0.1)]),
            (3, vec![json!("loot"), json!(0.2)]),
            (1, vec![json!("loot"), json!(0.1)]),
            (0, vec![json!("fixed"), json!(0.1)]),
        ];
        assert_eq!(
            merge_rows(rows, 1),
            vec![
                json!([[3], "loot", 0.2]),
                json!([[0], "fixed", 0.1]),
                json!([[1, 5], "loot", 0.1]),
            ]
        );
    }

    #[test]
    fn rounds_to_four_significant_digits() {
        assert_eq!(sig(0.0019060488945750187), 0.001906);
        assert_eq!(sig(1.0), 1.0);
        assert_eq!(sig(0.0), 0.0);
    }
}
