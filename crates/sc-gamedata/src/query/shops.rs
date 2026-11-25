//! Shop rows with a place and an age: UEX item prices joined to UEX terminals (port of
//! `query/shops.ts`).
//!
//! Shop stock is server-side and in no game file, so both inputs are UEX data, reported by
//! players. A price row names its terminal by id and nickname; the terminal row adds the
//! station, moon, planet and system. Each row carries the day UEX last confirmed it and a
//! stale flag past [`STALE_DAYS`]. Place matching takes the best tier that hits any
//! terminal: (1) the query is one of a terminal's names, (2) every query word is a word or
//! word start of some name, (3) for 4+ letters, a substring of a name. Filler words
//! ("station", "at", "shop") are dropped.

use super::types::ItemTags;
use super::{en_us, js_len, parse_iso_millis, words};
use crate::collate::locale_compare;
use crate::output::{iso_from_millis, js_number};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};

/// One `prices` row of `uex-item-prices.json`. Fields the query does not read are kept in
/// `extra` and written back, as the TypeScript spreads the row: in the dataset's column
/// order (`trimItemPriceRow`'s), so a row serialises as the raw object does.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
pub struct ShopPrice {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item_uuid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id_terminal: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price_buy: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price_sell: Option<f64>,
    /// Unix seconds: when UEX last had the row confirmed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date_modified: Option<f64>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Serialize for ShopPrice {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(None)?;
        let extra = |k: &str| self.extra.get(k);
        // `uex_market::ITEM_PRICE_KEYS` order, then any other column as stored.
        if let Some(v) = &self.item_name {
            map.serialize_entry("item_name", v)?;
        }
        if let Some(v) = &self.item_uuid {
            map.serialize_entry("item_uuid", v)?;
        }
        for k in ["id_item", "id_category"] {
            if let Some(v) = extra(k) {
                map.serialize_entry(k, v)?;
            }
        }
        if let Some(v) = &self.terminal_name {
            map.serialize_entry("terminal_name", v)?;
        }
        for (k, v) in [
            ("id_terminal", self.id_terminal),
            ("price_buy", self.price_buy),
            ("price_sell", self.price_sell),
            ("date_modified", self.date_modified),
        ] {
            if let Some(v) = v {
                map.serialize_entry(k, &v)?;
            }
        }
        for (k, v) in &self.extra {
            if k != "id_item" && k != "id_category" {
                map.serialize_entry(k, v)?;
            }
        }
        map.end()
    }
}

