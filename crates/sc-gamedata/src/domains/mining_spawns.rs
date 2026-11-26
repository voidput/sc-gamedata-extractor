//! Where ores actually spawn (port of `sc-gamedata/src/domains/mining-spawns.ts`).
//!
//! No single record connects a place to an ore, so four record types are walked:
//!
//! ```text
//! HarvestableProviderPreset   HPP_Stanton1, one per body or belt
//!   -> HarvestableElement       a spawn entry with a relative probability
//!     -> HarvestablePreset      Mining_Common_Iron
//!       -> EntityClassDefinition  MineableRock_SurfaceCommon_Iron
//!         -> MineableComposition  CommonShipMineables_Iron
//!           -> MineableElement    iron_ore at 34.3-84.3%
//! ```
//!
//! Provider presets are named after the location they cover with internal keys
//! (`Stanton1`, `Stanton1c`) that are themselves localization keys; keys without an entry
//! stay raw. A rock's RS signature is slot 4 of its radar `signatures`; the composition
//! name carries the deposit tier for tiered ship rocks.
//!
//! `ores` is the same data per ore key ([`ore_core`]: `MinableElement_FPS_Sadaryx` and
//! `minableelement_groundvehicle_carinite` reduce to `sadaryx` and `carinite`): the
//! element keys and `ResourceType`s it covers, the belts that roll it (one row per
//! location and rock, the spawn groups summed), and the hand-placed spots, slot presets
//! (`game-item-sources` `areas`) whose slots place a rock holding it. Some ores have no
//! spawn table at all (Sadaryx): the spots are then the only places.

use crate::collate::compare_names;
use crate::domain::{Domain, DomainError, ParseContext, field};
use crate::domains::item_sources::{LootArea, loot_areas};
use crate::domains::starmap::{Location, build_locations};
use crate::query::mined::{area_place, ore_core};
use crate::entity::component;
use crate::helpers_world::{js_round, last_segment};
use crate::localization::{Localization, localize, lookup_string};
use crate::output::js_key_order;
use crate::records::{GameRecord, RecordSet, array, at, num, ref_short_name, str_};
use serde::Serialize;
use serde_json::{Map, Value, json};
use std::collections::HashMap;

pub static DOMAIN: MiningSpawns = MiningSpawns;

pub struct MiningSpawns;

/// `describeProvider` result.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Provider {
    pub key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
}

/// `/^([a-z]+)\d+[a-z]*$/i`: the leading letters of a key like `Stanton1c`.
fn letters_digits_letters(key: &str) -> Option<&str> {
    let b = key.as_bytes();
    let letters = b.iter().take_while(|c| c.is_ascii_alphabetic()).count();
    let digits = b[letters..].iter().take_while(|c| c.is_ascii_digit()).count();
    let tail = &b[letters + digits..];
    (letters > 0 && digits > 0 && tail.iter().all(u8::is_ascii_alphabetic)).then(|| &key[..letters])
}

/// `system.charAt(0).toUpperCase() + system.slice(1).toLowerCase()`.
fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase(),
        None => String::new(),
    }
}

/// `describeProvider`: "HPP_Stanton1c" -> key Stanton1c, system Stanton, body Magda.
pub fn describe_provider(short_name: &str, localization: &Localization) -> Provider {
    let key = match short_name.get(..4) {
        Some(p) if p.eq_ignore_ascii_case("HPP_") => &short_name[4..],
        _ => short_name,
    };
    let body = lookup_string(localization, key).filter(|b| !b.is_empty());
    let system = letters_digits_letters(key).unwrap_or_else(|| key.split('_').next().unwrap_or(""));
    Provider {
        key: key.to_string(),
        system: (!system.is_empty()).then(|| capitalize(system)),
        body: body.map(str::to_string),
    }
}

/// `refFile`: the file name a `file://...json` reference ends with, minus the suffix,
/// lower-cased.
fn ref_file(value: Option<&Value>) -> Option<String> {
    let path = str_(value)?;
    let file = last_segment(path);
    if file.is_empty() {
        return None;
    }
    let stem = match file.len().checked_sub(5).and_then(|cut| file.get(cut..).map(|e| (cut, e))) {
        Some((cut, ext)) if ext.eq_ignore_ascii_case(".json") => &file[..cut],
        _ => file,
    };
    // An empty result is falsy at every TS call site, so it is no file at all.
    Some(stem.to_lowercase()).filter(|s| !s.is_empty())
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OreShare {
    pub element: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_percent: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_percent: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub probability: Option<f64>,
    /// Multiplier on the rolled quality of this ore in this rock.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality_scale: Option<f64>,
}

/// The rock behind a harvestable, as a scanner and a mining laser see it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RockInfo {
    pub rock: String,
    /// "ship", "vehicle" or "hand".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// "Common" .. "Legendary" for tiered ship rocks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub composition: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deposit: Option<String>,
    /// RS signature shown on scan.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minimum_distinct_elements: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ores: Option<Vec<OreShare>>,
    /// Only on the `rocks` list (`{ ...r, spawnsAt }`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spawns_at: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpawnEntry {
    pub group: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_probability: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relative_probability: Option<f64>,
    /// groupProbability x this entry's share of its group's relative weights.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spawn_share: Option<f64>,
    pub harvestable: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rock: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deposit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ores: Option<Vec<OreShare>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpawnLocation {
    pub key: String,
    pub record_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    /// A readable place: the body, else a name built from the key ([`belt_place`]).
    pub place: String,
    /// "data" when `place` is the body, "heuristic" when it is built from the key.
    pub place_source: &'static str,
    /// The matching star map object, when the key names one (planets, moons).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub starmap_record: Option<String>,
    /// Star map chain above the body.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<Vec<String>>,
    pub entries: Vec<SpawnEntry>,
    /// Every ore reachable at this location.
    pub ores: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct OreSpawn {
    ore: String,
    /// [`ore_core`] of `ore`: one key for an ore's element, rock and resource names.
    ore_key: String,
    /// The element's `ResourceType` (`Raw_Quantainium`).
    #[serde(skip_serializing_if = "Option::is_none")]
    resource: Option<String>,
    location: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    body: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rock: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    signature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    spawn_share: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    min_percent: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_percent: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    quality_scale: Option<f64>,
}

/// A belt or body that rolls an ore: the `oreSpawns` rows of one location and rock.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OreBelt {
    pub location: String,
    pub place: String,
    pub place_source: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    pub rock: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tier: Option<String>,
    /// The rows' `spawnShare` summed; see `_assumptions`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spawn_share: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_percent: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_percent: Option<f64>,
}

/// A slot preset (`game-item-sources` `areas[].name`) whose slots place a rock with the ore.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OreSpot {
    pub area: String,
    /// The rock with the best share in the area.
    pub rock: String,
    /// That rock's weight over its config's slot weights (0-1, rounded to 1e-8).
    pub slot_share: f64,
    /// Named configs of the area that place a rock with the ore.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub configs: Vec<String>,
    /// Where the area is, when the records do not say ([`area_place`], curated).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub place: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub place_source: Option<&'static str>,
}

