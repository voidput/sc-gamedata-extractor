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
//! Blueprint products and NPC gear classes that carry no record-level tags are not rows of
//! the domain itself; [`add_identity_rows`] adds them afterwards from `game-blueprints` and
//! `game-npc-loadouts` (spec `cross-dataset-derivations.md` D3, decision 2): identity only,
//! with `size` / `grade` from `AttachDef`, empty `tags` and `source`.
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
use crate::records::{GameRecord, RecordSource, at, num, ref_path, ref_path_str, str_};
use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::{BTreeSet, HashMap};
use std::path::Path;

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
    /// `AttachDef.Size`; identity rows only ([`add_identity_rows`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<f64>,
    /// `AttachDef.Grade`; identity rows only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grade: Option<f64>,
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
    /// Why an untagged record is listed: `blueprint` (a blueprint produces it) or
    /// `npcGear` (an NPC loadout wears it). Absent on tagged rows.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<&'static str>,
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
    Some(item_row(record, localization, paths))
}

/// A row from the record's `AttachDef`, with the given tag paths.
fn item_row(record: &GameRecord, localization: &Localization, paths: Vec<String>) -> ItemTags {
    let def = attach_def(record);
    let (localized, placeholder) = display_name(def, localization);
    let sub_type = str_(def.get("SubType"));
    ItemTags {
        name: if placeholder {
            record.short_name.clone()
        } else {
            localized.unwrap_or_default().to_string()
        },
        class_name: record.short_name.clone(),
        id: record.id.clone(),
        ty: str_(def.get("Type")).map(str::to_string),
        sub_type: sub_type.filter(|s| *s != "UNDEFINED").map(str::to_string),
        size: None,
        grade: None,
        manufacturer: manufacturer_of(def),
        is_template: placeholder.then_some(true),
        loc: loc_refs(
            localization,
            at(def, "Localization.Name"),
            at(def, "Localization.Description"),
        ),
        tags: paths,
        ammo_items: None,
        source: None,
    }
}

/// Output file name of the domain.
pub const FILE: &str = "game-item-tags.json";

/// What [`add_identity_rows`] added: rows per `source`, and wanted classes with no item
/// record in the DataCore.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IdentityRows {
    pub blueprint: usize,
    pub npc_gear: usize,
    pub not_found: Vec<String>,
}

fn read_json(file: &Path) -> Option<Value> {
    serde_json::from_slice(&std::fs::read(file).ok()?).ok()
}

