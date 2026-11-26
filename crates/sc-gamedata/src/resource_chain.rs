//! The resource -> ore -> refinery -> blueprint chain on `game-commodities.json` resources
//! (spec `cross-dataset-derivations.md` item D4).
//!
//! Derived from datasets, like [`crate::item_obtain`]: `game-commodities` cannot see the
//! records of the other domains, so after a run its file is joined with the
//! `game-mining-spawns`, `game-refining`, `game-blueprints` and `game-cargo` files beside it
//! and rewritten in place. A join runs only when its input carries the same `_build` as
//! commodities (one extract, one `_build`); otherwise its fields are left out. Rewriting
//! first drops every field this module adds, so a rerun never keeps a stale join.
//!
//! Per resource (each field absent when empty):
//!
//! ```text
//! minedAt[]: {via: "belt", oreKey, location, place, placeSource, system?, kind?, spawnShare?, rocks}
//!          | {via: "spot", oreKey, area, place?, placeSource?, slotShare?}
//!     game-mining-spawns ores[<oreKey>].belts / .spots for each minedAs oreKey; belts summed
//!     per (oreKey, location, kind): spawnShare is the sum over its rocks, rocks their count.
//! refinedBy[]: {raw, rawQuality?{tier, min, max, mean, stddev}, processesEstimate[]}
//!     on the refined resource of each game-refining refinables[] pair: the raw resource it
//!     comes from, that raw's quality curve (what the refinery receives) and the processes.
//! usedByBlueprints: {count, ids[]}
//!     game-blueprints whose recipe cost tree names the resource (choice options included),
//!     counted once per blueprint; ids are blueprint `id`s in game-blueprints order.
//! inManifests[]: game-cargo manifest names whose resources[] name the resource.
//! qualityOverrideBelts[]: {location, override}
//!     belts of minedAt in the star system a quality.locationOverrides[] entry names.
//! ```

use crate::collate::compare_names;
use crate::output::to_js_json;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

/// The file this module rewrites.
pub const FILE: &str = "game-commodities.json";
/// Inputs, in join order.
pub const SPAWNS: &str = "game-mining-spawns.json";
pub const REFINING: &str = "game-refining.json";
pub const BLUEPRINTS: &str = "game-blueprints.json";
pub const CARGO: &str = "game-cargo.json";

/// Every per-resource field this module adds (and removes before a rerun).
pub const FIELDS: &[&str] = &[
    "minedAt",
    "refinedBy",
    "usedByBlueprints",
    "inManifests",
    "qualityOverrideBelts",
];

/// `_assumptions` entry when any resource has `minedAt`.
pub const MINED_ASSUMPTION: &str = "minedAt[].spawnShare (via belt) sums game-mining-spawns ores[].belts[].spawnShare over the location's rocks that hold the ore (rocks = how many); it is that dataset's estimate (see its _assumptions) and compares only between rows of one kind. slotShare is copied as is.";
/// `_assumptions` entry when any resource has `refinedBy`.
pub const PROCESSES_ASSUMPTION: &str = "refinedBy[].processesEstimate lists every game-refining process for every refinable pair: no record ties a refining method to a resource.";
/// `_assumptions` entry when any resource has `qualityOverrideBelts`.
pub const OVERRIDE_ASSUMPTION: &str = "qualityOverrideBelts: a quality location override applies to the belts of the star system it names (location \"<system> System\" or \"<system>\" against game-mining-spawns system); overrides naming a station match no belt, and belts without a system (Lagrange points, asteroid clusters) match no override.";
const ASSUMPTIONS: &[&str] = &[MINED_ASSUMPTION, PROCESSES_ASSUMPTION, OVERRIDE_ASSUMPTION];

/// The joined datasets (whole files); `None` skips that join.
#[derive(Default, Clone, Copy)]
pub struct Inputs<'a> {
    pub spawns: Option<&'a Value>,
    pub refining: Option<&'a Value>,
    pub blueprints: Option<&'a Value>,
    pub cargo: Option<&'a Value>,
}

/// Resources given each field.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Counts {
    pub mined_at: usize,
    pub refined_by: usize,
    pub used_by_blueprints: usize,
    pub in_manifests: usize,
    pub quality_override_belts: usize,
}

