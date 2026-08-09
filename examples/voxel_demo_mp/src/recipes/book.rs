//! W478 - the unified recipe book over the generated tables.
//!
//! - `full_book()`: every live shaped + shapeless recipe as one
//!   indexable list (the 3x3 crafting-table book). First access
//!   emits the once-per-process skip summary (the generated tables
//!   carry the full `// skip:` detail).
//! - `book_2x2()`: the subset that fits the 2x2 inventory craft
//!   grid (`canCraftInDimensions(2, 2)` for shaped rows, <= 4
//!   ingredient slots for shapeless). The recipe-book UI and the
//!   `vx.recipe` wire auto-fill index into THIS list, so a client
//!   can never request a 3x3-only recipe into the 2x2 grid.
//! - `fill_craft_grid_2x2`: the shared auto-fill used by the local
//!   (offline / host) path and the server-side `vx.recipe` arm -
//!   shaped ingredients land in their pattern positions, each cell
//!   pays with the first acceptable member the player carries
//!   (set order, so e.g. coal outranks charcoal), components ride
//!   on the moved items, and a short inventory aborts without side
//!   effects.

use std::sync::OnceLock;

use super::crafting_table::{
    SHAPED, SHAPELESS, SKIPPED_RECIPES, SPECIAL_TYPES_SKIPPED,
};
use super::stonecutting_table::{SKIPPED_CUTS, STONECUTTER_RECIPES};
use super::{Ingredient, ShapedRecipe, ShapelessRecipe};
use crate::item::ItemStack;
use crate::item::stack::drop_into_first_free_stacks;

/// One entry of the unified book: a reference into either generated
/// table.
#[derive(Clone, Copy)]
pub(crate) enum AnyRecipe {
    Shaped(&'static ShapedRecipe),
    Shapeless(&'static ShapelessRecipe),
}

impl AnyRecipe {
    pub(crate) fn output(&self) -> (u16, u8) {
        match self {
            AnyRecipe::Shaped(r) => r.output,
            AnyRecipe::Shapeless(r) => r.output,
        }
    }

    pub(crate) fn name(&self) -> &'static str {
        match self {
            AnyRecipe::Shaped(r) => r.name,
            AnyRecipe::Shapeless(r) => r.name,
        }
    }

    /// Ingredient layout on a 3x3 grid: shaped patterns anchor
    /// top-left; shapeless slots spread across the first cells.
    pub(crate) fn cells_3x3(&self) -> [Option<Ingredient>; 9] {
        let mut out: [Option<Ingredient>; 9] = [None; 9];
        match self {
            AnyRecipe::Shaped(r) => {
                for y in 0..r.h as usize {
                    for x in 0..r.w as usize {
                        let ing = r.cells[y * r.w as usize + x];
                        if !ing.is_empty() {
                            out[y * 3 + x] = Some(ing);
                        }
                    }
                }
            }
            AnyRecipe::Shapeless(r) => {
                for (i, ing) in r.ingredients.iter().enumerate().take(9) {
                    out[i] = Some(*ing);
                }
            }
        }
        out
    }

    /// Ingredient layout on the 2x2 grid, or `None` when the recipe
    /// needs the full crafting table.
    pub(crate) fn cells_2x2(&self) -> Option<[Option<Ingredient>; 4]> {
        let mut out: [Option<Ingredient>; 4] = [None; 4];
        match self {
            AnyRecipe::Shaped(r) => {
                if !r.fits(2, 2) {
                    return None;
                }
                for y in 0..r.h as usize {
                    for x in 0..r.w as usize {
                        let ing = r.cells[y * r.w as usize + x];
                        if !ing.is_empty() {
                            out[y * 2 + x] = Some(ing);
                        }
                    }
                }
            }
            AnyRecipe::Shapeless(r) => {
                if r.ingredients.len() > 4 {
                    return None;
                }
                for (i, ing) in r.ingredients.iter().enumerate() {
                    out[i] = Some(*ing);
                }
            }
        }
        Some(out)
    }
}

/// Once-per-process summary of what the generator could not resolve.
/// The `// skip:` comments in the generated files carry the detail.
fn log_skip_summary() {
    let special_jsons: u16 =
        SPECIAL_TYPES_SKIPPED.iter().map(|(_, n)| *n).sum();
    tracing::info!(
        shaped = SHAPED.len(),
        shapeless = SHAPELESS.len(),
        cuts = STONECUTTER_RECIPES.len(),
        skipped_crafting = SKIPPED_RECIPES.len(),
        skipped_cuts = SKIPPED_CUTS.len(),
        special_types = SPECIAL_TYPES_SKIPPED.len(),
        special_jsons,
        "W478: MC recipe tables loaded; unresolved + special rows skipped"
    );
}

/// Every live recipe (shaped first, then shapeless, table order).
pub(crate) fn full_book() -> &'static [AnyRecipe] {
    static BOOK: OnceLock<Vec<AnyRecipe>> = OnceLock::new();
    BOOK.get_or_init(|| {
        log_skip_summary();
        SHAPED
            .iter()
            .map(AnyRecipe::Shaped)
            .chain(SHAPELESS.iter().map(AnyRecipe::Shapeless))
            .collect()
    })
}

