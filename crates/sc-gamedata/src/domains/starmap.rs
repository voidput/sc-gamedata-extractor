//! Star map locations: systems, planets, moons, stations and points of interest (port of
//! `sc-gamedata/src/domains/starmap.ts`).
//!
//! A `StarMapObject` carries its display name, a parent reference forming the location
//! hierarchy, jurisdiction and amenities. Objects flagged `hideInStarmap` are kept and
//! flagged. Type and amenity references resolve to UUIDs; the readable names live in one
//! `StarMapObjectTypes` / `StarMapAmenityTypes` record each. Parent, jurisdiction and
//! affiliation are `file://` paths, resolved through the path index; following parents
//! up gives each location its chain from the system down (`path`).
//!
//! DataCore places nothing in space. Positions come from the solar system object
//! containers ([`super::socpak`]), joined by record id. Surface locations ride a rotating
//! body and get `local` (relative to the body) instead of a fixed `position`.
//!
//! The filter `**/entities/monitoredzone` never selects anything at extraction time (see
//! `pipeline`), so the monitored reach is always the 50,000 km default, as in the golden
//! dump; the lookup is ported anyway.

use crate::collate::compare_names;
use crate::domain::{Domain, DomainError, ParseContext, field};
use crate::domains::socpak::{BodySpin, ContainerZones, Containers, PlacedContainer};
use crate::helpers_world::{js_hypot, js_round, last_segment};
use crate::localization::{LocRefs, Localization, is_placeholder_name, loc_refs, localize};
use crate::output::js_key_order;
use crate::records::{GameRecord, RecordSet, array, at, num, ref_path, ref_short_name, str_};
use serde::Serialize;
use serde_json::{Map, Value, json};
use std::collections::{HashMap, HashSet};

pub static DOMAIN: Starmap = Starmap;

pub struct Starmap;

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Xyz {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuantumGeometry {
    /// A route passing closer than this is blocked by the object.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub obstruction_radius: Option<f64>,
    /// Ships travelling here drop out this far from the centre.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arrival_radius: Option<f64>,
    /// Within this range the object's own children become travel targets.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adoption_radius: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arrival_point_detection_offset: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub_point_radius_multiplier: Option<f64>,
}

/// One location. Field order is output order: the `parseLocation` literal, then the keys
/// `buildLocations` assigns afterwards, in assignment order.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Location {
    pub name: String,
    pub record_name: String,
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// `nameKey`, `descriptionKey`, `nameStatus`.
    #[serde(flatten)]
    pub loc: LocRefs,
    /// Record name of the containing body, forming the hierarchy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub ty: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub affiliation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jurisdiction: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_scannable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hidden_in_starmap: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amenities: Option<Vec<String>>,
    /// Quantum travel geometry, metres from the object's centre.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quantum: Option<QuantumGeometry>,
    /// Hours a planet or moon takes to turn once.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rotation_hours: Option<f64>,
    /// Radius of a planet's or moon's rotating zone, metres.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grid_radius: Option<f64>,
    /// Where a place turning with a body sits from the body's centre, metres.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local: Option<Xyz>,
    /// With `local`: record name of the body it turns with.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turns_with: Option<String>,
    /// Absolute position in the system frame, metres, star at the origin.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<Xyz>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub armistice_radius: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monitored_radius: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jammer_radius: Option<f64>,
    /// Names from the system down to the parent: ["Stanton", "Crusader", "Daymar"].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<Vec<String>>,
    /// The root of the chain, normally the star system.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
}

/// Used when the MonitoredZone entity record was not extracted.
const DEFAULT_MONITORED_KM: f64 = 50_000.0;
/// Used when the MonitorZoneJammer entity record was not extracted.
const DEFAULT_JAMMER_KM: f64 = 40.0;

/// `monitoredRadius` / `jammerRadius`: the `MonitoredZoneParams.radiusKm` of one entity.
fn entity_radius(records: &RecordSet, name: &str, default_km: f64) -> f64 {
    for record in records.iter().filter(|r| r.name == name) {
        for component in array(record.value.get("Components")) {
            let km = num(at(component, "radiusKm"));
            if at(component, "_Type_").and_then(Value::as_str) == Some("MonitoredZoneParams")
                && let Some(km) = km.filter(|k| *k != 0.0)
            {
                return km * 1000.0;
            }
        }
    }
    default_km * 1000.0
}

