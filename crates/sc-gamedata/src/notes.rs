//! Port of `src/notes.ts`: player-facing patch notes between two builds.
//!
//! The same comparison as `diff` and `changes`, reduced to a short document: new ships
//! and gear, buffs and nerfs, crafting and mission rewards, loot sources and renames,
//! with everything else counted per dataset. Variants collapse (weapon family, base
//! class, armour set); templates, props and Vanduul (NPC-only) records are only counted;
//! numeric moves under `min_change` percent and fields only one side carries are hidden
//! and counted. Stat moves of tracked categories come from [`crate::changes`].
//!
//! [`notes`] reads two builds into a [`NotesDoc`]; [`notes_json`] is `notes --json`, and
//! [`render_notes`] the Markdown.

use crate::changes::{CATEGORIES, Direction, ItemStatus, PatchChanges, StatChange};
use crate::collate::{locale_compare, locale_compare_numeric};
use crate::diff::{
    Body, BuildSource, FieldChange, OrderedMap, OrderedSet, Result, align_redaction, cmp_utf16,
    collapse_ws, dataset_file, diff_body_fields, diff_collection, diff_entry, diff_entry_bodies,
    diffable_collections, entries_of, is_js_space, js_entries, js_strict_eq, js_string, js_trim,
    js_truthy, json_text, list_dataset_files, load_snapshot, nullish, sort_utf16, string_of,
    utf16_from, utf16_len, utf16_prefix,
};
use crate::js::number;
use crate::output::{js_number, to_js_json, to_js_json_compact};
use crate::search_index::weapon_family;
use serde::Serialize;
use serde::ser::{SerializeMap, Serializer};
use serde_json::{Map, Value};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

/// `NotesOptions`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NotesOptions {
    /// Hide numeric changes smaller than this percent of the old value (default 1).
    pub min_change: f64,
}

impl Default for NotesOptions {
    fn default() -> Self {
        Self { min_change: 1.0 }
    }
}

/// One stat or field that moved on a line's records. `before`/`after` are scalars;
/// `Some(Value::Null)` is a JSON null, `None` an absent value.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NoteChange {
    pub stat: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub percent: Option<f64>,
    pub direction: Direction,
}

/// One bullet: a record, or a group of variants that read as one thing.
#[derive(Debug, Clone, PartialEq)]
pub struct NoteLine {
    pub label: String,
    pub status: ItemStatus,
    pub category: Option<String>,
    pub records: Vec<String>,
    pub variants: Option<Vec<String>>,
    pub changes: Option<Vec<NoteChange>>,
    pub detail: Option<String>,
    /// The TypeScript sets `detail` before `changes` on changed-blueprint lines, which
    /// orders the two keys that way in the JSON.
    pub(crate) detail_first: bool,
}

impl NoteLine {
    fn new(
        label: String,
        status: ItemStatus,
        category: Option<String>,
        records: Vec<String>,
    ) -> Self {
        Self {
            label,
            status,
            category,
            records,
            variants: None,
            changes: None,
            detail: None,
            detail_first: false,
        }
    }
}

impl Serialize for NoteLine {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        let mut m = s.serialize_map(None)?;
        m.serialize_entry("label", &self.label)?;
        m.serialize_entry("status", &self.status)?;
        if let Some(c) = &self.category {
            m.serialize_entry("category", c)?;
        }
        m.serialize_entry("records", &self.records)?;
        if let Some(v) = &self.variants {
            m.serialize_entry("variants", v)?;
        }
        if self.detail_first
            && let Some(d) = &self.detail
        {
            m.serialize_entry("detail", d)?;
        }
        if let Some(c) = &self.changes {
            m.serialize_entry("changes", c)?;
        }
        if !self.detail_first
            && let Some(d) = &self.detail
        {
            m.serialize_entry("detail", d)?;
        }
        m.end()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SectionId {
    NewShips,
    ShipBalance,
    NewShipItems,
    NewPersonalWeapons,
    PersonalBalance,
    Armour,
    Crafting,
    Missions,
    Loot,
    Renames,
    Other,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NoteSection {
    pub id: SectionId,
    pub title: String,
    pub lines: Vec<NoteLine>,
    /// One-line remarks rendered in italics under the section.
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Fallback {
    pub source: String,
    pub files: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NotesSide {
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build: Option<String>,
    /// Datasets this side lacked and read from elsewhere (a snapshot's published tag).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback: Option<Fallback>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Hidden {
    pub minor: usize,
    pub newly_tracked: usize,
    pub untracked: usize,
}

/// `NotesDoc`: the machine form of a set of patch notes.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotesDoc {
    pub from: NotesSide,
    pub to: NotesSide,
    pub min_change: f64,
    pub sections: Vec<NoteSection>,
    pub hidden: Hidden,
}

/// Every dataset of a build that notes read, by file name, in load order.
#[derive(Debug, Clone, Default)]
pub struct BuildData {
    files: Vec<(String, Body)>,
}

impl BuildData {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn insert(&mut self, file: impl Into<String>, body: Body) {
        let file = file.into();
        match self.files.iter_mut().find(|(f, _)| *f == file) {
            Some(slot) => slot.1 = body,
            None => self.files.push((file, body)),
        }
    }
    pub fn get(&self, file: &str) -> Option<&Body> {
        self.files.iter().find(|(f, _)| f == file).map(|(_, b)| b)
    }
    pub fn files(&self) -> impl Iterator<Item = &str> {
        self.files.iter().map(|(f, _)| f.as_str())
    }
}

mod files {
    pub const WEAPONS: &str = "game-weapons.json";
    pub const FPS: &str = "game-fps-weapons.json";
    pub const SHIPS: &str = "game-ships.json";
    pub const COMPONENTS: &str = "game-ship-components.json";
    pub const ARMOR: &str = "game-armor.json";
    pub const BLUEPRINTS: &str = "game-blueprints.json";
    pub const MISSIONS: &str = "game-missions.json";
    pub const WIKELO: &str = "game-wikelo-trades.json";
    pub const TAGS: &str = "game-item-tags.json";
    pub const SOURCES: &str = "game-item-sources.json";
    pub const STRINGS: &str = "game-strings-english.json";
}
use files as F;

/// `ITEMISED`: collections whose changes a section itemises; `None` is "all".
fn itemised(file: &str) -> Option<Option<&'static [&'static str]>> {
    match file {
        F::WEAPONS | F::FPS | F::SHIPS | F::ARMOR | F::MISSIONS | F::TAGS | F::SOURCES => {
            Some(None)
        }
        F::BLUEPRINTS => Some(Some(&["blueprints", "rewardPools"])),
        F::WIKELO => Some(Some(&["trades"])),
        F::COMPONENTS => Some(Some(&[
            "weapons",
            "missiles",
            "shields",
            "powerPlants",
            "coolers",
            "quantumDrives",
            "radars",
            "armor",
            "thrusters",
        ])),
        _ => None,
    }
}

static EMPTY: LazyLock<Body> = LazyLock::new(Map::new);

// ---------------------------------------------------------------------------
// Naming and grouping
// ---------------------------------------------------------------------------

/// `shortRecord`: `EntityClassDefinition.ksar_lmg_energy_01` -> `ksar_lmg_energy_01`.
pub fn short_record(record: &str) -> &str {
    let n = record.bytes().take_while(u8::is_ascii_alphabetic).count();
    if n > 0 && record.as_bytes().get(n) == Some(&b'.') {
        &record[n + 1..]
    } else {
        record
    }
}

/// `/^VNCL_/i` on the short record: Vanduul, NPC-only.
fn npc_only(record: &str) -> bool {
    short_record(record)
        .as_bytes()
        .get(..5)
        .is_some_and(|p| p.eq_ignore_ascii_case(b"VNCL_"))
}

const VARIANT_WORDS: &[&str] = &[
    "tint",
    "store",
    "camo",
    "lr",
    "mr",
    "sf",
    "lumi",
    "collector",
    "iae",
    "arctic",
    "urban",
    "desert",
    "tundra",
    "black",
    "white",
    "red",
    "blue",
    "green",
    "tan",
    "grey",
    "gray",
    "gold",
    "purple",
    "orange",
    "yellow",
    "pink",
    "digital",
    "shin",
    "imp",
    "xenothreat",
    "showdown",
    "luminalia",
    "pirate",
    "prop",
    "civilian",
];

/// `baseClass`: the class name lower-cased, variant suffixes (`_tint01`, `_store01`,
/// `_prop`, ...) stripped repeatedly.
pub fn base_class(record: &str) -> String {
    let mut name = short_record(record).to_lowercase();
    loop {
        let t = name.trim_end_matches(|c: char| c.is_ascii_digit());
        let cut = VARIANT_WORDS.iter().find_map(|w| {
            t.len().checked_sub(w.len() + 1).filter(|&i| {
                t.as_bytes()[i] == b'_' && t.as_bytes()[i + 1..].eq_ignore_ascii_case(w.as_bytes())
            })
        });
        match cut {
            Some(i) => name.truncate(i),
            None => return name,
        }
    }
}

const SLOTS: &[&str] = &["arms", "core", "helmet", "legs", "backpack"];

/// `SLOT_TOKEN` (`/_(arms|core|helmet|legs|backpack)(?=_|$)/i`): byte range of the first.
fn find_slot_token(s: &str) -> Option<(usize, usize)> {
    let b = s.as_bytes();
    for (i, _) in s.match_indices('_') {
        for w in SLOTS {
            let end = i + 1 + w.len();
            if b.len() >= end
                && b[i + 1..end].eq_ignore_ascii_case(w.as_bytes())
                && (end == b.len() || b[end] == b'_')
            {
                return Some((i, end));
            }
        }
    }
    None
}

/// `armourSetKey`: `kap_combat_light_arms_02_03_01` -> `kap_combat_light_02_03_01`.
pub fn armour_set_key(record: &str) -> String {
    let s = short_record(record).to_lowercase();
    match find_slot_token(&s) {
        Some((a, b)) => format!("{}{}", &s[..a], &s[b..]),
        None => s,
    }
}

fn slot_of(record: &str) -> Option<String> {
    let s = short_record(record);
    find_slot_token(s).map(|(a, b)| s[a + 1..b].to_lowercase())
}

/// `SLOT_WORD` (`/\s+(Arms|Core|Helmet|Legs|Backpack)\b/`): byte range of the first.
fn find_slot_word(s: &str) -> Option<(usize, usize)> {
    const WORDS: &[&str] = &["Arms", "Core", "Helmet", "Legs", "Backpack"];
    let mut prev_space = false;
    for (i, c) in s.char_indices() {
        let space = is_js_space(c);
        if space && !prev_space {
            let j = i + s[i..]
                .find(|c: char| !is_js_space(c))
                .unwrap_or(s.len() - i);
            let rest = &s[j..];
            for w in WORDS {
                if rest.starts_with(w) {
                    let k = j + w.len();
                    let word_char = s
                        .as_bytes()
                        .get(k)
                        .is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_');
                    if !word_char {
                        return Some((i, k));
                    }
                }
            }
        }
        prev_space = space;
    }
    None
}

fn remove_slot_word(name: &str) -> String {
    match find_slot_word(name) {
        Some((a, b)) => format!("{}{}", &name[..a], &name[b..]),
        None => name.to_string(),
    }
}

/// `stripSlot`.
fn strip_slot(name: &str) -> String {
    js_trim(&collapse_ws(&remove_slot_word(name))).to_string()
}

/// `isRealName`: a display name worth printing (not empty, not a placeholder or a
/// localisation key, not the class name).
fn real_name<'a>(name: Option<&'a Value>, record: Option<&str>) -> Option<&'a str> {
    let Some(Value::String(name)) = name else {
        return None;
    };
    if js_trim(name).is_empty()
        || name.to_ascii_lowercase().contains("placeholder")
        || name.starts_with(['@', '~'])
    {
        return None;
    }
    if let Some(r) = record.filter(|r| !r.is_empty())
        && name.to_lowercase() == short_record(r).to_lowercase()
    {
        return None;
    }
    Some(name)
}

fn is_real_name(name: Option<&Value>, record: Option<&str>) -> bool {
    real_name(name, record).is_some()
}

fn is_real_str(name: &str) -> bool {
    is_real_name(Some(&Value::String(name.to_string())), None)
}

/// `splitVariant`: `Verdict "Derivation" LMG` -> (`Verdict LMG`, `Derivation`).
pub fn split_variant(name: &str) -> (String, Option<String>) {
    let chars: Vec<(usize, char)> = name.char_indices().collect();
    let n = chars.len();
    let at = |i: usize| if i < n { chars[i].0 } else { name.len() };
    for p in 0..n {
        let mut j = p;
        while j < n && is_js_space(chars[j].1) {
            j += 1;
        }
        if j >= n || !matches!(chars[j].1, '"' | '“') {
            continue;
        }
        let k = j + 1;
        let mut m = k;
        while m < n && !matches!(chars[m].1, '"' | '”') {
            m += 1;
        }
        if m == k || m >= n {
            continue;
        }
        let mut e = m + 1;
        while e < n && is_js_space(chars[e].1) {
            e += 1;
        }
        let base = format!("{} {}", &name[..at(p)], &name[at(e)..]);
        return (
            js_trim(&collapse_ws(&base)).to_string(),
            Some(name[at(k)..at(m)].to_string()),
        );
    }
    (name.to_string(), None)
}

fn split_base(name: &str) -> String {
    split_variant(name).0
}

/// `mostCommon`: most frequent, then shortest, then first by `localeCompare`.
fn most_common(values: &[String]) -> String {
    let mut counts: OrderedMap<usize> = OrderedMap::new();
    for v in values {
        *counts.entry_or(v, || 0) += 1;
    }
    let mut entries: Vec<(&String, usize)> = counts.iter().map(|(k, n)| (k, *n)).collect();
    entries.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| utf16_len(a.0).cmp(&utf16_len(b.0)))
            .then_with(|| locale_compare(a.0, b.0))
    });
    entries.first().map(|e| e.0.clone()).unwrap_or_default()
}

#[derive(Debug, Clone)]
struct Member {
    record: String,
    name: Option<String>,
    template: bool,
    family: Option<String>,
}

/// `_prop` as a word of the class name: `/_prop(_|$)/i`.
fn prop_word(s: &str) -> bool {
    let l = s.to_ascii_lowercase();
    l.match_indices("_prop").any(|(i, _)| {
        let end = i + 5;
        end == l.len() || l.as_bytes()[end] == b'_'
    })
}

/// `isTemplate`.
fn is_template(r: &Body) -> bool {
    r.get("isTemplate") == Some(&Value::Bool(true))
        || prop_word(&string_of(r, &["recordName", "className"]))
}

fn record_id(r: &Body) -> String {
    string_of(r, &["recordName", "className"])
}

fn member_of(r: &Body, family: Option<String>) -> Member {
    let record = record_id(r);
    let name = real_name(r.get("name"), Some(&record)).map(str::to_string);
    Member {
        record,
        name,
        template: is_template(r),
        family,
    }
}

fn sorted(mut v: Vec<String>) -> Vec<String> {
    sort_utf16(&mut v);
    v
}

fn dedup(v: impl IntoIterator<Item = String>) -> Vec<String> {
    v.into_iter().collect::<OrderedSet>().to_vec()
}

