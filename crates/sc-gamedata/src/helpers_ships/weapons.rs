//! The slice of `weapons.ts` and `ammo.ts` that `ships.ts` reads: a ship gun's display
//! name, size and headline DPS, a missile's name, size and damage, and `totalDamage`.
//!
//! The full weapons and ammo datasets are other ports (`weapons`, `ammo` slots); this is a
//! faithful copy of only the code paths that feed those few fields, so the ships port does
//! not wait on them. Fold into the full ports later.

use super::{at_map, round_to};
use crate::entity::{component, referenced_name};
use crate::localization::{Localization, is_placeholder_name, localize};
use crate::output::js_key_order;
use crate::records::{GameRecord, RecordSet, array, num, str_};
use serde_json::{Map, Value};
use std::collections::HashMap;

/// `round(value, 2)` from `weapons.ts`.
fn round2(x: f64) -> f64 {
    round_to(x, 100.0)
}

const DAMAGE_FIELDS: [(&str, &str); 6] = [
    ("physical", "DamagePhysical"),
    ("energy", "DamageEnergy"),
    ("distortion", "DamageDistortion"),
    ("thermal", "DamageThermal"),
    ("biochemical", "DamageBiochemical"),
    ("stun", "DamageStun"),
];

/// A `DamageInfo` as `(type, amount)` pairs in the TypeScript's key order.
pub type Damage = Vec<(&'static str, f64)>;

/// `parseDamageInfo` (ammo.ts): non-zero amounts only.
fn parse_damage_info(obj: &Map<String, Value>) -> Option<Damage> {
    let d: Damage = DAMAGE_FIELDS
        .iter()
        .filter_map(|&(k, f)| num(obj.get(f)).filter(|&a| a != 0.0).map(|a| (k, a)))
        .collect();
    (!d.is_empty()).then_some(d)
}

/// `findDamage` (ammo.ts): the first non-zero `DamageInfo` nested anywhere, in JS property
/// order.
fn find_damage(value: &Value) -> Option<Damage> {
    match value {
        Value::Array(a) => a.iter().find_map(find_damage),
        Value::Object(obj) => {
            if obj.get("_Type_").and_then(Value::as_str) == Some("DamageInfo")
                && let Some(d) = parse_damage_info(obj)
            {
                return Some(d);
            }
            js_key_order(obj)
                .into_iter()
                .find_map(|k| find_damage(&obj[k]))
        }
        _ => None,
    }
}

/// `parseAmmo(record).damageTotal`.
fn ammo_damage_total(record: &GameRecord) -> Option<f64> {
    let v = &record.value;
    let damage = v
        .get("projectileParams")
        .and_then(find_damage)
        .or_else(|| find_damage(v))?;
    Some(round2(
        damage.iter().map(|(_, x)| x).fold(0.0, |s, x| s + x),
    ))
}

/// `totalDamage`.
pub fn total_damage(damage: Option<&Damage>) -> Option<f64> {
    Some(round2(
        damage?.iter().map(|(_, x)| x).fold(0.0, |s, x| s + x),
    ))
}

/// `parseBeamDamage`: truthy amounts only.
fn parse_beam_damage(value: Option<&Value>) -> Option<Damage> {
    let obj = value?.as_object()?;
    let d: Damage = DAMAGE_FIELDS
        .iter()
        .filter_map(|&(k, f)| {
            num(obj.get(f))
                .filter(|&a| a != 0.0 && !a.is_nan())
                .map(|a| (k, a))
        })
        .collect();
    (!d.is_empty()).then_some(d)
}

/// What a gun contributes to a ship's loadout.
#[derive(Debug, Clone, PartialEq)]
pub struct WeaponSummary {
    pub name: String,
    pub size: Option<f64>,
    pub dps: Option<f64>,
}

/// What a missile contributes to a ship's loadout.
#[derive(Debug, Clone, PartialEq)]
pub struct MissileSummary {
    pub name: String,
    pub size: Option<f64>,
    pub damage: Option<Damage>,
}

/// `WeaponContext`: ammo damage totals by lower-case short name (`indexAmmo`), entities by
/// lower-case short name.
pub struct WeaponContext<'a> {
    pub localization: &'a Localization,
    /// `Some(total)` per AmmoParams; the inner `None` is ammo without damage.
    pub ammo: HashMap<String, Option<f64>>,
    pub entities: &'a HashMap<String, &'a GameRecord>,
}

