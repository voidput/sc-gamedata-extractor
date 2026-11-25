//! Fuzzy find over items and ships (port of `query/fuzzy.ts`): an fzf-style scorer and
//! the ranking. Entries come from `game-search-index.json` ([`from_index`]) or straight
//! from item-tags rows ([`item_entries`]), which give the same list.
//!
//! The scorer favours a contiguous hit over a scattered one, a word start over mid-word,
//! the display name over the class name, and player hulls over the AI and faction variants
//! that share their name. A term with no contiguous hit may still match a word of the
//! display name within a small edit distance (a typo); tiers per term: contiguous hit, then
//! typo of a name word, then scattered letters.
//!
//! Not ported: `toLine`/`parseLine`/`renderShip`, the fzf picker's line format.

use super::js_len;
use super::types::ItemTags;
use crate::search_index;
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryKind {
    Item,
    Ship,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Entry {
    pub kind: EntryKind,
    /// Class name for items, short record name for ships.
    pub key: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maker: Option<String>,
    /// Item type, or the ship's role.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub ty: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
}

fn js_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => "null".into(),
        other => other.to_string(),
    }
}

/// Entries of a `game-search-index.json` document (`loadSearchIndexEntries`).
pub fn from_index(doc: &Value, all: bool) -> Result<Vec<Entry>, search_index::UnknownFormat> {
    Ok(search_index::load_search_index_entries(doc, all)?
        .into_iter()
        .map(|e| Entry {
            kind: match e.kind {
                search_index::Kind::Item => EntryKind::Item,
                search_index::Kind::Ship => EntryKind::Ship,
            },
            key: e.key.as_ref().map(js_string).unwrap_or_default(),
            name: e.name.as_ref().map(js_string).unwrap_or_default(),
            maker: e.maker.as_ref().map(js_string),
            ty: e.ty.as_ref().map(js_string),
            family: e.family,
        })
        .collect())
}

/// `buildEntries(items, [], { all })` over typed rows.
pub fn item_entries(items: &[ItemTags], all: bool) -> Vec<Entry> {
    const NOISE: &[&str] = &[
        "Cargo",
        "Door",
        "Seat",
        "Usable",
        "UNDEFINED",
        "Light",
        "Room",
        "Display",
        "Elevator",
    ];
    let mut out = Vec::new();
    for item in items {
        let ty = item.ty.as_deref().filter(|t| !t.is_empty());
        if !all && ty.is_none_or(|t| NOISE.contains(&t)) {
            continue;
        }
        let tags: Vec<&str> = item.tags.iter().map(String::as_str).collect();
        let maker = tags
            .iter()
            .find_map(|t| t.strip_prefix("Manufacturer/"))
            .map(str::to_string)
            .or_else(|| item.manufacturer.clone());
        let family = search_index::weapon_family(&tags);
        let name = match &family {
            Some(f) if item.template() && !f.is_empty() => {
                format!("{f} (unnamed: {})", item.class_name)
            }
            _ => item.name.clone(),
        };
        out.push(Entry {
            kind: EntryKind::Item,
            key: item.class_name.clone(),
            name,
            maker,
            ty: item.ty.clone(),
            family,
        });
    }
    out
}

fn lower_chars(s: &str) -> Vec<char> {
    s.to_lowercase().chars().collect()
}

fn find_from(hay: &[char], needle: &[char], from: usize) -> Option<usize> {
    if needle.is_empty() {
        return (from <= hay.len()).then_some(from);
    }
    if needle.len() > hay.len() {
        return None;
    }
    (from..=hay.len() - needle.len()).find(|&i| hay[i..i + needle.len()] == *needle)
}

/// `/[\s_\-"(/.]/` on the character before `i` (true at the start).
fn boundary_at(t: &[char], i: usize) -> bool {
    i == 0 || {
        let c = t[i - 1];
        c.is_whitespace() || matches!(c, '_' | '-' | '"' | '(' | '/' | '.')
    }
}

fn squash_term(term: &str) -> Vec<char> {
    term.to_lowercase()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect()
}

