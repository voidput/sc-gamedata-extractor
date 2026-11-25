//! Loot: what containers, corpses and caches roll, and with what weight (port of
//! `sc-gamedata/src/domains/loot.ts`).
//!
//! A loot table lists archetypes with weights; an archetype picks items by tag ("any
//! common armour piece, not a helmet") rather than naming them, with secondary groups such
//! as Rarity weighting the result. Two generations of the format coexist: the original
//! `LootTable`/`LootArchetype` and the V3 records, which add choice limits, stack sizes,
//! spawn-with rules and pool filters.
//!
//! Both are kept in the game's own shape, compacted: tag GUIDs become tag paths
//! (`Rarity/Common`), record references become names. Expanding a tag into the concrete
//! items carrying it is left to the consumer, since it needs every item's tags.
//!
//! `tags.ts` (`buildTagIndex`, `compactValue`) lives in [`crate::helpers_ships`].

use crate::collate::compare_names;
use crate::domain::{Domain, DomainError, ParseContext, field};
use crate::helpers_ships::{TagIndex, build_tag_index, compact_value};
use crate::records::GameRecord;
use serde::Serialize;
use serde_json::{Map, Value};

pub static DOMAIN: Loot = Loot;

pub struct Loot;

/// `{ name, recordName, version, ...compacted }`: a spread key equal to one of the first
/// three replaces its value in place, as in JS.
fn spread(head: Vec<(&str, Value)>, compacted: Option<Value>) -> Map<String, Value> {
    let mut out: Map<String, Value> = head
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
    if let Some(Value::Object(m)) = compacted {
        for (k, v) in m {
            out.insert(k, v);
        }
    }
    out
}

/// `parseLootRecord`: V3 records wrap their content one level down (`lootTable`,
/// `lootArchetype`).
pub fn parse_loot_record(record: &GameRecord, tags: &TagIndex) -> Map<String, Value> {
    let v3 = record.ty.contains("V3");
    let present = |k: &str| record.value.get(k).filter(|v| !v.is_null());
    let body = if v3 {
        present("lootTable")
            .or_else(|| present("lootArchetype"))
            .unwrap_or(&record.value)
    } else {
        &record.value
    };
    spread(
        vec![
            ("name", Value::String(record.short_name.clone())),
            ("recordName", Value::String(record.name.clone())),
            ("version", Value::String(if v3 { "v3" } else { "v1" }.into())),
        ],
        compact_value(body, tags),
    )
}

