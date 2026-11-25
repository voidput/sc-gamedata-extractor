//! Port of `scripts/publish-dump.mjs` without its GitHub release step: one build's parsed
//! datasets, redacted ([`crate::localization::redaction::redact_dump`]), committed and
//! tagged in the public dumps checkout, with patch notes against the previous published
//! build on the same release line.
//!
//! - [`plan`] reads `data/`, redacts, resolves the build, channel and release title, and
//!   computes the notes from the checkout as it stands (no fetch);
//! - [`render_dry_run`] is the `--dry-run` stdout; [`PublishPlan::warnings`] its stderr;
//! - [`stage`] clones or fetches the checkout, writes the template files, `data/` and
//!   `notes/`, commits and tags, and pushes only when told to.
//!
//! Creating or editing the GitHub release (`gh release ...`) stays in the TypeScript
//! script; [`release_title`], [`release_flags`] and [`release_notes`] are its inputs.

use crate::diff::{
    BuildSource, Error, Result, js_string, js_to_number, js_truthy, normalize_numbers, sort_utf16,
};
use crate::localization::redaction::redact_dump;
use crate::notes::{NotesOptions, notes_against, render_notes};
use crate::output::{js_key_order, to_js_json_compact};
use serde_json::{Map, Value};
use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Channels whose builds are test builds, published as GitHub prereleases.
pub const PRERELEASE_CHANNELS: &[&str] = &["PTU", "EPTU", "TECH-PREVIEW"];

/// The files of `scripts/dump-repo-template`: `.github/` is always rewritten, the rest is
/// copied into the checkout when missing.
pub const TEMPLATE_FILES: &[(&str, &str)] = &[
    (
        ".github/workflows/fanout.yml",
        include_str!("../../../scripts/dump-repo-template/.github/workflows/fanout.yml"),
    ),
    (
        "NOTICE",
        include_str!("../../../scripts/dump-repo-template/NOTICE"),
    ),
    (
        "README.md",
        include_str!("../../../scripts/dump-repo-template/README.md"),
    ),
];

/// Where to read from and what to call the release. `None` paths take the script's
/// defaults (environment, then the checkout `root`).
#[derive(Debug, Clone, Default)]
pub struct PublishOptions {
    /// The sc-gamedata checkout.
    pub root: PathBuf,
    /// Default `$SC_DATA_DIR`, else `<root>/data`.
    pub data_dir: Option<PathBuf>,
    /// Default `$SC_DUMPS_WORKDIR`, else `<root>/.publish-repo`.
    pub work_dir: Option<PathBuf>,
    /// Default `$SC_EXTRACT_DIR`, else `<root>/extracted-data` (holds `_build.json`).
    pub extract_dir: Option<PathBuf>,
    /// Default `$SC_DUMPS_REPO`, else `voidput/sc-gamedata-dumps`.
    pub repo: Option<String>,
    /// Default `$SC_DUMPS_REMOTE`, else `git@github.com:<repo>.git`.
    pub remote: Option<String>,
    /// `--channel` (upper-cased here).
    pub channel: Option<String>,
    /// `--patch`: must look like `4.10.3`.
    pub patch: Option<String>,
    /// `--republish`.
    pub republish: bool,
}

/// One dataset as it will be published.
#[derive(Debug, Clone)]
pub struct Dump {
    pub file: String,
    pub parsed: Value,
    /// `JSON.stringify` bytes before and after redaction.
    pub original: usize,
    pub size: usize,
}

/// Channel and branch of the build (`resolveOrigin`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Origin {
    pub channel: String,
    pub branch: Option<String>,
}

/// One file under `notes/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotesFile {
    pub name: String,
    pub body: String,
}

/// The patch notes of a publish, or why there are none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublishNotes {
    Written { from: String, files: Vec<NotesFile> },
    Skipped(String),
}

/// Everything a publish would write, before touching the checkout.
#[derive(Debug, Clone)]
pub struct PublishPlan {
    pub build: String,
    pub repo: String,
    pub remote: String,
    pub work_dir: PathBuf,
    pub origin: Origin,
    pub patch: Option<String>,
    pub republish: bool,
    pub dumps: Vec<Dump>,
    pub notes: PublishNotes,
    /// What the script prints to stderr, in order.
    pub warnings: Vec<String>,
}

