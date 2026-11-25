//! Real-data checks against LIVE and the golden dumps. `#[ignore]`: they need a game
//! install (read only) and an sc-gamedata checkout (read only). Output goes to `.tmp/`.
//!
//! ```text
//! SC_DATA_P4K=/path/to/LIVE/Data.p4k GOLDEN_DIR=~/git/sc-gamedata/data \
//!   cargo test --release -p sc-gamedata --test live -- --ignored --nocapture
//! ```
//! Prefer `scripts/compare-gamedata.sh`, which runs every domain.

use sc_gamedata::compare::{Outcome, compare_text};
use sc_gamedata::domain::NoFiles;
use sc_gamedata::domains;
use sc_gamedata::output::iso_now;
use sc_gamedata::pipeline::{self, Game};
use sc_gamedata::source::{self, DataCoreSource};
use std::path::PathBuf;

fn golden_dir() -> PathBuf {
    std::env::var_os("GOLDEN_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_default();
            home.join("git/sc-gamedata/data")
        })
}

/// Datasets whose golden dump (sc-gamedata `data/`, build 4.10.200.58746) was built from
/// stale extracted records, so ours is right and the golden is not: 54 vehicles and two
/// items renamed or removed before that build (see the crate README, "Status").
const STALE_GOLDEN: &[&str] = &["game-fps-weapons", "game-item-tags", "game-ships"];

#[test]
#[ignore = "needs a LIVE install and the golden dumps"]
fn registered_domains_match_golden_on_live() {
    let p4k = source::find_data_p4k(None, "LIVE").expect("LIVE Data.p4k");
    let files = source::open(&p4k, false).expect("open archive");
    let records = DataCoreSource::parse(&files.dcb).expect("DataCore");
    let game = Game {
        records: &records,
        localization: &files.localization,
        files: &NoFiles,
        build: files.build.clone(),
    };
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.tmp/gamedata-live-test");
    std::fs::create_dir_all(&out).unwrap();
    let mut failures = Vec::new();
    for d in domains::all().into_iter().filter(|d| d.files().is_empty()) {
        let ds = pipeline::run(d, &game, &iso_now()).unwrap();
        let text = ds.to_json();
        std::fs::write(out.join(format!("{}.json", ds.name)), &text).unwrap();
        let golden = std::fs::read_to_string(golden_dir().join(format!("{}.json", ds.name)));
        let outcome = match golden {
            Ok(g) => compare_text(&text, &g),
            Err(_) => Outcome::MissingGolden,
        };
        println!("{}", outcome.report(ds.name, 20));
        if !outcome.is_ok() {
            failures.push(ds.name);
        }
    }
    let unexpected: Vec<_> = failures
        .iter()
        .filter(|f| !STALE_GOLDEN.contains(f))
        .collect();
    assert!(unexpected.is_empty(), "differ from golden: {unexpected:?}");
}
