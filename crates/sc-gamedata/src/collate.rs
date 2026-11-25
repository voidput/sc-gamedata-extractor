//! String ordering as `sc-gamedata` sorts: ICU collation, reimplemented for the characters
//! game names use, so no ICU dependency is needed.
//!
//! - [`locale_compare`]: `a.localeCompare(b)` in Node (ICU root/`en` collation, all three
//!   levels: base letters, then accents, then case with lower case first). Punctuation and
//!   spaces are not ignored and sort before digits, digits before letters.
//! - [`compare_names`]: `compareNames` in `records.ts`: `Intl.Collator("en", {numeric:
//!   true, sensitivity: "base", ignorePunctuation: true})`, ties broken by UTF-16 order.
//!
//! Covered exactly: ASCII and the Latin-1/Latin Extended-A letters with accents. Other
//! characters sort after the letters by code point, which can differ from ICU; if a domain
//! hits that, extend [`element`] (and its tests, which hold orders printed by Node 24).

use std::cmp::Ordering;

/// ASCII spaces, punctuation and symbols in ICU root order (printed by Node 24 with
/// `localeCompare`). All sort before digits.
const VARIABLE: &str = " _-,;:!?.'\"()[]{}@*/\\&#%`^+<=>|~$";
/// The subset `ignorePunctuation` skips (whitespace and punctuation; not symbols).
const IGNORABLE: &str = " !\"#%&'()*,-./:;?@[\\]_{}";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Element {
    primary: u32,
    secondary: u8,
    tertiary: u8,
    ignorable: bool,
}

const DIGIT_BASE: u32 = 100;
const LETTER_BASE: u32 = 200;
const OTHER_BASE: u32 = 1000;

/// Base letter and accent rank of a Latin letter with a diacritic.
fn decompose(c: char) -> Option<(char, u8)> {
    const TABLE: &[(&str, char)] = &[
        ("àáâãäåāăą", 'a'),
        ("ÀÁÂÃÄÅĀĂĄ", 'A'),
        ("çćĉċč", 'c'),
        ("ÇĆĈĊČ", 'C'),
        ("ďđ", 'd'),
        ("ĎĐ", 'D'),
        ("èéêëēĕėęě", 'e'),
        ("ÈÉÊËĒĔĖĘĚ", 'E'),
        ("ĝğġģ", 'g'),
        ("ĜĞĠĢ", 'G'),
        ("ĥħ", 'h'),
        ("ĤĦ", 'H'),
        ("ìíîïĩīĭįı", 'i'),
        ("ÌÍÎÏĨĪĬĮİ", 'I'),
        ("ĵ", 'j'),
        ("Ĵ", 'J'),
        ("ķ", 'k'),
        ("Ķ", 'K'),
        ("ĺļľŀł", 'l'),
        ("ĹĻĽĿŁ", 'L'),
        ("ñńņňŉ", 'n'),
        ("ÑŃŅŇ", 'N'),
        ("òóôõöøōŏő", 'o'),
        ("ÒÓÔÕÖØŌŎŐ", 'O'),
        ("ŕŗř", 'r'),
        ("ŔŖŘ", 'R'),
        ("śŝşš", 's'),
        ("ŚŜŞŠ", 'S'),
        ("ţťŧ", 't'),
        ("ŢŤŦ", 'T'),
        ("ùúûüũūŭůűų", 'u'),
        ("ÙÚÛÜŨŪŬŮŰŲ", 'U'),
        ("ŵ", 'w'),
        ("Ŵ", 'W'),
        ("ýÿŷ", 'y'),
        ("ÝŸŶ", 'Y'),
        ("źżž", 'z'),
        ("ŹŻŽ", 'Z'),
    ];
    TABLE.iter().find_map(|(set, base)| {
        set.chars()
            .position(|x| x == c)
            .map(|i| (*base, i as u8 + 1))
    })
}

fn element(c: char) -> Element {
    if let Some(i) = VARIABLE.find(c) {
        return Element {
            primary: 1 + i as u32,
            secondary: 0,
            tertiary: 0,
            ignorable: IGNORABLE.contains(c),
        };
    }
    if c.is_ascii_digit() {
        return Element {
            primary: DIGIT_BASE + (c as u32 - '0' as u32),
            secondary: 0,
            tertiary: 0,
            ignorable: false,
        };
    }
    let (base, secondary) = decompose(c).unwrap_or((c, 0));
    if base.is_ascii_alphabetic() {
        return Element {
            primary: LETTER_BASE + (base.to_ascii_lowercase() as u32 - 'a' as u32),
            secondary,
            tertiary: u8::from(base.is_ascii_uppercase()),
            ignorable: false,
        };
    }
    // Tabs and line breaks are spaces to ICU (they sort with it, before it in ICU, which
    // names never contain).
    if c.is_whitespace() {
        return Element {
            primary: 0,
            secondary: 0,
            tertiary: 0,
            ignorable: true,
        };
    }
    Element {
        primary: OTHER_BASE + c as u32,
        secondary: 0,
        tertiary: 0,
        ignorable: false,
    }
}

