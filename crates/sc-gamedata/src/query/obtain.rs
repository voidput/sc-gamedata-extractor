//! Where an item comes from, and where to look first (port of `query/obtain.ts`).
//!
//! Joins `game-item-tags` (what each item is), `game-loot` (tables, archetypes, pool
//! filters, secondary choices), `game-item-sources` (areas, fixed racks, contract awards),
//! `game-blueprints`, `game-wikelo-trades` and, when present, UEX shop rows.
//!
//! A container rolls its table: the table picks an archetype by weight, the archetype a
//! primary entry by weight (a tag selector), then each secondary layer a choice by weight.
//! The item is in the pool when it carries every positive tag of each pick (a tag counts
//! when the item has it or a tag below it), none of the negatives or the archetype's
//! excluded tags, and passes the slot's pool filter. `share` is the product of those weight
//! shares; `perRoll` divides by the pool's size, which assumes an even pick inside the pool
//! (an estimate the data does not state). Random loot is opt-in: only items tagged
//! `LootGeneration/CanGenerateAsLoot` roll; fixed and loose spawns ignore the flag.

use super::shops::{
    AsOf, LocateOptions, LocatedShop, STALE_DAYS, ShopPrice, UexTerminal, locate_shops,
    place_label, shop_row_text,
};
use super::types::{ContractAward, ItemTags, LootArea, SpawnLoot};
use super::{en_us, js_len};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

/// `game-loot.json`'s collections, as raw records (the TypeScript reads them as objects).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LootData {
    #[serde(default)]
    pub tables: Vec<Value>,
    #[serde(default)]
    pub archetypes: Vec<Value>,
    #[serde(default)]
    pub filters: Vec<Value>,
    #[serde(default)]
    pub secondary_choices: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct RewardRef {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mission: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pool: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chance: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlueprintLike {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub produces: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rewarded_by: Option<Vec<RewardRef>>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WikeloTradeLike {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reward_inferred: Option<String>,
}

/// Everything [`obtain`] reads. `items` is required; the rest narrows the answer when absent.
#[derive(Debug, Clone, Default)]
pub struct Datasets {
    pub items: Vec<ItemTags>,
    pub loot: Option<LootData>,
    pub areas: Option<Vec<LootArea>>,
    pub contract_awards: Option<Vec<ContractAward>>,
    pub blueprints: Option<Vec<BlueprintLike>>,
    pub wikelo: Option<Vec<WikeloTradeLike>>,
    pub shops: Option<Vec<ShopPrice>>,
    /// UEX terminals, to place shop rows.
    pub terminals: Option<Vec<UexTerminal>>,
    /// When the shop rows were fetched (ISO), for the stale flag. Default now.
    pub shops_as_of: Option<AsOf>,
}

// ---------------------------------------------------------------- tags

/// True when `tags` holds `wanted` or a tag below it.
pub fn has_tag(tags: &[String], wanted: &str) -> bool {
    tags.iter().any(|t| {
        t == wanted
            || (t.len() > wanted.len()
                && t.starts_with(wanted)
                && t.as_bytes()[wanted.len()] == b'/')
    })
}

const CAN: &str = "LootGeneration/CanGenerateAsLoot";
const CANNOT: &str = "LootGeneration/CannotGenerateAsLoot";

#[derive(Debug, Clone, PartialEq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LootEligibility {
    /// `Some(false)` when barred, `Some(true)` when flagged lootable, `None` when unflagged.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub can_loot: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rarity: Option<String>,
}

pub fn loot_eligibility(tags: &[String]) -> LootEligibility {
    let barred = tags
        .iter()
        .find(|t| *t == CANNOT || t.starts_with(&format!("{CANNOT}/")));
    let rarity = tags
        .iter()
        .find(|t| t.starts_with("LootGeneration/LootRarity/"))
        .and_then(|t| t.split('/').next_back())
        .map(str::to_string);
    if let Some(b) = barred {
        let reason = b.get(CANNOT.len() + 1..).filter(|r| !r.is_empty());
        return LootEligibility {
            can_loot: Some(false),
            reason: reason.map(str::to_string),
            rarity,
        };
    }
    LootEligibility {
        can_loot: has_tag(tags, CAN).then_some(true),
        reason: None,
        rarity,
    }
}

fn strings(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn number_or(value: Option<&Value>, fallback: f64) -> f64 {
    value
        .and_then(Value::as_f64)
        .filter(|x| x.is_finite())
        .unwrap_or(fallback)
}

fn obj_str<'a>(v: &'a Value, k: &str) -> Option<&'a str> {
    v.get(k).and_then(Value::as_str)
}

#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct Selector {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub weight: f64,
    pub positive: Vec<String>,
    pub negative: Vec<String>,
}

/// One weighted choice, normalised across v1 entries, V3 entries and secondary choices.
pub fn entry_selector(entry: &Value) -> Selector {
    let v3 = entry.get("selector").and_then(|s| s.get("tags"));
    let extra = entry.get("additionalTags");
    let mut positive: Vec<String> = obj_str(entry, "tag")
        .map(str::to_string)
        .into_iter()
        .collect();
    positive.extend(strings(v3.and_then(|v| v.get("positiveTags"))));
    positive.extend(strings(extra.and_then(|v| v.get("positiveTags"))));
    let mut negative = strings(v3.and_then(|v| v.get("negativeTags")));
    negative.extend(strings(extra.and_then(|v| v.get("negativeTags"))));
    Selector {
        name: obj_str(entry, "name").map(str::to_string),
        weight: number_or(entry.get("weight"), 1.0),
        positive,
        negative,
    }
}

/// An empty selector matches everything; that is how "Anything else" choices read.
pub fn selects(positive: &[String], negative: &[String], tags: &[String]) -> bool {
    positive.iter().all(|t| has_tag(tags, t)) && !negative.iter().any(|t| has_tag(tags, t))
}

fn selects_sel(s: &Selector, tags: &[String]) -> bool {
    selects(&s.positive, &s.negative, tags)
}

// ---------------------------------------------------------------- loot model

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Layer {
    pub name: Option<String>,
    pub choices: Vec<Selector>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Archetype {
    pub name: String,
    pub excluded: Vec<String>,
    pub primary: Vec<Selector>,
    pub layers: Vec<Layer>,
}

fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|x| x != 0.0 && !x.is_nan()),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}

fn truthy_items(v: Option<&Value>) -> Vec<&Value> {
    v.and_then(Value::as_array)
        .map(|a| a.iter().filter(|x| truthy(x)).collect())
        .unwrap_or_default()
}

pub fn normalise_archetype(raw: &Value, fallback_name: &str) -> Archetype {
    let primary_source = raw
        .get("primaryOrGroup")
        .and_then(|g| g.get("entries"))
        .filter(|v| !v.is_null())
        .or_else(|| raw.get("entries"));
    Archetype {
        name: obj_str(raw, "name").unwrap_or(fallback_name).to_string(),
        excluded: strings(raw.get("excludedTags").and_then(|e| e.get("tags"))),
        primary: truthy_items(primary_source)
            .into_iter()
            .map(entry_selector)
            .collect(),
        layers: truthy_items(raw.get("secondaryOrGroups"))
            .into_iter()
            .map(|g| Layer {
                name: obj_str(g, "groupName").map(str::to_string),
                choices: truthy_items(g.get("entries"))
                    .into_iter()
                    .map(entry_selector)
                    .collect(),
            })
            .collect(),
    }
}

/// Pool filter tree: cumulative parts must all pass; an additive part lets an item in alone.
#[derive(Debug, Clone, PartialEq)]
pub enum PoolFilter {
    All,
    Tags {
        positive: Vec<String>,
        negative: Vec<String>,
    },
    Group {
        cumulative: Vec<PoolFilter>,
        additive: Vec<PoolFilter>,
    },
}

impl PoolFilter {
    pub fn test(&self, tags: &[String]) -> bool {
        match self {
            Self::All => true,
            Self::Tags { positive, negative } => selects(positive, negative, tags),
            Self::Group {
                cumulative,
                additive,
            } => cumulative.iter().all(|p| p.test(tags)) || additive.iter().any(|p| p.test(tags)),
        }
    }
}

fn compile_filter(
    node: Option<&Value>,
    by_name: &HashMap<String, &Value>,
    depth: u32,
) -> PoolFilter {
    let Some(obj) = node.filter(|n| n.is_object()) else {
        return PoolFilter::All;
    };
    if depth > 16 {
        return PoolFilter::All;
    }
    if let Some(r) = obj_str(obj, "filterRecord") {
        let target = by_name.get(&r.to_lowercase());
        return compile_filter(target.and_then(|t| t.get("filter")), by_name, depth + 1);
    }
    if let Some(tf) = obj.get("tagFilter").filter(|v| v.is_object()) {
        return PoolFilter::Tags {
            positive: strings(tf.get("positiveTags")),
            negative: strings(tf.get("negativeTags")),
        };
    }
    if let Some(parts) = obj.get("filters").and_then(Value::as_array) {
        let mut cumulative = Vec::new();
        let mut additive = Vec::new();
        for f in parts.iter().filter(|f| truthy(f)) {
            let test = compile_filter(f.get("filter"), by_name, depth + 1);
            if obj_str(f, "mode") == Some("Additive") {
                additive.push(test);
            } else {
                cumulative.push(test);
            }
        }
        return PoolFilter::Group {
            cumulative,
            additive,
        };
    }
    PoolFilter::All
}

#[derive(Debug, Clone, PartialEq)]
pub struct TableSlot {
    pub weight: f64,
    pub archetype: Option<Archetype>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    pub name: String,
    pub version: String,
    pub slots: Vec<TableSlot>,
}

/// `LootIndex`: tables in `Map` insertion order (a later duplicate replaces the value in
/// place), filters and secondary choices by lower-case name.
#[derive(Debug, Clone, Default)]
pub struct LootIndex {
    pub tables: Vec<Table>,
    table_ix: HashMap<String, usize>,
    pub filters: HashMap<String, PoolFilter>,
    pub choices: HashMap<String, Vec<Layer>>,
}

