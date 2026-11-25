//! `sc-gamedata find`: the fzf picker over items and ships (port of `sc-gamedata/src/finder.ts`
//! and the line format and ship profile of `query/fuzzy.ts`: `toLine`, `parseLine`,
//! `renderShip`). Scoring and ranking are in [`crate::query::fuzzy`].
//!
//! With fzf on PATH and a terminal, the picker is fzf: every entry, a live preview, and keys
//! that run other commands on the highlighted entry. Every action goes back through the CLI
//! (`find-run <action> <kind> <key>`), so fzf only ever shells out to this program with a
//! kind and a record key.

use crate::output::js_number;
use crate::query::fuzzy::{Entry, EntryKind};
use crate::search_index::ship_key;
use serde_json::Value;
use std::io::Write;
use std::process::{Command, Stdio};

/// The picker's keys and what they do, in header order.
pub const FIND_KEYS: &[(&str, &str)] = &[
    ("enter", "report"),
    ("ctrl-j", "json"),
    ("ctrl-t", "tags"),
    ("ctrl-e", "engage (ship)"),
    ("ctrl-d", "deflect (ship)"),
    ("ctrl-y", "pick + exit"),
    ("tab", "multi-select"),
];

/// Whether `fzf --version` runs.
pub fn has_fzf() -> bool {
    Command::new("fzf")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// POSIX single-quote quoting.
pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// The shell command fzf runs for an action on the highlighted line (`{1}` kind, `{2}` key).
pub fn action_command(self_cmd: &[String], action: &str, trailing: &[String]) -> String {
    let mut parts: Vec<String> = self_cmd.iter().map(|s| shell_quote(s)).collect();
    parts.extend(["find-run".into(), action.into(), "{1}".into(), "{2}".into()]);
    parts.extend(trailing.iter().map(|s| shell_quote(s)));
    parts.join(" ")
}

/// The first `n` UTF-16 units of `s` (`String.prototype.slice(0, n)`), never splitting a
/// character.
pub fn js_slice(s: &str, n: usize) -> &str {
    let mut units = 0;
    for (i, c) in s.char_indices() {
        units += c.len_utf16();
        if units > n {
            return &s[..i];
        }
    }
    s
}

/// `String.prototype.padEnd(width)`, counting UTF-16 units.
pub fn js_pad_end(s: &str, width: usize) -> String {
    let len = s.encode_utf16().count();
    format!("{s}{}", " ".repeat(width.saturating_sub(len)))
}

fn fit(s: Option<&str>, width: usize) -> String {
    let clean = s.unwrap_or("").replace(['\t', '\n'], " ");
    if clean.encode_utf16().count() > width {
        format!("{}…", js_slice(&clean, width - 1))
    } else {
        js_pad_end(&clean, width)
    }
}

/// `toLine`: tab-separated kind and key (hidden from view), then the one padded column the
/// user reads and searches.
pub fn to_line(entry: &Entry) -> String {
    let ty = match entry.kind {
        EntryKind::Ship => format!("ship · {}", entry.ty.as_deref().unwrap_or("")),
        EntryKind::Item => entry.ty.clone().unwrap_or_default(),
    };
    let shown = format!(
        "{} {} {} {}",
        fit(Some(&entry.name), 46),
        fit(Some(&ty), 24),
        fit(entry.maker.as_deref(), 16),
        entry.key
    );
    let kind = match entry.kind {
        EntryKind::Item => "item",
        EntryKind::Ship => "ship",
    };
    format!("{kind}\t{}\t{shown}", entry.key)
}

/// `parseLine`: the kind and key of a picked line.
pub fn parse_line(line: &str) -> Option<(EntryKind, &str)> {
    let mut parts = line.split('\t');
    let kind = match parts.next()? {
        "item" => EntryKind::Item,
        "ship" => EntryKind::Ship,
        _ => return None,
    };
    let key = parts.next().filter(|k| !k.is_empty())?;
    Some((kind, key))
}

/// JavaScript `String(value)` for a JSON value.
fn js_string(v: Option<&Value>) -> String {
    match v {
        None => "undefined".into(),
        Some(Value::Null) => "null".into(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Number(n)) => js_number(n.as_f64().unwrap_or(f64::NAN)),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(a)) => a
            .iter()
            .map(|x| match x {
                Value::Null => String::new(),
                other => js_string(Some(other)),
            })
            .collect::<Vec<_>>()
            .join(","),
        Some(Value::Object(_)) => "[object Object]".into(),
    }
}

