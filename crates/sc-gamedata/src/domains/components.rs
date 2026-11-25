//! Ship components: everything that slots into a ship and carries a stat (port of
//! `sc-gamedata/src/domains/components.ts`, output `game-ship-components`).
//!
//! Every ship item is an `EntityClassDefinition` whose `Components` array holds one typed
//! params struct per aspect. Two of those matter here:
//!
//! - `SAttachableComponentParams.AttachDef` carries the common item facts: Type, Size,
//!   Grade, Manufacturer and the localized name.
//! - A category-specific struct carries the stats, e.g. `SCItemShieldGeneratorParams`.
//!
//! Category is decided by which params struct is present, falling back to `AttachDef.Type`
//! for items whose only stats live on the shared resource struct (power plants, coolers,
//! life support). Paths mislead: a glob for "shield" under ships also matches a seat blast
//! shield, which has no shield generator at all.
//!
//! Every component carries the same headline columns a loadout site shows: `power` drawn,
//! `em`/`ir` emitted, `health`, `mass` and `distortionMax`. Category stats sit in `stats` as
//! a flat numeric map; structured extras (resistance tables, armour multipliers) in
//! `detail`.
//!
//! `item-common.ts` lives in [`crate::helpers_ships::item_common`].

use crate::collate::locale_compare;
use crate::domain::{Domain, DomainError, ParseContext, field};
use crate::domains::containers::parse_container;
use crate::entity::{component, components, referenced_name};
use crate::helpers_ships::item_common::{
    damage_type_map, damage_type_ranges, distortion_max_of, distortion_of, durability_of,
    health_of, item_identity, mass_of, resource_profile, volume_of,
};
use crate::helpers_ships::{at_map, number};
use crate::localization::{LocRefs, Localization};
use crate::records::{GameRecord, RecordSet, array, at, num, str_};
use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::HashMap;

pub static DOMAIN: Components = Components;

pub struct Components;

/// One component. Field order is the TypeScript return literal's.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Component {
    pub name: String,
    /// True when the record has no real display name, i.e. a template or stub.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_template: Option<bool>,
    /// `nameKey`, `descriptionKey`, `nameStatus`, copied from the identity.
    #[serde(flatten)]
    pub loc: LocRefs,
    pub record_name: String,
    pub id: String,
    pub category: &'static str,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub ty: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grade: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manufacturer: Option<String>,
    /// Power drawn per second while online.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub power: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub em: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ir: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mass: Option<f64>,
    /// Distortion absorbed before the item shuts down.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub distortion_max: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub durability: Option<Map<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub distortion: Option<Map<String, Value>>,
    /// Inventory volume, micro-SCU.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume: Option<f64>,
    pub stats: Map<String, Value>,
    /// Category-specific structure that does not flatten to one number.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<Map<String, Value>>,
}

impl Component {
    /// A stat as a number.
    pub fn stat(&self, key: &str) -> Option<f64> {
        self.stats.get(key).and_then(Value::as_f64)
    }
}

/// Inventory containers by lower-case short name, for cargo grid capacity.
pub type ContainerIndex<'a> = HashMap<String, &'a GameRecord>;

/// `indexContainers`: records under `/inventorycontainers/` by lower-case short name.
pub fn index_containers(records: &RecordSet) -> ContainerIndex<'_> {
    let mut index = HashMap::new();
    for r in records.iter() {
        if r.record_path
            .to_lowercase()
            .contains("/inventorycontainers/")
        {
            index.insert(r.short_name.to_lowercase(), r);
        }
    }
    index
}