impl LootIndex {
    pub fn table(&self, name: &str) -> Option<&Table> {
        self.table_ix
            .get(&name.to_lowercase())
            .map(|&i| &self.tables[i])
    }
}

fn archetype_ref(slot: &Value) -> (Option<&str>, Option<&Value>) {
    match slot.get("archetype") {
        Some(Value::String(s)) => (Some(s), None),
        Some(v @ Value::Object(_)) => match obj_str(v, "lootArchetypeRecord") {
            Some(r) => (Some(r), None),
            None => (None, Some(v)),
        },
        _ => (None, None),
    }
}

fn js_string(v: Option<&Value>) -> String {
    match v {
        None => "undefined".into(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) => "null".into(),
        Some(Value::Number(n)) => crate::output::js_number(n.as_f64().unwrap_or(f64::NAN)),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Array(_)) => "".into(),
        Some(Value::Object(_)) => "[object Object]".into(),
    }
}

pub fn build_loot_index(loot: &LootData) -> LootIndex {
    let mut by_name: HashMap<String, Archetype> = HashMap::new();
    for raw in &loot.archetypes {
        let a = normalise_archetype(raw, "inline");
        by_name.insert(a.name.to_lowercase(), a);
    }
    let mut index = LootIndex::default();
    for t in &loot.tables {
        let name = js_string(t.get("name"));
        let slots = truthy_items(t.get("lootArchetypes"))
            .into_iter()
            .map(|slot| {
                let (r, inline) = archetype_ref(slot);
                let archetype = match (inline, r) {
                    (Some(inline), _) => {
                        let fallback = match obj_str(slot, "name") {
                            Some(n) => format!("{name}:{n}"),
                            None => format!("{name}:inline"),
                        };
                        Some(normalise_archetype(inline, &fallback))
                    }
                    (None, Some(r)) => by_name.get(&r.to_lowercase()).cloned(),
                    _ => None,
                };
                TableSlot {
                    weight: number_or(slot.get("weight"), 1.0),
                    archetype,
                }
            })
            .collect();
        let version = match t.get("version") {
            None | Some(Value::Null) => String::new(),
            v => js_string(v),
        };
        let table = Table {
            name: name.clone(),
            version,
            slots,
        };
        match index.table_ix.get(&name.to_lowercase()) {
            Some(&i) => index.tables[i] = table,
            None => {
                index
                    .table_ix
                    .insert(name.to_lowercase(), index.tables.len());
                index.tables.push(table);
            }
        }
    }

    let mut records: HashMap<String, &Value> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for f in &loot.filters {
        if let Some(n) = obj_str(f, "name")
            && records.insert(n.to_lowercase(), f).is_none()
        {
            order.push(n.to_lowercase());
        }
    }
    for name in order {
        let f = compile_filter(records[&name].get("filter"), &records, 0);
        index.filters.insert(name, f);
    }

    let mut single: HashMap<String, Layer> = HashMap::new();
    for c in &loot.secondary_choices {
        if let (Some(n), Some(choices)) = (
            obj_str(c, "name"),
            c.get("choices").and_then(Value::as_array),
        ) {
            single.insert(
                n.to_lowercase(),
                Layer {
                    name: Some(n.to_string()),
                    choices: choices
                        .iter()
                        .filter(|x| truthy(x))
                        .map(entry_selector)
                        .collect(),
                },
            );
        }
    }
    for (name, layer) in &single {
        index.choices.insert(name.clone(), vec![layer.clone()]);
    }
    for c in &loot.secondary_choices {
        if let Some(n) = obj_str(c, "name")
            && c.get("layers").is_some_and(Value::is_array)
        {
            let layers = strings(c.get("layers"))
                .iter()
                .filter_map(|l| single.get(&l.to_lowercase()).cloned())
                .collect();
            index.choices.insert(n.to_lowercase(), layers);
        }
    }
    index
}

/// Odds a layer's roll comes up with a choice the item satisfies; 0 when none does.
fn layer_share(layer: &Layer, tags: &[String]) -> f64 {
    let total: f64 = layer.choices.iter().map(|c| c.weight).sum();
    if total <= 0.0 {
        return 1.0;
    }
    layer
        .choices
        .iter()
        .filter(|c| selects_sel(c, tags))
        .map(|c| c.weight)
        .sum::<f64>()
        / total
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RollPath {
    pub table: String,
    pub archetype: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry: Option<String>,
    /// archetype x entry x every secondary layer, as odds per roll.
    pub share: f64,
    /// Lootable items the same path could produce, the item included.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pool_size: Option<usize>,
    /// share / poolSize: the item's odds per roll, assuming an even pick in the pool.
    pub per_roll: f64,
}

/// Pool sizes already counted, by (table, slot, entry, filter, extra layers, the choices
/// the item satisfies in each layer). Areas that roll the same table the same way share a
/// pool, so one report counts each once.
pub type PoolMemo = std::sync::Mutex<HashMap<PoolKey, usize>>;
pub type PoolKey = (String, usize, usize, usize, usize, Vec<Vec<usize>>);

#[derive(Default, Clone, Copy)]
pub struct RollContext<'a> {
    pub filter: Option<&'a PoolFilter>,
    pub extra_layers: &'a [Layer],
    /// Every item, for pool sizes.
    pub pool: Option<&'a [ItemTags]>,
    /// Optional cache of pool sizes (the answer is the same with or without it).
    pub memo: Option<&'a PoolMemo>,
    /// Optional cache of the lootable `pool` items each (table, slot, entry) picks; with
    /// it a pool size counts only those (same answer, far fewer tests over many items).
    pub entry_pools: Option<&'a EntryPools>,
}

/// Indices into the pool of the lootable items one (table, slot, entry) picks and its
/// archetype does not exclude, by (table address, slot, entry).
pub type EntryPools = std::sync::Mutex<HashMap<(usize, usize, usize), std::sync::Arc<Vec<usize>>>>;

/// Every way one roll of a table can produce the item.
pub fn table_rolls(
    index: &LootIndex,
    table_name: &str,
    item: &ItemTags,
    ctx: RollContext<'_>,
) -> Vec<RollPath> {
    let Some(table) = index.table(table_name) else {
        return Vec::new();
    };
    if loot_eligibility(&item.tags).can_loot != Some(true) {
        return Vec::new();
    }
    if ctx.filter.is_some_and(|f| !f.test(&item.tags)) {
        return Vec::new();
    }
    let total: f64 = table.slots.iter().map(|s| s.weight).sum();
    if total <= 0.0 {
        return Vec::new();
    }
    let mut paths = Vec::new();
    for (slot_ix, slot) in table.slots.iter().enumerate() {
        let Some(archetype) = &slot.archetype else {
            continue;
        };
        if slot.weight <= 0.0 || archetype.excluded.iter().any(|t| has_tag(&item.tags, t)) {
            continue;
        }
        // Cheapest test first: no primary entry picks the item, no path (the answer is the
        // same; most tables pick most items nowhere).
        if !archetype
            .primary
            .iter()
            .any(|e| e.weight > 0.0 && selects_sel(e, &item.tags))
        {
            continue;
        }
        let layers: Vec<&Layer> = archetype.layers.iter().chain(ctx.extra_layers).collect();
        let mut secondary = 1.0;
        for l in &layers {
            secondary *= layer_share(l, &item.tags);
        }
        if secondary == 0.0 {
            continue;
        }
        let primary_total: f64 = archetype.primary.iter().map(|e| e.weight).sum();
        if primary_total <= 0.0 {
            continue;
        }
        for (entry_ix, entry) in archetype.primary.iter().enumerate() {
            if entry.weight <= 0.0 || !selects_sel(entry, &item.tags) {
                continue;
            }
            let share = (slot.weight / total) * (entry.weight / primary_total) * secondary;
            let pool_size = ctx.pool.map(|pool| {
                let item_choices: Vec<Vec<&Selector>> = layers
                    .iter()
                    .map(|l| {
                        l.choices
                            .iter()
                            .filter(|c| selects_sel(c, &item.tags))
                            .collect()
                    })
                    .collect();
                let key: Option<PoolKey> = ctx.memo.map(|_| {
                    let picks = layers
                        .iter()
                        .map(|l| {
                            l.choices
                                .iter()
                                .enumerate()
                                .filter(|(_, c)| selects_sel(c, &item.tags))
                                .map(|(i, _)| i)
                                .collect()
                        })
                        .collect();
                    (
                        table.name.to_lowercase(),
                        slot_ix,
                        entry_ix,
                        ctx.filter.map_or(0, |f| f as *const PoolFilter as usize),
                        ctx.extra_layers.as_ptr() as usize ^ ctx.extra_layers.len(),
                        picks,
                    )
                });
                if let (Some(m), Some(k)) = (ctx.memo, &key)
                    && let Some(n) = m.lock().unwrap_or_else(|e| e.into_inner()).get(k)
                {
                    return *n;
                }
                let picked = |c: &ItemTags| {
                    loot_eligibility(&c.tags).can_loot == Some(true)
                        && selects_sel(entry, &c.tags)
                        && !archetype.excluded.iter().any(|t| has_tag(&c.tags, t))
                };
                let rest = |c: &ItemTags| {
                    ctx.filter.is_none_or(|f| f.test(&c.tags))
                        && item_choices.iter().enumerate().all(|(i, choices)| {
                            layers[i].choices.is_empty()
                                || choices.iter().any(|ch| selects_sel(ch, &c.tags))
                        })
                };
                let n = match ctx.entry_pools {
                    Some(ep) => {
                        let k = (table as *const Table as usize, slot_ix, entry_ix);
                        let hit = ep
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .get(&k)
                            .cloned();
                        let ix = hit.unwrap_or_else(|| {
                            let v: std::sync::Arc<Vec<usize>> = std::sync::Arc::new(
                                (0..pool.len()).filter(|&i| picked(&pool[i])).collect(),
                            );
                            ep.lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .insert(k, v.clone());
                            v
                        });
                        ix.iter().filter(|&&i| rest(&pool[i])).count()
                    }
                    None => pool.iter().filter(|c| picked(c) && rest(c)).count(),
                };
                if let (Some(m), Some(k)) = (ctx.memo, key) {
                    m.lock().unwrap_or_else(|e| e.into_inner()).insert(k, n);
                }
                n
            });
            paths.push(RollPath {
                table: table.name.clone(),
                archetype: archetype.name.clone(),
                entry: entry.name.clone(),
                share,
                pool_size,
                per_roll: match pool_size {
                    Some(n) if n > 0 => share / n as f64,
                    _ => share,
                },
            });
        }
    }
    paths
}

