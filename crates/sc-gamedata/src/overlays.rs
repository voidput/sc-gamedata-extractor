//! Community string overlays: the pack registry, fetching a pack, finding an installed one,
//! and loading a pack from disk (port of `sc-gamedata/src/sources/overlays.ts` and
//! `loadOverlay` in `localization.ts`; `applyOverlay`/`diffStrings` are in
//! [`crate::localization`]).
//!
//! An overlay is `key=value` lines in the `global.ini` format, so any file or directory of
//! `.ini` files works; the registry only lets a pack be fetched by name. This crate has no
//! HTTP client (the CLI's default build carries none): [`fetch_source`] takes the download
//! as a function, as the TypeScript takes `fetchImpl`, and does the naming and writing.

use crate::collate::locale_compare;
use crate::localization::{Localization, apply_overlay, parse_global_ini_bytes};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// One pack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlaySource {
    pub name: String,
    pub description: String,
    /// Where to read about it.
    pub homepage: Option<String>,
    /// Files to download, as full HTTPS URLs, applied in order.
    pub files: Vec<String>,
    /// Rebuilt per game patch, so an old copy should not be layered onto a new build.
    pub per_patch: bool,
}

const RAW: &str = "https://raw.githubusercontent.com";

/// Known packs. A convenience, not a gate: any pack works as a local overlay.
pub fn built_in_sources() -> Vec<OverlaySource> {
    let file = |f: &str| format!("{RAW}/MrKraken/StarStrings/master/src/For_Tool_Creators/{f}");
    vec![OverlaySource {
        name: "starstrings".into(),
        description: "MrKraken's StarStrings. Component type/size/grade prefixes, ore RS values, blueprint pools on contracts.".into(),
        homepage: Some("https://github.com/MrKraken/StarStrings".into()),
        files: ["components.ini", "contracts.ini", "mining.ini", "ordnance.ini"]
            .iter()
            .map(|f| file(f))
            .collect(),
        per_patch: true,
    }]
}

/// Where user-defined sources live (`overlay-sources.json` in `config_dir`).
pub fn sources_file(config_dir: &Path) -> PathBuf {
    config_dir.join("overlay-sources.json")
}

/// `validateSource`: check one user entry. Errors carry the TypeScript messages.
pub fn validate_source(entry: &Value) -> Result<OverlaySource, String> {
    let Some(obj) = entry.as_object() else {
        return Err("Overlay source must be an object.".into());
    };
    let name = obj
        .get("name")
        .and_then(Value::as_str)
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    if name.is_empty() {
        return Err("Overlay source needs a name.".into());
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    {
        return Err(format!(
            "Overlay source name \"{name}\" may only contain letters, digits, . _ and -."
        ));
    }
    let files: Vec<String> = obj
        .get("files")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    if files.is_empty() {
        return Err(format!("Overlay source \"{name}\" lists no files."));
    }
    for file in &files {
        // Only fetch over HTTPS; a local pack is used as a local overlay instead.
        let https = file
            .get(..8)
            .is_some_and(|p| p.eq_ignore_ascii_case("https://"));
        if !https {
            return Err(format!(
                "Overlay source \"{name}\" has a non-HTTPS URL: {file}"
            ));
        }
    }
    Ok(OverlaySource {
        description: obj
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        homepage: obj
            .get("homepage")
            .and_then(Value::as_str)
            .map(str::to_string),
        files,
        per_patch: obj.get("perPatch") != Some(&Value::Bool(false)),
        name,
    })
}

/// `loadSources`: the built-ins plus the user's `overlay-sources.json` (a bare array or
/// `{ sources: [...] }`); a user entry replaces a built-in of the same name. Sorted by
/// name. A missing file is normal; a malformed one is an error.
pub fn load_sources(config_dir: &Path) -> Result<Vec<OverlaySource>, String> {
    let mut merged = built_in_sources();
    let file = sources_file(config_dir);
    match std::fs::read(&file) {
        Ok(raw) => {
            let fail = |e: String| format!("Could not read {}: {e}", file.display());
            let parsed: Value = serde_json::from_slice(&raw).map_err(|e| fail(e.to_string()))?;
            let entries = match &parsed {
                Value::Array(a) => a.clone(),
                other => other
                    .get("sources")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default(),
            };
            for entry in &entries {
                let source = validate_source(entry).map_err(fail)?;
                match merged.iter_mut().find(|s| s.name == source.name) {
                    Some(slot) => *slot = source,
                    None => merged.push(source),
                }
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("Could not read {}: {e}", file.display())),
    }
    merged.sort_by(|a, b| locale_compare(&a.name, &b.name));
    Ok(merged)
}

/// The file name a URL ends with, used as the local name.
fn file_name_of(url: &str, index: usize) -> String {
    let path = url.split('?').next().unwrap_or_default();
    let tail = path.rsplit('/').next().unwrap_or_default();
    if tail.to_ascii_lowercase().ends_with(".ini") {
        return tail.to_string();
    }
    let tail = if tail.is_empty() { "overlay" } else { tail };
    format!("{index:02}-{tail}.ini")
}

/// A downloaded file, as the caller's HTTP client returns it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchResponse {
    pub status: u16,
    pub text: String,
}

/// One written file: its path and `text.length` (UTF-16 units, as the TypeScript reports).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchedFile {
    pub file: PathBuf,
    pub bytes: usize,
}

