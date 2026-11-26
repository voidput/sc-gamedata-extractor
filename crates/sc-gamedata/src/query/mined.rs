//! Where an ore can be mined: the belts and bodies whose spawn tables roll a rock holding it
//! (`game-mining-spawns` `oreSpawns`), and the hand-placed spots inside loot areas (caves,
//! asteroid bases, events) whose spawn slots place such a rock (`game-item-sources`).
//!
//! Some ores have no spawn table at all (Sadaryx): the spots are then the only places. Area
//! names are internal slot presets; [`area_place`] adds the place for the few whose
//! location the asset paths show and the records do not name.

use super::obtain::area_label;
use super::types::LootArea;
use crate::js::round_to;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One rock of `game-mining-spawns` `rocks`, as far as this reads it.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MinedRock {
    pub rock: String,
    pub kind: Option<String>,
    pub tier: Option<String>,
    pub ores: Vec<MinedOre>,
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MinedOre {
    pub element: String,
}

/// One row of `game-mining-spawns` `oreSpawns`.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct OreSpawn {
    pub ore: String,
    pub location: String,
    pub system: Option<String>,
    pub body: Option<String>,
    pub rock: String,
    pub kind: Option<String>,
    pub tier: Option<String>,
    pub spawn_share: Option<f64>,
    pub min_percent: Option<f64>,
    pub max_percent: Option<f64>,
}

/// The parts of `game-mining-spawns` this reads.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MiningSpawns {
    pub rocks: Vec<MinedRock>,
    pub ore_spawns: Vec<OreSpawn>,
}

/// A belt or body that rolls the ore.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MinedBelt {
    /// Provider key (`Stanton1c`).
    pub location: String,
    pub system: Option<String>,
    /// Readable body or belt name when the strings have one.
    pub body: Option<String>,
    pub rock: String,
    /// "ship", "vehicle" or "hand".
    pub kind: Option<String>,
    pub tier: Option<String>,
    /// Spawn weight of this rock at the location: group probability times its share of the
    /// group, summed over groups. Not normalised across groups (hand-mining groups weigh in
    /// the tens, ship groups below 1), so compare it only between rows of one kind.
    pub spawn_share: Option<f64>,
    /// Lowest and highest bound over the composition parts holding the ore, not a total.
    pub min_percent: Option<f64>,
    pub max_percent: Option<f64>,
}

/// A loot area whose spawn slots place a rock with the ore.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MinedSpot {
    /// Readable area name (`caves: loot caves occupied rock`).
    pub area: String,
    /// The place it is known to be, when the records do not say (`Rock Cracker, Nyx`).
    pub place: Option<String>,
    /// Spawn config names inside the area that place the rock (`FPS mineables`).
    pub configs: Vec<String>,
    pub rock: String,
    /// Best weight share of the rock among its config's slots (0..1).
    pub slot_share: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MinedReport {
    /// Element key (`minableelement_fps_sadaryx`, `quantainium_raw`).
    pub ore: String,
    /// Readable ore name (`Sadaryx`).
    pub name: String,
    /// The rocks that hold it.
    pub rocks: Vec<String>,
    /// Most common first.
    pub belts: Vec<MinedBelt>,
    /// Best slot share first.
    pub spots: Vec<MinedSpot>,
}

const PREFIXES: &[&str] = &[
    "minableelement_groundvehicle_",
    "minableelement_fps_",
    "minableelement_",
    "mineablerock_groundvehicle_",
    "mineablerock_fps_",
    "harvestable_mineral_1h_",
];

/// "MineableElement.MinableElement_FPS_Sadaryx" -> "sadaryx"; "quantainium_raw" ->
/// "quantainium". Lower case letters and digits only.
pub fn ore_core(s: &str) -> String {
    let s = s.rsplit('.').next().unwrap_or(s).to_lowercase();
    let mut s = s.as_str();
    for p in PREFIXES {
        if let Some(rest) = s.strip_prefix(p) {
            s = rest;
            break;
        }
    }
    for x in ["_ore", "_raw"] {
        if let Some(rest) = s.strip_suffix(x) {
            s = rest;
            break;
        }
    }
    s.chars().filter(char::is_ascii_alphanumeric).collect()
}