// ---------------------------------------------------------------- spawns

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SpawnKind {
    Loot,
    Fixed,
    Loose,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpawnHit {
    pub area: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<String>,
    pub spawn_tags: Vec<String>,
    /// The container, rack or loose item entity.
    pub entity: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity_name: Option<String>,
    /// Share of this config's spawns that are this entity.
    pub slot_share: f64,
    pub kind: SpawnKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub per_roll: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_results: Option<f64>,
    /// Rough odds the item is in one spawned entity.
    pub score: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paths: Option<Vec<RollPath>>,
}

fn slot_layers<'a>(index: &'a LootIndex, loot: &SpawnLoot) -> &'a [Layer] {
    loot.secondary_choices
        .as_deref()
        .and_then(|c| index.choices.get(&c.to_lowercase()))
        .map_or(&[], Vec::as_slice)
}

fn sort_desc<T>(v: &mut [T], key: impl Fn(&T) -> f64) {
    v.sort_by(|a, b| {
        key(b)
            .partial_cmp(&key(a))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
}

pub fn spawn_hits(
    datasets: &Datasets,
    index: Option<&LootIndex>,
    item: &ItemTags,
) -> Vec<SpawnHit> {
    spawn_hits_memo(datasets, index, item, None)
}

fn spawn_hits_memo(
    datasets: &Datasets,
    index: Option<&LootIndex>,
    item: &ItemTags,
    memo: Option<&PoolMemo>,
) -> Vec<SpawnHit> {
    let mut hits = Vec::new();
    let cls = item.class_name.to_lowercase();
    let lootable = loot_eligibility(&item.tags).can_loot == Some(true);
    for area in datasets.areas.iter().flatten() {
        for config in &area.configs {
            let total: f64 = config.slots.iter().map(|s| s.weight).sum();
            if total <= 0.0 {
                continue;
            }
            let fill = config.fill_chance.unwrap_or(1.0);
            for slot in &config.slots {
                let slot_share = slot.weight / total;
                if slot_share <= 0.0 {
                    continue;
                }
                let base = |kind, score| SpawnHit {
                    area: area.name.clone(),
                    config: config.name.clone(),
                    spawn_tags: config.spawn_tags.clone(),
                    entity: slot.entity.clone(),
                    entity_name: slot.entity_name.clone(),
                    slot_share,
                    kind,
                    table: None,
                    per_roll: None,
                    max_results: None,
                    score,
                    paths: None,
                };
                if slot.entity.to_lowercase() == cls {
                    hits.push(base(SpawnKind::Loose, slot_share * fill));
                    continue;
                }
                if slot
                    .contains
                    .iter()
                    .flatten()
                    .any(|c| c.to_lowercase() == cls)
                {
                    hits.push(base(SpawnKind::Fixed, slot_share * fill));
                    continue;
                }
                let (Some(loot), Some(index)) = (&slot.loot, index) else {
                    continue;
                };
                if !lootable {
                    continue;
                }
                let filter = loot
                    .pool_filter
                    .as_deref()
                    .and_then(|f| index.filters.get(&f.to_lowercase()));
                let paths = table_rolls(
                    index,
                    &loot.table,
                    item,
                    RollContext {
                        filter,
                        extra_layers: slot_layers(index, loot),
                        pool: Some(&datasets.items),
                        memo,
                        entry_pools: None,
                    },
                );
                if paths.is_empty() {
                    continue;
                }
                let per_roll = paths.iter().map(|p| p.per_roll).sum::<f64>().min(1.0);
                let rolls = loot.max_results.unwrap_or(1.0).max(1.0);
                let chance = loot.chance.unwrap_or(1.0);
                let mut hit = base(
                    SpawnKind::Loot,
                    slot_share * fill * chance * (1.0 - (1.0 - per_roll).powf(rolls)),
                );
                hit.table = Some(loot.table.clone());
                hit.per_roll = Some(per_roll);
                hit.max_results = loot.max_results;
                hit.paths = Some(paths);
                hits.push(hit);
            }
        }
    }
    sort_desc(&mut hits, |h| h.score);
    hits
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AreaSummary {
    pub area: String,
    /// Best single spawn in the area.
    pub best: SpawnHit,
    /// Spawn kinds (configs) in the area that can produce the item.
    pub configs: usize,
}

/// Collapse spawn hits to one row per area, best spawn first.
pub fn rank_areas(hits: &[SpawnHit]) -> Vec<AreaSummary> {
    let mut by_area: Vec<(String, Vec<&SpawnHit>)> = Vec::new();
    let mut ix: HashMap<&str, usize> = HashMap::new();
    for h in hits {
        match ix.get(h.area.as_str()) {
            Some(&i) => by_area[i].1.push(h),
            None => {
                ix.insert(&h.area, by_area.len());
                by_area.push((h.area.clone(), vec![h]));
            }
        }
    }
    let mut out: Vec<AreaSummary> = by_area
        .into_iter()
        .map(|(area, list)| {
            let mut best = list[0];
            for h in &list[1..] {
                if h.score > best.score {
                    best = h;
                }
            }
            let configs: HashSet<&str> = list
                .iter()
                .map(|h| h.config.as_deref().unwrap_or(""))
                .collect();
            AreaSummary {
                area,
                best: best.clone(),
                configs: configs.len(),
            }
        })
        .collect();
    sort_desc(&mut out, |a| a.best.score);
    out
}

/// Loot tables that can roll the item but that no area spawn uses.
pub fn other_tables(
    datasets: &Datasets,
    index: &LootIndex,
    item: &ItemTags,
    used: &HashSet<String>,
) -> Vec<RollPath> {
    other_tables_memo(datasets, index, item, used, None)
}

fn other_tables_memo(
    datasets: &Datasets,
    index: &LootIndex,
    item: &ItemTags,
    used: &HashSet<String>,
    memo: Option<&PoolMemo>,
) -> Vec<RollPath> {
    let mut out = Vec::new();
    for table in &index.tables {
        if used.contains(&table.name.to_lowercase()) {
            continue;
        }
        let paths = table_rolls(
            index,
            &table.name,
            item,
            RollContext {
                pool: Some(&datasets.items),
                memo,
                ..Default::default()
            },
        );
        if let Some(first) = paths.first() {
            let mut p = first.clone();
            p.per_roll = paths.iter().map(|x| x.per_roll).sum();
            p.share = paths.iter().map(|x| x.share).sum();
            out.push(p);
        }
    }
    sort_desc(&mut out, |p| p.per_roll);
    out
}

// ---------------------------------------------------------------- bulk

/// Where a slot is: area, config and slot positions (the order [`spawn_hits`] visits).
type SlotPos = (usize, usize, usize);

/// A loot slot of one area spawn, with what scores it.
struct LootSlot<'a> {
    pos: SlotPos,
    slot_share: f64,
    fill: f64,
    loot: &'a SpawnLoot,
}

/// The area spawns and shop rows of many items, for answering every item of a build at
/// once (the obtain hints of `global.ini` descriptions). Same answers as [`spawn_hits`] +
/// [`rank_areas`] and the shop rows of [`obtain`], built so one item costs only what can
/// hold it: loose and fixed slots are looked up by class, loot slots that roll the same
/// way are rolled once per item, pool sizes are counted once for every item, and shop rows
/// are found by id or name.
pub struct BulkObtain<'a> {
    datasets: &'a Datasets,
    index: Option<&'a LootIndex>,
    /// Lower-case class -> slots that are the item (loose).
    loose: HashMap<String, Vec<SlotPos>>,
    /// Lower-case class -> slots whose loadout holds the item (fixed).
    fixed: HashMap<String, Vec<SlotPos>>,
    /// Loot slots by (table, pool filter, secondary choices): one roll per group.
    loot: Vec<Vec<LootSlot<'a>>>,
    /// The items random loot can roll: the only ones a pool size counts.
    lootable: Vec<ItemTags>,
    /// Pool sizes and entry pools, shared by every item and thread.
    memo: PoolMemo,
    entry_pools: EntryPools,
    shops_by_id: HashMap<&'a str, Vec<usize>>,
    shops_by_name: HashMap<String, Vec<usize>>,
}

impl<'a> BulkObtain<'a> {
    pub fn new(datasets: &'a Datasets, index: Option<&'a LootIndex>) -> Self {
        let mut loose: HashMap<String, Vec<SlotPos>> = HashMap::new();
        let mut fixed: HashMap<String, Vec<SlotPos>> = HashMap::new();
        let mut groups: HashMap<(&str, Option<&str>, Option<&str>), usize> = HashMap::new();
        let mut loot: Vec<Vec<LootSlot>> = Vec::new();
        for (ai, area) in datasets.areas.iter().flatten().enumerate() {
            for (ci, config) in area.configs.iter().enumerate() {
                let total: f64 = config.slots.iter().map(|s| s.weight).sum();
                if total <= 0.0 {
                    continue;
                }
                let fill = config.fill_chance.unwrap_or(1.0);
                for (si, slot) in config.slots.iter().enumerate() {
                    let slot_share = slot.weight / total;
                    if slot_share <= 0.0 {
                        continue;
                    }
                    let pos = (ai, ci, si);
                    loose
                        .entry(slot.entity.to_lowercase())
                        .or_default()
                        .push(pos);
                    let mut held: Vec<String> = slot
                        .contains
                        .iter()
                        .flatten()
                        .map(|c| c.to_lowercase())
                        .collect();
                    held.dedup();
                    for c in held {
                        let v = fixed.entry(c).or_default();
                        if v.last() != Some(&pos) {
                            v.push(pos);
                        }
                    }
                    if let Some(l) = &slot.loot {
                        let key = (
                            l.table.as_str(),
                            l.pool_filter.as_deref(),
                            l.secondary_choices.as_deref(),
                        );
                        let g = *groups.entry(key).or_insert_with(|| {
                            loot.push(Vec::new());
                            loot.len() - 1
                        });
                        loot[g].push(LootSlot {
                            pos,
                            slot_share,
                            fill,
                            loot: l,
                        });
                    }
                }
            }
        }
        let mut shops_by_id: HashMap<&str, Vec<usize>> = HashMap::new();
        let mut shops_by_name: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, p) in datasets.shops.iter().flatten().enumerate() {
            match p.item_uuid.as_deref().filter(|u| !u.is_empty()) {
                Some(u) => shops_by_id.entry(u).or_default().push(i),
                None => {
                    if let Some(n) = p.item_name.as_deref() {
                        shops_by_name.entry(n.to_lowercase()).or_default().push(i);
                    }
                }
            }
        }
        Self {
            datasets,
            index,
            loose,
            fixed,
            loot,
            lootable: datasets
                .items
                .iter()
                .filter(|c| loot_eligibility(&c.tags).can_loot == Some(true))
                .cloned()
                .collect(),
            memo: PoolMemo::default(),
            entry_pools: EntryPools::default(),
            shops_by_id,
            shops_by_name,
        }
    }

    fn slot(
        &self,
        (ai, ci, si): SlotPos,
    ) -> (
        &'a LootArea,
        &'a super::types::SpawnConfig,
        &'a super::types::SpawnSlot,
    ) {
        let area = &self.datasets.areas.as_deref().unwrap_or_default()[ai];
        let config = &area.configs[ci];
        (area, config, &config.slots[si])
    }

    /// How each loot group rolls the item: (group, paths, odds per roll). Depends on the
    /// item's tags only, so items with the same tags share it.
    fn loot_rolls(&self, item: &ItemTags) -> Vec<(usize, Vec<RollPath>, f64)> {
        let Some(index) = self.index else {
            return Vec::new();
        };
        if loot_eligibility(&item.tags).can_loot != Some(true) {
            return Vec::new();
        }
        let mut out = Vec::new();
        for (g, group) in self.loot.iter().enumerate() {
            let Some(first) = group.first() else { continue };
            let loot = first.loot;
            let filter = loot
                .pool_filter
                .as_deref()
                .and_then(|f| index.filters.get(&f.to_lowercase()));
            if filter.is_some_and(|f| !f.test(&item.tags)) {
                continue;
            }
            let paths = table_rolls(
                index,
                &loot.table,
                item,
                RollContext {
                    filter,
                    extra_layers: slot_layers(index, loot),
                    pool: Some(&self.lootable),
                    memo: Some(&self.memo),
                    entry_pools: Some(&self.entry_pools),
                },
            );
            if paths.is_empty() {
                continue;
            }
            let per_roll = paths.iter().map(|p| p.per_roll).sum::<f64>().min(1.0);
            out.push((g, paths, per_roll));
        }
        out
    }

    /// Spawn hits from the rolls; `paths: false` leaves [`SpawnHit::paths`] out (callers
    /// that only rank areas do not read them).
    fn hits_with(
        &self,
        item: &ItemTags,
        rolls: &[(usize, Vec<RollPath>, f64)],
        paths_too: bool,
    ) -> Vec<SpawnHit> {
        let cls = item.class_name.to_lowercase();
        let mut hits: Vec<(SlotPos, SpawnHit)> = Vec::new();
        let base = |pos: SlotPos, kind, score| {
            let (area, config, slot) = self.slot(pos);
            let total: f64 = config.slots.iter().map(|s| s.weight).sum();
            SpawnHit {
                area: area.name.clone(),
                config: config.name.clone(),
                spawn_tags: config.spawn_tags.clone(),
                entity: slot.entity.clone(),
                entity_name: slot.entity_name.clone(),
                slot_share: slot.weight / total,
                kind,
                table: None,
                per_roll: None,
                max_results: None,
                score,
                paths: None,
            }
        };
        let fill = |pos: SlotPos| self.slot(pos).1.fill_chance.unwrap_or(1.0);
        let share = |pos: SlotPos| {
            let (_, config, slot) = self.slot(pos);
            slot.weight / config.slots.iter().map(|s| s.weight).sum::<f64>()
        };
        let mut taken: HashSet<SlotPos> = HashSet::new();
        for &pos in self.loose.get(&cls).into_iter().flatten() {
            taken.insert(pos);
            hits.push((pos, base(pos, SpawnKind::Loose, share(pos) * fill(pos))));
        }
        for &pos in self.fixed.get(&cls).into_iter().flatten() {
            if taken.insert(pos) {
                hits.push((pos, base(pos, SpawnKind::Fixed, share(pos) * fill(pos))));
            }
        }
        for (g, paths, per_roll) in rolls {
            for ls in &self.loot[*g] {
                if taken.contains(&ls.pos) {
                    continue;
                }
                let l = ls.loot;
                let rolls = l.max_results.unwrap_or(1.0).max(1.0);
                let chance = l.chance.unwrap_or(1.0);
                let mut hit = base(
                    ls.pos,
                    SpawnKind::Loot,
                    ls.slot_share * ls.fill * chance * (1.0 - (1.0 - per_roll).powf(rolls)),
                );
                hit.table = Some(l.table.clone());
                hit.per_roll = Some(*per_roll);
                hit.max_results = l.max_results;
                if paths_too {
                    hit.paths = Some(paths.clone());
                }
                hits.push((ls.pos, hit));
            }
        }
        hits.sort_by_key(|(pos, _)| *pos);
        let mut out: Vec<SpawnHit> = hits.into_iter().map(|(_, h)| h).collect();
        sort_desc(&mut out, |h| h.score);
        out
    }

    /// [`spawn_hits`] of one item.
    pub fn spawn_hits(&self, item: &ItemTags) -> Vec<SpawnHit> {
        self.hits_with(item, &self.loot_rolls(item), true)
    }

    /// Areas that hold the item, best first ([`rank_areas`] of [`Self::spawn_hits`]).
    pub fn areas(&self, item: &ItemTags) -> Vec<AreaSummary> {
        rank_areas(&self.spawn_hits(item))
    }

    /// [`Self::areas`] of every item, in parallel, without the roll paths of each best
    /// spawn (`best.paths` is `None`); the rolls of items with the same tags are worked out
    /// once per worker thread.
    pub fn areas_all(&self, items: &[&ItemTags]) -> Vec<Vec<AreaSummary>> {
        use rayon::prelude::*;
        type Rolls = std::sync::Arc<Vec<(usize, Vec<RollPath>, f64)>>;
        items
            .par_iter()
            .map_init(HashMap::<Vec<String>, Rolls>::new, |seen, item| {
                let rolls = seen
                    .entry(item.tags.clone())
                    .or_insert_with(|| std::sync::Arc::new(self.loot_rolls(item)))
                    .clone();
                rank_areas(&self.hits_with(item, &rolls, false))
            })
            .collect()
    }

    /// The item's shop rows, placed and dated (the `shops` of [`obtain`] without `at`).
    pub fn shops(&self, item: &ItemTags) -> Vec<LocatedShop> {
        let Some(all) = self.datasets.shops.as_deref() else {
            return Vec::new();
        };
        let mut ix: Vec<usize> = self
            .shops_by_id
            .get(item.id.as_str())
            .cloned()
            .unwrap_or_default();
        if !item.template()
            && let Some(v) = self.shops_by_name.get(&item.name.to_lowercase())
        {
            ix.extend(v);
        }
        if ix.is_empty() {
            return Vec::new();
        }
        ix.sort_unstable();
        let rows: Vec<&ShopPrice> = ix.iter().map(|&i| &all[i]).collect();
        locate_shops(
            &rows,
            &LocateOptions {
                terminals: self.datasets.terminals.as_deref().unwrap_or(&[]),
                as_of: self.datasets.shops_as_of.clone(),
                stale_days: None,
                at: None,
            },
        )
    }
}

