//! The commodity browser (ours): every tradable resource of `game-commodities`, grouped by
//! its resource group (`Metal > Unrefined Ores`), as short summaries for a list, and one
//! resource in full on request: refined form and raw source, quality, volatility and cargo
//! damage, the cargo items holding it, where it is mined ([`super::mined`]) and the UEX
//! commodity kiosks that buy and sell it ([`super::sells`]).
//!
//! The list carries no detail, so a caller can answer it cheaply and build a detail only
//! for the row a user picks. Kiosk prices are UEX data (player-reported), in no game file.

use super::mined::MinedReport;
use super::quality::{OreQuality, QualityIn, QualityResource, report};
use super::sells::{CommodityPrice, KioskPrice, Side, by_resource, kiosk_prices};
use super::shops::{AsOf, UexTerminal};
use crate::collate::compare_names;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::HashMap;

/// Kiosk rows a detail lists per side (the rest are counted).
pub const MAX_KIOSKS: usize = 12;

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct VolatilityIn {
    pub volatility: Option<f64>,
    pub health_decay_per_second: Option<f64>,
}

/// One `game-commodities` `resources` row, as far as the browser reads it.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ResourceRow {
    pub name: String,
    pub record_name: String,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub group: String,
    pub subgroups: Vec<String>,
    pub density_grams_per_cc: Option<f64>,
    /// The resource name it refines into (`Quantanium`).
    pub refines_to: Option<String>,
    pub volatility: Option<VolatilityIn>,
    pub quality: Option<QualityIn>,
}

/// One `game-commodities` `items` row (a cargo entity).
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CargoItemRow {
    pub name: String,
    pub record_name: String,
    pub display_name: Option<String>,
    #[serde(rename = "type")]
    pub ty: Option<String>,
    pub resource: Option<String>,
    pub capacity_scu: Option<f64>,
}

/// One `game-commodities` `damage` row (a `CommodityDamageConfiguration`).
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DamageRow {
    pub name: String,
    pub minimum_speed: Option<f64>,
    pub speed_squared_to_damage: Option<f64>,
    pub grace_period: Option<f64>,
    pub explosion_damage: Option<f64>,
    pub explosion_min_radius: Option<f64>,
    pub explosion_max_radius: Option<f64>,
    pub explosion_pressure: Option<f64>,
}

/// The parts of `game-commodities` the browser reads.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(default)]
pub struct CommodityData {
    pub resources: Vec<ResourceRow>,
    pub items: Vec<CargoItemRow>,
    pub damage: Vec<DamageRow>,
}

/// Whether the browser could read the build's commodities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CommodityDataState {
    Ready,
    /// The channel's game data was not read yet (`game-commodities` is not cached).
    NotRead,
    /// No channel to read (no install known).
    NoChannel,
}

/// One list row: small on purpose, the detail is a separate call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommoditySummary {
    /// The resource record (`ResourceType.Raw_Quantainium`): what the detail call takes.
    pub id: String,
    /// Display name, else the resource name.
    pub name: String,
    /// The top group, readable (`Metal`, `Processed Goods`).
    pub group: String,
    /// The groups below it, readable (`Unrefined Ores`); `None` at the top level.
    pub subgroup: Option<String>,
    /// Volatile cargo (it decays and can explode).
    pub volatile: bool,
    /// A raw form: it refines into something.
    pub raw: bool,
}

/// Whether the UEX commodity files were there, and when they were fetched.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct MarketMeta {
    /// `uex-commodity-prices.json` and `uex-terminals.json` were both readable.
    pub loaded: bool,
    /// `_fetched` of the prices file (ISO 8601).
    pub fetched: Option<String>,
    /// Where the UEX files go.
    pub folder: Option<String>,
}

/// `commodity_list`: every tradable resource, by group, then name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommodityDirectory {
    pub state: CommodityDataState,
    /// The build the data is of (`LIVE 4.10.201.48085`).
    pub build: Option<String>,
    pub commodities: Vec<CommoditySummary>,
    pub market: MarketMeta,
}

/// Another resource a detail names (refined form, raw source).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommodityRef {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommodityVolatility {
    /// The game's volatility factor.
    pub volatility: Option<f64>,
    /// Health lost per second while carried.
    pub decay_per_second: Option<f64>,
}

