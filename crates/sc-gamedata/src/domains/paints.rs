//! Ship paints (liveries) and the vehicles each one fits, by the game's own port rule.
//!
//! A paint is an `EntityClassDefinition` under `entities/scitem/ships/` whose
//! `AttachDef.Type` is `Paints`. It goes into a vehicle's paint item port
//! (`hardpoint_paint`), and the port decides what fits through tags: the port's required
//! tags must all be on the paint, and the paint's own `RequiredTags` must all be offered by
//! the port's tags or the vehicle's own `AttachDef.Tags` (see [`fits`] and [`RULE`]).
//!
//! The paint port is defined in one of two places. Most vehicles (1,039 of the 1,060 with
//! a paint port on LIVE 4.10.201.48085) define it in their `VehicleComponentParams.vehicleDefinition` XML, an
//! archive file under `Scripts/Entities/Vehicles/Implementations/Xml/`, as a `<Part>`
//! whose `<ItemPort>` lists `<Type type="Paints"/>`. A vehicle record that names a
//! `modification` takes that `<Modification>` of the file: its `patchFile` (another file,
//! under `Modifications/`) replaces whole sections, and its `<Elem idRef name value>`
//! entries set one attribute on the element with that `id` (the Idris, Ballista, Pisces,
//! Starlifter, Cutlass, Mustang Omega and Apollo/Hermes variants change their paint port's
//! tags this way). The rest (21 on LIVE: 890 Jump, Gladiator, Paladin) carry the port on the
//! entity itself, in `SItemPortContainerComponentParams.Ports`. No vehicle has both.
//!
//! Names are resolved as the other item domains resolve them ([`item_identity`]), and
//! vehicle names are exactly game-ships' ([`parse_vehicle`]). Templates (a placeholder
//! name, or `template` in the record name, as game-consumables skips them) are left out.
//! Only keys, names and tags are written: no texture, material or icon paths, and no
//! description prose (`descriptionKey` only).

use crate::collate::compare_names;
use crate::domain::{Domain, DomainError, FileSource, ParseContext, field, files_matching};
use crate::domains::manufacturers::{is_manufacturer_record, names_by_code, parse_manufacturer};
use crate::domains::ships::{is_vehicle_path, parse_vehicle};
use crate::entity::component;
use crate::helpers_items::item_common::{attach_def, item_identity};
use crate::localization::{LocRefs, Localization, is_placeholder_name, loc_key, localize};
use crate::records::{GameRecord, array, num, str_};
use crate::cryxml::XmlNode;
use crate::xml::read_document;
use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashMap};

pub static DOMAIN: Paints = Paints;

pub struct Paints;

/// The vehicle definition XML files and their `Modifications/` patch files.
const GLOBS: &[&str] = &["**/Scripts/Entities/Vehicles/Implementations/Xml/**"];

/// The compatibility rule [`fits`] implements, as written to `_assumptions`.
pub const RULE: &str = "Compatibility rule: a paint fits a vehicle when the vehicle has a paint item port (an item port whose Types list Paints) and (1) the paint's AttachDef.Type is a type the port lists, and its SubType is among that type's subtypes when the port names any; (2) when the port gives minSize/maxSize, the paint's AttachDef.Size lies within them; (3) every tag in the port's required tags (vehicle XML ItemPort requiredTags, entity SItemPortDef RequiredPortTags) is in the paint's AttachDef.Tags; and (4) every tag in the paint's AttachDef.RequiredTags is in the port's own tags (XML portTags, entity PortTags) or the vehicle's AttachDef.Tags. Tags are the whitespace-separated words of those strings, compared without regard to case; a word starting with @ is kept as written and matches only the same word.";

/// What else the output rests on that the records do not state.
pub const ASSUMPTIONS: &[&str] = &[
    RULE,
    "Paint ports come from the vehicle entity's SItemPortContainerComponentParams.Ports when it has a Paints port there, else from the XML file named by VehicleComponentParams.vehicleDefinition (matched without regard to case). For a vehicle whose VehicleComponentParams.modification names a <Modification> of that file: each top-level element of its patchFile (resolved beside the file, .xml added) replaces the element with the same id, else the vehicle's first child with the same tag; then each <Elem idRef name value> sets attribute name to value on the element whose id is idRef. A Part with skipPart=\"1\" (after that) and everything under it is left out. This is how the files read; the engine code that applies them is not in the data.",
    "Comparing tags without regard to case changes one result on LIVE 4.10.201.48085: the Argo MOTH's port asks for Paint_MOTH while its paints carry Paint_Moth. Compared exactly, those paints would fit no vehicle.",
    "manufacturer: the paint's AttachDef.Manufacturer. When it points at a paintcolorlogos record rather than a manufacturer, manufacturerName is that record's localised name and manufacturer is the code of the one manufacturer record with the same name key; it is left out when several codes share the name (Aegis Dynamics: AEGS, FSKI, ...).",
];

/// A paint's compatibility tags.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PaintTags {
    pub ty: String,
    pub sub_type: String,
    pub size: Option<f64>,
    pub tags: Vec<String>,
    pub required_tags: Vec<String>,
}

