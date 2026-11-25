//! What quality a mined or gathered resource comes in (`game-commodities` `resources[].quality`).
//!
//! Quality runs 0-1000. A resource rolls it from a curve (min, max, mean, spread), some
//! places override that curve (`Pyro System`), and the roll is then snapped to one of a few
//! shown values by quantization bands: Aphorite rolled anywhere in 201-399 shows as 348.
//! [`OreQuality::steps`] lists the shown values a roll can reach, each with an estimated
//! share. The share assumes a normal curve cut to min-max; the records name the curve's
//! numbers but not how the game samples it.

use super::mined::ore_core;
use serde::{Deserialize, Serialize};

/// The curve numbers, as the dataset writes them.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct QualityRange {
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub mean: Option<f64>,
    pub stddev: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(default)]
pub struct BandIn {
    pub start: Option<f64>,
    pub end: Option<f64>,
    pub value: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(default)]
pub struct OverrideIn {
    pub location: String,
    #[serde(flatten)]
    pub range: QualityRange,
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct QualityIn {
    pub tier: Option<String>,
    pub distribution: Option<QualityRange>,
    pub location_overrides: Vec<OverrideIn>,
    pub bands: Vec<BandIn>,
}

/// One `game-commodities` `resources` row, as far as this reads it.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct QualityResource {
    pub name: String,
    pub record_name: String,
    pub display_name: Option<String>,
    pub quality: Option<QualityIn>,
}

/// One `game-commodities` `items` row (a cargo entity), as far as this reads it.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CommodityItem {
    /// The entity class (`Procyon_canister_damage`).
    pub name: String,
    /// The resource it holds (`ProcyonCanister_Damage`).
    pub resource: Option<String>,
}

/// The parts of `game-commodities` this reads.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(default)]
pub struct Commodities {
    pub resources: Vec<QualityResource>,
    pub items: Vec<CommodityItem>,
}

impl Commodities {
    /// The resource a record is or holds: `ResourceType.Laranite` or `Laranite`, else a
    /// cargo entity (`EntityClassDefinition.<class>`) holding one.
    pub fn resource_of(&self, record: &str) -> Option<&QualityResource> {
        let short = record.rsplit('.').next().unwrap_or(record).trim();
        let by_name = |n: &str| {
            self.resources.iter().find(|r| {
                r.record_name.eq_ignore_ascii_case(record) || r.name.eq_ignore_ascii_case(n)
            })
        };
        by_name(short).or_else(|| {
            let item = self
                .items
                .iter()
                .find(|i| i.name.eq_ignore_ascii_case(short))?;
            by_name(item.resource.as_deref()?)
        })
    }
}

/// A place whose curve differs from the resource's own.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct QualityOverride {
    pub location: String,
    pub range: QualityRange,
}

/// A value the game shows: every roll in `start..=end` shows as `shown`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct QualityStep {
    pub start: f64,
    pub end: f64,
    pub shown: f64,
    /// Estimated share of rolls (0..1) on the resource's own curve; `None` without a spread.
    pub share: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct OreQuality {
    /// The resource (`ResourceType.Aphorite`).
    pub resource: String,
    /// Display name, else the resource name.
    pub name: String,
    /// The curve family (`FPSMineable`, `Rare`, `Gatherable`).
    pub tier: Option<String>,
    pub range: QualityRange,
    pub overrides: Vec<QualityOverride>,
    /// Lowest first; only steps a roll on the resource's curve can reach.
    pub steps: Vec<QualityStep>,
}

/// "Raw_Quantainium", "Ore_Gold", "RawOuratite" -> the ore core [`ore_core`] gives mined ores.
fn resource_core(name: &str) -> String {
    let lower = name.to_lowercase();
    let s = ["raw_", "ore_", "raw"]
        .iter()
        .find_map(|p| lower.strip_prefix(p).filter(|r| !r.is_empty()))
        .unwrap_or(&lower);
    ore_core(s)
}

/// Abramowitz-Stegun 7.1.26 (error below 1.5e-7).
pub(crate) fn erf(x: f64) -> f64 {
    let t = 1.0 / (1.0 + 0.327_591_1 * x.abs());
    let y = 1.0
        - (((((1.061_405_429 * t - 1.453_152_027) * t) + 1.421_413_741) * t - 0.284_496_736) * t
            + 0.254_829_592)
            * t
            * (-x * x).exp();
    if x < 0.0 { -y } else { y }
}

pub(crate) fn phi(x: f64, mean: f64, sd: f64) -> f64 {
    0.5 * (1.0 + erf((x - mean) / (sd * std::f64::consts::SQRT_2)))
}

/// Shown values a roll in `min..=max` reaches, with estimated shares.
fn steps(bands: &[BandIn], r: &QualityRange) -> Vec<QualityStep> {
    let (lo, hi) = (r.min.unwrap_or(0.0), r.max.unwrap_or(1000.0));
    // Integer rolls: a band covers start-0.5 .. end+0.5.
    let mass = |a: f64, b: f64| match (r.mean, r.stddev) {
        (Some(m), Some(sd)) if sd > 0.0 => Some(phi(b + 0.5, m, sd) - phi(a - 0.5, m, sd)),
        _ => None,
    };
    let total = mass(lo, hi).filter(|t| *t > 0.0);
    let mut out: Vec<QualityStep> = bands
        .iter()
        .filter_map(|b| {
            let (start, end, shown) = (b.start?, b.end?, b.value?);
            let (a, z) = (start.max(lo), end.min(hi));
            (a <= z).then(|| QualityStep {
                start,
                end,
                shown,
                share: total.and_then(|t| Some(mass(a, z)? / t)),
            })
        })
        .collect();
    out.sort_by(|a, b| a.start.total_cmp(&b.start));
    out
}

