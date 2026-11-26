//! Ships and ground vehicles (port of `sc-gamedata/src/domains/ships.ts`).
//!
//! A vehicle is an `EntityClassDefinition` carrying `VehicleComponentParams`, which holds
//! the identity fields: name, career, role, crew size, manufacturer and bounding box.
//! Career and role are localization keys (`@vehicle_focus_combat`), so the string table is
//! required.
//!
//! The stock loadout is `SEntityComponentDefaultLoadoutParams`: a tree of item ports, each
//! naming the item that ships in it (by class name or file reference) and, for mounts and
//! racks, the items nested inside. Items are sorted by what they are, read off the
//! resolved item's `AttachDef.Type`, not by what the port is called. Resolving needs the
//! ship item records and, for guns, their ammo.
//!
//! `totals` sums the loadout into the figures a patch is most likely to move: stock DPS,
//! shield pool, power output, fuel, cargo and thrust. `totals.byStateEstimate` sums the
//! stock components' resource-network draw and signatures per power state, and
//! `fireParts` names the stock parts listed in `game-fire` (see [`ASSUMPTIONS`]).
//!
//! `capabilities` flags what the stock loadout and role let a vehicle do (mine, salvage,
//! refuel, ...), by sc-telemetry's `derive_caps` rules (`sctelemetry/shipcaps.py`).
//!
//! Hull health on `SHealthComponentParams` reads 1.0 (normalised; the real figure is in
//! the vehicle XML, not DataCore), so no hull HP is invented; the normalisation value and
//! the hull's damage multipliers are surfaced instead. Flight numbers come from the stock
//! flight controller's `IFCSParams`.
//!
//! Known difference from the golden `game-ships.json` (LIVE 4.10.200.58746): the golden
//! has 54 extra vehicles (`AEGS_Hammerhead*` without `_GS`, `RSI_Constellation_Andromeda*`,
//! `_Aquila*`, `_Phoenix*`, `_Taurus*` variants, `RSI_Aurora_*_PU_AI_*`) and the
//! `byManufacturer`/`byRole`/`summary` counts that follow from them. Those records are not
//! in the 4.10.200 DataCore: they are stale files from a 2026-11-01 extraction of an
//! earlier build left in sc-gamedata's `extracted-data/` (its extraction does not clear
//! the directory, and `loadRecords` reads every file there). Not replicated. With those
//! 54 removed from the golden, every vehicle (order included) and every count is
//! identical.
//!
//! The parts of `weapons.ts`/`ammo.ts` this reads (gun DPS, missile damage) live in
//! [`crate::helpers_ships::weapons`]; `components.ts` is [`crate::domains::components`].

use crate::collate::locale_compare;
use crate::domain::{Domain, DomainError, ParseContext, field};
use crate::domains::components::{Component, index_containers, parse_component};
use crate::domains::fire::parse_fire_item;
use crate::entity::{component, referenced_name, vec};
use crate::helpers_ships::item_common::{
    ResourceProfile, attach_def, health_resistances, item_identity, resource_states,
};
use crate::helpers_ships::weapons::{
    MissileSummary, WeaponContext, WeaponSummary, index_ammo, parse_bomb, parse_missile,
    parse_weapon, total_damage,
};
use crate::helpers_ships::{at_map, number, round_to};
use crate::localization::{LocRefs, Localization, is_placeholder_name, loc_refs, localize};
use crate::output::js_key_order;
use crate::records::{GameRecord, RecordSet, array, at, num, str_};
use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};

pub static DOMAIN: Ships = Ships;

/// What `totals.byStateEstimate` and `fireParts` rest on that the records do not state.
pub const ASSUMPTIONS: &[&str] = &[
    "totals.byStateEstimate: per power state, the sum over the stock loadout's components (items in game-ship-components) of the resource-network Power and Coolant consumption (powerDraw, coolantDraw) and the EM / IR nominalSignature (em, ir). Every stock part is assumed to be in that state at once: Online is every part in its Online state (its first state when it has no Online, e.g. Idle); another key (Travelling, Transiting, ...) switches the parts that record that state to it and keeps the rest as in Online, and is left out when its sums equal Online's. How the game combines the parts (power allocation, signature falloff) is not stated.",
    "fireParts: the stock radars, power plants, coolers, shields, quantum and jump drives, life support, batteries, fuel and quantum fuel tanks, missiles and bombs that game-fire lists with a health pool (MCsim build_fire.py's slots).",
];

pub struct Ships;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Vehicle {
    pub name: String,
    pub record_name: String,
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// `nameKey`, `descriptionKey`, `nameStatus`.
    #[serde(flatten)]
    pub loc: LocRefs,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manufacturer: Option<String>,
    /// `Spaceship`, `Gravlev`, `Ground` and similar.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub movement_class: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub career: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crew_size: Option<f64>,
    /// The game's own size class, 1 to 6.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_gravlev: Option<bool>,
    /// Metres, as x/y/z.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bounding_box: Option<Map<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hull: Option<Hull>,
    /// Radar cross section, metres squared per axis.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cross_section: Option<Map<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loadout: Option<Map<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub totals: Option<Map<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flight: Option<Flight>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub armor: Option<Map<String, Value>>,
    /// Power pips the ship can route to each item type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub power_pools: Option<Map<String, Value>>,
    /// `DynamicPowerPool` entries: item type to the record's `maxItemCount` (-1 as recorded).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dynamic_power_pools: Option<Map<String, Value>>,
    /// Stock loadout items listed in `game-fire` with health (see [`ASSUMPTIONS`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fire_parts: Option<Vec<String>>,
    pub capabilities: Capabilities,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_template: Option<bool>,
}

/// What the stock loadout and role let a vehicle do. Keys and rules are sc-telemetry's
/// `derive_caps`; `cargoKnown: false` marks a vehicle with no cargo data at all (no cargo
/// grid in the stock loadout, no cargo SCU), where `cargo: false` means unknown, not none.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Capabilities {
    pub mining: bool,
    pub salvage: bool,
    pub refuel: bool,
    pub tractor: bool,
    pub guns: bool,
    pub scan: bool,
    pub quantum: bool,
    pub qed: bool,
    pub emp: bool,
    pub missiles: bool,
    pub cargo: bool,
    /// `totals.cargoScu` to 2 dp when positive, else null.
    pub cargo_scu: Option<f64>,
    #[serde(rename = "cargoKnown", skip_serializing_if = "Option::is_none")]
    pub cargo_known: Option<bool>,
}

/// `_present`: a bool is itself, a number is > 0, a list or object is non-empty.
fn present(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|x| x > 0.0),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
        _ => false,
    }
}