/// What [`write_resource_chain`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    pub file: PathBuf,
    pub counts: Counts,
    pub bytes: usize,
    /// Joins left out, and why.
    pub skipped: Vec<String>,
}

fn s<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

fn arr<'a>(v: Option<&'a Value>, key: &str) -> &'a [Value] {
    v.and_then(|v| v.get(key))
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

fn put(row: &mut Map<String, Value>, key: &str, value: Option<&Value>) {
    if let Some(v) = value.filter(|v| !v.is_null()) {
        row.insert(key.into(), v.clone());
    }
}

/// `minedAt` of one resource: belts (summed per oreKey, location and kind), then spots.
fn mined_at(resource: &Value, spawns: &Value) -> Vec<Value> {
    let ores = spawns.get("ores");
    let mut belts: Vec<(f64, Map<String, Value>)> = Vec::new();
    let mut spots: Vec<(f64, Map<String, Value>)> = Vec::new();
    let mut seen_keys = HashSet::new();
    for m in arr(Some(resource), "minedAs") {
        let Some(key) = s(m, "oreKey") else { continue };
        if !seen_keys.insert(key.to_string()) {
            continue;
        }
        let ore = ores.and_then(|o| o.get(key));
        // (location, kind) -> index into `belts`, in first-seen order.
        let mut at: BTreeMap<(String, String), usize> = BTreeMap::new();
        for b in arr(ore, "belts") {
            let Some(location) = s(b, "location") else {
                continue;
            };
            let kind = s(b, "kind").unwrap_or("");
            let share = b.get("spawnShare").and_then(Value::as_f64);
            match at.get(&(location.to_string(), kind.to_string())) {
                Some(&i) => {
                    let row = &mut belts[i];
                    row.0 += share.unwrap_or(0.0);
                    let rocks = row.1["rocks"].as_u64().unwrap_or(0) + 1;
                    row.1.insert("rocks".into(), json!(rocks));
                    if share.is_some() {
                        row.1.insert("spawnShare".into(), json!(row.0));
                    }
                }
                None => {
                    let mut row = Map::new();
                    row.insert("via".into(), json!("belt"));
                    row.insert("oreKey".into(), json!(key));
                    row.insert("location".into(), json!(location));
                    put(&mut row, "place", b.get("place"));
                    put(&mut row, "placeSource", b.get("placeSource"));
                    put(&mut row, "system", b.get("system"));
                    put(&mut row, "kind", b.get("kind"));
                    if let Some(x) = share {
                        row.insert("spawnShare".into(), json!(x));
                    }
                    row.insert("rocks".into(), json!(1));
                    at.insert((location.to_string(), kind.to_string()), belts.len());
                    belts.push((share.unwrap_or(0.0), row));
                }
            }
        }
        for p in arr(ore, "spots") {
            let Some(area) = s(p, "area") else { continue };
            let mut row = Map::new();
            row.insert("via".into(), json!("spot"));
            row.insert("oreKey".into(), json!(key));
            row.insert("area".into(), json!(area));
            put(&mut row, "place", p.get("place"));
            put(&mut row, "placeSource", p.get("placeSource"));
            put(&mut row, "slotShare", p.get("slotShare"));
            spots.push((
                p.get("slotShare").and_then(Value::as_f64).unwrap_or(0.0),
                row,
            ));
        }
    }
    for (sum, row) in &mut belts {
        if row.contains_key("spawnShare") {
            *sum = crate::js::round_to(*sum, 1e5);
            row.insert("spawnShare".into(), json!(*sum));
        }
    }
    let by = |key: &'static str| {
        move |a: &(f64, Map<String, Value>), b: &(f64, Map<String, Value>)| {
            b.0.total_cmp(&a.0).then_with(|| {
                compare_names(
                    a.1[key].as_str().unwrap_or(""),
                    b.1[key].as_str().unwrap_or(""),
                )
            })
        }
    };
    belts.sort_by(by("location"));
    spots.sort_by(by("area"));
    belts
        .into_iter()
        .chain(spots)
        .map(|(_, r)| Value::Object(r))
        .collect()
}

