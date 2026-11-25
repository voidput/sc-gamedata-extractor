---
title: Tool data sync from the dumps hub
tags: [docs, plans, specs]
updated: 2026-11-25
status: active
---

# Tool data sync from the dumps hub

Keep snares, MCsim and Bindscape on the latest game build, for LIVE and PTU, with no manual
step after `make publish`. The extract stays local, because it needs `Data.p4k`. Everything
after the push to `voidput/sc-gamedata-dumps` runs in GitHub Actions. Source of this plan: a
read of all four repos on 2026-11-25 (sc-gamedata HEAD `350e7053b`).

## State (2026-11-25)

| Tool | Repo | Site | Reads | Bake script → committed output | PTU |
|---|---|---|---|---|---|
| snares | `voidput/snares` | snare.starorg.tools | `game-starmap`, `game-ship-components`, `game-quantum`, `game-ships` | `scripts/build-data.mjs` → `public/data/universe.json` | yes, `universe-ptu.json` |
| MCsim | `voidput/MCsim` (local `persim`) | mcsim.starorg.tools | `game-ships`, `game-ship-components`, `game-weapons`, `game-fire` and **raw `extracted-data/libs/foundry/records`** | `build_fire.py`, `build_ordnance.py` → `public/{fire,ordnance}{,-ptu}.json` + `*-channels.json` | yes |
| Bindscape | `voidput/scsticks` | bindscape.starorg.tools | `game-keybinds`, `game-attributes` | `import_sc_keybinds.js`, `import_sc_attributes.js` → `public/data/games/star-citizen/{actions,attributes}.json` | no |

- Every bake script reads a **sibling checkout** (`../sc-gamedata/data`), so CI can't run it.
  The baked files are committed, so each Pages build works without game files.
- Each `pages.yml` deploys on a push to `main` or on `workflow_dispatch`. None of them
  refreshes data.
- `publish-dump.mjs` reads a hard-coded `data/`. A `make ptu` with the default `OUT` therefore
  overwrites the LIVE `data/`. `data-ptu/` holds only the 6 files that the consumers bake from.
- Gaps in the dumps:
  - `game-fire` is not in a published build yet; the newest tag is `4.10.200.58746`.
  - `game-keybinds` and `game-attributes` are only on `feat/keybinds-domain` (worktree
    `~/git/sc-gamedata-keybinds`) and are not merged.
  - MCsim's ordnance needs raw records, which the dumps never carry.
  - The dumps record the channel only in the release title and flags. No file holds it.
- **sc-userdata is not a hub input.** It is the private, auto-committed backup of the user's own
  game folder (actionmaps, `attributes.xml`, `USER.cfg`, characters). Bindscape uses it only for
  local round-trip tests (README "Tests"), and those tests skip when it is absent. Bindscape's
  game defaults come from Data.p4k through sc-gamedata. sc-userdata stays out of CI and off every
  public site.

## Design

```
launcher exit → sc-postexit.sh hook → release-on-exit.sh
local: make release-live|release-ptu           (needs Data.p4k, stays local)
          │ git push tag + gh release
          ▼
voidput/sc-gamedata-dumps  .github/workflows/fanout.yml
          │ on: release [published, edited] | workflow_dispatch(tag)
          │ reads build.json → {build, channel, patch}
          │ repository_dispatch "gamedata-published" ×3  (TOOLS_DISPATCH_TOKEN)
          ▼
snares / MCsim / scsticks  .github/workflows/data-sync.yml
          sparse clone dumps@tag (data/ only)
          → bake script with --data <dir> --channel live|ptu
          → commit baked files to main if changed ("data: <channel> <build>")
          → call pages.yml (workflow_call), because a GITHUB_TOKEN push triggers nothing
```

### Hub (`sc-gamedata-dumps`)