fn env_or(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

/// `/^\d+(\.\d+)+$/`.
fn is_version(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    parts.len() >= 2
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

/// `readDumps`: every `game-*.json` of `data_dir`, as published.
pub fn read_dumps(data_dir: &Path) -> Result<Vec<Dump>> {
    if !data_dir.exists() {
        return Err(Error::Message(format!(
            "No data directory at {}. Run `npm run all` first.",
            data_dir.display()
        )));
    }
    let io = |e| Error::Io {
        path: data_dir.to_path_buf(),
        source: e,
    };
    let mut files: Vec<String> = std::fs::read_dir(data_dir)
        .map_err(io)?
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|f| f.starts_with("game-") && f.ends_with(".json"))
        .collect();
    if files.is_empty() {
        return Err(Error::Message(format!(
            "No game-*.json in {}.",
            data_dir.display()
        )));
    }
    sort_utf16(&mut files);
    let mut dumps = Vec::new();
    for file in files {
        let path = data_dir.join(&file);
        let bytes = std::fs::read(&path).map_err(|e| Error::Io {
            path: path.clone(),
            source: e,
        })?;
        let mut parsed: Value =
            serde_json::from_str(&String::from_utf8_lossy(&bytes)).map_err(|source| {
                Error::Json {
                    file: path.display().to_string(),
                    source,
                }
            })?;
        normalize_numbers(&mut parsed);
        let original = to_js_json_compact(&parsed).len();
        let published = redact_dump(&file, &parsed);
        let size = to_js_json_compact(&published).len();
        dumps.push(Dump {
            file,
            parsed: published,
            original,
            size,
        });
    }
    Ok(dumps)
}

/// `resolveBuild`: the one build string every dump is stamped with.
pub fn resolve_build(dumps: &[Dump]) -> Result<String> {
    let mut builds: Vec<String> = Vec::new();
    for d in dumps {
        let b = d.parsed.get("_build");
        if js_truthy(b) {
            let s = js_string(b);
            if !builds.contains(&s) {
                builds.push(s);
            }
        }
    }
    match builds.len() {
        0 => Err(Error::Message(
            "No _build stamp in any dump — cannot tag a build that does not identify itself."
                .into(),
        )),
        1 => Ok(builds.remove(0)),
        _ => Err(Error::Message(format!(
            "Dumps disagree about the build: {}. Re-run the pipeline so every file comes from one extract.",
            builds.join(", ")
        ))),
    }
}

/// `resolveOrigin`: channel and branch from the extract marker (`_build.json`), trusted
/// only when it names the same build; `channel` (the flag) wins. The `Option<String>` is
/// the warning printed when no channel is known (LIVE is assumed).
pub fn resolve_origin(
    build: &str,
    marker: Option<&Value>,
    channel: Option<&str>,
) -> (Origin, Option<String>) {
    let empty = Value::Object(Map::new());
    let marker = marker.unwrap_or(&empty);
    let matches = marker.get("build").and_then(Value::as_str) == Some(build);
    let marker_text = |k: &str| {
        matches
            .then(|| marker.get(k))
            .flatten()
            .filter(|v| !v.is_null())
            .map(|v| js_string(Some(v)))
    };
    let channel = channel
        .map(str::to_string)
        .or_else(|| marker_text("channel"));
    // `channel ?? "LIVE"` after a falsy check: an empty channel warns but stays empty.
    let warning = (!channel.as_deref().is_some_and(|c| !c.is_empty())).then(|| {
        format!("No channel recorded for {build}; assuming LIVE. Pass --channel to override.")
    });
    (
        Origin {
            channel: channel.unwrap_or_else(|| "LIVE".into()),
            branch: marker_text("branch"),
        },
        warning,
    )
}

/// The first `\d+(\.\d+)+` in `s`.
fn first_version(s: &str) -> Option<&str> {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_digit() {
            let start = i;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
            let mut end = None;
            while i + 1 < b.len() && b[i] == b'.' && b[i + 1].is_ascii_digit() {
                i += 1;
                while i < b.len() && b[i].is_ascii_digit() {
                    i += 1;
                }
                end = Some(i);
            }
            if let Some(e) = end {
                return Some(&s[start..e]);
            }
        } else {
            i += 1;
        }
    }
    None
}

/// `releaseTitle`: `4.10.2 LIVE (4.10.197.2878)`, or `HOTFIX 1.0.191.55227` without a
/// patch. `patch` is `--patch`, else the first version in the branch name.
pub fn release_title(build: &str, origin: &Origin, patch: Option<&str>) -> String {
    let patch = patch.or_else(|| origin.branch.as_deref().and_then(first_version));
    match patch {
        Some(p) => format!("{p} {} ({build})", origin.channel),
        None => format!("{} {build}", origin.channel),
    }
}

