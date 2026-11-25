//! Port of `src/diff.ts`: patch-over-patch comparison of parsed datasets.
//!
//! A build is identified by where its datasets come from ([`BuildSource`]): a directory of
//! `game-*.json` (the local `data/` after a parse, any other checkout, or a gzipped
//! snapshot under `snapshots/<build>/`), or a tag in the public dumps checkout, read with
//! `git show <tag>:data/<file>`. Resolution order ([`resolve_source`]): `local`, a
//! directory, a local snapshot (with the published tag of the same build as fallback for
//! datasets it lacks), then a dumps-repo tag.
//!
//! Records are matched by `recordName` (or `className`, `id`, `reference`, `key`, `name`),
//! never by display name, and field changes are reported on the flattened object
//! (`ammo.damage.energy: 55 -> 60`). A redacted published side (string table cut to name
//! keys, or a dataset without its prose, `_redacted`) makes the full side cut the same way
//! first ([`align_redaction`]).
//!
//! Output is byte-identical to the TypeScript: [`diff_json`] is the `--json` text,
//! [`render_diff`] the terminal text of one dataset.

use crate::collate::locale_compare;
use crate::localization::redaction;
use crate::output::{iso_now, js_key_order, js_number, to_js_json, to_js_json_compact};
use serde::Serialize;
use serde_json::{Map, Value};
use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

/// A parsed dataset file (a JS object).
pub type Body = Map<String, Value>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A usage or lookup error, worded as the TypeScript throws it.
    #[error("{0}")]
    Message(String),
    #[error("{}: {source}", path.display())]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{file}: {source}")]
    Json {
        file: String,
        source: serde_json::Error,
    },
}

pub type Result<T> = std::result::Result<T, Error>;

fn io_err(path: &Path) -> impl FnOnce(std::io::Error) -> Error + '_ {
    move |source| Error::Io {
        path: path.to_path_buf(),
        source,
    }
}

// ---------------------------------------------------------------------------
// JavaScript semantics shared by diff, changes and notes
// ---------------------------------------------------------------------------

/// Default `Array.prototype.sort` order of strings: UTF-16 code units.
pub(crate) fn cmp_utf16(a: &str, b: &str) -> Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

pub(crate) fn sort_utf16(v: &mut [String]) {
    v.sort_by(|a, b| cmp_utf16(a, b));
}

/// `s.length`.
pub(crate) fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// Byte offset of UTF-16 unit `n` (clamped; a cut inside a surrogate pair moves past it).
fn utf16_offset(s: &str, n: usize) -> usize {
    let mut units = 0;
    for (i, c) in s.char_indices() {
        if units >= n {
            return i;
        }
        units += c.len_utf16();
    }
    s.len()
}

/// `s.slice(0, n)`.
pub(crate) fn utf16_prefix(s: &str, n: usize) -> &str {
    &s[..utf16_offset(s, n)]
}

/// `s.slice(n)`.
pub(crate) fn utf16_from(s: &str, n: usize) -> &str {
    &s[utf16_offset(s, n)..]
}

/// JavaScript `\s` (WhiteSpace and LineTerminator), also what `trim` removes.
pub(crate) fn is_js_space(c: char) -> bool {
    matches!(
        c,
        '\u{9}'..='\u{d}'
            | ' '
            | '\u{a0}'
            | '\u{1680}'
            | '\u{2000}'..='\u{200a}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202f}'
            | '\u{205f}'
            | '\u{3000}'
            | '\u{feff}'
    )
}

/// `s.trim()`.
pub(crate) fn js_trim(s: &str) -> &str {
    s.trim_matches(is_js_space)
}

/// `s.replace(/\s+/g, " ")`.
pub(crate) fn collapse_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_space = false;
    for c in s.chars() {
        if is_js_space(c) {
            if !in_space {
                out.push(' ');
            }
            in_space = true;
        } else {
            out.push(c);
            in_space = false;
        }
    }
    out
}

/// `String(x)` (and `${x}`) for a JSON value; `None` is `undefined`.
pub(crate) fn js_string(v: Option<&Value>) -> String {
    match v {
        None => "undefined".into(),
        Some(Value::Null) => "null".into(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Number(n)) => js_number(n.as_f64().unwrap_or(f64::NAN)),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(a)) => a
            .iter()
            .map(|e| match e {
                Value::Null => String::new(),
                e => js_string(Some(e)),
            })
            .collect::<Vec<_>>()
            .join(","),
        Some(Value::Object(_)) => "[object Object]".into(),
    }
}

/// JavaScript truthiness of a JSON value; `None` is `undefined`.
pub(crate) fn js_truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|x| x != 0.0 && !x.is_nan()),
        Some(Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

/// `a === b` between values of two different records (objects never share identity).
pub(crate) fn js_strict_eq(a: Option<&Value>, b: Option<&Value>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(Value::Null), Some(Value::Null)) => true,
        (Some(Value::Bool(x)), Some(Value::Bool(y))) => x == y,
        (Some(Value::Number(x)), Some(Value::Number(y))) => x.as_f64() == y.as_f64(),
        (Some(Value::String(x)), Some(Value::String(y))) => x == y,
        _ => false,
    }
}

/// Equality of two scalars as `JSON.stringify` prints them.
fn scalar_eq(a: Option<&Value>, b: Option<&Value>) -> bool {
    match (a, b) {
        (Some(Value::Number(x)), Some(Value::Number(y))) => x.as_f64() == y.as_f64(),
        (Some(x @ (Value::Array(_) | Value::Object(_))), Some(y)) => {
            to_js_json_compact(x) == to_js_json_compact(y)
        }
        _ => a == b,
    }
}

/// `JSON.stringify(x)`, `None` for `undefined`.
pub(crate) fn json_text(v: Option<&Value>) -> Option<String> {
    v.map(to_js_json_compact)
}

/// `a ?? b ?? ...` over record fields: the first that is neither absent nor null.
pub(crate) fn nullish<'a>(r: &'a Body, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().filter_map(|k| r.get(*k)).find(|v| !v.is_null())
}

/// `String(r.a ?? r.b ?? "")`.
pub(crate) fn string_of(r: &Body, keys: &[&str]) -> String {
    nullish(r, keys).map_or_else(String::new, |v| js_string(Some(v)))
}

/// `Number(s)` for a string.
pub(crate) fn js_to_number(s: &str) -> f64 {
    let t = js_trim(s);
    if t.is_empty() {
        return 0.0;
    }
    for (prefix, radix) in [
        ("0x", 16),
        ("0X", 16),
        ("0o", 8),
        ("0O", 8),
        ("0b", 2),
        ("0B", 2),
    ] {
        if let Some(digits) = t.strip_prefix(prefix) {
            if digits.is_empty() || !digits.chars().all(|c| c.is_digit(radix)) {
                return f64::NAN;
            }
            return digits.chars().fold(0.0, |acc, c| {
                acc * radix as f64 + c.to_digit(radix).unwrap_or(0) as f64
            });
        }
    }
    match t {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    // StrDecimalLiteral: [+-] (digits [. digits] | . digits) [e [+-] digits]
    let b = t.as_bytes();
    let mut i = usize::from(matches!(b.first(), Some(b'+' | b'-')));
    let int_start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    let mut digits = i - int_start;
    if i < b.len() && b[i] == b'.' {
        i += 1;
        let f = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        digits += i - f;
    }
    if digits == 0 {
        return f64::NAN;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            i += 1;
        }
        let e = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == e {
            return f64::NAN;
        }
    }
    if i != b.len() {
        return f64::NAN;
    }
    t.parse().unwrap_or(f64::NAN)
}

/// A canonical array index key (`"0"`, `"12"`), which JavaScript objects list first.
fn is_array_index(key: &str) -> Option<u32> {
    if key.is_empty() || key.len() > 10 || !key.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if key.len() > 1 && key.starts_with('0') {
        return None;
    }
    key.parse::<u64>()
        .ok()
        .filter(|&n| n < u64::from(u32::MAX))
        .map(|n| n as u32)
}

/// Keys in JavaScript property order (array indices ascending, then insertion order).
fn js_order<'a>(keys: impl Iterator<Item = &'a String>) -> Vec<&'a String> {
    let mut ints = Vec::new();
    let mut rest = Vec::new();
    for k in keys {
        match is_array_index(k) {
            Some(i) => ints.push((i, k)),
            None => rest.push(k),
        }
    }
    ints.sort_by_key(|(i, _)| *i);
    ints.into_iter().map(|(_, k)| k).chain(rest).collect()
}

/// `Object.entries(o)`.
pub(crate) fn js_entries(o: &Body) -> impl Iterator<Item = (&String, &Value)> {
    js_key_order(o).into_iter().map(move |k| (k, &o[k]))
}

/// Integers past 2^53 as JavaScript reads them (the nearest double).
pub(crate) fn normalize_numbers(v: &mut Value) {
    const SAFE: u64 = 1 << 53;
    match v {
        Value::Number(n) => {
            let big = n.as_u64().is_some_and(|u| u > SAFE)
                || n.as_i64().is_some_and(|i| i.unsigned_abs() > SAFE);
            if big && let Some(f) = n.as_f64().and_then(serde_json::Number::from_f64) {
                *n = f;
            }
        }
        Value::Array(a) => a.iter_mut().for_each(normalize_numbers),
        Value::Object(o) => o.values_mut().for_each(normalize_numbers),
        _ => {}
    }
}

/// A JavaScript `Map` with string keys: insertion order, a re-set keeps its place.
#[derive(Debug, Clone)]
pub(crate) struct OrderedMap<V> {
    entries: Vec<(String, V)>,
    index: HashMap<String, usize>,
}

