//! Object containers: where things sit in space (port of `sc-gamedata/src/socpak.ts`).
//!
//! DataCore names every location but places none of them. Positions live in the solar
//! system object containers, `.socpak` files in the archive, which are plain zips holding
//! one XML document that nests `<Child>` containers. Each child carries a `pos` and `rot`
//! relative to its parent and, when it is a star map location, the `starMapRecord` id of its
//! StarMapObject, the join back to DataCore. Positions are metres; rotations are
//! quaternions written `w,x,y,z`.
//!
//! The TypeScript extracts the system containers (`files`), then asks for more by reading
//! them (`moreFiles`, up to four rounds), and finally indexes whatever landed under
//! `Data/ObjectContainers`. [`Containers::extract`] reproduces that set from the archive
//! listing, so the port reads exactly the containers the TypeScript read.
//!
//! Differences from the TypeScript, none visible in the output: zip entries are inflated
//! only when read (the TypeScript inflates every entry up front, so a corrupt entry nobody
//! reads would have failed its whole container), and an unreadable container is skipped
//! with a warning where the TypeScript would have thrown out of the run.

use crate::cryxml::{find_cryxml, nodes_by_tag, parse_cryxml};
use crate::domain::{FileSource, files_matching};
use crate::helpers_world::{is_js_space, is_word_byte, js_hypot, js_to_number};
use std::collections::{BTreeSet, HashMap};

pub type Vec3 = [f64; 3];
pub type Quat = [f64; 4];

const IDENTITY: Quat = [1.0, 0.0, 0.0, 0.0];

/// One `<Child>` container with its absolute placement.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedContainer {
    /// Editor name of the container instance, e.g. `OOC_Stanton_1_Hurston`.
    pub entity_name: Option<String>,
    /// StarMapObject record id, absent for containers that are not locations.
    pub star_map_record: Option<String>,
    /// Archive path of the child's own container, when it is an external one.
    pub container: Option<String>,
    /// Absolute position in the system frame, metres.
    pub position: Vec3,
    /// Nesting depth below the system root, 0 for its direct children.
    pub depth: i64,
    /// `entity_name` of the containing child, absent at depth 0.
    pub parent: Option<String>,
    /// Record id of the nearest containing star map location.
    pub parent_record: Option<String>,
    /// Absolute position of that containing location.
    pub parent_position: Option<Vec3>,
    /// Absolute rotation, for placing the contents of the child's own container.
    pub rotation: Quat,
}

/// The frame a container document is placed in.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub position: Vec3,
    pub rotation: Quat,
    pub depth: i64,
    pub name: Option<String>,
    pub record: Option<String>,
    pub record_position: Option<Vec3>,
}

impl Frame {
    /// The system root.
    pub fn root() -> Self {
        Self {
            position: [0.0; 3],
            rotation: IDENTITY,
            depth: -1,
            name: None,
            record: None,
            record_position: None,
        }
    }

    /// A located child's own container is placed in the child's frame.
    pub fn of(placed: &PlacedContainer) -> Self {
        Self {
            position: placed.position,
            rotation: placed.rotation,
            depth: placed.depth,
            name: placed.entity_name.clone(),
            record: placed.star_map_record.clone(),
            record_position: Some(placed.position),
        }
    }
}

/// Rotate `v` by the unit quaternion `q` (w,x,y,z). Same operation order as the TS.
pub fn rotate(q: Quat, v: Vec3) -> Vec3 {
    let [w, x, y, z] = q;
    let [vx, vy, vz] = v;
    let tx = 2.0 * (y * vz - z * vy);
    let ty = 2.0 * (z * vx - x * vz);
    let tz = 2.0 * (x * vy - y * vx);
    [
        vx + w * tx + (y * tz - z * ty),
        vy + w * ty + (z * tx - x * tz),
        vz + w * tz + (x * ty - y * tx),
    ]
}

fn multiply(a: Quat, b: Quat) -> Quat {
    let [aw, ax, ay, az] = a;
    let [bw, bx, by, bz] = b;
    [
        aw * bw - ax * bx - ay * by - az * bz,
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
    ]
}

/// `value.split(",").map(Number)`, exactly `count` finite numbers.
fn numbers(value: Option<&str>, count: usize) -> Option<Vec<f64>> {
    let value = value.filter(|v| !v.is_empty())?;
    let parts: Vec<f64> = value.split(',').map(js_to_number).collect();
    (parts.len() == count && parts.iter().all(|p| p.is_finite())).then_some(parts)
}

fn is_name_byte(b: u8) -> bool {
    is_word_byte(b) || matches!(b, b':' | b'.' | b'-')
}

fn skip_js_space(s: &str, mut i: usize) -> usize {
    while let Some(c) = s[i..].chars().next() {
        if !is_js_space(c) {
            break;
        }
        i += c.len_utf8();
    }
    i
}

