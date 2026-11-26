//! `game-tag-index.json`: tag path -> the items carrying it and the loot records selecting
//! it, worked out once per build (spec `cross-dataset-derivations.md` D3).
//!
//! Derived from datasets, not game files, like [`crate::item_obtain`]: it needs
//! `game-item-tags.json` and `game-loot.json` beside it, is rebuilt after every extract that
//! leaves both on disk and removed when one is missing.
//!
//! A loot selector picks an item when the item carries the selected path or a path below it
//! ([`crate::query::obtain::has_tag`]), so `items` lists the classes carrying the path
//! itself and `matchCount` counts the classes it or a path below it picks. Item classes are
//! listed once (`classes[]`, in `game-item-tags` order) and rows point at them by index.
//! Shape:
//!
//! ```text
//! { _source, _extracted, _build, _schema,
//!   classes[],
//!   tags: { <path>: { items[], matchCount, archetypes[], secondaryChoices[] } },
//!   summary }
//! ```
//!
//! `archetypes` names the `game-loot` archetypes whose primary entries or secondary groups
//! (weight above 0) name the path as a positive tag (inline V3 table archetypes as `<table>:<entry>`, the
//! obtain query's name for them); `secondaryChoices` the single-layer secondary choice
//! records whose choices do. Tags only an excluded or negative list names are not
//! "selected". No estimates: every list is an exact match of the published datasets.

use crate::output::to_js_json;
use crate::query::obtain::{Archetype, LootData, entry_selector, normalise_archetype};
use crate::query::types::ItemTags;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Output base name.
pub const NAME: &str = "game-tag-index";
/// Output file name.
pub const FILE: &str = "game-tag-index.json";
/// `_schema` version (`game-tag-index@1`; bump rule:
/// [`crate::domain::Domain::schema_version`]).
pub const SCHEMA_VERSION: u32 = 1;
const SOURCE: &str = "Derived from game-item-tags and game-loot";

/// What [`write_tag_index`] wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    pub file: PathBuf,
    pub tags: usize,
    pub bytes: usize,
}

#[derive(Default)]
struct Row {
    items: Vec<usize>,
    match_count: usize,
    archetypes: BTreeSet<String>,
    choices: BTreeSet<String>,
}

/// `path` and every path above it (`A/B/C` -> `A/B/C`, `A/B`, `A`).
fn self_and_parents(path: &str) -> impl Iterator<Item = &str> {
    std::iter::successors(Some(path), |p| p.rfind('/').map(|i| &p[..i]))
}

/// Positive tags of every selector of an archetype (primary entries and secondary groups)
/// that can come up (weight above 0, as the obtain query rolls them).
fn archetype_tags(a: &Archetype) -> impl Iterator<Item = &String> {
    a.primary
        .iter()
        .chain(a.layers.iter().flat_map(|l| &l.choices))
        .filter(|s| s.weight > 0.0)
        .flat_map(|s| &s.positive)
}

/// Tag paths ordered by UTF-16 code units, as `game-item-tags` sorts them.
fn utf16_key(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

type Rows = BTreeMap<Vec<u16>, (String, Row)>;

fn row<'a>(rows: &'a mut Rows, path: &str) -> &'a mut Row {
    &mut rows
        .entry(utf16_key(path))
        .or_insert_with(|| (path.to_string(), Row::default()))
        .1
}

