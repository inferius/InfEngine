//! W478 - grid recipe matcher, ported from the MC 26.1.2 sources.
//!
//! Shaped semantics (`ShapedRecipePattern.matches` +
//! `CraftingInput.ofPositioned`):
//! 1. Trim the grid to the bounding box of its non-empty cells
//!    (`ofPositioned`) - a stray item outside the pattern expands
//!    the bbox and fails the dims check, which is exactly the
//!    "rest of the grid must be empty" rule.
//! 2. The trimmed dims must EQUAL the pattern dims and the grid's
//!    non-empty count must equal the pattern's ingredient count.
//! 3. Per-cell test, direct and horizontally mirrored
//!    (`this.width - x - 1`): an empty pattern cell requires an
//!    empty grid cell; a non-empty cell requires an item the slot's
//!    acceptable-id set contains.
//!
//! Shapeless semantics (`ShapelessRecipe.matches` +
//! `StackedContents.RecipePicker`): non-empty cell count must equal
//! the ingredient count, then every cell item must be assignable to
//! a DISTINCT ingredient whose set accepts it. MC solves this with
//! an augmenting-path bipartite matcher; grids cap at 9 cells so a
//! backtracking assignment is equivalent.

use super::crafting_table::{SHAPED, SHAPELESS};
use super::{Ingredient, ShapedRecipe};

/// A 3x3 craft grid, row-major. `None` = empty.
pub(crate) type Grid3 = [Option<(u16, u8)>; 9];

/// A 2x2 craft grid (the E-screen inventory craft), row-major.
pub(crate) type Grid2 = [Option<(u16, u8)>; 4];

/// Bounding box of the non-empty cells: `(left, top, w, h, count)`.
/// `None` for an all-empty grid (`CraftingInput.Positioned.EMPTY`).
fn trim_bbox(
    cells: &[Option<(u16, u8)>],
    gw: usize,
) -> Option<(usize, usize, usize, usize, usize)> {
    let gh = cells.len() / gw;
    let (mut l, mut r, mut t, mut b) = (gw, 0usize, gh, 0usize);
    let mut count = 0usize;
    for y in 0..gh {
        for x in 0..gw {
            if cells[y * gw + x].is_some() {
                count += 1;
                l = l.min(x);
                r = r.max(x);
                t = t.min(y);
                b = b.max(y);
            }
        }
    }
    if count == 0 {
        return None;
    }
    Some((l, t, r - l + 1, b - t + 1, count))
}

/// Per-cell test of `recipe` against the trimmed grid window at
/// `(left, top)`, optionally x-mirrored.
fn shaped_matches_at(
    recipe: &ShapedRecipe,
    cells: &[Option<(u16, u8)>],
    gw: usize,
    left: usize,
    top: usize,
    mirror: bool,
) -> bool {
    let (w, h) = (recipe.w as usize, recipe.h as usize);
    for y in 0..h {
        for x in 0..w {
            let px = if mirror { w - 1 - x } else { x };
            let want: Ingredient = recipe.cells[y * w + px];
            let have = cells[(top + y) * gw + (left + x)];
            match have {
                None => {
                    if !want.is_empty() {
                        return false;
                    }
                }
                Some((id, _)) => {
                    if want.is_empty() || !want.contains(&id) {
                        return false;
                    }
                }
            }
        }
    }
    true
}

/// Match every shaped recipe against a `gw`-wide grid. The trimmed
/// dims equality doubles as the dimension gate: a 3-wide pattern can
/// never equal a bbox cut from a 2-wide grid.
pub(crate) fn match_shaped_in(
    cells: &[Option<(u16, u8)>],
    gw: usize,
) -> Option<(u16, u8, &'static str)> {
    let (left, top, tw, th, count) = trim_bbox(cells, gw)?;
    for r in SHAPED {
        if r.w as usize != tw || r.h as usize != th {
            continue;
        }
        if r.ingredient_count() != count {
            continue;
        }
        if shaped_matches_at(r, cells, gw, left, top, false)
            || shaped_matches_at(r, cells, gw, left, top, true)
        {
            return Some((r.output.0, r.output.1, r.name));
        }
    }
    None
}

