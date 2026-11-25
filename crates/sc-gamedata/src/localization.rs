//! The game's string table (port of `sc-gamedata/src/localization.ts`).
//!
//! Records store display text as keys: a manufacturer's name is `@manufacturer_NameSASU`,
//! which resolves to "Sakura Sun". The table is `Data/Localization/english/global.ini`:
//! UTF-8 with a BOM, CRLF line endings, `key=value` lines whose value may contain `=`.
//!
//! [`Localization`] behaves like the JavaScript `Map` it replaces: insertion order is kept,
//! a repeated key keeps its first position and takes the last value.

use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
pub struct Localization {
    keys: Vec<String>,
    values: HashMap<String, String>,
    /// Lower-case key -> first key (in insertion order) with that lower-case form, for the
    /// case-insensitive fallback, which scans the map in order in the TypeScript.
    lower: HashMap<String, String>,
}

impl Localization {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.keys.len()
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }

    pub fn set(&mut self, key: &str, value: &str) {
        if let Some(v) = self.values.get_mut(key) {
            *v = value.to_string();
            return;
        }
        self.keys.push(key.to_string());
        self.values.insert(key.to_string(), value.to_string());
        self.lower
            .entry(key.to_lowercase())
            .or_insert_with(|| key.to_string());
    }

    /// Entries in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.keys
            .iter()
            .map(|k| (k.as_str(), self.values[k].as_str()))
    }

    /// The value of the first key equal to `key` ignoring case.
    fn get_ignore_case(&self, key: &str) -> Option<&str> {
        let first = self.lower.get(&key.to_lowercase())?;
        self.get(first)
    }

    /// The table's own spelling of a bare key (`tableKey` in the TypeScript): exact, then
    /// any case, then the same with the `,P` suffix some keys exist only in (the game
    /// resolves `@item_NameX` to `item_NameX,P`).
    fn table_key(&self, bare: &str) -> Option<&str> {
        if let Some((k, _)) = self.values.get_key_value(bare) {
            return Some(k);
        }
        let lower = bare.to_lowercase();
        if let Some(k) = self.lower.get(&lower) {
            return Some(k);
        }
        let suffixed = format!("{bare},P");
        if let Some((k, _)) = self.values.get_key_value(&suffixed) {
            return Some(k);
        }
        self.lower.get(&format!("{lower},p")).map(String::as_str)
    }
}

/// JavaScript `String.prototype.trim` whitespace: Unicode White_Space plus the BOM.
fn js_trim(s: &str) -> &str {
    s.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}')
}

/// Parse a `global.ini` into a table.
pub fn parse_global_ini(text: &str) -> Localization {
    let mut table = Localization::new();
    // Strip the BOM so the very first key does not carry an invisible prefix.
    let body = text.strip_prefix('\u{feff}').unwrap_or(text);
    // `split(/\r?\n/)`: a lone `\r` stays in the line (and is trimmed away).
    for raw in body.split('\n') {
        let line = js_trim(raw.strip_suffix('\r').unwrap_or(raw));
        if line.is_empty()
            || line.starts_with(';')
            || line.starts_with('#')
            || line.starts_with('[')
        {
            continue;
        }
        let Some(split) = line.find('=') else {
            continue;
        };
        if split == 0 {
            continue;
        }
        // Values can contain '=', so only the first one separates key from value.
        table.set(&line[..split], &line[split + 1..]);
    }
    table
}

/// Parse `global.ini` bytes; invalid UTF-8 is replaced, as Node's `utf-8` decoding does.
pub fn parse_global_ini_bytes(bytes: &[u8]) -> Localization {
    parse_global_ini(&String::from_utf8_lossy(bytes))
}

/// Layer `overlay` over `base`; later wins. The base is not changed.
pub fn apply_overlay(base: &Localization, overlay: &Localization) -> Localization {
    let mut merged = base.clone();
    for (k, v) in overlay.iter() {
        merged.set(k, v);
    }
    merged
}

/// One string an overlay changes or adds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StringChange {
    pub key: String,
    pub base: Option<String>,
    pub overlay: String,
}

/// What an overlay changes relative to a base table: `(changed, added)`.
pub fn diff_strings(
    base: &Localization,
    overlay: &Localization,
) -> (Vec<StringChange>, Vec<StringChange>) {
    let mut changed = Vec::new();
    let mut added = Vec::new();
    for (key, value) in overlay.iter() {
        match base.get(key) {
            None => added.push(StringChange {
                key: key.to_string(),
                base: None,
                overlay: value.to_string(),
            }),
            Some(original) if original != value => changed.push(StringChange {
                key: key.to_string(),
                base: Some(original.to_string()),
                overlay: value.to_string(),
            }),
            Some(_) => {}
        }
    }
    (changed, added)
}

/// Search by key or value, case-insensitively: `(total, first `limit` hits)`.
pub fn search_strings<'a>(
    table: &'a Localization,
    query: &str,
    limit: usize,
    keys_only: bool,
) -> (usize, Vec<(&'a str, &'a str)>) {
    let needle = query.to_lowercase();
    let mut hits = Vec::new();
    let mut total = 0;
    for (k, v) in table.iter() {
        let matched = k.to_lowercase().contains(&needle)
            || (!keys_only && v.to_lowercase().contains(&needle));
        if !matched {
            continue;
        }
        total += 1;
        if hits.len() < limit {
            hits.push((k, v));
        }
    }
    (total, hits)
}