struct Parsed {
    stats: Map<String, Value>,
    /// `None` values are JS `undefined` entries, dropped later.
    detail: Option<Vec<(&'static str, Option<Value>)>>,
}

impl Parsed {
    fn stats(stats: Map<String, Value>) -> Self {
        Self {
            stats,
            detail: None,
        }
    }
}

type Picker = fn(&Map<String, Value>, &GameRecord, Option<&ContainerIndex<'_>>) -> Parsed;

struct Category {
    category: &'static str,
    /// The stats struct that identifies the category, if it has one.
    params: Option<&'static str>,
    /// Otherwise, or additionally, the `AttachDef.Type` values that do.
    types: Option<&'static [&'static str]>,
    pick: Picker,
}

/// `pickStats`: numeric fields only, keyed by their last path segment.
fn pick_stats(params: &Map<String, Value>, fields: &[&str]) -> Map<String, Value> {
    let mut stats = Map::new();
    for f in fields {
        if let Some(v) = num(at_map(params, f)) {
            let key = f.rsplit('.').next().unwrap_or(f);
            stats.insert(key.to_string(), number(v));
        }
    }
    stats
}

/// `pickNumeric`: every top-level number on a struct.
fn pick_numeric(params: &Map<String, Value>) -> Map<String, Value> {
    params
        .iter()
        .filter(|(_, v)| v.is_number())
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

fn component_or_empty<'a>(record: &'a GameRecord, ty: &str) -> &'a Map<String, Value> {
    static EMPTY: std::sync::LazyLock<Map<String, Value>> = std::sync::LazyLock::new(Map::new);
    component(record, ty).unwrap_or(&EMPTY)
}

/// Ports that accept the given item type: how many, and the largest size they take.
fn ports_of_type(record: &GameRecord, ty: &str) -> (usize, Option<f64>) {
    let ports = array(component(record, "SItemPortContainerComponentParams").and_then(|c| c.get("Ports")));
    let matching: Vec<&Value> = ports
        .iter()
        .filter(|p| p.is_object())
        .filter(|p| {
            array(p.get("Types"))
                .iter()
                .any(|t| at(t, "Type").and_then(Value::as_str) == Some(ty))
        })
        .collect();
    let size = matching
        .iter()
        .filter_map(|p| num(p.get("MaxSize")))
        .reduce(f64::max);
    (matching.len(), size)
}

/// What the item generates per second of one resource, under a stat name.
fn generated(record: &GameRecord, resource: &str, name: &str) -> Map<String, Value> {
    let mut m = Map::new();
    if let Some(v) = resource_profile(record).generated(resource) {
        m.insert(name.to_string(), number(v));
    }
    m
}

/// Unbounded ranges read 3.4e38; that is a sentinel, not a figure.
const UNBOUNDED: f64 = 1e30;

fn resource_capacity(record: &GameRecord) -> Map<String, Value> {
    pick_stats(
        component_or_empty(record, "ResourceContainer"),
        &["capacity.standardCargoUnits"],
    )
}

