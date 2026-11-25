//! Port of `src/changes.ts`: patch changes as chart-ready rows, one per item per stat
//! that moved, with buff/nerf direction from which side of the stat is better.
//!
//! Items are matched by record name, so a rename is a change carrying both names.
//! [`changes`] reads two builds; [`changes_json`], [`render_changes`] and
//! [`export_changes`] are the CLI's `--json`, text and `exports/` output.

use crate::diff::{
    Body, BuildSource, OrderedMap, Result, dataset_file, js_strict_eq, js_string, load_snapshot,
};
use crate::js::round_to;
use crate::output::to_js_json;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Buff,
    Nerf,
    Changed,
}

impl Direction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Buff => "buff",
            Self::Nerf => "nerf",
            Self::Changed => "changed",
        }
    }
}

/// One tracked stat: where it lives on the item and which way is better.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatSpec {
    pub stat: &'static str,
    pub path: &'static str,
    /// `Some(true)` when a bigger number helps the item's owner; `None` when neither is.
    pub higher_is_better: Option<bool>,
}

/// One chart category: a dataset collection and its stats.
#[derive(Debug, Clone)]
pub struct CategorySpec {
    pub category: &'static str,
    pub dataset: &'static str,
    pub collection: &'static str,
    pub stats: Vec<StatSpec>,
    /// Skip items this returns false for (templates, AI hulls).
    pub include: Option<fn(&Body) -> bool>,
}

const fn up(stat: &'static str, path: &'static str) -> StatSpec {
    StatSpec {
        stat,
        path,
        higher_is_better: Some(true),
    }
}
const fn down(stat: &'static str, path: &'static str) -> StatSpec {
    StatSpec {
        stat,
        path,
        higher_is_better: Some(false),
    }
}
const fn neutral(stat: &'static str, path: &'static str) -> StatSpec {
    StatSpec {
        stat,
        path,
        higher_is_better: None,
    }
}

/// `notTemplate`.
pub fn not_template(item: &Body) -> bool {
    item.get("isTemplate") != Some(&Value::Bool(true))
}

/// `PLAYER_HULL` (case-insensitive): AI, NPC, event and test hulls.
fn npc_hull(record: &str) -> bool {
    const ANY: &[&str] = &[
        "_ai_",
        "_pu_",
        "_ea_",
        "template",
        "_pir",
        "_crim",
        "unmanned",
        "_test",
        "_s42",
        "_tutorial",
        "_npc",
        "_derelict",
        "_wreck",
        "_boarded",
        "_drone",
        "_hijacked",
        "_qig",
        "_fw_",
        "_mission",
        "_dummy",
        "_prop",
        "_showdown",
        "_cinematic",
        "_nointerior",
        "_display",
        "_temp",
        "_indestructible",
        "_fleetweek",
        "_drug",
        "_piano",
        "_swarm",
        "_advocacy",
    ];
    let l = record.to_ascii_lowercase();
    let digit_after = |needle: &str| {
        l.match_indices(needle).any(|(i, _)| {
            l.as_bytes()
                .get(i + needle.len())
                .is_some_and(u8::is_ascii_digit)
        })
    };
    ANY.iter().any(|w| l.contains(w))
        || l.ends_with("_ai")
        || l.ends_with("_plat")
        || digit_after("_tier_")
        || digit_after("_fw")
}

/// `playerShip`.
pub fn player_ship(item: &Body) -> bool {
    let record = item
        .get("recordName")
        .filter(|v| !v.is_null())
        .map_or_else(String::new, |v| js_string(Some(v)));
    not_template(item) && !npc_hull(&record)
}

const WEAPON_STATS: &[StatSpec] = &[
    up("dps", "dps"),
    up("sustainedDps", "sustainedDps"),
    up("alpha", "alpha"),
    up("fireRate", "fireRate"),
    up("range", "range"),
    up("speed", "speed"),
    down("spread", "spread"),
    up("penetration", "penetration"),
    up("ammoCapacity", "ammoCapacity"),
    up("burstDamage", "burst.damage"),
    up("burstSeconds", "burst.seconds"),
    up("capacitorRegen", "capacitor.regenPerSecond"),
    down("capacitorRegenDelay", "capacitor.regenCooldown"),
    down("power", "power"),
    down("em", "em"),
    down("ir", "ir"),
    up("health", "health"),
    down("mass", "mass"),
    up("damagePhysical", "ammo.damage.physical"),
    up("damageEnergy", "ammo.damage.energy"),
    up("damageDistortion", "ammo.damage.distortion"),
];

