//! Writing datasets exactly as `sc-gamedata` does: `JSON.stringify(body, null, 2)` with the
//! `_source` / `_extracted` / `_build` header first, no trailing newline.
//!
//! JavaScript differs from `serde_json` in three ways that matter for a byte-equal file, all
//! handled here rather than in each domain:
//!
//! - numbers: one number type; `5.0` prints as `5`, `1e21` as `1e+21`, `-0` as `0`, NaN and
//!   infinities as `null`;
//! - key order: an object lists integer-like keys (`"987"`) first in ascending numeric
//!   order, then the rest in insertion order;
//! - `undefined` fields vanish: domains express that with
//!   `#[serde(skip_serializing_if = "Option::is_none")]`.

use serde_json::{Map, Value};
use std::time::{SystemTime, UNIX_EPOCH};

/// `_source` of every game-file dataset.
pub const SOURCE: &str = "Star Citizen game files via StarBreaker";

/// The header plus the body's own keys, in that order (`{_source, _extracted, ..._build,
/// ...body}`). A body key repeating a header key overwrites its value in place, as the
/// object spread does.
pub fn envelope(body: Map<String, Value>, extracted: &str, build: Option<&str>) -> Value {
    let mut out = Map::new();
    out.insert("_source".into(), Value::String(SOURCE.into()));
    out.insert("_extracted".into(), Value::String(extracted.into()));
    if let Some(b) = build {
        out.insert("_build".into(), Value::String(b.into()));
    }
    for (k, v) in body {
        out.insert(k, v);
    }
    Value::Object(out)
}

/// `JSON.stringify(value, null, 2)`.
pub fn to_js_json(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value, 0);
    out
}

/// `JSON.stringify(value)`: no whitespace, same numbers, key order and escapes.
pub fn to_js_json_compact(value: &Value) -> String {
    let mut out = String::new();
    write_compact(&mut out, value);
    out
}

fn write_compact(out: &mut String, value: &Value) {
    match value {
        Value::Array(a) => {
            out.push('[');
            for (i, v) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_compact(out, v);
            }
            out.push(']');
        }
        Value::Object(m) => {
            out.push('{');
            for (i, k) in js_key_order(m).into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_string(out, k);
                out.push(':');
                write_compact(out, &m[k]);
            }
            out.push('}');
        }
        scalar => write_value(out, scalar, 0),
    }
}

/// A canonical array index as JavaScript orders it: `0`, or no leading zero, below 2^32-1.
fn array_index(key: &str) -> Option<u32> {
    if key.is_empty() || key.len() > 10 || !key.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if key.len() > 1 && key.starts_with('0') {
        return None;
    }
    key.parse::<u64>()
        .ok()
        .filter(|&n| n < u64::from(u32::MAX))
        .map(|n| n as u32)
}

/// Object keys in JavaScript property order.
pub fn js_key_order(map: &Map<String, Value>) -> Vec<&String> {
    let mut ints: Vec<(u32, &String)> = Vec::new();
    let mut rest = Vec::new();
    for k in map.keys() {
        match array_index(k) {
            Some(i) => ints.push((i, k)),
            None => rest.push(k),
        }
    }
    ints.sort_by_key(|(i, _)| *i);
    ints.into_iter().map(|(_, k)| k).chain(rest).collect()
}

fn indent(out: &mut String, level: usize) {
    out.push('\n');
    for _ in 0..level {
        out.push_str("  ");
    }
}

fn write_value(out: &mut String, value: &Value, level: usize) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => out.push_str(&js_number_of(n)),
        Value::String(s) => write_string(out, s),
        Value::Array(a) => {
            if a.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push('[');
            for (i, v) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                indent(out, level + 1);
                write_value(out, v, level + 1);
            }
            indent(out, level);
            out.push(']');
        }
        Value::Object(m) => {
            if m.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push('{');
            for (i, k) in js_key_order(m).into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                indent(out, level + 1);
                write_string(out, k);
                out.push_str(": ");
                write_value(out, &m[k], level + 1);
            }
            indent(out, level);
            out.push('}');
        }
    }
}

/// `JSON.stringify` string escaping: `"`, `\`, the short escapes, other control characters
/// as `\u00xx` (lower-case hex). Everything else verbatim.
fn write_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

fn js_number_of(n: &serde_json::Number) -> String {
    if let Some(i) = n.as_i64() {
        // Integers above 2^53 lose precision in JavaScript; none occur in game data.
        return i.to_string();
    }
    if let Some(u) = n.as_u64() {
        return u.to_string();
    }
    js_number(n.as_f64().unwrap_or(f64::NAN))
}

