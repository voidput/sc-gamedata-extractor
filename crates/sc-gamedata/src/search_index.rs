//! `game-search-index.json`: the fuzzy-find entries, precomputed (port of
//! `sc-gamedata/src/search-index.ts`, `query/search-index.ts` and the entry half of
//! `query/fuzzy.ts`).
//!
//! Derived from two datasets rather than from game files: `game-item-tags.json` and
//! `game-ships.json`. `sc-gamedata` rebuilds it after every parse that leaves both on disk
//! and removes it when either is missing, so a stale index never sits beside new data;
//! [`write_search_index`] does the same.
//!
//! Shape (format 1): `{ _source, _build, format, columns, makers, types, items, ships }`,
//! rows `[key, name, maker, type, family, noise]` with trailing zeros trimmed: `name` is 0
//! when it equals `key`, `maker`/`type` are 1-based indices into `makers`/`types` (0 for
//! none), `family` is the weapon family or 0, `noise` 1 for an entry the default list
//! drops. One row per line, compact otherwise ([`serialize`]).
//!
//! Values are kept as the JSON values the datasets hold, so `undefined` (`None`), `null`
//! and strings stay distinct exactly where the TypeScript distinguishes them.

use crate::output::to_js_json_compact;
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Output base name.
pub const NAME: &str = "game-search-index";
/// `SEARCH_INDEX_FILE`.
pub const FILE: &str = "game-search-index.json";
/// `SEARCH_INDEX_FORMAT`.
pub const FORMAT: u64 = 1;
const COLUMNS: [&str; 6] = ["key", "name", "maker", "type", "family", "noise"];
const SOURCE: &str =
    "Derived from game-item-tags.json and game-ships.json (see src/search-index.ts)";

/// Item types that are world furniture rather than things a player looks up.
const NOISE_TYPES: &[&str] = &[
    "Cargo",
    "Door",
    "Seat",
    "Usable",
    "UNDEFINED",
    "Light",
    "Room",
    "Display",
    "Elevator",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Item,
    Ship,
}

/// `Entry` in `fuzzy.ts`. `None` is `undefined`.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub kind: Kind,
    pub key: Option<Value>,
    pub name: Option<Value>,
    pub maker: Option<Value>,
    /// Item type, or the ship's role.
    pub ty: Option<Value>,
    /// Weapon family (items only).
    pub family: Option<String>,
}

