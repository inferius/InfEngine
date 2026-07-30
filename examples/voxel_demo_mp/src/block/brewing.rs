//! W50.5 - brewing stand block: 5-slot container + brew tick.
//!
//! Slot layout (locked to MC's order so future save-compat is
//! straightforward):
//! - slot 0: ingredient (single item consumed per batch).
//! - slot 1: fuel (blaze powder). For the demo this slot is
//!   advisory only - the tick burns time without consuming fuel
//!   so the player can experiment without grinding nether
//!   trips first. A future wave can gate the tick on slot 1.
//! - slots 2..=4: base bottles. The transform runs in parallel
//!   on each non-empty matching bottle.
//!
//! Cook duration matches MC's 400 ticks (20 s at 20 TPS, i.e.
//! `400 * TICK_MS = 20000 ms`).

#![allow(dead_code)]

use std::collections::HashMap;

use crate::container::Container;
use crate::recipes::brewing::{BREWING_RECIPES, brewing_for};

pub(crate) const BREWING_SLOTS: usize = 5;
pub(crate) const SLOT_INGREDIENT: usize = 0;
pub(crate) const SLOT_FUEL: usize = 1;
pub(crate) const SLOT_BOTTLE_0: usize = 2;
pub(crate) const SLOT_BOTTLE_1: usize = 3;
pub(crate) const SLOT_BOTTLE_2: usize = 4;

/// Game tick length in ms (shared with the furnace runtime).
pub(crate) const TICK_MS: u32 = 50;
/// 400 ticks = 20 s at 20 TPS.
pub(crate) const BREW_TICKS_PER_OUTPUT: u32 = 400;
pub(crate) const BREW_MS_PER_OUTPUT: u32 = BREW_TICKS_PER_OUTPUT * TICK_MS;

/// Live runtime per brewing stand. Tracks accumulated brewing
/// progress in milliseconds; when it tops the threshold the
/// tick consumes one ingredient + transforms every matching
/// bottle.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct BrewingRuntime {
    pub progress_ms: u32,
}

/// Accept-filter: ingredient slot. Allows any item that appears
/// as `ingredient` in a brewing recipe.
pub(crate) fn accept_ingredient(id: u16) -> bool {
    BREWING_RECIPES.iter().any(|r| r.ingredient == id)
}

/// Accept-filter: fuel slot. Demo accepts any item (the tick
/// ignores fuel for now; this leaves the slot usable as a real
/// blaze-powder gate in a follow-up).
pub(crate) fn accept_fuel(_id: u16) -> bool {
    true
}

/// Accept-filter: bottle slots. Allows any item that appears as
/// `base` in a brewing recipe (water bottle, awkward potion, ...).
pub(crate) fn accept_bottle(id: u16) -> bool {
    BREWING_RECIPES.iter().any(|r| r.base == id)
}

/// Build a fresh empty brewing stand container with the per-slot
/// accept filters wired up.
pub(crate) fn new_brewing_stand() -> Container {
    let mut c = Container::with_size(BREWING_SLOTS);
    c.set_accept(SLOT_INGREDIENT, accept_ingredient);
    c.set_accept(SLOT_FUEL, accept_fuel);
    c.set_accept(SLOT_BOTTLE_0, accept_bottle);
    c.set_accept(SLOT_BOTTLE_1, accept_bottle);
    c.set_accept(SLOT_BOTTLE_2, accept_bottle);
    c
}

/// Insert a fresh stand at `pos` (no-op if one exists). Mirrors
/// `furnace::place_furnace` so the map shapes stay aligned.
pub(crate) fn place_brewing_stand(
    stands: &mut HashMap<[i32; 3], Container>,
    runtimes: &mut HashMap<[i32; 3], BrewingRuntime>,
    pos: [i32; 3],
) {
    stands.entry(pos).or_insert_with(new_brewing_stand);
    runtimes.entry(pos).or_default();
}

/// Remove the stand at `pos` for break-drops. W468: returns one
/// FULL stack per occupied slot (counts + components intact).
pub(crate) fn break_brewing_stand(
    stands: &mut HashMap<[i32; 3], Container>,
    runtimes: &mut HashMap<[i32; 3], BrewingRuntime>,
    pos: [i32; 3],
) -> Vec<crate::item::ItemStack> {
    runtimes.remove(&pos);
    let Some(stand) = stands.remove(&pos) else {
        return Vec::new();
    };
    stand.slots().iter().flatten().cloned().collect()
}