/// `rawQuality` of a raw resource: its tier and base curve.
fn raw_quality(raw: &Value) -> Option<Value> {
    let q = raw.get("quality")?;
    let mut out = Map::new();
    put(&mut out, "tier", q.get("tier"));
    if let Some(Value::Object(d)) = q.get("distribution") {
        for k in ["min", "max", "mean", "stddev"] {
            put(&mut out, k, d.get(k));
        }
    }
    (!out.is_empty()).then_some(Value::Object(out))
}

/// The star systems a location override name points at.
fn override_systems<'a>(location: &str, systems: &'a [String]) -> Vec<&'a str> {
    systems
        .iter()
        .filter(|sys| {
            location.eq_ignore_ascii_case(sys)
                || location.eq_ignore_ascii_case(&format!("{sys} System"))
        })
        .map(String::as_str)
        .collect()
}

/// Join the inputs onto `commodities` (a whole `game-commodities` document), replacing any
/// fields an earlier join added.
pub fn enrich(commodities: &mut Value, inputs: Inputs<'_>) -> Counts {
    let mut counts = Counts::default();
    let Some(resources) = commodities
        .get_mut("resources")
        .and_then(Value::as_array_mut)
    else {
        return counts;
    };
    for r in resources.iter_mut() {
        if let Value::Object(m) = r {
            for f in FIELDS {
                m.remove(*f);
            }
        }
    }
    let lower = |v: &Value, k: &str| s(v, k).map(str::to_lowercase);

    // Reverse indices keyed by lower-cased resource name (names join case-insensitively, as
    // `rawSources` and `cargo` do).
    let mut blueprints: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for bp in arr(inputs.blueprints, "blueprints") {
        let Some(id) = bp.get("id").filter(|v| v.is_string()) else {
            continue;
        };
        let mut names = HashSet::new();
        collect_resources(bp.get("tiers"), &mut names);
        let mut names: Vec<String> = names.into_iter().collect();
        names.sort();
        for n in names {
            blueprints.entry(n).or_default().push(id.clone());
        }
    }
    let mut manifests: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for m in arr(inputs.cargo, "manifests") {
        let Some(name) = s(m, "name") else { continue };
        for row in arr(Some(m), "resources") {
            if let Some(res) = lower(row, "resource") {
                let list = manifests.entry(res).or_default();
                if !list.iter().any(|x| x == name) {
                    list.push(name.to_string());
                }
            }
        }
    }
    for list in manifests.values_mut() {
        list.sort_by(|a, b| compare_names(a, b));
    }
    let processes: Vec<Value> = arr(inputs.refining, "processes")
        .iter()
        .filter_map(|p| p.get("name").filter(|v| v.is_string()).cloned())
        .collect();
    // refined (lower) -> raw names, in game-refining order.
    let mut refined_from: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for pair in arr(inputs.refining, "refinables") {
        if let (Some(raw), Some(refined)) = (s(pair, "raw"), lower(pair, "refined")) {
            refined_from
                .entry(refined)
                .or_default()
                .push(raw.to_string());
        }
    }
    let mut systems: Vec<String> = arr(inputs.spawns, "locations")
        .iter()
        .filter_map(|l| s(l, "system").map(str::to_string))
        .collect();
    systems.sort();
    systems.dedup();

    let snapshot: Vec<Value> = resources.clone();
    let by_name = |name: &str| {
        snapshot
            .iter()
            .find(|x| s(x, "name").is_some_and(|n| n.eq_ignore_ascii_case(name)))
    };
    let mut any_mined = false;
    let mut any_refined = false;
    let mut any_override = false;
    for r in resources.iter_mut() {
        let Some(name) = lower(r, "name") else {
            continue;
        };
        let mined = inputs.spawns.map(|sp| mined_at(r, sp)).unwrap_or_default();
        let mut belts_hit: Vec<Value> = Vec::new();
        if inputs.spawns.is_some() {
            let mut seen = HashSet::new();
            let overrides = r
                .get("quality")
                .and_then(|q| q.get("locationOverrides"))
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            for o in &overrides {
                let Some(loc) = s(o, "location") else {
                    continue;
                };
                for sys in override_systems(loc, &systems) {
                    for b in mined.iter().filter(|b| s(b, "via") == Some("belt")) {
                        let Some(location) = s(b, "location") else {
                            continue;
                        };
                        if s(b, "system") == Some(sys)
                            && seen.insert((location.to_string(), loc.to_string()))
                        {
                            belts_hit.push(json!({"location": location, "override": loc}));
                        }
                    }
                }
            }
            belts_hit.sort_by(|a, b| {
                compare_names(
                    s(a, "location").unwrap_or(""),
                    s(b, "location").unwrap_or(""),
                )
            });
        }
        let refined_by: Vec<Value> = if inputs.refining.is_some() {
            refined_from
                .get(&name)
                .into_iter()
                .flatten()
                .map(|raw| {
                    let mut row = Map::new();
                    row.insert("raw".into(), json!(raw));
                    if let Some(q) = by_name(raw).and_then(raw_quality) {
                        row.insert("rawQuality".into(), q);
                    }
                    row.insert("processesEstimate".into(), Value::Array(processes.clone()));
                    Value::Object(row)
                })
                .collect()
        } else {
            Vec::new()
        };
        let Value::Object(m) = r else { continue };
        if !mined.is_empty() {
            counts.mined_at += 1;
            any_mined = true;
            m.insert("minedAt".into(), Value::Array(mined));
        }
        if !refined_by.is_empty() {
            counts.refined_by += 1;
            any_refined = true;
            m.insert("refinedBy".into(), Value::Array(refined_by));
        }
        if let Some(ids) = blueprints.get(&name) {
            counts.used_by_blueprints += 1;
            m.insert(
                "usedByBlueprints".into(),
                json!({"count": ids.len(), "ids": ids}),
            );
        }
        if let Some(list) = manifests.get(&name) {
            counts.in_manifests += 1;
            m.insert("inManifests".into(), json!(list));
        }
        if !belts_hit.is_empty() {
            counts.quality_override_belts += 1;
            any_override = true;
            m.insert("qualityOverrideBelts".into(), Value::Array(belts_hit));
        }
    }
    set_assumptions(commodities, any_mined, any_refined, any_override);
    counts
}

