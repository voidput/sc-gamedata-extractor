//! NPC loadouts: what each NPC type (actor) can spawn wearing and wielding, which is what its
//! body can be looted for besides the pockets its loot table fills (`game-loot`). Ported from
//! starorg-codex `origin/dev` (`crates/starorg-gamedata/src/domains/npc_loadouts.rs`); the
//! output matches its `game-npc-loadouts` apart from the `_assumptions` note added here and
//! the chance of an item one option holds more than once (counted once here, see `flat`).
//!
//! An actor is an `EntityClassDefinition` under `libs/foundry/records/actor/actors/`. Its
//! `SEntityComponentDefaultLoadoutParams.loadout` comes in these shapes, nested freely:
//!
//! - `SItemPortLoadoutXMLParams`: a loose loadout file in the archive
//!   (`Data/Scripts/Loadouts/**.xml`, CryXmlB or text), `<Item portName itemName>` nested;
//! - `SItemPortLoadoutManualParams`: inline `SItemPortLoadoutEntryParams` (a port, a class
//!   by name or reference, a nested loadout);
//! - `SItemPortLoadoutRandomGeneratedParams`: `LoadoutAssortments`, `SLoadoutAssortment`
//!   records (`loadoutkits/**`) whose `GroupedLoadouts` pick among their `Loadouts`, gated
//!   by a `Requirement` (tags required and banned, `SLoadoutRequirementOR`,
//!   `SLoadoutRequirementNOT`, nested);
//! - `SItemPortLoadoutOutfitParams`: a `BaseLoadout` plus `Outfits`, the loadouts of one
//!   outfit stacking and separate outfits being alternatives (the mission picks a named one
//!   by its `OutfitNameTags`);
//! - `SItemPortLoadoutRecordParams`: another record's loadout (`loadoutRecord`);
//!
//! plus `InventoryItems` on any of them.
//!
//! Requirements are evaluated against the actor's own tags (its record `tags` and its
//! geometry's model tags; a required parent tag is met by any of its child tags): a group
//! whose requirement fails is dropped, one that holds is kept, and only a requirement this
//! code cannot read (an unknown type) stays as a label. Groups of the same `Group` name at
//! one level (the assortments of one loadout together) are one pick, not several: their
//! options are pooled, a repeated option counting as weight. Inferred from the data, not
//! stated by it: of same-named groups, the gated ones whose requirement holds are the pick,
//! and the ungated ones are the fallback used only when no gated one holds (LIVE 4.10.201:
//! 490 actor kits pair one met and one failed gate of a name, 84 an open group with a failed
//! gate, 35 two met gates with an open one).
//!
//! The data gives no weights: a pick is assumed even, so an option of `n` is `1/n`,
//! compounded through nesting (an assumption the screens say). Body, head, hair, skin and mobiGlas ports are left out, as
//! are items tagged `Item/Inventory/Unlootable`. A referenced loadout file the archive lacks
//! is dropped with one warning. Which places or missions spawn an actor is runtime data and
//! not here; `faction` is the actor's primary organization record.
//!
//! Cheap on purpose: one DataCore parse, records pruned to what is read, each loadout file
//! parsed once however many actors use it.

use crate::collate::compare_names;
use crate::cryxml::{XmlNode, find_cryxml, parse_cryxml};
use crate::domain::{Domain, DomainError, FileSource, ParseContext, field};
use crate::entity::{component, has_component, prune_entity, referenced_name};
use crate::helpers_items::item_common::{attach_def, display_name};
use crate::helpers_items::tags::{TagIndex, build_tag_index};
use crate::records::{GameRecord, RecordSet, array, num, ref_name, ref_path_str, str_};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::rc::Rc;

pub static DOMAIN: NpcLoadoutsDomain = NpcLoadoutsDomain;

pub struct NpcLoadoutsDomain;

/// The dataset's name.
pub const NAME: &str = "game-npc-loadouts";

/// Items carrying this tag cannot be taken from a body.
pub const UNLOOTABLE_TAG: &str = "Item/Inventory/Unlootable";

/// The dataset's `_assumptions` note. `chance` keeps the starorg-codex name (the app reads it
/// as-is) instead of a `chanceEstimate`, so this note is what marks it as an estimate.
pub const PICK_ASSUMPTIONS: &str = "items[].chance is an estimate: the records give no weights, so each pick among a group's options is assumed even (1/n, a repeated option counting as weight), compounded through nesting; of same-named groups, gated ones whose requirement holds are the pick and ungated ones the fallback (inferred from the data, not stated by it).";

/// Where actor records live.
const ACTOR_DIR: &str = "/actor/actors/";

/// Loadout files, as archive names are matched (lower case, `/`, no `data/`).
const LOADOUT_DIR: &str = "scripts/loadouts/";

/// Nesting deeper than this is cut (a loop in the records would otherwise never end).
const MAX_DEPTH: usize = 8;

/// The gear slots, in the order the screens list them.
pub const SLOTS: &[&str] = &[
    "Undersuit",
    "Helmet",
    "Torso",
    "Arms",
    "Legs",
    "Backpack",
    "Clothing",
    "Primary weapon",
    "Sidearm",
    "Utility",
    "Optics",
    "Weapon attachments",
    "Magazines",
    "Grenades",
    "Medical",
    "Carried",
];

/// One item in a port of a worked-out loadout. In a group's option, `chance` is the chance
/// this entry is picked when the group applies (`1/n`, compounded through nesting) and
/// `requires` what it depends on beyond that (a requirement not read, a mission's outfit).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GearEntry {
    /// The port it sits in (`Armor_Helmet`, `wep_sidearm`); empty for carried inventory.
    pub port: String,
    /// The gear slot ([`SLOTS`]).
    pub slot: String,
    /// The item's class name.
    pub item: String,
    /// The game's name, when it has a real one.
    pub name: Option<String>,
    pub chance: f64,
    pub requires: Vec<String>,
}

/// One pick of a loadout: one of `options` (each a list of entries; an empty one means
/// "nothing in these slots").
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GearGroup {
    /// `Outfit`, `PrimaryWeapon`, `Grenades` (the kit's own word).
    pub group: String,
    pub options: Vec<Vec<GearEntry>>,
}

/// One item an actor can spawn with: the chance it is there (even picks assumed), and, when
/// that chance holds only under a condition the data does not settle, the conditions: each
/// entry one alternative (`outfit picked by the mission (X)`, `A and B`), the chance being
/// the likeliest of them.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GearItem {
    pub item: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub slot: String,
    pub chance: f64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requires: Vec<String>,
}

/// A loadout's `WearRange`, as the data gives it (`minimum`, `maximum`; 0 to 1).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Wear {
    pub min: f64,
    pub max: f64,
}

impl Wear {
    fn union(a: Option<Wear>, b: Option<Wear>) -> Option<Wear> {
        match (a, b) {
            (Some(x), Some(y)) => Some(Wear {
                min: x.min.min(y.min),
                max: x.max.max(y.max),
            }),
            (x, None) => x,
            (None, y) => y,
        }
    }
}

/// One actor (NPC type) with gear.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NpcActor {
    /// The record's class name.
    pub class: String,
    /// Readable words from the class name (the records carry no display name).
    pub name: String,
    /// The primary organization record's name (`genericcriminal`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub faction: Option<String>,
    /// The body's loot table record (`loottable_actor_acepilot`, lower case as referenced).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loot_table: Option<String>,
    /// Whether the body can be looted at all.
    pub lootable: bool,
    /// The loadout's wear range, when one is set (`None`: the data gives none here).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wear: Option<Wear>,
    /// Every item it can spawn with, by slot ([`SLOTS`]) then likeliest first.
    #[serde(default)]
    pub items: Vec<GearItem>,
}

/// The dataset as the query side reads it.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NpcLoadouts {
    #[serde(default)]
    pub actors: Vec<NpcActor>,
    /// Loot table -> the actors whose body rolls it.
    #[serde(default)]
    pub by_loot_table: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Summary {
    actors: usize,
    with_gear: usize,
    items: usize,
    kits: usize,
    loadout_files: usize,
    missing_loadout_files: usize,
    unlootable_dropped: usize,
    /// Kit groups left out because the actor's tags fail their requirement.
    groups_failed: usize,
    /// Requirements of a type this code does not read (kept as a label).
    requirements_not_read: usize,
    /// Loadouts nested deeper than [`MAX_DEPTH`], cut.
    depth_cut: usize,
}

/// A port and the class in it, as the records say.
#[derive(Debug, Clone, PartialEq)]
struct Picked {
    port: String,
    item: String,
}

/// A loadout worked out: what is always there and the picks.
#[derive(Debug, Clone, Default)]
struct Resolved {
    fixed: Vec<Picked>,
    groups: Vec<RGroup>,
    /// What this alternative depends on (a mission's outfit, a requirement not read).
    when: Vec<String>,
    wear: Option<Wear>,
}

#[derive(Debug, Clone, Default)]
struct RGroup {
    group: String,
    options: Vec<Resolved>,
    /// The options are alternatives a mission (or a condition) picks, not an even pick:
    /// each at its full chance under its own `when`.
    exclusive: bool,
}

impl Resolved {
    fn merge(&mut self, other: Resolved) {
        self.fixed.extend(other.fixed);
        self.groups.extend(other.groups);
        self.wear = Wear::union(self.wear, other.wear);
    }
}

/// A requirement against the actor's tags.
#[derive(Debug, Clone, PartialEq)]
enum Gate {
    /// No requirement.
    Open,
    /// A requirement that holds.
    Met,
    Failed,
    /// A requirement this code cannot read.
    Unread,
}

/// What an undecidable requirement says beside its gear.
pub const NOT_READ: &str = "condition not read";

/// Both gates at once (a kit's and its group's).
fn gate_and(a: Gate, b: Gate) -> Gate {
    use Gate::*;
    match (a, b) {
        (Failed, _) | (_, Failed) => Failed,
        (Unread, _) | (_, Unread) => Unread,
        (Met, _) | (_, Met) => Met,
        _ => Open,
    }
}

/// What is known of an item record.
#[derive(Debug, Clone, Default)]
struct ItemInfo {
    class: String,
    name: Option<String>,
    ty: String,
    sub_type: String,
    unlootable: bool,
}