/// `fuzzyScore(term, text)`: `None` means no match.
pub fn fuzzy_score(term: &str, text: &str) -> Option<f64> {
    let q = squash_term(term);
    if q.is_empty() {
        return Some(0.0);
    }
    let t = lower_chars(text);
    let len = js_len(&text.to_lowercase()) as f64;
    let mut best: Option<f64> = None;
    let mut from = 0;
    while let Some(i) = find_from(&t, &q, from) {
        let end = i + q.len();
        let s = 100.0
            + if boundary_at(&t, i) { 50.0 } else { 0.0 }
            + if end == t.len() || boundary_at(&t, end) {
                10.0
            } else {
                0.0
            };
        if best.is_none_or(|b| s > b) {
            best = Some(s);
        }
        from = i + 1;
    }
    if let Some(b) = best {
        return Some(b - len * 0.1);
    }
    // Scattered: every character in order, rewarding runs and word starts.
    let mut score = 0.0;
    let mut from = 0usize;
    let mut run = 0.0;
    for ch in &q {
        let found = (from..t.len()).find(|&i| t[i] == *ch)?;
        run = if found == from && from > 0 {
            run + 1.0
        } else {
            0.0
        };
        score += 1.0 + run * 2.0 + if boundary_at(&t, found) { 3.0 } else { 0.0 };
        from = found + 1;
    }
    Some(score - len * 0.1)
}

/// Optimal string alignment distance; returns `max + 1` once every alignment is past `max`.
pub fn edit_distance(a: &str, b: &str, max: Option<usize>) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let max = max.unwrap_or(usize::MAX - 1);
    if a.len().abs_diff(b.len()) > max {
        return max + 1;
    }
    let mut prev2: Vec<usize> = Vec::new();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut row = vec![i];
        let mut row_min = i;
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut d = (prev[j] + 1).min(row[j - 1] + 1).min(prev[j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d = d.min(prev2[j - 2] + 1);
            }
            row.push(d);
            row_min = row_min.min(d);
        }
        if row_min > max {
            return max + 1;
        }
        prev2 = prev;
        prev = row;
    }
    prev[b.len()]
}

/// Edits a typo may carry: none under 4 characters, 1 for 4-6, 2 for 7 and up.
pub fn typo_budget(length: usize) -> usize {
    if length >= 7 {
        2
    } else if length >= 4 {
        1
    } else {
        0
    }
}

const SCATTER_CAP: f64 = 50.0;
const SCATTER_SHIFT: f64 = -100.0;

/// Lower-case words of a name: letters and digits.
pub fn name_words(name: &str) -> Vec<String> {
    super::words(name)
}

/// Score for a term that is a typo of one of `words`; `None` when none is close enough.
pub fn typo_score(term: &str, words: &[String]) -> Option<f64> {
    let q: String = squash_term(term).into_iter().collect();
    let budget = typo_budget(js_len(&q));
    if budget == 0 {
        return None;
    }
    let mut best: Option<usize> = None;
    for w in words {
        let d = edit_distance(&q, w, Some(budget));
        if d <= budget && best.is_none_or(|b| d < b) {
            best = Some(d);
        }
        if best == Some(1) {
            break;
        }
    }
    best.map(|d| -(d as f64) * 20.0)
}

/// `/_(AI|PU_AI|CIV|CRIM|PIR|Template|Unmanned|EA)(_|$)/i`.
fn npc_ship(key: &str) -> bool {
    const TOKENS: &[&str] = &[
        "ai", "pu_ai", "civ", "crim", "pir", "template", "unmanned", "ea",
    ];
    let k = key.to_ascii_lowercase();
    k.match_indices('_').any(|(i, _)| {
        let rest = &k[i + 1..];
        TOKENS.iter().any(|t| {
            rest.strip_prefix(t)
                .is_some_and(|after| after.is_empty() || after.starts_with('_'))
        })
    })
}

/// One term against one entry: contiguous hit, else typo of a name word, else scattered.
fn term_score(term: &str, entry: &Entry, words: &[String], rest: &str) -> Option<f64> {
    let q = term.to_lowercase();
    let by_name = fuzzy_score(term, &entry.name);
    let by_rest = fuzzy_score(term, rest);
    let contiguous = entry.name.to_lowercase().contains(&q) || rest.to_lowercase().contains(&q);
    let best = by_name
        .map_or(f64::NEG_INFINITY, |s| s + 10.0)
        .max(by_rest.unwrap_or(f64::NEG_INFINITY));
    if contiguous {
        return Some(best);
    }
    if let Some(t) = typo_score(term, words) {
        return Some(t - js_len(&entry.name) as f64 * 0.1);
    }
    (best != f64::NEG_INFINITY).then(|| best.min(SCATTER_CAP) + SCATTER_SHIFT)
}