/// Resource names (lower-cased) in a blueprint's cost tree.
fn collect_resources(v: Option<&Value>, out: &mut HashSet<String>) {
    match v {
        Some(Value::Array(a)) => a.iter().for_each(|x| collect_resources(Some(x), out)),
        Some(Value::Object(o)) => {
            if o.get("kind").and_then(Value::as_str) == Some("resource")
                && let Some(r) = o.get("resource").and_then(Value::as_str)
            {
                out.insert(r.to_lowercase());
            }
            o.values().for_each(|x| collect_resources(Some(x), out));
        }
        _ => {}
    }
}

/// Keep this module's `_assumptions` entries in step with the fields written; a new
/// `_assumptions` goes before `resources`.
fn set_assumptions(doc: &mut Value, mined: bool, refined: bool, overrides: bool) {
    let Value::Object(m) = doc else { return };
    let mut list: Vec<Value> = m
        .get("_assumptions")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    list.retain(|v| !v.as_str().is_some_and(|t| ASSUMPTIONS.contains(&t)));
    if mined {
        list.push(json!(MINED_ASSUMPTION));
    }
    if refined {
        list.push(json!(PROCESSES_ASSUMPTION));
    }
    if overrides {
        list.push(json!(OVERRIDE_ASSUMPTION));
    }
    if m.contains_key("_assumptions") {
        if list.is_empty() {
            m.shift_remove("_assumptions");
        } else {
            m.insert("_assumptions".into(), Value::Array(list));
        }
        return;
    }
    if list.is_empty() {
        return;
    }
    let old = std::mem::take(m);
    for (k, v) in old {
        if k == "resources" {
            m.insert("_assumptions".into(), Value::Array(list.clone()));
        }
        m.insert(k, v);
    }
    if !m.contains_key("_assumptions") {
        m.insert("_assumptions".into(), Value::Array(list));
    }
}

fn read_json(file: &Path) -> Option<Value> {
    serde_json::from_slice(&std::fs::read(file).ok()?).ok()
}

