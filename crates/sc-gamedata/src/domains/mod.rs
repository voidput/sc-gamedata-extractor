//! The domain registry.
//!
//! One slot per `sc-gamedata` domain, each on its own line with a blank line around it, so
//! porters working in parallel never touch neighbouring lines (git merges such edits
//! cleanly; adjacent edits conflict). To register a port: create `src/domains/<slot>.rs`
//! exporting `pub static DOMAIN`, and uncomment its slot. Nothing else in this file
//! changes.
//!
//! The comment after each slot names the TypeScript file it ports (and the output file
//! when that differs from `game-<slot>`).

use crate::domain::Domain;

// Shared helpers several TypeScript domains import (not domains themselves). Same rule:
// one slot each, uncommented by whoever ports it first; dependants wait for it on main.

// pub mod item_common; // domains/item-common.ts (armor, blueprints, components, consumables, item-sources, item-tags, ships, weapons)

// pub mod tags; // src/tags.ts (item-sources, item-tags, loot)

pub mod socpak; // src/socpak.ts (starmap: object containers in the archive, read via ParseContext::files)

macro_rules! registry {
    ($($slot:ident),* $(,)?) => {
        $(pub mod $slot;)*

        /// Every ported domain, in slot order.
        pub fn all() -> Vec<&'static dyn Domain> {
            vec![$(&$slot::DOMAIN),*]
        }
    };
}

registry! {
    ammo, // ammo.ts

    armor, // armor.ts

    attributes, // attributes.ts (feat/keybinds-domain; archive file, no records)

    blueprints, // blueprints.ts

    cargo, // cargo.ts

    commodities, // commodities.ts

    components, // components.ts -> game-ship-components

    consumables, // consumables.ts

    containers, // containers.ts

    fire, // fire.ts

    fps_weapons, // fps-weapons.ts

    item_sources, // item-sources.ts

    item_tags, // item-tags.ts

    keybinds, // keybinds.ts (feat/keybinds-domain; archive file, no records)

    law, // law.ts

    loot, // loot.ts

    manufacturers, // manufacturers.ts

    mining, // mining.ts

    mining_spawns, // mining-spawns.ts

    missions, // missions.ts

    npc_loadouts, // Rust only (starorg-codex npc_loadouts.rs)

    paints, // Rust only (paint items and the vehicles their tags fit)

    quantum, // quantum.ts

    refining, // refining.ts

    reputation, // reputation.ts

    salvage, // salvage.ts

    ships, // ships.ts

    shops, // shops.ts

    starmap, // starmap.ts

    weapons, // weapons.ts

    wikelo, // wikelo.ts -> game-wikelo-trades
}

/// A domain by output name (`game-manufacturers`) or short name (`manufacturers`).
pub fn find(name: &str) -> Option<&'static dyn Domain> {
    all()
        .into_iter()
        .find(|d| d.name() == name || d.name().strip_prefix("game-") == Some(name))
}
