//! Where a commodity sells best: UEX commodity kiosk prices (`uex-commodity-prices.json`,
//! aUEC per SCU) placed through the UEX terminals, matched to the game's resources (ours;
//! sc-gamedata has no port of it yet).
//!
//! UEX names a commodity by its own id and the in-game name, never by a game id, so a row
//! is matched to a `ResourceType` by name: the resource's display name first (`Borase
//! (Ore)`, `Quantainium`), then its record name (`Raw_Quantainium`), both compared lower
//! case with letters and digits only, and only when exactly one resource has that name.
//! Rows are ranked by sell price, highest first; a kiosk that pays nothing (no or zero
//! `price_sell`) and one UEX marks unavailable on LIVE are left out.

use super::quality::QualityResource;
use super::shops::{
    AsOf, Locator, STALE_DAYS, ShopPrice, UexTerminal, normalise_place, price_age, reference_time,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

/// One `prices` row of `uex-commodity-prices.json`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct CommodityPrice {
    #[serde(default)]
    pub commodity_name: Option<String>,
    #[serde(default)]
    pub id_commodity: Option<f64>,
    #[serde(default)]
    pub terminal_name: Option<String>,
    #[serde(default)]
    pub id_terminal: Option<f64>,
    /// aUEC per SCU the kiosk asks.
    #[serde(default)]
    pub price_buy: Option<f64>,
    /// aUEC per SCU the kiosk pays.
    #[serde(default)]
    pub price_sell: Option<f64>,
    /// SCU the kiosk holds.
    #[serde(default)]
    pub scu_sell_stock: Option<f64>,
    /// UEX's stock level, 1-7.
    #[serde(default)]
    pub status_sell: Option<f64>,
    /// Unix seconds: when UEX last had the row confirmed.
    #[serde(default)]
    pub date_modified: Option<f64>,
}

/// A kiosk that buys the commodity, placed.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SellRow {
    /// The terminal as UEX names it (`TDD Area 18`).
    pub terminal: String,
    /// Place, moon or orbit, planet, system.
    pub location: String,
    pub system: Option<String>,
    /// aUEC per SCU.
    pub price_sell: f64,
    pub scu_stock: Option<f64>,
    /// `date_modified` as YYYY-MM-DD (UTC).
    pub updated: Option<String>,
    /// Not confirmed for [`STALE_DAYS`].
    pub stale: bool,
}

/// How the price rows matched the resources.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct MatchStats {
    /// Distinct commodity names in the rows.
    pub commodities: usize,
    pub matched: usize,
    /// Names no single resource carries, sorted.
    pub unmatched: Vec<String>,
}

/// Lower case, letters and digits only (`Borase (Ore)` -> `boraseore`).
pub fn commodity_key(name: &str) -> String {
    normalise_place(name)
}

/// Resources by [`commodity_key`] of their display name and of their record name; a key
/// two resources share is kept out (ambiguous).
pub struct ResourceIndex<'a> {
    display: HashMap<String, Option<&'a QualityResource>>,
    name: HashMap<String, Option<&'a QualityResource>>,
}

impl<'a> ResourceIndex<'a> {
    pub fn new(resources: &'a [QualityResource]) -> Self {
        let mut display: HashMap<String, Option<&QualityResource>> = HashMap::new();
        let mut name: HashMap<String, Option<&QualityResource>> = HashMap::new();
        let add = |m: &mut HashMap<String, Option<&'a QualityResource>>,
                   k: String,
                   r: &'a QualityResource| {
            if k.is_empty() {
                return;
            }
            m.entry(k)
                .and_modify(|e| {
                    if e.is_some_and(|x| x.record_name != r.record_name) {
                        *e = None;
                    }
                })
                .or_insert(Some(r));
        };
        for r in resources {
            if let Some(d) = r.display_name.as_deref() {
                add(&mut display, commodity_key(d), r);
            }
            add(&mut name, commodity_key(&r.name), r);
        }
        Self { display, name }
    }

    /// The one resource a UEX commodity name means: display name, then record name.
    pub fn find(&self, commodity: &str) -> Option<&'a QualityResource> {
        let k = commodity_key(commodity);
        match self.display.get(&k) {
            Some(hit) => *hit,
            None => self.name.get(&k).copied().flatten(),
        }
    }
}