const CATEGORIES: &[Category] = &[
    Category {
        category: "shields",
        params: Some("SCItemShieldGeneratorParams"),
        types: None,
        pick: |s, _, _| Parsed {
            stats: pick_stats(
                s,
                &[
                    "MaxShieldHealth",
                    "MaxShieldRegen",
                    "DecayRatio",
                    "DownedRegenDelay",
                    "DamagedRegenDelay",
                    "ReservePoolDrainRateRatio",
                    "ReservePoolMaxHealthRatio",
                    "ReservePoolRegenRateRatio",
                    "ElectricalChargeDamageResistance",
                    "stunParams.maxStunTime",
                ],
            ),
            detail: Some(vec![
                (
                    "resistance",
                    damage_type_ranges(s.get("ShieldResistance")).map(Value::Object),
                ),
                (
                    "absorption",
                    damage_type_ranges(s.get("ShieldAbsorption")).map(Value::Object),
                ),
            ]),
        },
    },
    Category {
        category: "quantumDrives",
        params: Some("SCItemQuantumDriveParams"),
        types: None,
        pick: |s, _, _| {
            let mut stats = pick_stats(
                s,
                &[
                    "quantumFuelRequirement",
                    "jumpRange",
                    "disconnectRange",
                    "params.driveSpeed",
                    "params.cooldownTime",
                    "params.stageOneAccelRate",
                    "params.stageTwoAccelRate",
                    "params.spoolUpTime",
                    "params.engageSpeed",
                    "params.calibrationRate",
                    "params.minCalibrationRequirement",
                    "params.maxCalibrationRequirement",
                    "heatParams.inFlightThermalEnergyDraw",
                ],
            );
            if stats
                .get("jumpRange")
                .and_then(Value::as_f64)
                .unwrap_or(0.0)
                > UNBOUNDED
            {
                // `delete`: later keys keep their order.
                stats.shift_remove("jumpRange");
            }
            if let Some(spline) = num(at_map(s, "splineJumpParams.driveSpeed")) {
                stats.insert("splineDriveSpeed".into(), number(spline));
            }
            Parsed::stats(stats)
        },
    },
    Category {
        category: "radars",
        params: Some("SCItemRadarComponentParams"),
        types: None,
        pick: |s, _, _| {
            let detection = s.get("signatureDetection").and_then(Value::as_array).map(|a| {
                Value::Array(
                    a.iter()
                        .map(|d| {
                            let mut o = Map::new();
                            if let Some(x) = num(at(d, "sensitivity")) {
                                o.insert("sensitivity".into(), number(x));
                            }
                            if let Some(x) = num(at(d, "piercing")) {
                                o.insert("piercing".into(), number(x));
                            }
                            let flag = |k: &str| at(d, k).and_then(Value::as_bool) == Some(true);
                            o.insert("passive".into(), Value::Bool(flag("permitPassiveDetection")));
                            o.insert("active".into(), Value::Bool(flag("permitActiveDetection")));
                            Value::Object(o)
                        })
                        .collect(),
                )
            });
            Parsed {
                stats: pick_stats(
                    s,
                    &[
                        "detectionLifetime",
                        "physicalRadarRange",
                        "pingProperties.cooldownTime",
                    ],
                ),
                detail: Some(vec![("signatureDetection", detection)]),
            }
        },
    },
    Category {
        category: "countermeasures",
        params: None,
        types: Some(&["WeaponDefensive"]),
        pick: |_, record, _| {
            let container = component_or_empty(record, "SAmmoContainerComponentParams");
            Parsed {
                stats: pick_stats(container, &["maxAmmoCount", "maxRestockCount"]),
                detail: Some(vec![(
                    "ammo",
                    referenced_name(container.get("ammoParamsRecord"))
                        .map(|n| Value::String(n.to_string())),
                )]),
            }
        },
    },
    Category {
        category: "weapons",
        params: Some("SCItemWeaponComponentParams"),
        types: None,
        pick: |s, record, _| {
            let mut stats = pick_stats(
                s,
                &["connectionParams.simplifiedHeatParams.overheatTemperature"],
            );
            for (k, v) in pick_stats(
                component_or_empty(record, "SAmmoContainerComponentParams"),
                &["maxAmmoCount"],
            ) {
                stats.insert(k, v);
            }
            Parsed::stats(stats)
        },
    },
    Category {
        category: "missiles",
        params: Some("SCItemMissileParams"),
        types: None,
        pick: |s, _, _| {
            Parsed::stats(pick_stats(
                s,
                &[
                    "targetingParams.lockTime",
                    "targetingParams.lockRangeMax",
                    "explosionParams.maxRadius",
                    "GCSParams.linearSpeed",
                    "maxLifetime",
                ],
            ))
        },
    },
    Category {
        category: "powerPlants",
        params: None,
        types: Some(&["PowerPlant"]),
        pick: |_, record, _| Parsed::stats(generated(record, "Power", "powerOutput")),
    },
    Category {
        category: "coolers",
        params: None,
        types: Some(&["Cooler"]),
        pick: |_, record, _| Parsed::stats(generated(record, "Coolant", "coolantOutput")),
    },
    Category {
        category: "lifeSupport",
        params: None,
        types: Some(&["LifeSupportGenerator"]),
        pick: |_, record, _| {
            Parsed::stats(generated(record, "LifeSupport", "lifeSupportOutput"))
        },
    },
    Category {
        category: "thrusters",
        params: Some("SCItemThrusterParams"),
        types: None,
        pick: |s, _, _| Parsed {
            stats: pick_stats(
                s,
                &[
                    "thrustCapacity",
                    "fuelBurnRatePer10KNewton",
                    "minHealthThrustMultiplier",
                ],
            ),
            detail: Some(vec![(
                "thrusterType",
                str_(s.get("thrusterType")).map(|t| Value::String(t.to_string())),
            )]),
        },
    },
    Category {
        category: "quantumFuelTanks",
        params: Some("SCItemFuelTankParams"),
        types: Some(&["QuantumFuelTank"]),
        pick: |_, record, _| Parsed::stats(resource_capacity(record)),
    },
    Category {
        category: "fuelTanks",
        params: Some("SCItemFuelTankParams"),
        types: None,
        pick: |_, record, _| Parsed::stats(resource_capacity(record)),
    },
    Category {
        category: "armor",
        params: Some("SCItemVehicleArmorParams"),
        types: None,
        pick: |s, _, _| Parsed {
            stats: pick_stats(
                s,
                &[
                    "signalInfrared",
                    "signalElectromagnetic",
                    "signalCrossSection",
                ],
            ),
            detail: Some(vec![
                (
                    "damageMultiplier",
                    damage_type_map(s.get("damageMultiplier")).map(Value::Object),
                ),
                (
                    "deflection",
                    damage_type_map(at_map(s, "armorDeflection.deflectionValue"))
                        .map(Value::Object),
                ),
                (
                    "penetrationAbsorption",
                    damage_type_map(at_map(
                        s,
                        "armorPenetrationResistance.penetrationAbsorptionForType",
                    ))
                    .map(Value::Object),
                ),
            ]),
        },
    },
    Category {
        category: "missileRacks",
        params: Some("SCItemMissileRackParams"),
        types: None,
        pick: |_, record, _| {
            let (count, size) = ports_of_type(record, "Missile");
            let mut stats = Map::new();
            stats.insert("missileCount".into(), Value::from(count));
            if let Some(size) = size {
                stats.insert("missileSize".into(), number(size));
            }
            Parsed::stats(stats)
        },
    },
    Category {
        category: "cargoGrids",
        params: Some("SCItemInventoryContainerComponentParams"),
        types: Some(&["CargoGrid"]),
        pick: |s, _, containers| {
            let name = referenced_name(s.get("containerParams"));
            let container = name
                .and_then(|n| containers.and_then(|c| c.get(&n.to_lowercase())))
                .copied();
            let parsed = container.map(parse_container);
            let mut stats = Map::new();
            if let Some(scu) = parsed.as_ref().and_then(|p| p.approx_scu) {
                stats.insert("scu".into(), number(scu));
            }
            Parsed {
                stats,
                detail: Some(vec![
                    ("container", name.map(|n| Value::String(n.to_string()))),
                    (
                        "interiorDimensions",
                        parsed
                            .and_then(|p| p.interior_dimensions)
                            .map(Value::Object),
                    ),
                ]),
            }
        },
    },
    Category {
        category: "jumpDrives",
        params: Some("SCItemJumpDriveParams"),
        types: None,
        pick: |s, _, _| {
            Parsed::stats(pick_stats(
                s,
                &[
                    "alignmentRate",
                    "alignmentDecayRate",
                    "tuningRate",
                    "tuningDecayRate",
                    "fuelUsageEfficiencyMultiplier",
                ],
            ))
        },
    },
    Category {
        category: "quantumEnforcement",
        params: Some("SCItemQuantumInterdictionGeneratorParams"),
        types: None,
        pick: |s, _, _| {
            Parsed::stats(pick_stats(
                s,
                &[
                    "jammerSettings.jammerRange",
                    "jammerSettings.maxPowerDraw",
                    "quantumInterdictionPulseSettings.chargeTimeSecs",
                    "quantumInterdictionPulseSettings.dischargeTimeSecs",
                    "quantumInterdictionPulseSettings.cooldownTimeSecs",
                    "quantumInterdictionPulseSettings.radiusMeters",
                ],
            ))
        },
    },
    Category {
        category: "emp",
        params: Some("SCItemEMPParams"),
        types: None,
        pick: |s, _, _| Parsed::stats(pick_numeric(s)),
    },
    Category {
        category: "selfDestruct",
        params: Some("SSCItemSelfDestructComponentParams"),
        types: None,
        pick: |s, _, _| Parsed::stats(pick_stats(s, &["damage", "minRadius", "radius", "time"])),
    },
    Category {
        category: "batteries",
        params: None,
        types: Some(&["Battery"]),
        pick: |_, record, _| Parsed::stats(resource_capacity(record)),
    },
    Category {
        // Gimbal and fixed mounts: no stats struct, but the gun size they take matters.
        category: "mounts",
        params: None,
        types: Some(&["Turret"]),
        pick: |_, record, _| {
            let (count, size) = ports_of_type(record, "WeaponGun");
            let mut stats = Map::new();
            stats.insert("gunCount".into(), Value::from(count));
            if let Some(size) = size {
                stats.insert("gunSize".into(), number(size));
            }
            Parsed::stats(stats)
        },
    },
];