/// `derive_caps` over a vehicle's role, stock loadout and totals. `salvage` is the role or a
/// stock salvage head (`loadout.salvageHeads`), the same vehicles `game-salvage` lists.
pub fn derive_capabilities(
    role: Option<&str>,
    loadout: Option<&Map<String, Value>>,
    totals: Option<&Map<String, Value>>,
) -> Capabilities {
    let role = role.unwrap_or_default().to_lowercase();
    let slot = |k: &str| loadout.and_then(|l| l.get(k));
    let has_type = |t: &str| {
        array(slot("weapons"))
            .iter()
            .any(|w| w.get("type").and_then(Value::as_str) == Some(t))
    };
    let cargo_scu = totals
        .and_then(|t| t.get("cargoScu"))
        .and_then(Value::as_f64)
        .filter(|x| *x > 0.0)
        .map(round2);
    let cargo = cargo_scu.is_some() || present(slot("cargoGrids"));
    Capabilities {
        mining: has_type("WeaponMining") || role.contains("mining"),
        salvage: role.contains("salvage") || present(slot("salvageHeads")),
        refuel: role.contains("refuel"),
        tractor: has_type("TractorBeam"),
        guns: has_type("WeaponGun") || present(slot("turrets")),
        scan: present(slot("radars")),
        quantum: present(slot("quantumDrives")),
        qed: present(slot("quantumEnforcement")),
        emp: has_type("EMP"),
        missiles: present(slot("missileRacks")),
        cargo,
        cargo_scu,
        cargo_known: (!cargo).then_some(false),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Hull {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub damage_normalization: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resistances: Option<Map<String, Value>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BoostCapacitor {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub regen_per_second: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub regen_delay: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Flight {
    /// Item record of the flight controller the numbers came from.
    pub controller: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scm_speed: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_speed: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub boost_speed_forward: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub boost_speed_backward: Option<f64>,
    /// Degrees per second.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pitch: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub yaw: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub roll: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub boost_capacitor: Option<BoostCapacitor>,
    /// G-load that stuns the pilot.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stun_threshold_gs: Option<f64>,
}

/// The records a loadout resolves against (`VehicleContext`).
pub struct VehicleContext<'a> {
    pub localization: &'a Localization,
    /// Ship item records by lower-case short name.
    pub items: HashMap<String, &'a GameRecord>,
    pub components: HashMap<String, Component>,
    pub weapons: HashMap<String, WeaponSummary>,
    pub missiles: HashMap<String, MissileSummary>,
    pub bombs: HashMap<String, MissileSummary>,
    /// Resource-network states of the items that are components, when they have any.
    pub states: HashMap<String, Vec<(String, ResourceProfile)>>,
    /// Items `game-fire` lists with a health pool.
    pub fire_parts: HashSet<String>,
}

/// `tidyClassifier`: `@vehicle_focus_combat` -> `combat`.
fn tidy_classifier(value: Option<&str>) -> Option<String> {
    let value = value.filter(|v| !v.is_empty())?;
    if !value.starts_with('@') {
        return Some(value.to_string());
    }
    // `.replace(/^@vehicle_(focus|class)_/i, "").replace(/^@/, "")`
    let lower = value.to_ascii_lowercase();
    let mut tail = value;
    for prefix in ["@vehicle_focus_", "@vehicle_class_"] {
        if lower.starts_with(prefix) {
            tail = &value[prefix.len()..];
            break;
        }
    }
    let tail = tail.strip_prefix('@').unwrap_or(tail);
    (!tail.is_empty()).then(|| tail.to_string())
}

fn parse_hull(record: &GameRecord) -> Option<Hull> {
    let vehicle = component(record, "VehicleComponentParams");
    let health = component(record, "SHealthComponentParams");
    let hull = Hull {
        damage_normalization: num(
            vehicle.and_then(|v| v.get("vehicleHullDamageNormalizationValue")),
        ),
        resistances: health_resistances(health.and_then(|h| h.get("DamageResistances"))),
    };
    (hull.damage_normalization.is_some() || hull.resistances.is_some()).then_some(hull)
}

/// One flattened loadout entry; `parent` indexes the same list.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub port: String,
    pub item: String,
    pub parent: Option<usize>,
}

/// `loadoutEntries`: flatten the loadout tree, depth first, keeping each entry's parent.
pub fn loadout_entries(record: &GameRecord) -> Vec<Entry> {
    fn walk(loadout: Option<&Value>, parent: Option<usize>, out: &mut Vec<Entry>) {
        let Some(entries) = loadout.and_then(|l| at(l, "entries")).and_then(Value::as_array)
        else {
            return;
        };
        for e in entries.iter().filter(|e| e.is_object()) {
            let item = str_(e.get("entityClassName"))
                .or_else(|| referenced_name(e.get("entityClassReference")))
                .filter(|s| !s.is_empty());
            let port = str_(e.get("itemPortName"));
            let (Some(item), Some(port)) = (item, port) else {
                continue;
            };
            out.push(Entry {
                port: port.to_string(),
                item: item.to_string(),
                parent,
            });
            let me = out.len() - 1;
            walk(e.get("loadout"), Some(me), out);
        }
    }
    let mut out = Vec::new();
    walk(
        component(record, "SEntityComponentDefaultLoadoutParams").and_then(|c| c.get("loadout")),
        None,
        &mut out,
    );
    out
}

/// `round(value, 2)`.
fn round2(x: f64) -> f64 {
    round_to(x, 100.0)
}

/// `sum`: rounded total of the present values, `None` when there are none.
fn sum(values: impl IntoIterator<Item = Option<f64>>) -> Option<f64> {
    let present: Vec<f64> = values.into_iter().flatten().collect();
    (!present.is_empty()).then(|| round2(present.iter().fold(0.0, |a, b| a + b)))
}

/// An object from `(key, value)` pairs in literal order, `None` values left out.
fn obj(fields: Vec<(&str, Option<Value>)>) -> Map<String, Value> {
    fields
        .into_iter()
        .filter_map(|(k, v)| v.map(|v| (k.to_string(), v)))
        .collect()
}

fn s(x: &str) -> Value {
    Value::String(x.to_string())
}

/// `countDuplicates`: identical entries (compared without `count`) collapse into one,
/// whose `count` key is appended at the end, as the JS assignment does.
pub fn count_duplicates(items: &[Value]) -> Vec<Value> {
    let identity = |v: &Value| {
        let mut v = v.clone();
        if let Value::Object(m) = &mut v {
            m.shift_remove("count");
        }
        serde_json::to_string(&v).unwrap_or_default()
    };
    let mut out: Vec<Value> = Vec::new();
    let mut keys: Vec<String> = Vec::new();
    for item in items {
        let id = identity(item);
        match keys.iter().position(|k| *k == id) {
            Some(i) => {
                if let Value::Object(m) = &mut out[i] {
                    let n = m.get("count").and_then(Value::as_f64).unwrap_or(1.0) + 1.0;
                    m.insert("count".into(), number(n));
                }
            }
            None => {
                out.push(item.clone());
                keys.push(id);
            }
        }
    }
    out
}

const GUN_TYPES: [&str; 5] = [
    "WeaponGun",
    "WeaponMining",
    "TractorBeam",
    "EMP",
    "WeaponDefensive",
];

/// `parseFlight`: IFCS speeds and rates off a flight controller.
pub fn parse_flight(controller: &GameRecord) -> Option<Flight> {
    let ifcs = component(controller, "IFCSParams")?;
    // `maxAngularVelocity` is the live figure; `speedProfile.angularVelocity` reads a
    // 60/200/60 placeholder on most hulls, so it is only a fallback (`??`).
    let angular = ifcs
        .get("maxAngularVelocity")
        .filter(|v| !v.is_null())
        .or_else(|| at_map(ifcs, "speedProfile.angularVelocity"));
    let boost = ifcs.get("afterburnerNew");
    let b = |k: &str| num(boost.and_then(|b| at(b, k)));
    let capacitor = BoostCapacitor {
        max: b("capacitorMax"),
        regen_per_second: b("capacitorRegenPerSec"),
        regen_delay: b("capacitorRegenDelayAfterUse"),
    };
    let has_capacitor = capacitor.max.is_some()
        || capacitor.regen_per_second.is_some()
        || capacitor.regen_delay.is_some();
    let a = |k: &str| num(angular.and_then(|a| at(a, k)));
    Some(Flight {
        controller: controller.short_name.clone(),
        scm_speed: num(ifcs.get("scmSpeed")),
        max_speed: num(ifcs.get("maxSpeed")),
        boost_speed_forward: num(ifcs.get("boostSpeedForward")),
        boost_speed_backward: num(ifcs.get("boostSpeedBackward")),
        // IFCS axes: x is pitch, y is roll, z is yaw.
        pitch: a("x"),
        yaw: a("z"),
        roll: a("y"),
        boost_capacitor: has_capacitor.then_some(capacitor),
        stun_threshold_gs: num(at_map(ifcs, "ifcsCoreParams.stunThresholdGs")),
    })
}

fn parse_armor(component: Option<&Component>) -> Option<Map<String, Value>> {
    let c = component?;
    let detail = |k: &str| c.detail.as_ref().and_then(|d| d.get(k)).cloned();
    // `recordName.replace(/^[^.]+\./, "")`
    let item = match c.record_name.find('.') {
        Some(i) if i > 0 => &c.record_name[i + 1..],
        _ => c.record_name.as_str(),
    };
    Some(obj(vec![
        ("item", Some(s(item))),
        ("health", c.health.map(number)),
        ("deflection", detail("deflection")),
        ("damageMultiplier", detail("damageMultiplier")),
        (
            "signals",
            (!c.stats.is_empty()).then(|| Value::Object(c.stats.clone())),
        ),
    ]))
}

/// `parsePowerPools`: power pips by item type, positive pools only.
pub fn parse_power_pools(record: &GameRecord) -> Option<Map<String, Value>> {
    let pools = component(record, "SItemPortContainerComponentParams")
        .and_then(|c| at_map(c, "resourceNetworkPowerPools.itemPools"))?
        .as_array()?;
    let mut out = Map::new();
    for pool in pools {
        let ty = str_(at(pool, "itemType"));
        let size = num(at(pool, "poolSize"));
        if let (Some(ty), Some(size)) = (ty, size)
            && size > 0.0
        {
            out.insert(ty.to_string(), number(size));
        }
    }
    (!out.is_empty()).then_some(out)
}

/// `DynamicPowerPool` entries: item type to `maxItemCount`, as recorded.
pub fn parse_dynamic_power_pools(record: &GameRecord) -> Option<Map<String, Value>> {
    let pools = component(record, "SItemPortContainerComponentParams")
        .and_then(|c| at_map(c, "resourceNetworkPowerPools.itemPools"))?
        .as_array()?;
    let mut out = Map::new();
    for pool in pools {
        let ty = str_(at(pool, "itemType"));
        let max = num(at(pool, "maxItemCount"));
        if let (Some(ty), Some(max)) = (ty, max) {
            out.insert(ty.to_string(), number(max));
        }
    }
    (!out.is_empty()).then_some(out)
}

/// Whether a vehicle's stock loadout carries a salvage head, by the rules [`parse_loadout`]
/// files one under `salvageHeads`.
pub fn carries_salvage_head(record: &GameRecord, items: &HashMap<String, &GameRecord>) -> bool {
    let entries = loadout_entries(record);
    let ty = |item: &str| {
        items
            .get(&item.to_lowercase())
            .and_then(|r| str_(attach_def(r).get("Type")))
    };
    entries.iter().any(|e| {
        ty(&e.item) == Some("SalvageHead")
            && !e
                .parent
                .and_then(|p| ty(&entries[p].item))
                .is_some_and(|t| GUN_TYPES.contains(&t))
    })
}

/// Loadout lists `fireParts` reads, in MCsim `build_fire.py`'s order (bombs added).
const FIRE_SLOTS: [&str; 12] = [
    "radars",
    "powerPlants",
    "coolers",
    "shields",
    "quantumDrives",
    "jumpDrives",
    "lifeSupport",
    "batteries",
    "fuelTanks",
    "quantumFuelTanks",
    "missiles",
    "bombs",
];

/// A part's state profile by name, else its default (`Online`, else its first state).
fn state_or_default<'a>(
    states: &'a [(String, ResourceProfile)],
    name: &str,
) -> Option<&'a ResourceProfile> {
    let default = || {
        states
            .iter()
            .find(|(n, _)| n == "Online")
            .or_else(|| states.first())
            .map(|(_, p)| p)
    };
    if name == "Online" {
        return default();
    }
    states
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, p)| p)
        .or_else(default)
}