/// Backtracking bipartite assignment: every item must pay a distinct
/// ingredient that accepts it (the `RecipePicker` outcome).
fn assign(items: &[u16], ings: &[Ingredient], idx: usize, used: &mut u16) -> bool {
    if idx == items.len() {
        return true;
    }
    for (j, ing) in ings.iter().enumerate() {
        let bit = 1u16 << j;
        if *used & bit == 0 && ing.contains(&items[idx]) {
            *used |= bit;
            if assign(items, ings, idx + 1, used) {
                return true;
            }
            *used &= !bit;
        }
    }
    false
}

/// Match every shapeless recipe against the grid cells (order and
/// position are irrelevant; the non-empty count must be exact).
pub(crate) fn match_shapeless_in(
    cells: &[Option<(u16, u8)>],
) -> Option<(u16, u8, &'static str)> {
    let items: Vec<u16> = cells.iter().flatten().map(|(id, _)| *id).collect();
    if items.is_empty() {
        return None;
    }
    for r in SHAPELESS {
        if r.ingredients.len() != items.len() {
            continue;
        }
        let mut used = 0u16;
        if assign(&items, r.ingredients, 0, &mut used) {
            return Some((r.output.0, r.output.1, r.name));
        }
    }
    None
}

/// Top-level entry point for the 3x3 crafting table. Shaped recipes
/// take precedence (more specific), then the shapeless table.
pub(crate) fn match_3x3(g: &Grid3) -> Option<(u16, u8, &'static str)> {
    match_shaped_in(&g[..], 3).or_else(|| match_shapeless_in(&g[..]))
}