/// Ports that hold the body, not gear.
const COSMETIC_PORTS: &[&str] = &[
    "body_itemport",
    "head_itemport",
    "eyes_itemport",
    "hair_itemport",
    "beard_itemport",
    "teeth_itemport",
    "eyelashes_itemport",
    "eyebrow_itemport",
    "lens_itemport",
    "skin_variant",
    "material_variant",
    "mobiglas_attach",
    "mobiglas_screen_attach",
    "legacy_mobiglas_screen_attach",
];

/// Item types that are the body itself.
const COSMETIC_TYPES: &[&str] = &[
    "char_body",
    "char_head",
    "char_skin",
    "char_hair",
    "char_accessory_head",
    "mobiglas",
];

/// The gear slot of an item in a port: from the item's type when its record is known (a
/// carried item of another type is `Carried`), else from the port name; `None` for anything
/// that is no personal gear (an ATLS's ship gun, vehicle hardpoints, props in hand).
fn slot_of(port: &str, info: Option<&ItemInfo>) -> Option<&'static str> {
    match info {
        Some(i) => type_slot(i).or_else(|| {
            const CARRIED: &[&str] = &["misc", "food", "drink", "gadget", "currency"];
            (port.is_empty() && CARRIED.contains(&i.ty.to_ascii_lowercase().as_str()))
                .then_some("Carried")
        }),
        None => port_slot(port),
    }
}

/// The slot a port name says.
fn port_slot(port: &str) -> Option<&'static str> {
    let p = port.to_ascii_lowercase();
    match p.as_str() {
        "armor_undersuit" => Some("Undersuit"),
        "armor_helmet" => Some("Helmet"),
        "armor_torso" => Some("Torso"),
        "armor_arms" => Some("Arms"),
        "armor_legs" => Some("Legs"),
        "backpack" => Some("Backpack"),
        "hat_itemport" | "eye_accessories_itemport" => Some("Clothing"),
        "wep_sidearm" => Some("Sidearm"),
        "optics_attach" => Some("Optics"),
        "barrel_attach" | "underbarrel_attach" => Some("Weapon attachments"),
        "" => Some("Carried"),
        _ if p.starts_with("clothing_") => Some("Clothing"),
        _ if p.starts_with("wep_stocked")
            || p.starts_with("weapon_attach_hand")
            || p.starts_with("righthandweapon") =>
        {
            Some("Primary weapon")
        }
        _ if p.starts_with("utility_attach") => Some("Utility"),
        _ if p.starts_with("magazine_attach") => Some("Magazines"),
        _ if p.starts_with("grenade_attach") => Some("Grenades"),
        _ if p.starts_with("medpen_attach") || p.starts_with("oxypen_attach") => Some("Medical"),
        _ => None,
    }
}

/// The slot an item's type says.
fn type_slot(info: &ItemInfo) -> Option<&'static str> {
    let ty = info.ty.to_ascii_lowercase();
    let sub = info.sub_type.to_ascii_lowercase();
    Some(match ty.as_str() {
        "char_armor_undersuit" => "Undersuit",
        "char_armor_helmet" => "Helmet",
        "char_armor_torso" => "Torso",
        "char_armor_arms" => "Arms",
        "char_armor_legs" => "Legs",
        "char_armor_backpack" | "char_clothing_backpack" => "Backpack",
        "char_accessory_eyes" => "Clothing",
        _ if ty.starts_with("char_clothing") => "Clothing",
        "weaponpersonal" => match sub.as_str() {
            "small" => "Sidearm",
            "grenade" => "Grenades",
            "knife" | "gadget" => "Utility",
            _ => "Primary weapon",
        },
        "weaponattachment" => match sub.as_str() {
            "ironsight" => "Optics",
            "magazine" => "Magazines",
            _ => "Weapon attachments",
        },
        "fps_consumable" => "Medical",
        _ => return None,
    })
}

/// Position of a slot in [`SLOTS`] (unknown ones last).
pub fn slot_rank(slot: &str) -> usize {
    SLOTS.iter().position(|s| *s == slot).unwrap_or(SLOTS.len())
}

/// Readable words from an actor class name (the archetype names are their folder path joined
/// by `-`), split on `-`, `_` and at case changes, without the words every actor shares
/// (`PU`, `Human`, `Enemy`, `NPC`, ...) and without repeats (a word, or its plural, said
/// before). `PU_Human_Enemy_GroundCombat_NPC_GenericCriminal_AcePilot` -> `Generic criminal
/// ace pilot`; `NPC_Archetypes-Male-Human-Civilians-Levski-Civilian_Levski_01` -> `Male
/// civilians levski 01`.
pub fn readable_class(class: &str) -> String {
    const NOISE: &[&str] = &[
        "pu",
        "human",
        "enemy",
        "npc",
        "groundcombat",
        "archetype",
        "archetypes",
    ];
    let stem = |w: &str| {
        let l = w.to_ascii_lowercase();
        match l.strip_suffix('s') {
            Some(s) if s.len() > 2 => s.to_string(),
            _ => l,
        }
    };
    let mut words: Vec<String> = Vec::new();
    for part in class
        .split(['-', '_'])
        .filter(|p| !p.is_empty() && !NOISE.contains(&p.to_ascii_lowercase().as_str()))
    {
        // A number right after a repeated word repeats too (`Area18` ... `Area18_16`).
        let mut skipped = false;
        for w in split_case(part) {
            let s = stem(&w);
            let digits = w.bytes().all(|b| b.is_ascii_digit());
            if if digits {
                skipped && words.contains(&w)
            } else {
                words.iter().any(|x| stem(x) == s)
            } {
                skipped = true;
                continue;
            }
            skipped = false;
            words.push(w);
        }
    }
    if words.is_empty() {
        return class.to_string();
    }
    let mut out: Vec<String> = Vec::new();
    for (i, w) in words.iter().enumerate() {
        let acronym = w.len() > 1 && w.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit());
        out.push(if acronym {
            w.clone()
        } else if i == 0 {
            let mut c = w.chars();
            c.next()
                .map(|f| f.to_uppercase().collect::<String>() + &c.as_str().to_lowercase())
                .unwrap_or_default()
        } else {
            w.to_lowercase()
        });
    }
    out.join(" ")
}

/// `GenericCriminal` -> `Generic`, `Criminal`; `ASDGrunt` -> `ASD`, `Grunt`; digits apart.
fn split_case(s: &str) -> Vec<String> {
    let cs: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut cur = String::new();
    for (i, &c) in cs.iter().enumerate() {
        let prev = i.checked_sub(1).map(|j| cs[j]);
        let next = cs.get(i + 1).copied();
        let brk = match prev {
            None => false,
            Some(p) => {
                (c.is_ascii_uppercase() && p.is_ascii_lowercase())
                    || (c.is_ascii_uppercase()
                        && p.is_ascii_uppercase()
                        && next.is_some_and(|n| n.is_ascii_lowercase()))
                    || (c.is_ascii_digit() != p.is_ascii_digit())
            }
        };
        if brk && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
        cur.push(c);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Archive name -> the key loadout paths are matched by (`scripts/loadouts/...xml`).
fn loadout_key(name: &str) -> String {
    let n = name.replace('\\', "/").to_ascii_lowercase();
    let n = n.trim_start_matches('/');
    n.strip_prefix("data/").unwrap_or(n).to_string()
}

/// A loadout file's items, in document order: every `<Item>` with a class, nested ones too.
fn xml_picks(root: &XmlNode) -> Vec<Picked> {
    fn walk(n: &XmlNode, out: &mut Vec<Picked>) {
        if n.tag.eq_ignore_ascii_case("Item") {
            let item = n.attr("itemName").unwrap_or("").trim();
            if !item.is_empty() {
                out.push(Picked {
                    port: n.attr("portName").unwrap_or("").to_string(),
                    item: item.to_string(),
                });
            }
        }
        for c in &n.children {
            walk(c, out);
        }
    }
    let mut out = Vec::new();
    walk(root, &mut out);
    out
}

/// A loadout file as a tree: CryXmlB, else XML text (elements and attributes only).
pub fn parse_loadout_file(bytes: &[u8]) -> Result<XmlNode, String> {
    if let Some(at) = find_cryxml(bytes) {
        return parse_cryxml(bytes, at).map_err(|e| e.to_string());
    }
    let text = std::str::from_utf8(bytes).map_err(|_| "neither CryXmlB nor UTF-8 text")?;
    parse_text_xml(text)
}

fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// Minimal XML text reader for loadout files: elements, attributes, comments, declarations;
/// text content is ignored.
fn parse_text_xml(text: &str) -> Result<XmlNode, String> {
    let bad = || "malformed XML".to_string();
    let b = text.as_bytes();
    let mut i = 0;
    let mut stack: Vec<XmlNode> = Vec::new();
    let mut root: Option<XmlNode> = None;
    while i < b.len() {
        if b[i] != b'<' {
            i += 1;
            continue;
        }
        let rest = &text[i..];
        if rest.starts_with("<!--") {
            i += rest.find("-->").ok_or_else(bad)? + 3;
            continue;
        }
        if rest.starts_with("<?") || rest.starts_with("<!") {
            i += rest.find('>').ok_or_else(bad)? + 1;
            continue;
        }
        if rest.starts_with("</") {
            i += rest.find('>').ok_or_else(bad)? + 1;
            let done = stack.pop().ok_or_else(bad)?;
            match stack.last_mut() {
                Some(p) => p.children.push(done),
                None => root = Some(done),
            }
            continue;
        }
        // An opening tag: name, attributes, `/>` or `>`.
        let mut j = i + 1;
        while j < b.len() && !b[j].is_ascii_whitespace() && b[j] != b'>' && b[j] != b'/' {
            j += 1;
        }
        let mut node = XmlNode {
            tag: text[i + 1..j].to_string(),
            ..Default::default()
        };
        let closed;
        loop {
            while j < b.len() && b[j].is_ascii_whitespace() {
                j += 1;
            }
            if j >= b.len() {
                return Err(bad());
            }
            if b[j] == b'>' {
                closed = false;
                j += 1;
                break;
            }
            if b[j] == b'/' {
                if b.get(j + 1) != Some(&b'>') {
                    return Err(bad());
                }
                closed = true;
                j += 2;
                break;
            }
            let k0 = j;
            while j < b.len() && b[j] != b'=' && !b[j].is_ascii_whitespace() {
                j += 1;
            }
            let key = text[k0..j].to_string();
            while j < b.len() && (b[j].is_ascii_whitespace() || b[j] == b'=') {
                j += 1;
            }
            let q = *b.get(j).ok_or_else(bad)?;
            if q != b'"' && q != b'\'' {
                return Err(bad());
            }
            let v0 = j + 1;
            let len = text[v0..].find(q as char).ok_or_else(bad)?;
            node.attrs.push((key, unescape(&text[v0..v0 + len])));
            j = v0 + len + 1;
        }
        i = j;
        if closed {
            match stack.last_mut() {
                Some(p) => p.children.push(node),
                None => root = Some(node),
            }
        } else {
            stack.push(node);
        }
    }
    if !stack.is_empty() {
        return Err(bad());
    }
    root.ok_or_else(bad)
}

/// Works out loadouts against the scoped records and the archive's loadout files, for one
/// actor at a time (requirements read its tags, [`Resolver::actor`]).
struct Resolver<'a> {
    records: &'a RecordSet,
    files: &'a dyn FileSource,
    tags: &'a TagIndex,
    items: &'a HashMap<String, ItemInfo>,
    /// Lower-case record path -> class name, for `entityClassReference` paths.
    item_paths: &'a HashMap<String, String>,
    /// [`loadout_key`] -> archive name.
    xml_names: &'a HashMap<String, String>,
    xml_cache: RefCell<HashMap<String, Option<Rc<Vec<Picked>>>>>,
    missing: RefCell<BTreeSet<String>>,
    unreadable: RefCell<BTreeSet<String>>,
    kits: RefCell<BTreeSet<String>>,
    /// The tag paths of the actor being worked out.
    actor: RefCell<Vec<String>>,
    groups_failed: Cell<usize>,
    not_read: Cell<usize>,
    depth_cut: Cell<usize>,
}

/// Whether the actor's tags hold `tag`: the tag itself or one of its children
/// (`AI/Archetype/PU_enemy/PU_CQC` is held by `.../PU_CQC/PU_CQC_Elite`).
fn holds(actor: &[String], tag: &str) -> bool {
    actor
        .iter()
        .any(|t| t == tag || (t.len() > tag.len() && t.starts_with(tag) && t.as_bytes()[tag.len()] == b'/'))
}

/// Every tag a value names: `{_RecordName_: "Tag.x"}` objects and `"Tag.x"` strings.
fn tag_names(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Object(m) => {
            if let Some(n) = m.get("_RecordName_").and_then(Value::as_str)
                && n.starts_with("Tag.")
            {
                out.push(n.to_string());
            }
            for x in m.values() {
                tag_names(x, out);
            }
        }
        Value::Array(a) => a.iter().for_each(|x| tag_names(x, out)),
        Value::String(s) if s.starts_with("Tag.") => out.push(s.clone()),
        _ => {}
    }
}

