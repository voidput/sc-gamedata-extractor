---
title: Dump reuse by starorg-codex
tags: [docs, plans, specs]
updated: 2026-11-25
status: draft
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
- Work: a TS domain plus a Rust domain in `crates/sc-gamedata`. The layout differs from the
  starorg crate: here `helpers_items.rs` is one file, not a `helpers_items/` module. Golden
  check: on LIVE 4.10.201.48085, the Rust output must be byte-identical to starorg-codex's and
  the TS output must match it. Measure the dump growth with `publish-dry`.
- Redaction: names and keys only, no prose. Add any description fields to `PROSE_FIELDS`.

### D. Publish LIVE 4.10.201.48085

- The post-exit hook was installed on 2026-11-25 with `SC_AUTO_PUBLISH=0`. On the next game exit
  it extracts and dry-runs from `~/git/sc-gamedata-release` (`origin/main`). After the dry run
  looks right, publish with `make -C ~/git/sc-gamedata-release publish CHANNEL=LIVE`.
- The release worktree runs `origin/main`. Until this repo's working changes (per-channel
  dirs, `SC_DATA_DIR`, `build.json`, the `.github` template, fire, item-sources) are merged,
  a publish from it has none of them. Merge first if the tag should carry `build.json`.

## Order

D (after merge) → A → B → C. The app works from D alone, by shape. A lets it verify each file.
B lets it skip a dataset without parsing it. C is new data.