/// `/([\w:.-]+)\s*=\s*"([^"]*)"/g` into a map; a repeated name keeps the last value.
fn attributes(source: &str) -> HashMap<&str, &str> {
    let b = source.as_bytes();
    let mut out = HashMap::new();
    let mut i = 0;
    while i < b.len() {
        if !is_name_byte(b[i]) {
            i += 1;
            continue;
        }
        let start = i;
        while i < b.len() && is_name_byte(b[i]) {
            i += 1;
        }
        let name = &source[start..i];
        // Every start inside the run fails or succeeds the same way, so resume after it.
        let mut j = skip_js_space(source, i);
        if b.get(j) != Some(&b'=') {
            continue;
        }
        j = skip_js_space(source, j + 1);
        if b.get(j) != Some(&b'"') {
            continue;
        }
        let Some(close) = source[j + 1..].find('"') else {
            continue;
        };
        out.insert(name, &source[j + 1..j + 1 + close]);
        i = j + 1 + close + 1;
    }
    out
}

/// One match of `/<(\/?)Child\b([^>]*?)(\/?)>/g`.
struct ChildTag<'a> {
    closing: bool,
    attrs: &'a str,
    self_closing: bool,
}

fn child_tags(xml: &str) -> Vec<ChildTag<'_>> {
    let b = xml.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(rel) = xml[i..].find('<') {
        let lt = i + rel;
        i = lt + 1;
        let mut p = lt + 1;
        let closing = b.get(p) == Some(&b'/');
        if closing {
            p += 1;
        }
        if !xml[p..].starts_with("Child") {
            continue;
        }
        p += 5;
        if b.get(p).is_some_and(|c| is_word_byte(*c)) {
            continue;
        }
        let Some(gt_rel) = xml[p..].find('>') else {
            continue;
        };
        let gt = p + gt_rel;
        let (attrs_end, self_closing) = if gt > p && b[gt - 1] == b'/' {
            (gt - 1, true)
        } else {
            (gt, false)
        };
        out.push(ChildTag {
            closing,
            attrs: &xml[p..attrs_end],
            self_closing,
        });
        i = gt + 1;
    }
    out
}

/// `parseContainerXml`: every `<Child>` container in a document, with its absolute
/// position. A child with an unreadable `pos` is placed at its parent.
pub fn parse_container_xml(xml: &str, frame: Option<&Frame>) -> Vec<PlacedContainer> {
    let mut placed = Vec::new();
    let mut stack = vec![frame.cloned().unwrap_or_else(Frame::root)];
    for tag in child_tags(xml) {
        if tag.closing {
            if stack.len() > 1 {
                stack.pop();
            }
            continue;
        }
        let attrs = attributes(tag.attrs);
        let Some(parent) = stack.last() else { break };
        let local = numbers(attrs.get("pos").copied(), 3).map_or([0.0; 3], |v| [v[0], v[1], v[2]]);
        let spin =
            numbers(attrs.get("rot").copied(), 4).map_or(IDENTITY, |v| [v[0], v[1], v[2], v[3]]);
        let offset = rotate(parent.rotation, local);
        let position = [
            parent.position[0] + offset[0],
            parent.position[1] + offset[1],
            parent.position[2] + offset[2],
        ];
        let non_empty = |k: &str| {
            attrs
                .get(k)
                .filter(|v| !v.is_empty())
                .map(|v| v.to_string())
        };
        let entity_name = non_empty("entityName");
        let star_map_record = non_empty("starMapRecord");
        let rotation = multiply(parent.rotation, spin);
        let depth = parent.depth + 1;
        let container = if attrs.get("external") == Some(&"1") {
            non_empty("name")
        } else {
            None
        };
        let next = (!tag.self_closing).then(|| Frame {
            position,
            rotation,
            depth,
            name: entity_name.clone(),
            record: star_map_record.clone().or_else(|| parent.record.clone()),
            record_position: if star_map_record.is_some() {
                Some(position)
            } else {
                parent.record_position
            },
        });
        placed.push(PlacedContainer {
            entity_name,
            star_map_record,
            container,
            position,
            depth,
            parent: parent.name.clone(),
            parent_record: parent.record.clone(),
            parent_position: parent.record_position,
            rotation,
        });
        if let Some(f) = next {
            stack.push(f);
        }
    }
    placed
}

/// One zip entry, located through the central directory.
#[derive(Debug, Clone)]
struct ZipEntry {
    method: u16,
    start: usize,
    size: usize,
}

fn u16_at(b: &[u8], at: usize) -> Result<usize, String> {
    b.get(at..at + 2)
        .map(|s| usize::from(u16::from_le_bytes([s[0], s[1]])))
        .ok_or_else(|| "truncated zip".to_string())
}

fn u32_at(b: &[u8], at: usize) -> Result<usize, String> {
    b.get(at..at + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]) as usize)
        .ok_or_else(|| "truncated zip".to_string())
}