/// The quality report of one resource with a quality curve.
pub fn report(r: &QualityResource, q: &QualityIn) -> OreQuality {
    let range = q.distribution.clone().unwrap_or_default();
    OreQuality {
        resource: r.record_name.clone(),
        name: r.display_name.clone().unwrap_or_else(|| r.name.clone()),
        tier: q.tier.clone(),
        steps: steps(&q.bands, &range),
        range,
        overrides: q
            .location_overrides
            .iter()
            .map(|o| QualityOverride {
                location: o.location.clone(),
                range: o.range.clone(),
            })
            .collect(),
    }
}

/// Quality of the record itself when it is a resource with a quality curve
/// (`ResourceType.Aphorite`), else of the resources behind the mined ores `ores` names
/// (`minableelement_fps_aphorite`, `quantainium_raw`).
pub fn quality_for(c: &Commodities, record: &str, ores: &[String]) -> Vec<OreQuality> {
    let with: Vec<(&QualityResource, &QualityIn)> = c
        .resources
        .iter()
        .filter_map(|r| Some((r, r.quality.as_ref()?)))
        .collect();
    let short = record.rsplit('.').next().unwrap_or(record);
    if let Some((r, q)) = with.iter().find(|(r, _)| {
        r.record_name.eq_ignore_ascii_case(record) || r.name.eq_ignore_ascii_case(short)
    }) {
        return vec![report(r, q)];
    }
    let mut out: Vec<OreQuality> = Vec::new();
    for ore in ores {
        let core = ore_core(ore);
        if let Some((r, q)) = with.iter().find(|(r, _)| resource_core(&r.name) == core)
            && !out.iter().any(|o| o.resource == r.record_name)
        {
            out.push(report(r, q));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data() -> Commodities {
        serde_json::from_value(serde_json::json!({"resources": [
            {"name": "Aphorite", "recordName": "ResourceType.Aphorite", "quality": {
                "tier": "FPSMineable",
                "distribution": {"min": 201, "max": 1000, "mean": 201, "stddev": 298},
                "locationOverrides": [{"location": "Pyro System", "min": 201, "max": 1000, "mean": 209, "stddev": 308}],
                "bands": [{"start": 0, "end": 399, "value": 348}, {"start": 400, "end": 599, "value": 523},
                          {"start": 600, "end": 1000, "value": 800}]}},
            {"name": "Raw_Quantainium", "recordName": "ResourceType.Raw_Quantainium", "displayName": "Quantainium (Raw)",
             "quality": {"tier": "Legendary", "distribution": {"min": 0, "max": 500}, "bands": [
                {"start": 0, "end": 499, "value": 250}, {"start": 500, "end": 999, "value": 750}]}},
            {"name": "Gold", "recordName": "ResourceType.Gold"}
        ]}))
        .unwrap()
    }

    #[test]
    fn the_record_itself_with_steps_and_shares() {
        let q = quality_for(&data(), "ResourceType.Aphorite", &[]);
        assert_eq!(q.len(), 1);
        let a = &q[0];
        assert_eq!(a.tier.as_deref(), Some("FPSMineable"));
        assert_eq!(a.overrides[0].location, "Pyro System");
        assert_eq!(a.overrides[0].range.mean, Some(209.0));
        let shown: Vec<f64> = a.steps.iter().map(|s| s.shown).collect();
        assert_eq!(shown, [348.0, 523.0, 800.0]);
        let sum: f64 = a.steps.iter().filter_map(|s| s.share).sum();
        assert!((sum - 1.0).abs() < 1e-6, "{sum}");
        // A curve peaking at its minimum: the lowest step is the likeliest.
        let s = &a.steps;
        assert!(s[0].share > s[1].share && s[1].share > s[2].share);
        assert!(
            (s[0].share.unwrap() - 0.497).abs() < 0.01,
            "{:?}",
            s[0].share
        );
    }

    #[test]
    fn records_name_their_resource_or_the_one_their_cargo_holds() {
        let mut c = data();
        c.items = vec![CommodityItem {
            name: "Gold_Cargo_1SCU".into(),
            resource: Some("Gold".into()),
        }];
        let of = |r: &str| c.resource_of(r).map(|x| x.name.clone());
        assert_eq!(of("ResourceType.Aphorite").as_deref(), Some("Aphorite"));
        assert_eq!(of("raw_quantainium").as_deref(), Some("Raw_Quantainium"));
        assert_eq!(
            of("EntityClassDefinition.gold_cargo_1scu").as_deref(),
            Some("Gold")
        );
        assert_eq!(of("EntityClassDefinition.rifle"), None);
    }

    #[test]
    fn mined_ores_map_to_their_raw_resource() {
        let q = quality_for(
            &data(),
            "EntityClassDefinition.rock",
            &["quantainium_raw".into()],
        );
        assert_eq!(q[0].name, "Quantainium (Raw)");
        // Only steps a roll in 0..500 reaches; no spread, so no share.
        assert_eq!(q[0].steps.len(), 2);
        assert_eq!(q[0].steps[1].shown, 750.0);
        assert!(q[0].steps[0].share.is_none());
        let fps = quality_for(
            &data(),
            "x",
            &["minableelement_fps_aphorite".into(), "aphorite_ore".into()],
        );
        assert_eq!(fps.len(), 1);
        assert!(quality_for(&data(), "ResourceType.Gold", &[]).is_empty());
    }
}