/// The field [`NpcLoadoutsDomain::prune`] keeps an actor's geometry tags in.
const GEOMETRY_TAGS: &str = "_geometryTags";

impl Resolver<'_> {
    fn tag_path(&self, name: &str) -> String {
        match self.tags.get(name).filter(|p| !p.is_empty()) {
            Some(p) => p.clone(),
            None => name.strip_prefix("Tag.").unwrap_or(name).to_string(),
        }
    }

    fn tag_list(&self, v: Option<&Value>) -> Vec<String> {
        array(v)
            .iter()
            .filter_map(|t| ref_name(Some(t)))
            .map(|n| self.tag_path(n))
            .collect()
    }

    /// The actor's tag paths: its record `tags` and its geometry's model tags.
    fn actor_tags(&self, r: &GameRecord) -> Vec<String> {
        let mut names = Vec::new();
        tag_names(r.value.get("tags").unwrap_or(&Value::Null), &mut names);
        tag_names(r.value.get(GEOMETRY_TAGS).unwrap_or(&Value::Null), &mut names);
        if let Some(g) = component(r, "SGeometryResourceParams") {
            tag_names(&Value::Object(g.clone()), &mut names);
        }
        let mut out: Vec<String> = names.iter().map(|n| self.tag_path(n)).collect();
        out.sort();
        out.dedup();
        out
    }

    /// A `Requirement` against the actor's tags: tags required and banned, `OR`, `NOT`,
    /// nested. A type it does not know is [`Gate::Unread`].
    fn gate(&self, req: Option<&Value>, depth: usize) -> Gate {
        let Some(r) = req.filter(|r| r.is_object()) else {
            return Gate::Open;
        };
        if depth > MAX_DEPTH {
            return Gate::Unread;
        }
        let met = |b: bool| if b { Gate::Met } else { Gate::Failed };
        match str_(r.get("_Type_")).unwrap_or("") {
            "SLoadoutRequirementTags" => {
                let required = self.tag_list(r.get("RequiredTags"));
                let banned = self.tag_list(r.get("BannedTags"));
                if required.is_empty() && banned.is_empty() {
                    return Gate::Open;
                }
                let actor = self.actor.borrow();
                met(required.iter().all(|t| holds(&actor, t))
                    && !banned.iter().any(|t| holds(&actor, t)))
            }
            "SLoadoutRequirementOR" => {
                let parts: Vec<Gate> = array(r.get("Requirements"))
                    .iter()
                    .map(|x| self.gate(Some(x), depth + 1))
                    .collect();
                if parts.iter().any(|g| matches!(g, Gate::Met | Gate::Open)) {
                    Gate::Met
                } else if parts.contains(&Gate::Unread) {
                    Gate::Unread
                } else {
                    Gate::Failed
                }
            }
            "SLoadoutRequirementNOT" => match self.gate(r.get("RequirementToInvert"), depth + 1) {
                Gate::Met | Gate::Open => Gate::Failed,
                Gate::Failed => Gate::Met,
                Gate::Unread => Gate::Unread,
            },
            _ => Gate::Unread,
        }
    }

    /// The class an entry or inventory item names: `entityClassName`, else its reference
    /// (a record path or a `{_RecordName_}`).
    fn class_of(&self, v: &Map<String, Value>) -> Option<String> {
        if let Some(n) = str_(v.get("entityClassName")) {
            return Some(n.to_string());
        }
        let r = v.get("entityClassReference")?;
        if let Some(name) = ref_name(Some(r)) {
            let short = name.split_once('.').map_or(name, |(_, s)| s);
            return (!short.is_empty()).then(|| short.to_string());
        }
        let s = str_(Some(r))?;
        if let Some(c) = ref_path_str(s).and_then(|p| self.item_paths.get(&p)) {
            return Some(c.clone());
        }
        referenced_name(Some(r)).map(str::to_string)
    }

    fn xml(&self, path: &str) -> Option<Rc<Vec<Picked>>> {
        let key = loadout_key(path);
        if let Some(hit) = self.xml_cache.borrow().get(&key) {
            return hit.clone();
        }
        let found = match self.xml_names.get(&key) {
            None => {
                self.missing.borrow_mut().insert(path.to_string());
                None
            }
            Some(name) => match self.files.read(name).and_then(|b| parse_loadout_file(&b)) {
                Ok(root) => Some(Rc::new(xml_picks(&root))),
                Err(e) => {
                    self.unreadable.borrow_mut().insert(format!("{name}: {e}"));
                    None
                }
            },
        };
        self.xml_cache.borrow_mut().insert(key, found.clone());
        found
    }

    fn inventory(&self, v: &Map<String, Value>, out: &mut Resolved) {
        for inv in array(v.get("InventoryItems")) {
            let Some(inv) = inv.as_object() else { continue };
            if num(inv.get("amount")).is_some_and(|a| a <= 0.0) {
                continue;
            }
            if let Some(c) = self.class_of(inv) {
                out.fixed.push(Picked {
                    port: String::new(),
                    item: c,
                });
            }
        }
    }

    fn entry(&self, e: &Map<String, Value>, depth: usize, out: &mut Resolved) {
        if let Some(c) = self.class_of(e) {
            out.fixed.push(Picked {
                port: str_(e.get("itemPortName")).unwrap_or("").to_string(),
                item: c,
            });
        }
        if let Some(l) = e.get("loadout") {
            out.merge(self.resolve(l, depth + 1));
        }
    }

    /// One `SLoadoutAssortment` (a reference): its groups with their gate.
    fn kit(&self, reference: &Value, depth: usize, out: &mut Vec<(String, Gate, Vec<Resolved>)>) {
        let Some(rec) = self.records.follow(Some(reference)) else {
            return;
        };
        self.kits.borrow_mut().insert(rec.short_name.clone());
        let v = &rec.value;
        let kit_gate = self.gate(v.get("Requirement"), 0);
        for g in array(v.get("GroupedLoadouts")) {
            let gate = gate_and(kit_gate.clone(), self.gate(g.get("Requirement"), 0));
            let options: Vec<Resolved> = array(g.get("Loadouts"))
                .iter()
                .map(|l| self.resolve(l, depth + 1))
                .collect();
            if options.is_empty() {
                continue;
            }
            out.push((str_(g.get("Group")).unwrap_or("").to_string(), gate, options));
        }
    }

    /// The kit groups of one loadout as its picks: groups of one name are one pick. A
    /// failed gate drops its group; of the rest, the met gated groups are the pick when
    /// there is one, else the open ones (the fallback); groups whose requirement is not read
    /// join with a label. Their options are pooled (a repeated option weighs more).
    fn pool(&self, groups: Vec<(String, Gate, Vec<Resolved>)>, out: &mut Resolved) {
        let mut names: Vec<String> = Vec::new();
        let mut by: HashMap<String, Vec<(Gate, Vec<Resolved>)>> = HashMap::new();
        for (name, gate, options) in groups {
            if gate == Gate::Failed {
                self.groups_failed.set(self.groups_failed.get() + 1);
                continue;
            }
            if gate == Gate::Unread {
                self.not_read.set(self.not_read.get() + 1);
            }
            let key = name.to_ascii_lowercase();
            if !by.contains_key(&key) {
                names.push(name.clone());
            }
            by.entry(key).or_default().push((gate, options));
        }
        for name in names {
            let list = by.remove(&name.to_ascii_lowercase()).unwrap_or_default();
            let any_met = list.iter().any(|(g, _)| *g == Gate::Met);
            let mut options: Vec<Resolved> = Vec::new();
            for (gate, opts) in list {
                let take = match gate {
                    Gate::Met => true,
                    Gate::Open => !any_met,
                    Gate::Unread => true,
                    Gate::Failed => false,
                };
                if !take {
                    continue;
                }
                for mut o in opts {
                    if gate == Gate::Unread && !o.when.iter().any(|w| w == NOT_READ) {
                        o.when.push(NOT_READ.to_string());
                    }
                    out.wear = Wear::union(out.wear, o.wear);
                    options.push(o);
                }
            }
            if !options.is_empty() {
                out.groups.push(RGroup {
                    group: name,
                    options,
                    exclusive: false,
                });
            }
        }
    }

    fn resolve(&self, v: &Value, depth: usize) -> Resolved {
        let mut out = Resolved::default();
        let Some(obj) = v.as_object() else {
            return out;
        };
        if depth > MAX_DEPTH {
            self.depth_cut.set(self.depth_cut.get() + 1);
            return out;
        }
        out.wear = wear_of(Some(v));
        match str_(obj.get("_Type_")).unwrap_or("") {
            "SItemPortLoadoutXMLParams" => {
                if let Some(p) = str_(obj.get("loadoutPath"))
                    && let Some(picks) = self.xml(p)
                {
                    out.fixed.extend(picks.iter().cloned());
                }
            }
            "SItemPortLoadoutManualParams" => {
                for e in array(obj.get("entries")) {
                    if let Some(e) = e.as_object() {
                        self.entry(e, depth, &mut out);
                    }
                }
            }
            "SItemPortLoadoutRandomGeneratedParams" => {
                let mut groups = Vec::new();
                for a in array(obj.get("LoadoutAssortments")) {
                    self.kit(a, depth, &mut groups);
                }
                self.pool(groups, &mut out);
            }
            "SItemPortLoadoutOutfitParams" => {
                let base = obj
                    .get("BaseLoadout")
                    .map(|b| self.resolve(b, depth + 1))
                    .unwrap_or_default();
                let mut named = false;
                let mut outfits: Vec<Resolved> = array(obj.get("Outfits"))
                    .iter()
                    .map(|o| {
                        let mut r = Resolved::default();
                        for l in array(o.get("Loadouts")) {
                            r.merge(self.resolve(l, depth + 1));
                        }
                        let names = self.tag_list(o.get("OutfitNameTags"));
                        named |= !names.is_empty();
                        r.when = names;
                        r
                    })
                    .collect();
                // An outfit fills a port over the base loadout (the base is what it leaves
                // free): a base item in a port some outfit fills goes into each outfit that
                // leaves that port free.
                let ports = |r: &Resolved| -> BTreeSet<String> {
                    r.fixed
                        .iter()
                        .filter(|p| !p.port.is_empty())
                        .map(|p| p.port.to_ascii_lowercase())
                        .collect()
                };
                let filled: Vec<BTreeSet<String>> = outfits.iter().map(ports).collect();
                let any: BTreeSet<String> = filled.iter().flatten().cloned().collect();
                let mut base = base;
                let (shadowed, kept): (Vec<Picked>, Vec<Picked>) = base
                    .fixed
                    .drain(..)
                    .partition(|p| any.contains(&p.port.to_ascii_lowercase()));
                base.fixed = kept;
                out.merge(base);
                for (o, f) in outfits.iter_mut().zip(&filled) {
                    for p in &shadowed {
                        if !f.contains(&p.port.to_ascii_lowercase()) {
                            o.fixed.push(p.clone());
                        }
                    }
                }
                match outfits.len() {
                    0 => out.fixed.extend(shadowed),
                    1 => out.merge(outfits.pop().unwrap_or_default()),
                    _ => {
                        // Named outfits: the mission picks one (no even pick).
                        for o in &mut outfits {
                            let label = if o.when.is_empty() {
                                "default outfit (no mission outfit picked)".to_string()
                            } else {
                                format!("outfit picked by the mission ({})", o.when.join(", "))
                            };
                            o.when = if named { vec![label] } else { Vec::new() };
                            out.wear = Wear::union(out.wear, o.wear);
                        }
                        out.groups.push(RGroup {
                            group: "Outfit".into(),
                            options: outfits,
                            exclusive: named,
                        });
                    }
                }
            }
            "SItemPortLoadoutRecordParams" => {
                if let Some(rec) = self.records.follow(obj.get("loadoutRecord")) {
                    let inner = rec.value.get("loadout").unwrap_or(&rec.value);
                    out.merge(self.resolve(inner, depth + 1));
                }
            }
            _ => {}
        }
        self.inventory(obj, &mut out);
        out
    }

    /// A pick as an output entry, or `None` when it is no lootable gear.
    fn gear(&self, p: &Picked, unlootable: &mut usize) -> Option<GearEntry> {
        let info = self.items.get(&p.item.to_ascii_lowercase());
        if let Some(i) = info {
            let ty = i.ty.to_ascii_lowercase();
            if COSMETIC_TYPES.iter().any(|c| ty.starts_with(c)) {
                return None;
            }
            if i.unlootable {
                *unlootable += 1;
                return None;
            }
        }
        if COSMETIC_PORTS.contains(&p.port.to_ascii_lowercase().as_str()) {
            return None;
        }
        let slot = slot_of(&p.port, info)?;
        Some(GearEntry {
            port: p.port.clone(),
            slot: slot.to_string(),
            item: info.map_or_else(|| p.item.clone(), |i| i.class.clone()),
            name: info.and_then(|i| i.name.clone()),
            chance: 1.0,
            requires: Vec::new(),
        })
    }

    /// An option's gear, one entry per item and condition set, each with its chance (`scale`
    /// times its nested picks) and what it depends on (the `when` of the alternatives it came
    /// through). An item the option holds more than once (two magazines of one class beside
    /// their weapon) is there as often as the option is, not twice as often: fixed gear is
    /// `scale`, within one nested pick the options' chances add up (at most `scale`), and
    /// separate sources are independent.
    fn flat(&self, o: &Resolved, scale: f64, requires: &[String], unlootable: &mut usize) -> Vec<GearEntry> {
        let mut req: Vec<String> = requires.to_vec();
        for w in &o.when {
            if !req.contains(w) {
                req.push(w.clone());
            }
        }
        let same = |a: &GearEntry, b: &GearEntry| a.item.eq_ignore_ascii_case(&b.item) && a.requires == b.requires;
        // Per entry: the chances of its sources (an item seen once keeps its chance as is).
        let mut out: Vec<(GearEntry, Vec<f64>)> = Vec::new();
        let mut add = |e: GearEntry, p: f64| match out.iter_mut().find(|(x, _)| same(x, &e)) {
            Some((_, sources)) => sources.push(p),
            None => out.push((e, vec![p])),
        };
        for mut e in o.fixed.iter().filter_map(|p| self.gear(p, unlootable)) {
            e.requires = req.clone();
            add(e, scale);
        }
        for g in &o.groups {
            let n = g.options.len() as f64;
            let each = if g.exclusive { scale } else { scale / n };
            let mut here: Vec<GearEntry> = Vec::new();
            for opt in &g.options {
                for e in self.flat(opt, each, &req, unlootable) {
                    match here.iter_mut().find(|x| same(x, &e)) {
                        Some(h) => h.chance += e.chance,
                        None => here.push(e),
                    }
                }
            }
            for e in here {
                let p = e.chance.min(scale);
                add(e, p);
            }
        }
        out.into_iter()
            .map(|(mut e, sources)| {
                e.chance = match sources.as_slice() {
                    [one] => *one,
                    _ if scale > 0.0 => {
                        scale * (1.0 - sources.iter().map(|p| 1.0 - p / scale).product::<f64>())
                    }
                    _ => 0.0,
                };
                e
            })
            .collect()
    }
}

