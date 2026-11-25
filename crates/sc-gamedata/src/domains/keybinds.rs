//! Keybinds: the game's default control bindings, per action map and device (port of
//! `sc-gamedata/src/domains/keybinds.ts`, branch `feat/keybinds-domain`).
//!
//! Not a DataCore record. The bindings ship as a raw archive file,
//! `Data/Libs/Config/defaultProfile.xml`, in binary CryXmlB, so this domain declares it
//! under [`Domain::files`] and reads it from the archive.
//!
//! The file's shape, as shipped:
//!
//! ```text
//! <profile>
//!   <ActivationModes>   named press/hold/tap timing presets
//!   <modifiers>         inputs that act as modifiers (lshift, kb_lalt, ...)
//!   <ActionCategories>  nested categories, referenced by an action's Category
//!   <optiontree type>   invert/curve/sensitivity groups actions point at (optionGroup)
//!   <actionmap name UILabel UICategory>   ~50 of these, in UI order
//!     <action name keyboard mouse gamepad joystick activationMode UILabel ...>
//! ```
//!
//! Quirks found in the real file:
//!
//! - A device attribute of `" "` means explicitly unbound on that device; an absent
//!   attribute means the device does not apply. Both are kept apart: `[]` versus `null`.
//! - A device can be a child element instead: `<gamepad input=" " activationMode="..."/>`,
//!   or `<keyboard><inputdata input="enter"/>...` carrying several inputs. The child wins
//!   over the attribute.
//! - The activation mode attribute is spelled `activationMode`, `ActivationMode` and
//!   `activationmode` on different actions.
//! - One action map misspells its label attribute `UILable`.
//!
//! Descriptions are kept as keys only (`descriptionKey`). Published datasets carry name
//! keys, never descriptive prose. Bindscape and sc-telemetry read `label`, `labelKey`,
//! `defaults`, `optionGroup` and `optionTrees`: none of them is redacted on publish.
//!
//! Objects are built field by field: a TypeScript field set to `undefined` (an attribute
//! the file lacks) is left out of the JSON, which serde structs would write as `null`.

use crate::cryxml::XmlNode;
use crate::domain::{Domain, DomainError, FileSource, ParseContext, files_matching};
use crate::helpers_world::{js_to_number, js_trim};
use crate::js::number;
use crate::localization::Localization;
use crate::xml::{child_named, children_named, read_document};
use serde_json::{Map, Value, json};
use std::cell::OnceCell;
use std::collections::{BTreeSet, HashMap};

pub static DOMAIN: Keybinds = Keybinds;

pub struct Keybinds;

pub const DEVICES: [&str; 4] = ["keyboard", "mouse", "gamepad", "joystick"];

const ACTIVATION_MODE_FIELDS: [&str; 9] = [
    "onPress",
    "onHold",
    "onRelease",
    "multiTap",
    "multiTapBlock",
    "pressTriggerThreshold",
    "releaseTriggerThreshold",
    "releaseTriggerDelay",
    "retriggerable",
];

/// Numeric action attributes copied into `flags` when present.
const NUMERIC_FLAGS: [&str; 12] = [
    "onPress",
    "onRelease",
    "onHold",
    "always",
    "retriggerable",
    "noModifiers",
    "holdTriggerDelay",
    "holdRepeatDelay",
    "pressTriggerThreshold",
    "releaseTriggerThreshold",
    "useAnalogCompare",
    "analogCompareVal",
];

/// String action attributes copied into `flags` when present.
const STRING_FLAGS: [&str; 1] = ["analogCompareOp"];

const GLOBS: &[&str] = &["**/Libs/Config/defaultProfile.xml"];

/// `num`: `Number(value)` when the attribute is set, non-blank and finite.
pub(crate) fn num(value: Option<&str>) -> Option<f64> {
    let v = value?;
    if js_trim(v).is_empty() {
        return None;
    }
    let n = js_to_number(v);
    n.is_finite().then_some(n)
}

/// `nonEmpty`: the value (untrimmed) unless absent or blank.
pub(crate) fn non_empty(value: Option<&str>) -> Option<&str> {
    value.filter(|v| !js_trim(v).is_empty())
}

fn opt_num(x: Option<f64>) -> Value {
    x.map_or(Value::Null, number)
}

fn opt_str(x: Option<&str>) -> Value {
    x.map_or(Value::Null, Value::from)
}

/// Insert unless `None` (an `undefined` field, dropped by `JSON.stringify`).
fn put(map: &mut Map<String, Value>, key: &str, value: Option<Value>) {
    if let Some(v) = value {
        map.insert(key.to_string(), v);
    }
}