// ---------------------------------------------------------------- lookup

/// Items matching a name or class name; exact matches win, then every word must appear.
pub fn find_items<'a>(items: &'a [ItemTags], query: &str) -> Vec<&'a ItemTags> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    let exact: Vec<&ItemTags> = items
        .iter()
        .filter(|i| i.name.to_lowercase() == q || i.class_name.to_lowercase() == q)
        .collect();
    if !exact.is_empty() {
        return exact;
    }
    let words: Vec<&str> = q.split_whitespace().collect();
    items
        .iter()
        .filter(|i| {
            let maker = i
                .tags
                .iter()
                .find_map(|t| t.strip_prefix("Manufacturer/"))
                .unwrap_or("");
            let hay = format!(
                "{} {} {} {}",
                i.name,
                i.class_name,
                i.manufacturer.as_deref().unwrap_or(""),
                maker
            )
            .to_lowercase();
            words.iter().all(|w| hay.contains(w))
        })
        .collect()
}

fn squash(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        .collect()
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NamedItem {
    pub class_name: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObtainReport {
    pub item: ItemTags,
    pub eligibility: LootEligibility,
    pub areas: Vec<AreaSummary>,
    pub spawns: Vec<SpawnHit>,
    /// Tables that roll the item but no area spawn references.
    pub other_tables: Vec<RollPath>,
    pub blueprints: Vec<BlueprintLike>,
    pub contract_awards: Vec<ContractAward>,
    /// Wikelo trades whose name implies this reward, used when no award record matches.
    pub wikelo_inferred: Vec<WikeloTradeLike>,
    /// Shop rows for the item, placed and dated; only those at `at` when it is set.
    pub shops: Vec<LocatedShop>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shops_elsewhere: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cheapest_elsewhere: Option<LocatedShop>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ammo_for: Option<Vec<NamedItem>>,
}

#[derive(Debug, Clone, Default)]
pub struct ObtainOptions<'a> {
    /// Keep only shops at this place.
    pub at: Option<&'a str>,
    /// The weapons this item was looked up as ammo for; only labels the report.
    pub ammo_for: &'a [&'a ItemTags],
}