fn find_params<'a>(record: &'a GameRecord, ty: &str) -> Option<&'a Map<String, Value>> {
    components(record).find(|c| c.get("_Type_").and_then(Value::as_str) == Some(ty))
}

/// `parseComponent`: the first category whose params struct (and/or type) the item has.
pub fn parse_component(
    record: &GameRecord,
    localization: &Localization,
    containers: Option<&ContainerIndex<'_>>,
) -> Option<Component> {
    let identity = item_identity(record, localization);
    let empty = Map::new();

    for cat in CATEGORIES {
        let found = cat.params.and_then(|p| find_params(record, p));
        if cat.params.is_some() && found.is_none() {
            continue;
        }
        if let Some(types) = cat.types
            && !identity.ty.as_deref().is_some_and(|t| types.contains(&t))
        {
            continue;
        }
        // Swappable mounts live under weapon_mounts; a ship's own turrets share the Turret
        // type but are part of the hull, not a component anyone swaps.
        if cat.category == "mounts"
            && !record
                .record_path
                .to_lowercase()
                .contains("/weapon_mounts/")
        {
            continue;
        }

        let picked = (cat.pick)(found.unwrap_or(&empty), record, containers);
        let detail: Option<Map<String, Value>> = picked.detail.map(|d| {
            d.into_iter()
                .filter_map(|(k, v)| v.map(|v| (k.to_string(), v)))
                .collect()
        });
        let resources = resource_profile(record);

        return Some(Component {
            name: identity.name,
            is_template: identity.is_template,
            loc: identity.loc,
            record_name: record.name.clone(),
            id: record.id.clone(),
            category: cat.category,
            ty: identity.ty,
            sub_type: identity.sub_type,
            size: identity.size,
            grade: identity.grade,
            manufacturer: identity.manufacturer,
            power: resources.power,
            em: resources.em,
            ir: resources.ir,
            health: health_of(record),
            mass: mass_of(record),
            distortion_max: distortion_max_of(record),
            durability: durability_of(record),
            distortion: distortion_of(record),
            volume: volume_of(record),
            stats: picked.stats,
            detail: detail.filter(|d| !d.is_empty()),
        });
    }
    None
}

