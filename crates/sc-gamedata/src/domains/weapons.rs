//! Combat baseline: every weapon joined to its ammunition, with the derived numbers a
//! loadout tool shows (DPS, alpha, rate, range, spread, power, EM, ammo, HP), plus missiles
//! (port of `sc-gamedata/src/domains/weapons.ts`).
//!
//! - Damage is on the ammo (`AmmoParams.projectileParams.damage`), never the weapon. A ship
//!   weapon embeds an `SAmmoContainerComponentParams` whose `ammoParamsRecord` points at it;
//!   an FPS weapon's default loadout names a magazine entity, which carries the container
//!   (weapon -> magazine -> ammo).
//! - Fire rate lives on the fire action. Ship guns often wrap it in an
//!   `SWeaponActionSequenceParams` whose entries carry the cadence the game runs.
//! - Range is `speed * lifetime`; power and EM come from the resource network, HP from
//!   `SHealthComponentParams.Health`.

use crate::domain::{Domain, DomainError, ParseContext, field};
use crate::domains::ammo::{Ammo, DamageInfo, PenetrationRadius, Pierce, parse_ammo};
use crate::entity::{component, referenced_name};
use crate::helpers_items::item_common::{
    Distortion, Durability, attach_def_or_empty, display_name, distortion_of, durability_of,
    manufacturer_of, resource_profile, sub_type_of, volume_of,
};
use crate::helpers_items::{js_round, locale_compare, truthy};
use crate::localization::{LocRefs, Localization, loc_refs};
use crate::records::{GameRecord, at, num, str_};
use serde::Serialize;
use serde_json::{Map, Value};
use std::cmp::Ordering;
use std::collections::HashMap;

pub static DOMAIN: Weapons = Weapons;

pub struct Weapons;

