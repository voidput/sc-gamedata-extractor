//! The real game: `Data.p4k` via the StarBreaker adapter in [`crate::archive`].
//!
//! Read-only. The archive is memory-mapped by StarBreaker and never written; reading LIVE
//! while the game runs is fine.

use crate::archive::{Archive, FullDataCore};
use crate::archive::{DCB_ENTRY, DCB_FALLBACK_ENTRY, INI_ENTRY, guarded};
use crate::domain::FileSource;
use crate::localization::{Localization, parse_global_ini_bytes};
use crate::records::{RecordMeta, RecordSource, record_path_of_file_name};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Main DataCore records as a [`RecordSource`].
pub struct DataCoreSource<'a> {
    dc: FullDataCore<'a>,
    metas: Vec<RecordMeta>,
}

impl<'a> DataCoreSource<'a> {
    /// Parse DataCore bytes (StarBreaker panics are turned into errors).
    pub fn parse(bytes: &'a [u8]) -> Result<Self, String> {
        let dc = guarded("DataCore parse", || {
            FullDataCore::parse(bytes).map_err(crate::archive::ExtractError::DataCore)
        })
        .map_err(|e| e.to_string())?;
        let metas = dc
            .records()
            .iter()
            .map(|r| {
                let file_name = r.file_name.replace('\\', "/");
                RecordMeta {
                    name: r.name.clone(),
                    record_path: record_path_of_file_name(&file_name),
                    file_name,
                    struct_type: r.struct_type.clone(),
                    id: r.id.clone(),
                }
            })
            .collect();
        Ok(Self { dc, metas })
    }
}

impl RecordSource for DataCoreSource<'_> {
    fn metas(&self) -> &[RecordMeta] {
        &self.metas
    }

    fn envelope(&self, index: usize) -> Result<Value, String> {
        let bytes =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.dc.export_json(index)))
                .map_err(|p| crate::archive::panic_message(p.as_ref()))??;
        serde_json::from_slice(&bytes).map_err(|e| format!("export is not JSON: {e}"))
    }
}

/// Raw archive entries as a [`FileSource`].
pub struct ArchiveFiles {
    archive: Archive,
    names: Vec<String>,
}

impl ArchiveFiles {
    pub fn new(archive: Archive) -> Self {
        let names = archive.entry_names().map(str::to_string).collect();
        Self { archive, names }
    }
}

impl FileSource for ArchiveFiles {
    fn names(&self) -> &[String] {
        &self.names
    }

    fn read(&self, name: &str) -> Result<Vec<u8>, String> {
        let info = self
            .archive
            .entry(name)
            .ok_or_else(|| format!("{name}: not in archive"))?;
        self.archive.read(&info).map_err(|e| e.to_string())
    }
}

/// What [`open`] reads out of the archive.
pub struct GameFiles {
    /// DataCore bytes (`Game2.dcb`, else `Game.dcb`).
    pub dcb: Vec<u8>,
    pub localization: Localization,
    /// `_build` for the header.
    pub build: Option<String>,
    /// The open archive, for domains reading raw files; `None` when not asked for.
    pub archive: Option<Archive>,
}

/// Open `p4k` and read the DataCore and the English string table. With `keep_archive`
/// the archive stays open (its index is about 400 MB).
pub fn open(p4k: &Path, keep_archive: bool) -> Result<GameFiles, String> {
    let archive = Archive::open(p4k).map_err(|e| e.to_string())?;
    let read = |name: &str| -> Option<Result<Vec<u8>, String>> {
        let info = archive.entry(name)?;
        Some(archive.read(&info).map_err(|e| e.to_string()))
    };
    let dcb = read(DCB_ENTRY)
        .or_else(|| read(DCB_FALLBACK_ENTRY))
        .ok_or("no DataCore (Game2.dcb / Game.dcb) in the archive")??;
    let localization = match read(INI_ENTRY) {
        Some(bytes) => parse_global_ini_bytes(&bytes?),
        None => Localization::new(),
    };
    Ok(GameFiles {
        dcb,
        localization,
        build: read_build_version(p4k),
        archive: keep_archive.then_some(archive),
    })
}