/// One paint item port on a vehicle, after any modification.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaintPort {
    pub port: String,
    /// `entity` (SItemPortContainerComponentParams) or `vehicleDefinition` (the XML).
    pub source: &'static str,
    pub port_tags: Vec<String>,
    pub required_tags: Vec<String>,
    #[serde(skip)]
    pub min_size: Option<f64>,
    #[serde(skip)]
    pub max_size: Option<f64>,
    /// `(type, subtypes)` the port accepts; empty subtypes accept any.
    #[serde(skip)]
    pub types: Vec<(String, Vec<String>)>,
}

/// Whitespace-separated tag words.
pub fn tag_words(s: Option<&str>) -> Vec<String> {
    s.unwrap_or_default()
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

fn has_tag(pool: &[&[String]], tag: &str) -> bool {
    pool.iter()
        .any(|set| set.iter().any(|t| t.eq_ignore_ascii_case(tag)))
}

/// The game's port rule ([`RULE`]): does `paint` fit `port` on a vehicle with `vehicle_tags`?
pub fn fits(paint: &PaintTags, port: &PaintPort, vehicle_tags: &[String]) -> bool {
    let type_ok = port.types.iter().any(|(ty, subs)| {
        ty.eq_ignore_ascii_case(&paint.ty)
            && (subs.is_empty() || subs.iter().any(|s| s.eq_ignore_ascii_case(&paint.sub_type)))
    });
    let size = paint.size.unwrap_or(0.0);
    let size_ok =
        port.min_size.is_none_or(|m| size >= m) && port.max_size.is_none_or(|m| size <= m);
    let required_on_paint = port
        .required_tags
        .iter()
        .all(|t| has_tag(&[&paint.tags], t));
    let offered = paint
        .required_tags
        .iter()
        .all(|t| has_tag(&[&port.port_tags, vehicle_tags], t));
    type_ok && size_ok && required_on_paint && offered
}

/// Paint ports on the entity: `SItemPortContainerComponentParams.Ports` typed `Paints`.
pub fn entity_paint_ports(record: &GameRecord) -> Vec<PaintPort> {
    let Some(container) = component(record, "SItemPortContainerComponentParams") else {
        return Vec::new();
    };
    array(container.get("Ports"))
        .iter()
        .filter_map(|p| {
            let types: Vec<(String, Vec<String>)> = array(p.get("Types"))
                .iter()
                .filter_map(|t| {
                    let ty = str_(t.get("Type"))?;
                    let subs = array(t.get("SubTypes"))
                        .iter()
                        .filter_map(|s| str_(Some(s)).or_else(|| str_(s.get("value"))))
                        .map(str::to_string)
                        .collect();
                    Some((ty.to_string(), subs))
                })
                .collect();
            types.iter().any(|(t, _)| t == "Paints").then(|| PaintPort {
                port: str_(p.get("Name")).unwrap_or_default().to_string(),
                source: "entity",
                port_tags: tag_words(str_(p.get("PortTags"))),
                required_tags: tag_words(str_(p.get("RequiredPortTags"))),
                min_size: num(p.get("MinSize")),
                max_size: num(p.get("MaxSize")),
                types,
            })
        })
        .collect()
}

fn set_attr(node: &mut XmlNode, key: &str, value: &str) {
    match node.attrs.iter_mut().rev().find(|(k, _)| k == key) {
        Some(slot) => slot.1 = value.to_string(),
        None => node.attrs.push((key.to_string(), value.to_string())),
    }
}

fn find_by_id_mut<'a>(node: &'a mut XmlNode, id: &str) -> Option<&'a mut XmlNode> {
    if node.attr("id") == Some(id) {
        return Some(node);
    }
    node.children
        .iter_mut()
        .find_map(|c| find_by_id_mut(c, id))
}

/// Lay a patch file's sections over the definition (see [`ASSUMPTIONS`]).
pub fn apply_patch(root: &mut XmlNode, patch: &XmlNode) {
    for section in &patch.children {
        if let Some(id) = section.attr("id")
            && let Some(target) = find_by_id_mut(root, id)
        {
            *target = section.clone();
            continue;
        }
        if let Some(target) = root.children.iter_mut().find(|c| c.tag == section.tag) {
            *target = section.clone();
        }
    }
}

/// `<Elem idRef name value>`: set one attribute on the element with that id.
pub fn apply_elems(root: &mut XmlNode, modification: &XmlNode) {
    for elems in modification.children.iter().filter(|c| c.tag == "Elems") {
        for elem in elems.children.iter().filter(|c| c.tag == "Elem") {
            let (Some(id), Some(name)) = (elem.attr("idRef"), elem.attr("name")) else {
                continue;
            };
            if let Some(target) = find_by_id_mut(root, id) {
                set_attr(target, name, elem.attr("value").unwrap_or_default());
            }
        }
    }
}

/// The `<Modification name=...>` of a definition, cloned.
pub fn modification_of(root: &XmlNode, name: &str) -> Option<XmlNode> {
    root.children
        .iter()
        .filter(|c| c.tag == "Modifications")
        .flat_map(|m| m.children.iter())
        .find(|m| m.tag == "Modification" && m.attr("name").is_some_and(|n| n == name))
        .cloned()
}

