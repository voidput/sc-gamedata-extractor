//! Shared helpers of the item domains (`ammo`, `armor`, `consumables`, `fps_weapons`,
//! `item_sources`, `item_tags`, `weapons`): ports of `sc-gamedata/src/domains/item-common.ts`
//! and `src/tags.ts` (`buildTagIndex`), plus JavaScript's `Math.round`.
//!
//! Kept in one batch file so this port does not collide with other batches; a later step
//! can move [`item_common`] and [`tags`] into the registry's helper slots.

pub use crate::js::{js_round, number, round_to, truthy};

const NBSP: char = '\u{a0}';

/// `a.localeCompare(b)`: [`crate::collate::locale_compare`] plus the no-break space, which
/// game names carry (`Arrowhead\u{a0}"Pathfinder"`). ICU gives U+00A0 the primary and
/// secondary weight of a space and a higher tertiary one, so it sorts with the space and
/// only breaks a tie (left to right, with case). `collate` puts it before every space.
/// Belongs in `collate` itself; kept here so this batch touches no shared module.
pub fn locale_compare(a: &str, b: &str) -> std::cmp::Ordering {
    use crate::collate::locale_compare as icu;
    use std::cmp::Ordering;
    if !a.contains(NBSP) && !b.contains(NBSP) {
        return icu(a, b);
    }
    let a2 = a.replace(NBSP, " ");
    let b2 = b.replace(NBSP, " ");
    // Primary and secondary levels: case only differs at the tertiary one.
    let base = icu(&a2.to_lowercase(), &b2.to_lowercase());
    if base != Ordering::Equal {
        return base;
    }
    let tertiary = |c: char| u8::from(c == NBSP || c.is_uppercase());
    a.chars()
        .map(tertiary)
        .cmp(b.chars().map(tertiary))
        .then_with(|| icu(&a2, &b2))
}

