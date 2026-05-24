//! W50.2 - 3x3 grid recipe matcher (shaped + shapeless).
//!
//! Pipeline:
//! 1. Try every shaped recipe at every slide offset (+ horizontal
//!    flip).
//! 2. Fall back to shapeless 2x2 RECIPES against the 3x3 grid
//!    (tally-based; same as `ui::match_recipe_named` but on a
//!    9-slot input).

#![allow(dead_code, clippy::needless_range_loop)]

use super::RECIPES;
use super::shaped::{SHAPED_RECIPES, ShapedRecipe, pattern_bbox};

/// A 3x3 craft grid, row-major. `None` = empty.
pub(crate) type Grid3 = [Option<(u16, u8)>; 9];

#[inline]
fn cell(g: &Grid3, r: usize, c: usize) -> Option<(u16, u8)> {
    g[r * 3 + c]
}

/// Try to match a shaped recipe at every legal anchor offset
/// (and its horizontal flip). Returns the output stack + recipe
/// name on success.
pub(crate) fn match_shaped(g: &Grid3) -> Option<(u16, u8, &'static str)> {
    for recipe in SHAPED_RECIPES {
        if let Some(hit) = try_shaped(g, &recipe.pattern, recipe) {
            return Some(hit);
        }
        // Horizontal flip - MC accepts mirrored anchoring.
        let mut flipped = [[0u16; 3]; 3];
        for r in 0..3 {
            for c in 0..3 {
                flipped[r][c] = recipe.pattern[r][2 - c];
            }
        }
        if let Some(hit) = try_shaped(g, &flipped, recipe) {
            return Some(hit);
        }
    }
    None
}

fn try_shaped(
    g: &Grid3,
    pattern: &[[u16; 3]; 3],
    recipe: &'static ShapedRecipe,
) -> Option<(u16, u8, &'static str)> {
    let (br, bc, bh, bw) = pattern_bbox(pattern)?;
    // Slide offsets that keep the bbox inside the 3x3 grid.
    let max_dr = 3 - bh;
    let max_dc = 3 - bw;
    for dr in 0..=max_dr {
        for dc in 0..=max_dc {
            if grid_matches(g, pattern, br, bc, bh, bw, dr, dc) {
                return Some((recipe.output.0, recipe.output.1, recipe.name));
            }
        }
    }
    None
}

#[allow(clippy::too_many_arguments)]
fn grid_matches(
    g: &Grid3,
    pattern: &[[u16; 3]; 3],
    br: usize,
    bc: usize,
    bh: usize,
    bw: usize,
    dr: usize,
    dc: usize,
) -> bool {
    // Walk every grid cell. Cells inside the slid bbox must
    // match the corresponding pattern cell (id + presence).
    // Cells outside the slid bbox must be empty.
    for r in 0..3 {
        for c in 0..3 {
            let inside_r = r >= dr && r < dr + bh;
            let inside_c = c >= dc && c < dc + bw;
            if inside_r && inside_c {
                // Map grid cell into pattern coords.
                let pr = br + (r - dr);
                let pc = bc + (c - dc);
                let want = pattern[pr][pc];
                if want == 0 {
                    if cell(g, r, c).is_some() {
                        return false;
                    }
                } else {
                    match cell(g, r, c) {
                        Some((id, n)) if id == want && n >= 1 => {}
                        _ => return false,
                    }
                }
            } else if cell(g, r, c).is_some() {
                return false;
            }
        }
    }
    true
}

/// Shapeless 3x3 match against the existing `RECIPES` table.
/// Tally aggregation - order doesn't matter.
pub(crate) fn match_shapeless(g: &Grid3) -> Option<(u16, u8, &'static str)> {
    let mut tally: Vec<(u16, u8)> = Vec::with_capacity(9);
    for cell in g.iter().flatten() {
        if let Some(slot) = tally.iter_mut().find(|(bb, _)| *bb == cell.0) {
            slot.1 = slot.1.saturating_add(cell.1);
        } else {
            tally.push((cell.0, cell.1));
        }
    }
    for recipe in RECIPES {
        let mut needed: Vec<(u16, u8)> = Vec::with_capacity(recipe.inputs.len());
        for (b, c) in recipe.inputs {
            if let Some(slot) = needed.iter_mut().find(|(bb, _)| *bb == *b) {
                slot.1 = slot.1.saturating_add(*c);
            } else {
                needed.push((*b, *c));
            }
        }
        if needed.len() != tally.len() {
            continue;
        }
        let mut ok = true;
        for (b, n) in &needed {
            match tally.iter().find(|(bb, _)| bb == b) {
                Some((_, have)) if *have >= *n => {}
                _ => {
                    ok = false;
                    break;
                }
            }
        }
        if ok {
            return Some((recipe.output.0, recipe.output.1, recipe.name));
        }
    }
    None
}

/// Top-level entry point for the 3x3 crafting table. Shaped
/// recipes take precedence (more specific). Falls back to the
/// shapeless table.
pub(crate) fn match_3x3(g: &Grid3) -> Option<(u16, u8, &'static str)> {
    match_shaped(g).or_else(|| match_shapeless(g))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(id: u16, n: u8) -> Option<(u16, u8)> {
        Some((id, n))
    }

    #[test]
    fn shaped_crafting_table_top_left() {
        let g: Grid3 = [
            cell(7, 1), cell(7, 1), None,
            cell(7, 1), cell(7, 1), None,
            None, None, None,
        ];
        let m = match_shaped(&g).expect("crafting table should match");
        assert_eq!(m.0, 553);
        assert_eq!(m.1, 1);
    }

    #[test]
    fn shaped_crafting_table_slid_to_center() {
        let g: Grid3 = [
            None, None, None,
            None, cell(7, 1), cell(7, 1),
            None, cell(7, 1), cell(7, 1),
        ];
        let m = match_shaped(&g).expect("crafting table should slide");
        assert_eq!(m.0, 553);
    }

    #[test]
    fn shaped_pickaxe_top_row_planks_handle() {
        let g: Grid3 = [
            cell(7, 1), cell(7, 1), cell(7, 1),
            None, cell(302, 1), None,
            None, cell(302, 1), None,
        ];
        let m = match_shaped(&g).expect("wood pickaxe should match");
        assert_eq!(m.0, 16);
    }

    #[test]
    fn shapeless_fallback_planks_to_sticks() {
        // 2 planks anywhere -> sticks (shapeless RECIPES has 2-plank
        // recipe; should fall through shaped + hit shapeless).
        let g: Grid3 = [
            cell(7, 1), None, None,
            None, cell(7, 1), None,
            None, None, None,
        ];
        let m = match_3x3(&g).expect("sticks shapeless match");
        assert_eq!(m.0, 302);
    }

    #[test]
    fn empty_grid_returns_none() {
        let g: Grid3 = [None; 9];
        assert!(match_3x3(&g).is_none());
    }

    #[test]
    fn extra_item_outside_bbox_blocks_shaped_match() {
        // Crafting-table corner + a stray plank at (2,2). The
        // shaped matcher should reject.
        let g: Grid3 = [
            cell(7, 1), cell(7, 1), None,
            cell(7, 1), cell(7, 1), None,
            None, None, cell(7, 1),
        ];
        assert!(match_shaped(&g).is_none());
    }
}