/// `totals.byStateEstimate`: draw and signatures of the stock components per power state.
fn by_state(parts: &[&[(String, ResourceProfile)]]) -> Option<Map<String, Value>> {
    let mut names: Vec<&str> = vec!["Online"];
    for states in parts {
        let default = state_or_default(states, "Online");
        for (n, p) in states.iter() {
            if !names.contains(&n.as_str()) && !default.is_some_and(|d| std::ptr::eq(d, p)) {
                names.push(n);
            }
        }
    }
    let mut out = Map::new();
    for name in names {
        let picked: Vec<&ResourceProfile> = parts
            .iter()
            .filter_map(|states| state_or_default(states, name))
            .collect();
        let total = |get: fn(&ResourceProfile) -> Option<f64>| {
            sum(picked.iter().copied().map(get)).map(number)
        };
        let row = obj(vec![
            ("powerDraw", total(|p| p.consumed("Power"))),
            ("coolantDraw", total(|p| p.consumed("Coolant"))),
            ("em", total(|p| p.em)),
            ("ir", total(|p| p.ir)),
        ]);
        // Another state that sums to the same figures as Online says nothing new.
        let same_as_online = name != "Online" && out.get("Online") == Some(&Value::Object(row.clone()));
        if !row.is_empty() && !same_as_online {
            out.insert(name.to_string(), Value::Object(row));
        }
    }
    (!out.is_empty()).then_some(out)
}

/// What [`parse_loadout`] resolves.
#[derive(Debug, Default)]
pub struct Resolved {
    pub loadout: Option<Map<String, Value>>,
    pub totals: Option<Map<String, Value>>,
    pub flight: Option<Flight>,
    pub armor: Option<Map<String, Value>>,
    pub fire_parts: Option<Vec<String>>,
}

fn push(loadout: &mut Map<String, Value>, key: &str, item: Map<String, Value>) {
    if let Value::Array(a) = loadout
        .entry(key.to_string())
        .or_insert_with(|| Value::Array(Vec::new()))
    {
        a.push(Value::Object(item));
    }
}

fn retro_port(port: &str) -> bool {
    port.to_lowercase().contains("retro")
}

fn f(v: &Value, k: &str) -> Option<f64> {
    v.get(k).and_then(Value::as_f64)
}