/// `readBuildVersion`: `Data.RequestedP4kVersion ?? Data.Version ?? Data.Branch` from the
/// `build_manifest.id` beside `Data.p4k`.
pub fn read_build_version(p4k: &Path) -> Option<String> {
    let manifest = p4k.parent()?.join("build_manifest.id");
    let parsed: Value = serde_json::from_slice(&std::fs::read(manifest).ok()?).ok()?;
    let data = parsed.get("Data")?;
    ["RequestedP4kVersion", "Version", "Branch"]
        .iter()
        .find_map(|k| data.get(*k).and_then(Value::as_str))
        .map(str::to_string)
}

const PREFIX_SUFFIX: &str = "drive_c/Program Files/Roberts Space Industries/StarCitizen";

/// `findDataP4k`: an explicit path, else `SC_DATA_P4K`, else the usual install locations
/// (Windows drive roots; on Linux Wine prefixes one or two levels below `/mnt`, `/media`,
/// `~/Games` and `~`).
pub fn find_data_p4k(explicit: Option<&Path>, channel: &str) -> Result<PathBuf, String> {
    if let Some(p) = explicit {
        return p
            .is_file()
            .then(|| p.to_path_buf())
            .ok_or_else(|| format!("Data.p4k not found at: {}", p.display()));
    }
    if let Some(env) = std::env::var_os("SC_DATA_P4K") {
        let p = PathBuf::from(env);
        return p
            .is_file()
            .then_some(p.clone())
            .ok_or_else(|| format!("SC_DATA_P4K points at a missing file: {}", p.display()));
    }
    for root in game_roots() {
        let candidate = root.join(channel).join("Data.p4k");
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(format!(
        "Could not find Data.p4k for channel {channel}. Pass --p4k <path> or set SC_DATA_P4K."
    ))
}

fn game_roots() -> Vec<PathBuf> {
    if cfg!(windows) {
        return ["C", "D", "E", "F"]
            .iter()
            .map(|d| {
                PathBuf::from(format!(
                    "{d}:/Program Files/Roberts Space Industries/StarCitizen"
                ))
            })
            .collect();
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut bases = vec![PathBuf::from("/mnt"), PathBuf::from("/media")];
    if let Some(h) = &home {
        bases.push(h.join("Games"));
        bases.push(h.clone());
    }
    let dirs = |d: &Path| -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = std::fs::read_dir(d)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().is_dir())
            .map(|e| e.path())
            .collect();
        v.sort();
        v
    };
    let mut roots = Vec::new();
    for base in bases.iter().filter(|b| b.exists()) {
        for first in dirs(base) {
            roots.push(first.join(PREFIX_SUFFIX));
            for second in dirs(&first) {
                roots.push(second.join(PREFIX_SUFFIX));
            }
        }
    }
    roots
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_version_prefers_requested_then_version() {
        let dir = tempfile::tempdir().unwrap();
        let p4k = dir.path().join("Data.p4k");
        let manifest = dir.path().join("build_manifest.id");
        std::fs::write(
            &manifest,
            r#"{"Data":{"Branch":"sc-alpha-4.10.0","Version":"4.10.200.58746"}}"#,
        )
        .unwrap();
        assert_eq!(read_build_version(&p4k).as_deref(), Some("4.10.200.58746"));
        std::fs::write(
            &manifest,
            r#"{"Data":{"Branch":"b","Version":"v","RequestedP4kVersion":"r"}}"#,
        )
        .unwrap();
        assert_eq!(read_build_version(&p4k).as_deref(), Some("r"));
        std::fs::write(&manifest, "not json").unwrap();
        assert_eq!(read_build_version(&p4k), None);
    }

    #[test]
    fn explicit_missing_p4k_is_an_error() {
        assert!(find_data_p4k(Some(Path::new("/nonexistent/Data.p4k")), "LIVE").is_err());
    }
}