/// One ore key of `ores`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OreSites {
    /// Mineable element keys with this ore key.
    pub elements: Vec<String>,
    /// Their `ResourceType`s.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub resources: Vec<String>,
    /// Highest `spawnShare` first.
    pub belts: Vec<OreBelt>,
    /// Best `slotShare` first.
    pub spots: Vec<OreSpot>,
}

/// What `ores` rests on that the records do not state.
pub const MINING_ASSUMPTIONS: [&str; 4] = [
    "ores[].belts[].spawnShare sums groupProbability x the rock's share of its group over a location's spawn groups, once per spawn entry. Group probabilities are not normalised across mining kinds (hand groups weigh in the tens, ship groups below 1): compare it only between belts of one kind.",
    "A rock composition can hold one ore in several parts (oreSpawns has a row per part). ores[].belts[].minPercent and maxPercent are the lowest and highest bound over those parts, not their total.",
    "place with placeSource \"heuristic\" is built from the provider key (Lagrange_B -> Lagrange points, Pyro_Cool01 -> Pyro asteroid fields); \"data\" is a localized body name; \"curated\" (spots) is a place the records do not name, known for the slot preset (rockcracker -> Rock Cracker, Nyx).",
    "system is the star map system of the body when the provider key names a star map object. Otherwise it is read from the key's stem when that stem starts with a star system's name (Pyro_Cool01 -> Pyro, Stanton2c_Belt -> Stanton); AaronHalo -> Stanton is curated; keys naming no system (Lagrange_*, AsteroidCluster_*, ResourceRush_*) have none.",
];

/// A mineable element a rock composition can hold, with its `ResourceType`.
#[derive(Debug, Clone, PartialEq)]
pub struct MinedElement {
    /// Lower-cased element file name (`quantainium_raw`), as in `oreSpawns[].ore`.
    pub element: String,
    pub ore_key: String,
    pub resource: Option<String>,
}

/// Every element named by a `MineableComposition`, by name.
fn composition_elements(records: &RecordSet, by_file: &HashMap<String, &GameRecord>) -> Vec<MinedElement> {
    let mut elements: Vec<String> = Vec::new();
    for c in records.of_type("MineableComposition") {
        for o in ores_of_composition(Some(c)).into_iter().flatten() {
            push_unique(&mut elements, &o.element);
        }
    }
    let mut out: Vec<MinedElement> = sorted_names(elements)
        .into_iter()
        .map(|element| {
            let record = by_file
                .get(&format!("{element}.json"))
                .or_else(|| by_file.get(&element));
            MinedElement {
                ore_key: ore_core(&element),
                resource: record
                    .and_then(|r| ref_short_name(r.value.get("resourceType")))
                    .map(str::to_string),
                element,
            }
        })
        .collect();
    out.dedup_by(|a, b| a.element == b.element);
    out
}

/// [`composition_elements`] of a record set (`game-commodities` `minedAs`).
pub fn mined_elements(records: &RecordSet) -> Vec<MinedElement> {
    composition_elements(records, &build_index(records))
}

/// Star systems a key stem is matched against when the record set holds no star map.
const SYSTEMS: [&str; 3] = ["Stanton", "Pyro", "Nyx"];

/// A real star system for a provider key's stem (`Stanton2c`, `Aaronhalo`), `None` for
/// stems that are no system (`Lagrange`, `Asteroidcluster`, `Resourcerush`). `known` is
/// the star map's systems, so a system a build adds is kept; [`SYSTEMS`] backs it up.
pub fn system_name(system: Option<&str>, location: &str, known: &[String]) -> Option<String> {
    let s = system.unwrap_or(location).to_ascii_lowercase();
    known
        .iter()
        .map(String::as_str)
        .chain(SYSTEMS)
        .find(|n| !n.is_empty() && s.starts_with(&n.to_ascii_lowercase()))
        .map(str::to_string)
        // Curated: the Aaron Halo is Stanton's belt, which no record says.
        .or_else(|| s.starts_with("aaronhalo").then(|| "Stanton".to_string()))
}