/// JavaScript truthiness of an optional JSON value.
fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|x| x != 0.0 && !x.is_nan()),
        Some(Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

/// `x ?? y` on optional JSON values.
fn coalesce(x: Option<&Value>, y: Option<&Value>) -> Option<Value> {
    match x {
        None | Some(Value::Null) => y.cloned(),
        Some(v) => Some(v.clone()),
    }
}

fn tag_strings(item: &Value) -> Vec<&str> {
    item.get("tags")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

/// `/^Weapon\/FPS\/(Stocked|Pistol|Shouldered|Melee)\/.+/`.
fn is_weapon_family_tag(tag: &str) -> bool {
    let Some(rest) = tag.strip_prefix("Weapon/FPS/") else {
        return false;
    };
    ["Stocked/", "Pistol/", "Shouldered/", "Melee/"]
        .iter()
        .filter_map(|p| rest.strip_prefix(p))
        .any(|tail| {
            tail.chars()
                .next()
                .is_some_and(|c| !matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}'))
        })
}

/// `weaponFamily`: leaf of the most specific Weapon/FPS family tag, e.g. "Verdict".
pub fn weapon_family(tags: &[&str]) -> Option<String> {
    let mut families: Vec<&str> = tags
        .iter()
        .copied()
        .filter(|t| is_weapon_family_tag(t))
        .collect();
    // Stable sort, deepest first.
    families.sort_by_key(|t| std::cmp::Reverse(t.split('/').count()));
    let parts: Vec<&str> = families.first()?.split('/').collect();
    // Weapon/FPS/Pistol/Hellion has the family at depth 4, Weapon/FPS/Stocked/LMG/Verdict at 5.
    let min_depth = if parts[2] == "Stocked" { 5 } else { 4 };
    (parts.len() >= min_depth).then(|| parts[parts.len() - 1].to_string())
}

/// `shipKey`.
pub fn ship_key(record_name: &str) -> &str {
    record_name
        .strip_prefix("EntityClassDefinition.")
        .unwrap_or(record_name)
}

/// `buildEntries(items, ships, { all })`.
pub fn build_entries(items: &[Value], ships: &[Value], all: bool) -> Vec<Entry> {
    let mut entries = Vec::new();
    for item in items {
        let ty = item.get("type");
        let noise_type = match ty {
            Some(Value::String(s)) => NOISE_TYPES.contains(&s.as_str()),
            _ => false,
        };
        if !all && (!truthy(ty) || noise_type) {
            continue;
        }
        let tags = tag_strings(item);
        let maker = match tags.iter().find(|t| t.starts_with("Manufacturer/")) {
            Some(t) => Some(Value::String(t["Manufacturer/".len()..].to_string())),
            None => item.get("manufacturer").cloned(),
        };
        let family = weapon_family(&tags);
        let class_name = item.get("className").cloned();
        let name = match (&family, &class_name) {
            (Some(f), _) if truthy(item.get("isTemplate")) && !f.is_empty() => {
                let class = class_name
                    .as_ref()
                    .map(js_string_of)
                    .unwrap_or_else(|| "undefined".to_string());
                Some(Value::String(format!("{f} (unnamed: {class})")))
            }
            _ => item.get("name").cloned(),
        };
        entries.push(Entry {
            kind: Kind::Item,
            key: class_name,
            name,
            maker,
            ty: ty.cloned(),
            family,
        });
    }
    for ship in ships {
        if truthy(ship.get("isTemplate")) && !all {
            continue;
        }
        let key = match ship.get("recordName") {
            Some(Value::String(s)) => Some(Value::String(ship_key(s).to_string())),
            other => other.cloned(),
        };
        let maker = match ship.get("manufacturer") {
            None | Some(Value::Null) => None,
            Some(Value::String(s)) => Some(Value::String(s.to_uppercase())),
            Some(v) => Some(v.clone()),
        };
        entries.push(Entry {
            kind: Kind::Ship,
            key,
            name: ship.get("name").cloned(),
            maker,
            ty: coalesce(ship.get("role"), ship.get("career")),
            family: None,
        });
    }
    entries
}

/// `String(value)` for the values a class name can hold.
fn js_string_of(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// The index document (`SearchIndexDoc`), in field order.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchIndexDoc {
    /// `None` when neither input has a `_build` (the field is then left out).
    pub build: Option<Value>,
    pub format: u64,
    pub makers: Vec<Value>,
    pub types: Vec<Value>,
    pub items: Vec<Vec<Value>>,
    pub ships: Vec<Vec<Value>>,
}

fn zero() -> Value {
    Value::from(0)
}

fn is_zero(v: &Value) -> bool {
    matches!(v, Value::Number(n) if n.as_f64() == Some(0.0))
}

/// `buildSearchIndex(items, ships, build)`.
pub fn build_search_index(
    items: &[Value],
    ships: &[Value],
    build: Option<Value>,
) -> SearchIndexDoc {
    let all = build_entries(items, ships, true);
    let kept = build_entries(items, ships, false);

    // The default list is a subsequence of the full one; whatever the walk skips is noise.
    let mut noise = vec![true; all.len()];
    let mut j = 0;
    for (i, e) in all.iter().enumerate() {
        if j >= kept.len() {
            break;
        }
        if *e == kept[j] {
            noise[i] = false;
            j += 1;
        }
    }

    let mut doc = SearchIndexDoc {
        build,
        format: FORMAT,
        makers: Vec::new(),
        types: Vec::new(),
        items: Vec::new(),
        ships: Vec::new(),
    };
    let mut maker_ix: HashMap<String, usize> = HashMap::new();
    let mut type_ix: HashMap<String, usize> = HashMap::new();
    fn intern(
        value: &Option<Value>,
        list: &mut Vec<Value>,
        ix: &mut HashMap<String, usize>,
    ) -> usize {
        let Some(v) = value else { return 0 };
        // Map keys are SameValueZero; the JSON text tells strings, null and numbers apart.
        let k = v.to_string();
        *ix.entry(k).or_insert_with(|| {
            list.push(v.clone());
            list.len()
        })
    }
    for (i, e) in all.iter().enumerate() {
        let undefined = || Value::Null; // JSON.stringify prints undefined in an array as null.
        let key = e.key.clone().unwrap_or_else(undefined);
        let name = if e.name == e.key {
            zero()
        } else {
            e.name.clone().unwrap_or_else(undefined)
        };
        let maker = intern(&e.maker, &mut doc.makers, &mut maker_ix);
        let ty = intern(&e.ty, &mut doc.types, &mut type_ix);
        let mut row = vec![
            key,
            name,
            Value::from(maker),
            Value::from(ty),
            e.family.clone().map(Value::String).unwrap_or_else(zero),
            Value::from(u8::from(noise[i])),
        ];
        while row.len() > 1 && row.last().is_some_and(is_zero) {
            row.pop();
        }
        match e.kind {
            Kind::Item => doc.items.push(row),
            Kind::Ship => doc.ships.push(row),
        }
    }
    doc
}

/// Why an index document cannot be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("Unknown search index format {found}; expected {FORMAT}")]
pub struct UnknownFormat {
    pub found: String,
}

/// `loadSearchIndexEntries(doc, { all })`: the entries [`build_entries`] gives for the
/// datasets the document was built from.
pub fn load_search_index_entries(doc: &Value, all: bool) -> Result<Vec<Entry>, UnknownFormat> {
    let format = doc.get("format");
    if format.and_then(Value::as_f64) != Some(FORMAT as f64) {
        return Err(UnknownFormat {
            found: format.map_or("undefined".to_string(), js_string_of),
        });
    }
    let list = |k: &str| -> Vec<Value> {
        doc.get(k)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    };
    let (makers, types) = (list("makers"), list("types"));
    let pick = |list: &[Value], ix: &Value| -> Option<Value> {
        if !truthy(Some(ix)) {
            return None;
        }
        let n = ix.as_f64()? as usize;
        n.checked_sub(1).and_then(|i| list.get(i)).cloned()
    };
    let mut entries = Vec::new();
    for (kind, rows) in [(Kind::Item, list("items")), (Kind::Ship, list("ships"))] {
        for row in rows {
            let cell = |i: usize| row.get(i).cloned().unwrap_or_else(zero);
            let key = row.get(0).cloned();
            let (name, maker, ty, family, noise) = (cell(1), cell(2), cell(3), cell(4), cell(5));
            if truthy(Some(&noise)) && !all {
                continue;
            }
            entries.push(Entry {
                kind,
                name: if is_zero(&name) {
                    key.clone()
                } else {
                    Some(Value::String(js_string_of(&name)))
                },
                key,
                maker: pick(&makers, &maker),
                ty: pick(&types, &ty),
                family: match kind {
                    Kind::Item if !is_zero(&family) => Some(js_string_of(&family)),
                    _ => None,
                },
            });
        }
    }
    Ok(entries)
}

/// `serializeSearchIndex`: one row per line, compact like a single-line dump.
pub fn serialize(doc: &SearchIndexDoc) -> String {
    let rows = |list: &[Vec<Value>]| -> String {
        if list.is_empty() {
            return "[]".to_string();
        }
        let lines: Vec<String> = list
            .iter()
            .map(|r| to_js_json_compact(&Value::Array(r.clone())))
            .collect();
        format!("[\n{}\n]", lines.join(",\n"))
    };
    let s = |v: Value| to_js_json_compact(&v);
    let mut fields = vec![format!("\"_source\": {}", s(Value::from(SOURCE)))];
    if let Some(b) = &doc.build {
        fields.push(format!("\"_build\": {}", s(b.clone())));
    }
    fields.push(format!("\"format\": {}", doc.format));
    fields.push(format!("\"columns\": {}", s(Value::from(COLUMNS.to_vec()))));
    fields.push(format!(
        "\"makers\": {}",
        s(Value::Array(doc.makers.clone()))
    ));
    fields.push(format!("\"types\": {}", s(Value::Array(doc.types.clone()))));
    fields.push(format!("\"items\": {}", rows(&doc.items)));
    fields.push(format!("\"ships\": {}", rows(&doc.ships)));
    format!("{{\n{}\n}}\n", fields.join(",\n"))
}

/// What [`write_search_index`] wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    pub file: PathBuf,
    pub entries: usize,
    pub bytes: usize,
}

fn read_json(file: &Path) -> Option<Value> {
    serde_json::from_slice(&std::fs::read(file).ok()?).ok()
}

/// `writeSearchIndex(dataDir)`: read `game-item-tags.json` and `game-ships.json` from
/// `dir`, write `game-search-index.json` beside them. When either input is missing or
/// unreadable, write nothing and remove an index already there (it would no longer restate
/// the files beside it).
pub fn write_search_index(dir: &Path) -> std::io::Result<Option<Written>> {
    let tags = read_json(&dir.join("game-item-tags.json"));
    let ships = read_json(&dir.join("game-ships.json"));
    let file = dir.join(FILE);
    let items = tags.as_ref().and_then(|t| t.get("items")?.as_array());
    let vehicles = ships.as_ref().and_then(|s| s.get("vehicles")?.as_array());
    let (Some(items), Some(vehicles)) = (items, vehicles) else {
        match std::fs::remove_file(&file) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
            _ => return Ok(None),
        }
    };
    let get_build = |v: &Option<Value>| v.as_ref().and_then(|v| v.get("_build")).cloned();
    let build = coalesce(get_build(&tags).as_ref(), get_build(&ships).as_ref());
    let doc = build_search_index(items, vehicles, build);
    let body = serialize(&doc);
    std::fs::write(&file, &body)?;
    Ok(Some(Written {
        file,
        entries: doc.items.len() + doc.ships.len(),
        bytes: body.len(),
    }))
}