/// `groupMembers`: added or removed records as lines; templates are only counted.
fn group_members(
    members: &[Member],
    status: ItemStatus,
    key_of: &dyn Fn(&Member) -> String,
    category: Option<&str>,
) -> (Vec<NoteLine>, usize) {
    let mut folded = 0;
    let mut groups: OrderedMap<Vec<&Member>> = OrderedMap::new();
    for m in members {
        if m.template {
            folded += 1;
            continue;
        }
        groups.entry_or(&key_of(m), Vec::new).push(m);
    }
    let mut lines = Vec::new();
    for group in groups.values() {
        let named: Vec<&&Member> = group.iter().filter(|m| m.name.is_some()).collect();
        let mut variants = Vec::new();
        let label = if named.is_empty() {
            let first = group[0];
            match &first.family {
                Some(f) => format!("{f} ({})", short_record(&first.record)),
                None => short_record(&first.record).to_string(),
            }
        } else {
            let names: Vec<&str> = named.iter().filter_map(|m| m.name.as_deref()).collect();
            let label = most_common(&names.iter().map(|n| split_base(n)).collect::<Vec<_>>());
            if names.len() == 1 {
                names[0].to_string()
            } else {
                variants = dedup(
                    names
                        .iter()
                        .filter(|n| **n != label)
                        .map(|n| split_variant(n).1.unwrap_or_else(|| n.to_string())),
                );
                label
            }
        };
        let records = sorted(
            group
                .iter()
                .map(|m| short_record(&m.record).to_string())
                .collect(),
        );
        let mut line = NoteLine::new(label, status, category.map(str::to_string), records);
        if !variants.is_empty() {
            line.variants = Some(sorted(variants));
        }
        lines.push(line);
    }
    lines.sort_by(|a, b| locale_compare(&a.label, &b.label));
    (lines, folded)
}

// ---------------------------------------------------------------------------
// Field-level changes
// ---------------------------------------------------------------------------

/// The last dotted segment of `field` is one of `words`, case-insensitively.
fn last_segment_is(field: &str, words: &[&str]) -> bool {
    let last = field.rsplit('.').next().unwrap_or(field);
    words.iter().any(|w| last.eq_ignore_ascii_case(w))
}

/// `BOOKKEEPING`: ids, build stamps, ordering, string-table keys.
fn bookkeeping(field: &str) -> bool {
    last_segment_is(
        field,
        &[
            "_build",
            "_extracted",
            "_source",
            "id",
            "guid",
            "uuid",
            "recordName",
            "className",
            "reference",
            "ref",
            "order",
            "index",
            "sortOrder",
            "sort",
            "name",
            "displayName",
            "nameKey",
            "descriptionKey",
            "titleKey",
            "giverKey",
            "nameStatus",
        ],
    )
}

/// `PROSE`: flavour text fields.
fn prose(field: &str) -> bool {
    last_segment_is(field, &["description", "desc", "flavorText", "lore"])
}

#[derive(Debug, Default, Clone, Copy)]
struct Counters {
    minor: usize,
    newly_tracked: usize,
    untracked: usize,
}

fn round2(x: f64) -> f64 {
    crate::js::round_to(x, 100.0)
}

enum Significance {
    NoPercent,
    Hidden,
    Percent(f64),
}

/// `significant`.
fn significant(before: f64, after: f64, min_change: f64, counters: &mut Counters) -> Significance {
    if before == 0.0 {
        return Significance::NoPercent;
    }
    let pct = round2(((after - before) / before.abs()) * 100.0);
    if pct.abs() < min_change {
        counters.minor += 1;
        return Significance::Hidden;
    }
    Significance::Percent(pct)
}

fn clip(v: &Value) -> Value {
    match v {
        Value::String(s) if utf16_len(s) > 60 => Value::String(format!("{}…", utf16_prefix(s, 59))),
        other => other.clone(),
    }
}

/// `fieldChanges`: player-visible field changes between two versions of a record.
fn field_changes(
    diff: Vec<FieldChange>,
    min_change: f64,
    counters: &mut Counters,
    only: Option<&dyn Fn(&str) -> bool>,
) -> Vec<NoteChange> {
    let mut out = Vec::new();
    for change in diff {
        if bookkeeping(&change.field) || only.is_some_and(|f| !f(&change.field)) {
            continue;
        }
        let (Some(from), Some(to)) = (&change.from, &change.to) else {
            counters.newly_tracked += 1;
            continue;
        };
        if let (Some(a), Some(b)) = (from.as_f64(), to.as_f64()) {
            let percent = match significant(a, b, min_change, counters) {
                Significance::Hidden => continue,
                Significance::NoPercent => None,
                Significance::Percent(p) => Some(p),
            };
            out.push(NoteChange {
                stat: change.field,
                before: Some(from.clone()),
                after: Some(to.clone()),
                percent,
                direction: Direction::Changed,
            });
            continue;
        }
        out.push(NoteChange {
            stat: change.field,
            before: Some(clip(from)),
            after: Some(clip(to)),
            percent: None,
            direction: Direction::Changed,
        });
    }
    out
}

/// `keepStat`: a changes.ts stat row that clears the threshold.
fn keep_stat(row: &StatChange, min_change: f64, counters: &mut Counters) -> Option<NoteChange> {
    let (Some(before), Some(after)) = (row.before, row.after) else {
        counters.newly_tracked += 1;
        return None;
    };
    if row.percent.is_some_and(|p| p.abs() < min_change) {
        counters.minor += 1;
        return None;
    }
    // From 0 is a value the old build never filled, not a balance call.
    let zero = before == 0.0;
    Some(NoteChange {
        stat: row.stat.clone(),
        before: Some(number(before)),
        after: Some(number(after)),
        percent: if zero { None } else { row.percent },
        direction: if zero {
            Direction::Changed
        } else {
            row.direction
        },
    })
}

fn json_or_undefined(v: Option<&Value>) -> String {
    json_text(v).unwrap_or_else(|| "undefined".into())
}

/// `signature`: a change set as one sortable string.
fn signature(changes: &[NoteChange]) -> String {
    let mut parts: Vec<String> = changes
        .iter()
        .map(|c| {
            format!(
                "{}={}>{}",
                c.stat,
                json_or_undefined(c.before.as_ref()),
                json_or_undefined(c.after.as_ref())
            )
        })
        .collect();
    sort_utf16(&mut parts);
    parts.join("|")
}

#[derive(Debug, Clone)]
struct ChangedRecord {
    record: String,
    label: String,
    key: String,
    category: Option<String>,
    changes: Vec<NoteChange>,
}

fn by_percent_then_stat(a: &NoteChange, b: &NoteChange) -> Ordering {
    let p = |c: &NoteChange| c.percent.unwrap_or(0.0).abs();
    p(b).partial_cmp(&p(a))
        .unwrap_or(Ordering::Equal)
        .then_with(|| locale_compare(&a.stat, &b.stat))
}

/// `groupChanged`: records sharing a key merge when they agree on every shared stat.
fn group_changed(records: Vec<ChangedRecord>) -> Vec<NoteLine> {
    let mut groups: OrderedMap<Vec<ChangedRecord>> = OrderedMap::new();
    for r in records {
        if r.changes.is_empty() {
            continue;
        }
        let key = format!("{}|{}", r.category.as_deref().unwrap_or(""), r.key);
        groups.entry_or(&key, Vec::new).push(r);
    }
    let mut lines = Vec::new();
    for group in groups.values() {
        let mut by_stat: OrderedMap<HashSet<String>> = OrderedMap::new();
        for r in group {
            for c in &r.changes {
                let pair = format!(
                    "[{},{}]",
                    json_text(c.before.as_ref()).unwrap_or_else(|| "null".into()),
                    json_text(c.after.as_ref()).unwrap_or_else(|| "null".into())
                );
                by_stat.entry_or(&c.stat, HashSet::new).insert(pair);
            }
        }
        let consistent = by_stat.values().all(|s| s.len() == 1);
        let mut buckets: OrderedMap<Vec<&ChangedRecord>> = OrderedMap::new();
        for r in group {
            let key = if consistent {
                "all".to_string()
            } else {
                signature(&r.changes)
            };
            buckets.entry_or(&key, Vec::new).push(r);
        }
        for bucket in buckets.values() {
            let mut merged: OrderedMap<NoteChange> = OrderedMap::new();
            for r in bucket {
                for c in &r.changes {
                    merged.insert(c.stat.clone(), c.clone());
                }
            }
            let mut changes: Vec<NoteChange> = merged.into_values().collect();
            changes.sort_by(by_percent_then_stat);
            let labels: Vec<String> = bucket.iter().map(|r| r.label.clone()).collect();
            let mut line = NoteLine::new(
                most_common(&labels),
                ItemStatus::Changed,
                bucket[0].category.clone(),
                sorted(
                    bucket
                        .iter()
                        .map(|r| short_record(&r.record).to_string())
                        .collect(),
                ),
            );
            line.changes = Some(changes);
            lines.push(line);
        }
    }
    lines.sort_by(|a, b| {
        locale_compare(
            a.category.as_deref().unwrap_or(""),
            b.category.as_deref().unwrap_or(""),
        )
        .then_with(|| locale_compare(&a.label, &b.label))
    });
    lines
}

// ---------------------------------------------------------------------------
// Dataset access
// ---------------------------------------------------------------------------

/// A build as the sections see it: its datasets, with the string table replaced by its
/// redaction-aligned form when that applies.
#[derive(Clone, Copy)]
struct View<'a> {
    data: &'a BuildData,
    strings: Option<&'a Body>,
}

impl<'a> View<'a> {
    fn get(&self, file: &str) -> Option<&'a Body> {
        if file == F::STRINGS
            && let Some(s) = self.strings
        {
            return Some(s);
        }
        self.data.get(file)
    }
    fn has(&self, file: &str) -> bool {
        self.data.get(file).is_some()
    }
}

/// `list`: a collection's records (non-objects read as empty records).
fn list<'a>(view: View<'a>, file: &str, collection: &str) -> Vec<&'a Body> {
    match view.get(file).and_then(|b| b.get(collection)) {
        Some(Value::Array(a)) => a.iter().map(|v| v.as_object().unwrap_or(&EMPTY)).collect(),
        _ => Vec::new(),
    }
}

/// Elements of an array field as records.
fn records_in<'a>(r: &'a Body, field: &str) -> Vec<&'a Body> {
    match r.get(field) {
        Some(Value::Array(a)) => a.iter().map(|v| v.as_object().unwrap_or(&EMPTY)).collect(),
        _ => Vec::new(),
    }
}

fn by_key<'a>(records: &[&'a Body]) -> OrderedMap<&'a Body> {
    let mut map = OrderedMap::new();
    for r in records {
        if let Some(Value::String(k)) = nullish(r, &["recordName", "className"]) {
            map.insert(k.clone(), *r);
        }
    }
    map
}

struct Split<'a> {
    added: Vec<&'a Body>,
    removed: Vec<&'a Body>,
    both: Vec<(&'a Body, &'a Body)>,
}

/// `split`: records only in `after`, only in `before`, and pairs present in both.
fn split<'a>(before: &[&'a Body], after: &[&'a Body]) -> Split<'a> {
    let a = by_key(before);
    let b = by_key(after);
    Split {
        added: b
            .iter()
            .filter(|(k, _)| !a.contains(k))
            .map(|(_, v)| *v)
            .collect(),
        removed: a
            .iter()
            .filter(|(k, _)| !b.contains(k))
            .map(|(_, v)| *v)
            .collect(),
        both: b
            .iter()
            .filter_map(|(k, v)| a.get(k).map(|p| (*p, *v)))
            .collect(),
    }
}

/// `humanise`: `quantumDrives` -> `quantum drives`.
fn humanise(collection: &str) -> String {
    let chars: Vec<char> = collection.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if i + 1 < chars.len() && chars[i].is_ascii_lowercase() && chars[i + 1].is_ascii_uppercase()
        {
            out.push(chars[i]);
            out.push(' ');
            out.push(chars[i + 1]);
            i += 2;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out.to_lowercase()
}

/// `categoryLabel`.
pub fn category_label(category: &str) -> String {
    let fixed = match category {
        "ships" => "Ships",
        "shipWeapons" => "Ship weapons",
        "missiles" => "Missiles",
        "fpsWeapons" => "Personal weapons",
        "shields" => "Shields",
        "powerPlants" => "Power plants",
        "coolers" => "Coolers",
        "quantumDrives" => "Quantum drives",
        "radars" => "Radars",
        "armor" => "Ship armour",
        "thrusters" => "Thrusters",
        _ => "",
    };
    if !fixed.is_empty() {
        return fixed.to_string();
    }
    let h = humanise(category);
    let mut chars = h.chars();
    match chars.next() {
        Some(c) if !matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}') => {
            c.to_uppercase().chain(chars).collect()
        }
        _ => h,
    }
}

