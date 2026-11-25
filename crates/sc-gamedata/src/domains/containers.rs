//! Inventory containers: cargo grids, personal inventories and storage (port of
//! `sc-gamedata/src/domains/containers.ts`).
//!
//! Capacity is expressed as interior dimensions rather than a single number, because what
//! fits depends on box size as well as total volume. Volume and an approximate SCU figure
//! are derived for convenience.
//!
//! Some containers carry sentinel dimensions meaning "unbounded" rather than a real size:
//! a landing zone reads 999x999x999, which computes to half a billion SCU. Those are
//! flagged instead of being reported as capacity.

use crate::domain::{Domain, DomainError, ParseContext, field};
use crate::entity::vec;
use crate::helpers_ships::{numbers_of, round_to, truthy};
use crate::records::{GameRecord, str_};
use serde::Serialize;
use serde_json::{Map, Value};

pub static DOMAIN: Containers = Containers;

pub struct Containers;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Container {
    pub name: String,
    pub record_name: String,
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inventory_type: Option<String>,
    /// Metres, as x/y/z.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interior_dimensions: Option<Map<String, Value>>,
    /// Cubic metres, derived from the dimensions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume_cubic_metres: Option<f64>,
    /// One SCU is a 1.25m cube, so 1.953125 cubic metres.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approx_scu: Option<f64>,
    /// Dimensions are a stand-in for "unbounded", not a real capacity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unbounded: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub excluded_sub_types: Option<Vec<String>>,
}

const CUBIC_METRES_PER_SCU: f64 = 1.25 * 1.25 * 1.25;

/// Above this, a dimension is a sentinel rather than a measurement. The largest real cargo
/// grids are tens of metres; 999 is the game's stand-in for unbounded.
const SENTINEL_DIMENSION: f64 = 900.0;

pub fn parse_container(record: &GameRecord) -> Container {
    let v = &record.value;
    let dims = vec(v.get("interiorDimensions"));

    let unbounded = dims
        .as_ref()
        .is_some_and(|d| numbers_of(d).any(|x| x >= SENTINEL_DIMENSION))
        .then_some(true);

    let mut volume = None;
    if let Some(d) = &dims {
        let g = |k: &str| d.get(k).and_then(Value::as_f64);
        let (x, y, z) = (g("x"), g("y"), g("z"));
        if truthy(x) && truthy(y) && truthy(z) && unbounded.is_none() {
            let (x, y, z) = (x.unwrap_or(0.0), y.unwrap_or(0.0), z.unwrap_or(0.0));
            volume = Some(round_to(x * y * z, 1000.0));
        }
    }

    let excluded: Option<Vec<String>> = v
        .get("excludedItemSubTypes")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|e| str_(Some(e)).map(str::to_string))
                .collect()
        });

    let inv = v.get("inventoryType");
    Container {
        name: record.short_name.clone(),
        record_name: record.name.clone(),
        id: record.id.clone(),
        inventory_type: str_(inv)
            .or_else(|| str_(inv.and_then(|i| i.get("_Type_"))))
            .map(str::to_string),
        interior_dimensions: dims,
        volume_cubic_metres: volume,
        approx_scu: volume
            .filter(|&v| truthy(Some(v)))
            .map(|v| round_to(v / CUBIC_METRES_PER_SCU, 100.0)),
        unbounded,
        excluded_sub_types: excluded.filter(|e| !e.is_empty()),
    }
}

/// Every `InventoryContainer`, largest volume first (stable, as JS sort).
pub fn parse_containers(records: &crate::records::RecordSet) -> Vec<Container> {
    let mut containers: Vec<Container> = records
        .of_type("InventoryContainer")
        .map(parse_container)
        .collect();
    containers.sort_by(|a, b| {
        let (a, b) = (
            a.volume_cubic_metres.unwrap_or(0.0),
            b.volume_cubic_metres.unwrap_or(0.0),
        );
        b.partial_cmp(&a).unwrap_or(std::cmp::Ordering::Equal)
    });
    containers
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Summary {
    containers: usize,
    with_dimensions: usize,
    unbounded: usize,
    largest_scu: f64,
}

impl Domain for Containers {
    fn name(&self) -> &'static str {
        "game-containers"
    }

    fn description(&self) -> &'static str {
        "Cargo grids and inventory containers with their dimensions and capacity."
    }

    fn filters(&self) -> &'static [&'static str] {
        &["**/inventorycontainers/**"]
    }

    fn parse(&self, ctx: &ParseContext<'_>) -> Result<Map<String, Value>, DomainError> {
        let containers = parse_containers(ctx.records);
        let sized: Vec<&Container> = containers
            .iter()
            .filter(|c| truthy(c.volume_cubic_metres))
            .collect();
        let summary = Summary {
            containers: containers.len(),
            with_dimensions: sized.len(),
            unbounded: containers.iter().filter(|c| c.unbounded.is_some()).count(),
            largest_scu: sized.first().and_then(|c| c.approx_scu).unwrap_or(0.0),
        };
        let mut body = Map::new();
        field(&mut body, "containers", &containers)?;
        field(&mut body, "summary", &summary)?;
        Ok(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::records::normalize_record;
    use serde_json::json;

    fn container(name: &str, value: Value) -> GameRecord {
        let mut v = value;
        v["_Type_"] = json!("InventoryContainer");
        normalize_record(
            json!({"_RecordName_": format!("InventoryContainer.{name}"), "_RecordId_": "id", "_RecordValue_": v}),
            "libs/foundry/records/inventorycontainers/x",
        )
        .unwrap()
    }

    #[test]
    fn derives_volume_and_scu() {
        let c = parse_container(&container(
            "Grid",
            json!({"interiorDimensions": {"_Type_": "Vec3", "x": 2.5, "y": 1.25, "z": 1.25},
                "inventoryType": {"_Type_": "InventoryOpenContainerType"},
                "excludedItemSubTypes": ["", "Personal"]}),
        ));
        assert_eq!(c.volume_cubic_metres, Some(3.906));
        assert_eq!(c.approx_scu, Some(2.0));
        assert_eq!(c.inventory_type.as_deref(), Some("InventoryOpenContainerType"));
        assert_eq!(c.excluded_sub_types, Some(vec!["Personal".to_string()]));
        assert_eq!(c.unbounded, None);
    }

    #[test]
    fn flags_sentinel_dimensions() {
        let c = parse_container(&container(
            "LZ",
            json!({"interiorDimensions": {"x": 999, "y": 999, "z": 999}, "excludedItemSubTypes": []}),
        ));
        assert_eq!(c.unbounded, Some(true));
        assert_eq!(c.volume_cubic_metres, None);
        assert_eq!(c.approx_scu, None);
        assert_eq!(c.excluded_sub_types, None);
    }

    #[test]
    fn zero_dimension_has_no_volume_and_sorting_is_stable() {
        let set = crate::records::RecordSet::new(vec![
            container("A", json!({"interiorDimensions": {"x": 0, "y": 1, "z": 1}})),
            container("B", json!({"interiorDimensions": {"x": 1, "y": 1, "z": 1}})),
            container("C", json!({})),
        ]);
        let all = parse_containers(&set);
        let names: Vec<_> = all.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["B", "A", "C"]);
        assert_eq!(all[1].volume_cubic_metres, None);
    }
}