/// `a.localeCompare(b)` (Node, default locale `en-US`). `Equal` means ICU calls them
/// equal; `Array.prototype.sort` then keeps input order.
pub fn locale_compare(a: &str, b: &str) -> Ordering {
    let ea: Vec<Element> = a.chars().map(element).collect();
    let eb: Vec<Element> = b.chars().map(element).collect();
    let level = |f: fn(&Element) -> u32| ea.iter().map(f).cmp(eb.iter().map(f));
    level(|e| e.primary)
        .then_with(|| level(|e| u32::from(e.secondary)))
        .then_with(|| level(|e| u32::from(e.tertiary)))
}

/// Primary keys for numeric, base-strength, punctuation-ignoring collation.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
enum NameKey {
    Char(u32),
    /// A digit run by value: (significant digit count, digits).
    Number(usize, Vec<u8>),
}

fn name_keys(s: &str) -> Vec<(u32, NameKey)> {
    let mut out = Vec::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_ascii_digit() {
            let start = i;
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
            let digits: Vec<u8> = chars[start..i]
                .iter()
                .map(|d| *d as u8 - b'0')
                .skip_while(|d| *d == 0)
                .collect();
            // Numbers sort where digits do, before letters.
            out.push((DIGIT_BASE, NameKey::Number(digits.len(), digits)));
            continue;
        }
        let e = element(c);
        if !e.ignorable {
            out.push((e.primary, NameKey::Char(e.primary)));
        }
        i += 1;
    }
    out
}

/// `compareNames`: people's order (numbers by value, case/accents/punctuation ignored),
/// then UTF-16 code unit order so the result is total.
pub fn compare_names(a: &str, b: &str) -> Ordering {
    name_keys(a)
        .cmp(&name_keys(b))
        .then_with(|| a.encode_utf16().cmp(b.encode_utf16()))
}

/// `a.localeCompare(b, "en", { numeric: true })`: [`locale_compare`] with each digit run
/// weighed by its value at the primary level (`S2` before `S10`). Punctuation counts.
pub fn locale_compare_numeric(a: &str, b: &str) -> Ordering {
    let primaries = |s: &str| {
        let chars: Vec<char> = s.chars().collect();
        let mut out = Vec::new();
        let mut i = 0;
        while i < chars.len() {
            if chars[i].is_ascii_digit() {
                let start = i;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
                let digits: Vec<u8> = chars[start..i]
                    .iter()
                    .map(|d| *d as u8 - b'0')
                    .skip_while(|d| *d == 0)
                    .collect();
                out.push((DIGIT_BASE, NameKey::Number(digits.len(), digits)));
                continue;
            }
            let p = element(chars[i]).primary;
            out.push((p, NameKey::Char(p)));
            i += 1;
        }
        out
    };
    let ea: Vec<Element> = a.chars().map(element).collect();
    let eb: Vec<Element> = b.chars().map(element).collect();
    let level = |f: fn(&Element) -> u32| ea.iter().map(f).cmp(eb.iter().map(f));
    primaries(a)
        .cmp(&primaries(b))
        .then_with(|| level(|e| u32::from(e.secondary)))
        .then_with(|| level(|e| u32::from(e.tertiary)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_order_matches_node() {
        // Node 24: [...].sort((a, b) => a.localeCompare(b, "en", { numeric: true }))
        assert_eq!(
            sorted(
                &["M10A", "M3A", "m3a", "M3A Cannon", "M4A"],
                locale_compare_numeric
            ),
            vec!["m3a", "M3A", "M3A Cannon", "M4A", "M10A"]
        );
    }

    fn sorted(v: &[&str], f: fn(&str, &str) -> Ordering) -> Vec<String> {
        let mut v: Vec<String> = v.iter().map(|s| s.to_string()).collect();
        v.sort_by(|a, b| f(a, b));
        v
    }

    #[test]
    fn ascii_order_matches_node() {
        // Node 24: [...printable ASCII].sort((a, b) => a.localeCompare(b)).join("")
        let expected = " _-,;:!?.'\"()[]{}@*/\\&#%`^+<=>|~$0123456789aAbBcCdDeEfFgGhHiIjJkKlLmMnNoOpPqQrRsStTuUvVwWxXyYzZ";
        let mut chars: Vec<String> = (32u8..127).map(|c| (c as char).to_string()).collect();
        chars.sort_by(|a, b| locale_compare(a, b));
        assert_eq!(chars.concat(), expected);
    }

    #[test]
    fn levels_match_node() {
        // Node 24 output.
        assert_eq!(
            sorted(
                &["a", "A", "á", "b", "ab", "a b", "a-b", "a_b", "aB", "Ab"],
                locale_compare
            ),
            vec!["a", "A", "á", "a b", "a_b", "a-b", "ab", "aB", "Ab", "b"]
        );
        assert_eq!(locale_compare("Aegis", "Aegis"), Ordering::Equal);
        assert_eq!(locale_compare("microTech", "Mirai"), Ordering::Less);
        assert_eq!(locale_compare("CBA", "Caldera"), Ordering::Greater);
    }

    #[test]
    fn compare_names_matches_node() {
        // Node 24 output for compareNames.
        assert_eq!(
            sorted(&["x10", "x2", "X2", "x 2", "x-1"], compare_names),
            vec!["x-1", "X2", "x 2", "x2", "x10"]
        );
        assert_eq!(compare_names("_2", "_10"), Ordering::Less);
        assert_eq!(
            compare_names("a07", "a7"),
            Ordering::Less,
            "tie broken by code unit"
        );
        assert_eq!(
            compare_names("\"Quantainium\" Water Bottle", "Quantainium"),
            Ordering::Greater
        );
    }
}