/// `round(value, 2)`: `Math.round(value * 100) / 100`.
fn round(value: f64) -> f64 {
    js_round(value * 100.0) / 100.0
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Spread {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attack: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decay: Option<f64>,
}

/// One selectable fire mode. Field order is the union of every mode kind's TypeScript key
/// order; each kind only sets its own fields, so each prints in its own order.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FireMode {
    pub name: String,
    /// single, rapid, burst, charged, beam, sequence, other.
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fire_rate: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heat_per_shot: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pellets: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub damage_multiplier: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spread: Option<Spread>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shot_count: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sustained_fire_rate: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub charge_time: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub charged_damage_multiplier: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alpha: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_damage_range: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zero_damage_range: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dps: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WeaponAmmo {
    pub record_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub magazine: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lifetime: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub damage: Option<DamageInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub damage_per_shot: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub penetration: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub penetration_radius: Option<PenetrationRadius>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub falloff: Option<Map<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pierce: Option<Pierce>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capacity: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Capacitor {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shots: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub regen_per_second: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub regen_cooldown: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_per_shot: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requested_regen_per_second: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Burst {
    pub shots: f64,
    pub seconds: f64,
    pub damage: f64,
    /// capacitor, magazine or heat: what ends it.
    pub limit: &'static str,
}

/// One row of `shipWeapons` / `fpsWeapons`. Field order is output order.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Weapon {
    pub name: String,
    pub record_name: String,
    pub id: String,
    /// ship or fps.
    pub class: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_template: Option<bool>,
    /// `nameKey`, `descriptionKey`, `nameStatus`.
    #[serde(flatten)]
    pub loc: LocRefs,
    #[serde(skip_serializing_if = "Option::is_none", rename = "type")]
    pub ty: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grade: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manufacturer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub damage_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dps: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alpha: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fire_rate: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spread: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub penetration: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub power: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub em: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ir: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ammo_capacity: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub durability: Option<Durability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mass: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heat_per_shot: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overheat_temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cooling_per_second: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overheat_seconds: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capacitor: Option<Capacitor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub burst: Option<Burst>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sustained_dps: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub distortion: Option<Distortion>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ammo: Option<WeaponAmmo>,
    pub modes: Vec<FireMode>,
}

/// One row of `missiles`. Field order is output order.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Missile {
    pub name: String,
    pub record_name: String,
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_template: Option<bool>,
    /// `nameKey`, `descriptionKey`, `nameStatus`.
    #[serde(flatten)]
    pub loc: LocRefs,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manufacturer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub damage: Option<DamageInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub damage_total: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub damage_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tracking_signal: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lock_time: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lock_range_min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lock_range_max: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locking_angle: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub boost_duration: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_lifetime: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explosion_radius: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health: Option<f64>,
}

/// `SustainInput`.
#[derive(Debug, Clone, Default)]
pub struct SustainInput {
    pub alpha: Option<f64>,
    /// Trigger pulls per minute.
    pub fire_rate: Option<f64>,
    pub capacitor: Option<Capacitor>,
    /// Rounds carried, for ballistics.
    pub magazine: Option<f64>,
    pub heat_per_shot: Option<f64>,
    pub overheat_temperature: Option<f64>,
    pub cooling_per_second: Option<f64>,
    pub overheat_fix_time: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Sustained {
    pub burst: Option<Burst>,
    pub sustained_dps: Option<f64>,
    pub overheat_seconds: Option<f64>,
}

/// `sustain`: burst and sustained output from the pool that limits a gun (the capacitor
/// for energy guns; heat or the magazine for ballistics).
pub fn sustain(input: &SustainInput) -> Sustained {
    let (Some(alpha), Some(fire_rate)) = (input.alpha, input.fire_rate) else {
        return Sustained::default();
    };
    if !truthy(Some(alpha)) || !truthy(Some(fire_rate)) {
        return Sustained::default();
    }
    let per_second = fire_rate / 60.0;
    let shots_in = |seconds: f64| {
        let n = (seconds * per_second + 1e-9).floor();
        if truthy(Some(n)) { n } else { 1.0 }
    };

    let mut overheat_seconds = None;
    if let (Some(heat), Some(temp)) = (input.heat_per_shot, input.overheat_temperature)
        && truthy(Some(heat))
        && truthy(Some(temp))
    {
        let net = heat * per_second - input.cooling_per_second.unwrap_or(0.0);
        if net > 0.0 {
            overheat_seconds = Some(round(temp / net));
        }
    }

    let capacitor = input.capacitor.as_ref();
    if let Some(pool) = capacitor.and_then(|c| c.shots).filter(|p| truthy(Some(*p))) {
        let seconds = pool / per_second;
        let regen = capacitor.and_then(|c| c.regen_per_second);
        let burst = Burst {
            shots: pool,
            seconds: round(seconds),
            damage: round(pool * alpha),
            limit: "capacitor",
        };
        let cycle = regen.filter(|r| truthy(Some(*r))).map(|r| {
            seconds + capacitor.and_then(|c| c.regen_cooldown).unwrap_or(0.0) + pool / r
        });
        return Sustained {
            burst: Some(burst),
            sustained_dps: cycle
                .filter(|c| truthy(Some(*c)))
                .map(|c| round(pool * alpha / c)),
            overheat_seconds,
        };
    }

    let magazine = input.magazine.filter(|m| truthy(Some(*m)));
    let magazine_seconds = magazine.map(|m| m / per_second);
    if let Some(overheat) = overheat_seconds
        && magazine_seconds.is_none_or(|m| overheat < m)
    {
        let shots = shots_in(overheat);
        let cycle = overheat + input.overheat_fix_time.unwrap_or(0.0);
        return Sustained {
            burst: Some(Burst {
                shots,
                seconds: overheat,
                damage: round(shots * alpha),
                limit: "heat",
            }),
            sustained_dps: Some(round(shots * alpha / cycle)),
            overheat_seconds,
        };
    }
    if let (Some(m), Some(ms)) = (magazine, magazine_seconds) {
        return Sustained {
            burst: Some(Burst {
                shots: m,
                seconds: round(ms),
                damage: round(m * alpha),
                limit: "magazine",
            }),
            sustained_dps: Some(round(alpha * per_second)),
            overheat_seconds,
        };
    }
    Sustained {
        overheat_seconds,
        ..Sustained::default()
    }
}

fn parse_capacitor(params: &Map<String, Value>) -> Option<Capacitor> {
    let r = params
        .get("weaponRegenConsumerParams")
        .filter(|v| v.is_object() || v.is_array())?;
    let capacitor = Capacitor {
        shots: num(r.get("maxAmmoLoad")),
        regen_per_second: num(r.get("maxRegenPerSec")),
        regen_cooldown: num(r.get("regenerationCooldown")),
        cost_per_shot: num(r.get("regenerationCostPerBullet")),
        requested_regen_per_second: num(r.get("requestedRegenPerSec")),
    };
    truthy(capacitor.shots).then_some(capacitor)
}

/// `totalDamage`: the rounded sum of every type.
pub fn total_damage(damage: Option<&DamageInfo>) -> Option<f64> {
    damage.map(|d| round(d.sum()))
}

/// `dominantDamageType`: the largest type, the first one on a tie.
pub fn dominant_damage_type(damage: Option<&DamageInfo>) -> Option<String> {
    let mut best: Option<(&str, f64)> = None;
    for (ty, amount) in damage?.entries() {
        if best.is_none_or(|(_, b)| amount > b) {
            best = Some((ty, amount));
        }
    }
    best.map(|(t, _)| t.to_string())
}

fn parse_spread(value: Option<&Value>) -> Option<Spread> {
    let v = value.filter(|v| v.is_object() || v.is_array())?;
    let spread = Spread {
        min: num(v.get("min")),
        max: num(v.get("max")),
        attack: num(v.get("attack")),
        decay: num(v.get("decay")),
    };
    (spread != Spread::default()).then_some(spread)
}

/// `sequenceFireRate`: rounds per minute of a whole sequence (total shots over total time).
pub fn sequence_fire_rate(entries: &[&Value]) -> Option<f64> {
    let mut shots = 0.0;
    let mut seconds = 0.0;
    for entry in entries {
        let Some(delay) = num(entry.get("delay")) else {
            continue;
        };
        let repetitions = num(entry.get("repetitions")).unwrap_or(1.0);
        if delay <= 0.0 {
            continue;
        }
        let per_shot = if str_(entry.get("unit")) == Some("Seconds") {
            delay
        } else {
            60.0 / delay
        };
        shots += repetitions;
        seconds += per_shot * repetitions;
    }
    (shots > 0.0 && seconds > 0.0).then(|| round(60.0 * shots / seconds))
}

/// `launchStats`: what one projectile-launching action contributes.
fn launch_stats(action: &Value, mode: &mut FireMode) {
    let launch = action.get("launchParams").filter(|l| !l.is_null());
    mode.fire_rate = num(action.get("fireRate"));
    mode.heat_per_shot = num(action.get("heatPerShot"));
    mode.pellets = num(launch.and_then(|l| l.get("pelletCount")));
    mode.damage_multiplier = num(launch.and_then(|l| l.get("damageMultiplier")));
    mode.spread = parse_spread(launch.and_then(|l| l.get("spreadParams")));
}

/// `parseBeamDamage`: the non-zero types of a damage struct.
fn parse_beam_damage(value: Option<&Value>) -> Option<DamageInfo> {
    DamageInfo::from_fields(value, |amount| amount != 0.0)
}

fn is_object(v: &&Value) -> bool {
    v.is_object() || v.is_array()
}

pub fn parse_fire_mode(action: &Value, ammo: Option<&Ammo>) -> Option<FireMode> {
    let ty = str_(action.get("_Type_"))?;
    let name = str_(action.get("name")).map_or_else(
        || {
            let t = ty.strip_prefix("SWeaponActionFire").unwrap_or(ty);
            t.strip_suffix("Params").unwrap_or(t).to_string()
        },
        str::to_string,
    );
    let per_shot = ammo.and_then(|a| a.damage_total);
    let finish = |mut mode: FireMode, rate_for_dps: Option<f64>| -> FireMode {
        if let Some(per_shot) = per_shot {
            let alpha =
                per_shot * mode.pellets.unwrap_or(1.0) * mode.damage_multiplier.unwrap_or(1.0);
            mode.alpha = Some(round(alpha));
            if let Some(rate) = rate_for_dps.filter(|r| truthy(Some(*r))) {
                mode.dps = Some(round(alpha * rate / 60.0));
            }
        }
        mode
    };

    let mut mode = FireMode {
        name,
        kind: "other",
        ..FireMode::default()
    };
    match ty {
        "SWeaponActionFireSingleParams" | "SWeaponActionFireRapidParams" => {
            mode.kind = if ty.contains("Rapid") {
                "rapid"
            } else {
                "single"
            };
            launch_stats(action, &mut mode);
            let rate = mode.fire_rate;
            Some(finish(mode, rate))
        }
        "SWeaponActionFireBurstParams" => {
            mode.kind = "burst";
            launch_stats(action, &mut mode);
            let shot_count = num(action.get("shotCount"));
            let cooldown = num(action.get("cooldownTime")).unwrap_or(0.0);
            let mut sustained = None;
            if let (Some(rate), Some(count)) = (mode.fire_rate, shot_count)
                && truthy(Some(rate))
                && truthy(Some(count))
            {
                let burst_seconds = count * 60.0 / rate + cooldown;
                sustained = Some(round(60.0 * count / burst_seconds));
            }
            mode.shot_count = shot_count;
            mode.sustained_fire_rate = sustained;
            let rate = sustained.or(mode.fire_rate);
            Some(finish(mode, rate))
        }
        "SWeaponActionFireChargedParams" => {
            mode.kind = "charged";
            let empty = Value::Object(Map::new());
            let inner = action
                .get("weaponAction")
                .filter(|v| !v.is_null())
                .unwrap_or(&empty);
            launch_stats(inner, &mut mode);
            mode.charge_time = num(action.get("chargeTime"));
            mode.charged_damage_multiplier = num(at(action, "maxChargeModifier.damageMultiplier"));
            let rate = mode.fire_rate;
            Some(finish(mode, rate))
        }
        "SWeaponActionSequenceParams" => {
            mode.kind = "sequence";
            let entries: Vec<&Value> = action
                .get("sequenceEntries")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter(|e| e.is_object() || e.is_array())
                .collect();
            let first = entries
                .iter()
                .filter_map(|e| e.get("weaponAction"))
                .find(is_object);
            if let Some(first) = first {
                launch_stats(first, &mut mode);
            }
            mode.fire_rate = sequence_fire_rate(&entries).or(mode.fire_rate);
            // Sequences are often unnamed; the nested action usually says "Single".
            mode.name = str_(action.get("name"))
                .or_else(|| first.and_then(|f| str_(f.get("name"))))
                .unwrap_or("Sequence")
                .to_string();
            let rate = mode.fire_rate;
            Some(finish(mode, rate))
        }
        "SWeaponActionFireBeamParams" => {
            mode.kind = "beam";
            let damage = parse_beam_damage(action.get("damagePerSecond"));
            mode.heat_per_shot = num(action.get("heatPerSecond"));
            mode.full_damage_range = num(action.get("fullDamageRange"));
            mode.zero_damage_range = num(action.get("zeroDamageRange"));
            mode.dps = total_damage(damage.as_ref());
            Some(mode)
        }
        // Tractor, salvage, healing, mining and the like: present, but no damage figure.
        _ => Some(mode),
    }
}

/// `indexAmmo`: ammo by lower-cased short name (what a `file://.../x_ammo.json` yields).
pub fn index_ammo<'a>(records: impl Iterator<Item = &'a GameRecord>) -> HashMap<String, Ammo> {
    records
        .filter(|r| r.ty == "AmmoParams")
        .map(|r| (r.short_name.to_lowercase(), parse_ammo(r)))
        .collect()
}

fn ammo_from_container<'a>(
    container: Option<&Map<String, Value>>,
    ammo_index: &'a HashMap<String, Ammo>,
) -> (Option<&'a Ammo>, Option<f64>) {
    let Some(name) = referenced_name(container.and_then(|c| c.get("ammoParamsRecord"))) else {
        return (None, None);
    };
    (
        ammo_index.get(&name.to_lowercase()),
        num(container.and_then(|c| c.get("maxAmmoCount"))),
    )
}

