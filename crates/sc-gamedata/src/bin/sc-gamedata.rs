//! `sc-gamedata`: the Rust CLI (port of `src/cli.ts`). Same commands, flags and output text
//! wherever the two pipelines allow; `help` lists what differs.
//!
//! The big difference: `extract` reads `Data.p4k` directly (DataCore and string table) and
//! writes every dataset in one pass, so there is no StarBreaker CLI, no `extracted-data/`
//! and no separate `parse` (an alias of `extract` here).
//!
//! `builds`, `snapshot`, `diff`, `changes` and `notes` use [`sc_gamedata::diff`],
//! [`sc_gamedata::changes`] and [`sc_gamedata::notes`]; `deflect` and `engage` use
//! [`sc_gamedata::deflection`] and [`sc_gamedata::engagement`].

use sc_gamedata::diff::{BuildSource, DiffOptions};
use sc_gamedata::domain::{Domain, FileSource, NoFiles};
use sc_gamedata::engagement::{EngageOptions, Gun, Target};
use sc_gamedata::finder::{self, FIND_KEYS, js_pad_end, js_slice};
use sc_gamedata::localization::{
    Localization, apply_overlay, diff_strings, lookup_string, parse_global_ini_bytes,
    search_strings,
};
use sc_gamedata::notes::NotesOptions;
use sc_gamedata::overlays::{self, FetchResponse};
use sc_gamedata::pipeline::{self, Game};
use sc_gamedata::query::fuzzy::{self, EntryKind};
use sc_gamedata::query::obtain::{
    BlueprintLike, Datasets, LootData, ObtainOptions, WikeloTradeLike, ammo_targets,
    build_loot_index, find_items, obtain, render_obtain,
};
use sc_gamedata::query::shops::{
    AsOf, SHOP_ITEM_LIMIT, SHOP_TERMINAL_LIMIT, ShopInputs, ShopPrice, UexTerminal, limit_shop,
    render_shop, shop_inventory,
};
use sc_gamedata::query::types::{ContractAward, ItemTags, LootArea};
use sc_gamedata::query::where_::{pick_ammo, render_where, where_summary};
use sc_gamedata::source::{self, ArchiveFiles, DataCoreSource};
use sc_gamedata::uex::{HttpResponse, UexOptions};
use sc_gamedata::{
    archive, changes, deflection, diff, domains, engagement, notes, output, publish, search_index,
    strings, uex_market,
};
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

type Flags = HashMap<String, String>;

struct Args {
    command: String,
    flags: Flags,
    /// Positionals after the command (domains, search terms, builds).
    rest: Vec<String>,
}

/// Flags that take no value. Without this list a boolean flag swallows the next
/// positional, so `strings --starstrings Ouratite` loses the search term.
const BOOLEAN_FLAGS: &[&str] = &[
    "help",
    "write",
    "keys-only",
    "diff",
    "installed",
    "json",
    "detail",
    "fetch",
    "all",
    "list",
    "print",
    "ammo",
];

/// `parseArgs`: `--flag value`, boolean flags, and positionals.
fn parse_args(argv: &[String]) -> Args {
    let command = argv.first().cloned().unwrap_or_else(|| "help".into());
    let mut flags = Flags::new();
    let mut rest = Vec::new();
    let tail = argv.get(1..).unwrap_or(&[]);
    let mut i = 0;
    while i < tail.len() {
        let token = &tail[i];
        let Some(key) = token.strip_prefix("--") else {
            rest.push(token.clone());
            i += 1;
            continue;
        };
        if BOOLEAN_FLAGS.contains(&key) {
            flags.insert(key.into(), "true".into());
            i += 1;
            continue;
        }
        match tail.get(i + 1) {
            Some(next) if !next.is_empty() && !next.starts_with("--") => {
                flags.insert(key.into(), next.clone());
                i += 2;
            }
            _ => {
                flags.insert(key.into(), "true".into());
                i += 1;
            }
        }
    }
    Args {
        command,
        flags,
        rest,
    }
}

/// `--limit` as a positive integer, or `fallback` when absent; `"all"` (when allowed) is
/// `None` (no limit).
fn limit_flag(flags: &Flags, fallback: usize, all: bool) -> Result<Option<usize>, String> {
    let Some(raw) = flags.get("limit") else {
        return Ok(Some(fallback));
    };
    if all && raw == "all" {
        return Ok(None);
    }
    match raw.trim().parse::<f64>() {
        Ok(n) if n >= 1.0 && n.fract() == 0.0 && n.is_finite() => Ok(Some(n as usize)),
        _ => Err(format!(
            "--limit needs a positive whole number{}, got \"{raw}\"",
            if all { " or \"all\"" } else { "" }
        )),
    }
}

fn limit(flags: &Flags, fallback: usize) -> Result<usize, String> {
    Ok(limit_flag(flags, fallback, false)?.unwrap_or(fallback))
}

const HELP: &str = r#"sc-gamedata — extract Star Citizen game data into structured JSON

Usage:
  sc-gamedata <command> [domains...] [options]

Commands:
  extract          Read Data.p4k and write every dataset, the string table and
                   the search index (no StarBreaker, no extracted-data/)
  parse            Same as extract: this pipeline reads the archive directly
  all              Same as extract
  fetch            Fetch API-sourced datasets (no game install needed)
  strings          Search or look up localized strings
  overlays         List known community string packs
  overlays fetch   Download a pack by name
  domains          List available domains
  languages        List string-table languages
  builds           List builds available to diff (local parse, snapshots, published tags)
  snapshot         Save the current data/ as a snapshot of its build (parse does this)
  diff <a> <b>     What changed between two builds, for one or every dataset
  deflect <ship>   Which ship weapons get through a ship's stock armour
  changes <a> <b>  Buffs and nerfs between two builds as chart-ready CSV and JSON
  notes <a> <b>    Player-facing patch notes between two builds (Markdown or JSON)
  find [query]     Fuzzy-find any item or ship (fzf, with live preview and
                   action keys); prints ranked matches without fzf or a TTY
  obtain <item>    Where an item comes from: best loot areas and boxes, loot
                   tables, blueprints, mission/Wikelo awards, shops
  where <item>     The same sources as one short table, easiest first:
                   buy, then contract/mission reward or crafting, then loot
  shop <place>     Every item UEX lists at a terminal or place (station, city,
                   moon, planet, system), with price and last-confirmed date
  mcp-query        One SCMCP tool request (JSON on stdin), its reply on stdout
  engage <ship> [target]
                   A loadout's hull DPS on a target by distance, and its effective ranges
  publish          Commit, tag and push data/ to the dumps repo (prints the
                   gh release command, or writes it to --gh-script, instead of
                   running it). Always --dry-run first
  help             Show this message

Options:
  --p4k <path>          Path to Data.p4k        (env SC_DATA_P4K)
  --channel <name>      Game channel when auto-detecting (default LIVE)
  --extract-dir <path>  extract: where the _build.json marker goes (default:
                        --out); strings: read global.ini from an extracted
                        tree instead of Data.p4k
  --out <path>          Where datasets land     (default ./data)
  --uex-token <token>   UEX API token           (env UEXTOKEN)
  --language <name>     String table language   (default english)
  --key <name>          strings: exact key lookup, with or without "@"
  --limit <n>           strings: max results (default 25)
  --keys-only           strings: match keys only, not values
  --write               strings: also write the table as a dataset
  --overlay <path>      strings: layer an .ini file or directory (repeatable
                        with commas) over the base
  --pack <name>         strings: layer a fetched pack by name
  --installed           strings: layer the pack installed in the game folder
  --diff                strings: show what the overlay changes
  --item <text>         diff: one item by name or record name, with every
                        changed field
  --collection <name>   diff: one collection of the dataset (default: all)
  --fields <a,b>        diff: only changes under these fields (e.g. dps,alpha)
  --detail              diff: list changed fields for every item
  --limit <n>           diff: items listed per collection (default 20)
  --json                diff: machine-readable output
  --fetch               builds: fetch tags from the dumps repo first
  --size <n>            deflect: only weapons of this size
  --limit <n>           obtain: areas listed (default 15); find: matches printed
  --all                 find: include world furniture (cargo, doors, seats, ...)
  --list                find: print ranked matches instead of opening fzf
  --print               find: print the picked key(s) only, for scripts
  --json                obtain, where, shop: machine-readable report
  --at <place>          obtain, where: only shops at this terminal or place
                        ("grim hex", "GrimHEX", "Yela", "Orison")
  --dry-run             publish: list what would be published, write nothing
  --republish           publish: replace a build already published (same build)
  --channel <name>      publish: release channel when the extract marker has none
  --patch <x.y.z>       publish: patch named in the release title
  --out <path>          publish: the datasets to publish (default <checkout>/data)
  --gh-script <file>    publish: write the gh release steps to this shell script
                        instead of printing them (make publish runs it)
  --ammo                obtain, where: report on the magazine or canister the
                        matched weapon loads instead of the weapon
  --limit <n>|all       shop: items listed per terminal (default 25)
  --all                 shop: list every matched terminal, not a place list
                        past 15 terminals
  --export-dir <path>   changes: where to write (default ./exports)
  --min-change <pct>    notes: hide numeric changes under this percent (default 1)
  --write <file.md>     notes: also write the notes to a file (.json writes JSON)
  --json                notes: the notes document as JSON
  --target <ship>       engage: the ship being shot (or give it as a second name)
  --weapons <list>      engage: guns instead of the stock loadout, by record,
                        "name S<size>" or name, each optionally *count
  --target-speed <m/s>  engage: target speed (default its SCM speed)
  --closing <m/s>       engage: closing speed, negative when opening
  --accuracy <a,b,...>  engage: hit-chance levels in percent (default 90,75,50,25)
  --spread min|max      engage: first-shot or sustained spread (default max)
  --accel <g,g,...>     engage: target jink acceleration in g; the first sets
                        the distance table, all are tabulated (default 0,3,6,9,13;
                        not in game data)
  --reaction <s>        engage: seconds before the target starts jinking
  --hit-radius <m>      engage: override the target's hit radius
  --bands <a,b,...>     engage: distances in metres to tabulate

Examples:
  sc-gamedata all
  sc-gamedata all mining
  sc-gamedata extract --channel PTU
  sc-gamedata fetch
  sc-gamedata fetch uex-terminals
  sc-gamedata strings Quantainium
  sc-gamedata strings --key @manufacturer_NameAEGS
  sc-gamedata strings --write --language german_(germany)
  sc-gamedata overlays
  sc-gamedata overlays fetch starstrings
  sc-gamedata strings --pack starstrings --diff
  sc-gamedata strings --installed Ouratite
  sc-gamedata strings --overlay ./mypack,./extra.ini
  sc-gamedata builds
  sc-gamedata diff 1.0.191.28374 local weapons
  sc-gamedata diff 1.0.191.28374 local weapons --item "M7A"
  sc-gamedata diff 4.10 local weapons --fields dps,alpha,fireRate --detail
  sc-gamedata diff ./data-old ./data --json
  sc-gamedata deflect perseus --size 3
  sc-gamedata find
  sc-gamedata find verdict
  sc-gamedata find kastak lmg --list
  sc-gamedata obtain "Verdict LMG"
  sc-gamedata obtain ksar_pistol_energy_01 --json
  sc-gamedata obtain "Cambio SRT Canister" --at grimhex
  sc-gamedata obtain "Cambio SRT" --ammo --at grimhex
  sc-gamedata where "FS-9 LMG"
  sc-gamedata shop "grim hex"
  sc-gamedata changes 4.10 local
  sc-gamedata notes 4.10.197 local --write notes.md
  sc-gamedata engage gladius --target perseus
  sc-gamedata engage --weapons "M7A Cannon*2,AD5B Ballistic Gatling" --target "Hammerhead" --accel 6

A build is "local" (the --out directory), a directory of game-*.json, or a
tag in the dumps repo. A tag prefix such as 4.10 picks the newest matching
build.

Differences from the former TypeScript CLI (removed 2026-11-25): extract reads
the archive itself (parse is an alias, --starbreaker is gone); extract also
writes game-strings-english.json when no domain is named (make all's strings
step) and snapshots it with the datasets; strings reads Data.p4k unless
--extract-dir is given."#;

/// `path.resolve`.
fn resolve(p: &str) -> PathBuf {
    diff::resolve_path(Path::new(p))
}