/// Advance every active brewing stand by `dt_ms`.
///
/// Per-stand algorithm:
/// 1. Inspect the ingredient slot (slot 0). If empty, reset
///    progress to 0 + skip.
/// 2. Scan bottles (slots 2..=4) for any whose contents combine
///    with the ingredient to a known `BrewingRecipe`. If none,
///    reset progress + skip.
/// 3. Add `dt_ms` to `progress_ms`. While `progress_ms >=
///    BREW_MS_PER_OUTPUT`, transform every matching bottle (one
///    consumed ingredient covers all 3 bottles in parallel) and
///    decrement the ingredient slot. Preserve the overflow on
///    `progress_ms` so a long frame doesn't lose fractional
///    progress.
pub(crate) fn brewing_tick(
    stands: &mut HashMap<[i32; 3], Container>,
    runtimes: &mut HashMap<[i32; 3], BrewingRuntime>,
    dt_ms: u32,
) {
    if dt_ms == 0 {
        return;
    }
    let positions: Vec<[i32; 3]> = stands.keys().copied().collect();
    for pos in positions {
        let rt = runtimes.entry(pos).or_default();
        let Some(stand) = stands.get_mut(&pos) else {
            continue;
        };
        step_one_stand(stand, rt, dt_ms);
    }
}

/// Single-stand brew step. Mutates `stand` slots + `runtime`
/// in place.
pub(crate) fn step_one_stand(
    stand: &mut Container,
    runtime: &mut BrewingRuntime,
    dt_ms: u32,
) {
    let Some(ingredient_id) =
        stand.slot_ref(SLOT_INGREDIENT).map(|st| st.id)
    else {
        runtime.progress_ms = 0;
        return;
    };
    // Find which bottle slots match the current ingredient.
    let mut matched: Vec<(usize, u16)> = Vec::with_capacity(3);
    for slot_idx in [SLOT_BOTTLE_0, SLOT_BOTTLE_1, SLOT_BOTTLE_2] {
        if let Some(base_id) = stand.slot_ref(slot_idx).map(|st| st.id)
            && let Some(recipe) = brewing_for(ingredient_id, base_id)
        {
            matched.push((slot_idx, recipe.output));
        }
    }
    if matched.is_empty() {
        runtime.progress_ms = 0;
        return;
    }
    runtime.progress_ms = runtime.progress_ms.saturating_add(dt_ms);
    while runtime.progress_ms >= BREW_MS_PER_OUTPUT {
        // Re-check ingredient + matches each iteration in case
        // the previous loop emptied something.
        let Some(cur_ing) = stand.slot(SLOT_INGREDIENT) else {
            runtime.progress_ms = 0;
            break;
        };
        let mut transforms: Vec<(usize, u16)> = Vec::with_capacity(3);
        for slot_idx in [SLOT_BOTTLE_0, SLOT_BOTTLE_1, SLOT_BOTTLE_2] {
            if let Some(base_id) = stand.slot_ref(slot_idx).map(|st| st.id)
                && let Some(recipe) = brewing_for(cur_ing.id, base_id)
            {
                transforms.push((slot_idx, recipe.output));
            }
        }
        if transforms.is_empty() {
            runtime.progress_ms = 0;
            break;
        }
        runtime.progress_ms -= BREW_MS_PER_OUTPUT;
        // Transform each matching bottle (count preserved). The
        // brewed potion is a NEW item - fresh empty components.
        for (slot_idx, output_id) in transforms {
            let count = stand.slot_ref(slot_idx).map(|st| st.count).unwrap_or(1);
            stand.set_slot(
                slot_idx,
                Some(crate::item::ItemStack::new(output_id, count)),
            );
        }
        // Consume one ingredient (components preserved on the
        // remainder).
        let new_count = cur_ing.count.saturating_sub(1);
        if new_count == 0 {
            stand.set_slot(SLOT_INGREDIENT, None);
        } else {
            stand.set_slot(SLOT_INGREDIENT, Some(cur_ing.with_count(new_count)));
        }
    }
}