/// `nameIndex`: lower-cased class name -> display name, across item-bearing datasets.
fn name_index(builds: &[View<'_>]) -> OrderedMap<String> {
    let mut index = OrderedMap::new();
    let mut add = |r: &Body| {
        let record = record_id(r);
        if !record.is_empty()
            && let Some(name) = real_name(r.get("name"), Some(&record))
        {
            let key = short_record(&record).to_lowercase();
            if !index.contains(&key) {
                index.insert(key, name.to_string());
            }
        }
    };
    for data in builds {
        list(*data, F::TAGS, "items").into_iter().for_each(&mut add);
        for c in ["shipWeapons", "fpsWeapons", "missiles"] {
            list(*data, F::WEAPONS, c).into_iter().for_each(&mut add);
        }
        for c in ["armor", "clothing"] {
            list(*data, F::ARMOR, c).into_iter().for_each(&mut add);
        }
        let components = data.get(F::COMPONENTS).unwrap_or(&EMPTY);
        for c in diffable_collections(components) {
            list(*data, F::COMPONENTS, &c)
                .into_iter()
                .for_each(&mut add);
        }
    }
    index
}

/// `familyIndex`: lower-cased class name -> weapon family from its item tags.
fn family_index(builds: &[View<'_>]) -> HashMap<String, String> {
    let mut index = HashMap::new();
    for data in builds {
        for item in list(*data, F::TAGS, "items") {
            let Some(Value::Array(tags)) = item.get("tags") else {
                continue;
            };
            let tags: Vec<&str> = tags.iter().filter_map(Value::as_str).collect();
            if let Some(family) = weapon_family(&tags).filter(|f| !f.is_empty())
                && let Some(Value::String(cls)) = item.get("className")
            {
                index.entry(cls.to_lowercase()).or_insert(family);
            }
        }
    }
    index
}

// ---------------------------------------------------------------------------
// Sections
// ---------------------------------------------------------------------------

struct Rename {
    from: String,
    to: String,
    record: String,
}

struct Context<'a> {
    before: View<'a>,
    after: View<'a>,
    changes: &'a PatchChanges,
    min_change: f64,
    counters: Counters,
    names: OrderedMap<String>,
    families: HashMap<String, String>,
    renames: Vec<Rename>,
    siblings: HashMap<String, Option<String>>,
}

fn section(id: SectionId, title: &str, lines: Vec<NoteLine>, notes: Vec<String>) -> NoteSection {
    NoteSection {
        id,
        title: title.to_string(),
        lines,
        notes,
    }
}

fn folded_note(n: isize, what: &str) -> Vec<String> {
    if n > 0 {
        vec![format!("+{n} {what} not listed")]
    } else {
        Vec::new()
    }
}

/// `file.replace(/^game-|\.json$/g, "")`.
fn dataset_name(file: &str) -> &str {
    let f = file.strip_prefix("game-").unwrap_or(file);
    f.strip_suffix(".json").unwrap_or(f)
}

/// `compared`: whether both builds carry `file`; says so when only the newer one does.
fn compared(ctx: &Context<'_>, file: &str, notes: &mut Vec<String>) -> bool {
    if ctx.before.has(file) && ctx.after.has(file) {
        return true;
    }
    if ctx.after.has(file) {
        notes.push(format!(
            "first build with {} data; nothing to compare against",
            dataset_name(file)
        ));
    }
    false
}

/// `s.split(/\s+/)`.
fn split_ws(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut in_space = false;
    let mut run_start = 0;
    for (i, c) in s.char_indices() {
        if is_js_space(c) {
            if !in_space {
                run_start = i;
                in_space = true;
            }
        } else if in_space {
            out.push(&s[start..run_start]);
            start = i;
            in_space = false;
        }
    }
    if in_space {
        out.push(&s[start..run_start]);
        out.push("");
    } else {
        out.push(&s[start..]);
    }
    out
}

/// `commonWordPrefix`.
fn common_word_prefix(names: &[String]) -> String {
    if names.is_empty() {
        return String::new();
    }
    let words: Vec<Vec<&str>> = names.iter().map(|n| split_ws(n)).collect();
    let mut out = Vec::new();
    let mut i = 0;
    while words.iter().all(|w| i < w.len() && w[i] == words[0][i]) {
        out.push(words[0][i]);
        i += 1;
    }
    out.join(" ")
}

/// `_\d+_\d+$` removed (`replace`), or `None` when there is none.
fn strip_two_numbers(s: &str) -> Option<&str> {
    let t = s.trim_end_matches(|c: char| c.is_ascii_digit());
    if t.len() == s.len() {
        return None;
    }
    let t = t.strip_suffix('_')?;
    let u = t.trim_end_matches(|c: char| c.is_ascii_digit());
    if u.len() == t.len() {
        return None;
    }
    u.strip_suffix('_')
}

fn without_two_numbers(s: &str) -> String {
    strip_two_numbers(s).unwrap_or(s).to_string()
}

/// `/^_\d+_\d+$/`.
fn is_two_numbers(s: &str) -> bool {
    strip_two_numbers(s) == Some("")
}

/// `siblingFamily`: a product-line name for an unnamed colour variant, from its named
/// siblings (`omc_utility_heavy_arms_01_03_01` -> `Corbel Arms`).
fn sibling_family(ctx: &mut Context<'_>, record: &str) -> Option<String> {
    let cls = short_record(record).to_lowercase();
    let prefix = without_two_numbers(&cls);
    if prefix == cls {
        return None;
    }
    if !ctx.siblings.contains_key(&prefix) {
        let names: Vec<String> = ctx
            .names
            .iter()
            .filter(|(key, _)| key.starts_with(&prefix) && is_two_numbers(&key[prefix.len()..]))
            .map(|(_, name)| name.clone())
            .collect();
        let family = if names.is_empty() {
            None
        } else {
            let common = common_word_prefix(&names);
            Some(if common.is_empty() {
                split_base(&names[0])
            } else {
                common
            })
        };
        ctx.siblings.insert(prefix.clone(), family);
    }
    ctx.siblings.get(&prefix).cloned().flatten()
}

fn note_renames(ctx: &mut Context<'_>, pairs: &[(&Body, &Body)]) {
    for (a, b) in pairs {
        let record = record_id(b);
        if is_template(b) {
            continue;
        }
        let (Some(from), Some(to)) = (
            real_name(a.get("name"), Some(&record)),
            real_name(b.get("name"), Some(&record)),
        ) else {
            continue;
        };
        if from == to {
            continue;
        }
        ctx.renames.push(Rename {
            from: from.to_string(),
            to: to.to_string(),
            record,
        });
    }
}

fn new_ships(ctx: &mut Context<'_>) -> NoteSection {
    let mut notes = Vec::new();
    if !compared(ctx, F::SHIPS, &mut notes) {
        return section(SectionId::NewShips, "New ships", Vec::new(), notes);
    }
    let items: Vec<_> = ctx
        .changes
        .items
        .iter()
        .filter(|i| i.category == "ships" && !npc_only(&i.record))
        .collect();
    let mut lines = Vec::new();
    for status in [ItemStatus::Added, ItemStatus::Removed] {
        let mut by_name: OrderedMap<Vec<String>> = OrderedMap::new();
        for i in items.iter().filter(|x| x.change == status) {
            by_name.entry_or(&i.name, Vec::new).push(i.record.clone());
        }
        // Default sort of [name, records] pairs: by their string form.
        let mut entries: Vec<(String, Vec<String>, String)> = by_name
            .iter()
            .map(|(k, v)| (k.clone(), v.clone(), format!("{k},{}", v.join(","))))
            .collect();
        entries.sort_by(|a, b| cmp_utf16(&a.2, &b.2));
        for (label, records, _) in entries {
            lines.push(NoteLine::new(label, status, None, sorted(records)));
        }
    }
    let s = split(
        &list(ctx.before, F::SHIPS, "vehicles"),
        &list(ctx.after, F::SHIPS, "vehicles"),
    );
    let player = items
        .iter()
        .filter(|i| i.change == ItemStatus::Added)
        .count();
    section(
        SectionId::NewShips,
        "New ships",
        lines,
        folded_note(
            s.added.len() as isize - player as isize,
            "AI/NPC/template variants",
        ),
    )
}

/// `balanceLines`: stat-row lines for the given categories; NPC-only records go in `npc`.
fn balance_lines(
    ctx: &mut Context<'_>,
    categories: &[&str],
    key_of: &dyn Fn(&Context<'_>, &StatChange) -> String,
    npc: &mut OrderedSet,
) -> Vec<NoteLine> {
    let mut per_record: OrderedMap<ChangedRecord> = OrderedMap::new();
    let changes = ctx.changes;
    for row in &changes.stats {
        if !categories.contains(&row.category.as_str()) {
            continue;
        }
        let id = format!("{}|{}", row.category, row.record);
        if npc_only(&row.record) {
            npc.add(&id);
            continue;
        }
        let key = key_of(ctx, row);
        let kept = keep_stat(row, ctx.min_change, &mut ctx.counters);
        let entry = per_record.entry_or(&id, || ChangedRecord {
            record: row.record.clone(),
            label: row.name.clone(),
            key,
            category: Some(row.category.clone()),
            changes: Vec::new(),
        });
        if let Some(k) = kept {
            entry.changes.push(k);
        }
    }
    let order = |c: Option<&String>| {
        categories
            .iter()
            .position(|x| Some(*x) == c.map(String::as_str))
            .map_or(-1, |i| i as isize)
    };
    let mut lines = group_changed(per_record.into_values().collect());
    lines.sort_by(|a, b| {
        order(a.category.as_ref())
            .cmp(&order(b.category.as_ref()))
            .then_with(|| locale_compare(&a.label, &b.label))
    });
    lines
}

const SHIP_BALANCE: &[&str] = &[
    "ships",
    "shipWeapons",
    "missiles",
    "shields",
    "powerPlants",
    "coolers",
    "quantumDrives",
    "radars",
    "armor",
    "thrusters",
];

struct TrackedSource {
    category: &'static str,
    file: String,
    collection: &'static str,
    include: Option<fn(&Body) -> bool>,
}

/// `TRACKED_SOURCES`.
static TRACKED_SOURCES: LazyLock<Vec<TrackedSource>> = LazyLock::new(|| {
    let mut out: Vec<TrackedSource> = CATEGORIES
        .iter()
        .map(|c| TrackedSource {
            category: c.category,
            file: dataset_file(c.dataset),
            collection: c.collection,
            include: c.include,
        })
        .collect();
    for c in ["shipWeapons", "missiles"] {
        out.push(TrackedSource {
            category: c,
            file: F::COMPONENTS.into(),
            collection: if c == "shipWeapons" {
                "weapons"
            } else {
                "missiles"
            },
            include: Some(|r| !is_template(r)),
        });
    }
    out
});

/// `untrackedNote`: tracked records that changed only outside the tracked stats.
fn untracked_note(ctx: &mut Context<'_>, categories: &[&str]) -> Vec<String> {
    let covered: HashSet<String> = ctx
        .changes
        .stats
        .iter()
        .map(|row| format!("{}|{}", row.category, short_record(&row.record)))
        .collect();
    let mut found: OrderedMap<OrderedSet> = OrderedMap::new();
    for src in TRACKED_SOURCES.iter() {
        if !categories.contains(&src.category) {
            continue;
        }
        let s = split(
            &list(ctx.before, &src.file, src.collection),
            &list(ctx.after, &src.file, src.collection),
        );
        for (a, b) in s.both {
            let id = record_id(b);
            let record = short_record(&id);
            if record.is_empty()
                || npc_only(record)
                || src.include.is_some_and(|f| !f(b))
                || covered.contains(&format!("{}|{record}", src.category))
            {
                continue;
            }
            let moved = diff_body_fields(a, b).iter().any(|(field, from, to)| {
                !bookkeeping(field) && !prose(field) && from.is_some() && to.is_some()
            });
            if moved {
                found.entry_or(src.category, OrderedSet::new).add(record);
            }
        }
    }
    let parts: Vec<String> = categories
        .iter()
        .filter_map(|c| {
            found
                .get(c)
                .map(|set| format!("{} ×{}", category_label(c).to_lowercase(), set.len()))
        })
        .collect();
    let total: usize = found.values().map(OrderedSet::len).sum();
    ctx.counters.untracked += total;
    if total > 0 {
        vec![format!(
            "changed only outside the tracked stats (loadouts, volumes, …), not listed: {}; see `diff`",
            parts.join(", ")
        )]
    } else {
        Vec::new()
    }
}

fn ship_balance(ctx: &mut Context<'_>) -> NoteSection {
    let mut npc = OrderedSet::new();
    let lines = balance_lines(ctx, SHIP_BALANCE, &|_, r| r.name.clone(), &mut npc);
    let mut notes = untracked_note(ctx, SHIP_BALANCE);
    notes.extend(folded_note(
        npc.len() as isize,
        "Vanduul (NPC-only) hulls and weapons with stat changes",
    ));
    section(SectionId::ShipBalance, "Ship balance", lines, notes)
}

struct Skin {
    skin: String,
    guns: OrderedMap<Option<f64>>,
    records: Vec<String>,
    category: Option<String>,
    same_stats: bool,
}

/// `foldNewSkins`: new skins of guns that already existed fold into one line per skin.
pub fn fold_new_skins(lines: Vec<NoteLine>, before: &[&Body], after: &[&Body]) -> Vec<NoteLine> {
    let mut base: HashMap<String, &Body> = HashMap::new();
    for r in before {
        let rn = js_string(r.get("recordName"));
        let key = base_class(&rn);
        if is_real_name(r.get("name"), Some(&rn)) && short_record(&rn).to_lowercase() == key {
            base.insert(key, r);
        }
    }
    let mut added: HashMap<String, &Body> = HashMap::new();
    for r in after {
        added.insert(
            short_record(&js_string(r.get("recordName"))).to_lowercase(),
            r,
        );
    }
    let mut skins: OrderedMap<Skin> = OrderedMap::new();
    let mut kept = Vec::new();
    for line in lines {
        let bases = dedup(line.records.iter().map(|r| base_class(r)));
        let base_rec = if bases.len() == 1 {
            base.get(&bases[0]).copied()
        } else {
            None
        };
        let gun = base_rec.map(|r| js_string(r.get("name")));
        let mut skin = None;
        if line.status == ItemStatus::Added
            && line.variants.is_none()
            && let Some(gun) = &gun
        {
            if let Some(quoted) = split_variant(&line.label).1 {
                skin = Some(quoted);
            } else if line
                .label
                .to_lowercase()
                .starts_with(&format!("{} ", gun.to_lowercase()))
            {
                skin = Some(js_trim(utf16_from(&line.label, utf16_len(gun))).to_string());
            }
        }
        let (Some(skin), Some(gun), Some(base_rec)) =
            (skin.filter(|s| !s.is_empty()), gun, base_rec)
        else {
            kept.push(line);
            continue;
        };
        let key = format!(
            "{}|{}",
            line.category.as_deref().unwrap_or(""),
            skin.to_lowercase()
        );
        let entry = skins.entry_or(&key, || Skin {
            skin: skin.clone(),
            guns: OrderedMap::new(),
            records: Vec::new(),
            category: line.category.clone(),
            same_stats: true,
        });
        entry.guns.insert(
            split_base(&gun),
            base_rec.get("size").and_then(Value::as_f64),
        );
        entry.records.extend(line.records.iter().cloned());
        for r in &line.records {
            match added.get(&r.to_lowercase()) {
                Some(variant) if same_stats(base_rec, variant) => {}
                _ => entry.same_stats = false,
            }
        }
    }
    let mut folded: Vec<NoteLine> = skins
        .values()
        .map(|e| {
            let mut guns: Vec<(&String, Option<f64>)> =
                e.guns.iter().map(|(k, v)| (k, *v)).collect();
            guns.sort_by(|a, b| {
                (a.1.unwrap_or(0.0) - b.1.unwrap_or(0.0))
                    .partial_cmp(&0.0)
                    .unwrap_or(Ordering::Equal)
                    .then_with(|| locale_compare_numeric(a.0, b.0))
            });
            let multi = e.guns.len() > 1 && e.category.as_deref().is_some_and(|c| !c.is_empty());
            let names: Vec<String> = guns
                .iter()
                .map(|(name, size)| match size {
                    Some(s) if multi => format!("{name} (S{})", js_number(*s)),
                    _ => name.to_string(),
                })
                .collect();
            let stats = if e.same_stats {
                "paint only, same stats as the base gun"
            } else {
                "stats differ from the base gun, see `diff`"
            };
            let mut line = NoteLine::new(
                format!("{} skin", e.skin),
                ItemStatus::Added,
                e.category.clone(),
                sorted(e.records.clone()),
            );
            line.detail = Some(format!("for {}; {stats}", names.join(", ")));
            line
        })
        .collect();
    folded.sort_by(|a, b| locale_compare(&a.label, &b.label));
    kept.extend(folded);
    kept
}

/// Combat stats a paint job must not change, compared where both records carry them.
const SKIN_STATS: &[&str] = &[
    "size",
    "dps",
    "alpha",
    "fireRate",
    "range",
    "speed",
    "ammoCapacity",
    "damageType",
    "heatPerShot",
    "spread",
];

fn same_stats(a: &Body, b: &Body) -> bool {
    let ammo = |r: &Body| -> Option<Value> {
        match r.get("ammo") {
            Some(Value::Object(o)) => o.get("recordName").cloned(),
            Some(Value::Array(_)) => None,
            other => other.cloned(),
        }
    };
    if !js_strict_eq(ammo(a).as_ref(), ammo(b).as_ref()) {
        return false;
    }
    SKIN_STATS.iter().all(|k| match (a.get(*k), b.get(*k)) {
        (Some(x), Some(y)) => to_js_json_compact(x) == to_js_json_compact(y),
        _ => true,
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Grouping {
    Class,
    Name,
}

fn new_ship_items(ctx: &mut Context<'_>) -> NoteSection {
    let mut lines = Vec::new();
    let mut folded = 0usize;
    let mut seen: HashSet<String> = HashSet::new();
    let mut take = |ctx: &mut Context<'_>,
                    collection: &str,
                    before: Vec<&Body>,
                    after: Vec<&Body>,
                    grouping: Grouping| {
        fn unseen<'b>(seen: &HashSet<String>, list: &[&'b Body]) -> Vec<&'b Body> {
            list.iter()
                .filter(|r| !seen.contains(&js_string(r.get("recordName"))))
                .copied()
                .collect()
        }
        let (ub, ua) = (unseen(&seen, &before), unseen(&seen, &after));
        let s = split(&ub, &ua);
        for r in before.iter().chain(after.iter()) {
            seen.insert(js_string(r.get("recordName")));
        }
        note_renames(ctx, &s.both);
        for (status, records) in [
            (ItemStatus::Added, &s.added),
            (ItemStatus::Removed, &s.removed),
        ] {
            let key: &dyn Fn(&Member) -> String = match grouping {
                Grouping::Class => &|m: &Member| base_class(&m.record),
                Grouping::Name => {
                    &|m: &Member| m.name.clone().unwrap_or_else(|| base_class(&m.record))
                }
            };
            // NPC-only (Vanduul) parts are folded with the templates.
            let members: Vec<Member> = records
                .iter()
                .map(|r| {
                    let mut m = member_of(r, None);
                    if npc_only(&m.record) {
                        m.template = true;
                    }
                    m
                })
                .collect();
            let (g, n) = group_members(&members, status, key, Some(collection));
            let grouped: Vec<NoteLine> = g
                .into_iter()
                .map(|mut l| {
                    if l.records.len() > 1 && grouping == Grouping::Name {
                        l.variants = None;
                    }
                    l
                })
                .collect();
            match grouping {
                Grouping::Class => lines.extend(fold_new_skins(grouped, &before, &after)),
                Grouping::Name => lines.extend(grouped),
            }
            folded += n;
        }
    };
    let mut notes = Vec::new();
    if compared(ctx, F::WEAPONS, &mut notes) {
        for c in ["shipWeapons", "missiles"] {
            let (b, a) = (
                list(ctx.before, F::WEAPONS, c),
                list(ctx.after, F::WEAPONS, c),
            );
            take(ctx, c, b, a, Grouping::Class);
        }
    }
    if compared(ctx, F::COMPONENTS, &mut notes) {
        for c in diffable_collections(ctx.after.get(F::COMPONENTS).unwrap_or(&EMPTY)) {
            if c != "weapons" && c != "missiles" {
                let (b, a) = (
                    list(ctx.before, F::COMPONENTS, &c),
                    list(ctx.after, F::COMPONENTS, &c),
                );
                take(ctx, &c, b, a, Grouping::Name);
            }
        }
    }
    notes.extend(folded_note(
        folded as isize,
        "unnamed templates, NPC-only and ship-specific parts",
    ));
    section(
        SectionId::NewShipItems,
        "New ship weapons & components",
        lines,
        notes,
    )
}

/// Identity of a value in a JavaScript `Set` (SameValueZero; objects never repeat).
fn set_key(v: Option<&Value>) -> Option<String> {
    match v {
        None => Some("u".into()),
        Some(Value::Null) => Some("n".into()),
        Some(Value::Bool(b)) => Some(format!("b{b}")),
        Some(Value::Number(n)) => Some(format!("d{}", js_number(n.as_f64().unwrap_or(f64::NAN)))),
        Some(Value::String(s)) => Some(format!("s{s}")),
        Some(_) => None,
    }
}

/// `personalWeapons`: game-weapons.fpsWeapons, plus melee only game-fps-weapons has.
fn personal_weapons<'a>(data: View<'a>) -> Vec<&'a Body> {
    let primary = list(data, F::WEAPONS, "fpsWeapons");
    let have: HashSet<String> = primary
        .iter()
        .filter_map(|r| set_key(r.get("recordName")))
        .collect();
    let mut out = primary.clone();
    out.extend(
        list(data, F::FPS, "weapons")
            .into_iter()
            .filter(|r| set_key(r.get("recordName")).is_none_or(|k| !have.contains(&k))),
    );
    out
}