/// The 2x2-capable subset (stable indices - the `vx.recipe` wire
/// message addresses this list).
pub(crate) fn book_2x2() -> &'static [AnyRecipe] {
    static BOOK: OnceLock<Vec<AnyRecipe>> = OnceLock::new();
    BOOK.get_or_init(|| {
        full_book()
            .iter()
            .copied()
            .filter(|r| r.cells_2x2().is_some())
            .collect()
    })
}

/// Pull ONE item some `set` member accepts out of the inventory:
/// members probe in set order (exact-id stacks first in the tag's
/// declared order), slots in inventory order. The taken single
/// clones the source stack's components. Shared with the 3x3
/// recipe-book autofill (`ui::crafting_panel`).
pub(crate) fn take_one_of(
    inv: &mut [Option<ItemStack>],
    set: Ingredient,
) -> Option<ItemStack> {
    for want in set {
        for slot in inv.iter_mut() {
            if let Some(st) = slot
                && st.id == *want
                && st.count >= 1
            {
                let one = st.with_count(1);
                if st.count == 1 {
                    *slot = None;
                } else {
                    st.count -= 1;
                }
                return Some(one);
            }
        }
    }
    None
}

/// Auto-fill the 2x2 craft grid from `book_2x2()[idx]`: every
/// ingredient lands in its pattern cell, paid from `inv`. Returns
/// `false` (inventory + grid untouched) when the index is bad or
/// any slot cannot be paid; on success the previous grid contents
/// return to the inventory (drops are the caller's concern only for
/// a FULL inventory, same as the panel close path).
pub(crate) fn fill_craft_grid_2x2(
    inv: &mut [Option<ItemStack>],
    grid: &mut [Option<ItemStack>; 4],
    idx: usize,
) -> bool {
    let Some(recipe) = book_2x2().get(idx) else {
        return false;
    };
    let Some(spec) = recipe.cells_2x2() else {
        return false;
    };
    // Pay every cell from a WORKING copy so a shortfall rolls back
    // by simply dropping the copy.
    let mut work: Vec<Option<ItemStack>> = inv.to_vec();
    let mut placed: [Option<ItemStack>; 4] = std::array::from_fn(|_| None);
    for (i, slot) in spec.iter().enumerate() {
        let Some(set) = slot else { continue };
        match take_one_of(&mut work, set) {
            Some(st) => placed[i] = Some(st),
            None => return false,
        }
    }
    // Commit the drained inventory, then return whatever sat in the
    // grid (components intact) before writing the new layout.
    for (dst, src) in inv.iter_mut().zip(work) {
        *dst = src;
    }
    for cell in grid.iter_mut() {
        if let Some(st) = cell.take() {
            let _ = drop_into_first_free_stacks(inv, st);
        }
    }
    *grid = placed;
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find_2x2(name: &str) -> Option<usize> {
        book_2x2().iter().position(|r| r.name() == name)
    }

    #[test]
    fn two_by_two_book_only_offers_2x2_capable_recipes() {
        assert!(!book_2x2().is_empty());
        for r in book_2x2() {
            match r {
                AnyRecipe::Shaped(s) => {
                    assert!(s.fits(2, 2), "{} in 2x2 book", s.name)
                }
                AnyRecipe::Shapeless(s) => assert!(
                    s.ingredients.len() <= 4,
                    "{} in 2x2 book",
                    s.name
                ),
            }
        }
        // The staples are offered...
        assert!(find_2x2("Crafting Table").is_some());
        assert!(find_2x2("Stick").is_some());
        assert!(find_2x2("Oak Planks").is_some());
        assert!(find_2x2("Torch").is_some());
        // ...and no 2x2 entry produces a chest (the ring is 3x3).
        assert!(
            book_2x2().iter().all(|r| r.output().0 != 41),
            "chest crept into the 2x2 book"
        );
        // The full book still carries it.
        assert!(full_book().iter().any(|r| r.output().0 == 41));
    }

    #[test]
    fn fill_places_shaped_pattern_positions() {
        // Stick = 1x2 planks column: pattern cells (0,0) + (1,0)
        // map to grid cells 0 and 2.
        let idx = find_2x2("Stick").expect("stick recipe");
        let mut inv: Vec<Option<ItemStack>> = vec![None; 9];
        inv[3] = Some(ItemStack::new(7, 5));
        let mut grid: [Option<ItemStack>; 4] = std::array::from_fn(|_| None);
        assert!(fill_craft_grid_2x2(&mut inv, &mut grid, idx));
        assert_eq!(grid[0], Some(ItemStack::new(7, 1)));
        assert!(grid[1].is_none());
        assert_eq!(grid[2], Some(ItemStack::new(7, 1)));
        assert!(grid[3].is_none());
        assert_eq!(inv[3], Some(ItemStack::new(7, 3)));
        // The filled grid actually matches the recipe it came from.
        let cells: [Option<(u16, u8)>; 4] =
            std::array::from_fn(|i| grid[i].as_ref().map(|s| (s.id, s.count)));
        let m = super::super::matcher::match_2x2(&cells).expect("match");
        assert_eq!((m.0, m.1), (302, 4));
    }

    #[test]
    fn fill_prefers_earlier_set_members() {
        // Torch fuel slot is [coal, charcoal]: with both in the
        // inventory the fill pays with coal (325) first.
        let idx = find_2x2("Torch").expect("torch recipe");
        let mut inv: Vec<Option<ItemStack>> = vec![None; 9];
        inv[0] = Some(ItemStack::new(993, 8)); // charcoal earlier slot
        inv[1] = Some(ItemStack::new(325, 8)); // coal later slot
        inv[2] = Some(ItemStack::new(302, 8)); // sticks
        let mut grid: [Option<ItemStack>; 4] = std::array::from_fn(|_| None);
        assert!(fill_craft_grid_2x2(&mut inv, &mut grid, idx));
        assert_eq!(grid[0].as_ref().map(|s| s.id), Some(325), "coal first");
        assert_eq!(grid[2].as_ref().map(|s| s.id), Some(302));
        assert_eq!(inv[1].as_ref().map(|s| s.count), Some(7));
        assert_eq!(inv[0].as_ref().map(|s| s.count), Some(8), "charcoal kept");
    }

    #[test]
    fn fill_shortfall_leaves_everything_untouched() {
        let idx = find_2x2("Crafting Table").expect("table recipe");
        let mut inv: Vec<Option<ItemStack>> = vec![None; 9];
        inv[0] = Some(ItemStack::new(7, 3)); // needs 4 planks
        let mut grid: [Option<ItemStack>; 4] = std::array::from_fn(|_| None);
        grid[1] = Some(ItemStack::new(1, 2));
        assert!(!fill_craft_grid_2x2(&mut inv, &mut grid, idx));
        assert_eq!(inv[0], Some(ItemStack::new(7, 3)));
        assert_eq!(grid[1], Some(ItemStack::new(1, 2)));
        // Bad index is an equally clean no-op.
        assert!(!fill_craft_grid_2x2(&mut inv, &mut grid, usize::MAX));
    }

    #[test]
    fn fill_returns_prior_grid_contents_to_inventory() {
        let idx = find_2x2("Crafting Table").expect("table recipe");
        let mut inv: Vec<Option<ItemStack>> = vec![None; 9];
        inv[0] = Some(ItemStack::new(7, 6));
        let mut grid: [Option<ItemStack>; 4] = std::array::from_fn(|_| None);
        grid[0] = Some(ItemStack::new(1, 5)); // stone parked in the grid
        assert!(fill_craft_grid_2x2(&mut inv, &mut grid, idx));
        for cell in &grid {
            assert_eq!(cell.as_ref().map(|s| s.id), Some(7));
        }
        assert_eq!(inv[0].as_ref().map(|s| s.count), Some(2));
        assert!(
            inv.iter()
                .any(|s| s.as_ref().is_some_and(|st| st.id == 1 && st.count == 5)),
            "parked stone returned to the inventory"
        );
    }

    #[test]
    fn book_indices_are_stable_across_calls() {
        let a = book_2x2().len();
        let b = book_2x2().len();
        assert_eq!(a, b);
        assert!(a >= 100, "expected a rich 2x2 book, got {a}");
        assert!(full_book().len() >= book_2x2().len());
    }
}
