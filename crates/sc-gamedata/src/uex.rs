//! UEX Corp API: live, crowdsourced market data (port of `sc-gamedata/src/sources/uex.ts`).
//!
//! The complement to the game files: the shipped data says what a commodity *is*, UEX says
//! what it currently sells for and where. Most endpoints need a bearer token from
//! <https://uexcorp.space> (My Apps), read from `UEXTOKEN` unless given.
//!
//! The crate's library has no HTTP client of its own: [`uex_fetch`] takes the request as a
//! function ([`Http`]), as the TypeScript takes `fetchImpl`; the `sc-gamedata` binary
//! supplies one built on `ureq`.

use serde_json::{Map, Value};

pub const BASE_URL: &str = "https://api.uexcorp.space/2.0";

/// `MissingTokenError`'s message.
pub const MISSING_TOKEN: &str = "UEX datasets need an API token. Set UEXTOKEN in the environment, or pass --uex-token. Register an app at https://uexcorp.space to get one.";

/// What the caller's HTTP client returns: the status and the body text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
}

/// A GET of `url`, with `Authorization: Bearer <token>` when a token is given. `Err` is a
/// transport failure (no response at all).
pub type Http<'a> = dyn FnMut(&str, Option<&str>) -> Result<HttpResponse, String> + 'a;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UexOptions {
    /// `None` falls back to `UEXTOKEN`; `Some("")` means no token (and no fallback).
    pub token: Option<String>,
    pub base_url: Option<String>,
    /// The endpoint is public: a token is still sent when one is set, but its absence is
    /// not an error. A wrong token is (UEX denies it even there).
    pub token_optional: bool,
}

impl UexOptions {
    /// The same options for a public endpoint.
    pub fn public(&self) -> Self {
        Self {
            token_optional: true,
            ..self.clone()
        }
    }

    /// `options.token ?? process.env.UEXTOKEN`, empty meaning none.
    fn resolved_token(&self) -> Option<String> {
        self.token
            .clone()
            .or_else(|| std::env::var("UEXTOKEN").ok())
            .filter(|t| !t.is_empty())
    }
}

/// `uexFetch`: one endpoint's rows. UEX answers HTTP 200 with a `status` field even when a
/// request fails, so the body is checked as well as the status code. A single object comes
/// back as a one-row list; no data as an empty one.
pub fn uex_fetch(
    endpoint: &str,
    options: &UexOptions,
    http: &mut Http<'_>,
) -> Result<Vec<Value>, String> {
    let token = options.resolved_token();
    if token.is_none() && !options.token_optional {
        return Err(MISSING_TOKEN.into());
    }
    let url = format!(
        "{}/{endpoint}",
        options.base_url.as_deref().unwrap_or(BASE_URL)
    );
    let response = http(&url, token.as_deref())?;
    if !(200..300).contains(&response.status) {
        return Err(format!("UEX {endpoint} returned HTTP {}", response.status));
    }
    let body: Value = serde_json::from_str(&response.body)
        .map_err(|e| format!("UEX {endpoint} returned invalid JSON: {e}"))?;
    let text = |k: &str| {
        body.get(k)
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
    };
    if let Some(status) = text("status")
        && status != "ok"
    {
        return Err(format!(
            "UEX {endpoint} failed: {}",
            text("message").unwrap_or(status)
        ));
    }
    Ok(match body.get("data") {
        Some(Value::Array(rows)) => rows.clone(),
        Some(row @ Value::Object(_)) => vec![row.clone()],
        _ => Vec::new(),
    })
}

/// `/_(min|max|avg)(_week|_month)?$/`
fn is_statistic(key: &str) -> bool {
    ["", "_week", "_month"].iter().any(|suffix| {
        key.strip_suffix(suffix)
            .is_some_and(|base| ["_min", "_max", "_avg"].iter().any(|s| base.ends_with(s)))
    })
}