/// `indexAmmo`, keeping only `damageTotal`.
pub fn index_ammo(records: &RecordSet) -> HashMap<String, Option<f64>> {
    records
        .of_type("AmmoParams")
        .map(|r| (r.short_name.to_lowercase(), ammo_damage_total(r)))
        .collect()
}

static EMPTY: std::sync::LazyLock<Map<String, Value>> = std::sync::LazyLock::new(Map::new);

fn def_of(record: &GameRecord) -> &Map<String, Value> {
    component(record, "SAttachableComponentParams")
        .and_then(|a| a.get("AttachDef"))
        .and_then(Value::as_object)
        .unwrap_or(&EMPTY)
}

/// `ammoFromContainer(...).ammo`: `None` when no reference or the ammo is not indexed;
/// `Some(damageTotal)` otherwise.
fn ammo_from_container(
    container: Option<&Map<String, Value>>,
    ctx: &WeaponContext<'_>,
) -> Option<Option<f64>> {
    let name = referenced_name(container.and_then(|c| c.get("ammoParamsRecord")))?;
    ctx.ammo.get(&name.to_lowercase()).copied()
}

/// `defaultMagazineName`.
fn default_magazine_name(record: &GameRecord) -> Option<&str> {
    let loadout = component(record, "SEntityComponentDefaultLoadoutParams")?;
    let entries = at_map(loadout, "loadout.entries")?.as_array()?;
    for e in entries.iter().filter(|e| e.is_object()) {
        if str_(e.get("itemPortName")).is_some_and(|p| p.to_lowercase().contains("magazine")) {
            return str_(e.get("entityClassName"));
        }
    }
    None
}

/// The numbers one projectile-launching action contributes (`launchStats`).
struct Launch {
    fire_rate: Option<f64>,
    pellets: Option<f64>,
    damage_multiplier: Option<f64>,
}

fn launch_stats(action: Option<&Value>) -> Launch {
    let get = |k: &str| num(action.and_then(|a| a.get(k)));
    let launch = action.and_then(|a| a.get("launchParams"));
    let l = |k: &str| num(launch.and_then(|l| l.get(k)));
    Launch {
        fire_rate: get("fireRate"),
        pellets: l("pelletCount"),
        damage_multiplier: l("damageMultiplier"),
    }
}

/// `sequenceFireRate`.
fn sequence_fire_rate(entries: &[&Value]) -> Option<f64> {
    let mut shots = 0.0;
    let mut seconds = 0.0;
    for e in entries {
        let Some(delay) = num(e.get("delay")).filter(|&d| d > 0.0) else {
            continue;
        };
        let repetitions = num(e.get("repetitions")).unwrap_or(1.0);
        let per_shot = if str_(e.get("unit")) == Some("Seconds") {
            delay
        } else {
            60.0 / delay
        };
        shots += repetitions;
        seconds += per_shot * repetitions;
    }
    (shots > 0.0 && seconds > 0.0).then(|| round2(60.0 * shots / seconds))
}

