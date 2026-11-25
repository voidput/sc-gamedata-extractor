//! Deflection check: which ship weapons get through a ship's stock armour (port of
//! `sc-gamedata/src/deflection.ts`).
//!
//! Armour carries a per-type deflection value and a per-type damage multiplier. A hit whose
//! damage of a type does not exceed that type's deflection is deflected for that type;
//! damage that does exceed it is scaled by the multiplier. The check is per projectile.
//! Distortion is kept apart: it drains shields and component pools, not hull.
//!
//! Two simplifications: deflection is a threshold, not subtracted from the hit; armour is
//! at full health.
//!
//! Works on the dataset rows as JSON (`game-weapons.json` weapons, a vehicle's `armor`), so
//! `null` and absent fields keep their JavaScript meaning ([`J`]).

use crate::collate::locale_compare;
use crate::finder::{js_pad_end, js_slice};
use crate::js::{js_round, number};
use serde_json::{Map, Value, json};

/// A JavaScript value read as a number: `undefined`, `null`, or a number (anything else is
/// `NaN`, as `ToNumber` makes it).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum J {
    #[default]
    U,
    N,
    V(f64),
}

impl J {
    pub fn of(v: Option<&Value>) -> J {
        match v {
            None => J::U,
            Some(Value::Null) => J::N,
            Some(Value::Number(n)) => J::V(n.as_f64().unwrap_or(f64::NAN)),
            Some(Value::Bool(b)) => J::V(f64::from(u8::from(*b))),
            Some(_) => J::V(f64::NAN),
        }
    }

    pub fn from_opt(x: Option<f64>) -> J {
        x.map_or(J::U, J::V)
    }

    /// `a ?? b`.
    pub fn or(self, other: J) -> J {
        match self {
            J::U | J::N => other,
            v => v,
        }
    }

    /// `ToNumber`: `undefined` is NaN, `null` 0.
    pub fn num(self) -> f64 {
        match self {
            J::U => f64::NAN,
            J::N => 0.0,
            J::V(x) => x,
        }
    }

    pub fn truthy(self) -> bool {
        matches!(self, J::V(x) if x != 0.0 && !x.is_nan())
    }

    pub fn defined(self) -> bool {
        self != J::U
    }

    /// `String(x)`.
    pub fn text(self) -> String {
        match self {
            J::U => "undefined".into(),
            J::N => "null".into(),
            J::V(x) => num_text(x),
        }
    }

    /// `String(x ?? fallback)`.
    pub fn text_or(self, fallback: &str) -> String {
        match self {
            J::U | J::N => fallback.into(),
            J::V(x) => num_text(x),
        }
    }

    /// The JSON value, `None` for `undefined` (left out of the object).
    pub fn json(self) -> Option<Value> {
        match self {
            J::U => None,
            J::N => Some(Value::Null),
            J::V(x) => Some(number(x)),
        }
    }

    /// `===`.
    pub fn strict_eq(self, other: J) -> bool {
        match (self, other) {
            (J::U, J::U) | (J::N, J::N) => true,
            (J::V(a), J::V(b)) => a == b,
            _ => false,
        }
    }
}

/// `String(n)` for a number: `NaN`, `Infinity`, else the shortest round trip.
pub fn num_text(x: f64) -> String {
    if x.is_nan() {
        "NaN".into()
    } else if x.is_infinite() {
        if x > 0.0 { "Infinity" } else { "-Infinity" }.into()
    } else {
        crate::output::js_number(x)
    }
}

/// `String(value)` for any JSON value (`undefined` when absent).
pub fn value_text(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(_)) | Some(Value::Null) | None | Some(Value::Bool(_)) => J::of(v).text(),
        Some(Value::Array(_)) => "[array]".into(),
        Some(Value::Object(_)) => "[object Object]".into(),
    }
}

/// `String.prototype.padStart`, UTF-16 units.
pub fn pad_start(s: &str, width: usize) -> String {
    let len = s.encode_utf16().count();
    format!("{}{s}", " ".repeat(width.saturating_sub(len)))
}

pub(crate) fn round2(x: f64) -> f64 {
    js_round(x * 100.0) / 100.0
}

pub(crate) fn field(v: &Value, path: &[&str]) -> J {
    let mut cur = Some(v);
    for k in path {
        cur = cur.and_then(|c| c.get(*k));
    }
    J::of(cur)
}

