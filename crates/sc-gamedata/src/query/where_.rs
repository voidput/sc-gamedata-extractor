//! "Where do I get X", one compact table (port of `query/where.ts`): every source of an
//! item from an [`ObtainReport`], one row each, easiest first.
//!
//! Effort order: buy (1) beats a contract, Wikelo reward or crafting blueprint (2), which
//! beats loot (3). Inside a tier: cheapest shop first, likeliest loot area first. Shop rows
//! are UEX data (player-reported), not game files; loot odds are the rough per-box
//! estimates of [`super::obtain`].

use super::en_us;
use super::obtain::{
    NamedItem, ObtainReport, SpawnKind, area_label, elsewhere_text, pct, rewarded_missions,
    weapons_label,
};
use super::shops::{LocatedShop, STALE_DAYS, place_label};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum WhereMethod {
    Buy,
    Contract,
    Wikelo,
    Craft,
    Loot,
}

impl WhereMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Buy => "buy",
            Self::Contract => "contract",
            Self::Wikelo => "wikelo",
            Self::Craft => "craft",
            Self::Loot => "loot",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WhereRow {
    pub method: WhereMethod,
    /// 1 buy, 2 reward or crafting, 3 loot.
    pub effort: u8,
    #[serde(rename = "where")]
    pub place: String,
    pub detail: String,
    /// buy: aUEC.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<f64>,
    /// buy: last confirmed, YYYY-MM-DD.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stale: Option<bool>,
    /// loot: rough odds per spawned box.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub odds: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WhereSummary {
    pub item: NamedItem,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ammo_for: Option<Vec<NamedItem>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shops_elsewhere: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cheapest_elsewhere: Option<WhereRow>,
    pub rows: Vec<WhereRow>,
    /// Lines worth saying that are not a source.
    pub notes: Vec<String>,
}

fn list(names: &[String], max: usize) -> String {
    let shown: Vec<&str> = names.iter().take(max).map(String::as_str).collect();
    let more = if names.len() > max {
        format!(" +{}", names.len() - max)
    } else {
        String::new()
    };
    format!("{}{more}", shown.join(", "))
}

fn buy_row(s: &LocatedShop, note: &str) -> WhereRow {
    WhereRow {
        method: WhereMethod::Buy,
        effort: 1,
        place: place_label(&s.terminal, &s.location),
        detail: format!(
            "{} aUEC, {}{}{}",
            en_us(s.row.price_buy.unwrap_or(0.0)),
            s.updated.as_deref().unwrap_or("undated"),
            if s.stale { " stale" } else { "" },
            if note.is_empty() {
                String::new()
            } else {
                format!("; {note}")
            }
        ),
        price: s.row.price_buy,
        updated: s.updated.clone(),
        stale: Some(s.stale),
        odds: None,
    }
}

fn truthy(x: Option<f64>) -> bool {
    x.is_some_and(|v| v != 0.0 && !v.is_nan())
}

