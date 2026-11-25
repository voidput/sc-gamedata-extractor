# starorg-gamedata

Rust port of [sc-gamedata](https://github.com/voidput/sc-gamedata-extractor): Star Citizen game data
read from `Data.p4k` and written as the same `game-<domain>.json` datasets the published
dumps carry, field for field and byte for byte. The shipped CLI and app use it
only through `starorg-mcp`, whose query layer (the local MCP server, [docs/mcp.md](../../docs/mcp.md))
reads its datasets; apply/enhance still use `starorg-data`'s `GameData`, unchanged. A
later step moves `GameData` onto this crate (see "Known limits" first).

## Status

Against LIVE 4.10.200.58746 and sc-gamedata's `data/` dumps of that build: all 26 record
domains, the string table and the derived `game-search-index` are ported (28 datasets).
24 are byte-identical to the golden files apart from the `_extracted` timestamp line.

The other four differ because the **golden** dumps are stale, not the port: sc-gamedata
parsed an `extracted-data/` folder that still held 65 record files from an older
extraction (dated 2026-11-01), and 56 of their records no longer exist under those names
in this build's DataCore. Ours is the LIVE data.

| Dataset | Paths | Why |
|---|---|---|
| `game-ships` | 71 | 54 pre-rename vehicles (`AEGS_Hammerhead*`, `RSI_Constellation_Andromeda/Aquila/Phoenix/Taurus*`, Aurora Mk I variants, ...) are in the golden only; LIVE has their `*_GS` replacements, which both sides carry. Golden minus those 54 equals ours, in order. |
| `game-fps-weapons` | 4 | `vncl_melee_01_tint03` is not in LIVE (only base, tint01, tint02, store01). |
| `game-item-tags` | 4 | the same melee tint, and `Carryable_2H_FL_05x05x05_CarraybleLootableItem_luminalia_box` (its GUID is in LIVE under a new name, which both sides have). |
| `game-search-index` | 25,187 | derived from the two above: exactly those 2 items and 54 ships are missing from ours, and the positional rows shift after them. Built from the golden item-tags and ships, ours is byte-identical to the golden index, and sc-gamedata's own code fed our item-tags and ships writes the file we write. |

`tests/live.rs` allows exactly these three record datasets to differ.

Not ported:

- `uex-item-prices.json` / `uex-terminals.json` (and our `uex-commodity-prices.json`, read by
  `query::sells`): third-party UEX API data, not game files.
- The HTTP download itself in `sources/overlays.ts`. The registry, validation, the
  user `overlay-sources.json`, file naming, installed-pack lookup and loading are in
  `overlays`; `fetch_source` takes the download as a function (the TypeScript takes
  `fetchImpl`), so this crate and the CLI's default build carry no HTTP client.
- The CLI-side report tools (`finder.ts`'s fzf picker, `notes`, `diff`, `changes`,
  `engagement`, `deflection`), which read datasets rather than game files. The query layer
  they share (`query/fuzzy`, `search-index`, `obtain`, `where`, `shops`) **is** ported, as
  `query` (see "Query layer" below).

## Known limits

- **Peak memory.** Every record a domain is handed is held as a `serde_json::Value` tree
  at once; item-tags and item-sources are handed all 24,914 `entities/scitem` records
  (1.5 GB as pretty JSON), so held whole they dominate. The four domains handed item
  records prune each one as it is loaded (`Domain::prune`) to the fields they read, with
  the compare still byte-identical. Peak RSS of
  one domain per process, LIVE 4.10.200.58746 (`/usr/bin/time`, DataCore alone about
  0.9 GB):

  | Domain | Whole records | Pruned |
  |---|---|---|
  | `item-sources` | 9.0 GB | 1.5 GB |
  | `item-tags` | 8.7 GB | 1.2 GB |
  | `blueprints` | 6.5 GB | 1.4 GB |
  | `armor` | 3.0 GB | 0.9 GB |
  | `ships` | 4.1 GB | not pruned (loadouts and ports read deep) |

  A full `extract` (all 28 datasets in one process) peaks at 7.3 GB, down from about
  10 GB; the allocator keeps what the biggest domain took. The local MCP server runs each
  domain in its own short-lived worker process for this reason (`docs/mcp.md`).
- The golden dumps are a moving target: sc-gamedata is being worked on, and the identity
  above holds for its `data/` as of 2026-11-15.

## Layout

| Module | Ports | What |
|---|---|---|
| `records` | `records.ts` | `GameRecord`, `RecordSet` (by type / name / record path / GUID, `follow` for both reference shapes), `at`, `num`, `str_`, `ref_*` |
| `entity` | `entity.ts` | components of `EntityClassDefinition` |
| `localization` | `localization.ts` | `global.ini` parsing, `localize`, `apply_overlay`/`diff_strings`, `redaction` (publish-dump rules, off by default) |
| `overlays` | `sources/overlays.ts`, `loadOverlay` | community pack registry, `overlay-sources.json`, `fetch_source` (caller supplies HTTP), installed pack, loading a pack |
| `search_index` | `search-index.ts`, `query/search-index.ts`, `query/fuzzy.ts` (`buildEntries`) | `game-search-index.json` from item-tags + ships |
| `cryxml` | `cryxml.ts` | CryXmlB documents |
| `glob`, `collate` | `glob.ts`, `Intl.Collator` | filter globs; `locale_compare` / `compare_names` orders |
| `output` | `JSON.stringify(_, null, 2)` | exact writer and the `_source`/`_extracted`/`_build` header |
| `domain`, `domains/` | `domains/types.ts`, `pipeline.ts` `DOMAINS` | `Domain` trait, `ParseContext`, the registry |
| `pipeline` | `pipeline.ts` (parse half) | extraction scoping, running a domain |
| `compare` | (new) | byte compare against golden dumps, with a path diff on a difference |
| `source` | `starbreaker.ts` | the archive through `starorg-data`'s StarBreaker adapter (read only) |
| `strings` | `writeStrings` | `game-strings-english.json` |
| `query` | `query/fuzzy.ts`, `obtain.ts`, `where.ts`, `shops.ts` | fuzzy find, loot odds, where to get an item, UEX shop rows and inventories (reads datasets, no I/O) |

Records come from StarBreaker's own JSON exporter (`FullDataCore` in
`starorg-data::starbreaker`, the exporter behind `starbreaker dcb extract --format json`),
so a domain sees exactly the values the TypeScript parsed.