/// Exact lookup, accepting a key with or without its leading `@`.
pub fn lookup_string<'a>(table: &'a Localization, key: &str) -> Option<&'a str> {
    let bare = key.strip_prefix('@').unwrap_or(key);
    table
        .get(bare)
        .or_else(|| table.get(key))
        .or_else(|| table.get_ignore_case(bare))
}

/// Resolve a possibly-localized string value.
///
/// `None` for a non-string, an empty string, `@LOC_EMPTY`, or a key resolving to an empty
/// value; plain text passes through; an unresolved `@key` comes back as the token itself
/// so the gap stays visible (see [`is_unresolved`]).
pub fn localize<'a>(
    table: &'a Localization,
    value: Option<&'a serde_json::Value>,
) -> Option<&'a str> {
    localize_str(table, value?.as_str()?)
}

/// [`localize`] for a string.
pub fn localize_str<'a>(table: &'a Localization, value: &'a str) -> Option<&'a str> {
    if value.is_empty() || value == "@LOC_EMPTY" {
        return None;
    }
    let Some(key) = value.strip_prefix('@') else {
        return Some(value);
    };
    let hit = table
        .get(key)
        .or_else(|| table.get(value))
        .or_else(|| table.table_key(key).and_then(|k| table.get(k)));
    match hit {
        Some(text) => (!text.is_empty()).then_some(text),
        None => Some(value),
    }
}

/// True when [`localize`] returned a token it could not resolve.
pub fn is_unresolved(value: Option<&str>) -> bool {
    value.is_some_and(|v| v.starts_with('@'))
}

/// True when a resolved string is not actually a name: unset, an unresolved key, one of
/// CIG's placeholder markers (`<= PLACEHOLDER =>`, `< = TEST NAME = >`), or a `PH -` prefix
/// on unfinished items (`PH - shoes_01`, seen once `,P` keys resolve).
pub fn is_placeholder_name(name: Option<&str>) -> bool {
    let Some(name) = name.filter(|n| !n.is_empty()) else {
        return true;
    };
    if name.starts_with('@') {
        return true;
    }
    let lower = name.to_lowercase();
    if lower.contains("placeholder") || has_ph_prefix(&lower) {
        return true;
    }
    let squashed: String = lower.chars().filter(|c| !c.is_whitespace()).collect();
    squashed.contains("<=") || squashed.contains("=>") || squashed.contains("testname")
}

/// `/^ph\s*-/` on a lower-cased name. JavaScript `\s` is Unicode White_Space plus the BOM.
fn has_ph_prefix(lower: &str) -> bool {
    lower.strip_prefix("ph").is_some_and(|rest| {
        rest.trim_start_matches(|c: char| c.is_whitespace() || c == '\u{feff}')
            .starts_with('-')
    })
}

/// What a record's name reference resolves to, for consumers that need to tell a real name
/// from a gap. Domains swap placeholders for the class name, so the name field alone never
/// says whether the game has a name for the record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NameStatus {
    /// Resolves to a usable name.
    Real,
    /// Resolves, or is literal text, but is a placeholder marker.
    Placeholder,
    /// An `@` key the table has no entry for, even with `,P`.
    Unresolved,
    /// Unset, `@LOC_EMPTY`, `@LOC_UNINITIALIZED`, or a blank value.
    Empty,
}

/// `EMPTY_KEYS`, compared lower-cased.
fn is_empty_key(bare: &str) -> bool {
    matches!(
        bare.to_lowercase().as_str(),
        "" | "loc_empty" | "loc_uninitialized"
    )
}

/// The string-table key a field references, without its `@`, spelled as the table spells it
/// (including a `,P` suffix the reference leaves off). An unresolved key is returned as
/// referenced, so the gap stays visible. Literal text, unset values and the empty markers
/// give `None`.
pub fn loc_key<'a>(
    table: &'a Localization,
    value: Option<&'a serde_json::Value>,
) -> Option<&'a str> {
    loc_key_str(table, value?.as_str()?)
}

/// [`loc_key`] for a string.
pub fn loc_key_str<'a>(table: &'a Localization, value: &'a str) -> Option<&'a str> {
    let bare = value.strip_prefix('@')?;
    if is_empty_key(bare) {
        return None;
    }
    Some(table.table_key(bare).unwrap_or(bare))
}

/// Classify a name reference; see [`NameStatus`].
pub fn name_status_of(table: &Localization, value: Option<&serde_json::Value>) -> NameStatus {
    match value.and_then(serde_json::Value::as_str) {
        Some(v) => name_status_of_str(table, v),
        None => NameStatus::Empty,
    }
}

/// [`name_status_of`] for a string.
pub fn name_status_of_str(table: &Localization, value: &str) -> NameStatus {
    let classify = |text: &str| {
        if is_placeholder_name(Some(text)) {
            NameStatus::Placeholder
        } else {
            NameStatus::Real
        }
    };
    if value.is_empty() {
        return NameStatus::Empty;
    }
    let Some(bare) = value.strip_prefix('@') else {
        return classify(value);
    };
    if is_empty_key(bare) {
        return NameStatus::Empty;
    }
    let Some(key) = table.table_key(bare) else {
        return NameStatus::Unresolved;
    };
    let text = js_trim(table.get(key).unwrap_or_default());
    if text.is_empty() {
        return NameStatus::Empty;
    }
    classify(text)
}