/// `attrAnyCase`: an attribute whatever its casing (exact name first, else the first key,
/// in object order, equal ignoring case).
fn attr_any_case<'a>(node: &'a XmlNode, name: &str) -> Option<&'a str> {
    if let Some(v) = node.attr(name) {
        return Some(v);
    }
    let lower = name.to_lowercase();
    let key = node
        .attrs
        .iter()
        .find(|(k, _)| k.to_lowercase() == lower)?
        .0
        .as_str();
    node.attr(key)
}

/// The archive file a domain reads: the first path, sorted, its globs select and `wanted`
/// accepts (the TypeScript walks the extraction directory, sorts, then `find`s).
pub(crate) fn read_profile_file(
    files: &dyn FileSource,
    globs: &[&str],
    wanted: impl Fn(&str) -> bool,
) -> Option<Result<XmlNode, String>> {
    let mut hits: Vec<(String, &str)> = files_matching(files, globs)
        .into_iter()
        .map(|n| (n.replace('\\', "/"), n))
        .collect();
    hits.sort();
    let (path, name) = hits.into_iter().find(|(p, _)| wanted(p))?;
    Some(
        files
            .read(name)
            .and_then(|bytes| read_document(&bytes))
            .map_err(|e| format!("{path}: {e}")),
    )
}

/// `(^|/)<file>$`, case-insensitive.
pub(crate) fn path_ends_with(path: &str, file: &str) -> bool {
    let p = path.to_lowercase();
    p == file || p.ends_with(&format!("/{file}"))
}

/// Resolves `@key` labels against the string table, case-insensitively, and remembers
/// which keys it could not resolve (`LabelResolver`).
pub struct LabelResolver<'a> {
    table: &'a Localization,
    lower: OnceCell<HashMap<String, &'a str>>,
    pub unresolved: BTreeSet<String>,
}

impl<'a> LabelResolver<'a> {
    pub fn new(table: &'a Localization) -> Self {
        Self {
            table,
            lower: OnceCell::new(),
            unresolved: BTreeSet::new(),
        }
    }

    /// About 13,000 keys in global.ini carry a `,P` suffix (`ui_CIMiningMode,P=`) while the
    /// XML references the bare key, so the suffix is tried and also indexed away. An exact
    /// key always wins over a suffixed one.
    fn lookup(&self, key: &str) -> Option<&'a str> {
        let table = self.table;
        if let Some(v) = table.get(key).or_else(|| table.get(&format!("{key},P"))) {
            return Some(v);
        }
        let lower = self.lower.get_or_init(|| {
            let mut lower: HashMap<String, &'a str> = HashMap::new();
            let mut suffixed: Vec<(String, &'a str)> = Vec::new();
            let mut seen_suffixed: std::collections::HashSet<String> = Default::default();
            for (k, v) in table.iter() {
                let lowered = k.to_lowercase();
                if let Some(comma) = lowered.find(',').filter(|&c| c > 0) {
                    let bare = lowered[..comma].to_string();
                    if seen_suffixed.insert(bare.clone()) {
                        suffixed.push((bare, v));
                    }
                }
                lower.entry(lowered).or_insert(v);
            }
            for (k, v) in suffixed {
                lower.entry(k).or_insert(v);
            }
            lower
        });
        lower.get(&key.to_lowercase()).copied()
    }

    /// Resolve a label key. A missing key yields the fallback; an unresolved key yields the
    /// raw `@token` so the gap stays visible, and is counted.
    pub fn resolve(&mut self, raw_key: Option<&str>, fallback: &str) -> String {
        let Some(raw) = raw_key.filter(|k| !k.is_empty()) else {
            return fallback.to_string();
        };
        let Some(key) = raw.strip_prefix('@') else {
            return raw.to_string();
        };
        match self.lookup(key) {
            Some(text) if !text.is_empty() => text.to_string(),
            _ => {
                self.unresolved.insert(raw.to_string());
                raw.to_string()
            }
        }
    }

    /// `resolve` with an optional fallback (`undefined` in, `undefined` out).
    fn resolve_opt(&mut self, raw_key: Option<&str>, fallback: Option<&str>) -> Option<String> {
        match (raw_key.filter(|k| !k.is_empty()), fallback) {
            (None, None) => None,
            (k, f) => Some(self.resolve(k, f.unwrap_or_default())),
        }
    }
}