impl<V> Default for OrderedMap<V> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            index: HashMap::new(),
        }
    }
}

impl<V> OrderedMap<V> {
    pub(crate) fn new() -> Self {
        Self::default()
    }
    pub(crate) fn get(&self, k: &str) -> Option<&V> {
        self.index.get(k).map(|&i| &self.entries[i].1)
    }
    pub(crate) fn contains(&self, k: &str) -> bool {
        self.index.contains_key(k)
    }
    pub(crate) fn insert(&mut self, k: String, v: V) {
        match self.index.get(&k) {
            Some(&i) => self.entries[i].1 = v,
            None => {
                self.index.insert(k.clone(), self.entries.len());
                self.entries.push((k, v));
            }
        }
    }
    pub(crate) fn entry_or(&mut self, k: &str, f: impl FnOnce() -> V) -> &mut V {
        let i = match self.index.get(k) {
            Some(&i) => i,
            None => {
                self.index.insert(k.to_string(), self.entries.len());
                self.entries.push((k.to_string(), f()));
                self.entries.len() - 1
            }
        };
        &mut self.entries[i].1
    }
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }
    pub(crate) fn iter(&self) -> impl Iterator<Item = (&String, &V)> {
        self.entries.iter().map(|(k, v)| (k, v))
    }
    pub(crate) fn keys(&self) -> impl Iterator<Item = &String> {
        self.entries.iter().map(|(k, _)| k)
    }
    pub(crate) fn values(&self) -> impl Iterator<Item = &V> {
        self.entries.iter().map(|(_, v)| v)
    }
    pub(crate) fn into_values(self) -> impl Iterator<Item = V> {
        self.entries.into_iter().map(|(_, v)| v)
    }
}

/// A JavaScript `Set` of strings: insertion order, no repeats.
#[derive(Debug, Clone, Default)]
pub(crate) struct OrderedSet(OrderedMap<()>);

impl OrderedSet {
    pub(crate) fn new() -> Self {
        Self::default()
    }
    pub(crate) fn add(&mut self, k: &str) {
        if !self.0.contains(k) {
            self.0.insert(k.to_string(), ());
        }
    }
    pub(crate) fn has(&self, k: &str) -> bool {
        self.0.contains(k)
    }
    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }
    pub(crate) fn iter(&self) -> impl Iterator<Item = &String> {
        self.0.keys()
    }
    pub(crate) fn to_vec(&self) -> Vec<String> {
        self.iter().cloned().collect()
    }
}

impl<S: AsRef<str>> FromIterator<S> for OrderedSet {
    fn from_iter<I: IntoIterator<Item = S>>(iter: I) -> Self {
        let mut s = Self::new();
        for k in iter {
            s.add(k.as_ref());
        }
        s
    }
}

/// `path.resolve(p)`: absolute against the working directory, `.` and `..` folded
/// lexically, no trailing separator. Symlinks are not followed.
pub fn resolve_path(p: &Path) -> PathBuf {
    let joined = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(p)
    };
    let mut out = PathBuf::new();
    for c in joined.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Directory entry names, sorted by byte value as Node's `readdirSync` returns them.
fn read_dir_names(dir: &Path) -> std::io::Result<Vec<String>> {
    let mut names: Vec<String> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    Ok(names)
}

fn read_text(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path).map_err(io_err(path))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

// ---------------------------------------------------------------------------
// Snapshots
// ---------------------------------------------------------------------------

/// Files beside the datasets that are derived from them (`DERIVED_FILES`): never
/// snapshotted, listed or diffed unless asked for by name.
pub const DERIVED_FILES: &[&str] = &[crate::search_index::FILE];

fn is_derived(file: &str) -> bool {
    DERIVED_FILES.contains(&file)
}

/// Where per-build copies live: `$SC_SNAPSHOTS_DIR`, else `<root>/snapshots`.
pub fn snapshots_dir(root: &Path) -> PathBuf {
    match std::env::var("SC_SNAPSHOTS_DIR") {
        Ok(v) if !v.is_empty() => PathBuf::from(v),
        _ => root.join("snapshots"),
    }
}

/// One build kept under the snapshots directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LocalSnapshot {
    pub build: String,
    pub path: PathBuf,
    pub datasets: usize,
    pub saved: String,
}

/// `/"_build":\s*"([^"]+)"/` over the first 4096 UTF-16 units.
fn stamp_of(text: &str) -> Option<String> {
    let head = utf16_prefix(text, 4096);
    const NEEDLE: &str = "\"_build\":";
    let mut from = 0;
    while let Some(i) = head[from..].find(NEEDLE) {
        let start = from + i;
        let rest = head[start + NEEDLE.len()..].trim_start_matches(is_js_space);
        if let Some(value) = rest.strip_prefix('"')
            && let Some(end) = value.find('"')
            && end > 0
        {
            return Some(value[..end].to_string());
        }
        from = start + 1;
    }
    None
}

/// `saveSnapshot`: copy the datasets stamped with `build` into `<root>/<build>/`, gzipped.
/// A dataset stamped with another build is skipped; derived files are never kept.
pub fn save_snapshot(
    data_dir: &Path,
    root: &Path,
    build: &str,
    files: Option<&[String]>,
) -> Result<LocalSnapshot> {
    let dir = root.join(build);
    std::fs::create_dir_all(&dir).map_err(io_err(&dir))?;
    let candidates: Vec<String> = match files {
        Some(f) => f.to_vec(),
        None => read_dir_names(data_dir)
            .map_err(io_err(data_dir))?
            .into_iter()
            .filter(|f| f.starts_with("game-") && f.ends_with(".json"))
            .collect(),
    };
    for file in candidates {
        if is_derived(&file) {
            continue;
        }
        let text = read_text(&data_dir.join(&file))?;
        if stamp_of(&text).as_deref() != Some(build) {
            continue;
        }
        let target = dir.join(format!("{file}.gz"));
        let out = std::fs::File::create(&target).map_err(io_err(&target))?;
        // Node's zlib writes OS 3 (Unix) and mtime 0.
        let mut gz = flate2::GzBuilder::new()
            .operating_system(3)
            .write(out, flate2::Compression::new(6));
        gz.write_all(text.as_bytes()).map_err(io_err(&target))?;
        gz.finish().map_err(io_err(&target))?;
    }
    let saved = iso_now();
    let marker = dir.join("_snapshot.json");
    let body = serde_json::json!({ "build": build, "saved": saved });
    std::fs::write(&marker, to_js_json(&body)).map_err(io_err(&marker))?;
    let datasets = read_dir_names(&dir)
        .map_err(io_err(&dir))?
        .iter()
        .filter(|f| f.ends_with(".json.gz"))
        .count();
    Ok(LocalSnapshot {
        build: build.to_string(),
        path: dir,
        datasets,
        saved,
    })
}

/// `snapshotDataset`: snapshot one dataset written outside parse under its own `_build`
/// stamp; `None` for an unstamped file.
pub fn snapshot_dataset(file: &Path, root: &Path) -> Result<Option<LocalSnapshot>> {
    let Some(build) = stamp_of(&read_text(file)?) else {
        return Ok(None);
    };
    let dir = file.parent().unwrap_or(Path::new(""));
    let name = file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    save_snapshot(dir, root, &build, Some(&[name])).map(Some)
}

/// `/^game-.*\.json(\.gz)?$/`.
fn is_dataset_name(f: &str) -> bool {
    let Some(rest) = f.strip_prefix("game-") else {
        return false;
    };
    let mid = rest
        .strip_suffix(".json.gz")
        .or_else(|| rest.strip_suffix(".json"));
    mid.is_some_and(|m| !m.contains(['\n', '\r', '\u{2028}', '\u{2029}']))
}

/// `listSnapshots`: local snapshots, newest build first by version order.
pub fn list_snapshots(root: &Path) -> Result<Vec<LocalSnapshot>> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut entries: Vec<(String, PathBuf)> = Vec::new();
    for e in std::fs::read_dir(root).map_err(io_err(root))? {
        let e = e.map_err(io_err(root))?;
        if e.file_type().is_ok_and(|t| t.is_dir()) {
            entries.push((e.file_name().to_string_lossy().into_owned(), e.path()));
        }
    }
    entries.sort();
    let mut out = Vec::new();
    for (name, _) in entries {
        let dir = root.join(&name);
        let saved = std::fs::read(dir.join("_snapshot.json"))
            .ok()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            .and_then(|v| v.get("saved").and_then(Value::as_str).map(str::to_string))
            .unwrap_or_default();
        let datasets = read_dir_names(&dir)
            .map_err(io_err(&dir))?
            .iter()
            .filter(|f| is_dataset_name(f))
            .count();
        if datasets > 0 {
            out.push(LocalSnapshot {
                build: name,
                path: dir,
                datasets,
                saved,
            });
        }
    }
    out.sort_by(|a, b| compare_versions(&b.build, &a.build));
    Ok(out)
}

/// `compareVersions`: numeric part by part (4.10.196 after 4.9.2), else `localeCompare`.
pub fn compare_versions(a: &str, b: &str) -> Ordering {
    let pa: Vec<f64> = a.split('.').map(js_to_number).collect();
    let pb: Vec<f64> = b.split('.').map(js_to_number).collect();
    for i in 0..pa.len().max(pb.len()) {
        let d = pa.get(i).copied().unwrap_or(0.0) - pb.get(i).copied().unwrap_or(0.0);
        if d != 0.0 && !d.is_nan() {
            return if d > 0.0 {
                Ordering::Greater
            } else {
                Ordering::Less
            };
        }
    }
    locale_compare(a, b)
}

