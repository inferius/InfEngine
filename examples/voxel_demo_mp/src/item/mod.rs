//! Item registry - the parallel "stackable thing in inventory" table.
//!
//! Mirrors MC's `net/minecraft/world/item` package. The split from
//! `block::BlockType` lets us add non-block items (stick, coal, ingot,
//! food) later without crowding the block table - and lets each block
//! row that DOES double as an inventory stack carry per-item metadata
//! (max stack size, RMB-place target, inventory icon, category) that
//! has no business living on the world-voxel descriptor.
//!
//! Wiring rules:
//! - Every non-air `BlockType` row in `block::BLOCK_TYPES` gets a
//!   matching `ItemType` row here with the same `id` + `name`. The
//!   `places` field decides whether RMB-placing the item produces a
//!   block (basic building blocks) or does nothing (tools, buckets,
//!   future ingredients).
//! - Tool-only ids (pickaxes / swords / shovels / axes / feather /
//!   buckets) keep `places = None` and `max_stack = 1`.
//! - Future "no block form" items (stick, ingot, ...) land here
//!   without needing a block-table slot at all.
//!
//! Existing `block::is_tool_block` / `block::tool_label` /
//! `block::block_required_tier` etc. stay in place. This module is
//! an ADDITIVE layer; the mining / placement / hotbar code keeps
//! reading the block table directly while the new inventory plumbing
//! (chest contents, crafting outputs, ...) consults `item_type`.

pub(crate) mod icons;
pub(crate) mod stack;

#[cfg(test)]
mod item_tests;

use crate::block::{BLOCK_TYPES, TileSpec};

// ---------------------------------------------------------------
// Categories.
// ---------------------------------------------------------------

/// Coarse classification used to drive inventory sort order and the
/// crafting recipe predicate. Mirrors MC's `CreativeModeTab` split at
/// a much coarser grain - one bin per role rather than per material.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ItemCategory {
    /// Core building material - stone, dirt, planks, sandstone, ...
    BuildingBlock,
    /// Placeable but cosmetic / situational - glass, wool, torch,
    /// glowstone, flower, cactus, bed, snow, coal/iron ore visuals.
    Decoration,
    /// Mining tool - pickaxe / shovel / axe (one slot per tier).
    Tool,
    /// Sword. Carries a damage multiplier when held.
    Weapon,
    /// Bucket variants (empty / water / lava). Tool-like single-stack
    /// items that interact with liquids on RMB.
    Bucket,
    /// Non-block crafting ingredient - feather today; stick, coal
    /// item, ingot land here in C.2 / C.3.
    Ingredient,
    /// Edible item - meat today; bread / apple in a future commit.
    Food,
}

// ---------------------------------------------------------------
// ItemType.
// ---------------------------------------------------------------

/// One stackable item kind. Built to OUTLIVE any block form: if the
/// player can pick up, carry, or drop this thing, it has an
/// `ItemType` row.
#[derive(Debug, Clone)]
pub(crate) struct ItemType {
    pub(crate) id: u16,
    pub(crate) name: &'static str,
    /// User-visible label shown in inventory tooltips. `None` falls
    /// back to a title-cased `name`.
    pub(crate) display: Option<&'static str>,
    /// `Some(block_id)` if RMB-placing this item produces a block.
    /// `None` for tool / ingredient items - and for liquid source
    /// ids that exist as blocks but only enter the world via a
    /// bucket.
    pub(crate) places: Option<u16>,
    /// Max stack size. Building blocks / decorations / ingredients
    /// stack to 64. Tools / weapons / buckets stack to 1.
    pub(crate) max_stack: u8,
    /// Explicit atlas tile for the inventory icon. `None` falls back
    /// to `block::block_type(id).top` when the item has a block form,
    /// and to the tinted swatch otherwise.
    pub(crate) icon: Option<icons::IconSpec>,
    /// Item category - drives sort order in the inventory grid + the
    /// crafting recipe predicate. Per-category behaviour (damage,
    /// mining tier) still routes through the existing `block::*`
    /// helpers; the category is only a coarse bucket here.
    pub(crate) category: ItemCategory,
}