fn inputs_of(raw: Option<&str>) -> Vec<String> {
    let Some(raw) = raw else {
        return Vec::new();
    };
    let trimmed = js_trim(raw).to_lowercase();
    if trimmed.is_empty() {
        Vec::new()
    } else {
        vec![trimmed]
    }
}

/// One device's default binding, from a child element if present, else the attribute.
/// `None`: device not applicable; empty: explicitly unbound; otherwise the inputs.
pub fn device_binding(action: &XmlNode, device: &str) -> Option<Vec<String>> {
    let child = child_named(action, device);
    if let Some(child) = child {
        let data: Vec<&XmlNode> = children_named(child, "inputdata").collect();
        if !data.is_empty() {
            return Some(
                data.iter()
                    .flat_map(|d| inputs_of(d.attr("input")))
                    .collect(),
            );
        }
        if child.attr("input").is_some() {
            return Some(inputs_of(child.attr("input")));
        }
    }
    if let Some(attr) = action.attr(device) {
        return Some(inputs_of(Some(attr)));
    }
    // A bare child with no input still says the device applies; it is unbound.
    child.map(|_| Vec::new())
}

fn binding_value(b: &Option<Vec<String>>) -> Value {
    b.as_ref().map_or(Value::Null, |v| json!(v))
}

fn parse_activation_modes(root: &XmlNode) -> Vec<Value> {
    let Some(container) = child_named(root, "ActivationModes") else {
        return Vec::new();
    };
    children_named(container, "ActivationMode")
        .map(|mode| {
            let mut m = Map::new();
            m.insert("name".into(), mode.attr("name").unwrap_or_default().into());
            for f in ACTIVATION_MODE_FIELDS {
                m.insert(f.into(), opt_num(num(mode.attr(f))));
            }
            Value::Object(m)
        })
        .collect()
}

fn parse_modifiers(root: &XmlNode) -> Vec<String> {
    let Some(container) = child_named(root, "modifiers") else {
        return Vec::new();
    };
    children_named(container, "mod")
        .flat_map(|m| inputs_of(m.attr("input")))
        .collect()
}

/// A category as written, and its resolved label (for actions' `Category` lookups).
struct Category {
    value: Value,
}

fn parse_categories(
    container: &XmlNode,
    labels: &mut LabelResolver<'_>,
    by_name: &mut HashMap<String, String>,
) -> Vec<Category> {
    children_named(container, "Category")
        .map(|category| {
            let name = category.attr("name").unwrap_or_default();
            let label_key = non_empty(category.attr("UILabel"));
            let label = labels.resolve(label_key, name);
            let mut m = Map::new();
            m.insert("name".into(), name.into());
            m.insert("action".into(), opt_str(non_empty(category.attr("action"))));
            m.insert("label".into(), label.clone().into());
            m.insert("labelKey".into(), opt_str(label_key));
            // `flattenCategories` sets a category before its children: a later one wins.
            by_name.insert(name.to_string(), label);
            let children = parse_categories(category, labels, by_name);
            m.insert(
                "children".into(),
                Value::Array(children.into_iter().map(|c| c.value).collect()),
            );
            Category {
                value: Value::Object(m),
            }
        })
        .collect()
}

/// One parsed action, with what the summary counts.
struct Action {
    value: Value,
    bound: [bool; 4],
    labelled: bool,
}

