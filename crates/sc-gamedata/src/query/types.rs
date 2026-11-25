//! The dataset shapes the query layer reads (port of `query/types.ts`): rows of
//! `game-item-tags` and `game-item-sources`. Unknown fields are ignored; every field the
//! TypeScript marks optional is an `Option` and stays out of the JSON when absent.

use serde::{Deserialize, Serialize};

fn is_false(b: &Option<bool>) -> bool {
    !matches!(b, Some(true))
}

/// One `items` row of `game-item-tags.json`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemTags {
    pub name: String,
    /// Record name without its type prefix, e.g. `ksar_lmg_energy_01`.
    pub class_name: String,
    pub id: String,
    #[serde(default, rename = "type", skip_serializing_if = "Option::is_none")]
    pub ty: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sub_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manufacturer: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub is_template: Option<bool>,
    /// String-table key of the name, as the table spells it (`,P` included).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description_key: Option<String>,
    /// Set when the game has no real name for the item (absent = real); `name` then falls
    /// back to the class name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name_status: Option<crate::localization::NameStatus>,
    /// Tag paths below Global, e.g. `Weapon/FPS/Stocked/LMG`.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Weapons: class names of the magazines or canisters it loads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ammo_items: Option<Vec<String>>,
}

impl ItemTags {
    pub fn template(&self) -> bool {
        self.is_template == Some(true)
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpawnLoot {
    /// Loot table record name (v1 or V3).
    pub table: String,
    #[serde(default)]
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pool_filter: Option<String>,
    /// Secondary choice record; multi-layer refs are expanded at query time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secondary_choices: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_results: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chance: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpawnSlot {
    /// Entity class that spawns: a container, a rack, or the item itself.
    pub entity: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
    pub weight: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loot: Option<SpawnLoot>,
    /// Items the entity's default loadout always holds (racks, displays).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contains: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpawnConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default)]
    pub spawn_tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill_chance: Option<f64>,
    #[serde(default)]
    pub slots: Vec<SpawnSlot>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct LootArea {
    /// Slot preset path below harvestable/slotpresets, e.g. `caves/loot_caves_occupied`.
    pub name: String,
    #[serde(default)]
    pub configs: Vec<SpawnConfig>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ContractAward {
    #[serde(default)]
    pub generator: String,
    pub contract: String,
    pub item: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amount: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weighting: Option<f64>,
}