// ---------------------------------------------------------------------------
// Build sources
// ---------------------------------------------------------------------------

/// Where a build's datasets come from.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BuildSource {
    /// A directory of `game-*.json` (or `.json.gz`); `fallback` serves any dataset it lacks
    /// (a snapshot's published twin).
    Dir {
        path: PathBuf,
        fallback: Option<Box<BuildSource>>,
    },
    /// A tag of the dumps checkout.
    Tag { tag: String, repo_dir: PathBuf },
}

impl BuildSource {
    /// A directory source without fallback.
    pub fn dir(path: impl Into<PathBuf>) -> Self {
        Self::Dir {
            path: path.into(),
            fallback: None,
        }
    }

    /// `describeSource`: the directory path, or `tag <tag>`.
    pub fn describe(&self) -> String {
        match self {
            Self::Dir { path, .. } => path.to_string_lossy().into_owned(),
            Self::Tag { tag, .. } => format!("tag {tag}"),
        }
    }

    fn fallback(&self) -> Option<&BuildSource> {
        match self {
            Self::Dir { fallback, .. } => fallback.as_deref(),
            Self::Tag { .. } => None,
        }
    }
}

/// The dumps checkout `publish-dump` shares (`DumpsRepoOptions`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DumpsRepo {
    pub work_dir: PathBuf,
    pub remote: String,
}

fn env_or(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

/// `dumpsRepoOptions`: `$SC_DUMPS_WORKDIR` or `<root>/.publish-repo`, remote from
/// `$SC_DUMPS_REMOTE` or `$SC_DUMPS_REPO`.
pub fn dumps_repo_options(root: &Path) -> DumpsRepo {
    let repo = env_or("SC_DUMPS_REPO").unwrap_or_else(|| "voidput/sc-gamedata-dumps".into());
    DumpsRepo {
        work_dir: env_or("SC_DUMPS_WORKDIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| root.join(".publish-repo")),
        remote: env_or("SC_DUMPS_REMOTE").unwrap_or_else(|| format!("git@github.com:{repo}.git")),
    }
}

/// `git <args>` in `repo_dir`, trimmed stdout; `Err` with stderr when it fails.
fn git(repo_dir: &Path, args: &[&str]) -> std::result::Result<String, String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(repo_dir)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(js_trim(&String::from_utf8_lossy(&out.stdout)).to_string())
}

/// A tag of the dumps checkout.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PublishedBuild {
    pub version: String,
    pub date: String,
}

/// `listPublishedBuilds`: tags of the dumps checkout, newest first. `fetch` clones the
/// checkout when missing, else fetches its tags.
pub fn list_published_builds(repo: &DumpsRepo, fetch: bool) -> Result<Vec<PublishedBuild>> {
    let fail = |what: &str, e: String| Error::Message(format!("git {what} failed: {e}"));
    if !repo.work_dir.join(".git").exists() {
        if !fetch {
            return Ok(Vec::new());
        }
        std::fs::create_dir_all(&repo.work_dir).map_err(io_err(&repo.work_dir))?;
        let status = Command::new("git")
            .arg("clone")
            .arg("--quiet")
            .arg(&repo.remote)
            .arg(&repo.work_dir)
            .status()
            .map_err(|e| fail("clone", e.to_string()))?;
        if !status.success() {
            return Err(fail("clone", status.to_string()));
        }
    } else if fetch {
        git(&repo.work_dir, &["fetch", "--quiet", "--tags", "origin"])
            .map_err(|e| fail("fetch", e))?;
    }
    let out = git(
        &repo.work_dir,
        &[
            "for-each-ref",
            "--sort=-creatordate",
            "--format=%(refname:short)\t%(creatordate:short)",
            "refs/tags",
        ],
    )
    .map_err(|e| fail("for-each-ref", e))?;
    if out.is_empty() {
        return Ok(Vec::new());
    }
    Ok(out
        .split('\n')
        .map(|line| {
            let mut parts = line.split('\t');
            PublishedBuild {
                version: parts.next().unwrap_or_default().to_string(),
                date: parts.next().unwrap_or_default().to_string(),
            }
        })
        .collect())
}

/// The published tag of exactly this build in the existing checkout; never fetches.
fn published_tag(build: &str, repo: &DumpsRepo) -> Option<BuildSource> {
    list_published_builds(repo, false)
        .ok()?
        .iter()
        .any(|b| b.version == build)
        .then(|| BuildSource::Tag {
            tag: build.to_string(),
            repo_dir: repo.work_dir.clone(),
        })
}

/// `resolveSource`: `local` is `data_dir` (already resolved by the caller), an existing
/// directory is used as is, then a local snapshot (by build or prefix, with the published
/// tag of the same build as fallback), then a dumps tag (fetched once if missing; a
/// prefix such as `4.10` picks the newest match).
pub fn resolve_source(
    spec: &str,
    data_dir: &Path,
    repo: &DumpsRepo,
    snapshots: Option<&Path>,
) -> Result<BuildSource> {
    if spec == "local" {
        return Ok(BuildSource::dir(data_dir));
    }
    if Path::new(spec).is_dir() {
        return Ok(BuildSource::dir(resolve_path(Path::new(spec))));
    }
    if let Some(root) = snapshots.filter(|s| !s.as_os_str().is_empty()) {
        let local = list_snapshots(root)?;
        let prefix = format!("{spec}.");
        let hit = local
            .iter()
            .find(|s| s.build == spec)
            .or_else(|| local.iter().find(|s| s.build.starts_with(&prefix)));
        if let Some(hit) = hit {
            return Ok(BuildSource::Dir {
                path: hit.path.clone(),
                fallback: published_tag(&hit.build, repo).map(Box::new),
            });
        }
    }
    let mut builds = list_published_builds(repo, false)?;
    if !builds.iter().any(|b| b.version == spec) {
        builds = list_published_builds(repo, true)?;
    }
    if !builds.iter().any(|b| b.version == spec) {
        let prefix = format!("{spec}.");
        if let Some(m) = builds.iter().find(|b| b.version.starts_with(&prefix)) {
            return Ok(BuildSource::Tag {
                tag: m.version.clone(),
                repo_dir: repo.work_dir.clone(),
            });
        }
        let list: Vec<&str> = builds.iter().map(|b| b.version.as_str()).collect();
        let list = if list.is_empty() {
            "(none)".to_string()
        } else {
            list.join(", ")
        };
        return Err(Error::Message(format!(
            "\"{spec}\" is neither a directory nor a published build. Published: {list}. Use \"local\" for {}.",
            data_dir.display()
        )));
    }
    Ok(BuildSource::Tag {
        tag: spec.to_string(),
        repo_dir: repo.work_dir.clone(),
    })
}

/// One dataset file read from a source.
#[derive(Debug, Clone)]
pub struct Snapshot {
    /// Where the file was actually read from; differs from the source on a fallback.
    pub source: String,
    pub build: Option<String>,
    pub extracted: Option<String>,
    pub body: Body,
}

/// `META_KEYS`: header and summary keys, never diffed as collections.
pub const META_KEYS: &[&str] = &[
    "_source",
    "_extracted",
    "_build",
    "_redacted",
    "_assumptions",
    "summary",
];

/// `loadSnapshot`: one dataset file from a source, `None` when the build lacks it.
pub fn load_snapshot(source: &BuildSource, file: &str) -> Result<Option<Snapshot>> {
    let text = match source {
        BuildSource::Dir { path, fallback } => {
            let full = path.join(file);
            let gz = PathBuf::from(format!("{}.gz", full.to_string_lossy()));
            if full.exists() {
                read_text(&full)?
            } else if gz.exists() {
                let raw = std::fs::read(&gz).map_err(io_err(&gz))?;
                let mut bytes = Vec::new();
                flate2::read::MultiGzDecoder::new(&raw[..])
                    .read_to_end(&mut bytes)
                    .map_err(io_err(&gz))?;
                String::from_utf8_lossy(&bytes).into_owned()
            } else {
                return match fallback {
                    Some(f) => load_snapshot(f, file),
                    None => Ok(None),
                };
            }
        }
        BuildSource::Tag { tag, repo_dir } => {
            match git(repo_dir, &["show", &format!("{tag}:data/{file}")]) {
                Ok(t) => t,
                Err(_) => return Ok(None),
            }
        }
    };
    let src = source;
    let mut parsed: Value = serde_json::from_str(&text).map_err(|source| Error::Json {
        file: format!("{}/{file}", src.describe()),
        source,
    })?;
    normalize_numbers(&mut parsed);
    let body = match parsed {
        Value::Object(m) => m,
        _ => Map::new(),
    };
    let text_field = |k: &str| body.get(k).and_then(Value::as_str).map(str::to_string);
    Ok(Some(Snapshot {
        source: source.describe(),
        build: text_field("_build"),
        extracted: text_field("_extracted"),
        body,
    }))
}

// ---------------------------------------------------------------------------
// Redaction alignment
// ---------------------------------------------------------------------------

/// `MAX_PUBLISHED_VALUE`: the longest string value a published table keeps (UTF-16 units).
pub const MAX_PUBLISHED_VALUE: usize = 120;

/// `isPublishedString`: whether publish keeps this localisation entry: a non-empty string
/// of at most [`MAX_PUBLISHED_VALUE`] units under a name-shaped key outside the prose
/// namespaces (the rule of `scripts/redact.mjs`, as [`redaction::keeps`] implements it).
pub fn is_published_string(key: &str, value: Option<&Value>) -> bool {
    let Some(Value::String(s)) = value else {
        return false;
    };
    !s.is_empty() && utf16_len(s) <= MAX_PUBLISHED_VALUE && redaction::keeps(key, s)
}

