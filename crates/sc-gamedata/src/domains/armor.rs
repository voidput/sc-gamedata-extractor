//! Personal armour and clothing: what a character wears, and what it protects against
//! (port of `sc-gamedata/src/domains/armor.ts`).
//!
//! Armour pieces carry `SCItemSuitArmorParams`, which points at a `DamageResistanceMacro`
//! record by path (`damage/lightarmor.json`) rather than holding resistances itself. A
//! multiplier is the share of damage that gets through, so 0.8 blocks 20%. Armour and
//! clothing alike carry `SCItemClothingParams` (temperature range, radiation, g-force);
//! storage is a reference to an inventory container, resolved here into a capacity.

use crate::collate::compare_names;
use crate::domain::{Domain, DomainError, ParseContext, field};
use crate::entity::{component, referenced_name};
use crate::helpers_items::item_common::{item_identity, mass_of};
use crate::helpers_items::{number, truthy};
use crate::localization::Localization;
use crate::records::{GameRecord, RecordSet, at, num, str_};
use serde::Serialize;
use serde_json::{Map, Value};

pub static DOMAIN: Armor = Armor;

pub struct Armor;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Resistance {
    pub multiplier: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub threshold: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub damage_cap: Option<f64>,
}

/// One row of `armor` or `clothing`. Field order is output order.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WornItem {
    pub name: String,
    pub record_name: String,
    pub id: String,
    pub slot: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weight: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manufacturer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grade: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mass: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature_min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature_max: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub radiation_capacity: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub radiation_dissipation_rate: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub g_force_resistance: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage_micro_scu: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resistance_profile: Option<String>,
    /// By damage type, in [`RESISTANCE_FIELDS`] order.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resistances: Option<Map<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub impact_force_resistance: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub em_signature: Option<f64>,
}

const RESISTANCE_FIELDS: [(&str, &str); 6] = [
    ("physical", "PhysicalResistance"),
    ("energy", "EnergyResistance"),
    ("distortion", "DistortionResistance"),
    ("thermal", "ThermalResistance"),
    ("biochemical", "BiochemicalResistance"),
    ("stun", "StunResistance"),
];

/// `parseResistances`: `(resistances, impactForceResistance)` of a resistance macro.
pub fn parse_resistances(macro_: Option<&GameRecord>) -> (Option<Map<String, Value>>, Option<f64>) {
    let Some(m) = macro_ else {
        return (None, None);
    };
    let table = m.value.get("damageResistance");
    let mut resistances = Map::new();
    for (key, field) in RESISTANCE_FIELDS {
        let entry = table.and_then(|t| t.get(field));
        let Some(multiplier) = num(entry.and_then(|e| at(e, "Multiplier"))) else {
            continue;
        };
        // Threshold and cap are zero on every profile seen so far; kept only when set.
        let set = |name: &str| num(entry.and_then(|e| at(e, name))).filter(|x| truthy(Some(*x)));
        let r = Resistance {
            multiplier,
            threshold: set("Threshold"),
            damage_cap: set("DamageCap"),
        };
        resistances.insert(
            key.to_string(),
            serde_json::to_value(r).unwrap_or(Value::Null),
        );
    }
    (
        (!resistances.is_empty()).then_some(resistances),
        num(at(&m.value, "impactForceResistance.impactForceResistance")),
    )
}

/// `containerMicroScu`: capacity of an inventory container record, in micro-SCU.
pub fn container_micro_scu(container: Option<&GameRecord>) -> Option<f64> {
    let capacity = at(&container?.value, "inventoryType.capacity")?;
    if let Some(micro) = num(at(capacity, "microSCU")) {
        return Some(micro);
    }
    if let Some(centi) = num(at(capacity, "centiSCU")) {
        return Some(centi * 10_000.0);
    }
    num(at(capacity, "standardCargoUnits")).map(|scu| scu * 1_000_000.0)
}