/// `buildManifest`: `build.json` at the dumps root, which the fan-out workflow reads.
/// Keys in the script's order, two-space indent, trailing newline; no timestamp, so an
/// identical republish stays identical.
pub fn build_manifest(build: &str, origin: &Origin, patch: Option<&str>) -> String {
    let patch = patch.or_else(|| origin.branch.as_deref().and_then(first_version));
    let s = |v: Option<&str>| {
        v.map_or_else(|| "null".to_string(), |v| Value::String(v.into()).to_string())
    };
    format!(
        "{{\n  \"build\": {},\n  \"channel\": {},\n  \"patch\": {},\n  \"branch\": {}\n}}\n",
        s(Some(build)),
        s(Some(&origin.channel)),
        s(patch),
        s(origin.branch.as_deref()),
    )
}

/// `releaseFlags`: `--prerelease` for test channels, `--latest` only for LIVE.
pub fn release_flags(origin: &Origin) -> Vec<&'static str> {
    if PRERELEASE_CHANNELS.contains(&origin.channel.as_str()) {
        vec!["--prerelease"]
    } else if origin.channel == "LIVE" {
        vec!["--latest"]
    } else {
        vec!["--latest=false"]
    }
}

/// `releaseNotes`: the GitHub release body.
pub fn release_notes(build: &str, origin: &Origin, patch: Option<&str>, dumps: &[Dump]) -> String {
    let mut lines = vec![
        format!("Parsed game data for Star Citizen build `{build}`."),
        String::new(),
        format!("- Channel: {}", origin.channel),
    ];
    if let Some(p) = patch {
        lines.push(format!("- Patch: {p}"));
    }
    if let Some(b) = origin.branch.as_deref().filter(|b| !b.is_empty()) {
        lines.push(format!("- Branch: `{b}`"));
    }
    lines.push(String::new());
    lines.push("| Dataset | Size |".into());
    lines.push("| --- | ---: |".into());
    for d in dumps {
        lines.push(format!("| `{}` | {} |", d.file, mb(d.size)));
    }
    lines.join("\n")
}