// ---------------------------------------------------------------
// Item table.
// ---------------------------------------------------------------

/// All known item kinds. IDs 1..=40 currently mirror `BLOCK_TYPES`;
/// future entries past 40 will be ingredient / food / non-block
/// items with `places = None`.
pub(crate) const ITEM_TYPES: &[ItemType] = &[
    // ---- Building blocks ----
    ItemType {
        id: 1, name: "stone", display: None,
        places: Some(1), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 2, name: "dirt", display: None,
        places: Some(2), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 3, name: "grass", display: None,
        places: Some(3), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 4, name: "sand", display: None,
        places: Some(4), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 5, name: "wood", display: None,
        places: Some(5), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 6, name: "leaves", display: None,
        places: Some(6), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 7, name: "planks", display: None,
        places: Some(7), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 8, name: "cobble", display: Some("Cobblestone"),
        places: Some(8), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    // ---- Decorations / cosmetic placeables ----
    ItemType {
        id: 9, name: "glass", display: None,
        places: Some(9), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 10, name: "torch", display: None,
        places: Some(10), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 11, name: "wool", display: None,
        places: Some(11), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 12, name: "meat", display: None,
        places: Some(12), max_stack: 64, icon: None,
        category: ItemCategory::Food,
    },
    ItemType {
        id: 13, name: "coal", display: Some("Coal Ore"),
        places: Some(13), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 14, name: "iron", display: Some("Iron Ore"),
        places: Some(14), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 15, name: "snow", display: None,
        places: Some(15), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    // ---- Pickaxes (tool tier 1..3) ----
    ItemType {
        id: 16, name: "pickaxe_wood", display: Some("Wood Pickaxe"),
        places: None, max_stack: 1, icon: None,
        category: ItemCategory::Tool,
    },
    ItemType {
        id: 17, name: "pickaxe_stone", display: Some("Stone Pickaxe"),
        places: None, max_stack: 1, icon: None,
        category: ItemCategory::Tool,
    },
    ItemType {
        id: 18, name: "pickaxe_iron", display: Some("Iron Pickaxe"),
        places: None, max_stack: 1, icon: None,
        category: ItemCategory::Tool,
    },
    // ---- Feather (non-block ingredient) ----
    ItemType {
        id: 19, name: "feather", display: None,
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Ingredient,
    },
    // ---- Flowers ----
    ItemType {
        id: 20, name: "flower", display: Some("Red Flower"),
        places: Some(20), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 21, name: "flower_yellow", display: Some("Yellow Flower"),
        places: Some(21), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 22, name: "flower_blue", display: Some("Blue Flower"),
        places: Some(22), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    // ---- Swords ----
    ItemType {
        id: 23, name: "sword_wood", display: Some("Wood Sword"),
        places: None, max_stack: 1, icon: None,
        category: ItemCategory::Weapon,
    },
    ItemType {
        id: 24, name: "sword_stone", display: Some("Stone Sword"),
        places: None, max_stack: 1, icon: None,
        category: ItemCategory::Weapon,
    },
    ItemType {
        id: 25, name: "sword_iron", display: Some("Iron Sword"),
        places: None, max_stack: 1, icon: None,
        category: ItemCategory::Weapon,
    },
    // ---- More building blocks (out-of-order ids, blame history) ----
    ItemType {
        id: 26, name: "sandstone", display: None,
        places: Some(26), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 27, name: "cactus", display: None,
        places: Some(27), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 28, name: "glowstone", display: None,
        places: Some(28), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    // ---- Shovels / axes ----
    ItemType {
        id: 29, name: "shovel_wood", display: Some("Wood Shovel"),
        places: None, max_stack: 1, icon: None,
        category: ItemCategory::Tool,
    },
    ItemType {
        id: 30, name: "shovel_stone", display: Some("Stone Shovel"),
        places: None, max_stack: 1, icon: None,
        category: ItemCategory::Tool,
    },
    ItemType {
        id: 31, name: "shovel_iron", display: Some("Iron Shovel"),
        places: None, max_stack: 1, icon: None,
        category: ItemCategory::Tool,
    },
    ItemType {
        id: 32, name: "axe_wood", display: Some("Wood Axe"),
        places: None, max_stack: 1, icon: None,
        category: ItemCategory::Tool,
    },
    ItemType {
        id: 33, name: "axe_stone", display: Some("Stone Axe"),
        places: None, max_stack: 1, icon: None,
        category: ItemCategory::Tool,
    },
    ItemType {
        id: 34, name: "axe_iron", display: Some("Iron Axe"),
        places: None, max_stack: 1, icon: None,
        category: ItemCategory::Tool,
    },
    // ---- Bed ----
    ItemType {
        id: 35, name: "bed", display: None,
        places: Some(35), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    // ---- Liquid source ids ----
    // Present so the BlockType <-> ItemType mirror stays 1:1, but
    // they aren't user-placeable from inventory: water / lava enter
    // the world via the matching bucket. `places = None` keeps the
    // RMB-place path from picking them up by accident.
    ItemType {
        id: 36, name: "water", display: None,
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 37, name: "lava", display: None,
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    // ---- Buckets ----
    // Tool-class single-stack items. RMB on a liquid scoops, RMB on
    // a placement target empties the bucket into a source block.
    ItemType {
        id: 38, name: "bucket", display: Some("Bucket"),
        places: None, max_stack: 1, icon: None,
        category: ItemCategory::Bucket,
    },
    ItemType {
        id: 39, name: "water_bucket", display: Some("Water Bucket"),
        places: None, max_stack: 1, icon: None,
        category: ItemCategory::Bucket,
    },
    ItemType {
        id: 40, name: "lava_bucket", display: Some("Lava Bucket"),
        places: None, max_stack: 1, icon: None,
        category: ItemCategory::Bucket,
    },
    // ---- Chest (block + container) ----
    // Stackable building block. RMB-placing the item from a slot
    // emits the chest block; RMB on an existing chest block opens
    // the chest UI - that path lives in the world interact chain,
    // not here.
    ItemType {
        id: 41, name: "chest", display: Some("Chest"),
        places: Some(41), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    // ---- Furnace (block + 3-slot container) ----
    // Stackable building block. RMB-placing emits the furnace
    // block; RMB on an existing furnace opens the furnace UI via
    // the world-interact chain (not here). The cook step runs on
    // `Voxel::furnaces` + the `block::furnace::furnace_tick` path.
    ItemType {
        id: 42, name: "furnace", display: Some("Furnace"),
        places: Some(42), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    // ---- Iron Ingot (smelt output, no block form) ----
    // Item-only: places = None. Output of the iron-ore smelt
    // recipe. Stacks to 64 like other ingredients. Icon falls
    // back to the tinted swatch until an explicit IconSpec lands.
    ItemType {
        id: 43, name: "iron_ingot", display: Some("Iron Ingot"),
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Ingredient,
    },
    // ---- Phase F: nether + portal blocks ----
    // Netherrack drops from nether terrain mining. Building
    // block category so it shows up in the BuildingBlock filter.
    ItemType {
        id: 44, name: "netherrack", display: None,
        places: Some(44), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    // Bedrock - placeable in creative for debugging only. The
    // place path will still gate on permission; for now the
    // item exists so a hostile / admin-spawned bedrock survives
    // round-trips through the inventory.
    ItemType {
        id: 45, name: "bedrock", display: None,
        places: Some(45), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    // Obsidian - block + portal frame material.
    ItemType {
        id: 46, name: "obsidian", display: None,
        places: Some(46), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    // Portal block: cosmetic + cross-dim trigger. Like the
    // liquid source ids, the item exists for save symmetry but
    // is NOT user-placeable (places = None). Players ignite a
    // portal with flint+steel; the place path can't conjure it.
    ItemType {
        id: 47, name: "portal", display: None,
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    // Flint and steel. Single-stack tool; RMB ignites a portal
    // frame. Stacks to 1 like buckets.
    ItemType {
        id: 48, name: "flint_and_steel", display: Some("Flint and Steel"),
        places: None, max_stack: 1, icon: None,
        category: ItemCategory::Tool,
    },
    // Fire (Q.4b). Block exists in the world but the player
    // never holds it as an item stack - it spawns via flint+steel
    // ignition or lava-burn. `places = None` keeps the place path
    // honest if a creative-/give chat command ever shoves a fire
    // stack into inventory.
    ItemType {
        id: 49, name: "fire", display: None,
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
];

// ---------------------------------------------------------------
// Lookup helpers.
// ---------------------------------------------------------------

/// Resolve an item id to its definition. Returns `None` for unknown
/// ids (including `0` / air).
pub(crate) fn item_type(id: u16) -> Option<&'static ItemType> {
    ITEM_TYPES.iter().find(|it| it.id == id)
}

/// Max stack size for an item id. Falls back to `64` for unknown ids
/// so legacy callers that haven't been migrated yet keep behaving
/// like the block table assumed.
pub(crate) fn item_max_stack(id: u16) -> u8 {
    item_type(id).map(|it| it.max_stack).unwrap_or(64)
}

/// `Some(block_id)` if RMB-placing the item produces a block.
/// `None` for tools / buckets / ingredients / liquid source ids.
pub(crate) fn item_places_block(id: u16) -> Option<u16> {
    item_type(id).and_then(|it| it.places)
}

/// User-visible display name for the item. Uses the explicit
/// `display` field when set, otherwise title-cases the snake-case
/// `name` ("pickaxe_wood" -> "Pickaxe Wood"). Returned owned
/// because the title-case happens on the fly.
pub(crate) fn item_display_name(id: u16) -> String {
    match item_type(id) {
        Some(it) => match it.display {
            Some(d) => d.to_string(),
            None => title_case_snake(it.name),
        },
        None => format!("item_{id}"),
    }
}

/// Item category (defaults to `Decoration` for unknown ids - the
/// least surprising bucket for a stray slot).
pub(crate) fn item_category(id: u16) -> ItemCategory {
    item_type(id).map(|it| it.category).unwrap_or(ItemCategory::Decoration)
}

/// Title-case a snake-case identifier. `"pickaxe_wood"` ->
/// `"Pickaxe Wood"`, `"stone"` -> `"Stone"`. ASCII-only; the item
/// names are all ASCII so we don't need a Unicode-aware caser.
fn title_case_snake(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut at_word_start = true;
    for c in s.chars() {
        if c == '_' {
            out.push(' ');
            at_word_start = true;
        } else if at_word_start {
            for u in c.to_uppercase() {
                out.push(u);
            }
            at_word_start = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// Internal: locate the matching `BlockType` row for an item id so
/// the icon fallback can resolve a tile spec. Returns `None` when
/// the item has no block form.
pub(crate) fn block_top_tile_for_item(id: u16) -> Option<&'static TileSpec> {
    BLOCK_TYPES.iter().find(|b| b.id == id).map(|b| &b.top)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_case_snake_basic() {
        assert_eq!(title_case_snake("stone"), "Stone");
        assert_eq!(title_case_snake("pickaxe_wood"), "Pickaxe Wood");
        assert_eq!(title_case_snake("flower_blue"), "Flower Blue");
    }

    #[test]
    fn item_display_falls_back_to_title_case() {
        // `dirt` has no explicit display - falls back.
        assert_eq!(item_display_name(2), "Dirt");
        // `pickaxe_wood` has an explicit display string.
        assert_eq!(item_display_name(16), "Wood Pickaxe");
    }
}