/// `readZip`'s directory: entries by central-directory name (`\` turned into `/`), in
/// first-seen order with a repeated name taking the later entry, as a JS `Map` does.
/// Directories and methods other than store/deflate are left out.
fn zip_directory(buffer: &[u8]) -> Result<Vec<(String, ZipEntry)>, String> {
    if buffer.len() < 22 {
        return Err("not a zip: no end of central directory".into());
    }
    let last = buffer.len() - 22;
    let lowest = last.saturating_sub(0xffff);
    let end = (lowest..=last)
        .rev()
        .find(|&i| buffer[i..i + 4] == [0x50, 0x4b, 0x05, 0x06])
        .ok_or("not a zip: no end of central directory")?;
    let count = u16_at(buffer, end + 10)?;
    let mut cursor = u32_at(buffer, end + 16)?;
    let mut entries: Vec<(String, ZipEntry)> = Vec::new();
    let mut position: HashMap<String, usize> = HashMap::new();
    for _ in 0..count {
        if u32_at(buffer, cursor)? != 0x0201_4b50 {
            return Err("corrupt zip central directory".into());
        }
        let method = u16_at(buffer, cursor + 10)? as u16;
        let compressed = u32_at(buffer, cursor + 20)?;
        let name_len = u16_at(buffer, cursor + 28)?;
        let extra_len = u16_at(buffer, cursor + 30)?;
        let comment_len = u16_at(buffer, cursor + 32)?;
        let local = u32_at(buffer, cursor + 42)?;
        let raw_name = buffer
            .get(cursor + 46..cursor + 46 + name_len)
            .ok_or("truncated zip")?;
        let name = String::from_utf8_lossy(raw_name).replace('\\', "/");
        cursor += 46 + name_len + extra_len + comment_len;
        if name.ends_with('/') {
            continue;
        }
        let start = local + 30 + u16_at(buffer, local + 26)? + u16_at(buffer, local + 28)?;
        if method != 0 && method != 8 {
            continue;
        }
        let entry = ZipEntry {
            method,
            start,
            size: compressed,
        };
        match position.get(&name) {
            Some(&i) => entries[i].1 = entry,
            None => {
                position.insert(name.clone(), entries.len());
                entries.push((name, entry));
            }
        }
    }
    Ok(entries)
}

fn zip_data(buffer: &[u8], entry: &ZipEntry) -> Result<Vec<u8>, String> {
    // `subarray` clamps to the buffer.
    let start = entry.start.min(buffer.len());
    let end = entry.start.saturating_add(entry.size).min(buffer.len());
    let data = &buffer[start..end];
    if entry.method == 0 {
        return Ok(data.to_vec());
    }
    miniz_oxide::inflate::decompress_to_vec(data).map_err(|e| format!("inflate: {e:?}"))
}

/// `readZip`: every stored or deflated entry, by name.
pub fn read_zip(buffer: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    zip_directory(buffer)?
        .into_iter()
        .map(|(name, e)| Ok((name, zip_data(buffer, &e)?)))
        .collect()
}

/// `readContainerXml`: the root-level `<name>.xml`, not its `_editor` sibling and not the
/// per-entity documents in subdirectories.
pub fn read_container_xml(socpak: &[u8]) -> Result<Option<String>, String> {
    for (name, entry) in zip_directory(socpak)? {
        if name.contains('/') || !name.ends_with(".xml") || name.ends_with("_editor.xml") {
            continue;
        }
        let data = zip_data(socpak, &entry)?;
        return Ok(Some(String::from_utf8_lossy(&data).into_owned()));
    }
    Ok(None)
}

/// What a location's own container says about the law around it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ContainerZones {
    /// Radius of the largest armistice (green) zone in the container, metres.
    pub armistice_radius: Option<f64>,
    /// The container holds a monitored zone: crime there is seen.
    pub monitored: bool,
    /// The container holds a monitor zone jammer; the inner value is its own radius in
    /// metres when it sets one.
    pub jammer: Option<Option<f64>>,
}

impl ContainerZones {
    /// `found.armisticeRadius || found.monitored || found.jammer`.
    pub fn any(&self) -> bool {
        self.armistice_radius.is_some_and(|r| r != 0.0) || self.monitored || self.jammer.is_some()
    }
}

/// `/<Object [^>]*type="MonitoredZone"/`.
fn has_monitored_zone(xml: &str) -> bool {
    let mut i = 0;
    while let Some(rel) = xml[i..].find("<Object ") {
        let start = i + rel + "<Object ".len();
        let region = match xml[start..].find('>') {
            Some(gt) => &xml[start..start + gt],
            None => &xml[start..],
        };
        if region.contains("type=\"MonitoredZone\"") {
            return true;
        }
        i = start;
    }
    false
}

fn attr_number(node: &crate::cryxml::XmlNode, key: &str) -> f64 {
    node.attr(key).map_or(f64::NAN, js_to_number)
}

