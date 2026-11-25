//! Helpers for `EntityClassDefinition` records (port of `sc-gamedata/src/entity.ts`).
//!
//! An entity is a bag of typed component structs in a `Components` array.

use crate::records::GameRecord;
use serde_json::{Map, Value};

/// Cut an entity record value down to `_Type_`, the `top` keys, and the `Components` of the
/// listed types (in their order), each with `_Type_` and the listed fields, or whole when
/// the field list is `None`. For [`crate::domain::Domain::prune`].
pub fn prune_entity(value: &mut Value, top: &[&str], keep: &[(&str, Option<&[&str]>)]) {
    let Some(obj) = value.as_object_mut() else {
        return;
    };
    obj.retain(|k, _| k == "_Type_" || k == "Components" || top.contains(&k.as_str()));
    if let Some(Value::Array(list)) = obj.get_mut("Components") {
        list.retain_mut(|c| {
            let Some(c) = c.as_object_mut() else {
                return false;
            };
            let ty = c.get("_Type_").and_then(Value::as_str).unwrap_or_default();
            match keep.iter().find(|(t, _)| *t == ty) {
                None => false,
                Some((_, None)) => true,
                Some((_, Some(fields))) => {
                    c.retain(|k, _| k == "_Type_" || fields.contains(&k.as_str()));
                    true
                }
            }
        });
    }
}

/// The entity's components that are objects.
pub fn components(record: &GameRecord) -> impl Iterator<Item = &Map<String, Value>> {
    record
        .value
        .get("Components")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
}

fn type_of(c: &Map<String, Value>) -> Option<&str> {
    c.get("_Type_").and_then(Value::as_str)
}

/// The first component of the given type.
pub fn component<'a>(record: &'a GameRecord, ty: &str) -> Option<&'a Map<String, Value>> {
    components(record).find(|c| type_of(c) == Some(ty))
}

/// Every component whose type contains `fragment`.
pub fn components_like<'a>(
    record: &'a GameRecord,
    fragment: &'a str,
) -> impl Iterator<Item = &'a Map<String, Value>> + 'a {
    components(record).filter(move |c| type_of(c).is_some_and(|t| t.contains(fragment)))
}

pub fn has_component(record: &GameRecord, ty: &str) -> bool {
    component(record, ty).is_some()
}

/// A Vec2/Vec3 struct's numeric fields, without `_Type_`, in field order. `None` when it
/// has none.
pub fn vec(value: Option<&Value>) -> Option<Map<String, Value>> {
    let obj = value?.as_object()?;
    let out: Map<String, Value> = obj
        .iter()
        .filter(|(k, v)| k.as_str() != "_Type_" && v.is_number())
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    (!out.is_empty()).then_some(out)
}

/// The record name a `file://...json` reference points at: the file stem, and for a
/// `scitemmanufacturer.aegs.json` double name its last part (`aegs`).
pub fn referenced_name(value: Option<&Value>) -> Option<&str> {
    let s = value?.as_str().filter(|s| !s.is_empty())?;
    let file = s.rsplit('/').next()?;
    // `.replace(/\.json$/i, "")`
    let cut = file.len().saturating_sub(5);
    let stem = match file.get(cut..) {
        Some(ext) if file.len() >= 5 && ext.eq_ignore_ascii_case(".json") => &file[..cut],
        _ => file,
    };
    stem.rsplit('.').next()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::records::normalize_record;
    use serde_json::json;

    #[test]
    fn prune_keeps_listed_components_and_fields_in_order() {
        let mut v = json!({"_Type_": "EntityClassDefinition", "tags": [1], "StaticEntityClassData": [2],
            "Components": [
                {"_Type_": "A", "keep": 1, "drop": 2},
                {"_Type_": "B", "x": 1},
                {"_Type_": "C", "y": 1},
                {"_Type_": "A", "keep": 3},
                "not an object"]});
        prune_entity(&mut v, &["tags"], &[("A", Some(&["keep"])), ("C", None)]);
        assert_eq!(
            v,
            json!({"_Type_": "EntityClassDefinition", "tags": [1], "Components": [
                {"_Type_": "A", "keep": 1}, {"_Type_": "C", "y": 1}, {"_Type_": "A", "keep": 3}]})
        );
    }

    fn entity(components: Value) -> GameRecord {
        normalize_record(
            json!({"_RecordName_": "EntityClassDefinition.X", "_RecordValue_": {"_Type_": "EntityClassDefinition", "Components": components}}),
            "p",
        )
        .unwrap()
    }

    #[test]
    fn component_lookup() {
        let e = entity(json!([
            {"_Type_": "SAttachableComponentParams", "a": 1},
            null,
            {"_Type_": "SCItemWeaponComponentParams"},
            {"_Type_": "SAttachableComponentParams", "a": 2}
        ]));
        assert_eq!(components(&e).count(), 3);
        assert_eq!(component(&e, "SAttachableComponentParams").unwrap()["a"], 1);
        assert_eq!(components_like(&e, "Weapon").count(), 1);
        assert!(has_component(&e, "SCItemWeaponComponentParams"));
        assert!(!has_component(&entity(json!(null)), "X"));
    }

    #[test]
    fn vec_and_names() {
        let v = vec(Some(
            &json!({"_Type_": "Vec3", "x": 1, "y": 2.5, "z": "no"}),
        ))
        .unwrap();
        assert_eq!(Value::Object(v), json!({"x": 1, "y": 2.5}));
        assert!(vec(Some(&json!({"_Type_": "Vec3"}))).is_none());
        let r = json!("file://./../x/scitemmanufacturer.aegs.json");
        assert_eq!(referenced_name(Some(&r)), Some("aegs"));
        assert_eq!(referenced_name(Some(&json!("a/B.JSON"))), Some("B"));
        assert_eq!(referenced_name(Some(&json!(""))), None);
    }
}
