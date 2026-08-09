//! W50.2 / W476 - smelting (furnace) recipe + fuel table API.
//!
//! Furnaces in MC consume fuel + an input item over `time_ticks`
//! and produce `output`. Standard MC tick budget is 200 ticks
//! per item (10 seconds at 20 TPS). The engine ticks at the same
//! rate, so we keep that number.
//!
//! W476: the hand-maintained row list (11 seed recipes incl. the
//! wheat -> cooked_beef and dirt -> bricks stand-ins) is GONE. Rows
//! now live in the generated `smelting_table` sibling, produced from
//! the MC 26.1.2 recipe jsons + FuelValues.java - this module keeps
//! only the row type + the lookup fns so call sites stay unchanged.

#![allow(dead_code)]

pub(crate) use super::smelting_table::{FUEL_ROWS, SMELTING_RECIPES};

/// One smelting recipe.
pub(crate) struct SmeltingRecipe {
    pub(crate) input: u16,
    pub(crate) output: (u16, u8),
    /// Player-facing label for the recipe book / tooltip.
    pub(crate) name: &'static str,
    /// Ticks of furnace burn time required to finish the smelt.
    pub(crate) time_ticks: u32,
    /// XP awarded when output is removed from the furnace; the
    /// demo doesn't track XP yet, but we keep the field so the
    /// table stays MC-shape from day one.
    pub(crate) xp: f32,
}

/// Fuel value (in ticks of burn time) for an item used in a
/// furnace fuel slot. Returns 0 for non-fuel items. Rows come from
/// the generated `FUEL_ROWS` (FuelValues.java semantics, incl. the
/// NON_FLAMMABLE_WOOD removals); the legacy ore-coal id 13 keeps
/// its burn via the hand fallback in `crate::smelting`.
pub(crate) fn fuel_burn_ticks(item: u16) -> u32 {
    FUEL_ROWS
        .iter()
        .find(|(id, _)| *id == item)
        .map(|(_, ticks)| *ticks)
        .unwrap_or(0)
}