/// A UEX terminal (`uex-terminals.json` `terminals`).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct UexTerminal {
    #[serde(default)]
    pub id: Option<f64>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub fullname: Option<String>,
    #[serde(default)]
    pub nickname: Option<String>,
    #[serde(default)]
    pub displayname: Option<String>,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default, rename = "type")]
    pub ty: Option<String>,
    #[serde(default)]
    pub star_system_name: Option<String>,
    #[serde(default)]
    pub planet_name: Option<String>,
    #[serde(default)]
    pub orbit_name: Option<String>,
    #[serde(default)]
    pub moon_name: Option<String>,
    #[serde(default)]
    pub space_station_name: Option<String>,
    #[serde(default)]
    pub city_name: Option<String>,
    #[serde(default)]
    pub outpost_name: Option<String>,
    #[serde(default)]
    pub is_available: Option<f64>,
    #[serde(default)]
    pub is_available_live: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShopLocation {
    /// The terminal as price rows name it, e.g. "Dumper's GrimHEX".
    pub terminal: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal_id: Option<f64>,
    /// Place, moon or orbit, planet, system.
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    /// `Some(false)` when UEX marks the terminal unavailable on LIVE.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub live: Option<bool>,
    /// Normalised names to match a place against.
    pub tokens: Vec<String>,
    /// Lower-case words of every name.
    pub words: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LocatedShop {
    #[serde(flatten)]
    pub row: ShopPrice,
    pub terminal: String,
    pub location: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub live: Option<bool>,
    /// `date_modified` as YYYY-MM-DD (UTC).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated: Option<String>,
    pub stale: bool,
}

pub const STALE_DAYS: f64 = 30.0;
const DAY_MS: f64 = 86_400_000.0;

/// Lower case, letters and digits only: "Grim HEX" -> "grimhex".
pub fn normalise_place(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        .collect()
}

fn text(v: Option<&str>) -> Option<String> {
    v.map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn truthy_num(x: Option<f64>) -> bool {
    x.is_some_and(|v| v != 0.0 && !v.is_nan())
}

/// Same place when equal, or one name's words start the other's.
fn same_name(a: &str, b: &str) -> bool {
    if normalise_place(a) == normalise_place(b) {
        return true;
    }
    let (wa, wb) = (words(a), words(b));
    let (x, y) = if wb.len() < wa.len() {
        (wb, wa)
    } else {
        (wa, wb)
    };
    !x.is_empty() && x.iter().enumerate().all(|(i, w)| y.get(i) == Some(w))
}

/// `c.replace(/\s*\(<system>( system)?\)$/i, "") || c`.
fn strip_system_suffix(c: &str, system: &str) -> String {
    for suffix in [format!("({system} system)"), format!("({system})")] {
        let n = suffix.len();
        if c.len() >= n
            && c.is_char_boundary(c.len() - n)
            && c[c.len() - n..].eq_ignore_ascii_case(&suffix)
        {
            let cut = c[..c.len() - n].trim_end();
            return if cut.is_empty() {
                c.to_string()
            } else {
                cut.to_string()
            };
        }
    }
    c.to_string()
}

/// Where a terminal is: place, moon, orbit, planet, then the star system.
pub fn terminal_location(t: Option<&UexTerminal>, fallback_name: Option<&str>) -> ShopLocation {
    let terminal = text(t.and_then(|t| t.nickname.as_deref()))
        .or_else(|| text(fallback_name))
        .or_else(|| text(t.and_then(|t| t.name.as_deref())))
        .unwrap_or_else(|| match t {
            Some(t) => format!("terminal {}", t.id.map_or("undefined".into(), js_number)),
            None => "unknown terminal".into(),
        });
    let Some(t) = t else {
        let n = normalise_place(&terminal);
        return ShopLocation {
            tokens: if n.is_empty() { vec![] } else { vec![n] },
            words: words(&terminal),
            label: terminal.clone(),
            terminal,
            terminal_id: None,
            system: None,
            live: None,
        };
    };
    let system = text(t.star_system_name.as_deref());
    let clean = |c: Option<String>| -> Option<String> {
        match (c, &system) {
            (Some(c), Some(s)) => Some(strip_system_suffix(&c, s)),
            (c, _) => c,
        }
    };
    let fixed = text(t.space_station_name.as_deref())
        .or_else(|| text(t.city_name.as_deref()))
        .or_else(|| text(t.outpost_name.as_deref()));
    let display = text(t.displayname.as_deref());
    let name = text(t.name.as_deref());
    let names_place = |d: &str| {
        name.as_deref().is_some_and(|n| {
            normalise_place(n) != normalise_place(d)
                && normalise_place(n).ends_with(&normalise_place(d))
        })
    };
    let cleaned_len = |s: &str| js_len(&clean(Some(s.to_string())).unwrap_or_default());
    let longer = |a: &str, b: &str| {
        if cleaned_len(b) > cleaned_len(a) {
            b.to_string()
        } else {
            a.to_string()
        }
    };
    let place = match (&display, &fixed) {
        (Some(d), Some(f)) if same_name(d, f) => Some(longer(d, f)),
        (Some(d), f) if f.is_none() || names_place(d) => Some(d.clone()),
        _ => fixed.clone().or(display.clone()),
    };
    let mut parts: Vec<String> = Vec::new();
    for c in [
        place,
        text(t.moon_name.as_deref()),
        text(t.orbit_name.as_deref()),
        text(t.planet_name.as_deref()),
    ]
    .into_iter()
    .map(clean)
    {
        let Some(c) = c.filter(|c| !c.is_empty()) else {
            continue;
        };
        if parts.iter().any(|p| same_name(p, &c)) {
            continue;
        }
        parts.push(c);
    }
    if let Some(s) = &system
        && !parts
            .iter()
            .any(|p| normalise_place(p) == normalise_place(s))
    {
        parts.push(s.clone());
    }
    let label = parts.join(", ");
    let all_names: Vec<String> = [
        Some(terminal.as_str()),
        t.name.as_deref(),
        t.fullname.as_deref(),
        t.nickname.as_deref(),
        t.displayname.as_deref(),
        t.code.as_deref(),
        t.space_station_name.as_deref(),
        t.city_name.as_deref(),
        t.outpost_name.as_deref(),
        t.moon_name.as_deref(),
        t.orbit_name.as_deref(),
        t.planet_name.as_deref(),
        t.star_system_name.as_deref(),
        fallback_name,
    ]
    .into_iter()
    .filter_map(text)
    .collect();
    let mut tokens = Vec::new();
    let mut seen = HashSet::new();
    for n in all_names.iter().map(|n| normalise_place(n)) {
        if !n.is_empty() && seen.insert(n.clone()) {
            tokens.push(n);
        }
    }
    let mut ws = Vec::new();
    let mut seen = HashSet::new();
    for w in all_names.iter().flat_map(|n| words(n)) {
        if seen.insert(w.clone()) {
            ws.push(w);
        }
    }
    ShopLocation {
        terminal: terminal.clone(),
        terminal_id: t.id,
        label: if label.is_empty() { terminal } else { label },
        system,
        live: (t.is_available_live == Some(0.0) || t.is_available == Some(0.0)).then_some(false),
        tokens,
        words: ws,
    }
}

fn id_key(id: f64) -> String {
    js_number(id)
}

/// Resolves price rows to places, one lookup per terminal; one spelling per place.
pub struct Locator<'a> {
    by_id: HashMap<String, &'a UexTerminal>,
    spelling: HashMap<String, String>,
    memo: std::cell::RefCell<HashMap<String, ShopLocation>>,
}

impl<'a> Locator<'a> {
    pub fn new(terminals: &'a [UexTerminal]) -> Self {
        let mut by_id: HashMap<String, &UexTerminal> = HashMap::new();
        let mut order: Vec<String> = Vec::new();
        for t in terminals {
            if let Some(id) = t.id {
                let k = id_key(id);
                if by_id.insert(k.clone(), t).is_none() {
                    order.push(k);
                }
            }
        }
        // The spelling most terminals at a place use wins; ties go to the first seen.
        let mut counts: Vec<(String, Vec<(String, usize)>)> = Vec::new();
        for k in &order {
            let label = terminal_location(Some(by_id[k]), None).label;
            let key = normalise_place(&label);
            let slot = match counts.iter().position(|(c, _)| *c == key) {
                Some(i) => &mut counts[i].1,
                None => {
                    counts.push((key, Vec::new()));
                    &mut counts.last_mut().unwrap().1
                }
            };
            match slot.iter_mut().find(|(l, _)| *l == label) {
                Some((_, n)) => *n += 1,
                None => slot.push((label, 1)),
            }
        }
        let mut spelling = HashMap::new();
        for (key, labels) in counts {
            let mut best = &labels[0];
            for l in &labels[1..] {
                if l.1 > best.1 {
                    best = l;
                }
            }
            spelling.insert(key, best.0.clone());
        }
        Self {
            by_id,
            spelling,
            memo: Default::default(),
        }
    }

    pub fn locate(&self, row: &ShopPrice) -> ShopLocation {
        let key = format!(
            "{}|{}",
            row.id_terminal.map(id_key).unwrap_or_default(),
            row.terminal_name.as_deref().unwrap_or("")
        );
        if let Some(l) = self.memo.borrow().get(&key) {
            return l.clone();
        }
        let t = row
            .id_terminal
            .and_then(|id| self.by_id.get(&id_key(id)).copied());
        let mut loc = terminal_location(t, row.terminal_name.as_deref());
        if let Some(label) = self.spelling.get(&normalise_place(&loc.label)) {
            loc.label = label.clone();
        }
        self.memo.borrow_mut().insert(key, loc.clone());
        loc
    }
}

/// Words a place query can carry that name no place.
const FILLER: &[&str] = &[
    "station",
    "at",
    "in",
    "on",
    "the",
    "of",
    "shop",
    "shops",
    "store",
    "terminal",
    "terminals",
    "near",
    "buy",
];

/// Shortest place query, after normalising, that is matched at all.
pub const MIN_PLACE_LENGTH: usize = 3;

#[derive(Debug, Clone, PartialEq)]
pub struct PlaceQuery {
    pub whole: String,
    pub words: Vec<String>,
}

/// The query's words without filler; `None` when too short to match.
pub fn place_query(place: &str) -> Option<PlaceQuery> {
    let all = words(place);
    let kept: Vec<String> = all
        .iter()
        .filter(|w| !FILLER.contains(&w.as_str()))
        .cloned()
        .collect();
    let used = if kept.is_empty() { all } else { kept };
    let whole = used.concat();
    (whole.len() >= MIN_PLACE_LENGTH).then_some(PlaceQuery { whole, words: used })
}

/// 1 exact name, 2 whole words or word starts, 3 substring (4+ letters).
pub fn place_tier(location: &ShopLocation, query: &PlaceQuery) -> Option<u8> {
    if location.tokens.contains(&query.whole) {
        return Some(1);
    }
    let word_hit = |q: &str| {
        location
            .words
            .iter()
            .any(|w| w == q || (q.len() >= 3 && w.starts_with(q)))
            || location
                .tokens
                .iter()
                .any(|t| t.starts_with(q) && q.len() >= 3)
    };
    if query.words.iter().all(|w| word_hit(w)) {
        return Some(2);
    }
    if query.whole.len() >= 4 && location.tokens.iter().any(|t| t.contains(&query.whole)) {
        return Some(3);
    }
    None
}

/// A filter for the locations matching `place` at the best tier any candidate reaches.
pub fn place_filter(
    candidates: &[ShopLocation],
    place: &str,
) -> impl Fn(&ShopLocation) -> bool + use<> {
    let query = place_query(place);
    let mut best: Option<u8> = None;
    if let Some(q) = &query {
        for c in candidates {
            if let Some(t) = place_tier(c, q)
                && best.is_none_or(|b| t < b)
            {
                best = Some(t);
            }
            if best == Some(1) {
                break;
            }
        }
    }
    move |loc| match (&query, best) {
        (Some(q), Some(b)) => place_tier(loc, q) == Some(b),
        _ => false,
    }
}

/// True when the place query names this location on its own (any tier).
pub fn place_matches(location: &ShopLocation, place: &str) -> bool {
    place_query(place).is_some_and(|q| place_tier(location, &q).is_some())
}

fn now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_millis() as f64)
}