fn parse_action(
    action: &XmlNode,
    labels: &mut LabelResolver<'_>,
    categories: &HashMap<String, String>,
) -> Action {
    let name = action.attr("name").unwrap_or_default();
    let label_key = non_empty(action.attr("UILabel"));

    // UICategory is a string key; Category names an <ActionCategories> entry.
    let ui_category = non_empty(action.attr("UICategory"));
    let action_category = non_empty(action.attr("Category"));
    let category: Option<String> = if let Some(ui) = ui_category {
        Some(labels.resolve(Some(ui), ui))
    } else {
        action_category.map(|c| categories.get(c).cloned().unwrap_or_else(|| c.to_string()))
    };

    let mut defaults = Map::new();
    let mut per_device_activation = Map::new();
    let mut per_device_flags = Map::new();
    let mut bound = [false; 4];
    for (i, device) in DEVICES.iter().enumerate() {
        let binding = device_binding(action, device);
        bound[i] = binding.as_ref().is_some_and(|b| !b.is_empty());
        defaults.insert((*device).into(), binding_value(&binding));
        let Some(child) = child_named(action, device) else {
            continue;
        };
        if let Some(mode) = non_empty(attr_any_case(child, "activationMode")) {
            per_device_activation.insert((*device).into(), mode.into());
        }
        let mut child_flags = Map::new();
        for flag in NUMERIC_FLAGS {
            if let Some(v) = num(child.attr(flag)) {
                child_flags.insert(flag.into(), number(v));
            }
        }
        if !child_flags.is_empty() {
            per_device_flags.insert((*device).into(), Value::Object(child_flags));
        }
    }

    let mut flags = Map::new();
    for flag in NUMERIC_FLAGS {
        if let Some(v) = num(action.attr(flag)) {
            flags.insert(flag.into(), number(v));
        }
    }
    for flag in STRING_FLAGS {
        if let Some(v) = non_empty(action.attr(flag)) {
            flags.insert(flag.into(), v.into());
        }
    }

    let mut m = Map::new();
    m.insert("name".into(), name.into());
    m.insert("label".into(), labels.resolve(label_key, name).into());
    m.insert("labelKey".into(), opt_str(label_key));
    m.insert(
        "descriptionKey".into(),
        opt_str(non_empty(action.attr("UIDescription"))),
    );
    m.insert("category".into(), category.map_or(Value::Null, Value::from));
    m.insert(
        "categoryKey".into(),
        opt_str(ui_category.or(action_category)),
    );
    m.insert(
        "activationMode".into(),
        opt_str(non_empty(attr_any_case(action, "activationMode"))),
    );
    m.insert("defaults".into(), Value::Object(defaults));
    m.insert(
        "optionGroup".into(),
        opt_str(non_empty(action.attr("optionGroup"))),
    );
    m.insert("flags".into(), Value::Object(flags));
    if !per_device_activation.is_empty() {
        m.insert(
            "perDeviceActivation".into(),
            Value::Object(per_device_activation),
        );
    }
    if !per_device_flags.is_empty() {
        m.insert("perDeviceFlags".into(), Value::Object(per_device_flags));
    }
    if let Some(states) = child_named(action, "states") {
        let states: Vec<Value> = children_named(states, "state")
            .map(|state| {
                let state_name = state.attr("name").unwrap_or_default();
                let state_key = non_empty(state.attr("UILabel"));
                json!({
                    "name": state_name,
                    "label": labels.resolve(state_key, state_name),
                    "labelKey": opt_str(state_key),
                })
            })
            .collect();
        m.insert("states".into(), Value::Array(states));
    }

    Action {
        value: Value::Object(m),
        bound,
        labelled: label_key.is_some(),
    }
}

fn parse_action_map(
    map: &XmlNode,
    labels: &mut LabelResolver<'_>,
    categories: &HashMap<String, String>,
    actions: &mut Vec<Action>,
) -> Value {
    let name = map.attr("name").unwrap_or_default();
    // One shipped map spells it UILable (`UILabel ?? UILable`).
    let label_key = non_empty(map.attr("UILabel").or_else(|| map.attr("UILable")));
    let category_key = non_empty(map.attr("UICategory"));
    let mut m = Map::new();
    m.insert("name".into(), name.into());
    m.insert("version".into(), opt_num(num(map.attr("version"))));
    m.insert("label".into(), labels.resolve(label_key, name).into());
    m.insert("labelKey".into(), opt_str(label_key));
    m.insert(
        "descriptionKey".into(),
        opt_str(non_empty(map.attr("UIDescription"))),
    );
    m.insert(
        "category".into(),
        category_key.map_or(Value::Null, |k| labels.resolve(Some(k), k).into()),
    );
    m.insert("categoryKey".into(), opt_str(category_key));
    let start = actions.len();
    for a in children_named(map, "action") {
        let parsed = parse_action(a, labels, categories);
        actions.push(parsed);
    }
    m.insert(
        "actions".into(),
        Value::Array(actions[start..].iter().map(|a| a.value.clone()).collect()),
    );
    Value::Object(m)
}