/// `parseFireMode(...).dps`: `None` for a mode without a figure; the outer `None` is a mode
/// that is dropped (no `_Type_`).
fn fire_mode_dps(action: &Value, per_shot: Option<f64>) -> Option<Option<f64>> {
    let ty = str_(action.get("_Type_"))?;
    let finish = |l: &Launch, rate: Option<f64>| -> Option<f64> {
        let per_shot = per_shot?;
        let alpha = per_shot * l.pellets.unwrap_or(1.0) * l.damage_multiplier.unwrap_or(1.0);
        rate.filter(|&r| r != 0.0 && !r.is_nan())
            .map(|r| round2(alpha * r / 60.0))
    };
    Some(match ty {
        "SWeaponActionFireSingleParams" | "SWeaponActionFireRapidParams" => {
            let l = launch_stats(Some(action));
            finish(&l, l.fire_rate)
        }
        "SWeaponActionFireBurstParams" => {
            let l = launch_stats(Some(action));
            let shot_count = num(action.get("shotCount"));
            let cooldown = num(action.get("cooldownTime")).unwrap_or(0.0);
            let mut sustained = None;
            if let (Some(rate), Some(count)) = (
                l.fire_rate.filter(|&r| r != 0.0),
                shot_count.filter(|&c| c != 0.0),
            ) {
                let burst_seconds = count * 60.0 / rate + cooldown;
                sustained = Some(round2(60.0 * count / burst_seconds));
            }
            finish(&l, sustained.or(l.fire_rate))
        }
        "SWeaponActionFireChargedParams" => {
            let inner = action.get("weaponAction").filter(|v| !v.is_null());
            let l = launch_stats(inner);
            finish(&l, l.fire_rate)
        }
        "SWeaponActionSequenceParams" => {
            let entries: Vec<&Value> = array(action.get("sequenceEntries"))
                .iter()
                .filter(|e| e.is_object())
                .collect();
            let first = entries
                .iter()
                .filter_map(|e| e.get("weaponAction"))
                .find(|a| a.is_object());
            let l = launch_stats(first);
            let rate = sequence_fire_rate(&entries).or(l.fire_rate);
            finish(&l, rate)
        }
        "SWeaponActionFireBeamParams" => {
            total_damage(parse_beam_damage(action.get("damagePerSecond")).as_ref())
        }
        _ => None,
    })
}

/// `parseWeapon(record, "ship", context)`, reduced to name, size and headline DPS.
pub fn parse_weapon(record: &GameRecord, ctx: &WeaponContext<'_>) -> Option<WeaponSummary> {
    let params = component(record, "SCItemWeaponComponentParams")?;
    let def = def_of(record);
    let localized = localize(ctx.localization, at_map(def, "Localization.Name"));
    // Store display props reuse a real gun's name but fire nothing (`/_prop$/i`).
    let short = &record.short_name;
    let is_prop = short.len() >= 5
        && short
            .get(short.len() - 5..)
            .is_some_and(|s| s.eq_ignore_ascii_case("_prop"));
    let placeholder = is_placeholder_name(localized) || is_prop;

    let mut ammo = ammo_from_container(component(record, "SAmmoContainerComponentParams"), ctx);
    if ammo.is_none()
        && let Some(magazine) = default_magazine_name(record)
        && let Some(mag) = ctx.entities.get(&magazine.to_lowercase())
    {
        ammo = ammo_from_container(component(mag, "SAmmoContainerComponentParams"), ctx);
    }
    let per_shot = ammo.flatten();

    let modes: Vec<Option<f64>> = array(params.get("fireActions"))
        .iter()
        .filter(|a| a.is_object())
        .filter_map(|a| fire_mode_dps(a, per_shot))
        .collect();
    // `best ?? modes[0]`: the first mode with the highest DPS; when none has one, the first
    // mode, whose DPS is then absent too.
    let mut best: Option<f64> = None;
    for dps in modes.iter().flatten() {
        if best.unwrap_or(-1.0) < *dps {
            best = Some(*dps);
        }
    }

    Some(WeaponSummary {
        name: if placeholder {
            short.clone()
        } else {
            localized.unwrap_or_default().to_string()
        },
        size: num(def.get("Size")),
        dps: best,
    })
}

