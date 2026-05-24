//! W50.2 - shaped 3x3 crafting recipes.
//!
//! Shaped recipes care about position. Pattern is a 3x3 grid of
//! block IDs where `0` = empty slot. The matcher tries every
//! origin offset and a horizontal flip (Minecraft's "symmetric"
//! interpretation) so the player doesn't have to anchor at (0,0).

#![allow(dead_code, clippy::needless_range_loop)]

/// One shaped recipe. `pattern[row][col]` (3 rows top-to-bottom,
/// 3 cols left-to-right). Slots marked `0` must be empty. The
/// rest of the grid (outside the recipe's bounding box) must
/// also be empty - shaped recipes are NOT shapeless.
pub(crate) struct ShapedRecipe {
    pub(crate) pattern: [[u16; 3]; 3],
    pub(crate) output: (u16, u8),
    pub(crate) name: &'static str,
}

/// Tight bbox of non-zero cells, useful for slide-matching.
pub(crate) fn pattern_bbox(p: &[[u16; 3]; 3]) -> Option<(usize, usize, usize, usize)> {
    let mut min_r = 3;
    let mut max_r = 0;
    let mut min_c = 3;
    let mut max_c = 0;
    let mut any = false;
    for r in 0..3 {
        for c in 0..3 {
            if p[r][c] != 0 {
                any = true;
                if r < min_r {
                    min_r = r;
                }
                if r > max_r {
                    max_r = r;
                }
                if c < min_c {
                    min_c = c;
                }
                if c > max_c {
                    max_c = c;
                }
            }
        }
    }
    if !any {
        return None;
    }
    Some((min_r, min_c, max_r - min_r + 1, max_c - min_c + 1))
}

/// W50.2 demo set: a handful of canonical shaped recipes that
/// the shapeless 2x2 table couldn't express. Use 0 for empty
/// slots, BlockId values for ingredients. Block ID legend (key
/// numbers used below):
///   5 = wood log, 7 = oak planks, 8 = cobble, 14 = iron ingot,
///   302 = stick, 304 = paper, 553 = crafting_table,
///   41 = chest, 42 = furnace, BUCKET_EMPTY_ID = empty bucket.
pub(crate) const SHAPED_RECIPES: &[ShapedRecipe] = &[
    // Crafting table (4 planks 2x2 corner) - shaped variant for
    // when the player anchors in a 3x3 grid.
    ShapedRecipe {
        pattern: [[7, 7, 0], [7, 7, 0], [0, 0, 0]],
        output: (553, 1),
        name: "Crafting table",
    },
    // Chest: planks around the perimeter of the top 2x3.
    ShapedRecipe {
        pattern: [[7, 7, 7], [7, 0, 7], [7, 7, 7]],
        output: (41, 1),
        name: "Chest",
    },
    // Furnace: 8 cobble around an empty center.
    ShapedRecipe {
        pattern: [[8, 8, 8], [8, 0, 8], [8, 8, 8]],
        output: (42, 1),
        name: "Furnace",
    },
    // Stick: 2 planks vertical.
    ShapedRecipe {
        pattern: [[7, 0, 0], [7, 0, 0], [0, 0, 0]],
        output: (302, 4),
        name: "Sticks",
    },
    // Wood pickaxe: 3 planks top row + 2 sticks vertical handle.
    ShapedRecipe {
        pattern: [[7, 7, 7], [0, 302, 0], [0, 302, 0]],
        output: (16, 1),
        name: "Wood pickaxe (shaped)",
    },
    // Stone pickaxe: 3 cobble top + 2 sticks handle.
    ShapedRecipe {
        pattern: [[8, 8, 8], [0, 302, 0], [0, 302, 0]],
        output: (17, 1),
        name: "Stone pickaxe (shaped)",
    },
    // Iron pickaxe.
    ShapedRecipe {
        pattern: [[14, 14, 14], [0, 302, 0], [0, 302, 0]],
        output: (18, 1),
        name: "Iron pickaxe (shaped)",
    },
    // Wood axe (L-shape: 2 planks top-left + 1 plank middle-left
    // + sticks).
    ShapedRecipe {
        pattern: [[7, 7, 0], [7, 302, 0], [0, 302, 0]],
        output: (32, 1),
        name: "Wood axe (shaped)",
    },
    // Stone axe.
    ShapedRecipe {
        pattern: [[8, 8, 0], [8, 302, 0], [0, 302, 0]],
        output: (33, 1),
        name: "Stone axe (shaped)",
    },
    // Iron axe.
    ShapedRecipe {
        pattern: [[14, 14, 0], [14, 302, 0], [0, 302, 0]],
        output: (34, 1),
        name: "Iron axe (shaped)",
    },
    // Wood shovel.
    ShapedRecipe {
        pattern: [[7, 0, 0], [302, 0, 0], [302, 0, 0]],
        output: (29, 1),
        name: "Wood shovel (shaped)",
    },
    // Stone shovel.
    ShapedRecipe {
        pattern: [[8, 0, 0], [302, 0, 0], [302, 0, 0]],
        output: (30, 1),
        name: "Stone shovel (shaped)",
    },
    // Iron shovel.
    ShapedRecipe {
        pattern: [[14, 0, 0], [302, 0, 0], [302, 0, 0]],
        output: (31, 1),
        name: "Iron shovel (shaped)",
    },
    // Wood sword: 2 planks vertical + 1 stick.
    ShapedRecipe {
        pattern: [[7, 0, 0], [7, 0, 0], [302, 0, 0]],
        output: (23, 1),
        name: "Wood sword (shaped)",
    },
    // Stone sword.
    ShapedRecipe {
        pattern: [[8, 0, 0], [8, 0, 0], [302, 0, 0]],
        output: (24, 1),
        name: "Stone sword (shaped)",
    },
    // Iron sword.
    ShapedRecipe {
        pattern: [[14, 0, 0], [14, 0, 0], [302, 0, 0]],
        output: (25, 1),
        name: "Iron sword (shaped)",
    },
    // Bucket: V of 3 iron ingots (bottom 3 + sides).
    ShapedRecipe {
        pattern: [[0, 0, 0], [14, 0, 14], [0, 14, 0]],
        output: (crate::BUCKET_EMPTY_ID, 1),
        name: "Bucket (shaped)",
    },
    // Ladder: 7 sticks in MC's H pattern.
    ShapedRecipe {
        pattern: [[302, 0, 302], [302, 302, 302], [302, 0, 302]],
        output: (595, 3),
        name: "Ladder (shaped)",
    },
    // Bed: 3 wool top row + 3 planks bottom row.
    ShapedRecipe {
        pattern: [[0, 0, 0], [11, 11, 11], [7, 7, 7]],
        output: (35, 1),
        name: "Bed (shaped)",
    },
    // Bowl: 3 planks V (W50.2 demo, item id 304 is paper - reuse
    // for now since the demo has no real bowl item).
    // Book: 3 paper.
    ShapedRecipe {
        pattern: [[0, 0, 0], [304, 304, 304], [0, 0, 0]],
        output: (305, 1),
        name: "Book (shaped)",
    },
];