/// `parseOptionTrees`: `<optiontree type="joystick">` holds the tunable functions
/// (`flight_move_pitch`, ...) that a player's `<options type="joystick" instance="N">`
/// overrides with invert and exponent. Actions point at them through their `optionGroup`
/// attribute. Flattened in tree order, keeping each group's parent.
pub fn parse_option_trees(root: &XmlNode, labels: &mut LabelResolver<'_>) -> Vec<Value> {
    fn walk(
        node: &XmlNode,
        parent: Option<Option<&str>>,
        labels: &mut LabelResolver<'_>,
        groups: &mut Vec<Value>,
    ) {
        for group in children_named(node, "optiongroup") {
            let name = group.attr("name");
            let label_key = non_empty(group.attr("UILabel"));
            let points: Vec<Value> = child_named(group, "nonlinearity_curve")
                .map(|curve| {
                    children_named(curve, "point")
                        .filter_map(|p| {
                            let (i, o) = (num(p.attr("in"))?, num(p.attr("out"))?);
                            Some(json!({"in": number(i), "out": number(o)}))
                        })
                        .collect()
                })
                .unwrap_or_default();
            let mut m = Map::new();
            put(&mut m, "name", name.map(Value::from));
            // `parent` is null at the top, the enclosing name below (undefined if unnamed).
            put(
                &mut m,
                "parent",
                match parent {
                    None => Some(Value::Null),
                    Some(p) => p.map(Value::from),
                },
            );
            put(
                &mut m,
                "label",
                labels.resolve_opt(label_key, name).map(Value::from),
            );
            m.insert("labelKey".into(), opt_str(label_key));
            m.insert(
                "showInvert".into(),
                opt_num(num(group.attr("UIShowInvert"))),
            );
            m.insert("showCurve".into(), opt_num(num(group.attr("UIShowCurve"))));
            m.insert(
                "showSensitivity".into(),
                opt_num(num(group.attr("UIShowSensitivity"))),
            );
            m.insert("invert".into(), opt_num(num(group.attr("invert"))));
            m.insert("exponent".into(), opt_num(num(group.attr("exponent"))));
            m.insert(
                "curve".into(),
                if points.is_empty() {
                    Value::Null
                } else {
                    Value::Array(points)
                },
            );
            groups.push(Value::Object(m));
            walk(group, Some(name), labels, groups);
        }
    }

    children_named(root, "optiontree")
        .map(|tree| {
            let mut groups = Vec::new();
            walk(tree, None, labels, &mut groups);
            let mut m = Map::new();
            put(&mut m, "type", tree.attr("type").map(Value::from));
            m.insert("instances".into(), opt_num(num(tree.attr("instances"))));
            m.insert(
                "sensitivityMin".into(),
                opt_num(num(tree.attr("UISensitivityMin"))),
            );
            m.insert(
                "sensitivityMax".into(),
                opt_num(num(tree.attr("UISensitivityMax"))),
            );
            m.insert("groups".into(), Value::Array(groups));
            Value::Object(m)
        })
        .collect()
}

/// `parseDefaultProfile`: the dataset body of a parsed `defaultProfile.xml`.
pub fn parse_default_profile(root: &XmlNode, localization: &Localization) -> Map<String, Value> {
    let mut labels = LabelResolver::new(localization);

    let mut by_name = HashMap::new();
    let categories = child_named(root, "ActionCategories")
        .map(|c| parse_categories(c, &mut labels, &mut by_name))
        .unwrap_or_default();

    let mut actions: Vec<Action> = Vec::new();
    let actionmaps: Vec<Value> = children_named(root, "actionmap")
        .map(|m| parse_action_map(m, &mut labels, &by_name, &mut actions))
        .collect();

    let mut bound_by_device = Map::new();
    for (i, device) in DEVICES.iter().enumerate() {
        let n = actions.iter().filter(|a| a.bound[i]).count();
        bound_by_device.insert((*device).into(), n.into());
    }

    let option_trees = parse_option_trees(root, &mut labels);
    let mut body = Map::new();
    body.insert(
        "activationModes".into(),
        Value::Array(parse_activation_modes(root)),
    );
    body.insert("modifiers".into(), json!(parse_modifiers(root)));
    body.insert(
        "categories".into(),
        Value::Array(categories.into_iter().map(|c| c.value).collect()),
    );
    body.insert("optionTrees".into(), Value::Array(option_trees));
    let summary = json!({
        "actionmaps": actionmaps.len(),
        "actions": actions.len(),
        "boundByDevice": bound_by_device,
        "unlabelledActions": actions.iter().filter(|a| !a.labelled).count(),
        "unresolvedLabels": labels.unresolved.len(),
        "unresolvedLabelKeys": labels.unresolved.iter().collect::<Vec<_>>(),
    });
    body.insert("actionmaps".into(), Value::Array(actionmaps));
    body.insert("summary".into(), summary);
    body
}

fn empty_body() -> Map<String, Value> {
    let mut body = Map::new();
    for k in [
        "activationModes",
        "modifiers",
        "categories",
        "optionTrees",
        "actionmaps",
    ] {
        body.insert(k.into(), Value::Array(Vec::new()));
    }
    body
}

