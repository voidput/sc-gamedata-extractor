//! Reverse links (spec `cross-dataset-derivations.md` D1): "where is this item used",
//! added to datasets that are already on disk.
//!
//! - `game-ship-components` items get `stockOn[]`: the `recordName`s of the vehicles in
//!   `game-ships` whose default `loadout` carries the item;
//! - `game-weapons` items (guns, missiles) get `mountedOn[]`, the same list for weapons;
//! - `game-ammo` entries get `firedBy[]`: the `recordName`s of the `game-weapons` weapons
//!   whose `ammo.recordName` is that ammo.
//!
//! Derived from published datasets, like [`crate::search_index`] and
//! [`crate::item_obtain`]: [`apply_reverse_links`] runs after the domains are written and
//! rewrites the three targets in place. A target is only linked against inputs of the same
//! `_build`, so it never restates another build. Lists are sorted and unique; an item with
//! no link gets no field (the field is optional, no `_schema` bump).
//!
//! The loadout walk ([`loadout_refs`]) is generic: every string anywhere in a vehicle's
//! `loadout`, at any key and depth, is a candidate item reference (display text keys
//! excepted), so loadout keys added later are picked up without code changes. A candidate
//! only becomes a link when it names an item of the target dataset. Loadouts name some
//! mounts and racks in lower case (`mount_gimbal_s3`), so classes match case-insensitively.

use crate::output::to_js_json;
use serde_json::{Map, Value};
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

pub const SHIPS_FILE: &str = "game-ships.json";
pub const COMPONENTS_FILE: &str = "game-ship-components.json";
pub const WEAPONS_FILE: &str = "game-weapons.json";
pub const AMMO_FILE: &str = "game-ammo.json";

/// Loadout keys holding display text, never an item reference.
const TEXT_KEYS: &[&str] = &[
    "name",
    "nameKey",
    "displayName",
    "description",
    "descriptionKey",
];

/// What [`apply_reverse_links`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Applied {
    /// Files rewritten (content changed).
    pub files: Vec<PathBuf>,
    /// Components with a `stockOn`.
    pub stock_on: usize,
    /// Weapons with a `mountedOn`.
    pub mounted_on: usize,
    /// Ammo with a `firedBy`.
    pub fired_by: usize,
    /// Ammo `recordName`s no weapon fires.
    pub unfired_ammo: Vec<String>,
}

/// Every candidate item reference in a vehicle's `loadout`: each string value at any key
/// and depth (arrays and objects alike) except [`TEXT_KEYS`].
pub fn loadout_refs(loadout: &Value) -> BTreeSet<String> {
    fn walk(v: &Value, key: Option<&str>, out: &mut BTreeSet<String>) {
        match v {
            Value::String(s) => {
                if !key.is_some_and(|k| TEXT_KEYS.contains(&k)) && !s.is_empty() {
                    out.insert(s.clone());
                }
            }
            Value::Array(a) => a.iter().for_each(|x| walk(x, key, out)),
            Value::Object(o) => o.iter().for_each(|(k, x)| walk(x, Some(k), out)),
            _ => {}
        }
    }
    let mut out = BTreeSet::new();
    walk(loadout, None, &mut out);
    out
}

/// Lower-cased loadout reference -> the `recordName`s of the vehicles carrying it.
pub fn stock_index(vehicles: &[Value]) -> HashMap<String, BTreeSet<String>> {
    let mut index: HashMap<String, BTreeSet<String>> = HashMap::new();
    for v in vehicles {
        let (Some(rec), Some(loadout)) = (
            v.get("recordName").and_then(Value::as_str),
            v.get("loadout"),
        ) else {
            continue;
        };
        for r in loadout_refs(loadout) {
            index
                .entry(r.to_lowercase())
                .or_default()
                .insert(rec.to_string());
        }
    }
    index
}

