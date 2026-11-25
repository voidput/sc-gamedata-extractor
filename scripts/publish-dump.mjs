#!/usr/bin/env node
/**
 * Publish one build's parsed datasets to the public dumps repo.
 *
 * The dumps repo is the patch history SCMCP reads: one commit per game build,
 * tagged with that build string, so the tag list *is* the list of comparable
 * patches. Nothing else serves data for a past patch.
 *
 * Run after `npm run all`, from a checkout whose `data/` (or `$SC_DATA_DIR`) is
 * the build you want to publish. Always dry-run first — this pushes to a public
 * repo.
 *
 * Each publish writes `build.json` ({build, channel, patch, branch}) at the
 * dumps root, which the dumps repo's fan-out workflow reads to tell the tool
 * sites (snares, MCsim, Bindscape) which channel to re-bake. The template's
 * `.github/` is always refreshed; its other files are copied only when missing.
 *
 * Each tag also gets a GitHub release, labelled with the channel the build was
 * extracted from: PTU-style channels are prereleases, and only LIVE is marked
 * latest. Re-running against an already-published tag creates its release if
 * it is missing, so older tags can be backfilled.
 *
 *   node scripts/publish-dump.mjs --dry-run
 *   node scripts/publish-dump.mjs
 *   node scripts/publish-dump.mjs --channel PTU   # when the extract predates the channel marker
 *   node scripts/publish-dump.mjs --patch 4.10.3  # when the branch names the wrong patch
 *   node scripts/publish-dump.mjs --republish     # replace an already-published build
 *
 * `--republish` is for adding datasets to a build already out, never for
 * swapping in a different build: the dumps must carry the same build string.
 * It commits on top of main, so history is not rewritten, then moves the tag
 * with a lease on its current remote target and refreshes the release notes.
 *
 * Each publish also writes player-facing patch notes against the previous
 * published build on the same release line (same major version, by version
 * order, not tag date) to `notes/<build>.md` and
 * `notes/<build>.json`, beside `data/`, from `dist/notes.js` (run `npm run
 * build` first). The newer side is the redacted dump as published, so a
 * comparison never counts keys that redaction strips. No earlier build on the
 * line, no built CLI, no dumps checkout yet, or notes that throw: the notes are
 * skipped with a warning and the publish goes on.
 */

import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import { redactDump } from "./redact.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(HERE, "..");
// `make` passes its OUT here, so a PTU extract in data-ptu/ publishes without touching data/.
const DATA_DIR = process.env.SC_DATA_DIR ? path.resolve(process.env.SC_DATA_DIR) : path.join(ROOT, "data");
const WORK_DIR = process.env.SC_DUMPS_WORKDIR || path.join(ROOT, ".publish-repo");
const TEMPLATE_DIR = path.join(HERE, "dump-repo-template");
const REPO = process.env.SC_DUMPS_REPO || "voidput/sc-gamedata-dumps";
// SSH by default: it is what the other checkouts here push over, so no HTTPS
// credential helper has to exist for this to work unattended.
const REMOTE = process.env.SC_DUMPS_REMOTE || `git@github.com:${REPO}.git`;

const EXTRACT_DIR = process.env.SC_EXTRACT_DIR || path.join(ROOT, "extracted-data");

const dryRun = process.argv.includes("--dry-run");
const republish = process.argv.includes("--republish");
const channelFlag = (() => {
  const i = process.argv.indexOf("--channel");
  return i >= 0 ? process.argv[i + 1]?.toUpperCase() : undefined;
})();
// The manifest branch does not always name the patch players see (4.10.3
// shipped from `sc-alpha-4.10.0`), so the release title can be told outright.
const patchFlag = (() => {
  const i = process.argv.indexOf("--patch");
  if (i < 0) return undefined;
  const value = process.argv[i + 1];
  if (!value || !/^\d+(\.\d+)+$/.test(value)) {
    console.error(`--patch needs a version like 4.10.3, got: ${value ?? "(nothing)"}`);
    process.exit(1);
  }
  return value;
})();

/** Channels whose builds are test builds, published as GitHub prereleases. */
const PRERELEASE_CHANNELS = new Set(["PTU", "EPTU", "TECH-PREVIEW"]);

/**
 * Localisation is the file carrying the most verbatim in-game prose, so only
 * name-shaped keys are published; the other datasets drop their description
 * fields. Both rules live in redact.mjs. Prose is the most copyrightable
 * content in the dump and the part nothing here reads.
 */
