//! Where items spawn and what hands them out: loot areas and contract awards (port of
//! `sc-gamedata/src/domains/item-sources.ts`).
//!
//! Loot reaches the world through harvestables. A slot preset (`harvestable/slotpresets`)
//! is the rulebook for one kind of area: for each spawn-point tag combination it lists what
//! may spawn there, by relative weight. A slot is a container with a `LootConfig` (the loot
//! table it rolls, plus for V3 tables a pool filter and secondary choices), a harvestable
//! preset whose entity has a fixed loadout (a weapon rack), or a loose item. Which concrete
//! locations use which preset lives in the object containers, so areas are named by their
//! preset. Contract awards are items a contract hands over on completion
//! (`ItemAwardEntityClass`), which is how Wikelo's rewards are stored.
//!
//! Ported from the sc-gamedata working tree of 2026-11-14 (`item-sources.ts` was not yet
//! committed there).

use crate::collate::compare_names;
use crate::domain::{Domain, DomainError, ParseContext, field};
use crate::helpers_items::item_common::{attach_def, display_name};
use crate::helpers_items::tags::{TagIndex, build_tag_index};
use crate::localization::Localization;
use crate::records::{GameRecord, RecordSet, at, num, ref_path, ref_path_str, str_};
use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::HashSet;

pub static DOMAIN: ItemSources = ItemSources;

pub struct ItemSources;

const SLOT_PRESETS: &str = "harvestable/slotpresets/";

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpawnLoot {
    pub table: String,
    /// v1 or v3.
    pub version: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pool_filter: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secondary_choices: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_results: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chance: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpawnSlot {
    pub entity: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
    pub weight: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loot: Option<SpawnLoot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contains: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpawnConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub spawn_tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fill_chance: Option<f64>,
    pub slots: Vec<SpawnSlot>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LootArea {
    pub name: String,
    pub configs: Vec<SpawnConfig>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ContractAward {
    pub generator: String,
    pub contract: String,
    pub item: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weighting: Option<f64>,
}

/// The last segment of a `refPath` string.
fn last_segment(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_string()
}

/// `recordNameOf`: the record a `file://` reference names, by its last path segment.
fn record_name_of(reference: Option<&Value>) -> Option<String> {
    ref_path(reference)
        .filter(|p| !p.is_empty())
        .map(|p| last_segment(&p))
}

/// `a ?? b` on raw values: `b` when `a` is missing or `null`.
fn nullish_or<'a>(a: Option<&'a Value>, b: Option<&'a Value>) -> Option<&'a Value> {
    match a {
        None | Some(Value::Null) => b,
        some => some,
    }
}

fn js_truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|x| x != 0.0 && !x.is_nan()),
        Some(Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

fn is_object(v: &Value) -> bool {
    v.is_object() || v.is_array()
}

fn tag_paths(list: Option<&Value>, tags: &TagIndex) -> Vec<String> {
    list.and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|r| at(r, "_RecordName_").and_then(Value::as_str))
        .filter_map(|name| tags.get(name).filter(|p| !p.is_empty()).cloned())
        .collect()
}

/// `loadoutItems`: item class names an entity's default loadout holds, at any depth, in
/// first-found order.
/// Every `SItemPortLoadoutEntryParams` below `value`, cut to the two fields
/// [`loadout_items`] reads (nested entries are listed on their own).
fn loadout_entries(value: &Value, out: &mut Vec<Value>) {
    match value {
        Value::Array(list) => list.iter().for_each(|v| loadout_entries(v, out)),
        Value::Object(obj) => {
            if obj.get("_Type_").and_then(Value::as_str) == Some("SItemPortLoadoutEntryParams") {
                let mut e = Map::new();
                for k in ["_Type_", "entityClassReference", "entityClassName"] {
                    if let Some(v) = obj.get(k) {
                        e.insert(k.into(), v.clone());
                    }
                }
                out.push(Value::Object(e));
            }
            obj.values().for_each(|v| loadout_entries(v, out));
        }
        _ => {}
    }
}

pub fn loadout_items(value: &Value) -> Vec<String> {
    fn walk(value: &Value, found: &mut Vec<String>, seen: &mut HashSet<String>) {
        let mut add = |s: String, found: &mut Vec<String>| {
            if seen.insert(s.clone()) {
                found.push(s);
            }
        };
        match value {
            Value::Array(list) => {
                for v in list {
                    walk(v, found, seen);
                }
            }
            Value::Object(obj) => {
                if obj.get("_Type_").and_then(Value::as_str) == Some("SItemPortLoadoutEntryParams")
                {
                    let by_ref = record_name_of(obj.get("entityClassReference"));
                    let by_name = str_(obj.get("entityClassName"));
                    match (by_ref.filter(|r| !r.is_empty()), by_name) {
                        (Some(r), _) => add(r, found),
                        (None, Some(n)) => add(n.to_lowercase(), found),
                        _ => {}
                    }
                }
                for v in obj.values() {
                    walk(v, found, seen);
                }
            }
            _ => {}
        }
    }
    let mut found = Vec::new();
    walk(value, &mut found, &mut HashSet::new());
    found
}

/// `parseLootConfig`.
pub fn parse_loot_config(value: Option<&Value>) -> Option<SpawnLoot> {
    let config = value.filter(|v| is_object(v))?;
    let v3 = record_name_of(config.get("lootTableV3"));
    let v1 = record_name_of(config.get("lootTable"));
    let table = v3.clone().or(v1).filter(|t| !t.is_empty())?;
    let constraints = config.get("lootConstraints").filter(|c| !c.is_null());
    let get = |k: &str| constraints.and_then(|c| c.get(k));
    let choices = get("secondaryChoices");
    Some(SpawnLoot {
        table,
        version: if v3.is_some_and(|v| !v.is_empty()) {
            "v3"
        } else {
            "v1"
        },
        pool_filter: record_name_of(get("poolFilter")),
        secondary_choices: record_name_of(nullish_or(
            choices.and_then(|c| c.get("multiLayerRecord")),
            choices.and_then(|c| c.get("singleLayerRecord")),
        )),
        max_results: num(get("totalResultsLimit")),
        chance: num(get("chanceToGenerate")),
    })
}

struct Context<'a> {
    records: &'a RecordSet,
    tags: TagIndex,
    localization: &'a Localization,
}

impl Context<'_> {
    fn by_path(&self, path: &str) -> Option<&GameRecord> {
        self.records.by_record_path(path)
    }
}

