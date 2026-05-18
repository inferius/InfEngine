//! Smelt-recipe + fuel lookup tables for the furnace block.
//!
//! Two static lookup fns: `smelt_recipe(input_id)` returns the
//! `(output_id, output_count)` pair for a smeltable input, and
//! `fuel_burn_ticks(item_id)` returns the per-item burn duration in
//! 50-ms game ticks (matches `world_save::timers::TICK_MS`).
//!
//! Both are intentionally narrow free fns rather than a table struct
//! - the furnace tick path queries them once per cook step, and the
//!   UI hover-tip will probe individual ids. A hash-map allocation
//!   per lookup would cost more than the match.
//!
//! Phase C.4 ships the seed recipes (cobble -> stone, iron ore ->
//! iron ingot) and the seed fuels (coal, planks, wood, sticks if
//! ever registered). Future commits append entries here without
//! touching the furnace runtime.
//!
//! Cross-module mapping:
//! - input + output ids are the same `u16` used by `item::ITEM_TYPES`
//!   and `block::BLOCK_TYPES`. The furnace runtime trusts the table -
//!   it does not verify that an item with id `output` actually
//!   exists; callers test that via the matching `item_type` lookup.
//! - the iron-ingot output points at item id `43`, the new item
//!   added in C.4 (no block form). See `item::ITEM_TYPES` for the
//!   row.

use crate::block::{CHEST_ID, COAL_ID, FURNACE_ID, IRON_INGOT_ID, IRON_ORE_ID, PLANKS_ID, WOOD_ID};

/// Resolve a smelt recipe.
///
/// Returns `Some((output_id, output_count))` if `input_id` has a
/// recipe registered, `None` otherwise. The runtime uses `None` as a
/// "this slot is not smeltable" gate: it stops the cook loop instead
/// of consuming fuel on a junk input.
///
/// Output count is currently always 1; the type leaves room for
/// multi-output recipes (e.g. sand -> 1 glass, but sand -> 2 sugar
/// in a different mod) without rewriting the callers.
pub(crate) fn smelt_recipe(input_id: u16) -> Option<(u16, u8)> {
    match input_id {
        // Cobblestone -> stone. Vanilla recipe, faithful 1:1.
        8 => Some((1, 1)),
        // Iron ore -> iron ingot. The output ID points at the new
        // item registered in `item::ITEM_TYPES` row 42.
        id if id == IRON_ORE_ID => Some((IRON_INGOT_ID, 1)),
        // Future recipes append here. Keep the entries grouped by
        // material category so the file stays readable.
        _ => None,
    }
}

/// Resolve a fuel's burn duration.
///
/// Returns the number of 50-ms game ticks the item burns for; `0`
/// means "not a fuel". Vanilla mappings:
/// - coal:   1600 ticks (80 s -> 8 outputs per coal).
/// - planks: 300 ticks (15 s -> 1.5 outputs per plank).
/// - wood:   300 ticks (matches planks: vanilla treats raw logs as
///   planks-equivalent fuel for the basic furnace).
///
/// The runtime multiplies these by `TICK_MS` to get the burn
/// duration in milliseconds. The catch-up dispatcher uses the same
/// constant on the offline path so the two units stay in sync.
pub(crate) fn fuel_burn_ticks(item_id: u16) -> u32 {
    match item_id {
        // Coal (the item-form drop of a coal-ore block).
        id if id == COAL_ID => 1600,
        // Planks: 300 ticks. Wood logs treated identically for now.
        id if id == PLANKS_ID => 300,
        id if id == WOOD_ID => 300,
        // Future fuels append here. The 0 default rejects every
        // other input via the accept filter on the furnace fuel
        // slot.
        _ => 0,
    }
}

/// True when `item_id` has a smelt recipe registered. Small helper
/// so the furnace fuel-slot accept filter can stay a `fn(u16) ->
/// bool` (no closures, since `AcceptFn` is a fn-pointer alias).
#[inline]
pub(crate) fn is_smeltable(item_id: u16) -> bool {
    smelt_recipe(item_id).is_some()
}

/// True when `item_id` burns. Mirror of `is_smeltable` for the fuel
/// slot.
#[inline]
pub(crate) fn is_fuel(item_id: u16) -> bool {
    fuel_burn_ticks(item_id) > 0
}

// Compile-time guard: the constants we reference here must resolve.
// The `_` bindings keep clippy quiet about unused statics.
const _: u16 = CHEST_ID;
const _: u16 = FURNACE_ID;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::{COAL_ID, IRON_INGOT_ID, IRON_ORE_ID, PLANKS_ID, WOOD_ID};

    #[test]
    fn smelt_cobble_to_stone() {
        // Cobble -> stone, 1 output.
        assert_eq!(smelt_recipe(8), Some((1, 1)));
    }

    #[test]
    fn smelt_iron_ore_to_iron_ingot() {
        let got = smelt_recipe(IRON_ORE_ID).expect("iron ore has a recipe");
        assert_eq!(got, (IRON_INGOT_ID, 1));
    }

    #[test]
    fn smelt_unknown_id_is_none() {
        assert!(smelt_recipe(0).is_none()); // air
        assert!(smelt_recipe(1).is_none()); // stone (already smelted)
        assert!(smelt_recipe(9999).is_none());
    }

    #[test]
    fn smelt_air_specifically() {
        // The runtime probes slot contents pre-filter, so the
        // `u16 = 0` (air / empty) lookup must NEVER return Some.
        assert_eq!(smelt_recipe(0), None);
    }

    #[test]
    fn fuel_coal_burns_1600_ticks() {
        assert_eq!(fuel_burn_ticks(COAL_ID), 1600);
    }

    #[test]
    fn fuel_planks_burns_300_ticks() {
        assert_eq!(fuel_burn_ticks(PLANKS_ID), 300);
    }

    #[test]
    fn fuel_wood_burns_300_ticks() {
        assert_eq!(fuel_burn_ticks(WOOD_ID), 300);
    }

    #[test]
    fn fuel_unknown_id_is_zero() {
        assert_eq!(fuel_burn_ticks(0), 0);
        assert_eq!(fuel_burn_ticks(1), 0); // stone is not fuel
        assert_eq!(fuel_burn_ticks(9999), 0);
    }

    #[test]
    fn is_smeltable_matches_recipe_some() {
        assert!(is_smeltable(8));
        assert!(is_smeltable(IRON_ORE_ID));
        assert!(!is_smeltable(0));
        assert!(!is_smeltable(1));
    }

    #[test]
    fn is_fuel_matches_burn_ticks_nonzero() {
        assert!(is_fuel(COAL_ID));
        assert!(is_fuel(PLANKS_ID));
        assert!(is_fuel(WOOD_ID));
        assert!(!is_fuel(0));
        assert!(!is_fuel(1));
    }

    #[test]
    fn iron_ingot_id_is_43() {
        // Pinned: the iron-ingot id is published in `block::mod`
        // as IRON_INGOT_ID and used as a save id. Bumping it is a
        // breaking change for existing saves. furnace = 42,
        // iron_ingot = 43 (the first item-only id past the block
        // table).
        assert_eq!(IRON_INGOT_ID, 43);
    }
}