/// Paint ports in a (modified) definition: `<Part>`s whose `<ItemPort>` lists `Paints`,
/// skipping parts marked `skipPart="1"`.
pub fn xml_paint_ports(root: &XmlNode) -> Vec<PaintPort> {
    fn walk(node: &XmlNode, out: &mut Vec<PaintPort>) {
        if node.tag == "Modifications" {
            return;
        }
        if node.tag == "Part" {
            if node.attr("skipPart") == Some("1") {
                return;
            }
            if let Some(port) = node.children.iter().find(|c| c.tag == "ItemPort") {
                let types: Vec<(String, Vec<String>)> = port
                    .children
                    .iter()
                    .filter(|c| c.tag == "Types")
                    .flat_map(|t| t.children.iter())
                    .filter(|t| t.tag == "Type")
                    .filter_map(|t| {
                        let subs = t
                            .attr("subtypes")
                            .unwrap_or_default()
                            .split(',')
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .map(str::to_string)
                            .collect();
                        Some((t.attr("type")?.to_string(), subs))
                    })
                    .collect();
                if types.iter().any(|(t, _)| t == "Paints") {
                    let size = |k: &str| port.attr(k).and_then(|v| v.trim().parse::<f64>().ok());
                    out.push(PaintPort {
                        port: node.attr("name").unwrap_or_default().to_string(),
                        source: "vehicleDefinition",
                        port_tags: tag_words(port.attr("portTags")),
                        required_tags: tag_words(port.attr("requiredTags")),
                        min_size: size("minSize"),
                        max_size: size("maxSize"),
                        types,
                    });
                }
            }
        }
        for c in &node.children {
            walk(c, out);
        }
    }
    let mut out = Vec::new();
    walk(root, &mut out);
    out
}

/// Archive name as a lookup key: lower case, `/`, no leading `data/`.
fn file_key(name: &str) -> String {
    let n = name.replace('\\', "/").to_ascii_lowercase();
    let n = n.trim_start_matches('/');
    n.strip_prefix("data/").unwrap_or(n).to_string()
}

/// What reading a vehicle's definition came to.
#[derive(Debug, Default)]
struct XmlRead {
    ports: Vec<PaintPort>,
    missing_file: bool,
    missing_patch: bool,
}

/// The vehicle definitions, read once each.
struct Definitions<'a> {
    files: &'a dyn FileSource,
    names: HashMap<String, &'a str>,
    cache: HashMap<String, Option<XmlNode>>,
}

impl<'a> Definitions<'a> {
    fn new(files: &'a dyn FileSource) -> Self {
        let names = files_matching(files, GLOBS)
            .into_iter()
            .map(|n| (file_key(n), n))
            .collect();
        Self {
            files,
            names,
            cache: HashMap::new(),
        }
    }

    fn load(&mut self, key: &str) -> Option<XmlNode> {
        if let Some(hit) = self.cache.get(key) {
            return hit.clone();
        }
        let tree = self
            .names
            .get(key)
            .and_then(|n| self.files.read(n).ok())
            .and_then(|b| read_document(&b).ok());
        self.cache.insert(key.to_string(), tree.clone());
        tree
    }

    fn ports(&mut self, definition: &str, modification: Option<&str>) -> XmlRead {
        let key = file_key(definition);
        let Some(mut root) = self.load(&key) else {
            return XmlRead {
                missing_file: true,
                ..XmlRead::default()
            };
        };
        let mut missing_patch = false;
        if let Some(m) = modification.and_then(|name| modification_of(&root, name)) {
            if let Some(patch) = m.attr("patchFile").filter(|p| !p.is_empty()) {
                let dir = key.rsplit_once('/').map_or("", |(d, _)| d);
                let patch_key = file_key(&format!("{dir}/{patch}.xml"));
                match self.load(&patch_key) {
                    Some(p) => apply_patch(&mut root, &p),
                    None => missing_patch = true,
                }
            }
            apply_elems(&mut root, &m);
        }
        XmlRead {
            ports: xml_paint_ports(&root),
            missing_file: false,
            missing_patch,
        }
    }
}

/// A vehicle a paint fits.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FitVehicle {
    pub class_name: String,
    pub name: String,
}

/// One row of `paints`. Field order is output order.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Paint {
    pub class_name: String,
    pub record_name: String,
    pub id: String,
    pub name: String,
    /// `nameKey`, `descriptionKey`, `nameStatus`.
    #[serde(flatten)]
    pub loc: LocRefs,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manufacturer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manufacturer_name: Option<String>,
    #[serde(rename = "type")]
    pub ty: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grade: Option<f64>,
    /// `AttachDef.Tags`, as words.
    pub tags: Vec<String>,
    /// `AttachDef.RequiredTags`, as words.
    pub required_tags: Vec<String>,
    pub vehicles: Vec<FitVehicle>,
}

/// One row of `paintPorts`: a vehicle's paint port and the tags it matches on.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VehiclePorts {
    pub class_name: String,
    pub name: String,
    /// The vehicle's `AttachDef.Tags`, as words.
    pub vehicle_tags: Vec<String>,
    pub ports: Vec<PaintPort>,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct Summary {
    paints: usize,
    templates_skipped: usize,
    paints_fitting_no_vehicle: usize,
    vehicles: usize,
    vehicles_with_paint_port: usize,
    vehicles_with_no_paints: usize,
    vehicles_without_paint_port: usize,
    ports_from_entity: usize,
    ports_from_vehicle_definition: usize,
    missing_vehicle_definitions: usize,
    missing_patch_files: usize,
}