/// `trimRow`: drop the historical and statistical columns (they roughly triple the
/// payload), nulls and empty strings. Zero stays: it is a real price. Non-objects pass
/// through.
pub fn trim_row(row: &Value) -> Value {
    let Value::Object(obj) = row else {
        return row.clone();
    };
    let out: Map<String, Value> = obj
        .iter()
        .filter(|(k, v)| {
            !is_statistic(k)
                && !k.starts_with("volatility_")
                && !v.is_null()
                && v.as_str() != Some("")
        })
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    Value::Object(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn stub(
        body: Value,
        status: u16,
    ) -> impl FnMut(&str, Option<&str>) -> Result<HttpResponse, String> {
        move |_, _| {
            Ok(HttpResponse {
                status,
                body: body.to_string(),
            })
        }
    }

    fn with_token(t: &str) -> UexOptions {
        UexOptions {
            token: Some(t.into()),
            ..Default::default()
        }
    }

    #[test]
    fn returns_the_data_array() {
        let rows = uex_fetch(
            "terminals",
            &with_token("t"),
            &mut stub(json!({"status": "ok", "data": [{"id": 1}, {"id": 2}]}), 200),
        )
        .unwrap();
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn wraps_a_single_object() {
        let rows = uex_fetch(
            "game_versions",
            &with_token("t"),
            &mut stub(json!({"status": "ok", "data": {"live": "4.10"}}), 200),
        )
        .unwrap();
        assert_eq!(rows, vec![json!({"live": "4.10"})]);
    }

    #[test]
    fn explains_how_to_get_a_token_when_none_is_set() {
        let e = uex_fetch("terminals", &with_token(""), &mut stub(json!({}), 200)).unwrap_err();
        assert_eq!(e, MISSING_TOKEN);
    }

    #[test]
    fn fails_on_a_body_level_error_even_though_http_said_200() {
        let e = uex_fetch(
            "terminals",
            &with_token("bad"),
            &mut stub(
                json!({"status": "access_denied", "message": "Invalid bearer token."}),
                200,
            ),
        )
        .unwrap_err();
        assert!(e.contains("Invalid bearer token"), "{e}");
    }

    #[test]
    fn fails_on_a_genuine_http_error() {
        let e = uex_fetch("terminals", &with_token("t"), &mut stub(json!({}), 502)).unwrap_err();
        assert!(e.contains("HTTP 502"), "{e}");
    }

    #[test]
    fn sends_no_token_for_a_public_endpoint_when_none_is_set() {
        let mut seen = Some("unset".to_string());
        let mut http = |url: &str, token: Option<&str>| {
            assert_eq!(url, "https://api.uexcorp.space/2.0/refineries_methods");
            seen = token.map(str::to_string);
            Ok(HttpResponse {
                status: 200,
                body: r#"{"status":"ok","data":[]}"#.into(),
            })
        };
        uex_fetch("refineries_methods", &with_token("").public(), &mut http).unwrap();
        assert_eq!(seen, None);
    }

    #[test]
    fn returns_an_empty_array_when_there_is_no_data() {
        let rows = uex_fetch(
            "x",
            &with_token("t"),
            &mut stub(json!({"status": "ok"}), 200),
        )
        .unwrap();
        assert!(rows.is_empty());
    }

    #[test]
    fn trim_drops_statistical_columns() {
        assert_eq!(
            trim_row(&json!({
                "price_buy": 100,
                "price_buy_min": 90,
                "price_buy_max_week": 120,
                "price_buy_avg_month": 105,
                "volatility_price_buy": 3,
            })),
            json!({"price_buy": 100})
        );
    }

    #[test]
    fn trim_drops_nulls_and_empty_strings_and_keeps_zero() {
        assert_eq!(
            trim_row(&json!({"a": 1, "b": null, "c": ""})),
            json!({"a": 1})
        );
        assert_eq!(trim_row(&json!({"price_buy": 0})), json!({"price_buy": 0}));
    }

    #[test]
    fn trim_passes_non_objects_through() {
        assert_eq!(trim_row(&json!("x")), json!("x"));
        assert_eq!(trim_row(&Value::Null), Value::Null);
    }
}
