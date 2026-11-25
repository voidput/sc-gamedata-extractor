//! Attributes: the game's default option values (graphics, audio, camera, flight, head
//! tracking, HUD...), the ones the in-game Options screen reads and writes (port of
//! `sc-gamedata/src/domains/attributes.ts`, branch `feat/keybinds-domain`).
//!
//! Not a DataCore record. The defaults ship as a raw archive file,
//! `Data/Libs/Config/Profiles/default/attributes.xml`, in binary CryXmlB, so this domain
//! declares it under [`Domain::files`] like `game-keybinds`.
//!
//! The file's shape, as shipped:
//!
//! ```text
//! <Attributes Version="35">
//!   <Attr name cvar value platform? confirmation? preview? gameOnly? nodefault? />
//! ```
//!
//! A player's own `USER/.../Profiles/default/attributes.xml` has the same shape but lists
//! only the options they changed, which is why the defaults matter.
//!
//! Quirks found in the real file:
//!
//! - One name can appear several times with different `platform` values (`Height` for
//!   `pc`, `xbox_s`, `ps5|xbox_x`). Every row is kept, in file order.
//! - `Resolution` has no `cvar`; it is kept with `cvar: null`.
//! - `platform` is a `|`-separated list; absent means every platform.
//!
//! The file carries no labels and no ranges. Labels are resolved only where the string
//! table demonstrably has a key built from the option name: the Options screen's
//! `pause_Options<Name>` or `pause_<Name>` (exact, case-insensitive, with the `,P` suffix
//! tolerated). Anything else stays unlabelled rather than guessed. Bindscape reads `label`,
//! `labelKey`, `platform`, `confirmation` and `version`: none of them is redacted.

use super::keybinds::{missing_files_warning, path_ends_with, read_profile_file};
use crate::cryxml::XmlNode;
use crate::domain::{Domain, DomainError, ParseContext, field};
use crate::helpers_world::{js_to_number, js_trim};
use crate::js::number;
use crate::localization::Localization;
use crate::xml::children_named;
use serde::Serialize;
use serde_json::{Map, Value, json};
use std::collections::{HashMap, HashSet};

pub static DOMAIN: Attributes = Attributes;

pub struct Attributes;

const GLOBS: &[&str] = &["**/Libs/Config/Profiles/default/attributes.xml"];

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameAttribute {
    pub name: String,
    /// The console variable the option drives, or null when it has none.
    pub cvar: Option<String>,
    /// The default value, as the file writes it (always a string).
    pub default: String,
    /// Platforms this row applies to (`pc`, `ps5`, ...), or null for all.
    pub platform: Option<Vec<String>>,
    /// The game asks the player to confirm a change (resolution, window mode...).
    pub confirmation: bool,
    /// The option previews live while the slider moves.
    pub preview: bool,
    pub game_only: bool,
    pub nodefault: bool,
    /// Display label from global.ini, or null when no key matches the name.
    pub label: Option<String>,
    pub label_key: Option<String>,
}

fn flag(value: Option<&str>) -> bool {
    value.is_some_and(|v| {
        let t = js_trim(v);
        !t.is_empty() && t != "0"
    })
}

/// `indexTable`: lowercased key (without a `,P` suffix) -> (original key, text). Exact
/// keys win.
pub fn index_table(table: &Localization) -> HashMap<String, (&str, &str)> {
    let mut index: HashMap<String, (&str, &str)> = HashMap::new();
    let mut suffixed: Vec<(String, &str, &str)> = Vec::new();
    for (key, value) in table.iter() {
        match key.find(',').filter(|&c| c > 0) {
            Some(comma) => suffixed.push((key[..comma].to_lowercase(), &key[..comma], value)),
            None => {
                index.entry(key.to_lowercase()).or_insert((key, value));
            }
        }
    }
    for (lower, bare, value) in suffixed {
        index.entry(lower).or_insert((bare, value));
    }
    index
}

/// `attributeLabel`: `pause_Options<name>` then `pause_<name>`, case-insensitively, `,P`
/// tolerated; `(label, labelKey)`.
pub fn attribute_label<'a>(
    name: &str,
    index: &HashMap<String, (&'a str, &'a str)>,
) -> Option<(&'a str, &'a str)> {
    [format!("pause_Options{name}"), format!("pause_{name}")]
        .iter()
        .find_map(|candidate| {
            let (key, text) = index.get(&candidate.to_lowercase())?;
            (!text.is_empty()).then_some((*text, *key))
        })
}

