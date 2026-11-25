//! DataCore records and how to navigate them (port of `sc-gamedata/src/records.ts`).
//!
//! `sc-gamedata` runs `starbreaker dcb extract --format json` and reads the files back.
//! Every file is an envelope:
//!
//! ```json
//! { "_RecordName_": "MineableElement.Quantainium_Raw", "_RecordId_": "fc62043c-...",
//!   "_RecordValue_": { "_Type_": "MineableElement", ...fields } }
//! ```
//!
//! Here the same envelopes come straight from StarBreaker's exporter (see [`RecordSource`]),
//! so the values a domain sees are byte-for-byte what the TypeScript parsed. References to
//! other records are `{"_RecordName_", "_RecordId_", ...}` objects or `file://` path strings
//! and are resolved through a [`RecordSet`]'s indices, never by reading outside the set.

use serde_json::Value;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::OnceLock;

/// One record without its value: what a [`RecordSource`] lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordMeta {
    /// `_RecordName_`.
    pub name: String,
    /// Where `sc-gamedata` found the file, relative to its extraction root, without `.json`:
    /// the DataCore file name with its extension removed and `\` turned into `/`, e.g.
    /// `libs/foundry/records/scitemmanufacturer/aegs`. Case is kept.
    pub record_path: String,
    /// The DataCore file name with `\\` turned into `/`, extension kept
    /// (`libs/foundry/records/scitemmanufacturer/aegs.xml`): what `dcb extract --filter`
    /// matches.
    pub file_name: String,
    /// DataCore struct type.
    pub struct_type: String,
    pub id: String,
}

/// Where records come from: the DataCore, or fixtures in tests.
pub trait RecordSource: Sync {
    /// Every main record.
    fn metas(&self) -> &[RecordMeta];
    /// The export envelope of `metas()[index]`. `Err` is a readable message; the caller
    /// skips the record, as `loadRecords` skips a file it cannot parse.
    fn envelope(&self, index: usize) -> Result<Value, String>;

    /// Load and normalise one record.
    fn load(&self, index: usize) -> Result<Option<GameRecord>, String> {
        let meta = &self.metas()[index];
        Ok(normalize_record(self.envelope(index)?, &meta.record_path))
    }
}

/// `sc-gamedata`'s record path for a DataCore file name: what `dcb extract` writes the
/// record as (`Path.ChangeExtension` to `.json`), seen from the extraction root.
pub fn record_path_of_file_name(file_name: &str) -> String {
    let stem = match file_name.rfind('.') {
        Some(dot) => &file_name[..dot],
        None => file_name,
    };
    stem.replace('\\', "/")
}

/// A record as a domain sees it (`GameRecord` in the TypeScript).
#[derive(Debug, Clone, PartialEq)]
pub struct GameRecord {
    /// Full record name, `MineableElement.Quantainium_Raw`.
    pub name: String,
    /// The name without its type prefix, `Quantainium_Raw`.
    pub short_name: String,
    /// `_RecordId_`, or empty.
    pub id: String,
    /// `_RecordValue_._Type_`, else the name's prefix.
    pub ty: String,
    /// See [`RecordMeta::record_path`].
    pub record_path: String,
    /// `_RecordValue_`.
    pub value: Value,
}

/// `normalizeRecord`: `None` unless the envelope has a string name and an object value.
pub fn normalize_record(mut raw: Value, record_path: &str) -> Option<GameRecord> {
    let name = raw.get("_RecordName_")?.as_str()?.to_string();
    let value = raw.get_mut("_RecordValue_")?.take();
    if !value.is_object() {
        return None;
    }
    let ty = match value.get("_Type_").and_then(Value::as_str) {
        Some(t) => t.to_string(),
        None => name.split('.').next().unwrap_or_default().to_string(),
    };
    let prefix = format!("{ty}.");
    let short_name = name.strip_prefix(&prefix).unwrap_or(&name).to_string();
    let id = raw
        .get("_RecordId_")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    Some(GameRecord {
        name,
        short_name,
        id,
        ty,
        record_path: record_path.to_string(),
        value,
    })
}

/// The order `loadRecords` yields records in: `findJsonFiles` sorts the full file paths,
/// which share the extraction-root prefix, by UTF-16 code unit.
pub fn file_order(a: &str, b: &str) -> Ordering {
    let fa = a.encode_utf16().chain(".json".encode_utf16());
    let fb = b.encode_utf16().chain(".json".encode_utf16());
    fa.cmp(fb)
}

