---
title: sc-gamedata docs
tags: [docs]
updated: 2026-11-25
---

# sc-gamedata docs

Vault index (Obsidian). Plain-link twin: [README.md](README.md).

## Plans

- [[plans/README]] — ([open](plans/README.md)) — plans index: now, next, later, specs
- [[plans/now/todo]] — ([open](plans/now/todo.md)) — active sprint
- [[plans/next/backlog]] — ([open](plans/next/backlog.md)) — confirmed backlog
- [[plans/later/ideas]] — ([open](plans/later/ideas.md)) — ice box

## Specs

- [[dump-enhancements]] — ([open](plans/specs/dump-enhancements.md)) — port starorg-codex derived fields into published dumps, strip prose (active)
- [[rust-pipeline]] — ([open](plans/specs/rust-pipeline.md)) — Rust crate here as the dataset producer (active)
- [[tool-data-sync]] — ([open](plans/specs/tool-data-sync.md)) — dumps hub fans out to snares, MCsim, Bindscape via GHA, LIVE + PTU (active)
- [[dump-reuse]] — ([open](plans/specs/dump-reuse.md)) — build.json manifest, _schema, game-npc-loadouts for starorg-codex reuse (active)
- [[consumer-datasets]] — ([open](plans/specs/consumer-datasets.md)) — dataset asks from snares/MCsim/Bindscape/sc-telemetry: game-fire gaps, game-ordnance, ship capabilities (active)
- [[cross-dataset-derivations]] — ([open](plans/specs/cross-dataset-derivations.md)) — joins over published datasets: reverse links, missions × reputation/starmap, tag index, resource chain, per-ship totals (active)

## Datasets

- [[datasets/README]] — ([open](datasets/README.md)) — the data pack: what a tag holds, header fields, dependencies, consumer matrix
- [[game-ammo]] — ([open](datasets/game-ammo.md)) — Every `AmmoParams` record
- [[game-armor]] — ([open](datasets/game-armor.md)) — Personal armour and clothing
- [[game-attributes]] — ([open](datasets/game-attributes.md)) — The game's default option values (graphics, audio, camera, flight, HUD, input), one row per `<Attr>` in…
- [[game-blueprints]] — ([open](datasets/game-blueprints.md)) — Crafting blueprints
- [[game-cargo]] — ([open](datasets/game-cargo.md)) — What NPC and mission ships spawn carrying (`CargoManifest` records), and the freight elevator and…
- [[game-commodities]] — ([open](datasets/game-commodities.md)) — Every resource type the economy knows (metals, gases, processed goods and the placeholder groups for heat,…
- [[game-consumables]] — ([open](datasets/game-consumables.md)) — Medpens, pills, food and drink, and what taking one does
- [[game-containers]] — ([open](datasets/game-containers.md)) — Every `InventoryContainer` record
- [[game-fire]] — ([open](datasets/game-fire.md)) — The fire-hazard rules every fire obeys (ignition by damage type, oxygen window, spread, room heat, player…
- [[game-fps-weapons]] — ([open](datasets/game-fps-weapons.md)) — A thin catalogue of personal weapons
- [[game-item-obtain]] — ([open](datasets/game-item-obtain.md)) — Where each item comes from, worked out once per build
- [[game-item-sources]] — ([open](datasets/game-item-sources.md)) — Where items spawn and what hands them out
- [[game-item-tags]] — ([open](datasets/game-item-tags.md)) — Every item record that carries record-level tags, with those tags resolved from GUIDs to paths…
- [[game-keybinds]] — ([open](datasets/game-keybinds.md)) — The game's default control bindings
- [[game-law]] — ([open](datasets/game-law.md)) — The law system
- [[game-loot]] — ([open](datasets/game-loot.md)) — Loot tables and archetypes with their weights, in the game's own shape, compacted
- [[game-manufacturers]] — ([open](datasets/game-manufacturers.md)) — The in-fiction companies behind ships, components and gear
- [[game-mining-spawns]] — ([open](datasets/game-mining-spawns.md)) — Where ores spawn
- [[game-mining]] — ([open](datasets/game-mining.md)) — The physics of breaking rock, per mineable element
- [[game-missions]] — ([open](datasets/game-missions.md)) — Every `MissionBrokerEntry` record, the contracts the mobiGlas contract manager offers
- [[game-npc-loadouts]] — ([open](datasets/game-npc-loadouts.md)) — What each NPC type (actor) can spawn wearing and wielding, which is what its body can be looted for…
- [[game-paints]] — ([open](datasets/game-paints.md)) — Every ship paint and the vehicles it fits, by the game's port tag rule
- [[game-quantum]] — ([open](datasets/game-quantum.md)) — The global quantum-travel rules every quantum drive obeys
- [[game-refining]] — ([open](datasets/game-refining.md)) — What the client knows about refining
- [[game-reputation]] — ([open](datasets/game-reputation.md)) — Every reputation rank (`SReputationStandingParams` record)
- [[game-salvage]] — ([open](datasets/game-salvage.md)) — The global salvage constants
- [[game-search-index]] — ([open](datasets/game-search-index.md)) — The fuzzy-find entries, precomputed
- [[game-ship-components]] — ([open](datasets/game-ship-components.md)) — Everything that slots into a ship and carries a stat
- [[game-ships]] — ([open](datasets/game-ships.md)) — Every ship and ground vehicle record
- [[game-shops]] — ([open](datasets/game-shops.md)) — The global rules that every shop and commodity kiosk applies
- [[game-starmap]] — ([open](datasets/game-starmap.md)) — Every `StarMapObject` record (star systems, stars, planets, moons, stations, outposts, points of interest,…
- [[game-strings-english]] — ([open](datasets/game-strings-english.md)) — The English string table (`global.ini`) as a dataset
- [[game-weapons]] — ([open](datasets/game-weapons.md)) — The combat baseline
- [[game-wikelo-trades]] — ([open](datasets/game-wikelo-trades.md)) — Wikelo Emporium barter trades
