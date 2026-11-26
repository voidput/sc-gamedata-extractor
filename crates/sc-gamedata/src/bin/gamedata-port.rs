//! `gamedata-port`: dev harness for the sc-gamedata port. Not part of the shipped CLI.
//!
//! ```text
//! gamedata-port extract --channel LIVE --out .tmp/gamedata/LIVE [--domain manufacturers]
//! gamedata-port compare .tmp/gamedata/LIVE ~/git/sc-gamedata/data [--domain manufacturers]
//! gamedata-port search-index .tmp/gamedata/LIVE
//! gamedata-port list
//! ```
//!
//! `extract` only reads the archive. Write `--out` somewhere under `.tmp/`, never into the
//! game install or the sc-gamedata checkout.

use clap::{Parser, Subcommand};
use sc_gamedata::compare::{Outcome, compare_text};
use sc_gamedata::domain::{Domain, FileSource, NoFiles};
use sc_gamedata::localization::redaction;
use sc_gamedata::pipeline::{self, Game};
use sc_gamedata::source::{self, ArchiveFiles, DataCoreSource};
use sc_gamedata::{domains, output, search_index, strings};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[path = "gamedata-port/query.rs"]
mod query;

#[derive(Parser)]
#[command(
    name = "gamedata-port",
    about = "Extract and compare sc-gamedata datasets"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Write game-<domain>.json for the registered domains (and the string table), then
    /// game-search-index.json when item-tags and ships are both in --out.
    Extract {
        /// LIVE, PTU, EPTU, HOTFIX: picks the install when --p4k is not given.
        #[arg(long, default_value = "LIVE")]
        channel: String,
        /// Data.p4k to read (else SC_DATA_P4K, else the usual install locations).
        #[arg(long)]
        p4k: Option<PathBuf>,
        #[arg(long)]
        out: PathBuf,
        /// Only these domains (`manufacturers` or `game-manufacturers`; `strings-english`
        /// for the string table). Repeatable. Default: every registered domain.
        #[arg(long)]
        domain: Vec<String>,
        /// Redact the string table as published dumps do (off by default, like data/).
        #[arg(long)]
        redact_strings: bool,
        /// Drop prose fields as published dumps do (`scripts/redact.mjs` `redactProse`):
        /// descriptions, long mission titles; `_redacted` lists what went.
        #[arg(long)]
        redact_prose: bool,
    },
    /// Byte compare of our datasets with golden ones (paths on a difference). Exits 1 on any
    /// difference or missing dataset.
    Compare {
        ours: PathBuf,
        golden: PathBuf,
        /// Only these datasets. Default: every game-*.json in <ours> or <golden>, so a
        /// dataset only the golden side has is reported as not produced.
        #[arg(long)]
        domain: Vec<String>,
        /// Example paths printed per dataset.
        #[arg(long, default_value_t = 20)]
        examples: usize,
    },
    /// (Re)build game-search-index.json in <dir> from the game-item-tags.json and
    /// game-ships.json there; removes a stale index when one of them is missing.
    SearchIndex { dir: PathBuf },
    /// Answer a query file (see scripts/query-golden.mjs) with the query port, as JSON.
    Query {
        #[arg(long)]
        data: PathBuf,
        #[arg(long)]
        queries: PathBuf,
    },
    /// List the registered domains.
    List,
}

fn file_name(domain: &str) -> String {
    let base = domain.strip_suffix(".json").unwrap_or(domain);
    if base.starts_with("game-") {
        format!("{base}.json")
    } else {
        format!("game-{base}.json")
    }
}

fn main() -> ExitCode {
    match Cli::parse().command {
        Command::List => {
            for d in domains::all() {
                println!("{:28} {}", d.name(), d.description());
            }
            println!(
                "{:28} The string table (not a record domain).",
                strings::NAME
            );
            println!(
                "{:28} Derived from item-tags + ships after an extract (not a record domain).",
                search_index::NAME
            );
            ExitCode::SUCCESS
        }
        Command::Extract {
            channel,
            p4k,
            out,
            domain,
            redact_strings,
            redact_prose,
        } => match extract(
            &channel,
            p4k.as_deref(),
            &out,
            &domain,
            redact_strings,
            redact_prose,
        ) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::from(2)
            }
        },
        Command::SearchIndex { dir } => match write_index(&dir) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::from(2)
            }
        },
        Command::Query { data, queries } => match query::run(&data, &queries) {
            Ok(text) => {
                println!("{text}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::from(2)
            }
        },
        Command::Compare {
            ours,
            golden,
            domain,
            examples,
        } => compare(&ours, &golden, &domain, examples),
    }
}