/// The warning the TypeScript pipeline gives a domain whose archive files are missing.
pub(crate) fn missing_files_warning(ctx: &ParseContext<'_>, name: &str, globs: &[&str]) {
    ctx.warn(format!(
        "{name}: none of its archive files were found ({}). Run extract first.",
        globs.join(", ")
    ));
}

impl Domain for Keybinds {
    fn name(&self) -> &'static str {
        "game-keybinds"
    }

    fn description(&self) -> &'static str {
        "Default control bindings per action map and device, from defaultProfile.xml."
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
            path_ends_with(p, "defaultprofile.xml")
        }) {
            None => {
                missing_files_warning(ctx, self.name(), GLOBS);
                Ok(empty_body())
            }
            Some(Err(e)) => Err(DomainError::Message(e)),
            Some(Ok(root)) => Ok(parse_default_profile(&root, ctx.localization)),
        }
    }
}

#[cfg(test)]
mod tests {
    //! Cases from `src/domains/keybinds.test.ts`.
    use super::*;
    use crate::fixtures::MemFiles;
    use crate::localization::parse_global_ini;
    use crate::records::RecordSet;
    use crate::xml::parse_xml;

    fn localization() -> Localization {
        parse_global_ini(
            &[
                "ui_CGSpaceFlightMovement=Flight - Movement",
                "ui_CCSpaceFlight=FLIGHT",
                "UI_CIPITCH=Pitch",
                "ui_CIEject,P=Eject",
                "ui_CGHacking=Hacking",
                "PIT_ACShipSystems=Ship Systems",
                "ui_DoorsOpen=Doors Open",
            ]
            .join("\n"),
        )
    }

    const PROFILE: &str = r#"<profile version="1">
  <ActivationModes>
    <ActivationMode name="tap" onPress="0" onHold="0" onRelease="1" multiTap="1" multiTapBlock="1" pressTriggerThreshold="-1" releaseTriggerThreshold="0.25" releaseTriggerDelay="0" retriggerable="0" />
  </ActivationModes>
  <modifiers>
    <mod input="LShift" />
    <mod input="kb_lalt" />
  </modifiers>
  <ActionCategories>
    <Category name="ShipSystems" action="pc_pit_ship_systems" UILabel="@PIT_ACShipSystems">
      <Category name="Nested" UILabel="@PIT_Missing" />
    </Category>
  </ActionCategories>
  <actionmap name="spaceship_movement" version="18" UILabel="@ui_CGSpaceFlightMovement" UICategory="@ui_CCSpaceFlight">
    <action name="v_pitch" activationMode="press" keyboard=" w " gamepad="thumbry" joystick=" " UILabel="@ui_CIPitch" UIDescription="@ui_CIPitchDesc" optionGroup="flight_move_pitch" holdTriggerDelay="0.25" analogCompareOp="GREATERTHAN" />
    <action name="v_eject" ActivationMode="tap" keyboard="ralt+y" UILabel="@ui_CIEject" Category="ShipSystems">
      <gamepad input=" " activationMode="delayed_press_medium" onHold="1" />
    </action>
    <action name="flashui_return" activationmode="hold" onPress="1" gamepad="a">
      <keyboard>
        <inputdata input="enter" />
        <inputdata input="np_enter" />
      </keyboard>
    </action>
    <action name="v_doors" keyboard="" UILabel="@ui_MissingKey">
      <states>
        <state name="opened" UILabel="@ui_DoorsOpen" />
      </states>
    </action>
  </actionmap>
  <actionmap name="hacking" version="1" UILable="@ui_CGHacking" UICategory="" />
</profile>"#;

    fn body() -> Map<String, Value> {
        parse_default_profile(&parse_xml(PROFILE).unwrap(), &localization())
    }

    fn action(name: &str) -> Value {
        body()["actionmaps"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|m| m["actions"].as_array().unwrap().clone())
            .find(|a| a["name"] == name)
            .unwrap_or_else(|| panic!("no action {name}"))
    }

    /// `toMatchObject`: every field of `want` equals the one in `got`.
    fn matches(got: &Value, want: Value) {
        for (k, v) in want.as_object().unwrap() {
            assert_eq!(&got[k], v, "{k} in {got}");
        }
    }

    #[test]
    fn reads_activation_modes_as_numbers() {
        assert_eq!(
            normalise(&body()["activationModes"]),
            normalise(&json!([{
                "name": "tap", "onPress": 0, "onHold": 0, "onRelease": 1, "multiTap": 1,
                "multiTapBlock": 1, "pressTriggerThreshold": -1, "releaseTriggerThreshold": 0.25,
                "releaseTriggerDelay": 0, "retriggerable": 0,
            }]))
        );
    }

