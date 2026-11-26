---
title: sc-gamedata docs
tags: [docs]
updated: 2026-11-25
---

# sc-gamedata docs

Plain-markdown index. Obsidian index: [Home.md](Home.md).

## Plans

- [Plans index](plans/README.md) — now, next, later, specs
- [Active sprint](plans/now/todo.md)
- [Backlog](plans/next/backlog.md)
- [Ideas](plans/later/ideas.md)

## Specs

- [Dump enhancements](plans/specs/dump-enhancements.md) — port starorg-codex derived fields into published dumps, strip prose (active)
- [Rust pipeline](plans/specs/rust-pipeline.md) — Rust crate here as the dataset producer (active)
- [Tool data sync](plans/specs/tool-data-sync.md) — dumps hub fans out to snares, MCsim, Bindscape via GHA, LIVE + PTU (active)
- [Dump reuse](plans/specs/dump-reuse.md) — build.json manifest, _schema, game-npc-loadouts for starorg-codex reuse (active)
- [Consumer datasets](plans/specs/consumer-datasets.md) — dataset asks from snares/MCsim/Bindscape/sc-telemetry: game-fire gaps, game-ordnance, ship capabilities (active)
- [Cross-dataset derivations](plans/specs/cross-dataset-derivations.md) — joins over published datasets: reverse links, missions × reputation/starmap, tag index, resource chain, per-ship totals (active)

## Datasets

- [The data pack](datasets/README.md) — what a tag holds, header fields, dependencies, consumer matrix
- [game-ammo](datasets/game-ammo.md) — Every `AmmoParams` record
- [game-armor](datasets/game-armor.md) — Personal armour and clothing
- [game-attributes](datasets/game-attributes.md) — The game's default option values (graphics, audio, camera, flight, HUD, input), one row per `<Attr>` in…
- [game-blueprints](datasets/game-blueprints.md) — Crafting blueprints
- [game-cargo](datasets/game-cargo.md) — What NPC and mission ships spawn carrying (`CargoManifest` records), and the freight elevator and…
- [game-commodities](datasets/game-commodities.md) — Every resource type the economy knows (metals, gases, processed goods and the placeholder groups for heat,…
- [game-consumables](datasets/game-consumables.md) — Medpens, pills, food and drink, and what taking one does
- [game-containers](datasets/game-containers.md) — Every `InventoryContainer` record
- [game-fire](datasets/game-fire.md) — The fire-hazard rules every fire obeys (ignition by damage type, oxygen window, spread, room heat, player…
- [game-fps-weapons](datasets/game-fps-weapons.md) — A thin catalogue of personal weapons
- [game-item-obtain](datasets/game-item-obtain.md) — Where each item comes from, worked out once per build
- [game-item-sources](datasets/game-item-sources.md) — Where items spawn and what hands them out
- [game-item-tags](datasets/game-item-tags.md) — Every item record that carries record-level tags, with those tags resolved from GUIDs to paths…
- [game-keybinds](datasets/game-keybinds.md) — The game's default control bindings
- [game-law](datasets/game-law.md) — The law system
- [game-loot](datasets/game-loot.md) — Loot tables and archetypes with their weights, in the game's own shape, compacted
- [game-manufacturers](datasets/game-manufacturers.md) — The in-fiction companies behind ships, components and gear
- [game-mining-spawns](datasets/game-mining-spawns.md) — Where ores spawn
- [game-mining](datasets/game-mining.md) — The physics of breaking rock, per mineable element
- [game-missions](datasets/game-missions.md) — Every `MissionBrokerEntry` record, the contracts the mobiGlas contract manager offers
- [game-npc-loadouts](datasets/game-npc-loadouts.md) — What each NPC type (actor) can spawn wearing and wielding, which is what its body can be looted for…
- [game-paints](datasets/game-paints.md) — Every ship paint and the vehicles it fits, by the game's port tag rule
- [game-quantum](datasets/game-quantum.md) — The global quantum-travel rules every quantum drive obeys
- [game-refining](datasets/game-refining.md) — What the client knows about refining
- [game-reputation](datasets/game-reputation.md) — Every reputation rank (`SReputationStandingParams` record)
- [game-salvage](datasets/game-salvage.md) — The global salvage constants
- [game-search-index](datasets/game-search-index.md) — The fuzzy-find entries, precomputed
- [game-ship-components](datasets/game-ship-components.md) — Everything that slots into a ship and carries a stat
- [game-ships](datasets/game-ships.md) — Every ship and ground vehicle record
- [game-shops](datasets/game-shops.md) — The global rules that every shop and commodity kiosk applies
- [game-starmap](datasets/game-starmap.md) — Every `StarMapObject` record (star systems, stars, planets, moons, stations, outposts, points of interest,…
- [game-strings-english](datasets/game-strings-english.md) — The English string table (`global.ini`) as a dataset
- [game-weapons](datasets/game-weapons.md) — The combat baseline
- [game-wikelo-trades](datasets/game-wikelo-trades.md) — Wikelo Emporium barter trades