/// Price rows by matched resource (its `name`, lower case), in file order, and how the
/// names matched.
pub fn by_resource<'r>(
    rows: &'r [CommodityPrice],
    resources: &[QualityResource],
) -> (BTreeMap<String, Vec<&'r CommodityPrice>>, MatchStats) {
    let index = ResourceIndex::new(resources);
    let mut out: BTreeMap<String, Vec<&CommodityPrice>> = BTreeMap::new();
    let mut seen: BTreeMap<String, bool> = BTreeMap::new();
    for row in rows {
        let Some(n) = row.commodity_name.as_deref().map(str::trim) else {
            continue;
        };
        if n.is_empty() {
            continue;
        }
        let hit = index.find(n);
        seen.entry(n.to_string()).or_insert(hit.is_some());
        if let Some(r) = hit {
            out.entry(r.name.to_ascii_lowercase())
                .or_default()
                .push(row);
        }
    }
    let stats = MatchStats {
        commodities: seen.len(),
        matched: seen.values().filter(|m| **m).count(),
        unmatched: seen
            .iter()
            .filter(|(_, m)| !**m)
            .map(|(n, _)| n.clone())
            .collect(),
    };
    (out, stats)
}

/// Which side of a kiosk price: what it pays players (`price_sell`) or what it asks of them
/// (`price_buy`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Sell,
    Buy,
}

/// One kiosk price of a commodity, placed (either side).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KioskPrice {
    /// The terminal as UEX names it (`TDD Area 18`).
    pub terminal: String,
    /// Place, moon or orbit, planet, system.
    pub location: String,
    pub system: Option<String>,
    /// aUEC per SCU.
    pub price: f64,
    /// SCU the kiosk holds (sell side only, when UEX knows).
    pub scu_stock: Option<f64>,
    /// `date_modified` as YYYY-MM-DD (UTC).
    pub updated: Option<String>,
    /// Not confirmed for [`STALE_DAYS`].
    pub stale: bool,
}

/// The kiosks that buy it, highest price first ([`kiosk_prices`] on the sell side).
pub fn best_sells(
    rows: &[&CommodityPrice],
    terminals: &[UexTerminal],
    as_of: Option<&AsOf>,
) -> Vec<SellRow> {
    kiosk_prices(rows, terminals, as_of, Side::Sell)
        .into_iter()
        .map(|k| SellRow {
            terminal: k.terminal,
            location: k.location,
            system: k.system,
            price_sell: k.price,
            scu_stock: k.scu_stock,
            updated: k.updated,
            stale: k.stale,
        })
        .collect()
}