/// `readZones`: armistice and monitored zones of one container. Armistice zones are
/// `GreenZone` entities in the container's `.soc` (one CryXmlB document); monitored zones
/// are `MonitoredZone` objects in the editor document; a `MonitorZoneJammer` entity in the
/// `.soc` is a dead zone with its own `radiusKm` when it departs from the default.
pub fn read_zones(socpak: &[u8]) -> Result<ContainerZones, String> {
    let mut zones = ContainerZones::default();
    for (name, entry) in zip_directory(socpak)? {
        if name.contains('/') {
            continue;
        }
        if name.ends_with("_editor.xml") {
            let data = zip_data(socpak, &entry)?;
            if has_monitored_zone(&String::from_utf8_lossy(&data)) {
                zones.monitored = true;
            }
        } else if name.ends_with(".soc") {
            let data = zip_data(socpak, &entry)?;
            let Some(start) = find_cryxml(&data) else {
                continue;
            };
            let root = parse_cryxml(&data, start).map_err(|e| e.to_string())?;
            for entity in nodes_by_tag(&root, "Entity") {
                if entity.attr("EntityClass") == Some("MonitorZoneJammer") {
                    let jammer = zones.jammer.get_or_insert(None);
                    for params in nodes_by_tag(entity, "EntityComponentMonitoredZone") {
                        let km = attr_number(params, "radiusKm");
                        if km > 0.0 {
                            *jammer = Some(jammer.unwrap_or(0.0).max(km * 1000.0));
                        }
                    }
                    continue;
                }
                if entity.attr("EntityClass") != Some("GreenZone") {
                    continue;
                }
                let mut radius = attr_number(entity, "Radius");
                for params in nodes_by_tag(entity, "EntityComponentGreenZone") {
                    let r = attr_number(params, "radius");
                    if r > 0.0 {
                        radius = r;
                    }
                }
                if radius > 0.0 {
                    zones.armistice_radius =
                        Some(zones.armistice_radius.unwrap_or(0.0).max(radius));
                }
            }
        }
    }
    Ok(zones)
}

/// How a planet or moon turns, from its own container document.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BodySpin {
    /// Hours the body takes to turn once (`planetRotationSpeed`).
    pub rotation_hours: f64,
    /// Radius of the body's rotating zone, metres (`gridRadius`).
    pub grid_radius: Option<f64>,
}

/// `\b<attr>="([^"]+)"` searched from `from`: the value of the first match.
fn quoted_value_after<'a>(xml: &'a str, at: usize, literal: &str) -> Option<&'a str> {
    let value_start = at + literal.len();
    let close = xml[value_start..].find('"')?;
    (close > 0).then(|| &xml[value_start..value_start + close])
}

fn boundary_before(b: &[u8], at: usize) -> bool {
    at == 0 || !is_word_byte(b[at - 1])
}

/// `xml.match(/\bplanetRotationSpeed="([^"]+)"/)?.[1]`.
fn rotation_speed(xml: &str) -> Option<&str> {
    const LIT: &str = "planetRotationSpeed=\"";
    let b = xml.as_bytes();
    let mut i = 0;
    while let Some(rel) = xml[i..].find(LIT) {
        let at = i + rel;
        if boundary_before(b, at)
            && let Some(v) = quoted_value_after(xml, at, LIT)
        {
            return Some(v);
        }
        i = at + 1;
    }
    None
}

/// `xml.match(/<ObjectContainer\b[^>]*\bgridRadius="([^"]+)"/)?.[1]`: the greedy
/// `[^>]*` takes the last `gridRadius` inside the tag that has a value.
fn grid_radius(xml: &str) -> Option<&str> {
    const OPEN: &str = "<ObjectContainer";
    const LIT: &str = "gridRadius=\"";
    let b = xml.as_bytes();
    let mut i = 0;
    while let Some(rel) = xml[i..].find(OPEN) {
        let at = i + rel;
        i = at + 1;
        let p = at + OPEN.len();
        if b.get(p).is_some_and(|c| is_word_byte(*c)) {
            continue;
        }
        let region_end = xml[p..].find('>').map_or(xml.len(), |g| p + g);
        let region = &xml[p..region_end];
        let mut hits: Vec<usize> = region.match_indices(LIT).map(|(k, _)| p + k).collect();
        hits.reverse();
        for h in hits {
            if boundary_before(b, h)
                && let Some(v) = quoted_value_after(xml, h, LIT)
            {
                return Some(v);
            }
        }
    }
    None
}

/// `readSpin`: the body's own entity carries `planetRotationSpeed`, the document root
/// `gridRadius`.
pub fn read_spin(xml: &str) -> Option<BodySpin> {
    let hours = rotation_speed(xml).map_or(f64::NAN, js_to_number);
    // `!(hours > 0)`: NaN fails too.
    if hours.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
        return None;
    }
    let grid = grid_radius(xml).map_or(f64::NAN, js_to_number);
    Some(BodySpin {
        rotation_hours: hours,
        grid_radius: (grid > 0.0).then_some(grid),
    })
}

