//! `game-strings-<language>.json`: the string table as a dataset (port of `writeStrings`
//! in `sc-gamedata/src/pipeline.ts`). Not a record domain; the harness runs it by name.

use crate::localization::{Localization, redaction};
use crate::output;
use serde_json::{Map, Value, json};

/// Output name for English, the only language this port extracts.
pub const NAME: &str = "game-strings-english";

/// The dataset. With `redact`, cut to what `publish-dump.mjs` publishes (adds `_redacted`
/// and `summary.published`); golden `data/` dumps are unredacted.
pub fn dataset(table: &Localization, extracted: &str, build: Option<&str>, redact: bool) -> Value {
    let language = "english";
    let mut out = Map::new();
    out.insert(
        "_source".into(),
        json!(format!(
            "{} (Data/Localization/{language}/global.ini)",
            output::SOURCE
        )),
    );
    out.insert("_extracted".into(), json!(extracted));
    if let Some(b) = build {
        out.insert("_build".into(), json!(b));
    }
    out.insert("language".into(), json!(language));
    let kept = if redact {
        redaction::redact(table)
    } else {
        table.clone()
    };
    let strings: Map<String, Value> = kept
        .iter()
        .map(|(k, v)| (k.to_string(), json!(v)))
        .collect();
    out.insert("strings".into(), Value::Object(strings));
    let mut summary = Map::new();
    summary.insert("strings".into(), json!(table.len()));
    if redact {
        summary.insert("published".into(), json!(kept.len()));
    }
    out.insert("summary".into(), Value::Object(summary));
    if redact {
        // `{...dump, _redacted, strings, summary}`: `strings` and `summary` keep their
        // places, `_redacted` is new and goes last.
        out.insert("_redacted".into(), json!(redaction::NOTE));
    }
    Value::Object(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::localization::parse_global_ini;

    #[test]
    fn shape() {
        let t = parse_global_ini("vehicle_NameX=X\nvehicle_DescX=long prose");
        let v = dataset(&t, "t", Some("4.10"), false);
        let keys: Vec<_> = v.as_object().unwrap().keys().cloned().collect();
        assert_eq!(
            keys,
            vec![
                "_source",
                "_extracted",
                "_build",
                "language",
                "strings",
                "summary"
            ]
        );
        assert_eq!(v["summary"]["strings"], 2);
        let r = dataset(&t, "t", None, true);
        assert_eq!(r["strings"], json!({"vehicle_NameX": "X"}));
        assert_eq!(r["summary"], json!({"strings": 2, "published": 1}));
        assert_eq!(
            r.as_object().unwrap().keys().next_back().unwrap(),
            "_redacted"
        );
    }
}