/// `/^<prefix>(\w+)$/` and `/^(\w+)<suffix>$/`: the captured part, when non-empty and all
/// ASCII word characters.
fn word(s: &str) -> Option<&str> {
    (!s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')).then_some(s)
}

/// Port of `domains/item-common.ts`.
pub mod item_common {
    use super::{js_round, number, truthy, word};
    use crate::entity::{component, referenced_name};
    use crate::localization::{LocRefs, Localization, is_placeholder_name, loc_refs, localize};
    use crate::records::{GameRecord, at, num, str_};
    use serde::Serialize;
    use serde_json::{Map, Value};
    use std::sync::OnceLock;

    fn empty_object() -> &'static Value {
        static EMPTY: OnceLock<Value> = OnceLock::new();
        EMPTY.get_or_init(|| Value::Object(Map::new()))
    }

    /// `ItemIdentity`. Field order is output order.
    #[derive(Debug, Clone, PartialEq, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct ItemIdentity {
        pub name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub is_template: Option<bool>,
        /// `nameKey`, `descriptionKey`, `nameStatus`.
        #[serde(flatten)]
        pub loc: LocRefs,
        #[serde(skip_serializing_if = "Option::is_none", rename = "type")]
        pub ty: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub sub_type: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub size: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub grade: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub manufacturer: Option<String>,
    }

    /// `attachDef`: `SAttachableComponentParams.AttachDef`, or an empty object.
    pub fn attach_def(record: &GameRecord) -> &Value {
        component(record, "SAttachableComponentParams")
            .and_then(|c| c.get("AttachDef"))
            .filter(|d| d.is_object() || d.is_array())
            .unwrap_or_else(|| empty_object())
    }

    /// `(attach?.AttachDef ?? {})`, as `fps-weapons.ts` and `weapons.ts` read it.
    pub fn attach_def_or_empty(record: &GameRecord) -> &Value {
        component(record, "SAttachableComponentParams")
            .and_then(|c| c.get("AttachDef"))
            .filter(|d| !d.is_null())
            .unwrap_or_else(|| empty_object())
    }

    /// `manufacturerOf`: the referenced manufacturer record's name, upper case.
    pub fn manufacturer_of(def: &Value) -> Option<String> {
        referenced_name(def.get("Manufacturer")).map(str::to_uppercase)
    }

    /// `str(def.SubType) === "UNDEFINED" ? undefined : str(def.SubType)`.
    pub fn sub_type_of(def: &Value) -> Option<String> {
        str_(def.get("SubType"))
            .filter(|s| *s != "UNDEFINED")
            .map(str::to_string)
    }

    /// The localized display name, and whether it is a placeholder.
    pub fn display_name<'a>(
        def: &'a Value,
        localization: &'a Localization,
    ) -> (Option<&'a str>, bool) {
        let localized = localize(localization, at(def, "Localization.Name"));
        (localized, is_placeholder_name(localized))
    }

    pub fn item_identity(record: &GameRecord, localization: &Localization) -> ItemIdentity {
        let def = attach_def(record);
        let (localized, placeholder) = display_name(def, localization);
        ItemIdentity {
            name: if placeholder {
                record.short_name.clone()
            } else {
                localized.unwrap_or_default().to_string()
            },
            is_template: placeholder.then_some(true),
            loc: loc_refs(
                localization,
                at(def, "Localization.Name"),
                at(def, "Localization.Description"),
            ),
            ty: str_(def.get("Type")).map(str::to_string),
            sub_type: sub_type_of(def),
            size: num(def.get("Size")),
            grade: num(def.get("Grade")),
            manufacturer: manufacturer_of(def),
        }
    }

    /// `ResourceProfile`.
    #[derive(Debug, Clone, Default, PartialEq, Serialize)]
    pub struct ResourceProfile {
        #[serde(skip_serializing_if = "Option::is_none")]
        pub power: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub em: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub ir: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub generation: Option<Map<String, Value>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub consumption: Option<Map<String, Value>>,
    }

    fn rate_of(value: Option<&Value>) -> Option<f64> {
        let v = value?;
        num(at(v, "resourceAmountPerSecond.standardResourceUnits"))
            .or_else(|| num(at(v, "resourceAmountPerSecond.units")))
    }

    fn add(bucket: &mut Map<String, Value>, key: &str, amount: f64) {
        let sum = bucket.get(key).and_then(Value::as_f64).unwrap_or(0.0) + amount;
        bucket.insert(key.to_string(), number(sum));
    }

    /// `resourceProfile`: draw, output and signatures from the "Online" state.
    pub fn resource_profile(record: &GameRecord) -> ResourceProfile {
        let resources = component(record, "ItemResourceComponentParams");
        let states: &[Value] = resources
            .and_then(|r| r.get("states"))
            .and_then(Value::as_array)
            .map_or(&[], Vec::as_slice);
        let online = states
            .iter()
            .find(|s| s.get("name").and_then(Value::as_str) == Some("Online"))
            .or(states.first());
        let Some(state) = online.filter(|s| s.is_object() || s.is_array()) else {
            return ResourceProfile::default();
        };

        let mut generation = Map::new();
        let mut consumption = Map::new();
        for delta in state
            .get("deltas")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
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

        ResourceProfile {
            power: consumption.get("Power").and_then(Value::as_f64),
            em: num(at(state, "signatureParams.EMSignature.nominalSignature")),
            ir: num(at(state, "signatureParams.IRSignature.nominalSignature")),
            generation: (!generation.is_empty()).then_some(generation),
            consumption: (!consumption.is_empty()).then_some(consumption),
        }
    }

    fn component_value<'a>(record: &'a GameRecord, ty: &str) -> Option<&'a Map<String, Value>> {
        component(record, ty)
    }

    fn at_map<'a>(map: Option<&'a Map<String, Value>>, dotted: &str) -> Option<&'a Value> {
        let (first, rest) = match dotted.split_once('.') {
            Some((f, r)) => (f, Some(r)),
            None => (dotted, None),
        };
        let v = map?.get(first)?;
        match rest {
            Some(r) => at(v, r),
            None => Some(v),
        }
    }

    pub fn health_of(record: &GameRecord) -> Option<f64> {
        num(at_map(
            component_value(record, "SHealthComponentParams"),
            "Health",
        ))
    }

    pub fn mass_of(record: &GameRecord) -> Option<f64> {
        num(at_map(
            component_value(record, "SEntityPhysicsControllerParams"),
            "PhysType.Mass",
        ))
    }

    /// Distortion the item absorbs before it shuts down.
    pub fn distortion_max_of(record: &GameRecord) -> Option<f64> {
        num(at_map(
            component_value(record, "SDistortionParams"),
            "Maximum",
        ))
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

    /// `damageTypeMap`: `{DamagePhysical: 0.75, ...}` as a lower-case map.
    pub fn damage_type_map(value: Option<&Value>) -> Option<Map<String, Value>> {
        let obj = value?.as_object()?;
        let mut out = Map::new();
        for (key, entry) in obj {
            if let Some(kind) = key.strip_prefix("Damage").and_then(word)
                && entry.is_number()
            {
                out.insert(kind.to_lowercase(), entry.clone());
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

    /// `healthResistances`: multipliers of a `DamageResistances` table that differ from 1.
    pub fn health_resistances(table: Option<&Value>) -> Option<Map<String, Value>> {
        let obj = table?.as_object()?;
        let mut out = Map::new();
        for (key, entry) in obj {
            let kind = key.strip_suffix("Resistance").and_then(word);
            let multiplier = num(at(entry, "Multiplier"));
            if let (Some(kind), Some(m)) = (kind, multiplier)
                && m != 1.0
            {
                out.insert(kind.to_lowercase(), number(m));
            }
        }
        (!out.is_empty()).then_some(out)
    }

    #[derive(Debug, Clone, Default, PartialEq, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct SelfRepair {
        #[serde(skip_serializing_if = "Option::is_none")]
        pub count: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub seconds: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub health_ratio: Option<f64>,
    }

    #[derive(Debug, Clone, Default, PartialEq, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Wear {
        #[serde(skip_serializing_if = "Option::is_none")]
        pub max_lifetime_hours: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub damage_conversion_rate: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub overheat_multiplier: Option<f64>,
    }

    #[derive(Debug, Clone, Default, PartialEq, Serialize)]
    pub struct MinMax {
        #[serde(skip_serializing_if = "Option::is_none")]
        pub min: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub max: Option<f64>,
    }

    /// `Durability`.
    #[derive(Debug, Clone, Default, PartialEq, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Durability {
        #[serde(skip_serializing_if = "Option::is_none")]
        pub health: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub resistances: Option<Map<String, Value>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub self_repair: Option<SelfRepair>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub repair_restore_ratio: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub wear: Option<Wear>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub misfire_window: Option<MinMax>,
    }

    /// `compact`: `None` when every field is unset.
    fn compact<T: Default + PartialEq>(value: T) -> Option<T> {
        (value != T::default()).then_some(value)
    }

    pub fn durability_of(record: &GameRecord) -> Option<Durability> {
        let health = component(record, "SHealthComponentParams");
        let resources = component(record, "ItemResourceComponentParams");
        // `at(_, "accumulators[0]")` never resolves; the array's first element does.
        let wear = component(record, "SDegradationParams")
            .and_then(|d| d.get("accumulators"))
            .and_then(Value::as_array)
            .and_then(|a| a.first());
        let misfire = component(record, "EntityComponentMisfireParams");
        let res = |dotted: &str| num(at_map(resources, dotted));
        let worn = |dotted: &str| num(wear.and_then(|w| at(w, dotted)));

        compact(Durability {
            health: num(health.and_then(|h| h.get("Health"))),
            resistances: health_resistances(health.and_then(|h| h.get("DamageResistances"))),
            self_repair: compact(SelfRepair {
                count: res("selfRepair.maxRepairCount"),
                seconds: res("selfRepair.timeToRepair"),
                health_ratio: res("selfRepair.healthRatio"),
            }),
            repair_restore_ratio: res("repairRestoreRatio"),
            wear: compact(Wear {
                max_lifetime_hours: worn("MaxLifetimeHours"),
                damage_conversion_rate: worn("DamageConversionRate"),
                overheat_multiplier: worn("HeatMultipliers.OverheatTemperatureMultiplier"),
            }),
            misfire_window: compact(MinMax {
                min: num(misfire.and_then(|m| m.get("minWindowLength"))),
                max: num(misfire.and_then(|m| m.get("maxWindowLength"))),
            }),
        })
    }

    /// Inventory volume the item occupies, in micro-SCU.
    pub fn volume_of(record: &GameRecord) -> Option<f64> {
        num(at(attach_def(record), "inventoryOccupancyVolume.microSCU"))
    }

    /// `Distortion`.
    #[derive(Debug, Clone, Default, PartialEq, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Distortion {
        #[serde(skip_serializing_if = "Option::is_none")]
        pub max: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub decay_delay: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub decay_rate: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub warning_ratio: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub recovery_ratio: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub seconds_to_recover: Option<f64>,
    }

    pub fn distortion_of(record: &GameRecord) -> Option<Distortion> {
        let params = component(record, "SDistortionParams")?;
        let max = num(params.get("Maximum"));
        let decay_delay = num(params.get("DecayDelay"));
        let decay_rate = num(params.get("DecayRate"));
        let recovery_ratio = num(params.get("RecoveryRatio"));
        let drain = max.map(|m| m * (1.0 - recovery_ratio.unwrap_or(0.0)));
        let seconds_to_recover = match (drain, decay_rate) {
            (Some(drain), Some(rate)) if truthy(Some(rate)) => {
                Some(js_round((decay_delay.unwrap_or(0.0) + drain / rate) * 100.0) / 100.0)
            }
            _ => None,
        };
        compact(Distortion {
            max,
            decay_delay,
            decay_rate,
            warning_ratio: num(params.get("WarningRatio")),
            recovery_ratio,
            seconds_to_recover,
        })
    }
}