/// Ammo `recordName` -> the `recordName`s of the weapons (every array of `game-weapons`)
/// whose `ammo.recordName` is that ammo.
pub fn firing_index(weapons: &Value) -> HashMap<String, BTreeSet<String>> {
    let mut index: HashMap<String, BTreeSet<String>> = HashMap::new();
    for w in item_arrays(weapons).flatten() {
        let rec = w.get("recordName").and_then(Value::as_str);
        let ammo = w
            .get("ammo")
            .and_then(|a| a.get("recordName"))
            .and_then(Value::as_str);
        if let (Some(rec), Some(ammo)) = (rec, ammo) {
            index
                .entry(ammo.to_string())
                .or_default()
                .insert(rec.to_string());
        }
    }
    index
}

/// The item class of a `recordName` (`EntityClassDefinition.X` -> `X`).
fn class_of(record_name: &str) -> &str {
    record_name.split_once('.').map_or(record_name, |(_, c)| c)
}

/// The top-level arrays of a dataset (its item collections).
fn item_arrays(doc: &Value) -> impl Iterator<Item = &Vec<Value>> {
    doc.as_object()
        .into_iter()
        .flat_map(|o| o.values())
        .filter_map(Value::as_array)
}

/// Set (or clear) `key` on every object in the dataset's top-level arrays from `index`,
/// looked up by `id_of(item)`. Returns how many items got a list.
fn link(
    doc: &mut Value,
    key: &str,
    index: &HashMap<String, BTreeSet<String>>,
    id_of: impl Fn(&Map<String, Value>) -> Option<String>,
) -> usize {
    let mut n = 0;
    let Some(obj) = doc.as_object_mut() else {
        return 0;
    };
    for arr in obj.values_mut().filter_map(Value::as_array_mut) {
        for item in arr.iter_mut().filter_map(Value::as_object_mut) {
            let links = id_of(item).and_then(|id| index.get(&id));
            match links {
                Some(l) if !l.is_empty() => {
                    item.insert(
                        key.to_string(),
                        Value::Array(l.iter().cloned().map(Value::String).collect()),
                    );
                    n += 1;
                }
                _ => {
                    item.shift_remove(key);
                }
            }
        }
    }
    n
}

fn class_key(item: &Map<String, Value>) -> Option<String> {
    item.get("recordName")
        .and_then(Value::as_str)
        .map(|r| class_of(r).to_lowercase())
}