/// Lower-case class -> `source` of the classes `game-blueprints` produces and
/// `game-npc-loadouts` actors wear (a blueprint product wins when both name it).
pub fn identity_wanted(blueprints: Option<&Value>, loadouts: Option<&Value>) -> Vec<(String, &'static str)> {
    let mut out: Vec<(String, &'static str)> = Vec::new();
    let mut seen = BTreeSet::new();
    let products = blueprints
        .and_then(|b| b.get("blueprints")?.as_array())
        .into_iter()
        .flatten()
        .filter_map(|b| b.get("produces")?.as_str());
    let worn = loadouts
        .and_then(|l| l.get("actors")?.as_array())
        .into_iter()
        .flatten()
        .flat_map(|a| a.get("items").and_then(Value::as_array).into_iter().flatten())
        .filter_map(|i| i.get("item")?.as_str());
    for (class, source) in products
        .map(|c| (c, "blueprint"))
        .chain(worn.map(|c| (c, "npcGear")))
    {
        let key = class.to_lowercase();
        if !key.is_empty() && seen.insert(key.clone()) {
            out.push((key, source));
        }
    }
    out
}

/// Add identity rows to `game-item-tags.json` in `dir` for the blueprint products and NPC
/// gear classes it lacks (`game-blueprints.json`, `game-npc-loadouts.json` beside it), read
/// from their item records (`EntityClassDefinition` under `entities/scitem/`). Rows a
/// previous run added (`source` set) are dropped first, so a rerun restates the inputs on
/// disk. `None` when the item-tags file is missing; the file is rewritten only on a change.
pub fn add_identity_rows(
    dir: &Path,
    records: &dyn RecordSource,
    localization: &Localization,
) -> std::io::Result<Option<IdentityRows>> {
    let file = dir.join(FILE);
    let Some(mut doc) = read_json(&file) else {
        return Ok(None);
    };
    let Some(items) = doc.get_mut("items").and_then(Value::as_array_mut) else {
        return Ok(None);
    };
    let before = items.clone();
    items.retain(|i| i.get("source").is_none());
    let have: BTreeSet<String> = items
        .iter()
        .filter_map(|i| i.get("className")?.as_str())
        .map(str::to_lowercase)
        .collect();
    let blueprints = read_json(&dir.join("game-blueprints.json"));
    let loadouts = read_json(&dir.join("game-npc-loadouts.json"));
    let wanted: HashMap<String, &'static str> =
        identity_wanted(blueprints.as_ref(), loadouts.as_ref())
            .into_iter()
            .filter(|(c, _)| !have.contains(c))
            .collect();
    // The first item record of each wanted class, in DataCore order.
    let mut found: HashMap<String, usize> = HashMap::new();
    for (i, m) in records.metas().iter().enumerate() {
        if m.struct_type != "EntityClassDefinition"
            || !m.record_path.to_lowercase().contains("entities/scitem/")
        {
            continue;
        }
        let short = m.name.split_once('.').map_or(m.name.as_str(), |(_, s)| s);
        let key = short.to_lowercase();
        if wanted.contains_key(&key) {
            found.entry(key).or_insert(i);
        }
    }
    let mut added = IdentityRows::default();
    let mut rows: Vec<ItemTags> = Vec::new();
    for (key, &i) in &found {
        let Ok(Some(record)) = records.load(i) else {
            added.not_found.push(key.clone());
            continue;
        };
        let def = attach_def(&record);
        let mut row = item_row(&record, localization, Vec::new());
        row.size = num(def.get("Size"));
        row.grade = num(def.get("Grade"));
        row.source = Some(wanted[key]);
        match wanted[key] {
            "blueprint" => added.blueprint += 1,
            _ => added.npc_gear += 1,
        }
        rows.push(row);
    }
    added
        .not_found
        .extend(wanted.keys().filter(|k| !found.contains_key(*k)).cloned());
    added.not_found.sort();
    for r in rows {
        items.push(serde_json::to_value(r).map_err(std::io::Error::other)?);
    }
    let name_of = |v: &Value| v.get("className").and_then(Value::as_str).unwrap_or("").to_string();
    items.sort_by(|a, b| compare_names(&name_of(a), &name_of(b)));
    let total = items.len();
    let changed = *items != before;
    if let Some(summary) = doc.get_mut("summary").and_then(Value::as_object_mut) {
        summary.insert("items".into(), Value::from(total));
        if added.blueprint + added.npc_gear > 0 {
            summary.insert(
                "identityRows".into(),
                serde_json::json!({"blueprint": added.blueprint, "npcGear": added.npc_gear}),
            );
        } else {
            summary.remove("identityRows");
        }
    }
    if changed {
        std::fs::write(&file, crate::output::to_js_json(&doc))?;
    }
    Ok(Some(added))
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
    fn schema_version(&self) -> u32 {
        1
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

    #[test]
    fn identity_rows_for_untagged_blueprint_products_and_npc_gear() {
        let dir = std::env::temp_dir().join(format!("sc-gamedata-identity-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let entity = |short: &str, path: &str, def: Value| {
            json!({"_RecordName_": format!("EntityClassDefinition.{short}"), "_RecordId_": format!("id-{short}"),
                   "_RecordPath_": format!("libs/foundry/records/entities/scitem/{path}/{short}.xml"),
                   "_RecordValue_": {"_Type_": "EntityClassDefinition", "tags": [],
                       "Components": [attach(def)]}})
        };
        let source = crate::fixtures::FixtureSource::from_envelopes(vec![
            entity("AMRS_LaserCannon_S1", "ships/weapons",
                json!({"Type": "WeaponGun", "SubType": "Gun", "Size": 1, "Grade": 2})),
            entity("bautista_outfit_01", "characters", json!({"Type": "Char_Clothing_Torso_1", "Size": 1,
                "Localization": {"Name": "@LOC_PLACEHOLDER"}})),
            entity("tagged_rifle", "weapons", json!({"Type": "WeaponPersonal"})),
        ])
        .unwrap();
        let write = |name: &str, v: Value| std::fs::write(dir.join(name), v.to_string()).unwrap();
        write(FILE, json!({"_build": "1.0.1", "items": [{"name": "Rifle", "className": "tagged_rifle", "id": "r", "tags": ["Weapon"]}],
                           "summary": {"items": 1, "withAmmo": 0}}));
        write("game-blueprints.json", json!({"blueprints": [{"name": "a", "produces": "amrs_lasercannon_s1"},
                                                            {"name": "b", "produces": "TAGGED_RIFLE"}]}));
        write("game-npc-loadouts.json", json!({"actors": [{"class": "npc", "items": [
            {"item": "bautista_outfit_01"}, {"item": "no_such_record"}]}]}));
        let loc = Localization::new();
        let r = add_identity_rows(&dir, &source, &loc).unwrap().unwrap();
        assert_eq!((r.blueprint, r.npc_gear), (1, 1));
        assert_eq!(r.not_found, vec!["no_such_record"]);
        let doc: Value = serde_json::from_slice(&std::fs::read(dir.join(FILE)).unwrap()).unwrap();
        assert_eq!(
            doc["items"][0],
            json!({"name": "AMRS_LaserCannon_S1", "className": "AMRS_LaserCannon_S1", "id": "id-AMRS_LaserCannon_S1",
                   "type": "WeaponGun", "subType": "Gun", "size": 1, "grade": 2, "isTemplate": true,
                   "nameStatus": "empty", "tags": [], "source": "blueprint"})
        );
        assert_eq!(doc["items"][1]["source"], "npcGear");
        assert_eq!(doc["items"][1]["isTemplate"], true);
        assert_eq!(doc["items"][2]["className"], "tagged_rifle");
        assert_eq!(doc["summary"]["items"], 3);
        assert_eq!(doc["summary"]["identityRows"], json!({"blueprint": 1, "npcGear": 1}));
        // A rerun restates the same rows (no duplicates).
        let again = add_identity_rows(&dir, &source, &loc).unwrap().unwrap();
        assert_eq!(again, r);
        let doc2: Value = serde_json::from_slice(&std::fs::read(dir.join(FILE)).unwrap()).unwrap();
        assert_eq!(doc2, doc);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