fn is_redacted_table(body: &Body) -> bool {
    js_truthy(body.get("_redacted"))
        && matches!(
            body.get("strings"),
            Some(Value::Object(_) | Value::Array(_))
        )
}

/// Two datasets made comparable; `note` says what was cut.
#[derive(Debug, Clone)]
pub struct Aligned<'a> {
    pub before: Cow<'a, Body>,
    pub after: Cow<'a, Body>,
    pub note: Option<String>,
}

/// `alignRedaction`: when exactly one side is the published, redacted form (a string
/// table cut to name keys, or a dataset without its prose), cut the other the same way.
pub fn align_redaction<'a>(before: &'a Body, after: &'a Body) -> Aligned<'a> {
    if let Some(prose) = align_prose(before, after) {
        return prose;
    }
    let unchanged = || Aligned {
        before: Cow::Borrowed(before),
        after: Cow::Borrowed(after),
        note: None,
    };
    let ra = is_redacted_table(before);
    let rb = is_redacted_table(after);
    if ra == rb {
        return unchanged();
    }
    let full = if ra { after } else { before };
    let strings = match full.get("strings") {
        Some(Value::Object(o)) => {
            let mut kept = Map::new();
            for (k, v) in js_entries(o) {
                if is_published_string(k, Some(v)) {
                    kept.insert(k.clone(), v.clone());
                }
            }
            kept
        }
        // `Object.entries` of an array: index keys, never name-shaped.
        Some(Value::Array(_)) => Map::new(),
        _ => return unchanged(),
    };
    let mut cut = Map::new();
    let mut strings = Some(strings);
    for (k, v) in full {
        if k == "strings" {
            cut.insert(k.clone(), Value::Object(strings.take().unwrap_or_default()));
        } else {
            cut.insert(k.clone(), v.clone());
        }
    }
    let (older, newer) = if ra {
        ("older", "newer")
    } else {
        ("newer", "older")
    };
    let note = format!(
        "{older} string table is the redacted published one; the {newer} table is compared on published name keys only"
    );
    if ra {
        Aligned {
            before: Cow::Borrowed(before),
            after: Cow::Owned(cut),
            note: Some(note),
        }
    } else {
        Aligned {
            before: Cow::Owned(cut),
            after: Cow::Borrowed(after),
            note: Some(note),
        }
    }
}

/// Fields a published dataset dropped, from its `_redacted` list.
fn prose_redaction(body: &Body) -> Option<Vec<String>> {
    match body.get("_redacted") {
        Some(Value::Array(a)) => Some(
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect(),
        ),
        _ => None,
    }
}

/// `"title over N characters"` -> N.
fn title_limit(field: &str) -> Option<&str> {
    let digits = field
        .strip_prefix("title over ")?
        .strip_suffix(" characters")?;
    (!digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())).then_some(digits)
}

/// `alignProse`: a full dataset cut to what publish keeps of it, when exactly one side
/// lists its dropped prose fields in `_redacted`.
fn align_prose<'a>(before: &'a Body, after: &'a Body) -> Option<Aligned<'a>> {
    let ra = prose_redaction(before);
    let rb = prose_redaction(after);
    if ra.is_none() == rb.is_none() {
        return None;
    }
    let older = ra.is_some();
    let list = ra.or(rb).unwrap_or_default();
    let full = if older { after } else { before };
    let fields: Vec<&str> = list.iter().map(String::as_str).collect();
    let limit = list.iter().find_map(|f| title_limit(f));
    let mut cut = Map::new();
    for (k, v) in full {
        if !fields.contains(&k.as_str()) {
            cut.insert(k.clone(), redaction::strip_fields(v, &fields));
        }
    }
    if let Some(limit) = limit
        && let Some(Value::Array(missions)) = cut.get_mut("missions")
    {
        let max = limit.parse::<usize>().unwrap_or(usize::MAX);
        for m in missions.iter_mut() {
            if let Some(short) = redaction::without_long_title(m, max) {
                *m = short;
            }
        }
    }
    let (o, n) = if older {
        ("older", "newer")
    } else {
        ("newer", "older")
    };
    let note = format!(
        "{o} side is the published dataset without its prose ({}); the {n} side is compared without it too",
        list.join(", ")
    );
    Some(if older {
        Aligned {
            before: Cow::Borrowed(before),
            after: Cow::Owned(cut),
            note: Some(note),
        }
    } else {
        Aligned {
            before: Cow::Owned(cut),
            after: Cow::Borrowed(after),
            note: Some(note),
        }
    })
}

// ---------------------------------------------------------------------------
// Collections and entries
// ---------------------------------------------------------------------------

/// `listDatasetFiles`: dataset files in either source, counting a snapshot's fallback;
/// derived files excluded. Sorted.
pub fn list_dataset_files(a: &BuildSource, b: &BuildSource) -> Vec<String> {
    let mut sources = Vec::new();
    for start in [a, b] {
        let mut s = Some(start);
        while let Some(src) = s {
            sources.push(src);
            s = src.fallback();
        }
    }
    let mut files = OrderedSet::new();
    for source in sources {
        let names: Vec<String> = match source {
            BuildSource::Dir { path, .. } => {
                if path.exists() {
                    read_dir_names(path).unwrap_or_default()
                } else {
                    Vec::new()
                }
            }
            BuildSource::Tag { tag, repo_dir } => {
                match git(
                    repo_dir,
                    &["ls-tree", "--name-only", &format!("{tag}:data")],
                ) {
                    Ok(out) => out.split('\n').map(str::to_string).collect(),
                    Err(_) => Vec::new(),
                }
            }
        };
        for name in names {
            let plain = name.strip_suffix(".gz").unwrap_or(&name);
            if plain.starts_with("game-") && plain.ends_with(".json") && !is_derived(plain) {
                files.add(plain);
            }
        }
    }
    let mut out = files.to_vec();
    sort_utf16(&mut out);
    out
}

/// `diffableCollections`: the arrays and maps of records in a dump, without meta keys and
/// derived `by*` indexes.
pub fn diffable_collections(body: &Body) -> Vec<String> {
    js_key_order(body)
        .into_iter()
        .filter(|k| !META_KEYS.contains(&k.as_str()) && !k.starts_with("by"))
        .filter(|k| matches!(body[*k], Value::Array(_) | Value::Object(_)))
        .cloned()
        .collect()
}

/// `[key, record]` pairs: array positions, or object entries in JavaScript order.
pub type Entry<'a> = (String, &'a Value);

/// `entriesOf`.
pub fn entries_of(collection: Option<&Value>) -> Vec<Entry<'_>> {
    match collection {
        Some(Value::Array(a)) => a
            .iter()
            .enumerate()
            .map(|(i, v)| (i.to_string(), v))
            .collect(),
        Some(Value::Object(o)) => js_entries(o).map(|(k, v)| (k.clone(), v)).collect(),
        _ => Vec::new(),
    }
}

/// `keyOf`: stable identity of a record across patches. Display names do not qualify.
pub fn key_of(entry: &Value) -> Option<&str> {
    let e = entry.as_object()?;
    ["recordName", "className", "id", "reference", "key", "name"]
        .iter()
        .find_map(|f| e.get(*f).and_then(Value::as_str).filter(|s| !s.is_empty()))
}

fn index_by<'a>(entries: &[Entry<'a>]) -> OrderedMap<&'a Value> {
    let mut map = OrderedMap::new();
    for (key, record) in entries {
        map.insert(key_of(record).unwrap_or(key).to_string(), *record);
    }
    map
}

fn flatten_into<'a>(value: &'a Value, prefix: String, out: &mut OrderedMap<&'a Value>) {
    match value {
        Value::Array(a) => {
            for (i, v) in a.iter().enumerate() {
                flatten_into(v, format!("{prefix}[{i}]"), out);
            }
        }
        Value::Object(o) => {
            for (k, v) in js_entries(o) {
                let p = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                flatten_into(v, p, out);
            }
        }
        _ => out.insert(prefix, value),
    }
}

fn flatten_ref(value: Option<&Value>) -> OrderedMap<&Value> {
    let mut out = OrderedMap::new();
    if let Some(v) = value {
        flatten_into(v, String::new(), &mut out);
    }
    out
}

/// `flatten`: leaves as dotted paths (`a.b[0]`), in JavaScript key order.
pub fn flatten(value: &Value) -> Map<String, Value> {
    let flat = flatten_ref(Some(value));
    let mut out = Map::new();
    for k in js_order(flat.keys()) {
        out.insert(k.clone(), (*flat.get(k).unwrap_or(&&Value::Null)).clone());
    }
    out
}

/// One leaf that moved; `None` is a side without the field.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FieldChange {
    pub field: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<Value>,
}

/// A record's leaves (`flatten` of an object).
fn flatten_body(o: &Body) -> OrderedMap<&Value> {
    let mut out = OrderedMap::new();
    for (k, v) in js_entries(o) {
        flatten_into(v, k.clone(), &mut out);
    }
    out
}

/// A changed leaf: field, old value, new value (`None` where a side lacks it).
pub(crate) type FieldMove<'a> = (String, Option<&'a Value>, Option<&'a Value>);

/// Changed leaves in the order the flattened objects list them (not sorted).
pub(crate) fn diff_fields<'a>(
    before: Option<&'a Value>,
    after: Option<&'a Value>,
) -> Vec<FieldMove<'a>> {
    diff_flat(&flatten_ref(before), &flatten_ref(after))
}

/// [`diff_fields`] of two records.
pub(crate) fn diff_body_fields<'a>(before: &'a Body, after: &'a Body) -> Vec<FieldMove<'a>> {
    diff_flat(&flatten_body(before), &flatten_body(after))
}

