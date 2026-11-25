//! Facts every ship item shares (port of `sc-gamedata/src/domains/item-common.ts`, for the
//! ships batch; the `item_common` slot may carry its own port, fold together later).
//!
//! `SAttachableComponentParams.AttachDef` carries identity (type, size, grade,
//! manufacturer, localized name). `ItemResourceComponentParams` carries the
//! resource-network profile: what the item draws and generates while online and the EM/IR
//! signature it emits doing so. Health, mass and distortion ceiling each live on their own
//! struct.
//!
//! Rates come in two spellings. Weapons and most generators quote
//! `standardResourceUnits`; power plants, coolers and shields quote `units`. Both are the
//! same per-second figure.

use super::{at_map, compact_obj, num_v, number, round_to};
use crate::entity::{component, referenced_name};
use crate::localization::{LocRefs, Localization, is_placeholder_name, loc_refs, localize};
use crate::records::{GameRecord, array, at, num, str_};
use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq)]
pub struct ItemIdentity {
    pub name: String,
    /// True when the record has no real display name, i.e. a template or stub.
    pub is_template: Option<bool>,
    /// String-table keys of the name and description, and the name's status.
    pub loc: LocRefs,
    pub ty: Option<String>,
    pub sub_type: Option<String>,
    pub size: Option<f64>,
    pub grade: Option<f64>,
    pub manufacturer: Option<String>,
}

static EMPTY: std::sync::LazyLock<Map<String, Value>> = std::sync::LazyLock::new(Map::new);

/// `attachDef`: `SAttachableComponentParams.AttachDef`, or an empty object.
pub fn attach_def(record: &GameRecord) -> &Map<String, Value> {
    component(record, "SAttachableComponentParams")
        .and_then(|a| a.get("AttachDef"))
        .and_then(Value::as_object)
        .unwrap_or(&EMPTY)
}

pub fn manufacturer_of(def: &Map<String, Value>) -> Option<String> {
    referenced_name(def.get("Manufacturer")).map(str::to_uppercase)
}