/// `weapon.modes.find((m) => m.dps === weapon.dps) ?? weapon.modes[0]`.
pub(crate) fn mode_of(weapon: &Value) -> Option<&Value> {
    let modes = weapon.get("modes").and_then(Value::as_array)?;
    let dps = J::of(weapon.get("dps"));
    modes
        .iter()
        .find(|m| J::of(m.get("dps")).strict_eq(dps))
        .or_else(|| modes.first())
}

/// Hull damage of one hit after deflection and multipliers (`hullDamage` / the loop of
/// `checkWeapon`); `deflected` collects the types at or below their threshold.
pub(crate) fn hull_damage<'a>(
    damage: impl Iterator<Item = (&'a str, J)>,
    armor: Option<&Value>,
    mut deflected: Option<&mut Vec<String>>,
) -> f64 {
    let mut applied = 0.0;
    for (ty, amount) in damage {
        if !amount.truthy() || ty == "distortion" {
            continue;
        }
        let amount = amount.num();
        let threshold = armor
            .map_or(J::U, |a| field(a, &["deflection", ty]))
            .or(J::V(0.0))
            .num();
        if amount <= threshold {
            if let Some(d) = deflected.as_deref_mut() {
                d.push(ty.to_string());
            }
            continue;
        }
        let mult = armor
            .map_or(J::U, |a| field(a, &["damageMultiplier", ty]))
            .or(J::V(1.0))
            .num();
        applied += amount * mult;
    }
    applied
}

/// One `DeflectionRow`.
#[derive(Debug, Clone, PartialEq)]
pub struct DeflectionRow {
    pub name: String,
    pub record_name: String,
    pub size: J,
    /// Per projectile, by damage type (the ammo's `damage` object as stored).
    pub damage: Map<String, Value>,
    pub deflected: Vec<String>,
    pub penetrates: bool,
    pub applied_per_hit: f64,
    pub distortion: Option<f64>,
    pub applied_dps: Option<f64>,
    pub dps: J,
}

impl DeflectionRow {
    /// The row as the TypeScript object, in its key order.
    pub fn to_value(&self) -> Value {
        let mut m = Map::new();
        m.insert("name".into(), json!(self.name));
        m.insert("recordName".into(), json!(self.record_name));
        if let Some(v) = self.size.json() {
            m.insert("size".into(), v);
        }
        m.insert("damage".into(), Value::Object(self.damage.clone()));
        m.insert("deflected".into(), json!(self.deflected));
        if let Some(d) = self.distortion {
            m.insert("distortion".into(), number(d));
        }
        m.insert("penetrates".into(), json!(self.penetrates));
        m.insert("appliedPerHit".into(), number(self.applied_per_hit));
        if let Some(d) = self.applied_dps {
            m.insert("appliedDps".into(), number(d));
        }
        if let Some(v) = self.dps.json() {
            m.insert("dps".into(), v);
        }
        Value::Object(m)
    }
}

fn text_of(v: &Value, key: &str) -> String {
    value_text(v.get(key))
}

/// `checkWeapon`: `None` for a weapon whose ammo has no damage.
pub fn check_weapon(weapon: &Value, armor: &Value) -> Option<DeflectionRow> {
    let damage = weapon
        .get("ammo")
        .and_then(|a| a.get("damage"))
        .and_then(Value::as_object)
        .filter(|d| !d.is_empty())?;
    let mut deflected = Vec::new();
    let applied = hull_damage(
        damage.iter().map(|(k, v)| (k.as_str(), J::of(Some(v)))),
        Some(armor),
        Some(&mut deflected),
    );
    let mode = mode_of(weapon);
    let pellets = mode
        .map_or(J::U, |m| J::of(m.get("pellets")))
        .or(J::V(1.0))
        .num();
    let multiplier = mode
        .map_or(J::U, |m| J::of(m.get("damageMultiplier")))
        .or(J::V(1.0))
        .num();
    let fire_rate = J::of(weapon.get("fireRate"));
    let per_second = fire_rate.truthy().then(|| fire_rate.num() / 60.0);
    let distortion = J::of(damage.get("distortion"));
    Some(DeflectionRow {
        name: text_of(weapon, "name"),
        record_name: text_of(weapon, "recordName"),
        size: J::of(weapon.get("size")),
        damage: damage.clone(),
        deflected,
        penetrates: applied > 0.0,
        applied_per_hit: round2(applied * multiplier),
        distortion: distortion.truthy().then(|| distortion.num()),
        applied_dps: per_second.map(|ps| round2(applied * multiplier * pellets * ps)),
        dps: J::of(weapon.get("dps")),
    })
}