/// Look up a smelting recipe by input item id.
pub(crate) fn smelting_for(input: u16) -> Option<&'static SmeltingRecipe> {
    SMELTING_RECIPES.iter().find(|r| r.input == input)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// W476: MC xp values ride along on the generated rows (stored
    /// for the eventual XP-orb pass; pinned here so a regen can't
    /// silently drift them).
    #[test]
    fn xp_pins_match_mc_recipe_jsons() {
        let xp = |input: u16| smelting_for(input).expect("recipe").xp;
        assert_eq!(xp(14), 0.7, "iron_ore -> iron_ingot");
        assert_eq!(xp(329), 0.7, "raw_iron -> iron_ingot");
        assert_eq!(xp(117), 1.0, "gold_ore -> gold_ingot");
        assert_eq!(xp(118), 1.0, "diamond_ore -> diamond");
        assert_eq!(xp(342), 0.35, "beef -> cooked_beef");
        assert_eq!(xp(5), 0.15, "oak_log -> charcoal");
        assert_eq!(xp(27), 1.0, "cactus -> green_dye");
    }

    /// W476: every engine log id smelts to the new charcoal item
    /// (MC `#logs_that_burn` tag expansion), not to the legacy
    /// ore-coal id 13.
    #[test]
    fn every_log_smelts_to_charcoal() {
        for log in [5u16, 88, 127, 130, 133, 136, 233, 237] {
            let r = smelting_for(log).expect("log has a smelt row");
            assert_eq!(r.output, (993, 1), "log {log} -> charcoal");
        }
    }

    /// W476: the pre-W476 stand-in rows are gone - wheat (either id)
    /// is food/crop, dirt is dirt.
    #[test]
    fn stand_in_rows_are_deleted() {
        assert!(smelting_for(493).is_none(), "no wheat -> cooked_beef");
        assert!(smelting_for(337).is_none(), "no wheat (item row) smelt");
        assert!(smelting_for(2).is_none(), "no dirt -> bricks");
    }

    /// W476: fresh MC parity rows land with both sides resolved in
    /// the engine registry.
    #[test]
    fn mc_parity_rows_present() {
        let out = |input: u16| smelting_for(input).expect("recipe").output;
        assert_eq!(out(343), (283, 1), "potato -> baked_potato");
        assert_eq!(out(13), (325, 1), "coal_ore (legacy id) -> coal item");
        assert_eq!(out(756), (292, 1), "kelp -> dried_kelp");
        assert_eq!(out(446), (445, 1), "wet_sponge -> sponge");
        assert_eq!(out(765), (419, 1), "clay -> terracotta");
        assert_eq!(out(994), (675, 1), "clay_ball -> brick");
        assert_eq!(out(361), (96, 1), "cobbled_deepslate -> deepslate");
        // netherrack stays UNsmeltable: MC's output (the nether_brick
        // ITEM) has no engine row yet - see the skip comment in the
        // generated table.
        assert!(smelting_for(44).is_none(), "netherrack skip-logged");
    }

    /// W476: every generated row keeps the MC 200-tick cook time
    /// (all 73 smelting jsons say cookingtime 200) and outputs one
    /// item.
    #[test]
    fn all_rows_cook_in_200_ticks() {
        for r in SMELTING_RECIPES {
            assert_eq!(r.time_ticks, 200, "row input {}", r.input);
            assert_eq!(r.output.1, 1, "row input {}", r.input);
        }
    }

    /// W476: input ids are unique + sorted (the generator's
    /// dedupe/order contract - keeps the linear probe deterministic).
    #[test]
    fn inputs_unique_and_sorted() {
        for w in SMELTING_RECIPES.windows(2) {
            assert!(w[0].input < w[1].input);
        }
        for w in FUEL_ROWS.windows(2) {
            assert!(w[0].0 < w[1].0);
        }
    }

    /// W476: FuelValues.java pins - the famous values plus the
    /// integer-division oddballs (carpet 67 = 1 + 200/3, dried kelp
    /// block 4001 = 1 + 200*20).
    #[test]
    fn fuel_pins_match_fuel_values_java() {
        assert_eq!(fuel_burn_ticks(40), 20000, "lava_bucket");
        assert_eq!(fuel_burn_ticks(296), 2400, "blaze_rod");
        assert_eq!(fuel_burn_ticks(436), 4001, "dried_kelp_block");
        assert_eq!(fuel_burn_ticks(68), 150, "oak_slab");
        assert_eq!(fuel_burn_ticks(11), 100, "white wool");
        assert_eq!(fuel_burn_ticks(108), 100, "black wool");
        assert_eq!(fuel_burn_ticks(217), 67, "white carpet");
        assert_eq!(fuel_burn_ticks(993), 1600, "charcoal");
        assert_eq!(fuel_burn_ticks(325), 1600, "coal item");
        assert_eq!(fuel_burn_ticks(16), 200, "wooden pickaxe");
        assert_eq!(fuel_burn_ticks(500), 50, "bamboo");
        assert_eq!(fuel_burn_ticks(594), 50, "scaffolding");
        assert_eq!(fuel_burn_ticks(718), 300, "bookshelf");
        assert_eq!(fuel_burn_ticks(553), 300, "crafting_table");
        assert_eq!(fuel_burn_ticks(41), 300, "chest");
    }

    /// W476: the `.remove(NON_FLAMMABLE_WOOD)` pass holds - the
    /// crimson / warped family never burns even though the tag
    /// expansions put it in earlier.
    #[test]
    fn non_flammable_wood_is_not_fuel() {
        for id in [240u16, 242, 241, 243, 245, 244, 750, 751, 575] {
            assert_eq!(fuel_burn_ticks(id), 0, "id {id} must not burn");
        }
    }

    /// W476: the new charcoal item is a proper registry row (pure
    /// item, stacks to 64) whose name IS the MC png stem - that
    /// equality is what makes the W53.7 match-name icon path
    /// resolve `charcoal.png` with no ITEM_NAME_MAP row.
    #[test]
    fn charcoal_item_row_is_complete() {
        let it = crate::item::item_type(993).expect("charcoal row");
        assert_eq!(it.name, "charcoal");
        assert_eq!(it.places, None);
        assert_eq!(it.max_stack, 64);
        let cb = crate::item::item_type(994).expect("clay_ball row");
        assert_eq!(cb.name, "clay_ball");
        assert_eq!(cb.places, None);
    }
}
