# sc-gamedata-dumps

Per-build snapshots of Star Citizen game data, parsed into JSON.

One commit per game build, tagged with that build string. The tag list is the
patch history — which makes this the only place to read what a value *was* in a
past patch, since every live API only ever serves the build it is synced to.

```
git tag                       # every published build
git show <build>:data/game-ships.json
```

Consumed by [SCMCP](https://github.com/voidput/SCMCP) (`sc_list_builds`,
`sc_diff_versions`). Files are fetched by tag sha and cached, so a published
build never changes.

## Datasets

`game-ships` · `game-ship-components` · `game-fps-weapons` · `game-ammo` ·
`game-mining` · `game-mining-spawns` · `game-blueprints` · `game-missions` ·
`game-reputation` · `game-containers` · `game-starmap` · `game-manufacturers` ·
`game-wikelo-trades` · `game-strings-english`

Each file is an envelope — `_source`, `_extracted`, `_build` — wrapping named
collections. The `by*` keys are derived indexes over the same records, not
additional records.

## What is not here

Localisation is limited to name keys. Descriptive text, dialogue and mission
briefings are not published. No game assets, binaries, models, textures or audio
are published, and never will be.

## Notice

This is an unofficial Star Citizen fan project, published non-commercially.

This site is not endorsed by or affiliated with the Cloud Imperium or Roberts
Space Industries group of companies. All game content and materials are
copyright Cloud Imperium Rights LLC and Cloud Imperium Rights Ltd. Star
Citizen®, Squadron 42®, Roberts Space Industries®, and Cloud Imperium® are
registered trademarks of Cloud Imperium Rights LLC. All rights reserved.

It will be removed on request from Cloud Imperium Games. See [NOTICE](NOTICE).