/// `KeegerBelt` -> `Keeger Belt`.
fn spaced(s: &str) -> String {
    let mut out = String::new();
    let mut prev: Option<char> = None;
    for c in s.chars() {
        if c == '_' {
            out.push(' ');
        } else {
            if c.is_ascii_uppercase() && prev.is_some_and(|p| p.is_ascii_lowercase()) {
                out.push(' ');
            }
            out.push(c);
        }
        prev = Some(c);
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A readable place for a provider key: its body when there is one, else a name from the
/// key (`Lagrange_B` -> `Lagrange points`, `Nyx_KeegerBelt` -> `Keeger Belt`,
/// `Stanton2c_Belt` -> `Yela Belt` given Yela as `Stanton2c`'s body).
pub fn belt_place(
    location: &str,
    body: Option<&str>,
    systems: &[String],
    body_of: impl Fn(&str) -> Option<String>,
) -> String {
    if let Some(b) = body.filter(|b| !b.trim().is_empty()) {
        return b.trim().to_string();
    }
    let lower = location.to_ascii_lowercase();
    if lower.starts_with("lagrange") {
        return "Lagrange points".into();
    }
    if lower.starts_with("asteroidcluster") {
        return "Asteroid clusters".into();
    }
    if let Some(stem) = location.strip_suffix("_Belt")
        && let Some(b) = body_of(stem)
    {
        return format!("{b} Belt");
    }
    // Only a system stem is dropped: `ResourceRush_Gold` stays `Resource Rush Gold` (the
    // app's `belt_place` gives `Gold`).
    let (sys, rest) = match location.split_once('_') {
        Some((sys, rest)) if system_name(Some(sys), location, systems).is_some() => (sys, rest),
        _ => ("", location),
    };
    let rest_lower = rest.to_ascii_lowercase();
    if rest_lower.starts_with("cool") || rest_lower.starts_with("warm") {
        return format!("{} asteroid fields", spaced(sys)).trim().to_string();
    }
    let s = spaced(rest);
    if s.is_empty() { spaced(location) } else { s }
}

fn round_to(x: f64, scale: f64) -> f64 {
    js_round(x * scale) / scale
}

/// The belts of one ore key: one per location and rock, the spawn groups summed. A spawn
/// entry counts once however many parts of its rock hold the ore (a composition can list
/// an element twice); the percent range spans those parts.
fn ore_belts(locations: &[SpawnLocation], key: &str) -> Vec<OreBelt> {
    let mut belts: Vec<OreBelt> = Vec::new();
    for l in locations {
        for e in &l.entries {
            let parts: Vec<&OreShare> = e
                .ores
                .iter()
                .flatten()
                .filter(|o| ore_core(&o.element) == key)
                .collect();
            if parts.is_empty() {
                continue;
            }
            let rock = e.rock.clone().unwrap_or_default();
            if let Some(b) = belts.iter_mut().find(|b| b.location == l.key && b.rock == rock) {
                b.spawn_share = match (b.spawn_share, e.spawn_share) {
                    (Some(x), Some(y)) => Some(round_to(x + y, 1e5)),
                    (x, y) => x.or(y),
                };
                continue;
            }
            belts.push(OreBelt {
                location: l.key.clone(),
                place: l.place.clone(),
                place_source: l.place_source,
                system: l.system.clone(),
                rock,
                kind: e.kind.clone(),
                tier: e.tier.clone(),
                spawn_share: e.spawn_share,
                min_percent: parts.iter().filter_map(|o| o.min_percent).reduce(f64::min),
                max_percent: parts.iter().filter_map(|o| o.max_percent).reduce(f64::max),
            });
        }
    }
    belts.sort_by(|a, b| {
        b.spawn_share
            .unwrap_or(0.0)
            .total_cmp(&a.spawn_share.unwrap_or(0.0))
            .then_with(|| compare_names(&a.location, &b.location))
            .then_with(|| compare_names(&a.rock, &b.rock))
    });
    belts
}

/// The areas whose slots place one of `rocks` (exact rock names): per area the rock with
/// the best share of its config. Slots of weight 0 never spawn and are skipped.
fn ore_spots(areas: &[LootArea], rocks: &[&str]) -> Vec<OreSpot> {
    let mut spots = Vec::new();
    for a in areas {
        let mut configs: Vec<String> = Vec::new();
        let mut best: Option<(f64, &str)> = None;
        for c in &a.configs {
            let total: f64 = c.slots.iter().map(|s| s.weight.max(0.0)).sum();
            for s in &c.slots {
                let Some(rock) = rocks.iter().find(|r| s.entity.eq_ignore_ascii_case(r)) else {
                    continue;
                };
                let share = if total > 0.0 { s.weight.max(0.0) / total } else { 0.0 };
                if share <= 0.0 {
                    continue;
                }
                if let Some(name) = &c.name {
                    push_unique(&mut configs, name);
                }
                if best.is_none_or(|(b, _)| share > b) {
                    best = Some((share, rock));
                }
            }
        }
        if let Some((share, rock)) = best {
            let place = area_place(&a.name);
            spots.push(OreSpot {
                area: a.name.clone(),
                rock: rock.to_string(),
                slot_share: round_to(share, 1e8),
                configs,
                place,
                // The place comes from a curated rule, not from the records.
                place_source: place.map(|_| "curated"),
            });
        }
    }
    spots.sort_by(|a, b| b.slot_share.total_cmp(&a.slot_share).then_with(|| compare_names(&a.area, &b.area)));
    spots
}

/// `buildIndexes`: records by lower-cased file name (a later duplicate wins).
fn build_index(records: &RecordSet) -> HashMap<String, &GameRecord> {
    let mut by_file = HashMap::new();
    for r in records.iter() {
        let file = last_segment(&r.record_path).to_lowercase();
        if !file.is_empty() {
            by_file.insert(file, r);
        }
    }
    by_file
}

fn ores_of_composition(composition: Option<&GameRecord>) -> Option<Vec<OreShare>> {
    let parts = composition?.value.get("compositionArray")?.as_array()?;
    let ores: Vec<OreShare> = parts
        .iter()
        .filter(|p| p.is_object() || p.is_array())
        .filter_map(|p| {
            Some(OreShare {
                element: ref_file(p.get("mineableElement"))?,
                min_percent: num(p.get("minPercentage")),
                max_percent: num(p.get("maxPercentage")),
                probability: num(p.get("probability")),
                quality_scale: num(p.get("qualityScale")),
            })
        })
        .collect();
    (!ores.is_empty()).then_some(ores)
}

const TIERS: [&str; 5] = ["Common", "Uncommon", "Rare", "Epic", "Legendary"];

/// "miningglobalparamsship" -> "ship"; FPS obstacle and organic rocks are hand-mined.
fn kind_of(global_params: Option<&str>) -> Option<String> {
    let g = global_params.filter(|g| !g.is_empty())?;
    let kind = if g.contains("ship") {
        "ship"
    } else if g.contains("groundvehicle") {
        "vehicle"
    } else if g.contains("fps") {
        "hand"
    } else {
        return None;
    };
    Some(kind.to_string())
}

/// `rockSignature`: slot 4 of the radar base signatures (0 counts as none).
pub fn rock_signature(entity: &GameRecord) -> Option<f64> {
    let c = component(entity, "SSCSignatureSystemParams")?;
    let signatures = at(c.get("radarProperties")?, "baseSignatureParams.signatures")?.as_array()?;
    num(signatures.get(4)).filter(|s| *s != 0.0)
}

/// `describeRock`: a mineable rock entity's scan, tier and composition.
pub fn describe_rock(
    entity: &GameRecord,
    by_file: &HashMap<String, &GameRecord>,
    localization: &Localization,
) -> Option<RockInfo> {
    let mineable = component(entity, "MineableParams")?;
    let composition = ref_file(mineable.get("composition")).and_then(|f| by_file.get(&f).copied());
    Some(RockInfo {
        rock: entity.short_name.clone(),
        kind: kind_of(ref_file(mineable.get("globalParams")).as_deref()),
        // Case-sensitive, so "Uncommon" never matches "Common".
        tier: composition.and_then(|c| {
            TIERS
                .iter()
                .find(|t| c.short_name.starts_with(*t))
                .map(|t| (*t).to_string())
        }),
        composition: composition.map(|c| c.short_name.clone()),
        deposit: composition
            .and_then(|c| localize(localization, c.value.get("depositName")))
            .map(str::to_string),
        signature: rock_signature(entity),
        minimum_distinct_elements: composition.and_then(|c| num(c.value.get("minimumDistinctElements"))),
        ores: ores_of_composition(composition),
        spawns_at: None,
    })
}

/// `rockOfHarvestable`: harvestable preset -> mineable entity -> rock.
fn rock_of_harvestable(
    file: &str,
    by_file: &HashMap<String, &GameRecord>,
    localization: &Localization,
) -> Option<RockInfo> {
    let preset = by_file
        .get(&format!("{file}.json"))
        .or_else(|| by_file.get(file))?;
    let entity = by_file.get(&ref_file(preset.value.get("entityClass"))?)?;
    describe_rock(entity, by_file, localization)
}

fn push_unique(list: &mut Vec<String>, s: &str) {
    if !list.iter().any(|x| x == s) {
        list.push(s.to_string());
    }
}

fn sorted_names(mut v: Vec<String>) -> Vec<String> {
    v.sort_by(|a, b| compare_names(a, b));
    v
}

impl Domain for MiningSpawns {
    fn name(&self) -> &'static str {
        "game-mining-spawns"
    }
    fn schema_version(&self) -> u32 {
        1
    }

    fn description(&self) -> &'static str {
        "Which ores spawn where, resolved from provider presets through to compositions."
    }

    fn filters(&self) -> &'static [&'static str] {
        &[
            "**/harvestable/**",
            "**/entities/mineable/**",
            "**/mining/**",
            "**/starmap/**",
        ]
    }

    fn needs_localization(&self) -> bool {
        true
    }

    fn parse(&self, ctx: &ParseContext<'_>) -> Result<Map<String, Value>, DomainError> {
        let loc = ctx.localization;
        let by_file = build_index(ctx.records);
        // Spawn zone keys match star map record names for planets and moons ("Pyro5e" is
        // Fuego), which names zones the string table does not.
        let located = build_locations(
            ctx.records,
            loc,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        );
        let mut systems: Vec<String> = located.iter().filter_map(|l| l.system.clone()).collect();
        systems.sort();
        systems.dedup();
        let starmap: HashMap<String, &Location> = located
            .iter()
            .map(|l| {
                let short = l.record_name.find('.').map_or(&l.record_name[..], |d| &l.record_name[d + 1..]);
                (short.to_lowercase(), l)
            })
            .collect();

        let mut locations: Vec<SpawnLocation> = Vec::new();
        for record in ctx.records.of_type("HarvestableProviderPreset") {
            let Some(groups) = record.value.get("harvestableGroups").and_then(Value::as_array) else {
                continue;
            };
            let mut entries = Vec::new();
            let mut ores: Vec<String> = Vec::new();
            for g in groups.iter().filter(|g| g.is_object() || g.is_array()) {
                let group_name = str_(g.get("groupName")).unwrap_or("unnamed");
                let group_probability = num(g.get("groupProbability"));
                let harvestables: Vec<&Value> = array(g.get("harvestables"))
                    .iter()
                    .filter(|e| e.is_object() || e.is_array())
                    .collect();
                let group_weight = harvestables
                    .iter()
                    .fold(0.0, |sum, e| sum + num(e.get("relativeProbability")).unwrap_or(0.0));
                for e in harvestables {
                    let Some(harvestable) =
                        ref_file(e.get("harvestable")).or_else(|| ref_file(e.get("harvestableEntityClass")))
                    else {
                        continue;
                    };
                    let rock = rock_of_harvestable(&harvestable, &by_file, loc);
                    let resolved = rock.as_ref().and_then(|r| r.ores.clone());
                    for ore in resolved.iter().flatten() {
                        push_unique(&mut ores, &ore.element);
                    }
                    let relative_probability = num(e.get("relativeProbability"));
                    let spawn_share = match (group_probability, relative_probability) {
                        (Some(gp), Some(rp)) if group_weight > 0.0 => {
                            Some(js_round(((gp * rp) / group_weight) * 1e5) / 1e5)
                        }
                        _ => None,
                    };
                    entries.push(SpawnEntry {
                        group: group_name.to_string(),
                        group_probability,
                        relative_probability,
                        spawn_share,
                        harvestable,
                        rock: rock.as_ref().map(|r| r.rock.clone()),
                        kind: rock.as_ref().and_then(|r| r.kind.clone()),
                        tier: rock.as_ref().and_then(|r| r.tier.clone()),
                        deposit: rock.as_ref().and_then(|r| r.deposit.clone()),
                        signature: rock.as_ref().and_then(|r| r.signature),
                        ores: resolved,
                    });
                }
            }
            let provider = describe_provider(&record.short_name, loc);
            let place = starmap.get(&provider.key.to_lowercase());
            // The star map's system when the key names a body; else the key's stem, which is
            // no system for belts and events ("Aaronhalo", "Lagrange").
            let system = place
                .and_then(|p| p.system.clone())
                .or_else(|| system_name(provider.system.as_deref(), &provider.key, &systems));
            locations.push(SpawnLocation {
                system,
                body: provider.body.or_else(|| place.map(|p| p.name.clone())),
                // Filled below, once every location's body is known.
                place: String::new(),
                place_source: "heuristic",
                starmap_record: place.map(|p| p.record_name.clone()),
                path: place.and_then(|p| p.path.clone()),
                key: provider.key,
                record_name: record.name.clone(),
                entries,
                ores: sorted_names(ores),
            });
        }
        locations.sort_by(|a, b| compare_names(&a.key, &b.key));
        let bodies: HashMap<String, String> = locations
            .iter()
            .filter_map(|l| Some((l.key.to_ascii_lowercase(), l.body.clone()?)))
            .collect();
        for l in &mut locations {
            let named = l.body.as_deref().is_some_and(|b| !b.trim().is_empty());
            l.place = belt_place(&l.key, l.body.as_deref(), &systems, |k| {
                bodies.get(&k.to_ascii_lowercase()).cloned()
            });
            l.place_source = if named { "data" } else { "heuristic" };
        }

        // The inverse index: where is this ore? (a JS object, so integer-like keys first)
        let mut ore_locations: Map<String, Value> = Map::new();
        for l in &locations {
            for ore in &l.ores {
                if let Value::Array(a) = ore_locations
                    .entry(ore.clone())
                    .or_insert_with(|| Value::Array(Vec::new()))
                {
                    a.push(json!(l.body.as_deref().unwrap_or(&l.key)));
                }
            }
        }
        let mut ore_entries: Vec<(String, Vec<String>)> = js_key_order(&ore_locations)
            .into_iter()
            .map(|k| {
                let list = array(ore_locations.get(k))
                    .iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect();
                (k.clone(), sorted_names(list))
            })
            .collect();
        ore_entries.sort_by(|a, b| compare_names(&a.0, &b.0));
        let distinct_ores = ore_entries.len();
        let ore_locations: Map<String, Value> = ore_entries
            .into_iter()
            .map(|(k, v)| (k, json!(v)))
            .collect();

        let resolved = locations.iter().filter(|l| !l.ores.is_empty()).count();

        let elements = composition_elements(ctx.records, &by_file);
        let resource_of: HashMap<&str, &str> = elements
            .iter()
            .filter_map(|e| Some((e.element.as_str(), e.resource.as_deref()?)))
            .collect();

        // One row per ore per spawn entry.
        let resource_of = &resource_of;
        let mut ore_spawns: Vec<OreSpawn> = locations
            .iter()
            .flat_map(|l| {
                l.entries.iter().flat_map(move |e| {
                    e.ores.iter().flatten().map(move |o| OreSpawn {
                        ore: o.element.clone(),
                        ore_key: ore_core(&o.element),
                        resource: resource_of.get(o.element.as_str()).map(|r| (*r).to_string()),
                        location: l.key.clone(),
                        system: l.system.clone(),
                        body: l.body.clone(),
                        rock: e.rock.clone(),
                        kind: e.kind.clone(),
                        tier: e.tier.clone(),
                        signature: e.signature,
                        spawn_share: e.spawn_share,
                        min_percent: o.min_percent,
                        max_percent: o.max_percent,
                        quality_scale: o.quality_scale,
                    })
                })
            })
            .collect();
        ore_spawns.sort_by(|a, b| {
            compare_names(&a.ore, &b.ore).then_with(|| compare_names(&a.location, &b.location))
        });

        // Every mineable rock, spawned or not, with where it spawns.
        let mut spawns_at: HashMap<String, Vec<String>> = HashMap::new();
        for l in &locations {
            for e in &l.entries {
                if let Some(rock) = e.rock.as_ref().filter(|r| !r.is_empty()) {
                    push_unique(spawns_at.entry(rock.clone()).or_default(), l.body.as_deref().unwrap_or(&l.key));
                }
            }
        }
        let mut rocks: Vec<RockInfo> = ctx
            .records
            .iter()
            .filter(|r| r.record_path.contains("/entities/mineable/"))
            .filter_map(|r| describe_rock(r, &by_file, loc))
            .map(|mut r| {
                r.spawns_at = Some(sorted_names(spawns_at.get(&r.rock).cloned().unwrap_or_default()));
                r
            })
            .collect();
        rocks.sort_by(|a, b| compare_names(&a.rock, &b.rock));

        // Per ore key: its elements and resources, belts and hand-placed spots.
        let mut keyed: Vec<(String, Vec<String>)> = Vec::new();
        let mut add = |element: &str| {
            let key = ore_core(element);
            match keyed.iter_mut().find(|(k, _)| *k == key) {
                Some((_, list)) => push_unique(list, element),
                None => keyed.push((key, vec![element.to_string()])),
            }
        };
        elements.iter().for_each(|e| add(&e.element));
        rocks.iter().flat_map(|r| r.ores.iter().flatten()).for_each(|o| add(&o.element));
        ore_spawns.iter().for_each(|s| add(&s.ore));
        keyed.sort_by(|a, b| compare_names(&a.0, &b.0));
        let areas = loot_areas(ctx.records, loc);
        let mut ores: Map<String, Value> = Map::new();
        let (mut with_belts, mut with_spots) = (0, 0);
        for (key, list) in keyed {
            let list = sorted_names(list);
            let mut resources: Vec<String> = Vec::new();
            for e in &list {
                if let Some(r) = resource_of.get(e.as_str()) {
                    push_unique(&mut resources, r);
                }
            }
            let holders: Vec<&str> = rocks
                .iter()
                .filter(|r| r.ores.iter().flatten().any(|o| list.contains(&o.element)))
                .map(|r| r.rock.as_str())
                .collect();
            let sites = OreSites {
                belts: ore_belts(&locations, &key),
                spots: ore_spots(&areas, &holders),
                resources: sorted_names(resources),
                elements: list,
            };
            with_belts += usize::from(!sites.belts.is_empty());
            with_spots += usize::from(!sites.spots.is_empty());
            field(&mut ores, &key, &sites)?;
        }

        let summary = json!({
            "locations": locations.len(),
            "locationsWithOres": resolved,
            "locationsNamed": locations.iter().filter(|l| l.body.as_deref().is_some_and(|b| !b.is_empty())).count(),
            "distinctOres": distinct_ores,
            "spawnEntries": locations.iter().map(|l| l.entries.len()).sum::<usize>(),
            "rocks": rocks.len(),
            "rocksWithSignature": rocks.iter().filter(|r| r.signature.is_some_and(|s| s != 0.0)).count(),
            "oreKeys": ores.len(),
            "oreKeysWithBelts": with_belts,
            "oreKeysWithSpots": with_spots,
        });

        let mut body = Map::new();
        body.insert("_assumptions".into(), json!(MINING_ASSUMPTIONS));
        field(&mut body, "locations", &locations)?;
        field(&mut body, "rocks", &rocks)?;
        field(&mut body, "oreSpawns", &ore_spawns)?;
        body.insert("oreLocations".into(), Value::Object(ore_locations));
        body.insert("ores".into(), Value::Object(ores));
        body.insert("summary".into(), summary);
        Ok(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::NoFiles;
    use crate::domains::starmap::tests::{file_ref, rec as srec};
    use crate::localization::parse_global_ini;
    use crate::records::normalize_record;

    fn rec(ty: &str, short: &str, path: &str, value: Value) -> GameRecord {
        let mut v = value;
        v["_Type_"] = json!(ty);
        normalize_record(
            json!({"_RecordName_": format!("{ty}.{short}"), "_RecordId_": format!("id-{short}"),
                   "_RecordValue_": v}),
            &format!("libs/foundry/records/{path}"),
        )
        .unwrap()
    }

    // Cases from sc-gamedata src/domains/mining-spawns.test.ts.
    #[test]
    fn describes_providers() {
        let s = parse_global_ini("Stanton1=Hurston\nStanton1c=Magda");
        assert_eq!(
            describe_provider("HPP_Stanton1", &s),
            Provider {
                key: "Stanton1".into(),
                system: Some("Stanton".into()),
                body: Some("Hurston".into())
            }
        );
        let magda = describe_provider("HPP_Stanton1c", &s);
        assert_eq!((magda.key.as_str(), magda.body.as_deref()), ("Stanton1c", Some("Magda")));
        let ring = describe_provider("HPP_Nyx_GlaciemRing", &s);
        assert_eq!((ring.key.as_str(), ring.system.as_deref()), ("Nyx_GlaciemRing", Some("Nyx")));
        assert_eq!(describe_provider("HPP_Sol9", &s).body, None);
        assert_eq!(describe_provider("hpp_pyro5e", &s).system.as_deref(), Some("Pyro"));
    }

    fn chain() -> Vec<GameRecord> {
        vec![
            rec(
                "HarvestableProviderPreset",
                "HPP_Stanton1",
                "harvestable/providerpresets/hpp_stanton1",
                json!({"harvestableGroups": [{"groupName": "SpaceShip_Mineables", "groupProbability": 6,
                    "harvestables": [{"harvestable": "file://./x/harvestable/harvestablepresets/mining_common_iron.json",
                                      "relativeProbability": 34.8}]}]}),
            ),
            rec(
                "HarvestablePreset",
                "Mining_Common_Iron",
                "harvestable/harvestablepresets/mining_common_iron",
                json!({"entityClass": "file://./x/entities/mineable/mineablerock_surfacecommon_iron.json"}),
            ),
            rec(
                "EntityClassDefinition",
                "MineableRock_SurfaceCommon_Iron",
                "entities/mineable/mineablerock_surfacecommon_iron",
                json!({"Components": [{"_Type_": "MineableParams",
                    "composition": "file://./x/mining/rockcompositionpresets/commonshipmineables_iron.json"}]}),
            ),
            rec(
                "MineableComposition",
                "CommonShipMineables_Iron",
                "mining/rockcompositionpresets/commonshipmineables_iron",
                json!({"compositionArray": [{"mineableElement": "file://./x/mining/mineableelements/iron_ore.json",
                    "minPercentage": 34.3, "maxPercentage": 84.3, "probability": 1}]}),
            ),
        ]
    }

    fn parse(records: Vec<GameRecord>, loc: &Localization) -> Map<String, Value> {
        let set = RecordSet::new(records);
        DOMAIN.parse(&ParseContext::new(&set, loc, &NoFiles)).unwrap()
    }

    #[test]
    fn resolves_a_location_through_to_its_ores() {
        let loc = parse_global_ini("Stanton1=Hurston");
        let out = parse(chain(), &loc);
        assert_eq!(out["locations"][0]["body"], "Hurston");
        assert_eq!(out["locations"][0]["ores"], json!(["iron_ore"]));
        assert_eq!(out["locations"][0]["entries"][0]["relativeProbability"], json!(34.8));
        assert_eq!(out["locations"][0]["entries"][0]["ores"][0]["minPercent"], json!(34.3));
        assert_eq!(out["oreLocations"]["iron_ore"], json!(["Hurston"]));
        // A tier is read only from tiered names; "CommonShipMineables" is Common.
        assert_eq!(out["rocks"][0]["tier"], "Common");
        assert_eq!(out["rocks"][0]["spawnsAt"], json!(["Hurston"]));
    }

    #[test]
    fn still_records_a_location_when_the_chain_breaks() {
        let loc = parse_global_ini("Stanton1=Hurston");
        let out = parse(chain().into_iter().take(1).collect(), &loc);
        assert_eq!(out["summary"]["locations"], 1);
        assert_eq!(out["summary"]["locationsWithOres"], 0);
        assert_eq!(out["summary"]["spawnEntries"], 1);
    }

    // Cases from sc-gamedata src/domains/links.test.ts ("mining rocks").
    fn rock(name: &str, composition: &str, signature: f64) -> GameRecord {
        srec(
            "EntityClassDefinition",
            &format!("entities/mineable/{}", name.to_lowercase()),
            json!({"Components": [
                {"_Type_": "MineableParams", "globalParams": file_ref("mining/miningglobalparamsship"),
                 "composition": file_ref(&format!("mining/rockcompositionpresets/{}", composition.to_lowercase()))},
                {"_Type_": "SSCSignatureSystemParams",
                 "radarProperties": {"baseSignatureParams": {"signatures": [0, 0, 0, 0, signature, 0, 0, 0]}}}
            ]}),
            Some(name),
        )
    }

    fn composition(name: &str, element: &str) -> GameRecord {
        srec(
            "MineableComposition",
            &format!("mining/rockcompositionpresets/{}", name.to_lowercase()),
            json!({"depositName": "@quant", "minimumDistinctElements": 3, "compositionArray": [
                {"mineableElement": file_ref(&format!("mining/mineableelements/{element}")),
                 "minPercentage": 28.3, "maxPercentage": 78.3, "probability": 1, "qualityScale": 0.49}]}),
            Some(name),
        )
    }

    fn rocks() -> Vec<GameRecord> {
        vec![
            rock("MineableRock_AsteroidLegendary_Quantainium", "LegendaryShipMineablesAsteroid_Quantainium", 3170.0),
            composition("LegendaryShipMineablesAsteroid_Quantainium", "quantainium_raw"),
            rock("MineableRock_Common_Iron", "UncommonShipMineables_Iron", 4270.0),
            composition("UncommonShipMineables_Iron", "iron_ore"),
        ]
    }

    #[test]
    fn reads_signature_and_tier() {
        let records = rocks();
        let set = RecordSet::new(records.clone());
        let by_file = build_index(&set);
        assert_eq!(rock_signature(&records[0]), Some(3170.0));
        let d = describe_rock(&records[0], &by_file, &parse_global_ini("quant=Quantainium (Raw)")).unwrap();
        assert_eq!(d.kind.as_deref(), Some("ship"));
        assert_eq!(d.tier.as_deref(), Some("Legendary"));
        assert_eq!(d.deposit.as_deref(), Some("Quantainium (Raw)"));
        assert_eq!(d.signature, Some(3170.0));
        let ores = d.ores.unwrap();
        assert_eq!((ores[0].element.as_str(), ores[0].quality_scale), ("quantainium_raw", Some(0.49)));
        // Case-sensitive, so Uncommon is not read as Common.
        assert_eq!(
            describe_rock(&records[2], &by_file, &Localization::new()).unwrap().tier.as_deref(),
            Some("Uncommon")
        );
    }

    #[test]
    fn shares_of_a_group_and_where_each_ore_spawns() {
        let mut records = rocks();
        records.extend([
            srec(
                "HarvestablePreset",
                "harvestable/harvestablepresets/mining_legendary_quantainium",
                json!({"entityClass": file_ref("entities/mineable/mineablerock_asteroidlegendary_quantainium")}),
                None,
            ),
            srec(
                "HarvestablePreset",
                "harvestable/harvestablepresets/mining_iron",
                json!({"entityClass": file_ref("entities/mineable/mineablerock_common_iron")}),
                None,
            ),
            srec(
                "HarvestableProviderPreset",
                "harvestable/providerpresets/hpp_aaronhalo",
                json!({"harvestableGroups": [{"groupName": "SpaceShip_Mineables", "groupProbability": 0.3,
                    "harvestables": [
                        {"harvestable": file_ref("harvestable/harvestablepresets/mining_legendary_quantainium"), "relativeProbability": 1},
                        {"harvestable": file_ref("harvestable/harvestablepresets/mining_iron"), "relativeProbability": 3}]}]}),
                Some("HPP_AaronHalo"),
            ),
        ]);
        let out = parse(records, &parse_global_ini("AaronHalo=Aaron Halo"));
        let q = out["oreSpawns"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["ore"] == "quantainium_raw")
            .unwrap();
        assert_eq!(q["body"], "Aaron Halo");
        assert_eq!(q["tier"], "Legendary");
        assert_eq!(q["signature"], json!(3170.0));
        assert_eq!(q["spawnShare"], json!(0.075));
        let iron = out["rocks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["rock"] == "MineableRock_Common_Iron")
            .unwrap();
        assert_eq!(iron["spawnsAt"], json!(["Aaron Halo"]));
    }

    // Cases from starorg-strings starorg-mcp hints.rs (`belt_places_read_like_places`).
    #[test]
    fn belt_places_read_like_places() {
        let none = |_: &str| None;
        assert_eq!(belt_place("Stanton1", Some("Hurston"), &[], none), "Hurston");
        assert_eq!(belt_place("Lagrange_B", None, &[], none), "Lagrange points");
        assert_eq!(belt_place("Nyx_KeegerBelt", None, &[], none), "Keeger Belt");
        assert_eq!(belt_place("Nyx_GlaciemRing", None, &[], none), "Glaciem Ring");
        assert_eq!(belt_place("Pyro_DeepSpaceAsteroids", None, &[], none), "Deep Space Asteroids");
        assert_eq!(belt_place("Pyro_Cool01", None, &[], none), "Pyro asteroid fields");
        assert_eq!(belt_place("AsteroidCluster_Low_Yield", None, &[], none), "Asteroid clusters");
        // A stem that is no system stays in the name.
        assert_eq!(belt_place("ResourceRush_Gold", None, &[], none), "Resource Rush Gold");
        let yela = |s: &str| (s == "Stanton2c").then(|| "Yela".to_string());
        assert_eq!(belt_place("Stanton2c_Belt", None, &[], yela), "Yela Belt");
        assert_eq!(system_name(Some("Stanton2c"), "x", &[]).as_deref(), Some("Stanton"));
        assert_eq!(system_name(Some("Aaronhalo"), "x", &[]).as_deref(), Some("Stanton"));
        assert_eq!(system_name(Some("Lagrange"), "Lagrange_A", &[]), None);
    }

    fn element(file: &str, resource: &str) -> GameRecord {
        srec(
            "MineableElement",
            &format!("mining/mineableelements/{file}"),
            json!({"resourceType": {"_RecordName_": format!("ResourceType.{resource}"), "_RecordId_": "r"}}),
            None,
        )
    }

    fn slot_preset(name: &str, config: &str, slots: &[(&str, f64)]) -> GameRecord {
        let slots: Vec<Value> = slots
            .iter()
            .map(|(e, w)| {
                json!({"_Type_": "SubHarvestableSlot", "relativeProbability": w,
                       "harvestableEntityClass": file_ref(&format!("entities/mineable/{e}"))})
            })
            .collect();
        srec(
            "SubHarvestableMultiConfigRecord",
            &format!("harvestable/slotpresets/{name}"),
            json!({"multiConfig": {"defaultConfig": null, "taggedConfigs": [
                {"name": config, "tagList": {"tags": []},
                 "subConfig": {"_Type_": "SubHarvestableConfigSingleManual", "subConfigManual": {
                     "_Type_": "SubHarvestableConfig", "initialSlotsProbability": 1,
                     "subHarvestables": slots}}}]}}),
            None,
        )
    }

    fn provider(key: &str, groups: Value) -> GameRecord {
        srec(
            "HarvestableProviderPreset",
            &format!("harvestable/providerpresets/hpp_{}", key.to_lowercase()),
            json!({"harvestableGroups": groups}),
            Some(&format!("HPP_{key}")),
        )
    }

    /// Quantainium on Yela, its belt (two groups) and a Lagrange point; Sadaryx only in a
    /// Rock Cracker slot preset; a cave whose Sadaryx slot weighs 0.
    fn world() -> Vec<GameRecord> {
        let mut records = rocks();
        let quant = |p: f64| {
            json!({"groupName": "SpaceShip_Mineables", "groupProbability": p, "harvestables": [
                {"harvestable": file_ref("harvestable/harvestablepresets/mining_legendary_quantainium"), "relativeProbability": 1}]})
        };
        records.extend([
            element("quantainium_raw", "Raw_Quantainium"),
            element("minableelement_fps_sadaryx", "Sadaryx"),
            srec(
                "HarvestablePreset",
                "harvestable/harvestablepresets/mining_legendary_quantainium",
                json!({"entityClass": file_ref("entities/mineable/mineablerock_asteroidlegendary_quantainium")}),
                None,
            ),
            provider("Stanton2c", json!([quant(0.5)])),
            provider("Stanton2c_Belt", json!([quant(0.1), quant(0.2)])),
            provider("Lagrange_B", json!([quant(0.05)])),
            rock("MineableRock_FPS_Sadaryx", "FPS_Composition_SadaryxDeposit", 3000.0),
            composition("FPS_Composition_SadaryxDeposit", "minableelement_fps_sadaryx"),
            slot_preset("v3slotpreset_rockcracker", "Sadaryx minable", &[("mineablerock_fps_sadaryx", 1.0)]),
            slot_preset("cave_x", "Loose", &[("mineablerock_fps_sadaryx", 0.0), ("other", 3.0)]),
        ]);
        records
    }

    #[test]
    fn one_ore_key_with_summed_belts_and_hand_spots() {
        let set = RecordSet::new(world());
        let out = crate::fixtures::parse_checked(&DOMAIN, &set, &parse_global_ini("Stanton2c=Yela"));
        let spawn = out["oreSpawns"].as_array().unwrap().iter().find(|s| s["location"] == "Stanton2c_Belt").unwrap();
        assert_eq!((&spawn["oreKey"], &spawn["resource"]), (&json!("quantainium"), &json!("Raw_Quantainium")));
        assert_eq!(spawn["system"], "Stanton", "the stem Stanton2c is no system");
        let lagrange = out["locations"].as_array().unwrap().iter().find(|l| l["key"] == "Lagrange_B").unwrap();
        assert_eq!((&lagrange["place"], &lagrange["placeSource"]), (&json!("Lagrange points"), &json!("heuristic")));
        assert!(lagrange.get("system").is_none(), "Lagrange is no system");

        let q = &out["ores"]["quantainium"];
        assert_eq!(q["elements"], json!(["quantainium_raw"]));
        assert_eq!(q["resources"], json!(["Raw_Quantainium"]));
        let belts: Vec<(&str, &str, f64)> = q["belts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| (b["location"].as_str().unwrap(), b["place"].as_str().unwrap(), b["spawnShare"].as_f64().unwrap()))
            .collect();
        assert_eq!(
            belts,
            [("Stanton2c", "Yela", 0.5), ("Stanton2c_Belt", "Yela Belt", 0.3), ("Lagrange_B", "Lagrange points", 0.05)],
            "two groups of one rock add up, highest first"
        );
        assert_eq!(q["belts"][0]["placeSource"], "data");
        assert_eq!(q["spots"], json!([]));

        let s = &out["ores"]["sadaryx"];
        assert_eq!(s["belts"], json!([]), "Sadaryx has no spawn table");
        assert_eq!(
            s["spots"],
            json!([{"area": "v3slotpreset_rockcracker", "rock": "MineableRock_FPS_Sadaryx", "slotShare": 1.0,
                    "configs": ["Sadaryx minable"], "place": "Rock Cracker, Nyx", "placeSource": "curated"}]),
            "the cave's slot weighs 0"
        );
        assert_eq!(out["summary"]["oreKeys"], 3);
        assert_eq!(out["summary"]["oreKeysWithSpots"], 1);
        assert_eq!(out["_assumptions"], json!(MINING_ASSUMPTIONS));
    }

    /// LIVE compositions list an element twice (Quantainium at 5.7-11.7 and 28.3-78.3).
    #[test]
    fn a_rock_holding_an_ore_in_two_parts_counts_its_spawn_once() {
        let mut records = world();
        let part = |min: f64, max: f64| {
            json!({"mineableElement": file_ref("mining/mineableelements/quantainium_raw"),
                   "minPercentage": min, "maxPercentage": max, "probability": 1})
        };
        records.extend([
            rock("MineableRock_Two_Parts", "LegendaryShipMineables_TwoParts", 3170.0),
            srec(
                "MineableComposition",
                "mining/rockcompositionpresets/legendaryshipmineables_twoparts",
                json!({"compositionArray": [part(5.7, 11.7), part(28.3, 78.3)]}),
                Some("LegendaryShipMineables_TwoParts"),
            ),
            srec(
                "HarvestablePreset",
                "harvestable/harvestablepresets/mining_two_parts",
                json!({"entityClass": file_ref("entities/mineable/mineablerock_two_parts")}),
                None,
            ),
            provider(
                "Pyro_Cool01",
                json!([{"groupName": "SpaceShip_Mineables", "groupProbability": 0.4, "harvestables": [
                    {"harvestable": file_ref("harvestable/harvestablepresets/mining_two_parts"), "relativeProbability": 1}]}]),
            ),
        ]);
        let out = crate::fixtures::parse_checked(&DOMAIN, &RecordSet::new(records), &Localization::new());
        let rows = out["oreSpawns"].as_array().unwrap().iter().filter(|s| s["location"] == "Pyro_Cool01").count();
        assert_eq!(rows, 2, "oreSpawns keeps a row per part");
        let belt = out["ores"]["quantainium"]["belts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|b| b["location"] == "Pyro_Cool01")
            .unwrap();
        assert_eq!(belt["spawnShare"], 0.4, "one spawn entry, not one per part");
        assert_eq!((&belt["minPercent"], &belt["maxPercent"]), (&json!(5.7), &json!(78.3)));
        assert_eq!((&belt["place"], &belt["system"]), (&json!("Pyro asteroid fields"), &json!("Pyro")));
    }

    #[test]
    fn a_system_the_star_map_adds_is_kept() {
        let known = ["Castra".to_string()];
        assert_eq!(system_name(Some("Castra2"), "Castra2_Belt", &known).as_deref(), Some("Castra"));
        assert_eq!(system_name(Some("Castra2"), "Castra2_Belt", &[]), None);
        assert_eq!(belt_place("Castra_Cool01", None, &known, |_| None), "Castra asteroid fields");
    }

    #[test]
    fn elements_name_their_resource() {
        let set = RecordSet::new(world());
        let mined = mined_elements(&set);
        let pairs: Vec<(&str, &str, Option<&str>)> = mined
            .iter()
            .map(|e| (e.element.as_str(), e.ore_key.as_str(), e.resource.as_deref()))
            .collect();
        assert_eq!(
            pairs,
            [
                ("iron_ore", "iron", None),
                ("minableelement_fps_sadaryx", "sadaryx", Some("Sadaryx")),
                ("quantainium_raw", "quantainium", Some("Raw_Quantainium")),
            ]
        );
    }
}