/// A [`NameStatus`] as a dataset field: `None` when real, the common case, to keep dumps small.
pub fn name_status_field(
    table: &Localization,
    value: Option<&serde_json::Value>,
) -> Option<NameStatus> {
    Some(name_status_of(table, value)).filter(|s| *s != NameStatus::Real)
}

/// Key refs for a record's name and description references (`locRefs`).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocRefs {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description_key: Option<String>,
    /// Absent when the name is real.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name_status: Option<NameStatus>,
}

/// [`LocRefs`] for a name and an optional description reference.
pub fn loc_refs(
    table: &Localization,
    name: Option<&serde_json::Value>,
    description: Option<&serde_json::Value>,
) -> LocRefs {
    LocRefs {
        name_key: loc_key(table, name).map(str::to_string),
        description_key: loc_key(table, description).map(str::to_string),
        name_status: name_status_field(table, name),
    }
}

/// The redaction `sc-gamedata`'s `scripts/publish-dump.mjs` applies to the string table of
/// a published dump: name-shaped keys only, no prose. Off unless asked for
/// ([`crate::pipeline::Options::redact_strings`]); domain parsing never uses it.
pub mod redaction {
    use super::Localization;
    use serde_json::{Map, Value};

    /// Value of `_redacted` in a published string table.
    pub const NOTE: &str =
        "Localisation limited to name keys; descriptive and dialogue text is not published.";
    /// A display name is never a paragraph.
    pub const MAX_VALUE_LENGTH: usize = 120;

    /// `KEEP_KEY = /(^|_)(Names?|Titles?|DisplayName|ShortName|Nickname)([A-Z0-9_]|$)/`.
    fn keep_key(key: &str) -> bool {
        const WORDS: [&str; 7] = [
            "Names",
            "Name",
            "Titles",
            "Title",
            "DisplayName",
            "ShortName",
            "Nickname",
        ];
        let b = key.as_bytes();
        for start in 0..=b.len() {
            if !(start == 0 || b[start - 1] == b'_') {
                continue;
            }
            for w in WORDS {
                if key[start..].starts_with(w) {
                    match b.get(start + w.len()) {
                        None => return true,
                        Some(c) if c.is_ascii_uppercase() || c.is_ascii_digit() || *c == b'_' => {
                            return true;
                        }
                        _ => {}
                    }
                }
            }
        }
        false
    }

    /// `DROP_KEY = /desc|tooltip|hint|subtitle|^Dlg_|^PU_|^DX[SM]/i`.
    fn drop_key(key: &str) -> bool {
        let l = key.to_ascii_lowercase();
        ["desc", "tooltip", "hint", "subtitle"]
            .iter()
            .any(|w| l.contains(w))
            || l.starts_with("dlg_")
            || l.starts_with("pu_")
            || l.starts_with("dxs")
            || l.starts_with("dxm")
    }

    /// Whether a published dump keeps this entry. `MAX_VALUE_LENGTH` counts UTF-16 units.
    pub fn keeps(key: &str, value: &str) -> bool {
        !value.is_empty()
            && value.encode_utf16().count() <= MAX_VALUE_LENGTH
            && !drop_key(key)
            && keep_key(key)
    }

    /// The table cut to what a published dump keeps.
    pub fn redact(table: &Localization) -> Localization {
        let mut out = Localization::new();
        for (k, v) in table.iter().filter(|(k, v)| keeps(k, v)) {
            out.set(k, v);
        }
        out
    }

    /// `PROSE_FIELDS` of `scripts/redact.mjs`: prose fields per dataset, removed wherever
    /// they appear in it. Records keep their `descriptionKey`, so the text stays one
    /// string-table lookup away.
    pub const PROSE_FIELDS: &[(&str, &[&str])] = &[
        ("game-commodities.json", &["description"]),
        ("game-manufacturers.json", &["description"]),
        ("game-missions.json", &["description"]),
        (
            "game-refining.json",
            &["description", "rawDescription", "refinedDescription"],
        ),
        ("game-ships.json", &["description"]),
        ("game-starmap.json", &["description"]),
    ];