/// `renderShip`: the short profile the picker previews for a ship.
pub fn render_ship(ship: &Value) -> String {
    let upper = ship
        .get("manufacturer")
        .and_then(Value::as_str)
        .map(|m| Value::String(m.to_uppercase()));
    let record = ship
        .get("recordName")
        .and_then(Value::as_str)
        .map(|r| Value::String(ship_key(r).to_string()));
    let fields: [(&str, Option<&Value>); 7] = [
        ("maker", upper.as_ref()),
        ("career", ship.get("career")),
        ("role", ship.get("role")),
        ("size", ship.get("size")),
        ("crew", ship.get("crewSize")),
        ("movement", ship.get("movementClass")),
        ("record", record.as_ref()),
    ];
    let title = match ship.get("displayName") {
        None | Some(Value::Null) => ship.get("name"),
        some => some,
    };
    let mut lines = vec![js_string(title)];
    for (label, value) in fields {
        match value {
            None | Some(Value::Null) => {}
            Some(Value::String(s)) if s.is_empty() => {}
            Some(v) => lines.push(format!("  {} {}", js_pad_end(label, 9), js_string(Some(v)))),
        }
    }
    if let Some(Value::String(d)) = ship.get("description") {
        lines.push(String::new());
        lines.push(d.replace("\\n", "\n"));
    }
    lines.push(String::new());
    lines.push("ctrl-e engage · ctrl-d deflection · ctrl-j JSON".into());
    lines.join("\n")
}

/// The fzf arguments for [`run_fzf`].
pub fn fzf_args(
    self_cmd: &[String],
    query: &str,
    trailing: &[String],
    extra: &[String],
) -> Vec<String> {
    let run = |action: &str| action_command(self_cmd, action, trailing);
    let pager = "${PAGER:-less -R}";
    let header = FIND_KEYS
        .iter()
        .map(|(k, v)| format!("{} {v}", k.replace("ctrl-", "^")))
        .collect::<Vec<_>>()
        .join(" · ");
    let mut args: Vec<String> = vec![
        "--delimiter=\t".into(),
        "--with-nth=3".into(),
        "--no-hscroll".into(),
        "--tiebreak=begin,length".into(),
        "--layout=reverse".into(),
        "--height=100%".into(),
        "--info=inline-right".into(),
        "--prompt=sc-gamedata> ".into(),
        format!("--header={header}"),
        "--preview-window=right,58%,wrap".into(),
        format!("--preview={}", run("preview")),
        format!("--bind=enter:execute({} | {pager})", run("report")),
        format!("--bind=ctrl-j:execute({} | {pager})", run("json")),
        format!("--bind=ctrl-t:execute({} | {pager})", run("tags")),
        format!("--bind=ctrl-e:execute({} | {pager})", run("engage")),
        format!("--bind=ctrl-d:execute({} | {pager})", run("deflect")),
        "--bind=ctrl-y:accept".into(),
        "--multi".into(),
    ];
    if !query.is_empty() {
        args.push(format!("--query={query}"));
    }
    args.extend(extra.iter().cloned());
    args
}

