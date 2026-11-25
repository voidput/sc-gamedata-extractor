//! Engagement maths: how much of a loadout's damage lands on a target, and out to what
//! distance (port of `sc-gamedata/src/engagement.ts`).
//!
//! Grounded inputs only: projectile speed and range, per-projectile damage, falloff
//! (personal weapons only), the target's armour and bounding box. Effective range is a
//! hit-chance question:
//!
//! ```text
//! flight time   t = d / (projectileSpeed + closingSpeed)
//! miss radius   R = jink(t) + d * tan(spread)
//! hit chance    p = 1 when R <= r, else (r / R)^2
//! ```
//!
//! `r` is the radius of a circle with the target's average projected bounding-box area.
//! Acceleration (jink) is not game data, so it defaults to 0 and results are tabulated at
//! several g levels. Weapons and vehicles are read as their dataset JSON rows.

use crate::deflection::{
    J, check_weapon, field, hull_damage, mode_of, num_text, pad_start, round2, value_text,
};
use crate::finder::{js_pad_end, js_slice};
use crate::js::{js_round, number};
use serde_json::{Map, Value, json};

pub const G: f64 = 9.80665;
const DEFAULT_ACCURACY: [f64; 4] = [0.9, 0.75, 0.5, 0.25];
const DEFAULT_BANDS: [f64; 13] = [
    0.0, 250.0, 500.0, 750.0, 1000.0, 1500.0, 2000.0, 2500.0, 3000.0, 4000.0, 5000.0, 6000.0,
    8000.0,
];

fn round(x: f64, places: i32) -> f64 {
    let f = 10f64.powi(places);
    js_round(x * f) / f
}

/// `Math.max` / `Math.min`: NaN wins.
fn js_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.max(b)
    }
}

fn js_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.min(b)
    }
}

/// One loadout entry.
#[derive(Debug, Clone)]
pub struct Gun<'a> {
    pub weapon: &'a Value,
    pub count: f64,
    pub mount: Option<String>,
}

/// `Target`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Target {
    pub name: String,
    /// The vehicle's `armor` as stored; `None` when absent.
    pub armor: Option<Value>,
    pub hit_radius: Option<f64>,
    pub speed: J,
}

impl Target {
    fn armor(&self) -> Option<&Value> {
        self.armor.as_ref().filter(|a| !a.is_null())
    }
    fn hit_radius_truthy(&self) -> bool {
        J::from_opt(self.hit_radius).truthy()
    }
}

/// `EngageOptions`. `None` is `undefined`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EngageOptions {
    pub target_speed: Option<f64>,
    pub closing: Option<f64>,
    /// `spread: "min"` (first shot) instead of `"max"`.
    pub spread_min: bool,
    /// m/s².
    pub accel: Option<f64>,
    pub reaction: Option<f64>,
    pub bands: Option<Vec<f64>>,
    pub accuracy: Option<Vec<f64>>,
}

impl EngageOptions {
    fn levels(&self) -> Vec<f64> {
        self.accuracy
            .clone()
            .unwrap_or_else(|| DEFAULT_ACCURACY.to_vec())
    }
}

fn level_key(p: f64) -> String {
    num_text(js_round(p * 100.0))
}

/// `weapon.range ?? weapon.ammo?.range`.
fn range_of(w: &Value) -> J {
    J::of(w.get("range")).or(field(w, &["ammo", "range"]))
}

fn damage_entries(w: &Value) -> Vec<(String, J)> {
    w.get("ammo")
        .and_then(|a| a.get("damage"))
        .and_then(Value::as_object)
        .map(|o| o.iter().map(|(k, v)| (k.clone(), J::of(Some(v)))).collect())
        .unwrap_or_default()
}

/// `damageAt`: per-projectile damage at a distance, after falloff.
pub fn damage_at(w: &Value, distance: f64) -> Vec<(String, J)> {
    damage_entries(w)
        .into_iter()
        .map(|(ty, amount)| {
            let drop = w
                .get("ammo")
                .and_then(|a| a.get("falloff"))
                .and_then(|f| f.get(&ty))
                .filter(|d| !d.is_null());
            let Some(drop) = drop else {
                return (ty, amount);
            };
            let min = J::of(drop.get("minDistance")).num();
            if distance <= min {
                return (ty, amount);
            }
            let floor = J::of(drop.get("floor")).num();
            let per = J::of(drop.get("perMeter")).num();
            (
                ty,
                J::V(round2(js_max(floor, amount.num() - per * (distance - min)))),
            )
        })
        .collect()
}