/// The JSON value of a document, for tests and callers that want it parsed.
pub fn to_value(doc: &SearchIndexDoc) -> Value {
    serde_json::from_str(&serialize(doc)).unwrap_or(Value::Object(Map::new()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn item(class: &str, name: &str, tags: &[&str], extra: Value) -> Value {
        let mut v = json!({"className": class, "name": name, "id": class, "type": "WeaponPersonal", "tags": tags});
        for (k, x) in extra.as_object().unwrap() {
            if x.is_null() {
                v.as_object_mut().unwrap().remove(k);
            } else {
                v[k] = x.clone();
            }
        }
        v
    }

    fn items() -> Vec<Value> {
        vec![
            item(
                "ksar_lmg_energy_01",
                "ksar_lmg_energy_01",
                &["Weapon/FPS/Stocked/LMG/Verdict", "Manufacturer/KastakArms"],
                json!({"isTemplate": true}),
            ),
            item(
                "ksar_lmg_energy_01_store01",
                "Verdict \"Derivation\" LMG",
                &["Weapon/FPS/Stocked/LMG/Verdict", "Manufacturer/KastakArms"],
                json!({}),
            ),
            item("door_01", "Door", &[], json!({"type": "Door"})),
            // `type: undefined` in the TS fixture: the key is absent.
            item(
                "no_type_01",
                "no_type_01",
                &[],
                json!({"type": null, "manufacturer": "Acme"}),
            ),
            item(
                "outlaw_arms_01",
                "Lynx Arms Grey",
                &["Manufacturer/KastakArms"],
                json!({"type": "Char_Armor_Arms"}),
            ),
        ]
    }

    fn ships() -> Vec<Value> {
        vec![
            json!({"name": "Aegis Gladius", "recordName": "EntityClassDefinition.AEGS_Gladius", "manufacturer": "aegs", "role": "Light Fighter"}),
            json!({"name": "Gladius Template", "recordName": "EntityClassDefinition.AEGS_Gladius_Template", "isTemplate": true}),
            json!({"name": "Cutter", "recordName": "EntityClassDefinition.DRAK_Cutter", "manufacturer": "drak", "career": "Exploration"}),
        ]
    }

    fn doc() -> SearchIndexDoc {
        build_search_index(&items(), &ships(), Some(json!("4.10.0.1")))
    }

    #[test]
    fn loads_back_exactly_what_build_entries_gives_in_both_modes() {
        let v = to_value(&doc());
        for all in [false, true] {
            assert_eq!(
                load_search_index_entries(&v, all).unwrap(),
                build_entries(&items(), &ships(), all)
            );
        }
    }

    #[test]
    fn survives_a_round_trip_through_the_file_encoding() {
        let parsed: Value = serde_json::from_str(&serialize(&doc())).unwrap();
        assert_eq!(parsed["_build"], "4.10.0.1");
        assert_eq!(
            load_search_index_entries(&parsed, true).unwrap(),
            build_entries(&items(), &ships(), true)
        );
    }

    #[test]
    fn flags_furniture_and_templates_as_noise_and_trims_empty_columns() {
        let d = doc();
        let row = |rows: &[Vec<Value>], key: &str| rows.iter().find(|r| r[0] == key).cloned();
        assert_eq!(row(&d.items, "door_01").unwrap().last(), Some(&json!(1)));
        assert_eq!(
            row(&d.ships, "AEGS_Gladius_Template").unwrap().last(),
            Some(&json!(1))
        );
        let ix = |list: &[Value], v: &str| list.iter().position(|x| x == v).unwrap() + 1;
        // A named ship with maker and role, no family, not noise: four columns.
        assert_eq!(
            d.ships[0],
            vec![
                json!("AEGS_Gladius"),
                json!("Aegis Gladius"),
                json!(ix(&d.makers, "AEGS")),
                json!(ix(&d.types, "Light Fighter")),
            ]
        );
        // An unnamed template with a family is named after it.
        assert_eq!(
            d.items[0][1],
            json!("Verdict (unnamed: ksar_lmg_energy_01)")
        );
    }

    #[test]
    fn rejects_a_format_it_does_not_know() {
        let mut v = to_value(&doc());
        v["format"] = json!(2);
        let e = load_search_index_entries(&v, false).unwrap_err();
        assert!(e.to_string().contains("format 2"), "{e}");
    }

    #[test]
    fn serializes_one_row_per_line_and_omits_an_undefined_build() {
        let d = build_search_index(&[], &[], None);
        assert_eq!(
            serialize(&d),
            "{\n\"_source\": \"Derived from game-item-tags.json and game-ships.json (see src/search-index.ts)\",\n\"format\": 1,\n\"columns\": [\"key\",\"name\",\"maker\",\"type\",\"family\",\"noise\"],\n\"makers\": [],\n\"types\": [],\n\"items\": [],\n\"ships\": []\n}\n"
        );
        let text = serialize(&doc());
        assert!(
            text.contains("\"ships\": [\n[\"AEGS_Gladius\",\"Aegis Gladius\",3,4],\n"),
            "{text}"
        );
    }

    #[test]
    fn weapon_family_takes_the_deepest_tag() {
        assert_eq!(
            weapon_family(&["Weapon/FPS/Pistol/Hellion"]).as_deref(),
            Some("Hellion")
        );
        assert_eq!(weapon_family(&["Weapon/FPS/Stocked/LMG"]), None);
        assert_eq!(
            weapon_family(&["Weapon/FPS/Stocked/LMG", "Weapon/FPS/Stocked/LMG/Verdict"]).as_deref(),
            Some("Verdict")
        );
        assert_eq!(weapon_family(&["Weapon/FPS/Grenade/X"]), None);
    }

    #[test]
    fn null_role_falls_back_to_career_and_null_maker_is_interned() {
        let ships = [json!({"name": "S", "recordName": "S", "role": null, "career": "Combat"})];
        let items =
            [json!({"className": "a", "name": "A", "type": "T", "manufacturer": null, "tags": []})];
        let d = build_search_index(&items, &ships, None);
        assert_eq!(d.types, vec![json!("T"), json!("Combat")]);
        assert_eq!(d.makers, vec![Value::Null]);
        // Name equal to the key is stored as 0.
        assert_eq!(d.ships[0], vec![json!("S"), json!(0), json!(0), json!(2)]);
        assert_eq!(d.items[0], vec![json!("a"), json!("A"), json!(1), json!(1)]);
        // A null `_build` is a value, not undefined: it is written.
        assert!(
            serialize(&build_search_index(&[], &[], Some(Value::Null)))
                .contains("\"_build\": null")
        );
    }

    #[test]
    fn writes_beside_its_inputs_and_skips_quietly_when_one_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        assert_eq!(write_search_index(p).unwrap(), None);
        std::fs::write(
            p.join("game-item-tags.json"),
            json!({"_build": "4.10.0.1", "items": items()}).to_string(),
        )
        .unwrap();
        assert_eq!(write_search_index(p).unwrap(), None);
        assert!(!p.join(FILE).exists());
        std::fs::write(
            p.join("game-ships.json"),
            json!({"vehicles": ships()}).to_string(),
        )
        .unwrap();
        let w = write_search_index(p).unwrap().unwrap();
        assert_eq!(w.file, p.join(FILE));
        assert_eq!(w.entries, 8);
        assert_eq!(w.bytes as u64, std::fs::metadata(&w.file).unwrap().len());
    }

    #[test]
    fn removes_a_stale_index_when_an_input_goes_missing() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        std::fs::write(
            p.join("game-item-tags.json"),
            json!({"_build": "4.10.0.1", "items": items()}).to_string(),
        )
        .unwrap();
        std::fs::write(
            p.join("game-ships.json"),
            json!({"vehicles": ships()}).to_string(),
        )
        .unwrap();
        assert!(write_search_index(p).unwrap().is_some());
        std::fs::remove_file(p.join("game-ships.json")).unwrap();
        assert_eq!(write_search_index(p).unwrap(), None);
        assert!(!p.join(FILE).exists());
    }
}
