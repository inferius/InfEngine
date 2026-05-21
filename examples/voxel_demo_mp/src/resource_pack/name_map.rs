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
    // W36.2: MC filename is "torch_on.png" (lit torch); "torch.png"
    // is the pre-1.13 name, not present in modern packs.
    BlockNameMap { infengine_id: 10, block: "torch_on", top: "", bottom: "", sides: "" },
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
    // W11.3 emitter blocks (ids 50..=67).
    // Torches: MC uses the same face texture for all sides.
    BlockNameMap { infengine_id: 50, block: "redstone_torch", top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 51, block: "redstone_torch", top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 52, block: "soul_torch",     top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 53, block: "soul_torch",     top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 54, block: "sea_lantern",    top: "", bottom: "", sides: "" },
    BlockNameMap {
        infengine_id: 55, block: "jack_o_lantern",
        top: "pumpkin_top", bottom: "pumpkin_top", sides: "jack_o_lantern",
    },
    BlockNameMap { infengine_id: 56, block: "soul_fire_0",   top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 57, block: "end_rod",        top: "end_rod_top", bottom: "end_rod_top", sides: "end_rod" },
    BlockNameMap { infengine_id: 58, block: "magma",          top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 59, block: "shroomlight",    top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 60, block: "lantern",        top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 61, block: "soul_lantern",   top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 62, block: "crying_obsidian", top: "", bottom: "", sides: "" },
    BlockNameMap {
        infengine_id: 63, block: "respawn_anchor_top",
        top: "respawn_anchor_top", bottom: "respawn_anchor_bottom", sides: "respawn_anchor_side0",
    },
    // Ender chest - MC ships it as an entity model; use obsidian as
    // a reasonable flat-face stand-in.
    BlockNameMap {
        infengine_id: 64, block: "obsidian",
        top: "obsidian", bottom: "obsidian", sides: "obsidian",
    },
    BlockNameMap { infengine_id: 65, block: "redstone_lamp_on",  top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 66, block: "redstone_ore",      top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 67, block: "light_block_15",    top: "", bottom: "", sides: "" },
    // W12.2 slab / stair / fence / door / pane / bars / wall
    // (ids 68..=82). For shapes that have no distinct top texture,
    // map to the full-block parent texture.
    BlockNameMap { infengine_id: 68, block: "oak_planks",       top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 69, block: "stone",            top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 70, block: "cobblestone",      top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 71, block: "oak_planks",       top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 72, block: "oak_planks",       top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 73, block: "stone",            top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 74, block: "cobblestone",      top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 75, block: "oak_planks",       top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 76, block: "nether_bricks",    top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 80, block: "glass",            top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 81, block: "iron_bars",        top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 82, block: "cobblestone",      top: "", bottom: "", sides: "" },
    // W35.3 - Stone variants + wood variants + wool + building blocks.
    BlockNameMap { infengine_id: 83, block: "granite",          top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 84, block: "diorite",          top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 85, block: "andesite",         top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 86, block: "bricks",           top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 87, block: "smooth_stone",     top: "", bottom: "", sides: "" },
    BlockNameMap {
        infengine_id: 88, block: "birch_log",
        top: "birch_log_top", bottom: "birch_log_top", sides: "birch_log",
    },
    BlockNameMap { infengine_id: 89, block: "birch_planks",     top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 90, block: "birch_leaves",     top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 91, block: "blue_wool",        top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 92, block: "red_wool",         top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 93, block: "yellow_wool",      top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 94, block: "green_wool",       top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 95, block: "tuff",             top: "", bottom: "", sides: "" },
    BlockNameMap {
        infengine_id: 96, block: "deepslate",
        top: "deepslate_top", bottom: "deepslate_top", sides: "deepslate",
    },
    BlockNameMap { infengine_id: 97, block: "calcite",          top: "", bottom: "", sides: "" },
    // W37.2 - wool colour variants (ids 98..=108).
    BlockNameMap { infengine_id: 98,  block: "orange_wool",     top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 99,  block: "magenta_wool",    top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 100, block: "light_blue_wool", top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 101, block: "lime_wool",       top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 102, block: "pink_wool",       top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 103, block: "gray_wool",       top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 104, block: "light_gray_wool", top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 105, block: "cyan_wool",       top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 106, block: "purple_wool",     top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 107, block: "brown_wool",      top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 108, block: "black_wool",      top: "", bottom: "", sides: "" },
    // Stone family extras (ids 109..=116).
    BlockNameMap { infengine_id: 109, block: "mossy_cobblestone",    top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 110, block: "stone_bricks",         top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 111, block: "mossy_stone_bricks",   top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 112, block: "cracked_stone_bricks", top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 113, block: "chiseled_stone_bricks",top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 114, block: "polished_granite",     top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 115, block: "polished_diorite",     top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 116, block: "polished_andesite",    top: "", bottom: "", sides: "" },
    // Ores (ids 117..=126).
    BlockNameMap { infengine_id: 117, block: "gold_ore",               top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 118, block: "diamond_ore",            top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 119, block: "redstone_ore",           top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 120, block: "lapis_ore",              top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 121, block: "emerald_ore",            top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 122, block: "copper_ore",             top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 123, block: "deepslate_coal_ore",     top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 124, block: "deepslate_iron_ore",     top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 125, block: "deepslate_diamond_ore",  top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 126, block: "deepslate_gold_ore",     top: "", bottom: "", sides: "" },
    // Wood variants: acacia (ids 127..=129).
    BlockNameMap {
        infengine_id: 127, block: "acacia_log",
        top: "acacia_log_top", bottom: "acacia_log_top", sides: "acacia_log",
    },
    BlockNameMap { infengine_id: 128, block: "acacia_planks",  top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 129, block: "acacia_leaves",  top: "", bottom: "", sides: "" },
    // Spruce (ids 130..=132).
    BlockNameMap {
        infengine_id: 130, block: "spruce_log",
        top: "spruce_log_top", bottom: "spruce_log_top", sides: "spruce_log",
    },
    BlockNameMap { infengine_id: 131, block: "spruce_planks",  top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 132, block: "spruce_leaves",  top: "", bottom: "", sides: "" },
    // Jungle (ids 133..=135).
    BlockNameMap {
        infengine_id: 133, block: "jungle_log",
        top: "jungle_log_top", bottom: "jungle_log_top", sides: "jungle_log",
    },
    BlockNameMap { infengine_id: 134, block: "jungle_planks",  top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 135, block: "jungle_leaves",  top: "", bottom: "", sides: "" },
    // Dark oak (ids 136..=138).
    BlockNameMap {
        infengine_id: 136, block: "dark_oak_log",
        top: "dark_oak_log_top", bottom: "dark_oak_log_top", sides: "dark_oak_log",
    },
    BlockNameMap { infengine_id: 137, block: "dark_oak_planks", top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 138, block: "dark_oak_leaves", top: "", bottom: "", sides: "" },
    // Flowers (ids 139..=147).
    BlockNameMap { infengine_id: 139, block: "allium",             top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 140, block: "azure_bluet",        top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 141, block: "red_tulip",          top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 142, block: "orange_tulip",       top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 143, block: "white_tulip",        top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 144, block: "pink_tulip",         top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 145, block: "oxeye_daisy",        top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 146, block: "cornflower",         top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 147, block: "lily_of_the_valley", top: "", bottom: "", sides: "" },
    // Misc terrain (ids 148..=153).
    BlockNameMap { infengine_id: 148, block: "end_stone",          top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 149, block: "soul_sand",          top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 150, block: "soul_soil",          top: "", bottom: "", sides: "" },
    BlockNameMap {
        infengine_id: 151, block: "bone_block",
        top: "bone_block_top", bottom: "bone_block_top", sides: "bone_block_side",
    },
    BlockNameMap {
        infengine_id: 152, block: "mycelium",
        top: "mycelium_top", bottom: "dirt", sides: "mycelium_side",
    },
    BlockNameMap {
        infengine_id: 153, block: "podzol",
        top: "podzol_top", bottom: "dirt", sides: "podzol_side",
    },
    // Gold + diamond tools (ids 154..=161). No block texture; icons
    // resolve via ITEM_NAME_MAP to MC item sprites.
    BlockNameMap { infengine_id: 154, block: "golden_pickaxe",  top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 155, block: "diamond_pickaxe", top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 156, block: "golden_sword",    top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 157, block: "diamond_sword",   top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 158, block: "golden_shovel",   top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 159, block: "diamond_shovel",  top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 160, block: "golden_axe",      top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 161, block: "diamond_axe",     top: "", bottom: "", sides: "" },
    // Food items (ids 162..=168). Block face resolves via ITEM_NAME_MAP.
    BlockNameMap { infengine_id: 162, block: "apple",           top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 163, block: "bread",           top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 164, block: "cooked_beef",     top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 165, block: "chicken_raw",     top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 166, block: "chicken_cooked",  top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 167, block: "cookie",          top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 168, block: "melon_slice",     top: "", bottom: "", sides: "" },
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
    // W11.3 emitter items - torches use item texture not block face.
    ItemNameMap { infengine_id: 50, item: "redstone_torch" },
    ItemNameMap { infengine_id: 51, item: "redstone_torch" },
    ItemNameMap { infengine_id: 52, item: "soul_torch" },
    ItemNameMap { infengine_id: 53, item: "soul_torch" },
    ItemNameMap { infengine_id: 60, item: "lantern" },
    ItemNameMap { infengine_id: 61, item: "soul_lantern" },
    ItemNameMap { infengine_id: 57, item: "end_rod" },
    // W12.2 door items - MC ships doors as flat item sprites.
    ItemNameMap { infengine_id: 77, item: "oak_door" },
    ItemNameMap { infengine_id: 78, item: "oak_door" },
    ItemNameMap { infengine_id: 79, item: "iron_door" },
    // W37.2 gold + diamond tools.
    ItemNameMap { infengine_id: 154, item: "golden_pickaxe" },
    ItemNameMap { infengine_id: 155, item: "diamond_pickaxe" },
    ItemNameMap { infengine_id: 156, item: "golden_sword" },
    ItemNameMap { infengine_id: 157, item: "diamond_sword" },
    ItemNameMap { infengine_id: 158, item: "golden_shovel" },
    ItemNameMap { infengine_id: 159, item: "diamond_shovel" },
    ItemNameMap { infengine_id: 160, item: "golden_axe" },
    ItemNameMap { infengine_id: 161, item: "diamond_axe" },
    // W37.2 food item sprites.
    ItemNameMap { infengine_id: 162, item: "apple" },
    ItemNameMap { infengine_id: 163, item: "bread" },
    ItemNameMap { infengine_id: 164, item: "cooked_beef" },
    ItemNameMap { infengine_id: 165, item: "chicken_raw" },
    ItemNameMap { infengine_id: 166, item: "chicken_cooked" },
    ItemNameMap { infengine_id: 167, item: "cookie" },
    ItemNameMap { infengine_id: 168, item: "melon_slice" },
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

    // W34.1 - new emitter block entries (ids 50..=67).
    #[test]
    fn w34_emitter_blocks_in_map() {
        assert_eq!(lookup_block(50).map(|m| m.block), Some("redstone_torch"));
        assert_eq!(lookup_block(51).map(|m| m.block), Some("redstone_torch"));
        assert_eq!(lookup_block(52).map(|m| m.block), Some("soul_torch"));
        assert_eq!(lookup_block(53).map(|m| m.block), Some("soul_torch"));
        assert_eq!(lookup_block(54).map(|m| m.block), Some("sea_lantern"));
        assert_eq!(lookup_block(55).map(|m| m.block), Some("jack_o_lantern"));
        assert_eq!(lookup_block(56).map(|m| m.block), Some("soul_fire_0"));
        assert_eq!(lookup_block(57).map(|m| m.block), Some("end_rod"));
        assert_eq!(lookup_block(58).map(|m| m.block), Some("magma"));
        assert_eq!(lookup_block(59).map(|m| m.block), Some("shroomlight"));
        assert_eq!(lookup_block(60).map(|m| m.block), Some("lantern"));
        assert_eq!(lookup_block(61).map(|m| m.block), Some("soul_lantern"));
        assert_eq!(lookup_block(62).map(|m| m.block), Some("crying_obsidian"));
        assert_eq!(lookup_block(63).map(|m| m.top),   Some("respawn_anchor_top"));
        assert_eq!(lookup_block(65).map(|m| m.block), Some("redstone_lamp_on"));
        assert_eq!(lookup_block(66).map(|m| m.block), Some("redstone_ore"));
        assert_eq!(lookup_block(67).map(|m| m.block), Some("light_block_15"));
    }

    // W34.1 - new slab/stair/fence/door/pane entries (ids 68..=82).
    #[test]
    fn w34_shape_blocks_in_map() {
        assert_eq!(lookup_block(68).map(|m| m.block), Some("oak_planks"));
        assert_eq!(lookup_block(69).map(|m| m.block), Some("stone"));
        assert_eq!(lookup_block(70).map(|m| m.block), Some("cobblestone"));
        assert_eq!(lookup_block(71).map(|m| m.block), Some("oak_planks"));
        assert_eq!(lookup_block(72).map(|m| m.block), Some("oak_planks"));
        assert_eq!(lookup_block(73).map(|m| m.block), Some("stone"));
        assert_eq!(lookup_block(74).map(|m| m.block), Some("cobblestone"));
        assert_eq!(lookup_block(75).map(|m| m.block), Some("oak_planks"));
        assert_eq!(lookup_block(76).map(|m| m.block), Some("nether_bricks"));
        assert_eq!(lookup_block(80).map(|m| m.block), Some("glass"));
        assert_eq!(lookup_block(81).map(|m| m.block), Some("iron_bars"));
        assert_eq!(lookup_block(82).map(|m| m.block), Some("cobblestone"));
    }

    // W34.1 - torch / door item overrides.
    #[test]
    fn w34_item_overrides() {
        assert_eq!(lookup_item(50), Some("redstone_torch"));
        assert_eq!(lookup_item(51), Some("redstone_torch"));
        assert_eq!(lookup_item(52), Some("soul_torch"));
        assert_eq!(lookup_item(53), Some("soul_torch"));
        assert_eq!(lookup_item(57), Some("end_rod"));
        assert_eq!(lookup_item(60), Some("lantern"));
        assert_eq!(lookup_item(61), Some("soul_lantern"));
        assert_eq!(lookup_item(77), Some("oak_door"));
        assert_eq!(lookup_item(78), Some("oak_door"));
        assert_eq!(lookup_item(79), Some("iron_door"));
    }

    // W34.1 - aggregate: at least 70 of 82 ITEM_TYPES entries resolve
    // through either lookup_block or lookup_item.
    #[test]
    fn w34_name_map_covers_70_of_82() {
        // All IDs present in ITEM_TYPES (ids 1..=82, skipping none).
        let item_ids: Vec<u16> = (1u16..=82).collect();
        let resolved = item_ids
            .iter()
            .filter(|&&id| lookup_item(id).is_some() || lookup_block(id).is_some())
            .count();
        assert!(
            resolved >= 70,
            "Only {resolved} of {} IDs resolve through name_map (need >= 70)",
            item_ids.len()
        );
    }

    // W37.2 - spot-check new block entries (wool, ores, wood, terrain).
    #[test]
    fn w37_new_blocks_in_name_map() {
        assert_eq!(lookup_block(98).map(|m| m.block),  Some("orange_wool"));
        assert_eq!(lookup_block(108).map(|m| m.block), Some("black_wool"));
        assert_eq!(lookup_block(110).map(|m| m.block), Some("stone_bricks"));
        assert_eq!(lookup_block(118).map(|m| m.block), Some("diamond_ore"));
        assert_eq!(lookup_block(127).map(|m| m.block), Some("acacia_log"));
        assert_eq!(lookup_block(127).map(|m| m.top),   Some("acacia_log_top"));
        assert_eq!(lookup_block(130).map(|m| m.top),   Some("spruce_log_top"));
        assert_eq!(lookup_block(145).map(|m| m.block), Some("oxeye_daisy"));
        assert_eq!(lookup_block(151).map(|m| m.top),   Some("bone_block_top"));
        assert_eq!(lookup_block(152).map(|m| m.top),   Some("mycelium_top"));
        assert_eq!(lookup_block(162).map(|m| m.block), Some("apple"));
    }

    // W37.2 - spot-check new tool + food item-name overrides.
    #[test]
    fn w37_new_item_overrides() {
        assert_eq!(lookup_item(154), Some("golden_pickaxe"));
        assert_eq!(lookup_item(155), Some("diamond_pickaxe"));
        assert_eq!(lookup_item(156), Some("golden_sword"));
        assert_eq!(lookup_item(157), Some("diamond_sword"));
        assert_eq!(lookup_item(158), Some("golden_shovel"));
        assert_eq!(lookup_item(159), Some("diamond_shovel"));
        assert_eq!(lookup_item(160), Some("golden_axe"));
        assert_eq!(lookup_item(161), Some("diamond_axe"));
        assert_eq!(lookup_item(162), Some("apple"));
        assert_eq!(lookup_item(163), Some("bread"));
        assert_eq!(lookup_item(164), Some("cooked_beef"));
        assert_eq!(lookup_item(165), Some("chicken_raw"));
        assert_eq!(lookup_item(168), Some("melon_slice"));
    }
}