fn entity_display_name(record: Option<&GameRecord>, localization: &Localization) -> Option<String> {
    let (name, placeholder) = display_name(attach_def(record?), localization);
    if placeholder {
        None
    } else {
        name.map(str::to_string)
    }
}

/// `parseSlot`.
fn parse_slot(slot: &Value, ctx: &Context<'_>) -> Option<SpawnSlot> {
    let preset_path = ref_path(slot.get("harvestable"));
    let preset = preset_path
        .as_deref()
        .filter(|p| !p.is_empty())
        .and_then(|p| ctx.by_path(p));
    let entity_path = ref_path(slot.get("harvestableEntityClass"))
        .or_else(|| ref_path(preset.and_then(|p| p.value.get("entityClass"))))
        .filter(|p| !p.is_empty())?;
    let entity_record = ctx.by_path(&entity_path);
    let mut contains = entity_record
        .map(|r| loadout_items(&r.value))
        .unwrap_or_default();
    contains.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
    Some(SpawnSlot {
        entity: last_segment(&entity_path),
        entity_name: entity_display_name(entity_record, ctx.localization),
        preset: preset_path.map(|p| last_segment(&p)),
        weight: num(slot.get("relativeProbability")).unwrap_or(1.0),
        loot: parse_loot_config(slot.get("lootConfig")),
        contains: (!contains.is_empty()).then_some(contains),
    })
}

