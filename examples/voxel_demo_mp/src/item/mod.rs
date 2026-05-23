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
    // ---- W11.3: emitter + cosmetic blocks (ids 50..=67). All
    // are placeable building / decoration items so item_places_block
    // returns Some(id) - they show up in the Creative inventory tab
    // automatically.
    ItemType {
        id: 50, name: "redstone_torch", display: Some("Redstone Torch"),
        places: Some(50), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 51, name: "redstone_wall_torch", display: Some("Redstone Wall Torch"),
        places: Some(51), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 52, name: "soul_torch", display: Some("Soul Torch"),
        places: Some(52), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 53, name: "soul_wall_torch", display: Some("Soul Wall Torch"),
        places: Some(53), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 54, name: "sea_lantern", display: Some("Sea Lantern"),
        places: Some(54), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 55, name: "jack_o_lantern", display: Some("Jack o'Lantern"),
        places: Some(55), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 56, name: "soul_fire", display: Some("Soul Fire"),
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 57, name: "end_rod", display: Some("End Rod"),
        places: Some(57), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 58, name: "magma_block", display: Some("Magma Block"),
        places: Some(58), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 59, name: "shroomlight", display: Some("Shroomlight"),
        places: Some(59), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 60, name: "lantern", display: Some("Lantern"),
        places: Some(60), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 61, name: "soul_lantern", display: Some("Soul Lantern"),
        places: Some(61), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 62, name: "crying_obsidian", display: Some("Crying Obsidian"),
        places: Some(62), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 63, name: "respawn_anchor", display: Some("Respawn Anchor"),
        places: Some(63), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 64, name: "ender_chest", display: Some("Ender Chest"),
        places: Some(64), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 65, name: "lit_redstone_lamp", display: Some("Redstone Lamp"),
        places: Some(65), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 66, name: "lit_redstone_ore", display: Some("Redstone Ore"),
        places: Some(66), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 67, name: "light_block", display: Some("Light Block"),
        places: Some(67), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    // ---- W12.2 - Slab / fence / door / stairs / pane / wall ----
    ItemType {
        id: 68, name: "oak_slab", display: Some("Oak Slab"),
        places: Some(68), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 69, name: "stone_slab", display: Some("Stone Slab"),
        places: Some(69), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 70, name: "cobblestone_slab", display: Some("Cobblestone Slab"),
        places: Some(70), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 71, name: "oak_slab_top", display: Some("Oak Slab (Top)"),
        places: Some(71), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 72, name: "oak_stairs", display: Some("Oak Stairs"),
        places: Some(72), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 73, name: "stone_stairs", display: Some("Stone Stairs"),
        places: Some(73), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 74, name: "cobblestone_stairs", display: Some("Cobblestone Stairs"),
        places: Some(74), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 75, name: "oak_fence", display: Some("Oak Fence"),
        places: Some(75), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 76, name: "nether_brick_fence", display: Some("Nether Brick Fence"),
        places: Some(76), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 77, name: "oak_door", display: Some("Oak Door"),
        places: Some(77), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 78, name: "oak_door_upper", display: Some("Oak Door (Top)"),
        places: Some(78), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 79, name: "iron_door", display: Some("Iron Door"),
        places: Some(79), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 80, name: "glass_pane", display: Some("Glass Pane"),
        places: Some(80), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 81, name: "iron_bars", display: Some("Iron Bars"),
        places: Some(81), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 82, name: "cobblestone_wall", display: Some("Cobblestone Wall"),
        places: Some(82), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    // ---- W35.3: Stone variants ----
    ItemType {
        id: 83, name: "granite", display: None,
        places: Some(83), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 84, name: "diorite", display: None,
        places: Some(84), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 85, name: "andesite", display: None,
        places: Some(85), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 86, name: "bricks", display: None,
        places: Some(86), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 87, name: "smooth_stone", display: Some("Smooth Stone"),
        places: Some(87), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    // ---- W35.3: Birch wood ----
    ItemType {
        id: 88, name: "birch_log", display: Some("Birch Log"),
        places: Some(88), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 89, name: "birch_planks", display: Some("Birch Planks"),
        places: Some(89), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 90, name: "birch_leaves", display: Some("Birch Leaves"),
        places: Some(90), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    // ---- W35.3: Wool colour variants ----
    ItemType {
        id: 91, name: "blue_wool", display: Some("Blue Wool"),
        places: Some(91), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 92, name: "red_wool", display: Some("Red Wool"),
        places: Some(92), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 93, name: "yellow_wool", display: Some("Yellow Wool"),
        places: Some(93), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 94, name: "green_wool", display: Some("Green Wool"),
        places: Some(94), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    // ---- W35.3: More building blocks ----
    ItemType {
        id: 95, name: "tuff", display: None,
        places: Some(95), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 96, name: "deepslate", display: None,
        places: Some(96), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 97, name: "calcite", display: None,
        places: Some(97), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    // ---- W37.2: Wool colour variants (remaining 11) ----
    ItemType {
        id: 98, name: "orange_wool", display: Some("Orange Wool"),
        places: Some(98), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 99, name: "magenta_wool", display: Some("Magenta Wool"),
        places: Some(99), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 100, name: "light_blue_wool", display: Some("Light Blue Wool"),
        places: Some(100), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 101, name: "lime_wool", display: Some("Lime Wool"),
        places: Some(101), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 102, name: "pink_wool", display: Some("Pink Wool"),
        places: Some(102), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 103, name: "gray_wool", display: Some("Gray Wool"),
        places: Some(103), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 104, name: "light_gray_wool", display: Some("Light Gray Wool"),
        places: Some(104), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 105, name: "cyan_wool", display: Some("Cyan Wool"),
        places: Some(105), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 106, name: "purple_wool", display: Some("Purple Wool"),
        places: Some(106), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 107, name: "brown_wool", display: Some("Brown Wool"),
        places: Some(107), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 108, name: "black_wool", display: Some("Black Wool"),
        places: Some(108), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    // ---- W37.2: Stone family extras ----
    ItemType {
        id: 109, name: "mossy_cobblestone", display: Some("Mossy Cobblestone"),
        places: Some(109), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 110, name: "stone_bricks", display: Some("Stone Bricks"),
        places: Some(110), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 111, name: "mossy_stone_bricks", display: Some("Mossy Stone Bricks"),
        places: Some(111), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 112, name: "cracked_stone_bricks", display: Some("Cracked Stone Bricks"),
        places: Some(112), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 113, name: "chiseled_stone_bricks", display: Some("Chiseled Stone Bricks"),
        places: Some(113), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 114, name: "polished_granite", display: Some("Polished Granite"),
        places: Some(114), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 115, name: "polished_diorite", display: Some("Polished Diorite"),
        places: Some(115), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 116, name: "polished_andesite", display: Some("Polished Andesite"),
        places: Some(116), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    // ---- W37.2: Ores ----
    ItemType {
        id: 117, name: "gold_ore", display: Some("Gold Ore"),
        places: Some(117), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 118, name: "diamond_ore", display: Some("Diamond Ore"),
        places: Some(118), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 119, name: "redstone_ore", display: Some("Redstone Ore"),
        places: Some(119), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 120, name: "lapis_ore", display: Some("Lapis Ore"),
        places: Some(120), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 121, name: "emerald_ore", display: Some("Emerald Ore"),
        places: Some(121), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 122, name: "copper_ore", display: Some("Copper Ore"),
        places: Some(122), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    // ---- W37.2: Deepslate ore variants ----
    ItemType {
        id: 123, name: "deepslate_coal_ore", display: Some("Deepslate Coal Ore"),
        places: Some(123), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 124, name: "deepslate_iron_ore", display: Some("Deepslate Iron Ore"),
        places: Some(124), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 125, name: "deepslate_diamond_ore", display: Some("Deepslate Diamond Ore"),
        places: Some(125), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 126, name: "deepslate_gold_ore", display: Some("Deepslate Gold Ore"),
        places: Some(126), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    // ---- W37.2: Wood variants - acacia ----
    ItemType {
        id: 127, name: "acacia_log", display: Some("Acacia Log"),
        places: Some(127), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 128, name: "acacia_planks", display: Some("Acacia Planks"),
        places: Some(128), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 129, name: "acacia_leaves", display: Some("Acacia Leaves"),
        places: Some(129), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    // ---- W37.2: Spruce ----
    ItemType {
        id: 130, name: "spruce_log", display: Some("Spruce Log"),
        places: Some(130), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 131, name: "spruce_planks", display: Some("Spruce Planks"),
        places: Some(131), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 132, name: "spruce_leaves", display: Some("Spruce Leaves"),
        places: Some(132), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    // ---- W37.2: Jungle ----
    ItemType {
        id: 133, name: "jungle_log", display: Some("Jungle Log"),
        places: Some(133), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 134, name: "jungle_planks", display: Some("Jungle Planks"),
        places: Some(134), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 135, name: "jungle_leaves", display: Some("Jungle Leaves"),
        places: Some(135), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    // ---- W37.2: Dark oak ----
    ItemType {
        id: 136, name: "dark_oak_log", display: Some("Dark Oak Log"),
        places: Some(136), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 137, name: "dark_oak_planks", display: Some("Dark Oak Planks"),
        places: Some(137), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 138, name: "dark_oak_leaves", display: Some("Dark Oak Leaves"),
        places: Some(138), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    // ---- W37.2: Flowers ----
    ItemType {
        id: 139, name: "allium", display: None,
        places: Some(139), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 140, name: "azure_bluet", display: Some("Azure Bluet"),
        places: Some(140), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 141, name: "red_tulip", display: Some("Red Tulip"),
        places: Some(141), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 142, name: "orange_tulip", display: Some("Orange Tulip"),
        places: Some(142), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 143, name: "white_tulip", display: Some("White Tulip"),
        places: Some(143), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 144, name: "pink_tulip", display: Some("Pink Tulip"),
        places: Some(144), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 145, name: "oxeye_daisy", display: Some("Oxeye Daisy"),
        places: Some(145), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 146, name: "cornflower", display: None,
        places: Some(146), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 147, name: "lily_of_the_valley", display: Some("Lily of the Valley"),
        places: Some(147), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    // ---- W37.2: Misc terrain ----
    ItemType {
        id: 148, name: "end_stone", display: Some("End Stone"),
        places: Some(148), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 149, name: "soul_sand", display: Some("Soul Sand"),
        places: Some(149), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 150, name: "soul_soil", display: Some("Soul Soil"),
        places: Some(150), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 151, name: "bone_block", display: Some("Bone Block"),
        places: Some(151), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 152, name: "mycelium", display: None,
        places: Some(152), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 153, name: "podzol", display: None,
        places: Some(153), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    // ---- W37.2: Gold + diamond tools ----
    ItemType {
        id: 154, name: "pickaxe_gold", display: Some("Golden Pickaxe"),
        places: None, max_stack: 1, icon: None,
        category: ItemCategory::Tool,
    },
    ItemType {
        id: 155, name: "pickaxe_diamond", display: Some("Diamond Pickaxe"),
        places: None, max_stack: 1, icon: None,
        category: ItemCategory::Tool,
    },
    ItemType {
        id: 156, name: "sword_gold", display: Some("Golden Sword"),
        places: None, max_stack: 1, icon: None,
        category: ItemCategory::Weapon,
    },
    ItemType {
        id: 157, name: "sword_diamond", display: Some("Diamond Sword"),
        places: None, max_stack: 1, icon: None,
        category: ItemCategory::Weapon,
    },
    ItemType {
        id: 158, name: "shovel_gold", display: Some("Golden Shovel"),
        places: None, max_stack: 1, icon: None,
        category: ItemCategory::Tool,
    },
    ItemType {
        id: 159, name: "shovel_diamond", display: Some("Diamond Shovel"),
        places: None, max_stack: 1, icon: None,
        category: ItemCategory::Tool,
    },
    ItemType {
        id: 160, name: "axe_gold", display: Some("Golden Axe"),
        places: None, max_stack: 1, icon: None,
        category: ItemCategory::Tool,
    },
    ItemType {
        id: 161, name: "axe_diamond", display: Some("Diamond Axe"),
        places: None, max_stack: 1, icon: None,
        category: ItemCategory::Tool,
    },
    // ---- W37.2: Food items (item-only, places = None) ----
    ItemType {
        id: 162, name: "apple", display: None,
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Food,
    },
    ItemType {
        id: 163, name: "bread", display: None,
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Food,
    },
    ItemType {
        id: 164, name: "cooked_beef", display: Some("Cooked Beef (Steak)"),
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Food,
    },
    ItemType {
        id: 165, name: "raw_chicken", display: Some("Raw Chicken"),
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Food,
    },
    ItemType {
        id: 166, name: "cooked_chicken", display: Some("Cooked Chicken"),
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Food,
    },
    ItemType {
        id: 167, name: "cookie", display: None,
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Food,
    },
    ItemType {
        id: 168, name: "melon_slice", display: Some("Melon Slice"),
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Food,
    },
    // ---- W40.2: Stained glass (ids 169..=184) ----
    ItemType {
        id: 169, name: "white_stained_glass", display: Some("White Stained Glass"),
        places: Some(169), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 170, name: "orange_stained_glass", display: Some("Orange Stained Glass"),
        places: Some(170), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 171, name: "magenta_stained_glass", display: Some("Magenta Stained Glass"),
        places: Some(171), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 172, name: "light_blue_stained_glass", display: Some("Light Blue Stained Glass"),
        places: Some(172), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 173, name: "yellow_stained_glass", display: Some("Yellow Stained Glass"),
        places: Some(173), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 174, name: "lime_stained_glass", display: Some("Lime Stained Glass"),
        places: Some(174), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 175, name: "pink_stained_glass", display: Some("Pink Stained Glass"),
        places: Some(175), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 176, name: "gray_stained_glass", display: Some("Gray Stained Glass"),
        places: Some(176), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 177, name: "light_gray_stained_glass", display: Some("Light Gray Stained Glass"),
        places: Some(177), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 178, name: "cyan_stained_glass", display: Some("Cyan Stained Glass"),
        places: Some(178), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 179, name: "purple_stained_glass", display: Some("Purple Stained Glass"),
        places: Some(179), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 180, name: "blue_stained_glass", display: Some("Blue Stained Glass"),
        places: Some(180), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 181, name: "brown_stained_glass", display: Some("Brown Stained Glass"),
        places: Some(181), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 182, name: "green_stained_glass", display: Some("Green Stained Glass"),
        places: Some(182), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 183, name: "red_stained_glass", display: Some("Red Stained Glass"),
        places: Some(183), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 184, name: "black_stained_glass", display: Some("Black Stained Glass"),
        places: Some(184), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    // ---- W40.2: Concrete (ids 185..=200) ----
    ItemType {
        id: 185, name: "white_concrete", display: Some("White Concrete"),
        places: Some(185), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 186, name: "orange_concrete", display: Some("Orange Concrete"),
        places: Some(186), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 187, name: "magenta_concrete", display: Some("Magenta Concrete"),
        places: Some(187), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 188, name: "light_blue_concrete", display: Some("Light Blue Concrete"),
        places: Some(188), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 189, name: "yellow_concrete", display: Some("Yellow Concrete"),
        places: Some(189), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 190, name: "lime_concrete", display: Some("Lime Concrete"),
        places: Some(190), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 191, name: "pink_concrete", display: Some("Pink Concrete"),
        places: Some(191), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 192, name: "gray_concrete", display: Some("Gray Concrete"),
        places: Some(192), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 193, name: "light_gray_concrete", display: Some("Light Gray Concrete"),
        places: Some(193), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 194, name: "cyan_concrete", display: Some("Cyan Concrete"),
        places: Some(194), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 195, name: "purple_concrete", display: Some("Purple Concrete"),
        places: Some(195), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 196, name: "blue_concrete", display: Some("Blue Concrete"),
        places: Some(196), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 197, name: "brown_concrete", display: Some("Brown Concrete"),
        places: Some(197), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 198, name: "green_concrete", display: Some("Green Concrete"),
        places: Some(198), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 199, name: "red_concrete", display: Some("Red Concrete"),
        places: Some(199), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 200, name: "black_concrete", display: Some("Black Concrete"),
        places: Some(200), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    // ---- W40.2: Concrete powder (ids 201..=216) ----
    ItemType {
        id: 201, name: "white_concrete_powder", display: Some("White Concrete Powder"),
        places: Some(201), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 202, name: "orange_concrete_powder", display: Some("Orange Concrete Powder"),
        places: Some(202), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 203, name: "magenta_concrete_powder", display: Some("Magenta Concrete Powder"),
        places: Some(203), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 204, name: "light_blue_concrete_powder", display: Some("Light Blue Concrete Powder"),
        places: Some(204), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 205, name: "yellow_concrete_powder", display: Some("Yellow Concrete Powder"),
        places: Some(205), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 206, name: "lime_concrete_powder", display: Some("Lime Concrete Powder"),
        places: Some(206), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 207, name: "pink_concrete_powder", display: Some("Pink Concrete Powder"),
        places: Some(207), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 208, name: "gray_concrete_powder", display: Some("Gray Concrete Powder"),
        places: Some(208), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 209, name: "light_gray_concrete_powder", display: Some("Light Gray Concrete Powder"),
        places: Some(209), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 210, name: "cyan_concrete_powder", display: Some("Cyan Concrete Powder"),
        places: Some(210), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 211, name: "purple_concrete_powder", display: Some("Purple Concrete Powder"),
        places: Some(211), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 212, name: "blue_concrete_powder", display: Some("Blue Concrete Powder"),
        places: Some(212), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 213, name: "brown_concrete_powder", display: Some("Brown Concrete Powder"),
        places: Some(213), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 214, name: "green_concrete_powder", display: Some("Green Concrete Powder"),
        places: Some(214), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 215, name: "red_concrete_powder", display: Some("Red Concrete Powder"),
        places: Some(215), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 216, name: "black_concrete_powder", display: Some("Black Concrete Powder"),
        places: Some(216), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    // ---- W40.2: Carpet (ids 217..=232) ----
    ItemType {
        id: 217, name: "white_carpet", display: Some("White Carpet"),
        places: Some(217), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 218, name: "orange_carpet", display: Some("Orange Carpet"),
        places: Some(218), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 219, name: "magenta_carpet", display: Some("Magenta Carpet"),
        places: Some(219), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 220, name: "light_blue_carpet", display: Some("Light Blue Carpet"),
        places: Some(220), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 221, name: "yellow_carpet", display: Some("Yellow Carpet"),
        places: Some(221), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 222, name: "lime_carpet", display: Some("Lime Carpet"),
        places: Some(222), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 223, name: "pink_carpet", display: Some("Pink Carpet"),
        places: Some(223), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 224, name: "gray_carpet", display: Some("Gray Carpet"),
        places: Some(224), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 225, name: "light_gray_carpet", display: Some("Light Gray Carpet"),
        places: Some(225), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 226, name: "cyan_carpet", display: Some("Cyan Carpet"),
        places: Some(226), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 227, name: "purple_carpet", display: Some("Purple Carpet"),
        places: Some(227), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 228, name: "blue_carpet", display: Some("Blue Carpet"),
        places: Some(228), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 229, name: "brown_carpet", display: Some("Brown Carpet"),
        places: Some(229), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 230, name: "green_carpet", display: Some("Green Carpet"),
        places: Some(230), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 231, name: "red_carpet", display: Some("Red Carpet"),
        places: Some(231), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 232, name: "black_carpet", display: Some("Black Carpet"),
        places: Some(232), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    // ---- W40.2: Mangrove wood (ids 233..=236) ----
    ItemType {
        id: 233, name: "mangrove_log", display: Some("Mangrove Log"),
        places: Some(233), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 234, name: "mangrove_planks", display: Some("Mangrove Planks"),
        places: Some(234), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 235, name: "mangrove_leaves", display: Some("Mangrove Leaves"),
        places: Some(235), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 236, name: "mangrove_roots", display: Some("Mangrove Roots"),
        places: Some(236), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    // ---- W40.2: Cherry wood (ids 237..=239) ----
    ItemType {
        id: 237, name: "cherry_log", display: Some("Cherry Log"),
        places: Some(237), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 238, name: "cherry_planks", display: Some("Cherry Planks"),
        places: Some(238), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 239, name: "cherry_leaves", display: Some("Cherry Leaves"),
        places: Some(239), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    // ---- W40.2: Crimson wood (ids 240..=242) ----
    ItemType {
        id: 240, name: "crimson_stem", display: Some("Crimson Stem"),
        places: Some(240), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 241, name: "crimson_planks", display: Some("Crimson Planks"),
        places: Some(241), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 242, name: "crimson_hyphae", display: Some("Crimson Hyphae"),
        places: Some(242), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    // ---- W40.2: Warped wood (ids 243..=245) ----
    ItemType {
        id: 243, name: "warped_stem", display: Some("Warped Stem"),
        places: Some(243), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 244, name: "warped_planks", display: Some("Warped Planks"),
        places: Some(244), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 245, name: "warped_hyphae", display: Some("Warped Hyphae"),
        places: Some(245), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    // ---- W40.2: Deepslate ores (ids 246..=249) ----
    ItemType {
        id: 246, name: "deepslate_redstone_ore", display: Some("Deepslate Redstone Ore"),
        places: Some(246), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 247, name: "deepslate_lapis_ore", display: Some("Deepslate Lapis Ore"),
        places: Some(247), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 248, name: "deepslate_emerald_ore", display: Some("Deepslate Emerald Ore"),
        places: Some(248), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 249, name: "deepslate_copper_ore", display: Some("Deepslate Copper Ore"),
        places: Some(249), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    // ---- W40.2: Nether ores (ids 250..=251) ----
    ItemType {
        id: 250, name: "nether_quartz_ore", display: Some("Nether Quartz Ore"),
        places: Some(250), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 251, name: "nether_gold_ore", display: Some("Nether Gold Ore"),
        places: Some(251), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    // ---- W40.2: Ancient debris (id 252) ----
    ItemType {
        id: 252, name: "ancient_debris", display: Some("Ancient Debris"),
        places: Some(252), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    // ---- W40.2: Nether building blocks (ids 253..=258) ----
    ItemType {
        id: 253, name: "nether_bricks", display: Some("Nether Bricks"),
        places: Some(253), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 254, name: "red_nether_bricks", display: Some("Red Nether Bricks"),
        places: Some(254), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 255, name: "blackstone", display: None,
        places: Some(255), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 256, name: "polished_blackstone", display: Some("Polished Blackstone"),
        places: Some(256), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 257, name: "basalt", display: None,
        places: Some(257), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 258, name: "smooth_basalt", display: Some("Smooth Basalt"),
        places: Some(258), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    // ---- W40.2: End blocks (ids 259..=262) ----
    ItemType {
        id: 259, name: "end_stone_bricks", display: Some("End Stone Bricks"),
        places: Some(259), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 260, name: "purpur_block", display: Some("Purpur Block"),
        places: Some(260), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 261, name: "purpur_pillar", display: Some("Purpur Pillar"),
        places: Some(261), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 262, name: "dragon_egg", display: Some("Dragon Egg"),
        places: Some(262), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    // ---- W40.2: Prismarine family (ids 263..=265) ----
    ItemType {
        id: 263, name: "prismarine", display: None,
        places: Some(263), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 264, name: "prismarine_bricks", display: Some("Prismarine Bricks"),
        places: Some(264), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 265, name: "dark_prismarine", display: Some("Dark Prismarine"),
        places: Some(265), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    // ---- W40.2: Copper blocks (ids 266..=270) ----
    ItemType {
        id: 266, name: "copper_block", display: Some("Copper Block"),
        places: Some(266), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 267, name: "exposed_copper", display: Some("Exposed Copper"),
        places: Some(267), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 268, name: "weathered_copper", display: Some("Weathered Copper"),
        places: Some(268), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 269, name: "oxidized_copper", display: Some("Oxidized Copper"),
        places: Some(269), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 270, name: "cut_copper", display: Some("Cut Copper"),
        places: Some(270), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    // ---- W40.2: Amethyst (ids 271..=273) ----
    ItemType {
        id: 271, name: "amethyst_block", display: Some("Amethyst Block"),
        places: Some(271), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 272, name: "budding_amethyst", display: Some("Budding Amethyst"),
        places: Some(272), max_stack: 64, icon: None,
        category: ItemCategory::BuildingBlock,
    },
    ItemType {
        id: 273, name: "amethyst_cluster", display: Some("Amethyst Cluster"),
        places: Some(273), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    // ---- W40.2: Netherite tools (ids 274..=278) ----
    ItemType {
        id: 274, name: "pickaxe_netherite", display: Some("Netherite Pickaxe"),
        places: None, max_stack: 1, icon: None,
        category: ItemCategory::Tool,
    },
    ItemType {
        id: 275, name: "sword_netherite", display: Some("Netherite Sword"),
        places: None, max_stack: 1, icon: None,
        category: ItemCategory::Weapon,
    },
    ItemType {
        id: 276, name: "shovel_netherite", display: Some("Netherite Shovel"),
        places: None, max_stack: 1, icon: None,
        category: ItemCategory::Tool,
    },
    ItemType {
        id: 277, name: "axe_netherite", display: Some("Netherite Axe"),
        places: None, max_stack: 1, icon: None,
        category: ItemCategory::Tool,
    },
    ItemType {
        id: 278, name: "hoe_netherite", display: Some("Netherite Hoe"),
        places: None, max_stack: 1, icon: None,
        category: ItemCategory::Tool,
    },
    // ---- W40.2: Netherite ingot + scrap (ids 279..=280) ----
    ItemType {
        id: 279, name: "netherite_ingot", display: Some("Netherite Ingot"),
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Ingredient,
    },
    ItemType {
        id: 280, name: "netherite_scrap", display: Some("Netherite Scrap"),
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Ingredient,
    },
    // ---- W40.2: More food items (ids 281..=292) ----
    ItemType {
        id: 281, name: "pumpkin_pie", display: Some("Pumpkin Pie"),
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Food,
    },
    ItemType {
        id: 282, name: "golden_apple", display: Some("Golden Apple"),
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Food,
    },
    ItemType {
        id: 283, name: "baked_potato", display: Some("Baked Potato"),
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Food,
    },
    ItemType {
        id: 284, name: "raw_porkchop", display: Some("Raw Porkchop"),
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Food,
    },
    ItemType {
        id: 285, name: "cooked_porkchop", display: Some("Cooked Porkchop"),
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Food,
    },
    ItemType {
        id: 286, name: "raw_mutton", display: Some("Raw Mutton"),
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Food,
    },
    ItemType {
        id: 287, name: "cooked_mutton", display: Some("Cooked Mutton"),
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Food,
    },
    ItemType {
        id: 288, name: "raw_cod", display: Some("Raw Cod"),
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Food,
    },
    ItemType {
        id: 289, name: "cooked_cod", display: Some("Cooked Cod"),
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Food,
    },
    ItemType {
        id: 290, name: "raw_salmon", display: Some("Raw Salmon"),
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Food,
    },
    ItemType {
        id: 291, name: "cooked_salmon", display: Some("Cooked Salmon"),
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Food,
    },
    ItemType {
        id: 292, name: "dried_kelp", display: Some("Dried Kelp"),
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Food,
    },
    // ---- W40.2: Mob drops (ids 293..=301) ----
    ItemType {
        id: 293, name: "rotten_flesh", display: Some("Rotten Flesh"),
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Ingredient,
    },
    ItemType {
        id: 294, name: "bone", display: None,
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Ingredient,
    },
    ItemType {
        id: 295, name: "ender_pearl", display: Some("Ender Pearl"),
        places: None, max_stack: 16, icon: None,
        category: ItemCategory::Ingredient,
    },
    ItemType {
        id: 296, name: "blaze_rod", display: Some("Blaze Rod"),
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Ingredient,
    },
    ItemType {
        id: 297, name: "ghast_tear", display: Some("Ghast Tear"),
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Ingredient,
    },
    ItemType {
        id: 298, name: "slime_ball", display: Some("Slime Ball"),
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Ingredient,
    },
    ItemType {
        id: 299, name: "gunpowder", display: None,
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Ingredient,
    },
    ItemType {
        id: 300, name: "spider_eye", display: Some("Spider Eye"),
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Ingredient,
    },
    ItemType {
        id: 301, name: "string", display: None,
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Ingredient,
    },
    // ---- W40.2: Misc ingredients (ids 302..=323) ----
    ItemType {
        id: 302, name: "stick", display: None,
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Ingredient,
    },
    ItemType {
        id: 303, name: "leather", display: None,
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Ingredient,
    },
    ItemType {
        id: 304, name: "paper", display: None,
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Ingredient,
    },
    ItemType {
        id: 305, name: "book", display: None,
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Ingredient,
    },
    ItemType {
        id: 306, name: "map", display: None,
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Ingredient,
    },
    ItemType {
        id: 307, name: "compass", display: None,
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Ingredient,
    },
    ItemType {
        id: 308, name: "clock", display: None,
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Ingredient,
    },
    ItemType {
        id: 309, name: "redstone_dust", display: Some("Redstone Dust"),
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Ingredient,
    },
    ItemType {
        id: 310, name: "glowstone_dust", display: Some("Glowstone Dust"),
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Ingredient,
    },
    ItemType {
        id: 311, name: "prismarine_shard", display: Some("Prismarine Shard"),
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Ingredient,
    },
    ItemType {
        id: 312, name: "prismarine_crystals", display: Some("Prismarine Crystals"),
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Ingredient,
    },
    ItemType {
        id: 313, name: "nautilus_shell", display: Some("Nautilus Shell"),
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Ingredient,
    },
    ItemType {
        id: 314, name: "heart_of_the_sea", display: Some("Heart of the Sea"),
        places: None, max_stack: 1, icon: None,
        category: ItemCategory::Ingredient,
    },
    ItemType {
        id: 315, name: "lapis_lazuli", display: Some("Lapis Lazuli"),
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Ingredient,
    },
    ItemType {
        id: 316, name: "diamond", display: None,
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Ingredient,
    },
    ItemType {
        id: 317, name: "emerald", display: None,
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Ingredient,
    },
    ItemType {
        id: 318, name: "gold_ingot", display: Some("Gold Ingot"),
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Ingredient,
    },
    ItemType {
        id: 319, name: "sugar", display: None,
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Ingredient,
    },
    ItemType {
        id: 320, name: "blaze_powder", display: Some("Blaze Powder"),
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Ingredient,
    },
    ItemType {
        id: 321, name: "magma_cream", display: Some("Magma Cream"),
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Ingredient,
    },
    ItemType {
        id: 322, name: "fermented_spider_eye", display: Some("Fermented Spider Eye"),
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Ingredient,
    },
    ItemType {
        id: 323, name: "amethyst_shard", display: Some("Amethyst Shard"),
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Ingredient,
    },
    // ---- W40.2: Extra item-only entries (ids 324..=350) ----
    ItemType {
        id: 324, name: "iron_ingot", display: Some("Iron Ingot"),
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Ingredient,
    },
    ItemType {
        id: 325, name: "coal", display: None,
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Ingredient,
    },
    ItemType {
        id: 326, name: "quartz", display: Some("Nether Quartz"),
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Ingredient,
    },
    ItemType {
        id: 327, name: "flint", display: None,
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Ingredient,
    },
    ItemType {
        id: 328, name: "copper_ingot", display: Some("Copper Ingot"),
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Ingredient,
    },
    ItemType {
        id: 329, name: "raw_iron", display: Some("Raw Iron"),
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Ingredient,
    },
    ItemType {
        id: 330, name: "raw_gold", display: Some("Raw Gold"),
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Ingredient,
    },
    ItemType {
        id: 331, name: "raw_copper", display: Some("Raw Copper"),
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Ingredient,
    },
    ItemType {
        id: 332, name: "wooden_hoe", display: Some("Wooden Hoe"),
        places: None, max_stack: 1, icon: None,
        category: ItemCategory::Tool,
    },
    ItemType {
        id: 333, name: "stone_hoe", display: Some("Stone Hoe"),
        places: None, max_stack: 1, icon: None,
        category: ItemCategory::Tool,
    },
    ItemType {
        id: 334, name: "iron_hoe", display: Some("Iron Hoe"),
        places: None, max_stack: 1, icon: None,
        category: ItemCategory::Tool,
    },
    ItemType {
        id: 335, name: "gold_hoe", display: Some("Golden Hoe"),
        places: None, max_stack: 1, icon: None,
        category: ItemCategory::Tool,
    },
    ItemType {
        id: 336, name: "diamond_hoe", display: Some("Diamond Hoe"),
        places: None, max_stack: 1, icon: None,
        category: ItemCategory::Tool,
    },
    ItemType {
        id: 337, name: "wheat", display: None,
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Ingredient,
    },
    ItemType {
        id: 338, name: "wheat_seeds", display: Some("Wheat Seeds"),
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Ingredient,
    },
    ItemType {
        id: 339, name: "pumpkin_seeds", display: Some("Pumpkin Seeds"),
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Ingredient,
    },
    ItemType {
        id: 340, name: "melon_seeds", display: Some("Melon Seeds"),
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Ingredient,
    },
    ItemType {
        id: 341, name: "beetroot_seeds", display: Some("Beetroot Seeds"),
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Ingredient,
    },
    ItemType {
        id: 342, name: "raw_beef", display: Some("Raw Beef"),
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Food,
    },
    ItemType {
        id: 343, name: "potato", display: None,
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Food,
    },
    ItemType {
        id: 344, name: "carrot", display: None,
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Food,
    },
    ItemType {
        id: 345, name: "beetroot", display: None,
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Food,
    },
    ItemType {
        id: 346, name: "mushroom_stew", display: Some("Mushroom Stew"),
        places: None, max_stack: 1, icon: None,
        category: ItemCategory::Food,
    },
    ItemType {
        id: 347, name: "bowl", display: None,
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Ingredient,
    },
    ItemType {
        id: 348, name: "ink_sac", display: Some("Ink Sac"),
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Ingredient,
    },
    ItemType {
        id: 349, name: "glow_ink_sac", display: Some("Glow Ink Sac"),
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Ingredient,
    },
    ItemType {
        id: 350, name: "echo_shard", display: Some("Echo Shard"),
        places: None, max_stack: 64, icon: None,
        category: ItemCategory::Ingredient,
    },
    // W46.21: copper torch family (MC 1.21+). Placeable decoration
    // matching torch / redstone_torch / soul_torch pattern.
    ItemType {
        id: 351, name: "copper_torch", display: Some("Copper Torch"),
        places: Some(351), max_stack: 64, icon: None,
        category: ItemCategory::Decoration,
    },
    ItemType {
        id: 352, name: "copper_wall_torch", display: Some("Copper Wall Torch"),
        places: Some(352), max_stack: 64, icon: None,
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