/// `defaultMagazineName`: the magazine an FPS weapon ships with, from its default loadout.
pub fn default_magazine_name(record: &GameRecord) -> Option<String> {
    let loadout = component(record, "SEntityComponentDefaultLoadoutParams")?;
    let entries = loadout.get("loadout").and_then(|l| at(l, "entries"))?.as_array()?;
    for entry in entries.iter().filter(|e| e.is_object()) {
        if str_(entry.get("itemPortName")).is_some_and(|p| p.to_lowercase().contains("magazine")) {
            return str_(entry.get("entityClassName")).map(str::to_string);
        }
    }
    None
}

/// `WeaponContext`.
pub struct WeaponContext<'a> {
    pub localization: &'a Localization,
    pub ammo_index: HashMap<String, Ammo>,
    /// Entities by lower-cased short name, for resolving magazine loadouts.
    pub entities: HashMap<String, &'a GameRecord>,
}

impl<'a> WeaponContext<'a> {
    pub fn new(records: &'a [GameRecord], localization: &'a Localization) -> Self {
        let mut entities = HashMap::new();
        for r in records.iter().filter(|r| r.ty == "EntityClassDefinition") {
            entities.insert(r.short_name.to_lowercase(), r);
        }
        Self {
            localization,
            ammo_index: index_ammo(records.iter()),
            entities,
        }
    }
}