/// `parseLoadout`.
pub fn parse_loadout(record: &GameRecord, ctx: &VehicleContext<'_>) -> Resolved {
    let mut loadout = Map::new();
    let mut flight: Option<Flight> = None;
    let (mut main, mut retro, mut maneuver) = (0u32, 0u32, 0u32);
    let mut countermeasures = Map::new();
    let mut turrets = 0u32;
    let mut racks = 0u32;
    let mut powered: Vec<&[(String, ResourceProfile)]> = Vec::new();

    let entries = loadout_entries(record);
    for entry in &entries {
        let Some(item_record) = ctx.items.get(&entry.item.to_lowercase()).copied() else {
            continue;
        };
        // AI variants fix a gun straight to the hardpoint and keep a stale child entry
        // naming the same gun under it. A gun holds attachments, not guns.
        let parent = entry.parent.map(|p| &entries[p]);
        let parent_record = parent.and_then(|p| ctx.items.get(&p.item.to_lowercase()));
        if let Some(pr) = parent_record
            && GUN_TYPES.contains(&str_(attach_def(pr).get("Type")).unwrap_or(""))
        {
            continue;
        }
        let def = attach_def(item_record);
        let ty = str_(def.get("Type"));
        let sub_type = str_(def.get("SubType"));
        let key = item_record.short_name.to_lowercase();
        let comp = ctx.components.get(&key);
        if let Some(states) = ctx.states.get(&key) {
            powered.push(states);
        }
        let base_name = comp
            .filter(|c| c.is_template.is_none())
            .map(|c| c.name.clone());
        let base_size = comp.and_then(|c| c.size).or_else(|| num(def.get("Size")));
        let base = |extra: Vec<(&str, Option<Value>)>| {
            let mut fields = vec![
                ("item", Some(s(&item_record.short_name))),
                ("name", base_name.clone().map(Value::String)),
                ("size", base_size.map(number)),
            ];
            for (k, v) in extra {
                match fields.iter_mut().find(|(fk, _)| *fk == k) {
                    Some(slot) => slot.1 = v,
                    None => fields.push((k, v)),
                }
            }
            obj(fields)
        };
        let parent_item = parent.map(|p| s(&p.item));

        match ty {
            Some("Shield") => push(&mut loadout, "shields", base(vec![])),
            Some("PowerPlant") => push(&mut loadout, "powerPlants", base(vec![])),
            Some("Cooler") => push(&mut loadout, "coolers", base(vec![])),
            Some("QuantumDrive") => push(&mut loadout, "quantumDrives", base(vec![])),
            Some("QuantumInterdictionGenerator") => {
                push(&mut loadout, "quantumEnforcement", base(vec![]));
            }
            Some("JumpDrive") => push(&mut loadout, "jumpDrives", base(vec![])),
            Some("Radar") => push(&mut loadout, "radars", base(vec![])),
            Some("Armor") => {
                loadout.insert("armor".into(), Value::Object(base(vec![])));
            }
            Some("FlightController") => {
                if flight.is_none() {
                    flight = parse_flight(item_record);
                }
            }
            Some("LifeSupportGenerator") => push(&mut loadout, "lifeSupport", base(vec![])),
            Some("FuelTank" | "ExternalFuelTank") => {
                push(&mut loadout, "fuelTanks", base(vec![]));
            }
            Some("QuantumFuelTank") => push(&mut loadout, "quantumFuelTanks", base(vec![])),
            Some("CargoGrid") => push(&mut loadout, "cargoGrids", base(vec![])),
            Some(t @ ("MainThruster" | "ManneuverThruster")) => {
                if sub_type == Some("Retro") || retro_port(&entry.port) {
                    retro += 1;
                } else if t == "MainThruster" {
                    main += 1;
                } else {
                    maneuver += 1;
                }
            }
            Some("Turret" | "TurretBase" | "UtilityTurret") => turrets += 1,
            Some("MissileLauncher" | "GroundVehicleMissileLauncher" | "BombLauncher") => {
                racks += 1;
            }
            Some("Missile") => {
                let missile = ctx.missiles.get(&key);
                let name = missile
                    .map(|m| m.name.clone())
                    .or_else(|| base_name.clone());
                let size = missile.and_then(|m| m.size).or(base_size);
                push(
                    &mut loadout,
                    "missiles",
                    base(vec![
                        ("name", name.map(Value::String)),
                        ("size", size.map(number)),
                        ("rack", parent_item),
                        (
                            "damage",
                            total_damage(missile.and_then(|m| m.damage.as_ref())).map(number),
                        ),
                    ]),
                );
            }
            Some("Bomb") => {
                let bomb = ctx.bombs.get(&key);
                let name = bomb.map(|m| m.name.clone()).or_else(|| base_name.clone());
                let size = bomb.and_then(|m| m.size).or(base_size);
                push(
                    &mut loadout,
                    "bombs",
                    base(vec![
                        ("name", name.map(Value::String)),
                        ("size", size.map(number)),
                        ("rack", parent_item),
                        (
                            "damage",
                            total_damage(bomb.and_then(|m| m.damage.as_ref())).map(number),
                        ),
                    ]),
                );
            }
            Some("Battery") => push(&mut loadout, "batteries", base(vec![])),
            Some(t @ ("SalvageHead" | "SalvageModifier" | "SalvageInternalStorage")) => {
                let identity = item_identity(item_record, ctx.localization);
                let name = base_name
                    .clone()
                    .or_else(|| identity.is_template.is_none().then_some(identity.name));
                let (list, extra) = match t {
                    "SalvageHead" => ("salvageHeads", ("mount", parent_item)),
                    "SalvageModifier" => ("salvageModifiers", ("head", parent_item)),
                    _ => (
                        "salvageStorage",
                        (
                            "scu",
                            num(component(item_record, "ResourceContainer")
                                .and_then(|c| at_map(c, "capacity.standardCargoUnits")))
                            .map(number),
                        ),
                    ),
                };
                push(
                    &mut loadout,
                    list,
                    base(vec![("name", name.map(Value::String)), extra]),
                );
            }
            Some("WeaponDefensive") => {
                let ammo = comp
                    .and_then(|c| c.detail.as_ref())
                    .and_then(|d| str_(d.get("ammo")));
                let kind = ammo.and_then(|a| a.rsplit('_').next());
                let capacity = comp.and_then(|c| c.stat("maxAmmoCount"));
                if let (Some(kind), Some(capacity)) = (kind.filter(|k| !k.is_empty()), capacity) {
                    let prev = countermeasures
                        .get(kind)
                        .and_then(Value::as_f64)
                        .unwrap_or(0.0);
                    countermeasures.insert(kind.to_string(), number(prev + capacity));
                }
                push(
                    &mut loadout,
                    "countermeasures",
                    base(vec![
                        ("ammo", ammo.map(s)),
                        ("capacity", capacity.map(number)),
                    ]),
                );
            }
            Some(t) if GUN_TYPES.contains(&t) => {
                let weapon = ctx.weapons.get(&key);
                let name = weapon
                    .map(|w| w.name.clone())
                    .or_else(|| base_name.clone());
                let size = weapon.and_then(|w| w.size).or(base_size);
                push(
                    &mut loadout,
                    "weapons",
                    base(vec![
                        ("name", name.map(Value::String)),
                        ("size", size.map(number)),
                        ("type", Some(s(t))),
                        ("mount", parent_item),
                        ("dps", weapon.and_then(|w| w.dps).map(number)),
                    ]),
                );
            }
            _ => {}
        }
    }

    // `for (const key of Object.keys(loadout))`: collapse every list.
    let keys: Vec<String> = js_key_order(&loadout).into_iter().cloned().collect();
    for k in keys {
        if let Some(Value::Array(list)) = loadout.get(&k) {
            let collapsed = count_duplicates(list);
            loadout.insert(k, Value::Array(collapsed));
        }
    }

    if main + retro + maneuver > 0 {
        let n = |c: u32| (c > 0).then(|| Value::from(c));
        loadout.insert(
            "thrusters".into(),
            Value::Object(obj(vec![
                ("main", n(main)),
                ("retro", n(retro)),
                ("maneuver", n(maneuver)),
            ])),
        );
    }
    if turrets > 0 {
        loadout.insert("turrets".into(), Value::from(turrets));
    }
    if racks > 0 {
        loadout.insert("missileRacks".into(), Value::from(racks));
    }
    if loadout.is_empty() {
        return Resolved {
            flight,
            ..Resolved::default()
        };
    }

    let list = |k: &str| -> &[Value] { array(loadout.get(k)) };
    let times = |value: Option<f64>, count: Option<f64>| value.map(|v| v * count.unwrap_or(1.0));
    let stat = |k: &str, field: &str| {
        sum(list(k).iter().map(|i| {
            let item = i.get("item").and_then(Value::as_str).unwrap_or_default();
            times(
                ctx.components
                    .get(&item.to_lowercase())
                    .and_then(|c| c.stat(field)),
                f(i, "count"),
            )
        }))
    };
    let count_of = |k: &str| {
        loadout.get(k).and_then(Value::as_array).map(|items| {
            items
                .iter()
                .map(|i| f(i, "count").unwrap_or(1.0))
                .fold(0.0, |a, b| a + b)
        })
    };
    let thrust = |role: &str| {
        sum(entries.iter().map(|e| {
            let c = ctx.components.get(&e.item.to_lowercase())?;
            if c.category != "thrusters" {
                return None;
            }
            let is_retro = c.sub_type.as_deref() == Some("Retro") || retro_port(&e.port);
            let is_main = c.ty.as_deref() == Some("MainThruster");
            let matches = match role {
                "Retro" => is_retro,
                "Main" => is_main && !is_retro,
                _ => !is_main && !is_retro,
            };
            if matches { c.stat("thrustCapacity") } else { None }
        }))
    };
    let armor_item = loadout
        .get("armor")
        .and_then(|a| a.get("item"))
        .and_then(Value::as_str)
        .map(str::to_lowercase);
    let armor_component = armor_item.as_ref().and_then(|k| ctx.components.get(k));

    let totals = obj(vec![
        (
            "weaponDps",
            sum(list("weapons")
                .iter()
                .map(|w| times(f(w, "dps"), f(w, "count"))))
            .map(number),
        ),
        ("weapons", count_of("weapons").map(number)),
        ("missiles", count_of("missiles").map(number)),
        (
            "missileDamage",
            sum(list("missiles")
                .iter()
                .map(|m| times(f(m, "damage"), f(m, "count"))))
            .map(number),
        ),
        (
            "shieldHealth",
            stat("shields", "MaxShieldHealth").map(number),
        ),
        ("shieldRegen", stat("shields", "MaxShieldRegen").map(number)),
        (
            "armorHealth",
            armor_component.and_then(|c| c.health).map(number),
        ),
        (
            "powerOutput",
            stat("powerPlants", "powerOutput").map(number),
        ),
        ("fuel", stat("fuelTanks", "standardCargoUnits").map(number)),
        (
            "quantumFuel",
            stat("quantumFuelTanks", "standardCargoUnits").map(number),
        ),
        ("cargoScu", stat("cargoGrids", "scu").map(number)),
        ("thrustMain", thrust("Main").map(number)),
        ("thrustRetro", thrust("Retro").map(number)),
        ("thrustManeuver", thrust("Maneuver").map(number)),
        (
            "countermeasures",
            (!countermeasures.is_empty()).then(|| Value::Object(countermeasures.clone())),
        ),
        ("bombs", count_of("bombs").map(number)),
        (
            "bombDamage",
            sum(list("bombs")
                .iter()
                .map(|m| times(f(m, "damage"), f(m, "count"))))
            .map(number),
        ),
        ("salvage", {
            let row = obj(vec![
                ("heads", count_of("salvageHeads").map(number)),
                ("modifiers", count_of("salvageModifiers").map(number)),
                (
                    "storageScu",
                    sum(list("salvageStorage")
                        .iter()
                        .map(|s| times(f(s, "scu"), f(s, "count"))))
                    .map(number),
                ),
            ]);
            (!row.is_empty()).then_some(Value::Object(row))
        }),
        ("byStateEstimate", by_state(&powered).map(Value::Object)),
    ]);

    let mut fire_parts: Vec<String> = Vec::new();
    for slot in FIRE_SLOTS {
        for i in list(slot) {
            let Some(item) = i.get("item").and_then(Value::as_str) else {
                continue;
            };
            if ctx.fire_parts.contains(&item.to_lowercase())
                && !fire_parts.iter().any(|p| p == item)
            {
                fire_parts.push(item.to_string());
            }
        }
    }

    let armor = if armor_item.is_some() {
        parse_armor(armor_component)
    } else {
        None
    };
    Resolved {
        loadout: Some(loadout),
        totals: Some(totals),
        flight,
        armor,
        fire_parts: (!fire_parts.is_empty()).then_some(fire_parts),
    }
}

