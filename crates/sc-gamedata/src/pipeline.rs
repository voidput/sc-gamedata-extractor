//! Records -> scoped domains -> datasets (port of the parse half of
//! `sc-gamedata/src/pipeline.ts`).
//!
//! `sc-gamedata` first runs `starbreaker dcb extract --filter <glob>` once per filter of
//! every registered domain, then hands each domain the extracted records its own filters
//! match. Both steps matter for an exact match and both are reproduced by [`scope`]:
//!
//! 1. extraction: StarBreaker's `glob_match`, **case-sensitive**, against the DataCore
//!    file name *with* its `.xml` extension, over the union of every TypeScript domain's
//!    filters ([`GOLDEN_EXTRACTION_FILTERS`]);
//! 2. scoping: `sc-gamedata`'s own glob, **case-insensitive**, against the record path
//!    *without* extension, over the domain's filters.

use crate::domain::{Domain, DomainError, FileSource, ParseContext};
use crate::glob::{Glob, matches_any};
use crate::localization::Localization;
use crate::output;
use crate::records::{GameRecord, RecordSet, RecordSource, file_order};
use rayon::prelude::*;
use serde_json::Value;

/// Every filter of every domain in the `sc-gamedata` registry that produced the golden
/// dumps (build 4.10.200.58746), i.e. everything its extraction pulled from the DataCore,
/// plus the filters of domains added since (`game-fire`: `roomsystem/firehazard`; the
/// mission joins: `factions/factionreputation`, `missiondata/pu_missionlocality`,
/// `missiongiver`).
/// A ported domain's `filters()` must be a subset; a test checks the registry.
pub const GOLDEN_EXTRACTION_FILTERS: &[&str] = &[
    "**/actor/actors/**",
    "**/ammoparams/**",
    "**/cargomanifest/**",
    "**/commodityconfiguration/**",
    "**/consumabletypesdatabase/**",
    "**/contracts/contractgenerator/**",
    "**/contracts/contractscenarios/**",
    "**/contracts/contracttemplates/**",
    "**/crafting/**",
    "**/crafting/globalparams/**",
    "**/crafting/qualitydistribution/**",
    "**/crafting/qualityquantization/**",
    "**/damage/**",
    "**/entities/commodities/**",
    "**/entities/groundvehicles/**",
    "**/entities/mineable/**",
    "**/entities/monitoredzone",
    "**/entities/scitem/**",
    "**/entities/scitem/carryables/**",
    "**/entities/scitem/carryables/1h/**",
    "**/entities/scitem/characters/human/**",
    "**/entities/scitem/characters/human/armor/**",
    "**/entities/scitem/characters/human/clothing/**",
    "**/entities/scitem/characters/human/starwear/**",
    "**/entities/scitem/consumables/**",
    "**/entities/scitem/ships/**",
    "**/entities/scitem/ships/weapons/**",
    "**/entities/scitem/weapons/**",
    "**/entities/spaceships/**",
    "**/factions/factionreputation/**",
    "**/globalcargoloadingparams/**",
    "**/globalquantumdriveparams/**",
    "**/globalshopparams/**",
    "**/harvestable/**",
    "**/inventorycontainers/**",
    "**/lawsystem/infractiondefinitions/**",
    "**/lawsystem/jurisdictions/**",
    "**/loadoutkits/**",
    "**/lootgeneration/**",
    "**/mining/**",
    "**/mission_entities/monitorzonejammer",
    "**/missionbroker/**",
    "**/missiondata/pu_missionlocality/**",
    "**/missiongiver/**",
    "**/refiningprocess/**",
    "**/reputation/**",
    "**/resourcetypedatabase/**",
    "**/roomsystem/firehazard/**",
    "**/scitemmanufacturer/**",
    "**/sglobalsalvagerepairbeamparams/**",
    "**/starmap/**",
    "**/starmap/pu/*",
    "**/starmapamenitytypes/**",
    "**/starmapobject/**",
    "**/tagdatabase/**",
    "**/vehiclesalvageglobalparams/**",
];