/// `parseAttributes`: the dataset body of a parsed defaults `attributes.xml`.
pub fn parse_attributes(
    root: &XmlNode,
    localization: &Localization,
) -> Result<Map<String, Value>, DomainError> {
    let index = index_table(localization);
    let version = root
        .attr("Version")
        .or_else(|| root.attr("version"))
        .map(js_to_number)
        .filter(|n| n.is_finite());

    let attributes: Vec<GameAttribute> = children_named(root, "Attr")
        .filter(|a| !js_trim(a.attr("name").unwrap_or_default()).is_empty())
        .map(|a| {
            let name = a.attr("name").unwrap_or_default();
            let cvar = a
                .attr("cvar")
                .map(js_trim)
                .filter(|c| !c.is_empty())
                .map(str::to_string);
            let platform = a
                .attr("platform")
                .filter(|p| !js_trim(p).is_empty())
                .map(|p| {
                    p.split('|')
                        .map(js_trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)
                        .collect()
                });
            let label = attribute_label(name, &index);
            GameAttribute {
                name: name.to_string(),
                cvar,
                default: a.attr("value").unwrap_or_default().to_string(),
                platform,
                confirmation: flag(a.attr("confirmation")),
                preview: flag(a.attr("preview")),
                game_only: flag(a.attr("gameOnly")),
                nodefault: flag(a.attr("nodefault")),
                label: label.map(|(l, _)| l.to_string()),
                label_key: label.map(|(_, k)| k.to_string()),
            }
        })
        .collect();

    let names: HashSet<&str> = attributes.iter().map(|a| a.name.as_str()).collect();
    let labelled: HashSet<&str> = attributes
        .iter()
        .filter(|a| a.label.is_some())
        .map(|a| a.name.as_str())
        .collect();
    let summary = json!({
        "rows": attributes.len(),
        "names": names.len(),
        "withCvar": attributes.iter().filter(|a| a.cvar.is_some()).count(),
        "platformSpecific": attributes.iter().filter(|a| a.platform.is_some()).count(),
        "labelled": labelled.len(),
    });

    let mut body = Map::new();
    body.insert("version".into(), version.map_or(Value::Null, number));
    field(&mut body, "attributes", &attributes)?;
    body.insert("summary".into(), summary);
    Ok(body)
}

impl Domain for Attributes {
    fn name(&self) -> &'static str {
        "game-attributes"
    }

    fn description(&self) -> &'static str {
        "Default option values and the console variable each drives, from Profiles/default/attributes.xml."
    }

    fn filters(&self) -> &'static [&'static str] {
        &[]
    }

    fn files(&self) -> &'static [&'static str] {
        GLOBS
    }

    fn needs_localization(&self) -> bool {
        true
    }

    fn parse(&self, ctx: &ParseContext<'_>) -> Result<Map<String, Value>, DomainError> {
        match read_profile_file(ctx.files, GLOBS, |p| {
            path_ends_with(p, "profiles/default/attributes.xml")
        }) {
            None => {
                missing_files_warning(ctx, self.name(), GLOBS);
                let mut body = Map::new();
                body.insert("version".into(), Value::Null);
                body.insert("attributes".into(), Value::Array(Vec::new()));
                Ok(body)
            }
            Some(Err(e)) => Err(DomainError::Message(e)),
            Some(Ok(root)) => parse_attributes(&root, ctx.localization),
        }
    }
}

#[cfg(test)]
mod tests {
    //! Cases from `src/domains/attributes.test.ts`.
    use super::*;
    use crate::fixtures::MemFiles;
    use crate::localization::parse_global_ini;
    use crate::records::RecordSet;
    use crate::xml::parse_xml;

    fn localization() -> Localization {
        parse_global_ini(
            &[
                "pause_OptionsFOV=Field Of View",
                "pause_OptionsCrosshairMode,P=Crosshair Mode",
                "pause_FilmGrain=Film Grain",
                "pause_OptionsVSync=",
                "pause_VSync=VSync",
                "ui_Unrelated_EspEnabled=Not a label",
            ]
            .join("\n"),
        )
    }

    const ATTRIBUTES: &str = r#"<Attributes Version="35">
  <Attr name="Resolution" value="-1" platform="pc" confirmation="1" />
  <Attr name="Height" cvar="r_height" value="720" platform="pc" confirmation="1" />
  <Attr name="Height" cvar="r_height" value="1080" platform="ps5|xbox_x" confirmation="1" />
  <Attr name="FOV" cvar="cl_fov" value="85" />
  <Attr name="CrosshairMode" cvar="w_crosshair_mode" value="1" />
  <Attr name="FilmGrain" cvar="r_filmgrain" value="1" preview="1" />
  <Attr name="VSync" cvar="r_VSync" value="1" platform="pc" confirmation="0" />
  <Attr name="EspEnabled" cvar="v_esp" value="1" gameOnly="1" nodefault="1" />
  <Attr value="orphan" />
</Attributes>"#;

    fn body() -> Map<String, Value> {
        parse_attributes(&parse_xml(ATTRIBUTES).unwrap(), &localization()).unwrap()
    }

    fn rows() -> Vec<Value> {
        body()["attributes"].as_array().unwrap().clone()
    }