fn family_of(ctx: &Context<'_>, record: &str) -> Option<String> {
    ctx.families
        .get(&short_record(record).to_lowercase())
        .or_else(|| ctx.families.get(&base_class(record)))
        .cloned()
}

fn new_personal_weapons(ctx: &mut Context<'_>) -> NoteSection {
    let mut notes = Vec::new();
    if !compared(ctx, F::WEAPONS, &mut notes) {
        return section(
            SectionId::NewPersonalWeapons,
            "New personal weapons",
            Vec::new(),
            notes,
        );
    }
    let pb = personal_weapons(ctx.before);
    let pa = personal_weapons(ctx.after);
    let s = split(&pb, &pa);
    note_renames(ctx, &s.both);
    let mut lines = Vec::new();
    let mut folded = 0;
    for (status, records) in [
        (ItemStatus::Added, &s.added),
        (ItemStatus::Removed, &s.removed),
    ] {
        let members: Vec<Member> = records
            .iter()
            .map(|r| member_of(r, family_of(ctx, &js_string(r.get("recordName")))))
            .collect();
        let key = |m: &Member| match &m.family {
            Some(f) => format!("family:{f}"),
            None => base_class(&m.record),
        };
        let (g, n) = group_members(&members, status, &key, None);
        lines.extend(fold_new_skins(g, &pb, &pa));
        folded += n;
    }
    notes.extend(folded_note(folded as isize, "unnamed templates and props"));
    section(
        SectionId::NewPersonalWeapons,
        "New personal weapons",
        lines,
        notes,
    )
}

fn personal_balance(ctx: &mut Context<'_>) -> NoteSection {
    let mut npc = OrderedSet::new();
    let mut lines = balance_lines(
        ctx,
        &["fpsWeapons"],
        &|ctx, r| family_of(ctx, &r.record).unwrap_or_else(|| split_base(&r.name)),
        &mut npc,
    );
    for line in &mut lines {
        if line.records.len() > 1 {
            line.label = split_base(&line.label);
        }
    }
    let notes = untracked_note(ctx, &["fpsWeapons"]);
    section(
        SectionId::PersonalBalance,
        "Personal weapon balance",
        lines,
        notes,
    )
}

/// A colour set's label and its pieces.
type NamedSet<'a> = (String, Vec<&'a Body>);

/// `armourLines`: added or removed armour and clothing, pieces of one colour set as one
/// set, sets of one product line as one line with the colour sets as variants.
fn armour_lines(
    ctx: &mut Context<'_>,
    records: &[&Body],
    status: ItemStatus,
    category: &str,
) -> (Vec<NoteLine>, usize) {
    let mut folded = 0;
    let mut product_lines: OrderedMap<OrderedMap<Vec<&Body>>> = OrderedMap::new();
    for r in records {
        if is_template(r) {
            folded += 1;
            continue;
        }
        let set = armour_set_key(&js_string(r.get("recordName")));
        let product = without_two_numbers(&set);
        product_lines
            .entry_or(&product, OrderedMap::new)
            .entry_or(&set, Vec::new)
            .push(*r);
    }
    let mut lines = Vec::new();
    for sets in product_lines.values() {
        let mut named: Vec<(String, Vec<&Body>)> = Vec::new();
        for (set, pieces) in sets.iter() {
            let names: Vec<String> = pieces
                .iter()
                .filter_map(|p| real_name(p.get("name"), Some(&js_string(p.get("recordName")))))
                .map(strip_slot)
                .collect();
            let label = if !names.is_empty() {
                most_common(&names)
            } else {
                let sibling = sibling_family(ctx, &js_string(pieces[0].get("recordName")));
                let family = sibling.map(|s| strip_slot(&s)).filter(|f| !f.is_empty());
                match family {
                    Some(f) => format!("{f} ({set})"),
                    None => format!("({set})"),
                }
            };
            named.push((label, pieces.clone()));
        }
        let prefix = if named.len() > 1 {
            common_word_prefix(&named.iter().map(|n| n.0.clone()).collect::<Vec<_>>())
        } else {
            String::new()
        };
        let groups: Vec<(String, Vec<&NamedSet>)> = if !prefix.is_empty() {
            vec![(prefix.clone(), named.iter().collect())]
        } else {
            named.iter().map(|n| (n.0.clone(), vec![n])).collect()
        };
        for (label, group_sets) in groups {
            let pieces: Vec<&Body> = group_sets
                .iter()
                .flat_map(|n| n.1.iter().copied())
                .collect();
            let slots = sorted(dedup(
                pieces
                    .iter()
                    .filter_map(|p| slot_of(&js_string(p.get("recordName")))),
            ));
            let mut line = NoteLine::new(
                label,
                status,
                Some(category.to_string()),
                sorted(
                    pieces
                        .iter()
                        .map(|p| short_record(&js_string(p.get("recordName"))).to_string())
                        .collect(),
                ),
            );
            if group_sets.len() > 1 {
                line.variants = Some(sorted(
                    group_sets
                        .iter()
                        .map(|n| {
                            let rest = js_trim(utf16_from(&n.0, utf16_len(&prefix)));
                            if rest.is_empty() {
                                n.0.clone()
                            } else {
                                rest.to_string()
                            }
                        })
                        .collect(),
                ));
            }
            if pieces.len() > 1 {
                line.detail = Some(if slots.len() > 1 {
                    format!("pieces: {}", slots.join(", "))
                } else {
                    format!("{} pieces", pieces.len())
                });
            }
            lines.push(line);
        }
    }
    lines.sort_by(|a, b| locale_compare(&a.label, &b.label));
    (lines, folded)
}

fn armour(ctx: &mut Context<'_>) -> NoteSection {
    let mut lines = Vec::new();
    let mut notes = Vec::new();
    let mut folded = 0;
    if !compared(ctx, F::ARMOR, &mut notes) {
        return section(SectionId::Armour, "Armour & clothing", lines, notes);
    }
    for collection in ["armor", "clothing"] {
        let s = split(
            &list(ctx.before, F::ARMOR, collection),
            &list(ctx.after, F::ARMOR, collection),
        );
        note_renames(ctx, &s.both);
        let category = if collection == "armor" {
            "Armour"
        } else {
            "Clothing"
        };
        for (status, records) in [
            (ItemStatus::Added, &s.added),
            (ItemStatus::Removed, &s.removed),
        ] {
            let (l, n) = armour_lines(ctx, records, status, category);
            lines.extend(l);
            folded += n;
        }
        let mut changed = Vec::new();
        for (a, b) in s.both.iter().filter(|(_, b)| !is_template(b)) {
            let record = js_string(b.get("recordName"));
            let label = match real_name(b.get("name"), Some(&record)) {
                Some(n) => strip_slot(n),
                None => short_record(&record).to_string(),
            };
            let changes = field_changes(
                diff_entry_bodies(a, b),
                ctx.min_change,
                &mut ctx.counters,
                None,
            );
            changed.push(ChangedRecord {
                key: armour_set_key(&record),
                record,
                label,
                category: Some(category.to_string()),
                changes,
            });
        }
        lines.extend(group_changed(changed));
    }
    section(
        SectionId::Armour,
        "Armour & clothing",
        lines,
        folded_note(folded as isize, "unnamed templates"),
    )
}

/// `cleanTitle`.
fn clean_title(title: Option<&Value>) -> Option<String> {
    let Some(Value::String(t)) = title else {
        return None;
    };
    if js_trim(t).is_empty() || t.starts_with(['~', '@']) {
        return None;
    }
    Some(js_trim(&collapse_ws(&t.replace("\\n", " "))).to_string())
}

/// `listSome`.
fn list_some(values: &[String], max: usize) -> String {
    if values.len() <= max {
        values.join(", ")
    } else {
        format!("{} +{} more", values[..max].join(", "), values.len() - max)
    }
}

/// `rewardMissions`: a blueprint's mission rewards, contract -> mission title.
fn reward_missions(bp: &Body) -> OrderedMap<String> {
    let mut out = OrderedMap::new();
    for r in records_in(bp, "rewardedBy") {
        let contract = string_of(r, &["contract", "mission"]);
        if !contract.is_empty() {
            let label = clean_title(r.get("mission")).unwrap_or_else(|| contract.clone());
            out.insert(contract, label);
        }
    }
    out
}

fn blueprint_label(ctx: &mut Context<'_>, bp: &Body) -> String {
    let record = string_of(bp, &["recordName"]);
    let produces = match bp.get("produces") {
        Some(Value::String(p)) => p.clone(),
        _ => {
            let s = short_record(&record);
            match s.get(..9) {
                Some(p) if p.eq_ignore_ascii_case("BP_CRAFT_") => s[9..].to_string(),
                _ => s.to_string(),
            }
        }
    };
    for candidate in [bp.get("displayName"), bp.get("name")] {
        if let Some(name) = real_name(candidate, Some(&record))
            && is_real_name(candidate, Some(&produces))
        {
            return name.to_string();
        }
    }
    if let Some(named) = ctx.names.get(&produces.to_lowercase()) {
        return named.clone();
    }
    match sibling_family(ctx, &produces).filter(|f| !f.is_empty()) {
        Some(family) => format!("{family} ({produces})"),
        None => format!("{produces} (unnamed)"),
    }
}

/// The first `_(arms|core|helmet|legs|backpack)_` (case-insensitive) replaced by `_`.
fn replace_slot_between(s: &str) -> String {
    let b = s.as_bytes();
    for (i, _) in s.match_indices('_') {
        for w in SLOTS {
            let end = i + 1 + w.len();
            if b.len() > end && b[i + 1..end].eq_ignore_ascii_case(w.as_bytes()) && b[end] == b'_' {
                return format!("{}_{}", &s[..i], &s[end + 1..]);
            }
        }
    }
    s.to_string()
}

fn is_craft_time(stat: &str) -> bool {
    stat.ends_with("craftTimeSeconds")
}

struct ChangedBlueprint {
    labels: Vec<String>,
    records: Vec<String>,
    detail: String,
    changes: Vec<NoteChange>,
}

fn crafting(ctx: &mut Context<'_>) -> NoteSection {
    let mut lines = Vec::new();
    let mut notes = Vec::new();
    if !compared(ctx, F::BLUEPRINTS, &mut notes) {
        return section(SectionId::Crafting, "Crafting", lines, notes);
    }
    let before_bps = list(ctx.before, F::BLUEPRINTS, "blueprints");
    let rewards_tracked = before_bps
        .iter()
        .any(|bp| matches!(bp.get("rewardedBy"), Some(Value::Array(_))));
    if !rewards_tracked {
        notes
            .push("first build with blueprint mission rewards; reward changes not compared".into());
    }
    let s = split(&before_bps, &list(ctx.after, F::BLUEPRINTS, "blueprints"));
    let product_key = |bp: &Body| match bp.get("produces") {
        Some(Value::String(p)) => armour_set_key(p),
        _ => armour_set_key(&js_string(bp.get("recordName"))),
    };

    for (status, records) in [
        (ItemStatus::Added, &s.added),
        (ItemStatus::Removed, &s.removed),
    ] {
        let mut groups: OrderedMap<Vec<&Body>> = OrderedMap::new();
        for bp in records {
            groups.entry_or(&product_key(bp), Vec::new).push(*bp);
        }
        for group in groups.values() {
            let labels: Vec<String> = group.iter().map(|bp| blueprint_label(ctx, bp)).collect();
            let produces = |bp: &Body| string_of(bp, &["produces"]);
            let armour_set =
                group.len() > 1 && group.iter().all(|bp| slot_of(&produces(bp)).is_some());
            let label = if armour_set {
                most_common(
                    &labels
                        .iter()
                        .map(|l| replace_slot_between(&remove_slot_word(l)))
                        .collect::<Vec<_>>(),
                )
            } else {
                labels[0].clone()
            };
            let missions =
                sorted(dedup(group.iter().flat_map(|bp| {
                    reward_missions(bp).into_values().collect::<Vec<_>>()
                })));
            let mut line = NoteLine::new(
                label,
                status,
                Some("Blueprints".into()),
                sorted(
                    group
                        .iter()
                        .map(|bp| short_record(&js_string(bp.get("recordName"))).to_string())
                        .collect(),
                ),
            );
            if group.len() > 1 && !armour_set {
                line.variants = Some(labels[1..].to_vec());
            }
            let mut parts = Vec::new();
            if armour_set {
                let slots = sorted(
                    group
                        .iter()
                        .map(|bp| {
                            slot_of(&js_string(bp.get("produces")))
                                .unwrap_or_else(|| "undefined".into())
                        })
                        .collect(),
                );
                parts.push(format!("pieces: {}", slots.join(", ")));
            }
            if status == ItemStatus::Added {
                parts.push(if missions.is_empty() {
                    "no mission reward".into()
                } else {
                    format!(
                        "rewarded by {} mission{}: {}",
                        missions.len(),
                        if missions.len() == 1 { "" } else { "s" },
                        list_some(&missions, 3)
                    )
                });
            }
            if !parts.is_empty() {
                line.detail = Some(parts.join("; "));
            }
            lines.push(line);
        }
    }

    // Changed blueprints: rewards compared as sets, and recipe edits.
    let mut changed: OrderedMap<ChangedBlueprint> = OrderedMap::new();
    for (a, b) in &s.both {
        let ra = reward_missions(a);
        let rb = if rewards_tracked {
            reward_missions(b)
        } else {
            reward_missions(a)
        };
        let gained = sorted(dedup(
            rb.iter()
                .filter(|(k, _)| !ra.contains(k))
                .map(|(_, v)| v.clone()),
        ));
        let lost = sorted(dedup(
            ra.iter()
                .filter(|(k, _)| !rb.contains(k))
                .map(|(_, v)| v.clone()),
        ));
        let recipe: Vec<NoteChange> = field_changes(
            diff_entry(a.get("tiers"), b.get("tiers")),
            ctx.min_change,
            &mut ctx.counters,
            None,
        )
        .into_iter()
        .map(|mut c| {
            c.stat = format!(
                "tiers{}{}",
                if c.stat.starts_with('[') { "" } else { "." },
                c.stat
            );
            c
        })
        .collect();
        let mut parts = Vec::new();
        if !gained.is_empty() {
            parts.push(format!("now also from {}", list_some(&gained, 3)));
        }
        if !lost.is_empty() {
            parts.push(format!("no longer from {}", list_some(&lost, 3)));
        }
        let time_only = recipe.iter().all(|c| is_craft_time(&c.stat));
        if !recipe.is_empty() && !time_only {
            parts.push("recipe changed".into());
        }
        if parts.is_empty() && recipe.is_empty() {
            continue;
        }
        let detail = parts.join("; ");
        let changes: Vec<NoteChange> = recipe
            .into_iter()
            .filter(|c| is_craft_time(&c.stat))
            .collect();
        let key = format!("{detail}|{}", signature(&changes));
        let label = blueprint_label(ctx, b);
        let record = short_record(&js_string(b.get("recordName"))).to_string();
        let entry = changed.entry_or(&key, || ChangedBlueprint {
            labels: Vec::new(),
            records: Vec::new(),
            detail: detail.clone(),
            changes: changes.clone(),
        });
        entry.labels.push(label);
        entry.records.push(record);
    }
    for entry in changed.into_values() {
        let mut line = NoteLine::new(
            list_some(&sorted(entry.labels), 4),
            ItemStatus::Changed,
            Some("Blueprints".into()),
            sorted(entry.records),
        );
        line.detail_first = true;
        if !entry.detail.is_empty() {
            line.detail = Some(entry.detail);
        }
        if !entry.changes.is_empty() {
            line.changes = Some(entry.changes);
        }
        lines.push(line);
    }

    // Reward pools carry only a name, so they are matched by it.
    let pools_a: HashSet<String> = list(ctx.before, F::BLUEPRINTS, "rewardPools")
        .iter()
        .map(|p| js_string(p.get("name")))
        .collect();
    let new_pools = list(ctx.after, F::BLUEPRINTS, "rewardPools")
        .iter()
        .filter(|p| !pools_a.contains(&js_string(p.get("name"))))
        .count();
    let tracked = matches!(
        ctx.before
            .get(F::BLUEPRINTS)
            .and_then(|b| b.get("rewardPools")),
        Some(Value::Array(_))
    );
    if tracked && new_pools > 0 {
        notes.push(format!(
            "{new_pools} new mission reward pool{}",
            if new_pools == 1 { "" } else { "s" }
        ));
    }
    section(SectionId::Crafting, "Crafting", lines, notes)
}