fn quantum_geometry(value: Option<&Value>) -> Option<QuantumGeometry> {
    let get = |k: &str| num(value.and_then(|v| at(v, k)));
    let g = QuantumGeometry {
        obstruction_radius: get("obstructionRadius"),
        arrival_radius: get("arrivalRadius"),
        adoption_radius: get("adoptionRadius"),
        arrival_point_detection_offset: get("arrivalPointDetectionOffset"),
        sub_point_radius_multiplier: get("subPointRadiusMultiplier"),
    };
    [
        g.obstruction_radius,
        g.arrival_radius,
        g.adoption_radius,
        g.arrival_point_detection_offset,
        g.sub_point_radius_multiplier,
    ]
    .iter()
    .any(Option::is_some)
    .then_some(g)
}

/// Metres to the millimetre: the containers carry float noise past that.
fn round_mm(value: f64) -> f64 {
    js_round(value * 1000.0) / 1000.0
}

fn short_or_str(value: &Value) -> Option<&str> {
    ref_short_name(Some(value))
        .filter(|s| !s.is_empty())
        .or_else(|| str_(Some(value)))
}

/// A type reference resolves through the lookup; the raw UUID is the fallback.
fn resolve_type(value: Option<&Value>, names: &HashMap<String, String>) -> Option<String> {
    let raw = short_or_str(value?)?;
    Some(names.get(raw).cloned().unwrap_or_else(|| raw.to_string()))
}

fn amenity_names(value: Option<&Value>, names: &HashMap<String, String>) -> Option<Vec<String>> {
    let entries = value?.as_array()?;
    let found: Vec<String> = entries
        .iter()
        .filter_map(short_or_str)
        .map(|n| names.get(n).cloned().unwrap_or_else(|| n.to_string()))
        .collect();
    (!found.is_empty()).then_some(found)
}

/// The id -> name lookup in every `<record_type>` record's `<list>` array.
fn lookup_names(records: &RecordSet, record_type: &str, list: &str) -> HashMap<String, String> {
    let mut names = HashMap::new();
    for record in records.of_type(record_type) {
        for entry in array(record.value.get(list)) {
            if let (Some(id), Some(name)) = (str_(entry.get("_RecordId_")), str_(entry.get("name"))) {
                names.insert(id.to_string(), name.to_string());
            }
        }
    }
    names
}

/// `buildTypeNames`.
pub fn build_type_names(records: &RecordSet) -> HashMap<String, String> {
    lookup_names(records, "StarMapObjectTypes", "types")
}

/// `buildAmenityNames`.
pub fn build_amenity_names(records: &RecordSet) -> HashMap<String, String> {
    lookup_names(records, "StarMapAmenityTypes", "amenityTypes")
}

/// `resolveRecord`: a reference that is a `file://` path, or an object carrying one in
/// `_RecordPath_`.
fn resolve_record<'a>(value: Option<&Value>, records: &'a RecordSet) -> Option<&'a GameRecord> {
    let value = value?;
    let path = ref_path(Some(value)).or_else(|| ref_path(value.get("_RecordPath_")))?;
    records.by_record_path(&path)
}

pub fn parse_location(
    record: &GameRecord,
    localization: &Localization,
    type_names: &HashMap<String, String>,
    records: &RecordSet,
    amenity_types: &HashMap<String, String>,
) -> Location {
    let v = &record.value;
    // Fall back to the referenced file's name when the target was not extracted.
    let name_of = |value: Option<&Value>| -> Option<String> {
        resolve_record(value, records)
            .map(|r| r.short_name.clone())
            .or_else(|| ref_short_name(value).map(str::to_string))
            .or_else(|| ref_path(value).map(|p| last_segment(&p).to_string()))
    };
    let display_name = localize(localization, v.get("name"));
    let description = localize(localization, v.get("description"));
    let placeholder = is_placeholder_name(display_name);
    Location {
        name: if placeholder {
            record.short_name.clone()
        } else {
            display_name.unwrap_or_default().to_string()
        },
        record_name: record.name.clone(),
        id: record.id.clone(),
        display_name: if placeholder {
            None
        } else {
            display_name.map(str::to_string)
        },
        description: if is_placeholder_name(description) {
            None
        } else {
            description.map(str::to_string)
        },
        loc: loc_refs(localization, v.get("name"), v.get("description")),
        parent: resolve_record(v.get("parent"), records)
            .map(|r| r.name.clone())
            .or_else(|| ref_short_name(v.get("parent")).map(str::to_string)),
        ty: resolve_type(v.get("type"), type_names),
        affiliation: name_of(v.get("affiliation")),
        jurisdiction: name_of(v.get("jurisdiction")),
        size: num(v.get("size")),
        is_scannable: v.get("isScannable").and_then(Value::as_bool),
        hidden_in_starmap: (v.get("hideInStarmap") == Some(&Value::Bool(true))).then_some(true),
        amenities: amenity_names(v.get("amenities"), amenity_types),
        quantum: quantum_geometry(v.get("quantumTravelData")),
        rotation_hours: None,
        grid_radius: None,
        local: None,
        turns_with: None,
        position: None,
        armistice_radius: None,
        monitored_radius: None,
        jammer_radius: None,
        path: None,
        system: None,
    }
}