/// `whereSummary(report, { shopsLoaded })`.
pub fn where_summary(report: &ObtainReport, shops_loaded: Option<bool>) -> WhereSummary {
    let mut rows: Vec<WhereRow> = Vec::new();
    let mut notes: Vec<String> = Vec::new();

    let mut buyable: Vec<&LocatedShop> = report
        .shops
        .iter()
        .filter(|s| truthy(s.row.price_buy))
        .collect();
    buyable.sort_by(|a, b| {
        a.row
            .price_buy
            .unwrap_or(0.0)
            .partial_cmp(&b.row.price_buy.unwrap_or(0.0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    if let Some(first) = buyable.first() {
        let at = report
            .at
            .as_deref()
            .map(|a| format!(" at \"{a}\""))
            .unwrap_or_default();
        let count = if buyable.len() > 1 {
            format!("cheapest of {} shops{at}", buyable.len())
        } else {
            format!("only shop{at}")
        };
        rows.push(buy_row(first, &count));
        if first.stale
            && let Some(fresh) = buyable.iter().find(|s| !s.stale)
        {
            rows.push(buy_row(
                fresh,
                &format!("cheapest confirmed in {STALE_DAYS} days"),
            ));
        }
    } else if shops_loaded == Some(false) {
        notes.push("no shop data loaded".into());
    } else if let Some(at) = &report.at {
        notes.push(format!(
            "not sold at \"{at}\" as far as UEX knows{}",
            elsewhere_text(report.shops_elsewhere, report.cheapest_elsewhere.as_ref())
        ));
    }

    let mut contracts: Vec<String> = Vec::new();
    for a in &report.contract_awards {
        if !contracts.contains(&a.contract) {
            contracts.push(a.contract.clone());
        }
    }
    if !contracts.is_empty() {
        rows.push(WhereRow {
            method: WhereMethod::Contract,
            effort: 2,
            place: list(&contracts, 3),
            detail: format!(
                "contract award{}",
                if contracts.len() > 1 {
                    format!(", {} contracts", contracts.len())
                } else {
                    String::new()
                }
            ),
            price: None,
            updated: None,
            stale: None,
            odds: None,
        });
    } else if !report.wikelo_inferred.is_empty() {
        let names: Vec<String> = report
            .wikelo_inferred
            .iter()
            .map(|t| t.name.clone())
            .collect();
        rows.push(WhereRow {
            method: WhereMethod::Wikelo,
            effort: 2,
            place: list(&names, 3),
            detail: "Wikelo trade, reward inferred from its name".into(),
            price: None,
            updated: None,
            stale: None,
            odds: None,
        });
    }

    for b in &report.blueprints {
        let missions = rewarded_missions(b);
        rows.push(WhereRow {
            method: WhereMethod::Craft,
            effort: 2,
            place: format!("blueprint {}", b.display_name.as_deref().unwrap_or(&b.name)),
            detail: if missions.is_empty() {
                "no mission in the data awards the blueprint".into()
            } else {
                format!("blueprint from {}", list(&missions, 3))
            },
            price: None,
            updated: None,
            stale: None,
            odds: None,
        });
    }

    for (i, a) in report.areas.iter().take(2).enumerate() {
        let best = &a.best;
        let entity = best.entity_name.as_deref().unwrap_or(&best.entity);
        let bx = match best.kind {
            SpawnKind::Loot => entity.to_string(),
            SpawnKind::Fixed => format!("fixed: {entity}"),
            SpawnKind::Loose => format!("loose: {entity}"),
        };
        rows.push(WhereRow {
            method: WhereMethod::Loot,
            effort: 3,
            place: area_label(&a.area),
            detail: format!(
                "{bx}, {} per box{}",
                pct(best.score),
                if i == 0 && report.areas.len() > 1 {
                    format!("; best of {} areas", report.areas.len())
                } else {
                    String::new()
                }
            ),
            price: None,
            updated: None,
            stale: None,
            odds: Some(best.score),
        });
    }
    if report.areas.is_empty()
        && let Some(t) = report.other_tables.first()
    {
        rows.push(WhereRow {
            method: WhereMethod::Loot,
            effort: 3,
            place: format!("table {}", t.table),
            detail: format!(
                "{} per roll; not placed by an area preset (corpses, events){}",
                pct(t.per_roll),
                if report.other_tables.len() > 1 {
                    format!("; +{} tables", report.other_tables.len() - 1)
                } else {
                    String::new()
                }
            ),
            price: None,
            updated: None,
            stale: None,
            odds: Some(t.per_roll),
        });
    }

    rows.sort_by_key(|r| r.effort);
    let at = report.at.clone();
    WhereSummary {
        item: NamedItem {
            class_name: report.item.class_name.clone(),
            name: report.item.name.clone(),
        },
        ammo_for: report.ammo_for.clone(),
        shops_elsewhere: at.as_ref().and(report.shops_elsewhere).filter(|n| *n > 0),
        cheapest_elsewhere: at
            .as_ref()
            .and(report.cheapest_elsewhere.as_ref())
            .map(|c| buy_row(c, "cheapest elsewhere")),
        at,
        rows,
        notes,
    }
}

/// Best (lowest) effort, then price. No source at the place but shops elsewhere ranks 3.5;
/// no source at all, 4.
pub fn summary_rank(s: &WhereSummary) -> (f64, f64) {
    if let Some(top) = s.rows.first() {
        return (top.effort as f64, top.price.unwrap_or(f64::INFINITY));
    }
    if s.shops_elsewhere.is_some_and(|n| n > 0) {
        (
            3.5,
            s.cheapest_elsewhere
                .as_ref()
                .and_then(|c| c.price)
                .unwrap_or(f64::INFINITY),
        )
    } else {
        (4.0, f64::INFINITY)
    }
}

/// Indexes of `list`, easiest first. Stable.
pub fn order_by_effort(list: &[WhereSummary]) -> Vec<usize> {
    let ranks: Vec<(f64, f64)> = list.iter().map(summary_rank).collect();
    let mut ix: Vec<usize> = (0..list.len()).collect();
    ix.sort_by(|&a, &b| {
        let d = ranks[a].0 - ranks[b].0;
        let d = if d != 0.0 && !d.is_nan() {
            d
        } else {
            let p = ranks[a].1 - ranks[b].1;
            if p.is_nan() { 0.0 } else { p }
        };
        d.partial_cmp(&0.0).unwrap_or(std::cmp::Ordering::Equal)
    });
    ix
}

/// Ammo summaries worth showing: sourceless twins dropped when another one has a source.
pub fn pick_ammo(list: &[WhereSummary]) -> (Vec<usize>, Vec<usize>) {
    let order = order_by_effort(list);
    let sourced: Vec<usize> = order
        .iter()
        .copied()
        .filter(|&i| summary_rank(&list[i]).0 < 4.0)
        .collect();
    if sourced.is_empty() {
        return (order, Vec::new());
    }
    let dropped = order
        .into_iter()
        .filter(|&i| summary_rank(&list[i]).0 >= 4.0)
        .collect();
    (sourced, dropped)
}

pub fn render_where(s: &WhereSummary) -> String {
    let ammo = match s.ammo_for.as_ref().filter(|a| !a.is_empty()) {
        Some(a) => {
            let names: Vec<&str> = a.iter().map(|w| w.name.as_str()).collect();
            format!("ammo for {}: ", weapons_label(&names))
        }
        None => String::new(),
    };
    let at =
        s.at.as_deref()
            .map(|a| format!("  at \"{a}\""))
            .unwrap_or_default();
    let mut lines = vec![format!(
        "{ammo}{}  ({}){at}",
        s.item.name, s.item.class_name
    )];
    let elsewhere = s.shops_elsewhere.filter(|n| *n > 0);
    if s.rows.is_empty() && s.at.is_some() && elsewhere.is_some() {
        lines.push(format!(
            "  no source at \"{}\"; sold at {} shop row(s) elsewhere",
            s.at.as_deref().unwrap_or(""),
            elsewhere.unwrap_or(0)
        ));
        if let Some(c) = &s.cheapest_elsewhere {
            lines.push(format!(
                "  {} {}  {}",
                super::shops::pad_end("buy", 8),
                c.place,
                c.detail
            ));
        }
    } else if s.rows.is_empty() {
        lines.push(
            "  no known source: not sold (UEX), awarded, craftable or rolled by any loot table"
                .into(),
        );
    } else {
        let width = s
            .rows
            .iter()
            .map(|r| super::js_len(&r.place))
            .max()
            .unwrap_or(0)
            .min(56);
        for r in &s.rows {
            let place = if super::js_len(&r.place) > width {
                format!(
                    "{}…",
                    super::shops::slice_chars(&r.place, width.saturating_sub(1))
                )
            } else {
                super::shops::pad_end(&r.place, width)
            };
            lines.push(format!(
                "  {} {place}  {}",
                super::shops::pad_end(r.method.as_str(), 8),
                r.detail
            ));
        }
    }
    let notes: Vec<&String> = if s.rows.is_empty() && elsewhere.is_some() {
        s.notes
            .iter()
            .filter(|n| !n.starts_with("not sold at"))
            .collect()
    } else {
        s.notes.iter().collect()
    };
    for n in notes {
        lines.push(format!("  note: {n}"));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::obtain::{
        AreaSummary, BlueprintLike, Datasets, ObtainOptions, RewardRef, RollPath, SpawnHit, obtain,
    };
    use crate::query::shops::{AsOf, ShopPrice, UexTerminal};
    use crate::query::types::{ContractAward, ItemTags};

    fn gun() -> ItemTags {
        ItemTags {
            name: "FS-9 LMG".into(),
            class_name: "behr_lmg_ballistic_01".into(),
            id: "id-fs9".into(),
            ..Default::default()
        }
    }

    fn base() -> ObtainReport {
        let d = Datasets {
            items: vec![gun()],
            ..Default::default()
        };
        obtain(&d, &gun(), None, &ObtainOptions::default())
    }

    fn day(iso: &str) -> f64 {
        crate::query::parse_iso_millis(iso).unwrap() as f64 / 1000.0
    }

    fn shop(terminal: &str, location: &str, price: f64, stale: bool) -> LocatedShop {
        LocatedShop {
            row: ShopPrice {
                price_buy: Some(price),
                ..Default::default()
            },
            terminal: terminal.into(),
            location: location.into(),
            system: None,
            live: None,
            updated: None,
            stale,
        }
    }

    #[test]
    fn buying_first_then_rewards_and_crafting_then_loot() {
        let row = |id: f64, t: &str, buy: f64, d: &str| ShopPrice {
            item_uuid: Some("id-fs9".into()),
            id_terminal: Some(id),
            terminal_name: Some(t.into()),
            price_buy: Some(buy),
            date_modified: Some(day(d)),
            ..Default::default()
        };
        let d = Datasets {
            items: vec![gun()],
            shops: Some(vec![
                row(1.0, "Old Cheap", 5000.0, "2026-08-01"),
                row(2.0, "Fresh", 6000.0, "2026-11-10"),
                row(3.0, "Dear", 9000.0, "2026-11-12"),
            ]),
            shops_as_of: Some(AsOf::Iso("2026-11-14T00:00:00Z".into())),
            blueprints: Some(vec![BlueprintLike {
                name: "BP_FS9".into(),
                display_name: Some("FS-9 LMG".into()),
                produces: Some("behr_lmg_ballistic_01".into()),
                rewarded_by: Some(vec![RewardRef {
                    mission: Some("Bounty".into()),
                    ..Default::default()
                }]),
                ..Default::default()
            }]),
            contract_awards: Some(vec![ContractAward {
                generator: "G".into(),
                contract: "TheCollector_FS9".into(),
                item: "behr_lmg_ballistic_01".into(),
                ..Default::default()
            }]),
            ..Default::default()
        };
        let mut report = obtain(&d, &gun(), None, &ObtainOptions::default());
        report.areas = vec![AreaSummary {
            area: "caves/loot_caves".into(),
            configs: 1,
            best: SpawnHit {
                area: "caves/loot_caves".into(),
                config: None,
                spawn_tags: vec![],
                entity: "crate".into(),
                entity_name: Some("Weapons Crate".into()),
                slot_share: 1.0,
                kind: SpawnKind::Loot,
                table: None,
                per_roll: None,
                max_results: None,
                score: 0.05,
                paths: None,
            },
        }];
        let s = where_summary(&report, Some(true));
        let methods: Vec<&str> = s.rows.iter().map(|r| r.method.as_str()).collect();
        assert_eq!(methods, ["buy", "buy", "contract", "craft", "loot"]);
        assert_eq!(
            (s.rows[0].place.as_str(), s.rows[0].price, s.rows[0].stale),
            ("Old Cheap", Some(5000.0), Some(true))
        );
        assert!(s.rows[0].detail.contains("cheapest of 3 shops"));
        assert_eq!(
            (s.rows[1].place.as_str(), s.rows[1].price, s.rows[1].stale),
            ("Fresh", Some(6000.0), Some(false))
        );
        assert_eq!(s.rows[3].detail, "blueprint from Bounty");
        assert_eq!(
            (s.rows[4].place.as_str(), s.rows[4].odds),
            ("caves: loot caves", Some(0.05))
        );
        let text = render_where(&s);
        assert_eq!(
            text.lines().next().unwrap(),
            "FS-9 LMG  (behr_lmg_ballistic_01)"
        );
        assert!(text.lines().any(|l| l.starts_with("  buy      Old Cheap ")
            && l.ends_with(" 5,000 aUEC, 2026-08-01 stale; cheapest of 3 shops")));
    }

    #[test]
    fn honours_a_place_and_says_when_sold_only_elsewhere() {
        let d = Datasets {
            items: vec![gun()],
            shops: Some(vec![ShopPrice {
                item_uuid: Some("id-fs9".into()),
                id_terminal: Some(1.0),
                terminal_name: Some("Guns Ruin".into()),
                price_buy: Some(7000.0),
                ..Default::default()
            }]),
            terminals: Some(vec![UexTerminal {
                id: Some(1.0),
                nickname: Some("Guns Ruin".into()),
                displayname: Some("Ruin Station".into()),
                star_system_name: Some("Pyro".into()),
                ..Default::default()
            }]),
            ..Default::default()
        };
        let at = |p| {
            where_summary(
                &obtain(
                    &d,
                    &gun(),
                    None,
                    &ObtainOptions {
                        at: Some(p),
                        ..Default::default()
                    },
                ),
                Some(true),
            )
        };
        assert!(at("ruin").rows[0].detail.contains("only shop at \"ruin\""));
        let elsewhere = at("grimhex");
        assert!(elsewhere.rows.is_empty());
        assert_eq!(
            elsewhere.notes,
            [
                "not sold at \"grimhex\" as far as UEX knows; 1 shop row(s) elsewhere, cheapest 7,000 aUEC at Guns Ruin (Ruin Station, Pyro) (undated stale)"
            ]
        );
        let text = render_where(&elsewhere);
        assert!(!text.contains("no known source"));
        assert!(text.contains("no source at \"grimhex\"; sold at 1 shop row(s) elsewhere"));
        assert!(text.contains(
            "buy      Guns Ruin (Ruin Station, Pyro)  7,000 aUEC, undated stale; cheapest elsewhere"
        ));
        assert!(!text.contains("note: not sold"));
        assert_eq!(summary_rank(&elsewhere), (3.5, 7000.0));
        assert_eq!(
            summary_rank(&where_summary(&base(), None)),
            (4.0, f64::INFINITY)
        );
    }

    #[test]
    fn labels_ammo_and_orders_the_buyable_twin_first() {
        let mut r = base();
        r.shops = vec![shop("Dumper's GrimHEX", "Grim HEX", 275.0, true)];
        let sold = where_summary(&r, None);
        let unsold = where_summary(&base(), None);
        let mut r = base();
        r.other_tables = vec![RollPath {
            table: "T".into(),
            archetype: "A".into(),
            entry: None,
            share: 0.1,
            pool_size: None,
            per_roll: 0.1,
        }];
        let loot = where_summary(&r, None);
        assert_eq!(
            order_by_effort(&[unsold.clone(), loot, sold.clone()]),
            [2, 1, 0]
        );
        let mut r = base();
        r.ammo_for = Some(vec![NamedItem {
            class_name: "grin_salvage_repair_01".into(),
            name: "Cambio SRT".into(),
        }]);
        assert_eq!(
            render_where(&where_summary(&r, None))
                .lines()
                .next()
                .unwrap(),
            "ammo for Cambio SRT: FS-9 LMG  (behr_lmg_ballistic_01)"
        );
        let mut r = base();
        r.shops = vec![shop("Guns", "Lorville", 50.0, false)];
        let sold = where_summary(&r, None);
        let creature = where_summary(&base(), None);
        assert_eq!(
            pick_ammo(&[creature.clone(), sold, creature.clone()]),
            (vec![1], vec![0, 2])
        );
        assert_eq!(
            pick_ammo(&[creature.clone(), creature]),
            (vec![0, 1], vec![])
        );
        assert_eq!(
            where_summary(&base(), Some(false)).notes,
            ["no shop data loaded"]
        );
    }
}