/// `archiveKey`: lower-cased, slash-separated archive path.
pub fn archive_key(value: &str) -> String {
    value.replace('\\', "/").to_lowercase()
}

/// `/^data\/objectcontainers\/pu\/system\/[^/]+\/[^/]*system\.socpak$/`.
fn is_system_container(key: &str) -> bool {
    let Some(rest) = key.strip_prefix("data/objectcontainers/pu/system/") else {
        return false;
    };
    let Some((system, file)) = rest.split_once('/') else {
        return false;
    };
    !system.is_empty() && !file.contains('/') && file.ends_with("system.socpak")
}

/// Everything read out of one extracted container.
#[derive(Debug, Default)]
struct Loaded {
    xml: Option<String>,
    zones: ContainerZones,
    spin: Option<BodySpin>,
}

/// The object containers the TypeScript run had on disk, by `archiveKey`, read once.
pub struct Containers {
    /// key -> archive entry name.
    index: HashMap<String, String>,
    loaded: HashMap<String, Loaded>,
    /// Containers that could not be read.
    pub warnings: Vec<String>,
}

impl Containers {
    /// Reproduce the TypeScript extraction: `files` globs, then `unextractedLocationContainers`
    /// rounds (at most four, stopping when nothing is asked for or nothing more lands).
    /// `radii` are the obstruction radii of every extracted `starmap` record by id.
    pub fn extract(files: &dyn FileSource, globs: &[&str], radii: &HashMap<String, f64>) -> Self {
        let mut c = Self {
            index: HashMap::new(),
            loaded: HashMap::new(),
            warnings: Vec::new(),
        };
        for name in files_matching(files, globs) {
            c.add(name);
        }
        let mut by_lower: Option<HashMap<String, Vec<&str>>> = None;
        for _ in 0..4 {
            let wanted = c.unextracted_location_containers(files, radii);
            if wanted.is_empty() {
                break;
            }
            let lower = by_lower.get_or_insert_with(|| {
                let mut m: HashMap<String, Vec<&str>> = HashMap::new();
                for n in files.names() {
                    let key = archive_key(n);
                    if key.starts_with("data/objectcontainers/") {
                        m.entry(key).or_default().push(n.as_str());
                    }
                }
                m
            });
            let hits: Vec<String> = wanted
                .iter()
                .filter_map(|w| lower.get(w))
                .flatten()
                .map(|n| (*n).to_string())
                .collect();
            if hits.is_empty() {
                break;
            }
            for n in &hits {
                c.add(n);
            }
        }
        c
    }

    fn add(&mut self, name: &str) {
        let key = archive_key(name);
        if key.starts_with("data/objectcontainers/") && key.ends_with(".socpak") {
            self.index.insert(key, name.to_string());
        }
    }

    /// Extracted container keys, sorted.
    pub fn keys(&self) -> Vec<&str> {
        let mut k: Vec<&str> = self.index.keys().map(String::as_str).collect();
        k.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
        k
    }

    pub fn has(&self, key: &str) -> bool {
        self.index.contains_key(key)
    }

    fn load(&mut self, files: &dyn FileSource, key: &str) -> Option<&Loaded> {
        if !self.loaded.contains_key(key) {
            let name = self.index.get(key)?.clone();
            let loaded = match files.read(&name) {
                Ok(bytes) => {
                    let xml = read_container_xml(&bytes).unwrap_or_else(|e| {
                        self.warnings.push(format!("{name}: {e}"));
                        None
                    });
                    // `loadZones` turns any failure into "no zones".
                    let zones = read_zones(&bytes).unwrap_or_default();
                    let spin = xml.as_deref().and_then(read_spin);
                    Loaded { xml, zones, spin }
                }
                Err(e) => {
                    self.warnings.push(format!("{name}: {e}"));
                    Loaded::default()
                }
            };
            self.loaded.insert(key.to_string(), loaded);
        }
        self.loaded.get(key)
    }

    fn document(&mut self, files: &dyn FileSource, key: &str) -> Option<String> {
        self.load(files, key).and_then(|l| l.xml.clone())
    }