fn is_body(l: &Location) -> bool {
    matches!(l.ty.as_deref(), Some("Planet" | "Moon"))
}

fn nonzero(x: Option<f64>) -> Option<f64> {
    x.filter(|v| *v != 0.0 && !v.is_nan())
}

/// `buildLocations`: every StarMapObject parsed, sorted, placed where the containers say,
/// with `path`/`system` filled in by walking parents up. Shared with mining spawns, which
/// calls it without containers.
pub fn build_locations(
    records: &RecordSet,
    localization: &Localization,
    placements: &HashMap<String, PlacedContainer>,
    zones: &HashMap<String, ContainerZones>,
    spins: &HashMap<String, BodySpin>,
) -> Vec<Location> {
    let monitored = if zones.is_empty() {
        0.0
    } else {
        entity_radius(records, "EntityClassDefinition.MonitoredZone", DEFAULT_MONITORED_KM)
    };
    let type_names = build_type_names(records);
    let amenity_types = build_amenity_names(records);

    let mut locations: Vec<Location> = records
        .of_type("StarMapObject")
        .map(|r| parse_location(r, localization, &type_names, records, &amenity_types))
        .collect();
    locations.sort_by(|a, b| compare_names(&a.name, &b.name));

    // `new Map(locations.map((l) => [l.id, l]))`: a later duplicate wins. Bodies are read
    // before anything is assigned to them below only through fields `parseLocation` set
    // (quantum, recordName), so a snapshot is equivalent.
    let by_id: HashMap<String, (String, Option<f64>)> = locations
        .iter()
        .map(|l| {
            (
                l.id.clone(),
                (
                    l.record_name.clone(),
                    l.quantum.as_ref().and_then(|q| q.obstruction_radius),
                ),
            )
        })
        .collect();

    for location in &mut locations {
        if let Some(spin) = spins.get(&location.id)
            && is_body(location)
        {
            location.rotation_hours = Some(spin.rotation_hours);
            if let Some(g) = nonzero(spin.grid_radius) {
                location.grid_radius = Some(g);
            }
        }

        let Some(placed) = placements.get(&location.id) else {
            continue;
        };
        let [x, y, z] = placed.position;

        // Anything inside its body's obstruction radius is on or near the surface, which
        // turns with the body.
        let body = placed.parent_record.as_ref().and_then(|p| by_id.get(p));
        if let (Some((body_record, body_radius)), Some([px, py, pz])) = (body, placed.parent_position)
        {
            let out = js_hypot(&[x - px, y - py, z - pz]);
            let grid = placed
                .parent_record
                .as_ref()
                .and_then(|p| spins.get(p))
                .and_then(|s| nonzero(s.grid_radius));
            if let Some(grid) = grid
                && out < grid
            {
                location.local = Some(Xyz {
                    x: round_mm(x - px),
                    y: round_mm(y - py),
                    z: round_mm(z - pz),
                });
                location.turns_with = Some(body_record.clone());
            }
            if let Some(r) = nonzero(*body_radius)
                && out < r
            {
                continue;
            }
        }
        location.position = Some(Xyz {
            x: round_mm(x),
            y: round_mm(y),
            z: round_mm(z),
        });

        // A planet's or moon's own container holds the zones of places on its surface.
        let zone = if is_body(location) {
            None
        } else {
            zones.get(&location.id)
        };
        if let Some(zone) = zone {
            if let Some(r) = nonzero(zone.armistice_radius) {
                location.armistice_radius = Some(r);
            }
            if zone.monitored {
                location.monitored_radius = Some(monitored);
            }
            if let Some(jammer) = zone.jammer {
                location.jammer_radius = Some(jammer.unwrap_or_else(|| {
                    entity_radius(records, "EntityClassDefinition.MonitorZoneJammer", DEFAULT_JAMMER_KM)
                }));
            }
        }
    }

    // Walk parents up to the root, guarding against a cycle in the data.
    let by_record: HashMap<String, (String, Option<String>)> = locations
        .iter()
        .map(|l| (l.record_name.clone(), (l.name.clone(), l.parent.clone())))
        .collect();
    for location in &mut locations {
        let mut chain: Vec<String> = Vec::new();
        let mut seen: HashSet<String> = HashSet::from([location.record_name.clone()]);
        let mut parent = location
            .parent
            .as_ref()
            .and_then(|p| by_record.get_key_value(p));
        while let Some((record_name, (name, next))) = parent {
            if seen.contains(record_name) {
                break;
            }
            seen.insert(record_name.clone());
            chain.insert(0, name.clone());
            parent = next.as_ref().and_then(|p| by_record.get_key_value(p));
        }
        if !chain.is_empty() {
            location.system = Some(chain[0].clone());
            location.path = Some(chain);
        }
    }
    locations
}