/// Rewrite `game-commodities.json` in `dir` with the chain joined from the files beside it.
/// `None` when there is no commodities file. An input that is missing, unreadable or of
/// another `_build` is skipped (listed in [`Written::skipped`]).
pub fn write_resource_chain(dir: &Path) -> std::io::Result<Option<Written>> {
    let file = dir.join(FILE);
    let Some(mut doc) = read_json(&file) else {
        return Ok(None);
    };
    let build = doc.get("_build").cloned();
    let mut skipped = Vec::new();
    let mut load = |name: &str| -> Option<Value> {
        let Some(v) = read_json(&dir.join(name)) else {
            skipped.push(format!("{name}: missing or unreadable"));
            return None;
        };
        if v.get("_build") != build.as_ref() {
            skipped.push(format!("{name}: _build differs from {FILE}"));
            return None;
        }
        Some(v)
    };
    let spawns = load(SPAWNS);
    let refining = load(REFINING);
    let blueprints = load(BLUEPRINTS);
    let cargo = load(CARGO);
    let counts = enrich(
        &mut doc,
        Inputs {
            spawns: spawns.as_ref(),
            refining: refining.as_ref(),
            blueprints: blueprints.as_ref(),
            cargo: cargo.as_ref(),
        },
    );
    let text = to_js_json(&doc);
    std::fs::write(&file, &text)?;
    Ok(Some(Written {
        file,
        counts,
        bytes: text.len(),
        skipped,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commodities() -> Value {
        json!({
            "_source": "x", "_build": "1.0.0", "_schema": "game-commodities@1",
            "resources": [
                {"name": "Gold", "rawSources": ["Ore_Gold"]},
                {"name": "Ore_Gold", "refinesTo": "Gold",
                 "quality": {"tier": "Rare", "distribution": {"min": 501, "max": 1000, "mean": 500, "stddev": 163},
                             "locationOverrides": [{"location": "Pyro System", "min": 501},
                                                   {"location": "QV Breaker Station", "min": 651}]},
                 "minedAs": [{"element": "gold_ore", "oreKey": "gold"}]},
                {"name": "Iron"}
            ],
            "summary": {"resources": 3}
        })
    }

    fn inputs() -> (Value, Value, Value, Value) {
        let spawns = json!({
            "locations": [{"key": "Pyro4", "system": "Pyro"}, {"key": "AaronHalo", "system": "Stanton"}],
            "ores": {"gold": {
                "belts": [
                    {"location": "AaronHalo", "place": "Aaron Halo", "placeSource": "data", "system": "Stanton",
                     "rock": "a", "kind": "ship", "spawnShare": 0.1},
                    {"location": "Pyro4", "place": "Pyro IV", "placeSource": "data", "system": "Pyro",
                     "rock": "a", "kind": "ship", "spawnShare": 0.2},
                    {"location": "Pyro4", "place": "Pyro IV", "placeSource": "data", "system": "Pyro",
                     "rock": "b", "kind": "ship", "spawnShare": 0.1}
                ],
                "spots": [{"area": "cave", "rock": "c", "slotShare": 0.5, "configs": ["x"]}]
            }}
        });
        let refining = json!({
            "processes": [{"name": "P-Slow"}, {"name": "P-Fast"}],
            "refinables": [{"raw": "Ore_Gold", "refined": "Gold"}]
        });
        let blueprints = json!({"blueprints": [
            {"id": "b1", "tiers": [{"costs": [{"kind": "resource", "resource": "Gold"},
                                              {"kind": "choice", "options": [{"kind": "resource", "resource": "gold"}]}]}]},
            {"id": "b2", "tiers": [{"costs": [{"kind": "choice", "options": [{"kind": "resource", "resource": "Iron"},
                                                                            {"kind": "item", "item": "Gold"}]}]}]}
        ]});
        let cargo = json!({"manifests": [
            {"name": "M_b", "resources": [{"resource": "Gold"}, {"resource": "Gold"}]},
            {"name": "M_a", "resources": [{"resource": "gold"}, {"resource": "Iron"}]}
        ]});
        (spawns, refining, blueprints, cargo)
    }

    #[test]
    fn joins_the_chain_and_is_idempotent() {
        let (spawns, refining, blueprints, cargo) = inputs();
        let all = Inputs {
            spawns: Some(&spawns),
            refining: Some(&refining),
            blueprints: Some(&blueprints),
            cargo: Some(&cargo),
        };
        let mut doc = commodities();
        let counts = enrich(&mut doc, all);
        assert_eq!(
            counts,
            Counts {
                mined_at: 1,
                refined_by: 1,
                used_by_blueprints: 2,
                in_manifests: 2,
                quality_override_belts: 1
            }
        );
        let gold = &doc["resources"][0];
        assert_eq!(
            gold["refinedBy"],
            json!([{"raw": "Ore_Gold",
                    "rawQuality": {"tier": "Rare", "min": 501, "max": 1000, "mean": 500, "stddev": 163},
                    "processesEstimate": ["P-Slow", "P-Fast"]}])
        );
        // Counted once per blueprint, case-insensitively; an item cost is not a resource.
        assert_eq!(gold["usedByBlueprints"], json!({"count": 1, "ids": ["b1"]}));
        assert_eq!(gold["inManifests"], json!(["M_a", "M_b"]));
        let ore = &doc["resources"][1];
        assert_eq!(
            ore["minedAt"],
            json!([
                {"via": "belt", "oreKey": "gold", "location": "Pyro4", "place": "Pyro IV",
                 "placeSource": "data", "system": "Pyro", "kind": "ship", "spawnShare": 0.3, "rocks": 2},
                {"via": "belt", "oreKey": "gold", "location": "AaronHalo", "place": "Aaron Halo",
                 "placeSource": "data", "system": "Stanton", "kind": "ship", "spawnShare": 0.1, "rocks": 1},
                {"via": "spot", "oreKey": "gold", "area": "cave", "slotShare": 0.5}
            ])
        );
        assert_eq!(
            ore["qualityOverrideBelts"],
            json!([{"location": "Pyro4", "override": "Pyro System"}])
        );
        assert!(ore.get("refinedBy").is_none());
        assert_eq!(
            doc["resources"][2]["usedByBlueprints"],
            json!({"count": 1, "ids": ["b2"]})
        );
        // `_assumptions` is created before `resources`.
        let keys: Vec<&String> = doc.as_object().unwrap().keys().collect();
        assert_eq!(keys[3], "_assumptions");
        assert_eq!(
            doc["_assumptions"],
            json!([MINED_ASSUMPTION, PROCESSES_ASSUMPTION, OVERRIDE_ASSUMPTION])
        );

        let once = doc.clone();
        enrich(&mut doc, all);
        assert_eq!(doc, once);

        // Without inputs every added field and assumption goes again.
        enrich(&mut doc, Inputs::default());
        assert_eq!(doc, commodities());
    }

    #[test]
    fn keeps_existing_assumptions() {
        let (_, refining, _, _) = inputs();
        let mut doc = commodities();
        doc.as_object_mut()
            .unwrap()
            .shift_insert(3, "_assumptions".into(), json!(["quality"]));
        enrich(
            &mut doc,
            Inputs {
                refining: Some(&refining),
                ..Inputs::default()
            },
        );
        assert_eq!(
            doc["_assumptions"],
            json!(["quality", PROCESSES_ASSUMPTION])
        );
        enrich(&mut doc, Inputs::default());
        assert_eq!(doc["_assumptions"], json!(["quality"]));
    }

    #[test]
    fn writes_in_place_and_skips_other_builds() {
        let dir = std::env::temp_dir().join(format!("sc-chain-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (spawns, refining, mut blueprints, cargo) = inputs();
        blueprints["_build"] = json!("0.9.0");
        let with_build = |mut v: Value| {
            v["_build"] = json!("1.0.0");
            v
        };
        std::fs::write(dir.join(FILE), to_js_json(&commodities())).unwrap();
        std::fs::write(dir.join(SPAWNS), to_js_json(&with_build(spawns))).unwrap();
        std::fs::write(dir.join(REFINING), to_js_json(&with_build(refining))).unwrap();
        std::fs::write(dir.join(BLUEPRINTS), to_js_json(&blueprints)).unwrap();
        std::fs::write(dir.join(CARGO), to_js_json(&with_build(cargo))).unwrap();
        let w = write_resource_chain(&dir).unwrap().unwrap();
        assert_eq!(w.counts.used_by_blueprints, 0);
        assert_eq!(w.counts.mined_at, 1);
        assert_eq!(
            w.skipped,
            vec![format!("{BLUEPRINTS}: _build differs from {FILE}")]
        );
        let doc: Value = serde_json::from_slice(&std::fs::read(dir.join(FILE)).unwrap()).unwrap();
        assert_eq!(doc["resources"][0]["inManifests"], json!(["M_a", "M_b"]));
        std::fs::remove_dir_all(&dir).ok();
    }
}
