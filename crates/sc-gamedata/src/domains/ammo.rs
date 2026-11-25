//! Ammunition, which is where weapon damage actually lives (port of
//! `sc-gamedata/src/domains/ammo.ts`).
//!
//! A weapon record carries fire rate and heat but no damage figure. The damage is on the
//! ammo it fires, at `projectileParams.damage`, as a `DamageInfo` split across the damage
//! types. `DeathExplosionParams.damage` on weapon records is the explosion when the weapon
//! itself is destroyed, not the weapon's damage.

use crate::collate::locale_compare;
use crate::domain::{Domain, DomainError, ParseContext, field};
use crate::helpers_items::{round_to, truthy};
use crate::records::{GameRecord, at, num};
use serde::Serialize;
use serde_json::{Map, Value};

pub static DOMAIN: AmmoDomain = AmmoDomain;

pub struct AmmoDomain;

/// `DamageInfo`: only non-zero types, in [`DAMAGE_FIELDS`] order.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct DamageInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub physical: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub energy: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub distortion: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thermal: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub biochemical: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stun: Option<f64>,
}

impl DamageInfo {
    fn slot(&mut self, key: &str) -> &mut Option<f64> {
        match key {
            "physical" => &mut self.physical,
            "energy" => &mut self.energy,
            "distortion" => &mut self.distortion,
            "thermal" => &mut self.thermal,
            "biochemical" => &mut self.biochemical,
            _ => &mut self.stun,
        }
    }

    /// `Object.entries(damage)`: the set types, in field order.
    pub fn entries(&self) -> Vec<(&'static str, f64)> {
        [
            ("physical", self.physical),
            ("energy", self.energy),
            ("distortion", self.distortion),
            ("thermal", self.thermal),
            ("biochemical", self.biochemical),
            ("stun", self.stun),
        ]
        .into_iter()
        .filter_map(|(k, v)| v.map(|v| (k, v)))
        .collect()
    }

    /// `Object.values(damage).reduce((sum, x) => sum + x, 0)`.
    pub fn sum(&self) -> f64 {
        self.entries().iter().fold(0.0, |sum, (_, x)| sum + x)
    }

    fn is_empty(&self) -> bool {
        self.entries().is_empty()
    }

    /// Damage from a `{DamagePhysical, ...}` struct, keeping the amounts `keep` accepts.
    pub fn from_fields(value: Option<&Value>, keep: impl Fn(f64) -> bool) -> Option<Self> {
        let obj = value?.as_object()?;
        let mut damage = DamageInfo::default();
        for (key, field) in DAMAGE_FIELDS {
            if let Some(amount) = num(obj.get(field))
                && keep(amount)
            {
                *damage.slot(key) = Some(amount);
            }
        }
        (!damage.is_empty()).then_some(damage)
    }
}

