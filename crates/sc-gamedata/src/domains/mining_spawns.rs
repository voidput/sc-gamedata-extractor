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

use crate::collate::compare_names;
use crate::domain::{Domain, DomainError, ParseContext, field};
use crate::domains::starmap::{Location, build_locations};
use crate::entity::component;
use crate::helpers_world::{js_round, last_segment};
use crate::localization::{Localization, localize, lookup_string};
use crate::output::js_key_order;
use crate::records::{GameRecord, RecordSet, array, at, num, str_};
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
            locations.push(SpawnLocation {
                system: place.and_then(|p| p.system.clone()).or(provider.system),
                body: provider.body.or_else(|| place.map(|p| p.name.clone())),
                starmap_record: place.map(|p| p.record_name.clone()),
                path: place.and_then(|p| p.path.clone()),
                key: provider.key,
                record_name: record.name.clone(),
                entries,
                ores: sorted_names(ores),
            });
        }
        locations.sort_by(|a, b| compare_names(&a.key, &b.key));

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

        // One row per ore per spawn entry.
        let mut ore_spawns: Vec<OreSpawn> = locations
            .iter()
            .flat_map(|l| {
                l.entries.iter().flat_map(move |e| {
                    e.ores.iter().flatten().map(move |o| OreSpawn {
                        ore: o.element.clone(),
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

        let summary = json!({
            "locations": locations.len(),
            "locationsWithOres": resolved,
            "locationsNamed": locations.iter().filter(|l| l.body.as_deref().is_some_and(|b| !b.is_empty())).count(),
            "distinctOres": distinct_ores,
            "spawnEntries": locations.iter().map(|l| l.entries.len()).sum::<usize>(),
            "rocks": rocks.len(),
            "rocksWithSignature": rocks.iter().filter(|r| r.signature.is_some_and(|s| s != 0.0)).count(),
        });

        let mut body = Map::new();
        field(&mut body, "locations", &locations)?;
        field(&mut body, "rocks", &rocks)?;
        field(&mut body, "oreSpawns", &ore_spawns)?;
        body.insert("oreLocations".into(), Value::Object(ore_locations));
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
}