/// Port of `src/tags.ts` (`buildTagIndex`).
pub mod tags {
    use crate::records::GameRecord;
    use serde_json::Value;
    use std::collections::HashMap;

    /// `Tag.<guid>` record name -> path below the root, e.g. "Rarity/Common".
    pub type TagIndex = HashMap<String, String>;

    fn walk(tag: &Value, path: &[String], index: &mut TagIndex) {
        let Some(obj) = tag.as_object() else {
            return;
        };
        let name = obj.get("tagName").and_then(Value::as_str).unwrap_or("?");
        // The root is always "Global"; leaving it in prefixes every path for nothing.
        let here: Vec<String> = if path.is_empty() && name == "Global" {
            Vec::new()
        } else {
            path.iter().cloned().chain([name.to_string()]).collect()
        };
        if let Some(record_name) = obj.get("_RecordName_").and_then(Value::as_str)
            && !here.is_empty()
        {
            index.insert(record_name.to_string(), here.join("/"));
        }
        for child in obj
            .get("children")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            walk(child, &here, index);
        }
    }

    pub fn build_tag_index(database: Option<&GameRecord>) -> TagIndex {
        let mut index = TagIndex::new();
        for tag in database
            .and_then(|d| d.value.get("tags"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            walk(tag, &[], &mut index);
        }
        index
    }
}