    /// `unextractedLocationContainers`.
    fn unextracted_location_containers(
        &mut self,
        files: &dyn FileSource,
        radii: &HashMap<String, f64>,
    ) -> Vec<String> {
        let mut wanted = BTreeSet::new();
        let systems: Vec<String> = self
            .keys()
            .into_iter()
            .filter(|k| is_system_container(k))
            .map(str::to_string)
            .collect();
        let want = |placed: &PlacedContainer,
                    index: &HashMap<String, String>,
                    wanted: &mut BTreeSet<String>| {
            let Some(container) = &placed.container else {
                return;
            };
            if placed.star_map_record.is_none() || on_surface(placed, radii) {
                return;
            }
            let target = archive_key(container);
            if !index.contains_key(&target) {
                wanted.insert(target);
            }
        };
        for key in systems {
            let Some(xml) = self.document(files, &key) else {
                continue;
            };
            for placed in parse_container_xml(&xml, None) {
                want(&placed, &self.index, &mut wanted);
                let Some(container) = placed.container.as_deref() else {
                    continue;
                };
                if placed.star_map_record.is_none() {
                    continue;
                }
                let Some(inner) = self.document(files, &archive_key(container)) else {
                    continue;
                };
                for child in parse_container_xml(&inner, Some(&Frame::of(&placed))) {
                    want(&child, &self.index, &mut wanted);
                }
            }
        }
        // A BTreeSet of ASCII-lower keys iterates in UTF-16 order for these paths.
        let mut out: Vec<String> = wanted.into_iter().collect();
        out.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
        out
    }

    /// `loadPlacements`: absolute positions of every star map location, by record id. A
    /// record placed more than once keeps its shallowest placement.
    pub fn load_placements(&mut self, files: &dyn FileSource) -> HashMap<String, PlacedContainer> {
        let mut placements: HashMap<String, PlacedContainer> = HashMap::new();
        let keep = |placed: PlacedContainer, placements: &mut HashMap<String, PlacedContainer>| {
            let Some(id) = placed.star_map_record.clone() else {
                return;
            };
            match placements.get(&id) {
                Some(existing) if placed.depth >= existing.depth => {}
                _ => {
                    placements.insert(id, placed);
                }
            }
        };
        let systems: Vec<String> = self
            .keys()
            .into_iter()
            .filter(|k| is_system_container(k))
            .map(str::to_string)
            .collect();
        for key in systems {
            let Some(xml) = self.document(files, &key) else {
                continue;
            };
            for placed in parse_container_xml(&xml, None) {
                let inner_key = match (&placed.star_map_record, &placed.container) {
                    (Some(_), Some(c)) => Some(archive_key(c)),
                    _ => None,
                };
                let frame = Frame::of(&placed);
                keep(placed, &mut placements);
                let Some(inner_key) = inner_key else { continue };
                let Some(inner) = self.document(files, &inner_key) else {
                    continue;
                };
                for child in parse_container_xml(&inner, Some(&frame)) {
                    keep(child, &mut placements);
                }
            }
        }
        placements
    }

    /// `loadZones`: zones of every extracted container a placement points at, by record id.
    pub fn load_zones(
        &mut self,
        files: &dyn FileSource,
        placements: &HashMap<String, PlacedContainer>,
    ) -> HashMap<String, ContainerZones> {
        let mut zones = HashMap::new();
        for (record, placed) in placements {
            let Some(container) = &placed.container else {
                continue;
            };
            let found = self
                .load(files, &archive_key(container))
                .map(|l| l.zones.clone())
                .unwrap_or_default();
            if found.any() {
                zones.insert(record.clone(), found);
            }
        }
        zones
    }

    /// `loadSpins`: rotation of every extracted body a placement points at, by record id.
    pub fn load_spins(
        &mut self,
        files: &dyn FileSource,
        placements: &HashMap<String, PlacedContainer>,
    ) -> HashMap<String, BodySpin> {
        let mut spins = HashMap::new();
        for (record, placed) in placements {
            let Some(container) = &placed.container else {
                continue;
            };
            if let Some(spin) = self
                .load(files, &archive_key(container))
                .and_then(|l| l.spin)
            {
                spins.insert(record.clone(), spin);
            }
        }
        spins
    }
}

