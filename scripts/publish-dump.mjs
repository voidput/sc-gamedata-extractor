#!/usr/bin/env node
/**
 * Publish one build's parsed datasets to the public dumps repo.
 *
 * The dumps repo is the patch history SCMCP reads: one commit per game build,
 * tagged with that build string, so the tag list *is* the list of comparable
 * patches. Nothing else serves data for a past patch.
 *
 * Run after `npm run all`, from a checkout whose `data/` is the build you want
 * to publish. Always dry-run first — this pushes to a public repo.
 *
 * Each tag also gets a GitHub release, labelled with the channel the build was
 * extracted from: PTU-style channels are prereleases, and only LIVE is marked
 * latest. Re-running against an already-published tag creates its release if
 * it is missing, so older tags can be backfilled.
 *
 *   node scripts/publish-dump.mjs --dry-run
 *   node scripts/publish-dump.mjs
 *   node scripts/publish-dump.mjs --channel PTU   # when the extract predates the channel marker
 *   node scripts/publish-dump.mjs --republish     # replace an already-published build
 *
 * `--republish` is for adding datasets to a build already out, never for
 * swapping in a different build: the dumps must carry the same build string.
 * It commits on top of main, so history is not rewritten, then moves the tag
 * with a lease on its current remote target and refreshes the release notes.
 */

import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(HERE, "..");
const DATA_DIR = path.join(ROOT, "data");
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

/** Channels whose builds are test builds, published as GitHub prereleases. */
const PRERELEASE_CHANNELS = new Set(["PTU", "EPTU", "TECH-PREVIEW"]);

/**
 * Localisation is the one file carrying verbatim in-game prose — descriptions,
 * dialogue, mission briefings. SCMCP only ever needs it to resolve a display
 * name, so only name-shaped keys are published. Everything else stays out of
 * the public repo: it is the most copyrightable content in the dump and the
 * part nothing here reads.
 */
const STRINGS_FILE = "game-strings-english.json";

/**
 * Name-shaped keys only, matched on a segment boundary. A substring match is
 * not enough: dialogue keys like `PU_..._MyNamesSpecial` contain "name" and
 * would drag whole spoken lines into a public repo.
 */
const KEEP_KEY = /(^|_)(Names?|Titles?|DisplayName|ShortName|Nickname)([A-Z0-9_]|$)/;
/** Dialogue and narrative namespaces, plus anything self-declaring as prose. */
const DROP_KEY = /desc|tooltip|hint|subtitle|^Dlg_|^PU_|^DX[SM]/i;
/** A display name is never a paragraph — a long value is prose that slipped through. */
const MAX_VALUE_LENGTH = 120;

function redactStrings(dump) {
  const strings = dump.strings ?? {};
  const kept = {};
  for (const [key, value] of Object.entries(strings)) {
    if (!value || value.length > MAX_VALUE_LENGTH) continue;
    if (DROP_KEY.test(key) || !KEEP_KEY.test(key)) continue;
    kept[key] = value;
  }
  return {
    ...dump,
    _redacted: "Localisation limited to name keys; descriptive and dialogue text is not published.",
    strings: kept,
    summary: { ...(dump.summary ?? {}), published: Object.keys(kept).length },
  };
}

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
    const published = file === STRINGS_FILE ? redactStrings(parsed) : parsed;
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

/** e.g. "4.10.2 LIVE (4.10.197.2878)", or "HOTFIX 1.0.191.55227" without a branch. */
function releaseTitle(build, { channel, branch }) {
  const patch = branch?.match(/(\d+(?:\.\d+)+)/)?.[1];
  return patch ? `${patch} ${channel} (${build})` : `${channel} ${build}`;
}

function releaseNotes(build, { channel, branch }, dumps) {
  const lines = [
    `Parsed game data for Star Citizen build \`${build}\`.`,
    "",
    `- Channel: ${channel}`,
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

function mb(bytes) {
  return `${(bytes / 1024 / 1024).toFixed(2)} MB`;
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
  console.log(`  ${d.file.padEnd(32)} ${mb(d.size).padStart(9)}${note}`);
}
console.log(`\n  ${"total".padEnd(32)} ${mb(totalPublished).padStart(9)}  of ${mb(totalOriginal)} parsed\n`);

if (dryRun) {
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

for (const name of fs.readdirSync(TEMPLATE_DIR)) {
  const target = path.join(WORK_DIR, name);
  if (!fs.existsSync(target)) fs.copyFileSync(path.join(TEMPLATE_DIR, name), target);
}

const outDir = path.join(WORK_DIR, "data");
fs.rmSync(outDir, { recursive: true, force: true });
fs.mkdirSync(outDir, { recursive: true });
for (const d of dumps) {
  fs.writeFileSync(path.join(outDir, d.file), JSON.stringify(d.parsed));
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