/// Cargo damage settings; configurations with the same numbers are one row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CargoDamage {
    /// `CommodityDamageConfigBase`, ...
    pub configs: Vec<String>,
    /// Impacts below this speed (m/s) do no damage.
    pub minimum_speed: Option<f64>,
    /// Seconds after loading before damage counts.
    pub grace_period: Option<f64>,
    pub explosion_damage: Option<f64>,
    pub explosion_min_radius: Option<f64>,
    pub explosion_max_radius: Option<f64>,
}

/// A cargo item holding the resource.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CargoItem {
    /// Display name, else the class.
    pub name: String,
    /// The entity class (`agricium_ore`).
    pub class_name: String,
    /// The game's type label (`Metal`).
    pub kind: Option<String>,
    pub capacity_scu: Option<f64>,
}

/// Kiosk prices of one resource. UEX (player-reported), aUEC per SCU.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct CommodityMarket {
    pub loaded: bool,
    pub fetched: Option<String>,
    pub folder: Option<String>,
    /// Kiosks paying for it, highest first, at most [`MAX_KIOSKS`].
    pub sell: Vec<KioskPrice>,
    pub sell_total: usize,
    /// Kiosks selling it, lowest first, at most [`MAX_KIOSKS`].
    pub buy: Vec<KioskPrice>,
    pub buy_total: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommodityDetail {
    pub id: String,
    pub name: String,
    pub group: String,
    pub subgroup: Option<String>,
    pub description: Option<String>,
    /// Grams per cm³; `None` when the game leaves the default 1 (most non-ores), which says
    /// nothing about the resource.
    pub density: Option<f64>,
    /// What it refines into.
    pub refined: Option<CommodityRef>,
    /// What refines into it.
    pub raw_sources: Vec<CommodityRef>,
    pub quality: Option<OreQuality>,
    pub volatility: Option<CommodityVolatility>,
    /// Only for volatile resources: what knocking the cargo about does.
    pub damage: Vec<CargoDamage>,
    pub cargo: Vec<CargoItem>,
    /// Where it (a raw form) is mined; `None` when the mining data is not cached or the
    /// resource is a refined form (see `raw_sources`), empty when it is mined nowhere known.
    pub mined: Option<Vec<MinedReport>>,
    pub market: CommodityMarket,
}

/// `commodity_detail`: one resource, `None` when the id names none.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommodityInfo {
    pub state: CommodityDataState,
    pub commodity: Option<CommodityDetail>,
}

/// `UnrefinedOres` -> `Unrefined Ores`, `Raw_Minerals` -> `Raw Minerals`.
pub fn group_label(key: &str) -> String {
    let mut out = String::new();
    let mut prev: Option<char> = None;
    for c in key.chars() {
        if c == '_' {
            if !out.ends_with(' ') && !out.is_empty() {
                out.push(' ');
            }
        } else {
            if c.is_uppercase() && prev.is_some_and(|p| p.is_lowercase()) {
                out.push(' ');
            }
            out.push(c);
        }
        prev = Some(c);
    }
    out.trim().to_string()
}

/// Placeholder groups (`HeatPlaceholder`, `PowerPlaceholder`) are ship systems, not trade.
pub fn is_tradable(r: &ResourceRow) -> bool {
    !r.group.to_ascii_lowercase().ends_with("placeholder")
}

/// The readable top group; `Other` when the dataset names none.
fn top_group(r: &ResourceRow) -> String {
    let g = group_label(&r.group);
    if g.is_empty() { "Other".into() } else { g }
}

fn name_of(r: &ResourceRow) -> String {
    r.display_name
        .clone()
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| r.name.clone())
}

fn subgroup_of(r: &ResourceRow) -> Option<String> {
    let s: Vec<String> = r.subgroups.iter().map(|g| group_label(g)).collect();
    (!s.is_empty()).then(|| s.join(" > "))
}