    /// The prose fields of a dataset file (`game-ships.json`), if it has any.
    pub fn prose_fields(file: &str) -> Option<&'static [&'static str]> {
        PROSE_FIELDS
            .iter()
            .find(|(f, _)| *f == file)
            .map(|(_, fields)| *fields)
    }

    /// `strip`: `fields` removed from every object under `value`, a copy.
    pub fn strip_fields(value: &Value, fields: &[&str]) -> Value {
        match value {
            Value::Array(a) => Value::Array(a.iter().map(|v| strip_fields(v, fields)).collect()),
            Value::Object(o) => Value::Object(
                o.iter()
                    .filter(|(k, _)| !fields.contains(&k.as_str()))
                    .map(|(k, v)| (k.clone(), strip_fields(v, fields)))
                    .collect(),
            ),
            other => other.clone(),
        }
    }

    /// The long-mission-title rule shared by `stripLongTitles` (redact.mjs) and
    /// `alignProse` (diff.ts): a title past `max` UTF-16 units is briefing text and goes;
    /// `name`, when it copies the title, becomes the record's short name. `None` when the
    /// mission keeps its title.
    pub fn without_long_title(mission: &Value, max: usize) -> Option<Value> {
        let m = mission.as_object()?;
        let title = m.get("title")?.as_str()?;
        if title.encode_utf16().count() <= max {
            return None;
        }
        // `String(m.recordName ?? "").split(".").pop() || m.id`
        let record_name = match m.get("recordName") {
            None | Some(Value::Null) => String::new(),
            Some(Value::String(s)) => s.clone(),
            Some(other) => other.to_string(),
        };
        let short = record_name.rsplit('.').next().unwrap_or_default();
        let short = if short.is_empty() {
            m.get("id").cloned()
        } else {
            Some(Value::String(short.to_string()))
        };
        let renames = m.get("name").and_then(Value::as_str) == Some(title);
        let mut out = Map::new();
        for (k, v) in m {
            match k.as_str() {
                "title" => {}
                // `{ ...rest, name: short }` keeps `name` in place; an undefined `name` drops.
                "name" if renames => {
                    if let Some(short) = &short {
                        out.insert(k.clone(), short.clone());
                    }
                }
                _ => {
                    out.insert(k.clone(), v.clone());
                }
            }
        }
        Some(Value::Object(out))
    }

    /// `redactProse`: a dataset as published, prose fields removed and listed in
    /// `_redacted` (after `_build`). `None` for a dataset without prose fields, which is
    /// published as is.
    pub fn redact_prose(file: &str, dump: &Value) -> Option<Value> {
        let fields = prose_fields(file)?;
        let mut out = strip_fields(dump, fields);
        let mut redacted: Vec<Value> = fields.iter().map(|f| Value::from(*f)).collect();
        if file == "game-missions.json"
            && let Some(Value::Array(missions)) = out.get_mut("missions")
        {
            let mut dropped = 0;
            for m in missions.iter_mut() {
                if let Some(cut) = without_long_title(m, MAX_VALUE_LENGTH) {
                    *m = cut;
                    dropped += 1;
                }
            }
            if dropped > 0 {
                redacted.push(Value::from(format!(
                    "title over {MAX_VALUE_LENGTH} characters"
                )));
            }
        }
        let Value::Object(body) = out else {
            return Some(out);
        };
        const HEAD: [&str; 3] = ["_source", "_extracted", "_build"];
        let mut published = Map::new();
        for k in HEAD {
            if let Some(v) = body.get(k) {
                published.insert(k.to_string(), v.clone());
            }
        }
        // `{ _source, _extracted, _build, _redacted, ...rest }`: a `_redacted` the dump
        // already had comes back through `rest` and wins, in fourth place.
        let own = body.get("_redacted").cloned();
        published.insert("_redacted".into(), own.unwrap_or(Value::Array(redacted)));
        for (k, v) in body {
            if !HEAD.contains(&k.as_str()) && k != "_redacted" {
                published.insert(k, v);
            }
        }
        Some(Value::Object(published))
    }

    /// `STRINGS_FILE`: the string table publish redacts to name keys.
    pub const STRINGS_FILE: &str = "game-strings-english.json";

    /// `redactStrings`: a string-table dump as published: `strings` cut to [`keeps`],
    /// `_redacted` set to [`NOTE`], `summary.published` counting the kept keys. Other
    /// keys stay where they were (`{ ...dump, _redacted, strings, summary }`).
    pub fn redact_strings(dump: &Value) -> Value {
        let mut kept = Map::new();
        if let Some(Value::Object(strings)) = dump.get("strings") {
            for k in crate::output::js_key_order(strings) {
                let v = &strings[k];
                // `!value || value.length > MAX`: a non-string value has no length.
                let keep = match v {
                    Value::String(s) => keeps(k, s),
                    Value::Null | Value::Bool(false) => false,
                    Value::Number(n) => {
                        n.as_f64().is_some_and(|x| x != 0.0) && !drop_key(k) && keep_key(k)
                    }
                    _ => !drop_key(k) && keep_key(k),
                };
                if keep {
                    kept.insert(k.clone(), v.clone());
                }
            }
        }
        let mut summary = match dump.get("summary") {
            Some(Value::Object(o)) => o.clone(),
            _ => Map::new(),
        };
        summary.insert("published".into(), Value::from(kept.len()));
        let mut out = match dump {
            Value::Object(o) => o.clone(),
            _ => Map::new(),
        };
        out.insert("_redacted".into(), Value::String(NOTE.into()));
        out.insert("strings".into(), Value::Object(kept));
        out.insert("summary".into(), Value::Object(summary));
        Value::Object(out)
    }

    /// `redactDump`: whatever publishing does to one dataset (the string table to name
    /// keys, prose datasets without their prose, the rest as is).
    pub fn redact_dump(file: &str, dump: &Value) -> Value {
        if file == STRINGS_FILE {
            redact_strings(dump)
        } else {
            redact_prose(file, dump).unwrap_or_else(|| dump.clone())
        }
    }

    #[cfg(test)]
    mod tests {
        //! Cases from `scripts/redact.test.mjs`.
        use super::*;
        use serde_json::json;

        const PROSE: &str =
            "A long paragraph of in-game flavour text that must never reach the public repo.";

        fn fixtures() -> Vec<(&'static str, Value)> {
            let h = |body: Value| {
                let mut v = json!({"_source": "test", "_extracted": "2026-11-23T00:00:00Z", "_build": "4.10.201.48085"});
                for (k, x) in body.as_object().unwrap() {
                    v[k] = x.clone();
                }
                v
            };
            vec![
                (
                    "game-commodities.json",
                    h(json!({
                    "resources": [{"name": "Gold", "description": PROSE, "descriptionKey": "items_commodities_gold_desc"}],
                    "items": [{"name": "gold_ore", "description": PROSE, "descriptionKey": "items_commodities_gold_desc"}]})),
                ),
                (
                    "game-manufacturers.json",
                    h(
                        json!({"manufacturers": [{"code": "AEGS", "name": "Aegis", "description": PROSE}]}),
                    ),
                ),
                (
                    "game-missions.json",
                    h(json!({"missions": [
                    {"name": "Short Title", "recordName": "MissionBrokerEntry.short", "title": "Short Title", "description": PROSE},
                    {"name": "x".repeat(121), "recordName": "MissionBrokerEntry.long_one", "title": "x".repeat(121), "titleKey": "k"}]})),
                ),
                (
                    "game-refining.json",
                    h(json!({
                    "processes": [{"name": "Cascade", "description": PROSE}],
                    "materials": [{"name": "Gold", "rawDescription": PROSE, "refinedDescription": PROSE}]})),
                ),
                (
                    "game-ships.json",
                    h(
                        json!({"vehicles": [{"name": "Gladius", "description": PROSE, "descriptionKey": "vehicle_DescAEGS_Gladius"}]}),
                    ),
                ),
                (
                    "game-starmap.json",
                    h(json!({"locations": [{"name": "Daymar", "description": PROSE}]})),
                ),
                (
                    "game-weapons.json",
                    h(json!({"shipWeapons": [{"name": "Gun"}]})),
                ),
            ]
        }

        fn keys_of(v: &Value, out: &mut Vec<String>) {
            match v {
                Value::Array(a) => a.iter().for_each(|x| keys_of(x, out)),
                Value::Object(o) => o.iter().for_each(|(k, x)| {
                    out.push(k.clone());
                    keys_of(x, out);
                }),
                _ => {}
            }
        }

        fn published() -> Vec<(&'static str, Value, Value)> {
            fixtures()
                .into_iter()
                .map(|(f, d)| (f, redact_dump(f, &d), d))
                .collect()
        }

        #[test]
        fn no_prose_field_or_text_survives() {
            for (file, dump, _) in published() {
                let mut keys = Vec::new();
                keys_of(&dump, &mut keys);
                for field in prose_fields(file).unwrap_or_default() {
                    assert!(!keys.iter().any(|k| k == field), "{file} {field}");
                }
                assert!(!keys.iter().any(|k| k == "description"), "{file}");
                assert!(!dump.to_string().contains(PROSE), "{file}");
            }
        }

        #[test]
        fn keeps_description_keys_and_lists_what_it_removed() {
            let p = published();
            let get = |f: &str| p.iter().find(|x| x.0 == f).unwrap().1.clone();
            assert_eq!(
                get("game-ships.json")["vehicles"][0]["descriptionKey"],
                "vehicle_DescAEGS_Gladius"
            );
            assert_eq!(
                get("game-commodities.json")["items"][0]["descriptionKey"],
                "items_commodities_gold_desc"
            );
            assert_eq!(
                get("game-refining.json")["_redacted"],
                json!(["description", "rawDescription", "refinedDescription"])
            );
            let ships = get("game-ships.json");
            let keys: Vec<&String> = ships.as_object().unwrap().keys().take(4).collect();
            assert_eq!(keys, vec!["_source", "_extracted", "_build", "_redacted"]);
        }

        #[test]
        fn long_mission_titles_go_and_rename_from_the_record() {
            let m = redact_dump("game-missions.json", &fixtures()[2].1);
            assert_eq!(m["missions"][0]["title"], "Short Title");
            assert!(m["missions"][1].get("title").is_none());
            assert_eq!(m["missions"][1]["name"], "long_one");
            assert_eq!(m["missions"][1]["titleKey"], "k");
            assert_eq!(
                m["_redacted"],
                json!(["description", "title over 120 characters"])
            );
        }

        #[test]
        fn datasets_without_prose_are_untouched() {
            for (file, out, input) in published() {
                if file == "game-weapons.json" {
                    assert_eq!(out, input);
                }
                if file == "game-ships.json" {
                    assert_eq!(input["vehicles"][0]["description"], PROSE);
                }
            }
        }

        #[test]
        fn string_table_reduces_to_name_keys() {
            let dump = json!({"_source": "test", "_build": "4.10.201.48085",
                "strings": {"item_NameGladius": "Gladius", "item_DescGladius": PROSE, "Dlg_hello": "Hi"}, "summary": {"total": 3}});
            let out = redact_dump(STRINGS_FILE, &dump);
            assert_eq!(out["strings"], json!({"item_NameGladius": "Gladius"}));
            assert_eq!(out["summary"], json!({"total": 3, "published": 1}));
            assert_eq!(out["_redacted"], NOTE);
            let keys: Vec<&String> = out.as_object().unwrap().keys().collect();
            assert_eq!(
                keys,
                vec!["_source", "_build", "strings", "summary", "_redacted"]
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // Cases from sc-gamedata src/localization.test.ts.
    #[test]
    fn parse_global_ini_cases() {
        let t = parse_global_ini("manufacturer_NameAEGS=Aegis Dynamics\nfoo=bar");
        assert_eq!(t.get("manufacturer_NameAEGS"), Some("Aegis Dynamics"));
        let t = parse_global_ini("\u{feff}first_key=value");
        assert_eq!(t.get("first_key"), Some("value"));
        let t = parse_global_ini("a=1\r\nb=2\r\n");
        assert_eq!((t.get("a"), t.get("b")), (Some("1"), Some("2")));
        let t = parse_global_ini("equation=x=y+1");
        assert_eq!(t.get("equation"), Some("x=y+1"));
        let t = parse_global_ini("; comment\n# other\n[Section]\n\nreal=yes");
        assert_eq!(t.len(), 1);
        assert_eq!(t.get("real"), Some("yes"));
        let t = parse_global_ini("=novalue\n  spaced = x  ");
        assert_eq!(t.len(), 1);
        assert_eq!(t.get("spaced "), Some(" x"));
    }

    #[test]
    fn repeated_key_keeps_position_takes_last_value() {
        let t = parse_global_ini("a=1\nb=2\na=3");
        let entries: Vec<_> = t.iter().collect();
        assert_eq!(entries, vec![("a", "3"), ("b", "2")]);
    }

    #[test]
    fn localize_cases() {
        let t = parse_global_ini("manufacturer_NameAEGS=Aegis Dynamics\nempty_value=");
        let l = |v: serde_json::Value| localize(&t, Some(&v)).map(str::to_string);
        assert_eq!(
            l(json!("@manufacturer_NameAEGS")).as_deref(),
            Some("Aegis Dynamics")
        );
        assert_eq!(l(json!("Just A Name")).as_deref(), Some("Just A Name"));
        assert_eq!(l(json!("@LOC_EMPTY")), None);
        assert_eq!(l(json!("@empty_value")), None);
        assert_eq!(
            l(json!("@MANUFACTURER_NAMEAEGS")).as_deref(),
            Some("Aegis Dynamics")
        );
        let r = l(json!("@manufacturer_NameNOPE"));
        assert_eq!(r.as_deref(), Some("@manufacturer_NameNOPE"));
        assert!(is_unresolved(r.as_deref()));
        assert_eq!(localize(&t, None), None);
        assert_eq!(l(json!(42)), None);
        assert_eq!(l(json!("")), None);
    }

    #[test]
    fn search_cases() {
        let t = parse_global_ini(
            "manufacturer_NameAEGS=Aegis Dynamics\nmanufacturer_NameDRAK=Drake Interplanetary\nStanton1=Hurston\nStanton1c=Magda\nATC_WikeloMods=Wikelo Emporium",
        );
        assert_eq!(
            search_strings(&t, "Wikelo", 25, false).1,
            vec![("ATC_WikeloMods", "Wikelo Emporium")]
        );
        assert_eq!(search_strings(&t, "manufacturer_Name", 25, false).0, 2);
        assert_eq!(search_strings(&t, "Hurston", 25, true).0, 0);
        assert_eq!(search_strings(&t, "Hurston", 25, false).0, 1);
        let (total, hits) = search_strings(&t, "manufacturer", 1, false);
        assert_eq!((total, hits.len()), (2, 1));
        assert_eq!(search_strings(&t, "aegis", 25, false).0, 1);
    }

    #[test]
    fn lookup_cases() {
        let t = parse_global_ini("Stanton1c=Magda\nmanufacturer_NameAEGS=Aegis Dynamics");
        assert_eq!(
            lookup_string(&t, "@manufacturer_NameAEGS"),
            Some("Aegis Dynamics")
        );
        assert_eq!(
            lookup_string(&t, "manufacturer_NameAEGS"),
            Some("Aegis Dynamics")
        );
        assert_eq!(lookup_string(&t, "Stanton1c"), Some("Magda"));
        assert_eq!(lookup_string(&t, "Nope"), None);
    }

    #[test]
    fn placeholder_names() {
        for p in [
            None,
            Some(""),
            Some("@item_NameFoo"),
            Some("<= PLACEHOLDER =>"),
            Some("< = TEST NAME = >"),
        ] {
            assert!(is_placeholder_name(p), "{p:?}");
        }
        assert!(!is_placeholder_name(Some("Aegis Dynamics")));
    }

    #[test]
    fn ph_prefix_is_placeholder() {
        assert!(is_placeholder_name(Some("PH - 987_shoes_01")));
        assert!(is_placeholder_name(Some("ph-shoes")));
        assert!(!is_placeholder_name(Some("Phoenix")));
        assert!(!is_placeholder_name(Some("Alpha PH - x")));
    }

    // Cases from sc-gamedata src/localization.test.ts, "locKey / nameStatusOf".
    fn loc_table() -> Localization {
        parse_global_ini(
            &[
                "item_NameReal=Arrowhead",
                "item_DescReal=A sniper rifle.",
                "item_NameP,P=Pembroke",
                "item_NameStub=<= PLACEHOLDER =>",
                "LOC_PLACEHOLDER=<= PLACEHOLDER =>",
                "item_NameBlank=",
                "Item_NameMixed=Mixed",
            ]
            .join("\r\n"),
        )
    }

    #[test]
    fn real_key() {
        let t = loc_table();
        assert_eq!(loc_key_str(&t, "@item_NameReal"), Some("item_NameReal"));
        assert_eq!(name_status_of_str(&t, "@item_NameReal"), NameStatus::Real);
    }

    #[test]
    fn localize_resolves_p_only_key() {
        let t = loc_table();
        assert_eq!(localize_str(&t, "@item_NameP"), Some("Pembroke"));
        assert_eq!(localize_str(&t, "@ITEM_NAMEP"), Some("Pembroke"));
    }

    #[test]
    fn loc_key_spells_p_and_case_as_table() {
        let t = loc_table();
        assert_eq!(loc_key_str(&t, "@item_NameP"), Some("item_NameP,P"));
        assert_eq!(name_status_of_str(&t, "@item_NameP"), NameStatus::Real);
        assert_eq!(loc_key_str(&t, "@item_namemixed"), Some("Item_NameMixed"));
    }

    #[test]
    fn placeholder_value_and_key() {
        let t = loc_table();
        assert_eq!(
            name_status_of_str(&t, "@item_NameStub"),
            NameStatus::Placeholder
        );
        assert_eq!(
            name_status_of_str(&t, "@LOC_PLACEHOLDER"),
            NameStatus::Placeholder
        );
        assert_eq!(loc_key_str(&t, "@item_NameStub"), Some("item_NameStub"));
    }

    #[test]
    fn unresolved_key_kept_as_referenced() {
        let t = loc_table();
        assert_eq!(loc_key_str(&t, "@item_NameGone"), Some("item_NameGone"));
        assert_eq!(
            name_status_of_str(&t, "@item_NameGone"),
            NameStatus::Unresolved
        );
    }

    #[test]
    fn empty_markers_have_no_key() {
        let t = loc_table();
        assert_eq!(loc_key(&t, None), None);
        assert_eq!(name_status_of(&t, None), NameStatus::Empty);
        for v in ["", "@LOC_EMPTY", "@loc_uninitialized", "@"] {
            assert_eq!(loc_key_str(&t, v), None, "{v:?}");
            assert_eq!(name_status_of_str(&t, v), NameStatus::Empty, "{v:?}");
        }
        assert_eq!(name_status_of_str(&t, "@item_NameBlank"), NameStatus::Empty);
    }

    #[test]
    fn literal_text_has_no_key() {
        let t = loc_table();
        assert_eq!(loc_key_str(&t, "Plain Name"), None);
        assert_eq!(name_status_of_str(&t, "Plain Name"), NameStatus::Real);
        assert_eq!(
            name_status_of_str(&t, "< = TEST NAME = >"),
            NameStatus::Placeholder
        );
        assert_eq!(
            name_status_of_str(&t, "PH - 987_shoes_01"),
            NameStatus::Placeholder
        );
        assert_eq!(name_status_of_str(&t, "Phoenix"), NameStatus::Real);
    }

    #[test]
    fn loc_refs_bundle() {
        let t = loc_table();
        let (name, desc) = (json!("@item_NameReal"), json!("@item_DescReal"));
        let refs = loc_refs(&t, Some(&name), Some(&desc));
        assert_eq!(
            serde_json::to_value(&refs).unwrap(),
            json!({"nameKey": "item_NameReal", "descriptionKey": "item_DescReal"})
        );
        assert_eq!(loc_refs(&t, Some(&name), None).name_status, None);
        let gone = json!("@item_NameGone");
        assert_eq!(
            loc_refs(&t, Some(&gone), None).name_status,
            Some(NameStatus::Unresolved)
        );
        let stub = json!("@item_NameStub");
        assert_eq!(name_status_field(&t, Some(&name)), None);
        assert_eq!(
            name_status_field(&t, Some(&stub)),
            Some(NameStatus::Placeholder)
        );
        assert_eq!(
            serde_json::to_value(NameStatus::Unresolved).unwrap(),
            json!("unresolved")
        );
    }

    // Cases from sc-gamedata scripts/redact.test.mjs.
    mod prose_redaction {
        use super::super::redaction::{PROSE_FIELDS, prose_fields, redact_prose};
        use serde_json::{Value, json};
        use std::collections::BTreeSet;

        const PROSE: &str =
            "A long paragraph of in-game flavour text that must never reach the public repo.";

        fn with_header(body: Value) -> Value {
            let mut out = json!({"_source": "test", "_extracted": "2026-11-23T00:00:00Z", "_build": "4.10.201.48085"});
            out.as_object_mut()
                .unwrap()
                .extend(body.as_object().unwrap().clone());
            out
        }

        fn keys_of(value: &Value, out: &mut BTreeSet<String>) {
            match value {
                Value::Array(a) => a.iter().for_each(|v| keys_of(v, out)),
                Value::Object(o) => {
                    for (k, v) in o {
                        out.insert(k.clone());
                        keys_of(v, out);
                    }
                }
                _ => {}
            }
        }

        fn fixtures() -> Vec<(&'static str, Value)> {
            let long = "x".repeat(121);
            vec![
                (
                    "game-commodities.json",
                    with_header(json!({
                    "resources": [{"name": "Gold", "description": PROSE, "descriptionKey": "items_commodities_gold_desc"}],
                    "items": [{"name": "gold_ore", "description": PROSE, "descriptionKey": "items_commodities_gold_desc"}]})),
                ),
                (
                    "game-manufacturers.json",
                    with_header(
                        json!({"manufacturers": [{"code": "AEGS", "name": "Aegis", "description": PROSE}]}),
                    ),
                ),
                (
                    "game-missions.json",
                    with_header(json!({"missions": [
                    {"name": "Short Title", "recordName": "MissionBrokerEntry.short", "title": "Short Title", "description": PROSE},
                    {"name": long, "recordName": "MissionBrokerEntry.long_one", "title": long, "titleKey": "k"}]})),
                ),
                (
                    "game-refining.json",
                    with_header(json!({
                    "processes": [{"name": "Cascade", "description": PROSE}],
                    "materials": [{"name": "Gold", "rawDescription": PROSE, "refinedDescription": PROSE}]})),
                ),
                (
                    "game-ships.json",
                    with_header(
                        json!({"vehicles": [{"name": "Gladius", "description": PROSE, "descriptionKey": "vehicle_DescAEGS_Gladius"}]}),
                    ),
                ),
                (
                    "game-starmap.json",
                    with_header(json!({"locations": [{"name": "Daymar", "description": PROSE}]})),
                ),
                (
                    "game-weapons.json",
                    with_header(json!({"shipWeapons": [{"name": "Gun"}]})),
                ),
            ]
        }

        fn published(file: &str) -> Value {
            let (_, dump) = fixtures().into_iter().find(|(f, _)| *f == file).unwrap();
            redact_prose(file, &dump).unwrap_or(dump)
        }

        #[test]
        fn leaves_no_prose_field_and_no_prose_text() {
            for (file, dump) in fixtures() {
                let out = redact_prose(file, &dump).unwrap_or(dump);
                let mut keys = BTreeSet::new();
                keys_of(&out, &mut keys);
                for field in prose_fields(file).unwrap_or_default() {
                    assert!(!keys.contains(*field), "{file} {field}");
                }
                assert!(!keys.contains("description"), "{file}");
                assert!(!out.to_string().contains(PROSE), "{file}");
            }
            assert_eq!(PROSE_FIELDS.len(), 6);
        }

        #[test]
        fn keeps_description_keys_and_lists_what_it_removed() {
            assert_eq!(
                published("game-ships.json")["vehicles"][0]["descriptionKey"],
                "vehicle_DescAEGS_Gladius"
            );
            assert_eq!(
                published("game-commodities.json")["items"][0]["descriptionKey"],
                "items_commodities_gold_desc"
            );
            assert_eq!(
                published("game-refining.json")["_redacted"],
                json!(["description", "rawDescription", "refinedDescription"])
            );
            let ships = published("game-ships.json");
            let keys: Vec<&str> = ships
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect();
            assert_eq!(keys[..4], ["_source", "_extracted", "_build", "_redacted"]);
        }

        #[test]
        fn keeps_short_mission_titles_and_drops_long_ones() {
            let missions = published("game-missions.json");
            let (short, long) = (&missions["missions"][0], &missions["missions"][1]);
            assert_eq!(short["title"], "Short Title");
            assert!(long.get("title").is_none());
            assert_eq!(long["name"], "long_one");
            assert_eq!(long["titleKey"], "k");
            let keys: Vec<&str> = long
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect();
            assert_eq!(keys, ["name", "recordName", "titleKey"]);
            assert_eq!(
                missions["_redacted"],
                json!(["description", "title over 120 characters"])
            );
        }

        #[test]
        fn leaves_datasets_without_prose_fields_alone() {
            let (_, weapons) = fixtures().pop().unwrap();
            assert_eq!(redact_prose("game-weapons.json", &weapons), None);
        }
    }

    #[test]
    fn overlay_and_diff() {
        let base = parse_global_ini("a=1\nb=2");
        let over = parse_global_ini("b=3\nc=4");
        let merged = apply_overlay(&base, &over);
        assert_eq!(
            merged.iter().collect::<Vec<_>>(),
            vec![("a", "1"), ("b", "3"), ("c", "4")]
        );
        assert_eq!(base.get("b"), Some("2"));
        let (changed, added) = diff_strings(&base, &over);
        assert_eq!(changed[0].key, "b");
        assert_eq!(added[0].key, "c");
    }

    #[test]
    fn redaction_matches_publish_dump() {
        use redaction::keeps;
        assert!(keeps("vehicle_NameAEGS_Gladius", "Gladius"));
        assert!(keeps("item_Name", "x"));
        assert!(keeps("Title_1", "x"));
        assert!(!keeps("vehicle_DescAEGS", "prose"), "no prose keys");
        assert!(!keeps("PU_x_NameY", "x"), "dialogue namespace");
        assert!(!keeps("MyNamesSpecial", "x"), "segment boundary");
        assert!(!keeps("item_Namex", "x"), "lower-case after the word");
        assert!(!keeps("item_Name", ""), "empty value");
        assert!(!keeps("item_Name", &"x".repeat(121)), "long value");
    }
}
