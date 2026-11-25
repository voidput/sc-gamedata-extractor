//! Diff of two datasets, for checking a port against `sc-gamedata`'s golden dumps.
//!
//! [`compare_text`] is a byte comparison: the two files are identical only when their text
//! is the same after removing the one top-level `_extracted` line (the timestamp). When the
//! text differs, [`diff`] says where, as paths:
//!
//! - `_extracted` at the top level is ignored;
//! - numbers are compared exactly, as parsed (`1` and `1.0` differ, so do integers one
//!   apart at any magnitude);
//! - arrays of objects are matched by a stable id field when one identifies every element
//!   on both sides ([`ID_FIELDS`], first that works), else by position; an order change is
//!   one difference of its own;
//! - object key order is compared too (one difference per object), since the goal is the
//!   same file, not just the same data.

use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};

/// Fields tried, in order, to match array elements.
pub const ID_FIELDS: &[&str] = &[
    "id",
    "recordName",
    "guid",
    "key",
    "code",
    "className",
    "ref",
    "name",
];

/// Top-level keys never compared.
pub const IGNORED_TOP_LEVEL: &[&str] = &["_extracted"];

/// Differences found, as `path: what` lines, in document order.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Diff {
    pub paths: Vec<String>,
}

impl Diff {
    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }
}

/// Compare `ours` with `golden`.
pub fn diff(ours: &Value, golden: &Value) -> Diff {
    let mut out = Diff::default();
    match (ours, golden) {
        (Value::Object(a), Value::Object(b)) => {
            diff_object(&mut out, "$", a, b, IGNORED_TOP_LEVEL);
        }
        _ => walk(&mut out, "$".to_string(), ours, golden),
    }
    out
}

fn short(v: &Value) -> String {
    let s = v.to_string();
    if s.chars().count() > 80 {
        format!("{}...", s.chars().take(77).collect::<String>())
    } else {
        s
    }
}

fn walk(out: &mut Diff, path: String, a: &Value, b: &Value) {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            if x != y {
                out.paths.push(format!("{path}: ours {x} vs golden {y}"));
            }
        }
        (Value::Object(x), Value::Object(y)) => diff_object(out, &path, x, y, &[]),
        (Value::Array(x), Value::Array(y)) => diff_array(out, &path, x, y),
        _ if a == b => {}
        _ => out
            .paths
            .push(format!("{path}: ours {} vs golden {}", short(a), short(b))),
    }
}

fn diff_object(
    out: &mut Diff,
    path: &str,
    a: &Map<String, Value>,
    b: &Map<String, Value>,
    ignore: &[&str],
) {
    let keep = |k: &&String| !ignore.contains(&k.as_str());
    for (k, bv) in b.iter().filter(|(k, _)| keep(k)) {
        let p = format!("{path}.{k}");
        match a.get(k) {
            Some(av) => walk(out, p, av, bv),
            None => out
                .paths
                .push(format!("{p}: missing in ours (golden {})", short(bv))),
        }
    }
    for (k, av) in a.iter().filter(|(k, _)| keep(k)) {
        if !b.contains_key(k) {
            out.paths
                .push(format!("{path}.{k}: extra in ours ({})", short(av)));
        }
    }
    let common_a: Vec<&String> = a
        .keys()
        .filter(keep)
        .filter(|k| b.contains_key(*k))
        .collect();
    let common_b: Vec<&String> = b
        .keys()
        .filter(keep)
        .filter(|k| a.contains_key(*k))
        .collect();
    if common_a != common_b {
        out.paths.push(format!(
            "{path}: key order differs (ours {:?}, golden {:?})",
            common_a.iter().take(6).collect::<Vec<_>>(),
            common_b.iter().take(6).collect::<Vec<_>>()
        ));
    }
}