fn by_group_then_name(a: &CommoditySummary, b: &CommoditySummary) -> Ordering {
    compare_names(&a.group, &b.group)
        .then_with(|| match (&a.subgroup, &b.subgroup) {
            (None, None) => Ordering::Equal,
            (None, Some(_)) => Ordering::Less,
            (Some(_), None) => Ordering::Greater,
            (Some(x), Some(y)) => compare_names(x, y),
        })
        .then_with(|| compare_names(&a.name, &b.name))
}

/// Every tradable resource as a list row, by group, subgroup (top level first), then name.
pub fn commodity_list(data: &CommodityData) -> Vec<CommoditySummary> {
    let mut out: Vec<CommoditySummary> = data
        .resources
        .iter()
        .filter(|r| is_tradable(r))
        .map(|r| CommoditySummary {
            id: r.record_name.clone(),
            name: name_of(r),
            group: top_group(r),
            subgroup: subgroup_of(r),
            volatile: r
                .volatility
                .as_ref()
                .is_some_and(|v| v.volatility.is_some_and(|x| x > 0.0)),
            raw: r.refines_to.is_some(),
        })
        .collect();
    out.sort_by(by_group_then_name);
    out
}

/// The resource an id names: its record name (`ResourceType.Gold`) or its name (`Gold`).
pub fn find_resource<'a>(data: &'a CommodityData, id: &str) -> Option<&'a ResourceRow> {
    let id = id.trim();
    let short = id.rsplit('.').next().unwrap_or(id);
    data.resources
        .iter()
        .find(|r| r.record_name.eq_ignore_ascii_case(id))
        .or_else(|| {
            data.resources
                .iter()
                .find(|r| r.name.eq_ignore_ascii_case(short))
        })
}

fn as_ref(r: &ResourceRow) -> CommodityRef {
    CommodityRef {
        id: r.record_name.clone(),
        name: name_of(r),
    }
}

/// The resources that refine into `r`.
pub fn raw_sources(data: &CommodityData, r: &ResourceRow) -> Vec<CommodityRef> {
    let mut v: Vec<CommodityRef> = data
        .resources
        .iter()
        .filter(|x| {
            x.refines_to
                .as_deref()
                .is_some_and(|t| t.eq_ignore_ascii_case(&r.name))
        })
        .map(as_ref)
        .collect();
    v.sort_by(|a, b| compare_names(&a.name, &b.name));
    v
}

fn damage_rows(rows: &[DamageRow]) -> Vec<CargoDamage> {
    let mut out: Vec<CargoDamage> = Vec::new();
    for d in rows {
        let row = CargoDamage {
            configs: vec![d.name.clone()],
            minimum_speed: d.minimum_speed,
            grace_period: d.grace_period,
            explosion_damage: d.explosion_damage,
            explosion_min_radius: d.explosion_min_radius,
            explosion_max_radius: d.explosion_max_radius,
        };
        let same = |o: &CargoDamage| {
            CargoDamage {
                configs: Vec::new(),
                ..o.clone()
            } == CargoDamage {
                configs: Vec::new(),
                ..row.clone()
            }
        };
        match out.iter_mut().find(|o| same(o)) {
            Some(o) => o.configs.push(d.name.clone()),
            None => out.push(row),
        }
    }
    out
}