## Compare with the golden dumps

```sh
scripts/compare-gamedata.sh                          # every ported domain, LIVE
scripts/compare-gamedata.sh --domain manufacturers   # one domain
GOLDEN_DIR=/path/to/dumps scripts/compare-gamedata.sh --channel PTU --p4k /path/PTU/Data.p4k
```

The script builds `gamedata-port` (into `.tmp/target-port`), extracts to
`.tmp/gamedata/<channel>/` and compares with `$GOLDEN_DIR` (default
`~/git/sc-gamedata/data`, build 4.10.200.58746 = LIVE). It reads the game and the golden
directory only. Exit status 1 on any difference or missing dataset. The harness directly:

```sh
gamedata-port extract --channel LIVE --out .tmp/gamedata/LIVE [--domain X]...
gamedata-port compare .tmp/gamedata/LIVE ~/git/sc-gamedata/data [--domain X]...
gamedata-port search-index .tmp/gamedata/LIVE   # rebuild the index from item-tags + ships
gamedata-port list
```

`extract` writes `game-search-index.json` after the domains whenever item-tags and ships
are both in `--out` (`--domain search-index` selects those two), and removes a stale one
otherwise, as sc-gamedata does.

`compare` is a byte compare: a dataset is `identical` only when its text equals the
golden file's after removing the one top-level `"_extracted": ...` line. With no
`--domain` it covers every `game-*.json` on either side, so a dataset only the golden
directory has is reported as `not produced`. On a difference it prints `N differing paths`
and the first 20: numbers compared exactly as parsed (`1` vs `1.0` differs), array
elements matched by the first of `id`, `recordName`, `guid`, `key`, `code`, `className`,
`ref`, `name` that identifies every element on both sides (else by position), key order
checked; when the values are equal and only the text differs (number formatting, escapes,
duplicate keys) it names the first differing line.

## Query layer

`query` ports sc-gamedata's `src/query/` (the code its CLI's `find`, `obtain`, `where` and
`shop` and SCMCP share): typed rows of the datasets in, answers out, no I/O. Its unit tests
port `fuzzy.test.ts`, `obtain.test.ts`, `where.test.ts` and `shops.test.ts`. Answers are
checked against sc-gamedata's own compiled code on the same data:

```sh
scripts/compare-query.sh [--sample 60]   # node runs ~/git/sc-gamedata/dist/query, read only
```

It writes a query file (fixed cases, contract-award and Wikelo items, places, every Nth
item), answers it with `node scripts/query-golden.mjs` and with `gamedata-port query`, and
compares the JSON (key order ignored, numbers to 1e-12). On LIVE 4.10.200.58746's golden
`data/` (with UEX prices and terminals of 2026-11-14): 514 of 514 answers identical at
`--sample 60` (36 s for the Rust side: pool sizes are counted once per report). Differences from the TypeScript by design: `renderObtain`'s "no shop data"
line names no `sc-gamedata fetch` command, and the fzf line format is not ported.

## Porting guide

Every port follows `src/domains/manufacturers.rs`.

1. **One file, one slot.** Write `src/domains/<slot>.rs` exporting `pub static DOMAIN`,
   then uncomment the slot in `src/domains/mod.rs`. Touch no other line there; shared code
   used by several TypeScript domains goes in its pre-listed helper slot (`item_common`,
   `tags`, `socpak`), ported once, before its dependants. Do not edit `app/`, the
   contract, `starorg-data`'s `GameData`, or another domain's file.
