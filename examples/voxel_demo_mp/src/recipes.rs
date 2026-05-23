//! W47.27 - crafting recipe table (relocated from main.rs).

#![allow(unused_imports)]

use crate::*;

/// Shapeless 2x2 crafting recipe. Each entry is `(BlockId, count)`
/// the player must place into the 2x2 grid (order doesn't matter).
/// Empty inputs are encoded by leaving them out of the list.
pub(crate) struct Recipe {
    pub(crate) inputs: &'static [(u16, u8)],
    pub(crate) output: (u16, u8),
    /// Human-readable name shown in the recipe book.
    pub(crate) name: &'static str,
}

/// Demo recipe table. Real Minecraft has shaped + tagged recipes;
/// this is a deliberately tiny showcase for the engine. New entries
/// = one line here, no other changes needed.
pub(crate) const RECIPES: &[Recipe] = &[
    // 4 dirt -> 1 grass (sodding).
    Recipe {
        inputs: &[(2, 1), (2, 1), (2, 1), (2, 1)],
        output: (3, 1),
        name: "Sodded grass",
    },
    // 4 leaves -> 1 wood (the tree's revenge).
    Recipe {
        inputs: &[(6, 1), (6, 1), (6, 1), (6, 1)],
        output: (5, 1),
        name: "Bundled log",
    },
    // 1 wood -> 4 planks (classic Minecraft).
    Recipe {
        inputs: &[(5, 1)],
        output: (7, 4),
        name: "Oak planks",
    },
    // 4 planks -> 1 wood (consolidate; lets you reverse the craft
    // chain).
    Recipe {
        inputs: &[(7, 1), (7, 1), (7, 1), (7, 1)],
        output: (5, 1),
        name: "Consolidated log",
    },
    // 4 stone -> 1 cobble (compressed cobblestone block).
    Recipe {
        inputs: &[(1, 1), (1, 1), (1, 1), (1, 1)],
        output: (8, 1),
        name: "Cobblestone",
    },
    // 4 cobble -> 4 stone (polish cobbles back into stone).
    Recipe {
        inputs: &[(8, 1), (8, 1), (8, 1), (8, 1)],
        output: (1, 4),
        name: "Polished stone",
    },
    // 4 sand -> 1 glass (we have no furnace yet, so the craft does
    // double duty as the heat source).
    Recipe {
        inputs: &[(4, 1), (4, 1), (4, 1), (4, 1)],
        output: (9, 1),
        name: "Glass block",
    },
    // 2 wood + 2 leaves -> 4 torches. Two-block input
    // disambiguates this from the planks recipe (1 wood alone).
    Recipe {
        inputs: &[(5, 1), (5, 1), (6, 1), (6, 1)],
        output: (10, 4),
        name: "Torches",
    },
    // 3 planks + 1 wood -> 1 wood pickaxe. The wood log stands
    // in for the stick handle the 2x2 grid can't shape.
    Recipe {
        inputs: &[(7, 1), (7, 1), (7, 1), (5, 1)],
        output: (16, 1),
        name: "Wood pickaxe",
    },
    // 3 cobble + 1 wood -> 1 stone pickaxe.
    Recipe {
        inputs: &[(8, 1), (8, 1), (8, 1), (5, 1)],
        output: (17, 1),
        name: "Stone pickaxe",
    },
    // 3 iron + 1 wood -> 1 iron pickaxe.
    Recipe {
        inputs: &[(14, 1), (14, 1), (14, 1), (5, 1)],
        output: (18, 1),
        name: "Iron pickaxe",
    },
    // Sword variants: 2 of the material + 1 wood handle.
    Recipe {
        inputs: &[(7, 1), (7, 1), (5, 1)],
        output: (23, 1),
        name: "Wood sword",
    },
    Recipe {
        inputs: &[(8, 1), (8, 1), (5, 1)],
        output: (24, 1),
        name: "Stone sword",
    },
    Recipe {
        inputs: &[(14, 1), (14, 1), (5, 1)],
        output: (25, 1),
        name: "Iron sword",
    },
    // 4 coal -> 1 glowstone block. Loose interpretation since
    // the demo has no nether biome; gives the player a use for
    // surplus coal beyond torches.
    Recipe {
        inputs: &[(13, 1), (13, 1), (13, 1), (13, 1)],
        output: (28, 1),
        name: "Glowstone",
    },
    // 2 sand + 2 cobble -> 1 sandstone. Distinct from the
    // 4-sand glass recipe so the matcher resolves cleanly.
    Recipe {
        inputs: &[(4, 1), (4, 1), (8, 1), (8, 1)],
        output: (26, 1),
        name: "Sandstone",
    },
    // Shovels: 1 head material + 2 wood handle.
    Recipe {
        inputs: &[(7, 1), (5, 1), (5, 1)],
        output: (29, 1),
        name: "Wood shovel",
    },
    Recipe {
        inputs: &[(8, 1), (5, 1), (5, 1)],
        output: (30, 1),
        name: "Stone shovel",
    },
    Recipe {
        inputs: &[(14, 1), (5, 1), (5, 1)],
        output: (31, 1),
        name: "Iron shovel",
    },
    // Axes: 2 head material + 2 wood handle. Distinct counts
    // from pickaxe (3 head + 1 wood) so the shapeless matcher
    // resolves cleanly.
    Recipe {
        inputs: &[(7, 1), (7, 1), (5, 1), (5, 1)],
        output: (32, 1),
        name: "Wood axe",
    },
    Recipe {
        inputs: &[(8, 1), (8, 1), (5, 1), (5, 1)],
        output: (33, 1),
        name: "Stone axe",
    },
    Recipe {
        inputs: &[(14, 1), (14, 1), (5, 1), (5, 1)],
        output: (34, 1),
        name: "Iron axe",
    },
    // 3 wool + 1 planks -> 1 bed. Loose interpretation of the
    // 3-wool + 3-plank MC bed shape, fit into the 2x2 grid.
    Recipe {
        inputs: &[(11, 1), (11, 1), (11, 1), (7, 1)],
        output: (35, 1),
        name: "Bed",
    },
    // 3 iron -> 1 empty bucket. MC shape (V of 3 ingots)
    // collapses to a shapeless 3-iron recipe on the 2x2 grid;
    // a real 3x3 crafting table comes later.
    Recipe {
        inputs: &[(14, 1), (14, 1), (14, 1)],
        output: (BUCKET_EMPTY_ID, 1),
        name: "Bucket",
    },
    // ============================================================
    // W50.1 - extended MC shapeless recipes batch.
    // Plank variants from logs + sticks + tools + utility.
    // ============================================================
    // Wood -> plank variants (1 log -> 4 planks).
    Recipe { inputs: &[(88, 1)], output: (89, 4), name: "Birch planks" },
    Recipe { inputs: &[(127, 1)], output: (128, 4), name: "Acacia planks" },
    Recipe { inputs: &[(130, 1)], output: (131, 4), name: "Spruce planks" },
    Recipe { inputs: &[(133, 1)], output: (134, 4), name: "Jungle planks" },
    Recipe { inputs: &[(136, 1)], output: (137, 4), name: "Dark oak planks" },
    Recipe { inputs: &[(233, 1)], output: (234, 4), name: "Mangrove planks" },
    Recipe { inputs: &[(237, 1)], output: (238, 4), name: "Cherry planks" },
    // 2 planks -> 4 sticks (shapeless on 2x2).
    Recipe { inputs: &[(7, 1), (7, 1)], output: (302, 4), name: "Sticks" },
    // Crafting table: 4 planks -> 1 crafting_table.
    Recipe { inputs: &[(7, 1), (7, 1), (7, 1), (7, 1)], output: (553, 1), name: "Crafting table" },
    // Chest: 4 planks (corners) - simplified shapeless on 2x2.
    Recipe { inputs: &[(7, 1), (7, 1), (7, 1), (7, 1)], output: (41, 1), name: "Chest" },
    // Furnace: 4 cobble.
    Recipe { inputs: &[(8, 1), (8, 1), (8, 1), (8, 1)], output: (42, 1), name: "Furnace" },
    // Bread: 3 wheat.
    Recipe { inputs: &[(493, 1), (493, 1), (493, 1)], output: (163, 1), name: "Bread" },
    // 9 iron_ingot -> 1 iron_block.
    Recipe { inputs: &[(14, 1), (14, 1), (14, 1), (14, 1)], output: (440, 1), name: "Iron block" },
    // 9 gold_ingot -> 1 gold_block.
    Recipe { inputs: &[(318, 1), (318, 1), (318, 1), (318, 1)], output: (391, 1), name: "Gold block" },
    // 4 diamond -> diamond_block.
    Recipe { inputs: &[(316, 1), (316, 1), (316, 1), (316, 1)], output: (392, 1), name: "Diamond block" },
    // 4 emerald -> emerald_block.
    Recipe { inputs: &[(317, 1), (317, 1), (317, 1), (317, 1)], output: (439, 1), name: "Emerald block" },
    // 4 lapis -> lapis_block.
    Recipe { inputs: &[(315, 1), (315, 1), (315, 1), (315, 1)], output: (438, 1), name: "Lapis block" },
    // 4 redstone -> redstone_block.
    Recipe { inputs: &[(309, 1), (309, 1), (309, 1), (309, 1)], output: (441, 1), name: "Redstone block" },
    // 4 raw_copper -> raw_copper_block.
    Recipe { inputs: &[(288, 1), (288, 1), (288, 1), (288, 1)], output: (388, 1), name: "Raw copper block" },
    // 4 raw_iron -> raw_iron_block.
    Recipe { inputs: &[(287, 1), (287, 1), (287, 1), (287, 1)], output: (389, 1), name: "Raw iron block" },
    // 4 raw_gold -> raw_gold_block.
    Recipe { inputs: &[(289, 1), (289, 1), (289, 1), (289, 1)], output: (390, 1), name: "Raw gold block" },
    // 4 amethyst_shard -> amethyst_block.
    Recipe { inputs: &[(323, 1), (323, 1), (323, 1), (323, 1)], output: (451, 1), name: "Amethyst block" },
    // 4 sticks -> ladder.
    Recipe { inputs: &[(302, 1), (302, 1), (302, 1), (302, 1)], output: (595, 1), name: "Ladder" },
    // Stone bricks: 4 stone -> 4 stone_bricks (placeholder; real MC uses chiseled pattern).
    Recipe { inputs: &[(1, 1), (1, 1), (1, 1), (1, 1)], output: (8, 4), name: "Stone bricks alt" },
    // 4 brick -> 1 bricks block (4 brick items to 1 bricks block).
    Recipe { inputs: &[(13, 1), (13, 1), (13, 1), (13, 1)], output: (86, 1), name: "Brick block" },
    // 4 bone_meal stand-in: 4 wheat -> hay_bale.
    Recipe { inputs: &[(493, 1), (493, 1), (493, 1), (493, 1)], output: (435, 1), name: "Hay bale" },
    // 4 sugar_cane -> 1 paper (shapeless).
    Recipe { inputs: &[(499, 1), (499, 1), (499, 1)], output: (304, 1), name: "Paper" },
    // 1 paper + 1 stick + 1 string -> compass (simplified shapeless).
    Recipe { inputs: &[(304, 1), (302, 1), (301, 1)], output: (307, 1), name: "Compass" },
    // 1 paper + 1 leather + 1 stick -> book.
    Recipe { inputs: &[(304, 1), (303, 1), (302, 1)], output: (305, 1), name: "Book" },
    // Slime block: 4 slime_balls -> slime_block (using id 298).
    Recipe { inputs: &[(298, 1), (298, 1), (298, 1), (298, 1)], output: (442, 1), name: "Slime block" },
    // Honey block (4 honey bottles - using ID 444 honeycomb_block as proxy).
    Recipe { inputs: &[(444, 1)], output: (443, 1), name: "Honey block" },
];