/// `onSurface`: `placed` sits inside the body that contains it.
fn on_surface(placed: &PlacedContainer, radii: &HashMap<String, f64>) -> bool {
    let (Some(record), Some(pp)) = (&placed.parent_record, placed.parent_position) else {
        return false;
    };
    let Some(&radius) = radii.get(record) else {
        return false;
    };
    if radius == 0.0 || radius.is_nan() {
        return false;
    }
    let [x, y, z] = placed.position;
    js_hypot(&[x - pp[0], y - pp[1], z - pp[2]]) < radius
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::fixtures::MemFiles;

    /// A minimal zip: local headers, central directory, end record (as socpak.test.ts).
    pub(crate) fn zip(files: &[(&str, Option<&str>, &[u8], bool)]) -> Vec<u8> {
        let mut locals = Vec::new();
        let mut central = Vec::new();
        for (name, local_name, data, deflate) in files {
            let body = if *deflate {
                miniz_oxide::deflate::compress_to_vec(data, 6)
            } else {
                data.to_vec()
            };
            let local_name = local_name.unwrap_or(name);
            let offset = locals.len() as u32;
            let mut local = vec![0u8; 30];
            local[0..4].copy_from_slice(&0x0403_4b50u32.to_le_bytes());
            local[8..10].copy_from_slice(&(if *deflate { 8u16 } else { 0 }).to_le_bytes());
            local[18..22].copy_from_slice(&(body.len() as u32).to_le_bytes());
            local[22..26].copy_from_slice(&(data.len() as u32).to_le_bytes());
            local[26..28].copy_from_slice(&(local_name.len() as u16).to_le_bytes());
            locals.extend(local);
            locals.extend(local_name.as_bytes());
            locals.extend(&body);
            let mut entry = vec![0u8; 46];
            entry[0..4].copy_from_slice(&0x0201_4b50u32.to_le_bytes());
            entry[10..12].copy_from_slice(&(if *deflate { 8u16 } else { 0 }).to_le_bytes());
            entry[20..24].copy_from_slice(&(body.len() as u32).to_le_bytes());
            entry[24..28].copy_from_slice(&(data.len() as u32).to_le_bytes());
            entry[28..30].copy_from_slice(&(name.len() as u16).to_le_bytes());
            entry[42..46].copy_from_slice(&offset.to_le_bytes());
            central.extend(entry);
            central.extend(name.as_bytes());
        }
        let mut end = vec![0u8; 22];
        end[0..4].copy_from_slice(&0x0605_4b50u32.to_le_bytes());
        end[8..10].copy_from_slice(&(files.len() as u16).to_le_bytes());
        end[10..12].copy_from_slice(&(files.len() as u16).to_le_bytes());
        end[12..16].copy_from_slice(&(central.len() as u32).to_le_bytes());
        end[16..20].copy_from_slice(&(locals.len() as u32).to_le_bytes());
        let mut out = locals;
        out.extend(central);
        out.extend(end);
        out
    }

    // Cases from sc-gamedata src/socpak.test.ts.
    #[test]
    fn reads_stored_and_deflated_entries() {
        let b = "<b/>".repeat(50);
        let entries = read_zip(&zip(&[
            ("a.xml", None, b"<a/>", false),
            ("b.xml", None, b.as_bytes(), true),
        ]))
        .unwrap();
        assert_eq!(entries[0], ("a.xml".to_string(), b"<a/>".to_vec()));
        assert_eq!(entries[1].1, b.as_bytes());
    }

    #[test]
    fn picks_the_root_container_document() {
        let xml = read_container_xml(&zip(&[
            ("sys_editor.xml", None, b"<Editor/>", false),
            (
                "sys/hcd/x.xml",
                Some("sys\\hcd\\x.xml"),
                b"<Nested/>",
                false,
            ),
            ("sys.xml", None, b"<ObjectContainer/>", true),
        ]))
        .unwrap();
        assert_eq!(xml.as_deref(), Some("<ObjectContainer/>"));
        assert!(read_zip(b"nope").is_err());
    }

    #[test]
    fn rotates_by_w_x_y_z() {
        let h = std::f64::consts::FRAC_1_SQRT_2;
        let [x, y, z] = rotate([h, 0.0, 0.0, h], [1.0, 0.0, 0.0]);
        assert!(x.abs() < 1e-9 && (y - 1.0).abs() < 1e-9 && z.abs() < 1e-9);
    }

    #[test]
    fn composes_child_positions_down_the_chain() {
        let h = std::f64::consts::FRAC_1_SQRT_2;
        let xml = format!(
            r#"
      <ObjectContainer>
        <ChildObjectContainers>
          <Child external="1" name="star.socpak" entityName="Star" pos="0,0,0" rot="1,0,0,0" starMapRecord="star-id"/>
          <Child external="1" name="planet.socpak" entityName="Planet" pos="1000,0,0" rot="{h},0,0,{h}" starMapRecord="planet-id">
            <InstanceMetadata/>
            <ChildObjectContainers>
              <Child entityName="Moon" pos="10,0,0" rot="1,0,0,0" starMapRecord="moon-id">
                <ChildObjectContainers>
                  <Child entityName="Wreck" pos="0,1,0"/>
                </ChildObjectContainers>
              </Child>
            </ChildObjectContainers>
          </Child>
          <Child entityName="After" pos="5,5,5"/>
        </ChildObjectContainers>
      </ObjectContainer>"#
        );
        let placed = parse_container_xml(&xml, None);
        assert_eq!(placed.len(), 5);
        let by = |n: &str| {
            placed
                .iter()
                .find(|p| p.entity_name.as_deref() == Some(n))
                .unwrap()
        };
        let planet = by("Planet");
        assert_eq!(planet.depth, 0);
        assert_eq!(planet.star_map_record.as_deref(), Some("planet-id"));
        assert_eq!(planet.container.as_deref(), Some("planet.socpak"));
        let moon = by("Moon");
        assert!((moon.position[0] - 1000.0).abs() < 1e-6 && (moon.position[1] - 10.0).abs() < 1e-6);
        assert_eq!((moon.depth, moon.parent.as_deref()), (1, Some("Planet")));
        assert_eq!(moon.parent_record.as_deref(), Some("planet-id"));
        let wreck = by("Wreck");
        assert!((wreck.position[0] - 999.0).abs() < 1e-6);
        assert_eq!(wreck.star_map_record, None);
        assert_eq!(wreck.parent_record.as_deref(), Some("moon-id"));
        let after = by("After");
        assert_eq!((after.depth, after.position), (0, [5.0, 5.0, 5.0]));
        assert_eq!(after.parent, None);
    }

    #[test]
    fn reads_spin() {
        let body = r#"<ObjectContainer name="stanton1" gridRadius="2000000" radius="1200000"><Entity label="stanton1" planetRadius="1000000" planetRotationSpeed="2.48" planetAxis="0,0,1"/></ObjectContainer>"#;
        assert_eq!(
            read_spin(body),
            Some(BodySpin {
                rotation_hours: 2.48,
                grid_radius: Some(2_000_000.0)
            })
        );
        assert_eq!(
            read_spin(r#"<Entity planetRotationSpeed="5.1"/>"#),
            Some(BodySpin {
                rotation_hours: 5.1,
                grid_radius: None
            })
        );
        assert_eq!(
            read_spin(
                r#"<ObjectContainer gridRadius="500"><Entity label="station"/></ObjectContainer>"#
            ),
            None
        );
        assert_eq!(read_spin(r#"<Entity planetRotationSpeed="0"/>"#), None);
        assert_eq!(read_spin(r#"<Entity xplanetRotationSpeed="5"/>"#), None);
    }

    #[test]
    fn attribute_and_tag_scanning() {
        let a = attributes(r#" a="1" b = "x y" a="2" bad= c"#);
        assert_eq!(a.get("a"), Some(&"2"));
        assert_eq!(a.get("b"), Some(&"x y"));
        assert_eq!(a.get("bad"), None);
        let tags = child_tags("<ChildObjectContainers><Child a=\"1\"/></Child><Children>");
        assert_eq!(tags.len(), 2);
        assert!(tags[0].self_closing && tags[0].attrs == " a=\"1\"");
        assert!(tags[1].closing);
        assert!(has_monitored_zone(
            r#"<Objects><Object name="z" type="MonitoredZone">"#
        ));
        assert!(!has_monitored_zone(
            r#"<Object name="z"> type="MonitoredZone""#
        ));
    }

    #[test]
    fn extraction_follows_located_containers_off_the_surface() {
        let mut f = MemFiles::default();
        let system = r#"<ObjectContainer><Child external="1" name="Data/ObjectContainers/PU/loc/planet.socpak" entityName="P" pos="0,0,0" starMapRecord="planet"/></ObjectContainer>"#;
        let planet = r#"<ObjectContainer gridRadius="5000"><Entity planetRotationSpeed="3"/>
            <Child external="1" name="Data/ObjectContainers/PU/loc/station.socpak" pos="20000,0,0" starMapRecord="station"/>
            <Child external="1" name="Data/ObjectContainers/PU/loc/outpost.socpak" pos="10,0,0" starMapRecord="outpost"/></ObjectContainer>"#;
        f.insert(
            r"Data\ObjectContainers\PU\system\x\xsystem.socpak",
            zip(&[("xsystem.xml", None, system.as_bytes(), true)]),
        );
        f.insert(
            r"Data\ObjectContainers\PU\loc\Planet.socpak",
            zip(&[("planet.xml", None, planet.as_bytes(), false)]),
        );
        f.insert(
            r"Data\ObjectContainers\PU\loc\station.socpak",
            zip(&[("station.xml", None, b"<ObjectContainer/>", false)]),
        );
        f.insert(
            r"Data\ObjectContainers\PU\loc\outpost.socpak",
            zip(&[("outpost.xml", None, b"<ObjectContainer/>", false)]),
        );
        let radii = HashMap::from([("planet".to_string(), 1000.0)]);
        let mut c = Containers::extract(
            &f,
            &["**/ObjectContainers/PU/system/*/*system.socpak"],
            &radii,
        );
        assert_eq!(
            c.keys(),
            vec![
                "data/objectcontainers/pu/loc/planet.socpak",
                "data/objectcontainers/pu/loc/station.socpak",
                "data/objectcontainers/pu/system/x/xsystem.socpak",
            ]
        );
        let placements = c.load_placements(&f);
        assert_eq!(placements["station"].depth, 1);
        assert_eq!(placements["station"].position, [20000.0, 0.0, 0.0]);
        let spins = c.load_spins(&f, &placements);
        assert_eq!(spins["planet"].grid_radius, Some(5000.0));
        assert!(c.has("data/objectcontainers/pu/loc/planet.socpak"));
    }
}
