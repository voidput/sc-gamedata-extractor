//! Rust port of `sc-gamedata`: Star Citizen game data domains read from `Data.p4k` and
//! written as the same JSON datasets (`game-<domain>.json`) the published dumps carry.
//!
//! Layers, each a port of one `sc-gamedata` module:
//!
//! - [`records`] (`records.ts`): records, record sets, reference following, accessors;
//! - [`entity`] (`entity.ts`): components of `EntityClassDefinition` records;
//! - [`localization`] (`localization.ts`): the string table, `localize`, overlays, and the
//!   published-dump redaction (off by default);
//! - [`overlays`] (`sources/overlays.ts`): the community pack registry, fetching (HTTP
//!   supplied by the caller), installed packs, loading a pack from disk;
//! - [`search_index`] (`search-index.ts`): `game-search-index.json`, derived from the
//!   item-tags and ships datasets;
//! - [`item_obtain`]: `game-item-obtain.json`, loot odds and other sources per item, derived
//!   from the item-tags, loot, item-sources, blueprints, Wikelo and NPC loadout datasets;
//! - [`tag_index`]: `game-tag-index.json`, tag path -> items and selecting loot records,
//!   derived from the item-tags and loot datasets;
//! - [`cryxml`] (`cryxml.ts`): CryXmlB documents in raw archive files; [`xml`] (`xml.ts`):
//!   text XML, and either form of an archive config file;
//! - [`glob`] (`glob.ts`), [`collate`] (`Intl.Collator`/`localeCompare` orders);
//! - [`output`]: `JSON.stringify(_, null, 2)`-exact writing and the dataset header;
//! - [`domain`] + [`domains`]: the `Domain` trait and the registry;
//! - [`pipeline`]: extraction scoping and running a domain;
//! - [`query`] (`query/*.ts`): fuzzy find, where an item comes from, shops (reads datasets);
//! - [`compare`]: the byte compare and path diff the `gamedata-port` harness uses against golden dumps;
//! - [`archive`] + [`source`]: the real archive via the StarBreaker adapter;
//! - [`fixtures`]: in-memory sources for tests.

pub mod archive;
pub mod collate;
pub mod compare;
pub mod cryxml;
pub mod domain;
pub mod domains;
pub mod entity;
pub mod fixtures;
pub mod glob;
pub mod helpers_gameplay;
pub mod helpers_items;
pub mod helpers_ships;
pub mod helpers_world;
// game-item-obtain.json: loot odds per item, derived from the datasets (spec item 3).
pub mod item_obtain;
pub mod js;
pub mod localization;
pub mod output;
pub mod overlays;
pub mod pipeline;
pub mod query;
pub mod records;
pub mod resource_chain;
// D1 reverse links (stockOn, mountedOn, firedBy), added to datasets on disk.
pub mod reverse_links;
pub mod search_index;
pub mod source;
pub mod strings;
// game-tag-index.json: tag -> items and loot records, derived (cross-dataset-derivations D3).
pub mod tag_index;
pub mod xml;

// For the `sc-gamedata` CLI: the fzf picker (`finder.ts`), the UEX client (`sources/uex.ts`)
// and the UEX datasets (`domains/uex-market.ts`); `deflect` (`deflection.ts`) and `engage`
// (`engagement.ts`).
pub mod deflection;
pub mod engagement;
pub mod finder;
pub mod uex;
pub mod uex_market;

// Build comparison (`diff.ts`, `changes.ts`, `notes.ts`): `diff`, `changes`, `notes`.
pub mod changes;
pub mod diff;
pub mod notes;
// Publishing a build to the dumps checkout (`scripts/publish-dump.mjs`, no GitHub release).
pub mod publish;
// SCMCP's item finder (`SCMCP/src/itemfinder.ts`) behind `sc-gamedata mcp-query`.
pub mod mcp;