fn out_dir(flags: &Flags) -> PathBuf {
    resolve(flags.get("out").map_or("data", String::as_str))
}

fn channel(flags: &Flags) -> &str {
    flags.get("channel").map_or("LIVE", String::as_str)
}

fn find_p4k(flags: &Flags) -> Result<PathBuf, String> {
    source::find_data_p4k(flags.get("p4k").map(Path::new), channel(flags))
}

/// `println!` panics when stdout is a closed pipe (`sc-gamedata find --list | head`). Node
/// stops quietly there; so does this, with the shell's SIGPIPE status.
fn quiet_broken_pipe() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = info
            .payload()
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| info.payload().downcast_ref::<&str>().copied())
            .unwrap_or("");
        if message.starts_with("failed printing to stdout") && message.contains("Broken pipe") {
            std::process::exit(141);
        }
        default(info);
    }));
}

fn main() -> ExitCode {
    quiet_broken_pipe();
    let argv: Vec<String> = std::env::args().skip(1).collect();
    match run(parse_args(&argv)) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("\nerror: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: Args) -> Result<ExitCode, String> {
    let Args {
        command,
        flags,
        rest,
    } = args;
    if command == "help" || flags.contains_key("help") {
        println!("{HELP}");
        return Ok(ExitCode::SUCCESS);
    }
    match command.as_str() {
        "domains" => list_domains(),
        "languages" => {
            println!("String-table languages (one global.ini each, no separate table):");
            for language in LANGUAGES {
                println!("  {language}");
            }
            Ok(ExitCode::SUCCESS)
        }
        "overlays" => run_overlays(&flags, &rest),
        "strings" => run_strings(&flags, &rest),
        "builds" => run_builds(&flags),
        "engage" => run_engage(&flags, &rest),
        "changes" => run_changes(&flags, &rest),
        "notes" => run_notes(&flags, &rest),
        "snapshot" => run_snapshot(&flags),
        "diff" => run_diff(&flags, &rest),
        "deflect" => run_deflect(&flags, &rest),
        "find" => run_find(&flags, &rest),
        "find-run" => run_find_action(&flags, &rest),
        "obtain" | "where" => run_obtain(&command, &flags, &rest),
        "shop" => run_shop(&flags, &rest),
        "mcp-query" => run_mcp_query(),
        "fetch" => run_fetch(&flags, &rest),
        "extract" | "all" | "parse" => run_extract(&command, &flags, &rest),
        "publish" => run_publish(&flags),
        _ => {
            eprintln!("Unknown command \"{command}\"\n");
            println!("{HELP}");
            Ok(ExitCode::FAILURE)
        }
    }
}

// ---------------------------------------------------------------- builds, diff, changes, notes

/// The checkout root (`repoRoot()`: dist/.. in the TypeScript): this crate's workspace root.
fn repo_root() -> PathBuf {
    diff::resolve_path(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
}

fn snapshots() -> PathBuf {
    diff::snapshots_dir(&repo_root())
}

/// JavaScript `Number(text)`: blank is 0, anything unreadable NaN.
fn js_number_of(text: &str) -> f64 {
    let t = text.trim();
    if t.is_empty() {
        return 0.0;
    }
    match t {
        "Infinity" | "+Infinity" => f64::INFINITY,
        "-Infinity" => f64::NEG_INFINITY,
        _ if t
            .chars()
            .any(|c| c.is_ascii_alphabetic() && c != 'e' && c != 'E') =>
        {
            f64::NAN
        }
        _ => t.parse().unwrap_or(f64::NAN),
    }
}

/// `[from, to]` positionals, or the TypeScript's error.
fn two_builds<'a>(rest: &'a [String], usage: &str) -> Result<(&'a str, &'a str), String> {
    match rest {
        [from, to, ..] => Ok((from, to)),
        _ => Err(usage.into()),
    }
}

/// `resolveSource` for both builds, against `--out`, the snapshots and the dumps checkout.
fn sources(flags: &Flags, from: &str, to: &str) -> Result<(BuildSource, BuildSource), String> {
    let data_dir = out_dir(flags);
    let repo = diff::dumps_repo_options(&repo_root());
    let snaps = snapshots();
    let get = |spec: &str| {
        diff::resolve_source(spec, &data_dir, &repo, Some(&snaps)).map_err(|e| e.to_string())
    };
    Ok((get(from)?, get(to)?))
}

fn run_builds(flags: &Flags) -> Result<ExitCode, String> {
    let data_dir = out_dir(flags);
    let repo = diff::dumps_repo_options(&repo_root());
    let local_files: Vec<String> = std::fs::read_dir(&data_dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|f| f.starts_with("game-") && f.ends_with(".json"))
        .collect();
    let mut local_builds: Vec<String> = Vec::new();
    for file in &local_files {
        if let Some(b) = read_json(&data_dir.join(file))
            .and_then(|v| v.get("_build")?.as_str().map(str::to_string))
            && !local_builds.contains(&b)
        {
            local_builds.push(b);
        }
    }
    println!("local ({})", data_dir.display());
    if local_files.is_empty() {
        println!("  no datasets; run parse first");
    } else {
        let builds = if local_builds.is_empty() {
            "unknown".to_string()
        } else {
            local_builds.join(", ")
        };
        println!("  {} dataset(s), build {builds}", local_files.len());
    }
    let snaps_dir = snapshots();
    let snaps = diff::list_snapshots(&snaps_dir).map_err(|e| e.to_string())?;
    println!("\nsnapshots ({})", snaps_dir.display());
    if snaps.is_empty() {
        println!("  none yet; every parse saves one");
    }
    for s in &snaps {
        println!(
            "  {}  {} dataset(s)  {}",
            s.build,
            s.datasets,
            js_slice(&s.saved, 10)
        );
    }
    let published = diff::list_published_builds(&repo, flags.contains_key("fetch"))
        .map_err(|e| e.to_string())?;
    println!("\npublished ({})", repo.work_dir.display());
    if published.is_empty() {
        println!("  none found; pass --fetch to clone or update the dumps checkout");
    }
    for b in &published {
        println!("  {}  {}", b.version, b.date);
    }
    Ok(ExitCode::SUCCESS)
}

/// `/"_build":\s*"([^"]+)"/` over the first 4096 characters of a dataset.
fn build_stamp(text: &str) -> Option<String> {
    let head = js_slice(text, 4096);
    let mut from = 0;
    while let Some(i) = head[from..].find("\"_build\":") {
        let start = from + i;
        let rest = head[start + 9..].trim_start();
        if let Some(value) = rest.strip_prefix('"')
            && let Some(end) = value.find('"')
            && end > 0
        {
            return Some(value[..end].to_string());
        }
        from = start + 1;
    }
    None
}

fn run_snapshot(flags: &Flags) -> Result<ExitCode, String> {
    let data_dir = out_dir(flags);
    let entries =
        std::fs::read_dir(&data_dir).map_err(|e| format!("{}: {e}", data_dir.display()))?;
    let mut stamps: Vec<String> = Vec::new();
    for e in entries.flatten() {
        let Ok(name) = e.file_name().into_string() else {
            continue;
        };
        if !(name.starts_with("game-") && name.ends_with(".json")) {
            continue;
        }
        let text = String::from_utf8_lossy(
            &std::fs::read(e.path()).map_err(|err| format!("{}: {err}", e.path().display()))?,
        )
        .into_owned();
        if let Some(stamp) = build_stamp(&text)
            && !stamps.contains(&stamp)
        {
            stamps.push(stamp);
        }
    }
    if stamps.is_empty() {
        return Err(format!(
            "No build-stamped datasets in {}; run parse first.",
            data_dir.display()
        ));
    }
    for build in &stamps {
        let kept =
            diff::save_snapshot(&data_dir, &snapshots(), build, None).map_err(|e| e.to_string())?;
        println!(
            "saved {build}: {} dataset(s) in {}",
            kept.datasets,
            kept.path.display()
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn run_diff(flags: &Flags, rest: &[String]) -> Result<ExitCode, String> {
    let (from, to) = two_builds(
        rest,
        "diff needs two builds: sc-gamedata diff <from> <to> [dataset]. Use \"local\" for the parse output, a directory, or a published tag (see \"builds\").",
    )?;
    let (from, to) = sources(flags, from, to)?;
    let fields: Option<Vec<String>> = flags
        .get("fields")
        .map(|f| {
            f.split(',')
                .map(str::trim)
                .filter(|f| !f.is_empty())
                .map(str::to_string)
                .collect()
        })
        .filter(|f: &Vec<String>| !f.is_empty());
    let limit = limit(flags, 20)?;
    let options = DiffOptions {
        item: flags.get("item").cloned(),
        collection: flags.get("collection").cloned(),
        fields,
        detail: flags.contains_key("detail"),
        limit: Some(limit),
    };
    let results = diff::diff(&from, &to, rest.get(2).map(String::as_str), &options)
        .map_err(|e| e.to_string())?;
    if flags.contains_key("json") {
        println!("{}", diff::diff_json(&results));
    } else {
        print!("{}", diff::render_diffs(&results, Some(limit)));
    }
    Ok(ExitCode::SUCCESS)
}

fn run_changes(flags: &Flags, rest: &[String]) -> Result<ExitCode, String> {
    let (from_spec, to_spec) = two_builds(
        rest,
        "changes needs two builds: sc-gamedata changes <from> <to>",
    )?;
    let (from, to) = sources(flags, from_spec, to_spec)?;
    let result = changes::changes(&from, &to).map_err(|e| e.to_string())?;
    let label = changes::export_label(&result, from_spec, to_spec);
    let dir = resolve(flags.get("export-dir").map_or("exports", String::as_str))
        .join("changes")
        .join(label);
    changes::export_changes(&result, &dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    if flags.contains_key("json") {
        println!("{}", changes::changes_json(&result));
        return Ok(ExitCode::SUCCESS);
    }
    print!(
        "{}",
        changes::render_changes(
            &result,
            from_spec,
            to_spec,
            limit(flags, 10)?,
            &dir.display().to_string()
        )
    );
    Ok(ExitCode::SUCCESS)
}

fn run_notes(flags: &Flags, rest: &[String]) -> Result<ExitCode, String> {
    // --write is a boolean flag for `strings`, so its file lands as the third positional.
    let (from_spec, to_spec) = two_builds(
        rest,
        "notes needs two builds: sc-gamedata notes <from> <to> [--json] [--min-change <pct>] [--write <file.md>]",
    )?;
    let min_change = flags.get("min-change").map_or(1.0, |m| js_number_of(m));
    if !min_change.is_finite() || min_change < 0.0 {
        return Err("--min-change takes a percent, e.g. --min-change 2".into());
    }
    let (from, to) = sources(flags, from_spec, to_spec)?;
    let doc = notes::notes(&from, &to, &NotesOptions { min_change }).map_err(|e| e.to_string())?;
    let json = notes::notes_json(&doc);
    let markdown = notes::render_notes(&doc);
    if flags.contains_key("write") {
        let Some(target) = rest.get(2) else {
            return Err(
                "--write needs a file: --write notes.md (or notes.json for the JSON document)"
                    .into(),
            );
        };
        let file = resolve(target);
        let text = if file.to_string_lossy().ends_with(".json") {
            format!("{json}\n")
        } else {
            markdown.clone()
        };
        std::fs::write(&file, text).map_err(|e| format!("{}: {e}", file.display()))?;
        eprintln!("wrote {}", file.display());
    }
    println!(
        "{}",
        if flags.contains_key("json") {
            &json
        } else {
            &markdown
        }
    );
    Ok(ExitCode::SUCCESS)
}

// ---------------------------------------------------------------- publish

/// The script's own argument rules (`process.argv.includes` / the token after a flag), on
/// the raw arguments, so `--patch --dry-run` reads as the script reads it.
struct PublishArgs {
    dry_run: bool,
    republish: bool,
    channel: Option<String>,
    patch: Option<String>,
}

fn publish_args(argv: &[String]) -> Result<PublishArgs, ExitCode> {
    let after = |flag: &str| {
        argv.iter()
            .position(|a| a == flag)
            .map(|i| argv.get(i + 1).cloned())
    };
    let patch = match after("--patch") {
        None => None,
        Some(value) => {
            let ok = value.as_deref().is_some_and(|v| {
                let parts: Vec<&str> = v.split('.').collect();
                parts.len() >= 2
                    && parts
                        .iter()
                        .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
            });
            if !ok {
                eprintln!(
                    "--patch needs a version like 4.10.3, got: {}",
                    value.as_deref().unwrap_or("(nothing)")
                );
                return Err(ExitCode::FAILURE);
            }
            value
        }
    };
    Ok(PublishArgs {
        dry_run: argv.iter().any(|a| a == "--dry-run"),
        republish: argv.iter().any(|a| a == "--republish"),
        channel: after("--channel").flatten().map(|c| c.to_uppercase()),
        patch,
    })
}

/// A shell command line, each argument quoted when it needs to be.
fn shell_line(args: &[String]) -> String {
    args.iter()
        .map(|a| {
            if !a.is_empty()
                && a.chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-_.=/:@+,".contains(c))
            {
                a.clone()
            } else {
                finder::shell_quote(a)
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// `gh release create` exactly as `ensureRelease` runs it.
fn release_create_line(plan: &publish::PublishPlan) -> String {
    let mut args: Vec<String> = [
        "gh",
        "release",
        "create",
        &plan.build,
        "-R",
        &plan.repo,
        "--verify-tag",
        "--title",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    args.push(plan.title());
    args.push("--notes".into());
    args.push(publish::release_notes(
        &plan.build,
        &plan.origin,
        plan.patch.as_deref(),
        &plan.dumps,
    ));
    args.extend(
        publish::release_flags(&plan.origin)
            .iter()
            .map(|f| f.to_string()),
    );
    shell_line(&args)
}

/// `gh release edit` as the republish path runs it.
fn release_edit_line(plan: &publish::PublishPlan) -> String {
    let args: Vec<String> = vec![
        "gh".into(),
        "release".into(),
        "edit".into(),
        plan.build.clone(),
        "-R".into(),
        plan.repo.clone(),
        "--title".into(),
        plan.title(),
        "--notes".into(),
        publish::release_notes(
            &plan.build,
            &plan.origin,
            plan.patch.as_deref(),
            &plan.dumps,
        ),
    ];
    shell_line(&args)
}

/// `gh release upload <tag> build.json --clobber`, run from the dumps checkout, so the
/// release carries the tag's manifest as an asset.
fn release_upload_line(plan: &publish::PublishPlan) -> String {
    shell_line(&[
        "gh".into(),
        "release".into(),
        "upload".into(),
        plan.build.clone(),
        "build.json".into(),
        "--clobber".into(),
        "-R".into(),
        plan.repo.clone(),
    ])
}

/// The upload, only when the checkout is at the tag (its `build.json` is then the tag's
/// blob; after an "already published" stop, main may have moved on to a later build).
fn release_upload_block(plan: &publish::PublishPlan) -> String {
    let work_dir = plan.work_dir.to_string_lossy().into_owned();
    let cd = shell_line(&["cd".into(), work_dir]);
    let tag_commit = shell_line(&[format!("{}^{{commit}}", plan.build)]);
    let skip = shell_line(&[
        "echo".into(),
        format!(
            "build.json: the checkout is not at {}; release asset not uploaded.",
            plan.build
        ),
    ]);
    format!(
        "{cd}\nif [ \"$(git rev-parse HEAD)\" = \"$(git rev-parse {tag_commit})\" ]; then\n  {}\nelse\n  {skip}\nfi\n",
        release_upload_line(plan)
    )
}

/// The tool repos told about a publish, `owner/repo` each: `$SC_TOOL_REPOS` (space
/// separated), else snares, MCsim and Bindscape (scsticks).
fn tool_repos() -> Vec<String> {
    std::env::var("SC_TOOL_REPOS")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .map(|v| v.split_whitespace().map(str::to_string).collect())
        .unwrap_or_else(|| {
            ["voidput/snares", "voidput/MCsim", "voidput/scsticks"]
                .map(str::to_string)
                .to_vec()
        })
}

/// `gh workflow run data-sync.yml` per tool repo for a LIVE or PTU build, so each tool
/// re-bakes from the new tag without a dispatch secret in the dumps repo. Other channels
/// are not dispatched. A repo whose default branch has no `data-sync.yml` yet fails its
/// line only: the publish has already happened. `SC_NO_DISPATCH=1` skips the block.
fn dispatch_block(plan: &publish::PublishPlan, repos: &[String]) -> String {
    let channel = plan.origin.channel.as_str();
    if channel != "LIVE" && channel != "PTU" {
        return format!("echo {}\n", shell_line(&[format!(
            "{channel} builds are not dispatched to the tool repos."
        )]));
    }
    let mut block = String::from("if [ -z \"${SC_NO_DISPATCH:-}\" ]; then\n");
    for repo in repos {
        let run = shell_line(&[
            "gh".into(),
            "workflow".into(),
            "run".into(),
            "data-sync.yml".into(),
            "-R".into(),
            repo.clone(),
            "-f".into(),
            format!("tag={}", plan.build),
            "-f".into(),
            format!("channel={channel}"),
        ]);
        let ok = shell_line(&["echo".into(), format!("dispatched data-sync to {repo}")]);
        let fail = shell_line(&[
            "echo".into(),
            format!("dispatch to {repo} failed (is data-sync.yml on its default branch?)"),
        ]);
        block.push_str(&format!("  if {run}; then {ok}; else {fail}; fi\n"));
    }
    block.push_str("fi\n");
    block
}

fn print_release_steps(plan: &publish::PublishPlan, edit: bool) {
    println!("\nrelease (run yourself; this tool does not call gh):");
    if edit {
        println!("{}", release_edit_line(plan));
    }
    println!(
        "# unless `gh release view {} -R {}` already finds one:",
        plan.build, plan.repo
    );
    println!("{}", release_create_line(plan));
    println!("# then, from {}:", plan.work_dir.display());
    println!("{}", release_upload_line(plan));
    println!("# then tell the tool repos:");
    print!("{}", dispatch_block(plan, &tool_repos()));
}

/// The script's GitHub release steps as a POSIX shell script, for `make publish` to run:
/// `gh release edit` first when the build was republished, then `ensureRelease` (create
/// the release unless `gh release view` finds one), then `gh release upload <tag>
/// build.json --clobber` from the dumps checkout. This tool never runs it.
fn release_script(plan: &publish::PublishPlan, edit: bool) -> String {
    let view = shell_line(&[
        "gh".into(),
        "release".into(),
        "view".into(),
        plan.build.clone(),
        "-R".into(),
        plan.repo.clone(),
    ]);
    let exists = shell_line(&[
        "echo".into(),
        format!("Release {} already exists.", plan.build),
    ]);
    let mut script = format!(
        "#!/bin/sh\n# GitHub release for {}, written by `sc-gamedata publish --gh-script`.\nset -e\n",
        plan.build
    );
    if edit {
        script.push_str(&release_edit_line(plan));
        script.push('\n');
    }
    script.push_str(&format!(
        "if {view} >/dev/null 2>&1; then\n  {exists}\nelse\n  {}\nfi\n",
        release_create_line(plan)
    ));
    script.push_str(&release_upload_block(plan));
    script.push_str(&dispatch_block(plan, &tool_repos()));
    script
}

/// The release steps: printed for the user to run, or written to `--gh-script <file>`.
fn release_steps(
    plan: &publish::PublishPlan,
    edit: bool,
    gh_script: Option<&Path>,
) -> Result<(), String> {
    let Some(file) = gh_script else {
        print_release_steps(plan, edit);
        return Ok(());
    };
    if let Some(dir) = file.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(file, release_script(plan, edit))
        .map_err(|e| format!("{}: {e}", file.display()))?;
    println!("\nrelease steps written to {}", file.display());
    Ok(())
}

fn is_notes_warning(w: &str) -> bool {
    w.starts_with("Notes:  skipped (")
}

fn run_publish(flags: &Flags) -> Result<ExitCode, String> {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match publish_args(&argv) {
        Ok(a) => a,
        Err(code) => return Ok(code),
    };
    let options = publish::PublishOptions {
        root: repo_root(),
        data_dir: flags.get("out").map(|o| resolve(o)),
        channel: args.channel.clone(),
        patch: args.patch.clone(),
        republish: args.republish,
        ..Default::default()
    };
    let gh_script = flags.get("gh-script").map(|f| resolve(f));
    let plan = publish::plan(&options).map_err(|e| e.to_string())?;
    // The script warns about the channel while resolving it, before the listing, and
    // about skipped notes after it.
    for w in plan.warnings.iter().filter(|w| !is_notes_warning(w)) {
        eprintln!("{w}");
    }
    // The listing alone: the dry-run text without notes and without its last line.
    let listing = {
        let mut bare = plan.clone();
        bare.notes = publish::PublishNotes::Skipped(String::new());
        let text = publish::render_dry_run(&bare);
        text.strip_suffix("Dry run — nothing written, nothing pushed.\n")
            .unwrap_or(&text)
            .to_string()
    };
    if args.dry_run {
        let text = publish::render_dry_run(&plan);
        print!("{listing}");
        for w in plan.warnings.iter().filter(|w| is_notes_warning(w)) {
            eprintln!("{w}");
        }
        print!("{}", text.strip_prefix(listing.as_str()).unwrap_or(&text));
        return Ok(ExitCode::SUCCESS);
    }
    print!("{listing}");
    let outcome = match publish::stage(&plan, &plan.work_dir, true) {
        Ok(o) => o,
        // The script prints these itself and exits 1, without an error prefix.
        Err(diff::Error::Message(m))
            if m.contains(" is identical to what is already published") =>
        {
            eprintln!("{m}");
            return Ok(ExitCode::FAILURE);
        }
        Err(e) => return Err(e.to_string()),
    };
    match outcome {
        publish::StageOutcome::AlreadyPublished => {
            println!(
                "Build {} is already published as a tag. Pass --republish to replace it.",
                plan.build
            );
            release_steps(&plan, false, gh_script.as_deref())?;
        }
        publish::StageOutcome::Staged {
            republished, notes, ..
        } => {
            match &notes {
                publish::PublishNotes::Skipped(why) => eprintln!("Notes:  skipped ({why})"),
                publish::PublishNotes::Written { from, files } => {
                    println!("Notes:  against {from}");
                    for f in files {
                        println!(
                            "  {} {}",
                            js_pad_end(&format!("notes/{}", f.name), 32),
                            deflection::pad_start(&publish::mb(f.body.len()), 9)
                        );
                    }
                    println!();
                }
            }
            release_steps(&plan, republished, gh_script.as_deref())?;
            println!(
                "\n{} {} to {}.",
                if republished {
                    "Republished"
                } else {
                    "Published"
                },
                plan.build,
                plan.repo
            );
        }
    }
    Ok(ExitCode::SUCCESS)
}

// ---------------------------------------------------------------- deflect, engage

fn load_rows(data_dir: &Path, file: &str) -> Result<Value, String> {
    let path = data_dir.join(file);
    let bytes = std::fs::read(&path).map_err(|e| {
        format!(
            "ENOENT: no such file or directory, open '{}' ({e})",
            path.display()
        )
    })?;
    serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))
}

fn rows<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    v.get(key)
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

fn text(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}

fn is_truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|x| x != 0.0 && !x.is_nan()),
        Some(Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

/// `/_(ai|pu|ea|template|pir|crim|unmanned)/i`: AI, template and event variants.
fn variant_record(record: &str) -> bool {
    let r = record.to_lowercase();
    [
        "_ai",
        "_pu",
        "_ea",
        "_template",
        "_pir",
        "_crim",
        "_unmanned",
    ]
    .iter()
    .any(|p| r.contains(p))
}

fn run_deflect(flags: &Flags, rest: &[String]) -> Result<ExitCode, String> {
    let wanted = rest.join(" ").trim().to_lowercase();
    if wanted.is_empty() {
        return Err("deflect needs a ship: sc-gamedata deflect <name or record name>".into());
    }
    let data_dir = out_dir(flags);
    let ships = load_rows(&data_dir, "game-ships.json")?;
    let weapons = load_rows(&data_dir, "game-weapons.json")?;
    let armored: Vec<&Value> = rows(&ships, "vehicles")
        .iter()
        .filter(|v| is_truthy(v.get("armor")))
        .collect();
    let exact: Vec<&Value> = armored
        .iter()
        .copied()
        .filter(|v| {
            text(v, "name").to_lowercase() == wanted
                || text(v, "recordName")
                    .to_lowercase()
                    .ends_with(&format!(".{wanted}"))
        })
        .collect();
    let matches = if exact.is_empty() {
        armored
            .iter()
            .copied()
            .filter(|v| {
                format!("{} {}", text(v, "name"), text(v, "recordName"))
                    .to_lowercase()
                    .contains(&wanted)
            })
            .collect()
    } else {
        exact
    };
    if matches.is_empty() {
        return Err(format!("No ship with armour matches \"{wanted}\"."));
    }
    // Prefer the player hull over AI, template and event variants of the same name.
    let ship = matches
        .iter()
        .copied()
        .find(|v| !is_truthy(v.get("isTemplate")) && !variant_record(&text(v, "recordName")))
        .unwrap_or(matches[0]);
    let size = flags.get("size").map(|s| js_number_of(s));
    let armor = ship.get("armor").cloned().unwrap_or(Value::Null);
    let found = deflection::deflection_check(&armor, rows(&weapons, "shipWeapons"), size);
    if flags.contains_key("json") {
        let v = json!({
            "ship": ship.get("recordName").cloned().unwrap_or(Value::Null),
            "armor": armor,
            "rows": found.iter().map(deflection::DeflectionRow::to_value).collect::<Vec<_>>(),
        });
        println!("{}", output::to_js_json(&v));
        return Ok(ExitCode::SUCCESS);
    }
    if matches.len() > 1 {
        println!(
            "{} ships match; showing {}. Use the record name to pick another.\n",
            matches.len(),
            text(ship, "recordName")
        );
    }
    println!(
        "{}",
        deflection::render_deflection(&text(ship, "name"), &armor, &found, limit(flags, 30)?)
    );
    Ok(ExitCode::SUCCESS)
}

fn short_record(r: &str) -> String {
    r.strip_prefix("EntityClassDefinition.")
        .unwrap_or(r)
        .to_lowercase()
}

/// `/^(.*)\s+s(\d+)$/` on a lower-cased weapon spec: the name and size of `"m7a cannon s5"`.
fn sized_spec(q: &str) -> Option<(&str, f64)> {
    let digits = q.len() - q.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    if digits == 0 {
        return None;
    }
    let head = q[..q.len() - digits].strip_suffix('s')?;
    let last = head.chars().next_back()?;
    if !last.is_whitespace() {
        return None;
    }
    // Greedy `(.*)` keeps every whitespace character but the last before "s".
    let name = &head[..head.len() - last.len_utf8()];
    Some((name, q[q.len() - digits..].parse().ok()?))
}

fn run_engage(flags: &Flags, rest: &[String]) -> Result<ExitCode, String> {
    let data_dir = out_dir(flags);
    let ships = load_rows(&data_dir, "game-ships.json")?;
    let arsenal = load_rows(&data_dir, "game-weapons.json")?;
    let ship_weapons = rows(&arsenal, "shipWeapons");
    let all_weapons: Vec<&Value> = ship_weapons
        .iter()
        .chain(rows(&arsenal, "fpsWeapons"))
        .filter(|w| !is_truthy(w.get("isTemplate")))
        .collect();
    let vehicles = rows(&ships, "vehicles");

    let find_ship = |wanted: &str| -> Result<&Value, String> {
        let q = wanted.to_lowercase();
        let pool: Vec<&Value> = vehicles
            .iter()
            .filter(|v| !is_truthy(v.get("isTemplate")))
            .collect();
        let exact: Vec<&Value> = pool
            .iter()
            .copied()
            .filter(|v| {
                text(v, "name").to_lowercase() == q || short_record(&text(v, "recordName")) == q
            })
            .collect();
        let matches = if exact.is_empty() {
            pool.iter()
                .copied()
                .filter(|v| {
                    format!("{} {}", text(v, "name"), text(v, "recordName"))
                        .to_lowercase()
                        .contains(&q)
                })
                .collect()
        } else {
            exact
        };
        if matches.is_empty() {
            return Err(format!("No ship matches \"{wanted}\"."));
        }
        Ok(matches
            .iter()
            .copied()
            .find(|v| !variant_record(&text(v, "recordName")) && is_truthy(v.get("armor")))
            .unwrap_or(matches[0]))
    };
    let find_weapon = |spec: &str| -> Result<&Value, String> {
        let q = spec.trim().to_lowercase();
        let name = |w: &Value| text(w, "name").to_lowercase();
        all_weapons
            .iter()
            .copied()
            .find(|w| short_record(&text(w, "recordName")) == q)
            .or_else(|| {
                let (n, size) = sized_spec(&q)?;
                all_weapons.iter().copied().find(|w| {
                    name(w) == n
                        && deflection::J::of(w.get("size")).strict_eq(deflection::J::V(size))
                })
            })
            .or_else(|| all_weapons.iter().copied().find(|w| name(w) == q))
            .or_else(|| all_weapons.iter().copied().find(|w| name(w).contains(&q)))
            .ok_or_else(|| {
                format!("No weapon matches \"{spec}\". Use a record name, or \"name S<size>\".")
            })
    };

    let attacker_name = rest.first();
    let target_arg = rest.get(1..).map(|r| r.join(" ")).unwrap_or_default();
    let target_name = flags
        .get("target")
        .cloned()
        .or_else(|| (!target_arg.is_empty()).then_some(target_arg));
    let label: String;
    let mut loadout: Vec<Gun> = Vec::new();
    if flags.contains_key("weapons") {
        for part in flags["weapons"].split(',').filter(|x| !x.trim().is_empty()) {
            let mut bits = part.split('*');
            let spec = bits.next().unwrap_or("");
            let count = bits
                .next()
                .filter(|c| !c.is_empty())
                .map_or(1.0, js_number_of);
            loadout.push(Gun {
                weapon: find_weapon(spec)?,
                count,
                mount: None,
            });
        }
        label = match attacker_name {
            Some(a) => format!("{} (custom guns)", text(find_ship(a)?, "name")),
            None => "custom loadout".into(),
        };
    } else {
        let Some(attacker_name) = attacker_name else {
            return Err("engage needs a ship for its stock loadout, or --weapons.".into());
        };
        let attacker = find_ship(attacker_name)?;
        let name = text(attacker, "name");
        label = format!("{name} stock");
        let entries = attacker
            .get("loadout")
            .and_then(|l| l.get("weapons"))
            .and_then(Value::as_array)
            .map_or(&[][..], Vec::as_slice);
        for entry in entries {
            let item = text(entry, "item").to_lowercase();
            let weapon = ship_weapons
                .iter()
                .find(|w| short_record(&text(w, "recordName")) == item);
            if let Some(w) =
                weapon.filter(|w| w.get("type").and_then(Value::as_str) == Some("WeaponGun"))
            {
                loadout.push(Gun {
                    weapon: w,
                    count: deflection::J::of(entry.get("count"))
                        .or(deflection::J::V(1.0))
                        .num(),
                    mount: entry
                        .get("mount")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                });
            }
        }
        if loadout.is_empty() {
            return Err(format!("{name} has no stock guns in the data."));
        }
    }
    let mut target: Option<Target> = match &target_name {
        Some(t) => Some(engagement::target_of(find_ship(t)?)),
        None => None,
    };
    if let Some(t) = target.as_mut()
        && let Some(r) = flags.get("hit-radius")
    {
        t.hit_radius = Some(js_number_of(r));
    }
    let accel_list: Vec<f64> = flags
        .get("accel")
        .map_or("0,3,6,9,13", String::as_str)
        .split(',')
        .map(js_number_of)
        .filter(|n| n.is_finite() && *n >= 0.0)
        .collect();
    let number_flag = |name: &str| flags.get(name).map(|v| js_number_of(v));
    let options = EngageOptions {
        accel: accel_list.first().map(|g| g * engagement::G),
        reaction: number_flag("reaction"),
        target_speed: number_flag("target-speed"),
        closing: number_flag("closing"),
        spread_min: flags.get("spread").map(String::as_str) == Some("min"),
        bands: flags.get("bands").map(|b| {
            b.split(',')
                .map(js_number_of)
                .filter(|n| n.is_finite())
                .collect()
        }),
        accuracy: flags.get("accuracy").map(|a| {
            a.split(',')
                .map(|n| js_number_of(n) / 100.0)
                .filter(|n| *n > 0.0 && *n <= 1.0)
                .collect()
        }),
    };
    let result = engagement::envelope(&loadout, target.as_ref(), &options);
    let table = target
        .as_ref()
        .map(|t| engagement::range_table(&loadout, t, &accel_list, &options));
    if flags.contains_key("json") {
        let mut v = Map::new();
        v.insert("attacker".into(), json!(label));
        v.extend(result.fields());
        if let Some(t) = &table {
            v.insert(
                "rangeByAccel".into(),
                Value::Array(t.iter().map(engagement::RangeRow::to_value).collect()),
            );
        }
        println!("{}", output::to_js_json(&Value::Object(v)));
        return Ok(ExitCode::SUCCESS);
    }
    println!(
        "{}",
        engagement::render_envelope(&label, &result, &options, target.as_ref(), table.as_deref())
    );
    Ok(ExitCode::SUCCESS)
}

// ---------------------------------------------------------------- domains, languages

/// `LANGUAGES`: one `Data/Localization/<language>/global.ini` each.
const LANGUAGES: &[&str] = &[
    "english",
    "german_(germany)",
    "french_(france)",
    "italian_(italy)",
    "spanish_(latin_america)",
    "spanish_(spain)",
    "portuguese_(brazil)",
    "polish_(poland)",
    "korean_(south_korea)",
    "japanese_(japan)",
    "chinese_(simplified)",
    "chinese_(traditional)",
];

/// `DOMAINS` order in `pipeline.ts`, which the default run and `domains` follow.
const TS_ORDER: &[&str] = &[
    "game-mining",
    "game-manufacturers",
    "game-ship-components",
    "game-blueprints",
    "game-wikelo-trades",
    "game-missions",
    "game-ships",
    "game-ammo",
    "game-fps-weapons",
    "game-starmap",
    "game-reputation",
    "game-containers",
    "game-mining-spawns",
    "game-weapons",
    "game-commodities",
    "game-refining",
    "game-shops",
    "game-cargo",
    "game-salvage",
    "game-quantum",
    "game-armor",
    "game-loot",
    "game-item-tags",
    "game-item-sources",
    "game-law",
    "game-consumables",
    "game-fire",
    "game-keybinds",
    "game-attributes",
    "game-npc-loadouts",
];

/// Every registered domain, in `pipeline.ts` order (any the order misses go last).
fn all_domains() -> Vec<&'static dyn Domain> {
    let mut all = domains::all();
    all.sort_by_key(|d| {
        TS_ORDER
            .iter()
            .position(|n| *n == d.name())
            .unwrap_or(usize::MAX)
    });
    all
}

/// `selectDomains`: every domain, or those named (`mining` or `game-mining`), in the order
/// given.
fn select_domains(names: &[String]) -> Result<Vec<&'static dyn Domain>, String> {
    let all = all_domains();
    if names.is_empty() {
        return Ok(all);
    }
    names
        .iter()
        .map(|name| {
            all.iter()
                .copied()
                .find(|d| d.name() == name || d.name() == format!("game-{name}"))
                .ok_or_else(|| {
                    let known: Vec<&str> = all.iter().map(|d| d.name()).collect();
                    format!("Unknown domain \"{name}\". Known: {}", known.join(", "))
                })
        })
        .collect()
}

fn list_domains() -> Result<ExitCode, String> {
    println!("From game files (needs a Star Citizen install):");
    for d in all_domains() {
        println!("  {}\n      {}", d.name(), d.description());
        // A domain built only from archive files has no record filters (`cli.ts`).
        if !d.filters().is_empty() {
            println!("      records: {}", d.filters().join(", "));
        }
        if d.filters().is_empty() && !d.files().is_empty() {
            println!("      files:   {}", d.files().join(", "));
        }
    }
    println!("\nFrom APIs (needs network access):");
    for d in &uex_market::API_DOMAINS {
        println!("  {}\n      {}", d.name, d.description);
    }
    Ok(ExitCode::SUCCESS)
}

// ---------------------------------------------------------------- HTTP

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .user_agent(format!("sc-gamedata/{}", env!("CARGO_PKG_VERSION")))
        .timeout_global(Some(std::time::Duration::from_secs(300)))
        .build()
        .into()
}

/// GET `url` (with a bearer token when given); any status is a response.
fn http_get(agent: &ureq::Agent, url: &str, token: Option<&str>) -> Result<HttpResponse, String> {
    let mut request = agent.get(url);
    if let Some(t) = token {
        request = request.header("Authorization", format!("Bearer {t}"));
    }
    let mut response = request.call().map_err(|e| format!("{url}: {e}"))?;
    let status = response.status().as_u16();
    let body = response
        .body_mut()
        .with_config()
        .limit(1 << 30)
        .read_to_string()
        .map_err(|e| format!("{url}: {e}"))?;
    Ok(HttpResponse { status, body })
}

// ---------------------------------------------------------------- fetch

fn run_fetch(flags: &Flags, rest: &[String]) -> Result<ExitCode, String> {
    let selected = uex_market::select_api_domains(rest);
    if selected.is_empty() {
        let known: Vec<&str> = uex_market::API_DOMAINS.iter().map(|d| d.name).collect();
        return Err(format!(
            "No API dataset matched. Known: {}",
            known.join(", ")
        ));
    }
    println!("fetching {} API dataset(s)", selected.len());
    let options = UexOptions {
        token: flags.get("uex-token").cloned(),
        ..Default::default()
    };
    let agent = agent();
    let mut http = |url: &str, token: Option<&str>| http_get(&agent, url, token);
    let written = uex_market::fetch_api_datasets(
        &out_dir(flags),
        &selected,
        &options,
        &mut http,
        &mut |m| println!("{m}"),
    )?;
    println!("\nwrote {} dataset(s)", written.len());
    for w in &written {
        let parts: Vec<String> = w
            .collections
            .iter()
            .map(|(k, n)| format!("{k}={n}"))
            .collect();
        println!("  {}  {}", w.dataset, parts.join(" "));
    }
    Ok(ExitCode::SUCCESS)
}

// ---------------------------------------------------------------- overlays

/// `loadSources()` reads `overlay-sources.json` in the working directory.
fn config_dir() -> &'static Path {
    Path::new("")
}

fn run_overlays(flags: &Flags, rest: &[String]) -> Result<ExitCode, String> {
    let sources = overlays::load_sources(config_dir())?;
    let names = || {
        sources
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };
    if rest.first().map(String::as_str) != Some("fetch") {
        println!("Known string packs:\n");
        for s in &sources {
            println!("  {}", s.name);
            if !s.description.is_empty() {
                println!("      {}", s.description);
            }
            if let Some(h) = s.homepage.as_deref().filter(|h| !h.is_empty()) {
                println!("      {h}");
            }
            println!("      {} file(s)", s.files.len());
        }
        println!(
            "\nAdd your own in {} as {{name, description, files:[url]}}.",
            overlays::sources_file(config_dir()).display()
        );
        println!("Any pack also works without registering: --overlay <file or dir>.");
        return Ok(ExitCode::SUCCESS);
    }
    let Some(wanted) = rest.get(1) else {
        return Err(format!("Name a pack to fetch. Known: {}", names()));
    };
    let Some(source) = sources.iter().find(|s| &s.name == wanted) else {
        return Err(format!("Unknown pack \"{wanted}\". Known: {}", names()));
    };
    let out = match flags.get("out") {
        Some(o) => resolve(o),
        None => resolve(&format!("overlays/{}", source.name)),
    };
    println!(
        "fetching {}{}",
        source.name,
        source
            .homepage
            .as_deref()
            .filter(|h| !h.is_empty())
            .map(|h| format!(" from {h}"))
            .unwrap_or_default()
    );
    let agent = agent();
    let written = overlays::fetch_source(source, &out, |url| {
        http_get(&agent, url, None).map(|r| FetchResponse {
            status: r.status,
            text: r.body,
        })
    })?;
    for w in &written {
        println!(
            "  {}  {:.1} KB",
            w.file.file_name().unwrap_or_default().to_string_lossy(),
            w.bytes as f64 / 1024.0
        );
    }
    println!("\nwrote {} file(s) to {}", written.len(), out.display());
    if source.per_patch {
        println!("This pack is rebuilt per patch; refetch after a game update.");
    }
    Ok(ExitCode::SUCCESS)
}

// ---------------------------------------------------------------- strings

/// `findGlobalIni`: the first `global.ini` (any case) under `dir`, depth-first with the
/// TypeScript's stack order.
fn find_global_ini(dir: &Path) -> Option<PathBuf> {
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        let mut entries: Vec<_> = entries.flatten().collect();
        // readdir order on Linux is the directory's; Node sorts nothing either, but a
        // stable order makes the pick reproducible.
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let path = e.path();
            if path.is_dir() {
                stack.push(path);
            } else if e.file_name().to_string_lossy().to_lowercase() == "global.ini" {
                return Some(path);
            }
        }
    }
    None
}

/// `readExtractedBuild`: `build` of `_build.json` in `dir`.
fn read_marker_build(dir: &Path) -> Option<String> {
    let v: Value = serde_json::from_slice(&std::fs::read(dir.join("_build.json")).ok()?).ok()?;
    v.get("build")?.as_str().map(str::to_string)
}

/// Where a string table came from.
struct StringTable {
    table: Localization,
    /// Printed in "N strings loaded from <label>".
    label: String,
    build: Option<String>,
}

fn load_string_table(flags: &Flags, language: &str) -> Result<StringTable, String> {
    if let Some(dir) = flags.get("extract-dir") {
        let dir = resolve(dir);
        let table = match find_global_ini(&dir) {
            Some(f) => parse_global_ini_bytes(
                &std::fs::read(&f).map_err(|e| format!("{}: {e}", f.display()))?,
            ),
            None => Localization::new(),
        };
        if table.is_empty() {
            return Err(format!(
                "No string table under {}. Run \"extract\" first, which fetches {language}/global.ini.",
                dir.display()
            ));
        }
        return Ok(StringTable {
            table,
            label: dir.display().to_string(),
            build: read_marker_build(&dir),
        });
    }
    let p4k = find_p4k(flags)?;
    let archive = archive::Archive::open(&p4k).map_err(|e| e.to_string())?;
    let entry = format!("Data\\Localization\\{language}\\global.ini");
    let table = match archive.entry(&entry) {
        Some(info) => parse_global_ini_bytes(&archive.read(&info).map_err(|e| e.to_string())?),
        None => Localization::new(),
    };
    if table.is_empty() {
        return Err(format!(
            "No string table for {language} in {} (Data/Localization/{language}/global.ini).",
            p4k.display()
        ));
    }
    Ok(StringTable {
        table,
        label: format!("{} ({language}/global.ini)", p4k.display()),
        build: source::read_build_version(&p4k),
    })
}

/// `writeStrings`: the base table as `game-strings-<language>.json`.
fn write_strings(
    out: &Path,
    table: &Localization,
    language: &str,
    build: Option<&str>,
) -> Result<PathBuf, String> {
    std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let mut value = strings::dataset(table, &output::iso_now(), build, false);
    if language != "english" {
        // strings::dataset is the English table's; the shape is the same for every language.
        value["_source"] = json!(format!(
            "{} (Data/Localization/{language}/global.ini)",
            output::SOURCE
        ));
        value["language"] = json!(language);
        value["_schema"] = json!(output::schema_tag(
            &format!("game-strings-{language}"),
            strings::SCHEMA_VERSION
        ));
    }
    let file = out.join(format!("game-strings-{language}.json"));
    std::fs::write(&file, output::to_js_json(&value))
        .map_err(|e| format!("{}: {e}", file.display()))?;
    Ok(file)
}

fn run_strings(flags: &Flags, rest: &[String]) -> Result<ExitCode, String> {
    let language = flags.get("language").map_or("english", String::as_str);
    let StringTable {
        table: base,
        label,
        build,
    } = load_string_table(flags, language)?;

    // Overlays layer in the order given: named packs, then the installed override, then
    // explicit paths. Later ones win.
    let mut overlay_paths: Vec<PathBuf> = Vec::new();
    if let Some(pack) = flags.get("pack") {
        let sources = overlays::load_sources(config_dir())?;
        if !sources.iter().any(|s| &s.name == pack) {
            let known: Vec<&str> = sources.iter().map(|s| s.name.as_str()).collect();
            return Err(format!(
                "Unknown pack \"{pack}\". Known: {}. Fetch it with \"overlays fetch {pack}\".",
                known.join(", ")
            ));
        }
        overlay_paths.push(match flags.get("overlay-dir") {
            Some(d) => resolve(d),
            None => resolve(&format!("overlays/{pack}")),
        });
    }
    if flags.contains_key("installed") {
        let Some(installed) = overlays::find_installed_override(&find_p4k(flags)?, language) else {
            return Err(format!(
                "No string pack is installed in the game folder for {language}. A pack installs as data/Localization/{language}/global.ini."
            ));
        };
        println!("installed pack: {}", installed.display());
        overlay_paths.push(installed);
    }
    if let Some(explicit) = flags.get("overlay") {
        for entry in explicit.split(',') {
            let trimmed = entry.trim();
            if !trimmed.is_empty() {
                overlay_paths.push(resolve(trimmed));
            }
        }
    }

    let mut table = base.clone();
    let mut overlay_table: Option<Localization> = None;
    if !overlay_paths.is_empty() {
        let mut merged = Localization::new();
        for p in &overlay_paths {
            let loaded = overlays::load_overlay(p).map_err(|_| {
                format!(
                    "Could not read overlay {}. For a named pack, run \"overlays fetch <name>\" first.",
                    p.display()
                )
            })?;
            merged = apply_overlay(&merged, &loaded);
        }
        table = apply_overlay(&base, &merged);
        println!(
            "{} base strings, {} from {} overlay(s)",
            base.len(),
            merged.len(),
            overlay_paths.len()
        );
        overlay_table = Some(merged);
    } else {
        println!("{} strings loaded from {label}", table.len());
    }

    if flags.contains_key("diff") {
        let Some(overlay) = &overlay_table else {
            return Err(
                "--diff needs an overlay: --pack <name>, --installed, or --overlay <path>.".into(),
            );
        };
        let (changed, added) = diff_strings(&base, overlay);
        println!(
            "\noverlay changes {} string(s) and adds {}",
            changed.len(),
            added.len()
        );
        for change in changed.iter().take(limit(flags, 10)?) {
            println!("  {}", change.key);
            println!(
                "      base    {}",
                js_slice(change.base.as_deref().unwrap_or(""), 90)
            );
            println!("      overlay {}", js_slice(&change.overlay, 90));
        }
    }

    let key = flags.get("key");
    if let Some(key) = key {
        match lookup_string(&table, key) {
            Some(v) => println!("{key} -> {v}"),
            None => println!("{key} -> (not found)"),
        }
    }

    let query = rest.join(" ");
    if !query.is_empty() {
        let (total, hits) = search_strings(
            &table,
            &query,
            limit(flags, 25)?,
            flags.contains_key("keys-only"),
        );
        let showing = if total > hits.len() {
            format!(", showing {}", hits.len())
        } else {
            String::new()
        };
        println!("\n{total} match(es) for \"{query}\"{showing}");
        for (k, v) in hits {
            println!("  {k}\n      {v}");
        }
    }

    if flags.contains_key("write") {
        let file = write_strings(&out_dir(flags), &base, language, build.as_deref())?;
        println!("\nwrote {} strings to {}", base.len(), file.display());
        // Filed under the table's own build stamp.
        if let Some(kept) =
            diff::snapshot_dataset(&file, &snapshots()).map_err(|e| e.to_string())?
        {
            println!(
                "snapshot    {} (diff against it as \"{}\")",
                kept.path.display(),
                kept.build
            );
        }
    }

    if key.is_none()
        && query.is_empty()
        && !flags.contains_key("write")
        && !flags.contains_key("diff")
    {
        println!("\nPass a search term, --key <name>, or --write.");
    }
    Ok(ExitCode::SUCCESS)
}

// ---------------------------------------------------------------- extract

/// `readBuildBranch`: `Data.Branch` of the `build_manifest.id` beside `Data.p4k`.
fn read_build_branch(p4k: &Path) -> Option<String> {
    let manifest = p4k.parent()?.join("build_manifest.id");
    let v: Value = serde_json::from_slice(&std::fs::read(manifest).ok()?).ok()?;
    v.get("Data")?.get("Branch")?.as_str().map(str::to_string)
}

/// `writeExtractedBuild`: `{ build, channel, branch?, extracted }`.
fn write_build_marker(
    dir: &Path,
    build: &str,
    channel: &str,
    branch: Option<&str>,
) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut v = Map::new();
    v.insert("build".into(), json!(build));
    v.insert("channel".into(), json!(channel));
    if let Some(b) = branch {
        v.insert("branch".into(), json!(b));
    }
    v.insert("extracted".into(), json!(output::iso_now()));
    let file = dir.join("_build.json");
    std::fs::write(&file, output::to_js_json(&Value::Object(v)))
        .map_err(|e| format!("{}: {e}", file.display()))?;
    Ok(file)
}

/// Collection sizes of a dataset body, as `parse` reports them (arrays and objects).
fn collections(value: &Value) -> Vec<(String, usize)> {
    value
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(k, v)| match v {
            Value::Array(a) => Some((k.clone(), a.len())),
            Value::Object(o) => Some((k.clone(), o.len())),
            _ => None,
        })
        .collect()
}