const COMPONENT_COMMON: &[StatSpec] = &[
    down("power", "power"),
    down("em", "em"),
    down("ir", "ir"),
    up("health", "health"),
    down("mass", "mass"),
    up("distortionMax", "distortionMax"),
    down("distortionRecover", "distortion.secondsToRecover"),
    down("selfRepairSeconds", "durability.selfRepair.seconds"),
];

fn component(collection: &'static str, stats: &[StatSpec]) -> CategorySpec {
    CategorySpec {
        category: collection,
        dataset: "ship-components",
        collection,
        stats: stats.iter().chain(COMPONENT_COMMON).copied().collect(),
        include: Some(not_template),
    }
}

/// `CATEGORIES`: every tracked category, in report order.
pub static CATEGORIES: LazyLock<Vec<CategorySpec>> = LazyLock::new(|| {
    let weapons = |category: &'static str| CategorySpec {
        category,
        dataset: "weapons",
        collection: category,
        stats: WEAPON_STATS.to_vec(),
        include: Some(not_template),
    };
    vec![
        weapons("shipWeapons"),
        weapons("fpsWeapons"),
        CategorySpec {
            category: "missiles",
            dataset: "weapons",
            collection: "missiles",
            stats: vec![
                up("damage", "damageTotal"),
                down("lockTime", "lockTime"),
                up("lockRangeMax", "lockRangeMax"),
                up("lockingAngle", "lockingAngle"),
                up("speed", "speed"),
                up("maxLifetime", "maxLifetime"),
                up("explosionRadius", "explosionRadius"),
            ],
            include: Some(not_template),
        },
        CategorySpec {
            category: "ships",
            dataset: "ships",
            collection: "vehicles",
            stats: vec![
                up("scmSpeed", "flight.scmSpeed"),
                up("maxSpeed", "flight.maxSpeed"),
                up("boostForward", "flight.boostSpeedForward"),
                up("pitch", "flight.pitch"),
                up("yaw", "flight.yaw"),
                up("roll", "flight.roll"),
                up("shieldHealth", "totals.shieldHealth"),
                up("shieldRegen", "totals.shieldRegen"),
                up("armorHealth", "totals.armorHealth"),
                up("deflectPhysical", "armor.deflection.physical"),
                up("deflectEnergy", "armor.deflection.energy"),
                down("armorMultPhysical", "armor.damageMultiplier.physical"),
                down("armorMultEnergy", "armor.damageMultiplier.energy"),
                up("weaponDps", "totals.weaponDps"),
                up("missiles", "totals.missiles"),
                up("missileDamage", "totals.missileDamage"),
                up("powerOutput", "totals.powerOutput"),
                up("weaponPips", "powerPools.WeaponGun"),
                up("fuel", "totals.fuel"),
                up("quantumFuel", "totals.quantumFuel"),
                up("cargoScu", "totals.cargoScu"),
                up("thrustMain", "totals.thrustMain"),
                up("thrustManeuver", "totals.thrustManeuver"),
                neutral("crew", "crewSize"),
            ],
            include: Some(player_ship),
        },
        component(
            "shields",
            &[
                up("shieldHealth", "stats.MaxShieldHealth"),
                up("shieldRegen", "stats.MaxShieldRegen"),
                down("downedRegenDelay", "stats.DownedRegenDelay"),
                down("damagedRegenDelay", "stats.DamagedRegenDelay"),
                up("absorbPhysicalMax", "detail.absorption.physical.max"),
                up("resistPhysicalMax", "detail.resistance.physical.max"),
                up("resistEnergyMax", "detail.resistance.energy.max"),
            ],
        ),
        component("powerPlants", &[up("powerOutput", "stats.powerOutput")]),
        component("coolers", &[up("coolantOutput", "stats.coolantOutput")]),
        component(
            "quantumDrives",
            &[
                up("driveSpeed", "stats.driveSpeed"),
                down("spoolUpTime", "stats.spoolUpTime"),
                down("cooldownTime", "stats.cooldownTime"),
                down("fuelRequirement", "stats.quantumFuelRequirement"),
                up("stageOneAccel", "stats.stageOneAccelRate"),
            ],
        ),
        component("radars", &[down("pingCooldown", "stats.cooldownTime")]),
        component(
            "armor",
            &[
                up("deflectPhysical", "detail.deflection.physical"),
                up("deflectEnergy", "detail.deflection.energy"),
                down("multPhysical", "detail.damageMultiplier.physical"),
                down("multEnergy", "detail.damageMultiplier.energy"),
                down("signalEm", "stats.signalElectromagnetic"),
                down("signalIr", "stats.signalInfrared"),
            ],
        ),
        component(
            "thrusters",
            &[
                up("thrust", "stats.thrustCapacity"),
                down("fuelBurn", "stats.fuelBurnRatePer10KNewton"),
            ],
        ),
    ]
});