/// `fetchSource`: download each of the source's files with `fetch` and write them into
/// `out_dir`, prefixed with their index so a directory load applies them in published
/// order. `fetch` returns the response or a transport error.
pub fn fetch_source(
    source: &OverlaySource,
    out_dir: &Path,
    mut fetch: impl FnMut(&str) -> Result<FetchResponse, String>,
) -> Result<Vec<FetchedFile>, String> {
    std::fs::create_dir_all(out_dir).map_err(|e| format!("{}: {e}", out_dir.display()))?;
    let mut written = Vec::new();
    for (index, url) in source.files.iter().enumerate() {
        let response = fetch(url)?;
        if !(200..300).contains(&response.status) {
            return Err(format!(
                "{}: {url} returned HTTP {}",
                source.name, response.status
            ));
        }
        let file = out_dir.join(format!("{index:02}-{}", file_name_of(url, index)));
        std::fs::write(&file, &response.text).map_err(|e| format!("{}: {e}", file.display()))?;
        written.push(FetchedFile {
            file,
            bytes: response.text.encode_utf16().count(),
        });
    }
    Ok(written)
}

/// `findInstalledOverride`: a loose `data/Localization/<language>/global.ini` (or `Data/`)
/// beside the archive, which the game reads in preference to the archived table.
pub fn find_installed_override(p4k: &Path, language: &str) -> Option<PathBuf> {
    let game_dir = p4k.parent().unwrap_or_else(|| Path::new(""));
    ["data", "Data"]
        .iter()
        .map(|d| {
            game_dir
                .join(d)
                .join("Localization")
                .join(language)
                .join("global.ini")
        })
        .find(|c| c.is_file())
}