/// The records one domain is handed, in `loadRecords` order, with lookup indices built on
/// first use. Lookups only ever see records in the set: a reference to a record outside a
/// domain's filters does not resolve, exactly as in `sc-gamedata`.
#[derive(Debug, Default)]
pub struct RecordSet {
    records: Vec<GameRecord>,
    by_name: OnceLock<HashMap<String, usize>>,
    by_path: OnceLock<HashMap<String, usize>>,
    by_id: OnceLock<HashMap<String, usize>>,
}

impl RecordSet {
    /// Records in the given order (callers pass [`file_order`] order).
    pub fn new(records: Vec<GameRecord>) -> Self {
        Self {
            records,
            ..Self::default()
        }
    }

    pub fn all(&self) -> &[GameRecord] {
        &self.records
    }

    pub fn iter(&self) -> std::slice::Iter<'_, GameRecord> {
        self.records.iter()
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Records whose `ty` is `ty`.
    pub fn of_type<'a>(&'a self, ty: &'a str) -> impl Iterator<Item = &'a GameRecord> + 'a {
        self.records.iter().filter(move |r| r.ty == ty)
    }

    /// `indexByName`: by full name; a later duplicate wins.
    pub fn by_name(&self, name: &str) -> Option<&GameRecord> {
        let index = self.by_name.get_or_init(|| {
            self.records
                .iter()
                .enumerate()
                .map(|(i, r)| (r.name.clone(), i))
                .collect()
        });
        index.get(name).map(|&i| &self.records[i])
    }

    /// `indexByRecordPath`: by lower-case path below `libs/foundry/records/` (see
    /// [`ref_path`]), which is how `file://` references name records.
    pub fn by_record_path(&self, path: &str) -> Option<&GameRecord> {
        let index = self.by_path.get_or_init(|| {
            let mut m = HashMap::new();
            for (i, r) in self.records.iter().enumerate() {
                if let Some(p) = ref_path_str(&r.record_path) {
                    m.insert(p, i);
                }
            }
            m
        });
        index.get(path).map(|&i| &self.records[i])
    }

    /// By `_RecordId_` (GUID, as exported: lower-case, hyphenated).
    pub fn by_id(&self, id: &str) -> Option<&GameRecord> {
        let index = self.by_id.get_or_init(|| {
            self.records
                .iter()
                .enumerate()
                .filter(|(_, r)| !r.id.is_empty())
                .map(|(i, r)| (r.id.clone(), i))
                .collect()
        });
        index.get(id).map(|&i| &self.records[i])
    }

    /// Follow a reference of either shape: a `{_RecordName_}` object (by name) or a
    /// `file://...json` string (by path).
    pub fn follow(&self, reference: Option<&Value>) -> Option<&GameRecord> {
        let reference = reference?;
        if let Some(name) = ref_name(Some(reference)) {
            return self.by_name(name);
        }
        self.by_record_path(&ref_path(Some(reference))?)
    }
}

/// The `_RecordName_` a reference object points at.
pub fn ref_name(value: Option<&Value>) -> Option<&str> {
    value?.as_object()?.get("_RecordName_")?.as_str()
}

/// Reference name without its type prefix: `ResourceType.Raw_Gold` -> `Raw_Gold`.
pub fn ref_short_name(value: Option<&Value>) -> Option<&str> {
    let name = ref_name(value)?;
    Some(name.split_once('.').map_or(name, |(_, rest)| rest))
}

