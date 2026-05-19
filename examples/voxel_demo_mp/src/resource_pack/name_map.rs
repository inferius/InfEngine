//! InfEngine block / item id -> MC asset name mapping.
//!
//! Most of our block names already line up with MC's
//! `assets/minecraft/textures/block/<name>.png`, but a handful
//! diverge (`wood` -> `oak_log`, `cobble` -> `cobblestone`,
//! `coal` -> `coal_ore`, ...). Centralising the table here keeps
//! the loader free of one-off conditionals and gives a single
//! place to extend when new blocks land.
//!
//! Resolution order:
//! 1. Per-face override (top / bottom / sides each get their own
//!    MC name if present).
//! 2. The shared block name (used for all faces by default).
//! 3. None - caller falls back to the placeholder tile.

/// Per-block + per-face MC asset names. `top` / `bottom` / `sides`
/// override the shared block name; an empty string means "fall
/// back to the shared name".
#[derive(Debug, Clone, Copy)]
pub(crate) struct BlockNameMap {
    pub(crate) infengine_id: u16,
    pub(crate) block: &'static str,
    pub(crate) top: &'static str,
    pub(crate) bottom: &'static str,
    pub(crate) sides: &'static str,
}

/// Full mapping table. Entries are matched on `infengine_id`.
/// Missing ids fall back to `block_type(id).name`.
pub(crate) const BLOCK_NAME_MAP: &[BlockNameMap] = &[
    // Basic blocks -- most MC names match ours 1:1.
    BlockNameMap { infengine_id: 1,  block: "stone",       top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 2,  block: "dirt",        top: "", bottom: "", sides: "" },
    BlockNameMap {
        infengine_id: 3, block: "grass_block",
        top: "grass_block_top", bottom: "dirt", sides: "grass_block_side",
    },
    BlockNameMap { infengine_id: 4,  block: "sand",        top: "", bottom: "", sides: "" },
    BlockNameMap {
        infengine_id: 5, block: "oak_log",
        top: "oak_log_top", bottom: "oak_log_top", sides: "oak_log",
    },
    BlockNameMap { infengine_id: 6,  block: "oak_leaves",  top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 7,  block: "oak_planks",  top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 8,  block: "cobblestone", top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 9,  block: "glass",       top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 10, block: "torch",       top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 11, block: "white_wool",  top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 12, block: "raw_beef",    top: "", bottom: "", sides: "" }, // item-only, no block tex
    BlockNameMap { infengine_id: 13, block: "coal_ore",    top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 14, block: "iron_ore",    top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 15, block: "snow",        top: "", bottom: "", sides: "" },
    // Flowers
    BlockNameMap { infengine_id: 20, block: "poppy",            top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 21, block: "dandelion",        top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 22, block: "blue_orchid",      top: "", bottom: "", sides: "" },
    // Sandstone / cactus
    BlockNameMap {
        infengine_id: 26, block: "sandstone",
        top: "sandstone_top", bottom: "sandstone_bottom", sides: "sandstone",
    },
    BlockNameMap {
        infengine_id: 27, block: "cactus",
        top: "cactus_top", bottom: "cactus_bottom", sides: "cactus_side",
    },
    BlockNameMap { infengine_id: 28, block: "glowstone", top: "", bottom: "", sides: "" },
    BlockNameMap {
        infengine_id: 35, block: "red_bed",
        top: "red_bed_top", bottom: "oak_planks", sides: "red_bed_side",
    },
    // Liquids - MC's animated names. `water_still` + `lava_still`
    // both ship as 16x(16*N) vertical strips with `.mcmeta` siblings.
    BlockNameMap {
        infengine_id: 36, block: "water_still",
        top: "water_still", bottom: "water_still", sides: "water_still",
    },
    BlockNameMap {
        infengine_id: 37, block: "lava_still",
        top: "lava_still", bottom: "lava_still", sides: "lava_still",
    },
    // Chest + furnace - real MC ships these via entity models. For
    // a flat block-face fallback we point at oak_planks (chest) and
    // furnace_top / furnace_front / furnace_side.
    BlockNameMap {
        infengine_id: 41, block: "chest",
        top: "oak_planks", bottom: "oak_planks", sides: "oak_planks",
    },
    BlockNameMap {
        infengine_id: 42, block: "furnace",
        top: "furnace_top", bottom: "furnace_top", sides: "furnace_front",
    },
    // Nether + portal blocks.
    BlockNameMap { infengine_id: 44, block: "netherrack", top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 45, block: "bedrock",    top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 46, block: "obsidian",   top: "", bottom: "", sides: "" },
    BlockNameMap {
        infengine_id: 47, block: "nether_portal",
        top: "nether_portal", bottom: "nether_portal", sides: "nether_portal",
    },
    // Fire layer - the animated overlay texture name.
    BlockNameMap {
        infengine_id: 49, block: "fire_0",
        top: "fire_0", bottom: "fire_0", sides: "fire_0",
    },
];

