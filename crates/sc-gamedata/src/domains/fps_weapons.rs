//! FPS weapons: guns and melee (port of `sc-gamedata/src/domains/fps-weapons.ts`).
//!
//! Personal weapons use the same `SCItemWeaponComponentParams` struct as ship weapons, so
//! they are told apart by where the record lives. Melee carries
//! `SMeleeWeaponComponentParams` instead, which has no ammo. Damage is on the ammunition
//! (see `game-ammo`).
//!
//! LIVE 4.10.200.58746 versus the golden dump: identical except one row the golden has and
//! the DataCore does not, `vncl_melee_01_tint03` (melee, template; so `weapons`, `melee`
//! and `templates` count one more there). That record is a stale file from an older build
//! (dated 2026-11-01) left in sc-gamedata's `extracted-data/`, which `loadRecords` walks
//! without clearing; it is not in this build's DataCore, so the port cannot produce it.

use crate::helpers_items::locale_compare;
use crate::domain::{Domain, DomainError, ParseContext, field};
use crate::entity::component;
use crate::helpers_items::item_common::{attach_def_or_empty, display_name, sub_type_of};
use crate::localization::Localization;
use crate::records::{GameRecord, num, str_};
use serde::Serialize;
use serde_json::{Map, Value};

pub static DOMAIN: FpsWeapons = FpsWeapons;

pub struct FpsWeapons;

/// One row of `weapons`. Field order is output order.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FpsWeapon {
    pub name: String,
    pub record_name: String,
    pub id: String,
    /// `ranged` or `melee`.
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none", rename = "type")]
    pub ty: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ammo_capacity: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heat_per_shot: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_template: Option<bool>,
}

pub fn parse_fps_weapon(record: &GameRecord, localization: &Localization) -> Option<FpsWeapon> {
    let ranged = component(record, "SCItemWeaponComponentParams");
    let melee = component(record, "SMeleeWeaponComponentParams");
    let params = ranged.or(melee)?;
    let def = attach_def_or_empty(record);
    let (localized, placeholder) = display_name(def, localization);
    Some(FpsWeapon {
        name: if placeholder {
            record.short_name.clone()
        } else {
            localized.unwrap_or_default().to_string()
        },
        record_name: record.name.clone(),
        id: record.id.clone(),
        kind: if ranged.is_some() { "ranged" } else { "melee" },
        ty: str_(def.get("Type")).map(str::to_string),
        sub_type: sub_type_of(def),
        size: num(def.get("Size")),
        ammo_capacity: num(params.get("ammoContainerCapacity")),
        heat_per_shot: num(params.get("heatPerShot")),
        is_template: placeholder.then_some(true),
    })
}

#[derive(Debug, Serialize)]
struct Summary {
    weapons: usize,
    ranged: usize,
    melee: usize,
    templates: usize,
}

impl Domain for FpsWeapons {
    fn name(&self) -> &'static str {
        "game-fps-weapons"
    }

    fn description(&self) -> &'static str {
        "Personal ranged and melee weapons."
    }

    fn filters(&self) -> &'static [&'static str] {
        &["**/entities/scitem/weapons/**"]
    }

    fn needs_localization(&self) -> bool {
        true
    }

    fn parse(&self, ctx: &ParseContext<'_>) -> Result<Map<String, Value>, DomainError> {
        let mut weapons: Vec<FpsWeapon> = ctx
            .records
            .iter()
            .filter_map(|r| parse_fps_weapon(r, ctx.localization))
            .collect();
        weapons.sort_by(|a, b| locale_compare(&a.name, &b.name));
        let count = |kind: &str| weapons.iter().filter(|w| w.kind == kind).count();
        let summary = Summary {
            weapons: weapons.len(),
            ranged: count("ranged"),
            melee: count("melee"),
            templates: weapons.iter().filter(|w| w.is_template.is_some()).count(),
        };
        let mut body = Map::new();
        field(&mut body, "weapons", &weapons)?;
        field(&mut body, "summary", &summary)?;
        Ok(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::NoFiles;
    use crate::localization::parse_global_ini;
    use crate::records::{RecordSet, normalize_record};
    use serde_json::json;

    fn entity(short: &str, components: Value) -> GameRecord {
        normalize_record(
            json!({"_RecordName_": format!("EntityClassDefinition.{short}"), "_RecordId_": format!("id-{short}"),
                   "_RecordValue_": {"_Type_": "EntityClassDefinition", "Components": components}}),
            &format!("libs/foundry/records/entities/scitem/weapons/fps_weapons/{short}"),
        )
        .unwrap()
    }

    fn attach(name: &str, sub: &str) -> Value {
        json!({"_Type_": "SAttachableComponentParams", "AttachDef": {
            "Type": "WeaponPersonal", "SubType": sub, "Size": 2, "Localization": {"Name": name}}})
    }

    #[test]
    fn ranged_melee_and_templates() {
        let loc = parse_global_ini("item_NameP4=P4-AR Rifle\nitem_NameKnife=Knife");
        let set = RecordSet::new(vec![
            entity(
                "rifle",
                json!([attach("@item_NameP4", "Rifle"), {"_Type_": "SCItemWeaponComponentParams",
                    "ammoContainerCapacity": 40, "heatPerShot": 0.5}]),
            ),
            entity(
                "knife",
                json!([attach("@item_NameKnife", "UNDEFINED"), {"_Type_": "SMeleeWeaponComponentParams"}]),
            ),
            entity(
                "stub",
                json!([attach("@missing", "Rifle"), {"_Type_": "SCItemWeaponComponentParams"}]),
            ),
            entity("magazine", json!([attach("@item_NameP4", "Magazine")])),
        ]);
        let body = DOMAIN
            .parse(&ParseContext::new(&set, &loc, &NoFiles))
            .unwrap();
        assert_eq!(
            body["weapons"][0],
            json!({"name": "Knife", "recordName": "EntityClassDefinition.knife", "id": "id-knife",
                   "kind": "melee", "type": "WeaponPersonal", "size": 2.0})
        );
        assert_eq!(body["weapons"][1]["ammoCapacity"], json!(40.0));
        assert_eq!(body["weapons"][2]["name"], "stub");
        assert_eq!(body["weapons"][2]["isTemplate"], true);
        assert_eq!(
            body["summary"],
            json!({"weapons": 3, "ranged": 2, "melee": 1, "templates": 1})
        );
    }
}
