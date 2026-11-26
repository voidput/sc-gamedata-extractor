---
title: Dump reuse by starorg-codex
tags: [docs, plans, specs]
updated: 2026-11-25
status: active
---

# Dump reuse by starorg-codex

Make the published dumps safe for the starorg-codex app to reuse in place of extracting. It is
an augment, not a replacement: the app still reads the local `Data.p4k`. When a dump tag
exactly matches the installed build, the app downloads the compatible datasets, verifies them,
swaps them in atomically and stamps them `source=dump:<tag>`. It extracts locally whatever is
missing, incompatible or failed, and it never uses another build. Requested by the
starorg-codex session (branch `feat/dataset-reuse`) and confirmed by the user on 2026-11-25.

## What the app reads

- `build.json` at the dumps root ([tool-data-sync](tool-data-sync.md)). Only `build` and
  `channel` are required. Every other field is optional, and unknown keys are ignored.
- Until `_schema` exists, the app checks compatibility by shape and skips any dataset that
  doesn't match. It also skips a dataset whose redaction removed fields it reads. Redaction
  stays as it is (`scripts/redact.mjs`: string table limited to name keys, `PROSE_FIELDS`
  stripped per file, `descriptionKey` kept), and no unredacted variant is planned.

## Items

### A. `build.json` as the per-tag manifest

Extend the existing file; there is no separate `manifest.json`.

```json
{
  "build": "4.10.201.48085",
  "channel": "LIVE",
  "patch": "4.10.0",
  "branch": "sc-alpha-4.10.0",
  "extractor": { "commit": "<sc-gamedata HEAD sha>" },
  "files": [
    { "name": "data/game-ships.json", "sha256": "<hex>", "bytes": 1234567, "schema": "game-ships@1" }
  ]
}
```

- `files` is sorted by `name` and covers every `data/game-*.json` that is written. The
  `sha256` and `bytes` are computed from the exact bytes on disk, after redaction. `schema`
  copies the dataset's `_schema` once item B lands, and is absent before then.
- `extractor.commit` is `git rev-parse HEAD` of the checkout that publishes. The hook always
  publishes from the clean release worktree, so this is a real commit on `main`. A publish from
  a dirty tree gets `"dirty": true` as well.
- There is still no timestamp. A republish of identical data with the same extractor stays
  "nothing to republish". A new extractor commit does change the file, which is intended: the
  manifest records what produced the data.
- Release asset: after `ensureRelease`, and on republish, run
  `gh release upload <tag> build.json --clobber`. The Rust `publish` prints that command along
  with the `gh release` one.
- Parity: the JS `buildManifest` and Rust `build_manifest` stay byte-identical. Extend the
  existing `build_manifest_matches_script` test.

#### Built (2026-11-25)

- Rust only; the JS `buildManifest` is gone with the TS pipeline, so the parity item is void.
  `crates/sc-gamedata/src/publish.rs`: `build_manifest(build, origin, patch, extractor,
  files)` writes `{build, channel, patch, branch, extractor: {commit[, dirty: true]}, files:
  [{name, sha256, bytes, schema}]}` (two-space indent, trailing newline, no timestamp).
- `extractor_of(root)`: `git rev-parse HEAD` of the publishing checkout (`PublishOptions::root`)
  and `dirty` when `git status --porcelain --untracked-files=no` lists anything. `plan` fails
  when the root is not a git checkout. The dry run prints an `Extractor:` line, flagging a dirty
  tree.
- `stage` writes `data/` first, hashes each file from the bytes it writes (`ManifestFile::of`,
  `sha2`), then writes `build.json`, so the manifest is in the commit and every hash is its
  blob's. An identical republish still stops at "Nothing to republish".
- `publish --gh-script` appends, after the create/edit steps, `cd <dumps checkout>` and
  `gh release upload <tag> build.json --clobber -R <repo>`, guarded by the checkout's HEAD
  being the tag (after an "already published" stop, main may hold a later build). Without
  `--gh-script` the command is printed with the others.
- Verified on LIVE 4.10.201.48085, staged into a local bare clone of the dumps repo with a stub
  `gh`: 33 files listed, sorted, the same set as `data/game-*.json` in the tag, every
  `sha256`/`bytes`/`schema` equal to its `git cat-file` blob, and the uploaded asset's sha256
  equal to the tag's `build.json` blob.

### B. `_schema` per dataset

- Each TS domain declares `schemaVersion` (starting at 1). `pipeline.ts` stamps
  `_schema: "<dataset>@<n>"` (for example `game-ships@1`) next to `_build`, and the string
  table gets `game-strings-english@1`. The Rust port does the same through the `Domain` trait,
  and `compare.rs` checks it.
- Bump rule: bump when a field is removed, renamed or retyped, or its meaning changes. Adding an
  optional field does not bump, because readers ignore unknown keys.
- Guard (optional): a test that hashes each fixture dataset's shape (key paths + JSON types) and
  fails when the shape changes without a bump, or without an updated snapshot.
- Redacted files keep the same `_schema`. The app reads `_redacted` (already set by
  `redactProse`) to see which fields were removed.

