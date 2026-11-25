//! Small helpers the gameplay ports (missions, wikelo, blueprints) share.
//!
//! - [`file_ref_stem`]: the identical `referencedName` (missions.ts), `entityName`
//!   (wikelo.ts) and `entityClassName` (blueprints.ts) functions;
//! - [`add_count`] / [`by_count_desc`]: the `Record<string, number>` tallies those domains
//!   publish as `Object.fromEntries(Object.entries(t).sort((a, b) => b[1] - a[1]))`;
//! - [`ref_short`]: `refShortName` exactly (an empty `_RecordName_` gives `None`);
//! - [`item_display_name`]: the part of `itemIdentity` (domains/item-common.ts) that
//!   blueprints uses. It lives here rather than in the `item_common` slot so this batch
//!   does not race the items batch for that file; once `item_common` lands, blueprints can
//!   call its `item_identity` instead and this goes.

use crate::entity::component;
use crate::localization::{Localization, is_placeholder_name, localize};
use crate::output::js_key_order;
use crate::records::{GameRecord, at, ref_name, str_};
use serde_json::{Map, Number, Value};

/// The last path segment of a `file://...json` reference with `.json` (any case) removed:
/// `file://./../missiontype/pu/race.json` -> `race`. `None` for a non-string, an empty
/// string or a path ending in `/`. As in the TypeScript, a bare `.json` file gives `""`.
pub fn file_ref_stem(value: Option<&Value>) -> Option<&str> {
    let path = str_(value)?;
    let file = path.rsplit('/').next().filter(|f| !f.is_empty())?;
    // `.replace(/\.json$/i, "")`
    let cut = file.len().saturating_sub(5);
    Some(match file.get(cut..) {
        Some(ext) if file.len() >= 5 && ext.eq_ignore_ascii_case(".json") => &file[..cut],
        _ => file,
    })
}

/// `refShortName`: the referenced record's name without its type prefix. Unlike
/// [`crate::records::ref_short_name`], an empty `_RecordName_` gives `None` (the TS `!name`
/// check); `"Tag."` still gives `Some("")`.
pub fn ref_short(value: Option<&Value>) -> Option<&str> {
    let name = ref_name(value).filter(|n| !n.is_empty())?;
    Some(name.split_once('.').map_or(name, |(_, rest)| rest))
}

/// `tally[key] = (tally[key] ?? 0) + by`.
pub fn add_count(tally: &mut Map<String, Value>, key: &str, by: f64) {
    let before = tally.get(key).and_then(Value::as_f64).unwrap_or(0.0);
    let sum = before + by;
    let value = Number::from_f64(sum).map_or(Value::Null, Value::Number);
    tally.insert(key.to_string(), value);
}

/// `Object.fromEntries(Object.entries(tally).sort((a, b) => b[1] - a[1]))`: entries in JS
/// property order, stably sorted by count, largest first.
pub fn by_count_desc(tally: &Map<String, Value>) -> Map<String, Value> {
    let mut entries: Vec<(&String, f64)> = js_key_order(tally)
        .into_iter()
        .map(|k| (k, tally[k].as_f64().unwrap_or(0.0)))
        .collect();
    entries.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    entries
        .into_iter()
        .map(|(k, _)| (k.clone(), tally[k].clone()))
        .collect()
}

/// `itemIdentity(record).name` when the item has a real display name (`isTemplate` unset),
/// else `None`: `SAttachableComponentParams.AttachDef.Localization.Name`, localized.
pub fn item_display_name(record: &GameRecord, localization: &Localization) -> Option<String> {
    let def = component(record, "SAttachableComponentParams")
        .and_then(|c| c.get("AttachDef"))
        .filter(|d| d.is_object());
    let localized = localize(localization, def.and_then(|d| at(d, "Localization.Name")));
    if is_placeholder_name(localized) {
        None
    } else {
        localized.map(str::to_string)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::localization::parse_global_ini;
    use crate::records::normalize_record;
    use serde_json::json;

    #[test]
    fn file_ref_stems() {
        // missions.test.ts: "takes the record name off a file reference".
        let r = json!("file://./../missiontype/pu/race.json");
        assert_eq!(file_ref_stem(Some(&r)), Some("race"));
        assert_eq!(file_ref_stem(Some(&json!("a/B.JSON"))), Some("B"));
        assert_eq!(file_ref_stem(Some(&json!("a/x.y.json"))), Some("x.y"));
        assert_eq!(file_ref_stem(Some(&json!("a/.json"))), Some(""));
        assert_eq!(file_ref_stem(Some(&json!("a/"))), None);
        assert_eq!(file_ref_stem(Some(&json!(""))), None);
        assert_eq!(file_ref_stem(Some(&json!(3))), None);
    }

    #[test]
    fn ref_shorts() {
        assert_eq!(
            ref_short(Some(&json!({"_RecordName_": "ResourceType.Raw_Gold"}))),
            Some("Raw_Gold")
        );
        assert_eq!(ref_short(Some(&json!({"_RecordName_": "Tag."}))), Some(""));
        assert_eq!(ref_short(Some(&json!({"_RecordName_": ""}))), None);
        assert_eq!(ref_short(Some(&json!("x"))), None);
    }

    #[test]
    fn tallies_sort_by_count_keeping_ties_in_js_order() {
        let mut t = Map::new();
        for (k, n) in [("b", 1.0), ("a", 2.0), ("10", 1.0), ("2", 1.0), ("b", 1.0)] {
            add_count(&mut t, k, n);
        }
        let sorted = by_count_desc(&t);
        let keys: Vec<&str> = sorted.keys().map(String::as_str).collect();
        // Integer-like keys enumerate first, ascending; ties keep that order.
        assert_eq!(keys, vec!["b", "a", "2", "10"]);
        assert_eq!(sorted["b"], json!(2.0));
    }

    #[test]
    fn item_display_names() {
        let loc = parse_global_ini("item_NameX=Widget");
        let item = |name: &str| {
            normalize_record(
                json!({"_RecordName_": "EntityClassDefinition.x", "_RecordValue_": {"_Type_": "EntityClassDefinition",
                    "Components": [{"_Type_": "SAttachableComponentParams", "AttachDef": {"Localization": {"Name": name}}}]}}),
                "p",
            )
            .unwrap()
        };
        assert_eq!(
            item_display_name(&item("@item_NameX"), &loc).as_deref(),
            Some("Widget")
        );
        assert_eq!(item_display_name(&item("@item_Missing"), &loc), None);
        assert_eq!(item_display_name(&item("<= PLACEHOLDER =>"), &loc), None);
    }
}