/// Every component in the set, grouped by category in first-seen order, each group sorted
/// by size then name (as the domain outputs them).
pub fn parse_components(
    records: &RecordSet,
    localization: &Localization,
) -> Vec<(&'static str, Vec<Component>)> {
    let containers = index_containers(records);
    let mut by_category: Vec<(&'static str, Vec<Component>)> = Vec::new();
    for record in records.of_type("EntityClassDefinition") {
        let Some(c) = parse_component(record, localization, Some(&containers)) else {
            continue;
        };
        match by_category.iter_mut().find(|(k, _)| *k == c.category) {
            Some((_, list)) => list.push(c),
            None => by_category.push((c.category, vec![c])),
        }
    }
    for (_, list) in &mut by_category {
        list.sort_by(|a, b| {
            let d = a.size.unwrap_or(0.0) - b.size.unwrap_or(0.0);
            d.partial_cmp(&0.0)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| locale_compare(&a.name, &b.name))
        });
    }
    by_category
}

impl Domain for Components {
    fn name(&self) -> &'static str {
        "game-ship-components"
    }

    fn description(&self) -> &'static str {
        "Ship shields, quantum drives, power plants, coolers, radars, thrusters, fuel tanks, armour, cargo grids and more, with their stats."
    }

    fn filters(&self) -> &'static [&'static str] {
        &["**/entities/scitem/ships/**", "**/inventorycontainers/**"]
    }

    fn needs_localization(&self) -> bool {
        true
    }

    fn parse(&self, ctx: &ParseContext<'_>) -> Result<Map<String, Value>, DomainError> {
        let by_category = parse_components(ctx.records, ctx.localization);
        let mut body = Map::new();
        let mut summary = Map::new();
        for (category, list) in &by_category {
            summary.insert((*category).to_string(), Value::from(list.len()));
            field(&mut body, category, list)?;
        }
        body.insert("summary".into(), Value::Object(summary));
        Ok(body)
    }
}