#### Built (2026-11-25)

- Rust only (no TS domains remain). `Domain::schema_version()` is a required trait method, so
  each domain file holds its own number (`1` everywhere now) and a bump is a one-line change
  there. `pipeline::run` stamps `_schema` through `output::envelope`, right after `_build`
  (after `_extracted` when there is no build).
- Outside the registry: `strings::SCHEMA_VERSION` (`game-strings-<language>@1`),
  `search_index::SCHEMA_VERSION` (`game-search-index@1`) and `item_obtain::SCHEMA_VERSION`
  (`game-item-obtain@1`), each placed after `_build`.
- Redaction keeps `_schema` in the header (`redact_prose` puts it before `_redacted`;
  `redact_strings` leaves it in place). `diff::META_KEYS` lists it, so notes never diff it.
- Bump rule, for every dataset above: bump `<n>` when a field is removed, renamed or retyped, or
  its meaning changes (units, scale, what a count counts). Adding an optional field does not
  bump. Bump in the same change as the shape change.
- Test: `pipeline::tests::every_domain_stamps_its_schema_after_build` runs every registered
  domain and checks `_schema == "<file stem>@<schema_version>"` in fourth place.
- `compare.rs` does not special-case `_schema`, so a golden from before this change differs by
  that one key. The shape-hash guard is not built.
- LIVE 4.10.201.48085: all 33 `game-*.json` files carry `_schema` right after `_build`.

### C. `game-npc-loadouts`

The gear NPCs spawn wearing and wielding, which is what a body can be looted for beyond its
loot-table pockets.

- Reference: starorg-codex `origin/dev`,
  `crates/starorg-gamedata/src/domains/npc_loadouts.rs` (2029 lines). Match its output shape
  (`NpcLoadouts` / `NpcActor` / `GearGroup` / `GearItem` / `Wear`) so the app can take the file
  as-is, and note here wherever it diverges.
- Inputs:
  - actor records under `libs/foundry/records/actor/actors/**` and `loadoutkits/**`;
  - loose loadout files `Data/Scripts/Loadouts/**`, in CryXmlB or text. Check that the TS
    extraction filters include them; they live outside DataCore.
- Rules carried over:
  - OR/NOT requirements are evaluated against the actor's tags.
  - Groups with the same `Group` name are pooled.
  - When groups share a name, gated groups win and ungated groups are the fallback.
  - Body, head, hair, skin and mobiGlas ports are dropped, and so is `Unlootable`.
- Estimates: the pick is assumed uniform, because the data has no weights. Per
  [dump-enhancements](dump-enhancements.md), that assumption goes in an `_assumptions` note and
  any share field gets the `Estimate` suffix, unless the app's shape fixes the name. In that
  case, record the divergence here.
- Work: Rust only (the TS parse is being retired on `feat/rust-switchover`), a domain in
  `crates/sc-gamedata`. The layout differs from the starorg crate: here `helpers_items.rs` is
  one file, not a `helpers_items/` module. Golden check: on LIVE 4.10.201.48085, the Rust
  output must be byte-identical to starorg-codex's. Measure the dump growth with `publish-dry`.
- Redaction: names and keys only, no prose. Add any description fields to `PROSE_FIELDS`.

#### Built (2026-11-25)

- `crates/sc-gamedata/src/domains/npc_loadouts.rs`, ported from starorg-codex `origin/dev`
  (`3d6a09e`). Its imports resolve unchanged here (`helpers_items::{item_common, tags}`). It is
  registered in `domains/mod.rs` and `TS_ORDER` (last). `GOLDEN_EXTRACTION_FILTERS` gains
  `**/actor/actors/**` and `**/loadoutkits/**`. On LIVE these add 2738 records, and none of
  them reach another domain's own filters. The loose files come from `files()`
  (`Data/Scripts/Loadouts/**`), which keeps the archive open for the run.
- Shape: `{ _assumptions, actors[], byLootTable, summary }`. Each actor has `class`, `name`
  (words from the class name), `faction`, `lootTable`, `lootable`, `wear {min,max}` and
  `items[] {item, name, slot, chance, requires}`. `GearGroup` is internal: the file is lean
  and has no per-group detail, the same as the codex.
- Golden (LIVE 4.10.201.48085, codex `gamedata-port extract --domain npc-loadouts` built
  from `origin/dev`): the actors (2162 with gear, of 2312), `byLootTable` (19) and `summary`
  are equal, and so are every actor's item set, slots and `requires`. The differences:
  - `_assumptions` (3 lines);
  - 60 `items[].name` values on 14 items, where the codex has no name. This crate's string
    table also resolves keys that exist only with the `,P` suffix
    (`item_Name_doom_combat_medium_helmet_01_01_01,P` → "Death's Head Helmet"), and the codex's
    does not. One of them is the game's own unfinished string, "DYL - jacket - name", which
    `game-armor` and `game-item-tags` carry too (the shared placeholder rule does not catch it);
  - 135 `items[].chance` values on 87 actors, all lower here (12 actors' items change order
    within a slot as a result). The codex adds an item's chance once per copy inside one
    option, so a rifle picked half the time with two magazines of one class gave the
    magazine a chance of 1. Here an item an option holds more than once is there as often as
    the option is (`flat` in `npc_loadouts.rs`, with a test). The codex needs the same fix.