/// The reference time for the stale flag.
#[derive(Debug, Clone, PartialEq)]
pub enum AsOf {
    Iso(String),
    Millis(f64),
}

/// Reference time in ms; now when absent or unreadable.
pub fn reference_time(as_of: Option<&AsOf>) -> f64 {
    let ms = match as_of {
        Some(AsOf::Millis(m)) => *m,
        Some(AsOf::Iso(s)) => parse_iso_millis(s).map_or(f64::NAN, |m| m as f64),
        None => f64::NAN,
    };
    if ms.is_finite() { ms } else { now_ms() }
}

/// `{ updated?, stale }` for a row.
pub fn price_age(
    date_modified: Option<f64>,
    ref_ms: f64,
    stale_days: f64,
) -> (Option<String>, bool) {
    match date_modified {
        Some(d) if d.is_finite() && d > 0.0 => {
            let ms = d * 1000.0;
            let day = iso_from_millis(ms as i64)[..10].to_string();
            (Some(day), ref_ms - ms > stale_days * DAY_MS)
        }
        _ => (None, true),
    }
}

#[derive(Debug, Clone, Default)]
pub struct LocateOptions<'a> {
    pub terminals: &'a [UexTerminal],
    pub as_of: Option<AsOf>,
    pub stale_days: Option<f64>,
    /// Keep only rows at this place.
    pub at: Option<&'a str>,
}

pub fn locate_shops(rows: &[&ShopPrice], options: &LocateOptions<'_>) -> Vec<LocatedShop> {
    let locator = Locator::new(options.terminals);
    let reference = reference_time(options.as_of.as_ref());
    let stale_days = options.stale_days.unwrap_or(STALE_DAYS);
    // The tier is chosen over every terminal, not just these rows.
    let matches = options.at.map(|at| {
        let mut candidates: Vec<ShopLocation> = options
            .terminals
            .iter()
            .map(|t| terminal_location(Some(t), None))
            .collect();
        candidates.extend(rows.iter().map(|r| locator.locate(r)));
        place_filter(&candidates, at)
    });
    let mut out = Vec::new();
    for row in rows {
        let loc = locator.locate(row);
        if let Some(m) = &matches
            && !m(&loc)
        {
            continue;
        }
        let (updated, stale) = price_age(row.date_modified, reference, stale_days);
        out.push(LocatedShop {
            row: (*row).clone(),
            terminal: loc.terminal,
            location: loc.label,
            system: loc.system,
            live: loc.live,
            updated,
            stale,
        });
    }
    out
}

