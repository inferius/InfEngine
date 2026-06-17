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
    /// W49: Redstone hardware - lever, button, pressure plate,
    /// repeater, comparator, observer, piston, dispenser, dropper,
    /// hopper, redstone_block, target.
    Redstone,
    /// W49: Transportation - boats, minecarts, rails (future blocks).
    Transportation,
    /// W49: Brewing - brewing_stand, nether_wart, potions (future).
    Brewing,
    /// W148: spawn eggs - their own creative tab like MC's
    /// `CreativeModeTabs.SPAWN_EGGS`.
    SpawnEgg,
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
    // ---- W48.1 - 50 new blocks mirror (ids 353-402) ----
    ItemType { id: 353, name: "polished_granite", display: Some("Polished Granite"), places: Some(353), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 354, name: "polished_diorite", display: Some("Polished Diorite"), places: Some(354), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 355, name: "polished_andesite", display: Some("Polished Andesite"), places: Some(355), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 356, name: "smooth_sandstone", display: Some("Smooth Sandstone"), places: Some(356), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 357, name: "cut_sandstone", display: Some("Cut Sandstone"), places: Some(357), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 358, name: "red_sandstone", display: Some("Red Sandstone"), places: Some(358), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 359, name: "smooth_red_sandstone", display: Some("Smooth Red Sandstone"), places: Some(359), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 360, name: "cut_red_sandstone", display: Some("Cut Red Sandstone"), places: Some(360), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 361, name: "cobbled_deepslate", display: Some("Cobbled Deepslate"), places: Some(361), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 362, name: "polished_deepslate", display: Some("Polished Deepslate"), places: Some(362), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 363, name: "deepslate_bricks", display: Some("Deepslate Bricks"), places: Some(363), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 364, name: "deepslate_tiles", display: Some("Deepslate Tiles"), places: Some(364), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 365, name: "chiseled_stone_bricks", display: Some("Chiseled Stone Bricks"), places: Some(365), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 366, name: "mossy_cobblestone", display: Some("Mossy Cobblestone"), places: Some(366), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 367, name: "mossy_stone_bricks", display: Some("Mossy Stone Bricks"), places: Some(367), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 368, name: "blackstone", display: Some("Blackstone"), places: Some(368), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 369, name: "polished_blackstone", display: Some("Polished Blackstone"), places: Some(369), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 370, name: "polished_blackstone_bricks", display: Some("Polished Blackstone Bricks"), places: Some(370), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 371, name: "gilded_blackstone", display: Some("Gilded Blackstone"), places: Some(371), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 372, name: "basalt", display: Some("Basalt"), places: Some(372), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 373, name: "polished_basalt", display: Some("Polished Basalt"), places: Some(373), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 374, name: "soul_sand", display: Some("Soul Sand"), places: Some(374), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 375, name: "soul_soil", display: Some("Soul Soil"), places: Some(375), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 376, name: "nether_bricks", display: Some("Nether Bricks"), places: Some(376), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 377, name: "nether_wart_block", display: Some("Nether Wart Block"), places: Some(377), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 378, name: "end_stone", display: Some("End Stone"), places: Some(378), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 379, name: "end_stone_bricks", display: Some("End Stone Bricks"), places: Some(379), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 380, name: "purpur_block", display: Some("Purpur Block"), places: Some(380), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 381, name: "purpur_pillar", display: Some("Purpur Pillar"), places: Some(381), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 382, name: "chorus_flower", display: Some("Chorus Flower"), places: Some(382), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 383, name: "copper_block", display: Some("Copper Block"), places: Some(383), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 384, name: "exposed_copper", display: Some("Exposed Copper"), places: Some(384), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 385, name: "weathered_copper", display: Some("Weathered Copper"), places: Some(385), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 386, name: "oxidized_copper", display: Some("Oxidized Copper"), places: Some(386), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 387, name: "cut_copper", display: Some("Cut Copper"), places: Some(387), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 388, name: "raw_copper_block", display: Some("Block of Raw Copper"), places: Some(388), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 389, name: "raw_iron_block", display: Some("Block of Raw Iron"), places: Some(389), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 390, name: "raw_gold_block", display: Some("Block of Raw Gold"), places: Some(390), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 391, name: "gold_block", display: Some("Block of Gold"), places: Some(391), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 392, name: "diamond_block", display: Some("Block of Diamond"), places: Some(392), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 393, name: "dirt_path", display: Some("Dirt Path"), places: Some(393), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 394, name: "podzol", display: Some("Podzol"), places: Some(394), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 395, name: "mycelium", display: Some("Mycelium"), places: Some(395), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 396, name: "coarse_dirt", display: Some("Coarse Dirt"), places: Some(396), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 397, name: "mud", display: Some("Mud"), places: Some(397), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 398, name: "packed_mud", display: Some("Packed Mud"), places: Some(398), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 399, name: "mud_bricks", display: Some("Mud Bricks"), places: Some(399), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 400, name: "ice", display: Some("Ice"), places: Some(400), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 401, name: "packed_ice", display: Some("Packed Ice"), places: Some(401), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 402, name: "blue_ice", display: Some("Blue Ice"), places: Some(402), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    // ---- W48.2 - 50 more blocks mirror (ids 403-452) ----
    ItemType { id: 403, name: "white_stained_glass", display: Some("White Stained Glass"), places: Some(403), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 404, name: "orange_stained_glass", display: Some("Orange Stained Glass"), places: Some(404), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 405, name: "magenta_stained_glass", display: Some("Magenta Stained Glass"), places: Some(405), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 406, name: "light_blue_stained_glass", display: Some("Light Blue Stained Glass"), places: Some(406), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 407, name: "yellow_stained_glass", display: Some("Yellow Stained Glass"), places: Some(407), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 408, name: "lime_stained_glass", display: Some("Lime Stained Glass"), places: Some(408), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 409, name: "pink_stained_glass", display: Some("Pink Stained Glass"), places: Some(409), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 410, name: "gray_stained_glass", display: Some("Gray Stained Glass"), places: Some(410), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 411, name: "light_gray_stained_glass", display: Some("Light Gray Stained Glass"), places: Some(411), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 412, name: "cyan_stained_glass", display: Some("Cyan Stained Glass"), places: Some(412), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 413, name: "purple_stained_glass", display: Some("Purple Stained Glass"), places: Some(413), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 414, name: "blue_stained_glass", display: Some("Blue Stained Glass"), places: Some(414), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 415, name: "brown_stained_glass", display: Some("Brown Stained Glass"), places: Some(415), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 416, name: "green_stained_glass", display: Some("Green Stained Glass"), places: Some(416), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 417, name: "red_stained_glass", display: Some("Red Stained Glass"), places: Some(417), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 418, name: "black_stained_glass", display: Some("Black Stained Glass"), places: Some(418), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 419, name: "terracotta", display: Some("Terracotta"), places: Some(419), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 420, name: "white_terracotta", display: Some("White Terracotta"), places: Some(420), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 421, name: "orange_terracotta", display: Some("Orange Terracotta"), places: Some(421), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 422, name: "magenta_terracotta", display: Some("Magenta Terracotta"), places: Some(422), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 423, name: "light_blue_terracotta", display: Some("Light Blue Terracotta"), places: Some(423), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 424, name: "yellow_terracotta", display: Some("Yellow Terracotta"), places: Some(424), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 425, name: "lime_terracotta", display: Some("Lime Terracotta"), places: Some(425), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 426, name: "pink_terracotta", display: Some("Pink Terracotta"), places: Some(426), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 427, name: "gray_terracotta", display: Some("Gray Terracotta"), places: Some(427), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 428, name: "light_gray_terracotta", display: Some("Light Gray Terracotta"), places: Some(428), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 429, name: "cyan_terracotta", display: Some("Cyan Terracotta"), places: Some(429), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 430, name: "purple_terracotta", display: Some("Purple Terracotta"), places: Some(430), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 431, name: "blue_terracotta", display: Some("Blue Terracotta"), places: Some(431), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 432, name: "brown_terracotta", display: Some("Brown Terracotta"), places: Some(432), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 433, name: "green_terracotta", display: Some("Green Terracotta"), places: Some(433), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 434, name: "red_terracotta", display: Some("Red Terracotta"), places: Some(434), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 435, name: "hay_block", display: Some("Hay Bale"), places: Some(435), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 436, name: "dried_kelp_block", display: Some("Dried Kelp Block"), places: Some(436), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 437, name: "bone_block", display: Some("Bone Block"), places: Some(437), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 438, name: "lapis_block", display: Some("Block of Lapis Lazuli"), places: Some(438), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 439, name: "emerald_block", display: Some("Block of Emerald"), places: Some(439), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 440, name: "iron_block", display: Some("Block of Iron"), places: Some(440), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 441, name: "redstone_block", display: Some("Block of Redstone"), places: Some(441), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 442, name: "slime_block", display: Some("Slime Block"), places: Some(442), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 443, name: "honey_block", display: Some("Honey Block"), places: Some(443), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 444, name: "honeycomb_block", display: Some("Honeycomb Block"), places: Some(444), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 445, name: "sponge", display: Some("Sponge"), places: Some(445), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 446, name: "wet_sponge", display: Some("Wet Sponge"), places: Some(446), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 447, name: "melon", display: Some("Melon"), places: Some(447), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 448, name: "pumpkin", display: Some("Pumpkin"), places: Some(448), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 449, name: "carved_pumpkin", display: Some("Carved Pumpkin"), places: Some(449), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 450, name: "obsidian_pillar", display: Some("Obsidian Pillar"), places: Some(450), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 451, name: "amethyst_block", display: Some("Block of Amethyst"), places: Some(451), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 452, name: "budding_amethyst", display: Some("Budding Amethyst"), places: Some(452), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    // ---- W48.3 - redstone + plants + crops (ids 453-502) ----
    ItemType { id: 453, name: "lever", display: Some("Lever"), places: Some(453), max_stack: 64, icon: None, category: ItemCategory::Redstone },
    ItemType { id: 454, name: "oak_button", display: Some("Oak Button"), places: Some(454), max_stack: 64, icon: None, category: ItemCategory::Redstone },
    ItemType { id: 455, name: "stone_button", display: Some("Stone Button"), places: Some(455), max_stack: 64, icon: None, category: ItemCategory::Redstone },
    ItemType { id: 456, name: "oak_pressure_plate", display: Some("Oak Pressure Plate"), places: Some(456), max_stack: 64, icon: None, category: ItemCategory::Redstone },
    ItemType { id: 457, name: "stone_pressure_plate", display: Some("Stone Pressure Plate"), places: Some(457), max_stack: 64, icon: None, category: ItemCategory::Redstone },
    ItemType { id: 458, name: "light_weighted_pressure_plate", display: Some("Light Weighted Pressure Plate"), places: Some(458), max_stack: 64, icon: None, category: ItemCategory::Redstone },
    ItemType { id: 459, name: "heavy_weighted_pressure_plate", display: Some("Heavy Weighted Pressure Plate"), places: Some(459), max_stack: 64, icon: None, category: ItemCategory::Redstone },
    ItemType { id: 460, name: "repeater", display: Some("Redstone Repeater"), places: Some(460), max_stack: 64, icon: None, category: ItemCategory::Redstone },
    ItemType { id: 461, name: "comparator", display: Some("Redstone Comparator"), places: Some(461), max_stack: 64, icon: None, category: ItemCategory::Redstone },
    ItemType { id: 462, name: "observer", display: Some("Observer"), places: Some(462), max_stack: 64, icon: None, category: ItemCategory::Redstone },
    ItemType { id: 463, name: "piston", display: Some("Piston"), places: Some(463), max_stack: 64, icon: None, category: ItemCategory::Redstone },
    ItemType { id: 464, name: "sticky_piston", display: Some("Sticky Piston"), places: Some(464), max_stack: 64, icon: None, category: ItemCategory::Redstone },
    ItemType { id: 465, name: "dispenser", display: Some("Dispenser"), places: Some(465), max_stack: 64, icon: None, category: ItemCategory::Redstone },
    ItemType { id: 466, name: "dropper", display: Some("Dropper"), places: Some(466), max_stack: 64, icon: None, category: ItemCategory::Redstone },
    ItemType { id: 467, name: "hopper", display: Some("Hopper"), places: Some(467), max_stack: 64, icon: None, category: ItemCategory::Redstone },
    ItemType { id: 468, name: "oak_sapling", display: Some("Oak Sapling"), places: Some(468), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 469, name: "birch_sapling", display: Some("Birch Sapling"), places: Some(469), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 470, name: "spruce_sapling", display: Some("Spruce Sapling"), places: Some(470), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 471, name: "jungle_sapling", display: Some("Jungle Sapling"), places: Some(471), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 472, name: "acacia_sapling", display: Some("Acacia Sapling"), places: Some(472), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 473, name: "dark_oak_sapling", display: Some("Dark Oak Sapling"), places: Some(473), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 474, name: "mangrove_propagule", display: Some("Mangrove Propagule"), places: Some(474), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 475, name: "cherry_sapling", display: Some("Cherry Sapling"), places: Some(475), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 476, name: "azalea", display: Some("Azalea"), places: Some(476), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 477, name: "flowering_azalea", display: Some("Flowering Azalea"), places: Some(477), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 478, name: "allium", display: Some("Allium"), places: Some(478), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 479, name: "blue_orchid", display: Some("Blue Orchid"), places: Some(479), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 480, name: "dandelion", display: Some("Dandelion"), places: Some(480), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 481, name: "poppy", display: Some("Poppy"), places: Some(481), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 482, name: "oxeye_daisy", display: Some("Oxeye Daisy"), places: Some(482), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 483, name: "cornflower", display: Some("Cornflower"), places: Some(483), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 484, name: "lily_of_the_valley", display: Some("Lily of the Valley"), places: Some(484), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 485, name: "sunflower", display: Some("Sunflower"), places: Some(485), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 486, name: "peony", display: Some("Peony"), places: Some(486), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 487, name: "rose_bush", display: Some("Rose Bush"), places: Some(487), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 488, name: "lilac", display: Some("Lilac"), places: Some(488), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 489, name: "wither_rose", display: Some("Wither Rose"), places: Some(489), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 490, name: "torchflower", display: Some("Torchflower"), places: Some(490), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 491, name: "pitcher_plant", display: Some("Pitcher Plant"), places: Some(491), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 492, name: "dead_bush", display: Some("Dead Bush"), places: Some(492), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 493, name: "wheat", display: Some("Wheat"), places: Some(493), max_stack: 64, icon: None, category: ItemCategory::Food },
    ItemType { id: 494, name: "carrots", display: Some("Carrots"), places: Some(494), max_stack: 64, icon: None, category: ItemCategory::Food },
    ItemType { id: 495, name: "potatoes", display: Some("Potatoes"), places: Some(495), max_stack: 64, icon: None, category: ItemCategory::Food },
    ItemType { id: 496, name: "beetroots", display: Some("Beetroots"), places: Some(496), max_stack: 64, icon: None, category: ItemCategory::Food },
    ItemType { id: 497, name: "melon_stem", display: Some("Melon Stem"), places: Some(497), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 498, name: "pumpkin_stem", display: Some("Pumpkin Stem"), places: Some(498), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 499, name: "sugar_cane", display: Some("Sugar Cane"), places: Some(499), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 500, name: "bamboo", display: Some("Bamboo"), places: Some(500), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 501, name: "cocoa", display: Some("Cocoa Pod"), places: Some(501), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 502, name: "nether_wart", display: Some("Nether Wart"), places: Some(502), max_stack: 64, icon: None, category: ItemCategory::Brewing },
    // ---- W48.4 - slabs/stairs/walls/fences/trapdoors (ids 503-552) ----
    ItemType { id: 503, name: "birch_slab", display: Some("Birch Slab"), places: Some(503), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 504, name: "spruce_slab", display: Some("Spruce Slab"), places: Some(504), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 505, name: "jungle_slab", display: Some("Jungle Slab"), places: Some(505), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 506, name: "acacia_slab", display: Some("Acacia Slab"), places: Some(506), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 507, name: "dark_oak_slab", display: Some("Dark Oak Slab"), places: Some(507), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 508, name: "mangrove_slab", display: Some("Mangrove Slab"), places: Some(508), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 509, name: "cherry_slab", display: Some("Cherry Slab"), places: Some(509), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 510, name: "smooth_stone_slab", display: Some("Smooth Stone Slab"), places: Some(510), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 511, name: "sandstone_slab", display: Some("Sandstone Slab"), places: Some(511), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 512, name: "red_sandstone_slab", display: Some("Red Sandstone Slab"), places: Some(512), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 513, name: "brick_slab", display: Some("Brick Slab"), places: Some(513), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 514, name: "stone_brick_slab", display: Some("Stone Brick Slab"), places: Some(514), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 515, name: "mossy_stone_brick_slab", display: Some("Mossy Stone Brick Slab"), places: Some(515), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 516, name: "nether_brick_slab", display: Some("Nether Brick Slab"), places: Some(516), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 517, name: "end_stone_brick_slab", display: Some("End Stone Brick Slab"), places: Some(517), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 518, name: "birch_stairs", display: Some("Birch Stairs"), places: Some(518), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 519, name: "spruce_stairs", display: Some("Spruce Stairs"), places: Some(519), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 520, name: "jungle_stairs", display: Some("Jungle Stairs"), places: Some(520), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 521, name: "acacia_stairs", display: Some("Acacia Stairs"), places: Some(521), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 522, name: "dark_oak_stairs", display: Some("Dark Oak Stairs"), places: Some(522), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 523, name: "mangrove_stairs", display: Some("Mangrove Stairs"), places: Some(523), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 524, name: "cherry_stairs", display: Some("Cherry Stairs"), places: Some(524), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 525, name: "sandstone_stairs", display: Some("Sandstone Stairs"), places: Some(525), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 526, name: "red_sandstone_stairs", display: Some("Red Sandstone Stairs"), places: Some(526), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 527, name: "brick_stairs", display: Some("Brick Stairs"), places: Some(527), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 528, name: "stone_brick_stairs", display: Some("Stone Brick Stairs"), places: Some(528), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 529, name: "mossy_stone_brick_stairs", display: Some("Mossy Stone Brick Stairs"), places: Some(529), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 530, name: "nether_brick_stairs", display: Some("Nether Brick Stairs"), places: Some(530), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 531, name: "granite_stairs", display: Some("Granite Stairs"), places: Some(531), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 532, name: "polished_granite_stairs", display: Some("Polished Granite Stairs"), places: Some(532), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 533, name: "granite_wall", display: Some("Granite Wall"), places: Some(533), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 534, name: "diorite_wall", display: Some("Diorite Wall"), places: Some(534), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 535, name: "andesite_wall", display: Some("Andesite Wall"), places: Some(535), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 536, name: "sandstone_wall", display: Some("Sandstone Wall"), places: Some(536), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 537, name: "red_sandstone_wall", display: Some("Red Sandstone Wall"), places: Some(537), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 538, name: "brick_wall", display: Some("Brick Wall"), places: Some(538), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 539, name: "stone_brick_wall", display: Some("Stone Brick Wall"), places: Some(539), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 540, name: "mossy_stone_brick_wall", display: Some("Mossy Stone Brick Wall"), places: Some(540), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 541, name: "nether_brick_wall", display: Some("Nether Brick Wall"), places: Some(541), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 542, name: "end_stone_brick_wall", display: Some("End Stone Brick Wall"), places: Some(542), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 543, name: "birch_fence", display: Some("Birch Fence"), places: Some(543), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 544, name: "spruce_fence", display: Some("Spruce Fence"), places: Some(544), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 545, name: "jungle_fence", display: Some("Jungle Fence"), places: Some(545), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 546, name: "acacia_fence", display: Some("Acacia Fence"), places: Some(546), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 547, name: "dark_oak_fence", display: Some("Dark Oak Fence"), places: Some(547), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 548, name: "oak_trapdoor", display: Some("Oak Trapdoor"), places: Some(548), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 549, name: "iron_trapdoor", display: Some("Iron Trapdoor"), places: Some(549), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 550, name: "birch_trapdoor", display: Some("Birch Trapdoor"), places: Some(550), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 551, name: "spruce_trapdoor", display: Some("Spruce Trapdoor"), places: Some(551), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 552, name: "jungle_trapdoor", display: Some("Jungle Trapdoor"), places: Some(552), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    // ---- W48.5 - utility + doors + beds + misc (ids 553-602) ----
    ItemType { id: 553, name: "crafting_table", display: Some("Crafting Table"), places: Some(553), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 554, name: "anvil", display: Some("Anvil"), places: Some(554), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 555, name: "chipped_anvil", display: Some("Chipped Anvil"), places: Some(555), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 556, name: "damaged_anvil", display: Some("Damaged Anvil"), places: Some(556), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 557, name: "smithing_table", display: Some("Smithing Table"), places: Some(557), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 558, name: "fletching_table", display: Some("Fletching Table"), places: Some(558), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 559, name: "cartography_table", display: Some("Cartography Table"), places: Some(559), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 560, name: "lectern", display: Some("Lectern"), places: Some(560), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 561, name: "smoker", display: Some("Smoker"), places: Some(561), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 562, name: "blast_furnace", display: Some("Blast Furnace"), places: Some(562), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 563, name: "grindstone", display: Some("Grindstone"), places: Some(563), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 564, name: "stonecutter", display: Some("Stonecutter"), places: Some(564), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 565, name: "loom", display: Some("Loom"), places: Some(565), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 566, name: "brewing_stand", display: Some("Brewing Stand"), places: Some(566), max_stack: 64, icon: None, category: ItemCategory::Brewing },
    ItemType { id: 567, name: "beacon", display: Some("Beacon"), places: Some(567), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 568, name: "birch_door", display: Some("Birch Door"), places: Some(568), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 569, name: "spruce_door", display: Some("Spruce Door"), places: Some(569), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 570, name: "jungle_door", display: Some("Jungle Door"), places: Some(570), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 571, name: "acacia_door", display: Some("Acacia Door"), places: Some(571), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 572, name: "dark_oak_door", display: Some("Dark Oak Door"), places: Some(572), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 573, name: "mangrove_door", display: Some("Mangrove Door"), places: Some(573), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 574, name: "cherry_door", display: Some("Cherry Door"), places: Some(574), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 575, name: "warped_door", display: Some("Warped Door"), places: Some(575), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 576, name: "white_bed", display: Some("White Bed"), places: Some(576), max_stack: 1, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 577, name: "orange_bed", display: Some("Orange Bed"), places: Some(577), max_stack: 1, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 578, name: "magenta_bed", display: Some("Magenta Bed"), places: Some(578), max_stack: 1, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 579, name: "light_blue_bed", display: Some("Light Blue Bed"), places: Some(579), max_stack: 1, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 580, name: "yellow_bed", display: Some("Yellow Bed"), places: Some(580), max_stack: 1, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 581, name: "lime_bed", display: Some("Lime Bed"), places: Some(581), max_stack: 1, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 582, name: "pink_bed", display: Some("Pink Bed"), places: Some(582), max_stack: 1, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 583, name: "gray_bed", display: Some("Gray Bed"), places: Some(583), max_stack: 1, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 584, name: "light_gray_bed", display: Some("Light Gray Bed"), places: Some(584), max_stack: 1, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 585, name: "cyan_bed", display: Some("Cyan Bed"), places: Some(585), max_stack: 1, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 586, name: "purple_bed", display: Some("Purple Bed"), places: Some(586), max_stack: 1, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 587, name: "blue_bed", display: Some("Blue Bed"), places: Some(587), max_stack: 1, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 588, name: "brown_bed", display: Some("Brown Bed"), places: Some(588), max_stack: 1, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 589, name: "green_bed", display: Some("Green Bed"), places: Some(589), max_stack: 1, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 590, name: "red_bed", display: Some("Red Bed"), places: Some(590), max_stack: 1, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 591, name: "black_bed", display: Some("Black Bed"), places: Some(591), max_stack: 1, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 592, name: "jukebox", display: Some("Jukebox"), places: Some(592), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 593, name: "note_block", display: Some("Note Block"), places: Some(593), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 594, name: "scaffolding", display: Some("Scaffolding"), places: Some(594), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 595, name: "ladder", display: Some("Ladder"), places: Some(595), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 596, name: "end_portal_frame", display: Some("End Portal Frame"), places: Some(596), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 597, name: "dragon_egg", display: Some("Dragon Egg"), places: Some(597), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 598, name: "conduit", display: Some("Conduit"), places: Some(598), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 599, name: "target", display: Some("Target"), places: Some(599), max_stack: 64, icon: None, category: ItemCategory::Redstone },
    ItemType { id: 600, name: "bee_nest", display: Some("Bee Nest"), places: Some(600), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 601, name: "beehive", display: Some("Beehive"), places: Some(601), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 602, name: "lodestone", display: Some("Lodestone"), places: Some(602), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    // ---- W48.6 - waxed copper + nether + lush cave + coral + sculk (603-632) ----
    ItemType { id: 603, name: "waxed_copper_block", display: Some("Waxed Block of Copper"), places: Some(603), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 604, name: "waxed_exposed_copper", display: Some("Waxed Exposed Copper"), places: Some(604), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 605, name: "waxed_weathered_copper", display: Some("Waxed Weathered Copper"), places: Some(605), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 606, name: "waxed_oxidized_copper", display: Some("Waxed Oxidized Copper"), places: Some(606), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 607, name: "crimson_nylium", display: Some("Crimson Nylium"), places: Some(607), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 608, name: "warped_nylium", display: Some("Warped Nylium"), places: Some(608), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 609, name: "crimson_fungus", display: Some("Crimson Fungus"), places: Some(609), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 610, name: "warped_fungus", display: Some("Warped Fungus"), places: Some(610), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 611, name: "weeping_vines", display: Some("Weeping Vines"), places: Some(611), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 612, name: "twisting_vines", display: Some("Twisting Vines"), places: Some(612), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 613, name: "nether_sprouts", display: Some("Nether Sprouts"), places: Some(613), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 614, name: "warped_wart_block", display: Some("Warped Wart Block"), places: Some(614), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 615, name: "moss_block", display: Some("Moss Block"), places: Some(615), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 616, name: "moss_carpet", display: Some("Moss Carpet"), places: Some(616), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 617, name: "big_dripleaf", display: Some("Big Dripleaf"), places: Some(617), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 618, name: "small_dripleaf", display: Some("Small Dripleaf"), places: Some(618), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 619, name: "hanging_roots", display: Some("Hanging Roots"), places: Some(619), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 620, name: "spore_blossom", display: Some("Spore Blossom"), places: Some(620), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 621, name: "tube_coral_block", display: Some("Tube Coral Block"), places: Some(621), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 622, name: "brain_coral_block", display: Some("Brain Coral Block"), places: Some(622), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 623, name: "bubble_coral_block", display: Some("Bubble Coral Block"), places: Some(623), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 624, name: "fire_coral_block", display: Some("Fire Coral Block"), places: Some(624), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 625, name: "horn_coral_block", display: Some("Horn Coral Block"), places: Some(625), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 626, name: "dead_brain_coral_block", display: Some("Dead Brain Coral Block"), places: Some(626), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 627, name: "glow_lichen", display: Some("Glow Lichen"), places: Some(627), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 628, name: "sculk", display: Some("Sculk"), places: Some(628), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 629, name: "sculk_catalyst", display: Some("Sculk Catalyst"), places: Some(629), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 630, name: "sculk_shrieker", display: Some("Sculk Shrieker"), places: Some(630), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 631, name: "sculk_sensor", display: Some("Sculk Sensor"), places: Some(631), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 632, name: "calibrated_sculk_sensor", display: Some("Calibrated Sculk Sensor"), places: Some(632), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    // W53.4 - missing tool items (hoes, bow, crossbow, shears,
    // fishing_rod). ItemCategory::Tool covers hoes/shears/fishing_rod;
    // bow + crossbow are ranged so they live in Weapon to land in
    // the Combat tab alongside swords. max_stack=1 enforced by
    // mc_stack_override regardless of table value, but we set it
    // explicitly so the table reads MC-strict at a glance.
    ItemType { id: 633, name: "hoe_wood", display: Some("Wood Hoe"), places: None, max_stack: 1, icon: None, category: ItemCategory::Tool },
    ItemType { id: 634, name: "hoe_stone", display: Some("Stone Hoe"), places: None, max_stack: 1, icon: None, category: ItemCategory::Tool },
    ItemType { id: 635, name: "hoe_iron", display: Some("Iron Hoe"), places: None, max_stack: 1, icon: None, category: ItemCategory::Tool },
    ItemType { id: 636, name: "hoe_gold", display: Some("Golden Hoe"), places: None, max_stack: 1, icon: None, category: ItemCategory::Tool },
    ItemType { id: 637, name: "hoe_diamond", display: Some("Diamond Hoe"), places: None, max_stack: 1, icon: None, category: ItemCategory::Tool },
    ItemType { id: 638, name: "bow", display: Some("Bow"), places: None, max_stack: 1, icon: None, category: ItemCategory::Weapon },
    ItemType { id: 639, name: "crossbow", display: Some("Crossbow"), places: None, max_stack: 1, icon: None, category: ItemCategory::Weapon },
    ItemType { id: 640, name: "shears", display: Some("Shears"), places: None, max_stack: 1, icon: None, category: ItemCategory::Tool },
    ItemType { id: 641, name: "fishing_rod", display: Some("Fishing Rod"), places: None, max_stack: 1, icon: None, category: ItemCategory::Tool },
    ItemType { id: 642, name: "shield", display: Some("Shield"), places: None, max_stack: 1, icon: None, category: ItemCategory::Weapon },
    ItemType { id: 643, name: "arrow", display: Some("Arrow"), places: None, max_stack: 64, icon: None, category: ItemCategory::Weapon },
    // W64.2 - gravity blocks (gravel + red_sand). Block-form items;
    // ids match their block ids (644/645).
    ItemType { id: 644, name: "gravel", display: Some("Gravel"), places: Some(644), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 645, name: "red_sand", display: Some("Red Sand"), places: Some(645), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    // W65.4 - immature crop stages. Item forms exist only to satisfy
    // the 1:1 block<->item invariant; they place their own young
    // block (which then grows) and harvest to nothing.
    ItemType { id: 646, name: "wheat_immature", display: Some("Wheat (Growing)"), places: Some(646), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 647, name: "carrots_immature", display: Some("Carrots (Growing)"), places: Some(647), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 648, name: "potatoes_immature", display: Some("Potatoes (Growing)"), places: Some(648), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 649, name: "beetroots_immature", display: Some("Beetroots (Growing)"), places: Some(649), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    // W67.1 - unlit redstone lamp (the placeable / off state).
    ItemType { id: 650, name: "redstone_lamp", display: Some("Redstone Lamp"), places: Some(650), max_stack: 64, icon: None, category: ItemCategory::Redstone },
    // W80 - MC ground cover plant items (icon auto-resolves from the
    // matching png stem: short_grass.png / fern.png).
    ItemType { id: 651, name: "short_grass", display: Some("Grass"), places: Some(651), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 652, name: "fern", display: Some("Fern"), places: Some(652), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    // W127 - rails (Transportation; carts are a later wave).
    ItemType { id: 653, name: "rail", display: None, places: Some(653), max_stack: 64, icon: None, category: ItemCategory::Transportation },
    ItemType { id: 654, name: "powered_rail", display: None, places: Some(654), max_stack: 64, icon: None, category: ItemCategory::Transportation },
    ItemType { id: 655, name: "detector_rail", display: None, places: Some(655), max_stack: 64, icon: None, category: ItemCategory::Transportation },
    ItemType { id: 656, name: "activator_rail", display: None, places: Some(656), max_stack: 64, icon: None, category: ItemCategory::Transportation },
    // W134: the minecart is item-only (places=None) - RMB on a rail
    // spawns a cart ENTITY via RmbAction::PlaceMinecart.
    ItemType { id: 657, name: "minecart", display: None, places: None, max_stack: 1, icon: None, category: ItemCategory::Transportation },
    // W145 - the 16 MC dyes (item-only; icons auto-resolve from the
    // matching png stems). Wool/sheep dyeing wires up in a follow-up;
    // the items exist so recipes + creative search match MC today.
    ItemType { id: 658, name: "white_dye", display: None, places: None, max_stack: 64, icon: None, category: ItemCategory::Ingredient },
    ItemType { id: 659, name: "light_gray_dye", display: None, places: None, max_stack: 64, icon: None, category: ItemCategory::Ingredient },
    ItemType { id: 660, name: "gray_dye", display: None, places: None, max_stack: 64, icon: None, category: ItemCategory::Ingredient },
    ItemType { id: 661, name: "black_dye", display: None, places: None, max_stack: 64, icon: None, category: ItemCategory::Ingredient },
    ItemType { id: 662, name: "brown_dye", display: None, places: None, max_stack: 64, icon: None, category: ItemCategory::Ingredient },
    ItemType { id: 663, name: "red_dye", display: None, places: None, max_stack: 64, icon: None, category: ItemCategory::Ingredient },
    ItemType { id: 664, name: "orange_dye", display: None, places: None, max_stack: 64, icon: None, category: ItemCategory::Ingredient },
    ItemType { id: 665, name: "yellow_dye", display: None, places: None, max_stack: 64, icon: None, category: ItemCategory::Ingredient },
    ItemType { id: 666, name: "lime_dye", display: None, places: None, max_stack: 64, icon: None, category: ItemCategory::Ingredient },
    ItemType { id: 667, name: "green_dye", display: None, places: None, max_stack: 64, icon: None, category: ItemCategory::Ingredient },
    ItemType { id: 668, name: "cyan_dye", display: None, places: None, max_stack: 64, icon: None, category: ItemCategory::Ingredient },
    ItemType { id: 669, name: "light_blue_dye", display: None, places: None, max_stack: 64, icon: None, category: ItemCategory::Ingredient },
    ItemType { id: 670, name: "blue_dye", display: None, places: None, max_stack: 64, icon: None, category: ItemCategory::Ingredient },
    ItemType { id: 671, name: "purple_dye", display: None, places: None, max_stack: 64, icon: None, category: ItemCategory::Ingredient },
    ItemType { id: 672, name: "magenta_dye", display: None, places: None, max_stack: 64, icon: None, category: ItemCategory::Ingredient },
    ItemType { id: 673, name: "pink_dye", display: None, places: None, max_stack: 64, icon: None, category: ItemCategory::Ingredient },
    // W145 - simple MC items whose textures already ship in the pack.
    ItemType { id: 674, name: "bone_meal", display: None, places: None, max_stack: 64, icon: None, category: ItemCategory::Ingredient },
    ItemType { id: 675, name: "brick", display: None, places: None, max_stack: 64, icon: None, category: ItemCategory::Ingredient },
    ItemType { id: 676, name: "beetroot_soup", display: None, places: None, max_stack: 1, icon: None, category: ItemCategory::Food },
    // W145 - spawn eggs for every implemented mob kind. RMB on a
    // block spawns the mob (RmbAction::SpawnEgg); 26.x packs ship
    // per-mob egg textures so the icons auto-resolve by stem.
    ItemType { id: 677, name: "chicken_spawn_egg", display: None, places: None, max_stack: 64, icon: None, category: ItemCategory::SpawnEgg },
    ItemType { id: 678, name: "cow_spawn_egg", display: None, places: None, max_stack: 64, icon: None, category: ItemCategory::SpawnEgg },
    ItemType { id: 679, name: "pig_spawn_egg", display: None, places: None, max_stack: 64, icon: None, category: ItemCategory::SpawnEgg },
    ItemType { id: 680, name: "sheep_spawn_egg", display: None, places: None, max_stack: 64, icon: None, category: ItemCategory::SpawnEgg },
    ItemType { id: 681, name: "zombie_spawn_egg", display: None, places: None, max_stack: 64, icon: None, category: ItemCategory::SpawnEgg },
    ItemType { id: 682, name: "skeleton_spawn_egg", display: None, places: None, max_stack: 64, icon: None, category: ItemCategory::SpawnEgg },
    ItemType { id: 683, name: "spider_spawn_egg", display: None, places: None, max_stack: 64, icon: None, category: ItemCategory::SpawnEgg },
    // W158 - wolf (ids 684-716 are the W150/W152 block families).
    ItemType { id: 717, name: "wolf_spawn_egg", display: None, places: None, max_stack: 64, icon: None, category: ItemCategory::SpawnEgg },
    // W159 - bookshelf (was missing).
    ItemType { id: 718, name: "bookshelf", display: Some("Bookshelf"), places: Some(718), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    // W160 - enchanting table (was missing).
    ItemType { id: 719, name: "enchanting_table", display: Some("Enchanting Table"), places: Some(719), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    // W161 - polar bear spawn egg.
    ItemType { id: 720, name: "polar_bear_spawn_egg", display: None, places: None, max_stack: 64, icon: None, category: ItemCategory::SpawnEgg },
    // W162 - rabbit spawn egg.
    ItemType { id: 721, name: "rabbit_spawn_egg", display: None, places: None, max_stack: 64, icon: None, category: ItemCategory::SpawnEgg },
    // W166 - the remaining wood trapdoors (were missing).
    ItemType { id: 722, name: "acacia_trapdoor", display: Some("Acacia Trapdoor"), places: Some(722), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 723, name: "dark_oak_trapdoor", display: Some("Dark Oak Trapdoor"), places: Some(723), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 724, name: "mangrove_trapdoor", display: Some("Mangrove Trapdoor"), places: Some(724), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 725, name: "cherry_trapdoor", display: Some("Cherry Trapdoor"), places: Some(725), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    // W167 - fence gates (were missing).
    ItemType { id: 726, name: "oak_fence_gate", display: Some("Oak Fence Gate"), places: Some(726), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 727, name: "birch_fence_gate", display: Some("Birch Fence Gate"), places: Some(727), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 728, name: "spruce_fence_gate", display: Some("Spruce Fence Gate"), places: Some(728), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 729, name: "jungle_fence_gate", display: Some("Jungle Fence Gate"), places: Some(729), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 730, name: "acacia_fence_gate", display: Some("Acacia Fence Gate"), places: Some(730), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 731, name: "dark_oak_fence_gate", display: Some("Dark Oak Fence Gate"), places: Some(731), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    // W168 - candles (were missing).
    ItemType { id: 732, name: "candle", display: Some("Candle"), places: Some(732), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 733, name: "white_candle", display: Some("White Candle"), places: Some(733), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 734, name: "orange_candle", display: Some("Orange Candle"), places: Some(734), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 735, name: "magenta_candle", display: Some("Magenta Candle"), places: Some(735), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 736, name: "light_blue_candle", display: Some("Light Blue Candle"), places: Some(736), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 737, name: "yellow_candle", display: Some("Yellow Candle"), places: Some(737), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 738, name: "lime_candle", display: Some("Lime Candle"), places: Some(738), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 739, name: "pink_candle", display: Some("Pink Candle"), places: Some(739), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 740, name: "gray_candle", display: Some("Gray Candle"), places: Some(740), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 741, name: "light_gray_candle", display: Some("Light Gray Candle"), places: Some(741), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 742, name: "cyan_candle", display: Some("Cyan Candle"), places: Some(742), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 743, name: "purple_candle", display: Some("Purple Candle"), places: Some(743), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 744, name: "blue_candle", display: Some("Blue Candle"), places: Some(744), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 745, name: "brown_candle", display: Some("Brown Candle"), places: Some(745), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 746, name: "green_candle", display: Some("Green Candle"), places: Some(746), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 747, name: "red_candle", display: Some("Red Candle"), places: Some(747), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 748, name: "black_candle", display: Some("Black Candle"), places: Some(748), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    // W169 - barrel (container, was missing).
    ItemType { id: 749, name: "barrel", display: Some("Barrel"), places: Some(749), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    // W174 - nether-wood trapdoors.
    ItemType { id: 750, name: "crimson_trapdoor", display: Some("Crimson Trapdoor"), places: Some(750), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 751, name: "warped_trapdoor", display: Some("Warped Trapdoor"), places: Some(751), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    // W150: black terracotta + the 16 glazed terracottas.
    ItemType { id: 684, name: "black_terracotta", display: Some("Black Terracotta"), places: Some(684), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 685, name: "white_glazed_terracotta", display: Some("White Glazed Terracotta"), places: Some(685), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 686, name: "orange_glazed_terracotta", display: Some("Orange Glazed Terracotta"), places: Some(686), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 687, name: "magenta_glazed_terracotta", display: Some("Magenta Glazed Terracotta"), places: Some(687), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 688, name: "light_blue_glazed_terracotta", display: Some("Light Blue Glazed Terracotta"), places: Some(688), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 689, name: "yellow_glazed_terracotta", display: Some("Yellow Glazed Terracotta"), places: Some(689), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 690, name: "lime_glazed_terracotta", display: Some("Lime Glazed Terracotta"), places: Some(690), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 691, name: "pink_glazed_terracotta", display: Some("Pink Glazed Terracotta"), places: Some(691), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 692, name: "gray_glazed_terracotta", display: Some("Gray Glazed Terracotta"), places: Some(692), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 693, name: "light_gray_glazed_terracotta", display: Some("Light Gray Glazed Terracotta"), places: Some(693), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 694, name: "cyan_glazed_terracotta", display: Some("Cyan Glazed Terracotta"), places: Some(694), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 695, name: "purple_glazed_terracotta", display: Some("Purple Glazed Terracotta"), places: Some(695), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 696, name: "blue_glazed_terracotta", display: Some("Blue Glazed Terracotta"), places: Some(696), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 697, name: "brown_glazed_terracotta", display: Some("Brown Glazed Terracotta"), places: Some(697), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 698, name: "green_glazed_terracotta", display: Some("Green Glazed Terracotta"), places: Some(698), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 699, name: "red_glazed_terracotta", display: Some("Red Glazed Terracotta"), places: Some(699), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    ItemType { id: 700, name: "black_glazed_terracotta", display: Some("Black Glazed Terracotta"), places: Some(700), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
    // W152: the 16 stained glass panes.
    ItemType { id: 701, name: "white_stained_glass_pane", display: Some("White Stained Glass Pane"), places: Some(701), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 702, name: "orange_stained_glass_pane", display: Some("Orange Stained Glass Pane"), places: Some(702), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 703, name: "magenta_stained_glass_pane", display: Some("Magenta Stained Glass Pane"), places: Some(703), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 704, name: "light_blue_stained_glass_pane", display: Some("Light Blue Stained Glass Pane"), places: Some(704), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 705, name: "yellow_stained_glass_pane", display: Some("Yellow Stained Glass Pane"), places: Some(705), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 706, name: "lime_stained_glass_pane", display: Some("Lime Stained Glass Pane"), places: Some(706), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 707, name: "pink_stained_glass_pane", display: Some("Pink Stained Glass Pane"), places: Some(707), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 708, name: "gray_stained_glass_pane", display: Some("Gray Stained Glass Pane"), places: Some(708), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 709, name: "light_gray_stained_glass_pane", display: Some("Light Gray Stained Glass Pane"), places: Some(709), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 710, name: "cyan_stained_glass_pane", display: Some("Cyan Stained Glass Pane"), places: Some(710), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 711, name: "purple_stained_glass_pane", display: Some("Purple Stained Glass Pane"), places: Some(711), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 712, name: "blue_stained_glass_pane", display: Some("Blue Stained Glass Pane"), places: Some(712), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 713, name: "brown_stained_glass_pane", display: Some("Brown Stained Glass Pane"), places: Some(713), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 714, name: "green_stained_glass_pane", display: Some("Green Stained Glass Pane"), places: Some(714), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 715, name: "red_stained_glass_pane", display: Some("Red Stained Glass Pane"), places: Some(715), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    ItemType { id: 716, name: "black_stained_glass_pane", display: Some("Black Stained Glass Pane"), places: Some(716), max_stack: 64, icon: None, category: ItemCategory::Decoration },
    // W197 Phase 5c - full snow block (id 752). Icon falls through to the
    // block-form TileSpec (snow.png) via the places=Some path.
    ItemType { id: 752, name: "snow_block", display: Some("Snow Block"), places: Some(752), max_stack: 64, icon: None, category: ItemCategory::BuildingBlock },
];

/// W145: mob kind spawned by a spawn-egg item id, `None` for
/// everything else. Drives `RmbAction::SpawnEgg`. Raw `MobKind`
/// discriminants (this module also builds in the lib target where
/// `crate::mob` doesn't exist); the bin tree locks the values
/// against `mob::kinds` in a test.
pub(crate) fn spawn_egg_kind(item_id: u16) -> Option<u8> {
    match item_id {
        677 => Some(2), // chicken
        678 => Some(4), // cow
        679 => Some(3), // pig
        680 => Some(0), // sheep
        681 => Some(1), // zombie
        682 => Some(5), // skeleton
        683 => Some(6), // spider
        717 => Some(7), // wolf (W158)
        720 => Some(8), // polar bear (W161)
        721 => Some(9), // rabbit (W162)
        _ => None,
    }
}

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
///
/// W53.3: applies the MC-strict override on top of the ITEM_TYPES
/// table value so tools/weapons/armor/buckets/beds/boats etc. stack
/// to 1 (and snowballs/eggs/signs/books to 16) regardless of what
/// the table row says. The override is name-pattern driven so newly
/// added items pick up the right cap automatically.
pub(crate) fn item_max_stack(id: u16) -> u8 {
    let Some(it) = item_type(id) else {
        return 64;
    };
    if let Some(n) = mc_stack_override(it.name) {
        return n;
    }
    it.max_stack
}

/// W175: max durability for a tool / weapon, or `None` for items that
/// don't wear out. Type detected by the tool-name shape (matches the
/// stack=1 tool list); tier by the material suffix using MC's values
/// (wood 59, gold 32, stone 131, iron 250, diamond 1561, netherite
/// 2031). Drives the per-slot durability tracker + the break-on-zero.
pub(crate) fn tool_max_durability(item_id: u16) -> Option<u16> {
    let it = item_type(item_id)?;
    let n = it.name;
    let is_tool = n.contains("pickaxe")
        || n.contains("shovel")
        || n.starts_with("axe_")
        || n.ends_with("_axe")
        || n.contains("sword")
        || n.starts_with("hoe_")
        || n.ends_with("_hoe");
    if !is_tool {
        return None;
    }
    let max = if n.ends_with("_wood") || n.ends_with("_wooden") {
        59
    } else if n.ends_with("_gold") || n.ends_with("_golden") {
        32
    } else if n.ends_with("_stone") {
        131
    } else if n.ends_with("_iron") {
        250
    } else if n.ends_with("_diamond") {
        1561
    } else if n.ends_with("_netherite") {
        2031
    } else {
        return None; // unknown tier - treat as unbreakable rather than guess
    };
    Some(max)
}

/// W53.3: return the MC-strict max_stack for items the ITEM_TYPES
/// table is wrong about. `None` = no override (use the table value).
fn mc_stack_override(name: &str) -> Option<u8> {
    // Stack = 1 items: tools, weapons, armor, single-use containers,
    // beds, vehicles, durability items.
    if name.contains("pickaxe")
        || name.contains("shovel")
        || name.starts_with("axe_")
        || name.ends_with("_axe")
        || name.contains("sword")
        || name.starts_with("hoe_")
        || name.ends_with("_hoe")
        || name == "shears"
        || name == "trident"
        || name == "bow"
        || name == "crossbow"
        || name == "fishing_rod"
        || name.ends_with("_helmet")
        || name.ends_with("_chestplate")
        || name.ends_with("_leggings")
        || name.ends_with("_boots")
        || name == "elytra"
        || name == "shield"
        || name == "saddle"
        || name == "flint_and_steel"
        || name == "compass"
        || name == "clock"
        || name == "name_tag"
        || name.contains("horse_armor")
        || name == "lead"
        || name == "carrot_on_a_stick"
        || name == "warped_fungus_on_a_stick"
        || name == "water_bucket"
        || name == "lava_bucket"
        || name == "milk_bucket"
        || name == "powder_snow_bucket"
        || name.starts_with("cod_bucket")
        || name.starts_with("salmon_bucket")
        || name.starts_with("pufferfish_bucket")
        || name.starts_with("tropical_fish_bucket")
        || name.starts_with("axolotl_bucket")
        || name.starts_with("tadpole_bucket")
        || name == "bed"
        || name.ends_with("_bed")
        || name.contains("boat")
        || name.contains("minecart")
        || name == "writable_book"
        || name == "written_book"
        || name == "potion"
        || name == "splash_potion"
        || name == "lingering_potion"
        || name == "milk"
        || name == "totem_of_undying"
        || name == "spyglass"
        || name == "goat_horn"
        || name == "bundle"
        || name == "armor_stand"
        || name == "cake"
        || name == "mushroom_stew"
        || name == "rabbit_stew"
        || name == "beetroot_soup"
        || name == "suspicious_stew"
    {
        return Some(1);
    }
    // Stack = 16 items: thrown projectiles, signs, snowballs, eggs,
    // ender pearls, honey bottles, empty bucket.
    if matches!(name,
        "snowball"
        | "egg"
        | "ender_pearl"
        | "ender_eye"
        | "honey_bottle"
        | "bucket"
        | "sign"
        | "oak_sign"
        | "birch_sign"
        | "spruce_sign"
        | "jungle_sign"
        | "acacia_sign"
        | "dark_oak_sign"
        | "mangrove_sign"
        | "cherry_sign"
        | "bamboo_sign"
        | "crimson_sign"
        | "warped_sign"
        | "oak_hanging_sign"
        | "birch_hanging_sign"
        | "spruce_hanging_sign"
        | "jungle_hanging_sign"
        | "acacia_hanging_sign"
        | "dark_oak_hanging_sign"
        | "mangrove_hanging_sign"
        | "cherry_hanging_sign"
        | "bamboo_hanging_sign"
        | "crimson_hanging_sign"
        | "warped_hanging_sign"
        | "enchanted_book"
        | "book")
    {
        return Some(16);
    }
    None
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

/// W124: extra search keywords per item id. These are ALIASES on
/// top of the localized + English + raw names the search already
/// matches - synonyms, slang, and cross-language hooks so e.g.
/// "bedna" finds the chest regardless of the UI language.
/// Extend freely; matched lowercase by `lang::search_haystack`.
pub(crate) const SEARCH_KEYWORDS: &[(u16, &[&str])] = &[
    (9, &["okno", "window"]),
    (10, &["svetlo", "light", "lampa"]),
    (18, &["krumpac", "pick"]),
    (25, &["mec", "zbran", "blade"]),
    (31, &["lopata", "ryc"]),
    (34, &["sekera", "sekyra", "hatchet"]),
    (35, &["postel", "spanek", "sleep"]),
    (38, &["kbelik", "vedro", "kyblik"]),
    (41, &["truhla", "bedna", "truhlice", "uloziste", "storage"]),
    (42, &["pec", "trouba", "smelt", "taveni"]),
    (77, &["dvere", "vchod"]),
    (162, &["jablko", "jidlo", "food"]),
    (163, &["chleba", "chleb", "jidlo", "food"]),
    (302, &["klacek", "hulka", "tyc"]),
    (553, &["pracovni stul", "ponk", "vyroba", "craft"]),
    (595, &["zebrik", "lezeni", "climb"]),
];

/// W124: keyword aliases for `id` (empty for the un-seeded rest).
pub(crate) fn item_search_keywords(id: u16) -> &'static [&'static str] {
    SEARCH_KEYWORDS
        .iter()
        .find(|(k, _)| *k == id)
        .map(|(_, kws)| *kws)
        .unwrap_or(&[])
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

    /// W175: tools wear out by material tier (MC durabilities);
    /// non-tools are unbreakable (None).
    #[test]
    fn tool_durability_by_tier() {
        // pickaxe_wood = 59, pickaxe_iron = 250, pickaxe_diamond = 1561.
        assert_eq!(tool_max_durability(16), Some(59)); // pickaxe_wood
        assert_eq!(tool_max_durability(18), Some(250)); // pickaxe_iron
        assert_eq!(tool_max_durability(155), Some(1561)); // pickaxe_diamond
        assert_eq!(tool_max_durability(154), Some(32)); // pickaxe_gold
        assert_eq!(tool_max_durability(274), Some(2031)); // pickaxe_netherite
        // A plain block / ingredient never wears out.
        assert_eq!(tool_max_durability(1), None); // stone block
    }

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