/// `parseVehicle`; the loadout is resolved only with a context.
pub fn parse_vehicle(
    record: &GameRecord,
    localization: &Localization,
    ctx: Option<&VehicleContext<'_>>,
) -> Option<Vehicle> {
    let params = component(record, "VehicleComponentParams")?;
    let raw_name = str_(params.get("vehicleName"));
    let display_name = raw_name.and_then(|n| crate::localization::localize_str(localization, n));
    let description = localize(localization, params.get("vehicleDescription"));
    // Career and role resolve through the string table when present, and fall back to the
    // key's own tail when the table has no entry for them.
    let career = localize(localization, params.get("vehicleCareer"));
    let role = localize(localization, params.get("vehicleRole"));
    let resolved = ctx.map_or_else(Resolved::default, |c| parse_loadout(record, c));
    let placeholder = is_placeholder_name(display_name);
    let classify = |value: Option<&str>, key: &str| {
        if is_placeholder_name(value) {
            tidy_classifier(str_(params.get(key)))
        } else {
            value.map(str::to_string)
        }
    };
    let role = classify(role, "vehicleRole");
    let capabilities = derive_capabilities(
        role.as_deref(),
        resolved.loadout.as_ref(),
        resolved.totals.as_ref(),
    );

    Some(Vehicle {
        name: if placeholder {
            record.short_name.clone()
        } else {
            display_name.unwrap_or_default().to_string()
        },
        record_name: record.name.clone(),
        id: record.id.clone(),
        display_name: (!placeholder).then(|| display_name.unwrap_or_default().to_string()),
        description: (!is_placeholder_name(description))
            .then(|| description.unwrap_or_default().to_string()),
        loc: loc_refs(
            localization,
            params.get("vehicleName"),
            params.get("vehicleDescription"),
        ),
        manufacturer: referenced_name(params.get("manufacturer")).map(str::to_string),
        movement_class: str_(params.get("movementClass")).map(str::to_string),
        career: classify(career, "vehicleCareer"),
        role,
        crew_size: num(params.get("crewSize")),
        size: num(attach_def(record).get("Size")),
        is_gravlev: (params.get("isGravlevVehicle") == Some(&Value::Bool(true))).then_some(true),
        bounding_box: vec(params.get("maxBoundingBoxSize")),
        hull: parse_hull(record),
        cross_section: vec(component(record, "SSCSignatureSystemParams").and_then(|c| {
            at_map(c, "radarProperties.crossSectionParams.crossSection")
        })),
        loadout: resolved.loadout,
        totals: resolved.totals,
        flight: resolved.flight,
        armor: resolved.armor,
        power_pools: parse_power_pools(record),
        dynamic_power_pools: parse_dynamic_power_pools(record),
        fire_parts: resolved.fire_parts,
        capabilities,
        is_template: placeholder.then_some(true),
    })
}

/// `buildVehicleContext`: every ship item, component, gun and missile a loadout can name.
pub fn build_vehicle_context<'a>(
    records: &'a RecordSet,
    localization: &'a Localization,
    entities: &'a HashMap<String, &'a GameRecord>,
) -> VehicleContext<'a> {
    let items = index_ship_items(records);
    let containers = index_containers(records);
    let weapon_ctx = WeaponContext {
        localization,
        ammo: index_ammo(records),
        entities,
    };
    let mut components = HashMap::new();
    let mut weapons = HashMap::new();
    let mut missiles = HashMap::new();
    let mut bombs = HashMap::new();
    let mut states = HashMap::new();
    let mut fire_parts = HashSet::new();
    for (key, record) in &items {
        if let Some(c) = parse_component(record, localization, Some(&containers)) {
            components.insert(key.clone(), c);
            let s = resource_states(record);
            if !s.is_empty() {
                states.insert(key.clone(), s);
            }
        }
        if parse_fire_item(record, localization)
            .is_some_and(|f| f.health.is_some_and(|h| h != 0.0))
        {
            fire_parts.insert(key.clone());
        }
        if !record
            .record_path
            .to_lowercase()
            .contains("/scitem/ships/weapons/")
        {
            continue;
        }
        if let Some(m) = parse_missile(record, localization) {
            missiles.insert(key.clone(), m);
            continue;
        }
        if let Some(b) = parse_bomb(record, localization) {
            bombs.insert(key.clone(), b);
            continue;
        }
        if let Some(w) = parse_weapon(record, &weapon_ctx) {
            weapons.insert(key.clone(), w);
        }
    }
    VehicleContext {
        localization,
        items,
        components,
        weapons,
        missiles,
        bombs,
        states,
        fire_parts,
    }
}

/// Ship item records (under `/entities/scitem/ships/`) by lower-case short name.
pub fn index_ship_items(records: &RecordSet) -> HashMap<String, &GameRecord> {
    let mut items: HashMap<String, &GameRecord> = HashMap::new();
    for r in records.of_type("EntityClassDefinition") {
        if r.record_path
            .to_lowercase()
            .contains("/entities/scitem/ships/")
        {
            items.insert(r.short_name.to_lowercase(), r);
        }
    }
    items
}

/// Every `EntityClassDefinition` by lower-case short name (a later duplicate wins).
pub fn index_entities(records: &RecordSet) -> HashMap<String, &GameRecord> {
    records
        .of_type("EntityClassDefinition")
        .map(|r| (r.short_name.to_lowercase(), r))
        .collect()
}

pub fn is_vehicle_path(path: &str) -> bool {
    // `/\/entities\/(spaceships|groundvehicles)\//i`
    let lower = path.to_lowercase();
    lower.contains("/entities/spaceships/") || lower.contains("/entities/groundvehicles/")
}