/// `subConfigs`: follow manual, by-reference and record wrappers down to a
/// `SubHarvestableConfig`.
fn sub_configs<'a>(node: Option<&'a Value>, ctx: &'a Context<'_>, seen: &mut HashSet<String>) -> Vec<&'a Value> {
    let Some(obj) = node.filter(|n| is_object(n)) else {
        return Vec::new();
    };
    if obj.get("_Type_").and_then(Value::as_str) == Some("SubHarvestableConfig") {
        return vec![obj];
    }
    if js_truthy(obj.get("subConfigManual")) {
        return sub_configs(obj.get("subConfigManual"), ctx, seen);
    }
    if js_truthy(obj.get("subConfig")) {
        return sub_configs(obj.get("subConfig"), ctx, seen);
    }
    if let Some(r) = ref_path(obj.get("subConfigRef")).filter(|r| !r.is_empty())
        && seen.insert(r.clone())
    {
        return sub_configs(ctx.by_path(&r).map(|rec| &rec.value), ctx, seen);
    }
    Vec::new()
}

fn parse_config(
    name: Option<String>,
    spawn_tags: Vec<String>,
    node: Option<&Value>,
    ctx: &Context<'_>,
) -> Option<SpawnConfig> {
    let configs = sub_configs(node, ctx, &mut HashSet::new());
    let first = configs.first()?;
    let slots: Vec<SpawnSlot> = configs
        .iter()
        .flat_map(|c| c.get("subHarvestables").and_then(Value::as_array).into_iter().flatten())
        .filter(|s| is_object(s))
        .filter_map(|s| parse_slot(s, ctx))
        .collect();
    if slots.is_empty() {
        return None;
    }
    let fill = num(first.get("initialSlotsProbability"));
    Some(SpawnConfig {
        name,
        spawn_tags,
        fill_chance: fill.filter(|f| *f != 1.0),
        slots,
    })
}

/// `parseArea`: one slot preset record.
fn parse_area(record: &GameRecord, ctx: &Context<'_>) -> Option<LootArea> {
    let path = ref_path_str(&record.record_path).unwrap_or_default();
    let name = match path.strip_prefix(SLOT_PRESETS) {
        Some(rest) => rest.to_string(),
        None => record.short_name.clone(),
    };
    let mut configs = Vec::new();
    let multi = record.value.get("multiConfig");
    if js_truthy(multi) {
        let multi = multi.unwrap_or(&Value::Null);
        if let Some(c) = parse_config(Some("default".into()), Vec::new(), multi.get("defaultConfig"), ctx) {
            configs.push(c);
        }
        for tagged in multi
            .get("taggedConfigs")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if !js_truthy(Some(tagged)) {
                continue;
            }
            let config = parse_config(
                str_(tagged.get("name")).map(str::to_string),
                tag_paths(at(tagged, "tagList.tags"), &ctx.tags),
                tagged.get("subConfig"),
                ctx,
            );
            if let Some(c) = config {
                configs.push(c);
            }
        }
    } else if let Some(c) = parse_config(None, Vec::new(), record.value.get("subConfig"), ctx) {
        configs.push(c);
    }
    (!configs.is_empty()).then_some(LootArea { name, configs })
}

/// `contractAwards`: every item a contract generator record awards.
pub fn contract_awards(record: &GameRecord) -> Vec<ContractAward> {
    fn walk(
        value: &Value,
        weighting: Option<f64>,
        names: (&str, &str),
        out: &mut Vec<ContractAward>,
    ) {
        match value {
            Value::Array(list) => {
                for v in list {
                    walk(v, weighting, names, out);
                }
            }
            Value::Object(obj) => match obj.get("_Type_").and_then(Value::as_str) {
                Some("ItemAwardWeightings") => {
                    let w = num(obj.get("weighting"));
                    for v in obj.values() {
                        walk(v, w, names, out);
                    }
                }
                Some("ItemAwardEntityClass") => {
                    if let Some(item) = record_name_of(obj.get("entityClass")) {
                        out.push(ContractAward {
                            generator: names.0.to_string(),
                            contract: names.1.to_string(),
                            item,
                            amount: num(obj.get("amountToAward")),
                            weighting,
                        });
                    }
                }
                _ => {
                    for v in obj.values() {
                        walk(v, weighting, names, out);
                    }
                }
            },
            _ => {}
        }
    }

    let mut out = Vec::new();
    for generator in record
        .value
        .get("generators")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if !js_truthy(Some(generator)) {
            continue;
        }
        let generator_name = str_(generator.get("debugName")).unwrap_or(&record.short_name);
        for contract in generator
            .get("contracts")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if !js_truthy(Some(contract)) {
                continue;
            }
            let contract_name = str_(contract.get("debugName")).unwrap_or(generator_name);
            if let Some(results) = contract.get("contractResults") {
                walk(results, None, (generator_name, contract_name), &mut out);
            }
        }
    }
    out
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Summary {
    areas: usize,
    configs: usize,
    slots: usize,
    loot_slots: usize,
    fixed_slots: usize,
    contract_awards: usize,
}