    /// Numbers compared as f64 (the parser writes every number as a double).
    fn normalise(v: &Value) -> Value {
        match v {
            Value::Number(n) => number(n.as_f64().unwrap()),
            Value::Array(a) => Value::Array(a.iter().map(normalise).collect()),
            Value::Object(o) => {
                Value::Object(o.iter().map(|(k, v)| (k.clone(), normalise(v))).collect())
            }
            other => other.clone(),
        }
    }

    #[test]
    fn normalises_modifiers_and_resolves_nested_categories() {
        let b = body();
        assert_eq!(b["modifiers"], json!(["lshift", "kb_lalt"]));
        matches(
            &b["categories"][0],
            json!({"name": "ShipSystems", "label": "Ship Systems"}),
        );
        matches(
            &b["categories"][0]["children"][0],
            json!({"name": "Nested", "label": "@PIT_Missing"}),
        );
    }

    #[test]
    fn keeps_map_order_and_resolves_map_labels_including_the_uilable_typo() {
        let b = body();
        let maps = b["actionmaps"].as_array().unwrap();
        let names: Vec<&str> = maps.iter().map(|m| m["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["spaceship_movement", "hacking"]);
        assert_eq!(maps[0]["version"].as_f64(), Some(18.0));
        matches(
            &maps[0],
            json!({
                "label": "Flight - Movement",
                "labelKey": "@ui_CGSpaceFlightMovement",
                "category": "FLIGHT",
                "categoryKey": "@ui_CCSpaceFlight",
            }),
        );
        matches(
            &maps[1],
            json!({"label": "Hacking", "labelKey": "@ui_CGHacking", "category": null}),
        );
        let actions: Vec<&str> = maps[0]["actions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["name"].as_str().unwrap())
            .collect();
        assert_eq!(actions, ["v_pitch", "v_eject", "flashui_return", "v_doors"]);
    }

    #[test]
    fn separates_bound_explicitly_unbound_and_not_applicable_devices() {
        assert_eq!(
            action("v_pitch")["defaults"],
            json!({"keyboard": ["w"], "mouse": null, "gamepad": ["thumbry"], "joystick": []})
        );
        assert_eq!(action("v_doors")["defaults"]["keyboard"], json!([]));
    }

    #[test]
    fn prefers_device_child_elements_over_attributes() {
        let eject = action("v_eject");
        assert_eq!(eject["defaults"]["gamepad"], json!([]));
        assert_eq!(
            eject["perDeviceActivation"],
            json!({"gamepad": "delayed_press_medium"})
        );
        assert_eq!(
            normalise(&eject["perDeviceFlags"]),
            normalise(&json!({"gamepad": {"onHold": 1}}))
        );

        let ret = action("flashui_return");
        assert_eq!(ret["defaults"]["keyboard"], json!(["enter", "np_enter"]));
        assert_eq!(ret["defaults"]["gamepad"], json!(["a"]));
        assert!(ret.get("perDeviceActivation").is_none());
    }

    #[test]
    fn reads_the_activation_mode_whatever_its_casing() {
        assert_eq!(action("v_pitch")["activationMode"], "press");
        assert_eq!(action("v_eject")["activationMode"], "tap");
        assert_eq!(action("flashui_return")["activationMode"], "hold");
        assert_eq!(action("v_doors")["activationMode"], Value::Null);
    }

    #[test]
    fn resolves_labels_case_insensitively_and_through_the_p_key_suffix() {
        matches(
            &action("v_pitch"),
            json!({"label": "Pitch", "labelKey": "@ui_CIPitch"}),
        );
        matches(
            &action("v_eject"),
            json!({"label": "Eject", "category": "Ship Systems", "categoryKey": "ShipSystems"}),
        );
    }

    #[test]
    fn falls_back_to_the_raw_key_or_the_action_name_and_counts_the_misses() {
        assert_eq!(action("v_doors")["label"], "@ui_MissingKey");
        matches(
            &action("flashui_return"),
            json!({"label": "flashui_return", "labelKey": null}),
        );
        matches(
            &body()["summary"],
            json!({
                "actionmaps": 2,
                "actions": 4,
                "unlabelledActions": 1,
                "unresolvedLabels": 2,
                "unresolvedLabelKeys": ["@PIT_Missing", "@ui_MissingKey"],
            }),
        );
    }

    #[test]
    fn keeps_description_keys_but_never_description_text() {
        assert_eq!(action("v_pitch")["descriptionKey"], "@ui_CIPitchDesc");
        assert!(
            !Value::Object(body())
                .to_string()
                .contains("\"description\"")
        );
    }

    #[test]
    fn copies_only_the_flags_an_action_sets() {
        assert_eq!(
            normalise(&action("v_pitch")["flags"]),
            normalise(&json!({"holdTriggerDelay": 0.25, "analogCompareOp": "GREATERTHAN"}))
        );
        assert_eq!(action("v_pitch")["optionGroup"], "flight_move_pitch");
        assert_eq!(
            normalise(&action("flashui_return")["flags"]),
            normalise(&json!({"onPress": 1}))
        );
    }

    #[test]
    fn parses_states_with_resolved_labels() {
        assert_eq!(
            action("v_doors")["states"],
            json!([{"name": "opened", "label": "Doors Open", "labelKey": "@ui_DoorsOpen"}])
        );
    }

    #[test]
    fn option_trees_flatten_groups_in_tree_order_with_parents_flags_defaults_and_curve() {
        const TREE: &str = r#"<profile>
    <optiontree type="joystick" instances="8" name="root" UISensitivityMin="0.01" UISensitivityMax="2.0">
      <optiongroup name="master" UILabel="@ui_CGSpaceFlightMovement" UIShowCurve="0" UIShowInvert="0">
        <optiongroup name="flight_view" exponent="2.5" UILabel="@missing" UIShowCurve="1" UIShowInvert="-1">
          <nonlinearity_curve reset="1"><point in="0.5" out="0.25"/></nonlinearity_curve>
          <optiongroup name="flight_move_pitch" UILabel="@ui_CIPitch" UIShowCurve="1" UIShowInvert="1" invert="1"/>
        </optiongroup>
      </optiongroup>
    </optiontree>
  </profile>"#;
        let b = parse_default_profile(&parse_xml(TREE).unwrap(), &localization());
        let tree = normalise(&b["optionTrees"][0]);
        matches(
            &tree,
            normalise(
                &json!({"type": "joystick", "instances": 8, "sensitivityMin": 0.01, "sensitivityMax": 2}),
            ),
        );
        let groups = tree["groups"].as_array().unwrap();
        let pairs: Vec<(Value, Value)> = groups
            .iter()
            .map(|g| (g["name"].clone(), g["parent"].clone()))
            .collect();
        assert_eq!(
            pairs,
            vec![
                (json!("master"), Value::Null),
                (json!("flight_view"), json!("master")),
                (json!("flight_move_pitch"), json!("flight_view")),
            ]
        );
        matches(
            &groups[1],
            normalise(
                &json!({"exponent": 2.5, "curve": [{"in": 0.5, "out": 0.25}], "showInvert": -1}),
            ),
        );
        matches(
            &groups[2],
            normalise(
                &json!({"label": "Pitch", "invert": 1, "showInvert": 1, "showSensitivity": null, "curve": null}),
            ),
        );
    }

    fn run(files: &MemFiles) -> Map<String, Value> {
        let records = RecordSet::new(Vec::new());
        let loc = localization();
        let ctx = ParseContext::new(&records, &loc, files);
        DOMAIN.parse(&ctx).unwrap()
    }

    #[test]
    fn domain_finds_default_profile_among_the_archive_files() {
        let mut files = MemFiles::default();
        files.insert(r"Data\Libs\Config\Other.xml", b"<x/>".to_vec());
        // As shipped: CryXmlB.
        let bin = crate::cryxml::tests::cryxml(&parse_xml(PROFILE).unwrap());
        files.insert(r"Data\Libs\Config\defaultProfile.xml", bin);
        let body = run(&files);
        assert_eq!(body["actionmaps"].as_array().unwrap().len(), 2);
        assert_eq!(body, self::body());
    }

    #[test]
    fn domain_yields_empty_collections_when_the_file_is_missing() {
        let body = run(&MemFiles::default());
        assert_eq!(body["actionmaps"], json!([]));
        assert!(body.get("summary").is_none());
    }

    #[test]
    fn publishing_keeps_labels_and_option_trees() {
        // Bindscape and sc-telemetry read these; publish redaction must leave the file be.
        let dump = Value::Object(body());
        assert_eq!(
            crate::localization::redaction::redact_dump("game-keybinds.json", &dump),
            dump
        );
    }

    #[test]
    fn declares_the_archive_file_and_no_records() {
        assert_eq!(DOMAIN.files(), ["**/Libs/Config/defaultProfile.xml"]);
        assert!(DOMAIN.filters().is_empty());
    }
}