/// `Object.fromEntries(Object.entries(counts).sort((a, b) => b[1] - a[1]))`.
fn by_count_desc(counts: &Map<String, Value>) -> Map<String, Value> {
    let mut entries: Vec<(&String, f64)> = js_key_order(counts)
        .into_iter()
        .map(|k| (k, counts[k].as_f64().unwrap_or(0.0)))
        .collect();
    entries.sort_by(|a, b| {
        (b.1 - a.1)
            .partial_cmp(&0.0)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    entries
        .into_iter()
        .map(|(k, n)| (k.clone(), number(n)))
        .collect()
}

fn bump(counts: &mut Map<String, Value>, key: &str) {
    let n = counts.get(key).and_then(Value::as_f64).unwrap_or(0.0) + 1.0;
    counts.insert(key.to_string(), number(n));
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Summary {
    vehicles: usize,
    named: usize,
    templates: usize,
    with_loadout: usize,
}

impl Domain for Ships {
    fn name(&self) -> &'static str {
        "game-ships"
    }
    fn schema_version(&self) -> u32 {
        1
    }

    fn description(&self) -> &'static str {
        "Ships and ground vehicles with manufacturer, role, crew, dimensions, hull resistances, stock loadout and its totals."
    }

    fn filters(&self) -> &'static [&'static str] {
        &[
            "**/entities/spaceships/**",
            "**/entities/groundvehicles/**",
            "**/entities/scitem/ships/**",
            "**/ammoparams/**",
            "**/inventorycontainers/**",
            crate::domains::manufacturers::MANUFACTURER_FILTER,
        ]
    }

    fn needs_localization(&self) -> bool {
        true
    }

    fn parse(&self, ctx: &ParseContext<'_>) -> Result<Map<String, Value>, DomainError> {
        let entities = index_entities(ctx.records);
        let context = build_vehicle_context(ctx.records, ctx.localization, &entities);
        let mut vehicles: Vec<Vehicle> = ctx
            .records
            .iter()
            .filter(|r| is_vehicle_path(&r.record_path))
            .filter_map(|r| parse_vehicle(r, ctx.localization, Some(&context)))
            .collect();
        vehicles.sort_by(|a, b| locale_compare(&a.name, &b.name));

        let named: Vec<&Vehicle> = vehicles.iter().filter(|v| v.is_template.is_none()).collect();
        let mut by_manufacturer = Map::new();
        let mut by_role = Map::new();
        for v in &named {
            if let Some(m) = v.manufacturer.as_deref().filter(|m| !m.is_empty()) {
                bump(&mut by_manufacturer, m);
            }
            if let Some(r) = v.role.as_deref().filter(|r| !r.is_empty()) {
                bump(&mut by_role, r);
            }
        }

        let summary = Summary {
            vehicles: vehicles.len(),
            named: named.len(),
            templates: vehicles.len() - named.len(),
            with_loadout: vehicles.iter().filter(|v| v.loadout.is_some()).count(),
        };
        let mut body = Map::new();
        body.insert("_assumptions".into(), serde_json::json!(ASSUMPTIONS));
        field(&mut body, "vehicles", &vehicles)?;
        let makers = crate::domains::manufacturers::names_by_code(ctx.records, ctx.localization);
        crate::domains::manufacturers::add_manufacturer_names(body.get_mut("vehicles"), &makers);
        body.insert(
            "byManufacturer".into(),
            Value::Object(by_count_desc(&by_manufacturer)),
        );
        body.insert("byRole".into(), Value::Object(by_count_desc(&by_role)));
        field(&mut body, "summary", &summary)?;
        Ok(body)
    }
}

#[cfg(test)]
mod tests {
    // Cases from sc-gamedata src/domains/ships.test.ts.
    use super::*;
    use crate::localization::parse_global_ini;
    use crate::records::normalize_record;
    use serde_json::json;

    fn localization() -> Localization {
        parse_global_ini(
            "vehicle_NameAEGS_Gladius=Aegis Gladius\nitem_NameAllStop=AllStop\nitem_NameRegulus=Regulus\nitem_NamePanther=CF-337 Panther Repeater\nitem_NameArrester=Arrester III Missile",
        )
    }

    fn record(short: &str, path: &str, components: Value) -> GameRecord {
        normalize_record(
            json!({"_RecordName_": format!("EntityClassDefinition.{short}"), "_RecordId_": format!("id-{short}"),
                "_RecordValue_": {"_Type_": "EntityClassDefinition", "Components": components}}),
            path,
        )
        .unwrap()
    }

    fn ship_item(short: &str, dir: &str, ty: &str, name_key: &str, extra: Vec<Value>) -> GameRecord {
        let mut list = vec![json!({"_Type_": "SAttachableComponentParams", "AttachDef": {"Type": ty,
            "SubType": "UNDEFINED", "Size": 1, "Localization": {"Name": name_key}}})];
        list.extend(extra);
        record(
            short,
            &format!(
                "libs/foundry/records/entities/scitem/ships/{dir}/{}",
                short.to_lowercase()
            ),
            Value::Array(list),
        )
    }

    fn entry(port: &str, item: &str, children: Option<Vec<Value>>) -> Value {
        let file = item.ends_with(".json");
        json!({"itemPortName": port,
            "entityClassName": if file { "" } else { item },
            "entityClassReference": if file { json!(format!("file://./x/{item}")) } else { Value::Null },
            "loadout": children.map_or(Value::Null, |c| json!({"entries": c}))})
    }

    fn gladius() -> GameRecord {
        record(
            "AEGS_Gladius",
            "libs/foundry/records/entities/spaceships/aegs_gladius",
            json!([
                {"_Type_": "VehicleComponentParams", "vehicleName": "@vehicle_NameAEGS_Gladius", "crewSize": 1,
                    "vehicleHullDamageNormalizationValue": 2134, "manufacturer": "file://./x/scitemmanufacturer.aegs.json"},
                {"_Type_": "SAttachableComponentParams", "AttachDef": {"Type": "NOITEM_Vehicle", "Size": 2}},
                {"_Type_": "SHealthComponentParams", "Health": 1, "DamageResistances": {
                    "PhysicalResistance": {"Multiplier": 1}, "EnergyResistance": {"Multiplier": 0.8}}},
                {"_Type_": "SSCSignatureSystemParams", "radarProperties": {"crossSectionParams": {
                    "crossSection": {"_Type_": "Vec3", "x": 6923, "y": 1731, "z": 8654}}}},
                {"_Type_": "SEntityComponentDefaultLoadoutParams", "loadout": {"entries": [
                    entry("hardpoint_seat_pilot", "AEGS_Gladius_SCItem_Seat_Pilot", None),
                    entry("hardpoint_power_plant", "POWR_AEGS_S01_Regulus_SCItem", None),
                    entry("hardpoint_shield_generator_left", "SHLD_GODI_S01_AllStop_SCItem", None),
                    entry("hardpoint_shield_generator_right", "SHLD_GODI_S01_AllStop_SCItem", None),
                    entry("hardpoint_gun_nose", "mount_gimbal_s3.json", Some(vec![entry("hardpoint_class_2", "klwe_laserrepeater_s3.json", None)])),
                    entry("hardpoint_gun_left_wing", "mount_gimbal_s3.json", Some(vec![entry("hardpoint_class_2", "klwe_laserrepeater_s3.json", None)])),
                    // Fixed gun with the stale self-reference AI variants carry.
                    entry("hardpoint_gun_right_wing", "KLWE_LaserRepeater_S3", Some(vec![entry("hardpoint_class_2", "klwe_laserrepeater_s3.json", None)])),
                    entry("hardpoint_missilerack_left", "MRCK_S03_BEHR_Single_S03", Some(vec![entry("missile_01_attach", "MISL_S03_CS_FSKI_Arrester", None)])),
                    entry("hardpoint_thruster_main_left", "AEGS_Gladius_Thruster_Main", None),
                    entry("hardpoint_thruster_retro_left", "AEGS_Gladius_Thruster_Retro", None),
                    entry("hardpoint_unknown", "Something_Not_Extracted", None),
                    entry("hardpoint_paint", "", None)
                ]}}
            ]),
        )
    }

