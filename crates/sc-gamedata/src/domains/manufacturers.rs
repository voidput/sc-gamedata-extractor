//! Manufacturers: the in-fiction companies behind ships, components and gear (port of
//! `sc-gamedata/src/domains/manufacturers.ts`, the template every port follows).
//!
//! The records are `SCItemManufacturer` under `libs/foundry/records/scitemmanufacturer`.
//! Names are localization keys (`@manufacturer_NameSASU`), so the domain needs the string
//! table. Not every record there is a manufacturer: real ones sit at the top level or under
//! `clothes`/`armor`; `paintcolorlogos` (900+ paint records carrying their maker's name
//! key), `shop` and `uistyleonly` only reference one.

use crate::collate::locale_compare;
use crate::domain::{Domain, DomainError, ParseContext, field};
use crate::localization::{LocRefs, Localization, loc_refs, localize};
use crate::output::js_key_order;
use crate::records::{GameRecord, RecordSet};
use std::collections::HashMap;
use serde::Serialize;
use serde_json::{Map, Value};

pub static DOMAIN: Manufacturers = Manufacturers;

pub struct Manufacturers;

/// One row of `manufacturers`. Field order is output order.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Manufacturer {
    /// Record short name, which doubles as the manufacturer code: `AEGS`.
    pub code: String,
    pub record_name: String,
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// `nameKey`, `descriptionKey`, `nameStatus`.
    #[serde(flatten)]
    pub loc: LocRefs,
    /// The record's own `Code` field (`AEG` where `code` is `AEGS`), `null` when empty or
    /// absent. `code` stays the key every dataset uses.
    pub record_code: Option<String>,
}

#[derive(Debug, Serialize)]
struct SharedName {
    name: String,
    codes: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Summary {
    manufacturers: usize,
    unnamed_or_placeholder: usize,
    names_used_by_multiple_codes: usize,
}

pub fn parse_manufacturer(record: &GameRecord, localization: &Localization) -> Manufacturer {
    let loc = record.value.get("Localization");
    let text = |key: &str| localize(localization, loc.and_then(|l| l.get(key))).map(str::to_string);
    Manufacturer {
        code: record.short_name.clone(),
        record_name: record.name.clone(),
        id: record.id.clone(),
        name: text("Name"),
        description: text("Description"),
        loc: loc_refs(
            localization,
            loc.and_then(|l| l.get("Name")),
            loc.and_then(|l| l.get("Description")),
        ),
        record_code: record
            .value
            .get("Code")
            .and_then(Value::as_str)
            .filter(|c| !c.is_empty())
            .map(str::to_string),
    }
}

/// Glob that hands a domain the manufacturer records, for [`names_by_code`].
pub const MANUFACTURER_FILTER: &str = "**/scitemmanufacturer/**";

/// `(code, display name)` per manufacturer code, keyed by the upper-cased code, from the same
/// records and in the same way as `byCode`: the join `manufacturerCode` / `manufacturerName`
/// on ships and item tags is read from. `code` is the `byCode` key as published (`AEGS`,
/// `GRIN_Weapons`).
pub fn names_by_code(
    records: &RecordSet,
    localization: &Localization,
) -> HashMap<String, (String, String)> {
    let mut named: Vec<Manufacturer> = records
        .of_type("SCItemManufacturer")
        .filter(|r| is_manufacturer_record(r))
        .map(|r| parse_manufacturer(r, localization))
        .filter(looks_real)
        .collect();
    named.sort_by(|a, b| {
        locale_compare(
            a.name.as_deref().unwrap_or(""),
            b.name.as_deref().unwrap_or(""),
        )
    });
    let mut out = HashMap::new();
    for m in named {
        if let Some(name) = m.name {
            out.insert(m.code.to_uppercase(), (m.code, name));
        }
    }
    out
}

/// Adds `manufacturerCode` (the `byCode` key) and `manufacturerName` right after
/// `manufacturer` on each row of `rows` (a serialized array of objects) whose code resolves
/// through `names` (case-insensitively). Rows with no code or an unknown one are left as
/// they are.
pub fn add_manufacturer_names(
    rows: Option<&mut Value>,
    names: &HashMap<String, (String, String)>,
) {
    let Some(Value::Array(rows)) = rows else {
        return;
    };
    for row in rows {
        let Value::Object(obj) = row else { continue };
        let Some((code, name)) = obj
            .get("manufacturer")
            .and_then(Value::as_str)
            .and_then(|c| names.get(&c.to_uppercase()))
        else {
            continue;
        };
        let at = obj
            .keys()
            .position(|k| k == "manufacturer")
            .map_or(obj.len(), |i| i + 1);
        obj.shift_insert(at, "manufacturerCode".into(), Value::String(code.clone()));
        obj.shift_insert(
            at + 1,
            "manufacturerName".into(),
            Value::String(name.clone()),
        );
    }
}

/// Subdirectories holding something other than a manufacturer.
const NON_MANUFACTURER_DIRS: [&str; 3] = ["paintcolorlogos", "shop", "uistyleonly"];

pub fn is_manufacturer_record(record: &GameRecord) -> bool {
    const MARKER: &str = "scitemmanufacturer/";
    let path = record.record_path.to_lowercase();
    let Some(index) = path.rfind(MARKER) else {
        return false;
    };
    let rest = &path[index + MARKER.len()..];
    match rest.find('/') {
        None => true, // top level
        Some(slash) => !NON_MANUFACTURER_DIRS.contains(&&rest[..slash]),
    }
}

/// Placeholder and test rows outnumber real manufacturers; drop the obvious ones.
fn looks_real(m: &Manufacturer) -> bool {
    let Some(name) = m.name.as_deref().filter(|n| !n.is_empty()) else {
        return false;
    };
    if name.starts_with('@') {
        return false;
    }
    let lower = name.to_lowercase();
    !lower.contains("placeholder") && !lower.contains("<=") && lower != "unknown"
}

impl Domain for Manufacturers {
    fn name(&self) -> &'static str {
        "game-manufacturers"
    }
    fn schema_version(&self) -> u32 {
        1
    }

