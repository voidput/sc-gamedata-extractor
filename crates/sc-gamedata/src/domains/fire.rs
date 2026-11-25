//! Fire: the global fire-hazard rules and what makes each ship item start one (port of
//! `sc-gamedata/src/domains/fire.ts`).
//!
//! `FireHazardGlobalParams` holds the rules every fire obeys, its per-damage-type arrays in
//! DamageInfo order. An item is listed when it has one of three fire paths: a
//! `FireHazardIgniteEvent` on entering a health state, a death explosion with a thermal
//! part, or a misfire that can pick `SFireMisfireEffect`. Its overheat and misfire
//! temperatures and cooling law come along for the heat estimate.

use crate::domain::{Domain, DomainError, ParseContext, field};
use crate::entity::{component, components};
use crate::helpers_items::item_common::{DAMAGE_TYPES, damage_type_map, item_identity};
use crate::helpers_items::{locale_compare, number, round_to, truthy};
use crate::localization::{LocRefs, Localization};
use crate::records::{GameRecord, at, num, str_};
use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::HashMap;

pub static DOMAIN: Fire = Fire;

pub struct Fire;

/// `at` on an optional value (`at(undefined, ...)` is undefined).
fn at_opt<'a>(value: Option<&'a Value>, dotted: &str) -> Option<&'a Value> {
    at(value?, dotted)
}

/// `at` on a component.
fn at_c<'a>(c: Option<&'a Map<String, Value>>, dotted: &str) -> Option<&'a Value> {
    let (head, rest) = dotted
        .split_once('.')
        .map_or((dotted, None), |(h, r)| (h, Some(r)));
    let v = c?.get(head)?;
    match rest {
        Some(r) => at(v, r),
        None => Some(v),
    }
}

