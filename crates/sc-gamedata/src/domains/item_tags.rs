//! Item tags: every item and the tag paths it carries (port of
//! `sc-gamedata/src/domains/item-tags.ts`).
//!
//! Loot never names items: an archetype asks for "anything tagged Weapon/FPS/Stocked/LMG,
//! rarity Common", so answering "where does X drop" means knowing X's tags. They sit on the
//! entity record as GUIDs, resolved here to paths through the tag database. Only the
//! record-level `tags` list is kept.
//!
//! Weapons also get `ammoItems`: the magazines or canisters they load, from
//! `SCItemWeaponComponentParams.ammoContainerRecord` (unless
//! `ShouldIgnorePrimaryAmmoContainer`) plus `secondaryAmmoContainers`, each widened to every
//! magazine-type item with the same attach tags.
//!
//! Ported from the sc-gamedata working tree of 2026-11-14 (`item-tags.ts` was not yet
//! committed there). LIVE 4.10.200.58746 versus the golden dump: identical except two rows
//! the golden has and this build's DataCore does not, `vncl_melee_01_tint03` and
//! `Carryable_2H_FL_05x05x05_CarraybleLootableItem_luminalia_box` (now
//! `Carryable_2H_FL_CarraybleLootableItem_luminalia_box`), so `summary.items` and
//! `cannotGenerateAsLoot` differ too. Both are stale files from an older build left in
//! sc-gamedata's `extracted-data/`, which `loadRecords` walks without clearing.

use crate::collate::compare_names;
use crate::domain::{Domain, DomainError, ParseContext, field};
use crate::entity::component;
use crate::helpers_items::item_common::{attach_def, display_name, manufacturer_of};
use crate::helpers_items::tags::{TagIndex, build_tag_index};
use crate::localization::{LocRefs, Localization, loc_refs};
use crate::records::{GameRecord, at, ref_path, ref_path_str, str_};
use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::{BTreeSet, HashMap};

pub static DOMAIN: ItemTagsDomain = ItemTagsDomain;

pub struct ItemTagsDomain;

/// One row of `items` (`ItemTags` in `query/types.ts`). Field order is output order.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemTags {
    pub name: String,
    pub class_name: String,
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none", rename = "type")]
    pub ty: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manufacturer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_template: Option<bool>,
    /// `nameKey`, `descriptionKey`, `nameStatus`.
    #[serde(flatten)]
    pub loc: LocRefs,
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ammo_items: Option<Vec<String>>,
}

/// `itemTagPaths`: the record-level tags as paths, deduplicated, in UTF-16 order.
pub fn item_tag_paths(record: &GameRecord, tags: &TagIndex) -> Vec<String> {
    let mut paths: Vec<String> = Vec::new();
    let mut seen = BTreeSet::new();
    for r in record
        .value
        .get("tags")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(name) = at(r, "_RecordName_").and_then(Value::as_str) else {
            continue;
        };
        if let Some(resolved) = tags.get(name).filter(|p| !p.is_empty())
            && seen.insert(resolved.clone())
        {
            paths.push(resolved.clone());
        }
    }
    paths.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
    paths
}

/// `tagKey`: the attach tags, split on whitespace, sorted, rejoined.
fn tag_key(def: &Value) -> String {
    let mut parts: Vec<&str> = str_(def.get("Tags"))
        .unwrap_or("")
        .split(|c: char| c.is_whitespace() || c == '\u{feff}')
        .filter(|s| !s.is_empty())
        .collect();
    parts.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
    parts.join(" ")
}

fn is_magazine(def: &Value) -> bool {
    str_(def.get("SubType")) == Some("Magazine")
}

/// `weaponAmmoRefs`: record paths (as `refPath` gives them) of the ammo containers a weapon
/// names, primary first.
pub fn weapon_ammo_refs(record: &GameRecord) -> Vec<String> {
    let Some(weapon) = component(record, "SCItemWeaponComponentParams") else {
        return Vec::new();
    };
    let mut refs = Vec::new();
    if let Some(primary) = ref_path(weapon.get("ammoContainerRecord"))
        && !primary.is_empty()
        && weapon.get("ShouldIgnorePrimaryAmmoContainer") != Some(&Value::Bool(true))
    {
        refs.push(primary);
    }
    for secondary in weapon
        .get("secondaryAmmoContainers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(r) = ref_path(at(secondary, "ammoContainerRecord"))
            && !r.is_empty()
            && !refs.contains(&r)
        {
            refs.push(r);
        }
    }
    refs
}

