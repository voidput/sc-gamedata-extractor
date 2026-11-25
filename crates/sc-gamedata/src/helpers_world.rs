//! Small JavaScript-exact helpers shared by the world/economy domain ports (commodities,
//! refining, mining, mining spawns, shops, starmap). No regex crate: each helper spells out
//! the one TypeScript expression it replaces.

use serde_json::Value;

/// `String.prototype.trim` whitespace: WhiteSpace + LineTerminator (ECMA-262), which
/// differs from Rust's `char::is_whitespace` (JS adds U+FEFF, drops U+0085).
pub fn is_js_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r' | ' ' | '\u{A0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200A}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202F}'
                | '\u{205F}'
                | '\u{3000}'
                | '\u{FEFF}'
    )
}

/// `s.trim()`.
pub fn js_trim(s: &str) -> &str {
    s.trim_matches(is_js_space)
}

/// `s.replace(/<[^>]*>/g, "")`: drop every `<...>` run that has a closing `>`.
pub fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(open) = rest.find('<') {
        match rest[open..].find('>') {
            Some(close) => {
                out.push_str(&rest[..open]);
                rest = &rest[open + close + 1..];
            }
            None => break,
        }
    }
    out.push_str(rest);
    out
}

/// `s.replace(/\\n/g, "\n")`: the string table's literal backslash-n.
pub fn literal_newlines(s: &str) -> String {
    s.replace("\\n", "\n")
}

/// `s.split("/").pop()`: the last path segment (the whole string without a slash).
pub fn last_segment(s: &str) -> &str {
    s.rsplit('/').next().unwrap_or(s)
}

/// `typeof v === "object" && v !== null && !Array.isArray(v)`-ish truthy object test used
/// by `.filter((e) => !!e && typeof e === "object")`; arrays pass in JS too.
pub fn is_js_object(v: &Value) -> bool {
    v.is_object() || v.is_array()
}

/// JS truthiness of a JSON value.
pub fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|x| x != 0.0 && !x.is_nan()),
        Some(Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

/// `Number(s)` for a string: JS whitespace trimmed, empty is 0, `0x`/`0o`/`0b` integers,
/// `Infinity`, else a decimal literal; anything else is NaN.
pub fn js_to_number(s: &str) -> f64 {
    let t = js_trim(s);
    if t.is_empty() {
        return 0.0;
    }
    for (prefix, radix) in [
        ("0x", 16),
        ("0X", 16),
        ("0o", 8),
        ("0O", 8),
        ("0b", 2),
        ("0B", 2),
    ] {
        if let Some(digits) = t.strip_prefix(prefix) {
            if digits.is_empty() || !digits.chars().all(|c| c.is_digit(radix)) {
                return f64::NAN;
            }
            return digits.chars().fold(0.0, |acc, c| {
                acc * f64::from(radix) + f64::from(c.to_digit(radix).unwrap_or(0))
            });
        }
    }
    match t {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    if !t
        .chars()
        .all(|c| c.is_ascii_digit() || matches!(c, '+' | '-' | '.' | 'e' | 'E'))
    {
        return f64::NAN;
    }
    t.parse::<f64>().unwrap_or(f64::NAN)
}

pub use crate::js::js_round;

/// `Math.hypot` as V8 computes it: scaled by the largest magnitude, Kahan-summed.
pub fn js_hypot(values: &[f64]) -> f64 {
    let mut max = 0.0f64;
    for v in values {
        let a = v.abs();
        if a.is_infinite() {
            return f64::INFINITY;
        }
        if a.is_nan() {
            return f64::NAN;
        }
        if a > max {
            max = a;
        }
    }
    if max == 0.0 {
        return 0.0;
    }
    let mut sum = 0.0;
    let mut compensation = 0.0;
    for v in values {
        let n = v.abs() / max;
        let summand = n * n - compensation;
        let preliminary = sum + summand;
        compensation = (preliminary - sum) - summand;
        sum = preliminary;
    }
    sum.sqrt() * max
}

/// `[A-Za-z0-9_]`, JavaScript's `\w` without the `u` flag.
pub fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trim_tags_and_newlines() {
        assert_eq!(js_trim("\u{FEFF} a \u{A0}"), "a");
        assert_eq!(js_trim("\u{85}a"), "\u{85}a");
        assert_eq!(strip_tags(r#"CO<font size="%d">2</font>"#), "CO2");
        assert_eq!(strip_tags("a < b"), "a < b");
        assert_eq!(strip_tags("<b>x</b> <i"), "x <i");
        assert_eq!(literal_newlines(r"a\nb"), "a\nb");
        assert_eq!(last_segment("a/b/c"), "c");
        assert_eq!(last_segment("c"), "c");
        assert_eq!(js_to_number(""), 0.0);
        assert_eq!(js_to_number(" 12.5 "), 12.5);
        assert_eq!(js_to_number("0x10"), 16.0);
        assert_eq!(js_to_number("1e3"), 1000.0);
        assert!(js_to_number("abc").is_nan());
        assert!(js_to_number("inf").is_nan());
        assert_eq!(js_to_number("-Infinity"), f64::NEG_INFINITY);
        assert_eq!(js_round(2.5), 3.0);
        assert_eq!(js_round(-2.5), -2.0);
        assert_eq!(js_round(0.49999999999999994), 0.0);
        assert_eq!(js_hypot(&[3.0, 4.0, 0.0]), 5.0);
    }
}