- Publish writes `build.json` at the repo root, beside `data/` and `notes/`:
  `{ build, channel, patch, branch }` (two-space JSON, `null` when unknown). It has no
  timestamp, so republishing an identical build still reads as "nothing to republish". The
  fan-out reads it instead of parsing the release title. Built: `build_manifest` in
  `crates/sc-gamedata/src/publish.rs` (byte-identical to the former `publish-dump.mjs`
  `buildManifest`, removed 2026-11-25). Publish
  always refreshes the template's `.github/`; README and NOTICE are copied only when missing.
- `fanout.yml`, kept in `scripts/dump-repo-template/.github/workflows/` so that publish
  installs it:
  - `on: release: types: [published, edited]`. `edited` covers `--republish`, which refreshes
    the notes. Also `workflow_dispatch` with a `tag` input, for backfill or a retry.
  - The job checks out the tag, reads `build.json`, and sends one
    `POST /repos/voidput/<repo>/dispatches` per tool with `event_type: gamedata-published` and
    `client_payload: {tag, build, channel}`.
  - A tool that fails does not block the others (`strategy.fail-fast: false` matrix over the
    repo list).
- Secret `TOOLS_DISPATCH_TOKEN`: a fine-grained PAT, or a GitHub App, with **Contents: write**
  on snares, MCsim and scsticks only. This is what the dispatches API requires. The user creates
  it and stores it in the dumps repo.

### Consumers (`data-sync.yml`, same shape in all three)

- `on: repository_dispatch: types: [gamedata-published]`, plus `workflow_dispatch` with `tag`
  and `channel` inputs.
- `permissions: contents: write`. Concurrency group `data-sync`, `cancel-in-progress: false`.
- The fetch is
  `git clone --depth 1 --branch "$TAG" --filter=blob:none --sparse https://github.com/voidput/sc-gamedata-dumps dumps && git -C dumps sparse-checkout set data`.
- Bake with the dumps dir as the source:
  - snares: `--data dumps/data --channel <ch>`. That flag already exists.
  - MCsim: `SC_GAMEDATA`, with `data/` mapped per channel.
  - Bindscape: positional source path.
- **No regress guard**: skip when the committed output's `build` is newer than the payload's,
  compared by version order (same rule as the notes). This covers out-of-order dispatches and
  an old-tag backfill.
- Commit only on a diff, with the bot identity `github-actions[bot]`. Then run
  `jobs.deploy: uses: ./.github/workflows/pages.yml`, which means adding `on: workflow_call` to
  each `pages.yml`.
- The test step stays in the deploy path. Bindscape's tests check that every curated key
  (`settings.json`, `essentials.json`) exists in the new data, so a patch that renames an action
  fails the sync instead of shipping a broken page.

### Make side

- `EXTRACT_DIR` and `OUT` default per channel: `./extracted-data` and `./data` for LIVE, and
  `./extracted-data-<ch>` and `./data-<ch>` otherwise (`make ptu` → `data-ptu`). The LIVE `data/` is
  never overwritten.
- The Rust `publish` reads `SC_DATA_DIR` (default `data/`) and `SC_EXTRACT_DIR`, and the
  Make `PUBLISH` passes `$(OUT)` and `$(EXTRACT_DIR)`.
- New targets: `make release-live` and `make release-ptu`, each `all` → `publish-dry` →
  `publish` for that channel. `make release-check` lists the installed builds against the dump
  tags.
- `data-ptu/` must then hold the full set of datasets, not the 6-file subset, because the dumps
  publish everything.

### Build trigger: launcher post-exit hook

The launcher already updates the install whenever you play. When the game exits, the hook
publishes any channel whose installed build is not yet in the dumps. It needs no server, no
second install and no extra login.

- **Hook point.** `sc-launch.sh` runs `/mnt/games/star-citizen/sc-postexit.sh` through
  `trap … EXIT`. That script already detaches `sc-backup.sh` with `setsid nohup`, and it
  carries a managed `starorg-codex` block. Add the same kind of managed block:
  `# >>> sc-gamedata hook >>>` … `# <<< sc-gamedata hook <<<`. The block calls
  `scripts/release-on-exit.sh` in this repo, detached, and logs to
  `~/.local/state/sc-gamedata-release.log`. It is installed and removed with
  `make hook-install` and `make hook-uninstall`, and never edited by hand.