fn mission_field(field: &str) -> bool {
    matches!(
        field,
        "reward" | "rewardMax" | "buyIn" | "lawful" | "difficulty" | "type" | "currency"
    )
}

fn missions(ctx: &mut Context<'_>) -> NoteSection {
    let mut lines = Vec::new();
    let mut notes = Vec::new();
    if ctx.before.has(F::MISSIONS) && ctx.after.has(F::MISSIONS) {
        let s = split(
            &list(ctx.before, F::MISSIONS, "missions"),
            &list(ctx.after, F::MISSIONS, "missions"),
        );
        let mut unreleased = 0;
        for (status, records) in [
            (ItemStatus::Added, &s.added),
            (ItemStatus::Removed, &s.removed),
        ] {
            let mut groups: OrderedMap<Vec<&Body>> = OrderedMap::new();
            for m in records {
                if m.get("notForRelease") == Some(&Value::Bool(true)) {
                    unreleased += 1;
                    continue;
                }
                let label = clean_title(m.get("title"))
                    .unwrap_or_else(|| short_record(&js_string(m.get("recordName"))).to_string());
                groups.entry_or(&label, Vec::new).push(*m);
            }
            // Default sort of [label, records] pairs: by their string form.
            let mut entries: Vec<(&String, &Vec<&Body>, String)> = groups
                .iter()
                .map(|(k, g)| {
                    (
                        k,
                        g,
                        format!("{k},{}", vec!["[object Object]"; g.len()].join(",")),
                    )
                })
                .collect();
            entries.sort_by(|a, b| cmp_utf16(&a.2, &b.2));
            for (label, group, _) in entries {
                let mut rewards: Vec<f64> = Vec::new();
                for m in group {
                    if let Some(r) = m.get("reward").and_then(Value::as_f64)
                        && !rewards.contains(&r)
                    {
                        rewards.push(r);
                    }
                }
                rewards.sort_by(|x, y| x.partial_cmp(y).unwrap_or(Ordering::Equal));
                let mut line = NoteLine::new(
                    label.clone(),
                    status,
                    Some("Missions".into()),
                    sorted(
                        group
                            .iter()
                            .map(|m| short_record(&js_string(m.get("recordName"))).to_string())
                            .collect(),
                    ),
                );
                let mut parts = Vec::new();
                if group.len() > 1 {
                    parts.push(format!("{} contracts", group.len()));
                }
                if let (Some(first), Some(last)) = (rewards.first(), rewards.last()) {
                    let currency = nullish(group[0], &["currency"])
                        .map_or_else(|| "aUEC".to_string(), |v| js_string(Some(v)));
                    parts.push(format!(
                        "{}{} {currency}",
                        fmt_number(*first),
                        if rewards.len() > 1 {
                            format!("–{}", fmt_number(*last))
                        } else {
                            String::new()
                        }
                    ));
                }
                if !parts.is_empty() {
                    line.detail = Some(parts.join(", "));
                }
                lines.push(line);
            }
        }
        if unreleased > 0 {
            notes.push(format!(
                "{unreleased} unreleased (not-for-release) mission record{} added or removed",
                if unreleased == 1 { "" } else { "s" }
            ));
        }
        let mut changed = Vec::new();
        for (a, b) in &s.both {
            let record = js_string(b.get("recordName"));
            let title = clean_title(b.get("title"));
            let changes = field_changes(
                diff_entry_bodies(a, b),
                ctx.min_change,
                &mut ctx.counters,
                Some(&mission_field),
            );
            changed.push(ChangedRecord {
                label: title
                    .clone()
                    .unwrap_or_else(|| short_record(&record).to_string()),
                key: title.unwrap_or_else(|| record.clone()),
                record,
                category: Some("Missions".into()),
                changes,
            });
        }
        lines.extend(group_changed(changed));
    }
    if ctx.before.has(F::WIKELO) && ctx.after.has(F::WIKELO) {
        let s = split(
            &list(ctx.before, F::WIKELO, "trades"),
            &list(ctx.after, F::WIKELO, "trades"),
        );
        let label = |t: &Body| match t.get("rewardInferred") {
            Some(Value::String(r)) => r.clone(),
            _ => short_record(&js_string(t.get("recordName"))).to_string(),
        };
        let short = |t: &Body| short_record(&js_string(t.get("recordName"))).to_string();
        for (status, records) in [
            (ItemStatus::Added, &s.added),
            (ItemStatus::Removed, &s.removed),
        ] {
            for t in records {
                lines.push(NoteLine::new(
                    label(t),
                    status,
                    Some("Wikelo".into()),
                    vec![short(t)],
                ));
            }
        }
        let hand_ins = |t: &Body| {
            sorted(
                records_in(t, "handIns")
                    .iter()
                    .map(|h| {
                        format!(
                            "{}x{}",
                            js_string(h.get("item")),
                            js_string(h.get("amount"))
                        )
                    })
                    .collect(),
            )
        };
        for (a, b) in &s.both {
            if hand_ins(a) != hand_ins(b) {
                let mut line = NoteLine::new(
                    label(b),
                    ItemStatus::Changed,
                    Some("Wikelo".into()),
                    vec![short(b)],
                );
                line.detail = Some("hand-ins changed".into());
                lines.push(line);
            }
        }
    }
    section(SectionId::Missions, "Missions & Wikelo", lines, notes)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LootState {
    Lootable,
    NotLootable,
    Unset,
}

fn loot_state(tags: Option<&Value>) -> LootState {
    let list: Vec<&str> = match tags {
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    };
    let has = |root: &str| {
        list.iter()
            .any(|t| *t == root || t.strip_prefix(root).is_some_and(|r| r.starts_with('/')))
    };
    if has("LootGeneration/CannotGenerateAsLoot") {
        LootState::NotLootable
    } else if has("LootGeneration/CanGenerateAsLoot") {
        LootState::Lootable
    } else {
        LootState::Unset
    }
}

/// `fixedPlacements`: items a loot area always holds, item -> areas.
fn fixed_placements(data: View<'_>) -> OrderedMap<OrderedSet> {
    let mut out: OrderedMap<OrderedSet> = OrderedMap::new();
    let mut add = |item: &str, area: &str| {
        out.entry_or(&item.to_lowercase(), OrderedSet::new)
            .add(area);
    };
    for area in list(data, F::SOURCES, "areas") {
        let name = js_string(area.get("name"));
        for config in records_in(area, "configs") {
            for slot in records_in(config, "slots") {
                if js_truthy(slot.get("loot")) {
                    continue;
                }
                match slot.get("contains") {
                    Some(Value::Array(c)) if !c.is_empty() => {
                        for item in c {
                            add(&js_string(Some(item)), &name);
                        }
                    }
                    _ => {
                        if !js_truthy(slot.get("preset"))
                            && let Some(Value::String(e)) = slot.get("entity")
                        {
                            add(e, &name);
                        }
                    }
                }
            }
        }
    }
    out
}

/// A loot line's detail from its group of classes.
type DetailOf<'a> = dyn Fn(&[String]) -> String + 'a;

fn loot(ctx: &mut Context<'_>) -> NoteSection {
    let mut lines: Vec<NoteLine> = Vec::new();
    let mut notes = Vec::new();
    let tags_both = ctx.before.has(F::TAGS) && ctx.after.has(F::TAGS);
    let sources_both = ctx.before.has(F::SOURCES) && ctx.after.has(F::SOURCES);
    if (ctx.after.has(F::TAGS) && !ctx.before.has(F::TAGS))
        || (ctx.after.has(F::SOURCES) && !ctx.before.has(F::SOURCES))
    {
        notes.push("first build with loot data; nothing to compare against".into());
    }
    let emit = |ctx: &Context<'_>,
                lines: &mut Vec<NoteLine>,
                classes: &[String],
                status: ItemStatus,
                category: &str,
                detail_of: Option<&DetailOf<'_>>| {
        let label = |cls: &str| {
            ctx.names
                .get(&cls.to_lowercase())
                .cloned()
                .unwrap_or_else(|| cls.to_string())
        };
        let mut groups: OrderedMap<Vec<String>> = OrderedMap::new();
        for cls in classes {
            let key = match family_of(ctx, cls) {
                Some(f) => format!("family:{f}"),
                None => base_class(cls),
            };
            groups.entry_or(&key, Vec::new).push(cls.clone());
        }
        for group in groups.values() {
            let named: Vec<String> = group
                .iter()
                .map(|c| (label(c), c))
                .filter(|(n, c)| n != *c)
                .map(|(n, _)| n)
                .collect();
            let base = if named.is_empty() {
                group[0].clone()
            } else {
                most_common(&named.iter().map(|n| split_base(n)).collect::<Vec<_>>())
            };
            let line_label = if named.len() == 1 && group.len() == 1 {
                named[0].clone()
            } else {
                base.clone()
            };
            let mut line = NoteLine::new(
                line_label,
                status,
                Some(category.to_string()),
                sorted(group.clone()),
            );
            if group.len() > 1 {
                let variants = sorted(dedup(
                    named
                        .iter()
                        .map(|n| split_variant(n).1.unwrap_or_else(|| n.clone()))
                        .filter(|v| *v != base),
                ));
                if !variants.is_empty() {
                    line.variants = Some(variants);
                }
            }
            if let Some(d) = detail_of.map(|f| f(group)).filter(|d| !d.is_empty()) {
                line.detail = Some(d);
            }
            lines.push(line);
        }
    };

    if tags_both {
        let mut before: HashMap<String, &Body> = HashMap::new();
        for i in list(ctx.before, F::TAGS, "items") {
            before.insert(js_string(i.get("className")), i);
        }
        let (mut lootable, mut not_lootable, mut unset) = (Vec::new(), Vec::new(), Vec::new());
        let mut folded_templates = 0;
        for item in list(ctx.after, F::TAGS, "items") {
            let cls = js_string(item.get("className"));
            let Some(prior) = before.get(&cls) else {
                continue;
            };
            let was = loot_state(prior.get("tags"));
            let now = loot_state(item.get("tags"));
            if was == now {
                continue;
            }
            if item.get("isTemplate") == Some(&Value::Bool(true))
                || !is_real_name(item.get("name"), Some(&cls))
            {
                folded_templates += 1;
                continue;
            }
            match now {
                LootState::Lootable => lootable.push(cls),
                LootState::NotLootable => not_lootable.push(cls),
                LootState::Unset => unset.push(cls),
            }
        }
        emit(
            ctx,
            &mut lines,
            &lootable,
            ItemStatus::Changed,
            "Now lootable",
            None,
        );
        emit(
            ctx,
            &mut lines,
            &not_lootable,
            ItemStatus::Changed,
            "No longer lootable",
            None,
        );
        emit(
            ctx,
            &mut lines,
            &unset,
            ItemStatus::Changed,
            "Loot flag removed",
            None,
        );
        notes.extend(folded_note(
            folded_templates,
            "templates with a changed loot flag",
        ));
    }

    if sources_both {
        let a = fixed_placements(ctx.before);
        let b = fixed_placements(ctx.after);
        let areas = |map: &OrderedMap<OrderedSet>, group: &[String]| {
            let all: OrderedSet = group
                .iter()
                .flat_map(|cls| {
                    map.get(&cls.to_lowercase())
                        .map(OrderedSet::to_vec)
                        .unwrap_or_default()
                })
                .collect();
            format!(
                "{} area{}: {}",
                all.len(),
                if all.len() == 1 { "" } else { "s" },
                list_some(&sorted(all.to_vec()), 2)
            )
        };
        let new_keys: Vec<String> = b.keys().filter(|k| !a.contains(k)).cloned().collect();
        emit(
            ctx,
            &mut lines,
            &new_keys,
            ItemStatus::Added,
            "Now placed in the world",
            Some(&|g| areas(&b, g)),
        );
        let gone_keys: Vec<String> = a.keys().filter(|k| !b.contains(k)).cloned().collect();
        emit(
            ctx,
            &mut lines,
            &gone_keys,
            ItemStatus::Removed,
            "No longer placed in the world",
            Some(&|g| areas(&a, g)),
        );

        let awards = |data: View<'_>| {
            let mut out: OrderedMap<OrderedSet> = OrderedMap::new();
            for award in list(data, F::SOURCES, "contractAwards") {
                out.entry_or(&js_string(award.get("item")), OrderedSet::new)
                    .add(&js_string(award.get("contract")));
            }
            out
        };
        let wa = awards(ctx.before);
        let wb = awards(ctx.after);
        let delta = |x: &OrderedMap<OrderedSet>,
                     y: &OrderedMap<OrderedSet>|
         -> Vec<(String, Vec<String>)> {
            x.iter()
                .map(|(item, contracts)| {
                    let gone: Vec<String> = contracts
                        .iter()
                        .filter(|c| !y.get(item).is_some_and(|s| s.has(c)))
                        .cloned()
                        .collect();
                    (item.clone(), gone)
                })
                .filter(|(_, c)| !c.is_empty())
                .collect()
        };
        for (item, contracts) in delta(&wb, &wa) {
            let d = format!("awarded by {}", list_some(&sorted(contracts), 3));
            emit(
                ctx,
                &mut lines,
                &[item],
                ItemStatus::Added,
                "Contract awards",
                Some(&|_| d.clone()),
            );
        }
        for (item, contracts) in delta(&wa, &wb) {
            let d = format!("no longer awarded by {}", list_some(&sorted(contracts), 3));
            emit(
                ctx,
                &mut lines,
                &[item],
                ItemStatus::Removed,
                "Contract awards",
                Some(&|_| d.clone()),
            );
        }
    }
    section(SectionId::Loot, "Loot & where to find things", lines, notes)
}