/// Look up the (block, top, bottom, sides) MC names for an infengine
/// block id. Empty per-face strings mean "fall back to `block`".
pub(crate) fn lookup_block(id: u16) -> Option<&'static BlockNameMap> {
    BLOCK_NAME_MAP.iter().find(|m| m.infengine_id == id)
}

/// Per-item MC asset name override for non-block items (tools,
/// buckets, ingots, food). Block-form items reuse the block face
/// texture; everything else lives here.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ItemNameMap {
    pub(crate) infengine_id: u16,
    pub(crate) item: &'static str,
}

pub(crate) const ITEM_NAME_MAP: &[ItemNameMap] = &[
    // Meat / food.
    ItemNameMap { infengine_id: 12, item: "beef" },
    // Pickaxes
    ItemNameMap { infengine_id: 16, item: "wooden_pickaxe" },
    ItemNameMap { infengine_id: 17, item: "stone_pickaxe" },
    ItemNameMap { infengine_id: 18, item: "iron_pickaxe" },
    // Feather
    ItemNameMap { infengine_id: 19, item: "feather" },
    // Swords
    ItemNameMap { infengine_id: 23, item: "wooden_sword" },
    ItemNameMap { infengine_id: 24, item: "stone_sword" },
    ItemNameMap { infengine_id: 25, item: "iron_sword" },
    // Shovels
    ItemNameMap { infengine_id: 29, item: "wooden_shovel" },
    ItemNameMap { infengine_id: 30, item: "stone_shovel" },
    ItemNameMap { infengine_id: 31, item: "iron_shovel" },
    // Axes
    ItemNameMap { infengine_id: 32, item: "wooden_axe" },
    ItemNameMap { infengine_id: 33, item: "stone_axe" },
    ItemNameMap { infengine_id: 34, item: "iron_axe" },
    // Buckets
    ItemNameMap { infengine_id: 38, item: "bucket" },
    ItemNameMap { infengine_id: 39, item: "water_bucket" },
    ItemNameMap { infengine_id: 40, item: "lava_bucket" },
    // Ingot
    ItemNameMap { infengine_id: 43, item: "iron_ingot" },
    // Flint and steel
    ItemNameMap { infengine_id: 48, item: "flint_and_steel" },
];

pub(crate) fn lookup_item(id: u16) -> Option<&'static str> {
    ITEM_NAME_MAP.iter().find(|m| m.infengine_id == id).map(|m| m.item)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_map_covers_common_ids() {
        assert!(lookup_block(1).is_some());
        assert!(lookup_block(3).is_some());
        assert_eq!(lookup_block(3).unwrap().top, "grass_block_top");
    }

    #[test]
    fn item_map_resolves_tools_and_buckets() {
        assert_eq!(lookup_item(16), Some("wooden_pickaxe"));
        assert_eq!(lookup_item(38), Some("bucket"));
        assert_eq!(lookup_item(48), Some("flint_and_steel"));
    }

    #[test]
    fn unknown_ids_return_none() {
        assert!(lookup_block(9999).is_none());
        assert!(lookup_item(9999).is_none());
    }
}