fn run_extract(command: &str, flags: &Flags, rest: &[String]) -> Result<ExitCode, String> {
    if command == "parse" {
        println!("parse: this pipeline reads Data.p4k directly, so parse runs extract");
    }
    let out = out_dir(flags);
    let selected = select_domains(rest)?;
    let p4k = find_p4k(flags)?;
    println!(
        "StarBreaker {} (linked crates)",
        &archive::STARBREAKER_COMMIT[..12]
    );
    println!("Data.p4k    {}", p4k.display());
    let build = source::read_build_version(&p4k);
    if let Some(b) = &build {
        println!("build       {b}");
    }
    let names: Vec<&str> = selected.iter().map(|d| d.name()).collect();
    println!("extracting  {}", names.join(", "));

    let keep_archive = selected.iter().any(|d| !d.files().is_empty());
    let files = source::open(&p4k, keep_archive)?;
    let records = DataCoreSource::parse(&files.dcb)?;
    let total = sc_gamedata::records::RecordSource::metas(&records).len();
    if total == 0 {
        return Err(format!("No records in the DataCore of {}.", p4k.display()));
    }
    println!("  loaded {total} records");
    if !files.localization.is_empty() {
        println!("  loaded {} localized strings", files.localization.len());
    }
    let archive_files = files.archive.map(ArchiveFiles::new);
    let file_source: &dyn FileSource = match &archive_files {
        Some(a) => a,
        None => &NoFiles,
    };
    let game = Game {
        records: &records,
        localization: &files.localization,
        files: file_source,
        build: build.clone(),
    };

    std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    let mut warnings = Vec::new();
    let mut written: Vec<(&str, Vec<(String, usize)>)> = Vec::new();
    for d in &selected {
        let dataset = pipeline::run(*d, &game, &output::iso_now()).map_err(|e| e.to_string())?;
        let file = out.join(format!("{}.json", dataset.name));
        std::fs::write(&file, dataset.to_json()).map_err(|e| format!("{}: {e}", file.display()))?;
        println!("  wrote {}.json", dataset.name);
        warnings.extend(dataset.warnings);
        written.push((dataset.name, collections(&dataset.value)));
    }
    if !warnings.is_empty() {
        println!();
        for w in &warnings {
            println!("  WARNING  {w}");
        }
    }

    if let Some(b) = &build {
        let marker_dir = flags
            .get("extract-dir")
            .map_or_else(|| out.clone(), |d| resolve(d));
        let branch = read_build_branch(&p4k);
        let marker = write_build_marker(
            &marker_dir,
            b,
            &channel(flags).to_uppercase(),
            branch.as_deref(),
        )?;
        println!("marker      {}", marker.display());
    }
    // The string table is part of a full dataset set (make all's strings step); a run
    // naming domains leaves it be.
    let mut snapshot_files: Vec<String> =
        written.iter().map(|(n, _)| format!("{n}.json")).collect();
    // The resource chain on game-commodities, joined from mining-spawns, refining,
    // blueprints and cargo of the same build beside it; rewritten after every run.
    if let Some(w) = sc_gamedata::resource_chain::write_resource_chain(&out)
        .map_err(|e| format!("{}: {e}", sc_gamedata::resource_chain::FILE))?
    {
        let c = &w.counts;
        println!(
            "chain       {} (minedAt {}, refinedBy {}, usedByBlueprints {}, inManifests {}, qualityOverrideBelts {})",
            w.file.display(),
            c.mined_at,
            c.refined_by,
            c.used_by_blueprints,
            c.in_manifests,
            c.quality_override_belts
        );
        for s in &w.skipped {
            println!("  WARNING  resource chain: {s}");
        }
        let file = sc_gamedata::resource_chain::FILE.to_string();
        if !snapshot_files.contains(&file) {
            snapshot_files.push(file);
        }
    }
    // Derived from item-tags + loot + item-sources (and blueprints, Wikelo trades), so
    // rebuilt after every run that leaves them on disk (removed when one is gone). A real
    // dataset (published, snapshotted, diffed), unlike the search index.
    // Blueprint products and NPC gear the item-tags rows lack (untagged records), from the
    // blueprints and loadouts on disk; before the obtain file, which lists them.
    if let Some(r) =
        sc_gamedata::domains::item_tags::add_identity_rows(&out, &records, &files.localization)
            .map_err(|e| format!("{}: {e}", sc_gamedata::domains::item_tags::FILE))?
    {
        println!(
            "item-tags   identity rows: {} blueprint, {} npcGear ({} not found)",
            r.blueprint,
            r.npc_gear,
            r.not_found.len()
        );
        // The rows changed the file on disk, so it goes in the snapshot even when this
        // run did not parse item-tags itself.
        let file = sc_gamedata::domains::item_tags::FILE.to_string();
        if !snapshot_files.contains(&file) {
            snapshot_files.push(file);
        }
    }
    let started = std::time::Instant::now();
    // Reverse links (stockOn, mountedOn, firedBy) from ships + weapons, added in place to
    // the components, weapons and ammo files of the same build, before they are snapshotted.
    if let Some(r) = sc_gamedata::reverse_links::apply_reverse_links(&out)
        .map_err(|e| format!("reverse links: {e}"))?
    {
        println!(
            "links       stockOn {}, mountedOn {}, firedBy {} ({} ammo fired by no weapon; {} file(s) rewritten)",
            r.stock_on,
            r.mounted_on,
            r.fired_by,
            r.unfired_ammo.len(),
            r.files.len()
        );
        // A run naming only an input (ships) can change a target it did not write: keep the
        // snapshot in step with the file on disk.
        for f in &r.files {
            if let Some(n) = f.file_name().and_then(|n| n.to_str())
                && !snapshot_files.iter().any(|s| s == n)
            {
                snapshot_files.push(n.to_string());
            }
        }
    }
    if let Some(w) = sc_gamedata::item_obtain::write_item_obtain(&out)
        .map_err(|e| format!("{}: {e}", sc_gamedata::item_obtain::FILE))?
    {
        println!(
            "obtain      {} ({} items, {:.0} KB, {:.2} s)",
            w.file.display(),
            w.items,
            w.bytes as f64 / 1024.0,
            started.elapsed().as_secs_f64()
        );
        snapshot_files.push(sc_gamedata::item_obtain::FILE.to_string());
    }
    if let Some(w) = sc_gamedata::tag_index::write_tag_index(&out)
        .map_err(|e| format!("{}: {e}", sc_gamedata::tag_index::FILE))?
    {
        println!(
            "tag-index   {} ({} tags, {:.0} KB)",
            w.file.display(),
            w.tags,
            w.bytes as f64 / 1024.0
        );
        snapshot_files.push(sc_gamedata::tag_index::FILE.to_string());
    }
    if rest.is_empty() {
        let file = write_strings(&out, &files.localization, "english", build.as_deref())?;
        println!(
            "strings     {} ({} strings)",
            file.display(),
            files.localization.len()
        );
        snapshot_files.push(format!("{}.json", strings::NAME));
    }
    // saveSnapshot, as parse does (the string table goes with it here, as make all's
    // `strings --write` files it under the same build).
    if let Some(b) = &build {
        let kept = diff::save_snapshot(&out, &snapshots(), b, Some(&snapshot_files))
            .map_err(|e| e.to_string())?;
        println!(
            "snapshot    {} ({} dataset(s); diff against it as \"{b}\")",
            kept.path.display(),
            kept.datasets
        );
    }
    // Derived from item-tags + ships, so rebuilt after every run that leaves both on disk
    // (and removed when one is gone).
    if let Some(w) = search_index::write_search_index(&out)
        .map_err(|e| format!("{}: {e}", search_index::FILE))?
    {
        println!(
            "search      {} ({} entries, {:.0} KB)",
            w.file.display(),
            w.entries,
            w.bytes as f64 / 1024.0
        );
    }

    println!("\nwrote {} dataset(s) to {}", written.len(), out.display());
    for (name, cols) in &written {
        let parts: Vec<String> = cols
            .iter()
            .filter(|(k, _)| k != "summary")
            .map(|(k, n)| format!("{k}={n}"))
            .collect();
        println!("  {name}  {}", parts.join(" "));
    }
    Ok(ExitCode::SUCCESS)
}