/// `ammoLinks`: weapon class name -> class names of the magazines it takes. Only containers
/// that are items themselves (`is_item`) are kept.
pub fn ammo_links(
    records: &[&GameRecord],
    is_item: impl Fn(&GameRecord) -> bool,
) -> HashMap<String, Vec<String>> {
    let mut by_path: HashMap<String, &GameRecord> = HashMap::new();
    for r in records {
        if let Some(p) = ref_path_str(&r.record_path) {
            by_path.insert(p, r);
        }
    }
    let mut by_tag_key: HashMap<String, Vec<&GameRecord>> = HashMap::new();
    for r in records {
        let def = attach_def(r);
        let key = tag_key(def);
        if !is_magazine(def) || key.is_empty() || !is_item(r) {
            continue;
        }
        by_tag_key.entry(key).or_default().push(r);
    }
    let mut links = HashMap::new();
    for record in records {
        let refs = weapon_ammo_refs(record);
        if refs.is_empty() {
            continue;
        }
        let mut ammo: Vec<String> = Vec::new();
        let mut add = |r: &GameRecord| {
            if !ammo.contains(&r.short_name) {
                ammo.push(r.short_name.clone());
            }
        };
        for r in &refs {
            let Some(container) = by_path.get(r).copied().filter(|c| is_item(c)) else {
                continue;
            };
            add(container);
            let def = attach_def(container);
            let mut siblings: Vec<&GameRecord> = if is_magazine(def) {
                by_tag_key.get(&tag_key(def)).cloned().unwrap_or_default()
            } else {
                Vec::new()
            };
            siblings.sort_by(|a, b| compare_names(&a.short_name, &b.short_name));
            for s in siblings {
                add(s);
            }
        }
        if !ammo.is_empty() {
            links.insert(record.short_name.clone(), ammo);
        }
    }
    links
}

