//! Questions over the datasets (port of `sc-gamedata/src/query/`): fuzzy find, where an item
//! comes from, where to buy it, what a shop sells.
//!
//! Like the TypeScript, this layer reads parsed datasets (`game-item-tags`, `game-loot`,
//! `game-item-sources`, `game-blueprints`, `game-wikelo-trades`, and the third-party UEX
//! `uex-item-prices` / `uex-commodity-prices` / `uex-terminals` when a caller has them),
//! never game files, and does no I/O itself. The local MCP server is its caller.
//!
//! | Module | Ports |
//! |---|---|
//! | [`commodities`] | (ours) the commodity browser: resource list, one resource in full, kiosk prices |
//! | [`types`] | `query/types.ts` (the dataset shapes it reads) |
//! | [`fuzzy`] | `query/fuzzy.ts` (scorer, ranking; entries come from [`crate::search_index`]) |
//! | [`shops`] | `query/shops.ts` (UEX rows placed, dated, filtered by place; shop inventory) |
//! | [`obtain`] | `query/obtain.ts` (loot model, spawns, blueprints, awards, shops) |
//! | [`mined`] | (ours) where an ore is mined: belts and bodies, and spots in loot areas |
//! | [`quality`] | (ours) the quality curve, place overrides and shown steps of a resource |
//! | [`sells`] | (ours) where a commodity sells best: UEX kiosk prices matched to resources |
//! | [`where_`] | `query/where.ts` (one row per source, easiest first) |
//!
//! JavaScript semantics kept where answers depend on them: stable sorts, `Map` insertion
//! order, `toLocaleString("en-US")` prices, `Date.parse` / `toISOString` dates, UTF-16
//! lengths in the fuzzy tie-breaks.

pub mod commodities;
pub mod fuzzy;
pub mod mined;
pub mod obtain;
pub mod quality;
pub mod sells;
pub mod shops;
pub mod types;
pub mod where_;

/// `n.toLocaleString("en-US")`: thousands separators, at most three decimals (rounded half
/// away from zero, as ICU does), trailing zeros dropped.
pub fn en_us(n: f64) -> String {
    if !n.is_finite() {
        return if n.is_nan() {
            "NaN".into()
        } else if n > 0.0 {
            "∞".into()
        } else {
            "-∞".into()
        };
    }
    let scaled = (n.abs() * 1000.0).round() as u128;
    let (int, frac) = (scaled / 1000, scaled % 1000);
    let digits = int.to_string();
    let mut out = String::new();
    if n < 0.0 && scaled != 0 {
        out.push('-');
    }
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if frac != 0 {
        let f = format!("{frac:03}");
        out.push('.');
        out.push_str(f.trim_end_matches('0'));
    }
    out
}

/// `Date.parse` for the ISO-8601 forms the datasets carry (`2026-11-14T17:21:07.962Z`,
/// `2026-11-14`, with or without seconds, fraction or `Z`/offset). `None` when unreadable.
pub fn parse_iso_millis(s: &str) -> Option<i64> {
    let s = s.trim();
    let num = |t: &str| -> Option<i64> {
        (!t.is_empty() && t.bytes().all(|b| b.is_ascii_digit()))
            .then(|| t.parse().ok())
            .flatten()
    };
    let (date, rest) = match s.find('T') {
        Some(i) => (&s[..i], Some(&s[i + 1..])),
        None => (s, None),
    };
    let mut dp = date.split('-');
    let year = num(dp.next()?)?;
    let month = dp.next().map_or(Some(1), num)?;
    let day = dp.next().map_or(Some(1), num)?;
    if dp.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let mut ms_of_day = 0i64;
    let mut offset_min = 0i64;
    if let Some(rest) = rest {
        let (time, zone) = match rest.find(['Z', 'z', '+', '-']) {
            Some(i) => (&rest[..i], &rest[i..]),
            None => (rest, ""),
        };
        let (hms, frac) = match time.split_once('.') {
            Some((a, b)) => (a, Some(b)),
            None => (time, None),
        };
        let mut tp = hms.split(':');
        let h = num(tp.next()?)?;
        let m = num(tp.next()?)?;
        let sec = tp.next().map_or(Some(0), num)?;
        if tp.next().is_some() || h > 24 || m > 59 || sec > 59 {
            return None;
        }
        let millis = match frac {
            Some(f) => {
                let f3: String = f.chars().chain("000".chars()).take(3).collect();
                num(&f3)?
            }
            None => 0,
        };
        ms_of_day = ((h * 60 + m) * 60 + sec) * 1000 + millis;
        match zone {
            "" | "Z" | "z" => {}
            z => {
                let sign = if z.starts_with('-') { -1 } else { 1 };
                let body = z[1..].replace(':', "");
                if body.len() != 4 {
                    return None;
                }
                offset_min = sign * (num(&body[..2])? * 60 + num(&body[2..])?);
            }
        }
    }
    // Days from civil (Howard Hinnant).
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400_000 + ms_of_day - offset_min * 60_000)
}

/// UTF-16 length, as JavaScript's `string.length`.
pub(crate) fn js_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// Lower-case words of a name: runs of ASCII letters and digits (`/[^a-z0-9]+/` after
/// `toLowerCase`).
pub(crate) fn words(s: &str) -> Vec<String> {
    s.to_lowercase()
        .split(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit()))
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locale_numbers() {
        assert_eq!(en_us(0.0), "0");
        assert_eq!(en_us(275.0), "275");
        assert_eq!(en_us(5000.0), "5,000");
        assert_eq!(en_us(1_234_567.0), "1,234,567");
        assert_eq!(en_us(1234.5678), "1,234.568");
        assert_eq!(en_us(-21200.25), "-21,200.25");
    }

    #[test]
    fn iso_dates() {
        assert_eq!(
            parse_iso_millis("2026-11-14T00:00:00Z"),
            Some(1_794_614_400_000)
        );
        assert_eq!(parse_iso_millis("2026-11-14"), Some(1_794_614_400_000));
        assert_eq!(
            parse_iso_millis("2026-11-14T17:21:07.962Z"),
            Some(1_794_614_400_000 + 62_467_962)
        );
        assert_eq!(
            parse_iso_millis("2026-11-14T01:00:00+01:00"),
            Some(1_794_614_400_000)
        );
        assert_eq!(parse_iso_millis("not a date"), None);
        assert_eq!(
            crate::output::iso_from_millis(parse_iso_millis("2026-09-05T12:00:00Z").unwrap()),
            "2026-09-05T12:00:00.000Z"
        );
    }
}