/// Places the asset paths show for slot presets whose records name none:
/// `Objects/space/3d/asteroid/.../rockcracker_*` and only Nyx's
/// `ObjectContainers/Setup/elevator_setup/Nyx/nyx_*rockcracker*` reach it.
pub fn area_place(area: &str) -> Option<&'static str> {
    let a = area.to_lowercase();
    a.contains("rockcracker").then_some("Rock Cracker, Nyx")
}

fn title(core: &str) -> String {
    let mut c = core.chars();
    c.next()
        .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
        .unwrap_or_default()
}

/// The ore element keys a query names: by element key, rock name or plain name
/// ("Sadaryx", "Carryable_..._Mineral_Sadaryx", "ResourceType.Sadaryx" all name Sadaryx).
fn ores_for(spawns: &MiningSpawns, query: &str) -> Vec<String> {
    let mut known: BTreeMap<String, String> = BTreeMap::new();
    for r in &spawns.rocks {
        for o in &r.ores {
            known.insert(ore_core(&o.element), o.element.clone());
        }
    }
    for s in &spawns.ore_spawns {
        known
            .entry(ore_core(&s.ore))
            .or_insert_with(|| s.ore.clone());
    }
    let q = query.trim();
    if q.is_empty() {
        return Vec::new();
    }
    // A rock names its own ores (with or without its record type).
    let short = q.rsplit('.').next().unwrap_or(q);
    if let Some(r) = spawns
        .rocks
        .iter()
        .find(|r| r.rock.eq_ignore_ascii_case(short))
    {
        return r.ores.iter().map(|o| o.element.clone()).collect();
    }
    let whole = ore_core(q);
    if let Some(e) = known.get(&whole) {
        return vec![e.clone()];
    }
    // Else the last word of a mining or commodity name ("..._Mineral_Sadaryx",
    // "ResourceType.Sadaryx", "Gold Ore"), not of any item ("rrs_armor_gold").
    let lower = q.to_lowercase();
    let words = q.split(['_', '.', ' ']).filter(|w| !w.is_empty()).count();
    let marked = [
        "mineral",
        "commodity",
        "resourcetype",
        "mineable",
        "minable",
        "harvestable",
    ]
    .iter()
    .any(|m| lower.contains(m));
    if !(marked || words <= 2) {
        return Vec::new();
    }
    let last = q
        .rsplit(['_', '.', ' '])
        .find(|w| !w.eq_ignore_ascii_case("ore") && !w.eq_ignore_ascii_case("raw"))
        .map(ore_core)
        .unwrap_or_default();
    known.get(&last).into_iter().cloned().collect()
}

/// Where the ores a query names are mined. Empty when it names none.
pub fn where_mined(spawns: &MiningSpawns, areas: &[LootArea], query: &str) -> Vec<MinedReport> {
    ores_for(spawns, query)
        .into_iter()
        .map(|ore| report(spawns, areas, ore))
        .collect()
}

/// `f` of both when both are set, else the one that is.
fn merge_opt(a: Option<f64>, b: Option<f64>, f: impl Fn(f64, f64) -> f64) -> Option<f64> {
    match (a, b) {
        (Some(x), Some(y)) => Some(f(x, y)),
        (x, y) => x.or(y),
    }
}