/// Is this record a paint item (`AttachDef.Type` `Paints` under `entities/scitem/ships/`)?
pub fn is_paint(record: &GameRecord) -> bool {
    record.ty == "EntityClassDefinition"
        && record
            .record_path
            .to_lowercase()
            .contains("/entities/scitem/ships/")
        && str_(attach_def(record).get("Type")) == Some("Paints")
}

/// The compatibility tags of a paint record.
pub fn paint_tags(record: &GameRecord) -> PaintTags {
    let def = attach_def(record);
    PaintTags {
        ty: str_(def.get("Type")).unwrap_or_default().to_string(),
        sub_type: str_(def.get("SubType")).unwrap_or_default().to_string(),
        size: num(def.get("Size")),
        tags: tag_words(str_(def.get("Tags"))),
        required_tags: tag_words(str_(def.get("RequiredTags"))),
    }
}

/// Manufacturer code and name of a paint (see [`ASSUMPTIONS`]).
struct Makers<'a> {
    by_code: HashMap<String, (String, String)>,
    codes_by_key: HashMap<String, Vec<String>>,
    localization: &'a Localization,
}

impl<'a> Makers<'a> {
    fn new(ctx: &'a ParseContext<'_>) -> Self {
        let by_code = names_by_code(ctx.records, ctx.localization);
        let mut codes_by_key: HashMap<String, Vec<String>> = HashMap::new();
        for r in ctx.records.of_type("SCItemManufacturer") {
            if !is_manufacturer_record(r) {
                continue;
            }
            let m = parse_manufacturer(r, ctx.localization);
            if let (Some(key), Some((code, _))) =
                (m.loc.name_key, by_code.get(&m.code.to_uppercase()))
            {
                codes_by_key
                    .entry(key.to_lowercase())
                    .or_default()
                    .push(code.clone());
            }
        }
        Self {
            by_code,
            codes_by_key,
            localization: ctx.localization,
        }
    }

    fn of(&self, ctx: &ParseContext<'_>, record: &GameRecord) -> (Option<String>, Option<String>) {
        let Some(maker) = ctx.records.follow(attach_def(record).get("Manufacturer")) else {
            return (None, None);
        };
        if is_manufacturer_record(maker) {
            return match self.by_code.get(&maker.short_name.to_uppercase()) {
                Some((code, name)) => (Some(code.clone()), Some(name.clone())),
                None => (None, None),
            };
        }
        let name_ref = maker.value.get("Localization").and_then(|l| l.get("Name"));
        let name = localize(self.localization, name_ref).filter(|n| !is_placeholder_name(Some(n)));
        let code = loc_key(self.localization, name_ref)
            .and_then(|k| self.codes_by_key.get(&k.to_lowercase()))
            .filter(|codes| codes.len() == 1)
            .map(|codes| codes[0].clone());
        (code, name.map(str::to_string))
    }
}