/// Top-level entry point for the 2x2 inventory craft grid. Only
/// recipes that fit 2x2 can match (the trimmed-dims equality gates
/// shaped rows; a shapeless row needs <= 4 cells to tally).
pub(crate) fn match_2x2(g: &Grid2) -> Option<(u16, u8, &'static str)> {
    match_shaped_in(&g[..], 2).or_else(|| match_shapeless_in(&g[..]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recipes::crafting_table::TAG_PLANKS;

    fn cell(id: u16, n: u8) -> Option<(u16, u8)> {
        Some((id, n))
    }

    // Engine ids used below: 7 oak planks, 302 stick, 553 crafting
    // table, 41 chest, 42 furnace, 43 iron ingot, 18 iron pickaxe
    // (legacy row), 325 coal, 993 charcoal, 10 torch, 337 wheat,
    // 435 hay bale, 675 brick item, 86 bricks block, 89 birch
    // planks, 5 oak log.

    #[test]
    fn shaped_crafting_table_matches_at_any_offset() {
        // Top-left anchor.
        let g: Grid3 = [
            cell(7, 1), cell(7, 1), None,
            cell(7, 1), cell(7, 1), None,
            None, None, None,
        ];
        let m = match_3x3(&g).expect("crafting table top-left");
        assert_eq!((m.0, m.1), (553, 1));
        // Slid to the bottom-right corner.
        let g: Grid3 = [
            None, None, None,
            None, cell(7, 1), cell(7, 1),
            None, cell(7, 1), cell(7, 1),
        ];
        assert_eq!(match_3x3(&g).expect("slid").0, 553);
    }

    #[test]
    fn shaped_tag_slot_accepts_any_plank_member() {
        assert!(TAG_PLANKS.contains(&7) && TAG_PLANKS.contains(&89));
        // Mixed oak + birch planks still craft the table (each cell
        // independently satisfies the #planks set).
        let g: Grid3 = [
            cell(7, 1), cell(89, 1), None,
            cell(89, 1), cell(7, 1), None,
            None, None, None,
        ];
        assert_eq!(match_3x3(&g).expect("mixed planks").0, 553);
    }

    #[test]
    fn shaped_chest_is_the_eight_plank_ring() {
        let g: Grid3 = [
            cell(7, 1), cell(7, 1), cell(7, 1),
            cell(7, 1), None, cell(7, 1),
            cell(7, 1), cell(7, 1), cell(7, 1),
        ];
        let m = match_3x3(&g).expect("chest ring");
        assert_eq!((m.0, m.1), (41, 1));
        // A filled centre violates the empty pattern cell.
        let g: Grid3 = [
            cell(7, 1), cell(7, 1), cell(7, 1),
            cell(7, 1), cell(7, 1), cell(7, 1),
            cell(7, 1), cell(7, 1), cell(7, 1),
        ];
        assert!(match_3x3(&g).is_none_or(|m| m.0 != 41));
    }

    #[test]
    fn shaped_furnace_ring_matches() {
        let g: Grid3 = [
            cell(8, 1), cell(8, 1), cell(8, 1),
            cell(8, 1), None, cell(8, 1),
            cell(8, 1), cell(8, 1), cell(8, 1),
        ];
        assert_eq!(match_3x3(&g).expect("furnace").0, 42);
    }

    #[test]
    fn shaped_iron_pickaxe_uses_ingots_and_sticks() {
        let g: Grid3 = [
            cell(43, 1), cell(43, 1), cell(43, 1),
            None, cell(302, 1), None,
            None, cell(302, 1), None,
        ];
        let m = match_3x3(&g).expect("iron pickaxe");
        assert_eq!((m.0, m.1), (18, 1));
    }

    #[test]
    fn shaped_axe_mirrors_horizontally() {
        // MC axe pattern (shrunk 2x3): XX / X# / _#. The mirrored
        // anchoring must also match.
        let direct: Grid3 = [
            cell(43, 1), cell(43, 1), None,
            cell(43, 1), cell(302, 1), None,
            None, cell(302, 1), None,
        ];
        let m = match_3x3(&direct).expect("axe direct");
        assert_eq!(m.0, 34, "iron axe (legacy row)");
        let mirrored: Grid3 = [
            cell(43, 1), cell(43, 1), None,
            cell(302, 1), cell(43, 1), None,
            cell(302, 1), None, None,
        ];
        assert_eq!(match_3x3(&mirrored).expect("axe mirrored").0, 34);
    }

    #[test]
    fn shaped_extra_item_outside_bbox_rejects() {
        // Crafting-table corner + a stray plank at (2,2): the bbox
        // grows to 3x3 and no 3x3 pattern matches this layout.
        let g: Grid3 = [
            cell(7, 1), cell(7, 1), None,
            cell(7, 1), cell(7, 1), None,
            None, None, cell(7, 1),
        ];
        assert!(match_shaped_in(&g[..], 3).is_none());
    }

    #[test]
    fn two_by_two_gate_chest_no_sticks_yes() {
        // Sticks (1x2 pattern) fit the 2x2 grid...
        let g: Grid2 = [cell(7, 1), None, cell(7, 1), None];
        let m = match_2x2(&g).expect("sticks in 2x2");
        assert_eq!((m.0, m.1), (302, 4));
        // ...and the crafting table (2x2) does too.
        let g: Grid2 = [cell(7, 1), cell(7, 1), cell(7, 1), cell(7, 1)];
        assert_eq!(match_2x2(&g).expect("table in 2x2").0, 553);
        // The chest (3x3 ring) can never match a 2x2 grid; no other
        // pure-plank recipe may produce a chest either.
        for g in [
            [cell(7, 1), cell(7, 1), cell(7, 1), None],
            [cell(7, 1), cell(7, 1), None, cell(7, 1)],
        ] {
            assert!(match_2x2(&g).is_none_or(|m| m.0 != 41), "no 2x2 chest");
        }
    }

    #[test]
    fn torch_takes_coal_or_charcoal_over_stick() {
        for fuel in [325u16, 993] {
            let g: Grid3 = [
                cell(fuel, 1), None, None,
                cell(302, 1), None, None,
                None, None, None,
            ];
            let m = match_3x3(&g).expect("torch");
            assert_eq!((m.0, m.1), (10, 4), "fuel {fuel}");
        }
        // The 1x2 pattern also fits (and slides in) the 2x2 grid.
        let g: Grid2 = [cell(325, 1), None, cell(302, 1), None];
        assert_eq!(match_2x2(&g).expect("torch 2x2").0, 10);
    }

    #[test]
    fn shapeless_hay_block_is_nine_wheat_and_back() {
        let g: Grid3 = std::array::from_fn(|_| cell(337, 1));
        let m = match_3x3(&g).expect("hay block");
        assert_eq!((m.0, m.1), (435, 1));
        // Reverse: 1 hay block -> 9 wheat (fits the 2x2 grid too).
        let g: Grid2 = [cell(435, 1), None, None, None];
        let m = match_2x2(&g).expect("wheat back");
        assert_eq!((m.0, m.1), (337, 9));
        // 4 wheat is NOT a recipe any more (the old hand stand-in).
        let g: Grid2 = [cell(337, 1), cell(337, 1), cell(337, 1), cell(337, 1)];
        assert!(match_2x2(&g).is_none_or(|m| m.0 != 435));
    }

    #[test]
    fn shapeless_order_is_irrelevant_and_extras_reject() {
        // Mushroom stew style check via oak planks: 1 oak log ->
        // planks in any cell.
        for i in 0..9 {
            let mut g: Grid3 = [None; 9];
            g[i] = cell(5, 1);
            let m = match_3x3(&g).expect("planks from log");
            assert_eq!((m.0, m.1), (7, 4));
        }
        // Log + an extra stick: the multiset has no 2-slot recipe
        // for it, and the shaped table must not fire either.
        let g: Grid3 = [
            cell(5, 1), cell(302, 1), None,
            None, None, None,
            None, None, None,
        ];
        assert!(match_3x3(&g).is_none());
    }

    #[test]
    fn shaped_bed_from_wool_and_planks() {
        // white_bed: 3 wool (11 = white via the LOOT funnel) over
        // 3 planks -> 576 (the W366 sleepable bed family).
        let g: Grid3 = [
            cell(11, 1), cell(11, 1), cell(11, 1),
            cell(7, 1), cell(7, 1), cell(7, 1),
            None, None, None,
        ];
        let m = match_3x3(&g).expect("white bed");
        assert_eq!((m.0, m.1), (576, 1));
    }

    #[test]
    fn shaped_bricks_block_from_brick_items() {
        let g: Grid2 = [cell(675, 1), cell(675, 1), cell(675, 1), cell(675, 1)];
        let m = match_2x2(&g).expect("bricks");
        assert_eq!((m.0, m.1), (86, 1));
    }

    #[test]
    fn shapeless_bipartite_overlap_backtracks() {
        // Dye-cyan-wool: [cyan_dye 668, any-wool-but-cyan]. Put the
        // dye in the SECOND cell so a greedy first-fit would try to
        // pay the wool slot with it and must backtrack.
        let g: Grid2 = [cell(11, 1), cell(668, 1), None, None];
        let m = match_2x2(&g).expect("dye cyan wool");
        assert_eq!((m.0, m.1), (105, 1));
    }

    #[test]
    fn empty_grid_matches_nothing() {
        assert!(match_3x3(&[None; 9]).is_none());
        assert!(match_2x2(&[None; 4]).is_none());
    }

    #[test]
    fn no_recipe_consumes_the_legacy_ore_coal_id() {
        // W477: the id-13 ore-coal hybrid is dead; W478 regenerated
        // the tables from MC data, which never reference it.
        for r in SHAPED {
            assert!(
                r.cells.iter().all(|c| !c.contains(&13)),
                "{} consumes id 13",
                r.name
            );
        }
        for r in SHAPELESS {
            assert!(
                r.ingredients.iter().all(|c| !c.contains(&13)),
                "{} consumes id 13",
                r.name
            );
        }
    }

    #[test]
    fn generated_tables_are_well_formed() {
        for r in SHAPED {
            assert!(r.w >= 1 && r.w <= 3 && r.h >= 1 && r.h <= 3, "{}", r.name);
            assert_eq!(
                r.cells.len(),
                r.w as usize * r.h as usize,
                "{} cell count",
                r.name
            );
            assert!(r.ingredient_count() >= 1, "{} all-empty", r.name);
            assert!(r.output.1 >= 1, "{} zero count", r.name);
        }
        for r in SHAPELESS {
            let n = r.ingredients.len();
            assert!((1..=9).contains(&n), "{} ingredient count", r.name);
            assert!(
                r.ingredients.iter().all(|i| !i.is_empty()),
                "{} empty slot",
                r.name
            );
            assert!(r.output.1 >= 1, "{} zero count", r.name);
        }
    }
}
