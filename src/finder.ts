/**
 * `sc-gamedata find`: fuzzy-pick an item or ship, then act on it.
 *
 * With fzf on PATH and a terminal, the picker is fzf: every item and ship in
 * the parsed datasets, a live preview (the `obtain` report for items, a short
 * profile for ships), and keys that run the other commands on the highlighted
 * entry without leaving the picker. Without fzf, or when piped, it prints the
 * best fuzzy matches instead, ranked by the same kind of subsequence score.
 *
 * Every action goes back through the CLI (`find-run <action> <kind> <key>`), so
 * fzf only ever shells out to this program with a kind and a record key.
 */

import { spawnSync } from "node:child_process";

import { parseLine, toLine, type Entry, type EntryKind } from "./query/fuzzy.js";

export * from "./query/fuzzy.js";

export function hasFzf(): boolean {
  const probe = spawnSync("fzf", ["--version"], { stdio: "ignore" });
  return probe.status === 0;
}

const shellQuote = (s: string) => `'${s.replace(/'/g, "'\\''")}'`;

export const FIND_KEYS = [
  ["enter", "report"],
  ["ctrl-j", "json"],
  ["ctrl-t", "tags"],
  ["ctrl-e", "engage (ship)"],
  ["ctrl-d", "deflect (ship)"],
  ["ctrl-y", "pick + exit"],
  ["tab", "multi-select"],
] as const;

/** The shell command fzf runs for an action on the highlighted line ({1} kind, {2} key). */
export function actionCommand(self: readonly string[], action: string, trailing: readonly string[] = []): string {
  return [...self.map(shellQuote), "find-run", action, "{1}", "{2}", ...trailing.map(shellQuote)].join(" ");
}

/**
 * Run fzf over the entries. `self` re-enters this CLI (node + script) and
 * `trailing` are flags every action needs (--out). Returns the keys picked with
 * ctrl-y, if any.
 */
export function runFzf(
  entries: readonly Entry[],
  self: string[],
  options: { query?: string; trailing?: string[]; /** Extra fzf flags; tests pass --filter to run headless. */ extraArgs?: string[] } = {},
): string[] {
  const run = (action: string) => actionCommand(self, action, options.trailing);
  const pager = `\${PAGER:-less -R}`;
  const header = FIND_KEYS.map(([k, v]) => `${k.replace("ctrl-", "^")} ${v}`).join(" · ");
  const args = [
    "--delimiter=\t",
    "--with-nth=3",
    "--no-hscroll",
    "--tiebreak=begin,length",
    "--layout=reverse",
    "--height=100%",
    "--info=inline-right",
    `--prompt=sc-gamedata> `,
    `--header=${header}`,
    "--preview-window=right,58%,wrap",
    `--preview=${run("preview")}`,
    `--bind=enter:execute(${run("report")} | ${pager})`,
    `--bind=ctrl-j:execute(${run("json")} | ${pager})`,
    `--bind=ctrl-t:execute(${run("tags")} | ${pager})`,
    `--bind=ctrl-e:execute(${run("engage")} | ${pager})`,
    `--bind=ctrl-d:execute(${run("deflect")} | ${pager})`,
    "--bind=ctrl-y:accept",
    "--multi",
    ...(options.query ? [`--query=${options.query}`] : []),
    ...(options.extraArgs ?? []),
  ];
  const result = spawnSync("fzf", args, {
    input: entries.map(toLine).join("\n"),
    stdio: ["pipe", "pipe", "inherit"],
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
  });
  if (result.error) throw result.error;
  // 130 is Esc / ctrl-c, 1 is no match: both are a normal way out.
  if (result.status !== 0) return [];
  return result.stdout
    .split("\n")
    .map(parseLine)
    .filter((p): p is { kind: EntryKind; key: string } => p !== undefined)
    .map((p) => p.key);
}