/// One stat of one item that moved.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatChange {
    pub category: String,
    pub record: String,
    pub name: String,
    /// Present when the item was renamed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<f64>,
    pub stat: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delta: Option<f64>,
    /// Percent of the old value; `None` when the old value was 0 or absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub percent: Option<f64>,
    pub direction: Direction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ItemStatus {
    Added,
    Removed,
    Changed,
}

impl ItemStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Added => "added",
            Self::Removed => "removed",
            Self::Changed => "changed",
        }
    }
}

/// One item that was added, removed, or changed in a tracked stat or its name.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemChange {
    pub category: String,
    pub record: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<f64>,
    pub change: ItemStatus,
    pub stats_changed: usize,
    pub buffs: usize,
    pub nerfs: usize,
}

/// `PatchChanges`; serialises in the TypeScript's key order (`from`/`to` are assigned
/// after the arrays).
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct PatchChanges {
    pub items: Vec<ItemChange>,
    pub stats: Vec<StatChange>,
    /// Datasets one build lacked, so their categories were skipped.
    pub skipped: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
}

/// `read`: a finite number at a dotted path.
fn read(item: &Body, dotted: &str) -> Option<f64> {
    let mut parts = dotted.split('.');
    let mut current = item.get(parts.next()?)?;
    for part in parts {
        current = match current {
            Value::Object(o) => o.get(part)?,
            Value::Array(a) => a.get(part.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    current.as_f64().filter(|x| x.is_finite())
}

fn round3(x: f64) -> f64 {
    round_to(x, 1000.0)
}

/// `directionOf`.
pub fn direction_of(spec: &StatSpec, before: Option<f64>, after: Option<f64>) -> Direction {
    match (spec.higher_is_better, before, after) {
        (Some(higher), Some(b), Some(a)) => {
            if (a > b) == higher {
                Direction::Buff
            } else {
                Direction::Nerf
            }
        }
        _ => Direction::Changed,
    }
}

fn size_of(item: &Body) -> Option<f64> {
    item.get("size").and_then(Value::as_f64)
}

/// `compareCategory`: changes for one category between two lists of items.
pub fn compare_category(
    spec: &CategorySpec,
    before: &[Value],
    after: &[Value],
) -> (Vec<ItemChange>, Vec<StatChange>) {
    fn keep<'v>(spec: &CategorySpec, list: &'v [Value]) -> OrderedMap<&'v Body> {
        let mut map = OrderedMap::new();
        for i in list.iter().filter_map(Value::as_object) {
            if let Some(Value::String(r)) = i.get("recordName")
                && spec.include.is_none_or(|f| f(i))
            {
                map.insert(r.clone(), i);
            }
        }
        map
    }
    let old = keep(spec, before);
    let now = keep(spec, after);
    let mut items = Vec::new();
    let mut stats = Vec::new();
    let short = |r: &str| {
        r.strip_prefix("EntityClassDefinition.")
            .unwrap_or(r)
            .to_string()
    };
    let name_of = |item: &Body, record: &str| match item.get("name").filter(|v| !v.is_null()) {
        Some(v) => js_string(Some(v)),
        None => record.to_string(),
    };

    for (record, item) in now.iter() {
        let name = name_of(item, record);
        let size = size_of(item);
        let Some(prior) = old.get(record) else {
            items.push(ItemChange {
                category: spec.category.into(),
                record: short(record),
                name,
                previous_name: None,
                size,
                change: ItemStatus::Added,
                stats_changed: 0,
                buffs: 0,
                nerfs: 0,
            });
            continue;
        };
        let previous_name = (!js_strict_eq(prior.get("name"), item.get("name")))
            .then(|| js_string(prior.get("name")));
        let mut moved = Vec::new();
        for stat in &spec.stats {
            let a = read(prior, stat.path);
            let b = read(item, stat.path);
            if a == b {
                continue;
            }
            let delta = match (a, b) {
                (Some(a), Some(b)) => Some(round3(b - a)),
                _ => None,
            };
            let percent = match (a, delta) {
                (Some(a), Some(d)) if a != 0.0 => Some(round_to((d / a.abs()) * 100.0, 100.0)),
                _ => None,
            };
            moved.push(StatChange {
                category: spec.category.into(),
                record: short(record),
                name: name.clone(),
                previous_name: previous_name.clone(),
                size,
                stat: stat.stat.into(),
                before: a,
                after: b,
                delta,
                percent,
                direction: direction_of(stat, a, b),
            });
        }
        let renamed = previous_name.as_deref().is_some_and(|p| !p.is_empty());
        if moved.is_empty() && !renamed {
            continue;
        }
        let count = |d: Direction| moved.iter().filter(|m| m.direction == d).count();
        let (buffs, nerfs) = (count(Direction::Buff), count(Direction::Nerf));
        let stats_changed = moved.len();
        stats.extend(moved);
        items.push(ItemChange {
            category: spec.category.into(),
            record: short(record),
            name,
            previous_name,
            size,
            change: ItemStatus::Changed,
            stats_changed,
            buffs,
            nerfs,
        });
    }
    for (record, item) in old.iter() {
        if now.contains(record) {
            continue;
        }
        items.push(ItemChange {
            category: spec.category.into(),
            record: short(record),
            name: name_of(item, record),
            previous_name: None,
            size: size_of(item),
            change: ItemStatus::Removed,
            stats_changed: 0,
            buffs: 0,
            nerfs: 0,
        });
    }
    (items, stats)
}

/// `patchChanges` over datasets already loaded: `before(file)` / `after(file)` give a
/// build's `game-<dataset>.json` body, `None` when the build lacks it.
pub fn patch_changes_from<'a>(
    before: &dyn Fn(&str) -> Option<&'a Body>,
    after: &dyn Fn(&str) -> Option<&'a Body>,
) -> PatchChanges {
    let mut result = PatchChanges::default();
    let stamp = |b: Option<&Body>| {
        b.and_then(|b| b.get("_build"))
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    let empty: Vec<Value> = Vec::new();
    for spec in CATEGORIES.iter() {
        let file = dataset_file(spec.dataset);
        let a = before(&file);
        let b = after(&file);
        if result.from.is_none() {
            result.from = stamp(a);
        }
        if result.to.is_none() {
            result.to = stamp(b);
        }
        let (Some(a), Some(b)) = (a, b) else {
            if !result.skipped.iter().any(|s| s == spec.dataset) {
                result.skipped.push(spec.dataset.into());
            }
            continue;
        };
        let list = |body: &'a Body| match body.get(spec.collection) {
            Some(Value::Array(v)) => v,
            _ => &empty,
        };
        let (items, stats) = compare_category(spec, list(a), list(b));
        result.items.extend(items);
        result.stats.extend(stats);
    }
    result.stats.sort_by(|x, y| {
        crate::collate::locale_compare(&x.category, &y.category).then_with(|| {
            let p = |s: &StatChange| s.percent.unwrap_or(0.0).abs();
            p(y).partial_cmp(&p(x)).unwrap_or(std::cmp::Ordering::Equal)
        })
    });
    result
}

