//! Thin adapter over the StarBreaker crates: open `Data.p4k`, read entries, parse DataCore.
//!
//! Duplicated from starorg-strings' `starorg-data` (`starbreaker.rs`, `extract.rs`), keeping
//! only what the game-data pipeline needs, so this repo does not pull in the app's asset
//! export stack. Everything StarBreaker-specific goes through this module; a rev bump
//! touches this file and the pin in the workspace `Cargo.toml` only.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

/// The StarBreaker commit we build against. Must equal the `rev` in the workspace
/// `Cargo.toml`; a test checks this.
pub const STARBREAKER_COMMIT: &str = "041e559215e18e1b13b124cab0e3ecb0f3aa50d4";

/// Entry names inside the p4k use backslashes; written as literals, never built with
/// `Path::join`.
pub const INI_ENTRY: &str = "Data\\Localization\\english\\global.ini";
pub const DCB_ENTRY: &str = "Data\\Game2.dcb";
/// Used only when `Game2.dcb` is absent.
pub const DCB_FALLBACK_ENTRY: &str = "Data\\Game.dcb";

/// Error from the StarBreaker layer.
#[derive(Debug, thiserror::Error)]
pub enum ArchiveError {
    #[error("cannot open archive: {0}")]
    Open(String),
    #[error("cannot read entry {name}: {detail}")]
    Read { name: String, detail: String },
}

#[derive(Debug, thiserror::Error)]
pub enum ExtractError {
    #[error(transparent)]
    Archive(#[from] ArchiveError),
    #[error("DataCore parse failed: {0}")]
    DataCore(String),
    #[error("panic during {stage}: {message}")]
    Panic {
        stage: &'static str,
        message: String,
    },
}

/// Text of a panic payload.
pub fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic payload".to_string()
    }
}

/// Run `f` and turn a panic into [`ExtractError::Panic`]. StarBreaker may panic on
/// malformed data, so the release profile must keep `panic = "unwind"`.
pub fn guarded<T>(
    stage: &'static str,
    f: impl FnOnce() -> Result<T, ExtractError>,
) -> Result<T, ExtractError> {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(r) => r,
        Err(payload) => Err(ExtractError::Panic {
            stage,
            message: panic_message(payload.as_ref()),
        }),
    }
}

/// Metadata of one archive entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryInfo {
    pub name: String,
    pub uncompressed_size: u64,
    pub crc32: u32,
}

/// An opened `Data.p4k`. Holds StarBreaker's in-memory index (about 400 MB on LIVE).
pub struct Archive {
    inner: starbreaker_p4k::MappedP4k,
}

impl Archive {
    pub fn open(path: &Path) -> Result<Self, ArchiveError> {
        starbreaker_p4k::MappedP4k::open(path)
            .map(|inner| Self { inner })
            .map_err(|e| ArchiveError::Open(e.to_string()))
    }

    /// Number of entries StarBreaker parsed from the central directory.
    pub fn parsed_entries(&self) -> usize {
        self.inner.len()
    }

    /// Look up an entry by its exact name (backslash separated), then case-insensitively.
    pub fn entry(&self, name: &str) -> Option<EntryInfo> {
        let e = self
            .inner
            .entry(name)
            .or_else(|| self.inner.entry_case_insensitive(name))?;
        Some(EntryInfo {
            name: e.name.clone(),
            uncompressed_size: e.uncompressed_size,
            crc32: e.crc32,
        })
    }

    /// Read and decompress an entry found with [`Archive::entry`].
    pub fn read(&self, info: &EntryInfo) -> Result<Vec<u8>, ArchiveError> {
        let entry = self
            .inner
            .entry(&info.name)
            .ok_or_else(|| ArchiveError::Read {
                name: info.name.clone(),
                detail: "entry vanished from index".into(),
            })?;
        self.inner.read(entry).map_err(|e| ArchiveError::Read {
            name: info.name.clone(),
            detail: e.to_string(),
        })
    }

    /// Every entry name in the archive, in index order (backslash separated).
    pub fn entry_names(&self) -> impl Iterator<Item = &str> {
        self.inner.entries().iter().map(|e| e.name.as_str())
    }
}

/// One main DataCore record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawRecordMeta {
    /// `_RecordName_`, `SCItemManufacturer.AEGS`.
    pub name: String,
    /// The DataCore file name as stored, `libs/foundry/records/scitemmanufacturer/aegs.xml`.
    pub file_name: String,
    /// DataCore struct type (equals `_RecordValue_._Type_`).
    pub struct_type: String,
    /// `_RecordId_`, lower-case hyphenated GUID.
    pub id: String,
}

/// Every main record of a DataCore, exported on demand with the JSON exporter the
/// StarBreaker CLI's `dcb extract --format json` uses.
pub struct FullDataCore<'a> {
    db: starbreaker_datacore::Database<'a>,
    metas: Vec<RawRecordMeta>,
    positions: Vec<usize>,
}

impl<'a> FullDataCore<'a> {
    /// Parse DataCore bytes. May panic on malformed data; run under [`guarded`].
    pub fn parse(bytes: &'a [u8]) -> Result<Self, String> {
        let db = starbreaker_datacore::Database::from_bytes(bytes).map_err(|e| e.to_string())?;
        let mut metas = Vec::new();
        let mut positions = Vec::new();
        for (i, r) in db.records().iter().enumerate() {
            if !db.is_main_record(r) {
                continue;
            }
            metas.push(RawRecordMeta {
                name: db.resolve_string2(r.name_offset).to_string(),
                file_name: db.resolve_string(r.file_name_offset).to_string(),
                struct_type: db.struct_name(r.struct_id()).to_string(),
                id: r.id.to_string(),
            });
            positions.push(i);
        }
        Ok(Self {
            db,
            metas,
            positions,
        })
    }

    /// Main records, in DataCore order.
    pub fn records(&self) -> &[RawRecordMeta] {
        &self.metas
    }

    /// The export envelope of `records()[index]`, as compact JSON bytes.
    pub fn export_json(&self, index: usize) -> Result<Vec<u8>, String> {
        let pos = *self
            .positions
            .get(index)
            .ok_or_else(|| format!("record {index}: out of range"))?;
        starbreaker_datacore::export::to_json_compact(&self.db, &self.db.records()[pos])
            .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commit_constant_matches_workspace_pin() {
        let manifest = include_str!("../../../Cargo.toml");
        let pins: Vec<&str> = manifest
            .lines()
            .filter(|l| l.starts_with("starbreaker-"))
            .collect();
        assert!(!pins.is_empty(), "StarBreaker crates are pinned");
        for line in pins {
            assert!(
                line.contains(&format!("rev = \"{STARBREAKER_COMMIT}\"")),
                "pin and STARBREAKER_COMMIT differ: {line}"
            );
        }
    }

    #[test]
    fn open_missing_file_is_an_error() {
        let err = Archive::open(Path::new("/nonexistent/Data.p4k")).err();
        assert!(matches!(err, Some(ArchiveError::Open(_))));
    }
}