2. **Copy the TypeScript's inputs verbatim.** `name()` is the output base name
   (`game-ship-components` for `components.ts`), `filters()` the TS `filters` array
   (each must appear in `pipeline::GOLDEN_EXTRACTION_FILTERS`; a test checks),
   `files()` the TS `files`, `needs_localization()` the TS flag.
3. **Only the scoped records.** `ctx.records` holds exactly what the TS domain was handed,
   in the same order (sorted file paths). Resolve references through `RecordSet`
   (`by_name`, `by_record_path`, `by_id`, `follow`); a target outside the domain's filters
   does not resolve, as in the TS. Never read the DataCore around the set.
4. **Same output shape.** Serde structs with `#[serde(rename_all = "camelCase")]`, field
   order = the TS object literal's key order, `Option` + `skip_serializing_if =
   "Option::is_none"` for anything that can be `undefined`. Build the body as a
   `serde_json::Map` (`domain::field`) in the TS return order. `serde_json` has
   `preserve_order` on. Do not round or reformat numbers; `output` prints them as
   JavaScript does. Integer-like object keys (`"987"`) are moved first by `output` and
   iterated first with `output::js_key_order`, as JS objects do; use it when the TS
   iterates an object it built.
5. **Same order.** `Array.prototype.sort` is stable: use `sort_by`, not `sort_unstable_by`.
   `localeCompare` -> `collate::locale_compare`, `compareNames` -> `collate::compare_names`,
   default `sort()` on strings -> UTF-16 order (`a.encode_utf16().cmp(b.encode_utf16())`).
   `collate` covers ASCII and accented Latin; extend it (with Node-verified tests) if a
   domain needs more.
6. **Same text.** `localize` keeps the TS rules (`@LOC_EMPTY` and empty values are
   `None`, unresolved keys come back as the token). Redaction is for published string
   tables only; never apply it in a domain.
7. **Tests.** Port the domain's `*.test.ts` cases as unit tests with synthetic records
   (`records::normalize_record`, `fixtures::FixtureSource`). No game data in committed
   tests. Real data only through `scripts/compare-gamedata.sh --domain <slot>` or the
   `#[ignore]` test in `tests/live.rs`; outputs under `.tmp/`.
8. **Done means identical.** `scripts/compare-gamedata.sh --domain <slot>` prints
   `identical` on LIVE. If the golden dump itself looks wrong, match it anyway and note
   it in the module docs; fixing behaviour is a separate change on both sides.
9. **Gate.** `cargo fmt --check`, `cargo clippy --workspace --all-targets --locked -- -D
   warnings`, `cargo test --workspace --locked`. Code must build and pass on Windows (no
   Unix-only APIs, paths through `Path`, archive names as stored with `\`).

Porting order, from the TypeScript imports: independent (`ammo`, `commodities`,
`containers`, `fps_weapons`, `law`, `mining`, `missions`, `quantum`, `reputation`,
`salvage`, `starmap` + `socpak`, `wikelo`); then the helpers `item_common`, `tags`; then
`armor`, `blueprints`, `consumables`, `loot`, `item_tags`, `item_sources`,
`components` (needs `containers`), `weapons` (needs `ammo`), `cargo` (needs `wikelo`),
`refining` (needs `commodities`), `mining_spawns` (needs `starmap`); last `ships`
(needs `components`, `weapons`) and `shops` (needs `cargo`, `wikelo`).

## Provenance and licence

The code is a port of sc-gamedata's TypeScript sources (`src/records.ts`, `entity.ts`,
`localization.ts`, `cryxml.ts`, `glob.ts`, `pipeline.ts`, `domains/*.ts`,
`sources/overlays.ts`, `search-index.ts`, `query/search-index.ts`, `query/fuzzy.ts`,
`scripts/publish-dump.mjs` redaction), by the same author, who releases this port under
this repository's MIT licence. sc-gamedata itself is Apache-2.0; its NOTICE attribution
carries over:

> The extract-then-parse pipeline design, the per-domain dataset breakdown, and the
> identification of which DataCore record paths carry which game data follow prior work in
> dumpers_repo (https://github.com/Sinedrone-Sentinel/dumpers_repo, Copyright (c) Michael
> Linzenmeyer, Apache-2.0). This is an independent implementation. It shares no source with
> that project. The proprietary DFP pricing engine carved out of that repository's licence
> is not used, referenced, or reimplemented here.

StarBreaker (https://github.com/diogotr7/StarBreaker, through the pinned mirror) reads the
archive and the DataCore. This crate ships no game content; it reads an installation the
user already has. Not endorsed by or affiliated with the Cloud Imperium or Roberts Space
Industries group of companies. Star Citizen® is a registered trademark of Cloud Imperium
Rights LLC.