/// The dataset body (`classes`, `tags`, `summary`).
pub fn build_body(items: &[ItemTags], loot: &LootData) -> Map<String, Value> {
    let mut rows = Rows::new();
    let mut classes: Vec<&str> = Vec::new();
    for item in items.iter().filter(|i| !i.tags.is_empty()) {
        let ix = classes.len();
        classes.push(&item.class_name);
        let mut counted: BTreeSet<&str> = BTreeSet::new();
        for t in &item.tags {
            row(&mut rows, t).items.push(ix);
            for p in self_and_parents(t) {
                if counted.insert(p) {
                    row(&mut rows, p).match_count += 1;
                }
            }
        }
    }

    let mut archetypes: Vec<Archetype> = loot
        .archetypes
        .iter()
        .map(|raw| {
            let name = raw.get("name").and_then(Value::as_str).unwrap_or("inline");
            normalise_archetype(raw, name)
        })
        .collect();
    // Inline V3 table archetypes (`{entries[]}`, no `lootArchetypeRecord`).
    for t in &loot.tables {
        let table = t.get("name").and_then(Value::as_str).unwrap_or("");
        for slot in t
            .get("lootArchetypes")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(inline) = slot
                .get("archetype")
                .filter(|a| a.is_object() && a.get("lootArchetypeRecord").is_none())
            else {
                continue;
            };
            let entry = slot.get("name").and_then(Value::as_str).unwrap_or("inline");
            archetypes.push(normalise_archetype(inline, &format!("{table}:{entry}")));
        }
    }
    let mut selecting = 0usize;
    for a in &archetypes {
        let mut any = false;
        for t in archetype_tags(a) {
            any |= row(&mut rows, t).archetypes.insert(a.name.clone());
        }
        selecting += usize::from(any);
    }
    let mut choice_records = 0usize;
    for c in &loot.secondary_choices {
        let (Some(name), Some(choices)) = (
            c.get("name").and_then(Value::as_str),
            c.get("choices").and_then(Value::as_array),
        ) else {
            continue;
        };
        let mut any = false;
        for t in choices
            .iter()
            .map(entry_selector)
            .filter(|ch| ch.weight > 0.0)
            .flat_map(|ch| ch.positive)
        {
            any |= row(&mut rows, &t).choices.insert(name.to_string());
        }
        choice_records += usize::from(any);
    }

    let mut tags = Map::new();
    let (mut with_items, mut selected) = (0usize, 0usize);
    for (_, (path, r)) in rows {
        let mut o = Map::new();
        if !r.items.is_empty() {
            with_items += 1;
            o.insert("items".into(), json!(r.items));
        }
        o.insert("matchCount".into(), json!(r.match_count));
        if !r.archetypes.is_empty() || !r.choices.is_empty() {
            selected += 1;
        }
        if !r.archetypes.is_empty() {
            o.insert("archetypes".into(), json!(r.archetypes));
        }
        if !r.choices.is_empty() {
            o.insert("secondaryChoices".into(), json!(r.choices));
        }
        tags.insert(path, Value::Object(o));
    }
    let mut body = Map::new();
    body.insert("classes".into(), json!(classes));
    let summary = json!({
        "tags": tags.len(),
        "classes": classes.len(),
        "withItems": with_items,
        "selected": selected,
        "archetypes": selecting,
        "secondaryChoices": choice_records,
    });
    body.insert("tags".into(), Value::Object(tags));
    body.insert("summary".into(), summary);
    body
}

fn read_json(file: &Path) -> Option<Value> {
    serde_json::from_slice(&std::fs::read(file).ok()?).ok()
}

/// The whole document from the two datasets: header (`_extracted` / `_build` copied from
/// item-tags), then [`build_body`]. `None` when either lacks its collections.
pub fn build_document(tags: &Value, loot: &Value) -> Option<Value> {
    let items: Vec<ItemTags> = serde_json::from_value(tags.get("items")?.clone()).ok()?;
    let loot: LootData = serde_json::from_value(loot.clone()).ok()?;
    let mut doc = Map::new();
    doc.insert("_source".into(), json!(SOURCE));
    if let Some(e) = tags.get("_extracted") {
        doc.insert("_extracted".into(), e.clone());
    }
    if let Some(b) = tags.get("_build") {
        doc.insert("_build".into(), b.clone());
    }
    doc.insert(
        "_schema".into(),
        json!(crate::output::schema_tag(NAME, SCHEMA_VERSION)),
    );
    doc.extend(build_body(&items, &loot));
    Some(Value::Object(doc))
}