impl Domain for Paints {
    fn name(&self) -> &'static str {
        "game-paints"
    }
    fn schema_version(&self) -> u32 {
        1
    }

    fn description(&self) -> &'static str {
        "Ship and vehicle paints with the vehicles each fits, by the paint port's tag rule."
    }

    fn filters(&self) -> &'static [&'static str] {
        &[
            "**/entities/spaceships/**",
            "**/entities/groundvehicles/**",
            "**/entities/scitem/ships/**",
            crate::domains::manufacturers::MANUFACTURER_FILTER,
        ]
    }

    fn files(&self) -> &'static [&'static str] {
        GLOBS
    }

    fn needs_localization(&self) -> bool {
        true
    }

    /// Entities keep their `AttachDef`, vehicle parameters and item ports.
    fn prune(&self, record: &mut GameRecord) {
        if record.ty == "EntityClassDefinition" {
            crate::entity::prune_entity(
                &mut record.value,
                &[],
                &[
                    ("SAttachableComponentParams", Some(&["AttachDef"])),
                    ("VehicleComponentParams", None),
                    ("SItemPortContainerComponentParams", Some(&["Ports"])),
                ],
            );
        }
    }

    fn parse(&self, ctx: &ParseContext<'_>) -> Result<Map<String, Value>, DomainError> {
        let mut defs = Definitions::new(ctx.files);
        let mut summary = Summary::default();
        let mut missing_defs: Vec<String> = Vec::new();
        let mut missing_patches: Vec<String> = Vec::new();

        // Vehicles and their paint ports.
        let mut vehicles: Vec<VehiclePorts> = Vec::new();
        for r in ctx.records.iter().filter(|r| is_vehicle_path(&r.record_path)) {
            let Some(vehicle) = parse_vehicle(r, ctx.localization, None) else {
                continue;
            };
            summary.vehicles += 1;
            let mut ports = entity_paint_ports(r);
            if ports.is_empty() {
                let params = component(r, "VehicleComponentParams");
                let definition = params
                    .and_then(|p| str_(p.get("vehicleDefinition")))
                    .filter(|d| !d.is_empty());
                let modification = params
                    .and_then(|p| str_(p.get("modification")))
                    .filter(|m| !m.is_empty());
                if let Some(definition) = definition {
                    let read = defs.ports(definition, modification);
                    if read.missing_file {
                        missing_defs.push(r.short_name.clone());
                    }
                    if read.missing_patch {
                        missing_patches.push(r.short_name.clone());
                    }
                    ports = read.ports;
                }
            }
            if ports.is_empty() {
                summary.vehicles_without_paint_port += 1;
                continue;
            }
            for p in &ports {
                if p.source == "entity" {
                    summary.ports_from_entity += 1;
                } else {
                    summary.ports_from_vehicle_definition += 1;
                }
            }
            vehicles.push(VehiclePorts {
                class_name: r.short_name.clone(),
                name: vehicle.name,
                vehicle_tags: tag_words(str_(attach_def(r).get("Tags"))),
                ports,
            });
        }
        vehicles.sort_by(|a, b| compare_names(&a.class_name, &b.class_name));
        summary.vehicles_with_paint_port = vehicles.len();
        summary.missing_vehicle_definitions = missing_defs.len();
        summary.missing_patch_files = missing_patches.len();
        if !missing_defs.is_empty() {
            ctx.warn(format!(
                "game-paints: {} vehicles name a vehicleDefinition the archive does not have (first: {})",
                missing_defs.len(),
                missing_defs[0]
            ));
        }
        if !missing_patches.is_empty() {
            ctx.warn(format!(
                "game-paints: {} vehicles' modification names a patchFile the archive does not have; base definition used (first: {})",
                missing_patches.len(),
                missing_patches[0]
            ));
        }

        // Paints, each matched against every paint port.
        let makers = Makers::new(ctx);
        let mut paints: Vec<Paint> = Vec::new();
        for r in ctx.records.iter().filter(|r| is_paint(r)) {
            let identity = item_identity(r, ctx.localization);
            if identity.is_template.is_some() || r.name.to_lowercase().contains("template") {
                summary.templates_skipped += 1;
                continue;
            }
            let tags = paint_tags(r);
            let mut fit: Vec<FitVehicle> = vehicles
                .iter()
                .filter(|v| v.ports.iter().any(|p| fits(&tags, p, &v.vehicle_tags)))
                .map(|v| FitVehicle {
                    class_name: v.class_name.clone(),
                    name: v.name.clone(),
                })
                .collect();
            fit.sort_by(|a, b| {
                compare_names(&a.name, &b.name).then_with(|| compare_names(&a.class_name, &b.class_name))
            });
            let (manufacturer, manufacturer_name) = makers.of(ctx, r);
            paints.push(Paint {
                class_name: r.short_name.clone(),
                record_name: r.name.clone(),
                id: r.id.clone(),
                name: identity.name,
                loc: identity.loc,
                manufacturer,
                manufacturer_name,
                ty: tags.ty,
                sub_type: identity.sub_type,
                size: identity.size,
                grade: identity.grade,
                tags: tags.tags,
                required_tags: tags.required_tags,
                vehicles: fit,
            });
        }
        paints.sort_by(|a, b| compare_names(&a.class_name, &b.class_name));
        summary.paints = paints.len();
        summary.paints_fitting_no_vehicle = paints.iter().filter(|p| p.vehicles.is_empty()).count();

        let mut by_vehicle: BTreeMap<&str, Vec<&str>> = vehicles
            .iter()
            .map(|v| (v.class_name.as_str(), Vec::new()))
            .collect();
        for p in &paints {
            for v in &p.vehicles {
                if let Some(list) = by_vehicle.get_mut(v.class_name.as_str()) {
                    list.push(&p.class_name);
                }
            }
        }
        summary.vehicles_with_no_paints = by_vehicle.values().filter(|l| l.is_empty()).count();
        let mut by_vehicle_out = Map::new();
        for v in &vehicles {
            let list = &by_vehicle[v.class_name.as_str()];
            by_vehicle_out.insert(v.class_name.clone(), serde_json::json!(list));
        }

        let mut body = Map::new();
        body.insert("_assumptions".into(), serde_json::json!(ASSUMPTIONS));
        field(&mut body, "paints", &paints)?;
        body.insert("byVehicle".into(), Value::Object(by_vehicle_out));
        field(&mut body, "paintPorts", &vehicles)?;
        field(&mut body, "summary", &summary)?;
        Ok(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::MemFiles;
    use crate::localization::parse_global_ini;
    use crate::records::{RecordSet, normalize_record};
    use serde_json::json;

    fn words(s: &str) -> Vec<String> {
        tag_words(Some(s))
    }

    fn port(port_tags: &str, required: &str) -> PaintPort {
        PaintPort {
            port: "hardpoint_paint".into(),
            source: "vehicleDefinition",
            port_tags: words(port_tags),
            required_tags: words(required),
            min_size: Some(1.0),
            max_size: Some(1.0),
            types: vec![("Paints".into(), Vec::new())],
        }
    }

    fn paint(tags: &str, required: &str) -> PaintTags {
        PaintTags {
            ty: "Paints".into(),
            sub_type: "UNDEFINED".into(),
            size: Some(1.0),
            tags: words(tags),
            required_tags: words(required),
        }
    }

    #[test]
    fn rule_matches_the_port_tag() {
        let p = port("Paint_400i", "Paint_400i");
        assert!(fits(
            &paint("Paint_400i @Paint_400i_Black_Gold", "Paint_400i"),
            &p,
            &words("ORIG_400i Ship_Dock_Refuel")
        ));
        // Another hull's paint lacks the port's required tag.
        assert!(!fits(&paint("Paint_600i", "Paint_600i"), &p, &[]));
        // A paint with no tags at all fits nothing that requires one.
        assert!(!fits(&paint("", ""), &p, &[]));
        // An @ word is only itself.
        assert!(!fits(&paint("@Paint_400i", ""), &p, &[]));
    }

    #[test]
    fn rule_checks_type_and_size() {
        let p = port("Paint_400i", "Paint_400i");
        let mut wrong_type = paint("Paint_400i", "");
        wrong_type.ty = "Misc".into();
        assert!(!fits(&wrong_type, &p, &[]));
        let mut big = paint("Paint_400i", "");
        big.size = Some(2.0);
        assert!(!fits(&big, &p, &[]));
        let mut open = port("Paint_400i", "Paint_400i");
        (open.min_size, open.max_size) = (None, None);
        assert!(fits(&big, &open, &[]));
        let mut picky = port("", "");
        picky.types = vec![("Paints".into(), vec!["Personal".into()])];
        assert!(!fits(&paint("", ""), &picky, &[]));
    }

    #[test]
    fn rule_multi_tag_paint_requirements() {
        // Cutlass Black: the port offers two tags, Blue/Red only the shared one.
        let black = port("Paint_Cutlass Paint_Cutlass_Black", "Paint_Cutlass");
        let blue = port("Paint_Cutlass Paint_Cutlass_Blue", "Paint_Cutlass");
        let procyon = paint(
            "Paint_Cutlass Paint_Cutlass_Black @Paint_Cutlass_Black_Procyon",
            "Paint_Cutlass_Black",
        );
        assert!(fits(&procyon, &black, &[]));
        assert!(!fits(&procyon, &blue, &[]));
        // Hornet F7A Mk II desert camo: one requirement from the port, one from the vehicle.
        let mk2 = port("Paint_Hornet_F7_Mk2", "Paint_Hornet_F7_Mk2");
        let camo = paint(
            "Paint_Hornet_F7_Mk2 @Paint_Hornet_F7A_Mk2_Desert_Camo",
            "Paint_Hornet_F7_Mk2 ANVL_Hornet_F7A_Mk2",
        );
        assert!(fits(&camo, &mk2, &words("ANVL_Hornet_F7A_Mk2 Ship_Dock_Refuel")));
        assert!(!fits(&camo, &mk2, &words("ANVL_Hornet_F7C_Mk2 Ship_Dock_Refuel")));
        // A port asking for several tags needs all of them on the paint.
        let two = port("A", "A B");
        assert!(fits(&paint("A B", ""), &two, &[]));
        assert!(!fits(&paint("A", ""), &two, &[]));
        // Case does not matter (Argo MOTH).
        assert!(fits(&paint("Paint_Moth", "Paint_Moth"), &port("Paint_MOTH", "Paint_MOTH"), &[]));
    }

    const ROOT: &str = "file://./../../../../../../../libs/foundry/records/";

    fn rec(short: &str, path: &str, components: Value) -> GameRecord {
        normalize_record(
            json!({"_RecordName_": format!("EntityClassDefinition.{short}"), "_RecordId_": format!("id-{short}"),
                "_RecordValue_": {"_Type_": "EntityClassDefinition", "Components": components}}),
            &format!("libs/foundry/records/{path}"),
        )
        .unwrap()
    }

    fn vehicle(short: &str, name_key: &str, def: &str, modification: &str, tags: &str, ports: Value) -> GameRecord {
        rec(
            short,
            &format!("entities/spaceships/{}", short.to_lowercase()),
            json!([
                {"_Type_": "VehicleComponentParams", "vehicleName": name_key,
                    "vehicleDefinition": def, "modification": modification},
                {"_Type_": "SAttachableComponentParams", "AttachDef": {"Type": "NOITEM_Vehicle", "Size": 1, "Tags": tags}},
                {"_Type_": "SItemPortContainerComponentParams", "Ports": ports}
            ]),
        )
    }

    fn paint_rec(short: &str, name_key: &str, tags: &str, required: &str, maker: &str) -> GameRecord {
        rec(
            short,
            &format!("entities/scitem/ships/paints/{}", short.to_lowercase()),
            json!([{"_Type_": "SAttachableComponentParams", "AttachDef": {"Type": "Paints", "SubType": "UNDEFINED",
                "Size": 1, "Grade": 1, "Tags": tags, "RequiredTags": required,
                "Manufacturer": format!("{ROOT}scitemmanufacturer/{maker}.json"),
                "Localization": {"Name": name_key, "Description": format!("{name_key}_Desc")}}}]),
        )
    }

    fn maker(path: &str, short: &str, name_key: &str) -> GameRecord {
        normalize_record(
            json!({"_RecordName_": format!("SCItemManufacturer.{short}"), "_RecordId_": format!("id-{short}"),
                "_RecordValue_": {"_Type_": "SCItemManufacturer", "Localization": {"Name": name_key}}}),
            &format!("libs/foundry/records/scitemmanufacturer/{path}"),
        )
        .unwrap()
    }

    const ORIG_100I: &str = r#"<Vehicle name="ORIG_100i" itemPortTags="">
  <Parts>
    <Part name="ORIG_100i" class="Animated">
      <Parts>
        <Part name="hardpoint_paint" class="ItemPort">
          <ItemPort minSize="1" maxSize="1" portTags="Paint_100i" requiredTags="Paint_100i" id="modPaint">
            <Types><Type type="Paints" /></Types>
          </ItemPort>
        </Part>
        <Part name="hardpoint_skipped" class="ItemPort" skipPart="1">
          <ItemPort portTags="Paint_Other" requiredTags="Paint_Other"><Types><Type type="Paints" /></Types></ItemPort>
        </Part>
      </Parts>
    </Part>
  </Parts>
  <Modifications>
    <Modification name="ORIG_125a" patchFile="">
      <Elems><Elem idRef="modPart_gun_rear" name="skipPart" value="0" /></Elems>
    </Modification>
    <Modification name="Special">
      <Elems><Elem idRef="modPaint" name="requiredTags" value="Paint_Special" /></Elems>
    </Modification>
    <Modification name="Patched" patchFile="Modifications/ORIG_100i_Patched">
      <Elems />
    </Modification>
  </Modifications>
</Vehicle>"#;

    const PATCH: &str = r#"<Modifications>
  <Parts id="xmlParts">
    <Part name="ORIG_100i" class="Animated">
      <Parts>
        <Part name="hardpoint_paint" class="ItemPort">
          <ItemPort portTags="Paint_Patched" requiredTags="Paint_Patched"><Types><Type type="Paints" /></Types></ItemPort>
        </Part>
      </Parts>
    </Part>
  </Parts>
</Modifications>"#;

    fn files() -> MemFiles {
        let mut f = MemFiles::default();
        f.insert(
            r"Data\Scripts\Entities\Vehicles\Implementations\Xml\ORIG_100i.xml",
            ORIG_100I.as_bytes().to_vec(),
        );
        f.insert(
            r"Data\Scripts\Entities\Vehicles\Implementations\Xml\Modifications\ORIG_100i_Patched.xml",
            PATCH.as_bytes().to_vec(),
        );
        f
    }

    fn records() -> RecordSet {
        let def = "Scripts/Entities/Vehicles/Implementations/Xml/ORIG_100i.xml";
        let entity_port = json!([{"_Type_": "SItemPortDef", "Name": "hardpoint_paint", "PortTags": "Paint_890J",
            "RequiredPortTags": "Paint_890J", "MinSize": 1, "MaxSize": 1,
            "Types": [{"_Type_": "SItemPortDefTypes", "Type": "Paints", "SubTypes": []}]}]);
        RecordSet::new(vec![
            vehicle("ORIG_100i", "@vehicle_NameORIG_100i", def, "", "ORIG_100i", json!([])),
            // Lower-case path, as most records spell it.
            vehicle("ORIG_125a", "@vehicle_NameORIG_125a", &def.to_lowercase(), "ORIG_125a", "ORIG_125a", json!([])),
            vehicle("ORIG_100i_Special", "@vehicle_NameORIG_100i", def, "Special", "", json!([])),
            vehicle("ORIG_100i_Patched", "@vehicle_NameORIG_100i", def, "Patched", "", json!([])),
            vehicle("ORIG_890Jump", "@vehicle_NameORIG_890Jump", "scripts/x/missing.xml", "", "ORIG_890Jump", entity_port),
            vehicle("ORIG_NoPort", "@vehicle_NameORIG_100i", "scripts/x/missing.xml", "", "", json!([])),
            paint_rec("Paint_100i_Blue_Gold", "@item_Name100i_Blue", "Paint_100i @Paint_100i_Blue_Gold", "Paint_100i", "paintcolorlogos/paint_100i_blue_gold_logo"),
            paint_rec("Paint_890j_Steel", "@item_Name890_Steel", "Paint_890J", "Paint_890J", "scitemmanufacturer.orig"),
            paint_rec("Paint_100i_Template", "@item_Name100i_Blue", "Paint_100i", "Paint_100i", "x"),
            paint_rec("Paint_Placeholder", "@LOC_PLACEHOLDER", "Paint_100i", "", "x"),
            paint_rec("Paint_Orphan", "@item_NameOrphan", "Paint_Nothing", "", "paintcolorlogos/aegis_logo"),
            maker("scitemmanufacturer.orig", "ORIG", "@manufacturer_NameOrigin"),
            maker("paintcolorlogos/paint_100i_blue_gold_logo", "Paint_100i_Blue_Gold_Logo", "@manufacturer_NameOrigin"),
            maker("scitemmanufacturer.aegs", "AEGS", "@manufacturer_NameAegis"),
            maker("scitemmanufacturer.fski", "FSKI", "@manufacturer_NameAegis"),
            maker("paintcolorlogos/aegis_logo", "Aegis_Logo", "@manufacturer_NameAegis"),
        ])
    }

    fn localization() -> Localization {
        parse_global_ini(
            "vehicle_NameORIG_100i=Origin 100i\nvehicle_NameORIG_125a=Origin 125a\nvehicle_NameORIG_890Jump=Origin 890 Jump\nitem_Name100i_Blue=100i Blue Gold\nitem_Name890_Steel=890 Jump Steel\nitem_NameOrphan=Orphan\nmanufacturer_NameOrigin=Origin Jumpworks\nmanufacturer_NameAegis=Aegis Dynamics",
        )
    }

    fn body() -> Map<String, Value> {
        let (set, loc, f) = (records(), localization(), files());
        DOMAIN.parse(&ParseContext::new(&set, &loc, &f)).unwrap()
    }

    fn paint_row<'a>(body: &'a Map<String, Value>, class: &str) -> &'a Value {
        body["paints"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["className"] == class)
            .unwrap()
    }

    #[test]
    fn paints_list_the_vehicles_they_fit() {
        let body = body();
        let blue = paint_row(&body, "Paint_100i_Blue_Gold");
        assert_eq!(blue["name"], "100i Blue Gold");
        assert_eq!(blue["recordName"], "EntityClassDefinition.Paint_100i_Blue_Gold");
        assert_eq!(blue["nameKey"], "item_Name100i_Blue");
        assert_eq!(blue["type"], "Paints");
        assert_eq!(blue["size"], 1.0);
        assert!(blue.get("subType").is_none(), "UNDEFINED is dropped");
        assert!(blue.get("description").is_none(), "no prose");
        assert_eq!(blue["tags"], json!(["Paint_100i", "@Paint_100i_Blue_Gold"]));
        assert_eq!(blue["requiredTags"], json!(["Paint_100i"]));
        // The 100i and the 125a modification share the port; Special and Patched changed it.
        assert_eq!(
            blue["vehicles"],
            json!([{"className": "ORIG_100i", "name": "Origin 100i"},
                   {"className": "ORIG_125a", "name": "Origin 125a"}])
        );
        // Logo record → the one manufacturer with that name key.
        assert_eq!(blue["manufacturer"], "ORIG");
        assert_eq!(blue["manufacturerName"], "Origin Jumpworks");
        let steel = paint_row(&body, "Paint_890j_Steel");
        assert_eq!(
            steel["vehicles"],
            json!([{"className": "ORIG_890Jump", "name": "Origin 890 Jump"}])
        );
        assert_eq!(steel["manufacturer"], "ORIG");
        // A name shared by two codes gives the name only.
        let orphan = paint_row(&body, "Paint_Orphan");
        assert_eq!(orphan["vehicles"], json!([]));
        assert!(orphan.get("manufacturer").is_none());
        assert_eq!(orphan["manufacturerName"], "Aegis Dynamics");
    }

    #[test]
    fn modifications_patch_files_and_skipped_parts() {
        let body = body();
        let ports = body["paintPorts"].as_array().unwrap();
        let of = |class: &str| {
            ports
                .iter()
                .find(|v| v["className"] == class)
                .map(|v| v["ports"].clone())
        };
        assert_eq!(
            of("ORIG_100i").unwrap(),
            json!([{"port": "hardpoint_paint", "source": "vehicleDefinition",
                    "portTags": ["Paint_100i"], "requiredTags": ["Paint_100i"]}]),
            "the skipPart=1 port is left out"
        );
        assert_eq!(of("ORIG_100i_Special").unwrap()[0]["requiredTags"], json!(["Paint_Special"]));
        assert_eq!(of("ORIG_100i_Patched").unwrap()[0]["portTags"], json!(["Paint_Patched"]));
        assert_eq!(of("ORIG_890Jump").unwrap()[0]["source"], "entity");
        assert!(of("ORIG_NoPort").is_none());
        assert_eq!(body["byVehicle"]["ORIG_100i"], json!(["Paint_100i_Blue_Gold"]));
        assert_eq!(body["byVehicle"]["ORIG_100i_Special"], json!([]));
    }

    #[test]
    fn templates_skipped_and_summary_counts() {
        let body = body();
        let classes: Vec<&str> = body["paints"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["className"].as_str().unwrap())
            .collect();
        assert_eq!(classes, ["Paint_100i_Blue_Gold", "Paint_890j_Steel", "Paint_Orphan"]);
        assert_eq!(
            body["summary"],
            json!({"paints": 3, "templatesSkipped": 2, "paintsFittingNoVehicle": 1, "vehicles": 6,
                "vehiclesWithPaintPort": 5, "vehiclesWithNoPaints": 2, "vehiclesWithoutPaintPort": 1,
                "portsFromEntity": 1, "portsFromVehicleDefinition": 4, "missingVehicleDefinitions": 1,
                "missingPatchFiles": 0})
        );
        assert_eq!(body["_assumptions"][0], RULE);
    }

    #[test]
    fn registered_with_its_archive_files() {
        assert!(crate::domains::find("paints").is_some());
        let f = files();
        assert_eq!(files_matching(&f, DOMAIN.files()).len(), 2);
    }
}