impl Domain for ItemSources {
    fn name(&self) -> &'static str {
        "game-item-sources"
    }

    fn description(&self) -> &'static str {
        "Loot spawn areas (spawn-point tags -> containers, racks, loose items, loot tables) and contract item awards."
    }

    fn filters(&self) -> &'static [&'static str] {
        &[
            "**/harvestable/**",
            "**/entities/scitem/**",
            "**/contracts/contractgenerator/**",
            "**/tagdatabase/**",
        ]
    }

    fn needs_localization(&self) -> bool {
        true
    }

    /// Item records are only read for their display name (`AttachDef`) and the loadout
    /// entries [`loadout_items`] collects; keep just those (9.0 GB whole on LIVE). The
    /// entries move to one `_loadout_` list, which the walk finds the same set in (it is
    /// deduplicated and sorted).
    fn prune(&self, record: &mut GameRecord) {
        if record.ty != "EntityClassDefinition"
            || !record.record_path.to_ascii_lowercase().contains("entities/scitem/")
        {
            return;
        }
        let mut entries = Vec::new();
        loadout_entries(&record.value, &mut entries);
        crate::entity::prune_entity(
            &mut record.value,
            &[],
            &[("SAttachableComponentParams", Some(&["AttachDef"]))],
        );
        if let Some(obj) = record.value.as_object_mut() {
            obj.insert("_loadout_".into(), Value::Array(entries));
        }
    }

    fn parse(&self, ctx: &ParseContext<'_>) -> Result<Map<String, Value>, DomainError> {
        let context = Context {
            records: ctx.records,
            tags: build_tag_index(ctx.records.iter().find(|r| r.ty == "TagDatabase")),
            localization: ctx.localization,
        };
        let under = |r: &GameRecord, prefix: &str| {
            ref_path_str(&r.record_path).is_some_and(|p| p.starts_with(prefix))
        };

        let mut areas: Vec<LootArea> = ctx
            .records
            .iter()
            .filter(|r| under(r, SLOT_PRESETS))
            .filter_map(|r| parse_area(r, &context))
            .collect();
        areas.sort_by(|a, b| compare_names(&a.name, &b.name));

        let mut awards: Vec<ContractAward> = ctx
            .records
            .iter()
            .filter(|r| under(r, "contracts/contractgenerator/"))
            .flat_map(contract_awards)
            .collect();
        awards.sort_by(|a, b| {
            compare_names(&a.item, &b.item).then_with(|| compare_names(&a.contract, &b.contract))
        });

        let slots: Vec<&SpawnSlot> = areas
            .iter()
            .flat_map(|a| a.configs.iter().flat_map(|c| c.slots.iter()))
            .collect();
        let summary = Summary {
            areas: areas.len(),
            configs: areas.iter().map(|a| a.configs.len()).sum(),
            slots: slots.len(),
            loot_slots: slots.iter().filter(|s| s.loot.is_some()).count(),
            fixed_slots: slots.iter().filter(|s| s.contains.is_some()).count(),
            contract_awards: awards.len(),
        };
        let mut body = Map::new();
        field(&mut body, "areas", &areas)?;
        field(&mut body, "contractAwards", &awards)?;
        field(&mut body, "summary", &summary)?;
        Ok(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::localization::parse_global_ini;
    use crate::records::normalize_record;
    use serde_json::json;

    const ROOT: &str = "file://./../../../../../libs/foundry/records/";

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

    fn tag(id: &str) -> Value {
        json!({"_RecordName_": format!("Tag.{id}"), "_RecordId_": id})
    }

    fn loot_config() -> Value {
        json!({"_Type_": "LootConfig", "lootTable": null,
               "lootTableV3": format!("{ROOT}lootgeneration/loottables/v3loottable_weapons_large.json"),
               "lootConstraints": {
                   "poolFilter": format!("{ROOT}lootgeneration/filters/v3poolfilter_generic.json"),
                   "secondaryChoices": {"multiLayerRecord": format!("{ROOT}lootgeneration/secondarychoices/v3multichoice_military_common.json")},
                   "totalResultsLimit": 15, "chanceToGenerate": 1}})
    }

    fn rack() -> GameRecord {
        rec(
            "EntityClassDefinition",
            "entities/scitem/usables/weapon_rack_behr_lmg",
            json!({"Components": [{"_Type_": "SEntityComponentDefaultLoadoutParams", "loadout": {"entries": [
                {"_Type_": "SItemPortLoadoutEntryParams",
                 "entityClassReference": format!("{ROOT}entities/scitem/weapons/fps_weapons/behr_lmg_ballistic_01.json")},
                {"_Type_": "SItemPortLoadoutEntryParams", "entityClassReference": null, "entityClassName": ""}]}}]}),
        )
    }

    fn collector() -> GameRecord {
        rec(
            "ContractGenerator",
            "contracts/contractgenerator/thecollector",
            json!({"generators": [{"debugName": "TheCollector_Small_Items", "contracts": [
                {"debugName": "TheCollector_OrbArm", "contractResults": {"contractResults": [{"itemAwardStructure": [
                    {"_Type_": "ItemAwardWeightings", "weighting": 2, "awards": [
                        {"_Type_": "ItemAwardEntityClass", "amountToAward": 1,
                         "entityClass": format!("{ROOT}entities/scitem/characters/arms_02.json")}]}]}]}}]}]}),
        )
    }

    fn records() -> Vec<GameRecord> {
        let tag_database = rec(
            "TagDatabase",
            "tagdatabase/tagdatabase.tagdatabase",
            json!({"tags": [{"tagName": "Global", "children": [
                {"tagName": "LootContents", "_RecordName_": "Tag.lc", "children": [{"tagName": "Weapons", "_RecordName_": "Tag.lcw"}]},
                {"tagName": "LootSize", "children": [{"tagName": "Large", "_RecordName_": "Tag.lsl"}]},
                {"tagName": "LootGeneration", "children": [{"tagName": "CanGenerateAsLoot", "_RecordName_": "Tag.can"}]},
                {"tagName": "Weapon", "children": [{"tagName": "FPS", "_RecordName_": "Tag.fps"}]}]}]}),
        );
        let crate_ = rec(
            "EntityClassDefinition",
            "entities/scitem/lootables/lootable_container_large",
            json!({"Components": [{"_Type_": "SAttachableComponentParams", "AttachDef": {"Localization": {"Name": "@item_crate"}}}]}),
        );
        let rack_preset = rec(
            "HarvestablePreset",
            "harvestable/harvestablepresets/harvestablepreset_rack_behr_lmg",
            json!({"entityClass": format!("{ROOT}entities/scitem/usables/weapon_rack_behr_lmg.json")}),
        );
        let shared = rec(
            "SubHarvestableConfigRecord",
            "harvestable/subharvestableconfigs/racks",
            json!({"subConfig": {"_Type_": "SubHarvestableConfig", "initialSlotsProbability": 0.5,
                "subHarvestables": [{"_Type_": "SubHarvestableSlot", "relativeProbability": 1,
                    "harvestable": format!("{ROOT}harvestable/harvestablepresets/harvestablepreset_rack_behr_lmg.json")}]}}),
        );
        let area = rec(
            "SubHarvestableMultiConfigRecord",
            "harvestable/slotpresets/v3slotpreset_loot_military",
            json!({"multiConfig": {"defaultConfig": null, "taggedConfigs": [
                {"name": "Weapons Large Common", "tagList": {"tags": [tag("lsl"), tag("lcw")]},
                 "subConfig": {"_Type_": "SubHarvestableConfigSingleManual", "subConfigManual": {
                     "_Type_": "SubHarvestableConfig", "initialSlotsProbability": 1,
                     "subHarvestables": [{"_Type_": "SubHarvestableSlot",
                         "harvestableEntityClass": format!("{ROOT}entities/scitem/lootables/lootable_container_large.json"),
                         "relativeProbability": 2, "lootConfig": loot_config()}]}}},
                {"name": "Weapons Rack", "tagList": {"tags": []},
                 "subConfig": {"_Type_": "SubHarvestableConfigSingleRef",
                     "subConfigRef": format!("{ROOT}harvestable/subharvestableconfigs/racks.json")}}]}}),
        );
        vec![tag_database, crate_, rack(), rack_preset, shared, area, collector()]
    }

    // Cases from sc-gamedata src/domains/item-sources.test.ts.
    #[test]
    fn reads_a_loot_configs_v3_table_filter_choices_and_limits() {
        assert_eq!(
            serde_json::to_value(parse_loot_config(Some(&loot_config()))).unwrap(),
            json!({"table": "v3loottable_weapons_large", "version": "v3", "poolFilter": "v3poolfilter_generic",
                   "secondaryChoices": "v3multichoice_military_common", "maxResults": 15.0, "chance": 1.0})
        );
        assert!(parse_loot_config(Some(&Value::Null)).is_none());
    }

    #[test]
    fn collects_a_racks_fixed_loadout_skipping_empty_ports() {
        assert_eq!(loadout_items(&rack().value), vec!["behr_lmg_ballistic_01"]);
    }

    #[test]
    fn builds_areas_from_tagged_configs_following_references() {
        let set = RecordSet::new(records());
        let loc = parse_global_ini("item_crate=Standard Container");
        let body = crate::fixtures::parse_checked(&DOMAIN, &set, &loc);
        let military = &body["areas"][0];
        assert_eq!(military["name"], "v3slotpreset_loot_military");
        assert_eq!(
            military["configs"][0]["spawnTags"],
            json!(["LootSize/Large", "LootContents/Weapons"])
        );
        let slot = &military["configs"][0]["slots"][0];
        assert_eq!(slot["entity"], "lootable_container_large");
        assert_eq!(slot["entityName"], "Standard Container");
        assert_eq!(slot["weight"], json!(2.0));
        assert_eq!(slot["loot"]["table"], "v3loottable_weapons_large");
        assert!(military["configs"][0].get("fillChance").is_none());
        assert_eq!(military["configs"][1]["name"], "Weapons Rack");
        assert_eq!(military["configs"][1]["fillChance"], json!(0.5));
        let rack_slot = &military["configs"][1]["slots"][0];
        assert_eq!(rack_slot["entity"], "weapon_rack_behr_lmg");
        assert_eq!(rack_slot["preset"], "harvestablepreset_rack_behr_lmg");
        assert_eq!(rack_slot["contains"], json!(["behr_lmg_ballistic_01"]));
        assert_eq!(
            body["summary"],
            json!({"areas": 1, "configs": 2, "slots": 2, "lootSlots": 1, "fixedSlots": 1, "contractAwards": 1})
        );
    }

    #[test]
    fn lists_contract_item_awards_with_their_weighting() {
        assert_eq!(
            serde_json::to_value(contract_awards(&collector())).unwrap(),
            json!([{"generator": "TheCollector_Small_Items", "contract": "TheCollector_OrbArm",
                    "item": "arms_02", "amount": 1.0, "weighting": 2.0}])
        );
    }
}