/// `parseMissile`, reduced to name, size and damage.
pub fn parse_missile(record: &GameRecord, localization: &Localization) -> Option<MissileSummary> {
    let params = component(record, "SCItemMissileParams")?;
    let def = def_of(record);
    let localized = localize(localization, at_map(def, "Localization.Name"));
    let placeholder = is_placeholder_name(localized);
    Some(MissileSummary {
        name: if placeholder {
            record.short_name.clone()
        } else {
            localized.unwrap_or_default().to_string()
        },
        size: num(def.get("Size")),
        damage: parse_beam_damage(at_map(params, "explosionParams.damage")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::localization::parse_global_ini;
    use crate::records::normalize_record;
    use serde_json::json;

    fn rec(name: &str, ty: &str, path: &str, value: Value) -> GameRecord {
        let mut v = value;
        v["_Type_"] = json!(ty);
        normalize_record(
            json!({"_RecordName_": format!("{ty}.{name}"), "_RecordId_": "id", "_RecordValue_": v}),
            path,
        )
        .unwrap()
    }

    #[test]
    fn ammo_damage_and_fire_modes() {
        let ammo = rec(
            "a_ammo",
            "AmmoParams",
            "libs/foundry/records/ammoparams/a_ammo",
            json!({"projectileParams": {"x": [null, {"_Type_": "DamageInfo", "DamageEnergy": 0}],
                "damage": {"_Type_": "DamageInfo", "DamagePhysical": 10.005, "DamageEnergy": 2}}}),
        );
        assert_eq!(ammo_damage_total(&ammo), Some(12.01));
        let gun = rec(
            "Gun",
            "EntityClassDefinition",
            "libs/foundry/records/entities/scitem/ships/weapons/gun",
            json!({"Components": [
                {"_Type_": "SAttachableComponentParams", "AttachDef": {"Size": 3, "Localization": {"Name": "@n"}}},
                {"_Type_": "SCItemWeaponComponentParams", "fireActions": [
                    {"_Type_": "SWeaponActionFireSingleParams", "fireRate": 600, "launchParams": {"pelletCount": 2}},
                    {"_Type_": "SWeaponActionFireBurstParams", "fireRate": 1200, "shotCount": 3, "cooldownTime": 0.35},
                    {"_Type_": "SWeaponActionSequenceParams", "sequenceEntries": [
                        {"delay": 900, "weaponAction": {"_Type_": "SWeaponActionFireSingleParams", "fireRate": 500}},
                        {"delay": 900}]},
                    {"_Type_": "SWeaponActionFireBeamParams", "damagePerSecond": {"DamageEnergy": 100}},
                    {"_Type_": "SWeaponActionFireTractorBeamParams"},
                    {"name": "untyped"}]},
                {"_Type_": "SAmmoContainerComponentParams", "ammoParamsRecord": "file://./x/ammoparams.a_ammo.json"}
            ]}),
        );
        let set = RecordSet::new(vec![ammo]);
        let entities = HashMap::new();
        let loc = parse_global_ini("n=Gun Name");
        let ctx = WeaponContext {
            localization: &loc,
            ammo: index_ammo(&set),
            entities: &entities,
        };
        let modes: Vec<_> = array(
            component(&gun, "SCItemWeaponComponentParams")
                .unwrap()
                .get("fireActions"),
        )
        .iter()
        .filter_map(|a| fire_mode_dps(a, Some(12.01)))
        .collect();
        // single: 24.02 * 600 / 60; burst: 3 shots per 0.5 s -> 360 RPM; sequence 900 RPM.
        assert_eq!(
            modes,
            vec![Some(240.2), Some(72.06), Some(180.15), Some(100.0), None]
        );
        let w = parse_weapon(&gun, &ctx).unwrap();
        assert_eq!(
            w,
            WeaponSummary {
                name: "Gun Name".into(),
                size: Some(3.0),
                dps: Some(240.2)
            }
        );
    }

    #[test]
    fn props_and_missiles() {
        let prop = rec(
            "Gun_Prop",
            "EntityClassDefinition",
            "p",
            json!({"Components": [{"_Type_": "SCItemWeaponComponentParams"}]}),
        );
        let entities = HashMap::new();
        let loc = Localization::new();
        let ctx = WeaponContext {
            localization: &loc,
            ammo: HashMap::new(),
            entities: &entities,
        };
        let w = parse_weapon(&prop, &ctx).unwrap();
        assert_eq!((w.name.as_str(), w.dps), ("Gun_Prop", None));
        let m = rec(
            "M",
            "EntityClassDefinition",
            "p",
            json!({"Components": [{"_Type_": "SCItemMissileParams",
                "explosionParams": {"damage": {"DamagePhysical": 3200, "DamageEnergy": 0}}}]}),
        );
        let m = parse_missile(&m, &loc).unwrap();
        assert_eq!(total_damage(m.damage.as_ref()), Some(3200.0));
        assert_eq!(m.name, "M");
    }
}