// ---------------------------------------------------------------- datasets

fn read_json(file: &Path) -> Option<Value> {
    serde_json::from_slice(&std::fs::read(file).ok()?).ok()
}

fn field<T: serde::de::DeserializeOwned>(v: Option<&Value>, key: &str) -> Option<T> {
    serde_json::from_value(v?.get(key)?.clone()).ok()
}

// ---------------------------------------------------------------- find

fn run_find(flags: &Flags, rest: &[String]) -> Result<ExitCode, String> {
    let data_dir = out_dir(flags);
    let all = flags.contains_key("all");
    let entries = match read_json(&data_dir.join(search_index::FILE)) {
        Some(doc) => fuzzy::from_index(&doc, all).map_err(|e| e.to_string())?,
        None => {
            let tags = read_json(&data_dir.join("game-item-tags.json"));
            let ships = read_json(&data_dir.join("game-ships.json"));
            if tags.is_none() && ships.is_none() {
                return Err(format!(
                    "No game-item-tags.json or game-ships.json in {}; run: make parse DOMAINS=\"item-tags ships\"",
                    data_dir.display()
                ));
            }
            let rows = |v: &Option<Value>, k: &str| -> Vec<Value> {
                v.as_ref()
                    .and_then(|v| v.get(k)?.as_array().cloned())
                    .unwrap_or_default()
            };
            let doc = search_index::build_search_index(
                &rows(&tags, "items"),
                &rows(&ships, "vehicles"),
                None,
            );
            fuzzy::from_index(&search_index::to_value(&doc), all).map_err(|e| e.to_string())?
        }
    };
    let query = rest.join(" ").trim().to_string();
    let fzf = finder::has_fzf();
    let interactive = !flags.contains_key("list")
        && std::io::stdout().is_terminal()
        && std::io::stdin().is_terminal()
        && fzf;
    if interactive {
        let all_entries: Vec<&fuzzy::Entry> = entries.iter().collect();
        let picked = finder::run_fzf(
            &all_entries,
            &self_command(),
            &query,
            &["--out".into(), data_dir.display().to_string()],
            &[],
        )?;
        if !picked.is_empty() {
            println!("{}", picked.join("\n"));
        }
        return Ok(ExitCode::SUCCESS);
    }
    let ranked = fuzzy::rank_entries(&entries, &query);
    let limit = limit(flags, 25)?;
    if flags.contains_key("print") {
        for e in ranked.iter().take(limit) {
            println!("{}", e.key);
        }
        return Ok(ExitCode::SUCCESS);
    }
    if ranked.is_empty() {
        return Err(format!("Nothing matches \"{query}\"."));
    }
    for e in ranked.iter().take(limit) {
        let ty = match e.kind {
            EntryKind::Ship => format!("ship {}", e.ty.as_deref().unwrap_or("")),
            EntryKind::Item => e.ty.clone().unwrap_or_default(),
        };
        println!(
            "  {} {} {} {}",
            js_pad_end(&e.key, 48),
            js_pad_end(js_slice(&e.name, 44), 44),
            js_pad_end(js_slice(&ty, 22), 22),
            e.maker.as_deref().unwrap_or("")
        );
    }
    if ranked.len() > limit {
        println!("  ... {} more (--limit)", ranked.len() - limit);
    }
    if !fzf {
        let keys: Vec<&str> = FIND_KEYS.iter().map(|(k, _)| *k).collect();
        println!(
            "\ninstall fzf for the interactive picker; keys: {}",
            keys.join(", ")
        );
    }
    Ok(ExitCode::SUCCESS)
}