/// Obstruction radius of every extracted `starmap` record by id: what the TypeScript's
/// `obstructionRadii` read from `libs/foundry/records/starmap` (case-sensitive) while
/// extracting.
fn obstruction_radii(records: &RecordSet) -> HashMap<String, f64> {
    records
        .iter()
        .filter(|r| r.record_path.starts_with("libs/foundry/records/starmap/"))
        .filter_map(|r| {
            let radius = at(&r.value, "quantumTravelData.obstructionRadius")?.as_f64()?;
            Some((r.id.clone(), radius))
        })
        .collect()
}

impl Domain for Starmap {
    fn name(&self) -> &'static str {
        "game-starmap"
    }
    fn schema_version(&self) -> u32 {
        1
    }

    fn description(&self) -> &'static str {
        "Star map locations with their hierarchy, jurisdiction and amenities."
    }

    fn filters(&self) -> &'static [&'static str] {
        // Jurisdictions so a location's jurisdiction is named as game-law names it. The
        // MonitoredZone entity carries the reach every placed monitored zone inherits.
        &[
            "**/starmap/**",
            "**/starmapobject/**",
            "**/lawsystem/jurisdictions/**",
            "**/entities/monitoredzone",
            "**/mission_entities/monitorzonejammer",
            "**/starmapamenitytypes/**",
        ]
    }

    fn files(&self) -> &'static [&'static str] {
        // Solar system containers, the only source of positions; `moreFiles` follows.
        &["**/ObjectContainers/PU/system/*/*system.socpak"]
    }

    fn needs_localization(&self) -> bool {
        true
    }

    fn parse(&self, ctx: &ParseContext<'_>) -> Result<Map<String, Value>, DomainError> {
        let type_names = build_type_names(ctx.records);
        let radii = obstruction_radii(ctx.records);
        let mut containers = Containers::extract(ctx.files, self.files(), &radii);
        let placements = containers.load_placements(ctx.files);
        let zones = containers.load_zones(ctx.files, &placements);
        let spins = containers.load_spins(ctx.files, &placements);
        for w in containers.warnings.drain(..) {
            ctx.warn(w);
        }
        let locations = build_locations(ctx.records, ctx.localization, &placements, &zones, &spins);

        let visible: Vec<&Location> = locations
            .iter()
            .filter(|l| l.hidden_in_starmap != Some(true))
            .collect();
        let mut by_type = Map::new();
        for l in &visible {
            if let Some(t) = l.ty.as_ref().filter(|t| !t.is_empty()) {
                let n = by_type.get(t).and_then(Value::as_u64).unwrap_or(0);
                by_type.insert(t.clone(), json!(n + 1));
            }
        }
        // `Object.entries` (JS key order), stable sort by count, descending.
        let mut entries: Vec<(String, u64)> = js_key_order(&by_type)
            .into_iter()
            .map(|k| (k.clone(), by_type[k.as_str()].as_u64().unwrap_or(0)))
            .collect();
        entries.sort_by_key(|e| std::cmp::Reverse(e.1)); // stable, like JS sort
        let by_type: Map<String, Value> = entries.into_iter().map(|(k, n)| (k, json!(n))).collect();

        let count = |f: fn(&Location) -> bool| locations.iter().filter(|l| f(l)).count();
        let summary = json!({
            "locations": locations.len(),
            "visible": visible.len(),
            "hidden": locations.len() - visible.len(),
            "positioned": count(|l| l.position.is_some()),
            "turning": count(|l| l.local.is_some()),
            "rotating": count(|l| nonzero(l.rotation_hours).is_some()),
            "armistice": count(|l| nonzero(l.armistice_radius).is_some()),
            "monitored": count(|l| nonzero(l.monitored_radius).is_some()),
            "jammed": count(|l| nonzero(l.jammer_radius).is_some()),
            "typeNamesResolved": type_names.len(),
        });

        let mut body = Map::new();
        field(&mut body, "locations", &locations)?;
        body.insert("byType".into(), Value::Object(by_type));
        body.insert("summary".into(), summary);
        Ok(body)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::domains::socpak::tests::zip;
    use crate::fixtures::MemFiles;
    use crate::localization::parse_global_ini;
    use crate::records::normalize_record;

    pub(crate) fn rec(ty: &str, path: &str, value: Value, short: Option<&str>) -> GameRecord {
        let short = short.map_or_else(|| last_segment(path).to_string(), str::to_string);
        let mut v = value;
        v["_Type_"] = json!(ty);
        normalize_record(
            json!({"_RecordName_": format!("{ty}.{short}"), "_RecordId_": format!("id-{short}"),
                   "_RecordValue_": v}),
            &format!("libs/foundry/records/{path}"),
        )
        .unwrap()
    }

    pub(crate) fn file_ref(path: &str) -> Value {
        json!(format!("file://./../../../../libs/foundry/records/{path}.json"))
    }

    fn none<T>() -> HashMap<String, T> {
        HashMap::new()
    }

    // Case from sc-gamedata src/domains/links.test.ts.
    #[test]
    fn resolves_path_string_parents_up_to_the_system() {
        let set = RecordSet::new(vec![
            rec("StarMapObject", "starmap/pu/system/stanton/starmapobject.stanton", json!({"name": "Stanton"}), Some("Stanton")),
            rec(
                "StarMapObject",
                "starmap/pu/system/stanton/stanton3/starmapobject.stanton3",
                json!({"name": "ArcCorp", "parent": file_ref("starmap/pu/system/stanton/starmapobject.stanton")}),
                Some("Stanton3"),
            ),
            rec(
                "StarMapObject",
                "starmap/pu/system/stanton/stanton3a/starmapobject.stanton3a",
                json!({"name": "Lyria",
                       "parent": file_ref("starmap/pu/system/stanton/stanton3/starmapobject.stanton3"),
                       "jurisdiction": file_ref("lawsystem/jurisdictions/stanton/arccorp")}),
                Some("Stanton3a"),
            ),
            rec("Jurisdiction", "lawsystem/jurisdictions/stanton/arccorp", json!({}), Some("ArcCorp")),
        ]);
        let locations = build_locations(&set, &Localization::new(), &none(), &none(), &none());
        let lyria = locations.iter().find(|l| l.name == "Lyria").unwrap();
        assert_eq!(lyria.parent.as_deref(), Some("StarMapObject.Stanton3"));
        assert_eq!(lyria.path, Some(vec!["Stanton".to_string(), "ArcCorp".to_string()]));
        assert_eq!(lyria.system.as_deref(), Some("Stanton"));
        assert_eq!(lyria.jurisdiction.as_deref(), Some("ArcCorp"));
    }

    #[test]
    fn types_amenities_and_placeholders() {
        let set = RecordSet::new(vec![
            rec(
                "StarMapObjectTypes",
                "starmap/types",
                json!({"types": [{"_RecordId_": "u-1", "name": "Planet"}, null]}),
                None,
            ),
            rec(
                "StarMapAmenityTypes",
                "starmapamenitytypes/all",
                json!({"amenityTypes": [{"_RecordId_": "a-1", "name": "Refinery"}]}),
                None,
            ),
            rec(
                "StarMapObject",
                "starmap/pu/x",
                json!({"name": "@ph", "type": {"_RecordName_": "StarMapObjectType.u-1"},
                       "amenities": [{"_RecordName_": "A.a-1"}, "raw-2", ""], "hideInStarmap": true,
                       "isScannable": false, "quantumTravelData": {"obstructionRadius": 5}}),
                Some("X"),
            ),
        ]);
        let loc = parse_global_ini("ph=<= PLACEHOLDER =>");
        let l = &build_locations(&set, &loc, &none(), &none(), &none())[0];
        assert_eq!(l.name, "X");
        assert_eq!(l.display_name, None);
        assert_eq!(l.ty.as_deref(), Some("Planet"));
        assert_eq!(l.amenities, Some(vec!["Refinery".to_string(), "raw-2".to_string()]));
        assert_eq!(l.hidden_in_starmap, Some(true));
        assert_eq!(l.is_scannable, Some(false));
        assert_eq!(l.quantum.as_ref().unwrap().obstruction_radius, Some(5.0));
    }

    #[test]
    fn places_from_containers_and_keeps_surface_places_local() {
        let system = r#"<ObjectContainer><ChildObjectContainers>
            <Child external="1" name="Data/ObjectContainers/PU/loc/planet.socpak" entityName="P" pos="1000000,0,0" starMapRecord="id-Planet"/>
            <Child external="1" name="Data/ObjectContainers/PU/loc/station.socpak" entityName="S" pos="0,5000,0.0004" starMapRecord="id-Station"/>
          </ChildObjectContainers></ObjectContainer>"#;
        let planet = r#"<ObjectContainer gridRadius="5000"><Entity planetRotationSpeed="3"/><ChildObjectContainers>
            <Child entityName="O" pos="10,0,0" starMapRecord="id-Outpost"/></ChildObjectContainers></ObjectContainer>"#;
        let mut f = MemFiles::default();
        f.insert(
            r"Data\ObjectContainers\PU\system\x\xsystem.socpak",
            zip(&[("xsystem.xml", None, system.as_bytes(), true)]),
        );
        f.insert(
            r"Data\ObjectContainers\PU\loc\planet.socpak",
            zip(&[("planet.xml", None, planet.as_bytes(), false)]),
        );
        f.insert(
            r"Data\ObjectContainers\PU\loc\station.socpak",
            zip(&[
                ("station.xml", None, b"<ObjectContainer/>", false),
                ("station_editor.xml", None, br#"<Object name="z" type="MonitoredZone"/>"#, false),
            ]),
        );
        let set = RecordSet::new(vec![
            rec("StarMapObjectTypes", "starmap/types", json!({"types": [{"_RecordId_": "t", "name": "Planet"}]}), None),
            rec(
                "StarMapObject",
                "starmap/pu/planet",
                json!({"name": "Planet", "type": "t", "quantumTravelData": {"obstructionRadius": 1000}}),
                Some("Planet"),
            ),
            rec("StarMapObject", "starmap/pu/outpost", json!({"name": "Outpost"}), Some("Outpost")),
            rec("StarMapObject", "starmap/pu/station", json!({"name": "Station", "hideInStarmap": true}), Some("Station")),
        ]);
        let loc = Localization::new();
        let out = DOMAIN.parse(&ParseContext::new(&set, &loc, &f)).unwrap();
        let by = |n: &str| {
            out["locations"]
                .as_array()
                .unwrap()
                .iter()
                .find(|l| l["name"] == n)
                .unwrap()
                .clone()
        };
        assert_eq!(by("Planet")["rotationHours"], json!(3.0));
        assert_eq!(by("Planet")["gridRadius"], json!(5000.0));
        assert_eq!(by("Planet")["position"], json!({"x": 1000000.0, "y": 0.0, "z": 0.0}));
        let outpost = by("Outpost");
        assert_eq!(outpost["local"], json!({"x": 10.0, "y": 0.0, "z": 0.0}));
        assert_eq!(outpost["turnsWith"], "StarMapObject.Planet");
        assert!(outpost.get("position").is_none(), "inside the obstruction radius");
        let station = by("Station");
        assert_eq!(station["position"], json!({"x": 0.0, "y": 5000.0, "z": 0.0}));
        assert_eq!(station["monitoredRadius"], json!(50_000_000.0));
        assert_eq!(out["byType"], json!({"Planet": 1}));
        assert_eq!(out["summary"]["positioned"], 2);
        assert_eq!(out["summary"]["turning"], 1);
        assert_eq!(out["summary"]["monitored"], 1);
        assert_eq!(out["summary"]["hidden"], 1);
        assert_eq!(out["summary"]["typeNamesResolved"], 1);
    }
}