fn truthy_value(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|x| x != 0.0 && !x.is_nan()),
        Some(Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

/// `deflectionCheck`: non-template weapons (of `size` when given), best applied DPS first.
pub fn deflection_check(armor: &Value, weapons: &[Value], size: Option<f64>) -> Vec<DeflectionRow> {
    let mut rows: Vec<DeflectionRow> = weapons
        .iter()
        .filter(|w| {
            !truthy_value(w.get("isTemplate"))
                && size.is_none_or(|s| J::of(w.get("size")).strict_eq(J::V(s)))
        })
        .filter_map(|w| check_weapon(w, armor))
        .collect();
    rows.sort_by(|a, b| {
        let d = b.applied_dps.unwrap_or(0.0) - a.applied_dps.unwrap_or(0.0);
        if d != 0.0 && !d.is_nan() {
            return d.partial_cmp(&0.0).unwrap_or(std::cmp::Ordering::Equal);
        }
        locale_compare(&a.name, &b.name)
    });
    rows
}

fn entries(armor: &Value, key: &str) -> Vec<(String, J)> {
    armor
        .get(key)
        .and_then(Value::as_object)
        .map(|o| o.iter().map(|(k, v)| (k.clone(), J::of(Some(v)))).collect())
        .unwrap_or_default()
}

/// `renderDeflection`.
pub fn render_deflection(
    ship: &str,
    armor: &Value,
    rows: &[DeflectionRow],
    limit: usize,
) -> String {
    let defl: Vec<String> = entries(armor, "deflection")
        .into_iter()
        .filter(|(_, v)| v.num() > 0.0)
        .map(|(k, v)| format!("{k} {}", v.text()))
        .collect();
    let mult: Vec<String> = entries(armor, "damageMultiplier")
        .into_iter()
        .filter(|(_, v)| !v.strict_eq(J::V(1.0)))
        .map(|(k, v)| format!("{k} x{}", v.text()))
        .collect();
    let or = |v: Vec<String>, none: &str| {
        if v.is_empty() {
            none.to_string()
        } else {
            v.join(", ")
        }
    };
    let mut lines = vec![
        format!(
            "{ship}  armour {}  health {}",
            text_of(armor, "item"),
            J::of(armor.get("health")).text_or("?")
        ),
        format!("  deflects at or below: {}", or(defl, "nothing")),
        format!("  hull damage multipliers: {}", or(mult, "none")),
        String::new(),
        "  size  weapon                                   per hit   applied  applied dps  listed dps"
            .into(),
    ];
    let penetrating: Vec<&DeflectionRow> = rows.iter().filter(|r| r.penetrates).collect();
    let distortion: Vec<&DeflectionRow> = rows
        .iter()
        .filter(|r| !r.penetrates && r.distortion.is_some())
        .collect();
    let blocked: Vec<&DeflectionRow> = rows
        .iter()
        .filter(|r| !r.penetrates && r.distortion.is_none())
        .collect();
    for row in penetrating.iter().take(limit) {
        let per_hit = row
            .damage
            .iter()
            .filter(|(k, _)| k.as_str() != "distortion")
            .fold(0.0, |a, (_, v)| a + J::of(Some(v)).num());
        let deflected = if row.deflected.is_empty() {
            String::new()
        } else {
            format!("  ({} deflected)", row.deflected.join(", "))
        };
        lines.push(format!(
            "  S{} {} {} {} {} {}{deflected}",
            js_pad_end(&row.size.text_or("?"), 3),
            js_pad_end(js_slice(&row.name, 40), 40),
            pad_start(&num_text(round2(per_hit)), 8),
            pad_start(&num_text(row.applied_per_hit), 9),
            pad_start(&row.applied_dps.map(num_text).unwrap_or_default(), 12),
            pad_start(&row.dps.text_or(""), 11),
        ));
    }
    if penetrating.len() > limit {
        lines.push(format!(
            "  ... {} more penetrate",
            penetrating.len() - limit
        ));
    }
    lines.push(String::new());
    lines.push(format!(
        "  {} penetrate, {} fully deflected, {} distortion only",
        penetrating.len(),
        blocked.len(),
        distortion.len()
    ));
    let names = |list: &[&DeflectionRow]| {
        list.iter()
            .take(limit)
            .map(|r| format!("{} (S{})", r.name, r.size.text()))
            .collect::<Vec<_>>()
            .join(", ")
    };
    if !blocked.is_empty() {
        lines.push(format!(
            "  deflected: {}{}",
            names(&blocked),
            if blocked.len() > limit { ", ..." } else { "" }
        ));
    }
    if !distortion.is_empty() {
        lines.push(format!(
            "  distortion only (shields and components, not hull): {}",
            names(&distortion)
        ));
    }
    lines.push(String::new());
    lines.push("  Full-health armour; deflection treated as a per-projectile threshold.".into());
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Perseus stock armour, as the files have it.
    fn perseus() -> Value {
        json!({
            "item": "ARMR_RSI_Perseus",
            "health": 48672,
            "deflection": {"physical": 495, "energy": 418, "distortion": 0},
            "damageMultiplier": {"physical": 0.7, "energy": 0.5, "distortion": 1},
        })
    }

    fn gun(name: &str, damage: Value, fire_rate: f64, extra: Value) -> Value {
        let total: f64 = damage
            .as_object()
            .unwrap()
            .values()
            .map(|v| v.as_f64().unwrap())
            .sum();
        let mut g = json!({
            "name": name, "recordName": format!("EntityClassDefinition.{name}"), "id": name,
            "class": "ship", "size": 3, "fireRate": fire_rate, "dps": total * (fire_rate / 60.0),
            "ammo": {"recordName": format!("AmmoParams.{name}"), "damage": damage}, "modes": [],
        });
        for (k, v) in extra.as_object().unwrap() {
            g[k] = v.clone();
        }
        g
    }

    #[test]
    fn deflects_a_hit_at_or_below_the_threshold() {
        let row = check_weapon(
            &gun("Omnisky_VI", json!({"energy": 145.8}), 150.0, json!({})),
            &perseus(),
        )
        .unwrap();
        assert!(!row.penetrates);
        assert_eq!(row.deflected, vec!["energy"]);
        assert_eq!(row.applied_per_hit, 0.0);
    }

    #[test]
    fn scales_what_gets_through_by_the_hull_multiplier() {
        let row = check_weapon(
            &gun("Sledge_III", json!({"physical": 1125}), 30.0, json!({})),
            &perseus(),
        )
        .unwrap();
        assert!(row.penetrates);
        assert_eq!(row.applied_per_hit, 787.5);
        assert_eq!(row.applied_dps, Some(393.75));
    }

    #[test]
    fn checks_each_type_on_its_own() {
        let row = check_weapon(
            &gun(
                "Mixed",
                json!({"physical": 600, "energy": 100}),
                60.0,
                json!({}),
            ),
            &perseus(),
        )
        .unwrap();
        assert_eq!(row.deflected, vec!["energy"]);
        assert_eq!(row.applied_per_hit, 420.0);
    }

    #[test]
    fn never_counts_distortion_as_hull_damage() {
        let row = check_weapon(
            &gun(
                "Suckerpunch",
                json!({"distortion": 182.52}),
                100.0,
                json!({}),
            ),
            &perseus(),
        )
        .unwrap();
        assert!(!row.penetrates);
        assert_eq!(row.distortion, Some(182.52));
    }

    #[test]
    fn none_for_a_weapon_with_no_ammo_damage() {
        assert!(check_weapon(&gun("Tractor", json!({}), 0.0, json!({})), &perseus()).is_none());
    }

    #[test]
    fn filters_by_size_skips_templates_and_ranks_by_applied_dps() {
        let rows = deflection_check(
            &perseus(),
            &[
                gun("Slow", json!({"physical": 1125}), 30.0, json!({})),
                gun("Fast", json!({"physical": 600}), 120.0, json!({})),
                gun("Small", json!({"physical": 2000}), 60.0, json!({"size": 1})),
                gun(
                    "Template",
                    json!({"physical": 2000}),
                    60.0,
                    json!({"isTemplate": true}),
                ),
            ],
            Some(3.0),
        );
        let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["Fast", "Slow"]);
    }

    #[test]
    fn js_number_semantics() {
        assert_eq!(J::of(None).or(J::of(Some(&Value::Null))), J::N);
        assert_eq!(J::N.num(), 0.0);
        assert!(J::N.strict_eq(J::N) && !J::N.strict_eq(J::U));
        assert_eq!(J::V(f64::INFINITY).text(), "Infinity");
        assert_eq!(pad_start("ab", 4), "  ab");
    }
}