- **Channel detection.** The hook does not know which channel was launched, so it checks each
  of `LIVE` and `PTU` under `$SC_ROOT`. HOTFIX waits on the open question below.
  - It reads `Data.Version` from `<channel>/build_manifest.id`. For example, PTU was
    `5.0.202.35026` (branch `sc-alpha-5.0.0`) on 2026-11-25.
  - It compares that against the tags from
    `git ls-remote --tags git@github.com:voidput/sc-gamedata-dumps.git`. The dumps are the
    record of what is published, so there is no local state file to drift.
  - A version with no tag is a channel to release.
- **Clean checkout.** The release never runs from the dev checkout. It uses a dedicated
  worktree, `~/git/worktrees/sc-gamedata-release`, detached at `origin/main`: fetch, checkout, then
  `make build`. A worktree with local changes is refused.
  This keeps the "no publish from a working branch" rule.
- **Run.** Wait `SC_RELEASE_DELAY` (60 s) so wine processes settle, and skip if
  `StarCitizen.exe` or `RSI Launcher.exe` is still running. Then for each new channel:
  `make all`, then `publish-dry`, then `publish`.
  - `flock ~/.local/state/sc-gamedata-release.lock`, so one run at a time.
  - `nice -n 19 ionice -c3`, so a relaunch is not starved.
  - A failed extract (bad or partial p4k) stops before publish. There is no partial release.
- **Opt-in gate.** `SC_AUTO_PUBLISH=1` (set in the hook block) enables the real `publish`.
  Without it the hook runs everything up to `publish-dry` and posts a desktop notification
  (`notify-send`) with the build and the size of the dry-run diff. Start with the gate off;
  turn it on after a couple of clean dry runs.
- **Relaunch guard.** Before publishing, the run compares a fingerprint taken at the start (the
  sha256 of `build_manifest.id`, plus the `Data.p4k` size and mtime) and checks that the game is
  not running. If either changed, it does not publish. The prelaunch block only warns (echo +
  `notify-send`) when the lock is held, because the launcher may be patching `Data.p4k`
  mid-read. It does not block the launch.
  The extract takes about 48 s (Rust, see [rust-pipeline](rust-pipeline.md)).
- **Release creation.** The Rust `publish` never runs `gh`: `make publish` / `republish` have
  it write the release steps to `.tmp/gh-release-<CHANNEL>.sh` (`--gh-script`), then `sh` that
  with the local `gh` auth (`gh release edit` on a republish, then create unless `gh release
  view` finds one). The hook runs `make publish`, so it publishes with Rust since 2026-11-25.
- **Limit.** PTU updates only when you open the launcher on PTU. Fixing that needs an
  unattended launcher; see the later option.

Later option, not planned: an always-on launcher service on zima (Wine + Xvfb + noVNC, pinned
to zima4) with a `build_manifest.id` watcher that triggers a `runs-on: zima` job. It needs a
login that persists through 2FA, possibly scripted update clicks, a ToS check on unattended
use, and 310 GB on `local-path`. See [ideas](../later/ideas.md).

## Steps

1. **Make + publish.** Add the per-channel `OUT` defaults, `SC_DATA_DIR`, `build.json`, and the
   `release-*` targets. Rust parity in `publish.rs`. Dry-run both channels.
2. **Post-exit trigger.** Add `scripts/release-on-exit.sh`, `make hook-install` and
   `make hook-uninstall`, the release worktree, and the prelaunch lock warning. Run with
   `SC_AUTO_PUBLISH` unset until two dry runs are clean.
3. **Hub fan-out.** Add `fanout.yml` to the dump template. The user creates
   `TOOLS_DISPATCH_TOKEN`. Test with `workflow_dispatch` on the current tag.
