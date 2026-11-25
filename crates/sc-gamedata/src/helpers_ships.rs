//! Small JavaScript-semantics helpers shared by the ships batch of ports (`quantum`,
//! `containers`, `components`, `cargo`, `loot`, `ships`).
//!
//! Kept in one batch file so parallel ports never edit the same lines; a later cleanup
//! can fold these into the shared helper slots once every batch has landed.

pub mod item_common;
pub mod weapons;

use serde_json::{Map, Value};

pub use crate::js::{js_round, number, round_to, truthy};

/// `Object.values(obj)` of a JSON object as numbers (non-numbers skipped).
pub fn numbers_of(map: &serde_json::Map<String, Value>) -> impl Iterator<Item = f64> + '_ {
    map.values().filter_map(Value::as_f64)
}

/// `entityName` from `wikelo.ts` (used by `cargo.ts`): the file stem of a `file://` entity
/// path, `.json` removed case-insensitively; `None` for a non-string or empty path.
pub use crate::helpers_gameplay::file_ref_stem as entity_name;

/// `src/tags.ts`, for `loot.ts`: `Tag.<guid>` record name -> path below the root
/// (`Rarity/Common`). Here rather than in the `tags` slot so a parallel port of that slot
/// does not conflict; fold together later.
pub type TagIndex = std::collections::HashMap<String, String>;

/// `buildTagIndex`: walk `TagDatabase.tags`, dropping the `Global` root from paths.
pub fn build_tag_index(database: Option<&crate::records::GameRecord>) -> TagIndex {
    fn walk(tag: &Map<String, Value>, path: &[String], index: &mut TagIndex) {
        let name = tag.get("tagName").and_then(Value::as_str).unwrap_or("?");
        let here: Vec<String> = if path.is_empty() && name == "Global" {
            Vec::new()
        } else {
            path.iter().cloned().chain([name.to_string()]).collect()
        };
        if let Some(rn) = tag.get("_RecordName_").and_then(Value::as_str)
            && !here.is_empty()
        {
            index.insert(rn.to_string(), here.join("/"));
        }
        for child in crate::records::array(tag.get("children")) {
            if let Some(c) = child.as_object() {
                walk(c, &here, index);
            }
        }
    }
    let mut index = TagIndex::new();
    if let Some(db) = database {
        for tag in crate::records::array(db.value.get("tags")) {
            if let Some(t) = tag.as_object() {
                walk(t, &[], &mut index);
            }
        }
    }
    index
}

const DROPPED_KEYS: [&str; 4] = ["_Type_", "_RecordPath_", "_RecordId_", "_Pointer_"];

/// Last path segment of a `file://` reference, minus `.json` (case-insensitive).
fn tag_file_name(reference: &str) -> &str {
    let file = reference.rsplit('/').next().unwrap_or(reference);
    let cut = file.len().saturating_sub(5);
    match file.get(cut..) {
        Some(ext) if file.len() >= 5 && ext.eq_ignore_ascii_case(".json") => &file[..cut],
        _ => file,
    }
}

/// `compactValue`: type tags and record ids go, tag references become tag paths, record
/// references and `file://` paths become names, nulls and empty strings/lists/objects
/// disappear (`None`).
pub fn compact_value(value: &Value, tags: &TagIndex) -> Option<Value> {
    match value {
        Value::Null => None,
        Value::String(s) => {
            if s.starts_with("file://") {
                Some(Value::String(tag_file_name(s).to_string()))
            } else if s.is_empty() {
                None
            } else {
                Some(value.clone())
            }
        }
        Value::Array(a) => {
            let out: Vec<Value> = a.iter().filter_map(|v| compact_value(v, tags)).collect();
            (!out.is_empty()).then_some(Value::Array(out))
        }
        Value::Object(obj) => {
            if let Some(rn) = obj.get("_RecordName_").and_then(Value::as_str) {
                if rn.starts_with("Tag.") {
                    return Some(Value::String(
                        tags.get(rn).cloned().unwrap_or_else(|| rn.to_string()),
                    ));
                }
                let has_fields = obj
                    .keys()
                    .any(|k| !DROPPED_KEYS.contains(&k.as_str()) && k != "_RecordName_");
                if !has_fields {
                    let short = rn.find('.').map_or(rn, |i| &rn[i + 1..]);
                    return Some(Value::String(short.to_string()));
                }
            }
            let mut out = Map::new();
            for (k, v) in obj {
                if DROPPED_KEYS.contains(&k.as_str()) {
                    continue;
                }
                if let Some(c) = compact_value(v, tags) {
                    out.insert(k.clone(), c);
                }
            }
            (!out.is_empty()).then_some(Value::Object(out))
        }
        _ => Some(value.clone()),
    }
}