/// One resource in full. `mined` is asked with the resource's record name, only for a
/// resource no other refines into (a refined form is not mined itself); `market` is its
/// kiosk prices. `None` when `id` names no resource.
pub fn commodity_detail(
    data: &CommodityData,
    id: &str,
    mined: impl FnOnce(&str) -> Option<Vec<MinedReport>>,
    market: impl FnOnce(&ResourceRow) -> CommodityMarket,
) -> Option<CommodityDetail> {
    let r = find_resource(data, id)?;
    let refined = r.refines_to.as_deref().map(|t| {
        data.resources
            .iter()
            .find(|x| x.name.eq_ignore_ascii_case(t))
            .map(as_ref)
            .unwrap_or_else(|| CommodityRef {
                id: format!("ResourceType.{t}"),
                name: group_label(t),
            })
    });
    let raw = raw_sources(data, r);
    let volatility = r
        .volatility
        .as_ref()
        .filter(|v| v.volatility.is_some() || v.health_decay_per_second.is_some())
        .map(|v| CommodityVolatility {
            volatility: v.volatility,
            decay_per_second: v.health_decay_per_second,
        });
    let volatile = volatility
        .as_ref()
        .is_some_and(|v| v.volatility.is_some_and(|x| x > 0.0));
    let quality = r.quality.as_ref().map(|q| {
        report(
            &QualityResource {
                name: r.name.clone(),
                record_name: r.record_name.clone(),
                display_name: r.display_name.clone(),
                quality: Some(q.clone()),
            },
            q,
        )
    });
    let mut cargo: Vec<CargoItem> = data
        .items
        .iter()
        .filter(|i| {
            i.resource
                .as_deref()
                .is_some_and(|x| x.eq_ignore_ascii_case(&r.name))
        })
        .map(|i| CargoItem {
            name: i.display_name.clone().unwrap_or_else(|| i.name.clone()),
            class_name: i.name.clone(),
            kind: i.ty.clone(),
            capacity_scu: i.capacity_scu,
        })
        .collect();
    cargo.sort_by(|a, b| compare_names(&a.name, &b.name));
    let mined = if raw.is_empty() {
        mined(&r.record_name)
    } else {
        None
    };
    Some(CommodityDetail {
        id: r.record_name.clone(),
        name: name_of(r),
        group: top_group(r),
        subgroup: subgroup_of(r),
        description: r.description.clone().filter(|d| !d.trim().is_empty()),
        density: r.density_grams_per_cc.filter(|d| *d != 1.0),
        refined,
        raw_sources: raw,
        quality,
        volatility,
        damage: if volatile {
            damage_rows(&data.damage)
        } else {
            Vec::new()
        },
        cargo,
        mined,
        market: market(r),
    })
}

/// Kiosk prices of every resource, built once per set of UEX files and resources.
#[derive(Debug, Clone, Default)]
pub struct Kiosks {
    /// By resource name, lower case: (sell side, buy side), best first.
    by: HashMap<String, (Vec<KioskPrice>, Vec<KioskPrice>)>,
    pub meta: MarketMeta,
}

impl Kiosks {
    /// Rows matched to `resources` by name ([`by_resource`]) and ranked on both sides.
    pub fn new(
        rows: &[CommodityPrice],
        terminals: &[UexTerminal],
        resources: &[QualityResource],
        meta: MarketMeta,
    ) -> Self {
        let as_of = meta.fetched.clone().map(AsOf::Iso);
        let (grouped, _) = by_resource(rows, resources);
        let by = grouped
            .into_iter()
            .map(|(k, rows)| {
                let sell = kiosk_prices(&rows, terminals, as_of.as_ref(), Side::Sell);
                let buy = kiosk_prices(&rows, terminals, as_of.as_ref(), Side::Buy);
                (k, (sell, buy))
            })
            .collect();
        Self { by, meta }
    }

    /// No UEX files: nothing loaded.
    pub fn missing(folder: Option<String>) -> Self {
        Self {
            by: HashMap::new(),
            meta: MarketMeta {
                loaded: false,
                fetched: None,
                folder,
            },
        }
    }

    /// One resource's prices, capped at [`MAX_KIOSKS`] per side.
    pub fn market(&self, resource_name: &str) -> CommodityMarket {
        let (sell, buy) = self
            .by
            .get(&resource_name.to_ascii_lowercase())
            .cloned()
            .unwrap_or_default();
        let (sell_total, buy_total) = (sell.len(), buy.len());
        CommodityMarket {
            loaded: self.meta.loaded,
            fetched: self.meta.fetched.clone(),
            folder: self.meta.folder.clone(),
            sell: sell.into_iter().take(MAX_KIOSKS).collect(),
            sell_total,
            buy: buy.into_iter().take(MAX_KIOSKS).collect(),
            buy_total,
        }
    }
}