function readDumps() {
  if (!fs.existsSync(DATA_DIR)) {
    throw new Error(`No data directory at ${DATA_DIR}. Run \`npm run all\` first.`);
  }
  const files = fs.readdirSync(DATA_DIR).filter((f) => f.startsWith("game-") && f.endsWith(".json"));
  if (files.length === 0) throw new Error(`No game-*.json in ${DATA_DIR}.`);

  const dumps = [];
  for (const file of files.sort()) {
    const parsed = JSON.parse(fs.readFileSync(path.join(DATA_DIR, file), "utf-8"));
    const original = Buffer.byteLength(JSON.stringify(parsed));
    const published = redactDump(file, parsed);
    dumps.push({ file, parsed: published, original, size: Buffer.byteLength(JSON.stringify(published)) });
  }
  return dumps;
}

/** The build string every dump is stamped with; it becomes the tag. */
function resolveBuild(dumps) {
  const builds = new Set(dumps.map((d) => d.parsed._build).filter(Boolean));
  if (builds.size === 0) {
    throw new Error("No _build stamp in any dump — cannot tag a build that does not identify itself.");
  }
  if (builds.size > 1) {
    throw new Error(
      `Dumps disagree about the build: ${[...builds].join(", ")}. Re-run the pipeline so every file comes from one extract.`,
    );
  }
  return [...builds][0];
}

/**
 * Channel and branch from the extract marker, trusted only when it describes
 * the same build as the dumps: a later extract of another channel overwrites
 * the marker without touching `data/`.
 */
function resolveOrigin(build) {
  let marker = {};
  try {
    marker = JSON.parse(fs.readFileSync(path.join(EXTRACT_DIR, "_build.json"), "utf-8"));
  } catch {
    // No marker: fall back to the flag, then LIVE.
  }
  const matches = marker.build === build;
  const channel = channelFlag ?? (matches ? marker.channel : undefined);
  if (!channel) {
    console.warn(`No channel recorded for ${build}; assuming LIVE. Pass --channel to override.`);
  }
  return { channel: channel ?? "LIVE", branch: matches ? marker.branch : undefined };
}

/** `--patch`, else the first version in the branch name (`sc-alpha-4.10.0` → 4.10.0). */
function resolvePatch(branch) {
  return patchFlag ?? branch?.match(/(\d+(?:\.\d+)+)/)?.[1];
}

/** e.g. "4.10.2 LIVE (4.10.197.2878)", or "HOTFIX 1.0.191.55227" without a branch. */
function releaseTitle(build, { channel, branch }) {
  const patch = resolvePatch(branch);
  return patch ? `${patch} ${channel} (${build})` : `${channel} ${build}`;
}

/**
 * `build.json` at the dumps root: what the fan-out workflow dispatches on. No
 * timestamp, so republishing an identical build still reads as identical.
 */
function buildManifest(build, { channel, branch }) {
  const manifest = { build, channel, patch: resolvePatch(branch) ?? null, branch: branch ?? null };
  return `${JSON.stringify(manifest, null, 2)}\n`;
}

function releaseNotes(build, { channel, branch }, dumps) {
  const lines = [
    `Parsed game data for Star Citizen build \`${build}\`.`,
    "",
    `- Channel: ${channel}`,
    ...(patchFlag ? [`- Patch: ${patchFlag}`] : []),
    ...(branch ? [`- Branch: \`${branch}\``] : []),
    "",
    "| Dataset | Size |",
    "| --- | ---: |",
    ...dumps.map((d) => `| \`${d.file}\` | ${mb(d.size)} |`),
  ];
  return lines.join("\n");
}

function releaseFlags({ channel }) {
  if (PRERELEASE_CHANNELS.has(channel)) return ["--prerelease"];
  return channel === "LIVE" ? ["--latest"] : ["--latest=false"];
}

/** Create the tag's GitHub release unless it already has one. */
function ensureRelease(build, origin, dumps) {
  try {
    execFileSync("gh", ["release", "view", build, "-R", REPO], { stdio: "pipe" });
    console.log(`Release ${build} already exists.`);
    return;
  } catch {
    // Not found: create it.
  }
  execFileSync(
    "gh",
    [
      "release", "create", build, "-R", REPO, "--verify-tag",
      "--title", releaseTitle(build, origin),
      "--notes", releaseNotes(build, origin, dumps),
      ...releaseFlags(origin),
    ],
    { stdio: "inherit" },
  );
}

function git(args, options = {}) {
  return execFileSync("git", args, { cwd: WORK_DIR, encoding: "utf-8", ...options }).trim();
}

/**
 * Records whose name is not real, by nameStatus ("real" is omitted in the
 * datasets), e.g. "1751 placeholder, 772 empty, 62 unresolved". Empty when the
 * dataset carries no name refs.
 */