    fn description(&self) -> &'static str {
        "Manufacturer codes and display names."
    }

    fn filters(&self) -> &'static [&'static str] {
        &["**/scitemmanufacturer/**"]
    }

    fn needs_localization(&self) -> bool {
        true
    }

    fn parse(&self, ctx: &ParseContext<'_>) -> Result<Map<String, Value>, DomainError> {
        let all: Vec<Manufacturer> = ctx
            .records
            .of_type("SCItemManufacturer")
            .filter(|r| is_manufacturer_record(r))
            .map(|r| parse_manufacturer(r, ctx.localization))
            .collect();

        let mut named: Vec<Manufacturer> = all.iter().filter(|m| looks_real(m)).cloned().collect();
        // `(a.name ?? "").localeCompare(b.name ?? "")`; `sort_by` is stable like JS sort.
        named.sort_by(|a, b| {
            locale_compare(
                a.name.as_deref().unwrap_or(""),
                b.name.as_deref().unwrap_or(""),
            )
        });

        // Records reference manufacturers by code, so code -> name is what consumers want.
        let mut by_code = Map::new();
        for m in &named {
            if let Some(name) = &m.name {
                by_code.insert(m.code.clone(), Value::String(name.clone()));
            }
        }

        // Codes sharing one name (FSKI carries Aegis's name key) are reported, not fixed.
        // Grouped in a JS object, so names are enumerated in JS property order.
        let mut groups: Map<String, Value> = Map::new();
        for m in &named {
            if let Some(name) = &m.name {
                let codes = groups
                    .entry(name.clone())
                    .or_insert_with(|| Value::Array(Vec::new()));
                if let Value::Array(a) = codes {
                    a.push(Value::String(m.code.clone()));
                }
            }
        }
        let shared: Vec<SharedName> = js_key_order(&groups)
            .into_iter()
            .filter_map(|name| {
                let mut codes: Vec<String> = groups[name]
                    .as_array()?
                    .iter()
                    .filter_map(|c| c.as_str().map(str::to_string))
                    .collect();
                (codes.len() > 1).then(|| {
                    // Default JS sort: UTF-16 code units.
                    codes.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
                    SharedName {
                        name: name.clone(),
                        codes,
                    }
                })
            })
            .collect();

        let mut body = Map::new();
        field(&mut body, "manufacturers", &named)?;
        body.insert("byCode".into(), Value::Object(by_code));
        field(&mut body, "sharedNames", &shared)?;
        field(
            &mut body,
            "summary",
            &Summary {
                manufacturers: named.len(),
                unnamed_or_placeholder: all.len() - named.len(),
                names_used_by_multiple_codes: shared.len(),
            },
        )?;
        Ok(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::NoFiles;
    use crate::localization::parse_global_ini;
    use crate::records::{RecordSet, normalize_record};
    use serde_json::json;

    fn localization() -> Localization {
        parse_global_ini(
            "manufacturer_NameAEGS=Aegis Dynamics\nmanufacturer_NameUBA=Universal Body Armor\nmanufacturer_Name987=987",
        )
    }

    fn manufacturer(code: &str, dir: &str, name_key: &str) -> GameRecord {
        normalize_record(
            json!({
                "_RecordName_": format!("SCItemManufacturer.{code}"),
                "_RecordId_": format!("id-{code}"),
                "_RecordValue_": {"_Type_": "SCItemManufacturer", "Localization": {"Name": name_key}}
            }),
            &format!(
                "libs/foundry/records/scitemmanufacturer/{dir}{}",
                code.to_lowercase()
            ),
        )
        .unwrap()
    }

    fn parse(records: Vec<GameRecord>, loc: &Localization) -> Map<String, Value> {
        let set = RecordSet::new(records);
        DOMAIN.parse(&ParseContext::new(&set, loc, &NoFiles)).unwrap()
    }

    // Cases from sc-gamedata src/domains/manufacturers.test.ts.
    #[test]
    fn is_manufacturer_record_cases() {
        assert!(is_manufacturer_record(&manufacturer("AEGS", "", "@x")));
        assert!(is_manufacturer_record(&manufacturer("987", "clothes/", "@x")));
        assert!(is_manufacturer_record(&manufacturer("UBA", "armor/", "@x")));
        assert!(!is_manufacturer_record(&manufacturer("Paint_Raft", "paintcolorlogos/", "@x")));
        assert!(!is_manufacturer_record(&manufacturer("Shop_Generic", "shop/shopkiosk/", "@x")));
        assert!(!is_manufacturer_record(&manufacturer("UILO_A", "uistyleonly/", "@x")));
    }

    #[test]
    fn resolves_the_localized_name_and_keeps_the_code() {
        let m = parse_manufacturer(&manufacturer("AEGS", "", "@manufacturer_NameAEGS"), &localization());
        assert_eq!(m.code, "AEGS");
        assert_eq!(m.name.as_deref(), Some("Aegis Dynamics"));
    }

    #[test]
    fn builds_a_code_to_name_map_from_real_manufacturers_only() {
        let body = parse(
            vec![
                manufacturer("AEGS", "", "@manufacturer_NameAEGS"),
                manufacturer("UBA", "armor/", "@manufacturer_NameUBA"),
                manufacturer("Paint_X", "paintcolorlogos/", "@manufacturer_NameAEGS"),
            ],
            &localization(),
        );
        assert_eq!(
            body["byCode"],
            json!({"AEGS": "Aegis Dynamics", "UBA": "Universal Body Armor"})
        );
    }

    #[test]
    fn reports_codes_that_share_a_name() {
        let body = parse(
            vec![
                manufacturer("AEGS", "", "@manufacturer_NameAEGS"),
                manufacturer("FSKI", "", "@manufacturer_NameAEGS"),
            ],
            &localization(),
        );
        assert_eq!(
            body["sharedNames"],
            json!([{"name": "Aegis Dynamics", "codes": ["AEGS", "FSKI"]}])
        );
    }

    #[test]
    fn drops_rows_whose_name_never_resolved() {
        let body = parse(
            vec![manufacturer("NOPE", "", "@manufacturer_NameMissing")],
            &localization(),
        );
        assert_eq!(body["manufacturers"], json!([]));
        assert_eq!(body["summary"]["unnamedOrPlaceholder"], 1);
    }

    #[test]
    fn yields_empty_output_with_no_localization() {
        let body = parse(
            vec![manufacturer("AEGS", "", "@manufacturer_NameAEGS")],
            &Localization::new(),
        );
        assert_eq!(body["manufacturers"], json!([]));
    }

    #[test]
    fn publishes_the_record_code_beside_the_short_name() {
        let mut aegs = manufacturer("AEGS", "", "@manufacturer_NameAEGS");
        aegs.value["Code"] = json!("AEG");
        let mut empty = manufacturer("UBA", "armor/", "@manufacturer_NameUBA");
        empty.value["Code"] = json!("");
        assert_eq!(
            parse_manufacturer(&aegs, &localization()).record_code.as_deref(),
            Some("AEG")
        );
        assert_eq!(parse_manufacturer(&empty, &localization()).record_code, None);
        let body = parse(vec![aegs], &localization());
        assert_eq!(body["manufacturers"][0]["code"], "AEGS");
        assert_eq!(body["manufacturers"][0]["recordCode"], "AEG");
    }

    #[test]
    fn names_codes_case_insensitively_and_inserts_after_the_code() {
        let set = RecordSet::new(vec![
            manufacturer("AEGS", "", "@manufacturer_NameAEGS"),
            manufacturer("Paint_X", "paintcolorlogos/", "@manufacturer_NameUBA"),
            manufacturer("NOPE", "", "@manufacturer_NameMissing"),
        ]);
        let names = names_by_code(&set, &localization());
        assert_eq!(names.len(), 1);
        let mut rows = json!([
            {"name": "G", "manufacturer": "aegs", "role": "Fighter"},
            {"name": "X", "manufacturer": "NOPE"},
            {"name": "Y"}
        ]);
        add_manufacturer_names(Some(&mut rows), &names);
        let keys: Vec<_> = rows[0].as_object().unwrap().keys().cloned().collect();
        assert_eq!(
            keys,
            [
                "name",
                "manufacturer",
                "manufacturerCode",
                "manufacturerName",
                "role"
            ]
        );
        assert_eq!(rows[0]["manufacturerCode"], "AEGS");
        assert_eq!(rows[0]["manufacturerName"], "Aegis Dynamics");
        assert_eq!(rows[1], json!({"name": "X", "manufacturer": "NOPE"}));
        assert_eq!(rows[2], json!({"name": "Y"}));
        add_manufacturer_names(None, &names);
    }

    #[test]
    fn output_shape_and_order() {
        let body = parse(
            vec![
                manufacturer("UBA", "armor/", "@manufacturer_NameUBA"),
                manufacturer("987", "clothes/", "@manufacturer_Name987"),
                manufacturer("AEGS", "", "@manufacturer_NameAEGS"),
            ],
            &localization(),
        );
        let keys: Vec<_> = body.keys().cloned().collect();
        assert_eq!(keys, vec!["manufacturers", "byCode", "sharedNames", "summary"]);
        // Digits sort before letters; rows without a description omit the field.
        assert_eq!(
            body["manufacturers"][0],
            json!({"code": "987", "recordName": "SCItemManufacturer.987", "id": "id-987", "name": "987",
                   "nameKey": "manufacturer_Name987", "recordCode": null})
        );
        assert_eq!(body["manufacturers"][1]["code"], "AEGS");
        assert_eq!(
            body["summary"],
            json!({"manufacturers": 3, "unnamedOrPlaceholder": 0, "namesUsedByMultipleCodes": 0})
        );
    }
}