#[cfg(test)]
mod tests {
    use super::item_common::*;
    use super::tags::build_tag_index;
    use super::*;
    use crate::records::{GameRecord, normalize_record};
    use serde_json::{Value, json};

    fn item(components: Value) -> GameRecord {
        normalize_record(
            json!({"_RecordName_": "EntityClassDefinition.X", "_RecordId_": "x",
                   "_RecordValue_": {"_Type_": "EntityClassDefinition", "Components": components}}),
            "libs/foundry/records/entities/scitem/ships/x/x",
        )
        .unwrap()
    }

    fn json_of<T: serde::Serialize>(v: &T) -> Value {
        serde_json::to_value(v).unwrap()
    }

    #[test]
    fn locale_compare_sorts_no_break_spaces_like_node() {
        // Node 24 (ICU 78): [...].sort((a, b) => a.localeCompare(b))
        let mut v = vec![
            "Arrowhead\u{a0}\"Pathfinder\"",
            "Arrowhead \"Boneyard\"",
            "Arrowhead \"Pathfinder\"",
            "Arrowhead\u{a0}\"pathfinder\"",
            "arrowhead\u{a0}\"Pathfinder\"",
            "Arrowhead\u{a0}Sniper",
            "Arrowhead Sniper",
            "LH86\u{a0}\"Voyager\" Pistol",
            "LH86 \"Pathfinder\" Pistol",
        ];
        v.sort_by(|a, b| locale_compare(a, b));
        assert_eq!(
            v,
            vec![
                "Arrowhead \"Boneyard\"",
                "arrowhead\u{a0}\"Pathfinder\"",
                "Arrowhead \"Pathfinder\"",
                "Arrowhead\u{a0}\"pathfinder\"",
                "Arrowhead\u{a0}\"Pathfinder\"",
                "Arrowhead Sniper",
                "Arrowhead\u{a0}Sniper",
                "LH86 \"Pathfinder\" Pistol",
                "LH86\u{a0}\"Voyager\" Pistol",
            ]
        );
    }