/// `runFzf`: run fzf over the entries. `self_cmd` re-enters this CLI and `trailing` are
/// flags every action needs (`--out`). Returns the keys picked with ctrl-y, if any. `extra`
/// are more fzf flags (tests pass `--filter` to run headless).
pub fn run_fzf(
    entries: &[&Entry],
    self_cmd: &[String],
    query: &str,
    trailing: &[String],
    extra: &[String],
) -> Result<Vec<String>, String> {
    let mut child = Command::new("fzf")
        .args(fzf_args(self_cmd, query, trailing, extra))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("fzf: {e}"))?;
    let input: String = entries
        .iter()
        .map(|e| to_line(e))
        .collect::<Vec<_>>()
        .join("\n");
    if let Some(mut stdin) = child.stdin.take() {
        // fzf may exit before reading everything (Esc); a broken pipe is fine then.
        let _ = stdin.write_all(input.as_bytes());
    }
    let out = child.wait_with_output().map_err(|e| format!("fzf: {e}"))?;
    // 130 is Esc / ctrl-c, 1 is no match: both are a normal way out.
    if !out.status.success() {
        return Ok(Vec::new());
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .split('\n')
        .filter_map(parse_line)
        .map(|(_, key)| key.to_string())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn item(key: &str, name: &str) -> Entry {
        Entry {
            kind: EntryKind::Item,
            key: key.into(),
            name: name.into(),
            maker: Some("Kastak Arms".into()),
            ty: Some("WeaponPersonal".into()),
            family: None,
        }
    }

    #[test]
    fn lines_round_trip_and_pad() {
        let line = to_line(&item("ksar_lmg_01", "Verdict\tLMG"));
        let (kind, key) = parse_line(&line).unwrap();
        assert_eq!((kind, key), (EntryKind::Item, "ksar_lmg_01"));
        let shown = line.split('\t').nth(2).unwrap();
        assert!(shown.starts_with("Verdict LMG "));
        assert_eq!(js_slice(shown, 47).chars().count(), 47);
        assert_eq!(parse_line("other\tx\ty"), None);
        assert_eq!(parse_line("item\t\ty"), None);
        let long = to_line(&item("k", &"x".repeat(60)));
        assert!(long.contains(&format!("{}…", "x".repeat(45))));
        let mut ship = item("AEGS_Gladius", "Gladius");
        ship.kind = EntryKind::Ship;
        ship.ty = Some("Light Fighter".into());
        assert!(to_line(&ship).contains("ship · Light Fighter"));
    }

    #[test]
    fn quoting_and_action_commands() {
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
        assert_eq!(
            action_command(
                &["/bin/sc".into()],
                "preview",
                &["--out".into(), "/d".into()]
            ),
            "'/bin/sc' find-run preview {1} {2} '--out' '/d'"
        );
        let args = fzf_args(&["sc".into()], "verdict", &[], &[]);
        assert!(args.contains(&"--query=verdict".to_string()));
        assert!(args[8].starts_with("--header=enter report · ^j json"));
    }

    #[test]
    fn ship_profile() {
        let text = render_ship(&json!({
            "name": "Gladius", "recordName": "EntityClassDefinition.AEGS_Gladius",
            "manufacturer": "aegs", "role": "Light Fighter", "career": "", "size": 2,
            "crewSize": null, "description": "Line one\\nLine two",
        }));
        assert_eq!(
            text,
            "Gladius\n  maker     AEGS\n  role      Light Fighter\n  size      2\n  record    AEGS_Gladius\n\nLine one\nLine two\n\nctrl-e engage · ctrl-d deflection · ctrl-j JSON"
        );
    }

    #[test]
    fn fzf_runs_headless_with_filter() {
        if !has_fzf() {
            return;
        }
        let (a, b) = (
            item("ksar_lmg_01", "Verdict LMG"),
            item("behr_lmg_01", "FS-9 LMG"),
        );
        let picked = run_fzf(
            &[&a, &b],
            &["true".into()],
            "",
            &[],
            &["--filter=verdict".into()],
        );
        assert_eq!(picked.unwrap(), vec!["ksar_lmg_01"]);
    }

    #[test]
    fn utf16_helpers() {
        assert_eq!(js_slice("a😀b", 2), "a");
        assert_eq!(js_slice("a😀b", 3), "a😀");
        assert_eq!(js_pad_end("😀", 4), "😀  ");
    }
}