/// JavaScript truthiness of any JSON value.
fn js_truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|x| x != 0.0 && !x.is_nan()),
        Some(Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

/// `{min?, max?}`: always an object, empty when neither is set.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Range {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DamageToPlayers {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minimum_damage_temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_damage: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference_health: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Extinguishing {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oxygen_reduction: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heat_transfer_multiplier: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FireRules {
    #[serde(rename = "fixedFPS", skip_serializing_if = "Option::is_none")]
    pub fixed_fps: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub global_ignition_chance_multiplier: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub global_flash_ignition_threshold_multiplier: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explosion_chance_multiplier: Option<Map<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub projectile_chance_multiplier: Option<Map<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collision_chance_multiplier: Option<Map<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_chance_multiplier: Option<Map<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub damage_type_ignition_modifiers: Option<Map<String, Value>>,
    pub oxygen_pressure_range: Range,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fire_mass_threshold: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maximum_radiation_radius: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maximum_convection_distance: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub standard_convection_temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub room_connector_ignite_threshold: Option<f64>,
    pub damage_to_players: DamageToPlayers,
    pub extinguishing: Extinguishing,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IgniteEvent {
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub below_health_ratio: Option<f64>,
    pub chance: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strength: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub radius: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Explosion {
    pub damage: Map<String, Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_radius: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_radius: Option<f64>,
}

impl Explosion {
    fn thermal(&self) -> bool {
        truthy(num(self.damage.get("thermal")))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Misfire {
    pub fire_by_level: Vec<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger: Option<Map<String, Value>>,
    pub window: Range,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Overheat {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovery: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Temperature {
    pub enabled: bool,
    pub cooling_equalization_rate: f64,
    pub temperature_difference: f64,
    pub heat_gen: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_cooling_temperature: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FireItem {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_template: Option<bool>,
    #[serde(flatten)]
    pub loc: LocRefs,
    pub record_name: String,
    pub id: String,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
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
    pub health: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub damage_multipliers: Option<Map<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ignite: Option<Vec<IgniteEvent>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub death_explosion: Option<Explosion>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub misfire: Option<Misfire>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overheat: Option<Overheat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub misfire_temperature: Option<Range>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<Temperature>,
}

/// `perType`: a positional array keyed by damage type.
fn per_type(value: Option<&Value>) -> Option<Map<String, Value>> {
    let list = value?.as_array()?;
    let mut out = Map::new();
    for (i, x) in list.iter().enumerate() {
        if let (Some(n), Some(ty)) = (num(Some(x)), DAMAGE_TYPES.get(i)) {
            out.insert((*ty).to_string(), number(n));
        }
    }
    (!out.is_empty()).then_some(out)
}

/// `String(value ?? "")` without the `_PointsTo_:` prefix.
fn ptr(value: Option<&Value>) -> String {
    let s = match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    };
    s.replacen("_PointsTo_:", "", 1)
}

/// The object elements of an array, or nothing.
fn as_list(value: Option<&Value>) -> impl Iterator<Item = &Map<String, Value>> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
}

pub fn parse_fire_rules(record: &GameRecord) -> FireRules {
    let v = &record.value;
    let n = |k: &str| num(at(v, k));
    FireRules {
        fixed_fps: n("update.fixedFPS"),
        global_ignition_chance_multiplier: n("ignition.globalIgnitionChanceMultiplier"),
        global_flash_ignition_threshold_multiplier: n(
            "ignition.globalFlashIgnitionThresholdMultiplier",
        ),
        explosion_chance_multiplier: per_type(at(v, "ignition.explosionChanceMultiplier")),
        projectile_chance_multiplier: per_type(at(v, "ignition.projectileChanceMultiplier")),
        collision_chance_multiplier: per_type(at(v, "ignition.collisionChanceMultiplier")),
        fallback_chance_multiplier: per_type(at(v, "ignition.fallbackChanceMultiplier")),
        damage_type_ignition_modifiers: per_type(at(v, "ignition.damageTypeIgnitionModifiers")),
        oxygen_pressure_range: Range {
            min: n("propagation.oxygenPressureRange.minimum"),
            max: n("propagation.oxygenPressureRange.maximum"),
        },
        fire_mass_threshold: n("propagation.fireMassThreshold"),
        maximum_radiation_radius: n("propagation.maximumRadiationRadius"),
        maximum_convection_distance: n("propagation.maximumConvectionDistance"),
        standard_convection_temperature: n("propagation.standardConvectionTemperature"),
        room_connector_ignite_threshold: n("roomConnector.igniteThreshold"),
        damage_to_players: DamageToPlayers {
            minimum_damage_temperature: n("damageToHealth.minimumDamageTemperature"),
            base_damage: n("damageToHealth.baseDamage"),
            reference_health: n("damageToHealth.referenceHealth"),
        },
        extinguishing: Extinguishing {
            oxygen_reduction: n("extinguishing.oxygenReduction"),
            heat_transfer_multiplier: n("extinguishing.heatTransferMultiplier"),
        },
    }
}

/// An ExplosionParams struct, or `None` when it deals no damage.
pub fn parse_explosion(value: Option<&Value>) -> Option<Explosion> {
    let damage = damage_type_map(at_opt(value, "damage"))?;
    let dealt: Map<String, Value> = damage
        .into_iter()
        .filter(|(_, x)| x.as_f64() != Some(0.0))
        .collect();
    if dealt.is_empty() {
        return None;
    }
    Some(Explosion {
        damage: dealt,
        min_radius: num(at_opt(value, "minRadius")),
        max_radius: num(at_opt(value, "maxRadius")),
    })
}

fn ignite_events(record: &GameRecord) -> Vec<IgniteEvent> {
    let mut out = Vec::new();
    let machines = components(record).filter(|c| {
        c.get("_Type_").and_then(Value::as_str) == Some("SInteractionStateMachineParams")
    });
    for machine in machines {
        for state_type in as_list(machine.get("StateTypes")) {
            let mut below: HashMap<String, Option<f64>> = HashMap::new();
            for m in as_list(state_type.get("StateModifiers")) {
                for range in as_list(m.get("stateRanges")) {
                    below.insert(ptr(range.get("state")), num(range.get("max")));
                }
            }
            for state in as_list(state_type.get("States")) {
                for ev in as_list(state.get("EnterEventDispatchers")) {
                    if ev.get("_Type_").and_then(Value::as_str) != Some("FireHazardIgniteEvent") {
                        continue;
                    }
                    out.push(IgniteEvent {
                        state: str_(state.get("StateName")).unwrap_or_default().to_string(),
                        below_health_ratio: below
                            .get(&ptr(state.get("_Pointer_")))
                            .copied()
                            .flatten(),
                        chance: num(ev.get("ignitionChance")).unwrap_or(0.0) / 100.0,
                        strength: num(ev.get("ignitionStrength")),
                        radius: num(ev.get("ignitionRadius")),
                    });
                }
            }
        }
    }
    out
}

fn misfire_of(record: &GameRecord) -> Option<Misfire> {
    let params = component(record, "EntityComponentMisfireParams")?;
    let fire: Vec<String> = as_list(params.get("misfires"))
        .filter(|m| m.get("_Type_").and_then(Value::as_str) == Some("SFireMisfireEffect"))
        .map(|m| ptr(m.get("_Pointer_")))
        .collect();
    if fire.is_empty() {
        return None;
    }
    let weight = |p: &Map<String, Value>| num(p.get("weight")).unwrap_or(0.0);
    let fire_by_level = as_list(params.get("misfireLevels"))
        .map(|level| {
            let picks: Vec<_> = as_list(level.get("misfireProbabilityArray")).collect();
            let total: f64 = picks.iter().map(|p| weight(p)).sum();
            let fires: f64 = picks
                .iter()
                .filter(|p| {
                    p.get("misfireArray")
                        .and_then(Value::as_array)
                        .is_some_and(|a| a.iter().any(|x| fire.contains(&ptr(Some(x)))))
                })
                .map(|p| weight(p))
                .sum();
            if total > 0.0 {
                round_to(fires / total, 1e4)
            } else {
                0.0
            }
        })
        .collect();
    let first = as_list(params.get("triggerConditions")).next();
    let mut trigger = Map::new();
    for key in ["degradation", "damage", "heat", "distortion"] {
        if let Some(n) = num(first.and_then(|f| f.get(key))) {
            trigger.insert(key.into(), number(n));
        }
    }
    Some(Misfire {
        fire_by_level,
        trigger: (!trigger.is_empty()).then_some(trigger),
        window: Range {
            min: num(params.get("minWindowLength")),
            max: num(params.get("maxWindowLength")),
        },
    })
}

fn temperature_of(record: &GameRecord) -> (Option<Overheat>, Option<Range>, Option<Temperature>) {
    let comp = component(record, "SEntityPhysicsControllerParams");
    let Some(t) = at_c(comp, "PhysType.temperature").filter(|t| t.is_object() || t.is_array())
    else {
        return (None, None, None);
    };
    let res = at(t, "itemResourceParams");
    let overheat = js_truthy(at_opt(res, "enableOverheat")).then(|| Overheat {
        warning: num(at_opt(res, "overheatWarningTemperature")),
        at: num(at_opt(res, "overheatTemperature")),
        recovery: num(at_opt(res, "overheatRecoveryTemperature")),
    });
    let mis_max = num(at(t, "misfireTemperatureRange.maximum"));
    let misfire_temperature = truthy(mis_max).then(|| Range {
        min: num(at(t, "misfireTemperatureRange.minimum")),
        max: mis_max,
    });
    let rate = num(at(t, "coolingEqualizationParams.coolingEqualizationRate"));
    let diff = num(at(t, "coolingEqualizationParams.temperatureDifference"));
    let temperature = match (rate, diff) {
        (Some(rate), Some(diff)) if truthy(Some(rate)) && truthy(Some(diff)) => {
            // One entry per state (a quantum drive's Online and Travelling); only one is
            // active, so the max.
            let heat = as_list(at_opt(res, "stateHeatGenerationValues"))
                .map(|h| num(h.get("baselineTemperatureChange")).unwrap_or(0.0))
                .fold(0.0_f64, f64::max);
            Some(Temperature {
                enabled: js_truthy(at(t, "enable")),
                cooling_equalization_rate: rate,
                temperature_difference: diff,
                heat_gen: round_to(heat, 1e4),
                min_cooling_temperature: num(at_opt(res, "minCoolingTemperature")),
            })
        }
        _ => None,
    };
    (overheat, misfire_temperature, temperature)
}

fn damage_multipliers(record: &GameRecord) -> Option<Map<String, Value>> {
    let table = at_c(
        component(record, "SHealthComponentParams"),
        "DamageResistances",
    )?
    .as_object()?;
    let mut out = Map::new();
    for (key, entry) in table {
        let word = key
            .strip_suffix("Resistance")
            .filter(|w| !w.is_empty() && w.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
        if let (Some(w), Some(x)) = (word, num(at(entry, "Multiplier"))) {
            out.insert(w.to_lowercase(), number(x));
        }
    }
    (!out.is_empty()).then_some(out)
}

pub fn parse_fire_item(record: &GameRecord, localization: &Localization) -> Option<FireItem> {
    let ignite = ignite_events(record);
    let health = component(record, "SHealthComponentParams");
    let death_explosion = parse_explosion(at_c(health, "DeathExplosionParams"));
    let misfire = misfire_of(record);
    let (overheat, misfire_temperature, temperature) = temperature_of(record);
    if ignite.is_empty()
        && !death_explosion.as_ref().is_some_and(Explosion::thermal)
        && misfire.is_none()
    {
        return None;
    }
    let identity = item_identity(record, localization);
    Some(FireItem {
        name: identity.name,
        is_template: identity.is_template,
        loc: identity.loc,
        record_name: record.name.clone(),
        id: record.id.clone(),
        ty: identity.ty,
        sub_type: identity.sub_type,
        size: identity.size,
        grade: identity.grade,
        manufacturer: identity.manufacturer,
        health: num(at_c(health, "Health")),
        damage_multipliers: damage_multipliers(record),
        ignite: (!ignite.is_empty()).then_some(ignite),
        death_explosion,
        misfire,
        overheat,
        misfire_temperature,
        temperature,
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Summary {
    rules: usize,
    items: usize,
    with_ignite_roll: usize,
    with_thermal_death_explosion: usize,
    with_fire_misfire: usize,
    by_type: Map<String, Value>,
}

impl Domain for Fire {
    fn name(&self) -> &'static str {
        "game-fire"
    }

    fn description(&self) -> &'static str {
        "Fire-hazard rules (ignition by damage type, oxygen, spread, room heat) and what makes each ship item start a fire: ignite rolls on death, thermal death explosions, fire misfires, overheat and misfire temperatures."
    }

    fn filters(&self) -> &'static [&'static str] {
        &["**/roomsystem/firehazard/**", "**/entities/scitem/ships/**"]
    }

    fn needs_localization(&self) -> bool {
        true
    }

    fn parse(&self, ctx: &ParseContext<'_>) -> Result<Map<String, Value>, DomainError> {
        let rules = ctx
            .records
            .iter()
            .find(|r| r.ty == "FireHazardGlobalParams");
        let mut items: Vec<FireItem> = ctx
            .records
            .iter()
            .filter(|r| r.ty == "EntityClassDefinition")
            .filter_map(|r| parse_fire_item(r, ctx.localization))
            .collect();
        items.sort_by(|a, b| {
            locale_compare(a.ty.as_deref().unwrap_or(""), b.ty.as_deref().unwrap_or(""))
                .then_with(|| {
                    let d = a.size.unwrap_or(0.0) - b.size.unwrap_or(0.0);
                    d.partial_cmp(&0.0).unwrap_or(std::cmp::Ordering::Equal)
                })
                .then_with(|| locale_compare(&a.record_name, &b.record_name))
        });
        let mut by_type = Map::new();
        for i in &items {
            let key = i.ty.clone().unwrap_or_else(|| "unknown".into());
            let n = by_type.get(&key).and_then(Value::as_u64).unwrap_or(0);
            by_type.insert(key, Value::from(n + 1));
        }
        let summary = Summary {
            rules: usize::from(rules.is_some()),
            items: items.len(),
            with_ignite_roll: items.iter().filter(|i| i.ignite.is_some()).count(),
            with_thermal_death_explosion: items
                .iter()
                .filter(|i| i.death_explosion.as_ref().is_some_and(Explosion::thermal))
                .count(),
            with_fire_misfire: items.iter().filter(|i| i.misfire.is_some()).count(),
            by_type,
        };
        let mut body = Map::new();
        if let Some(r) = rules {
            field(&mut body, "rules", &parse_fire_rules(r))?;
        }
        field(&mut body, "items", &items)?;
        field(&mut body, "summary", &summary)?;
        Ok(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::NoFiles;
    use crate::records::{RecordSet, normalize_record};
    use serde_json::json;

    fn parse(records: Vec<GameRecord>) -> Map<String, Value> {
        let set = RecordSet::new(records);
        let loc = Localization::new();
        DOMAIN
            .parse(&ParseContext::new(&set, &loc, &NoFiles))
            .unwrap()
    }

    fn rec(ty: &str, short: &str, value: Value) -> GameRecord {
        let mut v = value;
        v["_Type_"] = json!(ty);
        normalize_record(
            json!({"_RecordName_": format!("{ty}.{short}"), "_RecordId_": format!("id-{short}"), "_RecordValue_": v}),
            &format!("libs/foundry/records/entities/scitem/ships/{}", short.to_lowercase()),
        )
        .unwrap()
    }

    #[test]
    fn rules_keep_empty_groups_and_per_type_arrays() {
        let r = rec(
            "FireHazardGlobalParams",
            "Global",
            json!({"update": {"fixedFPS": 1},
                "ignition": {"explosionChanceMultiplier": [0, 0, 0, 0.25, 0, 0]},
                "roomConnector": {"igniteThreshold": 200}}),
        );
        let body = parse(vec![r]);
        assert_eq!(
            body["rules"],
            json!({"fixedFPS": 1.0,
                "explosionChanceMultiplier": {"physical": 0.0, "energy": 0.0, "distortion": 0.0,
                    "thermal": 0.25, "biochemical": 0.0, "stun": 0.0},
                "oxygenPressureRange": {}, "roomConnectorIgniteThreshold": 200.0,
                "damageToPlayers": {}, "extinguishing": {}})
        );
        assert_eq!(body["items"], json!([]));
        assert_eq!(body["summary"]["rules"], json!(1));
    }

    #[test]
    fn ignite_roll_misfire_and_temperatures() {
        let item = rec(
            "EntityClassDefinition",
            "SHLD_Test",
            json!({"Components": [
                {"_Type_": "SAttachableComponentParams", "AttachDef": {"Type": "Shield", "Size": 4,
                    "Localization": {"Name": "@item_NameSHLD_Test"}}},
                {"_Type_": "SHealthComponentParams", "Health": 43000,
                    "DamageResistances": {"PhysicalResistance": {"Multiplier": 0.9}},
                    "DeathExplosionParams": {"damage": {"DamagePhysical": 0, "DamageThermal": 0}}},
                {"_Type_": "SInteractionStateMachineParams", "StateTypes": [{
                    "StateModifiers": [{"stateRanges": [{"state": "_PointsTo_:S1", "max": 0.01}]}],
                    "States": [{"_Pointer_": "_PointsTo_:S1", "StateName": "Destroyed",
                        "EnterEventDispatchers": [{"_Type_": "FireHazardIgniteEvent",
                            "ignitionChance": 25, "ignitionRadius": 2}]}]}]},
                {"_Type_": "EntityComponentMisfireParams",
                    "misfires": [{"_Type_": "SFireMisfireEffect", "_Pointer_": "_PointsTo_:F"}],
                    "misfireLevels": [{"misfireProbabilityArray": [
                        {"weight": 1, "misfireArray": ["_PointsTo_:F"]}, {"weight": 2, "misfireArray": []}]}],
                    "triggerConditions": [{"heat": 0.5}], "minWindowLength": 3},
                {"_Type_": "SEntityPhysicsControllerParams", "PhysType": {"temperature": {
                    "enable": true,
                    "misfireTemperatureRange": {"minimum": 400, "maximum": 500},
                    "coolingEqualizationParams": {"coolingEqualizationRate": 0.2, "temperatureDifference": 50},
                    "itemResourceParams": {"enableOverheat": true, "overheatTemperature": 600,
                        "stateHeatGenerationValues": [{"baselineTemperatureChange": 1.23456}, {}]}}}}
            ]}),
        );
        let body = parse(vec![item]);
        let it = &body["items"][0];
        assert_eq!(it["nameKey"], json!("item_NameSHLD_Test"));
        assert_eq!(it["nameStatus"], json!("unresolved"));
        assert_eq!(
            it["ignite"],
            json!([{"state": "Destroyed", "belowHealthRatio": 0.01, "chance": 0.25, "radius": 2.0}])
        );
        assert!(it.get("deathExplosion").is_none());
        assert_eq!(
            it["misfire"],
            json!({"fireByLevel": [0.3333], "trigger": {"heat": 0.5}, "window": {"min": 3.0}})
        );
        assert_eq!(it["overheat"], json!({"at": 600.0}));
        assert_eq!(
            it["misfireTemperature"],
            json!({"min": 400.0, "max": 500.0})
        );
        assert_eq!(it["temperature"]["heatGen"], json!(1.2346));
        assert_eq!(it["damageMultipliers"], json!({"physical": 0.9}));
        assert_eq!(body["summary"]["byType"], json!({"Shield": 1}));
    }

    #[test]
    fn items_without_a_fire_path_are_left_out() {
        let plain = rec(
            "EntityClassDefinition",
            "PWR_Test",
            json!({"Components": [{"_Type_": "SHealthComponentParams", "Health": 10,
                "DeathExplosionParams": {"damage": {"DamagePhysical": 100}}}]}),
        );
        let hot = rec(
            "EntityClassDefinition",
            "PWR_Hot",
            json!({"Components": [{"_Type_": "SHealthComponentParams", "Health": 10,
                "DeathExplosionParams": {"damage": {"DamageThermal": 50}, "maxRadius": 5}}]}),
        );
        let body = parse(vec![plain, hot]);
        assert_eq!(body["items"].as_array().unwrap().len(), 1);
        assert_eq!(
            body["items"][0]["deathExplosion"],
            json!({"damage": {"thermal": 50}, "maxRadius": 5.0})
        );
    }
}