- Review (2026-11-25): a separate Python reading of the raw LIVE records and loadout files
  (2312 actors, 426 kit records, 3647 loadout files) gives the same item set and chance for
  all 22,247 actor items, the same 698 unlootable drops, and the same `lootable`,
  `lootTable` and `faction`. A full parse before and after the change differs only by the
  new file (30 → 31 files, 51.04 → 55.30 MB pretty).
- Divergences: `chance` keeps the codex name, not `chanceEstimate`, because the app reads it
  as-is. The `_assumptions` note marks it as an estimate (even picks, gated-before-ungated
  pooling). `_assumptions` is an extra top-level key, which readers ignore.
- Size: 4,265,834 B pretty, 2,673,939 B compact, 180,478 B gzipped. There are no prose
  fields, so `PROSE_FIELDS` is unchanged.
- Known gaps:
  - items that are no personal gear are left out, which includes the trophies creature
    actors hold in their ports (`Harvestable_Trophy_1H_KopionHorn`, Valakkar fangs,
    Quasigrazer eggs; 36 entries of type `Misc/Harvestable`);
  - `chance` is not rounded: 29 values that should be 1 print as `0.9999999999999999` or
    `0.9999999999999998`, as in the codex;
  - `actors[].name` is words made from the class name, not a string from the records, and
    the two `requires` labels in the file are English text this code writes
    (`outfit picked by the mission (Outfits/S42/Work)`).
- Warning on LIVE: the records name 65 loadout files that the archive lacks (creature
  loadouts such as `Qausi_graser/tauroch_*.xml`). They are left out, as in the codex.

### D. Publish LIVE 4.10.201.48085

- The post-exit hook was installed on 2026-11-25 with `SC_AUTO_PUBLISH=0`. On the next game exit
  it extracts and dry-runs from `~/git/worktrees/sc-gamedata-release` (`origin/main`). After the dry run
  looks right, publish with `make -C ~/git/worktrees/sc-gamedata-release publish CHANNEL=LIVE`.
- The release worktree runs `origin/main`. Until this repo's working changes (per-channel
  dirs, `SC_DATA_DIR`, `build.json`, the `.github` template, fire, item-sources) are merged,
  a publish from it has none of them. Merge first if the tag should carry `build.json`.

### E. Reader feedback from starorg-codex (2026-11-25)

The app's dump reader is built (not merged). Checking it against tag 4.10.200.58746 found the
following.

1. **One clean extraction per tag.**
   - Problem: 4.10.200.58746 carries stale records in `game-ships`, `game-fps-weapons` and
     `game-item-tags`, left over from an old `extracted-data/`. Its `game-item-tags` also lacks
     `ammoItems` / `summary.withAmmo`. The app refuses those three files for that tag.
   - Fixed going forward: the Rust pipeline (`feat/rust-switchover`) reads `Data.p4k` directly,
     with no stale extract folder to pick up, and the hook always runs a full `make all` in the
     release worktree. The 4.10.200 tag can't be republished, because that build is no longer
     installed, so it stays partly refused.
2. **`description` redaction. Decided 2026-11-25: option b.**
   - Problem: `PROSE_FIELDS` strips `description` from commodities, manufacturers, missions,
     refining (`description`, `rawDescription`, `refinedDescription`), ships and starmap. The
     app shows descriptions (commodity detail; MCP and Developer mode show whole records), so
     those six files are unusable for it today.
   - Option a: keep `description` in those files. It reverses the reason for the strip: prose
     is the most copyrightable content in the dumps and the part nothing else reads.
   - Option b (**chosen by the user**): keep the strip. The app fills the text from `descriptionKey`
     using the player's own local `global.ini`. It already reads the local `Data.p4k`, so it has
     the file.
3. **Reader rules.** Publish must keep these true:
   - `build.json` `build` and `channel` must match the tag and channel. A build published as
     PTU is never reused on LIVE.
   - `files[]` `sha256`/`bytes` are used when present (else the git blob sha and size).
   - `extractor.dirty: true` refuses the whole tag. The hook's clean release worktree never
     sets it.
   - `_schema` is checked when present, `_build` must equal the tag, and unknown fields are
     ignored.
4. **`game-npc-loadouts` shape:** top-level `actors`, `byLootTable` and `summary`, as in the
   codex's `npc_loadouts.rs` on `origin/dev`. The Rust port is running on
   `feat/rust-npc-loadouts`.
5. **Compatible as-is**, the same shape as the app's 4.10.201 output:
   - ammo, armor, blueprints, cargo, consumables, containers, item-sources, law, loot,
     mining, mining-spawns, quantum, reputation, salvage, ship-components, shops, weapons,
     wikelo-trades;
   - also commodities, manufacturers, missions, refining and starmap, which are blocked only
     by point 2.

## Order

D (after merge) → A → B → C. The app works from D alone, by shape. A lets it verify each file.
B lets it skip a dataset without parsing it. C is new data.