fn renames(ctx: &mut Context<'_>) -> NoteSection {
    let mut pairs: Vec<(String, String, String)> = ctx
        .renames
        .iter()
        .map(|r| (r.from.clone(), r.to.clone(), r.record.clone()))
        .collect();
    let sa = ctx.before.get(F::STRINGS).and_then(|b| b.get("strings"));
    let sb = ctx.after.get(F::STRINGS).and_then(|b| b.get("strings"));
    if let (Some(Value::Object(a)), Some(Value::Object(b))) = (sa, sb) {
        for (key, value) in js_entries(b) {
            let item_name = key
                .as_bytes()
                .get(..9)
                .is_some_and(|p| p.eq_ignore_ascii_case(b"item_name"));
            if !item_name {
                continue;
            }
            let prior = a.get(key);
            if let (Some(p), Some(v)) = (real_name(prior, None), real_name(Some(value), None))
                && p != v
            {
                pairs.push((p.to_string(), v.to_string(), key.clone()));
            }
        }
    }
    struct Group {
        from: String,
        to: String,
        records: OrderedSet,
    }
    let mut groups: OrderedMap<Group> = OrderedMap::new();
    for (from, to, record) in &pairs {
        let from = strip_slot(from);
        let to = strip_slot(to);
        let key = format!("{from}\u{0}{to}");
        groups
            .entry_or(&key, || Group {
                from,
                to,
                records: OrderedSet::new(),
            })
            .records
            .add(short_record(record));
    }
    let mut entries: Vec<(&String, &Group)> = groups.iter().collect();
    entries.sort_by(|x, y| locale_compare(&x.1.from, &y.1.from));
    let mut lines = Vec::new();
    let mut done: HashSet<String> = HashSet::new();
    for (key, g) in entries {
        if done.contains(key) {
            continue;
        }
        let reverse_key = format!("{}\u{0}{}", g.to, g.from);
        let reverse = groups.get(&reverse_key);
        let records: OrderedSet = g
            .records
            .iter()
            .chain(reverse.into_iter().flat_map(|r| r.records.iter()))
            .collect();
        if reverse.is_some() {
            done.insert(reverse_key);
        }
        let mut line = NoteLine::new(
            g.to.clone(),
            ItemStatus::Changed,
            None,
            sorted(records.to_vec()),
        );
        line.changes = Some(vec![NoteChange {
            stat: "name".into(),
            before: Some(Value::String(g.from.clone())),
            after: Some(Value::String(g.to.clone())),
            percent: None,
            direction: Direction::Changed,
        }]);
        if reverse.is_some() {
            line.detail = Some("swapped: each now carries the other name".into());
        }
        lines.push(line);
    }
    section(SectionId::Renames, "Renames", lines, Vec::new())
}

fn other(ctx: &mut Context<'_>, files: &[String], extra_notes: Vec<String>) -> NoteSection {
    let mut lines = Vec::new();
    let mut notes = extra_notes;
    for file in files {
        let dataset = dataset_name(file);
        let (Some(a), Some(b)) = (ctx.before.get(file), ctx.after.get(file)) else {
            let side = if ctx.before.get(file).is_some() {
                "older"
            } else {
                "newer"
            };
            notes.push(format!("{dataset}: only in the {side} build"));
            continue;
        };
        let itemised = itemised(file);
        if itemised == Some(None) {
            continue;
        }
        let mut parts = Vec::new();
        let mut counted = Vec::new();
        let collections: OrderedSet = diffable_collections(a)
            .into_iter()
            .chain(diffable_collections(b))
            .collect();
        for collection in collections.iter() {
            if itemised
                .flatten()
                .is_some_and(|l| l.contains(&collection.as_str()))
            {
                continue;
            }
            if !a.contains_key(collection) {
                parts.push(format!("{} first tracked", humanise(collection)));
                counted.push(collection.clone());
                continue;
            }
            let options = crate::diff::DiffOptions {
                limit: Some(0),
                ..Default::default()
            };
            let d = diff_collection(
                collection,
                &entries_of(a.get(collection)),
                &entries_of(b.get(collection)),
                &options,
            );
            let mut s = d.summary;
            if file == F::COMPONENTS {
                s.added = 0;
                s.removed = 0;
            }
            if s.added + s.removed + s.changed == 0 {
                continue;
            }
            let mut bits = Vec::new();
            if s.added > 0 {
                bits.push(format!("+{}", s.added));
            }
            if s.removed > 0 {
                bits.push(format!("−{}", s.removed));
            }
            if s.changed > 0 {
                bits.push(format!("~{}", s.changed));
            }
            parts.push(format!("{} {}", humanise(collection), bits.join(" ")));
            counted.push(collection.clone());
        }
        if !parts.is_empty() {
            let mut line = NoteLine::new(
                dataset.to_string(),
                ItemStatus::Changed,
                Some("Datasets".into()),
                counted,
            );
            line.detail = Some(parts.join(", "));
            lines.push(line);
        }
    }
    section(SectionId::Other, "Other changes", lines, notes)
}

// ---------------------------------------------------------------------------
// Entry points
// ---------------------------------------------------------------------------

/// The build stamp of the first dataset that carries one.
fn stamp(data: &BuildData) -> Option<String> {
    data.files
        .iter()
        .find_map(|(_, b)| b.get("_build").and_then(Value::as_str).map(str::to_string))
}

/// `composeNotes`: notes from data already in memory. `changes` is the
/// [`crate::changes`] result for the same two builds. `from`/`to` default to
/// `{ source: "before"/"after", build: <stamp> }`.
pub fn compose_notes(
    before: &BuildData,
    after: &BuildData,
    changes: &PatchChanges,
    options: &NotesOptions,
    from: Option<NotesSide>,
    to: Option<NotesSide>,
) -> NotesDoc {
    let min_change = options.min_change;
    let mut other_notes = Vec::new();
    let aligned = match (before.get(F::STRINGS), after.get(F::STRINGS)) {
        (Some(sa), Some(sb)) => {
            let a = align_redaction(sa, sb);
            a.note.is_some().then_some(a)
        }
        _ => None,
    };
    let mut vb = View {
        data: before,
        strings: None,
    };
    let mut va = View {
        data: after,
        strings: None,
    };
    if let Some(a) = &aligned {
        vb.strings = Some(&a.before);
        va.strings = Some(&a.after);
        other_notes.extend(a.note.clone());
    }
    let mut ctx = Context {
        before: vb,
        after: va,
        changes,
        min_change,
        counters: Counters::default(),
        names: name_index(&[va, vb]),
        families: family_index(&[va, vb]),
        renames: Vec::new(),
        siblings: HashMap::new(),
    };
    let files = sorted(dedup(
        before.files().chain(after.files()).map(str::to_string),
    ));
    let mut built = vec![
        new_ships(&mut ctx),
        ship_balance(&mut ctx),
        new_ship_items(&mut ctx),
        new_personal_weapons(&mut ctx),
        personal_balance(&mut ctx),
        armour(&mut ctx),
        crafting(&mut ctx),
        missions(&mut ctx),
        loot(&mut ctx),
    ];
    for item in &changes.items {
        if let Some(prev) = &item.previous_name
            && is_real_str(prev)
            && is_real_str(&item.name)
        {
            ctx.renames.push(Rename {
                from: prev.clone(),
                to: item.name.clone(),
                record: item.record.clone(),
            });
        }
    }
    built.push(renames(&mut ctx));
    built.push(other(&mut ctx, &files, other_notes));
    NotesDoc {
        from: from.unwrap_or_else(|| NotesSide {
            source: "before".into(),
            build: stamp(before),
            fallback: None,
        }),
        to: to.unwrap_or_else(|| NotesSide {
            source: "after".into(),
            build: stamp(after),
            fallback: None,
        }),
        min_change,
        sections: built
            .into_iter()
            .filter(|s| !s.lines.is_empty() || !s.notes.is_empty())
            .collect(),
        hidden: Hidden {
            minor: ctx.counters.minor,
            newly_tracked: ctx.counters.newly_tracked,
            untracked: ctx.counters.untracked,
        },
    }
}

/// `loadBuildData`: every listed file a source holds, and where each was read from.
pub fn load_build_data(
    source: &BuildSource,
    files: &[String],
) -> Result<(BuildData, Vec<(String, String)>)> {
    let mut data = BuildData::new();
    let mut served = Vec::new();
    for file in files {
        if let Some(snap) = load_snapshot(source, file)? {
            data.insert(file.clone(), snap.body);
            served.push((file.clone(), snap.source));
        }
    }
    Ok((data, served))
}

/// `sourceLabel`: a side without its local path (`dir <name>`, or `tag <tag>`).
fn source_label(source: &BuildSource) -> String {
    match source {
        BuildSource::Dir { path, .. } => format!(
            "dir {}",
            path.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        ),
        tag => tag.describe(),
    }
}

fn side(source: &BuildSource, data: &BuildData, served: &[(String, String)]) -> NotesSide {
    let own = source.describe();
    let elsewhere: Vec<&(String, String)> =
        served.iter().filter(|(_, from)| *from != own).collect();
    NotesSide {
        source: source_label(source),
        build: stamp(data),
        fallback: elsewhere.first().map(|first| Fallback {
            source: first.1.clone(),
            files: sorted(elsewhere.iter().map(|(f, _)| f.clone()).collect()),
        }),
    }
}

/// `buildNotes`: patch notes between two builds, read from their datasets.
pub fn notes(from: &BuildSource, to: &BuildSource, options: &NotesOptions) -> Result<NotesDoc> {
    let files = list_dataset_files(from, to);
    let (before, served_from) = load_build_data(from, &files)?;
    let (after, served_to) = load_build_data(to, &files)?;
    let changes = crate::changes::patch_changes_from(&|f| before.get(f), &|f| after.get(f));
    Ok(compose_notes(
        &before,
        &after,
        &changes,
        options,
        Some(side(from, &before, &served_from)),
        Some(side(to, &after, &served_to)),
    ))
}

/// [`notes`] with the newer side already in memory: `after` holds the datasets as a
/// directory named `after_name` would (publish passes the redacted dumps, named after
/// their build, so the side reads `dir <build>`). Derived files in `after` are skipped,
/// as a directory listing would skip them.
pub fn notes_against(
    from: &BuildSource,
    after: &[(String, Body)],
    after_name: &str,
    options: &NotesOptions,
) -> Result<NotesDoc> {
    let mut names: Vec<String> = list_dataset_files(from, from);
    for (file, _) in after {
        if file.starts_with("game-")
            && file.ends_with(".json")
            && !crate::diff::DERIVED_FILES.contains(&file.as_str())
            && !names.contains(file)
        {
            names.push(file.clone());
        }
    }
    sort_utf16(&mut names);
    let (before, served_from) = load_build_data(from, &names)?;
    let mut newer = BuildData::new();
    for file in &names {
        if let Some((_, body)) = after.iter().find(|(f, _)| f == file) {
            newer.insert(file.clone(), body.clone());
        }
    }
    let changes = crate::changes::patch_changes_from(&|f| before.get(f), &|f| newer.get(f));
    let to = NotesSide {
        source: format!("dir {after_name}"),
        build: stamp(&newer),
        fallback: None,
    };
    Ok(compose_notes(
        &before,
        &newer,
        &changes,
        options,
        Some(side(from, &before, &served_from)),
        Some(to),
    ))
}

/// `notes --json`: `JSON.stringify(doc, null, 2)` (no trailing newline).
pub fn notes_json(doc: &NotesDoc) -> String {
    to_js_json(&serde_json::to_value(doc).unwrap_or(Value::Null))
}

// ---------------------------------------------------------------------------
// Markdown
// ---------------------------------------------------------------------------

/// `fmtNumber`: `toLocaleString("en-US")` with 0, 2 or 3 fraction digits by magnitude.
pub fn fmt_number(value: f64) -> String {
    if value.is_nan() {
        return "NaN".into();
    }
    let neg = value.is_sign_negative();
    if value.is_infinite() {
        return if neg { "-∞".into() } else { "∞".into() };
    }
    let abs = value.abs();
    let places = if abs >= 100.0 {
        0
    } else if abs >= 1.0 {
        2
    } else {
        3
    };
    // Shortest round-trip digits, as ICU formats a double.
    let (mut digits, point): (Vec<u8>, i32) = if abs == 0.0 {
        (vec![0], 1)
    } else {
        let sci = format!("{abs:e}");
        let (m, e) = sci.split_once('e').unwrap_or((&sci, "0"));
        (
            m.bytes()
                .filter(u8::is_ascii_digit)
                .map(|b| b - b'0')
                .collect(),
            e.parse::<i32>().unwrap_or(0) + 1,
        )
    };
    // Integer digits and fraction digits.
    let (mut int, mut frac): (Vec<u8>, Vec<u8>) = if point <= 0 {
        let mut f = vec![0; (-point) as usize];
        f.append(&mut digits);
        (vec![0], f)
    } else if point as usize >= digits.len() {
        digits.resize(point as usize, 0);
        (digits, Vec::new())
    } else {
        let f = digits.split_off(point as usize);
        (digits, f)
    };
    if frac.len() > places {
        let up = frac[places] >= 5;
        frac.truncate(places);
        if up {
            let mut carry = true;
            for d in frac.iter_mut().rev().chain(int.iter_mut().rev()) {
                if !carry {
                    break;
                }
                if *d == 9 {
                    *d = 0;
                } else {
                    *d += 1;
                    carry = false;
                }
            }
            if carry {
                int.insert(0, 1);
            }
        }
    }
    while frac.last() == Some(&0) {
        frac.pop();
    }
    let start = int.iter().position(|d| *d != 0).unwrap_or(int.len() - 1);
    let int = &int[start..];
    let mut out = String::new();
    if neg {
        out.push('-');
    }
    for (i, d) in int.iter().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push((b'0' + d) as char);
    }
    if !frac.is_empty() {
        out.push('.');
        out.extend(frac.iter().map(|d| (b'0' + d) as char));
    }
    out
}

fn fmt_value(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => "–".into(),
        Some(Value::Number(n)) => fmt_number(n.as_f64().unwrap_or(f64::NAN)),
        Some(Value::String(s)) => format!("\"{s}\""),
        v => js_string(v),
    }
}

fn fmt_change(c: &NoteChange) -> String {
    let mut pct = String::new();
    if let Some(p) = c.percent {
        let ratio = 1.0 + p / 100.0;
        pct = if p.abs() >= 1000.0 {
            format!(" (×{})", fmt_number(ratio))
        } else {
            format!(
                " ({}{}%)",
                if p > 0.0 { "+" } else { "−" },
                fmt_number(crate::js::round_to(p, 10.0).abs())
            )
        };
    }
    format!(
        "{} {} → {}{pct}",
        c.stat,
        fmt_value(c.before.as_ref()),
        fmt_value(c.after.as_ref())
    )
}

fn verdict(changes: &[NoteChange]) -> &'static str {
    let buff = changes.iter().any(|c| c.direction == Direction::Buff);
    let nerf = changes.iter().any(|c| c.direction == Direction::Nerf);
    match (buff, nerf) {
        (false, false) => "",
        (true, true) => " (mixed)",
        (true, false) => " (buff)",
        (false, true) => " (nerf)",
    }
}