pub fn parse_weapon(record: &GameRecord, class: &'static str, ctx: &WeaponContext<'_>) -> Option<Weapon> {
    let params = component(record, "SCItemWeaponComponentParams")?;
    let def = attach_def_or_empty(record);
    let (localized, placeholder_name) = display_name(def, ctx.localization);
    // Store display props reuse a real gun's name but fire nothing.
    let placeholder = placeholder_name || record.short_name.to_lowercase().ends_with("_prop");

    // Ship guns carry their ammo container; FPS guns borrow the magazine's.
    let mut linked = ammo_from_container(
        component(record, "SAmmoContainerComponentParams"),
        &ctx.ammo_index,
    );
    let mut magazine = None;
    if linked.0.is_none() {
        magazine = default_magazine_name(record);
        let magazine_record = magazine
            .as_ref()
            .and_then(|m| ctx.entities.get(&m.to_lowercase()));
        if let Some(m) = magazine_record {
            linked = ammo_from_container(
                component(m, "SAmmoContainerComponentParams"),
                &ctx.ammo_index,
            );
        }
    }
    let (ammo, capacity) = linked;

    let modes: Vec<FireMode> = params
        .get("fireActions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|a| a.is_object() || a.is_array())
        .filter_map(|a| parse_fire_mode(a, ammo))
        .collect();
    let mut best: Option<&FireMode> = None;
    for mode in &modes {
        if let Some(dps) = mode.dps
            && best.and_then(|b| b.dps).unwrap_or(-1.0) < dps
        {
            best = Some(mode);
        }
    }
    let headline = best.or(modes.first());

    let resources = resource_profile(record);
    let heat = at_obj(params, "connectionParams.simplifiedHeatParams");
    let heat_num = |k: &str| num(heat.and_then(|h| h.get(k)));
    let capacitor = parse_capacitor(params);
    let rounds = capacity.filter(|c| *c > 0.0);
    let headline_rate = headline.and_then(|h| h.sustained_fire_rate.or(h.fire_rate));
    let sustained = sustain(&SustainInput {
        alpha: headline.and_then(|h| h.alpha),
        fire_rate: headline_rate,
        capacitor: capacitor.clone(),
        magazine: rounds,
        heat_per_shot: headline.and_then(|h| h.heat_per_shot),
        overheat_temperature: heat_num("overheatTemperature"),
        cooling_per_second: heat_num("coolingPerSecond"),
        overheat_fix_time: heat_num("overheatFixTime"),
    });
    let headline_spread = headline.and_then(|h| h.spread.as_ref());

    Some(Weapon {
        name: if placeholder {
            record.short_name.clone()
        } else {
            localized.unwrap_or_default().to_string()
        },
        record_name: record.name.clone(),
        id: record.id.clone(),
        class,
        is_template: placeholder.then_some(true),
        loc: loc_refs(
            ctx.localization,
            at(def, "Localization.Name"),
            at(def, "Localization.Description"),
        ),
        ty: str_(def.get("Type")).map(str::to_string),
        sub_type: sub_type_of(def),
        size: num(def.get("Size")),
        grade: num(def.get("Grade")),
        manufacturer: manufacturer_of(def),
        damage_type: dominant_damage_type(ammo.and_then(|a| a.damage.as_ref())),
        dps: headline.and_then(|h| h.dps),
        alpha: headline.and_then(|h| h.alpha),
        fire_rate: headline_rate,
        range: ammo
            .and_then(|a| a.range)
            .or_else(|| headline.and_then(|h| h.zero_damage_range)),
        speed: ammo.and_then(|a| a.speed),
        spread: headline_spread.and_then(|s| s.min.or(s.max)),
        penetration: ammo.and_then(|a| a.penetration),
        power: resources.power,
        em: resources.em,
        ir: resources.ir,
        ammo_capacity: rounds.or_else(|| capacitor.as_ref().and_then(|c| c.shots)),
        health: num(component(record, "SHealthComponentParams").and_then(|h| h.get("Health"))),
        durability: durability_of(record),
        mass: num(component(record, "SEntityPhysicsControllerParams")
            .and_then(|p| p.get("PhysType"))
            .and_then(|p| at(p, "Mass"))),
        heat_per_shot: headline.and_then(|h| h.heat_per_shot),
        overheat_temperature: heat_num("overheatTemperature"),
        cooling_per_second: heat_num("coolingPerSecond"),
        overheat_seconds: sustained.overheat_seconds,
        capacitor,
        burst: sustained.burst,
        sustained_dps: sustained.sustained_dps,
        volume: volume_of(record),
        distortion: distortion_of(record),
        ammo: ammo.map(|a| WeaponAmmo {
            record_name: a.record_name.clone(),
            magazine,
            speed: a.speed,
            lifetime: a.lifetime,
            range: a.range,
            damage: a.damage.clone(),
            damage_per_shot: a.damage_total,
            penetration: a.penetration,
            penetration_radius: a.penetration_radius.clone(),
            falloff: a.falloff.clone(),
            pierce: a.pierce.clone(),
            capacity: rounds,
        }),
        modes,
    })
}

/// `at` starting from a component map.
fn at_obj<'a>(map: &'a Map<String, Value>, dotted: &str) -> Option<&'a Value> {
    let (first, rest) = dotted.split_once('.').unwrap_or((dotted, ""));
    let v = map.get(first)?;
    if rest.is_empty() { Some(v) } else { at(v, rest) }
}