fn report(spawns: &MiningSpawns, areas: &[LootArea], ore: String) -> MinedReport {
    let rocks: Vec<&MinedRock> = spawns
        .rocks
        .iter()
        .filter(|r| r.ores.iter().any(|o| o.element == ore))
        .collect();
    // One row per location and rock: a rock several spawn groups roll adds up, as
    // `game-mining-spawns` `ores[].belts[]` does. `oreSpawns` has a row per composition
    // part, so a rock listing the ore twice gives each spawn entry two rows: the entry
    // counts once (the rows' shares over the parts), and the percent range spans the parts.
    let parts_of = |rock: &str| {
        spawns.rocks.iter().find(|r| r.rock == rock).map_or(1, |r| {
            r.ores.iter().filter(|o| o.element == ore).count().max(1)
        })
    };
    let mut belts: Vec<MinedBelt> = Vec::new();
    let mut totals: Vec<Option<f64>> = Vec::new();
    for s in spawns.ore_spawns.iter().filter(|s| s.ore == ore) {
        let i = match belts
            .iter()
            .position(|b| b.location == s.location && b.rock == s.rock)
        {
            Some(i) => {
                let b = &mut belts[i];
                b.min_percent = merge_opt(b.min_percent, s.min_percent, f64::min);
                b.max_percent = merge_opt(b.max_percent, s.max_percent, f64::max);
                i
            }
            None => {
                belts.push(MinedBelt {
                    location: s.location.clone(),
                    system: s.system.clone(),
                    body: s.body.clone(),
                    rock: s.rock.clone(),
                    kind: s.kind.clone(),
                    tier: s.tier.clone(),
                    spawn_share: None,
                    min_percent: s.min_percent,
                    max_percent: s.max_percent,
                });
                totals.push(None);
                belts.len() - 1
            }
        };
        totals[i] = merge_opt(totals[i], s.spawn_share, |x, y| x + y);
    }
    for (b, t) in belts.iter_mut().zip(totals) {
        // Entry shares are on a 1e-5 grid; the dataset rounds its sums to it too.
        b.spawn_share = t.map(|t| round_to(t / parts_of(&b.rock) as f64, 1e5));
    }
    belts.sort_by(|a, b| {
        b.spawn_share
            .unwrap_or(0.0)
            .total_cmp(&a.spawn_share.unwrap_or(0.0))
            .then_with(|| a.location.cmp(&b.location))
    });
    let names: Vec<String> = rocks.iter().map(|r| r.rock.to_lowercase()).collect();
    let mut spots = Vec::new();
    for a in areas {
        let mut configs = Vec::new();
        let mut best: Option<(f64, String)> = None;
        for c in &a.configs {
            let total: f64 = c.slots.iter().map(|s| s.weight.max(0.0)).sum();
            for s in &c.slots {
                if !names.iter().any(|n| s.entity.eq_ignore_ascii_case(n)) {
                    continue;
                }
                let share = if total > 0.0 { s.weight / total } else { 0.0 };
                let name = c.name.clone().unwrap_or_else(|| "(unnamed)".into());
                if !configs.contains(&name) {
                    configs.push(name);
                }
                if best.as_ref().is_none_or(|(b, _)| share > *b) {
                    best = Some((share, s.entity.clone()));
                }
            }
        }
        if let Some((slot_share, entity)) = best {
            let rock = rocks
                .iter()
                .find(|r| r.rock.eq_ignore_ascii_case(&entity))
                .map_or(entity, |r| r.rock.clone());
            spots.push(MinedSpot {
                area: area_label(&a.name),
                place: area_place(&a.name).map(str::to_string),
                configs,
                rock,
                slot_share,
            });
        }
    }
    spots.sort_by(|a, b| {
        b.slot_share
            .total_cmp(&a.slot_share)
            .then_with(|| a.area.cmp(&b.area))
    });
    MinedReport {
        // Not the rock's deposit: on ship rocks that is the asteroid type.
        name: title(&ore_core(&ore)),
        rocks: rocks.iter().map(|r| r.rock.clone()).collect(),
        ore,
        belts,
        spots,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::types::{SpawnConfig, SpawnSlot};

    fn spawns() -> MiningSpawns {
        serde_json::from_value(serde_json::json!({
            "rocks": [
                {"rock": "MineableRock_FPS_Sadaryx", "kind": "hand", "deposit": "Sadaryx",
                 "ores": [{"element": "minableelement_fps_sadaryx"}]},
                {"rock": "MineableRock_AsteroidRare_Quantainium", "kind": "ship", "tier": "Rare",
                 "ores": [{"element": "quantainium_raw"}, {"element": "iron_ore"}]}
            ],
            "oreSpawns": [
                {"ore": "quantainium_raw", "location": "Stanton1", "system": "Stanton", "body": "Hurston",
                 "rock": "MineableRock_AsteroidRare_Quantainium", "kind": "ship", "spawnShare": 0.01},
                {"ore": "quantainium_raw", "location": "Stanton1", "system": "Stanton", "body": "Hurston",
                 "rock": "MineableRock_AsteroidRare_Quantainium", "kind": "ship", "spawnShare": 0.02},
                {"ore": "quantainium_raw", "location": "Pyro2", "system": "Pyro",
                 "rock": "MineableRock_AsteroidRare_Quantainium", "kind": "ship", "spawnShare": 0.05}
            ]
        }))
        .unwrap()
    }

    fn area(name: &str, config: &str, slots: &[(&str, f64)]) -> LootArea {
        LootArea {
            name: name.into(),
            configs: vec![SpawnConfig {
                name: Some(config.into()),
                slots: slots
                    .iter()
                    .map(|(e, w)| SpawnSlot {
                        entity: (*e).into(),
                        weight: *w,
                        ..Default::default()
                    })
                    .collect(),
                ..Default::default()
            }],
        }
    }

    #[test]
    fn cores() {
        assert_eq!(
            ore_core("MineableElement.MinableElement_FPS_Sadaryx"),
            "sadaryx"
        );
        assert_eq!(ore_core("quantainium_raw"), "quantainium");
        assert_eq!(ore_core("minableelement_groundvehicle_beradom"), "beradom");
        assert_eq!(ore_core("Gold"), "gold");
    }

    #[test]
    fn names_resolve() {
        let s = spawns();
        for q in [
            "Sadaryx",
            "ResourceType.Sadaryx",
            "Carryable_TBO_FL_1SCU_Commodity_Mineral_Sadaryx",
            "MineableRock_FPS_Sadaryx",
            "EntityClassDefinition.MineableRock_FPS_Sadaryx",
        ] {
            assert_eq!(ores_for(&s, q), ["minableelement_fps_sadaryx"], "{q}");
        }
        assert_eq!(ores_for(&s, "Quantainium (Raw)"), Vec::<String>::new());
        assert_eq!(ores_for(&s, "quantainium"), ["quantainium_raw"]);
        assert!(ores_for(&s, "Arrowhead").is_empty());
        assert!(
            ores_for(&s, "rrs_armor_sadaryx").is_empty(),
            "an item, not the ore"
        );
        assert_eq!(
            ores_for(
                &s,
                "EntityClassDefinition.MineableRock_AsteroidRare_Quantainium"
            ),
            ["quantainium_raw", "iron_ore"]
        );
        assert!(ores_for(&s, "  ").is_empty());
    }

    #[test]
    fn spot_only_ore_reports_its_area_and_place() {
        let areas = [
            area(
                "v3slotpreset_rockcracker",
                "Sadaryx minable",
                &[("mineablerock_fps_sadaryx", 1.0)],
            ),
            area("cave_x", "Loose", &[("crate", 3.0), ("other", 1.0)]),
        ];
        let r = where_mined(&spawns(), &areas, "Sadaryx");
        assert_eq!(r.len(), 1);
        let r = &r[0];
        assert_eq!(r.name, "Sadaryx");
        assert!(r.belts.is_empty());
        assert_eq!(r.spots.len(), 1);
        assert_eq!(r.spots[0].area, "v3slotpreset rockcracker");
        assert_eq!(r.spots[0].place.as_deref(), Some("Rock Cracker, Nyx"));
        assert_eq!(r.spots[0].configs, ["Sadaryx minable"]);
        assert_eq!(r.spots[0].rock, "MineableRock_FPS_Sadaryx");
        assert_eq!(r.spots[0].slot_share, 1.0);
    }

    #[test]
    fn belts_most_common_first_and_shares() {
        let areas = [area(
            "cave_y",
            "FPS mineables",
            &[("mineablerock_asteroidrare_quantainium", 1.0), ("x", 3.0)],
        )];
        let r = &where_mined(&spawns(), &areas, "MineableRock_AsteroidRare_Quantainium");
        assert_eq!(r.len(), 2, "a rock names each of its ores");
        let q = &r[0];
        assert_eq!(q.ore, "quantainium_raw");
        assert_eq!(q.name, "Quantainium");
        assert_eq!(
            q.belts
                .iter()
                .map(|b| b.location.as_str())
                .collect::<Vec<_>>(),
            ["Pyro2", "Stanton1"]
        );
        assert!(
            (q.belts[1].spawn_share.unwrap() - 0.03).abs() < 1e-9,
            "groups add up"
        );
        assert_eq!(q.spots[0].slot_share, 0.25);
        assert_eq!(q.spots[0].place, None);
        assert_eq!(r[1].ore, "iron_ore");
        assert!(r[1].belts.is_empty());
    }

    /// LIVE compositions list an element twice (Quantainium at 5.7-11.7 and 28.3-78.3):
    /// `oreSpawns` has a row per part, the spawn entry counts once.
    #[test]
    fn a_rock_holding_an_ore_in_two_parts_counts_its_spawn_once() {
        let s: MiningSpawns = serde_json::from_value(serde_json::json!({
            "rocks": [
                {"rock": "MineableRock_SurfaceLegendary_Quantainium", "kind": "ship",
                 "ores": [{"element": "quantainium_raw"}, {"element": "quantainium_raw"},
                          {"element": "iron_ore"}]}
            ],
            "oreSpawns": [
                {"ore": "quantainium_raw", "location": "Stanton4a",
                 "rock": "MineableRock_SurfaceLegendary_Quantainium", "kind": "ship",
                 "spawnShare": 0.12012, "minPercent": 5.7, "maxPercent": 11.7},
                {"ore": "quantainium_raw", "location": "Stanton4a",
                 "rock": "MineableRock_SurfaceLegendary_Quantainium", "kind": "ship",
                 "spawnShare": 0.12012, "minPercent": 28.3, "maxPercent": 78.3},
                {"ore": "quantainium_raw", "location": "Stanton1",
                 "rock": "MineableRock_SurfaceLegendary_Quantainium", "kind": "ship",
                 "spawnShare": 0.1, "minPercent": 5.7, "maxPercent": 11.7},
                {"ore": "quantainium_raw", "location": "Stanton1",
                 "rock": "MineableRock_SurfaceLegendary_Quantainium", "kind": "ship",
                 "spawnShare": 0.1, "minPercent": 28.3, "maxPercent": 78.3},
                {"ore": "quantainium_raw", "location": "Stanton1",
                 "rock": "MineableRock_SurfaceLegendary_Quantainium", "kind": "ship",
                 "spawnShare": 0.02, "minPercent": 5.7, "maxPercent": 11.7},
                {"ore": "quantainium_raw", "location": "Stanton1",
                 "rock": "MineableRock_SurfaceLegendary_Quantainium", "kind": "ship",
                 "spawnShare": 0.02, "minPercent": 28.3, "maxPercent": 78.3},
                {"ore": "iron_ore", "location": "Stanton4a",
                 "rock": "MineableRock_SurfaceLegendary_Quantainium", "kind": "ship",
                 "spawnShare": 0.12012, "minPercent": 1.0, "maxPercent": 2.0}
            ]
        }))
        .unwrap();
        let r = &where_mined(&s, &[], "quantainium")[0];
        let belts: Vec<_> = r
            .belts
            .iter()
            .map(|b| {
                (
                    b.location.as_str(),
                    b.spawn_share,
                    b.min_percent,
                    b.max_percent,
                )
            })
            .collect();
        assert_eq!(
            belts,
            [
                ("Stanton4a", Some(0.12012), Some(5.7), Some(78.3)),
                ("Stanton1", Some(0.12), Some(5.7), Some(78.3)),
            ],
            "one entry per spawn, two groups at Stanton1 summed"
        );
        let iron = &where_mined(&s, &[], "iron")[0];
        assert_eq!(
            iron.belts[0].spawn_share,
            Some(0.12012),
            "a single part is as before"
        );
    }
}