#[cfg(test)]
mod tests {
    // Cases from sc-gamedata src/domains/components.test.ts.
    use super::*;
    use crate::domain::NoFiles;
    use crate::localization::{is_placeholder_name, parse_global_ini};
    use crate::records::normalize_record;
    use serde_json::json;

    fn localization() -> Localization {
        parse_global_ini("item_NameShield=5SA 'Rhada'")
    }

    fn entity(short: &str, path: &str, components: Value) -> GameRecord {
        normalize_record(
            json!({"_RecordName_": format!("EntityClassDefinition.{short}"), "_RecordId_": format!("id-{short}"),
                "_RecordValue_": {"_Type_": "EntityClassDefinition", "Components": components}}),
            &format!("libs/foundry/records/entities/scitem/ships/{path}/{short}"),
        )
        .unwrap()
    }

    fn ship_item(short: &str, params_type: &str, stats: Value, name_key: &str) -> GameRecord {
        let mut p = stats;
        p["_Type_"] = json!(params_type);
        entity(
            short,
            "x",
            json!([{"_Type_": "SAttachableComponentParams", "AttachDef": {"Type": "Shield",
                "SubType": "UNDEFINED", "Size": 2, "Grade": 1, "Localization": {"Name": name_key}}}, p]),
        )
    }

    fn ship_item_of_type(short: &str, ty: &str, extra: Vec<Value>, path: &str) -> GameRecord {
        let mut list = vec![json!({"_Type_": "SAttachableComponentParams", "AttachDef": {"Type": ty,
            "SubType": "UNDEFINED", "Size": 1, "Grade": 3,
            "Manufacturer": "file://./x/scitemmanufacturer.aegs.json",
            "Localization": {"Name": "@item_NameShield"}}})];
        list.extend(extra);
        entity(short, path, Value::Array(list))
    }


    #[test]
    fn placeholder_names() {
        for p in [None, Some(""), Some("@item_NameMissing"), Some("<= PLACEHOLDER =>"), Some("@LOC_PLACEHOLDER")] {
            assert!(is_placeholder_name(p), "{p:?}");
        }
        assert!(!is_placeholder_name(Some("5SA 'Rhada'")));
    }

    #[test]
    fn categorises_by_params_struct_and_reads_the_stats() {
        let r = ship_item(
            "SHLD_Test",
            "SCItemShieldGeneratorParams",
            json!({"MaxShieldHealth": 7200, "MaxShieldRegen": 1584, "DownedRegenDelay": 9.17, "Irrelevant": "ignored"}),
            "@item_NameShield",
        );
        let c = parse_component(&r, &localization(), None).unwrap();
        assert_eq!(c.category, "shields");
        assert_eq!(c.name, "5SA 'Rhada'");
        assert_eq!((c.size, c.grade), (Some(2.0), Some(1.0)));
        assert_eq!(c.sub_type, None);
        assert_eq!(
            Value::Object(c.stats),
            json!({"MaxShieldHealth": 7200.0, "MaxShieldRegen": 1584.0, "DownedRegenDelay": 9.17})
        );
    }