/// `slotOf`: "Char_Armor_Torso" -> "Core", "Char_Clothing_Torso_1" -> "Torso_1".
pub fn slot_of(ty: Option<&str>) -> String {
    let Some(ty) = ty.filter(|t| !t.is_empty()) else {
        return "Unknown".into();
    };
    if ty == "Char_Armor_Torso" {
        return "Core".into();
    }
    ty.strip_prefix("Char_Armor_")
        .or_else(|| ty.strip_prefix("Char_Clothing_"))
        .unwrap_or(ty)
        .to_string()
}

pub fn parse_worn_item(
    record: &GameRecord,
    records: &RecordSet,
    localization: &Localization,
) -> Option<WornItem> {
    let identity = item_identity(record, localization);
    if identity.is_template.is_some() {
        return None;
    }
    let clothing = component(record, "SCItemClothingParams");
    let armor = component(record, "SCItemSuitArmorParams");
    let inventory = component(record, "SCItemInventoryContainerComponentParams");
    let lookup = |r: Option<&Value>| {
        let path = crate::records::ref_path(r)?;
        records.by_record_path(&path)
    };

    let em = armor
        .and_then(|a| a.get("signatureParams"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|s| s.get("signatureType").and_then(Value::as_str) == Some("Electromagnetic"));
    let damage_ref = armor.and_then(|a| a.get("damageResistance"));
    let macro_ = lookup(damage_ref);
    let (resistances, impact) = parse_resistances(macro_);
    let cloth = |dotted: &str| {
        let (first, rest) = dotted.split_once('.').unwrap_or((dotted, ""));
        num(clothing
            .and_then(|c| c.get(first))
            .and_then(|v| at(v, rest)))
    };

    Some(WornItem {
        name: identity.name,
        record_name: record.name.clone(),
        id: record.id.clone(),
        slot: slot_of(identity.ty.as_deref()),
        weight: identity.sub_type,
        manufacturer: identity.manufacturer,
        grade: identity.grade,
        mass: mass_of(record),
        temperature_min: cloth("TemperatureResistance.MinResistance"),
        temperature_max: cloth("TemperatureResistance.MaxResistance"),
        radiation_capacity: cloth("RadiationResistance.MaximumRadiationCapacity"),
        radiation_dissipation_rate: cloth("RadiationResistance.RadiationDissipationRate"),
        g_force_resistance: cloth("Flight.gForceResistance"),
        storage_micro_scu: container_micro_scu(lookup(
            inventory.and_then(|i| i.get("containerParams")),
        )),
        resistance_profile: match macro_ {
            Some(m) => Some(m.short_name.clone()),
            None => referenced_name(damage_ref).map(str::to_string),
        },
        resistances,
        impact_force_resistance: impact,
        em_signature: num(em.and_then(|e| e.get("signatureEmission"))),
    })
}

#[derive(Debug, Serialize)]
struct Summary {
    armor: usize,
    clothing: usize,
}

/// `count`: items per slot, in first-seen order.
fn count(items: &[WornItem]) -> Map<String, Value> {
    let mut out = Map::new();
    for w in items {
        let n = out.get(&w.slot).and_then(Value::as_f64).unwrap_or(0.0) + 1.0;
        out.insert(w.slot.clone(), number(n));
    }
    out
}

impl Domain for Armor {
    fn name(&self) -> &'static str {
        "game-armor"
    }

    fn description(&self) -> &'static str {
        "Personal armour, undersuits, helmets, backpacks and clothing with resistances, temperature range and storage."
    }

    fn filters(&self) -> &'static [&'static str] {
        &[
            "**/entities/scitem/characters/human/armor/**",
            "**/entities/scitem/characters/human/starwear/**",
            "**/entities/scitem/characters/human/clothing/**",
            "**/damage/**",
            "**/inventorycontainers/**",
        ]
    }

    fn needs_localization(&self) -> bool {
        true
    }

    /// Worn items keep the five components [`parse_worn_item`] reads (3.0 GB whole on LIVE).
    fn prune(&self, record: &mut GameRecord) {
        if record.ty == "EntityClassDefinition"
            && record.record_path.contains("/entities/scitem/characters/")
        {
            crate::entity::prune_entity(
                &mut record.value,
                &[],
                &[
                    ("SAttachableComponentParams", Some(&["AttachDef"])),
                    ("SCItemClothingParams", None),
                    ("SCItemSuitArmorParams", None),
                    ("SCItemInventoryContainerComponentParams", None),
                    ("SEntityPhysicsControllerParams", Some(&["PhysType"])),
                ],
            );
        }
    }

    fn parse(&self, ctx: &ParseContext<'_>) -> Result<Map<String, Value>, DomainError> {
        // Armour and clothing share slot names (both have Legs), so the type prefix decides
        // which list an item belongs to, not the slot.
        let mut armor = Vec::new();
        let mut clothing = Vec::new();
        for record in ctx.records.iter() {
            if !record.record_path.contains("/entities/scitem/characters/") {
                continue;
            }
            let ty = str_(
                component(record, "SAttachableComponentParams")
                    .and_then(|c| c.get("AttachDef"))
                    .and_then(|d| at(d, "Type")),
            );
            let target = match ty {
                Some(t) if t.starts_with("Char_Armor_") => &mut armor,
                Some(t) if t.starts_with("Char_Clothing_") => &mut clothing,
                _ => continue,
            };
            if let Some(item) = parse_worn_item(record, ctx.records, ctx.localization) {
                target.push(item);
            }
        }
        let order = |a: &WornItem, b: &WornItem| {
            compare_names(&a.slot, &b.slot).then_with(|| compare_names(&a.name, &b.name))
        };
        armor.sort_by(order);
        clothing.sort_by(order);

        let mut by_slot = Map::new();
        by_slot.insert("armor".into(), Value::Object(count(&armor)));
        by_slot.insert("clothing".into(), Value::Object(count(&clothing)));

        let mut body = Map::new();
        field(&mut body, "armor", &armor)?;
        field(&mut body, "clothing", &clothing)?;
        body.insert("bySlot".into(), Value::Object(by_slot));
        field(
            &mut body,
            "summary",
            &Summary {
                armor: armor.len(),
                clothing: clothing.len(),
            },
        )?;
        Ok(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::localization::parse_global_ini;
    use crate::records::normalize_record;
    use serde_json::json;

    fn rec(ty: &str, path: &str, value: Value) -> GameRecord {
        let short = path.rsplit('/').next().unwrap();
        let mut v = json!({"_Type_": ty});
        if let (Some(o), Value::Object(extra)) = (v.as_object_mut(), value) {
            o.extend(extra);
        }
        normalize_record(
            json!({"_RecordName_": format!("{ty}.{short}"), "_RecordId_": format!("id-{short}"), "_RecordValue_": v}),
            &format!("libs/foundry/records/{path}"),
        )
        .unwrap()
    }

    const ROOT: &str = "file://./../../../libs/foundry/records/";

    fn worn(path: &str, ty: &str, name: &str, extra: Value) -> GameRecord {
        let mut components = vec![json!({"_Type_": "SAttachableComponentParams", "AttachDef": {
            "Type": ty, "SubType": "Light", "Grade": 1,
            "Manufacturer": "file://./x/scitemmanufacturer.cds.json",
            "Localization": {"Name": name}}})];
        if let Value::Array(more) = extra {
            components.extend(more);
        }
        rec(
            "EntityClassDefinition",
            path,
            json!({ "Components": components }),
        )
    }

    #[test]
    fn slots() {
        assert_eq!(slot_of(Some("Char_Armor_Torso")), "Core");
        assert_eq!(slot_of(Some("Char_Clothing_Torso_1")), "Torso_1");
        assert_eq!(slot_of(Some("Char_Armor_Legs")), "Legs");
        assert_eq!(slot_of(None), "Unknown");
    }

    #[test]
    fn container_capacity_in_any_unit() {
        let c = |cap: Value| {
            rec(
                "InventoryContainer",
                "inventorycontainers/x",
                json!({"inventoryType": {"capacity": cap}}),
            )
        };
        assert_eq!(container_micro_scu(Some(&c(json!({"microSCU": 5})))), Some(5.0));
        assert_eq!(container_micro_scu(Some(&c(json!({"centiSCU": 2})))), Some(20_000.0));
        assert_eq!(
            container_micro_scu(Some(&c(json!({"standardCargoUnits": 1})))),
            Some(1_000_000.0)
        );
        assert_eq!(container_micro_scu(None), None);
    }

    #[test]
    fn resolves_resistances_storage_and_splits_armour_from_clothing() {
        let loc = parse_global_ini("item_core=ORC-mkX Core\nitem_pants=Pants\nitem_arms=ORC Arms");
        let macro_ = rec(
            "DamageResistanceMacro",
            "damage/lightarmor",
            json!({"damageResistance": {
                "PhysicalResistance": {"Multiplier": 0.8, "Threshold": 0, "DamageCap": 0},
                "EnergyResistance": {"Multiplier": 0.9, "Threshold": 5}},
                "impactForceResistance": {"impactForceResistance": 0.3}}),
        );
        let bag = rec(
            "InventoryContainer",
            "inventorycontainers/core_light",
            json!({"inventoryType": {"capacity": {"centiSCU": 100}}}),
        );
        let core = worn(
            "entities/scitem/characters/human/armor/core",
            "Char_Armor_Torso",
            "@item_core",
            json!([
                {"_Type_": "SCItemSuitArmorParams",
                 "damageResistance": format!("{ROOT}damage/LightArmor.json"),
                 "signatureParams": [{"signatureType": "Infrared", "signatureEmission": 1},
                                     {"signatureType": "Electromagnetic", "signatureEmission": 2}]},
                {"_Type_": "SCItemClothingParams",
                 "TemperatureResistance": {"MinResistance": -30, "MaxResistance": 50}},
                {"_Type_": "SCItemInventoryContainerComponentParams",
                 "containerParams": format!("{ROOT}inventorycontainers/core_light.json")},
                {"_Type_": "SEntityPhysicsControllerParams", "PhysType": {"Mass": 9}}
            ]),
        );
        let arms = worn(
            "entities/scitem/characters/human/armor/arms",
            "Char_Armor_Arms",
            "@item_arms",
            json!([{"_Type_": "SCItemSuitArmorParams",
                    "damageResistance": format!("{ROOT}damage/missing.json")}]),
        );
        let pants = worn(
            "entities/scitem/characters/human/clothing/pants",
            "Char_Clothing_Legs",
            "@item_pants",
            json!([]),
        );
        let template = worn(
            "entities/scitem/characters/human/armor/stub",
            "Char_Armor_Legs",
            "@missing",
            json!([]),
        );
        let set = RecordSet::new(vec![macro_, bag, core, arms, pants, template]);
        let body = crate::fixtures::parse_checked(&DOMAIN, &set, &loc);
        assert_eq!(
            body["armor"][1],
            json!({
                "name": "ORC-mkX Core", "recordName": "EntityClassDefinition.core", "id": "id-core",
                "slot": "Core", "weight": "Light", "manufacturer": "CDS", "grade": 1.0, "mass": 9.0,
                "temperatureMin": -30.0, "temperatureMax": 50.0, "storageMicroScu": 1_000_000.0,
                "resistanceProfile": "lightarmor",
                "resistances": {"physical": {"multiplier": 0.8}, "energy": {"multiplier": 0.9, "threshold": 5.0}},
                "impactForceResistance": 0.3, "emSignature": 2.0
            })
        );
        assert_eq!(body["armor"][0]["slot"], "Arms");
        assert_eq!(body["armor"][0]["resistanceProfile"], "missing");
        assert_eq!(body["clothing"][0]["slot"], "Legs");
        assert_eq!(
            body["bySlot"],
            json!({"armor": {"Arms": 1.0, "Core": 1.0}, "clothing": {"Legs": 1.0}})
        );
        assert_eq!(body["summary"], json!({"armor": 2, "clothing": 1}));
    }
}