/// Indices (into `source.metas()`) of the records a domain with `filters` is handed, in
/// `sc-gamedata` load order. An empty filter list means no records, not all of them (a
/// domain built only from archive files, `game-keybinds`).
pub fn scope(source: &dyn RecordSource, filters: &[&str]) -> Vec<usize> {
    if filters.is_empty() {
        return Vec::new();
    }
    let extracted: Vec<Glob> = GOLDEN_EXTRACTION_FILTERS
        .iter()
        .map(|g| Glob::case_sensitive(g))
        .collect();
    let own: Vec<Glob> = filters.iter().map(|g| Glob::new(g)).collect();
    let metas = source.metas();
    let mut picked: Vec<usize> = (0..metas.len())
        .into_par_iter()
        .filter(|&i| {
            let m = &metas[i];
            extracted.iter().any(|g| g.is_match(&m.file_name)) && matches_any(&m.record_path, &own)
        })
        .collect();
    picked.sort_by(|&a, &b| file_order(&metas[a].record_path, &metas[b].record_path));
    picked
}

/// Load the scoped records (in parallel). Records that fail to export or are not
/// envelopes are skipped with a warning, as `loadRecords` skips unreadable files.
pub fn load(source: &dyn RecordSource, indices: &[usize]) -> (RecordSet, Vec<String>) {
    load_pruned(source, indices, &|_| {})
}

/// [`load`], passing each record through `prune` as soon as it is read, so only the pruned
/// records are ever held together ([`Domain::prune`]).
pub fn load_pruned(
    source: &dyn RecordSource,
    indices: &[usize],
    prune: &(dyn Fn(&mut GameRecord) + Sync),
) -> (RecordSet, Vec<String>) {
    let loaded: Vec<Result<Option<GameRecord>, String>> = indices
        .par_iter()
        .map(|&i| {
            source.load(i).map(|r| {
                r.map(|mut r| {
                    prune(&mut r);
                    r
                })
            })
        })
        .collect();
    let mut records = Vec::with_capacity(loaded.len());
    let mut warnings = Vec::new();
    for (r, &i) in loaded.into_iter().zip(indices) {
        match r {
            Ok(Some(rec)) => records.push(rec),
            Ok(None) => {}
            Err(e) => warnings.push(format!("{}: {e}", source.metas()[i].name)),
        }
    }
    (RecordSet::new(records), warnings)
}

/// Everything a run needs besides the domains.
pub struct Game<'a> {
    pub records: &'a dyn RecordSource,
    pub localization: &'a Localization,
    pub files: &'a dyn FileSource,
    /// `_build` (from `build_manifest.id`); omitted from the header when `None`.
    pub build: Option<String>,
}

/// One dataset, ready to write.
pub struct Dataset {
    /// `game-manufacturers`
    pub name: &'static str,
    /// The full file content as a value (header included).
    pub value: Value,
    pub warnings: Vec<String>,
}

impl Dataset {
    /// The file text, exactly as `sc-gamedata` writes it.
    pub fn to_json(&self) -> String {
        output::to_js_json(&self.value)
    }
}

/// Run one domain.
pub fn run(domain: &dyn Domain, game: &Game<'_>, extracted: &str) -> Result<Dataset, DomainError> {
    let indices = scope(game.records, domain.filters());
    let (set, mut warnings) = load_pruned(game.records, &indices, &|r| domain.prune(r));
    if set.is_empty() && !domain.filters().is_empty() {
        warnings.push(format!(
            "{}: none of the loaded records match its filters ({}).",
            domain.name(),
            domain.filters().join(", ")
        ));
    }
    if domain.needs_localization() && game.localization.is_empty() {
        warnings.push(format!(
            "{}: no localization loaded, so display names will come out as @keys.",
            domain.name()
        ));
    }
    let ctx = ParseContext::new(&set, game.localization, game.files);
    let body = domain.parse(&ctx)?;
    warnings.extend(ctx.take_warnings());
    warnings.extend(empty_collection_warnings(domain.name(), &body));
    Ok(Dataset {
        name: domain.name(),
        value: output::envelope(
            body,
            extracted,
            game.build.as_deref(),
            &output::schema_tag(domain.name(), domain.schema_version()),
        ),
        warnings,
    })
}

