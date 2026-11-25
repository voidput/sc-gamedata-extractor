---
title: Rust pipeline in sc-gamedata
tags: [plans, specs]
updated: 2026-11-25
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
- 2026-11-25: clean-extract parity for LIVE 4.10.201.48085 (fresh `EXTRACT_DIR`, `make all`
  vs `sc-gamedata all`): 29 of 29 `game-*.json` byte-identical apart from `_extracted`.
  `socpak.rs` gained `system_level_placements` (TS `systemLevelPlacements`, commit
  `350e7053b`), which places Nyx's bodies held by plain ring-segment containers (Levski and 21
  other paths in `game-starmap`). starorg-strings' `starorg-gamedata` copy still needs it.
- 2026-11-25 (step 4, uncommitted): the Makefile drives `./target/release/sc-gamedata`
  (`make build` = `cargo build --release`) for every target the TS CLI ran. `make publish` /
  `republish` run `sc-gamedata publish --gh-script .tmp/gh-release-<CHANNEL>.sh`, then `sh` it
  (`gh release edit` on a republish, then create unless `gh release view` finds one). `make
  publish-dry` was byte-identical to `node scripts/publish-dump.mjs --dry-run`.
- 2026-11-25 (step 4 done, uncommitted): the TS parse, diff, notes and publish are deleted
  (list under step 4). What is left of the TypeScript is `src/query/` (step 5) and the combat
  maths `make export` reads (`src/{deflection,engagement,combat-types}.ts`); `make build-ts`
  compiles only that. `make export` output on `.tmp/parity/rust` is identical to the old
  build's apart from the `generated` timestamps.

## Steps

1. **diff, changes, notes** → `src/{diff,changes,notes}.rs`: done. They are byte-identical
   to TS on 42 text and `--json` runs over 7 build pairs, including redacted sides.
2. **CLI** → `src/bin/sc-gamedata.rs`: done. Every `cli.ts` command, UEX `fetch`, `find`, and
   `deflect`/`engage` (`src/{deflection,engagement}.rs`) is byte-identical to `node dist/cli.js`
   in the parity runs. 381 lib tests pass and clippy is clean.
3. **Publish**: done. `src/publish.rs` + `sc-gamedata publish`. The dry run is byte-identical
   to the script; the push path was tested on a scratch bare repo; instead of calling `gh` it
   prints the `gh release` command for the user to run. 391 tests pass. This replaced
   `publish-dump.mjs` and `redact.mjs`; the prose and strings redaction reuse
   `localization::redaction`.
   - Port the dry run and the git commit/tag/push only.
   - The GitHub release step (`gh release`) stays in the TS script, or a Makefile line, so no
     Rust step needs `gh`.
4. **Switch over**: done 2026-11-25.
   - The Makefile calls the Rust binary for every target (`make build` = cargo;
     `make publish` / `republish` run the `--gh-script` it writes).
   - Parity gate: a clean re-extract of LIVE 4.10.201.48085 gave 29 of 29 datasets
     byte-identical to the TS, and `make publish-dry` matched the script byte for byte. The
     first release published from the Rust output is the next `make publish`.
   - Deleted: `src/cli.ts`, `pipeline.ts`, `localization.ts`, `diff.ts`, `changes.ts`,
     `notes.ts`, `finder.ts`, `search-index.ts`, the parse helpers only they used
     (`records.ts`, `entity.ts`, `cryxml.ts`, `glob.ts`, `socpak.ts`, `starbreaker.ts`,
     `tags.ts`), `src/domains/`, `src/sources/`, all their tests, and
     `scripts/publish-dump.mjs`, `scripts/redact.mjs` (+ test). `package.json` lost the
     `bin`, the `extract`/`parse`/`all`/`changes`/`publish-dump` scripts and the unused `zod`
     dependency. They stay in git history (`4dc44d483`).
   - Kept: `src/query/` (SCMCP vendors it, step 5); `src/deflection.ts` and
     `src/engagement.ts` (+ tests) for `scripts/export-infographic.mjs`, now typed by
     `src/combat-types.ts` instead of `src/domains/*`; `scripts/release-on-exit.sh`,
     `scripts/obsidian_frontmatter.py`, `scripts/csv-to-xlsx.py` (export) and
     `scripts/dump-repo-template/` (`publish.rs` embeds it).
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

- Since step 4 the Rust crate is the only producer. A dataset change goes into both copies
  of the crate (this one and starorg-strings' `starorg-gamedata`, step 6).