/// `falloffRange`: where every falling damage type has reached its floor.
pub fn falloff_range(w: &Value) -> Option<f64> {
    let mut furthest: Option<f64> = None;
    for (ty, amount) in damage_entries(w) {
        let Some(drop) = w
            .get("ammo")
            .and_then(|a| a.get("falloff"))
            .and_then(|f| f.get(&ty))
            .filter(|d| !d.is_null())
        else {
            continue;
        };
        let n = |k: &str| J::of(drop.get(k)).num();
        let reach = n("minDistance") + js_max(0.0, amount.num() - n("floor")) / n("perMeter");
        furthest = Some(js_max(furthest.unwrap_or(0.0), reach));
    }
    furthest.map(|f| round(f, 1))
}

struct Shots {
    per_second: Option<f64>,
    pellets: f64,
    multiplier: f64,
}

fn shots(w: &Value) -> Shots {
    let mode = mode_of(w);
    let fire = J::of(w.get("fireRate"));
    Shots {
        per_second: fire.truthy().then(|| fire.num() / 60.0),
        pellets: mode
            .map_or(J::U, |m| J::of(m.get("pellets")))
            .or(J::V(1.0))
            .num(),
        multiplier: mode
            .map_or(J::U, |m| J::of(m.get("damageMultiplier")))
            .or(J::V(1.0))
            .num(),
    }
}

fn out_of_range(w: &Value, distance: f64) -> bool {
    let range = range_of(w);
    range.defined() && distance > range.num()
}

/// `appliedDpsAt`: hull DPS one gun does at a distance; 0 out of range or deflected.
pub fn applied_dps_at(w: &Value, target: Option<&Target>, distance: f64) -> f64 {
    if out_of_range(w, distance) {
        return 0.0;
    }
    let s = shots(w);
    let has_damage = w
        .get("ammo")
        .and_then(|a| a.get("damage"))
        .is_some_and(|d| !d.is_null());
    let Some(per_second) = s.per_second.filter(|p| *p != 0.0 && !p.is_nan()) else {
        return 0.0;
    };
    if !has_damage {
        return 0.0;
    }
    let hit = damage_at(w, distance);
    hull_damage(
        hit.iter().map(|(k, v)| (k.as_str(), *v)),
        target.and_then(Target::armor),
        None,
    ) * s.pellets
        * s.multiplier
        * per_second
}

fn spread_of(w: &Value, options: &EngageOptions) -> f64 {
    let mode = mode_of(w);
    let get = |k: &str| mode.map_or(J::U, |m| field(m, &["spread", k]));
    let pick = if options.spread_min {
        get("min").or(get("max"))
    } else {
        get("max").or(get("min"))
    };
    pick.or(J::of(w.get("spread"))).or(J::V(0.0)).num()
}

/// `missRadius`: how far from the lead point the round can end up at a distance.
pub fn miss_radius(w: &Value, target: Option<&Target>, distance: f64, o: &EngageOptions) -> f64 {
    let speed = J::of(w.get("speed")).or(J::V(0.0)).num() + o.closing.unwrap_or(0.0);
    if speed <= 0.0 {
        return f64::INFINITY;
    }
    let t = distance / speed;
    let moving = js_max(0.0, t - o.reaction.unwrap_or(0.0));
    let target_speed = J::from_opt(o.target_speed)
        .or(target.map_or(J::U, |t| t.speed))
        .or(J::V(0.0))
        .num();
    let jink = if J::from_opt(o.accel).truthy() {
        let a = o.accel.unwrap_or(0.0);
        js_min(target_speed * moving, 0.5 * a * moving * moving)
    } else {
        0.0
    };
    let scatter = distance * (spread_of(w, o) * std::f64::consts::PI / 180.0).tan();
    jink + scatter
}

/// `hitChance`: share of rounds that land at a distance.
pub fn hit_chance(w: &Value, target: Option<&Target>, distance: f64, o: &EngageOptions) -> f64 {
    if out_of_range(w, distance) {
        return 0.0;
    }
    let Some(r) = target
        .and_then(|t| t.hit_radius)
        .filter(|r| *r != 0.0 && !r.is_nan())
    else {
        return 1.0;
    };
    let big_r = miss_radius(w, target, distance, o);
    if big_r <= r {
        1.0
    } else {
        let q = r / big_r;
        q * q
    }
}