/// The id of an element under `field`, as text.
fn id_of(v: &Value, field: &str) -> Option<String> {
    match v.as_object()?.get(field)? {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// The first field that names every element of both arrays uniquely.
fn id_field(a: &[Value], b: &[Value]) -> Option<&'static str> {
    if a.is_empty() && b.is_empty() {
        return None;
    }
    ID_FIELDS.iter().copied().find(|f| {
        [a, b].iter().all(|side| {
            let mut seen = HashSet::new();
            side.iter()
                .all(|v| id_of(v, f).is_some_and(|id| seen.insert(id)))
        })
    })
}

fn diff_array(out: &mut Diff, path: &str, a: &[Value], b: &[Value]) {
    let Some(field) = id_field(a, b) else {
        for (i, (x, y)) in a.iter().zip(b).enumerate() {
            walk(out, format!("{path}[{i}]"), x, y);
        }
        if a.len() != b.len() {
            out.paths.push(format!(
                "{path}: length ours {} vs golden {}",
                a.len(),
                b.len()
            ));
        }
        return;
    };
    let index_a: HashMap<String, usize> = a
        .iter()
        .enumerate()
        .filter_map(|(i, v)| Some((id_of(v, field)?, i)))
        .collect();
    let ids_b: Vec<String> = b.iter().filter_map(|v| id_of(v, field)).collect();
    let set_b: HashSet<&String> = ids_b.iter().collect();
    for (y, id) in b.iter().zip(&ids_b) {
        let p = format!("{path}[{field}={id}]");
        match index_a.get(id) {
            Some(&i) => walk(out, p, &a[i], y),
            None => out.paths.push(format!("{p}: missing in ours")),
        }
    }
    let mut order_a = Vec::new();
    for v in a {
        let id = id_of(v, field).unwrap_or_default();
        if set_b.contains(&id) {
            order_a.push(id);
        } else {
            out.paths
                .push(format!("{path}[{field}={id}]: extra in ours"));
        }
    }
    let order_b: Vec<&String> = ids_b
        .iter()
        .filter(|id| index_a.contains_key(*id))
        .collect();
    if let Some(i) = order_a.iter().zip(&order_b).position(|(x, y)| x != *y) {
        out.paths.push(format!(
            "{path}: order differs by {field}, first at common position {i} (ours {}, golden {})",
            order_a[i], order_b[i]
        ));
    }
}

/// Per-dataset outcome, printed by the harness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Identical,
    Differs(Diff),
    MissingGolden,
    /// The golden directory has the dataset and we did not produce it.
    MissingOurs,
    Unreadable(String),
}

impl Outcome {
    pub fn is_ok(&self) -> bool {
        matches!(self, Outcome::Identical)
    }

    /// Report lines: `identical`, or `N differing paths` and the first `examples`.
    pub fn report(&self, name: &str, examples: usize) -> String {
        match self {
            Outcome::Identical => format!("{name}: identical"),
            Outcome::MissingGolden => format!("{name}: no golden file to compare with"),
            Outcome::MissingOurs => format!("{name}: in the golden directory, not produced"),
            Outcome::Unreadable(e) => format!("{name}: cannot compare: {e}"),
            Outcome::Differs(d) => {
                let mut s = format!("{name}: {} differing paths", d.paths.len());
                for p in d.paths.iter().take(examples) {
                    s.push_str("\n  ");
                    s.push_str(p);
                }
                if d.paths.len() > examples {
                    s.push_str(&format!("\n  ... and {} more", d.paths.len() - examples));
                }
                s
            }
        }
    }
}

/// The text without its first top-level `"_extracted": ...` line (two-space indent, as
/// `JSON.stringify(_, null, 2)` writes the header).
pub fn strip_extracted(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut removed = false;
    for line in text.split_inclusive('\n') {
        if !removed && line.starts_with("  \"_extracted\": ") {
            removed = true;
            continue;
        }
        out.push_str(line);
    }
    out
}

/// The first line where two texts differ, as a report path.
fn first_text_difference(a: &str, b: &str) -> String {
    let (mut la, mut lb) = (a.lines(), b.lines());
    let mut n = 1;
    loop {
        match (la.next(), lb.next()) {
            (Some(x), Some(y)) if x == y => n += 1,
            (None, None) => {
                return "text differs only in line endings or a final newline".to_string();
            }
            (x, y) => {
                let cut = |s: Option<&str>| {
                    s.map(|s| short(&Value::String(s.to_string())))
                        .unwrap_or_else(|| "<end of file>".to_string())
                };
                return format!(
                    "line {n}: text differs with equal values (formatting, escapes or duplicate keys): ours {} vs golden {}",
                    cut(x),
                    cut(y)
                );
            }
        }
    }
}