fn diff_flat<'a>(fa: &OrderedMap<&'a Value>, fb: &OrderedMap<&'a Value>) -> Vec<FieldMove<'a>> {
    let mut keys = js_order(fa.keys());
    keys.extend(js_order(fb.keys()).into_iter().filter(|k| !fa.contains(k)));
    let mut out = Vec::new();
    for field in keys {
        let a = fa.get(field).copied();
        let b = fb.get(field).copied();
        if !scalar_eq(a, b) {
            out.push((field.clone(), a, b));
        }
    }
    out
}

/// `diffEntry`: every changed leaf between two records, sorted by field (`localeCompare`).
pub fn diff_entry(before: Option<&Value>, after: Option<&Value>) -> Vec<FieldChange> {
    sorted_changes(diff_fields(before, after))
}

/// [`diff_entry`] of two records.
pub fn diff_entry_bodies(before: &Body, after: &Body) -> Vec<FieldChange> {
    sorted_changes(diff_body_fields(before, after))
}

fn sorted_changes(moves: Vec<FieldMove<'_>>) -> Vec<FieldChange> {
    let mut changes: Vec<FieldChange> = moves
        .into_iter()
        .map(|(field, a, b)| FieldChange {
            field,
            from: a.cloned(),
            to: b.cloned(),
        })
        .collect();
    changes.sort_by(|a, b| locale_compare(&a.field, &b.field));
    changes
}