/// `rangeForChance`: farthest distance within max range where the hit chance is at least `p`.
pub fn range_for_chance(w: &Value, target: Option<&Target>, p: f64, o: &EngageOptions) -> f64 {
    let max = range_of(w).or(J::V(0.0)).num();
    if hit_chance(w, target, max, o) >= p {
        return max;
    }
    let (mut lo, mut hi) = (0.0, max);
    for _ in 0..40 {
        let mid = (lo + hi) / 2.0;
        if hit_chance(w, target, mid, o) >= p {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    js_round(lo)
}

/// `GunEnvelope`.
#[derive(Debug, Clone, PartialEq)]
pub struct GunEnvelope {
    pub name: String,
    pub record: String,
    pub size: J,
    pub count: f64,
    pub mount: Option<String>,
    pub speed: J,
    pub max_range: J,
    pub max_flight_time: Option<f64>,
    pub dps: Option<f64>,
    pub applied_dps: f64,
    pub penetrates: bool,
    pub deflected: Option<Vec<String>>,
    pub falloff_range: Option<f64>,
    pub spread: f64,
    /// Keyed by percent, in level order.
    pub range_for: Option<Vec<(String, f64)>>,
}

fn object(pairs: &[(String, f64)]) -> Value {
    let mut m = Map::new();
    for (k, v) in pairs {
        m.insert(k.clone(), number(*v));
    }
    Value::Object(m)
}

/// `Object.fromEntries` lookup: the last entry of a key wins.
fn lookup(pairs: &[(String, f64)], key: &str) -> Option<f64> {
    pairs.iter().rev().find(|(k, _)| k == key).map(|(_, v)| *v)
}

impl GunEnvelope {
    pub fn to_value(&self) -> Value {
        let mut m = Map::new();
        let mut put = |k: &str, v: Option<Value>| {
            if let Some(v) = v {
                m.insert(k.into(), v);
            }
        };
        put("name", Some(json!(self.name)));
        put("record", Some(json!(self.record)));
        put("size", self.size.json());
        put("count", Some(number(self.count)));
        put("mount", self.mount.as_ref().map(|s| json!(s)));
        put("speed", self.speed.json());
        put("maxRange", self.max_range.json());
        put("maxFlightTime", self.max_flight_time.map(number));
        put("dps", self.dps.map(number));
        put("appliedDps", Some(number(self.applied_dps)));
        put("penetrates", Some(json!(self.penetrates)));
        put("deflected", self.deflected.as_ref().map(|d| json!(d)));
        put("falloffRange", self.falloff_range.map(number));
        put("spread", Some(number(self.spread)));
        put("rangeFor", self.range_for.as_deref().map(object));
        Value::Object(m)
    }
}

/// `gunEnvelope`: one gun, `count` of them, against a target.
pub fn gun_envelope(gun: &Gun<'_>, target: Option<&Target>, o: &EngageOptions) -> GunEnvelope {
    let w = gun.weapon;
    let max_range = range_of(w);
    let check = target
        .and_then(Target::armor)
        .and_then(|a| check_weapon(w, a));
    let applied = applied_dps_at(w, target, 0.0) * gun.count;
    let range_for = target.filter(|t| t.hit_radius_truthy()).map(|_| {
        o.levels()
            .iter()
            .map(|&p| (level_key(p), range_for_chance(w, target, p, o)))
            .collect()
    });
    let speed = J::of(w.get("speed"));
    let dps = J::of(w.get("dps"));
    GunEnvelope {
        name: value_text(w.get("name")),
        record: value_text(w.get("recordName"))
            .strip_prefix("EntityClassDefinition.")
            .map(str::to_string)
            .unwrap_or_else(|| value_text(w.get("recordName"))),
        size: J::of(w.get("size")),
        count: gun.count,
        mount: gun.mount.clone(),
        speed,
        max_range,
        max_flight_time: (max_range.truthy() && speed.truthy())
            .then(|| round2(max_range.num() / speed.num())),
        dps: dps.defined().then(|| round2(dps.num() * gun.count)),
        applied_dps: round2(applied),
        penetrates: applied > 0.0,
        deflected: check.map(|c| c.deflected).filter(|d| !d.is_empty()),
        falloff_range: falloff_range(w),
        spread: round(spread_of(w, o), 3),
        range_for,
    }
}

/// `Band`.
#[derive(Debug, Clone, PartialEq)]
pub struct Band {
    pub distance: f64,
    pub guns_in_range: f64,
    pub dps: f64,
    pub applied_dps: f64,
    pub expected_dps: f64,
    pub hit_chance: Option<f64>,
    pub flight_time: Option<(f64, f64)>,
}

impl Band {
    pub fn to_value(&self) -> Value {
        let mut m = Map::new();
        m.insert("distance".into(), number(self.distance));
        m.insert("gunsInRange".into(), number(self.guns_in_range));
        m.insert("dps".into(), number(self.dps));
        m.insert("appliedDps".into(), number(self.applied_dps));
        m.insert("expectedDps".into(), number(self.expected_dps));
        if let Some(h) = self.hit_chance {
            m.insert("hitChance".into(), number(h));
        }
        if let Some((min, max)) = self.flight_time {
            m.insert(
                "flightTime".into(),
                json!({"min": number(min), "max": number(max)}),
            );
        }
        Value::Object(m)
    }
}

/// `Envelope`.
#[derive(Debug, Clone, PartialEq)]
pub struct Envelope {
    pub target: Option<String>,
    pub guns: Vec<GunEnvelope>,
    pub bands: Vec<Band>,
    pub applied_dps: f64,
    pub all_guns_range: Option<f64>,
    pub half_dps_range: Option<f64>,
    pub max_range: Option<f64>,
    pub effective_range: Option<Vec<(String, f64)>>,
    pub target_speed: J,
}

impl Envelope {
    /// The fields in the TypeScript's order, for `{ attacker, ...result, rangeByAccel }`.
    pub fn fields(&self) -> Map<String, Value> {
        let mut m = Map::new();
        if let Some(t) = &self.target {
            m.insert("target".into(), json!(t));
        }
        m.insert(
            "guns".into(),
            Value::Array(self.guns.iter().map(GunEnvelope::to_value).collect()),
        );
        m.insert(
            "bands".into(),
            Value::Array(self.bands.iter().map(Band::to_value).collect()),
        );
        m.insert("appliedDps".into(), number(self.applied_dps));
        for (k, v) in [
            ("allGunsRange", self.all_guns_range),
            ("halfDpsRange", self.half_dps_range),
            ("maxRange", self.max_range),
        ] {
            if let Some(v) = v {
                m.insert(k.into(), number(v));
            }
        }
        if let Some(e) = &self.effective_range {
            m.insert("effectiveRange".into(), object(e));
        }
        if let Some(v) = self.target_speed.json() {
            m.insert("targetSpeed".into(), v);
        }
        m
    }
}

/// `expectedDpsAt`.
pub fn expected_dps_at(
    loadout: &[Gun<'_>],
    target: Option<&Target>,
    distance: f64,
    o: &EngageOptions,
) -> f64 {
    loadout.iter().fold(0.0, |sum, g| {
        sum + applied_dps_at(g.weapon, target, distance)
            * g.count
            * hit_chance(g.weapon, target, distance, o)
    })
}

fn loadout_range_for(
    loadout: &[Gun<'_>],
    target: &Target,
    dps: f64,
    o: &EngageOptions,
    max: f64,
) -> f64 {
    // Expected DPS never rises with distance, so the crossing is found by bisection.
    let t = Some(target);
    if expected_dps_at(loadout, t, max, o) >= dps {
        return max;
    }
    if expected_dps_at(loadout, t, 0.0, o) < dps {
        return 0.0;
    }
    let (mut lo, mut hi) = (0.0, max);
    let mut i = 0;
    while i < 40 && hi - lo > 0.5 {
        let mid = (lo + hi) / 2.0;
        if expected_dps_at(loadout, t, mid, o) >= dps {
            lo = mid;
        } else {
            hi = mid;
        }
        i += 1;
    }
    js_round(lo)
}

/// `Math.max(0, ...xs) || undefined`.
fn max_or_none(xs: impl Iterator<Item = f64>) -> Option<f64> {
    let m = xs.fold(0.0, js_max);
    (m != 0.0 && !m.is_nan()).then_some(m)
}

/// `envelope`: a whole loadout against a target, by distance band.
pub fn envelope(loadout: &[Gun<'_>], target: Option<&Target>, o: &EngageOptions) -> Envelope {
    let guns: Vec<GunEnvelope> = loadout.iter().map(|g| gun_envelope(g, target, o)).collect();
    let gun_max = |g: &GunEnvelope| g.max_range.or(J::V(0.0)).num();
    let mut band_list: Vec<f64> = Vec::new();
    for d in o
        .bands
        .clone()
        .unwrap_or_else(|| DEFAULT_BANDS.to_vec())
        .into_iter()
        .chain(guns.iter().map(gun_max))
    {
        // `new Set`: SameValueZero.
        if !band_list
            .iter()
            .any(|b| *b == d || (b.is_nan() && d.is_nan()))
        {
            band_list.push(d);
        }
    }
    band_list.retain(|d| *d >= 0.0);
    band_list.sort_by(|a, b| {
        (a - b)
            .partial_cmp(&0.0)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let max_range = max_or_none(guns.iter().filter(|g| g.penetrates).map(gun_max));
    let reach = max_or_none(guns.iter().map(gun_max));
    let bands: Vec<Band> = band_list
        .into_iter()
        .filter(|d| reach.is_none_or(|r| *d <= r))
        .map(|distance| {
            let in_range: Vec<&Gun> = loadout
                .iter()
                .zip(&guns)
                .filter(|(_, g)| g.max_range.or(J::V(f64::INFINITY)).num() >= distance)
                .map(|(l, _)| l)
                .collect();
            let flights: Vec<f64> = in_range
                .iter()
                .filter_map(|g| {
                    let s = J::of(g.weapon.get("speed"));
                    s.truthy().then(|| distance / s.num())
                })
                .collect();
            let applied = loadout.iter().fold(0.0, |sum, g| {
                sum + applied_dps_at(g.weapon, target, distance) * g.count
            });
            let expected = expected_dps_at(loadout, target, distance, o);
            Band {
                distance,
                guns_in_range: in_range.iter().fold(0.0, |n, g| n + g.count),
                dps: round2(in_range.iter().fold(0.0, |sum, g| {
                    sum + J::of(g.weapon.get("dps")).or(J::V(0.0)).num() * g.count
                })),
                applied_dps: round2(applied),
                expected_dps: round2(expected),
                hit_chance: (applied > 0.0).then(|| round(expected / applied, 3)),
                flight_time: (!flights.is_empty()).then(|| {
                    (
                        round2(flights.iter().copied().fold(f64::INFINITY, js_min)),
                        round2(flights.iter().copied().fold(f64::NEG_INFINITY, js_max)),
                    )
                }),
            }
        })
        .collect();

    let point_blank = bands.first().map_or(0.0, |b| b.applied_dps);
    let half_dps_range = bands
        .iter()
        .rev()
        .find(|b| point_blank > 0.0 && b.applied_dps >= point_blank / 2.0)
        .map(|b| b.distance);
    let hurting = guns.iter().any(|g| g.penetrates);
    let effective_range = target
        .filter(|t| t.hit_radius_truthy() && point_blank > 0.0)
        .map(|t| {
            o.levels()
                .iter()
                .map(|&p| {
                    (
                        level_key(p),
                        loadout_range_for(loadout, t, p * point_blank, o, max_range.unwrap_or(0.0)),
                    )
                })
                .collect()
        });
    Envelope {
        target: target.map(|t| t.name.clone()),
        applied_dps: round2(point_blank),
        all_guns_range: hurting.then(|| {
            guns.iter()
                .filter(|g| g.penetrates)
                .map(|g| g.max_range.or(J::V(f64::INFINITY)).num())
                .fold(f64::INFINITY, js_min)
        }),
        half_dps_range,
        max_range,
        effective_range,
        target_speed: J::from_opt(o.target_speed).or(target.map_or(J::U, |t| t.speed)),
        guns,
        bands,
    }
}

/// `hitRadiusOf`: radius of a circle with the box's average projected face area.
pub fn hit_radius_of(box_: Option<&Value>) -> Option<f64> {
    let b = box_?;
    let (x, y, z) = (
        field(b, &["x"]).num(),
        field(b, &["y"]).num(),
        field(b, &["z"]).num(),
    );
    if !(x > 0.0 && y > 0.0 && z > 0.0) {
        return None;
    }
    let mean = (x * z + y * z + x * y) / 3.0;
    Some(round2((mean / std::f64::consts::PI).sqrt()))
}

/// One `rangeTable` row.
#[derive(Debug, Clone, PartialEq)]
pub struct RangeRow {
    pub accel_g: f64,
    pub ranges: Vec<(String, f64)>,
}

impl RangeRow {
    pub fn to_value(&self) -> Value {
        json!({"accelG": number(self.accel_g), "ranges": object(&self.ranges)})
    }
}

/// `rangeTable`: range for each hit-chance level at each acceleration (g).
pub fn range_table(
    loadout: &[Gun<'_>],
    target: &Target,
    accels_g: &[f64],
    o: &EngageOptions,
) -> Vec<RangeRow> {
    accels_g
        .iter()
        .map(|&g| {
            let opts = EngageOptions {
                accel: Some(g * G),
                bands: Some(vec![0.0]),
                ..o.clone()
            };
            RangeRow {
                accel_g: g,
                ranges: envelope(loadout, Some(target), &opts)
                    .effective_range
                    .unwrap_or_default(),
            }
        })
        .collect()
}

/// `targetOf`: a ship's target profile from its vehicle row.
pub fn target_of(vehicle: &Value) -> Target {
    Target {
        name: value_text(vehicle.get("name")),
        armor: vehicle.get("armor").cloned(),
        hit_radius: hit_radius_of(vehicle.get("boundingBox").filter(|b| !b.is_null())),
        speed: field(vehicle, &["flight", "scmSpeed"]),
    }
}

fn opt_text(x: Option<f64>) -> String {
    x.map(num_text).unwrap_or_default()
}

/// `renderEnvelope`.
pub fn render_envelope(
    attacker: &str,
    env: &Envelope,
    o: &EngageOptions,
    target: Option<&Target>,
    table: Option<&[RangeRow]>,
) -> String {
    let levels: Vec<String> = o.levels().iter().map(|&p| level_key(p)).collect();
    let mut lines = vec![format!(
        "{attacker}{}",
        env.target
            .as_deref()
            .filter(|t| !t.is_empty())
            .map(|t| format!(" vs {t}"))
            .unwrap_or_default()
    )];
    if let Some(t) = target {
        let truthy = |x: Option<f64>| J::from_opt(x).truthy();
        lines.push(format!(
            "  target moving {} m/s, hit radius {} m{}{}{}",
            env.target_speed.text_or("0"),
            J::from_opt(t.hit_radius).text_or("?"),
            if truthy(o.closing) {
                format!(", closing {} m/s", opt_text(o.closing))
            } else {
                String::new()
            },
            if truthy(o.accel) {
                format!(
                    ", accel {} g",
                    num_text(round(o.accel.unwrap_or(0.0) / G, 1))
                )
            } else {
                String::new()
            },
            if truthy(o.reaction) {
                format!(", reaction {} s", opt_text(o.reaction))
            } else {
                String::new()
            },
        ));
    }
    let with_radius = target.is_some_and(Target::hit_radius_truthy);
    lines.push(String::new());
    lines.push(format!(
        "  guns                                     n  size speed spread  max range  hull dps  {}",
        if with_radius {
            levels
                .iter()
                .map(|l| pad_start(&format!("{l}% hits"), 9))
                .collect::<String>()
        } else {
            String::new()
        }
    ));
    for g in &env.guns {
        let ranges = match &g.range_for {
            Some(r) => levels
                .iter()
                .map(|l| {
                    let v = lookup(r, l).map_or("undefined".into(), num_text);
                    pad_start(&format!("{v} m"), 9)
                })
                .collect::<String>(),
            None => String::new(),
        };
        lines.push(format!(
            "  {} {}  S{}{} {} {} {}  {ranges}{}{}",
            js_pad_end(js_slice(&g.name, 40), 40),
            pad_start(&num_text(g.count), 2),
            js_pad_end(&g.size.text_or("?"), 3),
            pad_start(&g.speed.text_or(""), 5),
            pad_start(&num_text(g.spread), 6),
            pad_start(&g.max_range.text_or(""), 10),
            pad_start(&num_text(g.applied_dps), 9),
            if g.penetrates { "" } else { "  deflected" },
            g.falloff_range
                .filter(|f| *f != 0.0 && !f.is_nan())
                .map(|f| format!("  falloff to {} m", num_text(f)))
                .unwrap_or_default(),
        ));
    }
    lines.push(String::new());
    lines.push("  distance  guns  listed dps  hull dps  hit chance  expected dps  flight s".into());
    for b in &env.bands {
        lines.push(format!(
            "  {} m {} {} {} {} {}  {}",
            pad_start(&num_text(b.distance), 7),
            pad_start(&num_text(b.guns_in_range), 4),
            pad_start(&num_text(b.dps), 11),
            pad_start(&num_text(b.applied_dps), 9),
            pad_start(
                &b.hit_chance
                    .map(|h| format!("{}%", num_text(round(h * 100.0, 1))))
                    .unwrap_or_default(),
                11
            ),
            pad_start(&num_text(b.expected_dps), 13),
            b.flight_time
                .map(|(min, max)| format!("{}-{}", num_text(min), num_text(max)))
                .unwrap_or_default(),
        ));
    }
    lines.push(String::new());
    lines.push(format!(
        "  point-blank hull dps   {}",
        num_text(env.applied_dps)
    ));
    if let Some(e) = &env.effective_range {
        for l in &levels {
            lines.push(format!(
                "  {}% of it lands to    {} m",
                pad_start(l, 3),
                lookup(e, l).map_or("undefined".into(), num_text)
            ));
        }
    }
    if let Some(r) = env.all_guns_range {
        lines.push(format!("  all guns reach         {} m", num_text(r)));
    }
    if let Some(r) = env.max_range {
        lines.push(format!("  any damage to          {} m", num_text(r)));
    }
    if target.is_some() && env.applied_dps == 0.0 {
        lines.push(String::new());
        lines.push(
            "  Nothing in this loadout gets through the target's armour, so there is no effective range."
                .into(),
        );
    } else if let Some(table) = table.filter(|t| !t.is_empty()) {
        lines.push(String::new());
        lines.push(
            "  effective range by target jink (share of point-blank hull dps that lands)".into(),
        );
        lines.push(format!(
            "  {}{}",
            js_pad_end("jink", 8),
            levels
                .iter()
                .map(|l| pad_start(&format!("{l}%"), 9))
                .collect::<String>()
        ));
        for row in table {
            lines.push(format!(
                "  {}{}",
                js_pad_end(&format!("{} g", num_text(row.accel_g)), 8),
                levels
                    .iter()
                    .map(|l| pad_start(
                        &format!(
                            "{} m",
                            lookup(&row.ranges, l).map(num_text).unwrap_or_default()
                        ),
                        9
                    ))
                    .collect::<String>()
            ));
        }
        lines.push(
            "  0 g is a target holding course: only spread limits hits. Jink needs hull mass, not in game data."
                .into(),
        );
    }
    if target.is_none() {
        lines.push(String::new());
        lines.push("  No target: hit chance needs one (--target), for its speed and size.".into());
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hammerhead() -> Value {
        json!({
            "item": "ARMR_AEGS_Hammerhead", "health": 25740,
            "deflection": {"physical": 531, "energy": 380},
            "damageMultiplier": {"physical": 0.7, "energy": 0.5},
        })
    }

    fn gun(
        name: &str,
        damage: Value,
        fire_rate: f64,
        speed: f64,
        range: f64,
        extra: Value,
    ) -> Value {
        let total: f64 = damage
            .as_object()
            .unwrap()
            .values()
            .map(|v| v.as_f64().unwrap())
            .sum();
        let mut g = json!({
            "name": name, "recordName": format!("EntityClassDefinition.{name}"), "id": name,
            "class": "ship", "size": 5, "fireRate": fire_rate, "speed": speed, "range": range,
            "dps": total * (fire_rate / 60.0),
            "ammo": {"recordName": format!("AmmoParams.{name}"), "damage": damage}, "modes": [],
        });
        for (k, v) in extra.as_object().unwrap() {
            g[k] = v.clone();
        }
        g
    }

    fn m7a() -> Value {
        gun(
            "BEHR_LaserCannon_S5",
            json!({"energy": 921.78}),
            100.0,
            1112.0,
            4003.0,
            json!({"spread": 0.4}),
        )
    }

    fn ad5b() -> Value {
        gun(
            "BEHR_BallisticGatling_S5",
            json!({"physical": 102.5}),
            900.0,
            1300.0,
            4003.0,
            json!({}),
        )
    }

    fn close(a: f64, b: f64, digits: i32) -> bool {
        (a - b).abs() < 10f64.powi(-digits) / 2.0
    }

    fn armoured(radius: Option<f64>, speed: Option<f64>) -> Target {
        Target {
            name: "Hammerhead".into(),
            armor: Some(hammerhead()),
            hit_radius: radius,
            speed: J::from_opt(speed),
        }
    }

    fn steady(speed: f64) -> Target {
        Target {
            name: "x".into(),
            armor: None,
            hit_radius: Some(6.0),
            speed: J::V(speed),
        }
    }

    #[test]
    fn falloff_holds_then_drops_to_the_floor() {
        let w = json!({"ammo": {"damage": {"physical": 18},
            "falloff": {"physical": {"minDistance": 14, "perMeter": 0.1, "floor": 15}}}});
        let at = |d: f64| damage_at(&w, d)[0].1;
        assert_eq!(at(10.0), J::V(18.0));
        assert_eq!(at(24.0), J::V(17.0));
        assert_eq!(at(500.0), J::V(15.0));
        assert_eq!(falloff_range(&w), Some(44.0));
        assert_eq!(
            falloff_range(&json!({"ammo": {"damage": {"physical": 18}}})),
            None
        );
    }

    #[test]
    fn applied_dps_applies_deflection_multipliers_and_range() {
        let t = armoured(None, None);
        assert!(close(
            applied_dps_at(&m7a(), Some(&t), 0.0),
            921.78 * 0.5 * (100.0 / 60.0),
            2
        ));
        assert_eq!(applied_dps_at(&ad5b(), Some(&t), 0.0), 0.0);
        assert_eq!(applied_dps_at(&m7a(), Some(&t), 4004.0), 0.0);
        let dps = m7a()["dps"].as_f64().unwrap();
        assert!(close(applied_dps_at(&m7a(), None, 1000.0), dps, 2));
    }

    #[test]
    fn hit_chance_model() {
        let o = EngageOptions::default();
        let s = steady(200.0);
        assert_eq!(hit_chance(&m7a(), Some(&s), 100.0, &o), 1.0);
        let scatter = 2000.0 * (0.4 * std::f64::consts::PI / 180.0).tan();
        assert!(close(miss_radius(&m7a(), Some(&s), 2000.0, &o), scatter, 2));
        assert!(close(
            hit_chance(&m7a(), Some(&s), 2000.0, &o),
            (6.0 / scatter).powi(2),
            2
        ));
        let t = 1000.0 / 1112.0;
        let tan = 1000.0 * (0.4 * std::f64::consts::PI / 180.0).tan();
        let jink = EngageOptions {
            accel: Some(6.0 * G),
            ..Default::default()
        };
        assert!(close(
            miss_radius(&m7a(), Some(&s), 1000.0, &jink),
            0.5 * 6.0 * G * t * t + tan,
            2
        ));
        let hard = EngageOptions {
            accel: Some(50.0 * G),
            ..Default::default()
        };
        assert!(close(
            miss_radius(&m7a(), Some(&steady(10.0)), 1000.0, &hard),
            10.0 * t + tan,
            2
        ));
        let closing = EngageOptions {
            closing: Some(200.0),
            ..jink.clone()
        };
        assert!(
            miss_radius(&m7a(), Some(&s), 1000.0, &closing)
                < miss_radius(&m7a(), Some(&s), 1000.0, &jink)
        );
        assert_eq!(hit_chance(&m7a(), Some(&s), 4004.0, &o), 0.0);
        let d = range_for_chance(&m7a(), Some(&s), 0.5, &o);
        assert!(hit_chance(&m7a(), Some(&s), d, &o) >= 0.5);
        assert!(hit_chance(&m7a(), Some(&s), d + 5.0, &o) < 0.5);
    }

    #[test]
    fn envelope_sums_and_reports_ranges() {
        let (m, a, short) = (
            m7a(),
            ad5b(),
            gun(
                "Short",
                json!({"energy": 500}),
                60.0,
                1000.0,
                1500.0,
                json!({}),
            ),
        );
        let target = armoured(Some(10.0), Some(150.0));
        let loadout = [
            Gun {
                weapon: &m,
                count: 2.0,
                mount: None,
            },
            Gun {
                weapon: &a,
                count: 2.0,
                mount: None,
            },
            Gun {
                weapon: &short,
                count: 1.0,
                mount: None,
            },
        ];
        let o = EngageOptions {
            bands: Some(vec![0.0, 1000.0, 2000.0]),
            ..Default::default()
        };
        let env = envelope(&loadout, Some(&target), &o);
        let at = |d: f64| env.bands.iter().find(|b| b.distance == d).unwrap();
        assert!(close(at(0.0).applied_dps, 1536.3 + 250.0, 0));
        assert!(close(at(2000.0).applied_dps, 1536.3, 0));
        assert_eq!(at(2000.0).guns_in_range, 4.0);
        assert!(
            !env.guns
                .iter()
                .find(|g| g.record == "BEHR_BallisticGatling_S5")
                .unwrap()
                .penetrates
        );
        assert_eq!(env.all_guns_range, Some(1500.0));
        assert_eq!(env.max_range, Some(4003.0));
        assert_eq!(env.half_dps_range, Some(4003.0));
        assert!(lookup(env.effective_range.as_ref().unwrap(), "50").unwrap() > 0.0);

        let one = [Gun {
            weapon: &m,
            count: 1.0,
            mount: None,
        }];
        let steady = envelope(
            &one,
            Some(&target),
            &EngageOptions {
                bands: Some(vec![0.0, 2000.0]),
                ..Default::default()
            },
        );
        let far = steady.bands.iter().find(|b| b.distance == 2000.0).unwrap();
        let expected =
            far.applied_dps * hit_chance(&m, Some(&target), 2000.0, &EngageOptions::default());
        assert!(close(far.expected_dps, expected, 1));
        let table = range_table(&one, &target, &[0.0, 6.0], &EngageOptions::default());
        assert!(lookup(&table[1].ranges, "50").unwrap() < lookup(&table[0].ranges, "50").unwrap());
    }

    #[test]
    fn target_of_uses_the_mean_projected_area_and_scm_speed() {
        let t = target_of(
            &json!({"name": "Gladius", "boundingBox": {"x": 17.5, "y": 21, "z": 5.5}, "flight": {"scmSpeed": 226}}),
        );
        let r = ((17.5 * 5.5 + 21.0 * 5.5 + 17.5 * 21.0) / 3.0 / std::f64::consts::PI).sqrt();
        assert!(close(t.hit_radius.unwrap(), r, 2));
        assert_eq!(t.speed, J::V(226.0));
    }
}
