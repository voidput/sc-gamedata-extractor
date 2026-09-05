/**
 * Wrapper around the StarBreaker CLI, which reads Star Citizen's P4k archive and
 * DataCore database.
 *
 * Download: https://github.com/diogotr7/StarBreaker/releases
 */

import { spawn } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

export interface StarBreakerConfig {
  /** Path to the starbreaker executable. */
  bin: string;
  /** Path to the game's Data.p4k. */
  p4k: string;
}

const WINDOWS_GAME_ROOTS = [
  "C:/Program Files/Roberts Space Industries/StarCitizen",
  "D:/Program Files/Roberts Space Industries/StarCitizen",
  "E:/Program Files/Roberts Space Industries/StarCitizen",
  "F:/Program Files/Roberts Space Industries/StarCitizen",
];

/**
 * Wine/Proton installs keep the Windows tree under a drive_c directory, and the
 * prefix itself can sit one or two levels below a mount point (a single prefix
 * at /mnt/sc, or several grouped under /mnt/games/star-citizen), so scan both.
 */
const PREFIX_SUFFIX = "drive_c/Program Files/Roberts Space Industries/StarCitizen";

function linuxGameRoots(): string[] {
  const roots: string[] = [];
  const bases = ["/mnt", "/media", path.join(os.homedir(), "Games"), os.homedir()];

  const readDirs = (dir: string): string[] => {
    try {
      return fs
        .readdirSync(dir, { withFileTypes: true })
        .filter((e) => e.isDirectory() || e.isSymbolicLink())
        .map((e) => e.name);
    } catch {
      return [];
    }
  };

  for (const base of bases) {
    if (!fs.existsSync(base)) continue;
    for (const first of readDirs(base)) {
      const firstPath = path.join(base, first);
      roots.push(path.join(firstPath, PREFIX_SUFFIX));
      for (const second of readDirs(firstPath)) {
        roots.push(path.join(firstPath, second, PREFIX_SUFFIX));
      }
    }
  }

  return roots;
}

/**
 * Locate Data.p4k. Explicit paths win, then SC_DATA_P4K, then the usual install
 * locations. `channel` selects LIVE, PTU, EPTU or HOTFIX.
 */
export function findDataP4k(explicit?: string, channel = "LIVE"): string {
  if (explicit) {
    if (!fs.existsSync(explicit)) throw new Error(`Data.p4k not found at: ${explicit}`);
    return explicit;
  }

  const fromEnv = process.env.SC_DATA_P4K;
  if (fromEnv) {
    if (!fs.existsSync(fromEnv)) throw new Error(`SC_DATA_P4K points at a missing file: ${fromEnv}`);
    return fromEnv;
  }

  const roots = process.platform === "win32" ? WINDOWS_GAME_ROOTS : linuxGameRoots();
  for (const root of roots) {
    const candidate = path.join(root, channel, "Data.p4k");
    if (fs.existsSync(candidate)) return candidate;
  }

  throw new Error(
    `Could not find Data.p4k for channel ${channel}. Pass --p4k <path> or set SC_DATA_P4K.`,
  );
}

/** Locate the StarBreaker executable via flag, env var, or PATH. */
export function findStarBreaker(explicit?: string): string {
  const candidates = [explicit, process.env.STARBREAKER_BIN].filter(Boolean) as string[];
  for (const candidate of candidates) {
    if (fs.existsSync(candidate)) return candidate;
    throw new Error(`StarBreaker not found at: ${candidate}`);
  }

  const exe = process.platform === "win32" ? "starbreaker.exe" : "starbreaker";
  for (const dir of (process.env.PATH ?? "").split(path.delimiter)) {
    if (!dir) continue;
    const candidate = path.join(dir, exe);
    if (fs.existsSync(candidate)) return candidate;
  }

  throw new Error(
    "StarBreaker not found. Pass --starbreaker <path>, set STARBREAKER_BIN, or put it on PATH. Download from https://github.com/diogotr7/StarBreaker/releases",
  );
}

function run(bin: string, args: string[]): Promise<string> {
  return new Promise((resolve, reject) => {
    const child = spawn(bin, args, { stdio: ["ignore", "pipe", "pipe"] });
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (d) => (stdout += d.toString()));
    child.stderr.on("data", (d) => (stderr += d.toString()));
    child.on("error", reject);
    child.on("close", (code) => {
      if (code === 0) resolve(stdout + stderr);
      else reject(new Error(`starbreaker exited ${code}: ${(stderr || stdout).slice(-500)}`));
    });
  });
}

export async function starbreakerVersion(bin: string): Promise<string> {
  return (await run(bin, ["--version"])).trim();
}

/**
 * Extract DataCore records matching a glob to `outDir` as JSON.
 *
 * The glob is matched against the full record path and a single star does not
 * cross a slash, so directory-spanning patterns need a double star. A pattern
 * like `*mineable*` silently matches zero records; the double-star form around
 * `mineableelements` is what works.
 */
export async function extractRecords(
  config: StarBreakerConfig,
  filter: string,
  outDir: string,
): Promise<{ filter: string; records: number }> {
  const output = await run(config.bin, [
    "dcb",
    "extract",
    "--p4k",
    config.p4k,
    "--output",
    outDir,
    "--format",
    "json",
    "--filter",
    filter,
  ]);

  const match = output.match(/Exporting (\d+) records/);
  return { filter, records: match ? Number(match[1]) : 0 };
}

/**
 * Languages the game ships a string table for. The directory name is the key.
 */
export const LANGUAGES = [
  "english",
  "german_(germany)",
  "french_(france)",
  "italian_(italy)",
  "spanish_(latin_america)",
  "spanish_(spain)",
  "portuguese_(brazil)",
  "polish_(poland)",
  "korean_(south_korea)",
  "japanese_(japan)",
  "chinese_(simplified)",
  "chinese_(traditional)",
] as const;

/**
 * Extract a language's string table from the archive.
 *
 * Records store display text as `@key` tokens; without this table every
 * human-readable field stays a token. There is exactly one table per language,
 * at `Data/Localization/<language>/global.ini`. No other string file exists.
 */
export async function extractLocalization(
  config: StarBreakerConfig,
  outDir: string,
  language = "english",
): Promise<void> {
  await run(config.bin, [
    "p4k",
    "extract",
    "--p4k",
    config.p4k,
    "--output",
    outDir,
    "--filter",
    `**/${language}/global.ini`,
  ]);
}