/// Recover the held cursor stack into the stand when the UI
/// closes. Returns the leftover stack (components intact - W468)
/// the caller spawns as a floor drop at the stand position.
pub(crate) fn recover_held_on_close(
    stand: Option<&mut Container>,
    held: Option<crate::item::ItemStack>,
) -> Option<crate::item::ItemStack> {
    let stack = held?;
    let leftover = match stand {
        Some(c) => c.insert(stack.clone()),
        None => stack.count,
    };
    (leftover > 0).then(|| stack.with_count(leftover))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::ItemStack;
    use crate::recipes::brewing::BREWING_RECIPES;

    #[test]
    fn brewing_stand_starts_empty() {
        let c = new_brewing_stand();
        assert_eq!(c.len(), BREWING_SLOTS);
        assert!(c.is_empty());
    }

    #[test]
    fn idle_stand_does_not_advance_progress() {
        let mut stand = new_brewing_stand();
        let mut rt = BrewingRuntime::default();
        step_one_stand(&mut stand, &mut rt, 1000);
        assert_eq!(rt.progress_ms, 0);
    }

    #[test]
    fn ingredient_plus_matching_base_transforms_after_brew_ms() {
        // Use the demo BREWING_RECIPES first entry: 700 + 701 ->
        // 702. Stage the stand with ingredient + a matching base
        // bottle, tick past the threshold, check the output.
        let recipe = &BREWING_RECIPES[0];
        let mut stand = new_brewing_stand();
        stand.set_slot(SLOT_INGREDIENT, Some(ItemStack::new(recipe.ingredient, 1)));
        stand.set_slot(SLOT_BOTTLE_0, Some(ItemStack::new(recipe.base, 1)));
        let mut rt = BrewingRuntime::default();
        // One big step covering the full brew duration plus
        // overflow.
        step_one_stand(&mut stand, &mut rt, BREW_MS_PER_OUTPUT + 100);
        assert_eq!(stand.slot_ref(SLOT_BOTTLE_0).map(|st| st.id), Some(recipe.output));
        // Ingredient consumed (was 1).
        assert!(stand.slot(SLOT_INGREDIENT).is_none());
        // Overflow preserved.
        assert_eq!(rt.progress_ms, 100);
    }

    #[test]
    fn three_bottles_transform_in_parallel() {
        let recipe = &BREWING_RECIPES[0];
        let mut stand = new_brewing_stand();
        stand.set_slot(SLOT_INGREDIENT, Some(ItemStack::new(recipe.ingredient, 1)));
        stand.set_slot(SLOT_BOTTLE_0, Some(ItemStack::new(recipe.base, 1)));
        stand.set_slot(SLOT_BOTTLE_1, Some(ItemStack::new(recipe.base, 1)));
        stand.set_slot(SLOT_BOTTLE_2, Some(ItemStack::new(recipe.base, 1)));
        let mut rt = BrewingRuntime::default();
        step_one_stand(&mut stand, &mut rt, BREW_MS_PER_OUTPUT);
        for slot in [SLOT_BOTTLE_0, SLOT_BOTTLE_1, SLOT_BOTTLE_2] {
            assert_eq!(
                stand.slot_ref(slot).map(|st| st.id),
                Some(recipe.output)
            );
        }
        // One ingredient covers all three bottles.
        assert!(stand.slot(SLOT_INGREDIENT).is_none());
    }

    #[test]
    fn no_match_resets_progress() {
        let recipe = &BREWING_RECIPES[0];
        let mut stand = new_brewing_stand();
        // Ingredient with no matching base.
        stand.set_slot(SLOT_INGREDIENT, Some(ItemStack::new(recipe.ingredient, 1)));
        stand.set_slot(SLOT_BOTTLE_0, Some(ItemStack::new(9999, 1)));
        let mut rt = BrewingRuntime { progress_ms: 1234 };
        step_one_stand(&mut stand, &mut rt, 100);
        assert_eq!(rt.progress_ms, 0);
    }

    #[test]
    fn empty_ingredient_resets_progress() {
        let mut stand = new_brewing_stand();
        let mut rt = BrewingRuntime { progress_ms: 5000 };
        step_one_stand(&mut stand, &mut rt, 100);
        assert_eq!(rt.progress_ms, 0);
    }

    #[test]
    fn break_returns_all_slot_contents_as_drops() {
        let recipe = &BREWING_RECIPES[0];
        let pos = [3, 4, 5];
        let mut stands: HashMap<[i32; 3], Container> = HashMap::new();
        let mut runtimes: HashMap<[i32; 3], BrewingRuntime> = HashMap::new();
        place_brewing_stand(&mut stands, &mut runtimes, pos);
        let stand = stands.get_mut(&pos).unwrap();
        stand.set_slot(SLOT_INGREDIENT, Some(ItemStack::new(recipe.ingredient, 2)));
        stand.set_slot(SLOT_BOTTLE_0, Some(ItemStack::new(recipe.base, 3)));
        let drops = break_brewing_stand(&mut stands, &mut runtimes, pos);
        // W468: one full stack per occupied slot, counts intact.
        assert_eq!(drops.len(), 2);
        assert!(drops.contains(&ItemStack::new(recipe.ingredient, 2)));
        assert!(drops.contains(&ItemStack::new(recipe.base, 3)));
        assert!(!stands.contains_key(&pos));
        assert!(!runtimes.contains_key(&pos));
    }
}