/// "Dumper's GrimHEX (Grim HEX, Yela, Crusader, Stanton)", without repeating a place the
/// terminal already names.
pub fn place_label(terminal: &str, location: &str) -> String {
    if normalise_place(terminal).contains(&normalise_place(location)) {
        terminal.to_string()
    } else {
        format!("{terminal} ({location})")
    }
}

fn pad_start(s: &str, w: usize) -> String {
    let n = js_len(s);
    if n >= w {
        s.to_string()
    } else {
        format!("{}{s}", " ".repeat(w - n))
    }
}

pub(crate) fn pad_end(s: &str, w: usize) -> String {
    let n = js_len(s);
    if n >= w {
        s.to_string()
    } else {
        format!("{s}{}", " ".repeat(w - n))
    }
}

pub(crate) fn slice_chars(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn age_text(updated: Option<&str>, stale: bool) -> String {
    format!(
        "{}{}",
        updated.unwrap_or("undated"),
        if stale { " stale" } else { "" }
    )
}

pub fn shop_row_text(shop: &LocatedShop) -> String {
    let price = if truthy_num(shop.row.price_buy) {
        format!("{} aUEC", en_us(shop.row.price_buy.unwrap_or(0.0)))
    } else if truthy_num(shop.row.price_sell) {
        format!(
            "sells for {} aUEC",
            en_us(shop.row.price_sell.unwrap_or(0.0))
        )
    } else {
        "no price".into()
    };
    format!(
        "{}  {} {}{}",
        pad_start(&price, 12),
        pad_end(&age_text(shop.updated.as_deref(), shop.stale), 16),
        place_label(&shop.terminal, &shop.location),
        if shop.live == Some(false) {
            " [not on LIVE]"
        } else {
            ""
        }
    )
}

// ---------------------------------------------------------------- shop inventory

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShopItem {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uex_name: Option<String>,
    #[serde(rename = "price_buy", skip_serializing_if = "Option::is_none")]
    pub price_buy: Option<f64>,
    #[serde(rename = "price_sell", skip_serializing_if = "Option::is_none")]
    pub price_sell: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated: Option<String>,
    pub stale: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShopTerminal {
    pub terminal: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal_id: Option<f64>,
    pub location: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub live: Option<bool>,
    pub items: Vec<ShopItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item_count: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShopPlace {
    pub location: String,
    pub terminals: usize,
    pub items: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShopReport {
    pub place: String,
    pub terminals: Vec<ShopTerminal>,
    /// Price rows that matched, over every terminal.
    pub rows: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item_limit: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal_limit: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub list_only: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub places: Option<Vec<ShopPlace>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub place_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal_count: Option<usize>,
}

pub const PLACE_LIST_LIMIT: usize = 40;
pub const SHOP_ITEM_LIMIT: usize = 25;
pub const SHOP_TERMINAL_LIMIT: usize = 15;

#[derive(Debug, Clone, Default)]
pub struct ShopInputs<'a> {
    pub shops: &'a [ShopPrice],
    pub terminals: &'a [UexTerminal],
    pub items: &'a [ItemTags],
    pub as_of: Option<AsOf>,
    pub stale_days: Option<f64>,
}

/// Every item UEX lists at the terminals matching `place`, grouped by terminal.
pub fn shop_inventory(inputs: &ShopInputs<'_>, place: &str) -> ShopReport {
    let mut by_id: HashMap<&str, &ItemTags> = HashMap::new();
    for i in inputs.items {
        by_id.insert(i.id.as_str(), i);
    }
    let rows: Vec<&ShopPrice> = inputs.shops.iter().collect();
    let located = locate_shops(
        &rows,
        &LocateOptions {
            terminals: inputs.terminals,
            as_of: inputs.as_of.clone(),
            stale_days: inputs.stale_days,
            at: Some(place),
        },
    );
    let mut groups: Vec<ShopTerminal> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for r in located {
        if !truthy_num(r.row.price_buy) && !truthy_num(r.row.price_sell) {
            continue;
        }
        let id = r
            .row
            .id_terminal
            .map(id_key)
            .unwrap_or_else(|| r.terminal.clone());
        let gi = *index.entry(id).or_insert_with(|| {
            groups.push(ShopTerminal {
                terminal: r.terminal.clone(),
                terminal_id: r.row.id_terminal,
                location: r.location.clone(),
                live: (r.live == Some(false)).then_some(false),
                items: Vec::new(),
                item_count: None,
            });
            groups.len() - 1
        });
        let record = r
            .row
            .item_uuid
            .as_deref()
            .filter(|u| !u.is_empty())
            .and_then(|u| by_id.get(u).copied());
        let game_name = record.filter(|x| !x.template()).map(|x| x.name.clone());
        let name = game_name
            .clone()
            .or_else(|| r.row.item_name.clone())
            .or_else(|| record.map(|x| x.name.clone()))
            .or_else(|| r.row.item_uuid.clone())
            .unwrap_or_else(|| "unknown item".into());
        let uex_name = match (&game_name, &r.row.item_name) {
            (Some(g), Some(u)) if !u.is_empty() && u != g => Some(u.clone()),
            _ => None,
        };
        groups[gi].items.push(ShopItem {
            name,
            key: record.map(|x| x.class_name.clone()),
            uex_name,
            price_buy: r.row.price_buy.filter(|p| truthy_num(Some(*p))),
            price_sell: r.row.price_sell.filter(|p| truthy_num(Some(*p))),
            updated: r.updated,
            stale: r.stale,
        });
    }
    groups.sort_by(|a, b| {
        locale_compare(&normalise_place(&a.location), &normalise_place(&b.location))
            .then_with(|| locale_compare(&a.terminal, &b.terminal))
    });
    for g in &mut groups {
        g.items.sort_by(|a, b| locale_compare(&a.name, &b.name));
    }
    let rows = groups.iter().map(|g| g.items.len()).sum();
    ShopReport {
        place: place.to_string(),
        terminals: groups,
        rows,
        item_limit: None,
        terminal_limit: None,
        list_only: None,
        places: None,
        place_count: None,
        terminal_count: None,
    }
}

/// Cuts a report to `limit` items per terminal and, past `max_terminals`, to a list of
/// places. `None` turns either cut off.
pub fn limit_shop(
    report: &ShopReport,
    limit: Option<usize>,
    max_terminals: Option<usize>,
) -> ShopReport {
    if let Some(max) = max_terminals
        && report.terminals.len() > max
    {
        let mut places: Vec<ShopPlace> = Vec::new();
        for g in &report.terminals {
            match places.iter_mut().find(|p| p.location == g.location) {
                Some(p) => {
                    p.terminals += 1;
                    p.items += g.items.len();
                }
                None => places.push(ShopPlace {
                    location: g.location.clone(),
                    terminals: 1,
                    items: g.items.len(),
                }),
            }
        }
        places.sort_by(|a, b| {
            b.items
                .cmp(&a.items)
                .then_with(|| locale_compare(&a.location, &b.location))
        });
        let count = places.len();
        places.truncate(PLACE_LIST_LIMIT);
        return ShopReport {
            place: report.place.clone(),
            rows: report.rows,
            terminals: Vec::new(),
            item_limit: limit,
            terminal_limit: max_terminals,
            list_only: Some(true),
            places: Some(places),
            place_count: Some(count),
            terminal_count: Some(report.terminals.len()),
        };
    }
    let mut out = report.clone();
    for g in &mut out.terminals {
        g.item_count = Some(g.items.len());
        if let Some(l) = limit {
            g.items.truncate(l);
        }
    }
    out.item_limit = limit;
    out.terminal_limit = max_terminals;
    out
}

pub fn render_shop(
    report: &ShopReport,
    limit: Option<usize>,
    max_terminals: Option<usize>,
) -> String {
    if report.terminals.is_empty() {
        let short = if place_query(&report.place).is_some() {
            String::new()
        } else {
            format!(" (a place needs {MIN_PLACE_LENGTH}+ letters)")
        };
        return format!(
            "No UEX terminal at \"{}\" lists items{short}.",
            report.place
        );
    }
    let cut = limit_shop(report, limit, max_terminals);
    let mut lines = vec![format!(
        "{} terminal(s) at \"{}\", {} item row(s) (UEX, player-reported; stale = not confirmed in {} days)",
        report.terminals.len(),
        cut.place,
        cut.rows,
        STALE_DAYS
    )];
    if cut.list_only == Some(true) {
        lines.push(format!(
            "More than {} terminals match; name one place (station, city, outpost or terminal). {} place(s):",
            cut.terminal_limit.unwrap_or(0),
            cut.place_count.unwrap_or(0)
        ));
        for p in cut.places.iter().flatten() {
            lines.push(format!(
                "  {} {} terminal(s) {} item(s)",
                pad_end(&slice_chars(&p.location, 64), 64),
                pad_start(&p.terminals.to_string(), 3),
                pad_start(&p.items.to_string(), 5)
            ));
        }
        let more = cut.place_count.unwrap_or(0) - cut.places.as_ref().map_or(0, Vec::len);
        if more > 0 {
            lines.push(format!("  ... {more} more places"));
        }
        return lines.join("\n");
    }
    for g in &cut.terminals {
        lines.push(String::new());
        lines.push(format!(
            "{}{}  {} item(s)",
            place_label(&g.terminal, &g.location),
            if g.live == Some(false) {
                " [not on LIVE]"
            } else {
                ""
            },
            g.item_count.unwrap_or(g.items.len())
        ));
        for i in &g.items {
            let price = match i.price_buy {
                Some(p) => en_us(p),
                None => format!("sell {}", i.price_sell.map_or("undefined".into(), en_us)),
            };
            lines.push(format!(
                "  {} {}  {}",
                pad_end(&slice_chars(&i.name, 48), 48),
                pad_start(&price, 11),
                age_text(i.updated.as_deref(), i.stale)
            ));
        }
        let more = g.item_count.unwrap_or(g.items.len()) - g.items.len();
        if more > 0 {
            lines.push(format!("  ... {more} more (raise the limit)"));
        }
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn term(id: f64, f: impl FnOnce(&mut UexTerminal)) -> UexTerminal {
        let mut t = UexTerminal {
            id: Some(id),
            ..Default::default()
        };
        f(&mut t);
        t
    }
    fn s(x: &str) -> Option<String> {
        Some(x.to_string())
    }

    fn grimhex() -> UexTerminal {
        term(115.0, |t| {
            t.name = s("Dumper's Depot - GrimHEX");
            t.nickname = s("Dumper's GrimHEX");
            t.displayname = s("Grim HEX");
            t.star_system_name = s("Stanton");
            t.planet_name = s("Crusader");
            t.orbit_name = s("Crusader");
            t.moon_name = s("Yela");
            t.space_station_name = s("Green Imperial Housing Exchange");
            t.is_available = Some(1.0);
            t.is_available_live = Some(1.0);
        })
    }
    fn skutters() -> UexTerminal {
        let mut t = grimhex();
        t.id = Some(143.0);
        t.name = s("Skutters - GrimHEX");
        t.nickname = s("Skutters Grim");
        t
    }
    fn arc_l2() -> UexTerminal {
        term(93.0, |t| {
            t.nickname = s("Clothing ARC-L2");
            t.displayname = s("ARC-L2 Lively Pathway Station");
            t.star_system_name = s("Stanton");
            t.planet_name = s("ArcCorp");
            t.orbit_name = s("ArcCorp Lagrange Point 2");
            t.space_station_name = s("ARC-L2 Lively Pathway Station");
        })
    }
    fn gateway() -> UexTerminal {
        term(296.0, |t| {
            t.nickname = s("Platinum Bay Terra (Stanton)");
            t.displayname = s("Terra Gateway");
            t.star_system_name = s("Stanton");
            t.orbit_name = s("Terra Gateway (Stanton system)");
            t.space_station_name = s("Terra Gateway (Stanton)");
            t.is_available_live = Some(0.0);
        })
    }
    fn terminals() -> Vec<UexTerminal> {
        vec![grimhex(), skutters(), arc_l2(), gateway()]
    }

    const NOW: f64 = 1_794_614_400_000.0;
    fn days_ago(d: f64) -> f64 {
        ((NOW - d * DAY_MS) / 1000.0).floor()
    }
    fn price(
        name: &str,
        uuid: &str,
        id: f64,
        terminal: Option<&str>,
        buy: f64,
        age: Option<f64>,
    ) -> ShopPrice {
        ShopPrice {
            item_name: s(name),
            item_uuid: s(uuid),
            id_terminal: Some(id),
            terminal_name: terminal.map(str::to_string),
            price_buy: Some(buy),
            date_modified: age.map(days_ago),
            ..Default::default()
        }
    }
    fn canister() -> ShopPrice {
        price(
            "Cambio SRT Canister",
            "can",
            115.0,
            Some("Dumper's GrimHEX"),
            275.0,
            Some(70.0),
        )
    }
    fn tool() -> ShopPrice {
        price(
            "Cambio SRT",
            "srt",
            143.0,
            Some("Skutters Grim"),
            21200.0,
            Some(2.0),
        )
    }
    fn shirt() -> ShopPrice {
        price(
            "Shirt",
            "shirt",
            93.0,
            Some("Clothing ARC-L2"),
            90.0,
            Some(1.0),
        )
    }
    fn orphan() -> ShopPrice {
        price("Lost Thing", "lost", 9999.0, Some("Ghost Kiosk"), 5.0, None)
    }

    #[test]
    fn labels_place_moon_planet_system() {
        assert_eq!(
            terminal_location(Some(&grimhex()), None).label,
            "Grim HEX, Yela, Crusader, Stanton"
        );
        assert_eq!(
            terminal_location(Some(&arc_l2()), None).label,
            "ARC-L2 Lively Pathway Station, ArcCorp Lagrange Point 2, Stanton"
        );
        assert_eq!(
            terminal_location(Some(&gateway()), None).label,
            "Terra Gateway, Stanton"
        );
        assert_eq!(terminal_location(Some(&gateway()), None).live, Some(false));
        assert_eq!(
            terminal_location(Some(&grimhex()), None).terminal,
            "Dumper's GrimHEX"
        );
        let tokens = terminal_location(Some(&grimhex()), None).tokens;
        for t in [
            "grimhex",
            "yela",
            "crusader",
            "stanton",
            "greenimperialhousingexchange",
            "dumpersgrimhex",
        ] {
            assert!(tokens.contains(&t.to_string()), "{t}");
        }
        let loc = terminal_location(None, Some("Ghost Kiosk"));
        assert_eq!(
            (loc.terminal.as_str(), loc.label.as_str(), loc.tokens),
            ("Ghost Kiosk", "Ghost Kiosk", vec!["ghostkiosk".to_string()])
        );
    }

    #[test]
    fn place_matching() {
        let loc = terminal_location(Some(&grimhex()), None);
        assert_eq!(normalise_place("Grim HEX"), "grimhex");
        for q in [
            "grimhex",
            "grim hex",
            "GrimHEX",
            "Grim-Hex",
            "Yela",
            "crusader",
            "Stanton",
            "dumper's",
            "dumpers grimhex",
        ] {
            assert!(place_matches(&loc, q), "{q}");
        }
        assert!(!place_matches(&loc, "orison"));
        assert!(!place_matches(&loc, "grim orison"));
        assert!(!place_matches(&loc, "  "));
    }

    #[test]
    fn price_ages() {
        assert_eq!(
            price_age(Some(days_ago(2.0)), NOW, STALE_DAYS),
            (Some("2026-11-12".into()), false)
        );
        assert!(price_age(Some(days_ago(31.0)), NOW, STALE_DAYS).1);
        assert!(price_age(Some(days_ago(10.0)), NOW, 7.0).1);
        assert_eq!(price_age(None, NOW, STALE_DAYS), (None, true));
    }

    #[test]
    fn locate_and_filter() {
        let rows_owned = [canister(), tool(), shirt(), orphan()];
        let rows: Vec<&ShopPrice> = rows_owned.iter().collect();
        let terms = terminals();
        let opts = |at| LocateOptions {
            terminals: &terms,
            as_of: Some(AsOf::Millis(NOW)),
            stale_days: None,
            at,
        };
        let located = locate_shops(&rows, &opts(None));
        let got: Vec<(String, String, bool)> = located
            .iter()
            .map(|r| (r.terminal.clone(), r.location.clone(), r.stale))
            .collect();
        assert_eq!(
            got,
            vec![
                (
                    "Dumper's GrimHEX".into(),
                    "Grim HEX, Yela, Crusader, Stanton".into(),
                    true
                ),
                (
                    "Skutters Grim".into(),
                    "Grim HEX, Yela, Crusader, Stanton".into(),
                    false
                ),
                (
                    "Clothing ARC-L2".into(),
                    "ARC-L2 Lively Pathway Station, ArcCorp Lagrange Point 2, Stanton".into(),
                    false
                ),
                ("Ghost Kiosk".into(), "Ghost Kiosk".into(), true),
            ]
        );
        assert_eq!(located[0].updated.as_deref(), Some("2026-09-05"));
        assert_eq!(located[0].system.as_deref(), Some("Stanton"));
        let uuids = |v: Vec<LocatedShop>| -> Vec<String> {
            v.into_iter().map(|r| r.row.item_uuid.unwrap()).collect()
        };
        assert_eq!(
            uuids(locate_shops(&rows, &opts(Some("grim hex")))),
            ["can", "srt"]
        );
        assert_eq!(uuids(locate_shops(&rows, &opts(Some("ghost")))), ["lost"]);
        let none = LocateOptions {
            terminals: &[],
            as_of: Some(AsOf::Millis(NOW)),
            stale_days: None,
            at: Some("yela"),
        };
        assert!(locate_shops(&rows, &none).is_empty());
        let sk = LocateOptions {
            at: Some("skutters"),
            ..none
        };
        assert_eq!(uuids(locate_shops(&rows, &sk)), ["srt"]);
    }

    fn inv_items() -> Vec<ItemTags> {
        let it = |name: &str, class: &str, id: &str, tpl: bool| ItemTags {
            name: name.into(),
            class_name: class.into(),
            id: id.into(),
            is_template: tpl.then_some(true),
            ..Default::default()
        };
        vec![
            it(
                "Cambio SRT Canister",
                "grin_tool_resource_salvage_repair_01",
                "can",
                false,
            ),
            it("Cambio SRT", "grin_salvage_repair_01", "srt", false),
            it("grin_unnamed", "grin_unnamed", "tpl", true),
        ]
    }

    #[test]
    fn inventory_groups_by_terminal_with_game_names() {
        let extra = ShopPrice {
            item_name: s("UEX Name"),
            item_uuid: s("tpl"),
            id_terminal: Some(115.0),
            price_buy: Some(10.0),
            date_modified: Some(days_ago(1.0)),
            ..Default::default()
        };
        let shops = [canister(), tool(), shirt(), orphan(), extra];
        let terms = terminals();
        let items = inv_items();
        let report = shop_inventory(
            &ShopInputs {
                shops: &shops,
                terminals: &terms,
                items: &items,
                as_of: Some(AsOf::Millis(NOW)),
                stale_days: None,
            },
            "GrimHEX",
        );
        assert_eq!(report.rows, 3);
        let names: Vec<&str> = report
            .terminals
            .iter()
            .map(|t| t.terminal.as_str())
            .collect();
        assert_eq!(names, ["Dumper's GrimHEX", "Skutters Grim"]);
        assert_eq!(
            report.terminals[0].items,
            vec![
                ShopItem {
                    name: "Cambio SRT Canister".into(),
                    key: s("grin_tool_resource_salvage_repair_01"),
                    uex_name: None,
                    price_buy: Some(275.0),
                    price_sell: None,
                    updated: s("2026-09-05"),
                    stale: true
                },
                ShopItem {
                    name: "UEX Name".into(),
                    key: s("grin_unnamed"),
                    uex_name: None,
                    price_buy: Some(10.0),
                    price_sell: None,
                    updated: s("2026-11-13"),
                    stale: false
                },
            ]
        );
        let text = render_shop(&report, Some(SHOP_ITEM_LIMIT), Some(SHOP_TERMINAL_LIMIT));
        assert!(text.contains("2 terminal(s)"));
        assert!(text.contains("Skutters Grim (Grim HEX, Yela, Crusader, Stanton)"));
        assert!(text.lines().any(|l| {
            let w: Vec<&str> = l.split_whitespace().collect();
            w.ends_with(&["Canister", "275", "2026-09-05", "stale"]) && l.contains("275  2026")
        }));
        let none = shop_inventory(
            &ShopInputs {
                shops: &[tool()],
                terminals: &terms,
                ..Default::default()
            },
            "orison",
        );
        assert!(render_shop(&none, None, None).contains("No UEX terminal at \"orison\""));
    }

    #[test]
    fn real_shaped_terminals() {
        let sys = |t: &mut UexTerminal, x: &str| t.star_system_name = s(x);
        let personal = term(732.0, |t| {
            t.name = s("Personal Weapons - Security Tower");
            t.nickname = s("Personal Weapons");
            t.displayname = s("Personal Weapons");
            t.city_name = s("Lorville");
            t.orbit_name = s("Hurston");
            t.planet_name = s("Hurston");
            sys(t, "Stanton");
        });
        let noodle = term(829.0, |t| {
            t.name = s("Noodle Bar - Lorville");
            t.nickname = s("Noodle Bar - Lorville");
            t.displayname = s("Noodle Bar - Lorville");
            t.city_name = s("Lorville");
            t.orbit_name = s("Hurston");
            t.planet_name = s("Hurston");
            sys(t, "Stanton");
        });
        let ae_hub = term(731.0, |t| {
            t.name = s("AydoExpress Hub");
            t.nickname = s("AE Hub");
            t.displayname = s("AE Hub");
            t.space_station_name = s("Seraphim Station");
            t.orbit_name = s("Crusader");
            t.planet_name = s("Crusader");
            sys(t, "Stanton");
        });
        let arc_l1 = term(131.0, |t| {
            t.name = s("Platinum Bay - ARC-L1");
            t.nickname = s("Platinum ARC-L1");
            t.displayname = s("ARC-L1 Wide Forest Station");
            t.space_station_name = s("ARC-L1 Wide Forest Station");
            t.orbit_name = s("ArcCorp Lagrange Point 1");
            t.planet_name = s("ArcCorp");
            sys(t, "Stanton");
        });
        let pyro_gw = term(307.0, |t| {
            t.name = s("Platinum Bay - Pyro Gateway (Stanton)");
            t.nickname = s("Platinum Pyro Gateway (Stanton)");
            t.displayname = s("Pyro Gateway");
            t.space_station_name = s("Pyro Gateway (Stanton)");
            t.orbit_name = s("Pyro Gateway (Stanton system)");
            sys(t, "Stanton");
        });
        let rustville = term(600.0, |t| {
            t.name = s("Admin - Rustville");
            t.nickname = s("Rustville");
            t.displayname = s("Rustville");
            t.outpost_name = s("Rustville");
            t.orbit_name = s("Pyro I");
            t.planet_name = s("Pyro I");
            sys(t, "Pyro");
        });
        let mut kc = grimhex();
        kc.id = Some(341.0);
        kc.name = s("KC Trending - GrimHEX");
        kc.nickname = s("KC Trending GrimHEX");
        kc.displayname = s("GrimHEX");
        let mut admin = grimhex();
        admin.id = Some(27.0);
        admin.name = s("Admin - GrimHEX");
        admin.nickname = s("GrimHEX");
        let mut all = vec![
            personal.clone(),
            noodle,
            ae_hub.clone(),
            arc_l1.clone(),
            pyro_gw.clone(),
            rustville.clone(),
            kc,
            admin.clone(),
        ];
        all.extend(terminals());
        let loc = Locator::new(&all);
        let locs: Vec<ShopLocation> = all
            .iter()
            .map(|t| {
                loc.locate(&ShopPrice {
                    id_terminal: t.id,
                    ..Default::default()
                })
            })
            .collect();
        let hits = |q: &str| {
            let f = place_filter(&locs, q);
            let mut v: Vec<String> = locs
                .iter()
                .filter(|l| f(l))
                .map(|l| l.terminal.clone())
                .collect();
            v.sort();
            v
        };
        let label = |t: &UexTerminal| terminal_location(Some(t), None).label;
        assert_eq!(label(&personal), "Lorville, Hurston, Stanton");
        assert_eq!(label(&ae_hub), "Seraphim Station, Crusader, Stanton");
        assert_eq!(label(&rustville), "Rustville, Pyro I, Pyro");
        assert_eq!(label(&pyro_gw), "Pyro Gateway, Stanton");
        assert_eq!(label(&admin), "Grim HEX, Yela, Crusader, Stanton");
        assert_eq!(
            loc.locate(&ShopPrice {
                id_terminal: Some(341.0),
                ..Default::default()
            })
            .label,
            "Grim HEX, Yela, Crusader, Stanton"
        );
        assert_eq!(
            place_label("Platinum Pyro Gateway (Stanton)", "Pyro Gateway, Stanton"),
            "Platinum Pyro Gateway (Stanton)"
        );
        assert_eq!(
            hits("grim hex station"),
            [
                "Dumper's GrimHEX",
                "GrimHEX",
                "KC Trending GrimHEX",
                "Skutters Grim"
            ]
        );
        assert_eq!(hits("arc-l1"), ["Platinum ARC-L1"]);
        assert_eq!(
            hits("lorville"),
            ["Noodle Bar - Lorville", "Personal Weapons"]
        );
        assert_eq!(hits("pyro"), ["Rustville"]);
        assert!(!hits("stanton").contains(&"Rustville".to_string()));
        assert_eq!(hits("seraphim"), ["AE Hub"]);
        assert!(hits("l1").is_empty());
        assert!(hits("in").is_empty());
        assert!(!place_matches(
            &terminal_location(Some(&arc_l1), None),
            "nb"
        ));
        let terms = terminals();
        let short = shop_inventory(
            &ShopInputs {
                shops: &[shirt()],
                terminals: &terms,
                ..Default::default()
            },
            "nb",
        );
        assert!(render_shop(&short, None, None).contains("3+ letters"));
        assert_eq!(
            hits("ville"),
            ["Noodle Bar - Lorville", "Personal Weapons", "Rustville"]
        );
    }

    #[test]
    fn limits() {
        let many: Vec<UexTerminal> = (0..20)
            .map(|i| {
                term(1000.0 + i as f64, |t| {
                    t.nickname = Some(format!("Kiosk {i}"));
                    t.displayname = s("Big Place");
                    t.star_system_name = s("Stanton");
                })
            })
            .collect();
        let rows: Vec<ShopPrice> = many
            .iter()
            .flat_map(|t| {
                (0..30).map(|j| ShopPrice {
                    item_name: Some(format!("Item {j:02}")),
                    id_terminal: t.id,
                    price_buy: Some(10.0 + j as f64),
                    date_modified: Some(days_ago(1.0)),
                    ..Default::default()
                })
            })
            .collect();
        let inputs = |shops| ShopInputs {
            shops,
            terminals: &many,
            as_of: Some(AsOf::Millis(NOW)),
            ..Default::default()
        };
        let report = shop_inventory(&inputs(&rows[..30]), "big place");
        let cut = limit_shop(&report, Some(SHOP_ITEM_LIMIT), Some(SHOP_TERMINAL_LIMIT));
        assert_eq!(cut.terminals[0].items.len(), 25);
        assert_eq!(cut.terminals[0].item_count, Some(30));
        assert!(render_shop(&report, Some(25), Some(15)).contains("... 5 more"));
        assert_eq!(
            limit_shop(&report, None, Some(15)).terminals[0].items.len(),
            30
        );

        let report = shop_inventory(&inputs(&rows), "stanton");
        let cut = limit_shop(&report, Some(SHOP_ITEM_LIMIT), Some(SHOP_TERMINAL_LIMIT));
        assert_eq!(cut.list_only, Some(true));
        assert!(cut.terminals.is_empty());
        assert_eq!(
            (cut.terminal_count, cut.place_count, cut.rows),
            (Some(20), Some(1), 600)
        );
        assert_eq!(
            cut.places,
            Some(vec![ShopPlace {
                location: "Big Place, Stanton".into(),
                terminals: 20,
                items: 600
            }])
        );
        let text = render_shop(&report, Some(25), Some(15));
        assert!(text.contains("More than 15 terminals match"));
        assert!(text.len() < 1000);
    }
}