/// A condition set as one label (`A and B`), or `None` when it contradicts itself (`X` and
/// `not X`).
fn condition(set: &[String]) -> Option<String> {
    let mut s: Vec<&String> = set.iter().collect();
    s.sort();
    s.dedup();
    if s.iter().any(|x| s.iter().any(|y| **y == format!("not {x}"))) {
        return None;
    }
    Some(s.iter().map(|x| x.as_str()).collect::<Vec<_>>().join(" and "))
}

/// Per item: the chance it is there. Fixed gear is 1; within one pick the options' chances
/// add up (they exclude each other), at most 1; separate picks are independent. Gear that
/// depends on a condition is counted per condition set: when nothing unconditional holds
/// the item, its chance is the likeliest set's and the sets are its alternatives.
fn item_chances(fixed: &[GearEntry], groups: &[GearGroup]) -> Vec<GearItem> {
    struct Acc {
        first: usize,
        name: Option<String>,
        slot: String,
        /// Per condition label ("" for none): the chance it is missing.
        miss: Vec<(String, f64)>,
    }
    let mut acc: Vec<(String, Acc)> = Vec::new();
    let mut at: HashMap<String, usize> = HashMap::new();
    let mut get = |e: &GearEntry, acc: &mut Vec<(String, Acc)>| -> usize {
        *at.entry(e.item.to_ascii_lowercase()).or_insert_with(|| {
            acc.push((
                e.item.clone(),
                Acc {
                    first: acc.len(),
                    name: e.name.clone(),
                    slot: e.slot.clone(),
                    miss: Vec::new(),
                },
            ));
            acc.len() - 1
        })
    };
    let miss = |a: &mut Acc, label: &str, p: f64| {
        let p = p.clamp(0.0, 1.0);
        match a.miss.iter_mut().find(|(l, _)| l == label) {
            Some(m) => m.1 *= 1.0 - p,
            None => a.miss.push((label.to_string(), 1.0 - p)),
        }
    };
    for e in fixed {
        let i = get(e, &mut acc);
        miss(&mut acc[i].1, "", 1.0);
    }
    for g in groups {
        // Per (item, condition) in this pick: the options' chances summed.
        let mut here: Vec<(usize, String, f64)> = Vec::new();
        for e in g.options.iter().flatten() {
            let Some(label) = condition(&e.requires) else {
                continue;
            };
            let i = get(e, &mut acc);
            match here.iter_mut().find(|(x, l, _)| *x == i && *l == label) {
                Some(h) => h.2 += e.chance,
                None => here.push((i, label, e.chance)),
            }
        }
        for (i, label, p) in here {
            miss(&mut acc[i].1, &label, p);
        }
    }
    let mut out: Vec<(usize, GearItem)> = acc
        .into_iter()
        .filter_map(|(item, a)| {
            let open = a
                .miss
                .iter()
                .find(|(l, _)| l.is_empty())
                .map_or(0.0, |(_, m)| 1.0 - m);
            let (chance, requires) = if open > 0.0 {
                (open, Vec::new())
            } else {
                let mut sets: Vec<(String, f64)> = a
                    .miss
                    .iter()
                    .filter(|(l, _)| !l.is_empty())
                    .map(|(l, m)| (l.clone(), 1.0 - m))
                    .filter(|(_, p)| *p > 0.0)
                    .collect();
                sets.sort_by(|x, y| x.0.cmp(&y.0));
                let best = sets.iter().map(|(_, p)| *p).fold(0.0, f64::max);
                (best, sets.into_iter().map(|(l, _)| l).collect())
            };
            (chance > 0.0).then_some((
                a.first,
                GearItem {
                    item,
                    name: a.name,
                    slot: a.slot,
                    chance,
                    requires,
                },
            ))
        })
        .collect();
    out.sort_by(|(fa, a), (fb, b)| {
        slot_rank(&a.slot)
            .cmp(&slot_rank(&b.slot))
            .then_with(|| b.chance.total_cmp(&a.chance))
            .then_with(|| fa.cmp(fb))
    });
    out.into_iter().map(|(_, i)| i).collect()
}