/// `loadOverlay`: every `.ini` file directly in a directory (sorted by name), or one file,
/// layered in order as one table.
pub fn load_overlay(target: &Path) -> std::io::Result<Localization> {
    let mut files = Vec::new();
    if std::fs::metadata(target)?.is_dir() {
        for entry in std::fs::read_dir(target)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if entry.file_type()?.is_file() && name.ends_with(".ini") {
                files.push(entry.path());
            }
        }
        // `Array.prototype.sort` on the joined paths: UTF-16 order.
        files.sort_by(|a, b| {
            let (a, b) = (a.to_string_lossy(), b.to_string_lossy());
            a.encode_utf16().cmp(b.encode_utf16())
        });
    } else {
        files.push(target.to_path_buf());
    }
    let mut table = Localization::new();
    for file in files {
        table = apply_overlay(&table, &parse_global_ini_bytes(&std::fs::read(file)?));
    }
    Ok(table)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn valid() -> Value {
        json!({"name": "mypack", "files": ["https://example.com/a.ini"]})
    }

    fn with(mut v: Value, k: &str, x: Value) -> Value {
        v[k] = x;
        v
    }

    #[test]
    fn validate_accepts_a_minimal_source_and_defaults_per_patch() {
        let s = validate_source(&valid()).unwrap();
        assert_eq!(s.name, "mypack");
        assert!(s.per_patch);
        assert!(
            !validate_source(&with(valid(), "perPatch", json!(false)))
                .unwrap()
                .per_patch
        );
    }

    #[test]
    fn validate_requires_a_name_and_a_file() {
        let e = validate_source(&json!({"files": ["https://example.com/a.ini"]})).unwrap_err();
        assert!(e.contains("needs a name"), "{e}");
        let e = validate_source(&json!({"name": "x", "files": []})).unwrap_err();
        assert!(e.contains("lists no files"), "{e}");
        assert!(
            validate_source(&json!(3))
                .unwrap_err()
                .contains("must be an object")
        );
    }

    #[test]
    fn validate_rejects_a_name_that_could_escape_a_directory() {
        for name in ["../etc", "a/b"] {
            let e = validate_source(&with(valid(), "name", json!(name))).unwrap_err();
            assert!(e.contains("may only contain"), "{e}");
        }
    }

    #[test]
    fn validate_rejects_a_non_https_url() {
        let e = validate_source(&with(valid(), "files", json!(["http://example.com/a.ini"])))
            .unwrap_err();
        assert!(e.contains("non-HTTPS"), "{e}");
    }

    #[test]
    fn load_sources_merges_user_packs() {
        let dir = tempfile::tempdir().unwrap();
        let names = |d: &Path| -> Vec<String> {
            load_sources(d)
                .unwrap()
                .into_iter()
                .map(|s| s.name)
                .collect()
        };
        assert_eq!(names(dir.path()), vec!["starstrings"]);

        let file = sources_file(dir.path());
        std::fs::write(
            &file,
            json!([{"name": "mypack", "files": ["https://example.com/a.ini"]}]).to_string(),
        )
        .unwrap();
        assert_eq!(names(dir.path()), vec!["mypack", "starstrings"]);

        // A user entry replaces a built-in of the same name.
        std::fs::write(
            &file,
            json!([{"name": "starstrings", "description": "pinned", "files": ["https://example.com/pinned.ini"]}]).to_string(),
        )
        .unwrap();
        let s = load_sources(dir.path()).unwrap();
        let star = s.iter().find(|s| s.name == "starstrings").unwrap();
        assert_eq!(star.description, "pinned");
        assert_eq!(star.files.len(), 1);

        // A { sources: [...] } wrapper as well as a bare array.
        std::fs::write(
            &file,
            json!({"sources": [{"name": "wrapped", "files": ["https://example.com/a.ini"]}]})
                .to_string(),
        )
        .unwrap();
        assert!(names(dir.path()).contains(&"wrapped".to_string()));

        // A malformed file is reported, not ignored.
        std::fs::write(&file, json!([{"name": "bad"}]).to_string()).unwrap();
        assert!(
            load_sources(dir.path())
                .unwrap_err()
                .contains("lists no files")
        );
    }

    fn source(files: &[&str]) -> OverlaySource {
        OverlaySource {
            name: "p".into(),
            description: String::new(),
            homepage: None,
            files: files.iter().map(|s| s.to_string()).collect(),
            per_patch: true,
        }
    }

    #[test]
    fn fetch_names_files_so_lexical_order_matches_published_order() {
        let dir = tempfile::tempdir().unwrap();
        let written = fetch_source(
            &source(&["https://x/components.ini", "https://x/contracts.ini"]),
            dir.path(),
            |_| {
                Ok(FetchResponse {
                    status: 200,
                    text: "k=v".into(),
                })
            },
        )
        .unwrap();
        let names: Vec<String> = written
            .iter()
            .map(|w| w.file.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["00-components.ini", "01-contracts.ini"]);
        assert_eq!(written[0].bytes, 3);
        assert_eq!(file_name_of("https://x/raw?token=1", 3), "03-raw.ini");
        assert_eq!(file_name_of("https://x/", 0), "00-overlay.ini");
    }

    #[test]
    fn fetch_names_the_pack_and_url_on_an_http_error() {
        let dir = tempfile::tempdir().unwrap();
        let e = fetch_source(&source(&["https://x/a.ini"]), dir.path(), |_| {
            Ok(FetchResponse {
                status: 404,
                text: String::new(),
            })
        })
        .unwrap_err();
        assert_eq!(e, "p: https://x/a.ini returned HTTP 404");
    }

    #[test]
    fn find_installed_override_checks_both_spellings_and_the_language() {
        let dir = tempfile::tempdir().unwrap();
        let p4k = dir.path().join("Data.p4k");
        assert_eq!(find_installed_override(&p4k, "english"), None);
        // The real install uses "Data"; checking only lowercase misses it.
        let target = dir.path().join("Data").join("Localization").join("english");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("global.ini"), "k=v").unwrap();
        let found = find_installed_override(&p4k, "english").unwrap();
        assert_eq!(found.file_name().unwrap(), "global.ini");
        assert!(found.starts_with(dir.path()));
        assert_eq!(find_installed_override(&p4k, "german_(germany)"), None);
    }

    #[test]
    fn load_overlay_layers_a_directory_in_name_order() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("01-b.ini"), "k=second\nonly_b=1").unwrap();
        std::fs::write(dir.path().join("00-a.INI"), "k=first\nonly_a=1").unwrap();
        std::fs::write(dir.path().join("notes.txt"), "k=ignored").unwrap();
        let t = load_overlay(dir.path()).unwrap();
        assert_eq!(t.get("k"), Some("second"));
        assert_eq!(t.get("only_a"), Some("1"));
        assert_eq!(t.len(), 3);
        let one = load_overlay(&dir.path().join("00-a.INI")).unwrap();
        assert_eq!(one.get("k"), Some("first"));
        assert!(load_overlay(&dir.path().join("missing")).is_err());
    }
}
