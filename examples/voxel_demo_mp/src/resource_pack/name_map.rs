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
    // W37 fix: MC 26.x ships block/torch.png (lit, default). The W36.2
    // "torch_on" rename was based on a pre-1.13 era filename that no
    // longer applies - reverting to "torch" so icon lookup hits the
    // real tile.
    BlockNameMap { infengine_id: 10, block: "torch", top: "", bottom: "", sides: "" },
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
    // Bed - MC ships as entity model; no block face textures exist.
    // Use oak_planks as a flat-face fallback for all faces.
    // W45.1: removed nonexistent "red_bed_top"/"red_bed_side" references.
    BlockNameMap {
        infengine_id: 35, block: "oak_planks",
        top: "", bottom: "", sides: "",
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
    // W46.21: copper torch family (MC 1.21+).
    // W133: the wall id maps to the BASE torch texture like rows
    // 51/53 - MC ships no copper_wall_torch.png (wall-torch models
    // reference copper_torch.png; orientation comes from the
    // dedicated torch render path).
    BlockNameMap { infengine_id: 351, block: "copper_torch", top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 352, block: "copper_torch", top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 54, block: "sea_lantern",    top: "", bottom: "", sides: "" },
    BlockNameMap {
        infengine_id: 55, block: "jack_o_lantern",
        top: "pumpkin_top", bottom: "pumpkin_top", sides: "jack_o_lantern",
    },
    BlockNameMap { infengine_id: 56, block: "soul_fire_0",   top: "", bottom: "", sides: "" },
    // W45.1: end_rod_top.png does not exist in MC 26.1.2; single "end_rod"
    // texture covers all faces. The JSON model (block/end_rod.json) references
    // only "#end_rod" throughout.
    BlockNameMap { infengine_id: 57, block: "end_rod",        top: "end_rod", bottom: "end_rod", sides: "end_rod" },
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
    // W45.1: light_block_15.png does not exist; MC 26.1.2 ships light blocks
    // as invisible blocks whose model points to an item sprite. Using
    // "glowstone" as the nearest visible flat-face fallback.
    BlockNameMap { infengine_id: 67, block: "glowstone",         top: "", bottom: "", sides: "" },
    // W12.2 / W45.1 slab / stair / fence / door / pane / bars / wall
    // (ids 68..=82). "block" field now holds the actual MC blockstate name
    // so the universal JSON pipeline (W45.5) can load the correct model.
    // The per-face top/bottom/sides fields are kept for the fallback cube
    // path when no MC pack is present.
    BlockNameMap { infengine_id: 68, block: "oak_slab",          top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 69, block: "stone_slab",         top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 70, block: "cobblestone_slab",   top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 71, block: "oak_slab",           top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 72, block: "oak_stairs",         top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 73, block: "stone_stairs",       top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 74, block: "cobblestone_stairs",  top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 75, block: "oak_fence",          top: "", bottom: "", sides: "" },
    // W131: W48 fence variants - the MC multipart blockstate grows
    // the post + arms; names match MC exactly.
    BlockNameMap { infengine_id: 543, block: "birch_fence",       top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 544, block: "spruce_fence",      top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 545, block: "jungle_fence",      top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 546, block: "acacia_fence",      top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 547, block: "dark_oak_fence",    top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 76, block: "nether_brick_fence",  top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 80, block: "glass_pane",          top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 81, block: "iron_bars",           top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 82, block: "cobblestone_wall",    top: "", bottom: "", sides: "" },
    // W261 - the ancient-city deepslate shapes (JSON pipeline by name).
    BlockNameMap { infengine_id: 799, block: "deepslate_brick_stairs",     top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 800, block: "deepslate_brick_slab",       top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 801, block: "deepslate_brick_wall",       top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 802, block: "deepslate_tile_stairs",      top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 803, block: "deepslate_tile_slab",        top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 804, block: "deepslate_tile_wall",        top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 805, block: "polished_deepslate_stairs",  top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 806, block: "polished_deepslate_slab",    top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 807, block: "polished_deepslate_wall",    top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 808, block: "cobbled_deepslate_stairs",   top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 809, block: "cobbled_deepslate_slab",     top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 810, block: "cobbled_deepslate_wall",     top: "", bottom: "", sides: "" },
    // W263 - trial-chambers shapes (JSON pipeline by name).
    BlockNameMap { infengine_id: 816, block: "polished_tuff_slab",         top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 823, block: "cut_copper_stairs",          top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 824, block: "oxidized_cut_copper_stairs", top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 825, block: "oxidized_cut_copper_slab",   top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 826, block: "copper_door",                top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 827, block: "oxidized_copper_door",       top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 828, block: "copper_trapdoor",            top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 829, block: "oxidized_copper_trapdoor",   top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 832, block: "cut_copper_slab",            top: "", bottom: "", sides: "" },
    // W270 - bastion blackstone/quartz shapes (JSON pipeline by name).
    BlockNameMap { infengine_id: 839, block: "blackstone_stairs",          top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 840, block: "blackstone_slab",            top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 841, block: "blackstone_wall",            top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 842, block: "polished_blackstone_brick_stairs", top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 843, block: "smooth_quartz_slab",         top: "", bottom: "", sides: "" },
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
    // W40.2: Stained glass (ids 169..=184).
    BlockNameMap { infengine_id: 169, block: "white_stained_glass",       top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 170, block: "orange_stained_glass",      top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 171, block: "magenta_stained_glass",     top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 172, block: "light_blue_stained_glass",  top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 173, block: "yellow_stained_glass",      top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 174, block: "lime_stained_glass",        top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 175, block: "pink_stained_glass",        top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 176, block: "gray_stained_glass",        top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 177, block: "light_gray_stained_glass",  top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 178, block: "cyan_stained_glass",        top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 179, block: "purple_stained_glass",      top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 180, block: "blue_stained_glass",        top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 181, block: "brown_stained_glass",       top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 182, block: "green_stained_glass",       top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 183, block: "red_stained_glass",         top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 184, block: "black_stained_glass",       top: "", bottom: "", sides: "" },
    // W443: stained glass PANES (701..=716). Without these rows the
    // panes fell to the static hand-coded thin pane - no multipart
    // arms, one fixed orientation (the R32 yellow-pane report). With
    // the blockstate JSON they connect like plain glass_pane (80).
    BlockNameMap { infengine_id: 701, block: "white_stained_glass_pane",      top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 702, block: "orange_stained_glass_pane",     top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 703, block: "magenta_stained_glass_pane",    top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 704, block: "light_blue_stained_glass_pane", top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 705, block: "yellow_stained_glass_pane",     top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 706, block: "lime_stained_glass_pane",       top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 707, block: "pink_stained_glass_pane",       top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 708, block: "gray_stained_glass_pane",       top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 709, block: "light_gray_stained_glass_pane", top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 710, block: "cyan_stained_glass_pane",       top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 711, block: "purple_stained_glass_pane",     top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 712, block: "blue_stained_glass_pane",       top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 713, block: "brown_stained_glass_pane",      top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 714, block: "green_stained_glass_pane",      top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 715, block: "red_stained_glass_pane",        top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 716, block: "black_stained_glass_pane",      top: "", bottom: "", sides: "" },
    // W40.2: Concrete (ids 185..=200).
    BlockNameMap { infengine_id: 185, block: "white_concrete",       top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 186, block: "orange_concrete",      top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 187, block: "magenta_concrete",     top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 188, block: "light_blue_concrete",  top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 189, block: "yellow_concrete",      top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 190, block: "lime_concrete",        top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 191, block: "pink_concrete",        top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 192, block: "gray_concrete",        top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 193, block: "light_gray_concrete",  top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 194, block: "cyan_concrete",        top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 195, block: "purple_concrete",      top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 196, block: "blue_concrete",        top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 197, block: "brown_concrete",       top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 198, block: "green_concrete",       top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 199, block: "red_concrete",         top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 200, block: "black_concrete",       top: "", bottom: "", sides: "" },
    // W40.2: Concrete powder (ids 201..=216).
    BlockNameMap { infengine_id: 201, block: "white_concrete_powder",       top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 202, block: "orange_concrete_powder",      top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 203, block: "magenta_concrete_powder",     top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 204, block: "light_blue_concrete_powder",  top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 205, block: "yellow_concrete_powder",      top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 206, block: "lime_concrete_powder",        top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 207, block: "pink_concrete_powder",        top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 208, block: "gray_concrete_powder",        top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 209, block: "light_gray_concrete_powder",  top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 210, block: "cyan_concrete_powder",        top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 211, block: "purple_concrete_powder",      top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 212, block: "blue_concrete_powder",        top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 213, block: "brown_concrete_powder",       top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 214, block: "green_concrete_powder",       top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 215, block: "red_concrete_powder",         top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 216, block: "black_concrete_powder",       top: "", bottom: "", sides: "" },
    // W40.2: Carpet (ids 217..=232). MC name = color + "_carpet".
    BlockNameMap { infengine_id: 217, block: "white_carpet",      top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 218, block: "orange_carpet",     top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 219, block: "magenta_carpet",    top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 220, block: "light_blue_carpet", top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 221, block: "yellow_carpet",     top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 222, block: "lime_carpet",       top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 223, block: "pink_carpet",       top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 224, block: "gray_carpet",       top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 225, block: "light_gray_carpet", top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 226, block: "cyan_carpet",       top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 227, block: "purple_carpet",     top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 228, block: "blue_carpet",       top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 229, block: "brown_carpet",      top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 230, block: "green_carpet",      top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 231, block: "red_carpet",        top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 232, block: "black_carpet",      top: "", bottom: "", sides: "" },
    // W40.2: Mangrove wood (ids 233..=236).
    BlockNameMap { infengine_id: 233, block: "mangrove_log",   top: "mangrove_log_top", bottom: "mangrove_log_top", sides: "mangrove_log" },
    BlockNameMap { infengine_id: 234, block: "mangrove_planks", top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 235, block: "mangrove_leaves", top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 236, block: "mangrove_roots",  top: "mangrove_roots_top", bottom: "mangrove_roots_top", sides: "mangrove_roots_side" },
    // W40.2: Cherry wood (ids 237..=239).
    BlockNameMap { infengine_id: 237, block: "cherry_log",    top: "cherry_log_top", bottom: "cherry_log_top", sides: "cherry_log" },
    BlockNameMap { infengine_id: 238, block: "cherry_planks", top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 239, block: "cherry_leaves", top: "", bottom: "", sides: "" },
    // W40.2: Crimson wood (ids 240..=242).
    BlockNameMap { infengine_id: 240, block: "crimson_stem",   top: "crimson_stem_top", bottom: "crimson_stem_top", sides: "crimson_stem" },
    BlockNameMap { infengine_id: 241, block: "crimson_planks", top: "", bottom: "", sides: "" },
    // W45.1: crimson_hyphae.png does not exist in MC 26.1.2. The JSON model
    // (crimson_hyphae.json) uses crimson_stem for all faces.
    BlockNameMap { infengine_id: 242, block: "crimson_stem",    top: "crimson_stem_top", bottom: "crimson_stem_top", sides: "crimson_stem" },
    // W40.2: Warped wood (ids 243..=245).
    BlockNameMap { infengine_id: 243, block: "warped_stem",   top: "warped_stem_top", bottom: "warped_stem_top", sides: "warped_stem" },
    BlockNameMap { infengine_id: 244, block: "warped_planks", top: "", bottom: "", sides: "" },
    // W45.1: warped_hyphae.png does not exist in MC 26.1.2. The JSON model
    // (warped_hyphae.json) uses warped_stem for all faces.
    BlockNameMap { infengine_id: 245, block: "warped_stem",     top: "warped_stem_top", bottom: "warped_stem_top", sides: "warped_stem" },
    // W40.2: Deepslate ores (ids 246..=249).
    BlockNameMap { infengine_id: 246, block: "deepslate_redstone_ore", top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 247, block: "deepslate_lapis_ore",    top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 248, block: "deepslate_emerald_ore",  top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 249, block: "deepslate_copper_ore",   top: "", bottom: "", sides: "" },
    // W40.2: Nether ores (ids 250..=251).
    BlockNameMap { infengine_id: 250, block: "nether_quartz_ore", top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 251, block: "nether_gold_ore",   top: "", bottom: "", sides: "" },
    // W40.2: Ancient debris (id 252).
    BlockNameMap { infengine_id: 252, block: "ancient_debris", top: "ancient_debris_top", bottom: "ancient_debris_top", sides: "ancient_debris_side" },
    // W40.2: Nether building blocks (ids 253..=258).
    BlockNameMap { infengine_id: 253, block: "nether_bricks",        top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 254, block: "red_nether_bricks",    top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 255, block: "blackstone",           top: "blackstone_top", bottom: "blackstone_top", sides: "blackstone" },
    BlockNameMap { infengine_id: 256, block: "polished_blackstone",  top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 257, block: "basalt",               top: "basalt_top", bottom: "basalt_top", sides: "basalt_side" },
    BlockNameMap { infengine_id: 258, block: "smooth_basalt",        top: "", bottom: "", sides: "" },
    // W40.2: End blocks (ids 259..=262).
    BlockNameMap { infengine_id: 259, block: "end_stone_bricks", top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 260, block: "purpur_block",     top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 261, block: "purpur_pillar",    top: "purpur_pillar_top", bottom: "purpur_pillar_top", sides: "purpur_pillar" },
    BlockNameMap { infengine_id: 262, block: "dragon_egg",       top: "", bottom: "", sides: "" },
    // W40.2: Prismarine family (ids 263..=265).
    BlockNameMap { infengine_id: 263, block: "prismarine",        top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 264, block: "prismarine_bricks", top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 265, block: "dark_prismarine",   top: "", bottom: "", sides: "" },
    // W40.2: Copper blocks (ids 266..=270).
    BlockNameMap { infengine_id: 266, block: "copper_block",     top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 267, block: "exposed_copper",   top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 268, block: "weathered_copper", top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 269, block: "oxidized_copper",  top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 270, block: "cut_copper",       top: "", bottom: "", sides: "" },
    // W40.2: Amethyst (ids 271..=273).
    BlockNameMap { infengine_id: 271, block: "amethyst_block",   top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 272, block: "budding_amethyst", top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 273, block: "amethyst_cluster", top: "", bottom: "", sides: "" },
    // W40.2: Netherite tools (ids 274..=278). Item sprites via ITEM_NAME_MAP.
    BlockNameMap { infengine_id: 274, block: "netherite_pickaxe", top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 275, block: "netherite_sword",   top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 276, block: "netherite_shovel",  top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 277, block: "netherite_axe",     top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 278, block: "netherite_hoe",     top: "", bottom: "", sides: "" },
    // W40.2: Netherite ingot + scrap (ids 279..=280).
    BlockNameMap { infengine_id: 279, block: "netherite_ingot", top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 280, block: "netherite_scrap",  top: "", bottom: "", sides: "" },
    // W40.2: More food items (ids 281..=292). Sprites via ITEM_NAME_MAP.
    BlockNameMap { infengine_id: 281, block: "pumpkin_pie",     top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 282, block: "golden_apple",    top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 283, block: "baked_potato",    top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 284, block: "porkchop_raw",    top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 285, block: "porkchop_cooked", top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 286, block: "mutton_raw",      top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 287, block: "mutton_cooked",   top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 288, block: "cod_raw",         top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 289, block: "cod_cooked",      top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 290, block: "salmon_raw",      top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 291, block: "salmon_cooked",   top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 292, block: "dried_kelp",      top: "", bottom: "", sides: "" },
    // W40.2: Mob drops (ids 293..=301). Sprites via ITEM_NAME_MAP.
    BlockNameMap { infengine_id: 293, block: "rotten_flesh",  top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 294, block: "bone",          top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 295, block: "ender_pearl",   top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 296, block: "blaze_rod",     top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 297, block: "ghast_tear",    top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 298, block: "slime_ball",    top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 299, block: "gunpowder",     top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 300, block: "spider_eye",    top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 301, block: "string",        top: "", bottom: "", sides: "" },
    // W40.2: Misc ingredients (ids 302..=323). Sprites via ITEM_NAME_MAP.
    BlockNameMap { infengine_id: 302, block: "stick",                    top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 303, block: "leather",                  top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 304, block: "paper",                    top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 305, block: "book_normal",              top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 306, block: "empty_map",                top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 307, block: "compass",                  top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 308, block: "clock",                    top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 309, block: "redstone_dust_dot",        top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 310, block: "glowstone_dust",           top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 311, block: "prismarine_shard",         top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 312, block: "prismarine_crystals",      top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 313, block: "nautilus_shell",           top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 314, block: "heart_of_the_sea",         top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 315, block: "lapis_lazuli",             top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 316, block: "diamond",                  top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 317, block: "emerald",                  top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 318, block: "gold_ingot",               top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 319, block: "sugar",                    top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 320, block: "blaze_powder",             top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 321, block: "magma_cream",              top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 322, block: "fermented_spider_eye",     top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 323, block: "amethyst_shard",           top: "", bottom: "", sides: "" },
    // W55.1: W48 trapdoor variants (548..=552). MC ships proper
    // 3-px slab blockstates that the universal JSON pipeline can
    // load once these rows point at the right MC name.
    BlockNameMap { infengine_id: 548, block: "oak_trapdoor",     top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 549, block: "iron_trapdoor",    top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 550, block: "birch_trapdoor",   top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 551, block: "spruce_trapdoor",  top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 552, block: "jungle_trapdoor",  top: "", bottom: "", sides: "" },
    // W440: legacy oak/iron door ids were MISSING here, so they
    // fell to the hand-coded static door_lower model - the mesher's
    // facing/half/hinge/open props never matched anything and every
    // oak door rendered closed with one fixed orientation (the R31
    // village-door report). With these rows the W401 variant cache
    // loads all 32 pre-rotated states like the 568+ doors.
    BlockNameMap { infengine_id: 77,  block: "oak_door",          top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 78,  block: "oak_door",          top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 79,  block: "iron_door",         top: "", bottom: "", sides: "" },
    // W55.1: W48 door variants (568..=575). MC ships proper
    // 3-px hinged door blockstates that the universal JSON
    // pipeline can load once these rows point at the right MC
    // name. Each variant has its own `*_door.json` blockstate
    // that the parser walks to pick the lower / upper half.
    BlockNameMap { infengine_id: 568, block: "birch_door",       top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 569, block: "spruce_door",      top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 570, block: "jungle_door",      top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 571, block: "acacia_door",      top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 572, block: "dark_oak_door",    top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 573, block: "mangrove_door",    top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 574, block: "cherry_door",      top: "", bottom: "", sides: "" },
    BlockNameMap { infengine_id: 575, block: "warped_door",      top: "", bottom: "", sides: "" },
    // W453: brewing stand - the multipart blockstate's
    // unconditional part is the base + rod model; the has_bottle_*
    // overlays stay off until container state reaches the mesher
    // (our multipart matcher only answers directional bools).
    // Without this row the stand cube-meshed with its item texture
    // on all six faces (the R34 screenshot).
    BlockNameMap { infengine_id: 566, block: "brewing_stand", top: "", bottom: "", sides: "" },
    // W442: ladder - facing variants (y-rotated single plane)
    // come straight from the MC blockstate.
    BlockNameMap { infengine_id: 595, block: "ladder", top: "", bottom: "", sides: "" },
    // W402: bell - the stand resolves from bell_floor.json via the
    // W401 variant cache; the swinging body stays a block entity.
    BlockNameMap { infengine_id: 966, block: "bell", top: "", bottom: "", sides: "" },
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
    // W293: crop-drop block-form items (493..=496). icon_ref skips
    // the item-sprite path for placeable items, so without these
    // rows the hotbar showed the CROP BLOCK texture (the user's
    // wrong wheat icon) instead of the MC item sprite.
    ItemNameMap { infengine_id: 493, item: "wheat" },
    ItemNameMap { infengine_id: 494, item: "carrot" },
    // W308: loot-batch items whose MC name has no same-named item
    // sprite - fall back to the closest vanilla sprite stem.
    ItemNameMap { infengine_id: 911, item: "golden_apple" },
    ItemNameMap { infengine_id: 915, item: "arrow" },
    ItemNameMap { infengine_id: 942, item: "bamboo" },
    ItemNameMap { infengine_id: 945, item: "string" },
    ItemNameMap { infengine_id: 495, item: "potato" },
    ItemNameMap { infengine_id: 496, item: "beetroot" },
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
    // W40.2 netherite tools.
    ItemNameMap { infengine_id: 274, item: "netherite_pickaxe" },
    ItemNameMap { infengine_id: 275, item: "netherite_sword" },
    ItemNameMap { infengine_id: 276, item: "netherite_shovel" },
    ItemNameMap { infengine_id: 277, item: "netherite_axe" },
    ItemNameMap { infengine_id: 278, item: "netherite_hoe" },
    // W40.2 netherite ingot + scrap.
    ItemNameMap { infengine_id: 279, item: "netherite_ingot" },
    ItemNameMap { infengine_id: 280, item: "netherite_scrap" },
    // W40.2 more food sprites.
    ItemNameMap { infengine_id: 281, item: "pumpkin_pie" },
    ItemNameMap { infengine_id: 282, item: "golden_apple" },
    ItemNameMap { infengine_id: 283, item: "baked_potato" },
    ItemNameMap { infengine_id: 284, item: "porkchop_raw" },
    ItemNameMap { infengine_id: 285, item: "porkchop_cooked" },
    ItemNameMap { infengine_id: 286, item: "mutton_raw" },
    ItemNameMap { infengine_id: 287, item: "mutton_cooked" },
    ItemNameMap { infengine_id: 288, item: "cod_raw" },
    ItemNameMap { infengine_id: 289, item: "cod_cooked" },
    ItemNameMap { infengine_id: 290, item: "salmon_raw" },
    ItemNameMap { infengine_id: 291, item: "salmon_cooked" },
    ItemNameMap { infengine_id: 292, item: "dried_kelp" },
    // W40.2 mob drops.
    ItemNameMap { infengine_id: 293, item: "rotten_flesh" },
    ItemNameMap { infengine_id: 294, item: "bone" },
    ItemNameMap { infengine_id: 295, item: "ender_pearl" },
    ItemNameMap { infengine_id: 296, item: "blaze_rod" },
    ItemNameMap { infengine_id: 297, item: "ghast_tear" },
    ItemNameMap { infengine_id: 298, item: "slime_ball" },
    ItemNameMap { infengine_id: 299, item: "gunpowder" },
    ItemNameMap { infengine_id: 300, item: "spider_eye" },
    ItemNameMap { infengine_id: 301, item: "string" },
    // W40.2 misc ingredients.
    ItemNameMap { infengine_id: 302, item: "stick" },
    ItemNameMap { infengine_id: 303, item: "leather" },
    ItemNameMap { infengine_id: 304, item: "paper" },
    ItemNameMap { infengine_id: 305, item: "book_normal" },
    ItemNameMap { infengine_id: 306, item: "empty_map" },
    ItemNameMap { infengine_id: 307, item: "compass_16" },
    ItemNameMap { infengine_id: 308, item: "clock" },
    ItemNameMap { infengine_id: 309, item: "redstone_dust_dot" },
    ItemNameMap { infengine_id: 310, item: "glowstone_dust" },
    ItemNameMap { infengine_id: 311, item: "prismarine_shard" },
    ItemNameMap { infengine_id: 312, item: "prismarine_crystals" },
    ItemNameMap { infengine_id: 313, item: "nautilus_shell" },
    ItemNameMap { infengine_id: 314, item: "heart_of_the_sea" },
    ItemNameMap { infengine_id: 315, item: "lapis_lazuli" },
    ItemNameMap { infengine_id: 316, item: "diamond" },
    ItemNameMap { infengine_id: 317, item: "emerald" },
    ItemNameMap { infengine_id: 318, item: "gold_ingot" },
    ItemNameMap { infengine_id: 319, item: "sugar" },
    ItemNameMap { infengine_id: 320, item: "blaze_powder" },
    ItemNameMap { infengine_id: 321, item: "magma_cream" },
    ItemNameMap { infengine_id: 322, item: "fermented_spider_eye" },
    ItemNameMap { infengine_id: 323, item: "amethyst_shard" },
    // W40.2 extra item-only entries (ids 324..=350).
    ItemNameMap { infengine_id: 324, item: "iron_ingot" },
    ItemNameMap { infengine_id: 325, item: "coal" },
    ItemNameMap { infengine_id: 326, item: "quartz" },
    ItemNameMap { infengine_id: 327, item: "flint" },
    ItemNameMap { infengine_id: 328, item: "copper_ingot" },
    ItemNameMap { infengine_id: 329, item: "raw_iron" },
    ItemNameMap { infengine_id: 330, item: "raw_gold" },
    ItemNameMap { infengine_id: 331, item: "raw_copper" },
    ItemNameMap { infengine_id: 332, item: "wooden_hoe" },
    ItemNameMap { infengine_id: 333, item: "stone_hoe" },
    ItemNameMap { infengine_id: 334, item: "iron_hoe" },
    ItemNameMap { infengine_id: 335, item: "golden_hoe" },
    ItemNameMap { infengine_id: 336, item: "diamond_hoe" },
    ItemNameMap { infengine_id: 337, item: "wheat" },
    ItemNameMap { infengine_id: 338, item: "wheat_seeds" },
    ItemNameMap { infengine_id: 339, item: "pumpkin_seeds" },
    ItemNameMap { infengine_id: 340, item: "melon_seeds" },
    ItemNameMap { infengine_id: 341, item: "beetroot_seeds" },
    ItemNameMap { infengine_id: 342, item: "beef" },
    ItemNameMap { infengine_id: 343, item: "potato" },
    ItemNameMap { infengine_id: 344, item: "carrot" },
    ItemNameMap { infengine_id: 345, item: "beetroot" },
    ItemNameMap { infengine_id: 346, item: "mushroom_stew" },
    ItemNameMap { infengine_id: 347, item: "bowl" },
    ItemNameMap { infengine_id: 348, item: "ink_sac" },
    ItemNameMap { infengine_id: 349, item: "glow_ink_sac" },
    ItemNameMap { infengine_id: 350, item: "echo_shard" },
    // W61.6: W48 door variants - item icon at textures/item/<name>.png.
    ItemNameMap { infengine_id: 568, item: "birch_door" },
    ItemNameMap { infengine_id: 569, item: "spruce_door" },
    ItemNameMap { infengine_id: 570, item: "jungle_door" },
    ItemNameMap { infengine_id: 571, item: "acacia_door" },
    ItemNameMap { infengine_id: 572, item: "dark_oak_door" },
    ItemNameMap { infengine_id: 573, item: "mangrove_door" },
    ItemNameMap { infengine_id: 574, item: "cherry_door" },
    ItemNameMap { infengine_id: 575, item: "warped_door" },
    // W53.4: hoes (infengine name "hoe_<tier>" -> MC "<tier>_hoe").
    ItemNameMap { infengine_id: 633, item: "wooden_hoe" },
    ItemNameMap { infengine_id: 634, item: "stone_hoe" },
    ItemNameMap { infengine_id: 635, item: "iron_hoe" },
    ItemNameMap { infengine_id: 636, item: "golden_hoe" },
    ItemNameMap { infengine_id: 637, item: "diamond_hoe" },
    // W53.4: bow / crossbow / shears / fishing_rod / shield / arrow.
    // infengine names already match MC names here, but explicit
    // rows make the mapping discoverable and keep the lookup fast.
    ItemNameMap { infengine_id: 638, item: "bow" },
    // MC ships crossbow as crossbow_standby.png (default state);
    // pulling animations live under crossbow_pulling_0..2.
    ItemNameMap { infengine_id: 639, item: "crossbow_standby" },
    ItemNameMap { infengine_id: 640, item: "shears" },
    ItemNameMap { infengine_id: 641, item: "fishing_rod" },
    ItemNameMap { infengine_id: 642, item: "shield" },
    ItemNameMap { infengine_id: 643, item: "arrow" },
    // W153: pitcher_plant ships a dedicated ITEM icon png while the
    // block faces use the pitcher_crop_* stages.
    ItemNameMap { infengine_id: 491, item: "pitcher_plant" },
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

    /// W440: legacy oak/iron door ids must resolve to the MC door
    /// blockstate names - without these rows the variant cache never
    /// loads and doors render closed with one fixed orientation.
    #[test]
    fn block_map_covers_legacy_doors() {
        assert_eq!(lookup_block(77).map(|m| m.block), Some("oak_door"));
        assert_eq!(lookup_block(78).map(|m| m.block), Some("oak_door"));
        assert_eq!(lookup_block(79).map(|m| m.block), Some("iron_door"));
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
        // W45.1: light_block_15 -> glowstone (fallback; no block_15 texture).
        assert_eq!(lookup_block(67).map(|m| m.block), Some("glowstone"));
    }

    // W34.1 / W45.1 - shape block entries (ids 68..=82).
    // W45.1 updated "block" fields to the actual MC blockstate name so the
    // universal JSON pipeline (W45.5) resolves the correct slab/stair/fence
    // model instead of a full-cube oak_planks/stone/cobblestone model.
    #[test]
    fn w34_shape_blocks_in_map() {
        assert_eq!(lookup_block(68).map(|m| m.block), Some("oak_slab"));
        assert_eq!(lookup_block(69).map(|m| m.block), Some("stone_slab"));
        assert_eq!(lookup_block(70).map(|m| m.block), Some("cobblestone_slab"));
        assert_eq!(lookup_block(71).map(|m| m.block), Some("oak_slab"));
        assert_eq!(lookup_block(72).map(|m| m.block), Some("oak_stairs"));
        assert_eq!(lookup_block(73).map(|m| m.block), Some("stone_stairs"));
        assert_eq!(lookup_block(74).map(|m| m.block), Some("cobblestone_stairs"));
        assert_eq!(lookup_block(75).map(|m| m.block), Some("oak_fence"));
        assert_eq!(lookup_block(76).map(|m| m.block), Some("nether_brick_fence"));
        assert_eq!(lookup_block(80).map(|m| m.block), Some("glass_pane"));
        assert_eq!(lookup_block(81).map(|m| m.block), Some("iron_bars"));
        assert_eq!(lookup_block(82).map(|m| m.block), Some("cobblestone_wall"));
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

    // W45.1: audit BLOCK_NAME_MAP "block", "top", "bottom", "sides" fields
    // against real MC 26.1.2 block textures. Skipped if the cached pack is
    // not present (CI / machines without MC). Checks that every non-empty
    // name resolves to a real .png file in
    //   <pack>/assets/minecraft/textures/block/<name>.png
    //
    // Names that intentionally point to item textures (e.g. tools) are in
    // ITEM_NAME_MAP, not BLOCK_NAME_MAP, so this test only covers entries
    // that are truly expected to exist in the block texture folder.
    //
    // The set of IDs that are item-only (no block texture) is excluded via
    // ITEM_ONLY_IDS. Everything else must resolve or the test fails.
    #[test]
    fn w45_block_name_map_textures_exist_in_mc_pack() {
        // Known path for the cached MC pack (Windows user-specific).
        let pack_dir = std::path::Path::new(
            r"C:\Users\micha\AppData\Roaming\InfEngine\voxel_demo_mp\data\cache\mc_assets\26.1.2\assets\minecraft\textures\block",
        );
        if !pack_dir.exists() {
            // No MC pack present - skip silently.
            return;
        }

        // IDs whose "block" field is NOT a texture name:
        // - Item-only IDs: tools, food, ingots (ITEM_NAME_MAP handles them)
        // - Shape block IDs (68..=82): "block" is the MC *blockstate* name
        //   (e.g. "oak_slab") for the universal JSON pipeline, not a texture.
        //   Their texture comes from the JSON model's texture declarations.
        const ITEM_ONLY_IDS: &[u16] = &[
            12,  // raw_beef - item only
            35,  // red_bed - entity model, no block textures
            68, 69, 70, 71, 72, 73, 74, 75, 76, 77, 78, 79, 80, 81, 82, // shape blocks
            // W261: deepslate shape blocks - blockstate names, textures
            // via the JSON model refs.
            799, 800, 801, 802, 803, 804, 805, 806, 807, 808, 809, 810,
            // W263: trial-chambers shape blocks (same rule).
            816, 823, 824, 825, 826, 827, 828, 829, 832,
            // W270: bastion blackstone/quartz shape blocks (same rule).
            839, 840, 841, 842, 843,
            543, 544, 545, 546, 547, // W133: wood fences (W131 rows) - blockstate names, texture via JSON planks ref
            966, // W402: bell - blockstate name, textures via JSON refs (planks/stone)
            568, 569, 570, 571, 572, 573, 574, 575, // W133: door variants - MC ships <wood>_door_top/_bottom.png only
            // W443: stained glass panes - blockstate names; textures
            // via the JSON refs (<color>_stained_glass + _pane_top).
            701, 702, 703, 704, 705, 706, 707, 708, 709, 710, 711, 712,
            713, 714, 715, 716,
            217, 218, 219, 220, 221, 222, 223, 224, 225, 226, 227, 228, 229, 230, 231, 232, // carpets (texture via JSON wool ref)
            154, 155, 156, 157, 158, 159, 160, 161, // gold/diamond tools
            162, 163, 164, 165, 166, 167, 168,       // food items
            274, 275, 276, 277, 278,                  // netherite tools
            279, 280,                                  // netherite ingot/scrap
            281, 282, 283, 284, 285, 286, 287, 288, 289, 290, 291, 292, // food
            293, 294, 295, 296, 297, 298, 299, 300, 301, // mob drops
            302, 303, 304, 305, 306, 307, 308, 309, 310, // ingredients
            311, 312, 313, 314, 315, 316, 317, 318, 319, 320, 321, 322, 323, // ingredients
        ];

        let check = |name: &str, id: u16, field: &str| {
            if name.is_empty() {
                return; // empty = fallback to block name, not checked separately
            }
            let path = pack_dir.join(format!("{name}.png"));
            assert!(
                path.exists(),
                "id={id} field={field} name=\"{name}\" not found at {}",
                path.display()
            );
        };

        for entry in BLOCK_NAME_MAP {
            let id = entry.infengine_id;
            if ITEM_ONLY_IDS.contains(&id) {
                continue;
            }
            // Only check the "block" field as a texture name when the entry
            // has no per-face overrides. When top/bottom/sides are provided,
            // the "block" field is used as the MC *blockstate* name (for the
            // universal JSON pipeline), not as a direct texture name. Example:
            // id=3 grass_block has block="grass_block" (blockstate name) but
            // individual top="grass_block_top", bottom="dirt", sides="grass_block_side".
            let has_face_overrides = !entry.top.is_empty()
                || !entry.bottom.is_empty()
                || !entry.sides.is_empty();
            if !has_face_overrides {
                check(entry.block, id, "block");
            }
            check(entry.top, id, "top");
            check(entry.bottom, id, "bottom");
            check(entry.sides, id, "sides");
        }
    }

    /// W145: every creative-menu item must resolve an inventory icon
    /// through the real chain - matching item png stem, ITEM_NAME_MAP
    /// override, or a block-face texture of the placed block. Anything
    /// that fails here renders as a blank tinted swatch in the
    /// creative tab (the user-reported "items with no icon"). Skips
    /// silently when the MC pack isn't on disk.
    #[test]
    fn w145_every_creative_item_icon_resolves() {
        let item_dir = std::path::Path::new(
            r"C:\Users\micha\AppData\Roaming\InfEngine\voxel_demo_mp\data\cache\mc_assets\26.1.2\assets\minecraft\textures\item",
        );
        let block_dir = std::path::Path::new(
            r"C:\Users\micha\AppData\Roaming\InfEngine\voxel_demo_mp\data\cache\mc_assets\26.1.2\assets\minecraft\textures\block",
        );
        if !item_dir.exists() || !block_dir.exists() {
            return;
        }
        // Items whose icon legitimately has no flat texture in the
        // pack:
        // - 642 shield: MC renders the inventory icon from the entity
        //   model (textures/entity/shield/...); a baked icon is an
        //   engine follow-up.
        const KNOWN_MODEL_RENDERED: &[u16] = &[642];
        let mut missing: Vec<String> = Vec::new();
        for it in crate::item::ITEM_TYPES.iter() {
            if KNOWN_MODEL_RENDERED.contains(&it.id)
                || crate::creative_tab::CREATIVE_BLACKLIST.contains(&it.id)
            {
                continue;
            }
            // 1. match-name path (png stem == item name).
            if item_dir.join(format!("{}.png", it.name)).is_file() {
                continue;
            }
            // 2. explicit ITEM_NAME_MAP override.
            if let Some(mapped) = lookup_item(it.id)
                && item_dir.join(format!("{mapped}.png")).is_file()
            {
                continue;
            }
            // 3. block-form fallback: any face texture of the placed
            //    block (registry TileSpec candidates + name-map faces).
            let bid = it.places.unwrap_or(it.id);
            let mut ok = false;
            if let Some(bt) = crate::block::block_type(bid) {
                for f in bt.sides.files.iter().chain(bt.top.files.iter()) {
                    let stem = f.trim_end_matches(".png");
                    if block_dir.join(format!("{stem}.png")).is_file() {
                        ok = true;
                        break;
                    }
                }
            }
            if !ok && let Some(e) = lookup_block(bid) {
                for stem in [e.sides, e.top, e.block] {
                    if !stem.is_empty()
                        && block_dir.join(format!("{stem}.png")).is_file()
                    {
                        ok = true;
                        break;
                    }
                }
            }
            if !ok {
                missing.push(format!("{} {}", it.id, it.name));
            }
        }
        assert!(
            missing.is_empty(),
            "creative items with unresolvable icons: {missing:?}"
        );
    }

    // W45.1 spot checks for corrected entries.
    #[test]
    fn w45_corrected_entries() {
        // end_rod: top/bottom now use "end_rod" not the nonexistent "end_rod_top"
        let e = lookup_block(57).unwrap();
        assert_eq!(e.top, "end_rod");
        assert_eq!(e.bottom, "end_rod");
        // light_block_15 -> glowstone fallback
        assert_eq!(lookup_block(67).unwrap().block, "glowstone");
        // crimson_hyphae -> crimson_stem
        assert_eq!(lookup_block(242).unwrap().block, "crimson_stem");
        // warped_hyphae -> warped_stem
        assert_eq!(lookup_block(245).unwrap().block, "warped_stem");
        // shape blocks use MC blockstate names
        assert_eq!(lookup_block(68).unwrap().block, "oak_slab");
        assert_eq!(lookup_block(72).unwrap().block, "oak_stairs");
        assert_eq!(lookup_block(75).unwrap().block, "oak_fence");
        assert_eq!(lookup_block(76).unwrap().block, "nether_brick_fence");
        assert_eq!(lookup_block(80).unwrap().block, "glass_pane");
        assert_eq!(lookup_block(82).unwrap().block, "cobblestone_wall");
    }
}