#[derive(Debug, Clone, PartialEq)]
pub struct AmmoTarget<'a> {
    pub ammo: &'a ItemTags,
    pub weapons: Vec<&'a ItemTags>,
}

/// The magazines or canisters the matched weapons load, each with the weapons that take it.
pub fn ammo_targets<'a>(items: &'a [ItemTags], matches: &[&'a ItemTags]) -> Vec<AmmoTarget<'a>> {
    let by_class: HashMap<String, &ItemTags> = items
        .iter()
        .map(|i| (i.class_name.to_lowercase(), i))
        .collect();
    let mut out: Vec<AmmoTarget> = Vec::new();
    for weapon in matches {
        for cls in weapon.ammo_items.iter().flatten() {
            let Some(ammo) = by_class.get(&cls.to_lowercase()).copied() else {
                continue;
            };
            let t = match out
                .iter_mut()
                .position(|t| t.ammo.class_name == ammo.class_name)
            {
                Some(i) => &mut out[i],
                None => {
                    out.push(AmmoTarget {
                        ammo,
                        weapons: Vec::new(),
                    });
                    out.last_mut().unwrap()
                }
            };
            if !t.weapons.iter().any(|w| std::ptr::eq(*w, *weapon)) {
                t.weapons.push(weapon);
            }
        }
    }
    out
}

/// "Cambio SRT" or "Cambio SRT (+2 variants)".
pub fn weapons_label(names: &[&str]) -> String {
    let mut uniq: Vec<&str> = Vec::new();
    for n in names {
        if !uniq.contains(n) {
            uniq.push(n);
        }
    }
    match uniq.len() {
        0 => String::new(),
        1 => uniq[0].to_string(),
        n => format!(
            "{} (+{} variant{})",
            uniq[0],
            n - 1,
            if n > 2 { "s" } else { "" }
        ),
    }
}

fn price_or_zero(s: &LocatedShop) -> f64 {
    s.row.price_buy.unwrap_or(0.0)
}

fn buyable(s: &LocatedShop) -> bool {
    s.row.price_buy.is_some_and(|p| p != 0.0 && !p.is_nan())
}

pub fn obtain(
    datasets: &Datasets,
    item: &ItemTags,
    index: Option<&LootIndex>,
    options: &ObtainOptions<'_>,
) -> ObtainReport {
    let built;
    let loot_index = match index {
        Some(i) => Some(i),
        None => match &datasets.loot {
            Some(l) => {
                built = build_loot_index(l);
                Some(&built)
            }
            None => None,
        },
    };
    let cls = item.class_name.to_lowercase();
    let memo = PoolMemo::default();
    let spawns = spawn_hits_memo(datasets, loot_index, item, Some(&memo));
    let used: HashSet<String> = datasets
        .areas
        .iter()
        .flatten()
        .flat_map(|a| a.configs.iter())
        .flat_map(|c| c.slots.iter())
        .map(|s| {
            s.loot
                .as_ref()
                .map(|l| l.table.to_lowercase())
                .unwrap_or_default()
        })
        .collect();
    let awards: Vec<ContractAward> = datasets
        .contract_awards
        .iter()
        .flatten()
        .filter(|a| a.item.to_lowercase() == cls)
        .cloned()
        .collect();
    let name_key = squash(&item.name);
    let rows: Vec<&ShopPrice> = datasets
        .shops
        .iter()
        .flatten()
        .filter(|p| match p.item_uuid.as_deref().filter(|u| !u.is_empty()) {
            Some(u) => u == item.id,
            None => {
                !item.template()
                    && p.item_name
                        .as_deref()
                        .is_some_and(|n| n.to_lowercase() == item.name.to_lowercase())
            }
        })
        .collect();
    let at = options.at.map(str::trim).filter(|s| !s.is_empty());
    let terminals = datasets.terminals.as_deref().unwrap_or(&[]);
    let shops = locate_shops(
        &rows,
        &LocateOptions {
            terminals,
            as_of: datasets.shops_as_of.clone(),
            stale_days: None,
            at,
        },
    );
    let mut cheapest_elsewhere = None;
    if at.is_some() && shops.len() < rows.len() {
        let key = |s: &LocatedShop| {
            format!(
                "{}|{}",
                s.row
                    .id_terminal
                    .map(crate::output::js_number)
                    .unwrap_or_default(),
                s.terminal
            )
        };
        let here: HashSet<String> = shops.iter().map(key).collect();
        let mut other: Vec<LocatedShop> = locate_shops(
            &rows,
            &LocateOptions {
                terminals,
                as_of: datasets.shops_as_of.clone(),
                stale_days: None,
                at: None,
            },
        )
        .into_iter()
        .filter(|s| buyable(s) && !here.contains(&key(s)))
        .collect();
        other.sort_by(|a, b| {
            price_or_zero(a)
                .partial_cmp(&price_or_zero(b))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        cheapest_elsewhere = other.into_iter().next();
    }
    let wikelo_inferred = if awards.is_empty() {
        datasets
            .wikelo
            .iter()
            .flatten()
            .filter(|t| {
                t.reward_inferred.as_deref().is_some_and(|r| {
                    !r.is_empty() && js_len(&name_key) > 3 && squash(r) == name_key
                })
            })
            .cloned()
            .collect()
    } else {
        Vec::new()
    };
    let shops_elsewhere = at.map(|_| rows.len() - shops.len());
    ObtainReport {
        item: item.clone(),
        eligibility: loot_eligibility(&item.tags),
        areas: rank_areas(&spawns),
        other_tables: loot_index.map_or_else(Vec::new, |i| {
            other_tables_memo(datasets, i, item, &used, Some(&memo))
        }),
        spawns,
        blueprints: datasets
            .blueprints
            .iter()
            .flatten()
            .filter(|b| {
                b.produces
                    .as_deref()
                    .is_some_and(|p| p.to_lowercase() == cls)
            })
            .cloned()
            .collect(),
        contract_awards: awards,
        wikelo_inferred,
        shops,
        at: at.map(str::to_string),
        shops_elsewhere,
        cheapest_elsewhere,
        ammo_for: (!options.ammo_for.is_empty()).then(|| {
            options
                .ammo_for
                .iter()
                .map(|w| NamedItem {
                    class_name: w.class_name.clone(),
                    name: w.name.clone(),
                })
                .collect()
        }),
    }
}

// ---------------------------------------------------------------- render

/// Percentages the way the TypeScript prints them.
pub fn pct(n: f64) -> String {
    let p = n * 100.0;
    if n >= 0.1 {
        format!("{p:.1}%")
    } else if n >= 0.001 {
        format!("{p:.2}%")
    } else if n > 0.0 {
        format!("{}%", to_precision2(p))
    } else {
        "0%".into()
    }
}

/// `Number.prototype.toPrecision(2)` for a positive number below 0.1.
fn to_precision2(x: f64) -> String {
    let e = x.log10().floor() as i32;
    let s = format!("{:.1e}", x);
    let (m, ex) = s.split_once('e').unwrap_or((&s, "0"));
    let ex: i32 = ex.parse().unwrap_or(e);
    if ex < -6 {
        let sign = if ex < 0 { "-" } else { "+" };
        return format!("{m}e{sign}{}", ex.abs());
    }
    let decimals = (1 - ex).max(0) as usize;
    format!("{x:.decimals$}")
}

/// "caves/loot_caves_occupied_rock_stanton" -> "caves: loot caves occupied rock stanton".
pub fn area_label(name: &str) -> String {
    name.replace('/', ": ").replace('_', " ")
}

fn tag_label(tags: &[String]) -> String {
    let wanted: Vec<String> = tags
        .iter()
        .filter(|t| {
            t.starts_with("LootSize/")
                || t.starts_with("LootRarity/")
                || t.starts_with("LootContents/")
        })
        .map(|t| t.split('/').skip(1).collect::<Vec<_>>().join("/"))
        .collect();
    if !wanted.is_empty() {
        return wanted.join(" ");
    }
    tags.iter()
        .filter(|t| !(t.starts_with("SpawnPoint") || t.starts_with("TrackInArea")))
        .cloned()
        .collect::<Vec<_>>()
        .join(" ")
}

fn age(s: &LocatedShop) -> String {
    format!(
        "{}{}",
        s.updated.as_deref().unwrap_or("undated"),
        if s.stale { " stale" } else { "" }
    )
}

/// "; 110 shop row(s) elsewhere, cheapest 275 aUEC at Dumper's GrimHEX (...)", or "".
pub fn elsewhere_text(shops_elsewhere: Option<usize>, cheapest: Option<&LocatedShop>) -> String {
    let Some(n) = shops_elsewhere.filter(|n| *n > 0) else {
        return String::new();
    };
    let c = match cheapest {
        Some(c) if buyable(c) => format!(
            ", cheapest {} aUEC at {} ({})",
            en_us(price_or_zero(c)),
            place_label(&c.terminal, &c.location),
            age(c)
        ),
        _ => String::new(),
    };
    format!("; {n} shop row(s) elsewhere{c}")
}

fn ammo_prefix(ammo_for: Option<&Vec<NamedItem>>) -> String {
    match ammo_for.filter(|a| !a.is_empty()) {
        Some(a) => {
            let names: Vec<&str> = a.iter().map(|w| w.name.as_str()).collect();
            format!("ammo for {}: ", weapons_label(&names))
        }
        None => String::new(),
    }
}

pub(crate) fn rewarded_missions(b: &BlueprintLike) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for r in b.rewarded_by.iter().flatten() {
        if let Some(m) = r
            .mission
            .as_ref()
            .or(r.contract.as_ref())
            .filter(|m| !m.is_empty())
            && !out.contains(m)
        {
            out.push(m.clone());
        }
    }
    out
}

fn pad(s: &str, w: usize) -> String {
    super::shops::pad_end(s, w)
}

fn pad_start(s: &str, w: usize) -> String {
    let n = js_len(s);
    if n >= w {
        s.to_string()
    } else {
        format!("{}{s}", " ".repeat(w - n))
    }
}

fn cut(s: &str, n: usize) -> String {
    super::shops::slice_chars(s, n)
}

pub fn render_obtain(report: &ObtainReport, limit: usize, shops_loaded: Option<bool>) -> String {
    let item = &report.item;
    let el = &report.eligibility;
    let mut lines: Vec<String> = Vec::new();
    lines.push(format!(
        "{}{}  ({}{}{})",
        ammo_prefix(report.ammo_for.as_ref()),
        item.name,
        item.class_name,
        item.manufacturer
            .as_deref()
            .filter(|m| !m.is_empty())
            .map(|m| format!(", {m}"))
            .unwrap_or_default(),
        if item.template() {
            ", no display name"
        } else {
            ""
        }
    ));
    let loot = match el.can_loot {
        Some(false) => format!(
            "no, CannotGenerateAsLoot{}",
            el.reason
                .as_deref()
                .map(|r| format!("/{r}"))
                .unwrap_or_default()
        ),
        Some(true) => "yes, CanGenerateAsLoot".into(),
        None => "no, not flagged CanGenerateAsLoot (random loot is opt-in)".into(),
    };
    lines.push(format!(
        "  lootable   {loot}{}",
        el.rarity
            .as_deref()
            .map(|r| format!(", rarity {r}"))
            .unwrap_or_default()
    ));
    if !report.areas.is_empty() {
        let n = report.areas.len();
        lines.push(format!(
            "\n  best areas ({n}{}); odds per spawned box, rough:",
            if n > limit {
                format!(", top {limit}")
            } else {
                String::new()
            }
        ));
        lines.push(format!(
            "    {} {} {} {} {} {}",
            pad("area", 52),
            pad("spawn point", 26),
            pad("box", 34),
            pad_start("box%", 6),
            pad_start("per roll", 9),
            pad_start("odds", 8)
        ));
        for a in report.areas.iter().take(limit) {
            let h = &a.best;
            let entity = h.entity_name.as_deref().unwrap_or(&h.entity);
            let bx = match h.kind {
                SpawnKind::Loot => entity.to_string(),
                SpawnKind::Fixed => format!("fixed: {entity}"),
                SpawnKind::Loose => format!("loose: {entity}"),
            };
            let point = h.config.clone().unwrap_or_else(|| tag_label(&h.spawn_tags));
            lines.push(format!(
                "    {} {} {} {} {} {}",
                pad(&cut(&area_label(&h.area), 52), 52),
                pad(&cut(&point, 26), 26),
                pad(&cut(&bx, 34), 34),
                pad_start(&pct(h.slot_share), 6),
                pad_start(&h.per_roll.map_or("fixed".into(), pct), 9),
                pad_start(&pct(h.score), 8)
            ));
        }
        lines.push(
            "    box% = share of that spawn point's spawns that are this box; per roll = item odds per loot roll;".into(),
        );
        lines.push(
            "    odds = box% x up to maxResults rolls. Even pick inside a pool is assumed, not stated by the data.".into(),
        );
    } else if el.can_loot == Some(true) {
        lines.push("  areas      no area spawn produces it".into());
    }
    if !report.other_tables.is_empty() {
        let names: Vec<String> = report
            .other_tables
            .iter()
            .take(10)
            .map(|t| format!("{} ({})", t.table, pct(t.per_roll)))
            .collect();
        lines.push(format!(
            "\n  other tables (corpses, events, legacy; not placed by an area preset): {}{}",
            names.join(", "),
            if report.other_tables.len() > 10 {
                ", ..."
            } else {
                ""
            }
        ));
    }
    lines.push(String::new());
    if report.blueprints.is_empty() {
        lines.push("  crafting   no blueprint".into());
    } else {
        for b in &report.blueprints {
            lines.push(format!(
                "  crafting   {}",
                b.display_name.as_deref().unwrap_or(&b.name)
            ));
            let missions = rewarded_missions(b);
            if !missions.is_empty() {
                lines.push(format!(
                    "             blueprint from missions: {}{}",
                    missions
                        .iter()
                        .take(8)
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", "),
                    if missions.len() > 8 {
                        format!(", +{}", missions.len() - 8)
                    } else {
                        String::new()
                    }
                ));
            }
        }
    }
    if !report.contract_awards.is_empty() {
        let mut by: Vec<&str> = Vec::new();
        for a in &report.contract_awards {
            if !by.contains(&a.contract.as_str()) {
                by.push(&a.contract);
            }
        }
        lines.push(format!("  contracts  awarded by {}", by.join(", ")));
    } else if !report.wikelo_inferred.is_empty() {
        let n: Vec<&str> = report
            .wikelo_inferred
            .iter()
            .map(|t| t.name.as_str())
            .collect();
        lines.push(format!(
            "  wikelo     {} (reward inferred from contract name)",
            n.join(", ")
        ));
    } else {
        lines.push("  contracts  no mission or Wikelo award".into());
    }
    let at = report
        .at
        .as_deref()
        .map(|a| format!(" at \"{a}\""))
        .unwrap_or_default();
    let mut buy: Vec<&LocatedShop> = report.shops.iter().filter(|s| buyable(s)).collect();
    buy.sort_by(|a, b| {
        price_or_zero(a)
            .partial_cmp(&price_or_zero(b))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    if !buy.is_empty() {
        let shown = buy.len().min(limit.max(10));
        lines.push(format!(
            "  shops{at}   (UEX, player-reported; date = last confirmed, stale = over {STALE_DAYS} days)"
        ));
        for s in &buy[..shown] {
            lines.push(format!("    {}", shop_row_text(s)));
        }
        if buy.len() > shown {
            lines.push(format!("    ... {} more", buy.len() - shown));
        }
    } else if !report.shops.is_empty() {
        let mut places: Vec<String> = Vec::new();
        for s in &report.shops {
            let p = place_label(&s.terminal, &s.location);
            if !places.contains(&p) {
                places.push(p);
            }
        }
        lines.push(format!(
            "  shops      listed{at}, no buy price: {} (UEX)",
            places.join("; ")
        ));
    } else if shops_loaded == Some(false) {
        lines.push("  shops      no shop data (run: sc-gamedata fetch uex-item-prices)".into());
    } else if report.at.is_some() {
        lines.push(format!(
            "  shops      not sold{at} as far as UEX knows{}",
            elsewhere_text(report.shops_elsewhere, report.cheapest_elsewhere.as_ref())
        ));
    } else {
        lines.push("  shops      not sold anywhere UEX knows of".into());
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::types::{SpawnConfig, SpawnSlot};
    use serde_json::json;

    const COMMON: &str = "LootGeneration/LootRarity/Common";
    const RARE: &str = "LootGeneration/LootRarity/Rare";

    fn item(class: &str, tags: &[&str], name: Option<&str>) -> ItemTags {
        ItemTags {
            name: name.unwrap_or(class).into(),
            class_name: class.into(),
            id: format!("id-{class}"),
            tags: tags.iter().map(|t| t.to_string()).collect(),
            ..Default::default()
        }
    }

    fn verdict() -> ItemTags {
        item(
            "ksar_lmg_energy_01",
            &[
                CAN,
                COMMON,
                "Weapon/FPS/Stocked/LMG/Verdict",
                "Manufacturer/KastakArms",
            ],
            Some("Verdict LMG"),
        )
    }
    fn fs9() -> ItemTags {
        item(
            "behr_lmg_ballistic_01",
            &[
                CAN,
                COMMON,
                "Weapon/FPS/Stocked/LMG/FS-9",
                "Manufacturer/Behring",
            ],
            Some("FS-9 LMG"),
        )
    }
    fn promo() -> ItemTags {
        item(
            "ksar_lmg_energy_01_store01",
            &[
                "LootGeneration/CannotGenerateAsLoot/PromotionalItem",
                COMMON,
                "Weapon/FPS/Stocked/LMG/Verdict",
            ],
            None,
        )
    }
    fn event_lmg() -> ItemTags {
        item(
            "event_lmg",
            &[
                CAN,
                COMMON,
                "Weapon/FPS/Stocked/LMG",
                "LootGeneration/Event/Xmas",
            ],
            None,
        )
    }
    fn items() -> Vec<ItemTags> {
        vec![
            verdict(),
            fs9(),
            item(
                "gmni_lmg_rare",
                &[
                    CAN,
                    RARE,
                    "Weapon/FPS/Stocked/LMG/F55",
                    "Manufacturer/Gemini",
                ],
                None,
            ),
            item(
                "ksar_pistol",
                &[
                    CAN,
                    COMMON,
                    "Weapon/FPS/Pistol/Hellion",
                    "Manufacturer/KastakArms",
                ],
                None,
            ),
            promo(),
            event_lmg(),
        ]
    }

    fn loot() -> LootData {
        serde_json::from_value(json!({
          "tables": [
            {"name": "Crate_Weapons", "version": "v1", "lootArchetypes": [{"archetype": "Arch_Weapons", "weight": 3}, {"archetype": "Arch_Ammo", "weight": 1}]},
            {"name": "V3_Weapons", "version": "v3", "lootArchetypes": [{"name": "Guns", "weight": 1, "archetype": {"lootArchetypeRecord": "V3Arch_Guns"}}]}
          ],
          "archetypes": [
            {"name": "Arch_Weapons", "version": "v1", "excludedTags": {"tags": ["LootGeneration/Event"]},
             "primaryOrGroup": {"entries": [{"name": "LMG", "tag": "Weapon/FPS/Stocked/LMG", "weight": 1}, {"name": "Pistols", "tag": "Weapon/FPS/Pistol", "weight": 1}]},
             "secondaryOrGroups": [{"groupName": "Rarity", "entries": [{"tag": COMMON, "weight": 3}, {"tag": RARE, "weight": 1}]}]},
            {"name": "Arch_Ammo", "version": "v1",
             "primaryOrGroup": {"entries": [{"name": "LMG ammo", "tag": "Weapon/FPS/Attachment/Magazine", "additionalTags": {"positiveTags": ["Weapon/FPS/Stocked/LMG"]}, "weight": 1}]}},
            {"name": "V3Arch_Guns", "version": "v3", "entries": [{"name": "LMG", "weight": 1, "selector": {"tags": {"positiveTags": ["Weapon/FPS/Stocked/LMG"]}}}]}
          ],
          "filters": [
            {"name": "V3PoolFilter_NoEvents", "filter": {"tagFilter": {"negativeTags": ["LootGeneration/Event"]}}},
            {"name": "V3PoolFilter_Generic", "filter": {"filters": [{"mode": "Cumulative", "filter": {"filterRecord": "v3poolfilter_noevents"}}]}}
          ],
          "secondaryChoices": [
            {"name": "V3ManufacturerWeights_Military", "choices": [
              {"name": "BEHR", "weight": 99, "selector": {"tags": {"positiveTags": ["Manufacturer/Behring"]}}},
              {"name": "KSAR", "weight": 1, "selector": {"tags": {"positiveTags": ["Manufacturer/KastakArms"]}}}]},
            {"name": "V3ManufacturerWeights_Even", "choices": [{"name": "Anything", "weight": 1, "selector": {"tags": {"negativeTags": ["Weapon/FPS/Melee"]}}}]},
            {"name": "V3Multi_Military", "layers": ["v3manufacturerweights_military"]}
          ]
        }))
        .unwrap()
    }

    fn tags(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn tag_rules() {
        assert!(has_tag(
            &tags(&["Weapon/FPS/Stocked/LMG/Verdict"]),
            "Weapon/FPS/Stocked/LMG"
        ));
        assert!(!has_tag(
            &tags(&["Weapon/FPS/Stocked/LMGX"]),
            "Weapon/FPS/Stocked/LMG"
        ));
        assert!(selects(&[], &tags(&["Weapon/FPS"]), &tags(&["Item/Food"])));
        assert!(!selects(
            &[],
            &tags(&["Weapon/FPS"]),
            &tags(&["Weapon/FPS/Pistol"])
        ));
        assert_eq!(
            loot_eligibility(&verdict().tags),
            LootEligibility {
                can_loot: Some(true),
                reason: None,
                rarity: Some("Common".into())
            }
        );
        assert_eq!(
            loot_eligibility(&promo().tags),
            LootEligibility {
                can_loot: Some(false),
                reason: Some("PromotionalItem".into()),
                rarity: Some("Common".into())
            }
        );
        assert_eq!(loot_eligibility(&tags(&["Weapon/FPS"])).can_loot, None);
    }

    #[test]
    fn table_rolls_multiply_shares_and_size_the_pool() {
        let index = build_loot_index(&loot());
        let pool = items();
        let ctx = RollContext {
            pool: Some(&pool),
            ..Default::default()
        };
        let paths = table_rolls(&index, "Crate_Weapons", &verdict(), ctx);
        assert!(close(paths[0].share, 0.75 * 0.5 * 0.75));
        assert_eq!(paths[0].pool_size, Some(2));
        assert!(close(paths[0].per_roll, paths[0].share / 2.0));
        let no_pool = table_rolls(&index, "Crate_Weapons", &verdict(), RollContext::default());
        assert_eq!(
            no_pool
                .iter()
                .map(|p| p.archetype.as_str())
                .collect::<Vec<_>>(),
            ["Arch_Weapons"]
        );
        assert!(table_rolls(&index, "Crate_Weapons", &promo(), RollContext::default()).is_empty());
        let npc = item("npc_lmg_lr01", &[COMMON, "Weapon/FPS/Stocked/LMG"], None);
        assert!(table_rolls(&index, "Crate_Weapons", &npc, RollContext::default()).is_empty());
        assert!(
            table_rolls(
                &index,
                "Crate_Weapons",
                &event_lmg(),
                RollContext::default()
            )
            .is_empty()
        );
    }

    #[test]
    fn v3_pool_filter_and_manufacturer_weights() {
        let index = build_loot_index(&loot());
        let pool = items();
        let filter = index.filters.get("v3poolfilter_generic");
        let layers = index.choices.get("v3multi_military").unwrap();
        let ctx = RollContext {
            filter,
            extra_layers: layers,
            pool: Some(&pool),
            memo: None,
            entry_pools: None,
        };
        assert!(close(
            table_rolls(&index, "V3_Weapons", &verdict(), ctx)[0].share,
            0.01
        ));
        assert!(close(
            table_rolls(&index, "V3_Weapons", &fs9(), ctx)[0].share,
            0.99
        ));
        let fctx = RollContext {
            filter,
            ..Default::default()
        };
        assert!(table_rolls(&index, "V3_Weapons", &event_lmg(), fctx).is_empty());
    }

    fn areas() -> Vec<LootArea> {
        let loot = |table: &str, version: &str, sc: Option<&str>, max: f64| SpawnLoot {
            table: table.into(),
            version: version.into(),
            secondary_choices: sc.map(str::to_string),
            max_results: Some(max),
            ..Default::default()
        };
        vec![
            LootArea {
                name: "military".into(),
                configs: vec![
                    SpawnConfig {
                        name: Some("Weapons Large Common".into()),
                        spawn_tags: tags(&["LootSize/Large", "LootContents/Weapons"]),
                        fill_chance: None,
                        slots: vec![SpawnSlot {
                            entity: "crate_large".into(),
                            weight: 1.0,
                            loot: Some(loot("V3_Weapons", "v3", Some("V3Multi_Military"), 3.0)),
                            ..Default::default()
                        }],
                    },
                    SpawnConfig {
                        name: Some("Rack".into()),
                        spawn_tags: vec![],
                        fill_chance: None,
                        slots: vec![SpawnSlot {
                            entity: "rack_fs9".into(),
                            weight: 1.0,
                            contains: Some(vec!["behr_lmg_ballistic_01".into()]),
                            ..Default::default()
                        }],
                    },
                ],
            },
            LootArea {
                name: "caves".into(),
                configs: vec![SpawnConfig {
                    name: Some("Weapons Small Common".into()),
                    spawn_tags: vec![],
                    fill_chance: None,
                    slots: vec![
                        SpawnSlot {
                            entity: "crate_small".into(),
                            weight: 3.0,
                            loot: Some(loot("Crate_Weapons", "v1", None, 2.0)),
                            ..Default::default()
                        },
                        SpawnSlot {
                            entity: "ksar_lmg_energy_01".into(),
                            weight: 1.0,
                            ..Default::default()
                        },
                    ],
                }],
            },
        ]
    }

    fn datasets() -> Datasets {
        Datasets {
            items: items(),
            loot: Some(loot()),
            areas: Some(areas()),
            ..Default::default()
        }
    }

    #[test]
    fn spawns_loot_fixed_loose_ranked() {
        let d = datasets();
        let index = build_loot_index(d.loot.as_ref().unwrap());
        let hits = spawn_hits(&d, Some(&index), &verdict());
        let kinds: Vec<String> = hits
            .iter()
            .map(|h| {
                format!(
                    "{}:{}",
                    h.area,
                    serde_json::to_value(h.kind).unwrap().as_str().unwrap()
                )
            })
            .collect();
        for k in ["caves:loose", "caves:loot", "military:loot"] {
            assert!(kinds.contains(&k.to_string()), "{k}");
        }
        assert_eq!(hits[0].kind, SpawnKind::Loose);
        assert!(close(hits[0].score, 0.25));
        let ranked: Vec<String> = rank_areas(&hits).into_iter().map(|a| a.area).collect();
        assert_eq!(ranked, ["caves", "military"]);
        let fixed: Vec<SpawnHit> = spawn_hits(&d, Some(&index), &fs9())
            .into_iter()
            .filter(|h| h.kind == SpawnKind::Fixed)
            .collect();
        assert_eq!(fixed.len(), 1);
        assert_eq!(fixed[0].entity, "rack_fs9");
    }

    #[test]
    fn blueprints_awards_and_shops_in_one_report() {
        let mut d = datasets();
        d.blueprints = Some(vec![BlueprintLike {
            name: "BP_FS9".into(),
            produces: Some("behr_lmg_ballistic_01".into()),
            rewarded_by: Some(vec![RewardRef {
                mission: Some("Bounty".into()),
                ..Default::default()
            }]),
            ..Default::default()
        }]);
        d.contract_awards = Some(vec![ContractAward {
            generator: "TheCollector".into(),
            contract: "TheCollector_FS9".into(),
            item: "behr_lmg_ballistic_01".into(),
            ..Default::default()
        }]);
        let row = |uuid: Option<&str>, name: Option<&str>, terminal: &str, buy: f64| ShopPrice {
            item_uuid: uuid.map(str::to_string),
            item_name: name.map(str::to_string),
            terminal_name: Some(terminal.into()),
            price_buy: Some(buy),
            ..Default::default()
        };
        d.shops = Some(vec![
            row(Some("id-behr_lmg_ballistic_01"), None, "Guns Ruin", 7029.0),
            row(None, Some("FS-9 LMG"), "No uuid row", 1.0),
            row(Some("someone-else"), Some("FS-9 LMG"), "Wrong", 2.0),
        ]);
        let index = build_loot_index(d.loot.as_ref().unwrap());
        let report = obtain(&d, &fs9(), Some(&index), &ObtainOptions::default());
        assert_eq!(report.blueprints.len(), 1);
        assert_eq!(report.contract_awards[0].contract, "TheCollector_FS9");
        let names: Vec<&str> = report
            .shops
            .iter()
            .map(|s| s.row.terminal_name.as_deref().unwrap())
            .collect();
        assert_eq!(names, ["Guns Ruin", "No uuid row"]);
        let text = render_obtain(&report, 15, Some(true));
        assert!(text.contains("Guns Ruin"));
        assert!(text.contains("TheCollector_FS9"));
    }

    #[test]
    fn places_dates_and_filters_shop_rows() {
        let mut d = datasets();
        let row = |id: f64, t: &str, buy: f64, ms: f64| ShopPrice {
            item_uuid: Some("id-behr_lmg_ballistic_01".into()),
            id_terminal: Some(id),
            terminal_name: Some(t.into()),
            price_buy: Some(buy),
            date_modified: Some(ms / 1000.0),
            ..Default::default()
        };
        // 2026-11-10 and 2026-09-01, UTC.
        d.shops = Some(vec![
            row(1.0, "Guns Ruin", 7029.0, 1_794_268_800_000.0),
            row(2.0, "Dumper's GrimHEX", 6900.0, 1_788_220_800_000.0),
        ]);
        let t =
            |id: f64, nick: &str, display: &str, moon: Option<&str>, orbit: &str, system: &str| {
                UexTerminal {
                    id: Some(id),
                    nickname: Some(nick.into()),
                    displayname: Some(display.into()),
                    moon_name: moon.map(str::to_string),
                    orbit_name: Some(orbit.into()),
                    star_system_name: Some(system.into()),
                    ..Default::default()
                }
            };
        d.terminals = Some(vec![
            t(1.0, "Guns Ruin", "Ruin Station", None, "Terminus", "Pyro"),
            t(
                2.0,
                "Dumper's GrimHEX",
                "Grim HEX",
                Some("Yela"),
                "Crusader",
                "Stanton",
            ),
        ]);
        d.shops_as_of = Some(AsOf::Iso("2026-11-14T00:00:00Z".into()));
        let index = build_loot_index(d.loot.as_ref().unwrap());
        let all = obtain(&d, &fs9(), Some(&index), &ObtainOptions::default());
        let got: Vec<(String, Option<String>, bool)> = all
            .shops
            .iter()
            .map(|s| (s.location.clone(), s.updated.clone(), s.stale))
            .collect();
        assert_eq!(
            got,
            vec![
                (
                    "Ruin Station, Terminus, Pyro".into(),
                    Some("2026-11-10".into()),
                    false
                ),
                (
                    "Grim HEX, Yela, Crusader, Stanton".into(),
                    Some("2026-09-01".into()),
                    true
                ),
            ]
        );
        assert_eq!(all.at, None);
        let there = obtain(
            &d,
            &fs9(),
            Some(&index),
            &ObtainOptions {
                at: Some("grim hex"),
                ..Default::default()
            },
        );
        assert_eq!(there.shops.len(), 1);
        assert_eq!(there.shops[0].terminal, "Dumper's GrimHEX");
        assert_eq!(
            (there.at.as_deref(), there.shops_elsewhere),
            (Some("grim hex"), Some(1))
        );
        let text = render_obtain(&there, 15, Some(true));
        assert!(text.contains("shops at \"grim hex\""));
        assert!(text.contains(
            "6,900 aUEC  2026-09-01 stale Dumper's GrimHEX (Grim HEX, Yela, Crusader, Stanton)"
        ));
        let nowhere = obtain(
            &d,
            &fs9(),
            Some(&index),
            &ObtainOptions {
                at: Some("orison"),
                ..Default::default()
            },
        );
        assert!(
            render_obtain(&nowhere, 15, Some(true))
                .contains("not sold at \"orison\" as far as UEX knows; 2 shop row(s) elsewhere")
        );
    }

    #[test]
    fn barred_item_says_why() {
        let d = datasets();
        let index = build_loot_index(d.loot.as_ref().unwrap());
        let text = render_obtain(
            &obtain(&d, &promo(), Some(&index), &ObtainOptions::default()),
            15,
            None,
        );
        assert!(text.contains("CannotGenerateAsLoot/PromotionalItem"));
        assert!(!text.contains("best areas"));
    }

    #[test]
    fn find_items_exact_then_words() {
        let all = items();
        let classes = |q: &str| -> Vec<String> {
            find_items(&all, q)
                .into_iter()
                .map(|i| i.class_name.clone())
                .collect()
        };
        assert_eq!(classes("verdict lmg"), ["ksar_lmg_energy_01"]);
        assert_eq!(classes("ksar_pistol"), ["ksar_pistol"]);
        assert_eq!(classes("kastak lmg"), ["ksar_lmg_energy_01"]);
        assert_eq!(classes("kastak"), ["ksar_lmg_energy_01", "ksar_pistol"]);
    }

    #[test]
    fn ammo_resolution_and_label() {
        let mag = |class: &str, name: &str| {
            let mut i = item(class, &[], Some(name));
            i.sub_type = Some("Magazine".into());
            i
        };
        let canister = mag(
            "grin_tool_resource_salvage_repair_01",
            "Cambio SRT Canister",
        );
        let filled = mag(
            "grin_tool_resource_salvage_repair_01_filled",
            "Cambio SRT Canister",
        );
        let lite = mag(
            "grin_multitool_resource_salvage_repair_01",
            "Cambio-Lite SRT Canister",
        );
        let weapon = |class: &str, name: &str, ammo: &[&ItemTags]| {
            let mut i = item(class, &[], Some(name));
            i.ty = Some("WeaponPersonal".into());
            i.ammo_items = Some(ammo.iter().map(|a| a.class_name.clone()).collect());
            i
        };
        let cambio = weapon(
            "grin_salvage_repair_01",
            "Cambio SRT",
            &[&filled, &canister],
        );
        let skin = weapon(
            "grin_salvage_repair_01_lumi01",
            "Cambio \"Piconalia\" SRT",
            &[&filled, &canister],
        );
        let multitool = weapon("grin_multitool_01", "Pyro RYT Multi-Tool", &[&lite]);
        let all = vec![
            canister.clone(),
            filled.clone(),
            lite,
            cambio.clone(),
            skin.clone(),
            multitool,
        ];
        let m: Vec<&ItemTags> = vec![&all[3], &all[4], &all[0]];
        let targets = ammo_targets(&all, &m);
        let classes: Vec<&str> = targets.iter().map(|t| t.ammo.class_name.as_str()).collect();
        assert_eq!(
            classes,
            [filled.class_name.as_str(), canister.class_name.as_str()]
        );
        assert_eq!(targets[0].weapons.len(), 2);
        assert!(ammo_targets(&all, &[&all[0]]).is_empty());
        assert_eq!(weapons_label(&["Cambio SRT"]), "Cambio SRT");
        assert_eq!(
            weapons_label(&["Cambio SRT", "Cambio \"Piconalia\" SRT"]),
            "Cambio SRT (+1 variant)"
        );
        let d = Datasets {
            items: all.clone(),
            shops: Some(vec![ShopPrice {
                item_uuid: Some(canister.id.clone()),
                id_terminal: Some(2.0),
                terminal_name: Some("Dumper's GrimHEX".into()),
                price_buy: Some(275.0),
                ..Default::default()
            }]),
            terminals: Some(vec![UexTerminal {
                id: Some(2.0),
                nickname: Some("Dumper's GrimHEX".into()),
                displayname: Some("Grim HEX".into()),
                moon_name: Some("Yela".into()),
                star_system_name: Some("Stanton".into()),
                ..Default::default()
            }]),
            ..Default::default()
        };
        let report = obtain(
            &d,
            &canister,
            None,
            &ObtainOptions {
                at: Some("grimhex"),
                ammo_for: &[&all[3]],
            },
        );
        assert_eq!(
            report.ammo_for,
            Some(vec![NamedItem {
                class_name: cambio.class_name.clone(),
                name: "Cambio SRT".into()
            }])
        );
        let text = render_obtain(&report, 15, Some(true));
        assert_eq!(
            text.lines().next().unwrap(),
            "ammo for Cambio SRT: Cambio SRT Canister  (grin_tool_resource_salvage_repair_01)"
        );
        assert!(text.contains("275 aUEC"));
        assert!(!text.contains("Cambio-Lite"));
    }

    #[test]
    fn percentages() {
        assert_eq!(pct(0.25), "25.0%");
        assert_eq!(pct(0.05), "5.00%");
        assert_eq!(pct(0.0005), "0.050%");
        assert_eq!(pct(0.0000123), "0.0012%");
        assert_eq!(pct(0.0), "0%");
    }

    #[test]
    fn bulk_answers_as_one_report_per_item() {
        let mut d = datasets();
        let row = |uuid: Option<&str>, name: Option<&str>, buy: f64| ShopPrice {
            item_uuid: uuid.map(str::to_string),
            item_name: name.map(str::to_string),
            terminal_name: Some("Shop".into()),
            price_buy: Some(buy),
            ..Default::default()
        };
        let first = d.items[0].clone();
        d.shops = Some(vec![
            row(Some(&first.id), None, 10.0),
            row(None, Some(&first.name.to_uppercase()), 12.0),
            row(Some("someone-else"), Some(&first.name), 9.0),
        ]);
        let index = build_loot_index(d.loot.as_ref().unwrap());
        let bulk = BulkObtain::new(&d, Some(&index));
        let mut any = 0;
        for item in &d.items {
            let r = obtain(&d, item, Some(&index), &ObtainOptions::default());
            assert_eq!(bulk.spawn_hits(item), r.spawns, "{}", item.class_name);
            assert_eq!(bulk.areas(item), r.areas, "{}", item.class_name);
            assert_eq!(bulk.shops(item), r.shops, "{}", item.class_name);
            any += r.spawns.len();
        }
        assert!(any > 0);
        assert_eq!(bulk.shops(&first).len(), 2);
        let all: Vec<&ItemTags> = d.items.iter().collect();
        let areas = bulk.areas_all(&all);
        for (item, a) in all.iter().zip(&areas) {
            let mut want = bulk.areas(item);
            for w in &mut want {
                w.best.paths = None;
            }
            assert_eq!(*a, want);
        }
    }
}