function nameStatusNote(value) {
  const counts = {};
  const walk = (v) => {
    if (Array.isArray(v)) for (const x of v) walk(x);
    else if (v && typeof v === "object") {
      if (typeof v.nameStatus === "string") counts[v.nameStatus] = (counts[v.nameStatus] ?? 0) + 1;
      for (const x of Object.values(v)) walk(x);
    }
  };
  walk(value);
  return Object.entries(counts)
    .sort((a, b) => b[1] - a[1])
    .map(([status, n]) => `${n} ${status}`)
    .join(", ");
}

function mb(bytes) {
  return `${(bytes / 1024 / 1024).toFixed(2)} MB`;
}

/** Build strings compared segment by segment as numbers: 4.10.200 > 4.10.197 > 1.0.191. */
function compareBuilds(a, b) {
  const pa = a.split(/[.-]/).map(Number);
  const pb = b.split(/[.-]/).map(Number);
  for (let i = 0; i < Math.max(pa.length, pb.length); i++) {
    const d = (pa[i] ?? 0) - (pb[i] ?? 0);
    if (d) return d;
  }
  return 0;
}

/**
 * The newest published build older than `build` on the same release line
 * (same major version), from the dumps checkout's tags. Another line (a
 * 1.0.x build beside 4.10.x) is a different game branch, and notes against it
 * would list every ship and armour piece as new, so it is never a base.
 */
function previousBuild(build) {
  if (!fs.existsSync(path.join(WORK_DIR, ".git"))) return undefined;
  const line = build.split(".")[0];
  const tags = git(["tag", "--list"])
    .split("\n")
    .filter((t) => /^\d+(\.\d+)+$/.test(t) && t.split(".")[0] === line && compareBuilds(t, build) < 0)
    .sort(compareBuilds);
  return tags.at(-1);
}

/**
 * Patch notes for this build against the previous published one, as the files
 * to commit under notes/. The newer side is the dumps exactly as they will be
 * published (redacted strings included), staged in a temp dir named after the
 * build so the notes' `source` reads "dir <build>", never a local path. The
 * search index needs no special case: notes skip derived files on both sides
 * (diff.ts DERIVED_FILES). Returns why it skipped instead of throwing, so
 * neither a missing prerequisite nor a notes bug ever blocks a publish.
 */
async function computeNotes(build, dumps) {
  try {
    return await notesFor(build, dumps);
  } catch (error) {
    return { skipped: `notes failed: ${error instanceof Error ? error.message : String(error)}` };
  }
}

async function notesFor(build, dumps) {
  const prev = previousBuild(build);
  if (!prev) return { skipped: `no earlier published ${build.split(".")[0]}.x build in the dumps checkout` };
  const notesModule = path.join(ROOT, "dist", "notes.js");
  if (!fs.existsSync(notesModule)) return { skipped: "dist/notes.js missing; run npm run build" };
  const { buildNotes, renderNotes } = await import(pathToFileURL(notesModule).href);

  const stage = fs.mkdtempSync(path.join(os.tmpdir(), "sc-publish-notes-"));
  try {
    const toDir = path.join(stage, build);
    fs.mkdirSync(toDir);
    for (const d of dumps) {
      fs.writeFileSync(path.join(toDir, d.file), JSON.stringify(d.parsed));
    }
    const doc = buildNotes({ kind: "tag", tag: prev, repoDir: WORK_DIR }, { kind: "dir", path: toDir });
    return {
      from: prev,
      files: [
        { name: `${build}.md`, body: renderNotes(doc) },
        // Compact: every line lists the records it covers, so pretty-printing doubles it.
        { name: `${build}.json`, body: `${JSON.stringify(doc)}\n` },
      ],
    };
  } finally {
    fs.rmSync(stage, { recursive: true, force: true });
  }
}

function printNotes(notes) {
  if (notes.skipped) {
    console.warn(`Notes:  skipped (${notes.skipped})`);
    return;
  }
  console.log(`Notes:  against ${notes.from}`);
  for (const f of notes.files) {
    console.log(`  ${`notes/${f.name}`.padEnd(32)} ${mb(Buffer.byteLength(f.body)).padStart(9)}`);
  }
  console.log("");
}

const dumps = readDumps();
const build = resolveBuild(dumps);
const origin = resolveOrigin(build);
const totalOriginal = dumps.reduce((n, d) => n + d.original, 0);
const totalPublished = dumps.reduce((n, d) => n + d.size, 0);

console.log(`\nBuild:  ${build}`);
console.log(`Repo:   ${REPO}`);
console.log(`Release: ${releaseTitle(build, origin)} [${releaseFlags(origin).join(" ")}]`);
console.log(`Files:  ${dumps.length}\n`);
for (const d of dumps) {
  const note = d.size === d.original ? "" : `  (redacted from ${mb(d.original)})`;
  const names = nameStatusNote(d.parsed);
  console.log(`  ${d.file.padEnd(32)} ${mb(d.size).padStart(9)}${note}${names ? `  names: ${names}` : ""}`);
}
console.log(`\n  ${"total".padEnd(32)} ${mb(totalPublished).padStart(9)}  of ${mb(totalOriginal)} parsed\n`);