/// How fzf re-enters this program.
fn self_command() -> Vec<String> {
    let exe = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "sc-gamedata".into());
    vec![exe]
}

/// Run this program again with `args --out <dir>`, passing its exit status on.
fn again(args: &[&str], data_dir: &Path) -> Result<ExitCode, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let status = std::process::Command::new(exe)
        .args(args)
        .arg("--out")
        .arg(data_dir)
        .status()
        .map_err(|e| e.to_string())?;
    Ok(ExitCode::from(
        status
            .code()
            .and_then(|c| u8::try_from(c).ok())
            .unwrap_or(1),
    ))
}

/// `find-run <action> <item|ship> <key>`: what the picker's keys run.
fn run_find_action(flags: &Flags, rest: &[String]) -> Result<ExitCode, String> {
    let [action, kind, key, ..] = rest else {
        return Err("find-run <action> <item|ship> <key>".into());
    };
    let data_dir = out_dir(flags);
    if kind == "item" {
        return match action.as_str() {
            "preview" => again(&["obtain", key, "--limit", "8"], &data_dir),
            "report" => again(&["obtain", key, "--limit", "60"], &data_dir),
            "json" => again(&["obtain", key, "--json"], &data_dir),
            "tags" => {
                let tags = read_json(&data_dir.join("game-item-tags.json"));
                let row = tags
                    .as_ref()
                    .and_then(|t| t.get("items")?.as_array())
                    .and_then(|items| {
                        items
                            .iter()
                            .find(|i| i.get("className").and_then(Value::as_str) == Some(key))
                    })
                    .cloned()
                    .unwrap_or(Value::Null);
                println!("{}", output::to_js_json(&row));
                Ok(ExitCode::SUCCESS)
            }
            _ => {
                println!("{action} works on ships; {key} is an item.");
                Ok(ExitCode::SUCCESS)
            }
        };
    }
    if action == "engage" || action == "deflect" {
        return again(&[action, key], &data_dir);
    }
    let ships = read_json(&data_dir.join("game-ships.json"));
    let ship = ships
        .as_ref()
        .and_then(|s| s.get("vehicles")?.as_array())
        .and_then(|vs| {
            vs.iter().find(|v| {
                v.get("recordName")
                    .and_then(Value::as_str)
                    .is_some_and(|r| search_index::ship_key(r) == key)
            })
        })
        .ok_or_else(|| format!("No ship {key} in game-ships.json"))?;
    if action == "preview" || action == "report" {
        println!("{}", finder::render_ship(ship));
    } else {
        println!("{}", output::to_js_json(ship));
    }
    Ok(ExitCode::SUCCESS)
}