/// Read a dotted path (`"a.b.c"`). Array elements are reachable by index segment
/// (`"Components.0"`), as property access on a JavaScript array is.
pub fn at<'a>(value: &'a Value, dotted: &str) -> Option<&'a Value> {
    let mut cur = value;
    for seg in dotted.split('.') {
        cur = match cur {
            Value::Object(m) => m.get(seg)?,
            Value::Array(a) => a.get(seg.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(cur)
}

/// `num`: a finite number.
pub fn num(value: Option<&Value>) -> Option<f64> {
    value?.as_f64().filter(|x| x.is_finite())
}

/// `str`: a non-empty string.
pub fn str_(value: Option<&Value>) -> Option<&str> {
    value?.as_str().filter(|s| !s.is_empty())
}

/// An array's elements, or nothing.
pub fn array(value: Option<&Value>) -> &[Value] {
    value.and_then(Value::as_array).map_or(&[], Vec::as_slice)
}

const RECORDS_ROOT: &str = "libs/foundry/records/";

/// `refPath`: `file://./../../libs/foundry/records/damage/lightarmor.json` ->
/// `damage/lightarmor` (lower case, `.json` removed).
pub fn ref_path(reference: Option<&Value>) -> Option<String> {
    ref_path_str(reference?.as_str()?)
}

/// [`ref_path`] for a string.
pub fn ref_path_str(reference: &str) -> Option<String> {
    let lower = reference.to_lowercase();
    let start = lower.find(RECORDS_ROOT)? + RECORDS_ROOT.len();
    let rest = &lower[start..];
    Some(rest.strip_suffix(".json").unwrap_or(rest).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn env(name: &str, ty: &str) -> Value {
        json!({"_RecordName_": name, "_RecordId_": "id-1", "_RecordValue_": {"_Type_": ty, "x": 1}})
    }

    // Cases from sc-gamedata src/records.test.ts.
    #[test]
    fn normalize_strips_the_type_prefix() {
        let r = normalize_record(env("MineableElement.Gold", "MineableElement"), "a/b").unwrap();
        assert_eq!(r.short_name, "Gold");
        assert_eq!(r.ty, "MineableElement");
        assert_eq!(r.id, "id-1");
        assert_eq!(r.record_path, "a/b");
    }

    #[test]
    fn normalize_falls_back_to_the_name_prefix() {
        let raw = json!({"_RecordName_": "Thing.X", "_RecordValue_": {"y": 1}});
        let r = normalize_record(raw, "p").unwrap();
        assert_eq!(
            (r.ty.as_str(), r.short_name.as_str(), r.id.as_str()),
            ("Thing", "X", "")
        );
        let other = normalize_record(env("Other.Name", "Thing"), "p").unwrap();
        assert_eq!(other.short_name, "Other.Name", "prefix differs from type");
    }

    #[test]
    fn normalize_rejects_non_envelopes() {
        assert!(normalize_record(json!({"build": "4.10"}), "_build").is_none());
        assert!(
            normalize_record(json!({"_RecordName_": "A.B", "_RecordValue_": 3}), "p").is_none()
        );
    }

    #[test]
    fn reference_helpers() {
        let r = json!({"_RecordName_": "ResourceType.Raw_Gold"});
        assert_eq!(ref_name(Some(&r)), Some("ResourceType.Raw_Gold"));
        assert_eq!(ref_short_name(Some(&r)), Some("Raw_Gold"));
        assert_eq!(ref_name(Some(&json!(["x"]))), None);
        assert_eq!(
            ref_path(Some(&json!(
                "file://./../../libs/foundry/records/Damage/LightArmor.json"
            ))),
            Some("damage/lightarmor".to_string())
        );
        assert_eq!(ref_path(Some(&json!("elsewhere.json"))), None);
        assert_eq!(ref_path(Some(&json!(3))), None);
    }

    #[test]
    fn at_num_str() {
        let v = json!({"a": {"b": [10, {"c": "x"}]}, "n": f64::NAN.to_string(), "e": ""});
        assert_eq!(num(at(&v, "a.b.0")), Some(10.0));
        assert_eq!(str_(at(&v, "a.b.1.c")), Some("x"));
        assert_eq!(at(&v, "a.z.q"), None);
        assert_eq!(str_(at(&v, "e")), None);
        assert_eq!(num(at(&v, "n")), None);
    }

    #[test]
    fn record_paths_and_order() {
        assert_eq!(
            record_path_of_file_name("libs/foundry/records/scitemmanufacturer/aegs.xml"),
            "libs/foundry/records/scitemmanufacturer/aegs"
        );
        assert_eq!(record_path_of_file_name(r"a\b.c.xml"), "a/b.c");
        // ".json" is part of the sorted name: "a.json" < "a/b.json" ('.' < '/').
        let mut v = vec!["a/b", "a", "a_b", "B"];
        v.sort_by(|x, y| file_order(x, y));
        assert_eq!(v, vec!["B", "a", "a/b", "a_b"]);
    }

    #[test]
    fn record_set_lookups() {
        let mk = |name: &str, path: &str, id: &str| {
            let mut e = env(name, "T");
            e["_RecordId_"] = json!(id);
            normalize_record(e, path).unwrap()
        };
        let set = RecordSet::new(vec![
            mk("T.A", "libs/foundry/records/x/a", "g-a"),
            mk("T.B", "libs/foundry/records/X/B", "g-b"),
        ]);
        assert_eq!(set.by_name("T.B").unwrap().id, "g-b");
        assert_eq!(set.by_record_path("x/b").unwrap().name, "T.B");
        assert_eq!(set.by_id("g-a").unwrap().name, "T.A");
        assert_eq!(
            set.follow(Some(&json!({"_RecordName_": "T.A"})))
                .unwrap()
                .id,
            "g-a"
        );
        assert_eq!(
            set.follow(Some(&json!("file://./../libs/foundry/records/x/b.json")))
                .unwrap()
                .name,
            "T.B"
        );
        assert!(
            set.follow(Some(&json!({"_RecordName_": "T.Missing"})))
                .is_none()
        );
        assert_eq!(set.of_type("T").count(), 2);
    }
}