fn record_key(item: &Map<String, Value>) -> Option<String> {
    item.get("recordName")
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// `stockOn` on the components dataset; returns the item count linked.
pub fn link_components(components: &mut Value, vehicles: &[Value]) -> usize {
    link(components, "stockOn", &stock_index(vehicles), class_key)
}

/// `mountedOn` on the weapons dataset; returns the item count linked.
pub fn link_weapons(weapons: &mut Value, vehicles: &[Value]) -> usize {
    link(weapons, "mountedOn", &stock_index(vehicles), class_key)
}

/// `firedBy` on the ammo dataset; returns the linked count and the ammo no weapon fires.
pub fn link_ammo(ammo: &mut Value, weapons: &Value) -> (usize, Vec<String>) {
    let index = firing_index(weapons);
    let n = link(ammo, "firedBy", &index, record_key);
    let unfired = item_arrays(ammo)
        .flatten()
        .filter_map(|a| a.get("recordName").and_then(Value::as_str))
        .filter(|r| !index.contains_key(*r))
        .map(str::to_string)
        .collect();
    (n, unfired)
}

fn read_json(file: &Path) -> Option<Value> {
    serde_json::from_slice(&std::fs::read(file).ok()?).ok()
}

fn build_of(v: &Value) -> Option<&Value> {
    v.get("_build")
}

/// Write `doc` to `file` when its text changed; records the path in `files`.
fn write_if_changed(file: &Path, doc: &Value, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    let text = to_js_json(doc);
    if std::fs::read(file).ok().as_deref() != Some(text.as_bytes()) {
        std::fs::write(file, text)?;
        files.push(file.to_path_buf());
    }
    Ok(())
}

/// Add the reverse links to the targets in `dir` that have their inputs beside them with
/// the same `_build` (`game-ships.json` for components and weapons, `game-weapons.json`
/// for ammo). `None` when no target could be linked.
pub fn apply_reverse_links(dir: &Path) -> std::io::Result<Option<Applied>> {
    let ships = read_json(&dir.join(SHIPS_FILE));
    let vehicles = ships.as_ref().and_then(|s| s.get("vehicles")?.as_array());
    let mut applied = Applied::default();
    let mut any = false;

    let same_build = |a: &Value, b: Option<&Value>| b.is_some_and(|b| build_of(a) == build_of(b));

    let components_file = dir.join(COMPONENTS_FILE);
    if let (Some(mut doc), Some(vs)) = (read_json(&components_file), vehicles)
        && same_build(&doc, ships.as_ref())
    {
        applied.stock_on = link_components(&mut doc, vs);
        write_if_changed(&components_file, &doc, &mut applied.files)?;
        any = true;
    }

    let weapons_file = dir.join(WEAPONS_FILE);
    let mut weapons = read_json(&weapons_file);
    if let (Some(doc), Some(vs)) = (weapons.as_mut(), vehicles)
        && same_build(doc, ships.as_ref())
    {
        applied.mounted_on = link_weapons(doc, vs);
        write_if_changed(&weapons_file, doc, &mut applied.files)?;
        any = true;
    }

    let ammo_file = dir.join(AMMO_FILE);
    if let (Some(mut doc), Some(w)) = (read_json(&ammo_file), weapons.as_ref())
        && same_build(&doc, Some(w))
    {
        let (n, unfired) = link_ammo(&mut doc, w);
        applied.fired_by = n;
        applied.unfired_ammo = unfired;
        write_if_changed(&ammo_file, &doc, &mut applied.files)?;
        any = true;
    }
    Ok(any.then_some(applied))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn vehicles() -> Vec<Value> {
        vec![
            json!({"recordName": "EntityClassDefinition.SHIP_A", "loadout": {
                "powerPlants": [{"item": "POWR_A", "name": "Regulus", "size": 1}],
                "armor": {"item": "ARMR_A", "name": "Armor"},
                "weapons": [{"item": "GUN_A", "name": "GUN_B", "mount": "mount_gimbal_s3"}],
                "missiles": [{"item": "MISL_A", "rack": "mrck_a"}],
                "thrusters": {"main": 2}
            }}),
            json!({"recordName": "EntityClassDefinition.SHIP_B", "loadout": {
                "powerPlants": [{"item": "POWR_A"}],
                // A key no code knows about, nested two levels deep: still walked.
                "salvageHeads": [{"head": {"item": "SALV_X", "size": 1}}],
                "bombs": {"bays": [["BOMB_Y"]]}
            }}),
        ]
    }

    fn rec(class: &str) -> Value {
        json!({"recordName": format!("EntityClassDefinition.{class}"), "size": 1})
    }

    #[test]
    fn walk_picks_up_unknown_keys_and_skips_text() {
        let v = vehicles();
        let refs = loadout_refs(&v[1]["loadout"]);
        assert!(refs.contains("SALV_X"));
        assert!(refs.contains("BOMB_Y"));
        let refs = loadout_refs(&v[0]["loadout"]);
        assert!(!refs.contains("Regulus"));
        assert!(
            !refs.contains("GUN_B"),
            "display name is text, not a reference"
        );
        assert!(refs.contains("mount_gimbal_s3"));
    }

    #[test]
    fn stock_on_round_trips_and_matches_case_insensitively() {
        let vs = vehicles();
        let mut comps = json!({
            "_build": "1",
            "powerPlants": [rec("POWR_A"), rec("POWR_UNUSED")],
            "mounts": [rec("Mount_Gimbal_S3")],
            "missileRacks": [rec("MRCK_A")],
            "salvage": [rec("SALV_X")],
            "bombs": [rec("BOMB_Y")],
            "summary": {"total": 6}
        });
        assert_eq!(link_components(&mut comps, &vs), 5);
        assert_eq!(
            comps["powerPlants"][0]["stockOn"],
            json!([
                "EntityClassDefinition.SHIP_A",
                "EntityClassDefinition.SHIP_B"
            ])
        );
        assert!(comps["powerPlants"][1].get("stockOn").is_none());
        assert_eq!(
            comps["mounts"][0]["stockOn"],
            json!(["EntityClassDefinition.SHIP_A"])
        );
        assert_eq!(
            comps["missileRacks"][0]["stockOn"],
            json!(["EntityClassDefinition.SHIP_A"])
        );
        assert_eq!(
            comps["salvage"][0]["stockOn"],
            json!(["EntityClassDefinition.SHIP_B"])
        );
        assert_eq!(
            comps["bombs"][0]["stockOn"],
            json!(["EntityClassDefinition.SHIP_B"])
        );
        // Round trip: every listed ship carries the item.
        for arr in item_arrays(&comps) {
            for item in arr {
                let class = class_of(item["recordName"].as_str().unwrap()).to_lowercase();
                for ship in item
                    .get("stockOn")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    let v = vs.iter().find(|v| v["recordName"] == *ship).unwrap();
                    let refs: BTreeSet<String> = loadout_refs(&v["loadout"])
                        .iter()
                        .map(|r| r.to_lowercase())
                        .collect();
                    assert!(refs.contains(&class));
                }
            }
        }
        // Idempotent; a link that goes away is removed.
        let before = comps.clone();
        link_components(&mut comps, &vs);
        assert_eq!(comps, before);
        link_components(&mut comps, &[]);
        assert!(comps["powerPlants"][0].get("stockOn").is_none());
    }

    #[test]
    fn mounted_on_and_fired_by() {
        let vs = vehicles();
        let mut weapons = json!({
            "shipWeapons": [
                {"recordName": "EntityClassDefinition.GUN_A", "ammo": {"recordName": "AmmoParams.A"}},
                {"recordName": "EntityClassDefinition.GUN_B", "ammo": {"recordName": "AmmoParams.A"}}
            ],
            "fpsWeapons": [{"recordName": "EntityClassDefinition.RIFLE", "ammo": {"recordName": "AmmoParams.R"}}],
            "missiles": [rec("MISL_A")]
        });
        assert_eq!(link_weapons(&mut weapons, &vs), 2);
        assert_eq!(
            weapons["shipWeapons"][0]["mountedOn"],
            json!(["EntityClassDefinition.SHIP_A"])
        );
        assert!(weapons["shipWeapons"][1].get("mountedOn").is_none());
        assert_eq!(
            weapons["missiles"][0]["mountedOn"],
            json!(["EntityClassDefinition.SHIP_A"])
        );
        let mut ammo = json!({"ammo": [
            {"recordName": "AmmoParams.A"}, {"recordName": "AmmoParams.R"}, {"recordName": "AmmoParams.Z"}
        ]});
        let (n, unfired) = link_ammo(&mut ammo, &weapons);
        assert_eq!(n, 2);
        assert_eq!(
            ammo["ammo"][0]["firedBy"],
            json!(["EntityClassDefinition.GUN_A", "EntityClassDefinition.GUN_B"])
        );
        assert_eq!(unfired, vec!["AmmoParams.Z".to_string()]);
    }

    #[test]
    fn apply_skips_other_builds() {
        let dir = std::env::temp_dir().join(format!("sc-gamedata-revlinks-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let write = |f: &str, v: Value| std::fs::write(dir.join(f), to_js_json(&v)).unwrap();
        write(SHIPS_FILE, json!({"_build": "2", "vehicles": vehicles()}));
        write(
            COMPONENTS_FILE,
            json!({"_build": "1", "powerPlants": [rec("POWR_A")]}),
        );
        write(
            WEAPONS_FILE,
            json!({"_build": "2", "shipWeapons": [rec("GUN_A")]}),
        );
        let applied = apply_reverse_links(&dir).unwrap().unwrap();
        assert_eq!(applied.stock_on, 0);
        assert_eq!(applied.mounted_on, 1);
        assert_eq!(applied.files, vec![dir.join(WEAPONS_FILE)]);
        let comps = read_json(&dir.join(COMPONENTS_FILE)).unwrap();
        assert!(comps["powerPlants"][0].get("stockOn").is_none());
        // Second run: nothing changes on disk.
        assert!(apply_reverse_links(&dir).unwrap().unwrap().files.is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
