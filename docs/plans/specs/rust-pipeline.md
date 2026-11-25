---
title: Rust pipeline in sc-gamedata
tags: [plans, specs]
updated: 2026-11-23
status: active
---

# Rust pipeline in sc-gamedata

Make this repo's own Rust crate the producer of the game datasets, so the published dumps
and the starorg-codex app run the same code and parity checks stop being needed.

## State (2026-11-23)

- Root `Cargo.toml` is a workspace, with `crates/sc-gamedata` duplicated from starorg-strings
  `crates/starorg-gamedata` (branch `feat/dump-loc-keys`, so it includes items 1, 2, 7, 6 and 10 of
  [dump-enhancements](dump-enhancements.md)). starorg-strings keeps its copy for now.
- The app-only `starorg-data` crate is replaced by `src/archive.rs`: only the p4k open and read,
  the DataCore export and the panic guard, at the same StarBreaker pin (`041e5592`), and the
  same lockfile.
- `target/release/gamedata-port extract --channel LIVE` reads Data.p4k directly in about 48 s
  (no StarBreaker CLI, no `extracted-data/`). It matches the TS golden on 23 of 28 datasets;
  ships, fps-weapons, item-tags and search-index differ only by stale 2026-11-01 records the
  TS parse still picks up from `extracted-data/`. There are 302 tests and clippy is clean.
- 2026-11-24: `game-fire` (persim handoff, `src/domains/fire.ts`) was ported as
  `domains/fire.rs` in both crates, and `roomsystem/firehazard` was added to
  `GOLDEN_EXTRACTION_FILTERS`.
  - The TS now carries `nameKey`/`descriptionKey`/`nameStatus` (item 1).
  - The LIVE 4.10.201.48085 output is byte-identical to the TS, with 490 items.

## Steps

1. **diff, changes, notes** → `src/{diff,changes,notes}.rs`: done. They are byte-identical
   to TS on 42 text and `--json` runs over 7 build pairs, including redacted sides.
2. **CLI** → `src/bin/sc-gamedata.rs`: done. Every `cli.ts` command, UEX `fetch`, `find`, and
   `deflect`/`engage` (`src/{deflection,engagement}.rs`) is byte-identical to `node dist/cli.js`
   in the parity runs. 381 lib tests pass and clippy is clean.
3. **Publish**: done. `src/publish.rs` + `sc-gamedata publish`. The dry run is byte-identical
   to the script; the push path was tested on a scratch bare repo; instead of calling `gh` it
   prints the `gh release` command for the user to run. 391 tests pass. → `publish-dump.mjs` and `redact.mjs` in Rust. The prose and strings redaction
   already exist in `localization::redaction`.
   - Port the dry run and the git commit/tag/push only.
   - The GitHub release step (`gh release`) stays in the TS script, or a Makefile line, so no
     Rust step needs `gh`.
4. **Switch over:**
   - The Makefile calls the Rust binary.
   - Data is published from the Rust output only after one release where both produce the
     same files (after a clean re-extract, so the stale rows are gone).
   - Then the TS parse, diff and notes are deleted.
5. **SCMCP** (decided 2026-11-23: Rust): SCMCP moves to the Rust query. The vendored TS copy of
   `src/query/*.ts` stays as the fallback for now, and `src/query/` is kept here until SCMCP no
   longer needs it.
   - Done 2026-11-23 (uncommitted): `sc-gamedata mcp-query` (`crates/sc-gamedata/src/mcp.rs`)
     ports SCMCP's `itemfinder.ts` rather than reusing the CLI's `--json`. The CLI's JSON and
     match rules differ from the MCP replies (candidates, `compactReport`), and the CLI reads
     shop rows from `data/`, while SCMCP has live UEX rows. One JSON request goes in on stdin
     (with the live rows), and the exact tool reply comes out.
   - SCMCP `src/rustquery.ts` takes the Rust route when `SCMCP_GAMEDATA_BIN` is set and
     `SCMCP_GAME_DATA_DIR` has item tags. A published dump, an odd `limit` or any Rust failure
     goes to the TS.
   - Parity: SCMCP `src/rustquery.test.ts` (skipped unless both env vars are set) compares
     both on `data/`.
   - Change `mcp.rs` whenever `itemfinder.ts` changes.
6. **starorg-strings** (decided 2026-11-23: keep both for now): its `starorg-gamedata` copy
   stays beside this crate. Changes go into both until a later decision makes one depend on
   the other.

## Rules

- Keep commits few and grouped. Plan in `docs/plans/` only, with no issues, boards, CI
  watching or 1Password reads.

- Until step 4, the TS output stays the reference. Any Rust change to a dataset ships with
  the same change in TS, or the same change in both copies of the crate.
- No publishing from the Rust pipeline before step 4.