const MAX_CHANGES: usize = 6;

fn render_line(line: &NoteLine) -> String {
    let mut text = format!("**{}**", line.label);
    let n = line.records.len();
    let detail = line.detail.as_deref();
    if let Some(v) = line.variants.as_ref().filter(|v| !v.is_empty()) {
        text += &format!(
            " (+{} variant{}: {})",
            v.len(),
            if v.len() == 1 { "" } else { "s" },
            list_some(v, 5)
        );
    } else if n > 1
        && line.status == ItemStatus::Changed
        && line.changes.is_some()
        && line.category.is_some()
    {
        text += &format!(" (+{} variant{})", n - 1, if n == 2 { "" } else { "s" });
    } else if n > 1
        && line.status != ItemStatus::Changed
        && !detail.is_some_and(|d| d.contains("pieces"))
        && !detail.is_some_and(|d| d.starts_with("for "))
    {
        text += &format!(" ×{n}");
    }
    if line.status == ItemStatus::Removed {
        text = format!("Removed: {text}");
    }
    if let Some(changes) = line.changes.as_ref().filter(|c| !c.is_empty()) {
        if changes.len() == 1 && changes[0].stat == "name" {
            let before = fmt_value(changes[0].before.as_ref());
            let before = before.strip_prefix('"').unwrap_or(&before);
            let before = before.strip_suffix('"').unwrap_or(before);
            text = format!("{before} → **{}**", line.label);
        } else {
            let shown: Vec<String> = changes.iter().take(MAX_CHANGES).map(fmt_change).collect();
            let more = if changes.len() > MAX_CHANGES {
                format!("; +{} more", changes.len() - MAX_CHANGES)
            } else {
                String::new()
            };
            text += &format!("{}: {}{more}", verdict(changes), shown.join("; "));
        }
    }
    if let Some(d) = detail.filter(|d| !d.is_empty()) {
        text += &format!(" — {d}");
    }
    format!("- {text}")
}

fn status_order(s: ItemStatus) -> u8 {
    match s {
        ItemStatus::Added => 0,
        ItemStatus::Changed => 1,
        ItemStatus::Removed => 2,
    }
}