/// `nameStatusNote`: records whose name is not real, by status, most first
/// (`1751 placeholder, 772 empty`).
pub fn name_status_note(value: &Value) -> String {
    fn walk(v: &Value, counts: &mut Vec<(String, usize)>) {
        match v {
            Value::Array(a) => a.iter().for_each(|x| walk(x, counts)),
            Value::Object(o) => {
                if let Some(Value::String(s)) = o.get("nameStatus") {
                    match counts.iter_mut().find(|(k, _)| k == s) {
                        Some(c) => c.1 += 1,
                        None => counts.push((s.clone(), 1)),
                    }
                }
                for k in js_key_order(o) {
                    walk(&o[k], counts);
                }
            }
            _ => {}
        }
    }
    let mut counts = Vec::new();
    walk(value, &mut counts);
    // `Object.entries(counts)`: array-index-like statuses would come first.
    let map: Map<String, Value> = counts
        .iter()
        .map(|(k, n)| (k.clone(), Value::from(*n)))
        .collect();
    let mut ordered: Vec<(&String, u64)> = js_key_order(&map)
        .into_iter()
        .map(|k| (k, map[k].as_u64().unwrap_or(0)))
        .collect();
    ordered.sort_by_key(|o| std::cmp::Reverse(o.1));
    ordered
        .iter()
        .map(|(s, n)| format!("{n} {s}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// `mb`: `(bytes / 1024 / 1024).toFixed(2) + " MB"` (ties round up, as `toFixed` does on
/// the exact quotient).
pub fn mb(bytes: usize) -> String {
    let b = bytes as u128;
    let denom = 1u128 << 20;
    let n = (b * 200 + denom) / (2 * denom);
    format!("{}.{:02} MB", n / 100, n % 100)
}

/// `compareBuilds`: segment by segment as numbers (`.` and `-` separate); a NaN
/// difference counts as equal.
pub fn compare_builds(a: &str, b: &str) -> Ordering {
    let pa: Vec<f64> = a.split(['.', '-']).map(js_to_number).collect();
    let pb: Vec<f64> = b.split(['.', '-']).map(js_to_number).collect();
    for i in 0..pa.len().max(pb.len()) {
        let d = pa.get(i).copied().unwrap_or(0.0) - pb.get(i).copied().unwrap_or(0.0);
        if d != 0.0 && !d.is_nan() {
            return if d > 0.0 {
                Ordering::Greater
            } else {
                Ordering::Less
            };
        }
    }
    Ordering::Equal
}

/// `previousBuild`: the newest tag older than `build` with the same major version (by
/// version order, not tag date). Another release line is never a base.
pub fn previous_build(build: &str, tags: &[String]) -> Option<String> {
    let line = build.split('.').next().unwrap_or("");
    let mut older: Vec<&String> = tags
        .iter()
        .filter(|t| {
            is_version(t)
                && t.split('.').next() == Some(line)
                && compare_builds(t, build) == Ordering::Less
        })
        .collect();
    older.sort_by(|a, b| compare_builds(a, b));
    older.last().map(|s| s.to_string())
}

fn git(work_dir: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(work_dir)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| Error::Message(format!("git {}: {e}", args.join(" "))))?;
    if !out.status.success() {
        return Err(Error::Message(format!(
            "Command failed: git {}\n{}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn git_ok(work_dir: &Path, args: &[&str]) -> bool {
    git(work_dir, args).is_ok()
}

/// `computeNotes`: notes for this build against the previous published one in the
/// checkout at `work_dir`; never fails, says why it skipped instead.
pub fn compute_notes(build: &str, dumps: &[Dump], work_dir: &Path) -> PublishNotes {
    match notes_for(build, dumps, work_dir) {
        Ok(n) => n,
        Err(e) => PublishNotes::Skipped(format!("notes failed: {e}")),
    }
}

fn notes_for(build: &str, dumps: &[Dump], work_dir: &Path) -> Result<PublishNotes> {
    let line = build.split('.').next().unwrap_or("");
    let prev = if work_dir.join(".git").exists() {
        let tags: Vec<String> = git(work_dir, &["tag", "--list"])?
            .split('\n')
            .map(str::to_string)
            .collect();
        previous_build(build, &tags)
    } else {
        None
    };
    let Some(prev) = prev else {
        return Ok(PublishNotes::Skipped(format!(
            "no earlier published {line}.x build in the dumps checkout"
        )));
    };
    let after: Vec<(String, Map<String, Value>)> = dumps
        .iter()
        .map(|d| {
            let body = match &d.parsed {
                Value::Object(o) => o.clone(),
                _ => Map::new(),
            };
            (d.file.clone(), body)
        })
        .collect();
    let from = BuildSource::Tag {
        tag: prev.clone(),
        repo_dir: work_dir.to_path_buf(),
    };
    let doc = notes_against(&from, &after, build, &NotesOptions::default())?;
    let json = serde_json::to_value(&doc).unwrap_or(Value::Null);
    Ok(PublishNotes::Written {
        from: prev,
        files: vec![
            NotesFile {
                name: format!("{build}.md"),
                body: render_notes(&doc),
            },
            NotesFile {
                name: format!("{build}.json"),
                body: format!("{}\n", to_js_json_compact(&json)),
            },
        ],
    })
}

/// Read the datasets, redact them, resolve build and origin, and compute the notes from
/// the checkout as it stands (a plan never fetches).
pub fn plan(opts: &PublishOptions) -> Result<PublishPlan> {
    if let Some(p) = &opts.patch
        && !is_version(p)
    {
        return Err(Error::Message(format!(
            "--patch needs a version like 4.10.3, got: {p}"
        )));
    }
    let root = &opts.root;
    let data_dir = opts.data_dir.clone().unwrap_or_else(|| {
        env_or("SC_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| root.join("data"))
    });
    let work_dir = opts.work_dir.clone().unwrap_or_else(|| {
        env_or("SC_DUMPS_WORKDIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| root.join(".publish-repo"))
    });
    let extract_dir = opts.extract_dir.clone().unwrap_or_else(|| {
        env_or("SC_EXTRACT_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| root.join("extracted-data"))
    });
    let repo = opts
        .repo
        .clone()
        .or_else(|| env_or("SC_DUMPS_REPO"))
        .unwrap_or_else(|| "voidput/sc-gamedata-dumps".into());
    let remote = opts
        .remote
        .clone()
        .or_else(|| env_or("SC_DUMPS_REMOTE"))
        .unwrap_or_else(|| format!("git@github.com:{repo}.git"));

    let dumps = read_dumps(&data_dir)?;
    let build = resolve_build(&dumps)?;
    let marker: Option<Value> = std::fs::read(extract_dir.join("_build.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok());
    let channel = opts.channel.as_ref().map(|c| c.to_uppercase());
    let (origin, warning) = resolve_origin(&build, marker.as_ref(), channel.as_deref());
    let mut warnings: Vec<String> = warning.into_iter().collect();
    let notes = compute_notes(&build, &dumps, &work_dir);
    if let PublishNotes::Skipped(why) = &notes {
        warnings.push(format!("Notes:  skipped ({why})"));
    }
    Ok(PublishPlan {
        build,
        repo,
        remote,
        work_dir,
        origin,
        patch: opts.patch.clone(),
        republish: opts.republish,
        dumps,
        notes,
        warnings,
    })
}

fn pad_end(s: &str, n: usize) -> String {
    format!("{s:<n$}")
}

fn pad_start(s: &str, n: usize) -> String {
    format!("{s:>n$}")
}

impl PublishPlan {
    pub fn title(&self) -> String {
        release_title(&self.build, &self.origin, self.patch.as_deref())
    }
}

/// The header and per-file listing every run prints (before the dry-run stop).
fn render_listing(plan: &PublishPlan) -> String {
    let mut out = String::new();
    let mut line = |s: String| {
        out.push_str(&s);
        out.push('\n');
    };
    line(format!("\nBuild:  {}", plan.build));
    line(format!("Repo:   {}", plan.repo));
    line(format!(
        "Release: {} [{}]",
        plan.title(),
        release_flags(&plan.origin).join(" ")
    ));
    line(format!("Files:  {}\n", plan.dumps.len()));
    for d in &plan.dumps {
        let note = if d.size == d.original {
            String::new()
        } else {
            format!("  (redacted from {})", mb(d.original))
        };
        let names = name_status_note(&d.parsed);
        let names = if names.is_empty() {
            names
        } else {
            format!("  names: {names}")
        };
        line(format!(
            "  {} {}{note}{names}",
            pad_end(&d.file, 32),
            pad_start(&mb(d.size), 9)
        ));
    }
    let total: usize = plan.dumps.iter().map(|d| d.size).sum();
    let original: usize = plan.dumps.iter().map(|d| d.original).sum();
    line(format!(
        "\n  {} {}  of {} parsed\n",
        pad_end("total", 32),
        pad_start(&mb(total), 9),
        mb(original)
    ));
    out
}

/// `printNotes` (stdout part; a skip goes to stderr, see [`PublishPlan::warnings`]).
fn render_notes_listing(notes: &PublishNotes) -> String {
    let PublishNotes::Written { from, files } = notes else {
        return String::new();
    };
    let mut out = format!("Notes:  against {from}\n");
    for f in files {
        out.push_str(&format!(
            "  {} {}\n",
            pad_end(&format!("notes/{}", f.name), 32),
            pad_start(&mb(f.body.len()), 9)
        ));
    }
    out.push('\n');
    out
}

/// `publish-dump.mjs --dry-run` stdout, byte for byte.
pub fn render_dry_run(plan: &PublishPlan) -> String {
    let mut out = render_listing(plan);
    out.push_str(&render_notes_listing(&plan.notes));
    out.push_str("Dry run — nothing written, nothing pushed.\n");
    out
}

/// What [`stage`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StageOutcome {
    /// The tag exists and `republish` was not asked for; nothing written. The TS script
    /// would now ensure the GitHub release exists.
    AlreadyPublished,
    Staged {
        /// The tag already existed (a republish).
        republished: bool,
        committed: bool,
        notes: PublishNotes,
        /// Whether the branch and tag were pushed.
        pushed: bool,
    },
}

/// The non-dry-run part of the script up to (not including) the GitHub release: clone or
/// fetch `work_dir`, copy the template files that are missing, rewrite `data/`, write this
/// build's `notes/` pair, commit, tag, and push only when `push` is true.
pub fn stage(plan: &PublishPlan, work_dir: &Path, push: bool) -> Result<StageOutcome> {
    let build = plan.build.as_str();
    let io = |p: &Path| {
        let p = p.to_path_buf();
        move |e| Error::Io { path: p, source: e }
    };
    if !work_dir.join(".git").exists() {
        std::fs::create_dir_all(work_dir).map_err(io(work_dir))?;
        let ok = Command::new("git")
            .arg("clone")
            .arg(&plan.remote)
            .arg(work_dir)
            .status()
            .map_err(|e| Error::Message(format!("git clone: {e}")))?
            .success();
        if !ok {
            return Err(Error::Message(format!("git clone {} failed", plan.remote)));
        }
    } else {
        git(work_dir, &["fetch", "--tags", "origin"])?;
        // A repo never published to has no origin/main to sync with.
        if git_ok(work_dir, &["rev-parse", "--verify", "origin/main"]) {
            git(work_dir, &["checkout", "main"])?;
            git(work_dir, &["pull", "--ff-only", "origin", "main"])?;
        }
    }

    let existing = !git(work_dir, &["tag", "--list", build])?.is_empty();
    if existing && !plan.republish {
        return Ok(StageOutcome::AlreadyPublished);
    }
    let remote_tag = if existing {
        git(
            work_dir,
            &["ls-remote", "origin", &format!("refs/tags/{build}")],
        )?
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_string()
    } else {
        String::new()
    };

    // `.github/` always follows the template; the rest only when missing, so edits made
    // in the dumps repo stay.
    for (name, body) in TEMPLATE_FILES {
        let target = work_dir.join(name);
        if name.starts_with(".github/") || !target.exists() {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(io(parent))?;
            }
            std::fs::write(&target, body).map_err(io(&target))?;
        }
    }
    let manifest = work_dir.join("build.json");
    std::fs::write(
        &manifest,
        build_manifest(build, &plan.origin, plan.patch.as_deref()),
    )
    .map_err(io(&manifest))?;
    let out_dir = work_dir.join("data");
    if out_dir.exists() {
        std::fs::remove_dir_all(&out_dir).map_err(io(&out_dir))?;
    }
    std::fs::create_dir_all(&out_dir).map_err(io(&out_dir))?;
    for d in &plan.dumps {
        let target = out_dir.join(&d.file);
        std::fs::write(&target, to_js_json_compact(&d.parsed)).map_err(io(&target))?;
    }

    // After the fetch, so the base is chosen from every published tag.
    let notes = compute_notes(build, &plan.dumps, work_dir);
    if let PublishNotes::Written { files, .. } = &notes {
        let dir = work_dir.join("notes");
        std::fs::create_dir_all(&dir).map_err(io(&dir))?;
        for f in files {
            let target = dir.join(&f.name);
            std::fs::write(&target, &f.body).map_err(io(&target))?;
        }
    }

    git(work_dir, &["add", "-A"])?;
    let has_commits = git_ok(work_dir, &["rev-parse", "HEAD"]);
    let dirty = !git(work_dir, &["status", "--porcelain"])?.is_empty();
    if dirty {
        let message = if existing {
            format!("{build} (republished)")
        } else {
            build.to_string()
        };
        git(work_dir, &["commit", "-m", &message])?;
    } else if existing {
        return Err(Error::Message(format!(
            "Build {build} is identical to what is already published. Nothing to republish."
        )));
    } else if !has_commits {
        return Err(Error::Message(format!(
            "Build {build} is identical to what is already published. Nothing to commit."
        )));
    }

    // Annotated, always with a message (a config defaulting tags to annotated makes a
    // bare `git tag` fail).
    if existing {
        git(
            work_dir,
            &[
                "tag",
                "-f",
                "-a",
                build,
                "-m",
                &format!("{build} (republished)"),
            ],
        )?;
    } else if git(work_dir, &["tag", "--list", build])?.is_empty() {
        git(work_dir, &["tag", "-a", build, "-m", build])?;
    }

    if push {
        let branch = git(work_dir, &["rev-parse", "--abbrev-ref", "HEAD"])?;
        git(work_dir, &["push", "origin", &format!("{branch}:main")])?;
        if existing {
            git(
                work_dir,
                &[
                    "push",
                    &format!("--force-with-lease=refs/tags/{build}:{remote_tag}"),
                    "origin",
                    &format!("refs/tags/{build}"),
                ],
            )?;
        } else {
            git(work_dir, &["push", "origin", build])?;
        }
    }
    Ok(StageOutcome::Staged {
        republished: existing,
        committed: dirty,
        notes,
        pushed: push,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn origin(channel: &str, branch: Option<&str>) -> Origin {
        Origin {
            channel: channel.into(),
            branch: branch.map(str::to_string),
        }
    }

    #[test]
    fn build_manifest_matches_script() {
        // JSON.stringify({build, channel, patch, branch}, null, 2) + "\n" in the script.
        assert_eq!(
            build_manifest(
                "5.0.202.35026",
                &origin("PTU", Some("sc-alpha-5.0.0")),
                None
            ),
            "{\n  \"build\": \"5.0.202.35026\",\n  \"channel\": \"PTU\",\n  \"patch\": \"5.0.0\",\n  \"branch\": \"sc-alpha-5.0.0\"\n}\n"
        );
        assert_eq!(
            build_manifest("1.0.191.55227", &origin("HOTFIX", None), Some("1.0.2")),
            "{\n  \"build\": \"1.0.191.55227\",\n  \"channel\": \"HOTFIX\",\n  \"patch\": \"1.0.2\",\n  \"branch\": null\n}\n"
        );
        assert_eq!(
            build_manifest("b", &origin("LIVE", None), None),
            "{\n  \"build\": \"b\",\n  \"channel\": \"LIVE\",\n  \"patch\": null,\n  \"branch\": null\n}\n"
        );
    }

    #[test]
    fn release_title_and_flags() {
        let o = origin("LIVE", Some("sc-alpha-4.10.0"));
        assert_eq!(
            release_title("4.10.197.2878", &o, None),
            "4.10.0 LIVE (4.10.197.2878)"
        );
        assert_eq!(
            release_title("4.10.197.2878", &o, Some("4.10.3")),
            "4.10.3 LIVE (4.10.197.2878)"
        );
        assert_eq!(
            release_title("1.0.191.55227", &origin("HOTFIX", None), None),
            "HOTFIX 1.0.191.55227"
        );
        assert_eq!(
            release_title("b", &origin("PTU", Some("sc-alpha-4")), None),
            "PTU b"
        );
        assert_eq!(release_flags(&origin("PTU", None)), vec!["--prerelease"]);
        assert_eq!(release_flags(&origin("LIVE", None)), vec!["--latest"]);
        assert_eq!(
            release_flags(&origin("HOTFIX", None)),
            vec!["--latest=false"]
        );
    }

    #[test]
    fn origin_trusts_the_marker_only_for_its_build() {
        let marker =
            json!({"build": "4.10.201.48085", "channel": "PTU", "branch": "sc-alpha-4.10.0"});
        let (o, w) = resolve_origin("4.10.201.48085", Some(&marker), None);
        assert_eq!(o, origin("PTU", Some("sc-alpha-4.10.0")));
        assert!(w.is_none());
        let (o, w) = resolve_origin("4.10.200.58746", Some(&marker), None);
        assert_eq!(o, origin("LIVE", None));
        assert_eq!(
            w.as_deref(),
            Some(
                "No channel recorded for 4.10.200.58746; assuming LIVE. Pass --channel to override."
            )
        );
        let (o, _) = resolve_origin("4.10.200.58746", Some(&marker), Some("EPTU"));
        assert_eq!(o.channel, "EPTU");
    }

    #[test]
    fn previous_build_stays_on_the_release_line() {
        let tags: Vec<String> = [
            "1.0.191.28374",
            "4.10.197.2878",
            "4.10.200.58746",
            "4.9.9.1",
            "v5",
            "",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(
            previous_build("4.10.201.48085", &tags).as_deref(),
            Some("4.10.200.58746")
        );
        assert_eq!(
            previous_build("4.10.200.58746", &tags).as_deref(),
            Some("4.10.197.2878")
        );
        assert_eq!(previous_build("1.0.191.28374", &tags), None);
        assert_eq!(previous_build("5.0.1", &tags), None);
        assert_eq!(compare_builds("4.10.0", "4.9.9"), Ordering::Greater);
    }

    #[test]
    fn sizes_and_name_status() {
        assert_eq!(mb(0), "0.00 MB");
        assert_eq!(mb(131072), "0.13 MB"); // 0.125 exactly: toFixed rounds up
        assert_eq!(mb(5 * 1048576 + 10), "5.00 MB");
        let v = json!({"a": [{"nameStatus": "empty"}, {"nameStatus": "placeholder"}, {"x": {"nameStatus": "placeholder"}}]});
        assert_eq!(name_status_note(&v), "2 placeholder, 1 empty");
        assert_eq!(name_status_note(&json!({"a": 1})), "");
        assert_eq!(first_version("sc-alpha-4.10.0"), Some("4.10.0"));
        assert_eq!(first_version("v4 then 4.1"), Some("4.1"));
        assert_eq!(first_version("4."), None);
    }

    fn git_in(dir: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .args([
                "-c",
                "user.name=test",
                "-c",
                "user.email=test@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "tag.gpgSign=false",
            ])
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?}");
    }

    fn write(path: &Path, v: Value) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, v.to_string()).unwrap();
    }

    #[test]
    fn plans_and_stages_without_pushing() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        // A remote with one published 1.0.1 build.
        let seed = root.join("seed");
        write(
            &seed.join("data/game-weapons.json"),
            json!({"_build": "1.0.1", "shipWeapons": [{"recordName": "EntityClassDefinition.A", "name": "Alpha", "dps": 100}], "fpsWeapons": [], "missiles": []}),
        );
        git_in(&seed, &["init", "--quiet", "-b", "main"]);
        git_in(&seed, &["add", "."]);
        git_in(&seed, &["commit", "--quiet", "-m", "1.0.1"]);
        git_in(&seed, &["tag", "-a", "1.0.1", "-m", "1.0.1"]);
        let remote = root.join("remote.git");
        git_in(
            root,
            &[
                "clone",
                "--quiet",
                "--bare",
                seed.to_str().unwrap(),
                remote.to_str().unwrap(),
            ],
        );
        // A work clone with a local identity, so the commit needs no global config.
        let work = root.join("work");
        git_in(
            root,
            &[
                "clone",
                "--quiet",
                remote.to_str().unwrap(),
                work.to_str().unwrap(),
            ],
        );
        for (k, v) in [
            ("user.name", "test"),
            ("user.email", "test@example.invalid"),
            ("commit.gpgsign", "false"),
            ("tag.gpgSign", "false"),
        ] {
            git_in(&work, &["config", k, v]);
        }

        let data = root.join("data");
        write(
            &data.join("game-weapons.json"),
            json!({"_build": "1.0.2", "shipWeapons": [{"recordName": "EntityClassDefinition.A", "name": "Alpha", "dps": 125, "nameStatus": "placeholder"}], "fpsWeapons": [], "missiles": []}),
        );
        write(
            &data.join("game-ships.json"),
            json!({"_build": "1.0.2", "vehicles": [{"recordName": "EntityClassDefinition.S", "name": "Ship", "description": "prose"}]}),
        );
        let opts = PublishOptions {
            root: root.to_path_buf(),
            data_dir: Some(data),
            work_dir: Some(work.clone()),
            extract_dir: Some(root.join("no-extract")),
            repo: Some("test/dumps".into()),
            remote: Some(remote.to_string_lossy().into_owned()),
            ..Default::default()
        };
        let p = plan(&opts).unwrap();
        assert_eq!(p.build, "1.0.2");
        assert_eq!(
            p.warnings[0],
            "No channel recorded for 1.0.2; assuming LIVE. Pass --channel to override."
        );
        let PublishNotes::Written { from, files } = &p.notes else {
            panic!("{:?}", p.notes)
        };
        assert_eq!(from, "1.0.1");
        assert!(
            files[0].body.contains("dps 100 → 125 (+25%)"),
            "{}",
            files[0].body
        );
        let text = render_dry_run(&p);
        assert!(text.starts_with(
            "\nBuild:  1.0.2\nRepo:   test/dumps\nRelease: LIVE 1.0.2 [--latest]\nFiles:  2\n\n"
        ));
        assert!(text.contains("(redacted from 0.00 MB)"));
        assert!(text.contains("names: 1 placeholder"));
        assert!(text.contains("Notes:  against 1.0.1\n  notes/1.0.2.md"));
        assert!(text.ends_with("Dry run — nothing written, nothing pushed.\n"));

        let outcome = stage(&p, &work, false).unwrap();
        let StageOutcome::Staged {
            republished,
            committed,
            pushed,
            ..
        } = outcome
        else {
            panic!()
        };
        assert!(!republished && committed && !pushed);
        assert!(git(&work, &["tag", "--list", "1.0.2"]).unwrap() == "1.0.2");
        assert!(work.join("notes/1.0.2.md").exists() && work.join("README.md").exists());
        let ships = std::fs::read_to_string(work.join("data/game-ships.json")).unwrap();
        assert!(!ships.contains("prose") && ships.contains("\"_redacted\":[\"description\"]"));
        // Nothing reached the remote.
        assert!(
            git(&remote, &["tag", "--list", "1.0.2"])
                .unwrap()
                .is_empty()
        );
        // A second run finds the tag.
        assert_eq!(
            stage(&p, &work, false).unwrap(),
            StageOutcome::AlreadyPublished
        );
    }
}
