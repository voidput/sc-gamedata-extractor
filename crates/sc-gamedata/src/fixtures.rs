//! In-memory sources for tests: records as StarBreaker export envelopes, files as bytes.

use crate::domain::FileSource;
use crate::records::{RecordMeta, RecordSource, record_path_of_file_name};
use serde_json::Value;
use std::collections::BTreeMap;

/// Records given as export envelopes plus the DataCore file name:
/// `{"_RecordName_", "_RecordId_", "_RecordPath_": "libs/foundry/records/....xml",
///   "_RecordValue_": {"_Type_", ...}}`. `_RecordPath_` is removed from the envelope.
#[derive(Debug, Default)]
pub struct FixtureSource {
    metas: Vec<RecordMeta>,
    envelopes: Vec<Value>,
}

impl FixtureSource {
    pub fn from_envelopes(envelopes: Vec<Value>) -> Result<Self, String> {
        let mut s = Self::default();
        for mut env in envelopes {
            let obj = env.as_object_mut().ok_or("fixture is not an object")?;
            let name = obj
                .get("_RecordName_")
                .and_then(Value::as_str)
                .ok_or("fixture without _RecordName_")?
                .to_string();
            let file_name = obj
                .remove("_RecordPath_")
                .and_then(|v| v.as_str().map(|p| p.replace('\\', "/")))
                .ok_or_else(|| format!("{name}: no _RecordPath_"))?;
            let struct_type = obj
                .get("_RecordValue_")
                .and_then(|v| v.get("_Type_"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let id = obj
                .get("_RecordId_")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            s.metas.push(RecordMeta {
                name,
                record_path: record_path_of_file_name(&file_name),
                file_name,
                struct_type,
                id,
            });
            s.envelopes.push(env);
        }
        Ok(s)
    }
}

impl RecordSource for FixtureSource {
    fn metas(&self) -> &[RecordMeta] {
        &self.metas
    }

    fn envelope(&self, index: usize) -> Result<Value, String> {
        self.envelopes
            .get(index)
            .cloned()
            .ok_or_else(|| format!("fixture {index}: out of range"))
    }
}

/// Archive files held in memory, by entry name.
#[derive(Debug, Default)]
pub struct MemFiles {
    names: Vec<String>,
    data: BTreeMap<String, Vec<u8>>,
}

impl MemFiles {
    pub fn insert(&mut self, name: &str, bytes: Vec<u8>) {
        if self.data.insert(name.to_string(), bytes).is_none() {
            self.names.push(name.to_string());
        }
    }
}

impl FileSource for MemFiles {
    fn names(&self) -> &[String] {
        &self.names
    }

    fn read(&self, name: &str) -> Result<Vec<u8>, String> {
        self.data
            .get(name)
            .or_else(|| {
                self.data
                    .iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case(name))
                    .map(|(_, v)| v)
            })
            .cloned()
            .ok_or_else(|| format!("{name}: not in archive"))
    }
}

/// The domain's output on `records`, checked to be the same with every record passed
/// through [`crate::domain::Domain::prune`] first (a prune must not change the output).
pub fn parse_checked(
    domain: &dyn crate::domain::Domain,
    records: &crate::records::RecordSet,
    localization: &crate::localization::Localization,
) -> serde_json::Map<String, Value> {
    use crate::domain::{NoFiles, ParseContext};
    let whole = domain
        .parse(&ParseContext::new(records, localization, &NoFiles))
        .expect("parse");
    let pruned = crate::records::RecordSet::new(
        records
            .iter()
            .map(|r| {
                let mut r = r.clone();
                domain.prune(&mut r);
                r
            })
            .collect(),
    );
    let after = domain
        .parse(&ParseContext::new(&pruned, localization, &NoFiles))
        .expect("parse pruned");
    assert_eq!(whole, after, "{}: prune changed the output", domain.name());
    whole
}