// ---------------------------------------------------------------- obtain, where

fn run_obtain(command: &str, flags: &Flags, rest: &[String]) -> Result<ExitCode, String> {
    let wanted = rest.join(" ").trim().to_string();
    if wanted.is_empty() {
        return Err(format!(
            "{command} needs an item: sc-gamedata {command} <name or class name>"
        ));
    }
    let data_dir = out_dir(flags);
    let load = |file: &str| read_json(&data_dir.join(file));
    let tags = load("game-item-tags.json").ok_or_else(|| {
        format!(
            "game-item-tags.json missing in {}; run: make parse DOMAINS=\"game-item-tags\"",
            data_dir.display()
        )
    })?;
    let loot: Option<LootData> = load("game-loot.json")
        .map(serde_json::from_value)
        .transpose()
        .map_err(|e| format!("game-loot.json: {e}"))?;
    let sources = load("game-item-sources.json");
    let shops = load("uex-item-prices.json");
    let terminals = load("uex-terminals.json");
    let shops_as_of = shops
        .as_ref()
        .and_then(|p| p.get("_fetched")?.as_str())
        .map(|s| AsOf::Iso(s.to_string()));
    let blueprints_doc = load("game-blueprints.json");
    let wikelo_doc = load("game-wikelo-trades.json");
    let datasets = Datasets {
        items: field::<Vec<ItemTags>>(Some(&tags), "items").unwrap_or_default(),
        loot,
        areas: field::<Vec<LootArea>>(sources.as_ref(), "areas"),
        contract_awards: field::<Vec<ContractAward>>(sources.as_ref(), "contractAwards"),
        blueprints: field::<Vec<BlueprintLike>>(blueprints_doc.as_ref(), "blueprints"),
        wikelo: field::<Vec<WikeloTradeLike>>(wikelo_doc.as_ref(), "trades"),
        shops: field::<Vec<ShopPrice>>(shops.as_ref(), "prices"),
        terminals: field::<Vec<UexTerminal>>(terminals.as_ref(), "terminals"),
        shops_as_of,
    };

    let mut found = find_items(&datasets.items, &wanted);
    if found.is_empty() {
        // No word match: take the best fuzzy one (typos land here), with every record of
        // that name.
        let item_entries = fuzzy::item_entries(&datasets.items, false);
        let ranked: Vec<&fuzzy::Entry> = fuzzy::rank_entries(&item_entries, &wanted)
            .into_iter()
            .filter(|e| e.kind == EntryKind::Item)
            .collect();
        let best = ranked
            .first()
            .and_then(|e| datasets.items.iter().find(|i| i.class_name == e.key))
            .ok_or_else(|| format!("No item matches \"{wanted}\"."))?;
        found = datasets
            .items
            .iter()
            .filter(|i| i.name == best.name)
            .collect();
        let mut others: Vec<&str> = Vec::new();
        for e in ranked.iter().skip(1) {
            if !others.contains(&e.name.as_str()) {
                others.push(&e.name);
            }
        }
        let others: Vec<&str> = others
            .into_iter()
            .filter(|n| *n != best.name)
            .take(4)
            .collect();
        eprintln!(
            "no item named \"{wanted}\"; using the closest, {} ({}){}",
            best.name,
            best.class_name,
            if others.is_empty() {
                String::new()
            } else {
                format!("; others: {}", others.join(", "))
            }
        );
    }
    let index = datasets.loot.as_ref().map(build_loot_index);
    let limit = limit(flags, 15)?;
    let at = flags.get("at").map(String::as_str);
    if at == Some("true") {
        return Err("--at needs a place, e.g. --at grimhex".into());
    }
    let ammo = flags.contains_key("ammo");
    let json = flags.contains_key("json");

    // --ammo: report on what the matched weapons load instead of the weapons.
    let targets: Vec<(&ItemTags, Vec<&ItemTags>)> = if ammo {
        let t = ammo_targets(&datasets.items, &found);
        if t.is_empty() {
            return Err(format!(
                "No weapon matching \"{wanted}\" names a magazine or canister in game-item-tags.json (re-parse item-tags if it predates ammoItems)."
            ));
        }
        t.into_iter().map(|t| (t.ammo, t.weapons)).collect()
    } else {
        found.iter().map(|i| (*i, Vec::new())).collect()
    };
    let shown = targets.len() <= 3 || ammo;
    let shops_loaded = shops.is_some();

    if !shown && !json {
        println!(
            "{} items match \"{wanted}\"; narrow it, or use a class name:\n",
            found.len()
        );
        for m in found.iter().take(40) {
            println!("  {} {}", js_pad_end(&m.class_name, 48), m.name);
        }
        if found.len() > 40 {
            println!("  ... {} more", found.len() - 40);
        }
        return Ok(ExitCode::SUCCESS);
    }
    let picked = if shown { &targets[..] } else { &targets[..1] };
    let reports: Vec<_> = picked
        .iter()
        .map(|(item, ammo_for)| {
            obtain(
                &datasets,
                item,
                index.as_ref(),
                &ObtainOptions { at, ammo_for },
            )
        })
        .collect();
    // Ammo can be several records (empty and filled canister): the one you can buy leads,
    // and a sourceless twin is dropped when another has a source.
    let (order, dropped) = if ammo {
        let summaries: Vec<_> = reports
            .iter()
            .map(|r| where_summary(r, Some(shops_loaded)))
            .collect();
        pick_ammo(&summaries)
    } else {
        ((0..reports.len()).collect(), Vec::new())
    };
    let dropped_note = (!dropped.is_empty()).then(|| {
        let names: Vec<&str> = dropped
            .iter()
            .map(|&i| reports[i].item.class_name.as_str())
            .collect();
        format!(
            "note: {} more compatible magazine(s) with no known source left out: {}",
            dropped.len(),
            names.join(", ")
        )
    });
    let matches: Vec<&str> = found.iter().map(|m| m.class_name.as_str()).collect();
    if command == "where" {
        let summaries: Vec<_> = order
            .iter()
            .map(|&i| where_summary(&reports[i], Some(shops_loaded)))
            .collect();
        if json {
            let v = json!({ "matches": matches, "summaries": to_value(&summaries)? });
            println!("{}", output::to_js_json(&v));
            return Ok(ExitCode::SUCCESS);
        }
        let text: Vec<String> = summaries.iter().map(render_where).collect();
        println!("{}", text.join("\n\n"));
        if let Some(n) = &dropped_note {
            println!("\n{n}");
        }
    } else if json {
        // The TypeScript reports carry the dataset rows themselves; the typed rows here keep
        // only what the query reads, so the JSON puts the raw rows back.
        let raw = |doc: &Option<Value>, key: &str| -> Vec<Value> {
            doc.as_ref()
                .and_then(|d| d.get(key)?.as_array().cloned())
                .unwrap_or_default()
        };
        let (raw_blueprints, raw_trades) = (
            raw(&blueprints_doc, "blueprints"),
            raw(&wikelo_doc, "trades"),
        );
        let mut list = Vec::new();
        for &i in &order {
            let r = &reports[i];
            let mut v = to_value(r)?;
            v["blueprints"] = restore(
                &r.blueprints,
                datasets.blueprints.as_deref(),
                &raw_blueprints,
            )?;
            v["wikeloInferred"] =
                restore(&r.wikelo_inferred, datasets.wikelo.as_deref(), &raw_trades)?;
            list.push(v);
        }
        let v = json!({ "matches": matches, "reports": list });
        println!("{}", output::to_js_json(&v));
        return Ok(ExitCode::SUCCESS);
    } else {
        for &i in &order {
            println!("{}", render_obtain(&reports[i], limit, Some(shops_loaded)));
            println!();
        }
        if let Some(n) = &dropped_note {
            println!("{n}");
        }
    }
    if command == "where" && shops_loaded {
        println!("\nshop rows: UEX, player-reported, not game files");
    }
    if sources.is_none() {
        println!(
            "note: game-item-sources.json missing, so no area ranking; run: make parse DOMAINS=game-item-sources"
        );
    }
    if !shops_loaded {
        println!("note: no shop data; run: sc-gamedata fetch uex-item-prices");
    }
    if shops_loaded && terminals.is_none() {
        println!(
            "note: no terminal places, so shops show terminal names only; run: sc-gamedata fetch uex-terminals"
        );
    }
    Ok(ExitCode::SUCCESS)
}

