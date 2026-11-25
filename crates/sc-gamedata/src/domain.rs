//! What a domain is (port of `sc-gamedata/src/domains/types.ts`).

use crate::localization::Localization;
use crate::records::RecordSet;
use serde_json::{Map, Value};
use std::sync::Mutex;

/// Raw archive files, for domains whose data is not in the DataCore (object containers).
pub trait FileSource: Sync {
    /// Every archive entry name, as stored (backslash separated, `Data\...`).
    fn names(&self) -> &[String];
    /// Read one entry by exact name, then case-insensitively; `Err` when absent or unreadable.
    fn read(&self, name: &str) -> Result<Vec<u8>, String>;
}

/// Entry names a domain's `files` globs select, as `starbreaker p4k extract --filter` does:
/// case-sensitive, against the name with `\\` turned into `/` (`Data/ObjectContainers/...`).
pub fn files_matching<'a>(files: &'a dyn FileSource, globs: &[&str]) -> Vec<&'a str> {
    let globs: Vec<crate::glob::Glob> = globs
        .iter()
        .map(|g| crate::glob::Glob::case_sensitive(g))
        .collect();
    files
        .names()
        .iter()
        .filter(|n| {
            let n = n.replace('\\', "/");
            globs.iter().any(|g| g.is_match(&n))
        })
        .map(String::as_str)
        .collect()
}

/// A [`FileSource`] with no files.
pub struct NoFiles;

impl FileSource for NoFiles {
    fn names(&self) -> &[String] {
        &[]
    }
    fn read(&self, name: &str) -> Result<Vec<u8>, String> {
        Err(format!("{name}: no archive files in this run"))
    }
}

/// What a domain gets to work with (`ParseContext`).
pub struct ParseContext<'a> {
    /// The records this domain's filters select, in `sc-gamedata` load order.
    pub records: &'a RecordSet,
    /// The string table; empty when the run has none.
    pub localization: &'a Localization,
    /// Archive files. TypeScript domains read only what their `files` globs (and
    /// `moreFiles`) extracted; a port must read no more than that.
    pub files: &'a dyn FileSource,
    warnings: Mutex<Vec<String>>,
}

impl<'a> ParseContext<'a> {
    pub fn new(
        records: &'a RecordSet,
        localization: &'a Localization,
        files: &'a dyn FileSource,
    ) -> Self {
        Self {
            records,
            localization,
            files,
            warnings: Mutex::new(Vec::new()),
        }
    }

    /// A non-fatal finding, printed by the harness (the TypeScript logs these).
    pub fn warn(&self, message: impl Into<String>) {
        self.warnings
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(message.into());
    }

    pub fn take_warnings(&self) -> Vec<String> {
        std::mem::take(
            &mut self
                .warnings
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DomainError {
    #[error("{0}")]
    Message(String),
    #[error("cannot serialise output: {0}")]
    Serialize(#[from] serde_json::Error),
}

/// A domain turns DataCore records into one dataset, `<name>.json`.
///
/// `filters` are `sc-gamedata`'s globs, copied verbatim: a record is handed to the domain
/// when the golden extraction pulled it out of the archive and one of these matches its
/// record path (see [`crate::pipeline::scope`]).
pub trait Domain: Sync {
    /// Output base name, `game-manufacturers`.
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn filters(&self) -> &'static [&'static str];
    /// Archive file globs the TypeScript extracts as-is (`files`).
    fn files(&self) -> &'static [&'static str] {
        &[]
    }
    /// Whether output resolves `@keys` (`needsLocalization`).
    fn needs_localization(&self) -> bool {
        false
    }
    /// Drop the parts of a scoped record that [`Domain::parse`] never reads, as each record
    /// is loaded (memory: a domain handed 25,000 item records would otherwise hold them all
    /// as full JSON trees). Must not change the output: `scripts/compare-gamedata.sh` stays
    /// byte-identical. Default: keep everything.
    fn prune(&self, _record: &mut crate::records::GameRecord) {}
    /// The dataset body: every key after the `_source`/`_extracted`/`_build` header.
    fn parse(&self, ctx: &ParseContext<'_>) -> Result<Map<String, Value>, DomainError>;
}

/// Serialise a value into a body field; for domains building their body from structs.
pub fn field<T: serde::Serialize>(
    body: &mut Map<String, Value>,
    key: &str,
    value: &T,
) -> Result<(), DomainError> {
    body.insert(key.to_string(), serde_json::to_value(value)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::MemFiles;

    #[test]
    fn files_match_like_starbreaker() {
        let mut f = MemFiles::default();
        f.insert(
            r"Data\ObjectContainers\PU\system\stanton\stantonsystem.socpak",
            vec![1],
        );
        f.insert(r"Data\objectcontainers\pu\system\x\xsystem.socpak", vec![2]);
        let hits = files_matching(&f, &["**/ObjectContainers/PU/system/*/*system.socpak"]);
        assert_eq!(hits.len(), 1, "case-sensitive");
        assert_eq!(f.read(hits[0]).unwrap(), vec![1]);
        assert!(NoFiles.read("x").is_err());
    }
}