pub fn parse_item_tags(
    record: &GameRecord,
    tags: &TagIndex,
    localization: &Localization,
) -> Option<ItemTags> {
    let paths = item_tag_paths(record, tags);
    if paths.is_empty() {
        return None;
    }
    let def = attach_def(record);
    let (localized, placeholder) = display_name(def, localization);
    let sub_type = str_(def.get("SubType"));
    Some(ItemTags {
        name: if placeholder {
            record.short_name.clone()
        } else {
            localized.unwrap_or_default().to_string()
        },
        class_name: record.short_name.clone(),
        id: record.id.clone(),
        ty: str_(def.get("Type")).map(str::to_string),
        sub_type: sub_type.filter(|s| *s != "UNDEFINED").map(str::to_string),
        manufacturer: manufacturer_of(def),
        is_template: placeholder.then_some(true),
        loc: loc_refs(
            localization,
            at(def, "Localization.Name"),
            at(def, "Localization.Description"),
        ),
        tags: paths,
        ammo_items: None,
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Summary {
    items: usize,
    with_ammo: usize,
    can_generate_as_loot: usize,
    cannot_generate_as_loot: usize,
    tags: usize,
}

impl Domain for ItemTagsDomain {
    fn name(&self) -> &'static str {
        "game-item-tags"
    }

    fn description(&self) -> &'static str {
        "Every item's tag paths, the join between items and tag-driven loot."
    }

    fn filters(&self) -> &'static [&'static str] {
        &["**/entities/scitem/**", "**/tagdatabase/**"]
    }

    fn needs_localization(&self) -> bool {
        true
    }

    /// Items keep their tag refs, `AttachDef` and the weapon's ammo container refs (peak
    /// memory of a LIVE extract: 8.7 GB whole, see the crate README).
    fn prune(&self, record: &mut GameRecord) {
        if record.ty == "EntityClassDefinition" {
            crate::entity::prune_entity(
                &mut record.value,
                &["tags"],
                &[
                    ("SAttachableComponentParams", Some(&["AttachDef"])),
                    (
                        "SCItemWeaponComponentParams",
                        Some(&[
                            "ammoContainerRecord",
                            "ShouldIgnorePrimaryAmmoContainer",
                            "secondaryAmmoContainers",
                        ]),
                    ),
                ],
            );
        }
    }

    fn parse(&self, ctx: &ParseContext<'_>) -> Result<Map<String, Value>, DomainError> {
        let tags = build_tag_index(ctx.records.iter().find(|r| r.ty == "TagDatabase"));
        let entities: Vec<&GameRecord> = ctx.records.of_type("EntityClassDefinition").collect();
        let ammo = ammo_links(&entities, |r| !item_tag_paths(r, &tags).is_empty());
        let mut items: Vec<ItemTags> = entities
            .iter()
            .filter_map(|r| parse_item_tags(r, &tags, ctx.localization))
            .map(|mut item| {
                item.ammo_items = ammo.get(&item.class_name).cloned();
                item
            })
            .collect();
        items.sort_by(|a, b| compare_names(&a.class_name, &b.class_name));

        let summary = Summary {
            items: items.len(),
            with_ammo: items.iter().filter(|i| i.ammo_items.is_some()).count(),
            can_generate_as_loot: items
                .iter()
                .filter(|i| i.tags.iter().any(|t| t == "LootGeneration/CanGenerateAsLoot"))
                .count(),
            cannot_generate_as_loot: items
                .iter()
                .filter(|i| {
                    i.tags
                        .iter()
                        .any(|t| t.starts_with("LootGeneration/CannotGenerateAsLoot"))
                })
                .count(),
            tags: tags.len(),
        };
        let mut body = Map::new();
        field(&mut body, "items", &items)?;
        field(&mut body, "summary", &summary)?;
        Ok(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::NoFiles;
    use crate::records::{RecordSet, normalize_record};
    use serde_json::json;

    const ROOT: &str = "file://./../../../../../../../libs/foundry/records/";
    const MAGS: &str = "entities/scitem/weapons/magazines/";

    fn rec(path: &str, mut value: Value, tagged: bool) -> GameRecord {
        let short = path.rsplit('/').next().unwrap();
        let obj = value.as_object_mut().unwrap();
        obj.insert("_Type_".into(), json!("EntityClassDefinition"));
        if tagged {
            obj.insert("tags".into(), json!([{"_RecordName_": "Tag.item"}]));
        }
        normalize_record(
            json!({"_RecordName_": format!("EntityClassDefinition.{short}"), "_RecordId_": format!("id-{short}"),
                   "_RecordValue_": value}),
            &format!("libs/foundry/records/{path}"),
        )
        .unwrap()
    }

    fn attach(def: Value) -> Value {
        json!({"_Type_": "SAttachableComponentParams", "AttachDef": def})
    }

    fn mag(name: &str, tags: &str) -> GameRecord {
        rec(
            &format!("{MAGS}{name}"),
            json!({"Components": [attach(json!({"Type": "WeaponAttachment", "SubType": "Magazine", "Tags": tags}))]}),
            true,
        )
    }

    fn weapon(name: &str, params: Value) -> GameRecord {
        let mut p = json!({"_Type_": "SCItemWeaponComponentParams"});
        p.as_object_mut()
            .unwrap()
            .extend(params.as_object().unwrap().clone());
        rec(
            &format!("entities/scitem/weapons/fps_weapons/{name}"),
            json!({"Components": [attach(json!({"Type": "WeaponPersonal"})), p]}),
            true,
        )
    }

    // Shapes as in 4.10.200 (cases from sc-gamedata src/domains/item-tags.test.ts).
    fn records() -> Vec<GameRecord> {
        let untagged = rec(&format!("{MAGS}internal_box"), json!({"Components": []}), false);
        vec![
            mag("grin_tool_01_mag", "grin_salvage_repair_01"),
            mag("grin_tool_resource_salvage_repair_01", "grin_tool_salvage_01_canister"),
            mag("grin_tool_resource_salvage_repair_01_filled", "grin_tool_salvage_01_canister"),
            mag("grin_multitool_resource_salvage_repair_01", "grin_multitool_01_canister"),
            mag("behr_rifle_ballistic_01_mag", "behr_rifle_ballistic_01"),
            mag("behr_rifle_ballistic_03_mag", "behr_rifle_ballistic_01"),
            untagged,
            weapon(
                "grin_salvage_repair_01",
                json!({"ammoContainerRecord": format!("{ROOT}{MAGS}grin_tool_01_mag.json"),
                       "ShouldIgnorePrimaryAmmoContainer": true,
                       "secondaryAmmoContainers": [{"_Type_": "SSecondaryAmmoContainerParams",
                           "ammoContainerRecord": format!("{ROOT}{MAGS}grin_tool_resource_salvage_repair_01_filled.json")}]}),
            ),
            weapon(
                "behr_rifle_ballistic_01",
                json!({"ammoContainerRecord": format!("{ROOT}{MAGS}behr_rifle_ballistic_01_mag.json")}),
            ),
            weapon(
                "ship_gun",
                json!({"ammoContainerRecord": format!("{ROOT}{MAGS}internal_box.json")}),
            ),
        ]
    }

    fn is_item(r: &GameRecord) -> bool {
        r.value
            .get("tags")
            .and_then(Value::as_array)
            .is_some_and(|t| !t.is_empty())
    }

    #[test]
    fn weapon_ammo_refs_drop_an_ignored_primary() {
        let rs = records();
        assert_eq!(
            weapon_ammo_refs(&rs[7]),
            vec![format!("{MAGS}grin_tool_resource_salvage_repair_01_filled")]
        );
        assert_eq!(
            weapon_ammo_refs(&rs[8]),
            vec![format!("{MAGS}behr_rifle_ballistic_01_mag")]
        );
        assert!(weapon_ammo_refs(&rs[1]).is_empty());
    }

    #[test]
    fn containers_widen_to_magazines_with_the_same_port_tag() {
        let rs = records();
        let refs: Vec<&GameRecord> = rs.iter().collect();
        let links = ammo_links(&refs, is_item);
        assert_eq!(
            links["grin_salvage_repair_01"],
            vec![
                "grin_tool_resource_salvage_repair_01_filled",
                "grin_tool_resource_salvage_repair_01"
            ]
        );
        assert_eq!(
            links["behr_rifle_ballistic_01"],
            vec!["behr_rifle_ballistic_01_mag", "behr_rifle_ballistic_03_mag"]
        );
        assert!(!links.contains_key("ship_gun"), "containers without tags are not linked");
    }

    #[test]
    fn writes_ammo_items_on_the_weapon_row() {
        let db = normalize_record(
            json!({"_RecordName_": "TagDatabase.tagdatabase", "_RecordId_": "db", "_RecordValue_": {
                "_Type_": "TagDatabase",
                "tags": [{"tagName": "Global", "children": [{"tagName": "Item", "_RecordName_": "Tag.item"}]}]}}),
            "libs/foundry/records/tagdatabase/tagdatabase.tagdatabase",
        )
        .unwrap();
        let mut all = vec![db];
        all.extend(records());
        let set = RecordSet::new(all);
        let body = crate::fixtures::parse_checked(&DOMAIN, &set, &Localization::new());
        let find = |class: &str| {
            body["items"]
                .as_array()
                .unwrap()
                .iter()
                .find(|i| i["className"] == class)
                .cloned()
                .unwrap()
        };
        assert_eq!(
            find("grin_salvage_repair_01")["ammoItems"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert!(
            find("grin_tool_resource_salvage_repair_01")
                .get("ammoItems")
                .is_none()
        );
        assert_eq!(body["summary"]["withAmmo"], 2);
    }

    // Case from sc-gamedata src/domains/item-sources.test.ts ("item-tags").
    #[test]
    fn resolves_record_level_tags_and_skips_untagged_records() {
        let db = normalize_record(
            json!({"_RecordName_": "TagDatabase.tagdatabase", "_RecordId_": "db", "_RecordValue_": {
                "_Type_": "TagDatabase",
                "tags": [{"tagName": "Global", "children": [
                    {"tagName": "LootGeneration", "children": [{"tagName": "CanGenerateAsLoot", "_RecordName_": "Tag.can"}]},
                    {"tagName": "Weapon", "children": [{"tagName": "FPS", "_RecordName_": "Tag.fps"}]}]}]}}),
            "libs/foundry/records/tagdatabase/tagdatabase.tagdatabase",
        )
        .unwrap();
        let gun = rec(
            "entities/scitem/weapons/fps_weapons/ksar_lmg_energy_01",
            json!({"tags": [{"_RecordName_": "Tag.fps"}, {"_RecordName_": "Tag.can"}, {"_RecordName_": "Tag.missing"}],
                   "Components": [attach(json!({"Type": "WeaponPersonal", "Localization": {"Name": "@nope"}}))]}),
            false,
        );
        let crate_ = rec(
            "entities/scitem/lootables/lootable_container_large",
            json!({"Components": [attach(json!({"Localization": {"Name": "@item_crate"}}))]}),
            false,
        );
        let set = RecordSet::new(vec![db, gun, crate_]);
        let body = DOMAIN
            .parse(&ParseContext::new(&set, &Localization::new(), &NoFiles))
            .unwrap();
        assert_eq!(
            body["items"],
            json!([{"name": "ksar_lmg_energy_01", "className": "ksar_lmg_energy_01",
                    "id": "id-ksar_lmg_energy_01", "type": "WeaponPersonal", "isTemplate": true,
                    "nameKey": "nope", "nameStatus": "unresolved",
                    "tags": ["LootGeneration/CanGenerateAsLoot", "Weapon/FPS"]}])
        );
        assert_eq!(body["summary"]["canGenerateAsLoot"], 1);
    }
}