/// Compare two files' text: identical when the bytes match apart from the top-level
/// `_extracted` line; otherwise the differing paths, or the first differing line when the
/// values are equal and only the text is not.
pub fn compare_text(ours: &str, golden: &str) -> Outcome {
    let (a_text, b_text) = (strip_extracted(ours), strip_extracted(golden));
    if a_text == b_text {
        return Outcome::Identical;
    }
    let parse = |s: &str| serde_json::from_str::<Value>(s);
    match (parse(ours), parse(golden)) {
        (Ok(a), Ok(b)) => {
            let mut d = diff(&a, &b);
            if d.is_empty() {
                d.paths.push(first_text_difference(&a_text, &b_text));
            }
            Outcome::Differs(d)
        }
        (Err(e), _) => Outcome::Unreadable(format!("ours: {e}")),
        (_, Err(e)) => Outcome::Unreadable(format!("golden: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn numbers_compare_exactly() {
        let a = json!({"_extracted": "x", "n": 0, "price": 2_500_001, "big": 9_007_199_254_740_993_u64});
        let b = json!({"_extracted": "y", "n": 0, "price": 2_500_000, "big": 9_007_199_254_740_992_u64});
        assert_eq!(
            diff(&a, &b).paths,
            vec![
                "$.price: ours 2500001 vs golden 2500000",
                "$.big: ours 9007199254740993 vs golden 9007199254740992",
            ]
        );
        let f = json!({"v": 0.3333334, "w": 1.0});
        let g = json!({"v": 0.3333331, "w": 1});
        assert_eq!(
            diff(&f, &g).paths,
            vec![
                "$.v: ours 0.3333334 vs golden 0.3333331",
                "$.w: ours 1.0 vs golden 1"
            ]
        );
    }

    #[test]
    fn text_identical_apart_from_extracted_line() {
        let a = "{\n  \"_source\": \"s\",\n  \"_extracted\": \"2026-01-01\",\n  \"v\": 1\n}\n";
        let b = "{\n  \"_source\": \"s\",\n  \"_extracted\": \"2026-02-02\",\n  \"v\": 1\n}\n";
        assert!(compare_text(a, b).is_ok());
        // A nested `_extracted` line is not the header and stays.
        assert_eq!(
            strip_extracted("{\n    \"_extracted\": 1\n}"),
            "{\n    \"_extracted\": 1\n}"
        );
    }

    #[test]
    fn equal_values_with_different_text_differ() {
        // Number formatting, escapes and duplicate keys parse to equal values.
        for (ours, golden) in [
            ("{\n  \"v\": 1.0\n}", "{\n  \"v\": 1\n}"),
            (
                "{\n  \"v\": 1e21\n}",
                "{\n  \"v\": 1000000000000000000000\n}",
            ),
            ("{\n  \"v\": \"\\u00e9\"\n}", "{\n  \"v\": \"\u{e9}\"\n}"),
            ("{\n  \"v\": 2,\n  \"v\": 1\n}", "{\n  \"v\": 1\n}"),
        ] {
            let Outcome::Differs(d) = compare_text(ours, golden) else {
                panic!("{ours:?} vs {golden:?} reported identical");
            };
            assert_eq!(d.paths.len(), 1, "{:?}", d.paths);
        }
        let Outcome::Differs(d) = compare_text("{\n  \"v\": 1.0\n}", "{\n  \"v\": 1\n}") else {
            unreachable!()
        };
        assert!(
            d.paths[0].starts_with("$.v: ours 1.0 vs golden 1"),
            "{:?}",
            d.paths
        );
        let Outcome::Differs(d) = compare_text("{\n  \"v\": 2,\n  \"v\": 1\n}", "{\n  \"v\": 1\n}")
        else {
            unreachable!()
        };
        assert!(
            d.paths[0].starts_with("line 2: text differs"),
            "{:?}",
            d.paths
        );
    }

    #[test]
    fn nested_extracted_is_compared() {
        let a = json!({"x": {"_extracted": 1}});
        let b = json!({"x": {"_extracted": 2}});
        assert_eq!(diff(&a, &b).paths.len(), 1);
    }

    #[test]
    fn arrays_match_by_id() {
        let a = json!({"rows": [{"code": "B", "v": 2}, {"code": "A", "v": 1}, {"code": "X"}]});
        let b = json!({"rows": [{"code": "A", "v": 1}, {"code": "B", "v": 3}, {"code": "C"}]});
        assert_eq!(
            diff(&a, &b).paths,
            vec![
                "$.rows[code=B].v: ours 2 vs golden 3",
                "$.rows[code=C]: missing in ours",
                "$.rows[code=X]: extra in ours",
                "$.rows: order differs by code, first at common position 0 (ours B, golden A)",
            ]
        );
    }

    #[test]
    fn id_must_be_unique_else_positional() {
        // "name" repeats, so rows are matched by position.
        let a = json!([{"name": "x", "v": 1}, {"name": "x", "v": 2}]);
        let b = json!([{"name": "x", "v": 1}, {"name": "x", "v": 3}, 4]);
        assert_eq!(
            diff(&a, &b).paths,
            vec!["$[1].v: ours 2 vs golden 3", "$: length ours 2 vs golden 3"]
        );
    }

    #[test]
    fn keys_missing_extra_and_order() {
        let a = json!({"b": 1, "a": 2, "z": 0});
        let b = json!({"a": 2, "b": 1, "y": true});
        let d = diff(&a, &b).paths;
        assert_eq!(d[0], "$.y: missing in ours (golden true)");
        assert_eq!(d[1], "$.z: extra in ours (0)");
        assert!(d[2].starts_with("$: key order differs"));
    }

    #[test]
    fn report_lists_first_examples() {
        let o = compare_text(r#"{"a":[1,2,3]}"#, r#"{"a":[4,5,6]}"#);
        let r = o.report("game-x", 2);
        assert!(r.starts_with("game-x: 3 differing paths"));
        assert!(r.ends_with("... and 1 more"));
        assert!(compare_text("{}", "{}").is_ok());
        assert!(matches!(compare_text("{", "{}"), Outcome::Unreadable(_)));
    }
}