/// `Number.prototype.toString()` for a double, as `JSON.stringify` prints it (non-finite
/// values print as `null`).
pub fn js_number(x: f64) -> String {
    if !x.is_finite() {
        return "null".into();
    }
    if x == 0.0 {
        return "0".into();
    }
    // Rust's `{:e}` gives the shortest round-trip digits, as ECMAScript requires.
    let sci = format!("{:e}", x.abs());
    let (mantissa, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let exp: i32 = exp.parse().unwrap_or(0);
    let k = digits.len() as i32;
    let n = exp + 1; // decimal point position
    let mut s = String::new();
    if x < 0.0 {
        s.push('-');
    }
    if k <= n && n <= 21 {
        s.push_str(&digits);
        s.extend(std::iter::repeat_n('0', (n - k) as usize));
    } else if 0 < n && n <= 21 {
        s.push_str(&digits[..n as usize]);
        s.push('.');
        s.push_str(&digits[n as usize..]);
    } else if -6 < n && n <= 0 {
        s.push_str("0.");
        s.extend(std::iter::repeat_n('0', (-n) as usize));
        s.push_str(&digits);
    } else {
        s.push_str(&digits[..1]);
        if k > 1 {
            s.push('.');
            s.push_str(&digits[1..]);
        }
        s.push('e');
        s.push(if n > 0 { '+' } else { '-' });
        s.push_str(&(n - 1).abs().to_string());
    }
    s
}

/// `new Date().toISOString()`: `2026-11-12T18:18:38.829Z`.
pub fn iso_now() -> String {
    let d = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    iso_from_millis(d.as_millis() as i64)
}

/// ISO-8601 UTC with milliseconds for a Unix time in milliseconds.
pub fn iso_from_millis(ms: i64) -> String {
    let days = ms.div_euclid(86_400_000);
    let rem = ms.rem_euclid(86_400_000);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        rem / 3_600_000,
        rem / 60_000 % 60,
        rem / 1000 % 60,
        rem % 1000
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn compact_matches_json_stringify() {
        let v: Value = serde_json::from_str(
            r#"{"b":[1,5.0,"x\n"],"10":null,"a":{"2":true,"1":-0.0},"e":1e21}"#,
        )
        .unwrap();
        assert_eq!(
            to_js_json_compact(&v),
            r#"{"10":null,"b":[1,5,"x\n"],"a":{"1":0,"2":true},"e":1e+21}"#
        );
    }

    #[test]
    fn numbers_print_like_javascript() {
        for (x, s) in [
            (5.0, "5"),
            (-0.0, "0"),
            (0.1, "0.1"),
            (0.1 + 0.2, "0.30000000000000004"),
            (1e21, "1e+21"),
            (1.5e21, "1.5e+21"),
            (123456789012345680000.0, "123456789012345680000"),
            (1e-7, "1e-7"),
            (1.25e-7, "1.25e-7"),
            (0.000001, "0.000001"),
            (-2.5, "-2.5"),
            (12000.0, "12000"),
            (f64::NAN, "null"),
            (f64::INFINITY, "null"),
            (f32::MAX as f64, "3.4028234663852886e+38"),
        ] {
            assert_eq!(js_number(x), s, "{x}");
        }
    }

    #[test]
    fn stringify_matches_javascript() {
        let v = json!({"b": 1.0, "987": "x", "a": [], "o": {}, "10": [1, 2.5, null], "01": "s\n\"\u{1}"});
        assert_eq!(
            to_js_json(&v),
            "{\n  \"10\": [\n    1,\n    2.5,\n    null\n  ],\n  \"987\": \"x\",\n  \"b\": 1,\n  \"a\": [],\n  \"o\": {},\n  \"01\": \"s\\n\\\"\\u0001\"\n}"
        );
    }

    #[test]
    fn header_comes_first() {
        let mut body = Map::new();
        body.insert("rows".into(), json!([]));
        let e = envelope(body, "2026-01-01T00:00:00.000Z", Some("4.10"));
        let keys: Vec<_> = e.as_object().unwrap().keys().cloned().collect();
        assert_eq!(keys, vec!["_source", "_extracted", "_build", "rows"]);
        let e = envelope(Map::new(), "t", None);
        assert!(e.get("_build").is_none());
    }

    #[test]
    fn iso_timestamps() {
        assert_eq!(iso_from_millis(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(
            iso_from_millis(1_794_507_518_829),
            "2026-11-12T18:18:38.829Z"
        );
        assert_eq!(iso_from_millis(951_782_400_000), "2000-02-29T00:00:00.000Z");
    }
}
