//! Shops: the global rules every shop and commodity kiosk applies (port of
//! `sc-gamedata/src/domains/shops.ts`).
//!
//! Prices are set per shop by the server and are not here. What ships is the rulebook:
//! sell-back adjustments (percent, with per-type overrides and two unnamed four-point
//! curves), the supply/demand thresholds behind kiosk labels, the auto-loading fee per box
//! size, licence-tier price modifiers and the tutorial cap.
//!
//! The TypeScript imports two one-liners from other domains: `boxSizes` (cargo.ts,
//! re-exported here as [`box_sizes`]) and `entityName` (wikelo.ts, [`entity_name`] over
//! [`crate::helpers_gameplay::file_ref_stem`]).

use crate::domain::{Domain, DomainError, ParseContext, field};
use crate::helpers_gameplay::file_ref_stem;
use crate::output::js_key_order;
use crate::records::{GameRecord, num, str_};
use serde::Serialize;
use serde_json::{Map, Value, json};

pub static DOMAIN: Shops = Shops;

pub struct Shops;

pub use crate::domains::cargo::{BoxSizeValue, box_sizes};

/// wikelo.ts `entityName`: last path segment minus `.json` (which can leave `""`).
pub fn entity_name(value: Option<&Value>) -> Option<String> {
    file_ref_stem(value).map(str::to_owned)
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemTypeModifier {
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub ty: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_percentage: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SellingRules {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_percentage: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub no_match_percentage: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mission_item_sell_price_reduction_percentage: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_inventory_curve: Option<Vec<f64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wear_curve: Option<Vec<f64>>,
    pub item_type_modifiers: Vec<ItemTypeModifier>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommodityRules {
    /// Threshold in percent behind each supply label (a JS object).
    pub supply_levels: Map<String, Value>,
    /// Threshold in percent behind each demand label.
    pub demand_levels: Map<String, Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_loading_base_price: Option<f64>,
    /// aUEC per box, by box size in SCU.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_loading_box_prices: Option<Vec<BoxSizeValue>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_kiosk_cargo_grid_display_size: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LicensedItemModifier {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub percentage_modifier: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuyingRules {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tutorial_item_buy_limit_per_hour: Option<f64>,
    pub licensed_item_modifiers: Vec<LicensedItemModifier>,
    /// Most items a single purchase can multiply up to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_buying_multiplier: Option<f64>,
}

/// `numbers`: an array's numbers, others dropped.
fn numbers(value: Option<&Value>) -> Option<Vec<f64>> {
    Some(value?.as_array()?.iter().filter_map(|v| num(Some(v))).collect())
}

/// `objects`: an array's truthy object (or array) elements.
fn objects(value: Option<&Value>) -> Vec<&Value> {
    value
        .and_then(Value::as_array)
        .map(|a| a.iter().filter(|x| x.is_object() || x.is_array()).collect())
        .unwrap_or_default()
}

pub fn parse_selling(record: &GameRecord) -> SellingRules {
    let v = &record.value;
    SellingRules {
        match_percentage: num(v.get("matchPercentage")),
        no_match_percentage: num(v.get("noMatchPercentage")),
        mission_item_sell_price_reduction_percentage: num(
            v.get("missionItemSellPriceReductionPercentage"),
        ),
        max_inventory_curve: numbers(v.get("maxInventoryCurve")),
        wear_curve: numbers(v.get("wearCurve")),
        item_type_modifiers: objects(v.get("itemTypeModifiers"))
            .into_iter()
            .map(|m| ItemTypeModifier {
                ty: str_(m.get("Type")).map(str::to_string),
                sub_type: str_(m.get("SubType"))
                    .filter(|s| *s != "UNDEFINED")
                    .map(str::to_string),
                match_percentage: num(m.get("matchPercentage")),
            })
            .collect(),
    }
}

/// `levels`: fields named `VeryLowSupplyLevel` or `noSupplyLevel` -> `{veryLow: 10, no: 0}`.
fn levels(v: &Value, suffix: &str) -> Map<String, Value> {
    let mut out = Map::new();
    let Some(obj) = v.as_object() else {
        return out;
    };
    let tail = format!("{suffix}Level");
    for key in js_key_order(obj) {
        // `/^(.+)<suffix>Level$/`: `.` excludes line terminators.
        let Some(head) = key.strip_suffix(tail.as_str()) else {
            continue;
        };
        if head.is_empty() || head.contains(['\n', '\r', '\u{2028}', '\u{2029}']) {
            continue;
        }
        let Some(n) = num(obj.get(key.as_str())) else {
            continue;
        };
        // `match[1][0].toLowerCase() + match[1].slice(1)`: the first UTF-16 unit.
        let units: Vec<u16> = head.encode_utf16().collect();
        let first = String::from_utf16_lossy(&units[..1]).to_lowercase();
        let rest = String::from_utf16_lossy(&units[1..]);
        out.insert(format!("{first}{rest}"), json!(n));
    }
    out
}

pub fn parse_commodity_rules(record: &GameRecord) -> CommodityRules {
    let v = &record.value;
    CommodityRules {
        supply_levels: levels(v, "Supply"),
        demand_levels: levels(v, "Demand"),
        auto_loading_base_price: num(v.get("autoLoadingBasePrice")),
        auto_loading_box_prices: box_sizes(v.get("autoLoadingBoxSizePrices")),
        max_kiosk_cargo_grid_display_size: num(v.get("MaxKioskCargoGridDisplaySize")),
    }
}

impl Domain for Shops {
    fn name(&self) -> &'static str {
        "game-shops"
    }

    fn description(&self) -> &'static str {
        "Global shop rules: sell-back cuts, supply and demand bands, auto-loading fees."
    }

    fn filters(&self) -> &'static [&'static str] {
        &["**/globalshopparams/**"]
    }

    fn parse(&self, ctx: &ParseContext<'_>) -> Result<Map<String, Value>, DomainError> {
        let by_type = |ty: &str| ctx.records.iter().find(|r| r.ty == ty);
        let selling = by_type("GlobalShopSellingParams");
        let commodity = by_type("GlobalShopCommodityParams");
        let buying = by_type("GlobalShopBuyingParams").map(|r| &r.value);
        let terminal = by_type("GlobalShopTerminalParams").map(|r| &r.value);

        let buying_rules = BuyingRules {
            tutorial_item_buy_limit_per_hour: num(buying.and_then(|b| b.get("tutorialItemBuyLimitPerHour"))),
            licensed_item_modifiers: objects(buying.and_then(|b| b.get("licensedItemModifiers")))
                .into_iter()
                .map(|m| LicensedItemModifier {
                    item: entity_name(m.get("LicensedItem")),
                    tier: str_(m.get("Type")).map(str::to_string),
                    percentage_modifier: num(m.get("PercentageModifier")),
                })
                .collect(),
            max_buying_multiplier: num(terminal.and_then(|t| t.get("maxBuyingMultiplier"))),
        };
        let selling_rules = selling.map(parse_selling);

        let mut body = Map::new();
        if let Some(s) = &selling_rules {
            field(&mut body, "selling", s)?;
        }
        if let Some(c) = commodity {
            field(&mut body, "commodities", &parse_commodity_rules(c))?;
        }
        field(&mut body, "buying", &buying_rules)?;
        body.insert(
            "summary".into(),
            json!({"itemTypeModifiers": selling_rules.as_ref().map_or(0, |s| s.item_type_modifiers.len())}),
        );
        Ok(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::NoFiles;
    use crate::domains::commodities::tests::rec;
    use crate::localization::Localization;
    use crate::records::RecordSet;

    // Cases from sc-gamedata src/domains/economy.test.ts.
    #[test]
    fn names_supply_and_demand_bands() {
        let rules = parse_commodity_rules(&rec(
            "GlobalShopCommodityParams",
            "Data",
            json!({"noSupplyLevel": 0, "VeryLowSupplyLevel": 10, "VeryHighDemandLevel": 10,
                   "autoLoadingBoxSizePrices": {"oneSCU": 30, "thirtyTwoSCU": 680}}),
            None,
        ));
        assert_eq!(Value::Object(rules.supply_levels), json!({"no": 0.0, "veryLow": 10.0}));
        assert_eq!(Value::Object(rules.demand_levels), json!({"veryHigh": 10.0}));
        assert_eq!(
            rules.auto_loading_box_prices,
            Some(vec![
                BoxSizeValue { scu: 1.0, value: 30.0 },
                BoxSizeValue { scu: 32.0, value: 680.0 }
            ])
        );
    }

    #[test]
    fn drops_the_undefined_subtype() {
        let selling = parse_selling(&rec(
            "GlobalShopSellingParams",
            "Data",
            json!({"matchPercentage": -20,
                   "itemTypeModifiers": [{"Type": "Cooler", "SubType": "UNDEFINED", "matchPercentage": -75}]}),
            None,
        ));
        assert_eq!(
            selling.item_type_modifiers,
            vec![ItemTypeModifier {
                ty: Some("Cooler".into()),
                sub_type: None,
                match_percentage: Some(-75.0)
            }]
        );
    }

    #[test]
    fn box_sizes_smallest_first_and_entity_names() {
        assert_eq!(
            box_sizes(Some(&json!({"sixteenSCU": 12, "label": "x", "one_eighthSCU": 0, "oneSCU": 2}))),
            Some(vec![
                BoxSizeValue { scu: 0.125, value: 0.0 },
                BoxSizeValue { scu: 1.0, value: 2.0 },
                BoxSizeValue { scu: 16.0, value: 12.0 },
            ])
        );
        assert_eq!(box_sizes(Some(&json!(3))), None);
        assert_eq!(
            entity_name(Some(&json!("file://./../entities/scitem/x/Item_A.JSON"))).as_deref(),
            Some("Item_A")
        );
        assert_eq!(entity_name(Some(&json!("a/.json"))).as_deref(), Some(""));
        assert_eq!(entity_name(Some(&json!("a/"))), None);
    }

    #[test]
    fn whole_domain_shape() {
        let set = RecordSet::new(vec![
            rec(
                "GlobalShopBuyingParams",
                "B",
                json!({"tutorialItemBuyLimitPerHour": 5, "licensedItemModifiers": [
                    {"LicensedItem": "file://./x/item_a.json", "Type": "Tier1", "PercentageModifier": 10}, null]}),
                None,
            ),
            rec("GlobalShopTerminalParams", "T", json!({"maxBuyingMultiplier": 99}), None),
        ]);
        let loc = Localization::new();
        let out = DOMAIN.parse(&ParseContext::new(&set, &loc, &NoFiles)).unwrap();
        assert_eq!(
            Value::Object(out),
            json!({
                "buying": {"tutorialItemBuyLimitPerHour": 5.0,
                           "licensedItemModifiers": [{"item": "item_a", "tier": "Tier1", "percentageModifier": 10.0}],
                           "maxBuyingMultiplier": 99.0},
                "summary": {"itemTypeModifiers": 0}
            })
        );
    }
}