if (dryRun) {
  // From the checkout as it stands: a dry run never fetches.
  printNotes(await computeNotes(build, dumps));
  console.log("Dry run — nothing written, nothing pushed.");
  process.exit(0);
}

// Clone on first run, reuse the checkout after.
if (!fs.existsSync(path.join(WORK_DIR, ".git"))) {
  fs.mkdirSync(WORK_DIR, { recursive: true });
  execFileSync("git", ["clone", REMOTE, WORK_DIR], { stdio: "inherit" });
} else {
  git(["fetch", "--tags", "origin"]);
  // A repo that has never been published to has no origin/main to sync with.
  const published = (() => {
    try {
      git(["rev-parse", "--verify", "origin/main"], { stdio: "pipe" });
      return true;
    } catch {
      return false;
    }
  })();
  if (published) {
    git(["checkout", "main"]);
    git(["pull", "--ff-only", "origin", "main"]);
  }
}

// A build is immutable unless --republish says otherwise. Republishing never
// rewrites history: it adds a commit on top of main and moves the tag to it.
const existing = git(["tag", "--list", build]);
if (existing && !republish) {
  console.log(`Build ${build} is already published as a tag. Pass --republish to replace it.`);
  ensureRelease(build, origin, dumps);
  process.exit(0);
}
// The tag's current remote target, for the force-with-lease when moving it.
const remoteTag = existing ? git(["ls-remote", "origin", `refs/tags/${build}`]).split(/\s+/)[0] : "";

// .github/ (the fan-out workflow) always follows the template; README and
// NOTICE are copied only when missing, so edits made in the dumps repo stay.
for (const name of fs.readdirSync(TEMPLATE_DIR)) {
  const source = path.join(TEMPLATE_DIR, name);
  const target = path.join(WORK_DIR, name);
  if (name === ".github") fs.cpSync(source, target, { recursive: true, force: true });
  else if (!fs.existsSync(target)) fs.copyFileSync(source, target);
}
fs.writeFileSync(path.join(WORK_DIR, "build.json"), buildManifest(build, origin));

const outDir = path.join(WORK_DIR, "data");
fs.rmSync(outDir, { recursive: true, force: true });
fs.mkdirSync(outDir, { recursive: true });
for (const d of dumps) {
  fs.writeFileSync(path.join(outDir, d.file), JSON.stringify(d.parsed));
}

// After the fetch above, so the previous build is chosen from every published tag.
// notes/ keeps earlier builds' files; only this build's pair is (re)written.
const notes = await computeNotes(build, dumps);
printNotes(notes);
if (notes.files) {
  const notesDir = path.join(WORK_DIR, "notes");
  fs.mkdirSync(notesDir, { recursive: true });
  for (const f of notes.files) fs.writeFileSync(path.join(notesDir, f.name), f.body);
}

git(["add", "-A"]);

const hasCommits = (() => {
  try {
    git(["rev-parse", "HEAD"], { stdio: "pipe" });
    return true;
  } catch {
    return false;
  }
})();

if (git(["status", "--porcelain"])) {
  git(["commit", "-m", existing ? `${build} (republished)` : build]);
} else if (existing) {
  console.error(`Build ${build} is identical to what is already published. Nothing to republish.`);
  process.exit(1);
} else if (!hasCommits) {
  console.error(`Build ${build} is identical to what is already published. Nothing to commit.`);
  process.exit(1);
}

// Annotated, and always with a message: a git config that defaults tags to
// annotated (tag.annotate / forceSignAnnotated) makes a bare `git tag` fail.
// Tagging separately from the commit also lets a half-finished run resume.
if (existing) {
  git(["tag", "-f", "-a", build, "-m", `${build} (republished)`]);
} else if (!git(["tag", "--list", build])) {
  git(["tag", "-a", build, "-m", build]);
}

const branch = git(["rev-parse", "--abbrev-ref", "HEAD"]);
git(["push", "origin", `${branch}:main`]);
if (existing) {
  git(["push", `--force-with-lease=refs/tags/${build}:${remoteTag}`, "origin", `refs/tags/${build}`]);
  execFileSync(
    "gh",
    ["release", "edit", build, "-R", REPO, "--title", releaseTitle(build, origin), "--notes", releaseNotes(build, origin, dumps)],
    { stdio: "inherit" },
  );
} else {
  git(["push", "origin", build]);
}
ensureRelease(build, origin, dumps);

console.log(`\n${existing ? "Republished" : "Published"} ${build} to ${REPO}.`);