pub fn item_identity(record: &GameRecord, localization: &Localization) -> ItemIdentity {
    let def = attach_def(record);
    let localized = localize(localization, at_map(def, "Localization.Name"));
    let placeholder = is_placeholder_name(localized);
    let sub_type = str_(def.get("SubType")).filter(|s| *s != "UNDEFINED");
    ItemIdentity {
        name: if placeholder {
            record.short_name.clone()
        } else {
            localized.unwrap_or_default().to_string()
        },
        is_template: placeholder.then_some(true),
        loc: loc_refs(
            localization,
            at_map(def, "Localization.Name"),
            at_map(def, "Localization.Description"),
        ),
        ty: str_(def.get("Type")).map(str::to_string),
        sub_type: sub_type.map(str::to_string),
        size: num(def.get("Size")),
        grade: num(def.get("Grade")),
        manufacturer: manufacturer_of(def),
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ResourceProfile {
    /// Power drawn per second while online.
    pub power: Option<f64>,
    pub em: Option<f64>,
    pub ir: Option<f64>,
    /// Everything the item generates while online, keyed by resource (insertion order).
    pub generation: Option<Vec<(String, f64)>>,
    /// Everything the item consumes while online, keyed by resource.
    pub consumption: Option<Vec<(String, f64)>>,
}

impl ResourceProfile {
    pub fn generated(&self, resource: &str) -> Option<f64> {
        self.generation
            .as_ref()?
            .iter()
            .find(|(k, _)| k == resource)
            .map(|(_, v)| *v)
    }
}

fn rate_of(value: Option<&Value>) -> Option<f64> {
    let v = value?;
    num(at(v, "resourceAmountPerSecond.standardResourceUnits"))
        .or_else(|| num(at(v, "resourceAmountPerSecond.units")))
}

fn add(bucket: &mut Vec<(String, f64)>, resource: &str, rate: f64) {
    match bucket.iter_mut().find(|(k, _)| k == resource) {
        Some((_, v)) => *v += rate,
        None => bucket.push((resource.to_string(), rate)),
    }
}

/// Draw, output and signatures from the resource-network `Online` state.
pub fn resource_profile(record: &GameRecord) -> ResourceProfile {
    let resources = component(record, "ItemResourceComponentParams");
    let states = array(resources.and_then(|r| r.get("states")));
    let online = states
        .iter()
        .find(|s| s.get("name").and_then(Value::as_str) == Some("Online"))
        .or_else(|| states.first());
    let Some(state) = online.filter(|s| s.is_object()) else {
        return ResourceProfile::default();
    };

    let mut generation = Vec::new();
    let mut consumption = Vec::new();
    for delta in array(state.get("deltas")) {
        if !delta.is_object() {
            continue;
        }
        for (side, bucket) in [
            ("generation", &mut generation),
            ("consumption", &mut consumption),
        ] {
            let block = delta.get(side);
            let resource = str_(block.and_then(|b| b.get("resource")));
            let rate = rate_of(block);
            if let (Some(resource), Some(rate)) = (resource, rate) {
                add(bucket, resource, rate);
            }
        }
    }

    // A JS object built by assignment: integer-like keys would enumerate first. Resource
    // names are never integer-like.
    let power = consumption
        .iter()
        .find(|(k, _)| k == "Power")
        .map(|(_, v)| *v);
    ResourceProfile {
        power,
        em: num(at(state, "signatureParams.EMSignature.nominalSignature")),
        ir: num(at(state, "signatureParams.IRSignature.nominalSignature")),
        generation: (!generation.is_empty()).then_some(generation),
        consumption: (!consumption.is_empty()).then_some(consumption),
    }
}

fn comp_num(record: &GameRecord, ty: &str, path: &str) -> Option<f64> {
    num(component(record, ty).and_then(|c| at_map(c, path)))
}

pub fn health_of(record: &GameRecord) -> Option<f64> {
    comp_num(record, "SHealthComponentParams", "Health")
}

pub fn mass_of(record: &GameRecord) -> Option<f64> {
    comp_num(record, "SEntityPhysicsControllerParams", "PhysType.Mass")
}

/// Distortion the item absorbs before it shuts down.
pub fn distortion_max_of(record: &GameRecord) -> Option<f64> {
    comp_num(record, "SDistortionParams", "Maximum")
}

/// The game's damage types, in the order its resistance arrays use.
pub const DAMAGE_TYPES: [&str; 6] = [
    "physical",
    "energy",
    "distortion",
    "thermal",
    "biochemical",
    "stun",
];

/// `\w+` in a JavaScript regex without the `u` flag: ASCII word characters.
fn is_word(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// `damageTypeMap`: a `{ DamagePhysical: 0.75, ... }` struct as a lower-case map.
pub fn damage_type_map(value: Option<&Value>) -> Option<Map<String, Value>> {
    let obj = value?.as_object()?;
    let mut out = Map::new();
    for (key, entry) in obj {
        if let Some(rest) = key.strip_prefix("Damage")
            && is_word(rest)
            && entry.is_number()
        {
            out.insert(rest.to_ascii_lowercase(), entry.clone());
        }
    }
    (!out.is_empty()).then_some(out)
}

/// `damageTypeRanges`: a positional `[{Min, Max}, ...]` array keyed by damage type.
pub fn damage_type_ranges(value: Option<&Value>) -> Option<Map<String, Value>> {
    let list = value?.as_array()?;
    let mut out = Map::new();
    for (index, entry) in list.iter().enumerate() {
        let min = num(at(entry, "Min"));
        let max = num(at(entry, "Max"));
        if let (Some(ty), Some(min), Some(max)) = (DAMAGE_TYPES.get(index), min, max) {
            let mut range = Map::new();
            range.insert("min".into(), number(min));
            range.insert("max".into(), number(max));
            out.insert((*ty).to_string(), Value::Object(range));
        }
    }
    (!out.is_empty()).then_some(out)
}

/// `healthResistances`: multipliers from a `DamageResistances` table that differ from 1.
pub fn health_resistances(table: Option<&Value>) -> Option<Map<String, Value>> {
    let obj = table?.as_object()?;
    let mut out = Map::new();
    for (key, entry) in obj {
        let prefix = key.strip_suffix("Resistance").filter(|p| is_word(p));
        let multiplier = num(at(entry, "Multiplier"));
        if let (Some(p), Some(m)) = (prefix, multiplier)
            && m != 1.0
        {
            out.insert(p.to_ascii_lowercase(), number(m));
        }
    }
    (!out.is_empty()).then_some(out)
}

/// `durabilityOf`: health, resistances, self-repair, wear and misfire in one block.
pub fn durability_of(record: &GameRecord) -> Option<Map<String, Value>> {
    let health = component(record, "SHealthComponentParams");
    let resources = component(record, "ItemResourceComponentParams");
    // The TypeScript first tries `at(_, "accumulators[0]")`, which never matches (no such
    // key), then takes the array's first element.
    let wear = component(record, "SDegradationParams")
        .and_then(|d| d.get("accumulators"))
        .and_then(Value::as_array)
        .and_then(|a| a.first());
    let misfire = component(record, "EntityComponentMisfireParams");
    let r = |path: &str| num(resources.and_then(|r| at_map(r, path)));
    let w = |path: &str| num(wear.and_then(|w| at(w, path)));
    let m = |k: &str| num(misfire.and_then(|m| m.get(k)));

    compact_obj(vec![
        ("health", num_v(num(health.and_then(|h| h.get("Health"))))),
        (
            "resistances",
            health_resistances(health.and_then(|h| h.get("DamageResistances"))).map(Value::Object),
        ),
        (
            "selfRepair",
            compact_obj(vec![
                ("count", num_v(r("selfRepair.maxRepairCount"))),
                ("seconds", num_v(r("selfRepair.timeToRepair"))),
                ("healthRatio", num_v(r("selfRepair.healthRatio"))),
            ])
            .map(Value::Object),
        ),
        ("repairRestoreRatio", num_v(r("repairRestoreRatio"))),
        (
            "wear",
            compact_obj(vec![
                ("maxLifetimeHours", num_v(w("MaxLifetimeHours"))),
                ("damageConversionRate", num_v(w("DamageConversionRate"))),
                (
                    "overheatMultiplier",
                    num_v(w("HeatMultipliers.OverheatTemperatureMultiplier")),
                ),
            ])
            .map(Value::Object),
        ),
        (
            "misfireWindow",
            compact_obj(vec![
                ("min", num_v(m("minWindowLength"))),
                ("max", num_v(m("maxWindowLength"))),
            ])
            .map(Value::Object),
        ),
    ])
}

/// Inventory volume the item occupies, in micro-SCU.
pub fn volume_of(record: &GameRecord) -> Option<f64> {
    num(at_map(
        attach_def(record),
        "inventoryOccupancyVolume.microSCU",
    ))
}

/// `distortionOf`: how distortion builds and drains on an item.
pub fn distortion_of(record: &GameRecord) -> Option<Map<String, Value>> {
    let params = component(record, "SDistortionParams")?;
    let g = |k: &str| num(params.get(k));
    let max = g("Maximum");
    let decay_delay = g("DecayDelay");
    let decay_rate = g("DecayRate");
    let recovery_ratio = g("RecoveryRatio");
    let drain = max.map(|m| m * (1.0 - recovery_ratio.unwrap_or(0.0)));
    let seconds_to_recover = match (drain, decay_rate) {
        (Some(drain), Some(rate)) if rate != 0.0 => {
            Some(round_to(decay_delay.unwrap_or(0.0) + drain / rate, 100.0))
        }
        _ => None,
    };
    compact_obj(vec![
        ("max", num_v(max)),
        ("decayDelay", num_v(decay_delay)),
        ("decayRate", num_v(decay_rate)),
        ("warningRatio", num_v(g("WarningRatio"))),
        ("recoveryRatio", num_v(recovery_ratio)),
        ("secondsToRecover", num_v(seconds_to_recover)),
    ])
}

#[cfg(test)]
mod tests {
    // Cases from sc-gamedata src/domains/item-common.test.ts.
    use super::*;
    use crate::records::normalize_record;
    use serde_json::json;

    fn item(components: Value) -> GameRecord {
        normalize_record(
            json!({"_RecordName_": "EntityClassDefinition.X", "_RecordId_": "x",
                "_RecordValue_": {"_Type_": "EntityClassDefinition", "Components": components}}),
            "libs/foundry/records/entities/scitem/ships/x/x",
        )
        .unwrap()
    }

    #[test]
    fn resource_profile_reads_the_online_state_in_either_unit_spelling() {
        let r = item(json!([{"_Type_": "ItemResourceComponentParams", "states": [
            {"name": "Offline", "deltas": []},
            {"name": "Online", "deltas": [
                {"consumption": {"resource": "Power", "resourceAmountPerSecond": {"units": 3}},
                 "generation": {"resource": "Coolant", "resourceAmountPerSecond": {"standardResourceUnits": 34}}},
                {"consumption": {"resource": "Coolant", "resourceAmountPerSecond": {"standardResourceUnits": 0}}}],
             "signatureParams": {"EMSignature": {"nominalSignature": 1490}, "IRSignature": {"nominalSignature": 7260}}}]}]));
        let p = resource_profile(&r);
        assert_eq!(p.power, Some(3.0));
        assert_eq!((p.em, p.ir), (Some(1490.0), Some(7260.0)));
        assert_eq!(p.generation, Some(vec![("Coolant".into(), 34.0)]));
        assert_eq!(
            p.consumption,
            Some(vec![("Power".into(), 3.0), ("Coolant".into(), 0.0)])
        );
        assert_eq!(
            resource_profile(&item(json!([]))),
            ResourceProfile::default()
        );
    }

    #[test]
    fn durability_rolls_everything_into_one_block() {
        let r = item(json!([
            {"_Type_": "SHealthComponentParams", "Health": 210, "DamageResistances": {
                "PhysicalResistance": {"Multiplier": 0.9}, "EnergyResistance": {"Multiplier": 1},
                "ThermalResistance": {"Multiplier": 0.1}}},
            {"_Type_": "ItemResourceComponentParams", "states": [],
                "selfRepair": {"maxRepairCount": 1, "timeToRepair": 40, "healthRatio": 0.2}, "repairRestoreRatio": 0.1},
            {"_Type_": "SDegradationParams", "accumulators": [{"MaxLifetimeHours": 0, "DamageConversionRate": 0.5,
                "HeatMultipliers": {"OverheatTemperatureMultiplier": 2}}]},
            {"_Type_": "EntityComponentMisfireParams", "minWindowLength": 45, "maxWindowLength": 600}
        ]));
        assert_eq!(
            Value::Object(durability_of(&r).unwrap()),
            json!({"health": 210.0, "resistances": {"physical": 0.9, "thermal": 0.1},
                "selfRepair": {"count": 1.0, "seconds": 40.0, "healthRatio": 0.2}, "repairRestoreRatio": 0.1,
                "wear": {"maxLifetimeHours": 0.0, "damageConversionRate": 0.5, "overheatMultiplier": 2.0},
                "misfireWindow": {"min": 45.0, "max": 600.0}})
        );
        assert!(durability_of(&item(json!([]))).is_none());
    }

    #[test]
    fn damage_type_tables() {
        assert_eq!(
            damage_type_map(Some(
                &json!({"DamagePhysical": 0.75, "DamageEnergy": 0.6, "_Type_": "x", "Damage": 1})
            ))
            .map(Value::Object),
            Some(json!({"physical": 0.75, "energy": 0.6}))
        );
        assert_eq!(
            damage_type_ranges(Some(
                &json!([{"Min": 0, "Max": 0.25}, {"Min": 0, "Max": 0}, {"Min": 0.75, "Max": 0.95}])
            ))
            .map(Value::Object),
            Some(
                json!({"physical": {"min": 0.0, "max": 0.25}, "energy": {"min": 0.0, "max": 0.0},
                "distortion": {"min": 0.75, "max": 0.95}})
            )
        );
        assert!(damage_type_ranges(Some(&json!("nope"))).is_none());
    }

    #[test]
    fn distortion_pool_and_drain_time() {
        let r = item(
            json!([{"_Type_": "SDistortionParams", "Maximum": 4050, "DecayDelay": 1.5,
            "DecayRate": 270, "WarningRatio": 0.75, "RecoveryRatio": 0}]),
        );
        assert_eq!(
            Value::Object(distortion_of(&r).unwrap()),
            json!({"max": 4050.0, "decayDelay": 1.5, "decayRate": 270.0, "warningRatio": 0.75,
                "recoveryRatio": 0.0, "secondsToRecover": 16.5})
        );
        let half = item(
            json!([{"_Type_": "SDistortionParams", "Maximum": 1000, "DecayDelay": 0,
            "DecayRate": 100, "RecoveryRatio": 0.5}]),
        );
        assert_eq!(distortion_of(&half).unwrap()["secondsToRecover"], 5.0);
        assert!(distortion_of(&item(json!([]))).is_none());
    }

    #[test]
    fn volume_in_micro_scu() {
        let r = item(json!([{"_Type_": "SAttachableComponentParams",
            "AttachDef": {"inventoryOccupancyVolume": {"microSCU": 4200000}}}]));
        assert_eq!(volume_of(&r), Some(4200000.0));
    }
}