/// `renderNotes`: Markdown for people (no trailing newline; the CLI prints one).
pub fn render_notes(doc: &NotesDoc) -> String {
    let mut out: Vec<String> = Vec::new();
    let name = |s: &NotesSide| s.build.clone().unwrap_or_else(|| s.source.clone());
    out.push(format!(
        "# Patch notes: {} → {}",
        name(&doc.from),
        name(&doc.to)
    ));
    out.push(String::new());
    for s in [&doc.from, &doc.to] {
        let Some(fb) = &s.fallback else { continue };
        let files: Vec<&str> = fb.files.iter().map(|f| dataset_name(f)).collect();
        out.push(format!(
            "_{}: {} read from {} (not in its snapshot)._",
            name(s),
            files.join(", "),
            fb.source
        ));
        out.push(String::new());
    }
    for s in &doc.sections {
        out.push(format!("## {}", s.title));
        out.push(String::new());
        let categories = dedup(
            s.lines
                .iter()
                .map(|l| l.category.clone().unwrap_or_default()),
        );
        let sub = categories.len() > 1
            || (categories.first().is_none_or(|c| !c.is_empty()) && s.id == SectionId::ShipBalance);
        for category in &categories {
            let mut lines: Vec<&NoteLine> = s
                .lines
                .iter()
                .filter(|l| l.category.as_deref().unwrap_or("") == category)
                .collect();
            lines.sort_by_key(|l| status_order(l.status));
            if sub && !category.is_empty() {
                out.push(format!("### {}", category_label(category)));
                out.push(String::new());
            }
            for line in lines {
                out.push(render_line(line));
            }
            out.push(String::new());
        }
        for note in &s.notes {
            out.push(format!("_{note}._"));
        }
        if !s.notes.is_empty() {
            out.push(String::new());
        }
    }
    if doc.sections.is_empty() {
        out.push("No player-facing changes.".into());
        out.push(String::new());
    }
    let mut hidden = Vec::new();
    let h = &doc.hidden;
    let plural = |n: usize| if n == 1 { "" } else { "s" };
    if h.minor > 0 {
        hidden.push(format!(
            "{} minor change{} under {}% hidden",
            h.minor,
            plural(h.minor),
            js_number(doc.min_change)
        ));
    }
    if h.untracked > 0 {
        hidden.push(format!(
            "{} record{} changed only outside the tracked stats",
            h.untracked,
            plural(h.untracked)
        ));
    }
    if h.newly_tracked > 0 {
        hidden.push(format!(
            "{} newly tracked field{} (no old value) hidden",
            h.newly_tracked,
            plural(h.newly_tracked)
        ));
    }
    if !hidden.is_empty() {
        out.push("---".into());
        out.push(String::new());
        out.push(format!("_{}._", hidden.join("; ")));
        out.push(String::new());
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A build directory of `game-<name>.json` fixtures, each stamped with `build`.
    fn write_build(
        root: &std::path::Path,
        name: &str,
        build: &str,
        datasets: Value,
    ) -> BuildSource {
        let dir = root.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        for (file, body) in datasets.as_object().unwrap() {
            let mut v = json!({"_build": build, "_extracted": build});
            for (k, x) in body.as_object().unwrap() {
                v[k] = x.clone();
            }
            std::fs::write(dir.join(format!("game-{file}.json")), v.to_string()).unwrap();
        }
        BuildSource::dir(dir)
    }

    fn ecd(cls: &str) -> String {
        format!("EntityClassDefinition.{cls}")
    }
    fn with(mut base: Value, extra: Value) -> Value {
        for (k, x) in extra.as_object().unwrap() {
            base[k] = x.clone();
        }
        base
    }
    fn ship(cls: &str, name: &str, extra: Value) -> Value {
        with(json!({"recordName": ecd(cls), "name": name}), extra)
    }
    fn gun(cls: &str, name: &str, extra: Value) -> Value {
        with(
            json!({"recordName": ecd(cls), "name": name, "size": 3}),
            extra,
        )
    }
    fn tags(cls: &str, name: &str, list: &[&str]) -> Value {
        json!({"className": cls, "name": name, "id": cls, "tags": list})
    }
    const SKARNOV: &[&str] = &[
        "Weapon/FPS/Stocked/Rifle/Skarnov",
        "Weapon/FPS/Stocked/Rifle",
        "Weapon/FPS/Stocked",
    ];

    fn before_after(root: &std::path::Path) -> (BuildSource, BuildSource) {
        let none = json!({});
        let rewards = |list: &[(&str, &str)]| -> Value {
            list.iter()
                .map(|(m, c)| json!({"mission": m, "contract": c}))
                .collect()
        };
        let before = write_build(
            root,
            "a",
            "1.0.1",
            json!({
                "ships": {"vehicles": [
                    ship("AEGS_Gladius", "Aegis Gladius", json!({"flight": {"scmSpeed": 200, "maxSpeed": 1000}, "totals": {"weaponDps": 1000}})),
                    ship("AEGS_Gladius_PU_AI_CIV", "Aegis Gladius", json!({"flight": {"scmSpeed": 200}})),
                ]},
                "weapons": {
                    "shipWeapons": [gun("BEHR_LaserCannon_S3", "M5A Cannon", json!({"dps": 500}))],
                    "fpsWeapons": [gun("behr_rifle_ballistic_01", "P4-AR Rifle", json!({"dps": 100}))],
                    "missiles": [],
                },
                "fps-weapons": {"weapons": [gun("behr_rifle_ballistic_01", "P4-AR Rifle", none.clone())]},
                "armor": {"armor": [
                    {"recordName": ecd("rsi_explorer_armor_light_arms_01_02_01"), "name": "Venture Arms Starwalker", "mass": 5},
                    {"recordName": ecd("rsi_explorer_armor_light_arms_01_03_01"), "name": "Venture Arms Infrared", "mass": 5},
                ], "clothing": []},
                "blueprints": {"blueprints": [
                    {"recordName": "CraftingBlueprintRecord.BP_CRAFT_volt_pistol_energy_01", "name": "Pulse Laser Pistol", "produces": "volt_pistol_energy_01",
                     "tiers": [{"craftTimeSeconds": 100}], "rewardedBy": rewards(&[("Recover Vanduul Tech", "IDS_Tech"), ("Operative Extraction", "InterSec_Nyx")])},
                    {"recordName": "CraftingBlueprintRecord.BP_CRAFT_volt_lmg_energy_01", "name": "Fresnel LMG", "produces": "volt_lmg_energy_01",
                     "tiers": [{"craftTimeSeconds": 300}], "rewardedBy": rewards(&[("Recover Vanduul Tech", "IDS_Tech"), ("Operative Extraction", "InterSec_Nyx")])},
                ], "rewardPools": []},
                "ammo": {"ammo": [{"recordName": "AmmoParams.a", "speed": 1}]},
            }),
        );
        let after = write_build(
            root,
            "b",
            "1.0.2",
            json!({
                "ships": {"vehicles": [
                    ship("AEGS_Gladius", "Aegis Gladius", json!({"flight": {"scmSpeed": 240, "maxSpeed": 1005}, "totals": {"weaponDps": 1000, "armorHealth": 900}})),
                    ship("AEGS_Gladius_PU_AI_CIV", "Aegis Gladius", json!({"flight": {"scmSpeed": 240}})),
                    ship("RSI_Galaxy", "RSI Galaxy", none.clone()),
                    ship("RSI_Galaxy_PU_AI_CRIM", "RSI Galaxy", none.clone()),
                    ship("RSI_Galaxy_AI_Template", "RSI Galaxy", json!({"isTemplate": true})),
                ]},
                "weapons": {
                    "shipWeapons": [gun("BEHR_LaserCannon_S3", "M5A Cannon", json!({"dps": 400})), gun("BEHR_LaserCannon_S3_mr02", "M5A Cannon Raider Camo", json!({"dps": 400}))],
                    "fpsWeapons": [
                        gun("behr_rifle_ballistic_01", "P4-AR Rifle", json!({"dps": 100})),
                        gun("lbco_rifle_energy_01", "Skarnov Rifle", none.clone()),
                        gun("lbco_rifle_energy_01_tint01", "Skarnov \"Ferrite\" Rifle", none.clone()),
                        gun("lbco_rifle_energy_01_mr01", "Skarnov \"Raider Camo\" Rifle", none.clone()),
                        gun("lbco_rifle_energy_01_store01_prop", "lbco_rifle_energy_01_store01_prop", json!({"isTemplate": true})),
                    ],
                    "missiles": [],
                },
                "fps-weapons": {"weapons": [
                    gun("behr_rifle_ballistic_01", "P4-AR Rifle", none.clone()),
                    gun("lbco_rifle_energy_01", "Skarnov Rifle", none.clone()),
                    gun("lbco_rifle_energy_01_tint01", "Skarnov \"Ferrite\" Rifle", none.clone()),
                    gun("banu_melee_05", "Kamba Knife", json!({"kind": "melee"})),
                ]},
                "armor": {"armor": [
                    {"recordName": ecd("rsi_explorer_armor_light_arms_01_02_01"), "name": "Venture Arms Infrared", "mass": 5},
                    {"recordName": ecd("rsi_explorer_armor_light_arms_01_03_01"), "name": "Venture Arms Starwalker", "mass": 5},
                    {"recordName": ecd("kap_combat_light_arms_02_03_01"), "name": "Geist Armor Arms Epoque II"},
                    {"recordName": ecd("kap_combat_light_core_02_03_01"), "name": "Geist Armor Core Epoque II"},
                    {"recordName": ecd("kap_combat_light_helmet_02_03_01"), "name": "Geist Armor Helmet Epoque"},
                ], "clothing": []},
                "blueprints": {"blueprints": [
                    {"recordName": "CraftingBlueprintRecord.BP_CRAFT_volt_pistol_energy_01", "name": "Pulse Laser Pistol", "produces": "volt_pistol_energy_01",
                     "tiers": [{"craftTimeSeconds": 100}], "rewardedBy": rewards(&[("Living Vanduul Sample", "IDS_CryoP_Blue"), ("Operative Extraction", "InterSec_Nyx"), ("Recover Vanduul Tech", "IDS_Tech")])},
                    {"recordName": "CraftingBlueprintRecord.BP_CRAFT_volt_lmg_energy_01", "name": "Fresnel LMG", "produces": "volt_lmg_energy_01",
                     "tiers": [{"craftTimeSeconds": 300}], "rewardedBy": rewards(&[("Operative Extraction", "InterSec_Nyx"), ("Recover Vanduul Tech", "IDS_Tech")])},
                ], "rewardPools": [{"name": "BP_MISSIONREWARD_New", "blueprints": []}]},
                "ammo": {"ammo": [{"recordName": "AmmoParams.a", "speed": 1}, {"recordName": "AmmoParams.b", "speed": 2}]},
                "item-tags": {"items": [
                    tags("lbco_rifle_energy_01", "Skarnov Rifle", SKARNOV),
                    tags("lbco_rifle_energy_01_tint01", "Skarnov \"Ferrite\" Rifle", SKARNOV),
                    tags("lbco_rifle_energy_01_mr01", "Skarnov \"Raider Camo\" Rifle", SKARNOV),
                ]},
            }),
        );
        (before, after)
    }

    fn build(from: &BuildSource, to: &BuildSource, min_change: f64) -> NotesDoc {
        notes(from, to, &NotesOptions { min_change }).unwrap()
    }

    fn section_of(doc: &NotesDoc, id: SectionId) -> Option<&NoteSection> {
        doc.sections.iter().find(|s| s.id == id)
    }

    fn labels(s: Option<&NoteSection>) -> Vec<String> {
        s.map(|s| s.lines.iter().map(|l| l.label.clone()).collect())
            .unwrap_or_default()
    }

    #[test]
    fn naming_helpers() {
        assert_eq!(
            base_class("EntityClassDefinition.ksar_lmg_energy_01_store01_prop"),
            "ksar_lmg_energy_01"
        );
        assert_eq!(
            base_class("lbco_rifle_energy_01_tint03"),
            "lbco_rifle_energy_01"
        );
        assert_eq!(
            base_class("BEHR_LaserCannon_S1_mr02"),
            "behr_lasercannon_s1"
        );
        assert_eq!(
            split_variant("Verdict \"Derivation\" LMG"),
            ("Verdict LMG".into(), Some("Derivation".into()))
        );
        assert_eq!(
            split_variant("Vanduul Mauler 'STRIKER' Repeater"),
            ("Vanduul Mauler 'STRIKER' Repeater".into(), None)
        );
        assert_eq!(
            armour_set_key(&ecd("kap_combat_light_arms_02_03_01")),
            armour_set_key("kap_combat_light_helmet_02_03_01")
        );
        assert_eq!(
            strip_slot("Geist Armor  Arms Epoque II"),
            "Geist Armor Epoque II"
        );
        assert_eq!(fmt_number(1234567.5), "1,234,568");
        assert_eq!(fmt_number(1.005), "1.01");
        assert_eq!(fmt_number(-0.0001), "-0");
        assert_eq!(fmt_number(99.995), "100");
        assert_eq!(fmt_number(0.00049), "0");
        assert_eq!(
            fmt_number(12345678901234567890.0),
            "12,345,678,901,234,567,000"
        );
    }

    #[test]
    fn build_notes() {
        let tmp = tempfile::tempdir().unwrap();
        let (before, after) = before_after(tmp.path());
        let doc = build(&before, &after, 1.0);

        // Player ships once; AI and template hulls folded.
        let s = section_of(&doc, SectionId::NewShips);
        assert_eq!(labels(s), vec!["RSI Galaxy"]);
        assert_eq!(
            s.unwrap().notes,
            vec!["+2 AI/NPC/template variants not listed"]
        );

        // Buffs and nerfs over the threshold; the rest counted.
        let lines = &section_of(&doc, SectionId::ShipBalance).unwrap().lines;
        let gladius = lines.iter().find(|l| l.label == "Aegis Gladius").unwrap();
        assert_eq!(
            gladius.changes.as_deref(),
            Some(
                &[NoteChange {
                    stat: "scmSpeed".into(),
                    before: Some(json!(200.0)),
                    after: Some(json!(240.0)),
                    percent: Some(20.0),
                    direction: Direction::Buff
                }][..]
            )
        );
        let cannon = lines.iter().find(|l| l.label == "M5A Cannon").unwrap();
        let c = &cannon.changes.as_ref().unwrap()[0];
        assert_eq!(
            (c.stat.as_str(), c.percent, c.direction),
            ("dps", Some(-20.0), Direction::Nerf)
        );
        assert!(doc.hidden.minor >= 1);
        assert!(doc.hidden.newly_tracked >= 1);
        assert!(build(&before, &after, 0.1).hidden.minor < doc.hidden.minor);

        // Weapon variants collapse by family; templates dropped; each weapon once.
        let s = section_of(&doc, SectionId::NewPersonalWeapons).unwrap();
        let mut l = labels(Some(s));
        l.sort();
        assert_eq!(l, vec!["Kamba Knife", "Skarnov Rifle"]);
        let skarnov = s.lines.iter().find(|l| l.label == "Skarnov Rifle").unwrap();
        assert_eq!(
            skarnov.variants.as_deref(),
            Some(&["Ferrite".to_string(), "Raider Camo".to_string()][..])
        );
        assert_eq!(skarnov.records.len(), 3);
        assert_eq!(s.notes, vec!["+1 unnamed templates and props not listed"]);
        let all: Vec<&String> = doc
            .sections
            .iter()
            .flat_map(|x| x.lines.iter().flat_map(|l| l.records.iter()))
            .collect();
        assert_eq!(
            all.iter().filter(|r| **r == "lbco_rifle_energy_01").count(),
            1
        );

        // Armour pieces group into a set.
        let geist = section_of(&doc, SectionId::Armour)
            .unwrap()
            .lines
            .iter()
            .find(|l| l.status == ItemStatus::Added)
            .unwrap();
        assert_eq!(geist.label, "Geist Armor Epoque II");
        assert_eq!(geist.detail.as_deref(), Some("pieces: arms, core, helmet"));

        // Blueprint rewards compare as sets.
        let crafting = section_of(&doc, SectionId::Crafting).unwrap();
        assert_eq!(crafting.lines.len(), 1);
        assert_eq!(crafting.lines[0].label, "Pulse Laser Pistol");
        assert_eq!(
            crafting.lines[0].detail.as_deref(),
            Some("now also from Living Vanduul Sample")
        );
        assert_eq!(crafting.notes, vec!["1 new mission reward pool"]);

        // Two records trading names read as one swap.
        let renames = &section_of(&doc, SectionId::Renames).unwrap().lines;
        assert_eq!(renames.len(), 1);
        assert_eq!(renames[0].records.len(), 2);
        assert!(renames[0].detail.as_deref().unwrap().contains("swapped"));

        // No loot data in the older build.
        assert!(
            section_of(&doc, SectionId::Loot).unwrap().notes[0]
                .contains("first build with loot data")
        );

        // Datasets without a section of their own are counted.
        let other = section_of(&doc, SectionId::Other).unwrap();
        assert_eq!(
            other
                .lines
                .iter()
                .find(|l| l.label == "ammo")
                .unwrap()
                .detail
                .as_deref(),
            Some("ammo +1")
        );
        assert!(
            other
                .notes
                .contains(&"item-tags: only in the newer build".to_string())
        );

        let md = render_notes(&doc);
        assert!(md.starts_with("# Patch notes: 1.0.1 → 1.0.2"));
        assert!(md.contains("## New ships"));
        assert!(md.contains("**Skarnov Rifle** (+2 variants: Ferrite, Raider Camo)"));
        assert!(md.contains("scmSpeed 200 → 240 (+20%)"));
        assert!(!md.contains("_build") && !md.contains("_extracted") && !md.contains("recordName"));
        assert!(md.contains(" under 1% hidden"));
    }

    #[test]
    fn loot_changes() {
        let tmp = tempfile::tempdir().unwrap();
        let loot_tags = |lootable: bool| {
            json!({"items": [
                tags("volt_rifle_energy_01", "Parallax Rifle", &[if lootable { "LootGeneration/CanGenerateAsLoot" } else { "LootGeneration/CannotGenerateAsLoot/InGameReward" }]),
                tags("cbd_hat_01", "Hat", &["LootGeneration/CanGenerateAsLoot"]),
            ]})
        };
        let area = |contains: &[&str]| json!({"areas": [{"name": "caves/loot_caves_occupied", "configs": [{"spawnTags": [], "slots": [{"entity": "rack_weapons", "weight": 1, "contains": contains}]}]}], "contractAwards": []});
        let a = write_build(
            tmp.path(),
            "loot-a",
            "2.0.1",
            json!({"item-tags": loot_tags(true), "item-sources": area(&["cbd_hat_01"])}),
        );
        let mut sources = area(&["volt_rifle_energy_01"]);
        sources["contractAwards"] =
            json!([{"generator": "g", "contract": "TheCollector_Rifle", "item": "cbd_hat_01"}]);
        let b = write_build(
            tmp.path(),
            "loot-b",
            "2.0.2",
            json!({"item-tags": loot_tags(false), "item-sources": sources}),
        );
        let doc = build(&a, &b, 1.0);
        let lines = &section_of(&doc, SectionId::Loot).unwrap().lines;
        let by = |category: &str| -> Vec<&str> {
            lines
                .iter()
                .filter(|l| l.category.as_deref() == Some(category))
                .map(|l| l.label.as_str())
                .collect()
        };
        assert_eq!(by("No longer lootable"), vec!["Parallax Rifle"]);
        assert_eq!(by("Now placed in the world"), vec!["Parallax Rifle"]);
        assert_eq!(by("No longer placed in the world"), vec!["Hat"]);
        let award = lines
            .iter()
            .find(|l| l.category.as_deref() == Some("Contract awards"))
            .unwrap();
        assert_eq!(
            award.detail.as_deref(),
            Some("awarded by TheCollector_Rifle")
        );
    }

    #[test]
    fn review_fixes() {
        let tmp = tempfile::tempdir().unwrap();
        let radar = |mass: u32| json!({"recordName": ecd("RADR_CHCO_S02_Observer"), "name": "Observer", "mass": mass});
        let harbinger = |turrets: u32| {
            ship(
                "AEGS_Vanguard_Harbinger",
                "Aegis Vanguard Harbinger",
                json!({"loadout": {"turrets": turrets}}),
            )
        };
        let cannon = |dps: u32| {
            gun(
                "VNCL_Gen2_Cannon_S5",
                "Vanduul Mauler 'ANNIHILATOR' Cannon",
                json!({"dps": dps}),
            )
        };
        let a = write_build(
            tmp.path(),
            "fix-a",
            "3.0.1",
            json!({
                "ships": {"vehicles": [harbinger(6), ship("VNCL_Mauler", "Vanduul Mauler Destroyer", json!({"totals": {"weaponDps": 100}}))]},
                "weapons": {"shipWeapons": [cannon(100)], "fpsWeapons": [], "missiles": []},
                "ship-components": {"radars": [radar(0)], "fuelTanks": []},
                "strings-english": {"_redacted": "name keys only", "strings": {"vehicle_NameA": "Gladius"}},
            }),
        );
        let b = write_build(
            tmp.path(),
            "fix-b",
            "3.0.2",
            json!({
                "ships": {"vehicles": [
                    harbinger(7),
                    ship("VNCL_Mauler", "Vanduul Mauler Destroyer", json!({"totals": {"weaponDps": 300}})),
                    ship("VNCL_Scythe", "Vanduul Scythe", json!({})),
                ]},
                "weapons": {"shipWeapons": [cannon(200)], "fpsWeapons": [], "missiles": []},
                "ship-components": {"radars": [radar(590)], "fuelTanks": [{"recordName": ecd("FUEL_Tank_S1"), "name": "Internal Tank"}]},
                "strings-english": {"strings": {"vehicle_NameA": "Gladius", "vehicle_NameB": "Galaxy", "vehicle_DescA": "Prose.", "Dlg_Line_1": "Hello"}},
                "search-index": {"format": 1, "items": [], "ships": []},
            }),
        );
        let doc = build(&a, &b, 1.0);

        // A build against itself: no sections, search index included.
        assert!(build(&b, &b, 1.0).sections.is_empty());
        assert!(render_notes(&build(&b, &b, 1.0)).contains("No player-facing changes."));
        assert!(!notes_json(&doc).contains("search-index"));

        // A redacted string table compares on published keys only.
        let other = section_of(&doc, SectionId::Other).unwrap();
        assert_eq!(
            other
                .lines
                .iter()
                .find(|l| l.label == "strings-english")
                .unwrap()
                .detail
                .as_deref(),
            Some("strings +1")
        );
        assert!(
            other
                .notes
                .iter()
                .any(|n| n.contains("redacted published one"))
        );

        // Ship-component additions are counted once, under their section.
        assert_eq!(
            labels(section_of(&doc, SectionId::NewShipItems)),
            vec!["Internal Tank"]
        );
        assert!(!other.lines.iter().any(|l| l.label == "ship-components"));

        // A stat filled in from 0 has no verdict.
        let observer = section_of(&doc, SectionId::ShipBalance)
            .unwrap()
            .lines
            .iter()
            .find(|l| l.label == "Observer")
            .unwrap();
        assert_eq!(
            observer.changes.as_deref(),
            Some(
                &[NoteChange {
                    stat: "mass".into(),
                    before: Some(json!(0.0)),
                    after: Some(json!(590.0)),
                    percent: None,
                    direction: Direction::Changed
                }][..]
            )
        );
        assert!(render_notes(&doc).contains("- **Observer**: mass 0 → 590"));

        // Vanduul hulls and weapons are only counted.
        let records: Vec<&String> = doc
            .sections
            .iter()
            .flat_map(|s| s.lines.iter().flat_map(|l| l.records.iter()))
            .collect();
        assert!(!records.iter().any(|r| r.starts_with("VNCL_")));
        assert_eq!(
            section_of(&doc, SectionId::NewShips).unwrap().notes,
            vec!["+1 AI/NPC/template variants not listed"]
        );
        let balance = section_of(&doc, SectionId::ShipBalance).unwrap();
        assert!(balance.notes.contains(
            &"+2 Vanduul (NPC-only) hulls and weapons with stat changes not listed".to_string()
        ));

        // Tracked records that changed outside the tracked stats are counted.
        assert!(
            balance.notes[0].starts_with("changed only outside the tracked stats")
                && balance.notes[0].contains("ships ×1")
        );
        assert_eq!(doc.hidden.untracked, 1);
        assert!(render_notes(&doc).contains("1 record changed only outside the tracked stats"));
    }

    fn added(label: &str, record: &str) -> NoteLine {
        NoteLine::new(
            label.into(),
            ItemStatus::Added,
            Some("shipWeapons".into()),
            vec![record.into()],
        )
    }

    fn bodies(v: &[Value]) -> Vec<&Body> {
        v.iter().map(|x| x.as_object().unwrap()).collect()
    }

    #[test]
    fn fold_new_skins_folds_one_skin_across_sizes() {
        let before = vec![
            json!({"name": "M3A Cannon", "recordName": "EntityClassDefinition.BEHR_LaserCannon_S1", "size": 1, "dps": 303.75}),
            json!({"name": "M4A Cannon", "recordName": "EntityClassDefinition.BEHR_LaserCannon_S2", "size": 2, "dps": 455.63}),
            json!({"name": "Coda Pistol", "recordName": "EntityClassDefinition.ksar_pistol_ballistic_01"}),
        ];
        let after = vec![
            json!({"name": "M3A Cannon Raider Camo", "recordName": "EntityClassDefinition.BEHR_LaserCannon_S1_mr02", "size": 1, "dps": 303.75}),
            json!({"name": "M4A Cannon Raider Camo", "recordName": "EntityClassDefinition.BEHR_LaserCannon_S2_mr02", "size": 2, "dps": 455.63}),
        ];
        let lines = fold_new_skins(
            vec![
                added("M3A Cannon Raider Camo", "BEHR_LaserCannon_S1_mr02"),
                added("M4A Cannon Raider Camo", "BEHR_LaserCannon_S2_mr02"),
            ],
            &bodies(&before),
            &bodies(&after),
        );
        let mut expected = NoteLine::new(
            "Raider Camo skin".into(),
            ItemStatus::Added,
            Some("shipWeapons".into()),
            vec![
                "BEHR_LaserCannon_S1_mr02".into(),
                "BEHR_LaserCannon_S2_mr02".into(),
            ],
        );
        expected.detail = Some(
            "for M3A Cannon (S1), M4A Cannon (S2); paint only, same stats as the base gun".into(),
        );
        assert_eq!(lines, vec![expected]);

        // A "skin" that changes stats is called out, not passed off as paint.
        let buffed = vec![with(after[0].clone(), json!({"dps": 400}))];
        let b = fold_new_skins(
            vec![added("M3A Cannon Raider Camo", "BEHR_LaserCannon_S1_mr02")],
            &bodies(&before),
            &bodies(&buffed),
        );
        assert!(
            b[0].detail
                .as_deref()
                .unwrap()
                .contains("stats differ from the base gun")
        );

        let doc = NotesDoc {
            from: NotesSide {
                source: "a".into(),
                build: None,
                fallback: None,
            },
            to: NotesSide {
                source: "b".into(),
                build: None,
                fallback: None,
            },
            min_change: 1.0,
            sections: vec![section(
                SectionId::NewShipItems,
                "New ship weapons & components",
                lines,
                Vec::new(),
            )],
            hidden: Hidden::default(),
        };
        assert!(
            render_notes(&doc).contains(
                "- **Raider Camo skin** — for M3A Cannon (S1), M4A Cannon (S2); paint only"
            )
        );

        // A quoted skin name; new guns and removals left alone.
        let mut piconalia = added(
            "Coda \"Piconalia\" Pistol",
            "ksar_pistol_ballistic_01_lumi01",
        );
        piconalia.category = None;
        let mut skarnov = added("Skarnov Rifle", "new_rifle_01");
        skarnov.variants = Some(vec!["Ferrite".into()]);
        let mut removed = added("M3A Cannon Raider Camo", "BEHR_LaserCannon_S1_mr02");
        removed.status = ItemStatus::Removed;
        let lines = fold_new_skins(vec![piconalia, skarnov, removed], &bodies(&before), &[]);
        assert_eq!(
            lines.iter().map(|l| l.label.as_str()).collect::<Vec<_>>(),
            vec!["Skarnov Rifle", "M3A Cannon Raider Camo", "Piconalia skin"]
        );
        // No new record to compare against: never claim "paint only" without checking.
        assert_eq!(
            lines[2].detail.as_deref(),
            Some("for Coda Pistol; stats differ from the base gun, see `diff`")
        );
    }

    #[test]
    fn changed_blueprint_lines_put_detail_before_changes() {
        let mut line = NoteLine::new(
            "x".into(),
            ItemStatus::Changed,
            Some("Blueprints".into()),
            vec!["r".into()],
        );
        line.detail = Some("d".into());
        line.changes = Some(Vec::new());
        assert_eq!(
            serde_json::to_string(&line).unwrap(),
            r#"{"label":"x","status":"changed","category":"Blueprints","records":["r"],"changes":[],"detail":"d"}"#
        );
        line.detail_first = true;
        assert_eq!(
            serde_json::to_string(&line).unwrap(),
            r#"{"label":"x","status":"changed","category":"Blueprints","records":["r"],"detail":"d","changes":[]}"#
        );
    }
}