    fn records() -> RecordSet {
        let ammo = normalize_record(
            json!({"_RecordName_": "AmmoParams.klwe_laserrepeater_s3_ammo", "_RecordId_": "ammo",
                "_RecordValue_": {"_Type_": "AmmoParams", "speed": 1000, "lifetime": 2,
                    "projectileParams": {"damage": {"_Type_": "DamageInfo", "DamageEnergy": 50}}}}),
            "libs/foundry/records/ammoparams/vehicle/ammoparams.klwe_laserrepeater_s3_ammo",
        )
        .unwrap();
        RecordSet::new(vec![
            gladius(),
            ship_item("KLWE_LaserRepeater_S3", "weapons", "WeaponGun", "@item_NamePanther", vec![
                json!({"_Type_": "SCItemWeaponComponentParams", "fireActions": [{"_Type_": "SWeaponActionFireSingleParams",
                    "name": "Fire", "fireRate": 600, "launchParams": {"pelletCount": 1}}]}),
                json!({"_Type_": "SAmmoContainerComponentParams", "maxAmmoCount": 0,
                    "ammoParamsRecord": "file://./x/ammoparams.klwe_laserrepeater_s3_ammo.json"}),
            ]),
            ship_item("Mount_Gimbal_S3", "weapon_mounts/gimbal", "Turret", "@item_NameMount", vec![]),
            ship_item("SHLD_GODI_S01_AllStop_SCItem", "shieldgenerator", "Shield", "@item_NameAllStop", vec![
                json!({"_Type_": "SCItemShieldGeneratorParams", "MaxShieldHealth": 3168, "MaxShieldRegen": 602}),
            ]),
            ship_item("POWR_AEGS_S01_Regulus_SCItem", "powerplant", "PowerPlant", "@item_NameRegulus", vec![
                json!({"_Type_": "ItemResourceComponentParams", "states": [{"name": "Online", "deltas": [
                    {"generation": {"resource": "Power", "resourceAmountPerSecond": {"units": 16}}}]}]}),
            ]),
            ship_item("MRCK_S03_BEHR_Single_S03", "missile_racks", "MissileLauncher", "@item_NameRack", vec![]),
            ship_item("MISL_S03_CS_FSKI_Arrester", "weapons", "Missile", "@item_NameArrester", vec![
                json!({"_Type_": "SCItemMissileParams", "explosionParams": {"damage": {"_Type_": "DamageInfo", "DamagePhysical": 3200}}}),
            ]),
            ship_item("AEGS_Gladius_Thruster_Main", "thrusters", "MainThruster", "@x", vec![
                json!({"_Type_": "SCItemThrusterParams", "thrustCapacity": 2324002}),
            ]),
            ship_item("AEGS_Gladius_Thruster_Retro", "thrusters", "ManneuverThruster", "@x", vec![
                json!({"_Type_": "SCItemThrusterParams", "thrustCapacity": 500000}),
            ]),
            ship_item("AEGS_Gladius_SCItem_Seat_Pilot", "seat", "Seat", "@x", vec![]),
            ammo,
        ])
    }

    #[test]
    fn loadout_entries_flatten_the_tree_and_keep_parents() {
        let g = gladius();
        let entries = loadout_entries(&g);
        let panther: Vec<&Entry> = entries.iter().filter(|e| e.item == "klwe_laserrepeater_s3").collect();
        assert_eq!(panther.len(), 3);
        assert_eq!(entries[panther[0].parent.unwrap()].item, "mount_gimbal_s3");
        assert!(!entries.iter().any(|e| e.port == "hardpoint_paint"));
    }

    #[test]
    fn count_duplicates_collapses_identical_items() {
        let out = count_duplicates(&[
            json!({"item": "A"}),
            json!({"item": "B"}),
            json!({"item": "A"}),
            json!({"item": "A", "size": 2}),
        ]);
        assert_eq!(
            out,
            vec![json!({"item": "A", "count": 2.0}), json!({"item": "B"}), json!({"item": "A", "size": 2})]
        );
    }

    #[test]
    fn vehicle_with_a_loadout() {
        let set = records();
        let loc = localization();
        let entities = index_entities(&set);
        let ctx = build_vehicle_context(&set, &loc, &entities);
        let g = gladius();
        let v = parse_vehicle(&g, &loc, Some(&ctx)).unwrap();
        assert_eq!(v.size, Some(2.0));
        assert_eq!(
            serde_json::to_value(&v.hull).unwrap(),
            json!({"damageNormalization": 2134.0, "resistances": {"energy": 0.8}})
        );
        assert_eq!(
            v.cross_section.clone().map(Value::Object),
            Some(json!({"x": 6923, "y": 1731, "z": 8654}))
        );
        let l = Value::Object(v.loadout.clone().unwrap());
        assert_eq!(
            l["powerPlants"],
            json!([{"item": "POWR_AEGS_S01_Regulus_SCItem", "name": "Regulus", "size": 1.0}])
        );
        assert_eq!(
            l["shields"],
            json!([{"item": "SHLD_GODI_S01_AllStop_SCItem", "name": "AllStop", "size": 1.0, "count": 2.0}])
        );
        assert_eq!(
            l["weapons"],
            json!([
                {"item": "KLWE_LaserRepeater_S3", "name": "CF-337 Panther Repeater", "size": 1.0, "type": "WeaponGun",
                    "mount": "mount_gimbal_s3", "dps": 500.0, "count": 2.0},
                {"item": "KLWE_LaserRepeater_S3", "name": "CF-337 Panther Repeater", "size": 1.0, "type": "WeaponGun", "dps": 500.0}
            ])
        );
        assert_eq!(
            l["missiles"],
            json!([{"item": "MISL_S03_CS_FSKI_Arrester", "name": "Arrester III Missile", "size": 1.0,
                "rack": "MRCK_S03_BEHR_Single_S03", "damage": 3200.0}])
        );
        assert_eq!(l["thrusters"], json!({"main": 1, "retro": 1}));
        assert_eq!(l["missileRacks"], 1);
        let text = l.to_string();
        assert!(!text.contains("Seat_Pilot") && !text.contains("Something_Not_Extracted"));
        assert_eq!(
            Value::Object(v.totals.clone().unwrap()),
            json!({"weaponDps": 1500.0, "weapons": 3.0, "missiles": 1.0, "missileDamage": 3200.0,
                "shieldHealth": 6336.0, "shieldRegen": 1204.0, "powerOutput": 16.0,
                "thrustMain": 2324002.0, "thrustRetro": 500000.0})
        );
        let bare = parse_vehicle(&g, &loc, None).unwrap();
        assert!(bare.loadout.is_none() && bare.totals.is_none());
        assert_eq!(bare.name, "Aegis Gladius");
        assert_eq!(bare.manufacturer.as_deref(), Some("aegs"));
    }

    #[test]
    fn flight_from_ifcs() {
        let controller = |ifcs: Value| {
            let mut p = ifcs;
            p["_Type_"] = json!("IFCSParams");
            record(
                "Controller_Flight_ORIG_100i",
                "libs/foundry/records/entities/scitem/ships/controller/controller_flight_orig_100i",
                json!([{"_Type_": "SAttachableComponentParams", "AttachDef": {"Type": "FlightController"}}, p]),
            )
        };
        let f = parse_flight(&controller(json!({"scmSpeed": 223, "maxSpeed": 1230, "boostSpeedForward": 610,
            "boostSpeedBackward": 290, "maxAngularVelocity": {"x": 60, "y": 155, "z": 52},
            "speedProfile": {"angularVelocity": {"x": 60, "y": 200, "z": 60}},
            "afterburnerNew": {"capacitorMax": 20, "capacitorRegenPerSec": 0.75, "capacitorRegenDelayAfterUse": 0.2},
            "ifcsCoreParams": {"stunThresholdGs": 50}})))
        .unwrap();
        assert_eq!(
            serde_json::to_value(&f).unwrap(),
            json!({"controller": "Controller_Flight_ORIG_100i", "scmSpeed": 223.0, "maxSpeed": 1230.0,
                "boostSpeedForward": 610.0, "boostSpeedBackward": 290.0, "pitch": 60.0, "yaw": 52.0, "roll": 155.0,
                "boostCapacitor": {"max": 20.0, "regenPerSecond": 0.75, "regenDelay": 0.2}, "stunThresholdGs": 50.0})
        );
        assert!(parse_flight(&record("X", "x", json!([]))).is_none());
    }

    #[test]
    fn power_pools_keep_positive_pools() {
        let ship = record(
            "S",
            "libs/foundry/records/entities/spaceships/s",
            json!([{"_Type_": "SItemPortContainerComponentParams", "resourceNetworkPowerPools": {
                "itemPools": [{"itemType": "WeaponGun", "poolSize": 4}, {"itemType": "Shield", "maxItemCount": 2}, null]}}]),
        );
        assert_eq!(
            parse_power_pools(&ship).map(Value::Object),
            Some(json!({"WeaponGun": 4.0}))
        );
        assert_eq!(
            parse_dynamic_power_pools(&ship).map(Value::Object),
            Some(json!({"Shield": 2.0}))
        );
    }