fn extract(
    channel: &str,
    p4k: Option<&Path>,
    out: &Path,
    names: &[String],
    redact_strings: bool,
    redact_prose: bool,
) -> Result<(), String> {
    let want_strings = names.is_empty()
        || names
            .iter()
            .any(|n| file_name(n) == format!("{}.json", strings::NAME));
    let is = |n: &String, name: &str| file_name(n) == format!("{name}.json");
    let selected: Vec<&'static dyn Domain> = if names.is_empty() {
        domains::all()
    } else {
        let mut wanted: Vec<&str> = Vec::new();
        for n in names {
            if is(n, strings::NAME) {
                continue;
            }
            if is(n, search_index::NAME) {
                // The index needs both of its inputs in --out.
                wanted.extend(["game-item-tags", "game-ships"]);
            } else {
                wanted.push(n.strip_suffix(".json").unwrap_or(n));
            }
        }
        let mut selected: Vec<&'static dyn Domain> = Vec::new();
        for n in wanted {
            let d = domains::find(n).ok_or_else(|| {
                let known: Vec<_> = domains::all().iter().map(|d| d.name()).collect();
                format!("unknown domain {n:?}; registered: {}", known.join(", "))
            })?;
            if !selected.iter().any(|s| s.name() == d.name()) {
                selected.push(d);
            }
        }
        selected
    };

    let p4k = source::find_data_p4k(p4k, channel)?;
    eprintln!("reading {}", p4k.display());
    let keep_archive = selected.iter().any(|d| !d.files().is_empty());
    let files = source::open(&p4k, keep_archive)?;
    eprintln!(
        "  build {}, {} localized strings",
        files.build.as_deref().unwrap_or("unknown"),
        files.localization.len()
    );
    let records = DataCoreSource::parse(&files.dcb)?;
    eprintln!(
        "  {} main records",
        sc_gamedata::records::RecordSource::metas(&records).len()
    );
    let archive_files = files.archive.map(ArchiveFiles::new);
    let file_source: &dyn FileSource = match &archive_files {
        Some(a) => a,
        None => &NoFiles,
    };
    let game = Game {
        records: &records,
        localization: &files.localization,
        files: file_source,
        build: files.build.clone(),
    };

    std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let write = |name: &str, text: &str| -> Result<(), String> {
        let path = out.join(format!("{name}.json"));
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
        eprintln!("  wrote {}", path.display());
        Ok(())
    };
    for d in &selected {
        let dataset = pipeline::run(*d, &game, &output::iso_now()).map_err(|e| e.to_string())?;
        for w in &dataset.warnings {
            eprintln!("  WARNING  {w}");
        }
        let file = format!("{}.json", dataset.name);
        match redact_prose
            .then(|| redaction::redact_prose(&file, &dataset.value))
            .flatten()
        {
            Some(published) => write(dataset.name, &output::to_js_json(&published))?,
            None => write(dataset.name, &dataset.to_json())?,
        }
    }
    if want_strings {
        let v = strings::dataset(
            &files.localization,
            &output::iso_now(),
            files.build.as_deref(),
            redact_strings,
        );
        write(strings::NAME, &output::to_js_json(&v))?;
    }
    // Identity rows for untagged blueprint products and NPC gear, before the obtain file.
    if let Some(r) =
        sc_gamedata::domains::item_tags::add_identity_rows(out, &records, &files.localization)
            .map_err(|e| format!("{}: {e}", sc_gamedata::domains::item_tags::FILE))?
    {
        eprintln!(
            "  item-tags identity rows: {} blueprint, {} npcGear",
            r.blueprint, r.npc_gear
        );
    }
    // Derived from item-tags + loot + item-sources: same rule as the index.
    if let Some(w) = sc_gamedata::item_obtain::write_item_obtain(out)
        .map_err(|e| format!("{}: {e}", sc_gamedata::item_obtain::FILE))?
    {
        eprintln!(
            "  wrote {} ({} items, {} KB)",
            w.file.display(),
            w.items,
            w.bytes / 1024
        );
    }
    if let Some(w) = sc_gamedata::tag_index::write_tag_index(out)
        .map_err(|e| format!("{}: {e}", sc_gamedata::tag_index::FILE))?
    {
        eprintln!(
            "  wrote {} ({} tags, {} KB)",
            w.file.display(),
            w.tags,
            w.bytes / 1024
        );
    }
    // Derived from item-tags + ships: rebuilt after every extract that leaves both in
    // --out, removed when one is missing (as sc-gamedata's parse does).
    write_index(out)
}

fn write_index(dir: &Path) -> Result<(), String> {
    match search_index::write_search_index(dir)
        .map_err(|e| format!("{}: {e}", search_index::FILE))?
    {
        Some(w) => eprintln!(
            "  wrote {} ({} entries, {} KB)",
            w.file.display(),
            w.entries,
            w.bytes / 1024
        ),
        None => eprintln!(
            "  {} not written: needs game-item-tags.json and game-ships.json in {}",
            search_index::FILE,
            dir.display()
        ),
    }
    Ok(())
}

/// The `game-*.json` file names in `dir` (none when it cannot be read).
fn datasets_in(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.starts_with("game-") && n.ends_with(".json"))
        .collect()
}

fn compare(ours: &Path, golden: &Path, names: &[String], examples: usize) -> ExitCode {
    let files: Vec<String> = if names.is_empty() {
        let mut v = datasets_in(ours);
        for name in datasets_in(golden) {
            if !v.contains(&name) {
                v.push(name);
            }
        }
        v.sort();
        v
    } else {
        names.iter().map(|n| file_name(n)).collect()
    };
    if files.is_empty() {
        eprintln!(
            "no game-*.json in {} or {}",
            ours.display(),
            golden.display()
        );
        return ExitCode::from(2);
    }
    let mut failed = 0;
    for f in &files {
        let name = f.strip_suffix(".json").unwrap_or(f);
        let read = |dir: &Path| std::fs::read_to_string(dir.join(f));
        let outcome = match (read(ours), read(golden)) {
            (Ok(a), Ok(b)) => compare_text(&a, &b),
            (Err(_), Ok(_)) if !ours.join(f).exists() => Outcome::MissingOurs,
            (Err(e), _) => Outcome::Unreadable(format!("ours: {e}")),
            (_, Err(_)) => Outcome::MissingGolden,
        };
        println!("{}", outcome.report(name, examples));
        if !outcome.is_ok() {
            failed += 1;
        }
    }
    println!(
        "{} of {} datasets identical",
        files.len() - failed,
        files.len()
    );
    if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