/// Read a dotted path below an object (`at` on a struct the TypeScript holds as a record).
pub fn at_map<'a>(map: &'a Map<String, Value>, dotted: &str) -> Option<&'a Value> {
    let (first, rest) = match dotted.split_once('.') {
        Some((f, r)) => (f, Some(r)),
        None => (dotted, None),
    };
    let v = map.get(first)?;
    match rest {
        Some(r) => crate::records::at(v, r),
        None => Some(v),
    }
}

/// An object from `(key, value)` pairs, `None` values dropped (JS `undefined`); `None` when
/// nothing is left (the TypeScript's `compact`).
pub fn compact_obj(fields: Vec<(&str, Option<Value>)>) -> Option<Map<String, Value>> {
    let out: Map<String, Value> = fields
        .into_iter()
        .filter_map(|(k, v)| v.map(|v| (k.to_string(), v)))
        .collect();
    (!out.is_empty()).then_some(out)
}

/// `Some(number)` as a JSON value.
pub fn num_v(x: Option<f64>) -> Option<Value> {
    x.map(number)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_like_javascript() {
        assert_eq!(js_round(2.5), 3.0);
        assert_eq!(js_round(-2.5), -2.0);
        assert_eq!(js_round(-2.6), -3.0);
        assert_eq!(js_round(0.49999999999999994), 0.0);
        assert_eq!(round_to(1.23456, 100.0), 1.23);
        assert_eq!(
            round_to(1.005, 100.0),
            1.0,
            "1.005*100 = 100.49999999999999"
        );
        assert!(truthy(Some(1.0)) && !truthy(Some(0.0)) && !truthy(None));
    }

    #[test]
    fn tags_and_compaction() {
        // Cases from sc-gamedata src/domains/personal.test.ts ("tags").
        use serde_json::json;
        let db = crate::records::normalize_record(
            json!({"_RecordName_": "TagDatabase.TagDatabase", "_RecordValue_": {"_Type_": "TagDatabase", "tags": [
                {"_RecordName_": "Tag.root", "tagName": "Global", "children": [
                    {"_RecordName_": "Tag.rarity", "tagName": "Rarity", "children": [
                        {"_RecordName_": "Tag.common", "tagName": "Common", "children": []}, null]}]}]}}),
            "libs/foundry/records/tagdatabase/tagdatabase.tagdatabase",
        )
        .unwrap();
        let tags = build_tag_index(Some(&db));
        assert_eq!(
            tags.get("Tag.common").map(String::as_str),
            Some("Rarity/Common")
        );
        assert!(!tags.contains_key("Tag.root"));
        let v = json!({"_Type_": "X",
            "archetype": "file://./../../../../libs/foundry/records/lootgeneration/lootarchetypes/lootarchetype_armorcache_rare.json",
            "tag": {"_RecordName_": "Tag.common", "_RecordId_": "common"},
            "resource": {"_RecordName_": "ResourceType.Gold", "_RecordId_": "g"},
            "weight": 0, "empty": [], "missing": null});
        assert_eq!(
            compact_value(&v, &tags),
            Some(
                json!({"archetype": "lootarchetype_armorcache_rare", "tag": "Rarity/Common", "resource": "Gold", "weight": 0})
            )
        );
        assert_eq!(compact_value(&json!({"_Type_": "X"}), &tags), None);
        assert_eq!(
            compact_value(&json!({"_RecordName_": "Tag.zzz"}), &tags),
            Some(json!("Tag.zzz"))
        );
    }

    #[test]
    fn entity_names() {
        use serde_json::json;
        let v = json!("file://./../../entities/scitem/x/Some_Item.JSON");
        assert_eq!(entity_name(Some(&v)), Some("Some_Item"));
        assert_eq!(entity_name(Some(&json!("a/"))), None);
        assert_eq!(entity_name(Some(&json!(""))), None);
        assert_eq!(entity_name(Some(&json!("x.json"))), Some("x"));
    }
}