    fn hyun() -> GameRecord {
        record(
            "GAMA_Hyun",
            "libs/foundry/records/entities/spaceships/gama_hyun",
            json!([
                {"_Type_": "VehicleComponentParams", "vehicleName": "@vehicle_NameHyun", "vehicleRole": "Generalist"},
                {"_Type_": "SEntityComponentDefaultLoadoutParams", "loadout": {"entries": [
                    entry("hardpoint_power_plant", "POWR_Test", None),
                    entry("hardpoint_salvage_arm_left", "Hyun_Salvage_Arm", Some(vec![
                        entry("hardpoint_salvage_head_left", "salvage_head_standard.json", Some(vec![
                            entry("hardpoint_salvage_subItem01", "Salvage_Modifier_Scraper_Medium", None)]))])),
                    entry("hardpoint_salvage_arm_right", "Hyun_Salvage_Arm", Some(vec![
                        entry("hardpoint_salvage_head_right", "salvage_head_standard.json", Some(vec![
                            entry("hardpoint_salvage_subItem01", "Salvage_Modifier_Scraper_Medium", None)]))])),
                    entry("hardpoint_internal_storage", "salvageinternalstorage_gama_hyun.json", None),
                    entry("hardpoint_bomb_system", "BMBRCK_Test", Some(vec![
                        entry("missile_01_attach", "bomb_s05_fski.json", None),
                        entry("missile_02_attach", "bomb_s05_fski.json", None)]))
                ]}}
            ]),
        )
    }

    fn hyun_records() -> RecordSet {
        let state = |name: &str, power: f64, em: f64| json!({"name": name, "deltas": [
            {"consumption": {"resource": "Power", "resourceAmountPerSecond": {"units": power}}},
            {"consumption": {"resource": "Coolant", "resourceAmountPerSecond": {"units": 1}}}],
            "signatureParams": {"EMSignature": {"nominalSignature": em}, "IRSignature": {"nominalSignature": 10}}});
        RecordSet::new(vec![
            hyun(),
            ship_item("POWR_Test", "powerplant", "PowerPlant", "@x", vec![
                json!({"_Type_": "ItemResourceComponentParams", "states": [state("Online", 2.0, 500.0), state("Travelling", 5.0, 500.0), state("Boost", 2.0, 500.0)]}),
                json!({"_Type_": "SHealthComponentParams", "Health": 100, "DeathExplosionParams": {"damage": {"_Type_": "DamageInfo", "DamageThermal": 5}}}),
            ]),
            ship_item("Hyun_Salvage_Arm", "utility/salvage", "UtilityTurret", "@x", vec![]),
            ship_item("Salvage_Head_Standard", "utility/salvage/salvagehead", "SalvageHead", "@item_NameBaler", vec![]),
            ship_item("Salvage_Modifier_Scraper_Medium", "utility/salvage/salvagemodifiers", "SalvageModifier", "@x", vec![]),
            ship_item("SalvageInternalStorage_GAMA_Hyun", "salvagemunching", "SalvageInternalStorage", "@x", vec![
                json!({"_Type_": "ResourceContainer", "capacity": {"_Type_": "SStandardCargoUnit", "standardCargoUnits": 40}}),
            ]),
            ship_item("BMBRCK_Test", "missile_racks", "BombLauncher", "@x", vec![]),
            ship_item("BOMB_S05_FSKI", "weapons/missiles", "Bomb", "@item_NameBomb", vec![
                json!({"_Type_": "SCItemBombParams", "explosionParams": {"damage": {"_Type_": "DamageInfo", "DamagePhysical": 22346, "DamageEnergy": 24356}}}),
                json!({"_Type_": "SHealthComponentParams", "Health": 10}),
            ]),
        ])
    }

    #[test]
    fn salvage_bombs_power_states_and_fire_parts() {
        let set = hyun_records();
        let loc = parse_global_ini("item_NameBaler=Baler Salvage Head\nitem_NameBomb=Stormburst Bomb");
        let entities = index_entities(&set);
        let ctx = build_vehicle_context(&set, &loc, &entities);
        let v = parse_vehicle(&hyun(), &loc, Some(&ctx)).unwrap();
        let l = Value::Object(v.loadout.clone().unwrap());
        assert_eq!(
            l["salvageHeads"],
            json!([{"item": "Salvage_Head_Standard", "name": "Baler Salvage Head", "size": 1.0,
                "mount": "Hyun_Salvage_Arm", "count": 2.0}])
        );
        assert_eq!(
            l["salvageModifiers"],
            json!([{"item": "Salvage_Modifier_Scraper_Medium", "size": 1.0, "head": "salvage_head_standard", "count": 2.0}])
        );
        assert_eq!(
            l["salvageStorage"],
            json!([{"item": "SalvageInternalStorage_GAMA_Hyun", "size": 1.0, "scu": 40.0}])
        );
        assert_eq!(
            l["bombs"],
            json!([{"item": "BOMB_S05_FSKI", "name": "Stormburst Bomb", "size": 1.0, "rack": "BMBRCK_Test",
                "damage": 46702.0, "count": 2.0}])
        );
        assert!(l.get("missiles").is_none() && l.get("weapons").is_none());
        assert_eq!(l["missileRacks"], 1);
        let t = Value::Object(v.totals.clone().unwrap());
        assert_eq!(t["bombs"], 2.0);
        assert_eq!(t["bombDamage"], 93404.0);
        assert_eq!(t["salvage"], json!({"heads": 2.0, "modifiers": 2.0, "storageScu": 40.0}));
        // Boost sums to Online's figures, so only Travelling is listed beside it.
        assert_eq!(
            t["byStateEstimate"],
            json!({"Online": {"powerDraw": 2.0, "coolantDraw": 1.0, "em": 500.0, "ir": 10.0},
                "Travelling": {"powerDraw": 5.0, "coolantDraw": 1.0, "em": 500.0, "ir": 10.0}})
        );
        assert_eq!(
            v.fire_parts,
            Some(vec!["POWR_Test".to_string(), "BOMB_S05_FSKI".to_string()])
        );
        assert!(v.capabilities.salvage, "a salvage head sets salvage whatever the role");
        assert!(carries_salvage_head(&hyun(), &index_ship_items(&set)));
        assert!(!carries_salvage_head(&gladius(), &index_ship_items(&records())));
    }

    #[test]
    fn classifiers_keep_the_meaningful_tail() {
        assert_eq!(tidy_classifier(Some("@vehicle_focus_combat")).as_deref(), Some("combat"));
        assert_eq!(tidy_classifier(Some("@Vehicle_Class_LightFighter")).as_deref(), Some("LightFighter"));
        assert_eq!(tidy_classifier(Some("@other")).as_deref(), Some("other"));
        assert_eq!(tidy_classifier(Some("Plain")).as_deref(), Some("Plain"));
        assert_eq!(tidy_classifier(Some("@")), None);
        assert_eq!(tidy_classifier(None), None);
    }

    #[test]
    fn capabilities_follow_derive_caps() {
        let loadout = json!({"weapons": [{"type": "WeaponMining"}, {"type": "TractorBeam"}],
            "radars": [{"item": "r"}], "quantumDrives": [], "turrets": 2, "missileRacks": 0,
            "cargoGrids": [{"item": "g"}]});
        let totals = json!({"cargoScu": 32.456});
        let caps = derive_capabilities(
            Some("Light Salvage / Mining"),
            loadout.as_object(),
            totals.as_object(),
        );
        assert_eq!(
            serde_json::to_value(&caps).unwrap(),
            json!({"mining": true, "salvage": true, "refuel": false, "tractor": true, "guns": true,
                "scan": true, "quantum": false, "qed": false, "emp": false, "missiles": false,
                "cargo": true, "cargo_scu": 32.46})
        );
        let none = serde_json::to_value(derive_capabilities(Some("Heavy Refueling"), None, None))
            .unwrap();
        assert_eq!(none["refuel"], json!(true));
        assert_eq!(none["cargo_scu"], Value::Null);
        assert_eq!(none["cargoKnown"], json!(false));
        // A grid without SCU still has cargo, of unknown size.
        let grid = json!({"cargoGrids": [{"item": "g"}]});
        let caps = derive_capabilities(None, grid.as_object(), None);
        assert!(caps.cargo && caps.cargo_scu.is_none() && caps.cargo_known.is_none());
    }

    #[test]
    fn vehicle_capabilities_from_its_loadout() {
        let set = records();
        let loc = localization();
        let entities = index_entities(&set);
        let ctx = build_vehicle_context(&set, &loc, &entities);
        let v = parse_vehicle(&gladius(), &loc, Some(&ctx)).unwrap();
        let caps = &v.capabilities;
        assert!(caps.guns && caps.missiles && !caps.mining && !caps.cargo);
        assert_eq!(caps.cargo_known, Some(false));
    }
}