    fn by_name(name: &str) -> Value {
        rows().into_iter().find(|a| a["name"] == name).unwrap()
    }

    fn matches(got: &Value, want: Value) {
        for (k, v) in want.as_object().unwrap() {
            assert_eq!(&got[k], v, "{k} in {got}");
        }
    }

    #[test]
    fn reads_the_version_and_every_named_row_in_file_order() {
        assert_eq!(body()["version"].as_f64(), Some(35.0));
        let names: Vec<String> = rows()
            .iter()
            .map(|a| a["name"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(
            names,
            [
                "Resolution",
                "Height",
                "Height",
                "FOV",
                "CrosshairMode",
                "FilmGrain",
                "VSync",
                "EspEnabled"
            ]
        );
    }

    #[test]
    fn keeps_platform_variants_apart_and_splits_the_platform_list() {
        let heights: Vec<(Value, Value)> = rows()
            .into_iter()
            .filter(|a| a["name"] == "Height")
            .map(|a| (a["default"].clone(), a["platform"].clone()))
            .collect();
        assert_eq!(
            heights,
            vec![
                (json!("720"), json!(["pc"])),
                (json!("1080"), json!(["ps5", "xbox_x"])),
            ]
        );
    }

    #[test]
    fn keeps_a_row_without_a_cvar_and_values_as_strings() {
        matches(
            &rows()[0],
            json!({"cvar": null, "default": "-1", "confirmation": true}),
        );
    }

    #[test]
    fn reads_boolean_flags_only_when_set_to_a_non_zero_value() {
        assert_eq!(by_name("VSync")["confirmation"], false);
        assert_eq!(by_name("FilmGrain")["preview"], true);
        matches(
            &by_name("EspEnabled"),
            json!({"gameOnly": true, "nodefault": true, "preview": false}),
        );
    }

    #[test]
    fn labels_only_from_pause_options_name_or_pause_name() {
        matches(
            &by_name("FOV"),
            json!({"label": "Field Of View", "labelKey": "pause_OptionsFOV"}),
        );
        matches(
            &by_name("CrosshairMode"),
            json!({"label": "Crosshair Mode", "labelKey": "pause_OptionsCrosshairMode"}),
        );
        assert_eq!(by_name("FilmGrain")["label"], "Film Grain");
        // An empty pause_Options key falls through to pause_<Name>.
        assert_eq!(by_name("VSync")["labelKey"], "pause_VSync");
        // A key that merely contains the name is not a match.
        matches(
            &by_name("EspEnabled"),
            json!({"label": null, "labelKey": null}),
        );
    }

    #[test]
    fn summarises_rows_names_and_label_coverage() {
        assert_eq!(
            body()["summary"],
            json!({"rows": 8, "names": 7, "withCvar": 7, "platformSpecific": 4, "labelled": 4})
        );
    }

    fn run(files: &MemFiles) -> Map<String, Value> {
        let records = RecordSet::new(Vec::new());
        let loc = localization();
        let ctx = ParseContext::new(&records, &loc, files);
        DOMAIN.parse(&ctx).unwrap()
    }

    #[test]
    fn domain_finds_the_defaults_file_among_the_archive_files() {
        let mut files = MemFiles::default();
        files.insert(
            r"Data\Libs\Config\defaultProfile.xml",
            b"<profile/>".to_vec(),
        );
        let bin = crate::cryxml::tests::cryxml(&parse_xml(ATTRIBUTES).unwrap());
        files.insert(r"Data\Libs\Config\Profiles\default\attributes.xml", bin);
        let body = run(&files);
        assert_eq!(body["attributes"].as_array().unwrap().len(), 8);
        assert_eq!(body, self::body());
    }

    #[test]
    fn matches_the_archive_path_the_declared_glob_selects() {
        assert_eq!(
            DOMAIN.files(),
            ["**/Libs/Config/Profiles/default/attributes.xml"]
        );
        let mut files = MemFiles::default();
        files.insert(
            r"Data\Libs\Config\Profiles\default\attributes.xml",
            b"<Attributes/>".to_vec(),
        );
        // A player's profile is not the defaults file.
        files.insert(
            r"USER\Client\0\Profiles\default\attributes.xml",
            b"<Attributes/>".to_vec(),
        );
        assert_eq!(
            crate::domain::files_matching(&files, DOMAIN.files()),
            [r"Data\Libs\Config\Profiles\default\attributes.xml"]
        );
    }

    #[test]
    fn publishing_keeps_labels_platforms_and_version() {
        let dump = Value::Object(body());
        assert_eq!(
            crate::localization::redaction::redact_dump("game-attributes.json", &dump),
            dump
        );
    }

    #[test]
    fn domain_yields_an_empty_list_when_the_file_is_missing() {
        let body = run(&MemFiles::default());
        assert_eq!(body["attributes"], json!([]));
        assert_eq!(body["version"], Value::Null);
    }
}
