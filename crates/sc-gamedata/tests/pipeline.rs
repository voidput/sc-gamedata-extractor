//! End to end on synthetic records: envelopes -> scope -> domain -> file text.

use sc_gamedata::compare::compare_text;
use sc_gamedata::domain::NoFiles;
use sc_gamedata::domains;
use sc_gamedata::fixtures::FixtureSource;
use sc_gamedata::localization::parse_global_ini;
use sc_gamedata::pipeline::{self, GOLDEN_EXTRACTION_FILTERS, Game};
use serde_json::{Value, json};

fn manufacturer(code: &str, dir: &str, name_key: &str) -> Value {
    json!({
        "_RecordName_": format!("SCItemManufacturer.{code}"),
        "_RecordId_": format!("id-{code}"),
        "_RecordPath_": format!("libs/foundry/records/scitemmanufacturer/{dir}{}.xml", code.to_lowercase()),
        "_RecordValue_": {"_Type_": "SCItemManufacturer", "Localization": {
            "_Type_": "SCItemLocalization", "Name": name_key, "Description": "@LOC_EMPTY"}}
    })
}

#[test]
fn manufacturers_file_is_byte_exact() {
    let source = FixtureSource::from_envelopes(vec![
        manufacturer("UBA", "armor/", "@manufacturer_NameUBA"),
        manufacturer("AEGS", "", "@manufacturer_NameAEGS"),
        manufacturer("987", "clothes/", "@manufacturer_Name987"),
        manufacturer("FSKI", "", "@manufacturer_NameAEGS"),
        manufacturer("Paint_X", "paintcolorlogos/", "@manufacturer_NameAEGS"),
        json!({"_RecordName_": "Other.X", "_RecordPath_": "libs/foundry/records/other/x.xml",
               "_RecordValue_": {"_Type_": "Other"}}),
    ])
    .unwrap();
    let loc = parse_global_ini(
        "\u{feff}manufacturer_NameAEGS=Aegis Dynamics\r\nmanufacturer_NameUBA=Universal Body Armor\r\nmanufacturer_Name987=987\r\n",
    );
    let game = Game {
        records: &source,
        localization: &loc,
        files: &NoFiles,
        build: Some("4.10.200.58746".into()),
    };
    let domain = domains::find("manufacturers").unwrap();
    let ds = pipeline::run(domain, &game, "2026-11-12T18:18:38.829Z").unwrap();
    let expected = r#"{
  "_source": "Star Citizen game files via StarBreaker",
  "_extracted": "2026-11-12T18:18:38.829Z",
  "_build": "4.10.200.58746",
  "manufacturers": [
    {
      "code": "987",
      "recordName": "SCItemManufacturer.987",
      "id": "id-987",
      "name": "987",
      "nameKey": "manufacturer_Name987"
    },
    {
      "code": "AEGS",
      "recordName": "SCItemManufacturer.AEGS",
      "id": "id-AEGS",
      "name": "Aegis Dynamics",
      "nameKey": "manufacturer_NameAEGS"
    },
    {
      "code": "FSKI",
      "recordName": "SCItemManufacturer.FSKI",
      "id": "id-FSKI",
      "name": "Aegis Dynamics",
      "nameKey": "manufacturer_NameAEGS"
    },
    {
      "code": "UBA",
      "recordName": "SCItemManufacturer.UBA",
      "id": "id-UBA",
      "name": "Universal Body Armor",
      "nameKey": "manufacturer_NameUBA"
    }
  ],
  "byCode": {
    "987": "987",
    "AEGS": "Aegis Dynamics",
    "FSKI": "Aegis Dynamics",
    "UBA": "Universal Body Armor"
  },
  "sharedNames": [
    {
      "name": "Aegis Dynamics",
      "codes": [
        "AEGS",
        "FSKI"
      ]
    }
  ],
  "summary": {
    "manufacturers": 4,
    "unnamedOrPlaceholder": 0,
    "namesUsedByMultipleCodes": 1
  }
}"#;
    assert_eq!(ds.to_json(), expected);
    assert!(ds.warnings.is_empty(), "{:?}", ds.warnings);
    // And the harness's diff agrees with itself on a different timestamp.
    let other = expected.replace("2026-11-12T18:18:38.829Z", "2030-01-01T00:00:00.000Z");
    assert!(compare_text(&ds.to_json(), &other).is_ok());
}

#[test]
fn every_registered_domain_is_covered_by_the_golden_extraction() {
    for d in domains::all() {
        for f in d.filters() {
            assert!(
                GOLDEN_EXTRACTION_FILTERS.contains(f),
                "{}: filter {f} was not part of the golden extraction; add it to GOLDEN_EXTRACTION_FILTERS only if sc-gamedata extracts it",
                d.name()
            );
        }
        assert!(d.name().starts_with("game-"), "{}", d.name());
    }
}

#[test]
fn domain_names_are_unique_and_findable() {
    let all = domains::all();
    let mut names: Vec<_> = all.iter().map(|d| d.name()).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), all.len());
    for d in all {
        assert_eq!(domains::find(d.name()).unwrap().name(), d.name());
    }
}