/// `labelOf`: `Name (key)`, or the key when the record has no usable name.
pub fn label_of(entry: &Value, key: &str) -> String {
    let Some(e) = entry.as_object() else {
        return key.to_string();
    };
    for field in ["name", "displayName", "title"] {
        if let Some(Value::String(v)) = e.get(field)
            && !v.is_empty()
            && !v.contains("PLACEHOLDER")
        {
            return if v == key {
                key.to_string()
            } else {
                format!("{v} ({key})")
            };
        }
    }
    key.to_string()
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangedEntry {
    pub key: String,
    pub item: String,
    pub change_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changes: Option<Vec<FieldChange>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub struct Summary {
    pub added: usize,
    pub removed: usize,
    pub changed: usize,
    pub unchanged: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CollectionDiff {
    pub collection: String,
    pub summary: Summary,
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub changed: Vec<ChangedEntry>,
}

/// `DiffOptions` plus `collection`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DiffOptions {
    /// Case-insensitive substring over keys and labels; implies field detail.
    pub item: Option<String>,
    /// Only changes touching these fields (prefix match on the flat path).
    pub fields: Option<Vec<String>>,
    /// Most-changed entries to keep per collection (default 50); also the rendered list
    /// length (default 20).
    pub limit: Option<usize>,
    /// Per-field changes even without an item filter.
    pub detail: bool,
    /// One collection of the dataset instead of all.
    pub collection: Option<String>,
}

/// `diffCollection`.
pub fn diff_collection(
    collection: &str,
    before_entries: &[Entry<'_>],
    after_entries: &[Entry<'_>],
    options: &DiffOptions,
) -> CollectionDiff {
    let before = index_by(before_entries);
    let after = index_by(after_entries);
    let needle = options.item.as_ref().map(|s| s.to_lowercase());
    let limit = options.limit.unwrap_or(50);
    let detail = options.detail || needle.is_some();

    let wanted = |key: &str, entry: &Value| match &needle {
        None => true,
        Some(n) => {
            key.to_lowercase().contains(n) || label_of(entry, key).to_lowercase().contains(n)
        }
    };
    let touches_wanted = |field: &str| match &options.fields {
        None => true,
        Some(fs) => fs.iter().any(|f| {
            field == f
                || field
                    .strip_prefix(f.as_str())
                    .is_some_and(|rest| rest.starts_with('.') || rest.starts_with('['))
        }),
    };

    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut changed = Vec::new();
    let mut unchanged = 0;
    for (key, entry) in after.iter() {
        if !before.contains(key) && wanted(key, entry) {
            added.push(label_of(entry, key));
        }
    }
    for (key, entry) in before.iter() {
        if !after.contains(key) && wanted(key, entry) {
            removed.push(label_of(entry, key));
        }
    }
    for (key, b) in before.iter() {
        let Some(a) = after.get(key) else { continue };
        if !wanted(key, a) {
            continue;
        }
        let changes: Vec<FieldChange> = diff_entry(Some(b), Some(a))
            .into_iter()
            .filter(|c| touches_wanted(&c.field))
            .collect();
        if changes.is_empty() {
            unchanged += 1;
            continue;
        }
        changed.push(ChangedEntry {
            key: key.clone(),
            item: label_of(a, key),
            change_count: changes.len(),
            changes: detail.then_some(changes),
        });
    }
    changed.sort_by(|x, y| {
        y.change_count
            .cmp(&x.change_count)
            .then_with(|| locale_compare(&x.item, &y.item))
    });
    let summary = Summary {
        added: added.len(),
        removed: removed.len(),
        changed: changed.len(),
        unchanged,
    };
    sort_utf16(&mut added);
    sort_utf16(&mut removed);
    changed.truncate(limit);
    CollectionDiff {
        collection: collection.to_string(),
        summary,
        added,
        removed,
        changed,
    }
}

/// One side of a dataset diff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiffSide {
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build: Option<String>,
}

/// Which side lacks the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Missing {
    From,
    To,
    Both,
}

/// `DatasetDiff`. Fields serialise in the order the TypeScript assigns them.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DatasetDiff {
    pub file: String,
    pub from: DiffSide,
    pub to: DiffSide,
    pub collections: Vec<CollectionDiff>,
    /// Caveats: a side served by a snapshot's fallback, a redacted side cut to match.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub missing: Option<Missing>,
    /// Records per collection on the side that has the file, when only one does.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub counts: Option<Map<String, Value>>,
}

fn collection_counts(body: &Body, only: Option<&str>) -> Map<String, Value> {
    let names: Vec<String> = match only {
        Some(n) => body
            .contains_key(n)
            .then(|| n.to_string())
            .into_iter()
            .collect(),
        None => diffable_collections(body),
    };
    names
        .into_iter()
        .map(|name| {
            let n = entries_of(body.get(&name)).len();
            (name, Value::from(n))
        })
        .collect()
}

/// `diffDataset`: one dataset file between two builds.
pub fn diff_dataset(
    file: &str,
    from: &BuildSource,
    to: &BuildSource,
    options: &DiffOptions,
) -> Result<DatasetDiff> {
    let before = load_snapshot(from, file)?;
    let after = load_snapshot(to, file)?;
    let side = |s: &Option<Snapshot>, src: &BuildSource| DiffSide {
        source: s
            .as_ref()
            .map_or_else(|| src.describe(), |s| s.source.clone()),
        build: s.as_ref().and_then(|s| s.build.clone()),
    };
    let mut result = DatasetDiff {
        file: file.to_string(),
        from: side(&before, from),
        to: side(&after, to),
        collections: Vec::new(),
        notes: None,
        missing: None,
        counts: None,
    };
    let mut notes = Vec::new();
    if let Some(b) = &before
        && b.source != from.describe()
    {
        notes.push(format!(
            "older side read from {} (the snapshot lacks this dataset)",
            b.source
        ));
    }
    if let Some(a) = &after
        && a.source != to.describe()
    {
        notes.push(format!(
            "newer side read from {} (the snapshot lacks this dataset)",
            a.source
        ));
    }
    let (Some(before), Some(after)) = (&before, &after) else {
        if !notes.is_empty() {
            result.notes = Some(notes);
        }
        result.missing = Some(match (&before, &after) {
            (Some(_), _) => Missing::To,
            (_, Some(_)) => Missing::From,
            _ => Missing::Both,
        });
        if let Some(present) = before.as_ref().or(after.as_ref()) {
            result.counts = Some(collection_counts(
                &present.body,
                options.collection.as_deref(),
            ));
        }
        return Ok(result);
    };

    let aligned = align_redaction(&before.body, &after.body);
    if let Some(n) = &aligned.note {
        notes.push(n.clone());
    }
    if !notes.is_empty() {
        result.notes = Some(notes);
    }
    let names: Vec<String> = match &options.collection {
        Some(c) => vec![c.clone()],
        None => diffable_collections(&aligned.before)
            .into_iter()
            .chain(diffable_collections(&aligned.after))
            .collect::<OrderedSet>()
            .to_vec(),
    };
    for name in names {
        if !aligned.before.contains_key(&name) && !aligned.after.contains_key(&name) {
            return Err(Error::Message(format!(
                "{file} has no collection \"{name}\". Available: {}",
                diffable_collections(&aligned.after).join(", ")
            )));
        }
        result.collections.push(diff_collection(
            &name,
            &entries_of(aligned.before.get(&name)),
            &entries_of(aligned.after.get(&name)),
            options,
        ));
    }
    Ok(result)
}

/// `datasetFile`: `weapons`, `game-weapons` and `game-weapons.json` all name
/// `game-weapons.json`; bare `strings` is the English table.
pub fn dataset_file(name: &str) -> String {
    let base = name.strip_suffix(".json").unwrap_or(name);
    let base = base.strip_prefix("game-").unwrap_or(base);
    let base = if base == "strings" {
        "strings-english"
    } else {
        base
    };
    format!("game-{base}.json")
}

/// The `diff` command: every requested dataset (one by name, else all of either build).
pub fn diff(
    from: &BuildSource,
    to: &BuildSource,
    dataset: Option<&str>,
    options: &DiffOptions,
) -> Result<Vec<DatasetDiff>> {
    let files = match dataset {
        Some(d) => vec![dataset_file(d)],
        None => list_dataset_files(from, to),
    };
    if files.is_empty() {
        return Err(Error::Message(
            "Neither build has any game-*.json dataset.".into(),
        ));
    }
    files
        .iter()
        .map(|f| diff_dataset(f, from, to, options))
        .collect()
}

/// `diff --json`: `JSON.stringify(results.length === 1 ? results[0] : results, null, 2)`
/// (no trailing newline; the CLI prints it with one).
pub fn diff_json(results: &[DatasetDiff]) -> String {
    let value = if results.len() == 1 {
        serde_json::to_value(&results[0])
    } else {
        serde_json::to_value(results)
    };
    to_js_json(&value.unwrap_or(Value::Null))
}

/// `diff` text output: [`render_diff`] of each dataset followed by an empty line, exactly
/// as the CLI prints it (print without adding a newline).
pub fn render_diffs(results: &[DatasetDiff], limit: Option<usize>) -> String {
    let mut out = String::new();
    for r in results {
        out.push_str(&render_diff(r, limit));
        out.push_str("\n\n");
    }
    out
}

fn fmt(value: Option<&Value>) -> String {
    match value {
        None => "(absent)".into(),
        Some(v @ Value::String(_)) => to_js_json_compact(v),
        Some(v) => js_string(Some(v)),
    }
}

/// `renderDiff`: plain text for the terminal (no trailing newline). `limit` (default 20)
/// caps each list.
pub fn render_diff(diff: &DatasetDiff, limit: Option<usize>) -> String {
    let mut lines = Vec::new();
    let name = |s: &DiffSide| s.build.clone().unwrap_or_else(|| s.source.clone());
    lines.push(format!(
        "{}  {} -> {}",
        diff.file,
        name(&diff.from),
        name(&diff.to)
    ));
    for note in diff.notes.iter().flatten() {
        lines.push(format!("  note: {note}"));
    }
    if let Some(missing) = diff.missing {
        let empty = Map::new();
        let counts = diff.counts.as_ref().unwrap_or(&empty);
        let total: u64 = counts.values().filter_map(Value::as_u64).sum();
        lines.push(match missing {
            Missing::Both => "  (not present in either build)".to_string(),
            Missing::From => {
                format!("  new dataset (not present in the older build): {total} record(s)")
            }
            Missing::To => format!(
                "  dropped (not present in the newer build): {total} record(s) in the older build"
            ),
        });
        for (name, n) in js_entries(counts) {
            lines.push(format!("  {name}: {}", js_string(Some(n))));
        }
        return lines.join("\n");
    }
    let list_limit = limit.unwrap_or(20);
    for c in &diff.collections {
        let s = c.summary;
        let quiet = s.added + s.removed + s.changed == 0;
        lines.push(format!(
            "  {}: +{} -{} ~{} ={}{}",
            c.collection,
            s.added,
            s.removed,
            s.changed,
            s.unchanged,
            if quiet { "  (no change)" } else { "" }
        ));
        for item in c.added.iter().take(list_limit) {
            lines.push(format!("    + {item}"));
        }
        if c.added.len() > list_limit {
            lines.push(format!(
                "    + ... {} more added",
                c.added.len() - list_limit
            ));
        }
        for item in c.removed.iter().take(list_limit) {
            lines.push(format!("    - {item}"));
        }
        if c.removed.len() > list_limit {
            lines.push(format!(
                "    - ... {} more removed",
                c.removed.len() - list_limit
            ));
        }
        for entry in c.changed.iter().take(list_limit) {
            lines.push(format!(
                "    ~ {}  ({} field{})",
                entry.item,
                entry.change_count,
                if entry.change_count == 1 { "" } else { "s" }
            ));
            for change in entry.changes.iter().flatten() {
                lines.push(format!(
                    "        {}: {} -> {}",
                    change.field,
                    fmt(change.from.as_ref()),
                    fmt(change.to.as_ref())
                ));
            }
        }
        if c.changed.len() > list_limit {
            lines.push(format!(
                "    ~ ... {} more changed",
                c.changed.len() - list_limit
            ));
        } else if s.changed > c.changed.len() {
            lines.push(format!(
                "    ~ ... {} more changed (raise --limit)",
                s.changed - c.changed.len()
            ));
        }
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn weapon(record: &str, name: &str, dps: f64) -> Value {
        json!({ "name": name, "recordName": record, "dps": dps, "ammo": { "damage": { "energy": dps / 10.0 } } })
    }

    fn obj(v: Value) -> Body {
        v.as_object().unwrap().clone()
    }

    fn entries(v: &[Value]) -> Vec<Entry<'_>> {
        v.iter()
            .enumerate()
            .map(|(i, v)| (i.to_string(), v))
            .collect()
    }

    fn change(field: &str, from: Option<Value>, to: Option<Value>) -> FieldChange {
        FieldChange {
            field: field.into(),
            from,
            to,
        }
    }

    #[test]
    fn keys_on_record_name_before_anything_renamable() {
        let w = json!({"name": "M7A Cannon", "recordName": "EntityClassDefinition.BEHR_LaserCannon_S5"});
        assert_eq!(
            key_of(&w),
            Some("EntityClassDefinition.BEHR_LaserCannon_S5")
        );
        assert_eq!(key_of(&json!({"name": "Only a name"})), Some("Only a name"));
        assert_eq!(key_of(&json!("not an object")), None);
    }

    #[test]
    fn flattens_nested_fields_into_dotted_paths() {
        let flat = flatten(&json!({"a": {"b": [1, {"c": 2}]}, "d": "x"}));
        assert_eq!(
            Value::Object(flat),
            json!({"a.b[0]": 1, "a.b[1].c": 2, "d": "x"})
        );
    }

    #[test]
    fn diff_entry_reports_each_changed_leaf() {
        let changes = diff_entry(
            Some(&json!({"dps": 100, "ammo": {"damage": {"energy": 10}}, "gone": true})),
            Some(&json!({"dps": 120, "ammo": {"damage": {"energy": 12}}, "fresh": 1})),
        );
        assert_eq!(
            changes,
            vec![
                change("ammo.damage.energy", Some(json!(10)), Some(json!(12))),
                change("dps", Some(json!(100)), Some(json!(120))),
                change("fresh", None, Some(json!(1))),
                change("gone", Some(json!(true)), None),
            ]
        );
        // Numbers compare as JavaScript reads them: 1 and 1.0 are the same.
        assert!(diff_entry(Some(&json!({"a": 1})), Some(&json!({"a": 1.0}))).is_empty());
    }

    fn collections() -> (Vec<Value>, Vec<Value>) {
        (
            vec![
                weapon("W.A", "Alpha", 100.0),
                weapon("W.B", "Bravo", 200.0),
                weapon("W.C", "Charlie", 300.0),
            ],
            vec![
                weapon("W.A", "Alpha Renamed", 100.0),
                weapon("W.B", "Bravo", 250.0),
                weapon("W.D", "Delta", 400.0),
            ],
        )
    }

    #[test]
    fn renamed_item_counts_as_changed() {
        let (b, a) = collections();
        let d = diff_collection(
            "weapons",
            &entries(&b),
            &entries(&a),
            &DiffOptions::default(),
        );
        assert_eq!(
            d.summary,
            Summary {
                added: 1,
                removed: 1,
                changed: 2,
                unchanged: 0
            }
        );
        assert_eq!(d.added, vec!["Delta (W.D)"]);
        assert_eq!(d.removed, vec!["Charlie (W.C)"]);
        assert_eq!(
            d.changed.iter().map(|c| c.key.as_str()).collect::<Vec<_>>(),
            vec!["W.B", "W.A"]
        );
        assert!(d.changed[0].changes.is_none());
    }

    #[test]
    fn field_detail_when_an_item_is_asked_for() {
        let (b, a) = collections();
        let options = DiffOptions {
            item: Some("bravo".into()),
            ..Default::default()
        };
        let d = diff_collection("weapons", &entries(&b), &entries(&a), &options);
        assert_eq!(d.summary.changed, 1);
        assert_eq!(
            d.changed[0].changes.as_deref(),
            Some(
                &[
                    change("ammo.damage.energy", Some(json!(20.0)), Some(json!(25.0))),
                    change("dps", Some(json!(200.0)), Some(json!(250.0)))
                ][..]
            )
        );
    }

    #[test]
    fn narrows_to_the_fields_that_matter() {
        let (b, a) = collections();
        let options = DiffOptions {
            fields: Some(vec!["dps".into()]),
            detail: true,
            ..Default::default()
        };
        let d = diff_collection("weapons", &entries(&b), &entries(&a), &options);
        assert_eq!((d.summary.changed, d.summary.unchanged), (1, 1));
        assert_eq!(
            d.changed[0].changes.as_deref(),
            Some(&[change("dps", Some(json!(200.0)), Some(json!(250.0)))][..])
        );
    }

    #[test]
    fn diffs_keyed_maps_like_arrays() {
        let (one, two, deux, three) = (json!("one"), json!("two"), json!("deux"), json!("three"));
        let before = vec![("@a".to_string(), &one), ("@b".to_string(), &two)];
        let after = vec![
            ("@a".to_string(), &one),
            ("@b".to_string(), &deux),
            ("@c".to_string(), &three),
        ];
        let d = diff_collection(
            "strings",
            &before,
            &after,
            &DiffOptions {
                detail: true,
                ..Default::default()
            },
        );
        assert_eq!(
            d.summary,
            Summary {
                added: 1,
                removed: 0,
                changed: 1,
                unchanged: 1
            }
        );
        assert_eq!(d.changed[0].key, "@b");
        assert_eq!(
            d.changed[0].changes.as_deref(),
            Some(&[change("", Some(json!("two")), Some(json!("deux")))][..])
        );
    }

    fn envelope(build: &str, body: Value) -> String {
        let mut v = json!({"_source": "test", "_extracted": "now", "_build": build});
        for (k, x) in body.as_object().unwrap() {
            v[k] = x.clone();
        }
        v["summary"] = json!({"n": 1});
        v.to_string()
    }

    struct Dirs {
        _tmp: tempfile::TempDir,
        older: BuildSource,
        newer: BuildSource,
        newer_path: PathBuf,
    }

    fn dirs() -> Dirs {
        let tmp = tempfile::tempdir().unwrap();
        let older = tmp.path().join("older");
        let newer = tmp.path().join("newer");
        std::fs::create_dir_all(&older).unwrap();
        std::fs::create_dir_all(&newer).unwrap();
        let w = |dps| json!({"shipWeapons": [weapon("W.A", "Alpha", dps)], "byName": {"Alpha": 0}});
        std::fs::write(older.join("game-weapons.json"), envelope("1.0.1", w(100.0))).unwrap();
        std::fs::write(newer.join("game-weapons.json"), envelope("1.0.2", w(110.0))).unwrap();
        std::fs::write(
            newer.join("game-new.json"),
            envelope("1.0.2", json!({"things": []})),
        )
        .unwrap();
        Dirs {
            older: BuildSource::dir(&older),
            newer: BuildSource::dir(&newer),
            newer_path: newer,
            _tmp: tmp,
        }
    }

    #[test]
    fn accepts_either_spelling_of_a_dataset_name() {
        assert_eq!(dataset_file("weapons"), "game-weapons.json");
        assert_eq!(dataset_file("game-weapons"), "game-weapons.json");
        assert_eq!(dataset_file("game-weapons.json"), "game-weapons.json");
        assert_eq!(dataset_file("strings"), "game-strings-english.json");
        assert_eq!(
            dataset_file("game-strings-german_(germany)"),
            "game-strings-german_(germany).json"
        );
    }

    #[test]
    fn reads_stamps_and_skips_meta_and_by_indexes() {
        let d = dirs();
        let snap = load_snapshot(&d.older, "game-weapons.json")
            .unwrap()
            .unwrap();
        assert_eq!(snap.build.as_deref(), Some("1.0.1"));
        assert!(
            load_snapshot(&d.older, "game-missing.json")
                .unwrap()
                .is_none()
        );
        assert_eq!(diffable_collections(&snap.body), vec!["shipWeapons"]);
    }

    #[test]
    fn diffs_every_collection_between_two_directories() {
        let d = dirs();
        let diff = diff_dataset(
            "game-weapons.json",
            &d.older,
            &d.newer,
            &DiffOptions {
                detail: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(diff.from.build.as_deref(), Some("1.0.1"));
        assert_eq!(diff.to.build.as_deref(), Some("1.0.2"));
        assert_eq!(diff.collections.len(), 1);
        assert!(
            diff.collections[0].changed[0]
                .changes
                .as_ref()
                .unwrap()
                .contains(&change("dps", Some(json!(100.0)), Some(json!(110.0))))
        );
        assert!(render_diff(&diff, None).contains("dps: 100 -> 110"));
    }

    #[test]
    fn summarises_a_dataset_only_one_side_has() {
        let d = dirs();
        let diff =
            diff_dataset("game-new.json", &d.older, &d.newer, &DiffOptions::default()).unwrap();
        assert_eq!(diff.missing, Some(Missing::From));
        assert!(render_diff(&diff, None).contains("not present in the older build"));

        let tags =
            json!({"items": [{"id": "a"}, {"id": "b"}], "tags": {"x": 1}, "byTag": {"x": [0]}});
        std::fs::write(d.newer_path.join("game-tags.json"), envelope("1.0.2", tags)).unwrap();
        let diff = diff_dataset(
            "game-tags.json",
            &d.older,
            &d.newer,
            &DiffOptions::default(),
        )
        .unwrap();
        assert_eq!(diff.missing, Some(Missing::From));
        assert_eq!(
            diff.counts.clone().map(Value::Object),
            Some(json!({"items": 2, "tags": 1}))
        );
        assert_eq!(diff.to.build.as_deref(), Some("1.0.2"));
        let text = render_diff(&diff, None);
        assert!(text.contains("new dataset (not present in the older build): 3 record(s)"));
        assert!(text.contains("  items: 2"));
        assert!(diff_json(&[diff]).contains("\"counts\": {\n    \"items\": 2,"));

        let dropped = diff_dataset(
            "game-tags.json",
            &d.newer,
            &d.older,
            &DiffOptions::default(),
        )
        .unwrap();
        assert_eq!(dropped.missing, Some(Missing::To));
        assert_eq!(
            dropped.counts.clone().map(Value::Object),
            Some(json!({"items": 2, "tags": 1}))
        );
        assert!(render_diff(&dropped, None).contains("dropped (not present in the newer build)"));

        let neither = diff_dataset(
            "game-nope.json",
            &d.older,
            &d.newer,
            &DiffOptions::default(),
        )
        .unwrap();
        assert_eq!(neither.missing, Some(Missing::Both));
        assert!(neither.counts.is_none());
        assert!(render_diff(&neither, None).contains("not present in either build"));
    }

    #[test]
    fn rejects_an_unknown_collection_with_the_available_ones() {
        let d = dirs();
        let options = DiffOptions {
            collection: Some("nope".into()),
            ..Default::default()
        };
        let err = diff_dataset("game-weapons.json", &d.older, &d.newer, &options).unwrap_err();
        assert!(err.to_string().contains("Available: shipWeapons"), "{err}");
    }

    #[test]
    fn resolves_local_and_directories_without_git() {
        let d = dirs();
        let BuildSource::Dir { path: older, .. } = &d.older else {
            unreachable!()
        };
        let repo = DumpsRepo {
            work_dir: older.join("no-such-checkout"),
            remote: "invalid".into(),
        };
        assert_eq!(
            resolve_source("local", older, &repo, None).unwrap(),
            d.older
        );
        let newer = d.newer_path.to_string_lossy().into_owned();
        assert_eq!(resolve_source(&newer, older, &repo, None).unwrap(), d.newer);
    }

    fn write_json(path: &Path, v: Value) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, v.to_string()).unwrap();
    }

    #[test]
    fn snapshots_save_list_resolve_and_read() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let data = root.join("data");
        let store = root.join("snapshots");
        write_json(
            &data.join("game-weapons.json"),
            json!({"_build": "4.10.196.1", "shipWeapons": [weapon("W.A", "Alpha", 100.0)]}),
        );
        // Stale dataset from another build: must not leak into this build's snapshot.
        write_json(
            &data.join("game-strings.json"),
            json!({"_build": "1.0.0", "strings": {}}),
        );

        let saved = save_snapshot(&data, &store, "4.10.196.1", None).unwrap();
        assert_eq!(saved.datasets, 1);
        assert!(store.join("4.10.196.1/game-weapons.json.gz").exists());
        assert!(!store.join("4.10.196.1/game-strings.json.gz").exists());

        write_json(
            &root.join("data9/game-weapons.json"),
            json!({"_build": "4.9.2.1", "shipWeapons": []}),
        );
        save_snapshot(&root.join("data9"), &store, "4.9.2.1", None).unwrap();
        let builds: Vec<String> = list_snapshots(&store)
            .unwrap()
            .into_iter()
            .map(|s| s.build)
            .collect();
        assert_eq!(builds, vec!["4.10.196.1", "4.9.2.1"]);
        let repo = DumpsRepo {
            work_dir: root.join("no-checkout"),
            remote: "invalid".into(),
        };
        assert_eq!(
            resolve_source("4.10.196.1", &data, &repo, Some(&store)).unwrap(),
            BuildSource::dir(store.join("4.10.196.1"))
        );
        assert_eq!(
            resolve_source("4.9", &data, &repo, Some(&store)).unwrap(),
            BuildSource::dir(store.join("4.9.2.1"))
        );

        // A gzipped snapshot reads like a plain directory.
        let source = BuildSource::dir(store.join("4.10.196.1"));
        assert_eq!(
            load_snapshot(&source, "game-weapons.json")
                .unwrap()
                .unwrap()
                .build
                .as_deref(),
            Some("4.10.196.1")
        );
        assert_eq!(
            list_dataset_files(&source, &source),
            vec!["game-weapons.json"]
        );

        // A dataset written after parse joins its build's snapshot.
        let file = root.join("later/game-strings-english.json");
        write_json(
            &file,
            json!({"_build": "4.10.196.1", "strings": {"@a": "one"}}),
        );
        let kept = snapshot_dataset(&file, &store).unwrap().unwrap();
        assert_eq!(kept.build, "4.10.196.1");
        let mut gz: Vec<String> = read_dir_names(&store.join("4.10.196.1"))
            .unwrap()
            .into_iter()
            .filter(|f| f.ends_with(".gz"))
            .collect();
        gz.sort();
        assert_eq!(
            gz,
            vec!["game-strings-english.json.gz", "game-weapons.json.gz"]
        );
        let strings = load_snapshot(&source, "game-strings-english.json")
            .unwrap()
            .unwrap();
        assert_eq!(strings.body.get("strings"), Some(&json!({"@a": "one"})));
        let unstamped = root.join("later/game-plain.json");
        write_json(&unstamped, json!({"strings": {}}));
        assert!(snapshot_dataset(&unstamped, &store).unwrap().is_none());
    }

    #[test]
    fn orders_versions_numerically() {
        assert_eq!(compare_versions("4.10.0", "4.9.9"), Ordering::Greater);
        assert_eq!(
            compare_versions("4.10.196.2", "4.10.196.10"),
            Ordering::Less
        );
    }

    fn git_in(dir: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .args([
                "-c",
                "user.name=test",
                "-c",
                "user.email=test@example.invalid",
            ])
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?}");
    }

    #[test]
    fn published_fallback_serves_what_a_snapshot_lacks() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let store = root.join("snapshots");
        let data = root.join("data");
        let dumps = root.join("dumps");
        write_json(
            &data.join("game-weapons.json"),
            json!({"_build": "4.10.197.1", "shipWeapons": []}),
        );
        save_snapshot(&data, &store, "4.10.197.1", None).unwrap();
        write_json(
            &dumps.join("data/game-strings-english.json"),
            json!({"_build": "4.10.197.1", "_redacted": "name keys only", "strings": {"vehicle_NameA": "one", "vehicle_NameB": "two"}}),
        );
        git_in(&dumps, &["init", "--quiet"]);
        git_in(&dumps, &["add", "."]);
        git_in(&dumps, &["commit", "--quiet", "-m", "dump"]);
        git_in(&dumps, &["tag", "4.10.197.1"]);
        let repo = DumpsRepo {
            work_dir: dumps.clone(),
            remote: "invalid".into(),
        };

        let from = resolve_source("4.10.197.1", &data, &repo, Some(&store)).unwrap();
        let tag = BuildSource::Tag {
            tag: "4.10.197.1".into(),
            repo_dir: dumps.clone(),
        };
        assert_eq!(
            from,
            BuildSource::Dir {
                path: store.join("4.10.197.1"),
                fallback: Some(Box::new(tag))
            }
        );

        let newer = root.join("newer");
        write_json(
            &newer.join("game-strings-english.json"),
            json!({"_build": "4.10.200.1", "strings": {"vehicle_NameA": "uno", "vehicle_NameB": "two", "vehicle_NameC": "three", "vehicle_DescA": "A long story.", "Dlg_Name_1": "Hi"}}),
        );
        let to = BuildSource::dir(&newer);
        assert_eq!(
            list_dataset_files(&from, &to),
            vec!["game-strings-english.json", "game-weapons.json"]
        );
        let diff = diff_dataset(
            "game-strings-english.json",
            &from,
            &to,
            &DiffOptions::default(),
        )
        .unwrap();
        assert!(diff.missing.is_none());
        assert_eq!(
            diff.from,
            DiffSide {
                source: "tag 4.10.197.1".into(),
                build: Some("4.10.197.1".into())
            }
        );
        assert_eq!(
            diff.collections[0].summary,
            Summary {
                added: 1,
                removed: 0,
                changed: 1,
                unchanged: 1
            }
        );
        assert_eq!(
            diff.notes.as_deref(),
            Some(&[
                "older side read from tag 4.10.197.1 (the snapshot lacks this dataset)".to_string(),
                "older string table is the redacted published one; the newer table is compared on published name keys only".to_string(),
            ][..])
        );
        assert!(render_diff(&diff, None).contains("note: older side read from tag 4.10.197.1"));
        // Snapshot copies still win over the tag.
        let w = load_snapshot(&from, "game-weapons.json").unwrap().unwrap();
        assert_eq!(w.source, store.join("4.10.197.1").to_string_lossy());

        // No fallback when the checkout has no matching tag.
        write_json(
            &root.join("data2/game-weapons.json"),
            json!({"_build": "4.10.198.1", "shipWeapons": []}),
        );
        save_snapshot(&root.join("data2"), &store, "4.10.198.1", None).unwrap();
        assert_eq!(
            resolve_source("4.10.198.1", &data, &repo, Some(&store)).unwrap(),
            BuildSource::dir(store.join("4.10.198.1"))
        );
    }

    #[test]
    fn leaves_the_search_index_out_unless_named() {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path().join("data");
        write_json(
            &data.join("game-weapons.json"),
            json!({"_build": "4.10.200.1", "shipWeapons": []}),
        );
        write_json(
            &data.join(crate::search_index::FILE),
            json!({"_build": "4.10.200.1", "format": 1, "items": [], "ships": []}),
        );
        assert!(DERIVED_FILES.contains(&crate::search_index::FILE));
        let source = BuildSource::dir(&data);
        assert_eq!(
            list_dataset_files(&source, &source),
            vec!["game-weapons.json"]
        );
        let saved =
            save_snapshot(&data, &tmp.path().join("snapshots"), "4.10.200.1", None).unwrap();
        assert_eq!(saved.datasets, 1);
        assert_eq!(
            read_dir_names(&saved.path).unwrap(),
            vec!["_snapshot.json", "game-weapons.json.gz"]
        );
        let d = diff_dataset(
            crate::search_index::FILE,
            &source,
            &source,
            &DiffOptions::default(),
        )
        .unwrap();
        assert!(d.missing.is_none());
    }

    #[test]
    fn cuts_a_full_table_to_published_keys() {
        let redacted = obj(json!({"_redacted": "x", "strings": {"ship_Name": "Gladius"}}));
        let full = obj(
            json!({"strings": {"ship_Name": "Gladius", "ship_Desc": "Prose.", "ship_NameLong": "x".repeat(121), "PU_Title": "Dialogue"}}),
        );
        let aligned = align_redaction(&redacted, &full);
        assert_eq!(
            aligned.after.get("strings"),
            Some(&json!({"ship_Name": "Gladius"}))
        );
        assert!(matches!(aligned.before, Cow::Borrowed(_)));
        assert!(
            aligned
                .note
                .unwrap()
                .starts_with("older string table is the redacted")
        );
        let same = align_redaction(&full, &full);
        assert!(same.note.is_none() && *same.before == full && *same.after == full);
        assert!(align_redaction(&redacted, &redacted).note.is_none());
    }

    /// The `diff.test.ts` case that compared this rule with `scripts/redact.mjs` by source
    /// text: here, the published-key rule must agree with the Rust redaction constants.
    #[test]
    fn published_key_rule_matches_the_redaction_constants() {
        assert_eq!(MAX_PUBLISHED_VALUE, redaction::MAX_VALUE_LENGTH);
        let s = |v: &str| Value::String(v.into());
        for (key, value, kept) in [
            ("vehicle_Name", s("Gladius"), true),
            ("vehicle_Desc", s("Prose"), false),
            ("PU_Mission_Title", s("Hello"), false),
            ("item_name_x", s("lower-case name"), false),
            ("item_NameLong", s(&"x".repeat(120)), true),
            ("item_NameLong", s(&"x".repeat(121)), false),
            ("item_Name", s(""), false),
            ("item_Name", json!(5), false),
            ("Title", s("t"), true),
            ("x_Titles_2", s("t"), true),
            ("x_Namesake", s("t"), false),
            ("dxs_Name", s("t"), false),
            ("x_ShortName", s("t"), true),
            ("x_Name_tooltip", s("t"), false),
        ] {
            assert_eq!(is_published_string(key, Some(&value)), kept, "{key}");
            if let Value::String(v) = &value {
                assert_eq!(redaction::keeps(key, v), kept, "{key} (redaction::keeps)");
            }
        }
        // The prose fields diff strips are the ones redaction lists.
        assert_eq!(
            redaction::prose_fields("game-missions.json"),
            Some(&["description"][..])
        );
    }

    #[test]
    fn prose_alignment_cuts_the_full_side_either_way_round() {
        let long = "x".repeat(130);
        let full = obj(json!({"_build": "b", "missions": [
            {"id": "1", "name": "Short", "recordName": "MissionBrokerEntry.short", "title": "Short", "description": "prose"},
            {"id": "2", "name": long, "recordName": "MissionBrokerEntry.long_one", "title": long},
        ]}));
        let published = obj(
            json!({"_build": "b", "_redacted": ["description", "title over 120 characters"], "missions": [
                {"id": "1", "name": "Short", "recordName": "MissionBrokerEntry.short", "title": "Short"},
                {"id": "2", "name": "long_one", "recordName": "MissionBrokerEntry.long_one"},
            ]}),
        );
        let newer = align_redaction(&full, &published);
        assert_eq!(newer.before.get("missions"), published.get("missions"));
        assert!(
            newer
                .note
                .unwrap()
                .starts_with("newer side is the published dataset without its prose")
        );
        let older = align_redaction(&published, &full);
        assert_eq!(older.after.get("missions"), published.get("missions"));
        assert!(align_redaction(&published, &published).note.is_none());
        assert!(align_redaction(&full, &full).note.is_none());
    }

    #[test]
    fn js_helpers() {
        assert_eq!(js_to_number(""), 0.0);
        assert_eq!(js_to_number(" 12 "), 12.0);
        assert!(js_to_number("1a").is_nan());
        assert!(js_to_number("inf").is_nan());
        assert_eq!(js_to_number("0x10"), 16.0);
        assert_eq!(collapse_ws(" a \t b\u{a0}"), " a b ");
        assert_eq!(js_string(Some(&json!([1, null, "x", [2, 3]]))), "1,,x,2,3");
        assert_eq!(stamp_of("{\"_build\":  \"4.10\"}").as_deref(), Some("4.10"));
        assert_eq!(cmp_utf16("\u{ffff}", "\u{10000}"), Ordering::Greater);
        assert_eq!(
            resolve_path(Path::new("/a/./b/../c/")),
            PathBuf::from("/a/c")
        );
        let mut big = json!(12345678901234567890u64);
        normalize_numbers(&mut big);
        assert_eq!(crate::output::to_js_json(&big), "12345678901234567000");
    }
}