/// The damage types the game models: output key and DataCore field.
pub const DAMAGE_FIELDS: [(&str, &str); 6] = [
    ("physical", "DamagePhysical"),
    ("energy", "DamageEnergy"),
    ("distortion", "DamageDistortion"),
    ("thermal", "DamageThermal"),
    ("biochemical", "DamageBiochemical"),
    ("stun", "DamageStun"),
];

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PenetrationRadius {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub near: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub far: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Falloff {
    pub min_distance: f64,
    pub per_meter: f64,
    pub floor: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Pierce {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_thickness: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level1: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level2: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level3: Option<f64>,
}

/// One row of `ammo`. Field order is output order.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Ammo {
    pub name: String,
    pub record_name: String,
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lifetime: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub damage: Option<DamageInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub damage_total: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub penetration: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub penetration_radius: Option<PenetrationRadius>,
    /// Per damage type, in [`DAMAGE_FIELDS`] order.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub falloff: Option<Map<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pierce: Option<Pierce>,
}

/// `parseDamageInfo`: the non-zero damage types of a `DamageInfo`.
pub fn parse_damage_info(value: Option<&Value>) -> Option<DamageInfo> {
    DamageInfo::from_fields(value, |amount| amount != 0.0)
}

/// `findDamage`: the first non-zero `DamageInfo` anywhere under a value.
pub fn find_damage(value: Option<&Value>) -> Option<DamageInfo> {
    match value? {
        Value::Array(list) => list.iter().find_map(|v| find_damage(Some(v))),
        Value::Object(obj) => {
            if obj.get("_Type_").and_then(Value::as_str) == Some("DamageInfo")
                && let Some(d) = parse_damage_info(value)
            {
                return Some(d);
            }
            obj.values().find_map(|v| find_damage(Some(v)))
        }
        _ => None,
    }
}

/// `parseFalloff`: per-type falloff from `BulletDamageDropParams`, only types that fall off.
pub fn parse_falloff(value: Option<&Value>) -> Option<Map<String, Value>> {
    let v = value.filter(|v| v.is_object() || v.is_array())?;
    let mut out = Map::new();
    for (key, field) in DAMAGE_FIELDS {
        let per_meter = num(at(v, &format!("damageDropPerMeter.{field}"))).unwrap_or(0.0);
        if per_meter <= 0.0 {
            continue;
        }
        let falloff = Falloff {
            min_distance: num(at(v, &format!("damageDropMinDistance.{field}"))).unwrap_or(0.0),
            per_meter,
            floor: num(at(v, &format!("damageDropMinDamage.{field}"))).unwrap_or(0.0),
        };
        out.insert(
            key.to_string(),
            serde_json::to_value(falloff).unwrap_or(Value::Null),
        );
    }
    (!out.is_empty()).then_some(out)
}

fn parse_pierce(value: Option<&Value>) -> Option<Pierce> {
    let v = value.filter(|v| v.is_object() || v.is_array())?;
    let pierce = Pierce {
        max_thickness: num(at(v, "maxPenetrationThickness")),
        level1: num(at(v, "damageFalloffLevel1")),
        level2: num(at(v, "damageFalloffLevel2")),
        level3: num(at(v, "damageFalloffLevel3")),
    };
    // `Object.values(pierce).some((v) => v)`: zero does not count.
    [
        pierce.max_thickness,
        pierce.level1,
        pierce.level2,
        pierce.level3,
    ]
    .into_iter()
    .any(truthy)
    .then_some(pierce)
}

pub fn parse_ammo(record: &GameRecord) -> Ammo {
    let v = &record.value;
    let damage = find_damage(v.get("projectileParams")).or_else(|| find_damage(Some(v)));
    let speed = num(v.get("speed"));
    let lifetime = num(v.get("lifetime"));
    let total = damage.as_ref().map(|d| round_to(d.sum(), 100.0));
    let range = match (speed, lifetime) {
        (Some(s), Some(l)) if truthy(Some(s)) && truthy(Some(l)) => {
            Some(crate::helpers_items::js_round(s * l))
        }
        _ => None,
    };
    let near = num(at(v, "projectileParams.penetrationParams.nearRadius"));
    let far = num(at(v, "projectileParams.penetrationParams.farRadius"));
    Ammo {
        name: record.short_name.clone(),
        record_name: record.name.clone(),
        id: record.id.clone(),
        size: num(v.get("size")),
        speed,
        lifetime,
        range,
        damage,
        damage_total: total,
        penetration: num(at(
            v,
            "projectileParams.penetrationParams.basePenetrationDistance",
        )),
        penetration_radius: (near.is_some() || far.is_some())
            .then_some(PenetrationRadius { near, far }),
        falloff: parse_falloff(at(v, "projectileParams.damageDropParams")),
        pierce: parse_pierce(at(v, "projectileParams.pierceabilityParams")),
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Summary {
    ammo: usize,
    with_damage: usize,
    max_damage: f64,
}

impl Domain for AmmoDomain {
    fn name(&self) -> &'static str {
        "game-ammo"
    }

    fn description(&self) -> &'static str {
        "Ammunition damage, projectile speed and range. Join to weapons for DPS."
    }

    fn filters(&self) -> &'static [&'static str] {
        &["**/ammoparams/**"]
    }

    fn parse(&self, ctx: &ParseContext<'_>) -> Result<Map<String, Value>, DomainError> {
        let mut ammo: Vec<Ammo> = ctx.records.of_type("AmmoParams").map(parse_ammo).collect();
        ammo.sort_by(|a, b| locale_compare(&a.name, &b.name));

        let damaging: Vec<&Ammo> = ammo
            .iter()
            .filter(|a| a.damage_total.unwrap_or(0.0) > 0.0)
            .collect();
        let max_damage = damaging
            .iter()
            .fold(0.0, |max: f64, a| max.max(a.damage_total.unwrap_or(0.0)));

        let mut body = Map::new();
        field(&mut body, "ammo", &ammo)?;
        field(
            &mut body,
            "summary",
            &Summary {
                ammo: ammo.len(),
                with_damage: damaging.len(),
                max_damage,
            },
        )?;
        Ok(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::NoFiles;
    use crate::localization::Localization;
    use crate::records::{RecordSet, normalize_record};
    use serde_json::json;

    fn rec(ty: &str, short: &str, value: Value) -> GameRecord {
        let mut v = json!({"_Type_": ty});
        if let (Some(o), Value::Object(extra)) = (v.as_object_mut(), value) {
            o.extend(extra);
        }
        normalize_record(
            json!({"_RecordName_": format!("{ty}.{short}"), "_RecordId_": format!("id-{short}"), "_RecordValue_": v}),
            &format!("libs/foundry/records/x/{}", short.to_lowercase()),
        )
        .unwrap()
    }

    // Cases from sc-gamedata src/domains/new-domains.test.ts ("ammo damage").
    #[test]
    fn finds_damage_nested_under_projectile_params() {
        let d = find_damage(Some(&json!({"projectileParams": {"damage":
            {"_Type_": "DamageInfo", "DamagePhysical": 52.7, "DamageEnergy": 0}}})))
        .unwrap();
        assert_eq!(serde_json::to_value(d).unwrap(), json!({"physical": 52.7}));
    }

    #[test]
    fn ignores_an_all_zero_damage_info() {
        assert!(
            find_damage(Some(&json!({"DeathExplosionParams": {"damage":
                {"_Type_": "DamageInfo", "DamagePhysical": 0}}})))
            .is_none()
        );
    }

    #[test]
    fn derives_range_from_speed_and_lifetime() {
        let a = parse_ammo(&rec(
            "AmmoParams",
            "Test",
            json!({"speed": 1000, "lifetime": 4, "projectileParams": {"damage":
                {"_Type_": "DamageInfo", "DamagePhysical": 10, "DamageEnergy": 5}}}),
        ));
        assert_eq!(a.range, Some(4000.0));
        assert_eq!(a.damage_total, Some(15.0));
    }

    #[test]
    fn counts_only_ammo_that_deals_damage() {
        let set = RecordSet::new(vec![
            rec(
                "AmmoParams",
                "A",
                json!({"projectileParams": {"damage": {"_Type_": "DamageInfo", "DamagePhysical": 10}}}),
            ),
            rec("AmmoParams", "B", json!({})),
        ]);
        let body = DOMAIN
            .parse(&ParseContext::new(&set, &Localization::new(), &NoFiles))
            .unwrap();
        assert_eq!(
            body["summary"],
            json!({"ammo": 2, "withDamage": 1, "maxDamage": 10.0})
        );
    }

    #[test]
    fn falloff_pierce_and_penetration_radius() {
        let a = parse_ammo(&rec(
            "AmmoParams",
            "F",
            json!({"projectileParams": {
                "penetrationParams": {"basePenetrationDistance": 0.5, "nearRadius": 0.1},
                "damageDropParams": {
                    "damageDropPerMeter": {"DamagePhysical": 0.2, "DamageEnergy": 0},
                    "damageDropMinDistance": {"DamagePhysical": 30},
                    "damageDropMinDamage": {"DamagePhysical": 4}},
                "pierceabilityParams": {"maxPenetrationThickness": 0, "damageFalloffLevel1": 0}}}),
        ));
        let v = serde_json::to_value(&a).unwrap();
        assert_eq!(v["penetrationRadius"], json!({"near": 0.1}));
        assert_eq!(
            v["falloff"],
            json!({"physical": {"minDistance": 30.0, "perMeter": 0.2, "floor": 4.0}})
        );
        assert!(a.pierce.is_none(), "all-zero pierce is dropped");
        assert!(a.range.is_none());
    }
}