    #[test]
    fn falls_back_to_the_record_code_and_flags_templates() {
        let r = ship_item(
            "QDRV_S01_Template",
            "SCItemQuantumDriveParams",
            json!({"jumpRange": 1}),
            "@LOC_PLACEHOLDER",
        );
        let c = parse_component(&r, &localization(), None).unwrap();
        assert_eq!(c.name, "QDRV_S01_Template");
        assert_eq!(c.is_template, Some(true));
    }

    #[test]
    fn ignores_unknown_categories_and_non_numeric_stats() {
        let seat = ship_item(
            "MISC_BlastShield_SeatAccess",
            "SCItemSeatAccessParams",
            json!({}),
            "@item_NameShield",
        );
        assert!(parse_component(&seat, &localization(), None).is_none());
        let r = ship_item(
            "X",
            "SCItemShieldGeneratorParams",
            json!({"MaxShieldHealth": null}),
            "@item_NameShield",
        );
        assert!(parse_component(&r, &localization(), None).unwrap().stats.is_empty());
    }

    #[test]
    fn groups_components_by_category_and_counts_them() {
        let set = RecordSet::new(vec![
            ship_item("A", "SCItemShieldGeneratorParams", json!({"MaxShieldHealth": 100}), "@item_NameShield"),
            ship_item("B", "SCItemShieldGeneratorParams", json!({"MaxShieldHealth": 200}), "@item_NameShield"),
            ship_item("C", "SCItemQuantumDriveParams", json!({"jumpRange": 5}), "@item_NameShield"),
        ]);
        let loc = localization();
        let body = DOMAIN.parse(&ParseContext::new(&set, &loc, &NoFiles)).unwrap();
        assert_eq!(body["summary"], json!({"shields": 2, "quantumDrives": 1}));
        let keys: Vec<_> = body.keys().cloned().collect();
        assert_eq!(keys, vec!["shields", "quantumDrives", "summary"]);
    }

    fn online(deltas: Value) -> Value {
        json!({"_Type_": "ItemResourceComponentParams", "states": [{"name": "Online", "deltas": deltas,
            "signatureParams": {"EMSignature": {"nominalSignature": 7920}}}]})
    }

    #[test]
    fn reads_a_power_plants_output_off_the_resource_network() {
        let plant = ship_item_of_type(
            "POWR_Test",
            "PowerPlant",
            vec![
                online(json!([{"generation": {"resource": "Power", "resourceAmountPerSecond": {"units": 16}}}])),
                json!({"_Type_": "SHealthComponentParams", "Health": 270}),
                json!({"_Type_": "SEntityPhysicsControllerParams", "PhysType": {"Mass": 250}}),
                json!({"_Type_": "SDistortionParams", "Maximum": 4100}),
            ],
            "x",
        );
        let c = parse_component(&plant, &localization(), None).unwrap();
        assert_eq!(c.category, "powerPlants");
        assert_eq!(c.manufacturer.as_deref(), Some("AEGS"));
        assert_eq!((c.em, c.health, c.mass, c.distortion_max), (Some(7920.0), Some(270.0), Some(250.0), Some(4100.0)));
        assert_eq!(c.durability.unwrap()["health"], 270.0);
        assert_eq!(Value::Object(c.stats), json!({"powerOutput": 16.0}));
    }

    #[test]
    fn countermeasure_launcher_sorts_before_its_weapon_struct() {
        let launcher = ship_item_of_type(
            "CML_Test",
            "WeaponDefensive",
            vec![
                json!({"_Type_": "SCItemWeaponComponentParams", "fireActions": []}),
                json!({"_Type_": "SAmmoContainerComponentParams", "maxAmmoCount": 48,
                    "ammoParamsRecord": "file://./x/ammoparams.behr_flare.json"}),
            ],
            "x",
        );
        let c = parse_component(&launcher, &localization(), None).unwrap();
        assert_eq!(c.category, "countermeasures");
        assert_eq!(Value::Object(c.stats), json!({"maxAmmoCount": 48.0}));
        assert_eq!(c.detail.map(Value::Object), Some(json!({"ammo": "behr_flare"})));
    }