/// `patchChanges`: every tracked category between two builds.
pub fn changes(from: &BuildSource, to: &BuildSource) -> Result<PatchChanges> {
    let mut a: HashMap<String, Body> = HashMap::new();
    let mut b: HashMap<String, Body> = HashMap::new();
    for spec in CATEGORIES.iter() {
        let file = dataset_file(spec.dataset);
        if !a.contains_key(&file)
            && let Some(s) = load_snapshot(from, &file)?
        {
            a.insert(file.clone(), s.body);
        }
        if !b.contains_key(&file)
            && let Some(s) = load_snapshot(to, &file)?
        {
            b.insert(file, s.body);
        }
    }
    Ok(patch_changes_from(&|f| a.get(f), &|f| b.get(f)))
}

/// `changes --json`: `JSON.stringify(result, null, 2)`.
pub fn changes_json(result: &PatchChanges) -> String {
    to_js_json(&serde_json::to_value(result).unwrap_or(Value::Null))
}

/// `JSON.stringify(value, null, 1)`: the two-space form with every indent halved (string
/// values never hold a raw line break, so each line's leading spaces are indentation).
fn to_js_json_indent1(value: &Value) -> String {
    to_js_json(value)
        .split('\n')
        .map(|line| {
            let n = line.len() - line.trim_start_matches(' ').len();
            format!("{}{}", " ".repeat(n / 2), &line[n..])
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn cell(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        v => {
            let text = js_string(v);
            if text.contains(['"', ',', '\n']) {
                format!("\"{}\"", text.replace('"', "\"\""))
            } else {
                text
            }
        }
    }
}

/// `toCsv`: a header row and one row per record, cells quoted when they need it.
pub fn to_csv<T: Serialize>(rows: &[T], columns: &[&str]) -> String {
    let mut lines = vec![columns.join(",")];
    for row in rows {
        let v = serde_json::to_value(row).unwrap_or(Value::Null);
        lines.push(
            columns
                .iter()
                .map(|c| cell(v.get(*c)))
                .collect::<Vec<_>>()
                .join(","),
        );
    }
    lines.join("\n") + "\n"
}

pub const STAT_COLUMNS: &[&str] = &[
    "category",
    "name",
    "size",
    "record",
    "stat",
    "before",
    "after",
    "delta",
    "percent",
    "direction",
    "previousName",
];
pub const ITEM_COLUMNS: &[&str] = &[
    "category",
    "name",
    "size",
    "record",
    "change",
    "statsChanged",
    "buffs",
    "nerfs",
    "previousName",
];

/// `<from>_to_<to>`: the export directory name under `<export-dir>/changes/`.
pub fn export_label(result: &PatchChanges, from_spec: &str, to_spec: &str) -> String {
    format!(
        "{}_to_{}",
        result.from.as_deref().unwrap_or(from_spec),
        result.to.as_deref().unwrap_or(to_spec)
    )
}

/// Write `stat_changes.csv`, `item_changes.csv` and `changes.json` into `dir` (created).
pub fn export_changes(result: &PatchChanges, dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::write(
        dir.join("stat_changes.csv"),
        to_csv(&result.stats, STAT_COLUMNS),
    )?;
    std::fs::write(
        dir.join("item_changes.csv"),
        to_csv(&result.items, ITEM_COLUMNS),
    )?;
    let v = serde_json::to_value(result).unwrap_or(Value::Null);
    std::fs::write(dir.join("changes.json"), to_js_json_indent1(&v))
}

/// The `changes` text output, exactly as the CLI prints it (every line ends in a newline,
/// the last one `wrote <dir>`). `top` is `--limit` (default 10).
pub fn render_changes(
    result: &PatchChanges,
    from_spec: &str,
    to_spec: &str,
    top: usize,
    dir: &str,
) -> String {
    let mut out = String::new();
    let mut line = |s: String| {
        out.push_str(&s);
        out.push('\n');
    };
    line(format!(
        "{} -> {}",
        result.from.as_deref().unwrap_or(from_spec),
        result.to.as_deref().unwrap_or(to_spec)
    ));
    let mut categories: Vec<&str> = Vec::new();
    for i in &result.items {
        if !categories.contains(&i.category.as_str()) {
            categories.push(&i.category);
        }
    }
    for category in &categories {
        let items: Vec<&ItemChange> = result
            .items
            .iter()
            .filter(|i| i.category == *category)
            .collect();
        let count = |c: ItemStatus| items.iter().filter(|i| i.change == c).count();
        let stats: Vec<&StatChange> = result
            .stats
            .iter()
            .filter(|s| s.category == *category)
            .collect();
        let dirs = |d: Direction| stats.iter().filter(|s| s.direction == d).count();
        line(format!(
            "  {:<15} +{} -{} ~{}  buffs {}  nerfs {}",
            category,
            count(ItemStatus::Added),
            count(ItemStatus::Removed),
            count(ItemStatus::Changed),
            dirs(Direction::Buff),
            dirs(Direction::Nerf)
        ));
    }
    if categories.is_empty() {
        line("  no tracked stat changed".into());
    }
    if !result.skipped.is_empty() {
        line(format!(
            "  skipped (missing in one build): {}",
            result.skipped.join(", ")
        ));
    }
    let mut moves: Vec<&StatChange> = result
        .stats
        .iter()
        .filter(|s| s.percent.is_some())
        .collect();
    moves.sort_by(|a, b| {
        let p = |s: &StatChange| s.percent.unwrap_or(0.0).abs();
        p(b).partial_cmp(&p(a)).unwrap_or(std::cmp::Ordering::Equal)
    });
    moves.truncate(top);
    if !moves.is_empty() {
        line("\n  largest moves:".into());
    }
    let num = |x: Option<f64>| js_string(x.map(crate::js::number).as_ref());
    for s in moves {
        let percent = s.percent.unwrap_or(0.0);
        line(format!(
            "    {:<7} {} (S{}) {}: {} -> {} ({}{}%)",
            s.direction.as_str(),
            s.name,
            s.size.map_or_else(|| "?".to_string(), |x| num(Some(x))),
            s.stat,
            num(s.before),
            num(s.after),
            if percent > 0.0 { "+" } else { "" },
            num(s.percent)
        ));
    }
    line(format!("\nwrote {dir}"));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn spec() -> CategorySpec {
        CategorySpec {
            category: "shipWeapons",
            dataset: "weapons",
            collection: "shipWeapons",
            stats: vec![
                up("dps", "dps"),
                down("em", "em"),
                up("burstDamage", "burst.damage"),
                neutral("crew", "crew"),
            ],
            include: Some(not_template),
        }
    }

    fn gun(record: &str, name: &str, extra: Value) -> Value {
        let mut v = json!({ "recordName": format!("EntityClassDefinition.{record}"), "name": name, "size": 5 });
        for (k, x) in extra.as_object().unwrap() {
            v[k] = x.clone();
        }
        v
    }

    #[test]
    fn direction_reads_which_side_is_better() {
        let s = spec();
        assert_eq!(
            direction_of(&s.stats[0], Some(100.0), Some(120.0)),
            Direction::Buff
        );
        assert_eq!(
            direction_of(&s.stats[0], Some(120.0), Some(100.0)),
            Direction::Nerf
        );
        assert_eq!(
            direction_of(&s.stats[1], Some(900.0), Some(1100.0)),
            Direction::Nerf
        );
        assert_eq!(
            direction_of(&s.stats[3], Some(1.0), Some(2.0)),
            Direction::Changed
        );
        assert_eq!(
            direction_of(&s.stats[0], None, Some(2.0)),
            Direction::Changed
        );
    }

    fn compared() -> (Vec<ItemChange>, Vec<StatChange>) {
        let before = vec![
            gun(
                "BEHR_LaserCannon_S5",
                "M7A Cannon",
                json!({"dps": 1536.3, "em": 941, "burst": {"damage": 23044.5}}),
            ),
            gun(
                "KLWE_LaserRepeater_S3",
                "CF-337 Panther Repeater",
                json!({"dps": 545.63}),
            ),
            gun(
                "GATS_BallisticGatling_S3",
                "Mantis GT-220 Gatling",
                json!({"dps": 853.33}),
            ),
            gun("Unchanged", "Steady", json!({"dps": 10})),
        ];
        let after = vec![
            gun(
                "BEHR_LaserCannon_S5",
                "M7A Cannon",
                json!({"dps": 1400, "em": 1100, "burst": {"damage": 23044.5}}),
            ),
            gun(
                "KLWE_LaserRepeater_S3",
                "CF-337 Panther Mk II",
                json!({"dps": 545.63}),
            ),
            gun("Unchanged", "Steady", json!({"dps": 10})),
            gun("New_Gun", "Fresh", json!({"dps": 1})),
            gun(
                "Template_Gun",
                "Template",
                json!({"dps": 1, "isTemplate": true}),
            ),
        ];
        compare_category(&spec(), &before, &after)
    }

    #[test]
    fn one_row_per_moved_stat() {
        let (_, stats) = compared();
        assert_eq!(stats.len(), 2);
        assert_eq!(stats[0].record, "BEHR_LaserCannon_S5");
        assert_eq!(stats[0].stat, "dps");
        assert_eq!(stats[0].before, Some(1536.3));
        assert_eq!(stats[0].after, Some(1400.0));
        assert_eq!(stats[0].delta, Some(-136.3));
        assert_eq!(stats[0].percent, Some(-8.87));
        assert_eq!(stats[0].direction, Direction::Nerf);
        assert_eq!(stats[1].stat, "em");
        assert_eq!(stats[1].delta, Some(159.0));
        assert_eq!(stats[1].percent, Some(16.9));
        assert_eq!(stats[1].direction, Direction::Nerf);
    }

    #[test]
    fn rename_is_a_change_carrying_the_old_name() {
        let (items, _) = compared();
        let i = items
            .iter()
            .find(|i| i.record == "KLWE_LaserRepeater_S3")
            .unwrap();
        assert_eq!(i.change, ItemStatus::Changed);
        assert_eq!(i.name, "CF-337 Panther Mk II");
        assert_eq!(i.previous_name.as_deref(), Some("CF-337 Panther Repeater"));
        assert_eq!(i.stats_changed, 0);
    }

    #[test]
    fn adds_removes_and_filters() {
        let (items, _) = compared();
        let by = |r: &str| items.iter().find(|i| i.record == r).map(|i| i.change);
        assert_eq!(by("New_Gun"), Some(ItemStatus::Added));
        assert_eq!(by("GATS_BallisticGatling_S3"), Some(ItemStatus::Removed));
        assert_eq!(by("Unchanged"), None);
        assert_eq!(by("Template_Gun"), None);
        let m7a = items
            .iter()
            .find(|i| i.record == "BEHR_LaserCannon_S5")
            .unwrap();
        assert_eq!((m7a.stats_changed, m7a.buffs, m7a.nerfs), (2, 0, 2));
    }

    #[test]
    fn patch_changes_reads_both_builds() {
        let tmp = tempfile::tempdir().unwrap();
        let write = |dir: &str, build: &str, dps: u32| {
            let d = tmp.path().join(dir);
            std::fs::create_dir_all(&d).unwrap();
            let body = json!({"_build": build, "shipWeapons": [gun("A", "Alpha", json!({"dps": dps}))], "fpsWeapons": [], "missiles": []});
            std::fs::write(d.join("game-weapons.json"), body.to_string()).unwrap();
            BuildSource::dir(d)
        };
        let old = write("old", "1.0.1", 100);
        let new = write("new", "1.0.2", 125);
        let r = changes(&old, &new).unwrap();
        assert_eq!(r.from.as_deref(), Some("1.0.1"));
        assert_eq!(r.to.as_deref(), Some("1.0.2"));
        assert_eq!(r.stats.len(), 1);
        assert_eq!(
            (
                r.stats[0].stat.as_str(),
                r.stats[0].percent,
                r.stats[0].direction
            ),
            ("dps", Some(25.0), Direction::Buff)
        );
        assert!(r.skipped.contains(&"ships".to_string()));
        assert!(r.skipped.contains(&"ship-components".to_string()));
        // Key order of the JSON document: the arrays first, then the stamps.
        assert!(
            changes_json(&r).find("\"skipped\"").unwrap()
                < changes_json(&r).find("\"from\"").unwrap()
        );
    }

    #[test]
    fn csv_quotes_cells_and_leaves_blanks() {
        let rows = vec![json!({"a": "say \"hi\", ok"})];
        assert_eq!(to_csv(&rows, &["a", "b"]), "a,b\n\"say \"\"hi\"\", ok\",\n");
    }

    #[test]
    fn player_hull_pattern() {
        for (r, npc) in [
            ("EntityClassDefinition.AEGS_Gladius", false),
            ("EntityClassDefinition.AEGS_Gladius_PU_AI_CIV", true),
            ("x_AI", true),
            ("x_tier_3", true),
            ("x_tier_x", false),
            ("x_fw22", true),
            ("x_Plat", true),
            ("x_plate", false),
        ] {
            assert_eq!(npc_hull(r), npc, "{r}");
        }
    }

    #[test]
    fn indent_one_halves_indentation() {
        assert_eq!(
            to_js_json_indent1(&json!({"a": [1, {"b": "x  y"}]})),
            "{\n \"a\": [\n  1,\n  {\n   \"b\": \"x  y\"\n  }\n ]\n}"
        );
    }
}