/// Rebuild `game-tag-index.json` in `dir` from the datasets beside it; when an input is
/// missing, write nothing and remove a file already there.
pub fn write_tag_index(dir: &Path) -> std::io::Result<Option<Written>> {
    let file = dir.join(FILE);
    let doc = read_json(&dir.join("game-item-tags.json")).and_then(|t| {
        let loot = read_json(&dir.join("game-loot.json"))?;
        build_document(&t, &loot)
    });
    let Some(doc) = doc else {
        return match std::fs::remove_file(&file) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(None),
        };
    };
    let text = to_js_json(&doc);
    std::fs::write(&file, &text)?;
    let tags = doc
        .get("tags")
        .and_then(Value::as_object)
        .map_or(0, Map::len);
    Ok(Some(Written {
        file,
        tags,
        bytes: text.len(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(class: &str, tags: &[&str]) -> ItemTags {
        ItemTags {
            name: class.into(),
            class_name: class.into(),
            id: class.into(),
            tags: tags.iter().map(|t| t.to_string()).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn indexes_items_and_selecting_loot_records() {
        let items = vec![
            item(
                "rifle",
                &["Weapon/FPS/Rifle", "LootGeneration/LootRarity/Common"],
            ),
            item("pistol", &["Weapon/FPS/Pistol"]),
            item("untagged", &[]),
        ];
        let loot: LootData = serde_json::from_value(json!({
            "tables": [{"name": "v3t", "lootArchetypes": [
                {"name": "Guns", "archetype": {"entries": [{"name": "g", "weight": 1,
                    "selector": {"tags": {"positiveTags": ["Weapon/FPS/Pistol"], "negativeTags": ["Weapon/FPS/Rifle"]}}}]}},
                {"name": "Ref", "archetype": {"lootArchetypeRecord": "arch"}}]}],
            "archetypes": [{"name": "arch", "excludedTags": {"tags": ["Item/Food"]},
                "primaryOrGroup": {"entries": [{"tag": "Weapon/FPS", "weight": 1}]},
                "secondaryOrGroups": [{"groupName": "Rarity", "entries": [{"tag": "LootGeneration/LootRarity/Common", "weight": 1}]}]}],
            "secondaryChoices": [{"name": "rarity_common", "choices": [
                {"name": "c", "weight": 1, "selector": {"tags": {"positiveTags": ["LootGeneration/LootRarity/Common"]}}},
                {"name": "never", "weight": 0, "selector": {"tags": {"positiveTags": ["Item/Never"]}}}]}]
        }))
        .unwrap();
        let body = build_body(&items, &loot);
        assert_eq!(body["classes"], json!(["rifle", "pistol"]));
        let t = &body["tags"];
        assert_eq!(
            t["Weapon/FPS/Rifle"],
            json!({"items": [0], "matchCount": 1})
        );
        assert_eq!(
            t["Weapon/FPS/Pistol"],
            json!({"items": [1], "matchCount": 1, "archetypes": ["v3t:Guns"]})
        );
        assert_eq!(
            t["Weapon/FPS"],
            json!({"matchCount": 2, "archetypes": ["arch"]})
        );
        assert_eq!(t["Weapon"], json!({"matchCount": 2}));
        assert_eq!(
            t["LootGeneration/LootRarity/Common"],
            json!({"items": [0], "matchCount": 1, "archetypes": ["arch"], "secondaryChoices": ["rarity_common"]})
        );
        assert!(t.get("Item/Food").is_none(), "excluded tags do not select");
        assert!(
            t.get("Item/Never").is_none(),
            "a 0-weight choice does not select"
        );
        assert_eq!(body["summary"]["archetypes"], 2);
        assert_eq!(body["summary"]["secondaryChoices"], 1);
        // UTF-16 order: "LootGeneration..." < "Weapon".
        let keys: Vec<&String> = t.as_object().unwrap().keys().collect();
        assert_eq!(keys[0], "LootGeneration");
    }

    #[test]
    fn header_copies_build_and_stamps_schema() {
        let tags = json!({"_build": "4.10.201.1", "items": [{"name": "a", "className": "a", "id": "a", "tags": ["X"]}]});
        let doc = build_document(&tags, &json!({"tables": []})).unwrap();
        assert_eq!(doc["_build"], "4.10.201.1");
        assert_eq!(doc["_schema"], "game-tag-index@1");
        assert_eq!(doc["tags"]["X"], json!({"items": [0], "matchCount": 1}));
    }
}
