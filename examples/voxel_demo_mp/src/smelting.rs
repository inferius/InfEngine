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
//! W476: the rows behind these fns are GENERATED from the MC 26.1.2
//! recipe jsons + FuelValues.java (see `recipes/smelting_table.rs`).
//! This wrapper adds exactly one engine-legacy exception on top: the
//! ore-coal id 13 (mining still drops it until the W477 loot pass)
//! keeps its 1600-tick burn even though MC's coal_ore is not a fuel.
//!
//! Cross-module mapping:
//! - input + output ids are the same `u16` used by `item::ITEM_TYPES`
//!   and `block::BLOCK_TYPES`. The furnace runtime trusts the table -
//!   it does not verify that an item with id `output` actually
//!   exists; callers test that via the matching `item_type` lookup.

use crate::block::{BUCKET_EMPTY_ID, BUCKET_LAVA_ID, CHEST_ID, COAL_ID, FURNACE_ID};

/// Resolve a smelt recipe.
///
/// Returns `Some((output_id, output_count))` if `input_id` has a
/// recipe registered, `None` otherwise. The runtime uses `None` as a
/// "this slot is not smeltable" gate: it stops the cook loop instead
/// of consuming fuel on a junk input.
///
/// Output count is currently always 1; the type leaves room for
/// multi-output recipes without rewriting the callers.
pub(crate) fn smelt_recipe(input_id: u16) -> Option<(u16, u8)> {
    // W50.4/W476: delegate to the generated recipes/smelting table
    // so the furnace tick + the recipe book share one source of
    // truth.
    crate::recipes::smelting::smelting_for(input_id)
        .map(|r| r.output)
}

/// Resolve a fuel's burn duration.
///
/// Returns the number of 50-ms game ticks the item burns for; `0`
/// means "not a fuel". Values come from the generated FuelValues
/// port (coal/charcoal 1600, logs + planks 300, stick 100, wooden
/// slabs 150, lava bucket 20000, ...).
///
/// The runtime multiplies these by `TICK_MS` to get the burn
/// duration in milliseconds. The catch-up dispatcher uses the same
/// constant on the offline path so the two units stay in sync.
pub(crate) fn fuel_burn_ticks(item_id: u16) -> u32 {
    let generated = crate::recipes::smelting::fuel_burn_ticks(item_id);
    if generated > 0 {
        return generated;
    }
    // W476 engine-legacy exception: id 13 is the ore-coal row that
    // mining currently drops and torch crafting consumes - it keeps
    // burning like the coal item (1600) until W477 reroutes ore
    // drops onto the real coal item (325).
    if item_id == COAL_ID { 1600 } else { 0 }
}

/// W476: crafting remainder left in the FUEL slot when the last item
/// of a fuel stack burns up. MC's `AbstractFurnaceBlockEntity`
/// replaces an emptied fuel stack with `item.getCraftingRemainder()`;
/// for vanilla fuels that is exactly the lava bucket -> empty
/// bucket pair.
pub(crate) fn fuel_remainder(item_id: u16) -> Option<u16> {
    (item_id == BUCKET_LAVA_ID).then_some(BUCKET_EMPTY_ID)
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
        // W476: stone SMELTS now (MC stone -> smooth_stone), so the
        // negative probe moved to dirt - MC has no dirt smelt (the
        // old dirt -> bricks stand-in is deleted).
        assert!(smelt_recipe(2).is_none());
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
    fn fuel_lava_bucket_burns_20000_ticks() {
        // W476: FuelValues.java baseUnit * 100.
        assert_eq!(fuel_burn_ticks(crate::block::BUCKET_LAVA_ID), 20000);
    }

    #[test]
    fn fuel_remainder_is_empty_bucket_for_lava_only() {
        // W476: MC crafting remainder - burning the lava bucket
        // leaves the empty bucket in the fuel slot.
        assert_eq!(
            fuel_remainder(crate::block::BUCKET_LAVA_ID),
            Some(crate::block::BUCKET_EMPTY_ID)
        );
        assert_eq!(fuel_remainder(COAL_ID), None);
        assert_eq!(fuel_remainder(0), None);
        assert_eq!(fuel_remainder(993), None); // charcoal
    }

    #[test]
    fn is_smeltable_matches_recipe_some() {
        assert!(is_smeltable(8));
        assert!(is_smeltable(IRON_ORE_ID));
        assert!(!is_smeltable(0));
        assert!(!is_smeltable(2)); // dirt (W476: stone smelts now)
    }

    #[test]
    fn is_fuel_matches_burn_ticks_nonzero() {
        assert!(is_fuel(COAL_ID));
        assert!(is_fuel(PLANKS_ID));
        assert!(is_fuel(WOOD_ID));
        assert!(is_fuel(993)); // W476: charcoal
        assert!(!is_fuel(0));
        assert!(!is_fuel(1));
    }

    #[test]
    fn smelt_sand_to_glass_via_recipes_table() {
        // W50.4: sand (id 4) -> glass (id 9). Comes from the
        // recipes/smelting table, not legacy match arms.
        assert_eq!(smelt_recipe(4), Some((9, 1)));
    }

    #[test]
    fn smelt_raw_iron_item_to_iron_ingot() {
        // W50.4: raw_iron (item id 329) -> iron_ingot (43).
        assert_eq!(smelt_recipe(329), Some((43, 1)));
    }

    #[test]
    fn smelt_oak_log_to_charcoal() {
        // W476: oak log (id 5) -> the real charcoal item (993), not
        // the legacy ore-coal id 13.
        assert_eq!(smelt_recipe(5), Some((993, 1)));
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
