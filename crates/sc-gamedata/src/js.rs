//! JavaScript number semantics the ported domains share (`Math.round`, the
//! `Math.round(x * f) / f` idiom, number truthiness, `JSON.stringify` of a number).
//!
//! The per-batch helper modules (`helpers_items`, `helpers_ships`, `helpers_world`)
//! re-export these, so call sites keep their batch paths.

use serde_json::Value;

/// `Math.round`: nearest integer, ties toward +infinity (`Math.round(-2.5) == -2`), unlike
/// [`f64::round`]. Non-finite values pass through.
pub fn js_round(x: f64) -> f64 {
    if !x.is_finite() {
        return x;
    }
    let f = x.floor();
    // `x - floor(x)` is exact for every double.
    if x - f >= 0.5 { f + 1.0 } else { f }
}

/// `Math.round(x * factor) / factor`, the TypeScript's fixed-decimals idiom.
pub fn round_to(x: f64, factor: f64) -> f64 {
    js_round(x * factor) / factor
}

/// JavaScript truthiness of an optional number: defined, non-zero, not NaN.
pub fn truthy(x: Option<f64>) -> bool {
    x.is_some_and(|v| v != 0.0 && !v.is_nan())
}

/// A number as a JSON value (`null` for a non-finite one, as `JSON.stringify` writes it).
pub fn number(x: f64) -> Value {
    serde_json::Number::from_f64(x).map_or(Value::Null, Value::Number)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn math_round_semantics() {
        assert_eq!(js_round(2.5), 3.0);
        assert_eq!(js_round(-2.5), -2.0);
        assert_eq!(js_round(-2.6), -3.0);
        assert_eq!(js_round(0.499_999_999_999_999_94), 0.0);
        assert!(js_round(f64::NAN).is_nan());
        assert_eq!(js_round(f64::INFINITY), f64::INFINITY);
        assert_eq!(round_to(1.005, 100.0), 1.0);
        assert!(
            truthy(Some(1.0)) && !truthy(Some(0.0)) && !truthy(Some(f64::NAN)) && !truthy(None)
        );
        assert_eq!(number(f64::NAN), Value::Null);
    }
}