pub fn parse_missile(record: &GameRecord, localization: &Localization) -> Option<Missile> {
    let params = component(record, "SCItemMissileParams")?;
    let def = attach_def_or_empty(record);
    let (localized, placeholder) = display_name(def, localization);
    let damage = parse_beam_damage(at_obj(params, "explosionParams.damage"));
    let targeting = |k: &str| params.get("targetingParams").and_then(|t| t.get(k));
    let gcs = |k: &str| num(params.get("GCSParams").and_then(|t| t.get(k)));
    Some(Missile {
        name: if placeholder {
            record.short_name.clone()
        } else {
            localized.unwrap_or_default().to_string()
        },
        record_name: record.name.clone(),
        id: record.id.clone(),
        is_template: placeholder.then_some(true),
        loc: loc_refs(
            localization,
            at(def, "Localization.Name"),
            at(def, "Localization.Description"),
        ),
        size: num(def.get("Size")),
        manufacturer: manufacturer_of(def),
        damage_total: total_damage(damage.as_ref()),
        damage_type: dominant_damage_type(damage.as_ref()),
        damage,
        tracking_signal: str_(targeting("trackingSignalType")).map(str::to_string),
        lock_time: num(targeting("lockTime")),
        lock_range_min: num(targeting("lockRangeMin")),
        lock_range_max: num(targeting("lockRangeMax")),
        locking_angle: num(targeting("lockingAngle")),
        speed: gcs("linearSpeed"),
        boost_duration: gcs("boostPhaseDuration"),
        max_lifetime: num(params.get("maxLifetime")),
        explosion_radius: num(at_obj(params, "explosionParams.maxRadius")),
        health: num(component(record, "SHealthComponentParams").and_then(|h| h.get("Health"))),
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Summary {
    ship_weapons: usize,
    ship_weapons_with_dps: usize,
    fps_weapons: usize,
    fps_weapons_with_dps: usize,
    missiles: usize,
    ammo: usize,
}

/// `(a.size ?? 0) - (b.size ?? 0) || a.name.localeCompare(b.name)`.
fn by_size_then_name(a: (Option<f64>, &str), b: (Option<f64>, &str)) -> Ordering {
    let diff = a.0.unwrap_or(0.0) - b.0.unwrap_or(0.0);
    if diff > 0.0 {
        Ordering::Greater
    } else if diff < 0.0 {
        Ordering::Less
    } else {
        locale_compare(a.1, b.1)
    }
}

impl Domain for Weapons {
    fn name(&self) -> &'static str {
        "game-weapons"
    }
    fn schema_version(&self) -> u32 {
        1
    }

    fn description(&self) -> &'static str {
        "Combat baseline: ship and FPS weapons joined to their ammo (DPS, alpha, rate, range, power, EM, HP) plus missiles."
    }

    fn filters(&self) -> &'static [&'static str] {
        &[
            "**/entities/scitem/ships/weapons/**",
            "**/entities/scitem/weapons/**",
            "**/ammoparams/**",
        ]
    }

    fn needs_localization(&self) -> bool {
        true
    }

    fn parse(&self, ctx: &ParseContext<'_>) -> Result<Map<String, Value>, DomainError> {
        let context = WeaponContext::new(ctx.records.all(), ctx.localization);
        let mut ship_weapons = Vec::new();
        let mut fps_weapons = Vec::new();
        let mut missiles = Vec::new();
        for record in ctx.records.of_type("EntityClassDefinition") {
            let path = record.record_path.to_lowercase();
            if path.contains("/scitem/ships/weapons/") {
                if let Some(m) = parse_missile(record, ctx.localization) {
                    missiles.push(m);
                    continue;
                }
                if let Some(w) = parse_weapon(record, "ship", &context) {
                    ship_weapons.push(w);
                }
            } else if path.contains("/scitem/weapons/")
                && let Some(w) = parse_weapon(record, "fps", &context)
            {
                fps_weapons.push(w);
            }
        }
        let weapon_order =
            |a: &Weapon, b: &Weapon| by_size_then_name((a.size, &a.name), (b.size, &b.name));
        ship_weapons.sort_by(weapon_order);
        fps_weapons.sort_by(weapon_order);
        missiles.sort_by(|a, b| by_size_then_name((a.size, &a.name), (b.size, &b.name)));

        let with_dps = |list: &[Weapon]| list.iter().filter(|w| w.dps.is_some()).count();
        let summary = Summary {
            ship_weapons: ship_weapons.len(),
            ship_weapons_with_dps: with_dps(&ship_weapons),
            fps_weapons: fps_weapons.len(),
            fps_weapons_with_dps: with_dps(&fps_weapons),
            missiles: missiles.len(),
            ammo: context.ammo_index.len(),
        };
        let mut body = Map::new();
        field(&mut body, "shipWeapons", &ship_weapons)?;
        field(&mut body, "fpsWeapons", &fps_weapons)?;
        field(&mut body, "missiles", &missiles)?;
        field(&mut body, "summary", &summary)?;
        Ok(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::NoFiles;
    use crate::localization::parse_global_ini;
    use crate::records::{RecordSet, normalize_record};
    use serde_json::json;

    fn localization() -> Localization {
        parse_global_ini(
            "item_NameBEHR_BallisticGatling_S5=AD5B Ballistic Gatling\nitem_Namebehr_rifle_ballistic_01=P4-AR Rifle\nitem_NameTALN_Valkyrie=Valkyrie",
        )
    }

    fn entity(short: &str, path: &str, components: Value) -> GameRecord {
        normalize_record(
            json!({"_RecordName_": format!("EntityClassDefinition.{short}"), "_RecordId_": format!("id-{short}"),
                   "_RecordValue_": {"_Type_": "EntityClassDefinition", "Components": components}}),
            path,
        )
        .unwrap()
    }

    fn ammo_record(short: &str, physical: f64, speed: f64, lifetime: f64) -> GameRecord {
        normalize_record(
            json!({"_RecordName_": format!("AmmoParams.{short}"), "_RecordId_": format!("id-{short}"),
                   "_RecordValue_": {"_Type_": "AmmoParams", "speed": speed, "lifetime": lifetime,
                     "projectileParams": {"damage": {"_Type_": "DamageInfo", "DamagePhysical": physical, "DamageEnergy": 0},
                                          "penetrationParams": {"basePenetrationDistance": 1}}}}),
            &format!("libs/foundry/records/ammoparams/vehicle/{}", short.to_lowercase()),
        )
        .unwrap()
    }

    fn attach(name_key: &str, ty: &str, size: u32) -> Value {
        json!({"_Type_": "SAttachableComponentParams", "AttachDef": {
            "Type": ty, "SubType": "Gun", "Size": size, "Grade": 1,
            "Manufacturer": "file://./../../libs/foundry/records/scitemmanufacturer/behr.json",
            "Localization": {"Name": name_key}}})
    }

    fn resources(power: f64, em: f64) -> Value {
        json!({"_Type_": "ItemResourceComponentParams", "states": [{"name": "Online",
            "deltas": [{"consumption": {"resource": "Power", "resourceAmountPerSecond": {"standardResourceUnits": power}}}],
            "signatureParams": {"EMSignature": {"nominalSignature": em}, "IRSignature": {"nominalSignature": 0}}}]})
    }

    fn rapid(fire_rate: f64, pellets: u32) -> Value {
        json!({"_Type_": "SWeaponActionFireRapidParams", "name": "Rapid", "fireRate": fire_rate, "heatPerShot": 0.57,
               "launchParams": {"pelletCount": pellets, "damageMultiplier": 1,
                 "spreadParams": {"min": 0.792, "max": 0.792, "attack": 0.008, "decay": 0.01}}})
    }

    fn gatling() -> GameRecord {
        entity(
            "BEHR_BallisticGatling_S5",
            "libs/foundry/records/entities/scitem/ships/weapons/behr_ballisticgatling_s5",
            json!([
                attach("@item_NameBEHR_BallisticGatling_S5", "WeaponGun", 5),
                {"_Type_": "SAmmoContainerComponentParams", "maxAmmoCount": 5004,
                 "ammoParamsRecord": "file://./../../libs/foundry/records/ammoparams/vehicle/behr_ballisticgatling_s5_ammo.json"},
                {"_Type_": "SCItemWeaponComponentParams", "fireActions": [rapid(900.0, 1)]},
                resources(0.1, 36.0),
                {"_Type_": "SHealthComponentParams", "Health": 1650},
                {"_Type_": "SEntityPhysicsControllerParams", "PhysType": {"Mass": 938}}
            ]),
        )
    }

    fn weapon(record: &GameRecord, class: &'static str, records: Vec<GameRecord>) -> Weapon {
        let loc = localization();
        let ctx = WeaponContext::new(&records, &loc);
        parse_weapon(record, class, &ctx).unwrap()
    }

    fn ammo_of(total: f64) -> Ammo {
        Ammo {
            name: "x".into(),
            record_name: "x".into(),
            id: "x".into(),
            size: None,
            speed: None,
            lifetime: None,
            range: None,
            damage: None,
            damage_total: Some(total),
            penetration: None,
            penetration_radius: None,
            falloff: None,
            pierce: None,
        }
    }

    // Cases from sc-gamedata src/domains/weapons.test.ts.
    #[test]
    fn ship_weapon_reproduces_every_loadout_site_column() {
        let g = gatling();
        let w = weapon(
            &g,
            "ship",
            vec![g.clone(), ammo_record("BEHR_BallisticGatling_S5_AMMO", 102.5, 1251.0, 3.2)],
        );
        assert_eq!(w.name, "AD5B Ballistic Gatling");
        assert_eq!(w.manufacturer.as_deref(), Some("BEHR"));
        assert_eq!(w.damage_type.as_deref(), Some("physical"));
        assert_eq!(
            (w.dps, w.alpha, w.fire_rate, w.range, w.speed, w.spread),
            (Some(1537.5), Some(102.5), Some(900.0), Some(4003.0), Some(1251.0), Some(0.792))
        );
        assert_eq!(
            (w.penetration, w.power, w.em, w.ammo_capacity, w.health, w.mass),
            (Some(1.0), Some(0.1), Some(36.0), Some(5004.0), Some(1650.0), Some(938.0))
        );
        assert!(w.is_template.is_none());
        let ammo = w.ammo.unwrap();
        assert_eq!(ammo.record_name, "AmmoParams.BEHR_BallisticGatling_S5_AMMO");
        assert!(ammo.magazine.is_none());
    }

    #[test]
    fn weapon_without_ammo_has_no_dps() {
        let g = gatling();
        let w = weapon(&g, "ship", vec![g.clone()]);
        assert!(w.dps.is_none() && w.ammo.is_none());
        assert_eq!((w.modes[0].kind, w.modes[0].fire_rate), ("rapid", Some(900.0)));
    }

    #[test]
    fn scattergun_alpha_counts_pellets() {
        let scatter = entity(
            "Scatter",
            "libs/foundry/records/entities/scitem/ships/weapons/scatter",
            json!([attach("@none", "WeaponGun", 5),
                   {"_Type_": "SAmmoContainerComponentParams", "ammoParamsRecord": "file://x/scatter_ammo.json"},
                   {"_Type_": "SCItemWeaponComponentParams", "fireActions": [rapid(60.0, 8)]}]),
        );
        let w = weapon(
            &scatter,
            "ship",
            vec![scatter.clone(), ammo_record("scatter_ammo", 90.0, 1251.0, 3.2)],
        );
        assert_eq!((w.alpha, w.dps), (Some(720.0), Some(720.0)));
        assert_eq!(w.ammo.unwrap().damage_per_shot, Some(90.0));
        assert_eq!(w.is_template, Some(true));
    }

    #[test]
    fn sequence_cadence_comes_from_its_entries() {
        let action = json!({"_Type_": "SWeaponActionSequenceParams", "name": "", "sequenceEntries": [
            {"delay": 900, "unit": "RPM", "repetitions": 1, "weaponAction": {"_Type_": "SWeaponActionFireSingleParams", "name": "Single", "fireRate": 500, "launchParams": {"pelletCount": 1}}},
            {"delay": 900, "unit": "RPM", "repetitions": 1, "weaponAction": {"_Type_": "SWeaponActionFireSingleParams", "name": "Single", "fireRate": 500, "launchParams": {"pelletCount": 1}}}
        ]});
        let mode = parse_fire_mode(&action, Some(&ammo_of(10.0))).unwrap();
        assert_eq!(
            serde_json::to_value(&mode).unwrap(),
            json!({"name": "Single", "kind": "sequence", "fireRate": 900.0, "pellets": 1.0, "alpha": 10.0, "dps": 150.0})
        );
        let a = json!({"delay": 60, "unit": "RPM", "repetitions": 1});
        let b = json!({"delay": 2, "unit": "Seconds", "repetitions": 1});
        assert_eq!(sequence_fire_rate(&[&a, &b]), Some(40.0));
        assert_eq!(sequence_fire_rate(&[]), None);
    }

    #[test]
    fn burst_charged_beam_and_other_modes() {
        let ammo = ammo_of(10.0);
        let burst = parse_fire_mode(
            &json!({"_Type_": "SWeaponActionFireBurstParams", "name": "Burst", "fireRate": 900, "shotCount": 3, "cooldownTime": 0.3, "launchParams": {"pelletCount": 1}}),
            Some(&ammo),
        )
        .unwrap();
        assert_eq!(
            (burst.kind, burst.fire_rate, burst.sustained_fire_rate, burst.dps),
            ("burst", Some(900.0), Some(360.0), Some(60.0))
        );
        let charged = parse_fire_mode(
            &json!({"_Type_": "SWeaponActionFireChargedParams", "name": "Charge", "chargeTime": 10,
                    "maxChargeModifier": {"damageMultiplier": 2},
                    "weaponAction": {"_Type_": "SWeaponActionFireSingleParams", "fireRate": 6, "launchParams": {"pelletCount": 1}}}),
            Some(&ammo),
        )
        .unwrap();
        assert_eq!(
            (charged.kind, charged.fire_rate, charged.charge_time, charged.charged_damage_multiplier, charged.dps),
            ("charged", Some(6.0), Some(10.0), Some(2.0), Some(1.0))
        );
        let beam = parse_fire_mode(
            &json!({"_Type_": "SWeaponActionFireBeamParams", "name": "Beam",
                    "damagePerSecond": {"_Type_": "DamageInfo", "DamageEnergy": 15000},
                    "fullDamageRange": 4000, "zeroDamageRange": 5000}),
            None,
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(&beam).unwrap(),
            json!({"name": "Beam", "kind": "beam", "fullDamageRange": 4000.0, "zeroDamageRange": 5000.0, "dps": 15000.0})
        );
        assert_eq!(
            serde_json::to_value(
                parse_fire_mode(&json!({"_Type_": "SWeaponActionFireTractorBeamParams", "name": "Tractor"}), Some(&ammo)).unwrap()
            )
            .unwrap(),
            json!({"name": "Tractor", "kind": "other"})
        );
        assert_eq!(
            parse_fire_mode(&json!({"_Type_": "SWeaponActionFireSingleParams"}), None).unwrap().name,
            "Single"
        );
    }

    fn rifle() -> GameRecord {
        entity(
            "behr_rifle_ballistic_01",
            "libs/foundry/records/entities/scitem/weapons/fps_weapons/behr_rifle_ballistic_01",
            json!([
                attach("@item_Namebehr_rifle_ballistic_01", "WeaponPersonal", 2),
                {"_Type_": "SCItemWeaponComponentParams", "fireActions": [rapid(810.0, 1), null]},
                {"_Type_": "SEntityComponentDefaultLoadoutParams", "loadout": {"entries": [
                    {"itemPortName": "optics_attach", "entityClassName": ""},
                    {"itemPortName": "magazine_attach", "entityClassName": "behr_rifle_ballistic_01_mag"}]}}
            ]),
        )
    }

    #[test]
    fn fps_weapon_joins_weapon_magazine_ammo() {
        let r = rifle();
        assert_eq!(
            default_magazine_name(&r).as_deref(),
            Some("behr_rifle_ballistic_01_mag")
        );
        assert_eq!(default_magazine_name(&gatling()), None);
        let magazine = entity(
            "behr_rifle_ballistic_01_mag",
            "libs/foundry/records/entities/scitem/weapons/magazines/behr_rifle_ballistic_01_mag",
            json!([{"_Type_": "SAmmoContainerComponentParams", "maxAmmoCount": 40,
                    "ammoParamsRecord": "file://./../../libs/foundry/records/ammoparams/fps/behr_rifle_ballistic_01_ammo_5mm.json"}]),
        );
        let ammo = ammo_record("behr_rifle_ballistic_01_ammo_5mm", 12.0, 550.0, 2.0);
        let w = weapon(&r, "fps", vec![r.clone(), magazine, ammo]);
        assert_eq!(
            (w.class, w.dps, w.alpha, w.fire_rate, w.range, w.ammo_capacity),
            ("fps", Some(162.0), Some(12.0), Some(810.0), Some(1100.0), Some(40.0))
        );
        assert_eq!(
            w.ammo.unwrap().magazine.as_deref(),
            Some("behr_rifle_ballistic_01_mag")
        );
        assert_eq!(w.modes.len(), 1, "a null fire action is skipped");
    }

    #[test]
    fn store_props_are_templates() {
        let prop = entity(
            "behr_rifle_ballistic_01_store01_prop",
            "libs/foundry/records/entities/scitem/weapons/fps_weapons/x",
            json!([attach("@item_Namebehr_rifle_ballistic_01", "WeaponPersonal", 2),
                   {"_Type_": "SCItemWeaponComponentParams", "fireActions": []}]),
        );
        let w = weapon(&prop, "fps", vec![prop.clone()]);
        assert_eq!(w.is_template, Some(true));
        assert_eq!(w.name, "behr_rifle_ballistic_01_store01_prop");
    }

    fn valkyrie(full: bool) -> GameRecord {
        let params = if full {
            json!({"_Type_": "SCItemMissileParams", "maxLifetime": 60,
                   "explosionParams": {"maxRadius": 32, "damage": {"_Type_": "DamageInfo", "DamagePhysical": 12000}},
                   "targetingParams": {"trackingSignalType": "Infrared", "lockTime": 1, "lockRangeMin": 2000, "lockRangeMax": 30000, "lockingAngle": 20},
                   "GCSParams": {"linearSpeed": 560, "boostPhaseDuration": 2}})
        } else {
            json!({"_Type_": "SCItemMissileParams", "explosionParams": {"damage": {"_Type_": "DamageInfo", "DamagePhysical": 1}}})
        };
        entity(
            "TALN_Valkyrie",
            "libs/foundry/records/entities/scitem/ships/weapons/missiles/x",
            json!([attach("@item_NameTALN_Valkyrie", "Missile", 5), params,
                   {"_Type_": "SHealthComponentParams", "Health": 300}]),
        )
    }

    #[test]
    fn missiles_read_damage_lock_and_flight() {
        let m = parse_missile(&valkyrie(true), &localization()).unwrap();
        assert_eq!(m.name, "Valkyrie");
        assert_eq!(
            (m.damage_total, m.damage_type.as_deref(), m.tracking_signal.as_deref()),
            (Some(12000.0), Some("physical"), Some("Infrared"))
        );
        assert_eq!(
            (m.lock_time, m.lock_range_max, m.speed, m.explosion_radius, m.health),
            (Some(1.0), Some(30000.0), Some(560.0), Some(32.0), Some(300.0))
        );
        assert!(parse_missile(&gatling(), &localization()).is_none());
        let mixed = DamageInfo {
            physical: Some(100.0),
            energy: Some(200.0),
            ..DamageInfo::default()
        };
        assert_eq!(dominant_damage_type(Some(&mixed)).as_deref(), Some("energy"));
        assert_eq!(dominant_damage_type(None), None);
    }

    #[test]
    fn domain_splits_ship_guns_missiles_and_fps_guns() {
        let set = RecordSet::new(vec![
            gatling(),
            ammo_record("BEHR_BallisticGatling_S5_AMMO", 102.5, 1251.0, 3.2),
            valkyrie(false),
        ]);
        let loc = localization();
        let body = DOMAIN
            .parse(&ParseContext::new(&set, &loc, &NoFiles))
            .unwrap();
        assert_eq!(
            body["summary"],
            json!({"shipWeapons": 1, "shipWeaponsWithDps": 1, "fpsWeapons": 0,
                   "fpsWeaponsWithDps": 0, "missiles": 1, "ammo": 1})
        );
        assert_eq!(body["shipWeapons"][0]["dps"], json!(1537.5));
    }

    #[test]
    fn sustain_cases() {
        // CF-337 Panther: 75 shots, 750 RPM, 15 shots/s regen after 0.74 s.
        let r = sustain(&SustainInput {
            alpha: Some(43.65),
            fire_rate: Some(750.0),
            capacitor: Some(Capacitor {
                shots: Some(75.0),
                regen_per_second: Some(15.0),
                regen_cooldown: Some(0.74),
                ..Capacitor::default()
            }),
            ..SustainInput::default()
        });
        assert_eq!(
            r.burst,
            Some(Burst { shots: 75.0, seconds: 6.0, damage: 3273.75, limit: "capacitor" })
        );
        assert_eq!(r.sustained_dps, Some(278.85));

        let heat = sustain(&SustainInput {
            alpha: Some(100.0),
            fire_rate: Some(600.0),
            magazine: Some(1000.0),
            heat_per_shot: Some(2.0),
            overheat_temperature: Some(100.0),
            cooling_per_second: Some(10.0),
            overheat_fix_time: Some(2.0),
            ..SustainInput::default()
        });
        assert_eq!(heat.overheat_seconds, Some(10.0));
        assert_eq!(
            heat.burst,
            Some(Burst { shots: 100.0, seconds: 10.0, damage: 10000.0, limit: "heat" })
        );
        assert_eq!(heat.sustained_dps, Some(833.33));

        let magazine = sustain(&SustainInput {
            alpha: Some(950.0),
            fire_rate: Some(100.0),
            magazine: Some(369.0),
            heat_per_shot: Some(6.0),
            overheat_temperature: Some(100.0),
            cooling_per_second: Some(18.3),
            ..SustainInput::default()
        });
        assert_eq!(magazine.overheat_seconds, None);
        assert_eq!(magazine.burst.as_ref().map(|b| (b.shots, b.limit)), Some((369.0, "magazine")));
        assert_eq!(magazine.sustained_dps, Some(1583.33));

        assert_eq!(
            sustain(&SustainInput { fire_rate: Some(600.0), ..SustainInput::default() }),
            Sustained::default()
        );
    }
}