fn wear_of(v: Option<&Value>) -> Option<Wear> {
    let w = v?.get("WearRange")?;
    Some(Wear {
        min: num(w.get("minimum"))?,
        max: num(w.get("maximum"))?,
    })
}

fn is_actor(r: &GameRecord) -> bool {
    r.ty == "EntityClassDefinition" && r.record_path.to_ascii_lowercase().contains(ACTOR_DIR)
}

fn faction_of(r: &GameRecord) -> Option<String> {
    array(r.value.get("StaticEntityClassData"))
        .iter()
        .find(|s| str_(s.get("_Type_")) == Some("SOrganizationRelationships"))
        .and_then(|s| s.get("primaryOrganization"))
        .and_then(|o| referenced_name(o.get("organization")))
        .map(str::to_string)
}

fn loot_table_of(r: &GameRecord) -> Option<String> {
    let c = component(r, "LootGenerationComponentParams")?.get("lootConfig")?;
    referenced_name(c.get("lootTableV3"))
        .or_else(|| referenced_name(c.get("lootTable")))
        .map(str::to_string)
}

impl Domain for NpcLoadoutsDomain {
    fn name(&self) -> &'static str {
        NAME
    }

    fn description(&self) -> &'static str {
        "What each NPC type can spawn wearing and wielding, from its loadout records (even picks assumed)."
    }

    fn filters(&self) -> &'static [&'static str] {
        &[
            "**/actor/actors/**",
            "**/loadoutkits/**",
            "**/entities/scitem/**",
            "**/tagdatabase/**",
        ]
    }

    fn files(&self) -> &'static [&'static str] {
        &["Data/Scripts/Loadouts/**"]
    }

    fn needs_localization(&self) -> bool {
        true
    }

    /// Actors keep their tags (the record's, and their geometry's model tags in
    /// [`GEOMETRY_TAGS`]), loadout, loot table, lootable flag and organization; items their
    /// tags and `AttachDef`. Kits and the tag database stay whole.
    fn prune(&self, record: &mut GameRecord) {
        if record.ty != "EntityClassDefinition" {
            return;
        }
        if is_actor(record) {
            let mut geometry = Vec::new();
            if let Some(g) = component(record, "SGeometryResourceParams") {
                tag_names(&Value::Object(g.clone()), &mut geometry);
            }
            prune_entity(
                &mut record.value,
                &["StaticEntityClassData", "tags"],
                &[
                    ("SEntityComponentDefaultLoadoutParams", None),
                    ("LootGenerationComponentParams", Some(&["lootConfig"])),
                    ("SEntityLootableContainerParams", Some(&[])),
                ],
            );
            if let Some(Value::Array(s)) = record.value.get_mut("StaticEntityClassData") {
                s.retain(|x| str_(x.get("_Type_")) == Some("SOrganizationRelationships"));
            }
            if let Some(obj) = record.value.as_object_mut() {
                obj.insert(
                    GEOMETRY_TAGS.into(),
                    Value::Array(geometry.into_iter().map(Value::String).collect()),
                );
            }
        } else {
            prune_entity(
                &mut record.value,
                &["tags"],
                &[("SAttachableComponentParams", Some(&["AttachDef"]))],
            );
        }
    }

    fn parse(&self, ctx: &ParseContext<'_>) -> Result<Map<String, Value>, DomainError> {
        let tags = build_tag_index(ctx.records.iter().find(|r| r.ty == "TagDatabase"));
        let mut items: HashMap<String, ItemInfo> = HashMap::new();
        let mut item_paths: HashMap<String, String> = HashMap::new();
        for r in ctx
            .records
            .of_type("EntityClassDefinition")
            .filter(|r| !is_actor(r))
        {
            let def = attach_def(r);
            let (localized, placeholder) = display_name(def, ctx.localization);
            let unlootable = array(r.value.get("tags")).iter().any(|t| {
                ref_name(Some(t))
                    .and_then(|n| tags.get(n))
                    .is_some_and(|p| p == UNLOOTABLE_TAG)
            });
            let info = ItemInfo {
                class: r.short_name.clone(),
                name: (!placeholder)
                    .then(|| localized.map(str::to_string))
                    .flatten()
                    .filter(|n| !n.trim().is_empty()),
                ty: str_(def.get("Type")).unwrap_or("").to_string(),
                sub_type: str_(def.get("SubType")).unwrap_or("").to_string(),
                unlootable,
            };
            if let Some(p) = ref_path_str(&r.record_path) {
                item_paths.insert(p, r.short_name.clone());
            }
            items.insert(r.short_name.to_ascii_lowercase(), info);
        }
        let xml_names: HashMap<String, String> = ctx
            .files
            .names()
            .iter()
            .filter_map(|n| {
                let k = loadout_key(n);
                k.starts_with(LOADOUT_DIR).then(|| (k, n.clone()))
            })
            .collect();
        let resolver = Resolver {
            records: ctx.records,
            files: ctx.files,
            tags: &tags,
            items: &items,
            item_paths: &item_paths,
            xml_names: &xml_names,
            xml_cache: RefCell::new(HashMap::new()),
            missing: RefCell::new(BTreeSet::new()),
            unreadable: RefCell::new(BTreeSet::new()),
            kits: RefCell::new(BTreeSet::new()),
            actor: RefCell::new(Vec::new()),
            groups_failed: Cell::new(0),
            not_read: Cell::new(0),
            depth_cut: Cell::new(0),
        };

        let mut actors: Vec<NpcActor> = Vec::new();
        let mut seen = 0usize;
        let mut unlootable = 0usize;
        let mut distinct: BTreeSet<String> = BTreeSet::new();
        for r in ctx.records.of_type("EntityClassDefinition").filter(|r| is_actor(r)) {
            let Some(loadout) =
                component(r, "SEntityComponentDefaultLoadoutParams").and_then(|c| c.get("loadout"))
            else {
                continue;
            };
            seen += 1;
            *resolver.actor.borrow_mut() = resolver.actor_tags(r);
            let res = resolver.resolve(loadout, 0);
            let fixed: Vec<GearEntry> = res
                .fixed
                .iter()
                .filter_map(|p| resolver.gear(p, &mut unlootable))
                .collect();
            let mut groups: Vec<GearGroup> = Vec::new();
            for g in &res.groups {
                let n = g.options.len() as f64;
                let each = if g.exclusive { 1.0 } else { 1.0 / n };
                let options: Vec<Vec<GearEntry>> = g
                    .options
                    .iter()
                    .map(|o| resolver.flat(o, each, &[], &mut unlootable))
                    .collect();
                if options.iter().all(Vec::is_empty) {
                    continue;
                }
                groups.push(GearGroup {
                    group: g.group.clone(),
                    options,
                });
            }
            let gear = item_chances(&fixed, &groups);
            if gear.is_empty() {
                continue;
            }
            distinct.extend(gear.iter().map(|i| i.item.to_ascii_lowercase()));
            actors.push(NpcActor {
                class: r.short_name.clone(),
                name: readable_class(&r.short_name),
                faction: faction_of(r),
                loot_table: loot_table_of(r),
                lootable: has_component(r, "SEntityLootableContainerParams"),
                wear: res.wear,
                items: gear,
            });
        }
        actors.sort_by(|a, b| compare_names(&a.class, &b.class));

        let mut by_loot_table: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for a in &actors {
            if let Some(t) = &a.loot_table {
                by_loot_table
                    .entry(t.clone())
                    .or_default()
                    .push(a.class.clone());
            }
        }

        let missing = resolver.missing.into_inner();
        if !missing.is_empty() {
            let first: Vec<&str> = missing.iter().take(3).map(String::as_str).collect();
            ctx.warn(format!(
                "{NAME}: {} loadout files the records name are not in the archive; left out ({}{})",
                missing.len(),
                first.join(", "),
                if missing.len() > 3 { ", ..." } else { "" }
            ));
        }
        let unreadable = resolver.unreadable.into_inner();
        if !unreadable.is_empty() {
            ctx.warn(format!(
                "{NAME}: {} loadout files could not be read; left out (first: {})",
                unreadable.len(),
                unreadable.iter().next().map(String::as_str).unwrap_or("")
            ));
        }
        let cut = resolver.depth_cut.get();
        if cut > 0 {
            ctx.warn(format!(
                "{NAME}: {cut} loadouts nested deeper than {MAX_DEPTH} levels were cut"
            ));
        }
        let summary = Summary {
            actors: seen,
            with_gear: actors.len(),
            items: distinct.len(),
            kits: resolver.kits.borrow().len(),
            loadout_files: resolver
                .xml_cache
                .borrow()
                .values()
                .filter(|v| v.is_some())
                .count(),
            missing_loadout_files: missing.len(),
            unlootable_dropped: unlootable,
            groups_failed: resolver.groups_failed.get(),
            requirements_not_read: resolver.not_read.get(),
            depth_cut: cut,
        };
        let mut body = Map::new();
        body.insert("_assumptions".into(), serde_json::json!([PICK_ASSUMPTIONS]));
        field(&mut body, "actors", &actors)?;
        field(&mut body, "byLootTable", &by_loot_table)?;
        field(&mut body, "summary", &summary)?;
        Ok(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cryxml::tests::{cryxml, node};
    use crate::fixtures::MemFiles;
    use crate::localization::Localization;
    use crate::records::normalize_record;
    use serde_json::json;

    const ROOT: &str = "file://./../../../../../libs/foundry/records/";
    const UNLOOT_GUID: &str = "add01cfc-0a7a-44b7-8e66-a9aaebab6210";
    const FEMALE_GUID: &str = "d0794a94-efb0-4cad-ad38-2558b4d3c253";
    const CQC_GUID: &str = "cqc";
    const CQC_ELITE_GUID: &str = "cqc-elite";
    const MISSION_GUID: &str = "mission-guard";

    /// A record at `dir/<short in lower case>`, named `ty.short`.
    fn rec(ty: &str, path: &str, value: Value) -> GameRecord {
        let short = path.rsplit('/').next().unwrap();
        let path = path.to_lowercase();
        let mut value = value;
        value
            .as_object_mut()
            .unwrap()
            .insert("_Type_".into(), json!(ty));
        normalize_record(
            json!({"_RecordName_": format!("{ty}.{short}"), "_RecordId_": format!("id-{short}"), "_RecordValue_": value}),
            &format!("libs/foundry/records/{path}"),
        )
        .unwrap()
    }

    fn tag_db() -> GameRecord {
        rec(
            "TagDatabase",
            "tagdatabase/tagdatabase.tagdatabase",
            json!({"tags": [{"tagName": "Global", "children": [
                {"tagName": "Item", "_RecordName_": "Tag.item", "children": [
                    {"tagName": "Inventory", "_RecordName_": "Tag.inv", "children": [
                        {"tagName": "Unlootable", "_RecordName_": format!("Tag.{UNLOOT_GUID}")}]}]},
                {"tagName": "Gender", "_RecordName_": "Tag.gender", "children": [
                    {"tagName": "Female", "_RecordName_": format!("Tag.{FEMALE_GUID}")}]},
                {"tagName": "AI", "_RecordName_": "Tag.ai", "children": [
                    {"tagName": "Archetype", "_RecordName_": "Tag.arch", "children": [
                        {"tagName": "PU_CQC", "_RecordName_": format!("Tag.{CQC_GUID}"), "children": [
                            {"tagName": "PU_CQC_Elite", "_RecordName_": format!("Tag.{CQC_ELITE_GUID}")}]}]}]},
                {"tagName": "Mission", "_RecordName_": "Tag.mission", "children": [
                    {"tagName": "Outfit", "_RecordName_": "Tag.mission-outfit", "children": [
                        {"tagName": "Guard", "_RecordName_": format!("Tag.{MISSION_GUID}")}]}]}]}]}),
        )
    }

    fn item(class: &str, ty: &str, sub: &str, unlootable: bool) -> GameRecord {
        let mut v = json!({"Components": [{"_Type_": "SAttachableComponentParams", "AttachDef": {
            "Type": ty, "SubType": sub, "Localization": {"Name": format!("@{class}_name")}}}]});
        if unlootable {
            v["tags"] = json!([{"_RecordName_": format!("Tag.{UNLOOT_GUID}")}]);
        }
        rec(
            "EntityClassDefinition",
            &format!("entities/scitem/characters/{class}"),
            v,
        )
    }

    fn actor(class: &str, loadout: Value, loot: Option<&str>) -> GameRecord {
        let mut comps = vec![
            json!({"_Type_": "SEntityComponentDefaultLoadoutParams", "loadout": loadout}),
            json!({"_Type_": "SEntityLootableContainerParams", "radius": 1.6}),
            json!({"_Type_": "SActorComponentParams", "big": [1, 2, 3]}),
        ];
        if let Some(t) = loot {
            comps.push(json!({"_Type_": "LootGenerationComponentParams", "lootConfig": {
                "lootTable": format!("{ROOT}lootgeneration/loottables/{t}.json"), "lootTableV3": null}}));
        }
        rec(
            "EntityClassDefinition",
            &format!("actor/actors/{class}"),
            json!({"StaticEntityClassData": [
                {"_Type_": "EntityUIDisplayParams", "displayName": "@LOC_UNINITIALIZED"},
                {"_Type_": "SOrganizationRelationships", "primaryOrganization": {
                    "organization": format!("{ROOT}factions/legacy/genericcriminal.json")}}],
                "Components": comps}),
        )
    }

    fn xml_ref(path: &str) -> Value {
        json!({"_Type_": "SItemPortLoadoutXMLParams", "WearRange": null, "InventoryItems": [], "loadoutPath": path})
    }

    fn manual(entries: Value) -> Value {
        json!({"_Type_": "SItemPortLoadoutManualParams", "InventoryItems": [], "entries": entries})
    }

    fn entry(port: &str, class: &str) -> Value {
        json!({"_Type_": "SItemPortLoadoutEntryParams", "itemPortName": port, "entityClassName": class,
               "entityClassReference": null, "loadout": null})
    }

    fn random(kits: &[&str]) -> Value {
        let refs: Vec<String> = kits
            .iter()
            .map(|k| format!("{ROOT}loadoutkits/pu_loadoutbundles/{k}.json"))
            .collect();
        json!({"_Type_": "SItemPortLoadoutRandomGeneratedParams", "WearRange": {"_Type_": "Range", "minimum": 0.0, "maximum": 0.5},
               "InventoryItems": [], "LoadoutAssortments": refs})
    }

    fn kit(name: &str, groups: Value) -> GameRecord {
        rec(
            "SLoadoutAssortment",
            &format!("loadoutkits/pu_loadoutbundles/{name}"),
            json!({"Requirement": null, "GroupedLoadouts": groups}),
        )
    }

    fn group(name: &str, required: &[&str], loadouts: Value) -> Value {
        let req: Vec<Value> = required
            .iter()
            .map(|g| json!({"_RecordName_": format!("Tag.{g}"), "_RecordId_": g}))
            .collect();
        json!({"_Type_": "SGroupedLoadouts", "Group": name,
               "Requirement": {"_Type_": "SLoadoutRequirementTags", "RequiredTags": req, "BannedTags": []},
               "Loadouts": loadouts})
    }

    fn files() -> MemFiles {
        let item = |port: &str, name: &str, kids: Vec<XmlNode>| {
            node(
                "Item",
                &[("portName", port), ("itemName", name)],
                if kids.is_empty() {
                    vec![]
                } else {
                    vec![node("Items", &[], kids)]
                },
            )
        };
        let doc = |kids: Vec<XmlNode>| node("Loadout", &[], vec![node("Items", &[], kids)]);
        let mut f = MemFiles::default();
        // The ace pilot's outfit, as CryXmlB: suit with helmet and core inside, a mobiGlas.
        f.insert(
            r"Data\Scripts\Loadouts\Character\Human\Armor\NPC\Pirate\criminal_acepilot_01.xml",
            cryxml(&doc(vec![item(
                "Body_ItemPort",
                "",
                vec![
                    item(
                        "Armor_Undersuit",
                        "basl_flightsuit",
                        vec![
                            item("Armor_Helmet", "basl_helmet", vec![]),
                            item("Armor_Torso", "rsi_core", vec![]),
                        ],
                    ),
                    item("mobiglas_attach", "MobiGlas", vec![]),
                ],
            )])),
        );
        // A body file (cosmetic only) and a weapon file in XML text.
        f.insert(
            r"Data\Scripts\Loadouts\Character\Human\Body\NPC\Body_01.xml",
            cryxml(&doc(vec![item(
                "Body_ItemPort",
                "m_body_01",
                vec![item("Head_ItemPort", "PU_Protos_Head", vec![])],
            )])),
        );
        f.insert(
            r"Data\Scripts\Loadouts\Character\Human\Weapon\volt_smg.xml",
            br#"<?xml version="1.0"?>
<!-- weapon -->
<Loadout><Items><Item portName="Body_ItemPort" itemName="">
  <Items><Item portName="Armor_Torso" itemName=""><Items>
    <Item portName="wep_stocked_3" itemName="volt_smg" ownership="0"/>
  </Items></Item></Items></Item></Items></Loadout>"#
                .to_vec(),
        );
        f.insert(
            r"Data\Scripts\Loadouts\Character\Human\Weapon\ksar_smg.xml",
            br#"<Loadout><Items><Item portName="wep_stocked_3" itemName="ksar_smg"/></Items></Loadout>"#.to_vec(),
        );
        f
    }

    fn loc() -> Localization {
        let mut l = Localization::new();
        for (k, v) in [
            ("basl_flightsuit_name", "Ace Interceptor Racing Flight Suit"),
            ("basl_helmet_name", "Ace Interceptor Helmet"),
            ("rsi_core_name", "Venture Core Olive"),
            ("volt_smg_name", "Quartz Energy SMG"),
            ("ksar_smg_name", "Custodian SMG"),
            ("frag_name", "Frag Grenade"),
            ("medpen_name", "MedPen"),
            ("ai_core_name", "<= PLACEHOLDER =>"),
        ] {
            l.set(k, v);
        }
        l
    }

    fn tags(guids: &[&str]) -> Value {
        Value::Array(
            guids
                .iter()
                .map(|g| json!({"_RecordName_": format!("Tag.{g}"), "_RecordId_": g}))
                .collect(),
        )
    }

    /// An actor with record tags and geometry model tags.
    fn tagged(class: &str, loadout: Value, record: &[&str], geometry: &[&str]) -> GameRecord {
        let mut a = actor(class, loadout, None);
        a.value["tags"] = tags(record);
        if !geometry.is_empty() {
            let comps = a.value["Components"].as_array_mut().unwrap();
            comps.push(json!({"_Type_": "SGeometryResourceParams", "Geometry": {
                "Geometry": {"ModelTag": {"tag": format!("Tag.{}", geometry[0])}}}}));
        }
        a
    }

    fn req_tags(required: &[&str], banned: &[&str]) -> Value {
        json!({"_Type_": "SLoadoutRequirementTags", "RequiredTags": tags(required), "BannedTags": tags(banned)})
    }

    fn group_req(name: &str, req: Value, loadouts: Value) -> Value {
        json!({"_Type_": "SGroupedLoadouts", "Group": name, "Requirement": req, "Loadouts": loadouts})
    }

    fn records() -> RecordSet {
        let x = "Scripts/Loadouts/Character/Human/";
        let pilot_kit = kit(
            "criminal_acepilot",
            json!([
                group("Body", &[], json!([xml_ref(&format!("{x}Body/NPC/Body_01.xml"))])),
                group("Outfit", &[], json!([xml_ref(&format!("{x}Armor/NPC/Pirate/criminal_acepilot_01.xml"))])),
                group("PrimaryWeapon", &[], json!([xml_ref(&format!("{x}Weapon/volt_smg.xml")), xml_ref(&format!("{x}Weapon/ksar_smg.xml"))])),
                // Nested: one of two, the second a nested kit of its own (one of two again).
                group("Grenades", &[], json!([manual(json!([entry("grenade_attach_1", "frag")])), random(&["nested_meds"])])),
                // Gated by a tag the pilot lacks: left out.
                group("Female", &[FEMALE_GUID], json!([manual(json!([entry("wep_sidearm", "pistol")]))])),
                group("Missing", &[], json!([xml_ref(&format!("{x}Nope/gone.xml"))])),
            ]),
        );
        let nested = kit(
            "nested_meds",
            json!([group("Meds", &[], json!([
                manual(json!([entry("medPen_attach_1", "medpen")])),
                manual(json!([entry("medPen_attach_1", "medpen"), entry("Armor_Torso", "ai_core")]))
            ]))]),
        );
        // Two assortments with groups of one name: one pick over the pooled options (the
        // ksar listed twice weighs double); a gated helmet group with an open fallback; OR,
        // NOT and an unknown requirement type.
        let shotguns_a = kit(
            "shotguns_a",
            json!([
                group("PrimaryWeapon", &[], json!([manual(json!([entry("wep_stocked_3", "volt_smg")])), manual(json!([entry("wep_stocked_3", "ksar_smg")]))])),
                group_req("Helmet", req_tags(&[CQC_GUID], &[]), json!([manual(json!([entry("Armor_Helmet", "rsi_helmet")]))])),
                group("Helmet", &[], json!([manual(json!([entry("Armor_Helmet", "basl_helmet")]))])),
            ]),
        );
        let shotguns_b = kit(
            "shotguns_b",
            json!([
                group("PrimaryWeapon", &[], json!([manual(json!([entry("wep_stocked_3", "ksar_smg")]))])),
                group_req("Sidearm", json!({"_Type_": "SLoadoutRequirementOR", "Requirements": [
                    req_tags(&[FEMALE_GUID], &[]), req_tags(&[CQC_GUID], &[])]}),
                    json!([manual(json!([entry("wep_sidearm", "pistol")]))])),
                group_req("Grenades", json!({"_Type_": "SLoadoutRequirementNOT", "RequirementToInvert": req_tags(&[CQC_GUID], &[])}),
                    json!([manual(json!([entry("grenade_attach_1", "frag")]))])),
                group_req("Medical", json!({"_Type_": "SLoadoutRequirementSomethingNew"}),
                    json!([manual(json!([entry("medPen_attach_1", "medpen")]))])),
            ]),
        );
        let outfit = json!({"_Type_": "SItemPortLoadoutOutfitParams", "WearRange": {"minimum": 0.1, "maximum": 0.3},
            "InventoryItems": [{"_Type_": "SLoadoutInventoryItem", "entityClassName": "", "entityClassReference": format!("{ROOT}entities/scitem/characters/medpen.json"), "amount": 1}],
            "BaseLoadout": xml_ref(&format!("{x}Weapon/ksar_smg.xml")),
            "Outfits": [
                {"_Type_": "SItemPortLoadoutOutfitTagParams", "OutfitNameTags": [], "Loadouts": [manual(json!([entry("Armor_Helmet", "basl_helmet")]))]},
                {"_Type_": "SItemPortLoadoutOutfitTagParams", "OutfitNameTags": [{"_RecordName_": format!("Tag.{MISSION_GUID}")}],
                 "Loadouts": [manual(json!([entry("Armor_Helmet", "rsi_helmet"), entry("Armor_Torso", "unloot_core")]))]}]});
        RecordSet::new(vec![
            tag_db(),
            item("basl_flightsuit", "Char_Armor_Undersuit", "UNDEFINED", false),
            item("basl_helmet", "Char_Armor_Helmet", "Light", false),
            item("rsi_helmet", "Char_Armor_Helmet", "Light", false),
            item("rsi_core", "Char_Armor_Torso", "Light", false),
            item("unloot_core", "Char_Armor_Torso", "Light", true),
            item("ai_core", "Char_Armor_Torso", "Light", false),
            item("volt_smg", "WeaponPersonal", "Medium", false),
            item("ksar_smg", "WeaponPersonal", "Medium", false),
            item("frag", "WeaponPersonal", "Grenade", false),
            item("pistol", "WeaponPersonal", "Small", false),
            item("medpen", "FPS_Consumable", "Medical", false),
            item("m_body_01", "Char_Body", "Male", false),
            item("PU_Protos_Head", "Char_Head", "UNDEFINED", false),
            pilot_kit,
            nested,
            shotguns_a,
            shotguns_b,
            actor(
                "PU_Human_Enemy_GroundCombat_NPC_GenericCriminal_AcePilot",
                random(&["criminal_acepilot"]),
                Some("loottable_actor_acepilot"),
            ),
            actor("NPC_Archetypes-Female-Human-Pirate-Outlaw_Guard_01", outfit, None),
            // Body only: no gear, not listed.
            actor(
                "Civilian_Bare",
                manual(json!([entry("Body_ItemPort", "m_body_01")])),
                None,
            ),
            // The same kits, without and with the CQC tag (an elite child tag, on the geometry).
            tagged("PU_Shotgunner", random(&["shotguns_a", "shotguns_b"]), &[], &[]),
            tagged(
                "PU_Shotgunner_Elite",
                random(&["shotguns_a", "shotguns_b"]),
                &[],
                &[CQC_ELITE_GUID],
            ),
            // An outfit over a base loadout: the outfit's helmet replaces the base's.
            actor(
                "PU_Override",
                json!({"_Type_": "SItemPortLoadoutOutfitParams", "InventoryItems": [],
                    "BaseLoadout": manual(json!([entry("Armor_Helmet", "basl_helmet"), entry("wep_sidearm", "pistol")])),
                    "Outfits": [{"_Type_": "SItemPortLoadoutOutfitTagParams", "OutfitNameTags": [],
                        "Loadouts": [manual(json!([entry("Armor_Helmet", "rsi_helmet")]))]}]}),
                None,
            ),
        ])
    }

    fn run() -> (Map<String, Value>, Vec<String>) {
        let set = records();
        let l = loc();
        let f = files();
        let ctx = ParseContext::new(&set, &l, &f);
        let body = DOMAIN.parse(&ctx).unwrap();
        (body, ctx.take_warnings())
    }

    fn data() -> NpcLoadouts {
        serde_json::from_value(Value::Object(run().0)).unwrap()
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    fn actor_of<'a>(d: &'a NpcLoadouts, class: &str) -> &'a NpcActor {
        d.actors.iter().find(|a| a.class == class).unwrap()
    }

    fn item_of<'a>(a: &'a NpcActor, item: &str) -> Option<&'a GearItem> {
        a.items.iter().find(|i| i.item == item)
    }

    #[test]
    fn names_and_words() {
        assert_eq!(
            readable_class("PU_Human_Enemy_GroundCombat_NPC_GenericCriminal_AcePilot"),
            "Generic criminal ace pilot"
        );
        assert_eq!(
            readable_class("NPC_Archetypes-Male-Human-Civilians-Levski-Civilian_Levski_01"),
            "Male civilians levski 01"
        );
        assert_eq!(readable_class("PU_Human_Enemy_GroundCombat_NPC_ASD_Grunt"), "ASD grunt");
        assert_eq!(
            readable_class("NPC_Archetypes-Male-Human-Civilians-Area18-Civilian_Area18_16"),
            "Male civilians area 18 16"
        );
        assert_eq!(split_case("ASDGrunt"), ["ASD", "Grunt"]);
        // A known item goes by its type: a ship gun in a hand port is no gear.
        let gun = ItemInfo {
            ty: "WeaponGun".into(),
            ..Default::default()
        };
        assert_eq!(slot_of("weapon_attach_hand_right", Some(&gun)), None);
        let bar = ItemInfo {
            ty: "Misc".into(),
            ..Default::default()
        };
        assert_eq!(slot_of("", Some(&bar)), Some("Carried"));
        assert_eq!(slot_of("magic_pocket", Some(&bar)), None);
        assert_eq!(
            loadout_key(r"Data\Scripts\Loadouts\X\Y.xml"),
            "scripts/loadouts/x/y.xml"
        );
        assert_eq!(slot_of("Armor_Helmet", None), Some("Helmet"));
        assert_eq!(slot_of("magazine_attach_3", None), Some("Magazines"));
        assert_eq!(slot_of("hardpoint_headlight", None), None);
    }

    #[test]
    fn text_xml_reads_elements_and_attributes() {
        let n = parse_text_xml(r#"<?xml version="1.0"?><!-- c --><A x="1" y='a &amp; b'><B z="2"/><C></C></A>"#)
            .unwrap();
        assert_eq!(n.tag, "A");
        assert_eq!(n.attr("y"), Some("a & b"));
        assert_eq!(n.children.len(), 2);
        assert_eq!(n.children[0].attr("z"), Some("2"));
        assert!(parse_text_xml("<A><B></A>").is_err() || parse_text_xml("<A>").is_err());
        assert!(parse_loadout_file(&[0xff, 0xfe]).is_err());
    }

    #[test]
    fn random_kits_give_fixed_gear_and_even_picks() {
        let d = data();
        let pilot = actor_of(&d, "PU_Human_Enemy_GroundCombat_NPC_GenericCriminal_AcePilot");
        assert_eq!(pilot.name, "Generic criminal ace pilot");
        assert_eq!(pilot.faction.as_deref(), Some("genericcriminal"));
        assert_eq!(pilot.loot_table.as_deref(), Some("loottable_actor_acepilot"));
        assert!(pilot.lootable);
        assert_eq!(pilot.wear, Some(Wear { min: 0.0, max: 0.5 }));
        let by = |c: &str| item_of(pilot, c);
        // One-option outfit: always there, with the game's names; no mobiGlas, no body.
        let suit = by("basl_flightsuit").unwrap();
        assert!(close(suit.chance, 1.0));
        assert_eq!(suit.slot, "Undersuit");
        assert_eq!(suit.name.as_deref(), Some("Ace Interceptor Racing Flight Suit"));
        assert!(by("MobiGlas").is_none() && by("m_body_01").is_none());
        // Two weapons: one each.
        assert!(close(by("volt_smg").unwrap().chance, 0.5));
        assert_eq!(by("ksar_smg").unwrap().slot, "Primary weapon");
        // A tag the pilot lacks: its group is left out.
        assert!(by("pistol").is_none());
        let slots: Vec<&str> = pilot.items.iter().map(|i| i.slot.as_str()).collect();
        assert_eq!(&slots[..3], ["Undersuit", "Helmet", "Torso"]);
        assert!(pilot.items.iter().all(|i| i.requires.is_empty()));
    }

    #[test]
    fn nesting_compounds_the_chance() {
        let d = data();
        let pilot = actor_of(&d, "PU_Human_Enemy_GroundCombat_NPC_GenericCriminal_AcePilot");
        let by = |c: &str| item_of(pilot, c).unwrap();
        // Grenades: frag 1/2; the nested kit 1/2, whose meds group gives the medpen in both
        // options (1/2 * (1/2 + 1/2)) and the placeholder-named core in one (1/2 * 1/2).
        assert!(close(by("frag").chance, 0.5));
        assert!(close(by("medpen").chance, 0.5));
        let core = by("ai_core");
        assert!(close(core.chance, 0.25));
        assert_eq!(core.name, None, "a placeholder name is no name");
    }

    #[test]
    fn an_item_twice_in_one_option_is_there_as_often_as_the_option() {
        // One of two weapons; the rifle comes with two magazines of one class, and a nested
        // pick that holds that magazine again in both of its options.
        let set = RecordSet::new(vec![
            tag_db(),
            item("rifle", "WeaponPersonal", "Medium", false),
            item("rifle_mag", "WeaponAttachment", "Magazine", false),
            item("smg", "WeaponPersonal", "Medium", false),
            item("frag", "WeaponPersonal", "Grenade", false),
            kit(
                "two_mags",
                json!([group("PrimaryWeapon", &[], json!([
                    json!({"_Type_": "SItemPortLoadoutManualParams", "InventoryItems": [], "entries": [
                        entry("wep_stocked_2", "rifle"),
                        entry("magazine_attach_1", "rifle_mag"),
                        entry("magazine_attach_2", "rifle_mag"),
                        {"_Type_": "SItemPortLoadoutEntryParams", "itemPortName": "", "entityClassName": "",
                         "entityClassReference": null, "loadout": random(&["spare_mags"])}]}),
                    manual(json!([entry("wep_stocked_2", "smg")])),
                ]))]),
            ),
            kit(
                "spare_mags",
                json!([group("Spare", &[], json!([
                    manual(json!([entry("magazine_attach_3", "rifle_mag")])),
                    manual(json!([entry("magazine_attach_3", "rifle_mag"), entry("grenade_attach_1", "frag")])),
                ]))]),
            ),
            actor("PU_TwoMags", random(&["two_mags"]), None),
        ]);
        let (l, f) = (loc(), files());
        let body = DOMAIN.parse(&ParseContext::new(&set, &l, &f)).unwrap();
        let d: NpcLoadouts = serde_json::from_value(Value::Object(body)).unwrap();
        let a = actor_of(&d, "PU_TwoMags");
        let by = |c: &str| item_of(a, c).unwrap().chance;
        assert_eq!(by("rifle"), 0.5);
        assert_eq!(by("smg"), 0.5);
        assert_eq!(by("rifle_mag"), 0.5, "never likelier than the weapon it comes with");
        assert_eq!(by("frag"), 0.25);
    }

    #[test]
    fn same_named_groups_are_one_pick_over_pooled_options() {
        let d = data();
        let plain = actor_of(&d, "PU_Shotgunner");
        // volt and ksar from one kit, ksar again from the other: one pick of three.
        let volt = item_of(plain, "volt_smg").unwrap();
        let ksar = item_of(plain, "ksar_smg").unwrap();
        assert!(close(volt.chance, 1.0 / 3.0) && close(ksar.chance, 2.0 / 3.0));
        let weapons: f64 = plain
            .items
            .iter()
            .filter(|i| i.slot == "Primary weapon")
            .map(|i| i.chance)
            .sum();
        assert!(close(weapons, 1.0), "one pick: the slot sums to 1");
    }

    #[test]
    fn requirements_read_the_actors_tags() {
        let d = data();
        let plain = actor_of(&d, "PU_Shotgunner");
        let elite = actor_of(&d, "PU_Shotgunner_Elite");
        // Gated helmet with an open fallback: no CQC tag, the fallback; the elite's child
        // tag (on its geometry) meets the gate, so only the gated one.
        assert!(close(item_of(plain, "basl_helmet").unwrap().chance, 1.0));
        assert!(item_of(plain, "rsi_helmet").is_none());
        assert!(close(item_of(elite, "rsi_helmet").unwrap().chance, 1.0));
        assert!(item_of(elite, "basl_helmet").is_none());
        // OR: met by CQC for the elite only. NOT CQC: the plain one only.
        assert!(item_of(plain, "pistol").is_none());
        assert!(close(item_of(elite, "pistol").unwrap().chance, 1.0));
        assert!(close(item_of(plain, "frag").unwrap().chance, 1.0));
        assert!(item_of(elite, "frag").is_none());
        // An unknown requirement type: kept, with its label.
        let med = item_of(plain, "medpen").unwrap();
        assert_eq!(med.requires, [NOT_READ]);
        assert!(holds(&["A/B/C".to_string()], "A/B") && !holds(&["A/BC".to_string()], "A/B"));
    }

    #[test]
    fn named_outfits_are_the_missions_pick_and_unlootable_items_are_dropped() {
        let d = data();
        let guard = actor_of(&d, "NPC_Archetypes-Female-Human-Pirate-Outlaw_Guard_01");
        assert_eq!(guard.name, "Female pirate outlaw guard 01");
        assert_eq!(guard.wear, Some(Wear { min: 0.1, max: 0.3 }));
        let by = |c: &str| item_of(guard, c);
        // The base loadout and the inventory medpen (by reference) are always there.
        assert!(close(by("ksar_smg").unwrap().chance, 1.0));
        let med = by("medpen").unwrap();
        assert_eq!((med.slot.as_str(), med.chance), ("Medical", 1.0));
        // Two outfits, one named: the mission picks, each whole under its own condition.
        let basl = by("basl_helmet").unwrap();
        assert!(close(basl.chance, 1.0));
        assert_eq!(basl.requires, ["default outfit (no mission outfit picked)"]);
        let rsi = by("rsi_helmet").unwrap();
        assert!(close(rsi.chance, 1.0));
        assert_eq!(rsi.requires, ["outfit picked by the mission (Mission/Outfit/Guard)"]);
        assert!(by("unloot_core").is_none());
    }

    #[test]
    fn an_outfit_fills_ports_over_its_base() {
        let d = data();
        let o = actor_of(&d, "PU_Override");
        assert!(item_of(o, "basl_helmet").is_none());
        assert!(close(item_of(o, "rsi_helmet").unwrap().chance, 1.0));
        assert!(close(item_of(o, "pistol").unwrap().chance, 1.0));
    }

    #[test]
    fn condition_sets_never_contradict() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(condition(&s(&["B", "A", "A"])).as_deref(), Some("A and B"));
        assert_eq!(condition(&s(&["X", "not X"])), None);
        assert_eq!(condition(&[]).as_deref(), Some(""));
        // Mutually exclusive conditions: the likeliest, the sets as alternatives.
        let e = |item: &str, chance: f64, req: &[&str]| GearEntry {
            port: "Armor_Helmet".into(),
            slot: "Helmet".into(),
            item: item.into(),
            name: None,
            chance,
            requires: s(req),
        };
        let g = GearGroup {
            group: "Outfit".into(),
            options: vec![vec![e("h", 1.0, &["A"])], vec![e("h", 0.5, &["B"]), e("h", 0.5, &["B", "not B"])]],
        };
        let items = item_chances(&[], &[g]);
        assert_eq!(items.len(), 1);
        assert!(close(items[0].chance, 1.0));
        assert_eq!(items[0].requires, ["A", "B"]);
    }

    #[test]
    fn indexes_summary_and_warnings() {
        let (body, warnings) = run();
        let d: NpcLoadouts = serde_json::from_value(Value::Object(body.clone())).unwrap();
        // The bare civilian has no gear: not listed.
        assert_eq!(d.actors.len(), 5);
        assert_eq!(d.actors[0].class, "NPC_Archetypes-Female-Human-Pirate-Outlaw_Guard_01");
        assert_eq!(
            d.by_loot_table["loottable_actor_acepilot"],
            ["PU_Human_Enemy_GroundCombat_NPC_GenericCriminal_AcePilot"]
        );
        // A lean file: no per-group detail.
        assert!(body.get("groups").is_none() && body.get("byItem").is_none());
        assert!(body["actors"][0].get("fixed").is_none());
        // `chance` keeps the codex name, so the note is what marks it an estimate.
        assert_eq!(body["_assumptions"], serde_json::json!([PICK_ASSUMPTIONS]));
        let s = &body["summary"];
        assert_eq!(s["actors"], 6);
        assert_eq!(s["withGear"], 5);
        assert_eq!(s["missingLoadoutFiles"], 1);
        assert_eq!(s["unlootableDropped"], 1);
        assert_eq!(s["kits"], 4);
        // The pilot's Female group; the plain shotgunner's gated helmet and OR sidearm; the
        // elite's NOT grenades.
        assert_eq!(s["groupsFailed"], 4);
        assert_eq!(s["requirementsNotRead"], 2);
        assert_eq!(s["depthCut"], 0);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains("1 loadout files") && warnings[0].contains("gone.xml"));
    }

    #[test]
    fn prune_keeps_what_parse_reads() {
        let mut a = tagged("PU_X", random(&["k"]), &[FEMALE_GUID], &[CQC_GUID]);
        a.value["Components"]
            .as_array_mut()
            .unwrap()
            .push(json!({"_Type_": "LootGenerationComponentParams", "lootConfig": {"lootTable": "file://./x/t.json"}}));
        DOMAIN.prune(&mut a);
        let comps: Vec<&str> = array(a.value.get("Components"))
            .iter()
            .filter_map(|c| str_(c.get("_Type_")))
            .collect();
        assert_eq!(
            comps,
            [
                "SEntityComponentDefaultLoadoutParams",
                "SEntityLootableContainerParams",
                "LootGenerationComponentParams"
            ]
        );
        assert_eq!(array(a.value.get("StaticEntityClassData")).len(), 1);
        assert_eq!(faction_of(&a).as_deref(), Some("genericcriminal"));
        // The record's tags stay; the geometry's model tag is kept apart.
        assert_eq!(array(a.value.get("tags")).len(), 1);
        assert_eq!(a.value[GEOMETRY_TAGS], json!([format!("Tag.{CQC_GUID}")]));
        let mut i = item("x", "WeaponPersonal", "Small", true);
        DOMAIN.prune(&mut i);
        assert!(i.value.get("tags").is_some());
    }
}