4. **snares.** Open as PR voidput/snares#10 (branch `ci/data-sync`), checks green, not merged.
   - `universe*.json` already carry top-level `build` and `channel`.
   - The `main` ruleset requires a PR plus the `check` status, squash only, so the sync uses a
     PR flow: branch `data/<channel>-<build>`, then `gh pr create`, then `gh workflow run ci.yml`
     on that branch (PRs opened by the token start no checks), then wait for checks, then
     `gh pr merge --squash`, which feeds the merge sha to `pages.yml`. Extra permissions:
     `pull-requests: write`, `actions: write`.
   - The commit title is `chore(data): <CHANNEL> <build>`, because the pr-title check rejects
     `data:`.
   - Test after merge: LIVE committed data is already 4.10.201.48085, so test on that tag once it
     is published, or on a PTU tag. 4.10.200 hits the no-regress guard.
5. **Bindscape.** Merge `feat/keybinds-domain` (game-keybinds and game-attributes) and
   publish.
   - 2026-11-25: `data-sync.yml` and `pages.yml` are committed in scsticks as `1ff6d16`, not
     pushed. `main` is unprotected and npm test passes (180).
   - The guard reads `_build`, which the import scripts copy from the dataset.
   - Labels survive redaction: redacted feat-branch data fed through both importers gives
     byte-identical output. Labels are resolved inline, not looked up in the string table.
   - **Keep unredacted** in game-keybinds and game-attributes (never add them to
     `PROSE_FIELDS`, nor to the Rust redaction list): `label`, `labelKey`, `optionTrees`
     (`showInvert`, `showCurve`, `invert`, `exponent`, `parent`), `optionGroup`, `platform`,
     `confirmation`, `version`.
   - PTU is skipped for now (LIVE only).
   - After `feat/rust-keybinds` merges, tell the scsticks session so it can switch 4 stale
     references in one commit: `scripts/import_sc_keybinds.js:10` and `docs/tooling.md:19`
     (`npm run all keybinds`), and `scripts/import_sc_attributes.js:11` and `docs/tooling.md:20`
     (`node dist/cli.js all game-attributes`). They become `make parse DOMAINS=...`. These are
     comments and docs only, since data-sync.yml passes paths explicitly.
6. **MCsim, fire.** 2026-11-25: `build_fire.py` is ported to parsed datasets (`game-fire` +
   `game-ships` + `game-ship-components` + `game-weapons`), and `data-sync.yml` no longer
   checks raw records. It is uncommitted in persim. Parity on LIVE 4.10.201.48085 is equal,
   plus 6 more ships.
   - The committed fire files stay on the old bake until
     [consumer-datasets](consumer-datasets.md) items 1 and 2 land (fuel tanks, explosive
     `hitExplosion`).
7. **MCsim, ordnance.** The field list is in [consumer-datasets](consumer-datasets.md) item 3. Move the raw-record reads of `build_ordnance.py` and
   `missile_controllers.py` into a sc-gamedata `game-ordnance` domain. Follow the `game-fire`
   handoff precedent. Since the TS parse was removed (2026-11-25, see
   [rust-pipeline](rust-pipeline.md)) a new domain is written in Rust only, in this crate and
   starorg-strings' copy. Until
   then, ordnance stays a local bake. `build_data.py` (GLB ship models) stays local, out of
   scope.

## Rules

- Values come from extracted records. CI never derives game data; it only re-bakes from a
  published tag.
- No publish from a working branch: `make publish` runs only after merge, on a real build.
- Consumer bakes are pure functions of `(tag, channel)`. Re-running one is a no-op.
- **No ship models in public repos** (user rule, 2026-11-25). GLBs and mesh geometry never go
  into `voidput/sc-gamedata-dumps`, snares, scsticks or any other public repo. Parsed stat
  datasets are fine. Publish copies only `data/game-*.json`, `notes/`, `build.json` and the
  template, so nothing changes today. Any new dataset that would carry geometry (vertices,
  meshes, hull shapes beyond bounding boxes and cross-sections) needs the user's OK first.

## Open questions

- Is `main` protected in the consumers? If so, the sync opens a PR and auto-merges it instead of
  pushing.
- Should HOTFIX sync as LIVE? It is published `--latest=false` and is not a prerelease.
- Combining the three sites into one is out of scope here. They already share `starorg.tools`
  subdomains.