    #[test]
    fn shield_resistance_table_is_detail() {
        let shield = ship_item(
            "SHLD_Test",
            "SCItemShieldGeneratorParams",
            json!({"MaxShieldHealth": 3168, "ShieldResistance": [{"Min": 0, "Max": 0.25}],
                "ShieldAbsorption": [{"Min": 0, "Max": 0.45}, {"Min": 1, "Max": 1}]}),
            "@item_NameShield",
        );
        let c = parse_component(&shield, &localization(), None).unwrap();
        assert_eq!(
            c.detail.map(Value::Object),
            Some(json!({"resistance": {"physical": {"min": 0.0, "max": 0.25}},
                "absorption": {"physical": {"min": 0.0, "max": 0.45}, "energy": {"min": 1.0, "max": 1.0}}}))
        );
    }

    #[test]
    fn drops_the_unbounded_jump_range_sentinel() {
        let drive = ship_item(
            "QDRV_Test",
            "SCItemQuantumDriveParams",
            json!({"jumpRange": 3.402823e38, "params": {"driveSpeed": 161410900, "spoolUpTime": 4.4}}),
            "@item_NameShield",
        );
        let c = parse_component(&drive, &localization(), None).unwrap();
        assert_eq!(Value::Object(c.stats), json!({"driveSpeed": 161410900.0, "spoolUpTime": 4.4}));
    }

    #[test]
    fn cargo_grid_capacity_through_its_inventory_container() {
        let grid = ship_item_of_type(
            "DRAK_Cutlass_CargoGrid_Main",
            "CargoGrid",
            vec![json!({"_Type_": "SCItemInventoryContainerComponentParams",
                "containerParams": "file://./x/inventorycontainers/ships/drak_cutlass_cargogrid_main.json"})],
            "x",
        );
        let container = normalize_record(
            json!({"_RecordName_": "InventoryContainer.drak_cutlass_cargogrid_main", "_RecordId_": "c",
                "_RecordValue_": {"_Type_": "InventoryContainer",
                    "interiorDimensions": {"_Type_": "Vec3", "x": 5, "y": 6.25, "z": 2.5}}}),
            "libs/foundry/records/inventorycontainers/ships/drak_cutlass_cargogrid_main",
        )
        .unwrap();
        let mut index = ContainerIndex::new();
        index.insert("drak_cutlass_cargogrid_main".into(), &container);
        let c = parse_component(&grid, &localization(), Some(&index)).unwrap();
        assert_eq!(Value::Object(c.stats), json!({"scu": 40.0}));
        assert_eq!(c.detail.unwrap()["container"], "drak_cutlass_cargogrid_main");
    }

    #[test]
    fn counts_gun_slots_on_a_swappable_mount_and_ignores_a_ships_turret() {
        let ports = json!({"_Type_": "SItemPortContainerComponentParams",
            "Ports": [{"Name": "hardpoint_class_2", "MaxSize": 3, "Types": [{"Type": "WeaponGun"}]}]});
        let mount = ship_item_of_type("Mount_Gimbal_S3", "Turret", vec![ports.clone()], "weapon_mounts/gimbal");
        let c = parse_component(&mount, &localization(), None).unwrap();
        assert_eq!(c.category, "mounts");
        assert_eq!(Value::Object(c.stats), json!({"gunCount": 1, "gunSize": 3.0}));
        let turret = ship_item_of_type(
            "DRAK_Cutlass_Turret",
            "Turret",
            vec![ports, json!({"_Type_": "SCItemTurretParams"})],
            "turret",
        );
        assert!(parse_component(&turret, &localization(), None).is_none());
    }
}