/// The pipeline's "silent empties are how bad data ships" warnings.
fn empty_collection_warnings(name: &str, body: &serde_json::Map<String, Value>) -> Vec<String> {
    let counts: Vec<(&String, usize)> = body
        .iter()
        .filter(|(k, _)| k.as_str() != "summary")
        .filter_map(|(k, v)| match v {
            Value::Array(a) => Some((k, a.len())),
            Value::Object(o) => Some((k, o.len())),
            _ => None,
        })
        .collect();
    if counts.iter().all(|(_, n)| *n == 0) {
        return vec![format!(
            "{name}: produced no data at all. Its record selectors likely no longer match."
        )];
    }
    let empty: Vec<&str> = counts
        .iter()
        .filter(|(_, n)| *n == 0)
        .map(|(k, _)| k.as_str())
        .collect();
    if empty.is_empty() {
        Vec::new()
    } else {
        vec![format!("{name}: empty collection(s): {}", empty.join(", "))]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::FixtureSource;
    use serde_json::json;

    fn source() -> FixtureSource {
        let env = |name: &str, path: &str| {
            json!({"_RecordName_": name, "_RecordId_": name, "_RecordPath_": path,
                   "_RecordValue_": {"_Type_": "T"}})
        };
        FixtureSource::from_envelopes(vec![
            env("T.b", "libs/foundry/records/scitemmanufacturer/b.xml"),
            env("T.a", "libs/foundry/records/scitemmanufacturer/a.xml"),
            env("T.sub", "libs/foundry/records/scitemmanufacturer/a/x.xml"),
            env("T.upper", "libs/foundry/records/SCItemManufacturer/u.xml"),
            env("T.other", "libs/foundry/records/elsewhere/o.xml"),
            env("T.zone", "libs/foundry/records/entities/monitoredzone.xml"),
        ])
        .unwrap()
    }

    #[test]
    fn scope_reproduces_extraction_then_filtering() {
        let s = source();
        let names = |filters: &[&str]| -> Vec<String> {
            scope(&s, filters)
                .into_iter()
                .map(|i| s.metas()[i].name.clone())
                .collect()
        };
        // Load order: a.json < a/x.json < b.json. The upper-case directory was never
        // extracted (StarBreaker's glob is case-sensitive), though our filter would match.
        assert_eq!(
            names(&["**/scitemmanufacturer/**"]),
            vec!["T.a", "T.sub", "T.b"]
        );
        // Not extracted by any golden filter.
        assert!(names(&["**/elsewhere/**"]).is_empty());
        // `**/entities/monitoredzone` matches the path without `.xml` only, so it never
        // selected anything at extraction time.
        assert!(names(&["**/entities/monitoredzone"]).is_empty());
        // No filters: no records (archive-file domains), not every extracted one.
        assert!(names(&[]).is_empty());
    }

    /// Every registered domain's file (`<name>.json`) carries `_schema: "<stem>@<n>"`,
    /// right after `_build`, with `<n>` its declared version.
    #[test]
    fn every_domain_stamps_its_schema_after_build() {
        let records = FixtureSource::from_envelopes(Vec::new()).unwrap();
        let loc = Localization::default();
        let game = Game {
            records: &records,
            localization: &loc,
            files: &crate::domain::NoFiles,
            build: Some("4.10.201.48085".into()),
        };
        let domains = crate::domains::all();
        assert!(domains.len() >= 30);
        for d in domains {
            let ds = run(d, &game, "t").unwrap_or_else(|e| panic!("{}: {e}", d.name()));
            let file = format!("{}.json", ds.name);
            let stem = file.strip_suffix(".json").unwrap();
            assert!(d.schema_version() >= 1, "{file}");
            assert_eq!(
                ds.value["_schema"],
                format!("{stem}@{}", d.schema_version()),
                "{file}"
            );
            let keys: Vec<&String> = ds.value.as_object().unwrap().keys().take(4).collect();
            assert_eq!(
                keys,
                ["_source", "_extracted", "_build", "_schema"],
                "{file}"
            );
            assert!(
                ds.to_json()
                    .contains("\"_build\": \"4.10.201.48085\",\n  \"_schema\": "),
                "{file}"
            );
        }
    }

    #[test]
    fn empty_warnings() {
        let mut body = serde_json::Map::new();
        body.insert("rows".into(), json!([]));
        body.insert("summary".into(), json!({"n": 0}));
        assert_eq!(empty_collection_warnings("d", &body).len(), 1);
        body.insert("more".into(), json!([1]));
        assert_eq!(
            empty_collection_warnings("d", &body),
            vec!["d: empty collection(s): rows"]
        );
    }
}