    #[test]
    fn js_round_matches_math_round() {
        assert_eq!(js_round(2.5), 3.0);
        assert_eq!(js_round(-2.5), -2.0);
        assert_eq!(js_round(0.499_999_999_999_999_94), 0.0);
        assert_eq!(round_to(1537.499, 100.0), 1537.5);
        assert!(truthy(Some(1.0)) && !truthy(Some(0.0)) && !truthy(None));
    }

    // Cases from sc-gamedata src/domains/item-common.test.ts.
    #[test]
    fn resource_profile_reads_the_online_state_in_either_unit_spelling() {
        let record = item(json!([{
            "_Type_": "ItemResourceComponentParams",
            "states": [
                {"name": "Offline", "deltas": []},
                {"name": "Online", "deltas": [
                    {"consumption": {"resource": "Power", "resourceAmountPerSecond": {"units": 3}},
                     "generation": {"resource": "Coolant", "resourceAmountPerSecond": {"standardResourceUnits": 34}}},
                    {"consumption": {"resource": "Coolant", "resourceAmountPerSecond": {"standardResourceUnits": 0}}}
                ],
                 "signatureParams": {"EMSignature": {"nominalSignature": 1490}, "IRSignature": {"nominalSignature": 7260}}}
            ]
        }]));
        assert_eq!(
            json_of(&resource_profile(&record)),
            json!({"power": 3.0, "em": 1490.0, "ir": 7260.0,
                   "generation": {"Coolant": 34.0}, "consumption": {"Power": 3.0, "Coolant": 0.0}})
        );
        assert_eq!(json_of(&resource_profile(&item(json!([])))), json!({}));
    }

    #[test]
    fn durability_rolls_everything_into_one_block() {
        let record = item(json!([
            {"_Type_": "SHealthComponentParams", "Health": 210, "DamageResistances": {
                "PhysicalResistance": {"Multiplier": 0.9},
                "EnergyResistance": {"Multiplier": 1},
                "ThermalResistance": {"Multiplier": 0.1}}},
            {"_Type_": "ItemResourceComponentParams", "states": [],
             "selfRepair": {"maxRepairCount": 1, "timeToRepair": 40, "healthRatio": 0.2},
             "repairRestoreRatio": 0.1},
            {"_Type_": "SDegradationParams", "accumulators": [
                {"MaxLifetimeHours": 0, "DamageConversionRate": 0.5,
                 "HeatMultipliers": {"OverheatTemperatureMultiplier": 2}}]},
            {"_Type_": "EntityComponentMisfireParams", "minWindowLength": 45, "maxWindowLength": 600}
        ]));
        assert_eq!(
            json_of(&durability_of(&record)),
            json!({
                "health": 210.0,
                "resistances": {"physical": 0.9, "thermal": 0.1},
                "selfRepair": {"count": 1.0, "seconds": 40.0, "healthRatio": 0.2},
                "repairRestoreRatio": 0.1,
                "wear": {"maxLifetimeHours": 0.0, "damageConversionRate": 0.5, "overheatMultiplier": 2.0},
                "misfireWindow": {"min": 45.0, "max": 600.0}
            })
        );
        assert!(durability_of(&item(json!([]))).is_none());
    }