/// The kiosks on one side, best first: highest price for [`Side::Sell`], lowest for
/// [`Side::Buy`] (ties in file order), one row per terminal and place. Rows without a
/// positive, finite price on that side and kiosks UEX marks unavailable on LIVE are left
/// out. `as_of` is the file's `_fetched` (for the stale flag).
pub fn kiosk_prices(
    rows: &[&CommodityPrice],
    terminals: &[UexTerminal],
    as_of: Option<&AsOf>,
    side: Side,
) -> Vec<KioskPrice> {
    let locator = Locator::new(terminals);
    let reference = reference_time(as_of);
    let mut ranked: Vec<(&CommodityPrice, f64)> = rows
        .iter()
        .filter_map(|r| {
            let p = match side {
                Side::Sell => r.price_sell,
                Side::Buy => r.price_buy,
            };
            let p = p.filter(|p| p.is_finite() && *p > 0.0)?;
            Some((*r, p))
        })
        .collect();
    match side {
        Side::Sell => ranked.sort_by(|a, b| b.1.total_cmp(&a.1)),
        Side::Buy => ranked.sort_by(|a, b| a.1.total_cmp(&b.1)),
    }
    let mut out: Vec<KioskPrice> = Vec::new();
    for (row, price) in ranked {
        let loc = locator.locate(&ShopPrice {
            id_terminal: row.id_terminal,
            terminal_name: row.terminal_name.clone(),
            ..Default::default()
        });
        if loc.live == Some(false) {
            continue;
        }
        if out
            .iter()
            .any(|o| o.terminal == loc.terminal && o.location == loc.label)
        {
            continue;
        }
        let (updated, stale) = price_age(row.date_modified, reference, STALE_DAYS);
        out.push(KioskPrice {
            terminal: loc.terminal,
            location: loc.label,
            system: loc.system,
            price,
            scu_stock: match side {
                Side::Sell => row.scu_sell_stock,
                Side::Buy => None,
            },
            updated,
            stale,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn res(name: &str, display: Option<&str>) -> QualityResource {
        QualityResource {
            name: name.into(),
            record_name: format!("ResourceType.{name}"),
            display_name: display.map(str::to_string),
            quality: None,
        }
    }

    fn resources() -> Vec<QualityResource> {
        vec![
            res("Laranite", Some("Laranite")),
            res("Raw_Laranite", Some("Laranite (Raw)")),
            res("Borase_Ore", Some("Borase (Ore)")),
            res("Etam", Some("E'tam")),
            res("Twin_A", Some("Twin")),
            res("Twin_B", Some("Twin")),
            res("Hydrogen", None),
        ]
    }

    fn row(name: &str, terminal: f64, sell: Option<f64>) -> CommodityPrice {
        CommodityPrice {
            commodity_name: Some(name.into()),
            id_terminal: Some(terminal),
            terminal_name: Some(format!("Kiosk {terminal}")),
            price_sell: sell,
            date_modified: Some(1_795_227_950.0),
            ..Default::default()
        }
    }

    fn terminal(id: f64, nick: &str, station: &str, live: f64) -> UexTerminal {
        UexTerminal {
            id: Some(id),
            nickname: Some(nick.into()),
            space_station_name: Some(station.into()),
            star_system_name: Some("Stanton".into()),
            is_available: Some(1.0),
            is_available_live: Some(live),
            ..Default::default()
        }
    }

    #[test]
    fn names_match_display_then_record_name_and_never_twice() {
        let r = resources();
        let i = ResourceIndex::new(&r);
        let name = |q: &str| i.find(q).map(|r| r.name.as_str());
        assert_eq!(name("Laranite"), Some("Laranite"));
        assert_eq!(name("laranite (raw)"), Some("Raw_Laranite"));
        assert_eq!(name("Borase (Ore)"), Some("Borase_Ore"));
        assert_eq!(name("E'tam"), Some("Etam"));
        // No display name: the record name.
        assert_eq!(name("Hydrogen"), Some("Hydrogen"));
        // Two resources show as "Twin": neither is guessed.
        assert_eq!(name("Twin"), None);
        assert_eq!(name("Unobtainium"), None);
    }

    #[test]
    fn rows_group_by_resource_with_a_match_rate() {
        let r = resources();
        let rows = vec![
            row("Laranite", 1.0, Some(9000.0)),
            row("Laranite", 2.0, Some(8800.0)),
            row("Borase (Ore)", 1.0, Some(10.0)),
            row("Twin", 1.0, Some(5.0)),
            row("Unobtainium", 1.0, Some(5.0)),
        ];
        let (by, stats) = by_resource(&rows, &r);
        assert_eq!(by["laranite"].len(), 2);
        assert_eq!(by["borase_ore"].len(), 1);
        assert_eq!(
            stats,
            MatchStats {
                commodities: 4,
                matched: 2,
                unmatched: vec!["Twin".into(), "Unobtainium".into()],
            }
        );
    }

    #[test]
    fn ranking_is_highest_sell_first_without_zero_missing_or_off_live() {
        let t = vec![
            terminal(1.0, "TDD Area 18", "Area 18", 1.0),
            terminal(2.0, "HUR-L2", "HUR-L2 Faithful Dream Station", 1.0),
            terminal(3.0, "Old Kiosk", "Gone Station", 0.0),
            terminal(4.0, "Ruin Station", "Ruin Station", 1.0),
        ];
        let rows = [
            row("Laranite", 1.0, Some(9000.0)),
            row("Laranite", 2.0, Some(9600.0)),
            row("Laranite", 3.0, Some(99_999.0)),
            row("Laranite", 4.0, Some(0.0)),
            row("Laranite", 4.0, None),
            row("Laranite", 4.0, Some(f64::NAN)),
            row("Laranite", 1.0, Some(8000.0)),
        ];
        let refs: Vec<&CommodityPrice> = rows.iter().collect();
        let as_of = AsOf::Iso("2026-11-22T00:00:00.000Z".into());
        let s = best_sells(&refs, &t, Some(&as_of));
        let got: Vec<(&str, f64)> = s
            .iter()
            .map(|r| (r.terminal.as_str(), r.price_sell))
            .collect();
        assert_eq!(got, [("HUR-L2", 9600.0), ("TDD Area 18", 9000.0)]);
        assert_eq!(s[0].location, "HUR-L2 Faithful Dream Station, Stanton");
        assert_eq!(s[0].system.as_deref(), Some("Stanton"));
        assert_eq!(s[0].updated.as_deref(), Some("2026-11-21"));
        assert!(!s[0].stale);
        assert!(best_sells(&[], &t, None).is_empty());
    }

    #[test]
    fn buy_side_is_lowest_price_first_without_stock() {
        let t = vec![
            terminal(1.0, "TDD Area 18", "Area 18", 1.0),
            terminal(2.0, "HUR-L2", "HUR-L2 Faithful Dream Station", 1.0),
            terminal(3.0, "Old Kiosk", "Gone Station", 0.0),
        ];
        let mut rows = [
            row("Laranite", 1.0, Some(9000.0)),
            row("Laranite", 2.0, None),
            row("Laranite", 3.0, None),
            row("Laranite", 1.0, None),
        ];
        rows[0].price_buy = Some(31.0);
        rows[0].scu_sell_stock = Some(40.0);
        rows[1].price_buy = Some(27.5);
        rows[2].price_buy = Some(1.0);
        rows[3].price_buy = Some(0.0);
        let refs: Vec<&CommodityPrice> = rows.iter().collect();
        let b = kiosk_prices(&refs, &t, None, Side::Buy);
        let got: Vec<(&str, f64)> = b.iter().map(|r| (r.terminal.as_str(), r.price)).collect();
        assert_eq!(got, [("HUR-L2", 27.5), ("TDD Area 18", 31.0)]);
        assert!(b.iter().all(|r| r.scu_stock.is_none()));
        let s = kiosk_prices(&refs, &t, None, Side::Sell);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].scu_stock, Some(40.0));
    }

    #[test]
    fn real_rows_parse() {
        let v = serde_json::json!({
            "commodity_name": "Aluminum", "id_commodity": 5, "terminal_name": "TDD Area 18",
            "id_terminal": 12, "price_sell": 3800, "scu_sell_stock": 5, "status_sell": 1,
            "date_modified": 1795227950
        });
        let r: CommodityPrice = serde_json::from_value(v).unwrap();
        assert_eq!(r.price_sell, Some(3800.0));
        assert_eq!(r.id_terminal, Some(12.0));
        assert_eq!(r.price_buy, None);
        // Unknown and missing keys are fine.
        let r: CommodityPrice =
            serde_json::from_value(serde_json::json!({ "x": 1, "price_sell": null })).unwrap();
        assert_eq!(r, CommodityPrice::default());
    }
}