fn by_name(a: &Map<String, Value>, b: &Map<String, Value>) -> std::cmp::Ordering {
    let n = |m: &Map<String, Value>| {
        m.get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    compare_names(&n(a), &n(b))
}

const TABLE_TYPES: [&str; 2] = ["LootTable", "LootTableV3Record"];
const ARCHETYPE_TYPES: [&str; 2] = ["LootArchetype", "LootArchetypeV3Record"];

#[derive(Debug, Serialize)]
struct Summary {
    tables: usize,
    archetypes: usize,
    tags: usize,
}

impl Domain for Loot {
    fn name(&self) -> &'static str {
        "game-loot"
    }

    fn description(&self) -> &'static str {
        "Loot tables and archetypes with weights, by tag, plus pool filters and holiday loot."
    }

    fn filters(&self) -> &'static [&'static str] {
        &["**/lootgeneration/**", "**/tagdatabase/**"]
    }

    fn parse(&self, ctx: &ParseContext<'_>) -> Result<Map<String, Value>, DomainError> {
        let tags = build_tag_index(ctx.records.iter().find(|r| r.ty == "TagDatabase"));
        let loot: Vec<&GameRecord> = ctx
            .records
            .iter()
            .filter(|r| r.record_path.contains("/lootgeneration/"))
            .collect();

        let of_types = |types: &[&str]| {
            let mut v: Vec<Map<String, Value>> = loot
                .iter()
                .filter(|r| types.contains(&r.ty.as_str()))
                .map(|r| parse_loot_record(r, &tags))
                .collect();
            v.sort_by(by_name);
            v
        };
        let tables = of_types(&TABLE_TYPES);
        let archetypes = of_types(&ARCHETYPE_TYPES);
        let other = |dir: &str| {
            let needle = format!("/lootgeneration/{dir}/");
            let mut v: Vec<Map<String, Value>> = loot
                .iter()
                .filter(|r| r.record_path.contains(&needle))
                .map(|r| {
                    spread(
                        vec![
                            ("name", Value::String(r.short_name.clone())),
                            ("recordName", Value::String(r.name.clone())),
                        ],
                        compact_value(&r.value, &tags),
                    )
                })
                .collect();
            v.sort_by(by_name);
            v
        };

        let global = loot.iter().find(|r| r.ty == "LootGenerationGlobalParams");
        let special_events = global
            .and_then(|g| g.value.get("specialEventArchetypes"))
            .and_then(|v| compact_value(v, &tags))
            .unwrap_or_else(|| Value::Array(Vec::new()));

        let mut body = Map::new();
        field(&mut body, "tables", &tables)?;
        field(&mut body, "archetypes", &archetypes)?;
        field(&mut body, "secondaryChoices", &other("secondarychoices"))?;
        field(&mut body, "filters", &other("filters"))?;
        body.insert("specialEvents".into(), special_events);
        field(
            &mut body,
            "summary",
            &Summary {
                tables: tables.len(),
                archetypes: archetypes.len(),
                tags: tags.len(),
            },
        )?;
        Ok(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::NoFiles;
    use crate::localization::Localization;
    use crate::records::{RecordSet, normalize_record};
    use serde_json::json;

    fn rec(ty: &str, path: &str, mut value: Value) -> GameRecord {
        let name = path.rsplit('/').next().unwrap();
        value["_Type_"] = json!(ty);
        normalize_record(
            json!({"_RecordName_": format!("{ty}.{name}"), "_RecordId_": format!("id-{name}"), "_RecordValue_": value}),
            &format!("libs/foundry/records/{path}"),
        )
        .unwrap()
    }

    fn file_ref(path: &str) -> String {
        format!("file://./../../../../libs/foundry/records/{path}.json")
    }

    // Case from sc-gamedata src/domains/personal.test.ts ("loot").
    #[test]
    fn unwraps_v3_records_and_resolves_tags_in_both_generations() {
        let mut db = rec(
            "TagDatabase",
            "tagdatabase/tagdatabase.tagdatabase",
            json!({"tags": [{"_RecordName_": "Tag.root", "tagName": "Global", "children": [
                {"_RecordName_": "Tag.rarity", "tagName": "Rarity", "children": [
                    {"_RecordName_": "Tag.common", "tagName": "Common", "children": []}, null]}]}]}),
        );
        db.short_name = "TagDatabase".into();
        let set = RecordSet::new(vec![
            db,
            rec(
                "LootTable",
                "lootgeneration/loottables/armor",
                json!({"lootArchetypes": [{"archetype": file_ref("lootgeneration/lootarchetypes/a"), "weight": 0.4}]}),
            ),
            rec(
                "LootTableV3Record",
                "lootgeneration/loottables/v3loottable_medical",
                json!({"lootTable": {"lootArchetypes": [{"name": "Medpen", "weight": 100, "optionalData": [{"choiceLimit": 5}]}]}}),
            ),
            rec(
                "LootArchetype",
                "lootgeneration/lootarchetypes/a",
                json!({"primaryOrGroup": {"entries": [{"name": "Any common",
                    "tag": {"_RecordName_": "Tag.common", "_RecordId_": "common"}, "weight": 1}]}}),
            ),
        ]);
        let loc = Localization::new();
        let out = DOMAIN
            .parse(&ParseContext::new(&set, &loc, &NoFiles))
            .unwrap();
        assert_eq!(
            out["tables"],
            json!([
                {"name": "armor", "recordName": "LootTable.armor", "version": "v1", "lootArchetypes": [{"archetype": "a", "weight": 0.4}]},
                {"name": "v3loottable_medical", "recordName": "LootTableV3Record.v3loottable_medical", "version": "v3",
                    "lootArchetypes": [{"name": "Medpen", "weight": 100, "optionalData": [{"choiceLimit": 5}]}]}
            ])
        );
        assert_eq!(
            out["archetypes"][0]["primaryOrGroup"]["entries"][0]["tag"],
            "Rarity/Common"
        );
        assert_eq!(out["specialEvents"], json!([]));
        assert_eq!(
            out["summary"],
            json!({"tables": 2, "archetypes": 1, "tags": 2})
        );
    }

    #[test]
    fn spread_replaces_in_place() {
        let m = spread(
            vec![("name", json!("a")), ("version", json!("v1"))],
            Some(json!({"x": 1, "name": "b"})),
        );
        assert_eq!(m.keys().collect::<Vec<_>>(), vec!["name", "version", "x"]);
        assert_eq!(m["name"], "b");
    }
}