/// `rows` (typed copies of dataset rows) as the dataset's own rows: each row's raw JSON
/// from `raw`, found by its position in `typed` (the same rows, deserialised).
fn restore<T: PartialEq + serde::Serialize>(
    rows: &[T],
    typed: Option<&[T]>,
    raw: &[Value],
) -> Result<Value, String> {
    let typed = typed.unwrap_or(&[]);
    let aligned = typed.len() == raw.len();
    rows.iter()
        .map(|row| match typed.iter().position(|t| t == row) {
            Some(i) if aligned => Ok(raw[i].clone()),
            _ => to_value(row),
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Value::Array)
}

fn to_value<T: serde::Serialize>(v: &T) -> Result<Value, String> {
    serde_json::to_value(v).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------- mcp-query

/// One SCMCP request as JSON on stdin; the tool reply on stdout (see `sc_gamedata::mcp`).
fn run_mcp_query() -> Result<ExitCode, String> {
    use std::io::Read;
    let mut input = String::new();
    std::io::stdin()
        .read_to_string(&mut input)
        .map_err(|e| e.to_string())?;
    let req: sc_gamedata::mcp::Request =
        serde_json::from_str(&input).map_err(|e| format!("mcp-query request: {e}"))?;
    print!("{}", sc_gamedata::mcp::answer(&req)?);
    Ok(ExitCode::SUCCESS)
}

// ---------------------------------------------------------------- shop

fn run_shop(flags: &Flags, rest: &[String]) -> Result<ExitCode, String> {
    let place = rest.join(" ").trim().to_string();
    if place.is_empty() {
        return Err("shop needs a terminal or place: sc-gamedata shop \"grim hex\"".into());
    }
    let data_dir = out_dir(flags);
    let load = |file: &str| read_json(&data_dir.join(file));
    let prices = load("uex-item-prices.json");
    let terminals = load("uex-terminals.json");
    let tags = load("game-item-tags.json");
    let Some(shops) = field::<Vec<ShopPrice>>(prices.as_ref(), "prices") else {
        return Err(format!(
            "uex-item-prices.json missing in {}; run: sc-gamedata fetch uex-item-prices",
            data_dir.display()
        ));
    };
    let fetched = prices
        .as_ref()
        .and_then(|p| p.get("_fetched")?.as_str())
        .map(str::to_string);
    let terminal_rows = field::<Vec<UexTerminal>>(terminals.as_ref(), "terminals");
    let items = field::<Vec<ItemTags>>(tags.as_ref(), "items").unwrap_or_default();
    let report = shop_inventory(
        &ShopInputs {
            shops: &shops,
            terminals: terminal_rows.as_deref().unwrap_or(&[]),
            items: &items,
            as_of: fetched.clone().map(AsOf::Iso),
            stale_days: None,
        },
        &place,
    );
    let limit = limit_flag(flags, SHOP_ITEM_LIMIT, true)?;
    let max_terminals = (!flags.contains_key("all")).then_some(SHOP_TERMINAL_LIMIT);
    if flags.contains_key("json") {
        let cut = serde_json::to_value(limit_shop(&report, limit, max_terminals))
            .map_err(|e| e.to_string())?;
        let mut v = Map::new();
        if let Some(f) = &fetched {
            v.insert("fetched".into(), json!(f));
        }
        if let Value::Object(o) = cut {
            v.extend(o);
        }
        println!("{}", output::to_js_json(&Value::Object(v)));
        return Ok(ExitCode::SUCCESS);
    }
    println!("{}", render_shop(&report, limit, max_terminals));
    if let Some(f) = &fetched {
        println!("\nprices fetched {}", js_slice(f, 10));
    }
    if terminals.is_none() {
        println!(
            "note: no terminal places, so only terminal names match; run: sc-gamedata fetch uex-terminals"
        );
    }
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &[&str]) -> Args {
        parse_args(&s.iter().map(|x| x.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn release_script_edits_only_on_republish_and_creates_unless_found() {
        let plan = publish::PublishPlan {
            build: "5.0.202.35026".into(),
            repo: "voidput/sc-gamedata-dumps".into(),
            remote: String::new(),
            work_dir: PathBuf::from("/tmp/dumps checkout"),
            origin: publish::Origin {
                channel: "PTU".into(),
                branch: Some("sc-alpha-5.0.0".into()),
            },
            patch: None,
            republish: false,
            extractor: publish::Extractor {
                commit: "c".into(),
                dirty: false,
            },
            dumps: Vec::new(),
            notes: publish::PublishNotes::Skipped(String::new()),
            warnings: Vec::new(),
        };
        let fresh = release_script(&plan, false);
        assert!(fresh.starts_with("#!/bin/sh\n"));
        assert!(fresh.contains("set -e\n"));
        assert!(!fresh.contains("gh release edit"));
        assert!(fresh.contains(
            "if gh release view 5.0.202.35026 -R voidput/sc-gamedata-dumps >/dev/null 2>&1; then\n  echo 'Release 5.0.202.35026 already exists.'\nelse\n  gh release create 5.0.202.35026 -R voidput/sc-gamedata-dumps --verify-tag --title '5.0.0 PTU (5.0.202.35026)' --notes "
        ));
        assert!(fresh.contains("--prerelease\nfi\n"));
        // build.json goes up after the release exists, from the checkout, at the tag only.
        let upload = fresh.find("gh release upload 5.0.202.35026 build.json --clobber -R voidput/sc-gamedata-dumps\n").unwrap();
        assert!(fresh.find("gh release create").unwrap() < upload);
        assert!(fresh.contains("\ncd '/tmp/dumps checkout'\nif [ \"$(git rev-parse HEAD)\" = \"$(git rev-parse '5.0.202.35026^{commit}')\" ]; then\n  gh release upload"));
        assert!(fresh.contains("release asset not uploaded.'\nfi\n"));
        // Then every tool repo is told, after the upload, unless SC_NO_DISPATCH is set.
        let dispatch = fresh.find("if [ -z \"${SC_NO_DISPATCH:-}\" ]; then\n").unwrap();
        assert!(upload < dispatch);
        assert!(fresh.ends_with("fi\n"));
        let edited = release_script(&plan, true);
        let edit = edited.find("gh release edit 5.0.202.35026").unwrap();
        assert!(edit < edited.find("gh release view").unwrap());
        assert!(edited.contains("gh release upload 5.0.202.35026 build.json --clobber"));
    }

    #[test]
    fn dispatch_block_runs_data_sync_per_tool_for_live_and_ptu_only() {
        let mut plan = publish::PublishPlan {
            build: "4.10.201.48085".into(),
            repo: "voidput/sc-gamedata-dumps".into(),
            remote: String::new(),
            work_dir: PathBuf::from("/tmp/d"),
            origin: publish::Origin {
                channel: "LIVE".into(),
                branch: None,
            },
            patch: None,
            republish: false,
            extractor: publish::Extractor {
                commit: "c".into(),
                dirty: false,
            },
            dumps: Vec::new(),
            notes: publish::PublishNotes::Skipped(String::new()),
            warnings: Vec::new(),
        };
        let repos = vec!["voidput/snares".to_string(), "voidput/MCsim".to_string()];
        let live = dispatch_block(&plan, &repos);
        assert_eq!(
            live,
            "if [ -z \"${SC_NO_DISPATCH:-}\" ]; then\n  if gh workflow run data-sync.yml -R voidput/snares -f tag=4.10.201.48085 -f channel=LIVE; then echo 'dispatched data-sync to voidput/snares'; else echo 'dispatch to voidput/snares failed (is data-sync.yml on its default branch?)'; fi\n  if gh workflow run data-sync.yml -R voidput/MCsim -f tag=4.10.201.48085 -f channel=LIVE; then echo 'dispatched data-sync to voidput/MCsim'; else echo 'dispatch to voidput/MCsim failed (is data-sync.yml on its default branch?)'; fi\nfi\n"
        );
        plan.origin.channel = "HOTFIX".into();
        let hotfix = dispatch_block(&plan, &repos);
        assert!(!hotfix.contains("gh workflow run"));
        assert!(hotfix.contains("not dispatched"));
    }

    #[test]
    fn boolean_flags_do_not_swallow_positionals() {
        let a = args(&[
            "strings",
            "--installed",
            "Ouratite",
            "--limit",
            "3",
            "--key",
        ]);
        assert_eq!(a.command, "strings");
        assert_eq!(a.rest, vec!["Ouratite"]);
        assert_eq!(a.flags["installed"], "true");
        assert_eq!(a.flags["limit"], "3");
        assert_eq!(a.flags["key"], "true");
        assert_eq!(args(&[]).command, "help");
        // `notes a b --write notes.md`: --write is boolean, the file is a positional.
        assert_eq!(
            args(&["notes", "a", "b", "--write", "n.md"]).rest,
            vec!["a", "b", "n.md"]
        );
    }

    #[test]
    fn limits() {
        let f = |v: &str| {
            let mut m = Flags::new();
            m.insert("limit".into(), v.into());
            m
        };
        assert_eq!(limit_flag(&Flags::new(), 25, false), Ok(Some(25)));
        assert_eq!(limit_flag(&f("7"), 25, false), Ok(Some(7)));
        assert_eq!(limit_flag(&f("all"), 25, true), Ok(None));
        assert!(limit_flag(&f("all"), 25, false).is_err());
        assert!(limit_flag(&f("0"), 25, false).is_err());
        assert!(limit_flag(&f("2.5"), 25, false).is_err());
    }

    #[test]
    fn ts_order_covers_the_registry() {
        let names: Vec<&str> = domains::all().iter().map(|d| d.name()).collect();
        assert_eq!(names.len(), TS_ORDER.len());
        for n in TS_ORDER {
            assert!(names.contains(n), "{n} is not registered");
        }
        assert_eq!(all_domains()[0].name(), "game-mining");
        let picked = select_domains(&["ships".into(), "game-ammo".into()]).unwrap();
        assert_eq!(picked[0].name(), "game-ships");
        assert_eq!(picked[1].name(), "game-ammo");
        assert!(
            select_domains(&["nope".into()])
                .err()
                .unwrap()
                .starts_with("Unknown domain \"nope\"")
        );
    }

    #[test]
    fn build_marker_shape() {
        let dir = tempfile::tempdir().unwrap();
        let file = write_build_marker(dir.path(), "4.10.201", "LIVE", None).unwrap();
        let v: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        let keys: Vec<&String> = v.as_object().unwrap().keys().collect();
        assert_eq!(keys, vec!["build", "channel", "extracted"]);
        assert_eq!(read_marker_build(dir.path()).as_deref(), Some("4.10.201"));
    }

    #[test]
    fn build_commands_check_their_arguments_first() {
        let none: Vec<String> = Vec::new();
        assert!(
            run_diff(&Flags::new(), &none)
                .unwrap_err()
                .starts_with("diff needs two builds")
        );
        assert!(run_changes(&Flags::new(), &none).is_err());
        assert!(run_notes(&Flags::new(), &none).is_err());
        let mut f = Flags::new();
        f.insert("min-change".into(), "-1".into());
        let two = vec!["a".to_string(), "b".to_string()];
        assert!(run_notes(&f, &two).unwrap_err().starts_with("--min-change"));
        assert!(run_deflect(&Flags::new(), &none).is_err());
    }

    #[test]
    fn publish_reads_arguments_as_the_script_does() {
        let argv = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let a = publish_args(&argv(&["publish", "--channel", "ptu", "--dry-run"]))
            .ok()
            .unwrap();
        assert!(a.dry_run && !a.republish);
        assert_eq!(a.channel.as_deref(), Some("PTU"));
        assert!(publish_args(&argv(&["publish", "--patch", "--dry-run"])).is_err());
        assert!(publish_args(&argv(&["publish", "--patch"])).is_err());
        let a = publish_args(&argv(&["publish", "--patch", "4.10.3"]))
            .ok()
            .unwrap();
        assert_eq!(a.patch.as_deref(), Some("4.10.3"));
        assert_eq!(
            shell_line(&argv(&["gh", "--title", "4.10.3 LIVE (x)", "it's"])),
            "gh --title '4.10.3 LIVE (x)' 'it'\\''s'"
        );
    }

    #[test]
    fn js_helpers() {
        assert_eq!(js_number_of(""), 0.0);
        assert_eq!(js_number_of(" 12.5 "), 12.5);
        assert!(js_number_of("true").is_nan());
        assert_eq!(sized_spec("m7a cannon s5"), Some(("m7a cannon", 5.0)));
        assert_eq!(sized_spec("a  s3"), Some(("a ", 3.0)));
        assert_eq!(sized_spec("m7a cannon"), None);
        assert_eq!(sized_spec("cannons5"), None);
        assert_eq!(
            build_stamp("{\n  \"_build\":  \"4.10.200\",").as_deref(),
            Some("4.10.200")
        );
        assert!(variant_record(
            "EntityClassDefinition.AEGS_Gladius_PU_AI_CIV"
        ));
        assert!(!variant_record("EntityClassDefinition.AEGS_Gladius"));
    }
}