    #[test]
    fn damage_type_tables() {
        assert_eq!(
            damage_type_map(Some(
                &json!({"DamagePhysical": 0.75, "DamageEnergy": 0.6, "_Type_": "x"})
            )),
            json!({"physical": 0.75, "energy": 0.6})
                .as_object()
                .cloned()
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
    fn distortion_pool_and_time_to_recover() {
        let record = item(
            json!([{"_Type_": "SDistortionParams", "Maximum": 4050, "DecayDelay": 1.5,
                                   "DecayRate": 270, "WarningRatio": 0.75, "RecoveryRatio": 0}]),
        );
        assert_eq!(
            json_of(&distortion_of(&record)),
            json!({"max": 4050.0, "decayDelay": 1.5, "decayRate": 270.0, "warningRatio": 0.75,
                   "recoveryRatio": 0.0, "secondsToRecover": 16.5})
        );
        let half = item(
            json!([{"_Type_": "SDistortionParams", "Maximum": 1000, "DecayDelay": 0,
                                 "DecayRate": 100, "RecoveryRatio": 0.5}]),
        );
        assert_eq!(distortion_of(&half).unwrap().seconds_to_recover, Some(5.0));
        assert!(distortion_of(&item(json!([]))).is_none());
    }

    #[test]
    fn volume_in_micro_scu() {
        let record = item(json!([{"_Type_": "SAttachableComponentParams",
                                   "AttachDef": {"inventoryOccupancyVolume": {"microSCU": 4200000}}}]));
        assert_eq!(volume_of(&record), Some(4_200_000.0));
    }

    #[test]
    fn identity_falls_back_to_the_short_name() {
        let record = item(
            json!([{"_Type_": "SAttachableComponentParams", "AttachDef": {
            "Type": "WeaponGun", "SubType": "UNDEFINED", "Size": 3,
            "Manufacturer": "file://./../scitemmanufacturer.behr.json",
            "Localization": {"Name": "@missing"}}}]),
        );
        let id = item_identity(&record, &crate::localization::Localization::new());
        assert_eq!(id.name, "X");
        assert_eq!(id.is_template, Some(true));
        assert_eq!(id.sub_type, None);
        assert_eq!(id.manufacturer.as_deref(), Some("BEHR"));
        assert_eq!(health_of(&record), None);
        assert_eq!(mass_of(&record), None);
        assert_eq!(distortion_max_of(&record), None);
    }

    #[test]
    fn tag_index_drops_the_global_root() {
        let db = normalize_record(
            json!({"_RecordName_": "TagDatabase.tagdatabase", "_RecordValue_": {"_Type_": "TagDatabase",
                "tags": [{"tagName": "Global", "_RecordName_": "Tag.root", "children": [
                    {"tagName": "Rarity", "_RecordName_": "Tag.r", "children": [
                        {"tagName": "Common", "_RecordName_": "Tag.c"}, null]},
                    {"_RecordName_": "Tag.anon"}]}]}}),
            "libs/foundry/records/tagdatabase/tagdatabase",
        )
        .unwrap();
        let index = build_tag_index(Some(&db));
        assert_eq!(
            index.get("Tag.c").map(String::as_str),
            Some("Rarity/Common")
        );
        assert_eq!(index.get("Tag.r").map(String::as_str), Some("Rarity"));
        assert_eq!(index.get("Tag.anon").map(String::as_str), Some("?"));
        assert!(!index.contains_key("Tag.root"));
        assert!(build_tag_index(None).is_empty());
    }
}