/// The resources as the price matcher reads them.
pub fn quality_resources(data: &CommodityData) -> Vec<QualityResource> {
    data.resources
        .iter()
        .map(|r| QualityResource {
            name: r.name.clone(),
            record_name: r.record_name.clone(),
            display_name: r.display_name.clone(),
            quality: None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn data() -> CommodityData {
        serde_json::from_value(json!({
            "resources": [
                {"name": "Gold", "recordName": "ResourceType.Gold", "displayName": "Gold",
                 "group": "Metal", "subgroups": [], "densityGramsPerCc": 19.3},
                {"name": "Ore_Gold", "recordName": "ResourceType.Ore_Gold", "displayName": "Gold (Ore)",
                 "group": "Metal", "subgroups": ["UnrefinedOres"], "refinesTo": "Gold"},
                {"name": "Raw_Quantainium", "recordName": "ResourceType.Raw_Quantainium",
                 "displayName": "Quantainium (Raw)", "description": "Unstable.",
                 "group": "Mineral", "subgroups": ["Raw_Minerals"], "refinesTo": "Quantanium",
                 "volatility": {"volatility": 1, "healthDecayPerSecond": 0.0675},
                 "quality": {"tier": "Legendary", "distribution": {"min": 501, "max": 1000, "mean": 500, "stddev": 203},
                             "bands": [{"start": 0, "end": 399, "value": 344}, {"start": 950, "end": 998, "value": 974}]}},
                {"name": "Quantanium", "recordName": "ResourceType.Quantanium", "displayName": "Quantainium",
                 "group": "Mineral", "subgroups": []},
                {"name": "HeatPlaceholder", "recordName": "ResourceType.HeatPlaceholder",
                 "group": "HeatPlaceholder", "subgroups": []},
                {"name": "VIPCryopod", "recordName": "ResourceType.VIPCryopod", "group": "CleanAir", "subgroups": []}
            ],
            "items": [
                {"name": "agricium_ore", "recordName": "EntityClassDefinition.gold_ore", "displayName": "Gold (Ore)",
                 "type": "Metal", "resource": "Ore_Gold", "capacityScu": 0.01},
                {"name": "Gold", "recordName": "EntityClassDefinition.Gold", "displayName": "Gold",
                 "type": "Metal", "resource": "Gold", "capacityScu": 0.01}
            ],
            "damage": [
                {"name": "CommodityDamageConfigBase", "minimumSpeed": 15, "gracePeriod": 10, "explosionDamage": 375},
                {"name": "CommodityDamageConfigMole", "minimumSpeed": 15, "gracePeriod": 10, "explosionDamage": 375},
                {"name": "CommodityDamageConfigOdd", "minimumSpeed": 5}
            ]
        }))
        .unwrap()
    }

    #[test]
    fn default_density_of_one_is_not_shown() {
        let d = data();
        let density = |id: &str| {
            commodity_detail(&d, id, |_| None, |_| CommodityMarket::default())
                .unwrap()
                .density
        };
        assert_eq!(density("ResourceType.Gold"), Some(19.3));
        let mut one = data();
        one.resources[3].density_grams_per_cc = Some(1.0);
        let q = commodity_detail(
            &one,
            "ResourceType.Quantanium",
            |_| None,
            |_| CommodityMarket::default(),
        )
        .unwrap();
        assert_eq!(q.density, None);
    }

    #[test]
    fn group_keys_read_as_words() {
        assert_eq!(group_label("UnrefinedOres"), "Unrefined Ores");
        assert_eq!(group_label("Raw_Minerals"), "Raw Minerals");
        assert_eq!(group_label("ProcessedGoods"), "Processed Goods");
        assert_eq!(group_label("Nonmetal"), "Nonmetal");
        assert_eq!(group_label("Bulk_Supplies"), "Bulk Supplies");
        assert_eq!(group_label(""), "");
    }

    #[test]
    fn list_is_tradable_resources_by_group_then_name() {
        let l = commodity_list(&data());
        let rows: Vec<(&str, &str, Option<&str>)> = l
            .iter()
            .map(|s| (s.group.as_str(), s.name.as_str(), s.subgroup.as_deref()))
            .collect();
        assert_eq!(
            rows,
            [
                ("Clean Air", "VIPCryopod", None),
                ("Metal", "Gold", None),
                ("Metal", "Gold (Ore)", Some("Unrefined Ores")),
                ("Mineral", "Quantainium", None),
                ("Mineral", "Quantainium (Raw)", Some("Raw Minerals")),
            ]
        );
        let q = &l[4];
        assert!(q.volatile && q.raw);
        assert_eq!(q.id, "ResourceType.Raw_Quantainium");
        assert!(!l[1].volatile && !l[1].raw);
    }

    #[test]
    fn detail_links_refined_and_raw_forms_and_asks_mined_only_for_raw() {
        let d = data();
        let mut asked = Vec::new();
        let raw = commodity_detail(
            &d,
            "ResourceType.Raw_Quantainium",
            |q| {
                asked.push(q.to_string());
                Some(Vec::new())
            },
            |_| CommodityMarket::default(),
        )
        .unwrap();
        assert_eq!(asked, ["ResourceType.Raw_Quantainium"]);
        assert_eq!(
            raw.refined,
            Some(CommodityRef {
                id: "ResourceType.Quantanium".into(),
                name: "Quantainium".into()
            })
        );
        assert!(raw.raw_sources.is_empty());
        assert_eq!(raw.mined, Some(Vec::new()));
        let q = raw.quality.as_ref().unwrap();
        assert_eq!(q.tier.as_deref(), Some("Legendary"));
        // 0-399 is below the curve's min: only the 974 step is reachable.
        assert_eq!(q.steps.len(), 1);
        assert_eq!(
            raw.volatility.as_ref().unwrap().decay_per_second,
            Some(0.0675)
        );
        // Identical damage configurations are one row.
        assert_eq!(raw.damage.len(), 2);
        assert_eq!(
            raw.damage[0].configs,
            ["CommodityDamageConfigBase", "CommodityDamageConfigMole"]
        );

        let refined = commodity_detail(
            &d,
            "Quantanium",
            |_| panic!("not mined"),
            |_| CommodityMarket::default(),
        )
        .unwrap();
        assert_eq!(refined.raw_sources[0].id, "ResourceType.Raw_Quantainium");
        assert!(refined.mined.is_none() && refined.refined.is_none());
        assert!(refined.damage.is_empty() && refined.volatility.is_none());

        let gold = commodity_detail(
            &d,
            "resourcetype.ore_gold",
            |_| None,
            |_| CommodityMarket::default(),
        )
        .unwrap();
        assert_eq!(gold.cargo.len(), 1);
        assert_eq!(gold.cargo[0].capacity_scu, Some(0.01));
        assert!(gold.mined.is_none() && gold.quality.is_none());
        assert!(
            commodity_detail(&d, "Unobtainium", |_| None, |_| CommodityMarket::default()).is_none()
        );
    }

    #[test]
    fn kiosks_rank_both_sides_and_cap() {
        let d = data();
        let res = quality_resources(&d);
        let terminals: Vec<UexTerminal> = (1..=20)
            .map(|i| UexTerminal {
                id: Some(f64::from(i)),
                nickname: Some(format!("Kiosk {i}")),
                space_station_name: Some(format!("Station {i}")),
                star_system_name: Some("Stanton".into()),
                is_available: Some(1.0),
                is_available_live: Some(1.0),
                ..Default::default()
            })
            .collect();
        let rows: Vec<CommodityPrice> = (1..=20)
            .map(|i| CommodityPrice {
                commodity_name: Some("Gold".into()),
                id_terminal: Some(f64::from(i)),
                price_sell: Some(f64::from(i) * 10.0),
                price_buy: (i <= 3).then(|| f64::from(40 - i)),
                ..Default::default()
            })
            .collect();
        let meta = MarketMeta {
            loaded: true,
            fetched: Some("2026-11-22T00:00:00.000Z".into()),
            folder: None,
        };
        let k = Kiosks::new(&rows, &terminals, &res, meta);
        let m = k.market("Gold");
        assert!(m.loaded);
        assert_eq!((m.sell.len(), m.sell_total), (MAX_KIOSKS, 20));
        assert_eq!(m.sell[0].price, 200.0);
        assert_eq!((m.buy.len(), m.buy_total), (3, 3));
        assert_eq!(m.buy[0].price, 37.0);
        let none = k.market("Ore_Gold");
        assert!(none.loaded && none.sell.is_empty() && none.buy.is_empty());
        let missing = Kiosks::missing(Some("/x".into())).market("Gold");
        assert!(!missing.loaded && missing.folder.as_deref() == Some("/x"));
    }
}