/// Entries matching every space-separated term, best first, with their scores.
pub fn rank_scored<'a>(entries: &'a [Entry], query: &str) -> Vec<(&'a Entry, f64)> {
    let terms: Vec<&str> = query.split_whitespace().collect();
    if terms.is_empty() {
        return entries.iter().map(|e| (e, 0.0)).collect();
    }
    let mut scored: Vec<(&Entry, f64)> = Vec::new();
    for entry in entries {
        let rest = format!(
            "{} {} {} {}",
            entry.key,
            entry.family.as_deref().unwrap_or(""),
            entry.maker.as_deref().unwrap_or(""),
            entry.ty.as_deref().unwrap_or("")
        );
        let words = name_words(&entry.name);
        let mut total = if entry.name == entry.key { -5.0 } else { 0.0 }
            + if entry.kind == EntryKind::Ship && npc_ship(&entry.key) {
                -15.0
            } else {
                0.0
            };
        let mut ok = true;
        for term in &terms {
            match term_score(term, entry, &words, &rest) {
                Some(s) => total += s,
                None => {
                    ok = false;
                    break;
                }
            }
        }
        if ok {
            scored.push((entry, total));
        }
    }
    // Stable, like Array.prototype.sort.
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored
}

/// `rankEntries(entries, query)`.
pub fn rank_entries<'a>(entries: &'a [Entry], query: &str) -> Vec<&'a Entry> {
    rank_scored(entries, query)
        .into_iter()
        .map(|(e, _)| e)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn item(class: &str, name: &str, tags: &[&str], ty: &str, template: bool) -> Value {
        let mut v = json!({"className": class, "name": name, "id": class, "type": ty,
                           "tags": tags});
        if template {
            v["isTemplate"] = json!(true);
        }
        v
    }

    fn typed(v: &[Value]) -> Vec<ItemTags> {
        v.iter()
            .map(|x| serde_json::from_value(x.clone()).unwrap())
            .collect()
    }

    fn items() -> Vec<Value> {
        vec![
            item(
                "ksar_lmg_energy_01",
                "ksar_lmg_energy_01",
                &["Weapon/FPS/Stocked/LMG/Verdict", "Manufacturer/KastakArms"],
                "WeaponPersonal",
                true,
            ),
            item(
                "ksar_lmg_energy_01_store01",
                "Verdict \"Derivation\" LMG",
                &["Weapon/FPS/Stocked/LMG/Verdict", "Manufacturer/KastakArms"],
                "WeaponPersonal",
                false,
            ),
            item(
                "behr_lmg_ballistic_01",
                "FS-9 LMG",
                &["Weapon/FPS/Stocked/LMG/FS-9", "Manufacturer/Behring"],
                "WeaponPersonal",
                false,
            ),
            item(
                "outlaw_arms_01",
                "Lynx Arms Grey",
                &["Manufacturer/KastakArms"],
                "Char_Armor_Arms",
                false,
            ),
            item("door_01", "Door", &[], "Door", false),
        ]
    }

    fn ships() -> Vec<Value> {
        vec![
            json!({"name": "Aegis Gladius", "recordName": "EntityClassDefinition.AEGS_Gladius_AI_Super", "manufacturer": "aegs", "role": "Light Fighter"}),
            json!({"name": "Aegis Gladius", "recordName": "EntityClassDefinition.AEGS_Gladius", "manufacturer": "aegs", "role": "Light Fighter"}),
            json!({"name": "Gladius Template", "recordName": "EntityClassDefinition.AEGS_Gladius_Template", "isTemplate": true}),
        ]
    }

    /// Entries through the published index, as the server reads them.
    fn entries(items: &[Value], ships: &[Value], all: bool) -> Vec<Entry> {
        let doc = search_index::to_value(&search_index::build_search_index(items, ships, None));
        from_index(&doc, all).unwrap()
    }

    fn keys(v: &[&Entry]) -> Vec<String> {
        v.iter().map(|e| e.key.clone()).collect()
    }

    #[test]
    fn drops_world_furniture_and_templates_unless_asked() {
        let e = entries(&items(), &ships(), false);
        assert!(!e.iter().any(|x| x.key == "door_01"));
        assert!(!e.iter().any(|x| x.key == "AEGS_Gladius_Template"));
        assert!(
            entries(&items(), &ships(), true)
                .iter()
                .any(|x| x.key == "door_01")
        );
        // The typed builder agrees with the index for items.
        let typed_e = item_entries(&typed(&items()), false);
        let from_idx: Vec<Entry> = e
            .into_iter()
            .filter(|x| x.kind == EntryKind::Item)
            .collect();
        assert_eq!(typed_e, from_idx);
    }

    #[test]
    fn names_an_unnamed_gun_by_its_family() {
        let e = entries(&items(), &ships(), false);
        assert_eq!(
            e.iter()
                .find(|x| x.key == "ksar_lmg_energy_01")
                .unwrap()
                .name,
            "Verdict (unnamed: ksar_lmg_energy_01)"
        );
    }

    #[test]
    fn contiguous_beats_scattered() {
        assert!(
            fuzzy_score("lmg", "FS-9 LMG").unwrap() > fuzzy_score("lmg", "Lynx Arms Grey").unwrap()
        );
        assert_eq!(fuzzy_score("xyz", "FS-9 LMG"), None);
    }

    #[test]
    fn ranks_every_term_display_name_first_player_hulls_first() {
        let e = entries(&items(), &ships(), false);
        let mut top: Vec<String> = keys(&rank_entries(&e, "kastak lmg")[..2]);
        top.sort();
        assert_eq!(top, ["ksar_lmg_energy_01", "ksar_lmg_energy_01_store01"]);
        assert!(keys(&rank_entries(&e, "verdict")).contains(&"ksar_lmg_energy_01".to_string()));
        assert_eq!(rank_entries(&e, "gladius")[0].key, "AEGS_Gladius");
        assert_eq!(rank_entries(&e, "fs9")[0].key, "behr_lmg_ballistic_01");
    }

    #[test]
    fn edit_distance_counts_swaps_once() {
        assert_eq!(edit_distance("cambino", "cambio", None), 1);
        assert_eq!(edit_distance("verdcit", "verdict", None), 1);
        assert_eq!(edit_distance("kitten", "sitting", None), 3);
        assert_eq!(edit_distance("abc", "abc", None), 0);
        assert_eq!(edit_distance("gladius", "zzzzzzz", Some(2)), 3);
        assert_eq!(typo_budget(3), 0);
        assert_eq!(typo_budget(5), 1);
        assert_eq!(typo_budget(9), 2);
    }

    #[test]
    fn typo_of_a_name_word_ranks_below_real_hits() {
        let mut pool = items();
        pool.push(item(
            "grin_salvage_repair_01",
            "Cambio SRT",
            &["Manufacturer/GreyCat"],
            "WeaponPersonal",
            false,
        ));
        pool.push(item(
            "grin_multitool_resource_salvage_repair_01",
            "Cambio-Lite SRT Canister",
            &["Manufacturer/GreyCat"],
            "WeaponAttachment",
            false,
        ));
        let e = item_entries(&typed(&pool), false);
        assert_eq!(
            rank_entries(&e, "cambino srt")[0].key,
            "grin_salvage_repair_01"
        );
        assert!(
            keys(&rank_entries(&e, "verdcit")).contains(&"ksar_lmg_energy_01_store01".to_string())
        );
        assert_eq!(typo_score("lgm", &["lmg".into()]), None);
        assert!(
            typo_score("cambino", &["cambio".into(), "srt".into()]).unwrap()
                < fuzzy_score("srt", "Lynx Arms Grey Cambio-Lite SRT Canister").unwrap()
        );
        assert_eq!(typo_score("cxmbxo", &["cambio".into()]), None);
    }

    #[test]
    fn typo_beats_scattered_letters() {
        let mut pool = vec![
            item(
                "grin_all_lens_ent_01",
                "Remote Turret",
                &[],
                "Turret",
                false,
            ),
            item(
                "devs_stampede_module_ator",
                "Stampede Module",
                &[],
                "Misc",
                false,
            ),
        ];
        pool.extend(items());
        pool.push(item(
            "klwe_rifle_energy_01",
            "Gallant Rifle",
            &["Manufacturer/Klaus&Werner"],
            "WeaponPersonal",
            false,
        ));
        pool.push(item(
            "ksar_shotgun_energy_01",
            "Devastator Shotgun",
            &["Manufacturer/KastakArms"],
            "WeaponPersonal",
            false,
        ));
        let e = item_entries(&typed(&pool), false);
        assert_eq!(rank_entries(&e, "gallent")[0].key, "klwe_rifle_energy_01");
        assert_eq!(
            rank_entries(&e, "devestator")[0].key,
            "ksar_shotgun_energy_01"
        );
        assert_eq!(rank_entries(&e, "rifel")[0].key, "klwe_rifle_energy_01");
        assert!(keys(&rank_entries(&e, "gllnt")).contains(&"grin_all_lens_ent_01".to_string()));
        assert_eq!(rank_entries(&e, "gallant")[0].key, "klwe_rifle_energy_01");
    }

    #[test]
    fn npc_hull_pattern() {
        assert!(npc_ship("AEGS_Gladius_AI_Super"));
        assert!(npc_ship("AEGS_Gladius_PU_AI_CIV"));
        assert!(npc_ship("X_Template"));
        assert!(!npc_ship("AEGS_Gladius"));
        assert!(!npc_ship("AEGS_Gladius_AIR"));
    }
}
